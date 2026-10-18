// SPDX-License-Identifier: GPL-3.0-or-later
// Physics: hair, cloth, earrings, chains and more that swing and lag when the character moves. Each thing that
// moves is a chain of points running along a Line changer. The root follows the part it is fixed to; the other points
// are pulled back toward where they belong (stiffer near the root), pushed by gravity and wind, and kept at their
// length. The result is turned into the Line changer's offsets, so it adds to whatever the sliders already do.
// At rest nothing moves: what hangs down in the drawing stays as drawn.

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use super::{Id, Kind as ChangerKind, Pose, Rig, curve_samples};

/// What kind of thing it is: this decides how it moves (the numbers can be changed afterwards).
#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Kind {
    Hair,
    Cloth,
    /// Earrings, pendants: a pendulum.
    Hanging,
    Chain,
    /// Fur collars, feathers: a slight trembling.
    Soft,
    /// Chest, cheeks, belly: bouncy and returns.
    Elastic,
    /// Tails and animal ears: like hair, stiffer at the root.
    Tail,
    /// Armor, crowns: follows the body, no physics.
    Solid,
}

impl Kind {
    pub const ALL: [Kind; 8] = [Kind::Hair, Kind::Cloth, Kind::Hanging, Kind::Chain, Kind::Soft, Kind::Elastic, Kind::Tail, Kind::Solid];

    pub fn label(self) -> &'static str {
        match self {
            Kind::Hair => "Hair",
            Kind::Cloth => "Cloth",
            Kind::Hanging => "Hanging accessory",
            Kind::Chain => "Chain",
            Kind::Soft => "Soft",
            Kind::Elastic => "Elastic",
            Kind::Tail => "Tail or animal ear",
            Kind::Solid => "Solid",
        }
    }

    pub fn about(self) -> &'static str {
        match self {
            Kind::Hair => "Strands, braids, curls: stiff at the root, free at the tip.",
            Kind::Cloth => "Skirts, sleeves, capes, ribbons: flows and ripples.",
            Kind::Hanging => "Earrings, pendants: swings like a pendulum.",
            Kind::Chain => "Chains and pearl strings: every link follows the one before.",
            Kind::Soft => "Fur collars, feathers: a slight trembling.",
            Kind::Elastic => "Chest, cheeks, belly: bouncy, comes back.",
            Kind::Tail => "Tails and animal ears: like hair, but stiffer.",
            Kind::Solid => "Armor, shoulders, crowns: no physics, goes with the body.",
        }
    }
}

/// How heavy the cloth or hair is.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Material {
    /// Chiffon, silk.
    Light,
    Normal,
    /// Brocade, leather.
    Heavy,
}

impl Material {
    pub const ALL: [Material; 3] = [Material::Light, Material::Normal, Material::Heavy];

    pub fn label(self) -> &'static str {
        match self {
            Material::Light => "Light",
            Material::Normal => "Normal",
            Material::Heavy => "Heavy",
        }
    }
}

#[derive(Clone, PartialEq, Debug, Serialize, Deserialize)]
pub struct PhysChain {
    pub name: String,
    pub kind: Kind,
    pub material: Material,
    /// The Line or Path changer this moves.
    pub changer: Id,
    /// Which curve of a Path changer (0 for a Line changer).
    #[serde(default)]
    pub path: usize,
    /// How strongly the points are pulled back to where they belong, 0..1: at the root and at the tip.
    pub stiff_root: f32,
    pub stiff_tip: f32,
    /// Loss of speed, 0..1 a second (higher: stops sooner).
    pub damping: f32,
    /// How much gravity pulls on it (1 = the world's gravity).
    pub gravity: f32,
    /// How much the wind moves it.
    pub wind: f32,
    pub enabled: bool,
}

