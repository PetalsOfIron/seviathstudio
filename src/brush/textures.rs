// SPDX-License-Identifier: GPL-3.0-or-later
// Brush textures: tileable 256×256 patterns. `a` = how much paint is taken away (0..1), the same
// way paper grain works; textures stay fixed to the canvas so patterns line up across strokes.
// Every texture also has natural colors, used when "Keep texture colors" is on.

use std::f32::consts::TAU;
use std::sync::OnceLock;

use super::TexKind;
use super::noise::{Rng, hash, pfbm, pnoise, smoothstep};
use super::raster::blur;

pub const TEX: usize = 256;

pub struct Texture {
    pub a: Vec<f32>,
    pub col: Vec<[f32; 3]>,
}

static CACHE: [OnceLock<Texture>; 24] = [const { OnceLock::new() }; 24];

pub fn get(kind: TexKind) -> &'static Texture {
    // "My texture" needs its picture: without it the brush uses plain paper
    let kind = if kind == TexKind::Custom { TexKind::Paper } else { kind };
    CACHE[kind as usize].get_or_init(|| make(kind))
}

/// The texture a brush uses: one of the built-in ones, or the picture of its own.
pub fn for_brush(kind: TexKind, image: &Option<super::tipimage::TipImage>) -> &'static Texture {
    match (kind, image) {
        (TexKind::Custom, Some(img)) => custom(img),
        _ => get(kind),
    }
}

/// A texture made from a picture (dark = paint stays, light = paint is taken away), repeating.
/// Made once per picture and kept: a few can exist at the same time.
fn custom(img: &super::tipimage::TipImage) -> &'static Texture {
    use std::collections::HashMap;
    use std::sync::Mutex;
    static MADE: OnceLock<Mutex<HashMap<u64, &'static Texture>>> = OnceLock::new();
    let made = MADE.get_or_init(|| Mutex::new(HashMap::new()));
    let mut m = made.lock().unwrap_or_else(|e| e.into_inner());
    if let Some(t) = m.get(&img.id) {
        return t;
    }
    if m.len() >= 48 {
        return get(TexKind::Paper); // more than this at once is not a thing: do not grow without end
    }
    let k = img.n as f32 / TEX as f32;
    let a: Vec<f32> = (0..TEX * TEX).map(|i| 1.0 - img.at(((i % TEX) as f32 + 0.5) * k, ((i / TEX) as f32 + 0.5) * k)).collect();
    let col = a.iter().map(|&t| [1.0 - t; 3]).collect();
    let tex: &'static Texture = Box::leak(Box::new(Texture { a, col }));
    m.insert(img.id, tex);
    tex
}

/// Bilinear, wrapping sample of the "paint taken away" value at document position (x, y).
#[inline]
pub fn sample_a(t: &Texture, x: f32, y: f32, scale: f32) -> f32 {
    let s = scale.max(0.1);
    let (u, v) = (x / s - 0.5, y / s - 0.5);
    let (x0, y0) = (u.floor(), v.floor());
    let (fx, fy) = (u - x0, v - y0);
    let w = |i: i32| i.rem_euclid(TEX as i32) as usize;
    let (xi, yi) = (x0 as i32, y0 as i32);
    let at = |x: i32, y: i32| t.a[w(y) * TEX + w(x)];
    let top = at(xi, yi) + (at(xi + 1, yi) - at(xi, yi)) * fx;
    let bot = at(xi, yi + 1) + (at(xi + 1, yi + 1) - at(xi, yi + 1)) * fx;
    top + (bot - top) * fy
}

#[inline]
pub fn sample_col(t: &Texture, x: f32, y: f32, scale: f32) -> [f32; 3] {
    let s = scale.max(0.1);
    let w = |v: f32| (v.floor() as i32).rem_euclid(TEX as i32) as usize;
    t.col[w(y / s) * TEX + w(x / s)]
}

