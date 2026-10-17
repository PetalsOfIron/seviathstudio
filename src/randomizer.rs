// SPDX-License-Identifier: GPL-3.0-or-later
// Color randomizer (plan §W1): believable colors, not rainbow noise. Palettes come from how real
// things distribute value and chroma (skin gets warm shadows, foliage yellow-green lights, metals
// stay low in chroma…), in OKLCh so mixes stay clean, and respect harmony rules. The same seed
// always gives the same palette. Applying: flat, gradient, speckle, or recolor the flats of a
// drawing while keeping the lineart and the shading.

use eframe::egui::Color32;
use rayon::prelude::*;

use crate::blend;
use crate::doc::{Layer, TILE, TileKey};
use crate::gradient::{Gradient, Repeat, Shape, Stop};
use crate::oklab::{lab_to_lch, lch_to_rgb_fit, rgb8_to_oklab, rgb_to_u8};
use crate::select::Selection;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Intent {
    Anything,
    Skin,
    Foliage,
    Sky,
    Sunset,
    Water,
    Leather,
    Metal,
    Stone,
    Hair,
    Fabric,
}

struct Profile {
    /// hue range in OKLCh degrees
    hue: (f32, f32),
    /// how many degrees the hue turns in the darkest swatches (shadows get warmer, cooler…)
    dark_shift: f32,
    /// lightness range
    l: (f32, f32),
    /// the most chroma (reached in the middle values)
    chroma: f32,
}

impl Intent {
    pub const ALL: [(Intent, &'static str, &'static str); 11] = [
        (Intent::Anything, "Anything", "Any hue, balanced values."),
        (Intent::Skin, "Skin", "Warm tones; shadows turn redder, highlights stay soft."),
        (Intent::Foliage, "Foliage", "Greens; shadows go blue-green, lights yellow-green."),
        (Intent::Sky, "Sky", "Blues, paler and warmer near the horizon."),
        (Intent::Sunset, "Sunset", "Purples to oranges to gold."),
        (Intent::Water, "Water", "Cool teals and blues, darker with depth."),
        (Intent::Leather, "Leather", "Browns with low, warm chroma."),
        (Intent::Metal, "Metal", "Low chroma, a wide value range."),
        (Intent::Stone, "Stone", "Muted grays with a hint of warmth."),
        (Intent::Hair, "Hair", "Natural hair colors, from dark roots to bright tips."),
        (Intent::Fabric, "Fabric", "Saturated dyed colors."),
    ];

