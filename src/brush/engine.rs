// SPDX-License-Identifier: GPL-3.0-or-later
// The stroke engine: pen samples → stabilizer rope → smooth curve → evenly spaced dabs (pressure,
// taper, jitter, scatter, particles, direction) → stroke buffer (or straight onto the layer for
// color-mixing brushes) → whole-stroke edges when the pen lifts → merged into the layer.
// Same behavior as the web version (App.svelte + brush.ts).

use std::collections::HashMap;
use std::f32::consts::{PI, TAU};

use eframe::egui::Color32;

use super::noise::{Rng, fbm, noise, smoothstep};
use super::raster::blur;
use super::textures;
use super::tips::{self, sample};
use super::{BrushSettings, Edge, Ends, MixRefer};
use crate::blend::{self, Px};
use crate::doc::{Document, LayerId, TILE, TileData, TileKey};
use crate::stroke::{LiveStroke, StrokeLook, StrokeTile};

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Sample {
    pub x: f32,
    pub y: f32,
    pub p: f32, // pen pressure 0..1 (mouse: 1)
}

#[derive(Clone, Copy, Debug)]
struct Dab {
    x: f32,
    y: f32,
    p: f32,
    d: f32, // distance from the stroke start
}

/// The composited picture at a document pixel (for "mix with all layers").
pub type SampleAll<'a> = &'a dyn Fn(i32, i32) -> Option<Color32>;

/// What a finished stroke changed (used by undo, next step).
#[allow(dead_code)]
pub struct StrokeResult {
    pub layer: LayerId,
    pub before: HashMap<TileKey, Option<TileData>>,
}

pub struct Stroke {
    s: BrushSettings,
    color: [f32; 3],
    erase: bool,
    zoom: f32,
    layer: LayerId,
    li: usize,
    direct: bool,
    // input smoothing
    rope_end: Sample,
    last: Sample,
    curve_ctrl: Option<Sample>,
    drawn_to: Sample,
    last_raw: Sample,
    // dabs
    since_dab: f32,
    stroke_len: f32,
    cur_d: f32,
    dabs: Vec<Dab>,
    dir_last: Option<(f32, f32, f32)>,
    rng: Rng,
    seed: u32,
    variants: u32,
    bbox: [f32; 4],
    // color mixing
    mix_p: [f32; 4],
    last_sample: (f32, f32),
    recent: Vec<(f32, f32, f32)>, // last particles (x, y, r), for "keep particles apart"
    before: HashMap<TileKey, Option<TileData>>,
}

fn rgb01(c: Color32) -> [f32; 3] {
    [c.r() as f32 / 255.0, c.g() as f32 / 255.0, c.b() as f32 / 255.0]
}

impl Stroke {
    #[allow(clippy::too_many_arguments)]
    pub fn begin(
        doc: &mut Document,
        li: usize,
        s: &BrushSettings,
        color: Color32,
        erase: bool,
        zoom: f32,
        pen: bool,
        at: Sample,
        sample_all: SampleAll,
    ) -> Stroke {
        let layer = doc.layers[li].id;
        let direct = s.mix > 0.0 && !erase;
        let col = rgb01(color);
        if !direct {
            doc.live = Some(LiveStroke {
                layer,
                tiles: HashMap::new(),
                look: StrokeLook {
                    opacity: s.opacity,
                    texture: s.texture,
                    tex_kind: s.tex_kind,
                    tex_scale: s.tex_scale,
                    tex_color: s.tex_color,
                    tex_palette: s.tex_palette,
                    canvas_texture: !s.dir_texture(),
                    pen,
                    color: col,
                    erase,
                    lock_alpha: doc.layers[li].lock_alpha,
                },
            });
        }
        let seed = (at.x.to_bits() ^ at.y.to_bits().rotate_left(7)) | 1;
        let mut st = Stroke {
            s: s.clone(),
            color: col,
            erase,
            zoom: zoom.max(0.01),
            layer,
            li,
            direct,
            rope_end: at,
            last: at,
            curve_ctrl: None,
            drawn_to: at,
            last_raw: at,
            since_dab: 0.0,
            stroke_len: 0.0,
            cur_d: 0.0,
            dabs: Vec::new(),
            dir_last: None,
            rng: Rng::new(seed),
            seed,
            variants: tips::variant_count(s),
            bbox: [f32::MAX, f32::MAX, f32::MIN, f32::MIN],
            mix_p: [col[0], col[1], col[2], 1.0],
            last_sample: (f32::MAX, f32::MAX),
            recent: Vec::new(),
            before: HashMap::new(),
        };
        st.stamp(doc, at, sample_all);
        st
    }

    /// New pen samples (document coordinates).
    pub fn add(&mut self, doc: &mut Document, raw: &[Sample], sample_all: SampleAll) {
        // Stabilizer ("lazy rope", like Krita and CSP): the brush hangs behind the pen on an
        // invisible rope and only moves when the pen pulls it tight. Measured on screen.
        let s = self.s.stabilize.clamp(0.0, 1.0);
        let rope = s.powf(1.5) * 90.0 / self.zoom;
        let k = 1.0 - s * 0.6;
        for &q in raw {
            self.last_raw = q;
            let (dx, dy) = (q.x - self.rope_end.x, q.y - self.rope_end.y);
            let d = (dx * dx + dy * dy).sqrt();
            self.rope_end.p += (q.p - self.rope_end.p) * k.max(0.3);
            if d > rope {
                let t = (d - rope) / d;
                self.rope_end.x += dx * t;
                self.rope_end.y += dy * t;
            } else if rope > 0.0 {
                continue;
            }
            let next = Sample {
                x: self.last.x + (self.rope_end.x - self.last.x) * k,
                y: self.last.y + (self.rope_end.y - self.last.y) * k,
                p: self.rope_end.p,
            };
            self.curve_to(doc, next, sample_all);
            self.last = next;
        }
    }

