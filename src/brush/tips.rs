// SPDX-License-Identifier: GPL-3.0-or-later
// Brush tips ("dabs"): each tip is drawn once as a 256×256 mask, gets its edge effects
// (ragged, torn, wet rim, grain, bristle streaks, warp, in-tip texture, extra tips), and is kept
// at several sizes (mip levels) so small dabs stay smooth. Ported from the web version.

use std::collections::HashMap;
use std::f32::consts::{FRAC_PI_2, PI, TAU};
use std::sync::{Arc, Mutex, OnceLock};

use super::noise::{Rng, fbm, noise, smoothstep};
use super::raster::{Mask, Mode, Path};
use super::textures;
use super::{BrushSettings, Edge, ShapeBase, SubMode, SubTip, Tip};

pub const N: usize = 256;
pub const MAX_VARIANTS: u32 = 8;

/// A tip mask at several sizes: level 0 is 256 px, each next level half as big.
pub struct TipMask {
    pub mips: Vec<Mask>,
}

impl TipMask {
    fn new(base: Mask) -> Self {
        let mut mips = vec![base];
        while mips.last().unwrap().n > 8 {
            let next = mips.last().unwrap().resized(mips.last().unwrap().n / 2);
            mips.push(next);
        }
        Self { mips }
    }

    /// The level whose size best matches a dab `diameter` pixels wide (never blurrier than needed).
    pub fn level(&self, diameter: f32) -> &Mask {
        let mut best = &self.mips[0];
        for m in &self.mips {
            if m.n as f32 >= diameter {
                best = m;
            }
        }
        best
    }
}

/// Bilinear sample at mask coordinates (pixel centers at +0.5), 0 outside.
#[inline]
pub fn sample(m: &Mask, x: f32, y: f32) -> f32 {
    let (u, v) = (x - 0.5, y - 0.5);
    let (x0, y0) = (u.floor(), v.floor());
    let (fx, fy) = (u - x0, v - y0);
    let n = m.n as i32;
    let (xi, yi) = (x0 as i32, y0 as i32);
    let at = |x: i32, y: i32| if x < 0 || y < 0 || x >= n || y >= n { 0.0 } else { m.a[(y * n + x) as usize] };
    let top = at(xi, yi) + (at(xi + 1, yi) - at(xi, yi)) * fx;
    let bot = at(xi, yi + 1) + (at(xi + 1, yi + 1) - at(xi, yi + 1)) * fx;
    top + (bot - top) * fy
}

fn cache() -> &'static Mutex<HashMap<String, Arc<TipMask>>> {
    static C: OnceLock<Mutex<HashMap<String, Arc<TipMask>>>> = OnceLock::new();
    C.get_or_init(|| Mutex::new(HashMap::new()))
}

fn cached(key: String, make: impl FnOnce() -> Mask) -> Arc<TipMask> {
    if let Some(m) = cache().lock().unwrap().get(&key) {
        return m.clone();
    }
    let made = Arc::new(TipMask::new(make()));
    let mut c = cache().lock().unwrap();
    if c.len() > 400 {
        c.clear(); // simple cap; tips are quick to rebuild
    }
    c.insert(key, made.clone());
    made
}

fn base_key(tip: Tip, s: &BrushSettings) -> String {
    match tip {
        Tip::Round | Tip::Chalk | Tip::Wet => format!("{tip:?}:{:.2}", s.hardness),
        Tip::Shape => format!("shape:{:?}:{}:{:.2}:{:.2}", s.shape_base, s.sides, s.star_inner, s.hardness),
        Tip::Image => format!("image:{:x}:{:.2}", s.tip_image.as_ref().map(|t| t.id).unwrap_or(0), s.hardness),
        _ => format!("{tip:?}"),
    }
}

fn edge_key(s: &BrushSettings) -> String {
    format!(
        "{:?},{:.2},{:.2},{:.2},{:.2},{:.2},{:.2},{},{:?}{:.2}{:.2},{:?}",
        s.edge,
        s.edge_amount,
        s.edge_detail,
        s.wet_rim,
        s.grain,
        s.streaks,
        s.deform,
        s.seed,
        if s.dir_texture() { Some(s.tex_kind) } else { None },
        s.texture,
        s.tex_scale,
        s.extra_tips.iter().map(|t| (t.tip, t.shape_base, (t.scale * 100.0) as i32, (t.scatter * 100.0) as i32, t.mode, (t.strength * 100.0) as i32)).collect::<Vec<_>>()
    )
}

