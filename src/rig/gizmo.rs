// SPDX-License-Identifier: GPL-3.0-or-later
// The rotation gizmo's brain: which sliders turn the head or the body (found by name from the auto-rig), how a drag on
// a ring becomes a slider value, snapping, and the "spin to the side" positions. It drives the existing sliders, so it
// works with anything that moves them.

use super::{Id, Kind, Rig, Values};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Axis {
    X,
    Y,
    Z,
}

/// Something the gizmo can turn: up to three sliders (left-right, up-down, tilt) and where its rings are drawn.
#[derive(Clone, PartialEq, Debug)]
pub struct Target {
    pub name: String,
    pub x: Option<Id>,
    pub y: Option<Id>,
    pub z: Option<Id>,
    pub pivot: [f32; 2],
}

impl Target {
    pub fn param(&self, a: Axis) -> Option<Id> {
        match a {
            Axis::X => self.x,
            Axis::Y => self.y,
            Axis::Z => self.z,
        }
    }
}

/// The head and the body, if the rig has the sliders for them.
pub fn targets(rig: &Rig) -> Vec<Target> {
    let by_name = |n: &str| rig.params.iter().find(|p| p.name == n).map(|p| p.id);
    let pivot_of = |changer: &str| rig.changers.iter().find(|c| c.name == changer).and_then(|c| match c.kind {
        Kind::Rotation { pivot } => Some(pivot),
        _ => None,
    });
    let mut out = Vec::new();
    let (hx, hy, hz) = (by_name("Angle X"), by_name("Angle Y"), by_name("Angle Z"));
    if hx.is_some() || hy.is_some() || hz.is_some() {
        // the middle of the head: a bit above the pivot of its tilt
        let pivot = pivot_of("Head tilt").map(|p| [p[0], p[1] - 150.0]).unwrap_or_else(|| mesh_centre(rig));
        out.push(Target { name: "Head".into(), x: hx, y: hy, z: hz, pivot });
    }
    if let Some(z) = by_name("Body tilt") {
        let pivot = pivot_of("Body tilt").map(|p| [p[0], p[1] - 250.0]).unwrap_or_else(|| mesh_centre(rig));
        out.push(Target { name: "Body".into(), x: None, y: None, z: Some(z), pivot });
    }
    out
}

fn mesh_centre(rig: &Rig) -> [f32; 2] {
    let (mut x0, mut y0, mut x1, mut y1) = (f32::MAX, f32::MAX, f32::MIN, f32::MIN);
    for v in rig.meshes.iter().flat_map(|m| &m.verts) {
        x0 = x0.min(v[0]);
        y0 = y0.min(v[1]);
        x1 = x1.max(v[0]);
        y1 = y1.max(v[1]);
    }
    if x1 < x0 { [0.0, 0.0] } else { [(x0 + x1) / 2.0, (y0 + y1) / 2.0] }
}

/// Round to the nearest step (0 = no snapping).
pub fn snap(v: f32, step: f32) -> f32 {
    if step > 0.0 { (v / step).round() * step } else { v }
}

/// Put a value on a slider: kept inside the slider's range (the rig's limit) and snapped. Returns what was set.
pub fn set_value(rig: &Rig, vals: &mut Values, id: Id, v: f32, snap_step: f32) -> f32 {
    let Some(p) = rig.param(id) else { return v };
    let x = snap(v, snap_step).clamp(p.min, p.max);
    vals.set(id, x);
    x
}

/// Half the range of a slider (what a full drag of a ring moves it by).
pub fn half_range(rig: &Rig, id: Id) -> f32 {
    rig.param(id).map(|p| (p.max - p.min) / 2.0).unwrap_or(1.0)
}

/// Dragging along the left-right ring: 100 screen pixels move the slider by half its range.
pub fn x_delta(dx_px: f32, half: f32) -> f32 {
    dx_px / 100.0 * half
}

/// Dragging along the up-down ring: up raises it.
pub fn y_delta(dy_px: f32, half: f32) -> f32 {
    -dy_px / 100.0 * half
}

/// Dragging around the tilt ring: half a turn moves the slider by half its range.
pub fn z_delta(from: [f32; 2], to: [f32; 2], centre: [f32; 2], half: f32) -> f32 {
    let a0 = (from[1] - centre[1]).atan2(from[0] - centre[0]);
    let a1 = (to[1] - centre[1]).atan2(to[0] - centre[0]);
    let mut d = a1 - a0;
    while d > std::f32::consts::PI {
        d -= std::f32::consts::TAU;
    }
    while d < -std::f32::consts::PI {
        d += std::f32::consts::TAU;
    }
    d.to_degrees() / 180.0 * half
}

/// The positions of "spin to the side": a share of the slider's end, from the left side (-1) to the right side (1).
pub const SPIN: [(&str, f32); 5] = [("Left side", -1.0), ("Three quarters left", -0.5), ("Front", 0.0), ("Three quarters right", 0.5), ("Right side", 1.0)];

