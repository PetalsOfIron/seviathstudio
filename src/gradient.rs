// SPDX-License-Identifier: GPL-3.0-or-later
// Gradients (port of the first app's gradient.ts): color stops with opacity, five shapes
// (linear, radial, angle, diamond, mirrored), repeat modes, blending in OKLab so the middle stays
// clean, a little dither against banding, applying to a layer with any blend mode, the gradient
// map (lightness → gradient colors) and colors along a brush stroke.

use eframe::egui::{Color32, ColorImage, Pos2};
use rayon::prelude::*;

use crate::blend::{self, Px};
use crate::doc::{Blend, Layer, TILE, TileKey};
use crate::oklab::{oklab_to_rgb, rgb_to_oklab};
use crate::select::Selection;

#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Stop {
    /// 0..1 along the gradient
    pub pos: f32,
    pub color: [u8; 3],
    /// 0..1
    pub alpha: f32,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Shape {
    Linear,
    Radial,
    Angle,
    Diamond,
    Mirrored,
}

impl Shape {
    pub const ALL: [(Shape, &'static str, &'static str); 5] = [
        (Shape::Linear, "Linear", "From the first point to the second."),
        (Shape::Radial, "Radial", "Circles around the first point; the second point sets the radius."),
        (Shape::Angle, "Angle", "One turn around the first point, starting in the drag direction."),
        (Shape::Diamond, "Diamond", "Like radial, with a diamond outline."),
        (Shape::Mirrored, "Mirrored", "Linear, mirrored on both sides of the first point."),
    ];
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Repeat {
    None,
    Repeat,
    Mirror,
}

impl Repeat {
    pub const ALL: [(Repeat, &'static str); 3] = [(Repeat::None, "Once"), (Repeat::Repeat, "Repeat"), (Repeat::Mirror, "Back and forth")];
}

#[derive(Clone, PartialEq, Debug)]
pub struct Gradient {
    pub stops: Vec<Stop>,
    pub shape: Shape,
    pub repeat: Repeat,
    /// Blend the colors in OKLab (clean middle) instead of plain RGB.
    pub smooth: bool,
    /// A tiny noise so smooth gradients show no bands.
    pub dither: bool,
    /// 0..1
    pub opacity: f32,
    pub blend: Blend,
}

impl Default for Gradient {
    fn default() -> Self {
        Self {
            stops: vec![Stop { pos: 0.0, color: [34, 34, 34], alpha: 1.0 }, Stop { pos: 1.0, color: [255, 255, 255], alpha: 1.0 }],
            shape: Shape::Linear,
            repeat: Repeat::None,
            smooth: true,
            dither: true,
            opacity: 1.0,
            blend: Blend::Normal,
        }
    }
}

fn stop(pos: f32, hex: u32, alpha: f32) -> Stop {
    Stop { pos, color: [(hex >> 16) as u8, (hex >> 8) as u8, hex as u8], alpha }
}

/// Ready-made stop sets. The first two come from your two colors.
pub fn presets(primary: [u8; 3], secondary: [u8; 3]) -> Vec<(&'static str, Vec<Stop>)> {
    vec![
        ("Main to clear", vec![Stop { pos: 0.0, color: primary, alpha: 1.0 }, Stop { pos: 1.0, color: primary, alpha: 0.0 }]),
        ("Main to second", vec![Stop { pos: 0.0, color: primary, alpha: 1.0 }, Stop { pos: 1.0, color: secondary, alpha: 1.0 }]),
        (
            "Rainbow",
            [0xff3b30, 0xff9500, 0xffcc00, 0x34c759, 0x00c7be, 0x007aff, 0xaf52de].iter().enumerate().map(|(i, c)| stop(i as f32 / 6.0, *c, 1.0)).collect(),
        ),
        ("Sunset", vec![stop(0.0, 0x2b1055, 1.0), stop(0.45, 0xd53a9d, 1.0), stop(0.75, 0xff8a5c, 1.0), stop(1.0, 0xffd56b, 1.0)]),
        ("Sky", vec![stop(0.0, 0x1e3c72, 1.0), stop(0.6, 0x5fa8e8, 1.0), stop(1.0, 0xe6f3ff, 1.0)]),
        ("Pastel", vec![stop(0.0, 0xffc8dd, 1.0), stop(0.5, 0xcdb4db, 1.0), stop(1.0, 0xa2d2ff, 1.0)]),
        ("Skin shadow", vec![stop(0.0, 0xf2c6a8, 1.0), stop(1.0, 0xb06a5b, 1.0)]),
        ("Hair shine", vec![stop(0.0, 0xffffff, 0.0), stop(0.5, 0xffffff, 0.85), stop(1.0, 0xffffff, 0.0)]),
        ("Fire", vec![stop(0.0, 0x3d0000, 1.0), stop(0.4, 0xe2241b, 1.0), stop(0.75, 0xff9e1b, 1.0), stop(1.0, 0xfff3b0, 1.0)]),
        ("Metal", vec![stop(0.0, 0x4a4f57, 1.0), stop(0.3, 0xdfe3e8, 1.0), stop(0.5, 0x7c828b, 1.0), stop(0.75, 0xf4f6f8, 1.0), stop(1.0, 0x5b6068, 1.0)]),
    ]
}

const LUT_SIZE: usize = 1024;

/// The gradient sampled 1024 times: fast enough to color every pixel of a big canvas.
pub struct Lut {
    v: Vec<[f32; 4]>,
    repeat: Repeat,
}

fn wrap(t: f32, repeat: Repeat) -> f32 {
    match repeat {
        Repeat::None => t.clamp(0.0, 1.0),
        Repeat::Repeat => t - t.floor(),
        Repeat::Mirror => {
            let m = t - 2.0 * (t / 2.0).floor();
            if m <= 1.0 { m } else { 2.0 - m }
        }
    }
}

impl Lut {
    /// Color (not premultiplied, 0..1) and alpha at `t`; `t` is wrapped by the repeat mode.
    #[inline]
    pub fn at(&self, t: f32) -> [f32; 4] {
        let i = (wrap(t, self.repeat) * (LUT_SIZE - 1) as f32).round() as usize;
        self.v[i.min(LUT_SIZE - 1)]
    }
}

impl Gradient {
    pub fn sorted(&self) -> Vec<Stop> {
        let mut s = self.stops.clone();
        s.sort_by(|a, b| a.pos.total_cmp(&b.pos));
        s
    }

    /// Color at `t` (0..1): [r, g, b, alpha], not premultiplied.
    pub fn color_at(&self, t: f32) -> [f32; 4] {
        let s = self.sorted();
        if s.is_empty() {
            return [0.0; 4];
        }
        let rgb = |c: [u8; 3]| [c[0] as f32 / 255.0, c[1] as f32 / 255.0, c[2] as f32 / 255.0];
        let first = &s[0];
        let last = &s[s.len() - 1];
        if t <= first.pos {
            let c = rgb(first.color);
            return [c[0], c[1], c[2], first.alpha];
        }
        if t >= last.pos {
            let c = rgb(last.color);
            return [c[0], c[1], c[2], last.alpha];
        }
        let mut i = 0;
        while i + 2 < s.len() && s[i + 1].pos < t {
            i += 1;
        }
        let (a, b) = (s[i], s[i + 1]);
        let u = if b.pos > a.pos { (t - a.pos) / (b.pos - a.pos) } else { 0.0 };
        let alpha = a.alpha + (b.alpha - a.alpha) * u;
        // a fully clear stop takes the other stop's color, so fading out never turns gray
        let (ca, cb) = (rgb(if a.alpha <= 0.0 { b.color } else { a.color }), rgb(if b.alpha <= 0.0 { a.color } else { b.color }));
        if !self.smooth {
            return [ca[0] + (cb[0] - ca[0]) * u, ca[1] + (cb[1] - ca[1]) * u, ca[2] + (cb[2] - ca[2]) * u, alpha];
        }
        let (la, lb) = (rgb_to_oklab(ca), rgb_to_oklab(cb));
        let c = oklab_to_rgb([la[0] + (lb[0] - la[0]) * u, la[1] + (lb[1] - la[1]) * u, la[2] + (lb[2] - la[2]) * u]);
        [c[0], c[1], c[2], alpha]
    }

    pub fn lut(&self) -> Lut {
        let v = (0..LUT_SIZE).map(|i| self.color_at(i as f32 / (LUT_SIZE - 1) as f32)).collect();
        Lut { v, repeat: self.repeat }
    }

    /// Position along the gradient (not yet wrapped) of the canvas point `(x, y)`.
    pub fn t_at(&self, p0: Pos2, p1: Pos2, x: f32, y: f32) -> f32 {
        let (dx, dy) = (p1.x - p0.x, p1.y - p0.y);
        let len2 = (dx * dx + dy * dy).max(1e-6);
        let len = len2.sqrt();
        let (px, py) = (x - p0.x, y - p0.y);
        match self.shape {
            Shape::Linear => (px * dx + py * dy) / len2,
            Shape::Mirrored => (px * dx + py * dy).abs() / len2,
            Shape::Radial => (px * px + py * py).sqrt() / len,
            Shape::Diamond => {
                let (ux, uy) = (dx / len, dy / len);
                let (a, b) = (px * ux + py * uy, -px * uy + py * ux);
                (a.abs() + b.abs()) / len
            }
            Shape::Angle => {
                let mut a = py.atan2(px) - dy.atan2(dx);
                a -= (a / std::f32::consts::TAU).floor() * std::f32::consts::TAU;
                a / std::f32::consts::TAU
            }
        }
    }

    /// The gradient at a pixel: [r, g, b, alpha], not premultiplied, with the dither noise.
    #[inline]
    fn pixel(&self, lut: &Lut, p0: Pos2, p1: Pos2, x: i32, y: i32) -> [f32; 4] {
        let mut c = lut.at(self.t_at(p0, p1, x as f32 + 0.5, y as f32 + 0.5));
        if self.dither {
            let n = (hash2(x, y) - 0.5) * (2.0 / 255.0);
            c[0] += n;
            c[1] += n;
            c[2] += n;
        }
        c
    }

    /// A small picture of the gradient over `bounds` (x0, y0, x1, y1), for the live preview.
    pub fn preview(&self, p0: Pos2, p1: Pos2, bounds: (i32, i32, i32, i32), sel: Option<&Selection>, max_side: usize) -> ColorImage {
        let (x0, y0, x1, y1) = bounds;
        let (bw, bh) = ((x1 - x0).max(1) as f32, (y1 - y0).max(1) as f32);
        let k = (max_side as f32 / bw.max(bh)).min(1.0);
        let (w, h) = (((bw * k).round() as usize).max(1), ((bh * k).round() as usize).max(1));
        let lut = self.lut();
        let mut px = vec![Color32::TRANSPARENT; w * h];
        px.par_chunks_mut(w).enumerate().for_each(|(j, row)| {
            for (i, out) in row.iter_mut().enumerate() {
                let x = x0 as f32 + (i as f32 + 0.5) / k;
                let y = y0 as f32 + (j as f32 + 0.5) / k;
                let cov = sel.map(|s| s.at(x as i32, y as i32) as f32 / 255.0).unwrap_or(1.0);
                let c = lut.at(self.t_at(p0, p1, x, y));
                let a = (c[3] * self.opacity * cov).clamp(0.0, 1.0);
                *out = blend::from_px([c[0] * a, c[1] * a, c[2] * a, a]);
            }
        });
        ColorImage::new([w, h], px)
    }

    /// Put the gradient onto a layer inside `bounds` (clipped to the canvas), limited by the
    /// selection, using the gradient's opacity and blend mode. Returns the tiles that changed.
    pub fn apply(&self, layer: &mut Layer, p0: Pos2, p1: Pos2, sel: Option<&Selection>, bounds: (i32, i32, i32, i32)) -> Vec<TileKey> {
        let (x0, y0, x1, y1) = bounds;
        if x1 <= x0 || y1 <= y0 {
            return Vec::new();
        }
        let t = TILE as i32;
        let keys: Vec<TileKey> = (y0.div_euclid(t)..=(y1 - 1).div_euclid(t))
            .flat_map(|ty| (x0.div_euclid(t)..=(x1 - 1).div_euclid(t)).map(move |tx| (tx, ty)))
            .collect();
        let lut = self.lut();
        let lock_alpha = layer.lock_alpha;
        let results: Vec<(TileKey, Vec<Color32>)> = keys
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
                    if y < y0 || y >= y1 {
                        continue;
                    }
                    for i in 0..t {
                        let x = key.0 * t + i;
                        if x < x0 || x >= x1 {
                            continue;
                        }
                        let k = (j * t + i) as usize;
                        let cov = mask.map(|m| m[k] as f32 / 255.0).unwrap_or(1.0);
                        if cov <= 0.0 {
                            continue;
                        }
                        let c = self.pixel(&lut, p0, p1, x, y);
                        let a = (c[3] * cov).clamp(0.0, 1.0);
                        if a <= 0.0 {
                            continue;
                        }
                        let src: Px = [c[0].clamp(0.0, 1.0) * a, c[1].clamp(0.0, 1.0) * a, c[2].clamp(0.0, 1.0) * a, a];
                        let dst = blend::to_px(px[k]);
                        let out = if lock_alpha { blend::atop(dst, src, self.opacity, self.blend) } else { blend::over(dst, src, self.opacity, self.blend) };
                        px[k] = blend::from_px(out);
                        changed = true;
                    }
                }
                changed.then_some((key, px))
            })
            .collect();
        commit(layer, results)
    }