/// Shades of `base` (0..1 rgb) for "colors from my brush": gaps darker, raised parts lighter.
pub fn brush_shade(base: [f32; 3], a: f32) -> [f32; 3] {
    let t = 1.0 - a;
    let dark = base.map(|c| c * 0.45);
    let light = base.map(|c| c + (1.0 - c) * 0.35);
    if t < 0.5 {
        let k = t / 0.5;
        [0, 1, 2].map(|i| dark[i] + (base[i] - dark[i]) * k)
    } else {
        let k = (t - 0.5) / 0.5;
        [0, 1, 2].map(|i| base[i] + (light[i] - base[i]) * k)
    }
}

fn rgb(hex: u32) -> [f32; 3] {
    [((hex >> 16) & 255) as f32 / 255.0, ((hex >> 8) & 255) as f32 / 255.0, (hex & 255) as f32 / 255.0]
}

/// Natural colors, from the gaps (dark) to the raised parts (light).
fn palette(kind: TexKind) -> [[f32; 3]; 3] {
    let p = |a, b, c| [rgb(a), rgb(b), rgb(c)];
    match kind {
        TexKind::Paper | TexKind::Watercolor | TexKind::Canvas => p(0xb8b0a2, 0xe2dccf, 0xf7f3ea),
        TexKind::Cloth => p(0x8c7b62, 0xc9b896, 0xe8dcc0),
        TexKind::Denim => p(0x1c2c4f, 0x3d5a8c, 0x8aa6cf),
        TexKind::Knit => p(0x9a8c78, 0xd9ccb6, 0xf3ece0),
        TexKind::Hair => p(0x2b1a12, 0x6b4430, 0xb88a5e),
        TexKind::Fur => p(0x4a3322, 0x9a6f48, 0xe0c39a),
        TexKind::Skin => p(0xc98f78, 0xedc0a8, 0xfbe0d0),
        TexKind::Leather => p(0x3b2214, 0x7a4a2c, 0xa8724a),
        TexKind::Scales => p(0x123a33, 0x2f8a72, 0x9fe0c4),
        TexKind::Wood => p(0x5a3519, 0x9a6438, 0xd2a06a),
        TexKind::Stone => p(0x55575c, 0x8d8f94, 0xc4c5c8),
        TexKind::Grunge => p(0x3a2414, 0x7e4a24, 0xb98a5c),
        TexKind::Crosshatch | TexKind::Dots => p(0x111111, 0x555555, 0xeeeeee),
        TexKind::Bark => p(0x23160d, 0x5a3c26, 0x8f6a4a),
        TexKind::Sand => p(0xa4835a, 0xd8b98a, 0xf2deb8),
        TexKind::Marble => p(0x6e6a68, 0xd8d4d0, 0xfaf8f5),
        TexKind::Water => p(0x0d3b66, 0x2a7fb8, 0xb8ecff),
        TexKind::Brick => p(0x8e8a84, 0x9c3b24, 0xc8603c),
        TexKind::Feathers => p(0x8a8f99, 0xd9dde4, 0xffffff),
        TexKind::SnakeSkin => p(0x0c2a1c, 0x2ebf71, 0xd6a84c),
        TexKind::Foliage => p(0x0f2e14, 0x2f7a2a, 0x8fd14f),
        TexKind::Custom => p(0x333333, 0x999999, 0xeeeeee),
    }
}

/// Anisotropic tiling noise: separate repeat periods for x and y.
fn pn2(x: f32, y: f32, px: i32, py: i32, seed: i32) -> f32 {
    let (xf, yf) = (x.floor(), y.floor());
    let (fx, fy) = (x - xf, y - yf);
    let (sx, sy) = (fx * fx * (3.0 - 2.0 * fx), fy * fy * (3.0 - 2.0 * fy));
    let (xi, yi) = (xf as i32, yf as i32);
    let h = |x: i32, y: i32| hash(x.rem_euclid(px), y.rem_euclid(py), seed);
    let (a, b, c, d) = (h(xi, yi), h(xi + 1, yi), h(xi, yi + 1), h(xi + 1, yi + 1));
    a + (b - a) * sx + (c - a) * sy + (a - b - c + d) * sx * sy
}