impl PhysChain {
    /// A chain with the usual numbers for its kind and material.
    pub fn new(name: &str, kind: Kind, material: Material, changer: Id) -> PhysChain {
        // (root, tip, damping, gravity, wind)
        let (r, t, d, g, w): (f32, f32, f32, f32, f32) = match kind {
            Kind::Hair => (0.9, 0.15, 0.55, 0.6, 1.0),
            Kind::Cloth => (0.6, 0.10, 0.5, 0.8, 1.3),
            Kind::Hanging => (0.0, 0.0, 0.35, 1.0, 0.7),
            Kind::Chain => (0.0, 0.0, 0.3, 1.0, 0.5),
            Kind::Soft => (0.85, 0.55, 0.7, 0.2, 1.4),
            Kind::Elastic => (0.8, 0.5, 0.25, 0.3, 0.2),
            Kind::Tail => (0.95, 0.25, 0.5, 0.6, 0.9),
            Kind::Solid => (1.0, 1.0, 1.0, 0.0, 0.0),
        };
        let (mass_k, wind_k, damp_k): (f32, f32, f32) = match material {
            Material::Light => (0.8, 1.5, 0.8),
            Material::Normal => (1.0, 1.0, 1.0),
            Material::Heavy => (1.25, 0.45, 1.2),
        };
        PhysChain {
            name: name.chars().filter(|c| !c.is_control()).take(40).collect(),
            kind,
            material,
            changer,
            path: 0,
            // heavy cloth hangs more and is slower to be moved
            stiff_root: (r * if material == Material::Heavy { 0.9 } else { 1.0 }).clamp(0.0, 1.0),
            stiff_tip: (t * mass_k * 0.9_f32.max(0.5)).clamp(0.0, 1.0),
            damping: (d * damp_k).clamp(0.0, 0.98),
            gravity: g * mass_k,
            wind: w * wind_k,
            enabled: kind != Kind::Solid,
        }
    }
}

/// A round shape that points are pushed out of (the head, the body), in rest position; it goes with `changer`.
#[derive(Clone, PartialEq, Debug, Serialize, Deserialize)]
pub struct Collider {
    pub name: String,
    pub center: [f32; 2],
    pub radius: f32,
    pub changer: Option<Id>,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Wind {
    #[default]
    None,
    Breeze,
    Gust,
    Storm,
}

impl Wind {
    pub const ALL: [Wind; 4] = [Wind::None, Wind::Breeze, Wind::Gust, Wind::Storm];

    pub fn label(self) -> &'static str {
        match self {
            Wind::None => "No wind",
            Wind::Breeze => "Breeze",
            Wind::Gust => "Gusts",
            Wind::Storm => "Storm",
        }
    }

    /// The push at time `t` for a point `i` along a chain, as a share of gravity (negative: from the right).
    fn push(self, t: f32, i: usize) -> f32 {
        let ph = i as f32 * 0.7;
        match self {
            Wind::None => 0.0,
            Wind::Breeze => 0.22 * (0.6 + 0.4 * (std::f32::consts::TAU * 0.35 * t + ph).sin()),
            Wind::Gust => {
                let burst = (std::f32::consts::TAU * 0.12 * t).sin().powi(4);
                0.9 * burst * (0.8 + 0.2 * (std::f32::consts::TAU * 1.3 * t + ph).sin())
            }
            Wind::Storm => 1.3 * (0.7 + 0.3 * (std::f32::consts::TAU * 0.5 * t + ph).sin()) + 0.4 * (std::f32::consts::TAU * 2.1 * t + ph * 1.7).sin(),
        }
    }
}

#[derive(Clone, PartialEq, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct World {
    /// Page pixels a second squared.
    pub gravity: f32,
    pub wind: Wind,
    /// -1 = from the right, 1 = from the left.
    pub wind_dir: f32,
}

impl Default for World {
    fn default() -> Self {
        Self { gravity: 2400.0, wind: Wind::None, wind_dir: 1.0 }
    }
}

/// The length that fits the thing best: more points for longer things.
pub fn segments_for(kind: Kind, length: f32, head: f32) -> usize {
    let n = (length / (0.22 * head.max(1.0))).round() as usize;
    match kind {
        Kind::Chain => n.clamp(5, 10),
        Kind::Hanging => n.clamp(3, 5),
        Kind::Elastic | Kind::Soft => n.clamp(3, 4),
        _ => n.clamp(3, 6),
    }
}

const STEP: f32 = 1.0 / 120.0;
const MAX_STEPS: usize = 16;

struct State {
    pos: Vec<[f32; 2]>,
    prev: Vec<[f32; 2]>,
    /// How far it hangs from where it belongs when nothing moves (cancelled out of the result).
    baseline: Vec<[f32; 2]>,
}

/// The running simulation of every chain (kept by the app, not saved).
#[derive(Default)]
pub struct PhysSim {
    states: HashMap<(Id, usize), State>,
    /// Which rig the states belong to.
    key: u64,
    acc: f32,
    pub time: f32,
}

fn dist(a: [f32; 2], b: [f32; 2]) -> f32 {
    (a[0] - b[0]).hypot(a[1] - b[1])
}