    /// The pen lifted: finish the line, shape the ends, apply whole-stroke edges, merge.
    pub fn finish(mut self, doc: &mut Document, sample_all: SampleAll) -> StrokeResult {
        if let Some(c) = self.curve_ctrl.take() {
            self.segment(doc, self.drawn_to, c, sample_all);
            self.drawn_to = c;
        }
        if self.s.stabilize > 0.0 {
            // let the stabilized line catch up with where the pen stopped
            let (a, b) = (self.drawn_to, self.last_raw);
            self.segment(doc, a, b, sample_all);
        }
        self.shape_ends(doc, sample_all);
        if self.direct {
            if let Some(i) = doc.index_of(self.layer) {
                doc.layers[i].rev += 1;
            }
            return StrokeResult { layer: self.layer, before: std::mem::take(&mut self.before) };
        }
        if self.s.has_stroke_edges() {
            self.stroke_edges(doc);
        }
        // Remember what the layer looked like, then merge.
        let mut before = HashMap::new();
        if let (Some(live), Some(i)) = (&doc.live, doc.index_of(self.layer)) {
            for key in live.tiles.keys() {
                before.insert(*key, doc.layers[i].tiles.get(key).cloned());
            }
        }
        doc.commit_live();
        StrokeResult { layer: self.layer, before }
    }

    // ---------- Path ----------

    /// Smooth strokes at any speed: the line runs through the middles between samples as curves.
    fn curve_to(&mut self, doc: &mut Document, pt: Sample, sa: SampleAll) {
        let Some(c) = self.curve_ctrl else {
            let mid = mid(self.drawn_to, pt);
            self.segment(doc, self.drawn_to, mid, sa);
            self.drawn_to = mid;
            self.curve_ctrl = Some(pt);
            return;
        };
        let end = mid(c, pt);
        let a = self.drawn_to;
        let len = dist(a, c) + dist(c, end);
        let n = if len < 6.0 { 1 } else { ((len / 4.0).ceil() as usize).min(64) };
        let mut prev = a;
        for i in 1..=n {
            let t = i as f32 / n as f32;
            let u = 1.0 - t;
            let q = Sample {
                x: u * u * a.x + 2.0 * u * t * c.x + t * t * end.x,
                y: u * u * a.y + 2.0 * u * t * c.y + t * t * end.y,
                p: a.p + (end.p - a.p) * t,
            };
            self.segment(doc, prev, q, sa);
            prev = q;
        }
        self.drawn_to = end;
        self.curve_ctrl = Some(pt);
    }

    /// Dabs placed every `spacing` along the line, carried over between pieces.
    fn segment(&mut self, doc: &mut Document, a: Sample, b: Sample, sa: SampleAll) {
        let d_ab = dist(a, b);
        if d_ab <= 1e-6 {
            return;
        }
        let r = self.radius_at(a.p.min(b.p)) * self.taper_at(self.stroke_len, f32::INFINITY);
        let step = (r * 2.0 * self.s.spacing).max(0.3);
        let mut d = (step - self.since_dab).max(0.0);
        while d <= d_ab {
            let t = d / d_ab;
            self.stamp(doc, Sample { x: a.x + (b.x - a.x) * t, y: a.y + (b.y - a.y) * t, p: a.p + (b.p - a.p) * t }, sa);
            d += step;
        }
        self.since_dab = d_ab - (d - step);
    }

    fn stamp(&mut self, doc: &mut Document, pt: Sample, sa: SampleAll) {
        if let Some(prev) = self.dabs.last() {
            self.stroke_len += ((pt.x - prev.x).powi(2) + (pt.y - prev.y).powi(2)).sqrt();
        }
        let dab = Dab { x: pt.x, y: pt.y, p: pt.p, d: self.stroke_len };
        self.dabs.push(dab);
        self.cur_d = dab.d;
        // while drawing only the start can be tapered; the end is tapered when the stroke ends
        let f = self.taper_at(dab.d, f32::INFINITY);
        self.draw_dab(doc, pt, f, sa);
    }

    fn radius_at(&self, p: f32) -> f32 {
        let s = &self.s;
        let f = if s.size_pressure { s.min_size + (1.0 - s.min_size) * p.clamp(0.0, 1.0) } else { 1.0 };
        s.size / 2.0 * f
    }

    fn taper_len(&self) -> f32 {
        (self.s.taper * self.s.size * 15.0).max(4.0)
    }

    /// 1 = full size, smaller towards the tapered tips.
    fn taper_at(&self, d: f32, total: f32) -> f32 {
        if self.s.ends != Ends::Taper {
            return 1.0;
        }
        let l = self.taper_len();
        let ease = |t: f32| 1.0 - (1.0 - t.clamp(0.0, 1.0)).powi(2);
        let tip = 0.06;
        let mut f = tip + (1.0 - tip) * ease(d / l);
        if total.is_finite() {
            f = f.min(tip + (1.0 - tip) * ease((total - d) / l));
        }
        f
    }

