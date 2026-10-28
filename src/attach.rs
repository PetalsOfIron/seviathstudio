// SPDX-License-Identifier: GPL-3.0-or-later
// Parts that are put on a base as geometry, with numbers you change (the variables of the mesh, not of the texture): horns, wings, a tail, ears. How many,
// how big, how curled, how far apart, and where on the body. Every part is built from the same strokes as the mesh brushes (`meshbrush.rs`) along curves that
// are worked out from the size of the base: the head is the top of it, the shoulder blades are at three quarters of its height, the back is where the body
// ends behind. The numbers are shares of the height, so the same numbers fit a tall ogre and a small pixie.

use crate::assetview::AssetMesh;
use crate::meshbrush::{self, Brush, Built, Kind, Spot};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Part {
    Horns,
    Wings,
    Tail,
    Ears,
    /// A plume: feathers in a fan.
    Feathers,
}

impl Part {
    pub const ALL: [Part; 5] = [Part::Horns, Part::Wings, Part::Tail, Part::Ears, Part::Feathers];

    pub fn label(self) -> &'static str {
        match self {
            Part::Horns => "Horns",
            Part::Wings => "Wings",
            Part::Tail => "Tail",
            Part::Ears => "Ears",
            Part::Feathers => "Feathers",
        }
    }

    /// Ready shapes of a part: a name and the numbers (change them afterwards like any others).
    pub fn presets(self) -> Vec<(&'static str, Params)> {
        let d = self.defaults();
        match self {
            Part::Horns => vec![
                ("Small", Params { size: 0.55, curl: 0.2, spread: 18.0, ..d }),
                ("Devil", Params { size: 1.0, curl: 0.6, spread: 30.0, thick: 1.2, ..d }),
                ("Ram", Params { size: 1.3, curl: 1.0, spread: 65.0, thick: 1.6, apart: 1.2, ..d }),
                ("Antlers", Params { count: 3, size: 1.5, curl: 0.3, spread: 22.0, thick: 0.6, ..d }),
                ("Unicorn", Params { size: 1.6, curl: 0.0, spread: 0.0, thick: 1.0, middle: true, ..d }),
                ("Long", Params { size: 1.8, curl: 0.1, spread: 12.0, thick: 0.9, ..d }),
            ],
            Part::Wings => vec![
                ("Bird", Params { count: 12, size: 1.0, spread: 35.0, curl: 0.2, thick: 1.0, ..d }),
                ("Angel", Params { count: 18, size: 1.5, spread: 55.0, curl: 0.15, thick: 1.1, ..d }),
                ("Fairy", Params { count: 4, size: 0.7, spread: 60.0, curl: 0.3, thick: 2.6, ..d }),
                ("Bat", Params { count: 5, size: 1.3, spread: 40.0, curl: -0.1, thick: 2.3, ..d }),
                ("Small", Params { count: 8, size: 0.5, spread: 45.0, curl: 0.2, thick: 0.9, ..d }),
            ],
            Part::Tail => vec![
                ("Cat", Params { size: 1.0, thick: 0.8, curl: 0.5, ..d }),
                ("Fox", Params { size: 1.0, thick: 2.6, curl: 0.3, ..d }),
                ("Lizard", Params { size: 1.3, thick: 1.1, curl: 0.15, ..d }),
                ("Dragon", Params { size: 1.6, thick: 1.8, curl: 0.35, ..d }),
                ("Bunny", Params { size: 0.12, thick: 2.6, curl: 0.0, ..d }),
                ("Whip", Params { size: 1.5, thick: 0.5, curl: 0.9, ..d }),
            ],
            Part::Ears => vec![
                ("Elf", Params { size: 1.0, spread: 70.0, ..d }),
                ("Cat", Params { size: 0.6, spread: 15.0, thick: 2.0, ..d }),
                ("Rabbit", Params { size: 2.0, spread: 8.0, thick: 1.3, curl: 0.3, ..d }),
            ],
            Part::Feathers => vec![("Plume", d), ("Wide fan", Params { count: 11, spread: 120.0, size: 0.8, ..d }), ("Single", Params { count: 1, size: 1.4, curl: 0.4, ..d })],
        }
    }

    /// The first numbers of a part.
    pub fn defaults(self) -> Params {
        match self {
            Part::Horns => Params { count: 1, size: 1.0, thick: 1.0, curl: 0.5, spread: 25.0, up: 0.0, back: 0.0, apart: 1.0, middle: false },
            Part::Wings => Params { count: 10, size: 1.0, thick: 1.0, curl: 0.2, spread: 35.0, up: 0.0, back: 0.0, apart: 1.0, middle: false },
            Part::Tail => Params { count: 1, size: 1.0, thick: 1.0, curl: 0.4, spread: 0.0, up: 0.0, back: 0.0, apart: 1.0, middle: false },
            Part::Ears => Params { count: 1, size: 1.0, thick: 1.0, curl: 0.0, spread: 55.0, up: 0.0, back: 0.0, apart: 1.0, middle: false },
            Part::Feathers => Params { count: 7, size: 1.0, thick: 1.0, curl: 0.3, spread: 70.0, up: 0.0, back: 0.0, apart: 1.0, middle: false },
        }
    }
}

