// SPDX-License-Identifier: GPL-3.0-or-later
// The other sex of a base, by hand-made rules and no AI: the shoulders, the chest, the waist, the hips and the neck are moved by proportion, as a share of the
// height, with soft edges, and the arms, the legs and anything that hangs on the body (wings, tails) are carried along so nothing tears. A mesh with a
// person standing on the floor (feet at the lowest point, the front towards +z) is what the rules expect; what comes out is a start to paint and move on.

use crate::assetview::AssetMesh;

/// How far the shape goes towards the other sex: 0 changes nothing, 1 is the other sex, and a little more is stronger.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Shift {
    pub amount: f32,
    /// A mesh that is a man (true) goes towards a woman, and the other way round.
    pub from_male: bool,
}

fn smooth(a: f32, b: f32, x: f32) -> f32 {
    let t = ((x - a) / (b - a)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

/// A bump that is 1 between `a` and `b` (heights as shares of the whole) and fades out over `fade` on both sides.
fn band(y: f32, a: f32, b: f32, fade: f32) -> f32 {
    smooth(a - fade, a, y) * (1.0 - smooth(b, b + fade, y))
}

/// The same mesh, with the shape of the other sex.
pub fn other_sex(mesh: &AssetMesh, s: Shift) -> AssetMesh {
    let mut out = mesh.clone();
    let (lo, hi) = mesh.bounds();
    let h = (hi[1] - lo[1]).max(1e-4);
    // the middle of the body in x and z: the points of the trunk (not the arms or the tails)
    let (mut sx, mut sz, mut n) = (0.0f64, 0.0f64, 0u32);
    for v in &mesh.verts {
        let y = (v[1] - lo[1]) / h;
        if (0.45..0.75).contains(&y) {
            sx += v[0] as f64;
            sz += v[2] as f64;
            n += 1;
        }
    }
    let (cx, cz) = if n > 0 { ((sx / n as f64) as f32, (sz / n as f64) as f32) } else { ((lo[0] + hi[0]) / 2.0, (lo[2] + hi[2]) / 2.0) };
    // to a man: broader shoulders and neck, flatter chest, narrower hips, a straighter waist, a little taller. To a woman: the other way round.
    let t = if s.from_male { -s.amount } else { s.amount };
    let core = 0.12 * h;
    for v in &mut out.verts {
        let y = (v[1] - lo[1]) / h;
        let (x, z) = (v[0] - cx, v[2] - cz);
        // how much of the width is the trunk: arms and tails beyond are carried by the same distance
        let wide = |amount: f32, w: f32| x.signum() * x.abs().min(core) * amount * w;
        let mut dx = 0.0;
        dx += wide(0.10 * t, band(y, 0.72, 0.82, 0.05));
        dx += wide(0.04 * t, band(y, 0.56, 0.64, 0.05));
        dx += wide(-0.09 * t, band(y, 0.45, 0.54, 0.06));
        dx += wide(0.10 * t, band(y, 0.84, 0.88, 0.03));
        let trunk = 1.0 - smooth(core * 0.9, core * 1.6, x.abs());
        // the chest: the front goes back (a man) or forward (a woman); the hips go back a little the other way
        let mut dz = 0.0;
        if z > 0.0 {
            dz += -z * 0.14 * t * band(y, 0.66, 0.76, 0.04) * trunk;
        }
        dz += z * 0.05 * t * band(y, 0.45, 0.54, 0.05) * trunk * -1.0;
        let dy = (v[1] - lo[1]) * 0.03 * t * smooth(0.0, 1.0, y);
        v[0] += dx;
        v[2] += dz;
        v[1] += dy;
    }
    out
}

/// The name of the other sex's file: `Midlanderbasefemale` is `Midlanderbasemale`. None when the name says neither.
pub fn other_name(stem: &str) -> Option<String> {
    let lower = stem.to_lowercase();
    let end = |word: &str| lower.ends_with(word).then(|| stem.len() - word.len());
    if let Some(i) = end("female") {
        let m = &stem[i..];
        let male = if m.chars().next().is_some_and(|c| c.is_uppercase()) { "Male" } else { "male" };
        return Some(format!("{}{male}", &stem[..i]));
    }
    if let Some(i) = end("male") {
        let f = if stem[i..].chars().next().is_some_and(|c| c.is_uppercase()) { "Female" } else { "female" };
        return Some(format!("{}{f}", &stem[..i]));
    }
    None
}

/// Is the base a woman by its name (a name without a sex: false)?
pub fn is_female(stem: &str) -> bool {
    stem.to_lowercase().ends_with("female")
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A body like a person with the arms out: a trunk, shoulders, a head, arms to the sides and legs.
    fn person() -> AssetMesh {
        let mut verts = Vec::new();
        for yi in 0..=40 {
            let y = yi as f32 * 0.05;
            for k in 0..16 {
                let a = k as f32 / 16.0 * std::f32::consts::TAU;
                let r = if y < 1.0 { 0.07 } else { 0.13 };
                verts.push([a.cos() * r, y, a.sin() * r * 0.7]);
            }
        }
        // the arms at the height of the shoulders
        for xi in 0..=20 {
            let x = 0.15 + xi as f32 * 0.04;
            for sgn in [-1.0f32, 1.0] {
                verts.push([sgn * x, 1.5, 0.0]);
            }
        }
        let uvs = vec![[0.0; 2]; verts.len()];
        AssetMesh { verts, uvs, tris: Vec::new(), texture: None, normal: None }
    }

    #[test]
    fn the_shoulders_are_broader_for_a_man_and_the_arms_move_with_them() {
        let m = person();
        let man = other_sex(&m, Shift { amount: 1.0, from_male: false });
        // the same point of the arm is further out
        let (i0, i1) = (657, 657 + 2 * 20);
        assert!(man.verts[i0][0].abs() > m.verts[i0][0].abs() + 0.005, "{:?} {:?}", man.verts[i0], m.verts[i0]);
        // the outer end of the arm moved by the same distance as the shoulder: the arm is as long as before
        let (a, b) = ((m.verts[i1][0] - m.verts[i0][0]).abs(), (man.verts[i1][0] - man.verts[i0][0]).abs());
        assert!((a - b).abs() < 0.01, "{a} {b}");
        // the feet stay on the floor
        assert!(man.verts.iter().map(|v| v[1]).fold(f32::MAX, f32::min).abs() < 1e-6);
    }

    #[test]
    fn a_man_going_to_a_woman_does_the_opposite() {
        let m = person();
        let woman = other_sex(&m, Shift { amount: 1.0, from_male: true });
        let man = other_sex(&m, Shift { amount: 1.0, from_male: false });
        let i = 657;
        assert!(woman.verts[i][0].abs() < m.verts[i][0].abs() && man.verts[i][0].abs() > m.verts[i][0].abs());
        assert_eq!(other_sex(&m, Shift { amount: 0.0, from_male: false }).verts, m.verts, "nothing changes with 0");
    }

    #[test]
    fn the_other_name_keeps_the_way_it_was_written() {
        assert_eq!(other_name("Midlanderbasefemale").as_deref(), Some("Midlanderbasemale"));
        assert_eq!(other_name("WolfBeastsBasemale").as_deref(), Some("WolfBeastsBasefemale"));
        assert_eq!(other_name("Ogrebasemale").as_deref(), Some("Ogrebasefemale"));
        assert_eq!(other_name("Lantern"), None);
        assert!(is_female("Pixiebasefemale") && !is_female("Ogrebasemale"));
    }
}
