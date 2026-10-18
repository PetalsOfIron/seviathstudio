// SPDX-License-Identifier: GPL-3.0-or-later
// The other changers, from Live2D and Inochi2D and from the plan: Path (several curves on one part, each dragged at
// its own spots), Pin (puppet-warp pins), Move, Scale, Flip, Fade (opacity), Order (draw order) and Wave. The simple
// ones are a few numbers (`Form::Vals`); Pins and Paths have their own shapes.

use serde::{Deserialize, Serialize};

use super::{ArtMesh, Id, Kind, Pose, Rig, bend_along};

/// One curve of a Path changer: where it lies on the picture and how far around it the picture follows it.
#[derive(Clone, PartialEq, Debug, Serialize, Deserialize)]
pub struct PathSpot {
    pub pts: Vec<[f32; 2]>,
    pub width: f32,
}

/// The shifts of all the curves of a Path changer in one list (curve after curve), as the editor keeps them.
pub fn flatten(paths: &[PathSpot], off: Option<&Vec<Vec<[f32; 2]>>>) -> Vec<[f32; 2]> {
    let mut out = Vec::new();
    for (k, sp) in paths.iter().enumerate() {
        for i in 0..sp.pts.len() {
            out.push(off.and_then(|o| o.get(k)).and_then(|o| o.get(i)).copied().unwrap_or([0.0, 0.0]));
        }
    }
    out
}

/// The other way: one list back into a list for each curve.
pub fn unflatten(paths: &[PathSpot], flat: &[[f32; 2]]) -> Vec<Vec<[f32; 2]>> {
    let mut at = 0;
    paths
        .iter()
        .map(|sp| {
            let v: Vec<[f32; 2]> = (0..sp.pts.len()).map(|i| flat.get(at + i).copied().unwrap_or([0.0, 0.0])).collect();
            at += sp.pts.len();
            v
        })
        .collect()
}

pub const MAX_PATHS: usize = 32;
pub const MAX_PINS: usize = 64;

/// The numbers of a numeric changer when nothing is changed.
pub fn rest_vals(kind: &Kind) -> Option<Vec<f32>> {
    Some(match kind {
        Kind::Move => vec![0.0, 0.0],
        Kind::Scale { .. } => vec![1.0, 1.0],
        Kind::Flip { .. } => vec![0.0],
        Kind::Fade => vec![1.0],
        Kind::Order => vec![0.0],
        // height of the wave, length of one wave, how far it has travelled (in waves), the way it travels (degrees),
        // and how much it is held still at the start (0 = the whole part waves, 1 = it grows from the start)
        Kind::Wave { .. } => vec![0.0, 120.0, 0.0, 0.0, 0.0],
        _ => return None,
    })
}

/// What these numbers are called, in the order they are stored.
pub fn val_names(kind: &Kind) -> &'static [&'static str] {
    match kind {
        Kind::Move => &["Right", "Down"],
        Kind::Scale { .. } => &["Wider", "Taller"],
        Kind::Flip { .. } => &["Flipped"],
        Kind::Fade => &["Opacity"],
        Kind::Order => &["Forward"],
        Kind::Wave { .. } => &["Height", "Length", "Travelled", "Direction", "Held at start"],
        _ => &[],
    }
}

/// (lowest, highest, step) of each number, for the sliders in the panel.
pub fn val_ranges(kind: &Kind) -> &'static [(f32, f32, f32)] {
    match kind {
        Kind::Move => &[(-2000.0, 2000.0, 1.0), (-2000.0, 2000.0, 1.0)],
        Kind::Scale { .. } => &[(0.0, 4.0, 0.01), (0.0, 4.0, 0.01)],
        Kind::Flip { .. } => &[(0.0, 1.0, 0.01)],
        Kind::Fade => &[(0.0, 1.0, 0.01)],
        Kind::Order => &[(-50.0, 50.0, 0.1)],
        Kind::Wave { .. } => &[(-300.0, 300.0, 0.5), (8.0, 2000.0, 1.0), (-20.0, 20.0, 0.01), (-180.0, 180.0, 1.0), (0.0, 1.0, 0.01)],
        _ => &[],
    }
}

