// SPDX-License-Identifier: GPL-3.0-or-later
// The looks of a base as variable textures: skin colour and tone, a pattern over the body (spots, stripes, patches), and the face (eyes, brows, nose, mouth,
// blush, freckles) drawn by the place of every texel on the mesh, so it lands on the face of any unwrapped base. The numbers are the "variable features";
// one press on Random gives another person. What is drawn is only the first layer of the texture: the rest is painted by hand over it.

use rayon::prelude::*;

use crate::assetview::AssetMesh;

/// Where the face is on a mesh: the middle of the eye line on the front of the head, the size of a unit of the face (metres), and which way the face looks.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Face {
    pub centre: [f32; 3],
    pub scale: f32,
    /// +1: the face looks towards +z, -1: towards -z.
    pub front: f32,
}

impl Face {
    /// A guess from the size of the mesh: the head is the top of it. Fine to move afterwards.
    pub fn guess(mesh: &AssetMesh) -> Face {
        let (lo, hi) = mesh.bounds();
        let h = (hi[1] - lo[1]).max(1e-4);
        let head: Vec<&[f32; 3]> = mesh.verts.iter().filter(|v| v[1] > hi[1] - 0.12 * h).collect();
        let (mut sx, mut n) = (0.0f64, 0u32);
        for v in &head {
            sx += v[0] as f64;
            n += 1;
        }
        let cx = if n > 0 { (sx / n as f64) as f32 } else { (lo[0] + hi[0]) / 2.0 };
        // the front of the face: the points near the middle line that stick out most towards +z (a face is flatter and deeper than the back of a head)
        let (mut zmax, mut zmin) = (f32::MIN, f32::MAX);
        for v in head.iter().filter(|v| (v[0] - cx).abs() < 0.03 * h) {
            zmax = zmax.max(v[2]);
            zmin = zmin.min(v[2]);
        }
        let front = if zmax.abs() >= zmin.abs() { 1.0 } else { -1.0 };
        let z = if front > 0.0 { zmax } else { zmin };
        Face { centre: [cx, hi[1] - 0.075 * h, if zmax >= zmin { z } else { 0.0 }], scale: 0.037 * h, front }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Pattern {
    None,
    Spots,
    Stripes,
    Patches,
}

impl Pattern {
    pub const ALL: [Pattern; 4] = [Pattern::None, Pattern::Spots, Pattern::Stripes, Pattern::Patches];

    pub fn label(self) -> &'static str {
        match self {
            Pattern::None => "None",
            Pattern::Spots => "Spots",
            Pattern::Stripes => "Stripes",
            Pattern::Patches => "Patches",
        }
    }
}

/// Every number of the looks.
#[derive(Clone, Debug, PartialEq)]
pub struct Look {
    pub skin: [u8; 3],
    /// How much darker the skin is towards the feet (0 to 1).
    pub tone: f32,
    pub pattern: Pattern,
    pub pattern_color: [u8; 3],
    pub pattern_size: f32,
    pub pattern_strength: f32,
    pub eye: [u8; 3],
    pub eye_w: f32,
    pub eye_h: f32,
    pub eye_gap: f32,
    pub eye_y: f32,
    pub brow: [u8; 3],
    pub brow_thick: f32,
    pub brow_y: f32,
    pub brow_tilt: f32,
    pub lip: [u8; 3],
    pub mouth_w: f32,
    pub mouth_y: f32,
    pub mouth_thick: f32,
    pub nose_y: f32,
    pub nose_shade: f32,
    pub blush: f32,
    pub blush_color: [u8; 3],
    pub freckles: f32,
}

impl Default for Look {
    fn default() -> Self {
        Look {
            skin: [226, 178, 150],
            tone: 0.1,
            pattern: Pattern::None,
            pattern_color: [150, 100, 90],
            pattern_size: 1.0,
            pattern_strength: 0.6,
            eye: [70, 140, 90],
            eye_w: 0.3,
            eye_h: 0.18,
            eye_gap: 0.5,
            eye_y: 0.0,
            brow: [70, 45, 40],
            brow_thick: 0.05,
            brow_y: 0.34,
            brow_tilt: 0.1,
            lip: [190, 90, 100],
            mouth_w: 0.45,
            mouth_y: -1.05,
            mouth_thick: 0.09,
            nose_y: -0.55,
            nose_shade: 0.35,
            blush: 0.25,
            blush_color: [235, 120, 120],
            freckles: 0.0,
        }
    }
}

fn hash(x: i32, y: i32, z: i32, seed: u32) -> f32 {
    let mut h = (x as u32).wrapping_mul(0x9E37_79B1) ^ (y as u32).wrapping_mul(0x85EB_CA77) ^ (z as u32).wrapping_mul(0xC2B2_AE3D) ^ seed.wrapping_mul(0x27D4_EB2F);
    h ^= h >> 15;
    h = h.wrapping_mul(0x2C1B_3C6D);
    h ^= h >> 12;
    h = h.wrapping_mul(0x297A_2D39);
    h ^= h >> 15;
    (h & 0xFFFF) as f32 / 65535.0
}

/// Smooth noise in space (0 to 1).
fn noise(p: [f32; 3], seed: u32) -> f32 {
    let (x, y, z) = (p[0].floor(), p[1].floor(), p[2].floor());
    let (fx, fy, fz) = (p[0] - x, p[1] - y, p[2] - z);
    let s = |t: f32| t * t * (3.0 - 2.0 * t);
    let (ux, uy, uz) = (s(fx), s(fy), s(fz));
    let (xi, yi, zi) = (x as i32, y as i32, z as i32);
    let at = |dx, dy, dz| hash(xi + dx, yi + dy, zi + dz, seed);
    let l = |a: f32, b: f32, t: f32| a + (b - a) * t;
    l(
        l(l(at(0, 0, 0), at(1, 0, 0), ux), l(at(0, 1, 0), at(1, 1, 0), ux), uy),
        l(l(at(0, 0, 1), at(1, 0, 1), ux), l(at(0, 1, 1), at(1, 1, 1), ux), uy),
        uz,
    )
}

fn mix(a: [f32; 3], b: [u8; 3], t: f32) -> [f32; 3] {
    let t = t.clamp(0.0, 1.0);
    [a[0] + (b[0] as f32 - a[0]) * t, a[1] + (b[1] as f32 - a[1]) * t, a[2] + (b[2] as f32 - a[2]) * t]
}

/// The colour of one place of the surface: the place in space, the way the surface faces, and the height of the mesh as a share (0 at the feet).
pub fn colour_at(look: &Look, face: &Face, p: [f32; 3], normal: [f32; 3], rel_y: f32, seed: u32) -> [u8; 3] {
    let skin = look.skin;
    let mut c = [skin[0] as f32, skin[1] as f32, skin[2] as f32];
    // darker towards the feet
    let dark = 1.0 - look.tone * (1.0 - rel_y).clamp(0.0, 1.0) * 0.5;
    c = [c[0] * dark, c[1] * dark, c[2] * dark];
    // the pattern over the body
    let freq = 6.0 / look.pattern_size.max(0.1);
    let pat = match look.pattern {
        Pattern::None => 0.0,
        Pattern::Spots => ((noise([p[0] * freq, p[1] * freq, p[2] * freq], seed) - 0.62) * 9.0).clamp(0.0, 1.0),
        Pattern::Stripes => {
            let wobble = noise([p[0] * freq * 0.5, p[1] * freq * 0.5, p[2] * freq * 0.5], seed) * 2.0;
            (((p[1] * freq * 1.5 + wobble).sin()) * 3.0).clamp(0.0, 1.0)
        }
        Pattern::Patches => ((noise([p[0] * freq * 0.5, p[1] * freq * 0.5, p[2] * freq * 0.5], seed) - 0.5) * 6.0).clamp(0.0, 1.0),
    };
    c = mix(c, look.pattern_color, pat * look.pattern_strength);
    // the face
    let facing = normal[0] * 0.0 + normal[2] * face.front;
    let (fx, fy) = ((p[0] - face.centre[0]) / face.scale, (p[1] - face.centre[1]) / face.scale);
    if facing > 0.12 && fx.abs() < 1.6 && fy > -1.7 && fy < 1.0 && (p[2] - face.centre[2]) * face.front > -0.12 {
        // freckles, on the cheeks and the nose
        if look.freckles > 0.0 && fy < 0.1 && fy > -0.9 && fx.abs() < 1.1 {
            let (cx, cy) = ((fx * 14.0).floor() as i32, (fy * 14.0).floor() as i32);
            if hash(cx, cy, 7, seed) < look.freckles * 0.35 {
                let (u, v) = (fx * 14.0 - cx as f32 - 0.5, fy * 14.0 - cy as f32 - 0.5);
                if u * u + v * v < 0.07 {
                    c = mix(c, [120, 70, 50], 0.55);
                }
            }
        }
        // the blush
        for e in [-1.0f32, 1.0] {
            let d = (((fx - e * 0.8).powi(2) + ((fy + 0.45) * 1.4).powi(2)).sqrt()) / 0.42;
            if d < 1.0 {
                c = mix(c, look.blush_color, look.blush * (1.0 - d) * (1.0 - d));
            }
        }
        // the nose: two nostrils and a shade under the tip
        for e in [-1.0f32, 1.0] {
            let d = (((fx - e * 0.13).powi(2) / 0.012) + ((fy - look.nose_y).powi(2) / 0.006)).sqrt();
            if d < 1.0 {
                c = mix(c, [60, 30, 30], look.nose_shade * (1.0 - d));
            }
        }
        // the eyes
        for e in [-1.0f32, 1.0] {
            let (dx, dy) = (fx - e * look.eye_gap, fy - look.eye_y);
            let r = ((dx / look.eye_w).powi(2) + (dy / look.eye_h).powi(2)).sqrt();
            if r < 1.0 {
                // the white, the iris with a darker ring, the pupil and a small light
                c = [245.0, 242.0, 238.0];
                let ri = ((dx / (look.eye_w * 0.55)).powi(2) + (dy / (look.eye_h * 0.95)).powi(2)).sqrt();
                if ri < 1.0 {
                    let ring = smooth_step(0.7, 1.0, ri);
                    c = mix([look.eye[0] as f32, look.eye[1] as f32, look.eye[2] as f32], [20, 20, 25], ring * 0.7);
                    if ri < 0.38 {
                        c = [12.0, 12.0, 16.0];
                    }
                    let (hx, hy) = (dx / look.eye_w + 0.12, dy / look.eye_h - 0.3);
                    if hx * hx + hy * hy < 0.025 {
                        c = [255.0, 255.0, 255.0];
                    }
                }
                // the upper lid line
                if dy > 0.0 && r > 0.82 {
                    c = mix(c, [30, 20, 22], 0.9);
                }
            } else if r < 1.18 && dy > -0.02 * look.eye_h {
                // lashes: a dark rim above the eye
                c = mix(c, [35, 25, 28], (1.0 - (r - 1.0) / 0.18) * 0.8);
            }
            // the brow: a curved bar that tilts towards the temple
            let along = dx;
            let by = look.brow_y + look.brow_tilt * (along * e);
            if along.abs() < look.eye_w * 1.35 && (fy - by).abs() < look.brow_thick * (1.0 - (along / (look.eye_w * 1.4)).powi(2)).max(0.0).sqrt() + 0.01 {
                c = mix(c, look.brow, 0.9);
            }
        }
        // the mouth: a thin lens with a darker line along the middle
        let (mx, my) = (fx, fy - look.mouth_y);
        if mx.abs() < look.mouth_w {
            let shape = (1.0 - (mx / look.mouth_w).powi(2)).max(0.0).sqrt();
            let half = look.mouth_thick * shape;
            if my.abs() < half {
                c = mix(c, look.lip, 0.95);
                if my.abs() < half * 0.18 {
                    c = mix(c, [70, 25, 35], 0.9);
                }
            }
        }
    }
    [c[0].clamp(0.0, 255.0) as u8, c[1].clamp(0.0, 255.0) as u8, c[2].clamp(0.0, 255.0) as u8]
}

fn smooth_step(a: f32, b: f32, x: f32) -> f32 {
    let t = ((x - a) / (b - a)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

/// The texture of the looks: every texel of every triangle gets its colour from the place on the mesh. The texels between the pieces get the colour of the
/// nearest piece, so nothing shows at the seams.
pub fn bake(mesh: &AssetMesh, size: u32, look: &Look, face: &Face, seed: u32) -> image::RgbaImage {
    let (lo, hi) = mesh.bounds();
    let h = (hi[1] - lo[1]).max(1e-4);
    let s = size as f32;
    let cells: Vec<(u32, u32, [u8; 3])> = mesh
        .tris
        .par_iter()
        .flat_map_iter(|t| {
            let (pa, pb, pc) = (mesh.verts[t[0] as usize], mesh.verts[t[1] as usize], mesh.verts[t[2] as usize]);
            let e1 = [pb[0] - pa[0], pb[1] - pa[1], pb[2] - pa[2]];
            let e2 = [pc[0] - pa[0], pc[1] - pa[1], pc[2] - pa[2]];
            let mut n = [e1[1] * e2[2] - e1[2] * e2[1], e1[2] * e2[0] - e1[0] * e2[2], e1[0] * e2[1] - e1[1] * e2[0]];
            let l = (n[0] * n[0] + n[1] * n[1] + n[2] * n[2]).sqrt().max(1e-12);
            n = [n[0] / l, n[1] / l, n[2] / l];
            let (a, b, c) = (mesh.uvs[t[0] as usize], mesh.uvs[t[1] as usize], mesh.uvs[t[2] as usize]);
            let (ax, ay, bx, by, cx, cy) = (a[0] * s, a[1] * s, b[0] * s, b[1] * s, c[0] * s, c[1] * s);
            let area = (bx - ax) * (cy - ay) - (by - ay) * (cx - ax);
            let mut out = Vec::new();
            if area.abs() < 1e-9 {
                return out.into_iter();
            }
            let (x0, x1) = ((ax.min(bx).min(cx) - 1.0).floor().max(0.0) as u32, (ax.max(bx).max(cx) + 1.0).ceil().min(s - 1.0) as u32);
            let (y0, y1) = ((ay.min(by).min(cy) - 1.0).floor().max(0.0) as u32, (ay.max(by).max(cy) + 1.0).ceil().min(s - 1.0) as u32);
            for y in y0..=y1 {
                for x in x0..=x1 {
                    let (px, py) = (x as f32 + 0.5, y as f32 + 0.5);
                    let w0 = ((bx - px) * (cy - py) - (by - py) * (cx - px)) / area;
                    let w1 = ((cx - px) * (ay - py) - (cy - py) * (ax - px)) / area;
                    let w2 = 1.0 - w0 - w1;
                    // a little outside of the triangle counts too (the border of the piece)
                    if w0 < -0.15 || w1 < -0.15 || w2 < -0.15 {
                        continue;
                    }
                    let (w0, w1, w2) = (w0.clamp(0.0, 1.0), w1.clamp(0.0, 1.0), w2.clamp(0.0, 1.0));
                    let q = [w0 * pa[0] + w1 * pb[0] + w2 * pc[0], w0 * pa[1] + w1 * pb[1] + w2 * pc[1], w0 * pa[2] + w1 * pb[2] + w2 * pc[2]];
                    out.push((x, y, colour_at(look, face, q, n, (q[1] - lo[1]) / h, seed)));
                }
            }
            out.into_iter()
        })
        .collect();
    // the texture is the colour of the skin where nothing was drawn
    let base = [look.skin[0], look.skin[1], look.skin[2], 255];
    let mut img = image::RgbaImage::from_pixel(size, size, image::Rgba(base));
    for (x, y, c) in cells {
        img.put_pixel(x, y, image::Rgba([c[0], c[1], c[2], 255]));
    }
    img
}

/// A look chosen by chance from the seed: another person every time, with a skin that fits a body.
pub fn random_look(seed: u32) -> Look {
    let r = |k: u32| hash(k as i32, 3, 9, seed);
    let skins: [[u8; 3]; 8] = [[240, 205, 185], [226, 178, 150], [200, 150, 115], [160, 110, 80], [110, 75, 55], [170, 190, 210], [150, 200, 160], [200, 150, 190]];
    let eyes: [[u8; 3]; 8] = [[70, 140, 90], [60, 110, 200], [110, 70, 40], [150, 60, 60], [200, 170, 60], [140, 90, 190], [60, 60, 70], [90, 190, 200]];
    let lips: [[u8; 3]; 5] = [[190, 90, 100], [170, 80, 80], [150, 90, 110], [200, 120, 120], [120, 70, 90]];
    let brows: [[u8; 3]; 5] = [[70, 45, 40], [30, 25, 25], [120, 80, 50], [190, 170, 120], [150, 150, 160]];
    let pick = |k: u32, n: usize| ((r(k) * n as f32) as usize).min(n - 1);
    Look {
        skin: skins[pick(1, 8)],
        eye: eyes[pick(2, 8)],
        lip: lips[pick(3, 5)],
        brow: brows[pick(4, 5)],
        eye_w: 0.24 + r(5) * 0.14,
        eye_h: 0.14 + r(6) * 0.1,
        eye_gap: 0.42 + r(7) * 0.2,
        eye_y: (r(8) - 0.5) * 0.12,
        brow_thick: 0.035 + r(9) * 0.05,
        brow_y: 0.28 + r(10) * 0.14,
        brow_tilt: (r(11) - 0.4) * 0.3,
        mouth_w: 0.36 + r(12) * 0.2,
        mouth_y: -0.95 - r(13) * 0.2,
        mouth_thick: 0.06 + r(14) * 0.08,
        nose_y: -0.5 - r(15) * 0.15,
        nose_shade: 0.2 + r(16) * 0.3,
        blush: r(17) * 0.5,
        freckles: if r(18) > 0.7 { r(19) } else { 0.0 },
        tone: r(20) * 0.2,
        ..Look::default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A slab that faces +z with the eye line at its middle, laid out in the square.
    fn slab() -> AssetMesh {
        let verts = vec![[-0.2, -0.2, 0.0], [0.2, -0.2, 0.0], [0.2, 0.2, 0.0], [-0.2, 0.2, 0.0]];
        let uvs = vec![[0.0, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 1.0]];
        AssetMesh { verts, uvs, tris: vec![[0, 1, 2], [0, 2, 3]], texture: None, normal: None }
    }

    #[test]
    fn the_eyes_are_where_the_face_says_and_the_skin_is_everywhere_else() {
        let m = slab();
        let face = Face { centre: [0.0, 0.0, 0.0], scale: 0.1, front: 1.0 };
        let look = Look::default();
        let img = bake(&m, 256, &look, &face, 1);
        // the middle of the left eye (the row of the texture is the v of the texture coordinate): iris colour, near the eye gap
        let px = |x: f32, y: f32| img.get_pixel((x * 256.0) as u32, (y * 256.0) as u32).0;
        let eye_x = 0.5 + look.eye_gap * 0.1 / 0.4;
        let at = px(eye_x + 0.03, 0.5 - 0.01);
        assert!(at[1] > at[0] && at[1] > 90, "an iris of the eye colour: {at:?}");
        // far from the face the colour is the skin
        let far = px(0.05, 0.9);
        let d = (far[0] as i32 - look.skin[0] as i32).abs() + (far[1] as i32 - look.skin[1] as i32).abs();
        assert!(d < 60, "{far:?}");
        // the mouth is the colour of the lips
        let mouth = px(0.5, 0.5 + look.mouth_y * 0.1 / 0.4);
        assert!(mouth[0] > mouth[1] + 30, "{mouth:?}");
    }

    #[test]
    fn a_face_that_looks_the_other_way_gets_no_face_on_this_side() {
        let m = slab();
        let face = Face { centre: [0.0, 0.0, 0.0], scale: 0.1, front: -1.0 };
        let img = bake(&m, 128, &Look::default(), &face, 1);
        let p = img.get_pixel(64, 64).0;
        let skin = Look::default().skin;
        assert!((p[0] as i32 - skin[0] as i32).abs() < 30 && (p[1] as i32 - skin[1] as i32).abs() < 30, "{p:?}");
    }

    #[test]
    fn another_seed_is_another_person_and_the_same_seed_the_same() {
        assert_eq!(random_look(5), random_look(5));
        assert_ne!(random_look(5), random_look(6));
        let l = random_look(9);
        assert!(l.eye_w > 0.2 && l.eye_h > 0.1 && l.mouth_w > 0.3);
    }

    #[test]
    fn patterns_change_the_body_colour() {
        let face = Face { centre: [9.0, 9.0, 9.0], scale: 0.1, front: 1.0 };
        let plain = Look::default();
        let spots = Look { pattern: Pattern::Spots, pattern_strength: 1.0, ..Look::default() };
        let mut changed = 0;
        for i in 0..400 {
            let p = [i as f32 * 0.013, i as f32 * 0.021, 0.3];
            if colour_at(&plain, &face, p, [0.0, 0.0, 1.0], 0.5, 1) != colour_at(&spots, &face, p, [0.0, 0.0, 1.0], 0.5, 1) {
                changed += 1;
            }
        }
        assert!(changed > 10 && changed < 390, "{changed}");
    }
}