/// The tip mask for these settings; `variant` picks one of the slightly different versions.
pub fn tip_mask(s: &BrushSettings, variant: u32) -> Arc<TipMask> {
    let effects = s.has_edge_effects();
    let v = if effects { variant.min(MAX_VARIANTS - 1) } else { 0 };
    let mut key = base_key(s.tip, s);
    if effects {
        key += &format!("|{}|{v}", edge_key(s));
    }
    if !s.antialias {
        key += "|aa0";
    }
    cached(key, || {
        let base = base_mask(s.tip, s);
        let mut m = if effects { shape_edges(&base, s, v) } else { base };
        if !s.antialias {
            for a in &mut m.a {
                *a = if *a >= 110.0 / 255.0 { 1.0 } else { 0.0 };
            }
        }
        m
    })
}

/// How many versions this brush cycles through.
pub fn variant_count(s: &BrushSettings) -> u32 {
    if !s.has_edge_effects() {
        return 1;
    }
    let want = if s.extra_tips.is_empty() { s.variants } else { s.variants.max(4) };
    want.clamp(1, MAX_VARIANTS)
}

// ---------- Base tips ----------

fn round_mask(hardness: f32) -> Mask {
    let mut m = Mask::new(N);
    let c = N as f32 / 2.0;
    let h = hardness.clamp(0.0, 0.97); // keep a tiny soft edge so lines are anti-aliased
    m.radial(c, c, c, &[(0.0, 1.0), (h, 1.0), (h + (1.0 - h) * 0.5, 0.35), (1.0, 0.0)], Mode::Over);
    m
}

/// Your picture as a tip, centered in the 256 square; `hardness` below 1 softens its edges.
fn image_mask(img: &super::tipimage::TipImage, hardness: f32) -> Mask {
    let mut m = Mask::new(N);
    let k = img.n as f32 / N as f32;
    for y in 0..N {
        for x in 0..N {
            m.a[y * N + x] = img.at((x as f32 + 0.5) * k, (y as f32 + 0.5) * k);
        }
    }
    let soft = (1.0 - hardness.clamp(0.0, 1.0)) * 6.0;
    if soft > 0.4 {
        m.blur_css(soft);
    }
    m
}

fn base_mask(tip: Tip, s: &BrushSettings) -> Mask {
    match tip {
        Tip::Round => round_mask(s.hardness),
        Tip::Shape => shape_mask(s.shape_base, s.sides, s.star_inner, s.hardness),
        Tip::Image => match &s.tip_image {
            Some(img) => image_mask(img, s.hardness),
            None => round_mask(s.hardness),
        },
        _ => procedural(tip, s.hardness),
    }
}