/// A small change in the world position when the local position of a point changes: how the changers around it
/// stretch and turn it, found by trying a step in each direction.
fn jacobian(rig: &Rig, base: &Pose, parent: Option<Id>, at: [f32; 2]) -> [[f32; 2]; 2] {
    let f0 = rig.deform_point(base, parent, at);
    let fx = rig.deform_point(base, parent, [at[0] + 1.0, at[1]]);
    let fy = rig.deform_point(base, parent, [at[0], at[1] + 1.0]);
    [[fx[0] - f0[0], fy[0] - f0[0]], [fx[1] - f0[1], fy[1] - f0[1]]]
}

fn solve(j: [[f32; 2]; 2], v: [f32; 2]) -> [f32; 2] {
    let det = j[0][0] * j[1][1] - j[0][1] * j[1][0];
    if det.abs() < 1e-6 {
        return v;
    }
    [(j[1][1] * v[0] - j[0][1] * v[1]) / det, (-j[1][0] * v[0] + j[0][0] * v[1]) / det]
}

/// The key that says "the same rig": the physics setup and the slider-independent shape of the changers.
fn rig_key(rig: &Rig) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    rig.physics.len().hash(&mut h);
    for c in &rig.physics {
        (c.changer, c.kind as u8, c.enabled).hash(&mut h);
        for v in [c.stiff_root, c.stiff_tip, c.damping, c.gravity, c.wind] {
            v.to_bits().hash(&mut h);
        }
    }
    for c in &rig.physics {
        c.path.hash(&mut h);
    }
    for c in &rig.changers {
        let lists: Vec<&Vec<[f32; 2]>> = match &c.kind {
            ChangerKind::Line { pts, .. } => vec![pts],
            ChangerKind::Paths { paths } => paths.iter().map(|p| &p.pts).collect(),
            _ => continue,
        };
        c.id.hash(&mut h);
        c.parent.hash(&mut h);
        for pts in lists {
            for p in pts {
                p[0].to_bits().hash(&mut h);
                p[1].to_bits().hash(&mut h);
            }
        }
    }
    rig.colliders.len().hash(&mut h);
    rig.world.gravity.to_bits().hash(&mut h);
    h.finish()
}

impl PhysSim {
    pub fn reset(&mut self) {
        self.states.clear();
        self.acc = 0.0;
        self.time = 0.0;
        self.key = 0;
    }

    /// Where the simulation has the points of a chain now (page positions), for drawing and tests.
    pub fn world_positions(&self, changer: Id, path: usize) -> Option<&Vec<[f32; 2]>> {
        self.states.get(&(changer, path)).map(|s| &s.pos)
    }

    /// Advance by `dt` seconds. `base` is the pose the sliders make (without physics); `rest` the pose with every
    /// slider at its default (used to find out how things hang when nothing moves). Returns, for each Line changer
    /// with physics, the offsets to add to its pose.
    pub fn step(&mut self, rig: &Rig, base: &Pose, rest: &Pose, dt: f32) -> HashMap<(Id, usize), Vec<[f32; 2]>> {
        let key = rig_key(rig);
        if key != self.key {
            self.states.clear();
            self.key = key;
        }
        let dt = if dt.is_finite() { dt.clamp(0.0, 0.25) } else { 0.0 };
        self.acc += dt;
        let steps = ((self.acc / STEP) as usize).min(MAX_STEPS);
        self.acc = (self.acc - steps as f32 * STEP).min(STEP);
        let mut out = HashMap::new();
        for ch in rig.physics.iter().filter(|c| c.enabled && c.kind != Kind::Solid) {
            let Some(line) = rig.changer(ch.changer) else { continue };
            // the curve this moves and how the sliders have bent it already
            let (pts, base_off): (&Vec<[f32; 2]>, Vec<[f32; 2]>) = match &line.kind {
                ChangerKind::Line { pts, .. } => (pts, base.line.get(&ch.changer).cloned().unwrap_or_default()),
                ChangerKind::Paths { paths } => match paths.get(ch.path) {
                    Some(sp) => (&sp.pts, base.paths.get(&ch.changer).and_then(|p| p.get(ch.path)).cloned().unwrap_or_default()),
                    None => continue,
                },
                _ => continue,
            };
            let slot = (ch.changer, ch.path);
            if pts.len() < 2 {
                continue;
            }
            // where each point belongs now (the sliders' pose of the line and everything around it)
            let target: Vec<[f32; 2]> = pts.iter().map(|p| rig.deform_point(base, Some(line.id), *p)).collect();
            let rest_target: Vec<[f32; 2]> = pts.iter().map(|p| rig.deform_point(rest, Some(line.id), *p)).collect();
            if !self.states.contains_key(&slot) {
                // start hanging where it belongs, then let it settle for two seconds with nothing moving
                let mut st = State { pos: rest_target.clone(), prev: rest_target.clone(), baseline: vec![[0.0, 0.0]; pts.len()] };
                for _ in 0..240 {
                    advance(rig, ch, &mut st, &rest_target, rest, Wind::None, 1.0, 0.0);
                }
                st.baseline = st.pos.iter().zip(&rest_target).map(|(p, t)| [p[0] - t[0], p[1] - t[1]]).collect();
                st.pos = target.clone();
                st.prev = target.clone();
                for (i, b) in st.baseline.iter().enumerate() {
                    st.pos[i] = [target[i][0] + b[0], target[i][1] + b[1]];
                    st.prev[i] = st.pos[i];
                }
                self.states.insert(slot, st);
            }
            let (wind, dir) = (rig.world.wind, rig.world.wind_dir);
            let t0 = self.time;
            if let Some(st) = self.states.get_mut(&slot) {
                for s in 0..steps {
                    advance(rig, ch, st, &target, base, wind, dir, t0 + s as f32 * STEP);
                }
                // turn the result into offsets of the line (in its own frame, before the changers around it)
                let mut offs = Vec::with_capacity(pts.len());
                for (i, p) in pts.iter().enumerate() {
                    let world = [st.pos[i][0] - target[i][0] - st.baseline[i][0], st.pos[i][1] - target[i][1] - st.baseline[i][1]];
                    let moved = [p[0] + base_off.get(i).map(|o| o[0]).unwrap_or(0.0), p[1] + base_off.get(i).map(|o| o[1]).unwrap_or(0.0)];
                    let local = solve(jacobian(rig, base, line.parent, moved), world);
                    let ok = local[0].is_finite() && local[1].is_finite();
                    offs.push(if ok && i > 0 { local } else { [0.0, 0.0] });
                }
                out.insert(slot, offs);
            }
        }
        self.time += steps as f32 * STEP;
        out
    }
}