/// Move a point by a numeric changer.
pub fn apply_vals(kind: &Kind, v: &[f32], p: [f32; 2]) -> [f32; 2] {
    let g = |i: usize, d: f32| v.get(i).copied().filter(|x| x.is_finite()).unwrap_or(d);
    match kind {
        Kind::Move => [p[0] + g(0, 0.0), p[1] + g(1, 0.0)],
        Kind::Scale { center } => [center[0] + (p[0] - center[0]) * g(0, 1.0), center[1] + (p[1] - center[1]) * g(1, 1.0)],
        Kind::Flip { center, vertical } => {
            let k = 1.0 - 2.0 * g(0, 0.0).clamp(0.0, 1.0);
            if *vertical { [p[0], center[1] + (p[1] - center[1]) * k] } else { [center[0] + (p[0] - center[0]) * k, p[1]] }
        }
        Kind::Wave { center } => {
            let (amp, len, phase, deg, held) = (g(0, 0.0), g(1, 120.0).max(1.0), g(2, 0.0), g(3, 0.0), g(4, 0.0).clamp(0.0, 1.0));
            if amp.abs() < 1e-6 {
                return p;
            }
            let (s, c) = deg.to_radians().sin_cos();
            // along: the way the wave travels; the push is to the side of that
            let along = (p[0] - center[0]) * c + (p[1] - center[1]) * s;
            let grow = if held > 0.0 { (1.0 - held) + held * (along / (len * 2.0)).clamp(0.0, 1.0) } else { 1.0 };
            let h = amp * grow * (std::f32::consts::TAU * (along / len - phase)).sin();
            [p[0] - s * h, p[1] + c * h]
        }
        _ => p,
    }
}

/// Pins (puppet warp): the picture follows the pins as if it were stiff cloth. A "similarity" moving-least-squares
/// fit: near a pin the picture goes with the pin, far away it turns and slides as the pins say, and it never shears
/// or folds. Pins that you do not move hold the picture still around them.
pub fn pin_shift(pts: &[[f32; 2]], off: &[[f32; 2]], p: [f32; 2]) -> [f32; 2] {
    let n = pts.len().min(off.len());
    if n == 0 || off[..n].iter().all(|o| o[0].abs() < 1e-4 && o[1].abs() < 1e-4) {
        return p;
    }
    // weights: the nearer pin counts more
    let mut w = Vec::with_capacity(n);
    for i in 0..n {
        let d2 = (p[0] - pts[i][0]).powi(2) + (p[1] - pts[i][1]).powi(2);
        if d2 < 1e-6 {
            return [pts[i][0] + off[i][0], pts[i][1] + off[i][1]];
        }
        w.push(1.0 / d2);
    }
    let wsum: f32 = w.iter().sum();
    let (mut ps, mut qs) = ([0.0f32; 2], [0.0f32; 2]);
    for i in 0..n {
        ps[0] += w[i] * pts[i][0];
        ps[1] += w[i] * pts[i][1];
        qs[0] += w[i] * (pts[i][0] + off[i][0]);
        qs[1] += w[i] * (pts[i][1] + off[i][1]);
    }
    let (ps, qs) = ([ps[0] / wsum, ps[1] / wsum], [qs[0] / wsum, qs[1] / wsum]);
    // complex numbers: a turn and a scale is one multiplication
    let (mut mu, mut num) = (0.0f32, [0.0f32; 2]);
    for i in 0..n {
        let ph = [pts[i][0] - ps[0], pts[i][1] - ps[1]];
        let qh = [pts[i][0] + off[i][0] - qs[0], pts[i][1] + off[i][1] - qs[1]];
        mu += w[i] * (ph[0] * ph[0] + ph[1] * ph[1]);
        // qh * conj(ph)
        num[0] += w[i] * (qh[0] * ph[0] + qh[1] * ph[1]);
        num[1] += w[i] * (qh[1] * ph[0] - qh[0] * ph[1]);
    }
    if mu < 1e-6 {
        // all pins in one place: just slide
        return [p[0] + qs[0] - ps[0], p[1] + qs[1] - ps[1]];
    }
    let z = [p[0] - ps[0], p[1] - ps[1]];
    let m = [num[0] / mu, num[1] / mu];
    [qs[0] + z[0] * m[0] - z[1] * m[1], qs[1] + z[0] * m[1] + z[1] * m[0]]
}