fn procedural(tip: Tip, hardness: f32) -> Mask {
    let mut c = Mask::new(N);
    let m = N as f32 / 2.0;
    let mut r = Rng::new(7 + tip.label().len() as u32 * 31);
    match tip {
        Tip::Bristle => {
            // many small hair tips in a bundle
            for _ in 0..70 {
                let (a, d) = (r.next() * TAU, r.next().sqrt() * m * 0.85);
                let alpha = 0.35 + r.next() * 0.65;
                let rad = 6.0 + r.next() * 12.0;
                c.circle(m + a.cos() * d, m + a.sin() * d, rad, alpha, Mode::Over);
            }
            c.blur_css(2.0);
        }
        Tip::Chalk => {
            c = round_mask(hardness.max(0.4));
            // knock random holes in it
            for _ in 0..900 {
                let alpha = r.next() * 0.8;
                let (x, y) = (r.next() * N as f32, r.next() * N as f32);
                let (w, h) = (2.0 + r.next() * 6.0, 2.0 + r.next() * 6.0);
                c.fill(&Path::rect(x, y, w, h), alpha, Mode::Out);
            }
            c.blur_css(1.0);
        }
        Tip::Wet => {
            let h = hardness.clamp(0.5, 0.9);
            c.radial(m, m, m, &[(0.0, 0.45), (h * 0.85, 0.6), (h, 0.95), ((h + 0.08).min(0.99), 0.5), (1.0, 0.0)], Mode::Over);
        }
        Tip::Splatter => {
            for _ in 0..26 {
                let (a, d) = (r.next() * TAU, r.next().sqrt() * m * 0.85);
                let rad = 2.0 + r.next().powi(3) * 22.0;
                c.circle(m + a.cos() * d, m + a.sin() * d, rad, 1.0, Mode::Over);
            }
        }
        Tip::Leaf => {
            let mut p = Path::new();
            p.move_to(m, 10.0)
                .cubic_to(m + 95.0, 70.0, m + 70.0, 190.0, m, N as f32 - 10.0)
                .cubic_to(m - 70.0, 190.0, m - 95.0, 70.0, m, 10.0);
            c.fill(&p, 1.0, Mode::Over);
            c.line((m, 30.0), (m, N as f32 - 20.0), 5.0, 5.0, 0.5, Mode::Out);
        }
        Tip::Grass => {
            for _ in 0..4 {
                let x = m + (r.next() - 0.5) * 60.0;
                let lean = (r.next() - 0.5) * 80.0;
                let top = 10.0 + r.next() * 60.0;
                let w = 7.0 + r.next() * 7.0;
                let alpha = 0.75 + r.next() * 0.25;
                let nn = N as f32;
                let mut p = Path::new();
                p.move_to(x - w, nn)
                    .quad_to(x - w * 0.5 + lean * 0.3, (nn + top) / 2.0, x + lean, top)
                    .quad_to(x + w * 0.5 + lean * 0.3, (nn + top) / 2.0, x + w, nn);
                c.fill(&p, alpha, Mode::Over);
            }
        }
        Tip::Star => {
            c.radial(m, m, m * 0.35, &[(0.0, 1.0), (1.0, 0.0)], Mode::Over);
            for (w, l) in [(10.0f32, m), (6.0, m * 0.6)] {
                for k in 0..4 {
                    let rot = k as f32 * FRAC_PI_2 + if l < m { PI / 4.0 } else { 0.0 };
                    let (s, co) = rot.sin_cos();
                    let tr = |x: f32, y: f32| (m + x * co - y * s, m + x * s + y * co);
                    let (a, b, cc) = (tr(0.0, -w), tr(l, 0.0), tr(0.0, w));
                    let mut p = Path::new();
                    p.move_to(a.0, a.1).line_to(b.0, b.1).line_to(cc.0, cc.1);
                    c.fill(&p, 1.0, Mode::Over);
                }
            }
        }
        Tip::Strands => {
            // fine hairs along the tip's length (x axis): thick at the root, pointed at the tip
            for _ in 0..46 {
                let y0 = m + (r.next() - 0.5) * m * 1.3;
                let x0 = m - m * (0.85 - r.next() * 0.5);
                let len = m * (0.7 + r.next());
                let bend = (r.next() - 0.5) * m * 0.35;
                let wid = 1.2 + r.next() * 2.4;
                let alpha = 0.35 + r.next() * 0.65;
                let seg = 12;
                for k in 0..seg {
                    let (t0, t1) = (k as f32 / seg as f32, (k + 1) as f32 / seg as f32);
                    let a = (x0 + len * t0, y0 + bend * t0 * t0);
                    let b = (x0 + len * t1, y0 + bend * t1 * t1);
                    c.line(a, b, (wid * (1.0 - t0)).max(0.3), (wid * (1.0 - t1)).max(0.3), alpha, Mode::Over);
                }
            }
        }
        Tip::Speckle => {
            for _ in 0..60 {
                let (a, d) = (r.next() * TAU, r.next().powf(0.7) * m * 0.92);
                let alpha = 0.5 + r.next() * 0.5;
                c.circle(m + a.cos() * d, m + a.sin() * d, 1.5 + r.next().powf(2.5) * 10.0, alpha, Mode::Over);
            }
        }
        Tip::Grunge => {
            for y in 0..N {
                for x in 0..N {
                    let (dx, dy) = ((x as f32 - m) / m, (y as f32 - m) / m);
                    let d = (dx * dx + dy * dy).sqrt();
                    let n = fbm(x as f32 / 22.0, y as f32 / 22.0, 41) * 0.6 + noise(x as f32 / 5.0, y as f32 / 5.0, 43) * 0.4;
                    let edge = 1.0 - smoothstep(0.75, 1.0, d + (n - 0.5) * 0.5);
                    c.a[y * N + x] = edge * smoothstep(0.32, 0.6, n);
                }
            }
        }
        Tip::Scale => {
            let r2 = m * 0.92;
            let mut p = Path::new();
            p.move_to(m - r2 * 0.75, m - r2 * 0.72)
                .quad_to(m + r2 * 0.55, m - r2 * 0.85, m + r2, m)
                .quad_to(m + r2 * 0.55, m + r2 * 0.85, m - r2 * 0.75, m + r2 * 0.72)
                .quad_to(m - r2 * 0.45, m, m - r2 * 0.75, m - r2 * 0.72);
            c.fill(&p, 1.0, Mode::Over);
            // slightly lighter middle
            let (gx, gy, gr) = (m - r2 * 0.1, m, r2 * 0.8);
            for y in 0..N {
                for x in 0..N {
                    let t = ((x as f32 + 0.5 - gx).powi(2) + (y as f32 + 0.5 - gy).powi(2)).sqrt() / gr;
                    let g = super::raster::gradient(&[(0.0, 0.35), (0.75, 0.12), (1.0, 0.0)], t);
                    c.a[y * N + x] *= 1.0 - g;
                }
            }
            // the ridge down the middle
            c.line((m - r2 * 0.45, m), (m + r2 * 0.85, m), 5.0, 5.0, 0.6, Mode::Over);
        }
        Tip::Hatch => {
            for i in -6..=6 {
                let off = i as f32 * (m / 6.5) + (r.next() - 0.5) * 4.0;
                let half = (m * m * 0.85 - off * off).max(0.0).sqrt();
                let alpha = 0.55 + r.next() * 0.45;
                let w = 2.0 + r.next() * 2.5;
                let a = (m - half * (0.7 + r.next() * 0.3), m + off);
                let b = (m + half * (0.7 + r.next() * 0.3), m + off + (r.next() - 0.5) * 6.0);
                c.line(a, b, w, w, alpha, Mode::Over);
            }
        }
        Tip::Cloud => {
            for _ in 0..9 {
                let (a, d, rad) = (r.next() * TAU, r.next() * m * 0.45, m * (0.3 + r.next() * 0.25));
                c.radial(m + a.cos() * d, m + a.sin() * d, rad, &[(0.0, 0.8), (1.0, 0.0)], Mode::Over);
            }
        }
        Tip::Sponge => {
            // a natural sea sponge: lumpy outline full of round holes
            for y in 0..N {
                for x in 0..N {
                    let (fx, fy) = (x as f32, y as f32);
                    let d = ((fx - m).powi(2) + (fy - m).powi(2)).sqrt() / m;
                    let edge = 1.0 - smoothstep(0.62, 0.92, d + (fbm(fx / 30.0, fy / 30.0, 51) - 0.5) * 0.45);
                    let holes = noise(fx / 9.0, fy / 9.0, 52) * 0.6 + noise(fx / 4.0, fy / 4.0, 53) * 0.4;
                    c.a[y * N + x] = edge * smoothstep(0.36, 0.5, holes);
                }
            }
        }
        Tip::Feather => {
            // a feather lying along the stroke: shaft with barbs fanning toward the tip
            let (x0, x1) = (m * 0.15, N as f32 - m * 0.1);
            c.line((x0 - 8.0, m), (x1, m), 4.0, 1.5, 1.0, Mode::Over);
            let n_barbs = 60;
            for k in 0..n_barbs {
                let t = k as f32 / n_barbs as f32;
                let x = x0 + (x1 - x0) * t;
                let half = (PI * t).sin().powf(0.7) * m * 0.42 * (0.9 + r.next() * 0.2);
                for side in [-1.0f32, 1.0] {
                    let end = (x + half * 0.7, m + side * half);
                    c.line((x, m), end, 1.6, 0.6, 0.8 + r.next() * 0.2, Mode::Over);
                }
            }
            c.blur_css(0.8);
        }
        Tip::Round | Tip::Shape | Tip::Image => return round_mask(hardness),
    }
    c
}