    /// Gradient map: every existing pixel takes the gradient color for its lightness (dark = the
    /// start of the gradient, light = the end). Alpha is kept.
    pub fn map_lightness(&self, layer: &mut Layer, sel: Option<&Selection>, bounds: (i32, i32, i32, i32)) -> Vec<TileKey> {
        let (x0, y0, x1, y1) = bounds;
        let t = TILE as i32;
        let lut = self.lut();
        let keys: Vec<TileKey> = layer.tiles.keys().copied().collect();
        let results: Vec<(TileKey, Vec<Color32>)> = keys
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
                        let k = (j * t + i) as usize;
                        let dst = blend::to_px(px[k]);
                        if dst[3] <= 0.0 {
                            continue;
                        }
                        let cov = mask.map(|m| m[k] as f32 / 255.0).unwrap_or(1.0);
                        if cov <= 0.0 {
                            continue;
                        }
                        let un = [dst[0] / dst[3], dst[1] / dst[3], dst[2] / dst[3]];
                        let l = rgb_to_oklab(un)[0];
                        let c = lut.at(l);
                        let a = dst[3] * c[3];
                        let new: Px = [c[0] * a, c[1] * a, c[2] * a, a];
                        px[k] = blend::from_px(blend::lerp(dst, new, cov));
                        changed = true;
                    }
                }
                changed.then_some((key, px))
            })
            .collect();
        commit(layer, results)
    }

    /// Colors along a brush stroke, `length` canvas pixels for one pass of the gradient.
    pub fn along_stroke(&self, length: f32) -> StrokeGradient {
        let stops: Vec<f32> = self.sorted().iter().map(|s| s.pos).collect();
        StrokeGradient { lut: self.lut(), stops, length: length.max(1.0), repeat: self.repeat }
    }
}