/// One fixed step of one chain.
#[allow(clippy::too_many_arguments)]
fn advance(rig: &Rig, ch: &PhysChain, st: &mut State, target: &[[f32; 2]], pose: &Pose, wind: Wind, wind_dir: f32, t: f32) {
    let n = st.pos.len().min(target.len());
    if n < 2 {
        return;
    }
    let g = rig.world.gravity;
    let keep = (1.0 - ch.damping.clamp(0.0, 0.99)).powf(STEP);
    st.pos[0] = target[0];
    st.prev[0] = target[0];
    for i in 1..n {
        let k = ch.stiff_root + (ch.stiff_tip - ch.stiff_root) * (i as f32 / (n - 1) as f32);
        let vel = [(st.pos[i][0] - st.prev[i][0]) * keep, (st.pos[i][1] - st.prev[i][1]) * keep];
        let push = wind.push(t, i) * ch.wind * wind_dir * g;
        let acc = [push, g * ch.gravity];
        let mut next = [st.pos[i][0] + vel[0] + acc[0] * STEP * STEP, st.pos[i][1] + vel[1] + acc[1] * STEP * STEP];
        // pulled back toward where it belongs
        let pull = 1.0 - (1.0 - k.clamp(0.0, 1.0)).powf(STEP * 30.0);
        next[0] += (target[i][0] - next[0]) * pull;
        next[1] += (target[i][1] - next[1]) * pull;
        st.prev[i] = st.pos[i];
        st.pos[i] = next;
    }
    // keep the length of every piece (a few rounds, from the root out), and stay out of the colliders
    for _ in 0..4 {
        for i in 1..n {
            let want = dist(target[i], target[i - 1]).max(1e-3);
            let (a, b) = (st.pos[i - 1], st.pos[i]);
            let d = dist(a, b).max(1e-6);
            let k = want / d;
            st.pos[i] = [a[0] + (b[0] - a[0]) * k, a[1] + (b[1] - a[1]) * k];
        }
        for c in &rig.colliders {
            let centre = rig.deform_point(pose, c.changer, c.center);
            for p in st.pos.iter_mut().skip(1) {
                let d = dist(*p, centre);
                if d < c.radius && d > 1e-6 {
                    let k = c.radius / d;
                    *p = [centre[0] + (p[0] - centre[0]) * k, centre[1] + (p[1] - centre[1]) * k];
                }
            }
        }
    }
    // never run away
    let reach: f32 = (1..n).map(|i| dist(target[i], target[i - 1])).sum::<f32>() * 2.0 + 10.0;
    for i in 0..n {
        let bad = !st.pos[i][0].is_finite() || !st.pos[i][1].is_finite() || dist(st.pos[i], target[i]) > reach;
        if bad {
            st.pos[i] = target[i];
            st.prev[i] = target[i];
        }
    }
}