/// Tip designer shapes (heart, star, drop…); hardness softens the outline.
fn shape_mask(base: ShapeBase, sides: u32, star_inner: f32, hardness: f32) -> Mask {
    let mut c = Mask::new(N);
    let m = N as f32 / 2.0;
    let r = m * 0.92;
    let poly = |n: usize, inner: f32| {
        let k = if inner < 1.0 { n * 2 } else { n };
        let mut p = Path::new();
        for i in 0..k {
            let rr = if inner < 1.0 && i % 2 == 1 { r * inner } else { r };
            let a = -FRAC_PI_2 + i as f32 * TAU / k as f32;
            let (x, y) = (m + a.cos() * rr, m + a.sin() * rr);
            if i == 0 {
                p.move_to(x, y);
            } else {
                p.line_to(x, y);
            }
        }
        p
    };
    match base {
        ShapeBase::Circle => c.fill(&Path::circle(m, m, r), 1.0, Mode::Over),
        ShapeBase::Square => c.fill(&Path::rect(m - r * 0.8, m - r * 0.8, r * 1.6, r * 1.6), 1.0, Mode::Over),
        ShapeBase::Rounded => c.fill(&Path::round_rect(m - r * 0.82, m - r * 0.82, r * 1.64, r * 1.64, r * 0.35), 1.0, Mode::Over),
        ShapeBase::Triangle => c.fill(&poly(3, 1.0), 1.0, Mode::Over),
        ShapeBase::Diamond => c.fill(&poly(4, 1.0), 1.0, Mode::Over),
        ShapeBase::Polygon => c.fill(&poly(sides.clamp(3, 12) as usize, 1.0), 1.0, Mode::Over),
        ShapeBase::Star => c.fill(&poly(sides.clamp(3, 12) as usize, star_inner.clamp(0.1, 0.9)), 1.0, Mode::Over),
        ShapeBase::Heart => {
            let mut p = Path::new();
            p.move_to(m, m + r * 0.85)
                .cubic_to(m - r * 1.25, m + r * 0.05, m - r * 0.75, m - r * 0.95, m, m - r * 0.38)
                .cubic_to(m + r * 0.75, m - r * 0.95, m + r * 1.25, m + r * 0.05, m, m + r * 0.85);
            c.fill(&p, 1.0, Mode::Over);
        }
        ShapeBase::Crescent => {
            c.fill(&Path::circle(m, m, r * 0.9), 1.0, Mode::Over);
            c.fill(&Path::circle(m + r * 0.42, m - r * 0.18, r * 0.78), 1.0, Mode::Out);
        }
        ShapeBase::Drop => {
            let mut p = Path::new();
            p.move_to(m, m - r)
                .cubic_to(m + r * 0.25, m - r * 0.4, m + r * 0.75, m + r * 0.05, m + r * 0.72, m + r * 0.38)
                .arc(m, m + r * 0.38, r * 0.72, 0.0, PI)
                .cubic_to(m - r * 0.75, m + r * 0.05, m - r * 0.25, m - r * 0.4, m, m - r);
            c.fill(&p, 1.0, Mode::Over);
        }
    }
    let soft = (1.0 - hardness.clamp(0.0, 1.0)) * 26.0;
    if soft > 0.3 {
        c.blur_css(soft);
    }
    c
}

