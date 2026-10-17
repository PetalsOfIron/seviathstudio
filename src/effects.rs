// SPDX-License-Identifier: GPL-3.0-or-later
// Layer effects and adjustment layers: both are non-destructive. Effects (drop shadow, glow,
// outline, color overlay, gradient overlay) are kept on the layer and drawn when the picture is put
// together, so the paint underneath is never touched. An adjustment layer applies one of the
// color filters (hue / saturation, brightness, levels, curves, invert, posterize, threshold) to
// everything below it in the same folder.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::blend::{self, CLEAR, Px};
use crate::doc::Blend;
use crate::filters::{self, Params};
use crate::gradient::{Gradient, Stop};

/// The farthest an effect may reach from the paint (pixels). Smaller than a tile, so a tile only
/// ever needs its eight neighbours.
pub const MAX_REACH: usize = 200;

#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub enum Place {
    Outside,
    Inside,
    Center,
}

impl Place {
    pub const ALL: [(Place, &'static str); 3] = [(Place::Outside, "Outside"), (Place::Center, "Center"), (Place::Inside, "Inside")];
}

#[derive(Clone, PartialEq, Debug, Serialize, Deserialize)]
pub enum FxKind {
    Shadow { color: [u8; 3], opacity: f32, dx: f32, dy: f32, blur: f32, spread: f32 },
    Glow { color: [u8; 3], opacity: f32, size: f32, spread: f32 },
    Outline { color: [u8; 3], opacity: f32, width: f32, place: Place },
    Color { color: [u8; 3], opacity: f32 },
    Gradient { from: [u8; 3], to: [u8; 3], angle: f32, opacity: f32 },
}

#[derive(Clone, PartialEq, Debug, Serialize, Deserialize)]
pub struct Effect {
    pub on: bool,
    pub kind: FxKind,
}

impl Effect {
    pub fn name(&self) -> &'static str {
        match self.kind {
            FxKind::Shadow { .. } => "Drop shadow",
            FxKind::Glow { .. } => "Glow",
            FxKind::Outline { .. } => "Outline",
            FxKind::Color { .. } => "Color overlay",
            FxKind::Gradient { .. } => "Gradient overlay",
        }
    }

    /// The effects you can add, with their starting values.
    pub fn presets() -> Vec<Effect> {
        let e = |kind| Effect { on: true, kind };
        vec![
            e(FxKind::Shadow { color: [0, 0, 0], opacity: 0.6, dx: 8.0, dy: 8.0, blur: 10.0, spread: 0.0 }),
            e(FxKind::Glow { color: [255, 230, 120], opacity: 0.8, size: 16.0, spread: 0.2 }),
            e(FxKind::Outline { color: [20, 18, 24], opacity: 1.0, width: 4.0, place: Place::Outside }),
            e(FxKind::Color { color: [255, 120, 160], opacity: 0.5 }),
            e(FxKind::Gradient { from: [40, 30, 90], to: [255, 140, 120], angle: 90.0, opacity: 0.7 }),
        ]
    }
}

/// True when something is switched on.
pub fn any_on(fx: &[Effect]) -> bool {
    fx.iter().any(|e| e.on)
}

/// How far (pixels) the effects reach beyond the paint.
pub fn reach(fx: &[Effect]) -> usize {
    fx.iter()
        .filter(|e| e.on)
        .map(|e| match &e.kind {
            FxKind::Shadow { dx, dy, blur, .. } => (dx.abs().max(dy.abs()) + blur * 1.5).ceil() as usize + 2,
            FxKind::Glow { size, .. } => (size * 1.5).ceil() as usize + 2,
            FxKind::Outline { width, place, .. } => {
                if *place == Place::Inside {
                    0
                } else {
                    width.ceil() as usize + 2
                }
            }
            _ => 0,
        })
        .max()
        .unwrap_or(0)
        .min(MAX_REACH)
}

fn under(out: &mut [Px], plane: &[f32], color: [u8; 3], opacity: f32) {
    let c = [color[0] as f32 / 255.0, color[1] as f32 / 255.0, color[2] as f32 / 255.0];
    for (o, &p) in out.iter_mut().zip(plane) {
        let a = (p * opacity).clamp(0.0, 1.0);
        if a > 0.0 {
            *o = blend::over(*o, [c[0] * a, c[1] * a, c[2] * a, a], 1.0, Blend::Normal);
        }
    }
}

/// Squared distance transform of one line (Felzenszwalb).
fn edt_line(f: &[f32], d: &mut [f32], v: &mut [usize], z: &mut [f32]) {
    const INF: f32 = 1e9;
    let n = f.len();
    let mut k = 0usize;
    v[0] = 0;
    z[0] = -INF;
    z[1] = INF;
    for q in 1..n {
        loop {
            let p = v[k];
            let s = ((f[q] + (q * q) as f32) - (f[p] + (p * p) as f32)) / (2.0 * q as f32 - 2.0 * p as f32);
            if s <= z[k] {
                k -= 1;
            } else {
                k += 1;
                v[k] = q;
                z[k] = s;
                z[k + 1] = INF;
                break;
            }
        }
    }
    k = 0;
    for (q, out) in d.iter_mut().enumerate() {
        while z[k + 1] < q as f32 {
            k += 1;
        }
        let dq = q as f32 - v[k] as f32;
        *out = dq * dq + f[v[k]];
    }
}

/// Distance (pixels) from every pixel to the nearest pixel where `feature` is true.
pub(crate) fn distance_to(feature: &[bool], n: usize) -> Vec<f32> {
    const INF: f32 = 1e9;
    let mut g: Vec<f32> = feature.iter().map(|&b| if b { 0.0 } else { INF }).collect();
    let (mut d, mut v, mut z) = (vec![0f32; n], vec![0usize; n], vec![0f32; n + 1]);
    let mut col = vec![0f32; n];
    for x in 0..n {
        for y in 0..n {
            col[y] = g[y * n + x];
        }
        edt_line(&col, &mut d, &mut v, &mut z);
        for y in 0..n {
            g[y * n + x] = d[y];
        }
    }
    let mut row = vec![0f32; n];
    for y in 0..n {
        row.copy_from_slice(&g[y * n..(y + 1) * n]);
        edt_line(&row, &mut d, &mut v, &mut z);
        g[y * n..(y + 1) * n].copy_from_slice(&d);
    }
    g.iter().map(|s| s.sqrt()).collect()
}

fn shifted(alpha: &[f32], n: usize, dx: f32, dy: f32) -> Vec<f32> {
    let (dx, dy) = (dx.round() as isize, dy.round() as isize);
    let mut out = vec![0f32; n * n];
    for y in 0..n {
        let sy = y as isize - dy;
        if sy < 0 || sy >= n as isize {
            continue;
        }
        for x in 0..n {
            let sx = x as isize - dx;
            if sx >= 0 && sx < n as isize {
                out[y * n + x] = alpha[sy as usize * n + sx as usize];
            }
        }
    }
    out
}

fn soft(plane: Vec<f32>, n: usize, blur: f32, spread: f32) -> Vec<f32> {
    let mut p = if blur > 0.3 { filters::gauss_channel(&plane, n, n, blur / 2.0) } else { plane };
    if spread > 0.0 {
        let k = 1.0 / (1.0 - spread.min(1.0) * 0.95);
        for v in p.iter_mut() {
            *v = (*v * k).min(1.0);
        }
    }
    p
}

/// How much of a pixel the outline covers: `band` = width, `outside` = the band lies outside the paint.
fn band(alpha: &[f32], n: usize, width: f32, outside: bool) -> Vec<f32> {
    let feature: Vec<bool> = alpha.iter().map(|&a| if outside { a >= 0.5 } else { a < 0.5 }).collect();
    distance_to(&feature, n).into_iter().map(|d| (width - d + 1.0).clamp(0.0, 1.0)).collect()
}

/// Draw the effects of one layer. `src` is the layer's own pixels in an `n`×`n` square whose top left
/// is at document pixel `origin`; the result has the same size.
pub fn render(fx: &[Effect], src: &[Px], n: usize, origin: (i32, i32), doc: (u32, u32)) -> Vec<Px> {
    let alpha: Vec<f32> = src.iter().map(|p| p[3]).collect();
    let mut out = vec![CLEAR; n * n];
    let mut layer = src.to_vec();
    let mut over_layer: Vec<(&FxKind, Vec<f32>)> = Vec::new();
    for e in fx.iter().filter(|e| e.on) {
        match &e.kind {
            FxKind::Shadow { color, opacity, dx, dy, blur, spread } => {
                let p = soft(shifted(&alpha, n, *dx, *dy), n, *blur, *spread);
                under(&mut out, &p, *color, *opacity);
            }
            FxKind::Glow { color, opacity, size, spread } => {
                let p = soft(alpha.clone(), n, *size, *spread);
                under(&mut out, &p, *color, *opacity);
            }
            FxKind::Outline { color, opacity, width, place } => {
                let w = width.max(0.0);
                match place {
                    Place::Outside => under(&mut out, &band(&alpha, n, w, true), *color, *opacity),
                    Place::Center => {
                        under(&mut out, &band(&alpha, n, w / 2.0, true), *color, *opacity);
                        over_layer.push((&e.kind, band(&alpha, n, w / 2.0, false)));
                    }
                    Place::Inside => over_layer.push((&e.kind, band(&alpha, n, w, false))),
                }
            }
            FxKind::Color { .. } | FxKind::Gradient { .. } => over_layer.push((&e.kind, Vec::new())),
        }
    }
    // overlays and inside outlines change the paint itself
    for (kind, cover) in over_layer {
        match kind {
            FxKind::Color { color, opacity } => tint(&mut layer, color_of(*color), *opacity, |_| 1.0),
            FxKind::Outline { color, opacity, .. } => tint(&mut layer, color_of(*color), *opacity, |i| cover[i]),
            FxKind::Gradient { from, to, angle, opacity } => {
                let g = Gradient {
                    stops: vec![Stop { pos: 0.0, color: *from, alpha: 1.0 }, Stop { pos: 1.0, color: *to, alpha: 1.0 }],
                    ..Gradient::default()
                };
                let lut = g.lut();
                let (s, c) = angle.to_radians().sin_cos();
                let (cx, cy) = (doc.0 as f32 / 2.0, doc.1 as f32 / 2.0);
                let half = (doc.0 as f32 * c.abs() + doc.1 as f32 * s.abs()) / 2.0;
                for i in 0..n * n {
                    let a = layer[i][3];
                    if a <= 0.0 {
                        continue;
                    }
                    let (x, y) = ((origin.0 + (i % n) as i32) as f32 + 0.5, (origin.1 + (i / n) as i32) as f32 + 0.5);
                    let t = ((x - cx) * c + (y - cy) * s) / (2.0 * half.max(1.0)) + 0.5;
                    let [r, gg, b, _] = lut.at(t.clamp(0.0, 1.0));
                    let k = *opacity;
                    for (ch, v) in [r, gg, b].into_iter().enumerate() {
                        layer[i][ch] += (v * a - layer[i][ch]) * k;
                    }
                }
            }
            _ => {}
        }
    }
    for (o, l) in out.iter_mut().zip(&layer) {
        if l[3] > 0.0 {
            *o = blend::over(*o, *l, 1.0, Blend::Normal);
        }
    }
    out
}

fn color_of(c: [u8; 3]) -> [f32; 3] {
    [c[0] as f32 / 255.0, c[1] as f32 / 255.0, c[2] as f32 / 255.0]
}

/// Mix a color into the paint (only where there is paint), `cover(i)` = how much of pixel `i`.
fn tint(layer: &mut [Px], color: [f32; 3], opacity: f32, cover: impl Fn(usize) -> f32) {
    for (i, p) in layer.iter_mut().enumerate() {
        let a = p[3];
        if a <= 0.0 {
            continue;
        }
        let k = (opacity * cover(i)).clamp(0.0, 1.0);
        if k > 0.0 {
            for ch in 0..3 {
                p[ch] += (color[ch] * a - p[ch]) * k;
            }
        }
    }
}

/// Ranges the sliders allow; loaded files are held to the same limits.
pub const SHADOW_OFFSET: f32 = 100.0;
pub const SHADOW_BLUR: f32 = 60.0;
pub const GLOW_SIZE: f32 = 100.0;
pub const OUTLINE_WIDTH: f32 = 60.0;
pub const MAX_EFFECTS: usize = 12;

/// Effects from a file with every number brought into range.
pub fn sanitize(fx: Vec<Effect>) -> Vec<Effect> {
    let f = |v: f32, lo: f32, hi: f32| if v.is_finite() { v.clamp(lo, hi) } else { lo };
    fx.into_iter()
        .take(MAX_EFFECTS)
        .map(|mut e| {
            match &mut e.kind {
                FxKind::Shadow { opacity, dx, dy, blur, spread, .. } => {
                    *opacity = f(*opacity, 0.0, 1.0);
                    *dx = f(*dx, -SHADOW_OFFSET, SHADOW_OFFSET);
                    *dy = f(*dy, -SHADOW_OFFSET, SHADOW_OFFSET);
                    *blur = f(*blur, 0.0, SHADOW_BLUR);
                    *spread = f(*spread, 0.0, 1.0);
                }
                FxKind::Glow { opacity, size, spread, .. } => {
                    *opacity = f(*opacity, 0.0, 1.0);
                    *size = f(*size, 0.0, GLOW_SIZE);
                    *spread = f(*spread, 0.0, 1.0);
                }
                FxKind::Outline { opacity, width, .. } => {
                    *opacity = f(*opacity, 0.0, 1.0);
                    *width = f(*width, 0.0, OUTLINE_WIDTH);
                }
                FxKind::Color { opacity, .. } => *opacity = f(*opacity, 0.0, 1.0),
                FxKind::Gradient { opacity, angle, .. } => {
                    *opacity = f(*opacity, 0.0, 1.0);
                    *angle = f(*angle, -360.0, 360.0);
                }
            }
            e
        })
        .collect()
}

// ---------- adjustment layers ----------

/// The color filters that look at one pixel at a time, so they work on any tile on their own.
pub const ADJUSTMENTS: [&str; 7] = ["hsl", "brightness", "levels", "curves", "invert", "posterize", "threshold"];

#[derive(Clone, PartialEq, Debug)]
pub struct Adjust {
    pub id: &'static str,
    pub params: Params,
}

#[derive(Clone, Serialize, Deserialize)]
pub struct AdjustJson {
    id: String,
    #[serde(default)]
    v: BTreeMap<String, f32>,
    #[serde(default)]
    colors: BTreeMap<String, [u8; 3]>,
    #[serde(default)]
    curve: Vec<[f32; 2]>,
}

impl Adjust {
    pub fn new(id: &str) -> Option<Adjust> {
        let def = filters::find(id).filter(|d| ADJUSTMENTS.contains(&d.id))?;
        Some(Adjust { id: def.id, params: Params::defaults(def) })
    }