/// Turn a target to one of the spin positions (only the left-right slider).
pub fn spin(rig: &Rig, vals: &mut Values, t: &Target, share: f32) -> Option<f32> {
    let id = t.x?;
    let p = rig.param(id)?;
    let v = if share >= 0.0 { share * (p.max - p.default) + p.default } else { -share * (p.min - p.default) + p.default };
    Some(set_value(rig, vals, id, v, 0.0))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rig::{Kind as K, Rig};

    fn head_rig() -> Rig {
        let mut r = Rig::default();
        for n in ["Angle X", "Angle Y", "Angle Z"] {
            r.add_param(n, -30.0, 30.0, 0.0);
        }
        r.add_changer("Head tilt", K::Rotation { pivot: [500.0, 700.0] }, None);
        r.add_param("Body tilt", -10.0, 10.0, 0.0);
        r.add_changer("Body tilt", K::Rotation { pivot: [500.0, 1500.0] }, None);
        r
    }

    #[test]
    fn the_head_and_the_body_are_found_by_their_sliders() {
        let r = head_rig();
        let t = targets(&r);
        assert_eq!(t.iter().map(|x| x.name.as_str()).collect::<Vec<_>>(), ["Head", "Body"]);
        assert!(t[0].x.is_some() && t[0].y.is_some() && t[0].z.is_some());
        assert!(t[1].x.is_none() && t[1].z.is_some());
        assert_eq!(t[0].param(Axis::Y), t[0].y);
        assert!(t[0].pivot[1] < 700.0, "the rings sit on the head, above its pivot");
        assert!(targets(&Rig::default()).is_empty());
    }

    #[test]
    fn values_are_snapped_and_kept_inside_the_limits_of_the_slider() {
        let r = head_rig();
        let t = &targets(&r)[0];
        let mut v = Values::default();
        let x = t.x.unwrap();
        assert_eq!(set_value(&r, &mut v, x, 12.0, 5.0), 10.0);
        assert_eq!(set_value(&r, &mut v, x, 13.0, 5.0), 15.0);
        assert_eq!(set_value(&r, &mut v, x, 99.0, 0.0), 30.0, "the rig's limit");
        assert_eq!(set_value(&r, &mut v, x, -99.0, 15.0), -30.0);
        assert_eq!(set_value(&r, &mut v, x, 7.3, 0.0), 7.3, "no snapping");
        assert_eq!(v.0[&x], 7.3);
        assert_eq!(snap(22.0, 15.0), 15.0);
        assert_eq!(snap(23.0, 15.0), 30.0);
    }

    #[test]
    fn a_drag_on_a_ring_becomes_a_slider_change() {
        // 100 px right on the left-right ring: half the range
        assert_eq!(x_delta(100.0, 30.0), 30.0);
        assert_eq!(x_delta(-50.0, 30.0), -15.0);
        // up raises the up-down slider
        assert_eq!(y_delta(-100.0, 30.0), 30.0);
        assert_eq!(y_delta(40.0, 30.0), -12.0);
        // around the tilt ring: from the right of the centre to below it is a quarter turn (90 degrees)
        let d = z_delta([200.0, 100.0], [100.0, 200.0], [100.0, 100.0], 30.0);
        assert!((d - 15.0).abs() < 0.01, "{d}");
        // the other way round is the opposite
        assert!((z_delta([100.0, 200.0], [200.0, 100.0], [100.0, 100.0], 30.0) + 15.0).abs() < 0.01);
        // across the seam of the angle (from just above to just below the left side) stays a small move
        let small = z_delta([0.0, 99.0], [0.0, 101.0], [100.0, 100.0], 30.0);
        assert!(small.abs() < 2.0, "{small}");
        assert_eq!(half_range(&head_rig(), targets(&head_rig())[0].x.unwrap()), 30.0);
    }

    #[test]
    fn spin_goes_from_the_left_side_through_the_front_to_the_right_side() {
        let r = head_rig();
        let t = &targets(&r)[0];
        let mut v = Values::default();
        let got: Vec<f32> = SPIN.iter().map(|(_, s)| spin(&r, &mut v, t, *s).unwrap()).collect();
        assert_eq!(got, [-30.0, -15.0, 0.0, 15.0, 30.0]);
        // the body has no left-right slider: nothing to spin
        assert!(spin(&r, &mut v, &targets(&r)[1], 1.0).is_none());
    }

    #[test]
    fn poses_and_clips_are_kept_and_cleaned() {
        let mut r = head_rig();
        let x = r.params[0].id;
        let mut v = Values::default();
        v.set(x, 20.0);
        v.set(r.params[1].id, 0.0); // at its default: not kept
        let kept = r.values_to_keep(&v);
        assert_eq!(kept, vec![(x, 20.0)]);
        r.poses.push(crate::rig::SavedPose { name: "Looking left".into(), values: kept.clone() });
        let back = r.values_from(&kept);
        assert_eq!(back.0[&x], 20.0);
        // a value outside the slider's range comes back inside it
        assert_eq!(r.values_from(&[(x, 99.0)]).0[&x], 30.0);
        let clip = crate::rig::Clip { name: "Nod".into(), fps: 10.0, frames: (0..20).map(|i| vec![(x, i as f32)]).collect() };
        assert!((clip.seconds() - 2.0).abs() < 1e-4);
        assert_eq!(clip.frame_at(0.35).unwrap()[0].1, 3.0);
        assert_eq!(clip.frame_at(2.05).unwrap()[0].1, 0.0, "it loops");
        r.clips.push(clip);
        let back: Rig = serde_json::from_str(&serde_json::to_string(&r).unwrap()).unwrap();
        assert_eq!(back, r);
        // bad data: a pose that names a slider that is gone, NaN values, an absurd frame rate
        let mut bad = r.clone();
        bad.poses.push(crate::rig::SavedPose { name: "x".into(), values: vec![(9999, 1.0), (x, f32::NAN)] });
        bad.clips[0].fps = f32::NAN;
        let ok = bad.sanitized();
        assert!(ok.poses.last().unwrap().values.is_empty());
        assert_eq!(ok.clips[0].fps, 30.0);
    }
}