    fn profile(self) -> Profile {
        let (hue, dark_shift, l, chroma) = match self {
            Intent::Anything => ((0.0, 360.0), 0.0, (0.3, 0.92), 0.15),
            Intent::Skin => ((45.0, 70.0), -18.0, (0.45, 0.9), 0.095),
            Intent::Foliage => ((120.0, 145.0), 22.0, (0.3, 0.85), 0.15),
            Intent::Sky => ((235.0, 255.0), 10.0, (0.55, 0.92), 0.13),
            Intent::Sunset => ((330.0, 70.0), -25.0, (0.3, 0.9), 0.17),
            Intent::Water => ((195.0, 235.0), 15.0, (0.3, 0.8), 0.12),
            Intent::Leather => ((50.0, 65.0), -8.0, (0.28, 0.65), 0.07),
            Intent::Metal => ((240.0, 280.0), 5.0, (0.25, 0.95), 0.025),
            Intent::Stone => ((60.0, 90.0), 0.0, (0.35, 0.8), 0.025),
            Intent::Hair => ((40.0, 70.0), -12.0, (0.2, 0.85), 0.08),
            Intent::Fabric => ((0.0, 360.0), 6.0, (0.35, 0.8), 0.17),
        };
        Profile { hue, dark_shift, l, chroma }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Harmony {
    Free,
    Analogous,
    Complementary,
    Triadic,
    Split,
}

impl Harmony {
    pub const ALL: [(Harmony, &'static str, &'static str); 5] = [
        (Harmony::Free, "Natural", "The look of the material, nothing more."),
        (Harmony::Analogous, "Analogous", "Neighbors on the color wheel: calm."),
        (Harmony::Complementary, "Complementary", "Two opposite hues: strong contrast."),
        (Harmony::Triadic, "Triadic", "Three hues spread evenly."),
        (Harmony::Split, "Split", "One hue and the two beside its opposite."),
    ];

    /// Degrees away from the base hue for swatch `i` of `n`.
    fn offset(self, i: usize, n: usize) -> f32 {
        match self {
            Harmony::Free => 0.0,
            Harmony::Analogous => {
                let t = if n > 1 { i as f32 / (n - 1) as f32 } else { 0.5 };
                -30.0 + 60.0 * t
            }
            Harmony::Complementary => {
                if i % 2 == 0 { 0.0 } else { 180.0 }
            }
            Harmony::Triadic => (i % 3) as f32 * 120.0,
            Harmony::Split => [0.0, 150.0, 210.0][i % 3],
        }
    }
}

/// A small seeded random number generator (xorshift).
pub struct Rng(u64);

impl Rng {
    pub fn new(seed: u32) -> Rng {
        let mut r = Rng(0x9E37_79B9_7F4A_7C15 ^ ((seed as u64) << 17) ^ seed as u64);
        for _ in 0..4 {
            r.next();
        }
        r
    }
    pub fn next(&mut self) -> f32 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        ((self.0 >> 40) as f32) / (1u64 << 24) as f32
    }
    pub fn range(&mut self, lo: f32, hi: f32) -> f32 {
        lo + (hi - lo) * self.next()
    }
}

fn lerp_hue(range: (f32, f32), t: f32) -> f32 {
    // a range may wrap around 0 (330 to 70)
    let (a, mut b) = range;
    if b < a {
        b += 360.0;
    }
    (a + (b - a) * t).rem_euclid(360.0)
}

/// `count` swatches (2..=8). A `Some` in `locked` stays exactly as it is.
pub fn generate(intent: Intent, harmony: Harmony, seed: u32, count: usize, locked: &[Option<[u8; 3]>]) -> Vec<[u8; 3]> {
    let count = count.clamp(2, 8);
    let p = intent.profile();
    let mut rng = Rng::new(seed);
    let base = lerp_hue(p.hue, rng.next());
    (0..count)
        .map(|i| {
            // draw the random numbers for every slot, so a locked swatch doesn't change the others
            let (j_l, j_h, j_c) = (rng.range(-0.03, 0.03), rng.range(-6.0, 6.0), rng.range(0.78, 1.0));
            if let Some(Some(c)) = locked.get(i) {
                return *c;
            }
            // values run dark to light along the palette
            let t = i as f32 / (count - 1) as f32;
            let l = (p.l.0 + (p.l.1 - p.l.0) * t + j_l).clamp(0.05, 0.97);
            // chroma peaks in the middle values and fades toward black and white
            let bell = 1.0 - (2.0 * t - 1.0).powi(2) * 0.55;
            let c = p.chroma * bell * j_c;
            let h = (base + harmony.offset(i, count) + p.dark_shift * (1.0 - t) + j_h).rem_euclid(360.0);
            rgb_to_u8(lch_to_rgb_fit([l, c, h]))
        })
        .collect()
}

/// The palette as an evenly spaced gradient, turned by `angle` degrees (0 = top to bottom).
pub fn palette_gradient(colors: &[[u8; 3]]) -> Gradient {
    let n = colors.len().max(2);
    let stops = colors.iter().enumerate().map(|(i, c)| Stop { pos: i as f32 / (n - 1) as f32, color: *c, alpha: 1.0 }).collect();
    Gradient { stops, shape: Shape::Linear, repeat: Repeat::None, smooth: true, dither: true, opacity: 1.0, blend: crate::doc::Blend::Normal }
}

// ---------- Applying ----------

fn smooth(t: f32) -> f32 {
    t * t * (3.0 - 2.0 * t)
}

fn hash(x: i32, y: i32, seed: u32) -> f32 {
    let mut h = (x as u32).wrapping_mul(0x9E37_79B1) ^ (y as u32).wrapping_mul(0x85EB_CA77) ^ seed.wrapping_mul(0xC2B2_AE3D);
    h ^= h >> 15;
    h = h.wrapping_mul(0x2C1B_3C6D);
    h ^= h >> 12;
    h = h.wrapping_mul(0x297A_2D39);
    h ^= h >> 15;
    (h & 0xFFFF) as f32 / 65535.0
}

/// Smooth value noise in 0..1 with features about `cell` pixels wide.
fn noise(x: f32, y: f32, cell: f32, seed: u32) -> f32 {
    let (fx, fy) = (x / cell, y / cell);
    let (ix, iy) = (fx.floor(), fy.floor());
    let (tx, ty) = (smooth(fx - ix), smooth(fy - iy));
    let (ix, iy) = (ix as i32, iy as i32);
    let a = hash(ix, iy, seed) * (1.0 - tx) + hash(ix + 1, iy, seed) * tx;
    let b = hash(ix, iy + 1, seed) * (1.0 - tx) + hash(ix + 1, iy + 1, seed) * tx;
    a * (1.0 - ty) + b * ty
}

fn commit(layer: &mut Layer, results: Vec<(TileKey, Vec<Color32>)>) -> Vec<TileKey> {
    let mut out = Vec::with_capacity(results.len());
    for (key, px) in results {
        if px.iter().all(|c| c.a() == 0) {
            layer.tiles.remove(&key);
        } else {
            *layer.tile_mut(key) = px;
        }
        out.push(key);
    }
    if !out.is_empty() {
        layer.rev += 1;
    }
    out
}

fn tile_keys(bounds: (i32, i32, i32, i32)) -> Vec<TileKey> {
    let t = TILE as i32;
    let (x0, y0, x1, y1) = bounds;
    if x1 <= x0 || y1 <= y0 {
        return Vec::new();
    }
    (y0.div_euclid(t)..=(y1 - 1).div_euclid(t)).flat_map(|ty| (x0.div_euclid(t)..=(x1 - 1).div_euclid(t)).map(move |tx| (tx, ty))).collect()
}

/// Speckle: the palette scattered as soft patches with a fine grain, like real pigment.
/// `cell` is the size of the patches in pixels. Paints over the selection (or `bounds`).
pub fn speckle(layer: &mut Layer, colors: &[[u8; 3]], sel: Option<&Selection>, bounds: (i32, i32, i32, i32), cell: f32, seed: u32) -> Vec<TileKey> {
    let lut = palette_gradient(colors).lut();
    let t = TILE as i32;
    let lock_alpha = layer.lock_alpha;
    let results: Vec<(TileKey, Vec<Color32>)> = tile_keys(bounds)
        .par_iter()
        .filter_map(|&key| {
            let mask = match sel {
                Some(s) => Some(s.tile(key)?),
                None => None,
            };
            let old = layer.tiles.get(&key);
            if lock_alpha && old.is_none() {
                return None;
            }
            let mut px = old.map(|o| (**o).clone()).unwrap_or_else(|| vec![Color32::TRANSPARENT; TILE * TILE]);
            let mut changed = false;
            for j in 0..t {
                let y = key.1 * t + j;
                if y < bounds.1 || y >= bounds.3 {
                    continue;
                }
                for i in 0..t {
                    let x = key.0 * t + i;
                    if x < bounds.0 || x >= bounds.2 {
                        continue;
                    }
                    let k = (j * t + i) as usize;
                    let cov = mask.map(|m| m[k] as f32 / 255.0).unwrap_or(1.0);
                    if cov <= 0.0 {
                        continue;
                    }
                    // big soft patches, a middle size and a fine grain
                    let n = noise(x as f32, y as f32, cell.max(2.0), seed) * 0.62
                        + noise(x as f32, y as f32, (cell * 0.35).max(1.5), seed ^ 0x55) * 0.26
                        + hash(x, y, seed ^ 0xAA) * 0.12;
                    let c = lut.at(n);
                    let a = cov;
                    let src = [c[0].clamp(0.0, 1.0) * a, c[1].clamp(0.0, 1.0) * a, c[2].clamp(0.0, 1.0) * a, a];
                    let dst = blend::to_px(px[k]);
                    let out = if lock_alpha { blend::atop(dst, src, 1.0, crate::doc::Blend::Normal) } else { blend::over(dst, src, 1.0, crate::doc::Blend::Normal) };
                    px[k] = blend::from_px(out);
                    changed = true;
                }
            }
            changed.then_some((key, px))
        })
        .collect();
    commit(layer, results)
}

/// Biggest area (pixels) `recolor_flats` works on at once.
pub const RECOLOR_MAX_PIXELS: usize = 40_000_000;

fn find(parent: &mut [u32], mut x: u32) -> u32 {
    while parent[x as usize] != x {
        parent[x as usize] = parent[parent[x as usize] as usize];
        x = parent[x as usize];
    }
    x
}

/// Recolor the flat areas of a drawing: lineart (dark pixels) is left alone; each connected area
/// of similar colors takes one palette color, and keeps its shading (lighter and darker parts stay
/// lighter and darker). Returns the changed tiles, or an error for areas that are too big.
pub fn recolor_flats(layer: &mut Layer, colors: &[[u8; 3]], sel: Option<&Selection>, bounds: (i32, i32, i32, i32), seed: u32) -> Result<Vec<TileKey>, String> {
    let (x0, y0, x1, y1) = bounds;
    let (w, h) = ((x1 - x0).max(0) as usize, (y1 - y0).max(0) as usize);
    if w == 0 || h == 0 || colors.is_empty() {
        return Ok(Vec::new());
    }
    if w * h > RECOLOR_MAX_PIXELS {
        return Err("That area is too big to recolor at once. Select a smaller part.".into());
    }
    let img = crate::files::layer_rgba(layer, x0, y0, w as u32, h as u32);
    let raw = img.as_raw();
    let lab_of = |i: usize| rgb8_to_oklab([raw[i * 4], raw[i * 4 + 1], raw[i * 4 + 2]]);
    // flat candidates: visible and not lineart
    let cand: Vec<bool> = (0..w * h).into_par_iter().map(|i| raw[i * 4 + 3] >= 24 && lab_of(i)[0] > 0.3).collect();
    let mut parent: Vec<u32> = (0..(w * h) as u32).collect();
    let near = |a: usize, b: usize| (0..3).all(|k| (raw[a * 4 + k] as i32 - raw[b * 4 + k] as i32).abs() <= 26);
    for y in 0..h {
        for x in 0..w {
            let i = y * w + x;
            if !cand[i] {
                continue;
            }
            for j in [if x + 1 < w { Some(i + 1) } else { None }, if y + 1 < h { Some(i + w) } else { None }].into_iter().flatten() {
                if cand[j] && near(i, j) {
                    let (ra, rb) = (find(&mut parent, i as u32), find(&mut parent, j as u32));
                    if ra != rb {
                        parent[ra as usize] = rb;
                    }
                }
            }
        }
    }
    // area and average lightness of every region
    let mut area = vec![0u32; w * h];
    let mut sum_l = vec![0f32; w * h];
    let mut root_of = vec![u32::MAX; w * h];
    for i in 0..w * h {
        if cand[i] {
            let r = find(&mut parent, i as u32);
            root_of[i] = r;
            area[r as usize] += 1;
            sum_l[r as usize] += lab_of(i)[0];
        }
    }
    let swatches: Vec<[f32; 3]> = colors.iter().map(|c| lab_to_lch(rgb8_to_oklab(*c))).collect();
    // biggest regions first, handing out the swatches in a shuffled order so neighbors differ
    let mut roots: Vec<u32> = (0..(w * h) as u32).filter(|&i| cand[i as usize] && root_of[i as usize] == i && area[i as usize] >= 40).collect();
    roots.sort_by_key(|&r| (std::cmp::Reverse(area[r as usize]), r));
    let mut order: Vec<usize> = (0..swatches.len()).collect();
    let mut rng = Rng::new(seed);
    for i in (1..order.len()).rev() {
        order.swap(i, (rng.next() * (i + 1) as f32) as usize % (i + 1));
    }
    let mut choice: std::collections::HashMap<u32, usize> = std::collections::HashMap::new();
    for (n, r) in roots.iter().enumerate() {
        choice.insert(*r, order[n % order.len()]);
    }
    let t = TILE as i32;
    let results: Vec<(TileKey, Vec<Color32>)> = tile_keys(bounds)
        .par_iter()
        .filter_map(|&key| {
            let mask = match sel {
                Some(s) => Some(s.tile(key)?),
                None => None,
            };
            let old = layer.tiles.get(&key)?;
            let mut px = (**old).clone();
            let mut changed = false;
            for j in 0..t {
                let y = key.1 * t + j;
                if y < y0 || y >= y1 {
                    continue;
                }
                for i in 0..t {
                    let x = key.0 * t + i;
                    if x < x0 || x >= x1 {
                        continue;
                    }
                    let idx = (y - y0) as usize * w + (x - x0) as usize;
                    let r = root_of[idx];
                    if r == u32::MAX || area[r as usize] < 40 {
                        continue;
                    }
                    let k = (j * t + i) as usize;
                    let cov = mask.map(|m| m[k] as f32 / 255.0).unwrap_or(1.0);
                    if cov <= 0.0 {
                        continue;
                    }
                    let dst = blend::to_px(px[k]);
                    if dst[3] <= 0.0 {
                        continue;
                    }
                    let sw = swatches[*choice.get(&r).unwrap_or(&0)];
                    let lab = lab_of(idx);
                    let mean_l = sum_l[r as usize] / area[r as usize] as f32;
                    // shading stays: this pixel's lightness relative to its region carries over
                    let l = (lab[0] + (sw[0] - mean_l)).clamp(0.04, 0.98);
                    let rgb = lch_to_rgb_fit([l, sw[1], sw[2]]);
                    let new = [rgb[0] * dst[3], rgb[1] * dst[3], rgb[2] * dst[3], dst[3]];
                    px[k] = blend::from_px(blend::lerp(dst, new, cov));
                    changed = true;
                }
            }
            changed.then_some((key, px))
        })
        .collect();
    Ok(commit(layer, results))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::doc::Document;
    use crate::oklab::lab_to_lch;

    fn hue_of(c: [u8; 3]) -> f32 {
        lab_to_lch(rgb8_to_oklab(c))[2]
    }

    fn angle_gap(a: f32, b: f32) -> f32 {
        let d = (a - b).rem_euclid(360.0);
        d.min(360.0 - d)
    }

    #[test]
    fn same_seed_same_palette_and_locks_hold() {
        let a = generate(Intent::Skin, Harmony::Free, 7, 5, &[]);
        assert_eq!(a, generate(Intent::Skin, Harmony::Free, 7, 5, &[]));
        assert_ne!(a, generate(Intent::Skin, Harmony::Free, 8, 5, &[]));
        let locked = [None, Some([1, 2, 3]), None, None, Some([9, 9, 9])];
        let b = generate(Intent::Skin, Harmony::Free, 8, 5, &locked);
        assert_eq!((b[1], b[4]), ([1, 2, 3], [9, 9, 9]));
        let plain = generate(Intent::Skin, Harmony::Free, 8, 5, &[]);
        assert_eq!((b[0], b[2], b[3]), (plain[0], plain[2], plain[3]), "a lock does not shuffle the others");
    }

    #[test]
    fn skin_is_warm_and_values_run_dark_to_light() {
        for seed in 0..20 {
            let p = generate(Intent::Skin, Harmony::Free, seed, 5, &[]);
            for c in &p {
                let h = hue_of(*c);
                assert!((15.0..95.0).contains(&h), "skin hue {h} for {c:?}");
            }
            let l: Vec<f32> = p.iter().map(|c| rgb8_to_oklab(*c)[0]).collect();
            assert!(l.windows(2).all(|w| w[1] > w[0] - 0.08), "{l:?}");
            assert!(l[4] - l[0] > 0.25);
        }
    }

    #[test]
    fn harmony_shapes_hues() {
        let p = generate(Intent::Fabric, Harmony::Complementary, 3, 4, &[]);
        assert!(angle_gap(hue_of(p[0]), hue_of(p[1])) > 140.0, "{:?}", p);
        assert!(angle_gap(hue_of(p[0]), hue_of(p[2])) < 40.0);
        let t = generate(Intent::Fabric, Harmony::Triadic, 3, 3, &[]);
        assert!(angle_gap(hue_of(t[0]), hue_of(t[1])) > 80.0 && angle_gap(hue_of(t[1]), hue_of(t[2])) > 80.0);
        let a = generate(Intent::Fabric, Harmony::Analogous, 3, 5, &[]);
        assert!(angle_gap(hue_of(a[0]), hue_of(a[4])) < 90.0);
    }

    #[test]
    fn metal_is_low_chroma() {
        for c in generate(Intent::Metal, Harmony::Free, 1, 6, &[]) {
            assert!(lab_to_lch(rgb8_to_oklab(c))[1] < 0.06, "{c:?}");
        }
    }

    #[test]
    fn speckle_stays_in_the_selection() {
        let mut d = Document::new(300, 300);
        let sel = Selection::polygon(&[eframe::egui::pos2(0.0, 0.0), eframe::egui::pos2(100.0, 0.0), eframe::egui::pos2(100.0, 300.0), eframe::egui::pos2(0.0, 300.0)], 300, 300, false);
        let colors = generate(Intent::Foliage, Harmony::Free, 2, 5, &[]);
        speckle(&mut d.layers[0], &colors, sel.as_ref(), (0, 0, 300, 300), 14.0, 5);
        let img = crate::files::layer_rgba(&d.layers[0], 0, 0, 300, 300);
        assert_eq!(img.get_pixel(50, 50).0[3], 255);
        assert_eq!(img.get_pixel(200, 50).0[3], 0);
        let distinct: std::collections::HashSet<[u8; 4]> = (0..100u32).flat_map(|x| (0..100u32).map(move |y| (x, y))).map(|(x, y)| img.get_pixel(x, y).0).collect();
        assert!(distinct.len() > 40, "a patchy texture, not one flat color: {}", distinct.len());
    }

    #[test]
    fn recolor_keeps_lineart_and_shading() {
        let mut d = Document::new(200, 100);
        // two flats separated by a black line, left one with a lighter right half (shading)
        let img = image::RgbaImage::from_fn(200, 100, |x, _| {
            if (96..104).contains(&x) {
                image::Rgba([10, 10, 10, 255])
            } else if x < 48 {
                image::Rgba([220, 150, 130, 255])
            } else if x < 96 {
                image::Rgba([215, 145, 125, 255])
            } else {
                image::Rgba([120, 160, 220, 255])
            }
        });
        d.layers[0].tiles = crate::files::tiles_from_rgba(&img, 0, 0, 200, 100);
        let colors = [[40, 160, 80], [200, 60, 60]];
        let keys = recolor_flats(&mut d.layers[0], &colors, None, (0, 0, 200, 100), 3).unwrap();
        assert!(!keys.is_empty());
        let out = crate::files::layer_rgba(&d.layers[0], 0, 0, 200, 100);
        assert_eq!(out.get_pixel(99, 50).0, [10, 10, 10, 255], "lineart untouched");
        let (a, b) = (out.get_pixel(10, 50).0, out.get_pixel(150, 50).0);
        assert_ne!(a, [220, 150, 130, 255]);
        assert_ne!(b, [120, 160, 220, 255]);
        // one region = one color (the near-identical halves joined)
        let (a2, a3) = (out.get_pixel(10, 50).0, out.get_pixel(70, 50).0);
        assert!((a2[0] as i32 - a3[0] as i32).abs() < 12 && (a2[1] as i32 - a3[1] as i32).abs() < 12, "{a2:?} {a3:?}");
        assert_ne!((a[0], a[1], a[2]), (b[0], b[1], b[2]), "two neighbouring flats get different swatches");
        assert!(recolor_flats(&mut d.layers[0], &colors, None, (0, 0, 10_000, 10_000), 3).is_err());
    }
}