/// Several curves on one part: each bends the picture around it (like a Line changer); where curves overlap their
/// pushes are averaged, so the picture never jumps from one to the other.
pub fn blend_paths(paths: &[PathSpot], off: &[Vec<[f32; 2]>], p: [f32; 2]) -> [f32; 2] {
    let (mut sum, mut total) = ([0.0f32; 2], 0.0f32);
    for (k, sp) in paths.iter().enumerate() {
        let Some(o) = off.get(k) else { continue };
        if o.iter().all(|v| v[0].abs() < 1e-5 && v[1].abs() < 1e-5) {
            continue;
        }
        let (new, w) = bend_along(&sp.pts, o, sp.width, p);
        if w <= 0.0 {
            continue;
        }
        sum[0] += w * (new[0] - p[0]);
        sum[1] += w * (new[1] - p[1]);
        total += w;
    }
    // an average where they overlap, the full push where only one reaches
    let k = if total > 1.0 { 1.0 / total } else { 1.0 };
    [p[0] + sum[0] * k, p[1] + sum[1] * k]
}

impl Rig {
    /// How see-through a mesh is, from its Fade changers (1 = as drawn).
    pub fn mesh_fade(&self, mesh: &ArtMesh, pose: &Pose) -> f32 {
        let mut f = 1.0f32;
        let (mut cur, mut depth) = (mesh.parent, 0);
        while let Some(id) = cur {
            let Some(c) = self.changer(id) else { break };
            if matches!(c.kind, Kind::Fade) {
                f *= pose.vals.get(&id).and_then(|v| v.first()).copied().unwrap_or(1.0).clamp(0.0, 1.0);
            }
            cur = c.parent;
            depth += 1;
            if depth > 16 {
                break;
            }
        }
        f
    }

    /// How far a mesh is moved forward in the stack of layers, from its Order changers (0 = where it is).
    pub fn mesh_order(&self, mesh: &ArtMesh, pose: &Pose) -> f32 {
        let mut s = 0.0f32;
        let (mut cur, mut depth) = (mesh.parent, 0);
        while let Some(id) = cur {
            let Some(c) = self.changer(id) else { break };
            if matches!(c.kind, Kind::Order) {
                s += pose.vals.get(&id).and_then(|v| v.first()).copied().unwrap_or(0.0);
            }
            cur = c.parent;
            depth += 1;
            if depth > 16 {
                break;
            }
        }
        s
    }

    /// Does anything in the rig change opacity or the stacking? (When not, drawing skips the extra work.)
    pub fn has_fade_or_order(&self) -> bool {
        self.changers.iter().any(|c| matches!(c.kind, Kind::Fade | Kind::Order))
    }

    /// A Path changer holding one curve; more can be added with `add_path`.
    pub fn new_path_changer(&mut self, name: &str, first: PathSpot, parent: Option<Id>) -> Id {
        self.add_changer(name, Kind::Paths { paths: vec![first] }, parent)
    }

    /// Put another curve on a Path changer. Returns its number. The shapes that are saved get a quiet curve for it.
    pub fn add_path(&mut self, changer: Id, spot: PathSpot) -> Option<usize> {
        let n = {
            let c = self.changers.iter_mut().find(|c| c.id == changer)?;
            let Kind::Paths { paths } = &mut c.kind else { return None };
            if paths.len() >= MAX_PATHS {
                return None;
            }
            paths.push(spot.clone());
            paths.len() - 1
        };
        for b in self.bindings.iter_mut().filter(|b| b.changer == changer) {
            for f in &mut b.forms {
                if let super::Form::Paths { offsets } = f {
                    offsets.push(vec![[0.0, 0.0]; spot.pts.len()]);
                }
            }
        }
        Some(n)
    }

