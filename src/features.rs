// SPDX-License-Identifier: GPL-3.0-or-later
// The looks of a base as variable textures: skin colour, how much darker it is towards the feet, and a pattern over the body (spots, stripes, patches),
// baked by the place of every texel on the mesh. The face is not drawn: whoever makes the character draws it by hand with the painting tools. What is baked
// is only the first layer of the texture; everything else is painted by hand over it.

use rayon::prelude::*;

use crate::assetview::AssetMesh;

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
}

impl Default for Look {
    fn default() -> Self {
        Look { skin: [226, 178, 150], tone: 0.1, pattern: Pattern::None, pattern_color: [150, 100, 90], pattern_size: 1.0, pattern_strength: 0.6 }
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

/// The colour of one place of the surface: the place in space and the height of the mesh as a share (0 at the feet).
pub fn colour_at(look: &Look, p: [f32; 3], rel_y: f32, seed: u32) -> [u8; 3] {
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
    [c[0].clamp(0.0, 255.0) as u8, c[1].clamp(0.0, 255.0) as u8, c[2].clamp(0.0, 255.0) as u8]
}

/// The texture of the looks: every texel of every triangle gets its colour from the place on the mesh. The texels just outside the pieces get the colour of
/// the nearest piece, so nothing shows at the seams.
pub fn bake(mesh: &AssetMesh, size: u32, look: &Look, seed: u32) -> image::RgbaImage {
    let (lo, hi) = mesh.bounds();
    let h = (hi[1] - lo[1]).max(1e-4);
    let s = size as f32;
    let cells: Vec<(u32, u32, [u8; 3])> = mesh
        .tris
        .par_iter()
        .flat_map_iter(|t| {
            let (pa, pb, pc) = (mesh.verts[t[0] as usize], mesh.verts[t[1] as usize], mesh.verts[t[2] as usize]);
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
                    out.push((x, y, colour_at(look, q, (q[1] - lo[1]) / h, seed)));
                }
            }
            out.into_iter()
        })
        .collect();
    // the texture is the colour of the skin where nothing was drawn
    let mut img = image::RgbaImage::from_pixel(size, size, image::Rgba([look.skin[0], look.skin[1], look.skin[2], 255]));
    for (x, y, c) in cells {
        img.put_pixel(x, y, image::Rgba([c[0], c[1], c[2], 255]));
    }
    img
}

/// A look chosen by chance from the seed: another skin and tone, and the pattern colour that goes with it. The pattern itself stays what it was.
pub fn random_look(seed: u32, keep: &Look) -> Look {
    let r = |k: u32| hash(k as i32, 3, 9, seed);
    let skins: [[u8; 3]; 10] = [[240, 205, 185], [226, 178, 150], [200, 150, 115], [160, 110, 80], [110, 75, 55], [170, 190, 210], [150, 200, 160], [200, 150, 190], [120, 160, 200], [190, 120, 110]];
    let pick = |k: u32, n: usize| ((r(k) * n as f32) as usize).min(n - 1);
    let skin = skins[pick(1, skins.len())];
    let pattern_color = [(skin[0] as f32 * 0.6) as u8, (skin[1] as f32 * 0.55) as u8, (skin[2] as f32 * 0.6) as u8];
    Look { skin, tone: r(2) * 0.25, pattern_color, pattern_size: 0.6 + r(3) * 1.4, ..keep.clone() }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A slab laid out in the square.
    fn slab() -> AssetMesh {
        let verts = vec![[-0.2, 0.0, 0.0], [0.2, 0.0, 0.0], [0.2, 1.0, 0.0], [-0.2, 1.0, 0.0]];
        let uvs = vec![[0.0, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 1.0]];
        AssetMesh { verts, uvs, tris: vec![[0, 1, 2], [0, 2, 3]], texture: None, normal: None }
    }

    #[test]
    fn the_whole_surface_is_the_skin_and_no_face_is_drawn() {
        let look = Look { tone: 0.0, ..Look::default() };
        let img = bake(&slab(), 64, &look, 1);
        assert!(img.pixels().all(|p| p.0 == [look.skin[0], look.skin[1], look.skin[2], 255]));
    }

    #[test]
    fn the_skin_gets_darker_towards_the_feet() {
        let look = Look { tone: 1.0, ..Look::default() };
        let img = bake(&slab(), 64, &look, 1);
        // the feet are the low v of the texture (row 0), the head the high one
        assert!(img.get_pixel(32, 2).0[0] < img.get_pixel(32, 61).0[0]);
    }

    #[test]
    fn another_seed_is_another_skin_and_the_same_seed_the_same() {
        let keep = Look::default();
        assert_eq!(random_look(5, &keep), random_look(5, &keep));
        assert_ne!(random_look(5, &keep), random_look(6, &keep));
        assert_eq!(random_look(5, &Look { pattern: Pattern::Spots, ..keep.clone() }).pattern, Pattern::Spots, "the pattern stays");
    }

    #[test]
    fn patterns_change_the_body_colour() {
        let plain = Look::default();
        let spots = Look { pattern: Pattern::Spots, pattern_strength: 1.0, ..Look::default() };
        let mut changed = 0;
        for i in 0..400 {
            let p = [i as f32 * 0.013, i as f32 * 0.021, 0.3];
            if colour_at(&plain, p, 0.5, 1) != colour_at(&spots, p, 0.5, 1) {
                changed += 1;
            }
        }
        assert!(changed > 10 && changed < 390, "{changed}");
    }
}