// ---------- Edge effects ----------

fn warp(src: &[f32], amp: f32, seed: i32) -> Vec<f32> {
    let mut out = vec![0.0; src.len()];
    let nf = N as f32;
    for y in 0..N {
        for x in 0..N {
            let (u, v) = (x as f32 / nf, y as f32 / nf);
            let sx = (x as f32 + (fbm(u * 2.2, v * 2.2, seed + 71) - 0.5) * 2.0 * amp).round() as i32;
            let sy = (y as f32 + (fbm(u * 2.2 + 9.0, v * 2.2, seed + 73) - 0.5) * 2.0 * amp).round() as i32;
            if sx >= 0 && sy >= 0 && (sx as usize) < N && (sy as usize) < N {
                out[y * N + x] = src[sy as usize * N + sx as usize];
            }
        }
    }
    out
}

/// Gives any tip a new outline, a wet rim, inner grain, bristle streaks, a warp, in-tip texture
/// and the extra tips of dual/triple brushes.
fn shape_edges(base: &Mask, s: &BrushSettings, variant: u32) -> Mask {
    let nf = N as f32;
    let seed = (s.seed as i32) * 101 + variant as i32 * 977;
    let amt = s.edge_amount.clamp(0.0, 1.0);
    let freq = 2.0 + s.edge_detail * 26.0;
    let blur_px = match s.edge {
        Edge::Feathered => 14.0,
        Edge::Torn => 5.0,
        _ => 12.0,
    };
    let mut field = base.clone();
    if !matches!(s.edge, Edge::Smooth | Edge::Dry) {
        field.blur_css(blur_px);
    }
    let mut orig = base.a.clone();
    let mut fa = field.a;
    if s.deform > 0.0 {
        let amp = s.deform * nf * 0.22;
        orig = warp(&orig, amp, seed);
        fa = warp(&fa, amp, seed);
    }
    let soft = ((1.0 - s.hardness) * 0.3).max(0.03);
    let mut out = Mask::new(N);
    for y in 0..N {
        for x in 0..N {
            let i = y * N + x;
            let (u, v) = (x as f32 / nf, y as f32 / nf);
            out.a[i] = match s.edge {
                Edge::Smooth => orig[i],
                Edge::Ragged => smoothstep(0.5 - soft, 0.5 + soft, fa[i] + (fbm(u * freq, v * freq, seed) - 0.5) * amt * 1.3),
                Edge::Torn => smoothstep(0.47, 0.53, fa[i] + (fbm(u * freq * 2.0, v * freq * 2.0, seed) - 0.5) * amt * 1.3),
                Edge::Feathered => smoothstep(0.2, 0.8, fa[i] + (fbm(u * freq * 0.5, v * freq * 0.5, seed) - 0.5) * amt * 0.7),
                Edge::Bristly => {
                    let ang = (v - 0.5).atan2(u - 0.5);
                    let rr = ((u - 0.5).powi(2) + (v - 0.5).powi(2)).sqrt();
                    let n = fbm(ang * freq * 1.3 + 50.0, rr * 3.0, seed);
                    smoothstep(0.5 - soft, 0.5 + soft, fa[i] + (n - 0.5) * amt * 1.4)
                }
                Edge::Dry => {
                    // gaps at the same place in every version, so overlapping dabs keep them
                    let bs = s.seed as i32 * 7;
                    let n = noise(v * freq * 1.6, 0.5, bs + 1) * 0.75 + noise(v * freq * 3.3, 1.5, bs + 2) * 0.25;
                    let keep = smoothstep(amt * 0.6 - 0.08, amt * 0.6 + 0.08, n);
                    orig[i] * keep * keep * (0.85 + 0.15 * noise(u * 3.0, v * 4.0, seed))
                }
            };
        }
    }
    // paint collects at the edge (watercolor): the middle gets lighter
    if s.wet_rim > 0.0 {
        let mut inner = out.clone();
        inner.blur_css(9.0);
        for (o, d) in out.a.iter_mut().zip(inner.a.iter()) {
            *o *= 1.0 - s.wet_rim * 0.65 * smoothstep(0.55, 0.98, *d);
        }
    }
    // grain: tiny holes inside the tip
    if s.grain > 0.0 {
        for y in 0..N {
            for x in 0..N {
                let n = noise(x as f32 / 1.6, y as f32 / 1.6, seed + 7) * 0.5 + noise(x as f32 / 0.8, y as f32 / 0.8, seed + 9) * 0.5;
                out.a[y * N + x] *= 1.0 - s.grain * 0.6 * smoothstep(0.45, 0.85, n);
            }
        }
    }
    // hair / fur / feather textures lie inside the tip, along the stroke
    if s.dir_texture() {
        let tex = textures::get(s.tex_kind);
        let sc = 0.38 / s.tex_scale.max(0.25);
        for y in 0..N {
            for x in 0..N {
                let tx = ((x as f32 * sc) as usize) & 255;
                let ty = ((y as f32 * sc) as usize) & 255;
                let k = 1.0 - (s.texture * 1.15).min(1.0) * tex.a[ty * 256 + tx];
                out.a[y * N + x] *= k * k;
            }
        }
    }
    // bristle marks: grooves across the tip's x axis (bristle presets turn the tip 90°, so these
    // run along the stroke). They depend on the brush seed only, so they line up into long streaks.
    if s.streaks > 0.0 {
        let lines = 14.0 + (s.edge_detail * 26.0).round();
        let ss = s.seed as i32 * 13 + 3;
        for x in 0..N {
            let u = (x as f32 / nf) * lines;
            let groove = smoothstep(0.25, 0.75, noise(u, 0.5, ss) * 0.7 + noise(u * 2.3, 1.5, ss + 1) * 0.3);
            let k0 = 1.0 - (s.streaks * 1.1).min(1.0) * (1.0 - groove) * (0.85 + 0.15 * noise(u, variant as f32 + 3.5, ss + 2));
            let k = k0 * k0;
            for y in 0..N {
                out.a[y * N + x] *= k;
            }
        }
    }
    // dual / triple tips
    let mut r = Rng::new(s.seed.wrapping_mul(31).wrapping_add(variant * 7919));
    for sub in &s.extra_tips {
        apply_sub_tip(&mut out, sub, s, &mut r);
    }
    out
}