    /// Take a curve away from a Path changer (and from every saved shape). The last one stays.
    pub fn remove_path(&mut self, changer: Id, n: usize) -> bool {
        {
            let Some(c) = self.changers.iter_mut().find(|c| c.id == changer) else { return false };
            let Kind::Paths { paths } = &mut c.kind else { return false };
            if paths.len() <= 1 || n >= paths.len() {
                return false;
            }
            paths.remove(n);
        }
        for b in self.bindings.iter_mut().filter(|b| b.changer == changer) {
            for f in &mut b.forms {
                if let super::Form::Paths { offsets } = f {
                    if n < offsets.len() {
                        offsets.remove(n);
                    }
                }
            }
        }
        for p in &mut self.physics {
            if p.changer == changer && p.path > n {
                p.path -= 1;
            }
        }
        self.physics.retain(|p| !(p.changer == changer && p.path == n));
        true
    }

    /// Add a pin to a Pin changer at a place on the picture. Returns its number.
    pub fn add_pin(&mut self, changer: Id, at: [f32; 2]) -> Option<usize> {
        let n = {
            let c = self.changers.iter_mut().find(|c| c.id == changer)?;
            let Kind::Pins { pts } = &mut c.kind else { return None };
            if pts.len() >= MAX_PINS {
                return None;
            }
            pts.push(at);
            pts.len() - 1
        };
        for b in self.bindings.iter_mut().filter(|b| b.changer == changer) {
            for f in &mut b.forms {
                if let super::Form::Pins { offsets } = f {
                    offsets.push([0.0, 0.0]);
                }
            }
        }
        Some(n)
    }

    pub fn remove_pin(&mut self, changer: Id, n: usize) -> bool {
        {
            let Some(c) = self.changers.iter_mut().find(|c| c.id == changer) else { return false };
            let Kind::Pins { pts } = &mut c.kind else { return false };
            if n >= pts.len() {
                return false;
            }
            pts.remove(n);
        }
        for b in self.bindings.iter_mut().filter(|b| b.changer == changer) {
            for f in &mut b.forms {
                if let super::Form::Pins { offsets } = f {
                    if n < offsets.len() {
                        offsets.remove(n);
                    }
                }
            }
        }
        true
    }
}