/// The numbers of a part.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Params {
    /// How many on each side (horns), how many feathers on each wing.
    pub count: usize,
    /// The size as a multiplier of the usual size.
    pub size: f32,
    /// How thick (horns, tail) or how wide (feathers, ears), as a multiplier.
    pub thick: f32,
    /// How much it bends, from -1 to 1 (towards the back or the front, up or down).
    pub curl: f32,
    /// The angle it stands out from the body, in degrees (horns, ears), or how far the wings are raised.
    pub spread: f32,
    /// Moved up and moved back, as shares of the height.
    pub up: f32,
    pub back: f32,
    /// How far from the middle, as a multiplier.
    pub apart: f32,
    /// One horn in the middle (a unicorn) instead of a pair.
    pub middle: bool,
}

/// Where the parts go on a base, worked out from its size.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Anchors {
    pub height: f32,
    /// The middle of the body (x), the top of the head, the depth (z) of the middle of the head, and of the back at the shoulder blades and at the hips.
    pub cx: f32,
    pub head_top: f32,
    pub head_z: f32,
    pub floor: f32,
    pub back_shoulder_z: f32,
    pub back_hip_z: f32,
}

impl Anchors {
    pub fn of(mesh: &AssetMesh) -> Anchors {
        let (lo, hi) = mesh.bounds();
        let h = (hi[1] - lo[1]).max(1e-4);
        let band = |a: f32, b: f32, f: &mut dyn FnMut(&[f32; 3])| {
            for v in &mesh.verts {
                let y = (v[1] - lo[1]) / h;
                if y >= a && y < b {
                    f(v);
                }
            }
        };
        // the middle of the body in x: the legs and the trunk
        let (mut sx, mut n) = (0.0f64, 0u32);
        band(0.4, 0.8, &mut |v| {
            if v[0].abs() < 0.2 * h {
                sx += v[0] as f64;
                n += 1;
            }
        });
        let cx = if n > 0 { (sx / n as f64) as f32 } else { (lo[0] + hi[0]) / 2.0 };
        let (mut top, mut zs, mut zn) = (f32::MIN, 0.0f64, 0u32);
        band(0.88, 1.01, &mut |v| {
            if (v[0] - cx).abs() < 0.08 * h {
                top = top.max(v[1]);
                zs += v[2] as f64;
                zn += 1;
            }
        });
        let back = |a: f32, b: f32| {
            let mut z = f32::MAX;
            band(a, b, &mut |v| {
                if (v[0] - cx).abs() < 0.1 * h {
                    z = z.min(v[2]);
                }
            });
            z
        };
        let head_z = if zn > 0 { (zs / zn as f64) as f32 } else { 0.0 };
        let (bs, bh) = (back(0.7, 0.8), back(0.45, 0.55));
        Anchors { height: h, cx, head_top: if top > f32::MIN { top } else { hi[1] }, head_z, floor: lo[1], back_shoulder_z: if bs < f32::MAX { bs } else { head_z - 0.05 * h }, back_hip_z: if bh < f32::MAX { bh } else { head_z - 0.05 * h } }
    }
}