pub struct StrokeGradient {
    lut: Lut,
    stops: Vec<f32>,
    length: f32,
    repeat: Repeat,
}

impl StrokeGradient {
    /// The color (0..1) and opacity (0..1) after `d` pixels of stroke.
    pub fn at(&self, d: f32) -> ([f32; 3], f32) {
        let mut t = wrap(d / self.length, self.repeat);
        // along a stroke each color holds for a while before it blends into the next, so every
        // color gets a fair share of the line (two colors are about half and half)
        for w in self.stops.windows(2) {
            let (a, b) = (w[0], w[1]);
            if t >= a && t <= b && b > a {
                let f = (t - a) / (b - a);
                let hold = 0.22;
                let g = if f < hold { 0.0 } else if f > 1.0 - hold { 1.0 } else { (f - hold) / (1.0 - 2.0 * hold) };
                t = a + g * g * (3.0 - 2.0 * g) * (b - a);
                break;
            }
        }
        let c = self.lut.at(t);
        ([c[0], c[1], c[2]], c[3])
    }
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

/// A cheap repeatable noise in 0..1 for dithering.
#[inline]
fn hash2(x: i32, y: i32) -> f32 {
    let mut h = (x as u32).wrapping_mul(0x9E37_79B1) ^ (y as u32).wrapping_mul(0x85EB_CA77);
    h ^= h >> 15;
    h = h.wrapping_mul(0x2C1B_3C6D);
    h ^= h >> 12;
    h = h.wrapping_mul(0x297A_2D39);
    h ^= h >> 15;
    (h & 0xFFFF) as f32 / 65535.0
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::doc::Document;
    use eframe::egui::pos2;

    fn two(a: [u8; 3], b: [u8; 3]) -> Gradient {
        Gradient {
            stops: vec![Stop { pos: 0.0, color: a, alpha: 1.0 }, Stop { pos: 1.0, color: b, alpha: 1.0 }],
            dither: false,
            ..Default::default()
        }
    }

    #[test]
    fn shapes_measure_distance() {
        let mut g = two([0, 0, 0], [255, 255, 255]);
        let (p0, p1) = (pos2(100.0, 100.0), pos2(200.0, 100.0));
        assert!((g.t_at(p0, p1, 150.0, 400.0) - 0.5).abs() < 1e-5, "linear ignores the sideways position");
        g.shape = Shape::Radial;
        assert!((g.t_at(p0, p1, 100.0, 150.0) - 0.5).abs() < 1e-5);
        g.shape = Shape::Mirrored;
        assert!((g.t_at(p0, p1, 50.0, 0.0) - 0.5).abs() < 1e-5);
        g.shape = Shape::Diamond;
        assert!((g.t_at(p0, p1, 150.0, 150.0) - 1.0).abs() < 1e-5, "50 along + 50 across");
        g.shape = Shape::Angle;
        assert!(g.t_at(p0, p1, 200.0, 100.0).abs() < 1e-4 || (g.t_at(p0, p1, 200.0, 100.0) - 1.0).abs() < 1e-4);
        assert!((g.t_at(p0, p1, 100.0, 200.0) - 0.25).abs() < 1e-4, "a quarter turn clockwise on screen");
    }

    #[test]
    fn repeat_modes_wrap() {
        assert!((wrap(1.25, Repeat::Repeat) - 0.25).abs() < 1e-6);
        assert!((wrap(1.25, Repeat::Mirror) - 0.75).abs() < 1e-6);
        assert_eq!(wrap(1.25, Repeat::None), 1.0);
        assert_eq!(wrap(-3.0, Repeat::None), 0.0);
    }

    #[test]
    fn oklab_middle_stays_colorful() {
        let mut g = two([0, 0, 255], [255, 255, 0]);
        let spread = |c: [f32; 4]| c[..3].iter().cloned().fold(0.0f32, f32::max) - c[..3].iter().cloned().fold(1.0f32, f32::min);
        g.smooth = true;
        assert!(spread(g.color_at(0.5)) > 0.2);
        g.smooth = false;
        assert!(spread(g.color_at(0.5)) < 0.01, "plain RGB gives gray");
    }

    #[test]
    fn clear_stop_keeps_the_other_color() {
        let g = Gradient {
            stops: vec![Stop { pos: 0.0, color: [255, 0, 0], alpha: 1.0 }, Stop { pos: 1.0, color: [0, 0, 255], alpha: 0.0 }],
            ..Default::default()
        };
        let c = g.color_at(0.5);
        assert!(c[0] > 0.8 && c[2] < 0.2 && (c[3] - 0.5).abs() < 1e-5, "fading out must stay red, not turn purple: {c:?}");
    }

    #[test]
    fn apply_respects_selection_opacity_and_blend() {
        let mut d = Document::new(300, 300);
        let sel = Selection::polygon(&[pos2(0.0, 0.0), pos2(100.0, 0.0), pos2(100.0, 300.0), pos2(0.0, 300.0)], 300, 300, false);
        let mut g = two([255, 0, 0], [255, 0, 0]);
        g.opacity = 0.5;
        let keys = g.apply(&mut d.layers[0], pos2(0.0, 0.0), pos2(300.0, 0.0), sel.as_ref(), (0, 0, 300, 300));
        assert!(!keys.is_empty());
        let img = crate::files::layer_rgba(&d.layers[0], 0, 0, 300, 300);
        let p = img.get_pixel(50, 50).0;
        assert!(p[0] >= 250 && p[3] == 128, "{p:?}");
        assert_eq!(img.get_pixel(200, 50).0[3], 0, "outside the selection");
        // Multiply over existing paint darkens it
        let mut base = Document::new(64, 64);
        let red = image::RgbaImage::from_pixel(64, 64, image::Rgba([200, 100, 50, 255]));
        base.layers[0].tiles = crate::files::tiles_from_rgba(&red, 0, 0, 64, 64);
        let mut m = two([128, 128, 128], [128, 128, 128]);
        m.blend = Blend::Multiply;
        m.apply(&mut base.layers[0], pos2(0.0, 0.0), pos2(64.0, 0.0), None, (0, 0, 64, 64));
        let p = crate::files::layer_rgba(&base.layers[0], 0, 0, 64, 64).get_pixel(10, 10).0;
        assert!((p[0] as i32 - 100).abs() <= 2 && (p[2] as i32 - 25).abs() <= 2, "{p:?}");
    }

    #[test]
    fn gradient_map_follows_lightness() {
        let mut d = Document::new(64, 64);
        let img = image::RgbaImage::from_fn(64, 64, |x, _| if x < 32 { image::Rgba([0, 0, 0, 255]) } else { image::Rgba([255, 255, 255, 255]) });
        d.layers[0].tiles = crate::files::tiles_from_rgba(&img, 0, 0, 64, 64);
        let g = two([200, 0, 0], [0, 0, 200]);
        g.map_lightness(&mut d.layers[0], None, (0, 0, 64, 64));
        let out = crate::files::layer_rgba(&d.layers[0], 0, 0, 64, 64);
        let (dark, light) = (out.get_pixel(5, 5).0, out.get_pixel(60, 5).0);
        assert!(dark[0] > 180 && dark[2] < 40, "black became the first color: {dark:?}");
        assert!(light[2] > 180 && light[0] < 40, "white became the last color: {light:?}");
    }

    #[test]
    fn stroke_colors_hold_then_blend() {
        let g = two([255, 0, 0], [0, 0, 255]);
        let s = g.along_stroke(1000.0);
        let (c0, _) = s.at(0.0);
        let (c1, _) = s.at(1000.0);
        let (early, _) = s.at(100.0);
        assert!(c0[0] > 0.95 && c1[2] > 0.95);
        assert!(early[0] > 0.95, "the first color holds for the first stretch");
    }

    #[test]
    fn dither_is_tiny_and_repeatable() {
        let g = Gradient { dither: true, ..two([100, 100, 100], [100, 100, 100]) };
        let lut = g.lut();
        let a = g.pixel(&lut, pos2(0.0, 0.0), pos2(10.0, 0.0), 5, 7);
        let b = g.pixel(&lut, pos2(0.0, 0.0), pos2(10.0, 0.0), 5, 7);
        assert_eq!(a, b);
        assert!((a[0] - 100.0 / 255.0).abs() <= 1.0 / 255.0 + 1e-4);
    }
}