/// A path along a stroke the user drew on the picture: at most `n` points spread along it.
pub fn path_from_stroke(stroke: &[[f32; 2]], n: usize, width: f32) -> Option<PathSpot> {
    let n = n.clamp(2, super::MAX_LINE_POINTS);
    if stroke.len() < 2 {
        return None;
    }
    let mut len = vec![0.0f32];
    for w in stroke.windows(2) {
        len.push(len.last().unwrap() + (w[1][0] - w[0][0]).hypot(w[1][1] - w[0][1]));
    }
    let total = *len.last().unwrap();
    if total < 4.0 {
        return None;
    }
    let mut pts = Vec::with_capacity(n);
    for k in 0..n {
        let want = total * k as f32 / (n - 1) as f32;
        let i = len.iter().rposition(|l| *l <= want).unwrap_or(0).min(stroke.len() - 2);
        let span = (len[i + 1] - len[i]).max(1e-6);
        let t = ((want - len[i]) / span).clamp(0.0, 1.0);
        pts.push([stroke[i][0] + (stroke[i + 1][0] - stroke[i][0]) * t, stroke[i][1] + (stroke[i + 1][1] - stroke[i][1]) * t]);
    }
    Some(PathSpot { pts, width: width.clamp(2.0, 5000.0) })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rig::{Form, Values};

    fn near(a: [f32; 2], b: [f32; 2], e: f32) -> bool {
        (a[0] - b[0]).abs() <= e && (a[1] - b[1]).abs() <= e
    }

    #[test]
    fn the_simple_changers_move_a_point_as_they_say() {
        let p = [150.0, 100.0];
        assert_eq!(apply_vals(&Kind::Move, &[10.0, -5.0], p), [160.0, 95.0]);
        assert!(near(apply_vals(&Kind::Scale { center: [100.0, 100.0] }, &[2.0, 0.5], p), [200.0, 100.0], 1e-4));
        let flip = Kind::Flip { center: [100.0, 100.0], vertical: false };
        assert_eq!(apply_vals(&flip, &[1.0], p), [50.0, 100.0], "fully flipped: the other side");
        assert_eq!(apply_vals(&flip, &[0.5], p)[0], 100.0, "half way it is flat");
        assert_eq!(apply_vals(&flip, &[0.0], p), p);
        let vflip = Kind::Flip { center: [100.0, 100.0], vertical: true };
        assert_eq!(apply_vals(&vflip, &[1.0], [150.0, 130.0]), [150.0, 70.0]);
        // a wave: no height = no change; with height the point goes to the side of the way it travels
        let wave = Kind::Wave { center: [0.0, 0.0] };
        assert_eq!(apply_vals(&wave, &[0.0, 100.0, 0.0, 0.0, 0.0], p), p);
        let q = apply_vals(&wave, &[20.0, 100.0, 0.0, 0.0, 0.0], [25.0, 0.0]);
        assert!(near(q, [25.0, 20.0], 1e-3), "a quarter of a wave along: the top: {q:?}");
        // moving the phase by a whole wave gives the same shape back
        let q2 = apply_vals(&wave, &[20.0, 100.0, 1.0, 0.0, 0.0], [25.0, 0.0]);
        assert!(near(q, q2, 1e-3));
        // held at the start: the beginning stays and it grows along
        let held_start = apply_vals(&wave, &[20.0, 100.0, 0.0, 0.0, 1.0], [0.0, 0.0]);
        assert!(near(held_start, [0.0, 0.0], 1e-3));
        let held_far = apply_vals(&wave, &[20.0, 100.0, 0.0, 0.0, 1.0], [225.0, 0.0]);
        assert!((held_far[1] - 20.0).abs() < 1e-3, "full height far away: {held_far:?}");
    }

    #[test]
    fn pins_follow_without_folding_and_unmoved_pins_hold() {
        let pts = [[0.0, 0.0], [200.0, 0.0], [0.0, 200.0], [200.0, 200.0]];
        // nothing moved: nothing changes
        assert_eq!(pin_shift(&pts, &[[0.0; 2]; 4], [90.0, 40.0]), [90.0, 40.0]);
        // one pin dragged: it goes exactly there, the others stay
        let mut off = [[0.0f32; 2]; 4];
        off[3] = [60.0, 40.0];
        assert!(near(pin_shift(&pts, &off, [200.0, 200.0]), [260.0, 240.0], 1e-3));
        assert!(near(pin_shift(&pts, &off, [0.0, 0.0]), [0.0, 0.0], 1e-3));
        // the middle moves part of the way, in the same direction
        let mid = pin_shift(&pts, &off, [100.0, 100.0]);
        assert!(mid[0] > 100.0 && mid[0] < 160.0 && mid[1] > 100.0 && mid[1] < 140.0, "{mid:?}");
        // all pins turned by 90 degrees about the middle: every point turns with them, nothing is stretched
        let turned: Vec<[f32; 2]> = pts.iter().map(|p| [100.0 - (p[1] - 100.0), 100.0 + (p[0] - 100.0)]).collect();
        let off2: Vec<[f32; 2]> = pts.iter().zip(&turned).map(|(a, b)| [b[0] - a[0], b[1] - a[1]]).collect();
        let got = pin_shift(&pts, &off2, [150.0, 100.0]);
        assert!(near(got, [100.0, 150.0], 0.01), "{got:?}");
        // a single pin slides the picture
        assert!(near(pin_shift(&[[10.0, 10.0]], &[[5.0, 0.0]], [50.0, 50.0]), [55.0, 50.0], 1e-3));
        // the shift is smooth: a tiny step of the point gives a tiny step of the result
        let a = pin_shift(&pts, &off, [120.0, 130.0]);
        let b = pin_shift(&pts, &off, [120.5, 130.0]);
        assert!((a[0] - b[0]).hypot(a[1] - b[1]) < 2.0);
    }

    #[test]
    fn several_paths_each_bend_their_own_spot() {
        let a = PathSpot { pts: vec![[100.0, 100.0], [100.0, 140.0], [100.0, 180.0]], width: 25.0 };
        let b = PathSpot { pts: vec![[400.0, 100.0], [400.0, 140.0], [400.0, 180.0]], width: 25.0 };
        let paths = [a, b];
        let mut off = vec![vec![[0.0; 2]; 3], vec![[0.0; 2]; 3]];
        // only the first path is dragged to the right
        off[0] = vec![[0.0, 0.0], [30.0, 0.0], [30.0, 0.0]];
        let near_a = blend_paths(&paths, &off, [100.0, 160.0]);
        assert!(near_a[0] > 120.0, "{near_a:?}");
        let near_b = blend_paths(&paths, &off, [400.0, 160.0]);
        assert_eq!(near_b, [400.0, 160.0], "the other spot does not move");
        let far = blend_paths(&paths, &off, [250.0, 160.0]);
        assert_eq!(far, [250.0, 160.0], "between them nothing moves");
        // overlapping curves are averaged, never added up
        let two = [PathSpot { pts: vec![[100.0, 100.0], [100.0, 200.0]], width: 40.0 }, PathSpot { pts: vec![[100.0, 100.0], [100.0, 200.0]], width: 40.0 }];
        let off2 = vec![vec![[0.0, 0.0], [40.0, 0.0]], vec![[0.0, 0.0], [40.0, 0.0]]];
        let r = blend_paths(&two, &off2, [100.0, 200.0]);
        assert!((r[0] - 140.0).abs() < 0.5, "{r:?}");
    }

    #[test]
    fn the_rig_keeps_paths_pins_fade_and_order_straight() {
        let mut r = Rig::default();
        let p = r.add_param("Open", 0.0, 1.0, 1.0);
        let spot = |x: f32| PathSpot { pts: vec![[x, 0.0], [x, 50.0], [x, 100.0]], width: 20.0 };
        let c = r.new_path_changer("Lash", spot(0.0), None);
        let mut off = vec![vec![[0.0; 2]; 3]];
        off[0][1] = [0.0, 10.0];
        r.set_keyform(c, p, 0.0, Form::Paths { offsets: off });
        // adding a curve gives every saved shape a quiet one for it
        assert_eq!(r.add_path(c, spot(300.0)), Some(1));
        match &r.bindings[0].forms[0] {
            Form::Paths { offsets } => assert_eq!(offsets.len(), 2),
            f => panic!("{f:?}"),
        }
        assert!(r.remove_path(c, 1));
        assert!(!r.remove_path(c, 0), "the last curve stays");
        // pins
        let pc = r.add_changer("Pins", Kind::Pins { pts: vec![[0.0, 0.0]] }, None);
        assert_eq!(r.add_pin(pc, [100.0, 0.0]), Some(1));
        assert!(r.remove_pin(pc, 0));
        // fade and order follow the parents
        let fade = r.add_changer("Fade", Kind::Fade, None);
        let order = r.add_changer("Order", Kind::Order, Some(fade));
        let id = r.fresh_id();
        let m = ArtMesh { id, layer: 1, verts: vec![[0.0, 0.0]], tris: vec![], parent: Some(order) };
        r.set_keyform(fade, p, 0.0, Form::Vals { v: vec![0.25] });
        r.set_keyform(order, p, 0.0, Form::Vals { v: vec![3.0] });
        let open = r.pose(&Values::default());
        assert_eq!((r.mesh_fade(&m, &open), r.mesh_order(&m, &open)), (1.0, 0.0), "open is the picture as drawn");
        let mut v = Values::default();
        v.set(p, 0.0);
        let closed = r.pose(&v);
        assert_eq!((r.mesh_fade(&m, &closed), r.mesh_order(&m, &closed)), (0.25, 3.0));
        assert!(r.has_fade_or_order());
        // a stroke becomes a path with its points spread out evenly
        let spot = path_from_stroke(&[[0.0, 0.0], [100.0, 0.0], [100.0, 100.0]], 5, 30.0).unwrap();
        assert_eq!(spot.pts.len(), 5);
        assert!(near(spot.pts[2], [100.0, 0.0], 0.01) && near(spot.pts[4], [100.0, 100.0], 0.01));
        assert!(path_from_stroke(&[[0.0, 0.0], [1.0, 0.0]], 4, 10.0).is_none(), "too short to be a path");
    }
}