fn apply_sub_tip(out: &mut Mask, sub: &SubTip, s: &BrushSettings, r: &mut Rng) {
    let sub_settings = BrushSettings { tip: sub.tip, shape_base: sub.shape_base, hardness: s.hardness, ..BrushSettings::default() };
    let sm = base_mask(sub.tip, &sub_settings);
    let m = N as f32 / 2.0;
    let scale = sub.scale.clamp(0.1, 2.0);
    let (a, d) = (r.next() * TAU, r.next().sqrt() * sub.scatter * m * 0.6);
    let (cx, cy) = (m + a.cos() * d, m + a.sin() * d);
    let rot = r.signed() * PI * sub.angle_jitter;
    let (sn, cs) = (-rot).sin_cos();
    let k = sub.strength.clamp(0.0, 1.0);
    for y in 0..N {
        for x in 0..N {
            // position in the sub tip's own coordinates
            let (dx, dy) = (x as f32 + 0.5 - cx, y as f32 + 0.5 - cy);
            let (lx, ly) = ((dx * cs - dy * sn) / scale + m, (dx * sn + dy * cs) / scale + m);
            let v = sample(&sm, lx, ly);
            let o = &mut out.a[y * N + x];
            match sub.mode {
                SubMode::Mask => *o *= 1.0 - k + k * v,
                SubMode::Add => *o = 1.0 - (1.0 - *o) * (1.0 - v * k),
                SubMode::Cut => *o *= 1.0 - v * k,
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::brush::presets;

    #[test]
    fn every_preset_tip_builds() {
        for p in presets() {
            let v = variant_count(&p.s);
            for k in 0..v.min(2) {
                let t = tip_mask(&p.s, k);
                let sum: f32 = t.mips[0].a.iter().sum();
                assert!(sum > 50.0, "{} has an empty tip", p.name);
                assert!(t.mips[0].a.iter().all(|a| a.is_finite() && (-0.001..=1.001).contains(a)), "{}", p.name);
                assert!(t.mips.last().unwrap().n <= 8);
            }
        }
    }

    #[test]
    fn mip_level_choice() {
        let s = BrushSettings::default();
        let t = tip_mask(&s, 0);
        assert_eq!(t.level(300.0).n, 256);
        assert_eq!(t.level(20.0).n, 32);
        assert_eq!(t.level(2.0).n, 8);
    }
}