    pub fn label(&self) -> &'static str {
        filters::find(self.id).map(|d| d.label).unwrap_or("Adjustment")
    }

    pub fn to_json(&self) -> AdjustJson {
        AdjustJson {
            id: self.id.into(),
            v: self.params.v.iter().map(|(k, v)| (k.to_string(), *v)).collect(),
            colors: self.params.colors.iter().map(|(k, v)| (k.to_string(), *v)).collect(),
            curve: self.params.curve.clone(),
        }
    }

    pub fn from_json(j: &AdjustJson) -> Option<Adjust> {
        let mut a = Adjust::new(&j.id)?;
        let def = filters::find(a.id)?;
        for pd in def.params {
            if let Some(v) = j.v.get(pd.key) {
                a.params.v.insert(pd.key, *v);
            }
            if let Some(c) = j.colors.get(pd.key) {
                a.params.colors.insert(pd.key, *c);
            }
        }
        if j.curve.len() >= 2 && j.curve.len() <= 64 {
            a.params.curve = j.curve.clone();
        }
        Some(a)
    }

    /// Change the colors of `px` (a premultiplied tile or any run of pixels) by `opacity` (0..1).
    pub fn apply(&self, px: &mut [Px], width: usize, opacity: f32) {
        let height = px.len() / width.max(1);
        let mut img = image::RgbaImage::new(width as u32, height as u32);
        for (o, p) in img.pixels_mut().zip(px.iter()) {
            let a = p[3];
            *o = if a <= 0.0 {
                image::Rgba([0, 0, 0, 0])
            } else {
                image::Rgba([(p[0] / a * 255.0).round().clamp(0.0, 255.0) as u8, (p[1] / a * 255.0).round().clamp(0.0, 255.0) as u8, (p[2] / a * 255.0).round().clamp(0.0, 255.0) as u8, 255])
            };
        }
        let res = filters::run(self.id, &img, &self.params, 1.0);
        for (p, r) in px.iter_mut().zip(res.pixels()) {
            let a = p[3];
            if a <= 0.0 {
                continue;
            }
            let new = [r.0[0] as f32 / 255.0 * a, r.0[1] as f32 / 255.0 * a, r.0[2] as f32 / 255.0 * a, a];
            *p = blend::lerp(*p, new, opacity);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const N: usize = 96;

    fn square(x0: usize, y0: usize, s: usize, c: [f32; 3]) -> Vec<Px> {
        let mut v = vec![CLEAR; N * N];
        for y in y0..y0 + s {
            for x in x0..x0 + s {
                v[y * N + x] = [c[0], c[1], c[2], 1.0];
            }
        }
        v
    }

    fn fx(kind: FxKind) -> Vec<Effect> {
        vec![Effect { on: true, kind }]
    }

    #[test]
    fn distance_is_euclidean() {
        let mut f = vec![false; 21 * 21];
        f[10 * 21 + 10] = true;
        let d = distance_to(&f, 21);
        assert!((d[10 * 21 + 13] - 3.0).abs() < 1e-3);
        assert!((d[13 * 21 + 14] - 5.0).abs() < 1e-3, "3-4-5 triangle");
        assert_eq!(d[10 * 21 + 10], 0.0);
    }

    #[test]
    fn shadow_lands_beside_the_paint_and_not_on_it() {
        let src = square(30, 30, 20, [1.0, 0.0, 0.0]);
        let out = render(&fx(FxKind::Shadow { color: [0, 0, 0], opacity: 1.0, dx: 10.0, dy: 0.0, blur: 0.0, spread: 0.0 }), &src, N, (0, 0), (N as u32, N as u32));
        assert!(out[40 * N + 55][3] > 0.99, "shadow to the right of the square");
        assert_eq!(out[40 * N + 55][0], 0.0);
        assert_eq!(out[40 * N + 40], [1.0, 0.0, 0.0, 1.0], "the paint stays on top");
        assert_eq!(out[40 * N + 10][3], 0.0, "nothing on the other side");
    }

    #[test]
    fn glow_fades_out_and_outline_has_the_width_asked() {
        let src = square(30, 30, 20, [1.0, 1.0, 1.0]);
        let g = render(&fx(FxKind::Glow { color: [255, 0, 0], opacity: 1.0, size: 10.0, spread: 0.0 }), &src, N, (0, 0), (96, 96));
        let (near, far) = (g[40 * N + 27][3], g[40 * N + 15][3]);
        assert!(near > far && near > 0.1 && far < near, "glow fades: {near} {far}");
        let o = render(&fx(FxKind::Outline { color: [0, 0, 255], opacity: 1.0, width: 4.0, place: Place::Outside }), &src, N, (0, 0), (96, 96));
        let row: Vec<f32> = (20..30).map(|x| o[40 * N + x][3]).collect();
        let full = row.iter().filter(|a| **a > 0.99).count();
        assert_eq!(full, 4, "a four pixel outline: {row:?}");
        // inside: the paint keeps its size and gets a colored rim
        let i = render(&fx(FxKind::Outline { color: [0, 0, 255], opacity: 1.0, width: 3.0, place: Place::Inside }), &src, N, (0, 0), (96, 96));
        assert_eq!(i[40 * N + 29][3], 0.0, "nothing grows outside");
        assert!(i[40 * N + 31][2] > 0.99 && i[40 * N + 31][0] < 0.01, "blue rim inside");
        assert!(i[40 * N + 40][0] > 0.99, "the middle stays white");
    }

    #[test]
    fn overlays_only_touch_the_paint() {
        let src = square(30, 30, 20, [1.0, 0.0, 0.0]);
        let c = render(&fx(FxKind::Color { color: [0, 0, 255], opacity: 1.0 }), &src, N, (0, 0), (96, 96));
        assert_eq!(c[40 * N + 40], [0.0, 0.0, 1.0, 1.0]);
        assert_eq!(c[10 * N + 10][3], 0.0);
        let g = render(&fx(FxKind::Gradient { from: [0, 0, 0], to: [255, 255, 255], angle: 0.0, opacity: 1.0 }), &src, N, (0, 0), (96, 96));
        assert!(g[40 * N + 31][0] < g[40 * N + 48][0], "left is darker than right");
    }

    #[test]
    fn effects_off_change_nothing_and_reach_is_capped() {
        let mut e = fx(FxKind::Glow { color: [255, 0, 0], opacity: 1.0, size: 500.0, spread: 0.0 });
        assert_eq!(reach(&e), MAX_REACH);
        e[0].on = false;
        assert_eq!(reach(&e), 0);
        assert!(!any_on(&e));
        let src = square(30, 30, 20, [1.0, 0.0, 0.0]);
        assert_eq!(render(&e, &src, N, (0, 0), (96, 96)), src);
    }

    #[test]
    fn adjustment_changes_colors_keeps_alpha_and_round_trips() {
        let mut a = Adjust::new("invert").unwrap();
        let mut px = vec![[1.0, 0.0, 0.0, 1.0], [0.0, 0.0, 0.0, 0.0], [0.5, 0.0, 0.0, 0.5]];
        a.apply(&mut px, 3, 1.0);
        assert!(px[0][0] < 0.01 && px[0][1] > 0.99 && px[0][2] > 0.99, "red inverted to cyan");
        assert_eq!(px[1], [0.0; 4], "clear stays clear");
        assert_eq!(px[2][3], 0.5, "alpha kept");
        let mut half = vec![[1.0, 0.0, 0.0, 1.0]];
        a.apply(&mut half, 1, 0.5);
        assert!((half[0][0] - 0.5).abs() < 0.02, "half strength: {:?}", half[0]);
        a = Adjust::new("hsl").unwrap();
        a.params.v.insert("hue", 90.0);
        let j = serde_json::to_string(&a.to_json()).unwrap();
        let back = Adjust::from_json(&serde_json::from_str(&j).unwrap()).unwrap();
        assert_eq!(back, a);
        assert!(Adjust::new("blur").is_none(), "neighbour filters can't be adjustment layers");
        assert!(Adjust::from_json(&AdjustJson { id: "oil".into(), v: BTreeMap::new(), colors: BTreeMap::new(), curve: vec![] }).is_none());
    }

    // ---------- in a whole document ----------

    use crate::doc::{Document, LayerKind, TILE};
    use eframe::egui::Color32;

    fn doc_with_red_square() -> Document {
        let mut d = Document::new(600, 300);
        let paper = d.layers.iter().position(|l| l.kind == LayerKind::Paper).unwrap();
        d.layers[paper].visible = false;
        for y in 100..140 {
            for x in 230..250 {
                d.layers[0].tile_mut((0, 0))[y * TILE + x] = Color32::from_rgb(255, 0, 0);
            }
        }
        d.take_dirty();
        d
    }

    fn px(d: &Document, x: usize, y: usize) -> Px {
        d.flatten_tile(((x / TILE) as i32, (y / TILE) as i32))[(y % TILE) * TILE + x % TILE]
    }

    #[test]
    fn shadow_crosses_the_tile_border_in_the_picture() {
        let mut d = doc_with_red_square();
        assert_eq!(px(&d, 270, 120)[3], 0.0, "nothing there before");
        d.layers[0].fx = fx(FxKind::Shadow { color: [0, 0, 0], opacity: 1.0, dx: 40.0, dy: 0.0, blur: 0.0, spread: 0.0 });
        assert!(px(&d, 275, 120)[3] > 0.99, "the shadow is in the next tile");
        assert_eq!(px(&d, 240, 120), [1.0, 0.0, 0.0, 1.0], "the paint is still red on top");
        assert_eq!(d.layers[0].tiles.len(), 1, "no paint was added to the layer");
        assert!(d.layers[0].tiles[&(0, 0)][120 * TILE + 250 - 1] == Color32::from_rgb(255, 0, 0));
        // changing a tile redraws its neighbours, because effects reach over the border
        d.mark_tile_dirty((0, 0));
        let mut keys = d.take_dirty();
        keys.sort();
        assert!(keys.contains(&(1, 0)) && keys.contains(&(0, 1)) && keys.contains(&(1, 1)), "{keys:?}");
    }

    #[test]
    fn adjustment_layer_changes_what_is_below_and_can_be_hidden() {
        let mut d = doc_with_red_square();
        let mut l = d.new_layer("Invert", LayerKind::Adjust);
        l.adjust = Adjust::new("invert");
        let top = d.layers[0].id;
        d.insert_layer(l, top);
        let c = px(&d, 240, 120);
        assert!(c[0] < 0.01 && c[1] > 0.99 && c[2] > 0.99, "red turned cyan: {c:?}");
        assert_eq!(px(&d, 10, 10)[3], 0.0, "clear stays clear");
        let at = d.index_of(top).unwrap() - 1;
        d.layers[at].visible = false;
        assert_eq!(px(&d, 240, 120), [1.0, 0.0, 0.0, 1.0]);
    }

    #[test]
    fn effects_and_adjustments_survive_save_and_open() {
        let mut d = doc_with_red_square();
        d.layers[0].fx = vec![
            Effect { on: true, kind: FxKind::Outline { color: [1, 2, 3], opacity: 0.5, width: 6.0, place: Place::Center } },
            Effect { on: false, kind: FxKind::Gradient { from: [9, 8, 7], to: [6, 5, 4], angle: 45.0, opacity: 0.25 } },
        ];
        let mut a = d.new_layer("Hue", LayerKind::Adjust);
        let mut adj = Adjust::new("hsl").unwrap();
        adj.params.v.insert("hue", 77.0);
        a.adjust = Some(adj);
        let top = d.layers[0].id;
        d.insert_layer(a, top);
        let path = std::env::temp_dir().join(format!("seviath_fx_test_{}.seviath", std::process::id()));
        crate::files::project::save(d.w, d.h, d.paper, &d.layers, &[], &Default::default(), &path).unwrap();
        let back = crate::files::project::load(&path).unwrap();
        std::fs::remove_file(&path).ok();
        let (la, lb) = (d.layers.iter().find(|l| l.kind == LayerKind::Raster).unwrap(), back.layers.iter().find(|l| l.kind == LayerKind::Raster).unwrap());
        assert_eq!(la.fx, lb.fx);
        let (aa, ab) = (d.layers.iter().find(|l| l.kind == LayerKind::Adjust).unwrap(), back.layers.iter().find(|l| l.kind == LayerKind::Adjust).unwrap());
        assert_eq!(aa.adjust, ab.adjust);
    }

    #[test]
    fn layered_exports_get_the_effects_and_adjustments_baked_in() {
        let mut d = doc_with_red_square();
        d.layers[0].fx = fx(FxKind::Shadow { color: [0, 0, 0], opacity: 1.0, dx: 40.0, dy: 0.0, blur: 0.0, spread: 0.0 });
        let mut l = d.new_layer("Invert", LayerKind::Adjust);
        l.adjust = Adjust::new("invert");
        let top = d.layers[0].id;
        d.insert_layer(l, top);
        let want = [px(&d, 240, 120), px(&d, 275, 120), px(&d, 10, 10)];
        let baked = crate::files::bake(d);
        assert!(baked.layers.iter().all(|l| l.kind != LayerKind::Adjust && l.fx.is_empty()), "nothing of ours is left in the file");
        let got = [px(&baked, 240, 120), px(&baked, 275, 120), px(&baked, 10, 10)];
        for (w, g) in want.iter().zip(&got) {
            assert!(w.iter().zip(g).all(|(a, b)| (a - b).abs() < 0.02), "same look: {w:?} vs {g:?}");
        }
    }
}