/// A Line changer along the long side of a part, made for physics: root at one end, `n` points down to the tip.
/// `root_at` is where it is fixed: the end of the part nearest to it.
pub fn make_line(rig: &mut Rig, name: &str, rect: [f32; 4], root_at: [f32; 2], n: usize, parent: Option<Id>) -> Id {
    let (w, h) = (rect[2] - rect[0], rect[3] - rect[1]);
    let vertical = h >= w;
    let (a, b) = if vertical { ([(rect[0] + rect[2]) / 2.0, rect[1]], [(rect[0] + rect[2]) / 2.0, rect[3]]) } else { ([rect[0], (rect[1] + rect[3]) / 2.0], [rect[2], (rect[1] + rect[3]) / 2.0]) };
    let (root, tip) = if dist(a, root_at) <= dist(b, root_at) { (a, b) } else { (b, a) };
    let n = n.max(3);
    let pts: Vec<[f32; 2]> = (0..n).map(|i| [root[0] + (tip[0] - root[0]) * i as f32 / (n - 1) as f32, root[1] + (tip[1] - root[1]) * i as f32 / (n - 1) as f32]).collect();
    let width = (w.min(h) * 0.6).max(20.0);
    rig.add_changer(name, ChangerKind::Line { pts, width }, parent)
}