type V3 = [f32; 3];

fn add(a: V3, b: V3) -> V3 {
    [a[0] + b[0], a[1] + b[1], a[2] + b[2]]
}

fn mul(a: V3, k: f32) -> V3 {
    [a[0] * k, a[1] * k, a[2] * k]
}

fn cross(a: V3, b: V3) -> V3 {
    [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]]
}

fn unit(a: V3) -> V3 {
    let l = (a[0] * a[0] + a[1] * a[1] + a[2] * a[2]).sqrt().max(1e-9);
    mul(a, 1.0 / l)
}

/// Turn a direction about an axis (Rodrigues).
fn turn(d: V3, axis: V3, angle: f32) -> V3 {
    let (s, c) = angle.sin_cos();
    let k = axis;
    let kd = k[0] * d[0] + k[1] * d[1] + k[2] * d[2];
    add(add(mul(d, c), mul(cross(k, d), s)), mul(k, kd * (1.0 - c)))
}

/// A curve from a start, going in a direction, that bends about an axis as it goes: the spots are along it, and they all face the axis.
fn arc(start: V3, dir: V3, length: f32, axis: V3, bend: f32, steps: usize) -> Vec<Spot> {
    let mut p = start;
    let mut out = vec![Spot { p, n: axis }];
    for i in 0..steps {
        let t = (i as f32 + 0.5) / steps as f32;
        let d = turn(unit(dir), unit(axis), bend * t);
        p = add(p, mul(d, length / steps as f32));
        out.push(Spot { p, n: axis });
    }
    out
}

fn stroke(kind: Kind, width: f32, taper: f32, spots: &[Spot], root: [u8; 3], tip: [u8; 3]) -> Option<Built> {
    meshbrush::build(&Brush { kind, width, taper, lift: 0.0, droop: 0.0, root, tip, ..Brush::default() }, spots)
}

fn join(parts: Vec<Built>) -> Built {
    let mut out = Built::default();
    for b in parts {
        let base = out.verts.len() as u32;
        out.verts.extend(b.verts);
        out.uvs.extend(b.uvs);
        out.tris.extend(b.tris.into_iter().map(|t| [t[0] + base, t[1] + base, t[2] + base]));
    }
    out
}