/// Distances to the nearest and second-nearest random point (tiling cells).
fn worley(u: f32, v: f32, cells: i32, seed: i32) -> (f32, f32) {
    let (cx, cy) = (u * cells as f32, v * cells as f32);
    let (mut d1, mut d2) = (9.0f32, 9.0f32);
    for j in -1..=1 {
        for i in -1..=1 {
            let (gx, gy) = (cx.floor() as i32 + i, cy.floor() as i32 + j);
            let (wx, wy) = (gx.rem_euclid(cells), gy.rem_euclid(cells));
            let (px, py) = (gx as f32 + hash(wx, wy, seed), gy as f32 + hash(wx, wy, seed + 1));
            let d = ((px - cx).powi(2) + (py - cy).powi(2)).sqrt();
            if d < d1 {
                d2 = d1;
                d1 = d;
            } else if d < d2 {
                d2 = d;
            }
        }
    }
    (d1, d2)
}

fn frac(x: f32) -> f32 {
    x - x.floor()
}

fn paper() -> Vec<f32> {
    // Coarse paper bumps + fine grain, blurred with wrap-around so the tile is seamless.
    let mut r = Rng::new(1234);
    let coarse: Vec<f32> = (0..64 * 64).map(|_| r.next().powi(3)).collect();
    let fine: Vec<f32> = (0..TEX * TEX).map(|_| r.next().powi(3)).collect();
    let mut tile = vec![0.0f32; TEX * TEX];
    for y in 0..TEX {
        for x in 0..TEX {
            let c = coarse[(y / 4) * 64 + x / 4] * 0.65;
            let f = fine[y * TEX + x] * 0.8;
            tile[y * TEX + x] = 1.0 - (1.0 - c) * (1.0 - f);
        }
    }
    let big_n = TEX * 3;
    let mut big = vec![0.0f32; big_n * big_n];
    for y in 0..big_n {
        for x in 0..big_n {
            big[y * big_n + x] = tile[(y % TEX) * TEX + x % TEX];
        }
    }
    blur(&mut big, big_n, big_n, 1.2);
    let mut out = vec![0.0; TEX * TEX];
    for y in 0..TEX {
        for x in 0..TEX {
            out[y * TEX + x] = big[(y + TEX) * big_n + x + TEX];
        }
    }
    out
}

fn make(kind: TexKind) -> Texture {
    let a: Vec<f32> = if kind == TexKind::Paper {
        paper()
    } else {
        let mut a = vec![0.0f32; TEX * TEX];
        for y in 0..TEX {
            for x in 0..TEX {
                let (u, v) = (x as f32 / TEX as f32, y as f32 / TEX as f32);
                a[y * TEX + x] = texel(kind, u, v).clamp(0.0, 1.0);
            }
        }
        a
    };
    let pal = palette(kind);
    let col = a
        .iter()
        .map(|&t| {
            let k = 1.0 - t;
            if k < 0.5 {
                let f = k / 0.5;
                [0, 1, 2].map(|i| pal[0][i] + (pal[1][i] - pal[0][i]) * f)
            } else {
                let f = (k - 0.5) / 0.5;
                [0, 1, 2].map(|i| pal[1][i] + (pal[2][i] - pal[1][i]) * f)
            }
        })
        .collect();
    Texture { a, col }
}