/// How long a Line changer's curve is.
pub fn curve_length(pts: &[[f32; 2]]) -> f32 {
    curve_samples(pts, 4).windows(2).map(|w| dist(w[0], w[1])).sum()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rig::{Form, Kind as K, Values};

    /// A rig with a head (a rotation) and a strand of hair hanging from it: 5 points straight down from (500, 400).
    fn hair_rig() -> (Rig, Id, Id, Id) {
        let mut r = Rig::default();
        let head = r.add_changer("Head", K::Rotation { pivot: [500.0, 700.0] }, None);
        let tilt = r.add_param("Tilt", -30.0, 30.0, 0.0);
        r.bind_rotation(head, tilt, 30.0);
        let pts: Vec<[f32; 2]> = (0..5).map(|i| [500.0, 400.0 + 80.0 * i as f32]).collect();
        let line = r.add_changer("Strand", K::Line { pts, width: 40.0 }, Some(head));
        r.physics.push(PhysChain::new("Strand", Kind::Hair, Material::Normal, line));
        (r, head, tilt, line)
    }

    fn run(sim: &mut PhysSim, r: &Rig, vals: &Values, seconds: f32) -> HashMap<(Id, usize), Vec<[f32; 2]>> {
        let base = r.pose(vals);
        let rest = r.pose(&Values::default());
        let mut out = HashMap::new();
        for _ in 0..(seconds * 60.0) as usize {
            out = sim.step(r, &base, &rest, 1.0 / 60.0);
        }
        out
    }

    fn tip_len(o: &HashMap<(Id, usize), Vec<[f32; 2]>>, line: Id) -> f32 {
        let t = o[&(line, 0)].last().unwrap();
        t[0].hypot(t[1])
    }

    #[test]
    fn at_rest_nothing_moves_even_with_gravity() {
        let (r, _, _, line) = hair_rig();
        let mut sim = PhysSim::default();
        let o = run(&mut sim, &r, &Values::default(), 2.0);
        assert!(tip_len(&o, line) < 1.0, "the hair stays as drawn: {}", tip_len(&o, line));
        // a pendulum that hangs straight down stays too
        let mut p = r.clone();
        p.physics[0] = PhysChain::new("Earring", Kind::Hanging, Material::Normal, line);
        let o = run(&mut PhysSim::default(), &p, &Values::default(), 2.0);
        assert!(tip_len(&o, line) < 1.0);
    }

    #[test]
    fn a_sudden_turn_makes_the_tip_lag_and_swing_and_then_settle() {
        let (r, _, tilt, line) = hair_rig();
        let mut sim = PhysSim::default();
        run(&mut sim, &r, &Values::default(), 0.5);
        let mut v = Values::default();
        v.set(tilt, 30.0);
        // the head turns at once: right after, the tip is far from where it belongs
        let early = run(&mut sim, &r, &v, 0.15);
        assert!(tip_len(&early, line) > 15.0, "the tip lags behind: {}", tip_len(&early, line));
        // later it has caught up
        let late = run(&mut sim, &r, &v, 5.0);
        assert!(tip_len(&late, line) < tip_len(&early, line) * 0.3, "it settles: {} then {}", tip_len(&early, line), tip_len(&late, line));
        // it swings: back and forth over the first second the tip passes through "caught up"
        let mut sim2 = PhysSim::default();
        run(&mut sim2, &r, &Values::default(), 0.5);
        let base = r.pose(&v);
        let rest = r.pose(&Values::default());
        let xs: Vec<f32> = (0..90).map(|_| sim2.step(&r, &base, &rest, 1.0 / 60.0)[&(line, 0)].last().unwrap()[0]).collect();
        let (hi, lo) = (xs.iter().cloned().fold(f32::MIN, f32::max), xs.iter().cloned().fold(f32::MAX, f32::min));
        assert!(hi - lo > 5.0, "it moves back and forth: {lo}..{hi}");
    }

    #[test]
    fn stiff_things_move_less_and_the_root_never_moves() {
        let (mut stiff, _, tilt, line) = hair_rig();
        stiff.physics[0].stiff_root = 1.0;
        stiff.physics[0].stiff_tip = 0.9;
        let (loose, ..) = hair_rig();
        let mut v = Values::default();
        v.set(tilt, 30.0);
        let (mut a, mut b) = (PhysSim::default(), PhysSim::default());
        run(&mut a, &stiff, &Values::default(), 0.3);
        run(&mut b, &loose, &Values::default(), 0.3);
        let (os, ol) = (run(&mut a, &stiff, &v, 0.2), run(&mut b, &loose, &v, 0.2));
        assert!(tip_len(&os, line) < tip_len(&ol, line) * 0.5, "{} vs {}", tip_len(&os, line), tip_len(&ol, line));
        assert_eq!(ol[&(line, 0)][0], [0.0, 0.0], "the root is fixed to its place");
    }

    #[test]
    fn wind_pushes_and_the_heavy_one_moves_less_than_the_light_one() {
        let (mut light, _, _, line) = hair_rig();
        light.physics[0] = PhysChain::new("Silk", Kind::Cloth, Material::Light, line);
        light.world.wind = Wind::Storm;
        let mut heavy = light.clone();
        heavy.physics[0] = PhysChain::new("Leather", Kind::Cloth, Material::Heavy, line);
        let calm = {
            let mut c = light.clone();
            c.world.wind = Wind::None;
            run(&mut PhysSim::default(), &c, &Values::default(), 3.0)
        };
        assert!(tip_len(&calm, line) < 1.0);
        let mut sim_l = PhysSim::default();
        let mut sim_h = PhysSim::default();
        let (mut max_l, mut max_h) = (0.0f32, 0.0f32);
        let base = light.pose(&Values::default());
        for _ in 0..240 {
            let l = sim_l.step(&light, &base, &base, 1.0 / 60.0);
            let h = sim_h.step(&heavy, &base, &base, 1.0 / 60.0);
            max_l = max_l.max(l[&(line, 0)].last().unwrap()[0]);
            max_h = max_h.max(h[&(line, 0)].last().unwrap()[0]);
        }
        assert!(max_l > 20.0, "wind from the left pushes it to the right: {max_l}");
        assert!(max_h < max_l, "heavy {max_h} vs light {max_l}");
        // wind from the other side pushes the other way
        light.world.wind_dir = -1.0;
        let mut sim = PhysSim::default();
        let mut min_x = 0.0f32;
        for _ in 0..240 {
            min_x = min_x.min(sim.step(&light, &base, &base, 1.0 / 60.0)[&(line, 0)].last().unwrap()[0]);
        }
        assert!(min_x < -20.0, "{min_x}");
    }

    #[test]
    fn a_collider_keeps_the_hair_out_of_the_face() {
        let (mut r, _, _, line) = hair_rig();
        // a round face right where the hair would swing to
        r.colliders.push(Collider { name: "Head".into(), center: [560.0, 560.0], radius: 70.0, changer: None });
        r.world.wind = Wind::Storm;
        let mut sim = PhysSim::default();
        let base = r.pose(&Values::default());
        let mut worst = f32::MAX;
        for _ in 0..300 {
            sim.step(&r, &base, &base, 1.0 / 60.0);
            for p in sim.world_positions(line, 0).unwrap().iter().skip(1) {
                worst = worst.min(dist(*p, [560.0, 560.0]));
            }
        }
        assert!(worst > 60.0, "no point goes into the face: {worst}");
        // without it the hair would go through
        r.colliders.clear();
        let mut free = PhysSim::default();
        let mut through = f32::MAX;
        for _ in 0..300 {
            free.step(&r, &base, &base, 1.0 / 60.0);
            for p in free.world_positions(line, 0).unwrap().iter().skip(1) {
                through = through.min(dist(*p, [560.0, 560.0]));
            }
        }
        assert!(through < worst || through > 60.0);
    }

    #[test]
    fn the_result_is_in_the_lines_own_frame_so_the_head_turn_does_not_double_it() {
        let (r, _, tilt, line) = hair_rig();
        let mut sim = PhysSim::default();
        let mut v = Values::default();
        v.set(tilt, 20.0);
        let base = r.pose(&v);
        let rest = r.pose(&Values::default());
        let mut offs = HashMap::new();
        for _ in 0..30 {
            offs = sim.step(&r, &base, &rest, 1.0 / 60.0);
        }
        // add the offsets to the pose like the app does, and see where the points go
        let mut pose = base.clone();
        let e = pose.line.entry(line).or_insert_with(|| vec![[0.0, 0.0]; 5]);
        for (a, o) in e.iter_mut().zip(&offs[&(line, 0)]) {
            a[0] += o[0];
            a[1] += o[1];
        }
        let world = sim.world_positions(line, 0).unwrap().clone();
        let ChangerKindRef(pts) = points_of(&r, line);
        for (i, p) in pts.iter().enumerate() {
            let got = r.deform_point(&pose, Some(line), *p);
            assert!(dist(got, world[i]) < 4.0, "point {i}: the picture has it at {got:?}, the simulation at {:?}", world[i]);
        }
    }

    struct ChangerKindRef(Vec<[f32; 2]>);

    fn points_of(r: &Rig, line: Id) -> ChangerKindRef {
        match &r.changer(line).unwrap().kind {
            K::Line { pts, .. } => ChangerKindRef(pts.clone()),
            _ => panic!(),
        }
    }

    #[test]
    fn a_curve_of_a_path_changer_flows_like_a_line_and_the_other_curves_stay() {
        use crate::rig::PathSpot;
        let mut r = Rig::default();
        let head = r.add_changer("Head", K::Rotation { pivot: [500.0, 700.0] }, None);
        let tilt = r.add_param("Tilt", -30.0, 30.0, 0.0);
        r.bind_rotation(head, tilt, 30.0);
        let strand = |x: f32| PathSpot { pts: (0..5).map(|i| [x, 400.0 + 80.0 * i as f32]).collect(), width: 40.0 };
        let pc = r.new_path_changer("Cloth", strand(500.0), Some(head));
        assert_eq!(r.add_path(pc, strand(600.0)), Some(1));
        // only the second curve has physics
        let mut chain = PhysChain::new("Hem", Kind::Cloth, Material::Normal, pc);
        chain.path = 1;
        r.physics.push(chain);
        let mut sim = PhysSim::default();
        run(&mut sim, &r, &Values::default(), 0.5);
        let mut v = Values::default();
        v.set(tilt, 30.0);
        let early = run(&mut sim, &r, &v, 0.15);
        assert!(early.contains_key(&(pc, 1)) && !early.contains_key(&(pc, 0)));
        let t = early[&(pc, 1)].last().unwrap();
        assert!(t[0].hypot(t[1]) > 10.0, "the curve lags behind the head: {t:?}");
        assert!(sim.world_positions(pc, 1).is_some() && sim.world_positions(pc, 0).is_none());
        // at rest nothing moves
        let calm = run(&mut PhysSim::default(), &r, &Values::default(), 2.0);
        let t = calm[&(pc, 1)].last().unwrap();
        assert!(t[0].hypot(t[1]) < 1.0);
        // deleting the curve takes its physics with it
        assert!(r.add_path(pc, strand(700.0)).is_some());
        assert!(r.remove_path(pc, 1));
        assert!(r.physics.is_empty());
    }

    #[test]
    fn kinds_have_their_own_feel_and_solid_does_not_move() {
        let line = 7;
        let hair = PhysChain::new("h", Kind::Hair, Material::Normal, line);
        let tail = PhysChain::new("t", Kind::Tail, Material::Normal, line);
        let pend = PhysChain::new("p", Kind::Hanging, Material::Normal, line);
        assert!(hair.stiff_root > hair.stiff_tip, "stiff at the root, free at the tip");
        assert!(tail.stiff_root >= hair.stiff_root && tail.stiff_tip > hair.stiff_tip, "tails are stiffer");
        assert_eq!((pend.stiff_root, pend.stiff_tip), (0.0, 0.0), "a pendulum hangs free");
        let solid = PhysChain::new("s", Kind::Solid, Material::Heavy, line);
        assert!(!solid.enabled);
        let silk = PhysChain::new("x", Kind::Cloth, Material::Light, line);
        let brocade = PhysChain::new("y", Kind::Cloth, Material::Heavy, line);
        assert!(silk.wind > brocade.wind, "light cloth catches more wind");
        assert!(silk.damping < brocade.damping);
        for k in Kind::ALL {
            assert!(!k.label().is_empty() && !k.about().is_empty());
        }
        // the number of points: long things get more
        assert!(segments_for(Kind::Hair, 900.0, 300.0) > segments_for(Kind::Hair, 120.0, 300.0));
        assert_eq!(segments_for(Kind::Chain, 100.0, 300.0), 5);
        assert!(segments_for(Kind::Hanging, 5000.0, 300.0) <= 5);
    }

    #[test]
    fn a_line_for_physics_runs_from_the_attachment_to_the_tip() {
        let mut r = Rig::default();
        // a tall strand with the head above it: the root is the top end
        let id = make_line(&mut r, "Hair", [400.0, 500.0, 480.0, 900.0], [440.0, 300.0], 5, None);
        match &r.changer(id).unwrap().kind {
            K::Line { pts, width } => {
                assert_eq!(pts.len(), 5);
                assert_eq!(pts[0], [440.0, 500.0]);
                assert_eq!(pts[4], [440.0, 900.0]);
                assert!((*width - 48.0).abs() < 0.01);
            }
            _ => panic!(),
        }
        // a wide tail with the body to its left: the root is the left end
        let t = make_line(&mut r, "Tail", [600.0, 700.0, 900.0, 760.0], [450.0, 730.0], 4, None);
        match &r.changer(t).unwrap().kind {
            K::Line { pts, .. } => {
                assert_eq!(pts[0][0], 600.0);
                assert_eq!(pts[3][0], 900.0);
            }
            _ => panic!(),
        }
        assert!((curve_length(&[[0.0, 0.0], [0.0, 50.0], [0.0, 100.0]]) - 100.0).abs() < 1.0);
    }

    #[test]
    fn the_simulation_stays_finite_whatever_it_is_given() {
        let (mut r, _, tilt, line) = hair_rig();
        r.world.wind = Wind::Storm;
        let mut sim = PhysSim::default();
        let mut v = Values::default();
        let mut worst = 0.0f32;
        for i in 0..400 {
            v.set(tilt, if i % 2 == 0 { 30.0 } else { -30.0 });
            let base = r.pose(&v);
            let rest = r.pose(&Values::default());
            let dt = match i % 5 {
                0 => 0.5,
                1 => f32::NAN,
                2 => 0.0,
                3 => 1e9,
                _ => 1.0 / 30.0,
            };
            let o = sim.step(&r, &base, &rest, dt);
            for p in &o[&(line, 0)] {
                assert!(p[0].is_finite() && p[1].is_finite());
                worst = worst.max(p[0].hypot(p[1]));
            }
        }
        assert!(worst < 3000.0, "{worst}");
        // a changer that was deleted is ignored
        let mut gone = r.clone();
        gone.changers.retain(|c| c.id != line);
        assert!(sim.step(&gone, &gone.pose(&v), &gone.pose(&Values::default()), 0.016).is_empty());
        let _ = Form::Rot { deg: 0.0 };
    }

    #[test]
    fn physics_is_saved_with_the_rig_and_bad_data_is_cleaned() {
        let (mut r, head, _, line) = hair_rig();
        r.colliders.push(Collider { name: "Head".into(), center: [500.0, 500.0], radius: 120.0, changer: Some(head) });
        r.world.wind = Wind::Gust;
        let back: Rig = serde_json::from_str(&serde_json::to_string(&r).unwrap()).unwrap();
        assert_eq!(back, r);
        assert_eq!(back.world.wind, Wind::Gust);
        let mut bad = r.clone();
        bad.physics.push(PhysChain { stiff_root: f32::NAN, ..PhysChain::new("x", Kind::Hair, Material::Normal, 9999) });
        bad.physics.push(PhysChain { damping: 5.0, gravity: f32::INFINITY, ..PhysChain::new("y", Kind::Hair, Material::Normal, line) });
        bad.colliders.push(Collider { name: "Bad".into(), center: [f32::NAN, 0.0], radius: -5.0, changer: Some(4242) });
        bad.world.gravity = f32::NAN;
        let ok = bad.sanitized();
        assert_eq!(ok.physics.len(), 2, "the one for a missing changer is gone");
        assert!(ok.physics[1].damping <= 0.98 && ok.physics[1].gravity.is_finite());
        assert_eq!(ok.colliders.len(), 1);
        assert!(ok.world.gravity.is_finite() && ok.world.gravity > 0.0);
        // a line that was deleted takes its physics with it
        let mut gone = r.clone();
        gone.changers.retain(|c| c.id != line);
        assert!(gone.sanitized().physics.is_empty());
    }
}