/// The geometry of a part (one piece of geometry, with texture coordinates inside one cell: v from the root to the tip).
pub fn build(part: Part, p: &Params, a: &Anchors) -> Option<Built> {
    let h = a.height;
    let mut pieces: Vec<Built> = Vec::new();
    let colours = ([0u8; 3], [0u8; 3]);
    let _ = colours;
    let side_signs = [-1.0f32, 1.0];
    match part {
        Part::Horns => {
            let n = p.count.clamp(1, 4);
            for s in if p.middle { vec![0.0f32] } else { side_signs.to_vec() } {
                for i in 0..n {
                    // the roots stand in a row on the top of the head, the outer ones lower and further out
                    let k = if n == 1 { 0.0 } else { i as f32 / (n - 1) as f32 };
                    let x = a.cx + s * (0.03 + 0.03 * k) * h * p.apart * if p.middle { 0.0 } else { 1.0 };
                    let start = [x, a.head_top - (0.02 + 0.03 * k) * h + p.up * h, a.head_z - 0.02 * h - p.back * h];
                    let ang = p.spread.to_radians() * (1.0 + 0.4 * k);
                    let dir = [s * ang.sin(), ang.cos(), if p.middle { 0.35 } else { -0.2 }];
                    let axis = [0.0, 0.0, 1.0f32];
                    // bending about z makes a horn curve in or out; the sign of the side makes both curve the same way
                    let len = 0.12 * h * p.size * (1.0 - 0.25 * k);
                    let spots = arc(start, dir, len, [0.0, 0.0, -s * axis[2]], p.curl * 1.6, 10);
                    pieces.extend(stroke(Kind::Rope, 0.018 * h * p.thick, 1.0, &spots, [0; 3], [0; 3]));
                }
            }
        }
        Part::Ears => {
            for s in side_signs {
                let start = [a.cx + s * 0.075 * h * p.apart, a.head_top - 0.07 * h + p.up * h, a.head_z - p.back * h];
                let ang = p.spread.to_radians();
                let dir = [s * ang.sin(), ang.cos() * 0.8, -0.1];
                let spots = arc(start, dir, 0.07 * h * p.size, [0.0, 0.0, -s], p.curl * 1.2, 6);
                pieces.extend(stroke(Kind::Card, 0.03 * h * p.thick, 0.85, &spots, [0; 3], [0; 3]));
            }
        }
        Part::Feathers => {
            // a plume on the back of the head: the feathers fan out in the plane of sideways and up-and-back
            let n = p.count.clamp(1, 24);
            let v = unit([0.0, 1.0, -0.5]);
            let u = [1.0f32, 0.0, 0.0];
            let normal = unit(cross(u, v));
            let root = [a.cx, a.head_top - 0.04 * h + p.up * h, a.head_z - 0.05 * h - p.back * h];
            let fan = p.spread.to_radians();
            for i in 0..n {
                let t = if n == 1 { 0.5 } else { i as f32 / (n - 1) as f32 };
                let phi = (t - 0.5) * fan;
                let dir = add(mul(v, phi.cos()), mul(u, phi.sin()));
                let len = 0.25 * h * p.size * (1.0 - 0.3 * (t - 0.5).abs() * 2.0);
                let spots = arc(add(root, mul(u, (t - 0.5) * 0.04 * h * p.apart)), dir, len, normal, p.curl * 0.8 * (t - 0.5).signum(), 8);
                pieces.extend(stroke(Kind::Feather, 0.05 * h * p.thick, 0.25, &spots, [0; 3], [0; 3]));
            }
        }
        Part::Tail => {
            let start = [a.cx, a.floor + 0.5 * h + p.up * h, a.back_hip_z + 0.01 * h - p.back * h];
            let spots = arc(start, [0.0, -0.35, -1.0], 0.5 * h * p.size, [1.0, 0.0, 0.0], p.curl * 2.2, 24);
            pieces.extend(stroke(Kind::Rope, 0.035 * h * p.thick, 0.85, &spots, [0; 3], [0; 3]));
        }
        Part::Wings => {
            let n = p.count.clamp(3, 24);
            let lift = p.spread.to_radians();
            for s in side_signs {
                let u = [s, 0.0, 0.0];
                // the wing stands out sideways, up and back
                let v = unit([0.0, lift.sin(), -lift.cos().max(0.2)]);
                let normal = unit(cross(u, v));
                let root = [a.cx + s * 0.03 * h * p.apart, a.floor + 0.76 * h + p.up * h, a.back_shoulder_z - 0.01 * h - p.back * h];
                for i in 0..n {
                    let t = i as f32 / (n - 1) as f32;
                    // the feathers fan from straight up (the first) to straight out (the last)
                    let phi = std::f32::consts::FRAC_PI_2 * (0.1 + 0.9 * t);
                    let dir = add(mul(u, phi.sin()), mul(v, phi.cos()));
                    let len = 0.45 * h * p.size * (0.55 + 0.45 * (1.0 - (t - 0.55).abs() * 1.4).clamp(0.0, 1.0));
                    let start = add(root, mul(u, 0.02 * h * p.apart * t));
                    let spots = arc(start, dir, len, normal, p.curl * 1.2 * s, 8);
                    pieces.extend(stroke(Kind::Card, 0.05 * h * p.thick, 0.55, &spots, [0; 3], [0; 3]));
                }
            }
        }
    }
    let b = join(pieces);
    (!b.tris.is_empty()).then_some(b)
}

/// A chain of bones for a part: a name, and the joints from the root to the end.
#[derive(Clone, Debug, PartialEq)]
pub struct RigChain {
    pub name: String,
    pub joints: Vec<[f32; 3]>,
}