    fn direction(&mut self, x: f32, y: f32) -> f32 {
        let (mut a, moved) = match self.dir_last {
            Some((px, py, pa)) => {
                let far = ((x - px).powi(2) + (y - py).powi(2)).sqrt() > 0.5;
                (if far { (y - py).atan2(x - px) } else { pa }, far)
            }
            None => (0.0, true),
        };
        if let (None, Some(d)) = (self.dir_last, self.dabs.first()) {
            if (x - d.x).abs() + (y - d.y).abs() > 0.5 {
                a = (y - d.y).atan2(x - d.x);
            }
        }
        if moved {
            self.dir_last = Some((x, y, a));
        }
        a
    }

    /// Color density random: thicker and thinner patches of paint along the stroke.
    fn density_at(&mut self, d: f32) -> f32 {
        if self.s.opacity_jitter <= 0.0 {
            return 1.0;
        }
        let smooth = noise(d / (self.s.size * 1.3).max(6.0), 0.5, self.s.seed as i32 * 13 + 1);
        let v = smooth * 0.8 + self.rng.next() * 0.2;
        1.0 - self.s.opacity_jitter * 0.9 * v
    }

    /// Lighter/darker and warmer/cooler shades drifting along the stroke.
    fn jitter_color(&mut self, c: [f32; 3], d: f32) -> [f32; 3] {
        let amount = self.s.color_jitter;
        if amount <= 0.0 || self.erase {
            return c;
        }
        let p = (self.s.size * 1.2).max(6.0);
        let seed = self.s.seed as i32 * 7;
        let nl = noise(d / p, 1.5, seed + 2) * 2.0 - 1.0;
        let ns = noise(d / (p * 1.7), 2.5, seed + 3) * 2.0 - 1.0;
        let light = (nl * 0.8 + self.rng.signed() * 0.2) * amount * 80.0 / 255.0;
        let shift = ns * amount * 45.0 / 255.0;
        [(c[0] + light + shift).clamp(0.0, 1.0), (c[1] + light + shift * 0.4).clamp(0.0, 1.0), (c[2] + light - shift).clamp(0.0, 1.0)]
    }

    fn draw_dab(&mut self, doc: &mut Document, pt: Sample, factor: f32, sa: SampleAll) {
        let r = (self.radius_at(pt.p) * factor).max(0.3);
        let dir = self.direction(pt.x, pt.y);
        let press = if self.s.opacity_pressure { 0.25 + 0.75 * pt.p } else { 1.0 };

        // where and how this dab lands
        let s = &self.s;
        let mut size = r;
        if s.size_jitter > 0.0 {
            size *= 1.0 - self.rng.next() * s.size_jitter;
        }
        let mut ang = s.angle.to_radians();
        if s.follows_stroke() {
            ang += dir;
        }
        if s.angle_jitter > 0.0 {
            ang += self.rng.signed() * PI * s.angle_jitter;
        }
        let (mut ox, mut oy) = (0.0, 0.0);
        if s.scatter > 0.0 {
            let off = self.rng.signed() * s.scatter * r * 2.0;
            ox = -dir.sin() * off;
            oy = dir.cos() * off;
        }
        let d = self.cur_d;
        let dens = self.density_at(d);
        let (x, y) = (pt.x + ox, pt.y + oy);

        if self.direct {
            self.mix_dab(doc, x, y, size.max(0.5), ang, press * dens, pt.p, sa);
            return;
        }
        let color = self.jitter_color(self.color, d);
        let alpha = self.s.flow * press * dens;
        if self.s.particles > 0 {
            let gap = self.s.particle_gap.clamp(0.0, 1.0);
            for _ in 0..self.s.particles {
                let pr = (self.s.particle_size * (0.45 + self.rng.next())).max(0.03);
                let rad = (size * pr).max(0.4);
                // "Keep particles apart": try a few spots, keep the first one far enough from
                // the particles placed recently; skip the particle if there is no room.
                let tries = if gap > 0.0 { 10 } else { 1 };
                let mut spot = None;
                for _ in 0..tries {
                    let (pa, pd) = (self.rng.next() * TAU, self.rng.next().sqrt());
                    let (px, py) = (x + pa.cos() * pd * size, y + pa.sin() * pd * size);
                    let free = gap <= 0.0
                        || self.recent.iter().all(|&(qx, qy, qr)| ((px - qx).powi(2) + (py - qy).powi(2)).sqrt() >= (rad + qr) * gap * 1.6);
                    if free {
                        spot = Some((px, py));
                        break;
                    }
                }
                let Some((px, py)) = spot else { continue };
                if gap > 0.0 {
                    self.recent.push((px, py, rad));
                    if self.recent.len() > 96 {
                        self.recent.remove(0);
                    }
                }
                let pang = self.rng.next() * TAU + ang;
                let v = self.pick_variant();
                self.stamp_buffer(doc, px, py, rad, pang, alpha, color, pt.p, v);
            }
        } else {
            let v = self.pick_variant();
            self.stamp_buffer(doc, x, y, size.max(0.4), ang, alpha, color, pt.p, v);
        }
    }

    fn pick_variant(&mut self) -> u32 {
        if self.variants > 1 { (self.rng.next() * self.variants as f32) as u32 % self.variants } else { 0 }
    }