fn texel(kind: TexKind, u: f32, v: f32) -> f32 {
    match kind {
        TexKind::Paper | TexKind::Custom => 0.0,
        TexKind::Watercolor => smoothstep(0.42, 0.75, pfbm(u, v, 8, 3, 4)) * 0.9,
        TexKind::Canvas => {
            let t1 = 0.5 + 0.5 * (u * TAU * 32.0).sin();
            let t2 = 0.5 + 0.5 * (v * TAU * 32.0).sin();
            let over = if ((u * 32.0).floor() as i32 + (v * 32.0).floor() as i32) % 2 != 0 { t1 } else { t2 };
            (1.0 - over) * 0.75 + (pfbm(u, v, 16, 5, 3) - 0.5) * 0.3
        }
        TexKind::Cloth => {
            let thx = 0.5 + 0.5 * (u * TAU * 48.0 + pfbm(u, v, 4, 6, 3) * 3.0).sin();
            let thy = 0.5 + 0.5 * (v * TAU * 48.0 + pfbm(u, v, 4, 7, 3) * 3.0).sin();
            (1.0 - thx.max(thy)) * 0.8 + pnoise(u * 64.0, v * 64.0, 64, 8) * 0.2
        }
        TexKind::Denim => {
            let d = 0.5 + 0.5 * ((u + v) * TAU * 40.0 + pfbm(u, v, 8, 9, 3) * 2.0).sin();
            (1.0 - d) * 0.7 + smoothstep(0.6, 0.9, pn2(u * 128.0, v * 16.0, 128, 16, 10)) * 0.35
        }
        TexKind::Knit => {
            let (row, col) = (v * 24.0, u * 24.0);
            let (fy, fx) = (frac(row), frac(col));
            let vshape = (fx - 0.5).abs() * 2.0 - (1.0 - fy);
            smoothstep(-0.15, 0.25, vshape.abs() - 0.3) * 0.8 + pnoise(u * 48.0, v * 48.0, 48, 11) * 0.15
        }
        TexKind::Hair => {
            let n = pn2(u * 4.0, v * 96.0 + (u * TAU * 2.0).sin() * 2.0, 4, 96, 12) * 0.65 + pn2(u * 8.0, v * 180.0, 8, 180, 13) * 0.35;
            smoothstep(0.42, 0.62, n)
        }
        TexKind::Fur => {
            let n = pn2(u * 16.0, v * 64.0, 16, 64, 14) * 0.6 + pn2(u * 32.0, v * 128.0, 32, 128, 15) * 0.4;
            smoothstep(0.38, 0.62, n) * 0.9 + pfbm(u, v, 4, 16, 3) * 0.1
        }
        TexKind::Skin => {
            let mott = pfbm(u, v, 6, 17, 3);
            let pore = smoothstep(0.78, 0.95, pnoise(u * 90.0, v * 90.0, 90, 18));
            (mott - 0.4) * 0.35 + pore * 0.6
        }
        TexKind::Leather => {
            let (d1, d2) = worley(u, v, 14, 19);
            (1.0 - smoothstep(0.02, 0.12, d2 - d1)) * 0.85 + pnoise(u * 80.0, v * 80.0, 80, 21) * 0.2
        }
        TexKind::Scales => {
            let (x, y) = (u * 6.0, v * 6.0);
            let r0 = (y / 0.6).floor() as i32;
            let (mut best, mut bd) = (i32::MIN, 9.0f32);
            for r in r0 - 1..=r0 + 1 {
                let off = if r.rem_euclid(2) == 1 { 0.5 } else { 0.0 };
                let cy = r as f32 * 0.6;
                let base = (x - off).floor() + off + 0.5;
                for cx in [base - 1.0, base, base + 1.0] {
                    let dd = ((x - cx).powi(2) + ((y - cy) * 1.05).powi(2)).sqrt() / 0.56;
                    if dd < 1.0 && (best == i32::MIN || r < best) {
                        best = r;
                        bd = dd;
                    }
                }
            }
            if best == i32::MIN {
                1.0
            } else {
                let gap = smoothstep(0.84, 0.9, bd);
                gap + (1.0 - gap) * smoothstep(0.35, 0.84, bd) * 0.35
            }
        }
        TexKind::Wood => {
            let warp = pfbm(u, v, 3, 22, 3) * 3.0;
            let ring = 0.5 + 0.5 * ((v * 18.0 + warp) * TAU).sin();
            smoothstep(0.55, 1.0, ring) * 0.7 + pn2(u * 160.0, v * 8.0, 160, 8, 23) * 0.3
        }
        TexKind::Stone => {
            let n = pfbm(u, v, 10, 24, 4);
            smoothstep(0.35, 0.8, n) * 0.6 + smoothstep(0.82, 0.92, pnoise(u * 40.0, v * 40.0, 40, 25)) * 0.5
        }
        TexKind::Grunge => {
            let n = pfbm(u, v, 6, 26, 4);
            smoothstep(0.45, 0.62, n) * 0.85 + smoothstep(0.92, 0.97, pn2(u * 2.0, v * 120.0, 2, 120, 27)) * 0.8
        }
        TexKind::Crosshatch => {
            let l1 = 0.5 + 0.5 * ((u + v) * TAU * 20.0).sin();
            let l2 = 0.5 + 0.5 * ((u - v) * TAU * 20.0).sin();
            smoothstep(0.15, 0.4, l1.min(l2)) * 0.85
        }
        TexKind::Dots => {
            let (fx, fy) = (frac(u * 32.0) - 0.5, frac(v * 32.0) - 0.5);
            smoothstep(0.25, 0.32, (fx * fx + fy * fy).sqrt()) * 0.9
        }
        TexKind::Bark => {
            // vertical ridges split by deep cracks
            let ridge = pn2(u * 20.0 + pfbm(u, v, 4, 31, 3) * 3.0, v * 3.0, 20, 3, 32);
            let crack = smoothstep(0.62, 0.8, ridge);
            let fiber = pn2(u * 96.0, v * 12.0, 96, 12, 33);
            crack * 0.85 + fiber * 0.25
        }
        TexKind::Sand => {
            let fine = pnoise(u * 128.0, v * 128.0, 128, 34);
            let speck = smoothstep(0.86, 0.95, pnoise(u * 64.0, v * 64.0, 64, 35));
            let dune = 0.5 + 0.5 * ((v * 6.0 + pfbm(u, v, 3, 36, 3) * 2.0) * TAU).sin();
            fine * 0.4 + speck * 0.5 + dune * 0.15
        }
        TexKind::Marble => {
            let s = ((u + v * 0.4) * TAU * 2.0 + pfbm(u, v, 5, 37, 4) * 7.0).sin().abs();
            let vein = 1.0 - smoothstep(0.0, 0.08, s);
            vein * 0.9 + pfbm(u, v, 8, 38, 3) * 0.12
        }
        TexKind::Water => {
            let w1 = ((u * 5.0 + pfbm(u, v, 4, 39, 3) * 2.0) * TAU).sin();
            let w2 = ((v * 7.0 + pfbm(u, v, 4, 40, 3) * 2.0) * TAU).sin();
            let caustic = 1.0 - (w1 * w2).abs();
            smoothstep(0.55, 0.95, caustic) * 0.75
        }
        TexKind::Brick => {
            let rows = 8.0;
            let ry = v * rows;
            let row = ry.floor() as i32;
            let off = if row % 2 == 0 { 0.0 } else { 0.5 };
            let bx = frac(u * 4.0 + off);
            let by = frac(ry);
            // mortar lines: thin vertical joints between bricks, horizontal joints between rows
            let joint_x = 1.0 - smoothstep(0.015, 0.035, bx.min(1.0 - bx));
            let joint_y = 1.0 - smoothstep(0.04, 0.08, by.min(1.0 - by));
            let mortar = joint_x.max(joint_y);
            mortar + (1.0 - mortar) * pfbm(u, v, 16, 41, 3) * 0.25
        }
        TexKind::Feathers => {
            // Rows of overlapping feathers lying along the stroke (x): an oval vane around a
            // shaft, with barbs slanting back from the shaft. Odd rows are shifted half a feather.
            let row = (v * 5.0).floor() as i32;
            let shift = if row % 2 == 0 { 0.0 } else { 0.5 };
            let fx = frac(u * 3.0 + shift); // 0 = quill end … 1 = tip
            let fy = frac(v * 5.0) - 0.5; // across the feather
            let half = 0.46 * (std::f32::consts::PI * fx).sin().powf(0.55);
            let outside = smoothstep(half - 0.03, half + 0.01, fy.abs());
            let shaft = 1.0 - smoothstep(0.012, 0.03, fy.abs());
            let barb = frac(fx * 22.0 - fy.abs() * 9.0);
            let barbs = smoothstep(0.55, 0.9, barb) * 0.55;
            let body = (barbs + fy.abs() / half.max(0.05) * 0.25 - shaft * 0.5).clamp(0.0, 1.0);
            outside.max(body * (1.0 - outside))
        }
        TexKind::SnakeSkin => {
            // Diamond scales: a square cell in the 45°-turned grid is a diamond on the canvas.
            // Each scale is darker toward its rim, light along a center ridge, with a thin dark gap.
            let (x, y) = ((u + v) * 7.0, (v - u) * 7.0);
            let (fx, fy) = (frac(x) - 0.5, frac(y) - 0.5);
            let d = fx.abs().max(fy.abs()) * 2.0; // 0 at the center, 1 at the rim
            let gap = smoothstep(0.86, 0.96, d);
            let ridge = 1.0 - smoothstep(0.0, 0.12, (fx + fy).abs());
            let shade = smoothstep(0.2, 0.9, d) * 0.55 - ridge * (1.0 - d) * 0.3;
            gap + (1.0 - gap) * shade.max(0.0)
        }
        TexKind::Foliage => {
            let (d1, d2) = worley(u, v, 12, 42);
            let leaf_gap = smoothstep(0.03, 0.0, d2 - d1).max(smoothstep(0.42, 0.55, d1));
            leaf_gap * 0.85 + d1 * 0.3
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// All textures in their natural colors: target/screens/texture_sheet.png (6 × 4, 200 px each).
    #[test]
    fn texture_sheet() {
        let (cell, cols) = (200u32, 6usize);
        let rows = TexKind::ALL.len().div_ceil(cols) as u32;
        let mut img = image::RgbaImage::new(cell * cols as u32, cell * rows);
        for (i, k) in TexKind::ALL.iter().enumerate() {
            let t = get(*k);
            let (ox, oy) = ((i % cols) as u32 * cell, (i / cols) as u32 * cell);
            for y in 0..cell {
                for x in 0..cell {
                    let c = sample_col(t, x as f32, y as f32, 1.0);
                    let q = |v: f32| (v * 255.0).round().clamp(0.0, 255.0) as u8;
                    img.put_pixel(ox + x, oy + y, image::Rgba([q(c[0]), q(c[1]), q(c[2]), 255]));
                }
            }
        }
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("target").join("screens");
        std::fs::create_dir_all(&dir).unwrap();
        img.save(dir.join("texture_sheet.png")).unwrap();
    }

    #[test]
    fn every_texture_builds_with_contrast() {
        for k in TexKind::ALL {
            let t = get(k);
            assert_eq!(t.a.len(), TEX * TEX);
            let (lo, hi) = t.a.iter().fold((1.0f32, 0.0f32), |(l, h), &v| (l.min(v), h.max(v)));
            assert!(hi - lo > 0.2, "{k:?} has too little contrast: {lo}..{hi}");
            assert!(t.a.iter().all(|v| v.is_finite() && (0.0..=1.0).contains(v)), "{k:?}");
        }
    }
}