/// The bones of a part that moves: a tail has a chain along it (it swings), a wing has a chain along its span on each side (it flaps and folds). Parts that stay
/// still (horns, ears, feathers) have none.
pub fn rigging(part: Part, p: &Params, a: &Anchors) -> Vec<RigChain> {
    let h = a.height;
    match part {
        Part::Tail => {
            let start = [a.cx, a.floor + 0.5 * h + p.up * h, a.back_hip_z + 0.01 * h - p.back * h];
            let spots = arc(start, [0.0, -0.35, -1.0], 0.5 * h * p.size, [1.0, 0.0, 0.0], p.curl * 2.2, 24);
            let pieces = 6;
            let joints: Vec<[f32; 3]> = (0..=pieces).map(|k| spots[(k * (spots.len() - 1)) / pieces].p).collect();
            vec![RigChain { name: "tail".into(), joints }]
        }
        Part::Wings => {
            let span = 0.45 * h * p.size;
            [("L", 1.0f32), ("R", -1.0)]
                .into_iter()
                .map(|(side, s)| {
                    let root = [a.cx + s * 0.03 * h * p.apart, a.floor + 0.76 * h + p.up * h, a.back_shoulder_z - 0.01 * h - p.back * h];
                    // along the span: out to the side, a little up and back
                    let lift = p.spread.to_radians();
                    let dir = unit([s * 1.0, lift.sin() * 0.3, -lift.cos().max(0.2) * 0.25]);
                    RigChain { name: format!("wing_{side}"), joints: (0..=3).map(|k| add(root, mul(dir, span * k as f32 / 3.0))).collect() }
                })
                .collect()
        }
        _ => Vec::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A person of 1.8 metres: a trunk and a head, as points and triangles.
    fn person() -> AssetMesh {
        let mut verts = Vec::new();
        for yi in 0..=36 {
            let y = yi as f32 * 0.05;
            for k in 0..12 {
                let a = k as f32 / 12.0 * std::f32::consts::TAU;
                let r = if y > 1.55 { 0.09 } else { 0.16 };
                verts.push([a.cos() * r, y, 0.4 + a.sin() * r * 0.6]);
            }
        }
        AssetMesh { uvs: vec![[0.0; 2]; verts.len()], verts, tris: Vec::new(), texture: None, normal: None }
    }

    #[test]
    fn the_anchors_come_from_the_size_of_the_body() {
        let a = Anchors::of(&person());
        assert!((a.height - 1.8).abs() < 1e-4);
        assert!((a.head_top - 1.8).abs() < 1e-4 && a.floor.abs() < 1e-4);
        assert!(a.back_shoulder_z < 0.4 && a.back_shoulder_z > 0.2, "{}", a.back_shoulder_z);
        assert!(a.cx.abs() < 0.03);
    }

    #[test]
    fn horns_stand_in_pairs_on_the_head_and_more_of_them_means_more_geometry() {
        let a = Anchors::of(&person());
        let one = build(Part::Horns, &Part::Horns.defaults(), &a).unwrap();
        let three = build(Part::Horns, &Params { count: 3, ..Part::Horns.defaults() }, &a).unwrap();
        assert!(three.verts.len() > one.verts.len() * 2);
        // both sides, above the head
        assert!(one.verts.iter().any(|v| v[0] > 0.02) && one.verts.iter().any(|v| v[0] < -0.02));
        assert!(one.verts.iter().all(|v| v[1] > 1.6), "the horns are on the head");
        let big = build(Part::Horns, &Params { size: 2.0, ..Part::Horns.defaults() }, &a).unwrap();
        let top = |b: &Built| b.verts.iter().map(|v| v[1]).fold(f32::MIN, f32::max);
        assert!(top(&big) > top(&one) + 0.05);
    }

    #[test]
    fn wings_come_out_of_the_back_at_the_shoulders_on_both_sides() {
        let a = Anchors::of(&person());
        let w = build(Part::Wings, &Part::Wings.defaults(), &a).unwrap();
        let (mut lo, mut hi) = ([f32::MAX; 3], [f32::MIN; 3]);
        for v in &w.verts {
            for k in 0..3 {
                lo[k] = lo[k].min(v[k]);
                hi[k] = hi[k].max(v[k]);
            }
        }
        assert!(lo[0] < -0.4 && hi[0] > 0.4, "{lo:?} {hi:?}: a span to both sides");
        let mean_z = w.verts.iter().map(|v| v[2]).sum::<f32>() / w.verts.len() as f32;
        assert!(mean_z < a.back_shoulder_z && hi[2] < a.back_shoulder_z + 0.15, "behind the back: {mean_z} {hi:?} {}", a.back_shoulder_z);
        assert!(w.verts.iter().all(|v| v[1] > 1.2), "at the shoulders and above");
        let few = build(Part::Wings, &Params { count: 4, ..Part::Wings.defaults() }, &a).unwrap();
        assert!(few.verts.len() < w.verts.len());
    }

    #[test]
    fn a_tail_hangs_from_the_hips_and_ears_stand_on_the_sides_of_the_head() {
        let a = Anchors::of(&person());
        let t = build(Part::Tail, &Part::Tail.defaults(), &a).unwrap();
        assert!(t.verts.iter().all(|v| v[2] < a.back_hip_z + 0.05), "behind the hips");
        let e = build(Part::Ears, &Part::Ears.defaults(), &a).unwrap();
        assert!(e.verts.iter().any(|v| v[0] > 0.05) && e.verts.iter().any(|v| v[0] < -0.05));
        assert!(e.tris.iter().flatten().all(|i| (*i as usize) < e.verts.len()));
        assert!(e.uvs.iter().all(|q| (0.0..=1.0).contains(&q[0]) && (0.0..=1.0).contains(&q[1])));
    }

    #[test]
    fn a_plume_of_feathers_stands_on_the_head_and_more_feathers_are_more_geometry() {
        let a = Anchors::of(&person());
        let f = build(Part::Feathers, &Part::Feathers.defaults(), &a).unwrap();
        assert!(f.verts.iter().all(|v| v[1] > 1.5), "on the head");
        let few = build(Part::Feathers, &Params { count: 3, ..Part::Feathers.defaults() }, &a).unwrap();
        assert!(few.verts.len() < f.verts.len());
        assert!(f.verts.iter().any(|v| v[0] > 0.02) && f.verts.iter().any(|v| v[0] < -0.02), "a fan: both sides");
    }

    #[test]
    fn every_part_has_presets_that_build_and_a_unicorn_has_one_horn_in_the_middle() {
        let a = Anchors::of(&person());
        for part in Part::ALL {
            let presets = part.presets();
            assert!(presets.len() >= 3, "{part:?}");
            for (name, p) in presets {
                let m = build(part, &p, &a).unwrap_or_else(|| panic!("{part:?} {name} makes nothing"));
                assert!(m.verts.iter().flatten().all(|x| x.is_finite()), "{part:?} {name}");
            }
        }
        let unicorn = Part::Horns.presets().into_iter().find(|p| p.0 == "Unicorn").unwrap().1;
        let m = build(Part::Horns, &unicorn, &a).unwrap();
        let xs: Vec<f32> = m.verts.iter().map(|v| v[0]).collect();
        assert!(xs.iter().all(|x| x.abs() < 0.06), "one horn in the middle");
        let pair = build(Part::Horns, &Part::Horns.defaults(), &a).unwrap();
        assert!(m.verts.len() < pair.verts.len(), "one horn, not two");
    }

    #[test]
    fn a_tail_has_a_chain_of_bones_along_it_and_wings_one_along_each_span() {
        let a = Anchors::of(&person());
        let tail = rigging(Part::Tail, &Part::Tail.defaults(), &a);
        assert_eq!(tail.len(), 1);
        assert_eq!(tail[0].joints.len(), 7);
        assert!(tail[0].joints.iter().all(|j| j[2] < a.back_hip_z + 0.05), "behind the hips");
        let wings = rigging(Part::Wings, &Part::Wings.defaults(), &a);
        assert_eq!(wings.iter().map(|w| w.name.as_str()).collect::<Vec<_>>(), vec!["wing_L", "wing_R"]);
        assert!(wings[0].joints.last().unwrap()[0] > 0.2 && wings[1].joints.last().unwrap()[0] < -0.2, "one on each side");
        assert!(rigging(Part::Horns, &Part::Horns.defaults(), &a).is_empty());
    }
}