    /// Pixels a dab covers, with its mask value at each. Calls `f(tile_key, index_in_tile, x, y, m)`.
    fn for_dab_pixels(
        s: &BrushSettings,
        w: u32,
        h: u32,
        x: f32,
        y: f32,
        r: f32,
        ang: f32,
        variant: u32,
        mut f: impl FnMut(TileKey, usize, i32, i32, f32),
    ) -> [i32; 4] {
        let tip = tips::tip_mask(s, variant);
        let m = tip.level(r * 2.0);
        let n = m.n as f32;
        let ro = s.roundness.clamp(0.05, 1.0);
        let (sn, cs) = ang.sin_cos();
        let reach = r * 1.42 + 2.0;
        let x0 = ((x - reach).floor() as i32).max(0);
        let y0 = ((y - reach).floor() as i32).max(0);
        let x1 = ((x + reach).ceil() as i32).min(w as i32);
        let y1 = ((y + reach).ceil() as i32).min(h as i32);
        let t = TILE as i32;
        let k = n / (2.0 * r);
        for py in y0..y1 {
            let dy = py as f32 + 0.5 - y;
            for px in x0..x1 {
                let dx = px as f32 + 0.5 - x;
                let lx = dx * cs + dy * sn;
                let ly = (-dx * sn + dy * cs) / ro;
                let (u, v) = (lx * k + n / 2.0, ly * k + n / 2.0);
                if u < -1.0 || v < -1.0 || u > n + 1.0 || v > n + 1.0 {
                    continue;
                }
                let mv = sample(m, u, v);
                if mv <= 0.001 {
                    continue;
                }
                let key = (px.div_euclid(t), py.div_euclid(t));
                let i = (py.rem_euclid(t) * t + px.rem_euclid(t)) as usize;
                f(key, i, px, py, mv);
            }
        }
        [x0, y0, x1, y1]
    }

    #[allow(clippy::too_many_arguments)]
    fn stamp_buffer(&mut self, doc: &mut Document, x: f32, y: f32, r: f32, ang: f32, alpha: f32, color: [f32; 3], p: f32, variant: u32) {
        let (w, h) = (doc.w, doc.h);
        let Some(live) = doc.live.as_mut() else { return };
        let tiles = &mut live.tiles;
        let rect = Self::for_dab_pixels(&self.s, w, h, x, y, r, ang, variant, |key, i, _, _, mv| {
            let t = tiles.entry(key).or_insert_with(StrokeTile::new);
            let m = (mv * alpha).min(1.0);
            let px = &mut t.px[i];
            let k = 1.0 - m;
            *px = [px[0] * k + color[0] * m, px[1] * k + color[1] * m, px[2] * k + color[2] * m, px[3] * k + m];
            if p > t.pr[i] {
                t.pr[i] = p;
            }
        });
        self.grow(rect);
        doc.mark_dirty_rect(rect[0], rect[1], rect[2], rect[3]);
    }

    fn grow(&mut self, r: [i32; 4]) {
        if r[2] <= r[0] || r[3] <= r[1] {
            return;
        }
        self.bbox[0] = self.bbox[0].min(r[0] as f32);
        self.bbox[1] = self.bbox[1].min(r[1] as f32);
        self.bbox[2] = self.bbox[2].max(r[2] as f32);
        self.bbox[3] = self.bbox[3].max(r[3] as f32);
    }

    // ---------- Color mixing (painted straight onto the layer) ----------

    /// Average color (and transparency) under the brush, premultiplied, mixed into the paint.
    fn pick_up(&mut self, doc: &Document, x: f32, y: f32, r: f32, amount: f32, from_before: bool, sa: SampleAll) {
        let sr = (r * 0.3).round().clamp(1.0, 4.0) as i32;
        let (cx, cy) = (x.round() as i32, y.round() as i32);
        let layer = &doc.layers[self.li];
        let t = TILE as i32;
        let (mut acc, mut n) = ([0.0f32; 4], 0.0f32);
        for py in cy - sr..=cy + sr {
            for px in cx - sr..=cx + sr {
                n += 1.0;
                if px < 0 || py < 0 || px >= doc.w as i32 || py >= doc.h as i32 {
                    continue;
                }
                let key = (px.div_euclid(t), py.div_euclid(t));
                let i = (py.rem_euclid(t) * t + px.rem_euclid(t)) as usize;
                let tile = if from_before {
                    match self.before.get(&key) {
                        Some(b) => b.as_ref(),
                        None => layer.tiles.get(&key),
                    }
                } else {
                    layer.tiles.get(&key)
                };
                let mut c = tile.map(|t| blend::to_px(t[i])).unwrap_or([0.0; 4]);
                if self.s.mix_refer == MixRefer::All {
                    if let Some(under) = sa(px, py) {
                        let u = blend::to_px(under);
                        let k = 1.0 - c[3];
                        c = [c[0] + u[0] * k, c[1] + u[1] * k, c[2] + u[2] * k, c[3] + u[3] * k];
                    }
                }
                for j in 0..4 {
                    acc[j] += c[j];
                }
            }
        }
        if n > 0.0 {
            for j in 0..4 {
                self.mix_p[j] += (acc[j] / n - self.mix_p[j]) * amount;
            }
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn mix_dab(&mut self, doc: &mut Document, x: f32, y: f32, r: f32, ang: f32, amount: f32, _p: f32, sa: SampleAll) {
        let s_mix = self.s.mix;
        if ((x - self.last_sample.0).powi(2) + (y - self.last_sample.1).powi(2)).sqrt() >= (r * 0.4).max(1.0) {
            // fresh paint from the brush slowly comes back, unless mix is 100%
            let reload = (1.0 - s_mix) * 0.3;
            let fresh = [self.color[0], self.color[1], self.color[2], 1.0];
            for j in 0..4 {
                self.mix_p[j] += (fresh[j] - self.mix_p[j]) * reload;
            }
            self.pick_up(doc, x, y, r, s_mix * 0.3, s_mix < 0.9, sa);
            self.last_sample = (x, y);
        }
        let a = self.mix_p[3].clamp(0.0, 1.0);
        let mut rgb = if a > 0.001 { [self.mix_p[0] / a, self.mix_p[1] / a, self.mix_p[2] / a] } else { [0.0; 3] };
        rgb = self.jitter_color(rgb.map(|v| v.clamp(0.0, 1.0)), self.cur_d);
        let f = self.s.opacity * self.s.flow * amount;
        let lock_alpha = doc.layers[self.li].lock_alpha;
        let tex = (self.s.texture > 0.0 && !self.s.dir_texture()).then(|| textures::get(self.s.tex_kind));
        let (texture, tex_scale) = (self.s.texture, self.s.tex_scale);
        let v = self.pick_variant();
        let (w, h) = (doc.w, doc.h);
        let li = self.li;
        let before = &mut self.before;
        let layer = &mut doc.layers[li];
        let rect = Self::for_dab_pixels(&self.s, w, h, x, y, r, ang, v, |key, i, px, py, mv| {
            before.entry(key).or_insert_with(|| layer.tiles.get(&key).cloned());
            if lock_alpha && !layer.tiles.contains_key(&key) {
                return;
            }
            let mut m = (mv * f).min(1.0);
            if let Some(t) = tex {
                m *= 1.0 - texture.min(1.0) * textures::sample_a(t, px as f32 + 0.5, py as f32 + 0.5, tex_scale);
            }
            let tile = layer.tile_mut(key);
            let o = blend::to_px(tile[i]);
            let k = 1.0 - m;
            let paint: Px = [rgb[0] * a, rgb[1] * a, rgb[2] * a, a];
            let mut out = [o[0] * k + paint[0] * m, o[1] * k + paint[1] * m, o[2] * k + paint[2] * m, o[3] * k + paint[3] * m];
            if lock_alpha {
                let s = if out[3] > 0.0 { o[3] / out[3] } else { 0.0 };
                out = [out[0] * s, out[1] * s, out[2] * s, o[3]];
            }
            tile[i] = blend::from_px(out);
        });
        self.grow(rect);
        doc.layers[li].rev += 1;
        doc.mark_dirty_rect(rect[0], rect[1], rect[2], rect[3]);
    }

    // ---------- Ends ----------

    fn shape_ends(&mut self, doc: &mut Document, sa: SampleAll) {
        if self.dabs.len() < 2 {
            return;
        }
        match self.s.ends {
            Ends::Taper => {
                // Redraw the whole stroke with both ends tapered.
                self.clear_painted(doc);
                let dabs = std::mem::take(&mut self.dabs);
                let total = self.stroke_len;
                self.rng = Rng::new(self.seed);
                self.dir_last = None;
                self.mix_p = [self.color[0], self.color[1], self.color[2], 1.0];
                self.last_sample = (f32::MAX, f32::MAX);
                self.recent.clear();
                self.dabs = vec![dabs[0]];
                self.cur_d = 0.0;
                let f0 = self.taper_at(0.0, total);
                self.draw_dab(doc, Sample { x: dabs[0].x, y: dabs[0].y, p: dabs[0].p }, f0, sa);
                for w in dabs.windows(2) {
                    let (a, b) = (w[0], w[1]);
                    let ra = self.radius_at(a.p) * self.taper_at(a.d, total);
                    let rb = self.radius_at(b.p) * self.taper_at(b.d, total);
                    let step = (ra.min(rb) * 2.0 * self.s.spacing).max(0.3);
                    let len = ((b.x - a.x).powi(2) + (b.y - a.y).powi(2)).sqrt();
                    let n = ((len / step).ceil() as usize).max(1);
                    for j in 1..=n {
                        let t = j as f32 / n as f32;
                        let d = a.d + (b.d - a.d) * t;
                        self.cur_d = d;
                        let f = self.taper_at(d, total);
                        self.draw_dab(doc, Sample { x: a.x + (b.x - a.x) * t, y: a.y + (b.y - a.y) * t, p: a.p + (b.p - a.p) * t }, f, sa);
                    }
                }
                self.dabs = dabs;
            }
            Ends::Flat if !self.direct => {
                let first = self.dabs[0];
                let last = *self.dabs.last().unwrap();
                let fwd: Vec<Dab> = self.dabs.clone();
                let rev: Vec<Dab> = self.dabs.iter().rev().copied().collect();
                self.cut_flat(doc, first, &fwd);
                self.cut_flat(doc, last, &rev);
            }
            _ => {}
        }
    }

    /// Undo what this stroke painted so far (for the tapered redraw).
    fn clear_painted(&mut self, doc: &mut Document) {
        if self.direct {
            let li = self.li;
            for (key, b) in self.before.drain() {
                match b {
                    Some(t) => {
                        doc.layers[li].tiles.insert(key, t);
                    }
                    None => {
                        doc.layers[li].tiles.remove(&key);
                    }
                }
                doc.mark_tile_dirty(key);
            }
        } else if let Some(live) = doc.live.as_mut() {
            let keys: Vec<TileKey> = live.tiles.keys().copied().collect();
            live.tiles.clear();
            for k in keys {
                doc.mark_tile_dirty(k);
            }
        }
    }

    /// Cuts off the round cap beyond an end point, along the stroke direction.
    fn cut_flat(&mut self, doc: &mut Document, end: Dab, from_end: &[Dab]) {
        let r = self.radius_at(end.p);
        let inner = from_end.iter().find(|q| ((q.x - end.x).powi(2) + (q.y - end.y).powi(2)).sqrt() >= r.max(2.0)).copied().unwrap_or(*from_end.last().unwrap());
        let (dx, dy) = (end.x - inner.x, end.y - inner.y);
        let len = (dx * dx + dy * dy).sqrt();
        if len == 0.0 {
            return;
        }
        let (ux, uy) = (dx / len, dy / len);
        let rr = r + 3.0;
        let Some(live) = doc.live.as_mut() else { return };
        let t = TILE as i32;
        for py in ((end.y - rr * 1.5) as i32).max(0)..((end.y + rr * 1.5) as i32).min(doc.h as i32) {
            for px in ((end.x - rr * 1.5) as i32).max(0)..((end.x + rr * 1.5) as i32).min(doc.w as i32) {
                let (vx, vy) = (px as f32 + 0.5 - end.x, py as f32 + 0.5 - end.y);
                let along = vx * ux + vy * uy;
                let side = (vx * -uy + vy * ux).abs();
                if along > 0.0 && along <= rr && side <= rr {
                    if let Some(tile) = live.tiles.get_mut(&(px.div_euclid(t), py.div_euclid(t))) {
                        let i = (py.rem_euclid(t) * t + px.rem_euclid(t)) as usize;
                        tile.px[i] = [0.0; 4];
                    }
                }
            }
        }
    }

    // ---------- Whole-stroke edges (wet media, outline only, holes, warp) ----------

    fn stroke_edges(&mut self, doc: &mut Document) {
        let s = self.s.clone();
        if self.bbox[0] > self.bbox[2] {
            return;
        }
        let pad = (s.size.max(4.0) * (0.08 + s.deform * 0.35)).ceil() + 2.0;
        let x0 = ((self.bbox[0] - pad).floor() as i32).max(0);
        let y0 = ((self.bbox[1] - pad).floor() as i32).max(0);
        let x1 = ((self.bbox[2] + pad).ceil() as i32).min(doc.w as i32);
        let y1 = ((self.bbox[3] + pad).ceil() as i32).min(doc.h as i32);
        let (w, h) = ((x1 - x0).max(0) as usize, (y1 - y0).max(0) as usize);
        if w == 0 || h == 0 || w * h > 16_000_000 {
            return;
        }
        let Some(live) = doc.live.as_mut() else { return };
        let t = TILE as i32;
        // gather the stroke into one block
        let mut px = vec![[0.0f32; 4]; w * h];
        for (key, tile) in &live.tiles {
            let (tx, ty) = (key.0 * t, key.1 * t);
            for j in 0..t {
                let y = ty + j;
                if y < y0 || y >= y1 {
                    continue;
                }
                for i in 0..t {
                    let x = tx + i;
                    if x < x0 || x >= x1 {
                        continue;
                    }
                    px[(y - y0) as usize * w + (x - x0) as usize] = tile.px[(j * t + i) as usize];
                }
            }
        }
        if s.deform > 0.0 {
            deform(&mut px, w, h, x0, y0, &s);
        }
        let amax = px.iter().map(|p| p[3]).fold(0.0f32, f32::max);
        if amax <= 0.0 {
            return;
        }
        let cov: Vec<f32> = px.iter().map(|p| p[3] / amax).collect();
        let f = edge_factors(&cov, w, h, x0, y0, &s);
        for (p, k) in px.iter_mut().zip(f.iter()) {
            if *k != 1.0 {
                for c in p.iter_mut() {
                    *c *= k;
                }
            }
        }
        // write back
        for y in 0..h {
            for x in 0..w {
                let v = px[y * w + x];
                let (dx, dy) = (x as i32 + x0, y as i32 + y0);
                let key = (dx.div_euclid(t), dy.div_euclid(t));
                let i = (dy.rem_euclid(t) * t + dx.rem_euclid(t)) as usize;
                match live.tiles.get_mut(&key) {
                    Some(tile) => tile.px[i] = v,
                    None if v[3] > 0.0 => {
                        let mut tile = StrokeTile::new();
                        tile.px[i] = v;
                        live.tiles.insert(key, tile);
                    }
                    None => {}
                }
            }
        }
        let keys: Vec<TileKey> = live.tiles.keys().copied().collect();
        for k in keys {
            doc.mark_tile_dirty(k);
        }
    }
}

fn mid(a: Sample, b: Sample) -> Sample {
    Sample { x: (a.x + b.x) / 2.0, y: (a.y + b.y) / 2.0, p: (a.p + b.p) / 2.0 }
}

fn dist(a: Sample, b: Sample) -> f32 {
    ((b.x - a.x).powi(2) + (b.y - a.y).powi(2)).sqrt()
}

/// Moves the paint with a smooth canvas-fixed warp (deform).
fn deform(px: &mut [Px], w: usize, h: usize, x0: i32, y0: i32, s: &BrushSettings) {
    let amp = s.size.max(4.0) * s.deform * 0.32;
    let cell = (s.size * 1.3).max(6.0);
    let seed = s.seed as i32 * 17 + 3;
    let src = px.to_vec();
    for j in 0..h {
        for i in 0..w {
            let (u, v) = ((x0 + i as i32) as f32 / cell, (y0 + j as i32) as f32 / cell);
            let sx = (i as f32 + (fbm(u, v, seed) - 0.5) * 2.0 * amp).round() as i32;
            let sy = (j as f32 + (fbm(u + 7.3, v + 2.1, seed + 1) - 0.5) * 2.0 * amp).round() as i32;
            px[j * w + i] = if sx < 0 || sy < 0 || sx >= w as i32 || sy >= h as i32 { [0.0; 4] } else { src[sy as usize * w + sx as usize] };
        }
    }
}

/// Per pixel, how much of the stroke stays: outline (ragged, torn, feathered, bristly), the
/// watercolor rim, outline-only, edge holes, density patches and grain. `cov` = paint 0..1.
fn edge_factors(cov: &[f32], w: usize, h: usize, x: i32, y: i32, s: &BrushSettings) -> Vec<f32> {
    let mut f = vec![1.0f32; w * h];
    let cover: Vec<f32> = cov.iter().map(|c| (c * 3.0).min(1.0)).collect();
    // CSS blur(r) in the web version = Gaussian with standard deviation r
    let blurred = |r: f32| {
        let mut b = cover.clone();
        blur(&mut b, w, h, r * 2.0);
        b
    };
    let size = s.size.max(4.0);
    let edgy = matches!(s.edge, Edge::Ragged | Edge::Torn | Edge::Feathered | Edge::Bristly);
    let near = edgy.then(|| {
        blurred((size * match s.edge {
            Edge::Feathered => 0.2,
            Edge::Bristly => 0.08,
            _ => 0.12,
        })
        .max(1.5))
    });
    let deep = (s.wet_rim > 0.0).then(|| blurred((size * 0.12).max(2.0)));
    let amt = s.edge_amount.clamp(0.0, 1.0);
    let scale = size * (0.9 - s.edge_detail.clamp(0.0, 1.0) * 0.75);
    let seed = s.seed as i32 * 31 + 5;
    let soft = match s.edge {
        Edge::Torn | Edge::Bristly => 0.05,
        Edge::Feathered => 0.3,
        _ => 0.12,
    };
    let grain = s.grain.clamp(0.0, 1.0);
    let hollow = s.hollow.clamp(0.0, 1.0);
    let inner = (hollow > 0.0).then(|| blurred((size * s.outline_width.clamp(0.05, 0.6) * 0.55).max(1.0)));
    let holes = s.edge_holes.clamp(0.0, 1.0);
    let rim = (holes > 0.0).then(|| blurred((size * (0.08 + holes * 0.32)).max(1.5)));
    let hole_cell = size * (0.08 + holes * 0.45);
    let dens = s.opacity_jitter.clamp(0.0, 1.0);
    let dens_cell = size * 0.8;
    for j in 0..h {
        for i in 0..w {
            let p = j * w + i;
            if cov[p] <= 0.0 {
                continue;
            }
            let (gx, gy) = ((x + i as i32) as f32, (y + j as i32) as f32);
            let mut k = 1.0;
            if let Some(near) = &near {
                let n0 = near[p];
                if n0 <= 0.5 - soft - amt * 1.3 {
                    k = 0.0;
                } else if n0 < 0.5 + soft + amt * 1.3 {
                    let bristly = s.edge == Edge::Bristly;
                    let fine = if bristly { 0.07 } else { 0.18 };
                    let n = (fbm(gx / scale, gy / scale, seed) - 0.5) * if bristly { 0.3 } else { 0.75 }
                        + (noise(gx / (scale * fine), gy / (scale * fine), seed + 3) - 0.5) * if bristly { 1.1 } else { 0.45 };
                    let gain = if matches!(s.edge, Edge::Torn | Edge::Bristly) { 1.6 } else { 1.3 };
                    k = smoothstep(0.5 - soft, 0.5 + soft, n0 + n * amt * gain);
                }
            }
            if let Some(deep) = &deep {
                let dp = deep[p];
                let rimline = smoothstep(0.12, 0.45, dp) * (1.0 - smoothstep(0.55, 0.9, dp));
                k *= (1.0 - s.wet_rim * 0.6 * smoothstep(0.55, 0.97, dp)) * (1.0 + s.wet_rim * 0.6 * rimline);
            }
            if let Some(inner) = &inner {
                k *= 1.0 - hollow * smoothstep(0.72, 0.97, inner[p]);
            }
            if let Some(rim) = &rim {
                let e = 1.0 - smoothstep(0.45, 0.97, rim[p]);
                if e > 0.0 {
                    let n = ((fbm(gx / hole_cell, gy / hole_cell, seed + 41) - 0.5) * 2.4 + 0.5).clamp(0.0, 1.0);
                    let hh = holes * 0.9 * e;
                    k *= smoothstep(hh - 0.04, hh + 0.04, n);
                }
            }
            if dens > 0.0 {
                k *= 1.0 - dens * 0.5 * smoothstep(0.3, 0.75, fbm(gx / dens_cell, gy / dens_cell, seed + 53));
            }
            if grain > 0.0 {
                let g = noise(gx / 1.7, gy / 1.7, seed + 7) * 0.55 + noise(gx / 4.5, gy / 4.5, seed + 9) * 0.45;
                k *= 1.0 - grain * 0.85 * smoothstep(0.42, 0.78, g);
            }
            f[p] = k;
        }
    }
    f
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::brush::presets;

    fn none(_: i32, _: i32) -> Option<Color32> {
        None
    }

    fn line(doc: &mut Document, s: &BrushSettings, erase: bool) -> StrokeResult {
        line_at(doc, s, erase, 200.0)
    }

    fn line_at(doc: &mut Document, s: &BrushSettings, erase: bool, y: f32) -> StrokeResult {
        let start = Sample { x: 100.0, y, p: 0.2 };
        let mut st = Stroke::begin(doc, 0, s, Color32::from_rgb(200, 30, 30), erase, 1.0, true, start, &none);
        let pts: Vec<Sample> =
            (1..=60).map(|i| Sample { x: 100.0 + i as f32 * 6.0, y: y + (i as f32 * 0.2).sin() * 30.0, p: (i as f32 / 60.0).min(1.0) }).collect();
        for c in pts.chunks(5) {
            st.add(doc, c, &none);
        }
        st.finish(doc, &none)
    }

    fn painted(doc: &Document) -> usize {
        doc.layers[0].tiles.values().map(|t| t.iter().filter(|c| c.a() > 0).count()).sum()
    }

    #[test]
    fn every_preset_paints_a_stroke() {
        for p in presets() {
            let mut doc = Document::new(600, 400);
            if p.s.mix >= 0.9 {
                // Blenders add no color of their own: give them paint to smear.
                let base = BrushSettings { size: 80.0, size_pressure: false, stabilize: 0.0, ..BrushSettings::default() };
                line(&mut doc, &base, false);
                let before: Vec<Color32> = doc.layers[0].tiles.values().flat_map(|t| t.iter().copied()).collect();
                line_at(&mut doc, &p.s, false, 250.0); // across the edge of the paint
                let after: Vec<Color32> = doc.layers[0].tiles.values().flat_map(|t| t.iter().copied()).collect();
                assert!(before.len() == after.len() && before != after, "{} didn't change the paint", p.name);
                continue;
            }
            let r = line(&mut doc, &p.s, false);
            assert!(doc.live.is_none(), "{}: live stroke not merged", p.name);
            assert!(painted(&doc) > 100, "{} painted almost nothing ({})", p.name, painted(&doc));
            if p.s.mix == 0.0 {
                assert!(!r.before.is_empty(), "{}", p.name);
            }
        }
    }

    #[test]
    fn particles_kept_apart() {
        let mut doc = Document::new(600, 400);
        let s = BrushSettings {
            size: 80.0,
            particles: 30,
            particle_size: 0.08,
            particle_gap: 1.0,
            spacing: 0.3,
            size_pressure: false,
            stabilize: 0.0,
            ..BrushSettings::default()
        };
        let mut st = Stroke::begin(&mut doc, 0, &s, Color32::BLACK, false, 1.0, false, Sample { x: 100.0, y: 200.0, p: 1.0 }, &none);
        let pts: Vec<Sample> = (1..=20).map(|i| Sample { x: 100.0 + i as f32 * 10.0, y: 200.0, p: 1.0 }).collect();
        st.add(&mut doc, &pts, &none);
        assert!(st.recent.len() > 10, "too few particles placed: {}", st.recent.len());
        for (i, a) in st.recent.iter().enumerate() {
            for b in &st.recent[i + 1..] {
                let d = ((a.0 - b.0).powi(2) + (a.1 - b.1).powi(2)).sqrt();
                assert!(d >= (a.2 + b.2) * 1.6 - 1e-3, "particles too close: {d}");
            }
        }
    }

    #[test]
    fn eraser_removes_paint() {
        let mut doc = Document::new(600, 400);
        let s = BrushSettings { size: 30.0, stabilize: 0.0, ..BrushSettings::default() };
        line(&mut doc, &s, false);
        let before = painted(&doc);
        let e = BrushSettings { size: 60.0, hardness: 1.0, size_pressure: false, stabilize: 0.0, ..BrushSettings::default() };
        line(&mut doc, &e, true);
        assert!(painted(&doc) < before / 4, "{} → {}", before, painted(&doc));
    }

    #[test]
    fn opacity_caps_a_marker() {
        let mut doc = Document::new(600, 400);
        let s = BrushSettings { size: 40.0, opacity: 0.5, size_pressure: false, stabilize: 0.0, spacing: 0.02, ..BrushSettings::default() };
        line(&mut doc, &s, false);
        let max_a = doc.layers[0].tiles.values().flat_map(|t| t.iter().map(|c| c.a())).max().unwrap();
        assert!((120..=134).contains(&max_a), "max alpha {max_a}");
    }

    #[test]
    fn taper_makes_thin_ends() {
        let mut doc = Document::new(600, 400);
        let s = BrushSettings { size: 30.0, ends: Ends::Taper, taper: 0.3, size_pressure: false, stabilize: 0.0, ..BrushSettings::default() };
        line(&mut doc, &s, false);
        // coverage in a column near the start vs the middle of the line
        let col = |x: i32| -> usize {
            (150..250)
                .filter(|&y| {
                    let key = (x / TILE as i32, y / TILE as i32);
                    doc.layers[0].tiles.get(&key).map(|t| t[(y as usize % TILE) * TILE + x as usize % TILE].a() > 128).unwrap_or(false)
                })
                .count()
        };
        assert!(col(104) < col(300), "start {} vs middle {}", col(104), col(300));
    }
}
