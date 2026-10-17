// SPDX-License-Identifier: GPL-3.0-or-later
// The stroke engine: pen samples → stabilizer rope → smooth curve → evenly spaced dabs (pressure,
// taper, jitter, scatter, particles, direction) → stroke buffer (or straight onto the layer for
// color-mixing brushes) → whole-stroke edges when the pen lifts → merged into the layer.
// Same behavior as the web version (App.svelte + brush.ts).

use std::collections::HashMap;
use std::f32::consts::{PI, TAU};

use eframe::egui::Color32;

use super::local::Local;
use super::noise::{Rng, fbm, hash, noise, smoothstep};
use super::raster::blur;
use super::textures;
use super::tips::{self, sample};
use super::{BrushSettings, Dip, Edge, Ends, Fray, MixRefer, Paint, Tip, Transition};
use crate::oklab::{oklab_to_rgb, rgb_to_oklab};
use crate::pigment::{Mixer, Pigment};
use crate::blend::{self, Px};
use crate::doc::{Document, LayerId, TILE, TileData, TileKey};
use crate::stroke::{LiveStroke, StrokeLook, StrokeTile};
use crate::symmetry::Mirror;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Sample {
    pub x: f32,
    pub y: f32,
    pub p: f32, // pen pressure 0..1 (mouse: 1)
}

struct QueuedDot {
    x: f32,
    y: f32,
    r: f32,
    alpha: f32,
    color: [f32; 3],
    p: f32,
    /// Distance along the stroke where it was made.
    d: f32,
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
    /// Real paint: remembers colors while pigments are mixed, and where the brush was last dipped.
    mixer: Mixer,
    last_dip: f32,
    /// How fast the pen moves (pixels per pen sample, smoothed): for the speed color transition.
    speed: f32,
    /// The round soft brush used for dots and threads.
    dot_s: BrushSettings,
    /// Threads (and dots next to a normal stroke) wait here until the main dabs near them are done,
    /// so a later dab doesn't paint over them.
    queue: Vec<QueuedDot>,
    /// Where each strand or thread was last drawn, so the dabs between are joined into a line.
    path_last: HashMap<usize, (f32, f32)>,
    recent: Vec<(f32, f32, f32)>, // last particles (x, y, r), for "keep particles apart"
    before: HashMap<TileKey, Option<TileData>>,
    /// Symmetry: every dab is also drawn at these mirror images.
    mirrors: Vec<Mirror>,
    /// Colors along the stroke from a gradient (instead of one color).
    gradient: Option<crate::gradient::StrokeGradient>,
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
        Self::begin_mirrored(doc, li, s, color, erase, zoom, pen, at, sample_all, Vec::new())
    }

    /// Like `begin`, with symmetry copies.
    #[allow(clippy::too_many_arguments)]
    pub fn begin_mirrored(
        doc: &mut Document,
        li: usize,
        s: &BrushSettings,
        color: Color32,
        erase: bool,
        zoom: f32,
        pen: bool,
        at: Sample,
        sample_all: SampleAll,
        mirrors: Vec<Mirror>,
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
                    tex_image: s.tex_image.clone(),
                    tex_scale: s.tex_scale,
                    tex_color: s.tex_color,
                    tex_palette: s.tex_palette,
                    canvas_texture: !s.dir_texture(),
                    pen,
                    color: col,
                    erase,
                    lock_alpha: doc.layers[li].lock_alpha,
                    paint: s.paint,
                    water: s.water,
                    granulation: s.granulation,
                },
                sel: doc.selection.clone(),
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
            mixer: Mixer::default(),
            last_dip: 0.0,
            speed: 0.0,
            queue: Vec::new(),
            path_last: HashMap::new(),
            dot_s: BrushSettings {
                tip: Tip::Round,
                roundness: 1.0,
                edge: Edge::Smooth,
                edge_amount: 0.0,
                grain: 0.0,
                variants: 1,
                streaks: 0.0,
                deform: 0.0,
                hollow: 0.0,
                edge_holes: 0.0,
                particles: 0,
                wet_rim: 0.0,
                texture: 0.0,
                extra_tips: Vec::new(),
                tex_kind: crate::brush::TexKind::Paper,
                hardness: s.hardness.clamp(0.0, 0.92),
                terminator: 0.0,
                antialias: true,
                ..s.clone()
            },
            recent: Vec::new(),
            before: HashMap::new(),
            mirrors,
            gradient: None,
        };
        st.stamp(doc, at, sample_all);
        st
    }

    /// Color the stroke with a gradient along its length (not for mixing brushes).
    pub fn set_gradient(&mut self, g: Option<crate::gradient::StrokeGradient>) {
        self.gradient = g;
    }

    /// New pen samples (document coordinates).
    pub fn add(&mut self, doc: &mut Document, raw: &[Sample], sample_all: SampleAll) {
        // Stabilizer ("lazy rope", like Krita and CSP): the brush hangs behind the pen on an
        // invisible rope and only moves when the pen pulls it tight. Measured on screen.
        let s = self.s.stabilize.clamp(0.0, 1.0);
        let rope = s.powf(1.5) * 90.0 / self.zoom;
        let k = 1.0 - s * 0.6;
        for &q in raw {
            let moved = ((q.x - self.last_raw.x).powi(2) + (q.y - self.last_raw.y).powi(2)).sqrt();
            self.speed += (moved - self.speed) * 0.25;
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
        self.flush_queue(doc, true);
        if self.direct {
            if let Some(i) = doc.index_of(self.layer) {
                doc.layers[i].rev += 1;
                crate::wet::finish_stroke(doc, i, &self.before, self.bbox, &self.s);
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
        if let Some(i) = doc.index_of(self.layer) {
            if self.erase && self.s.blot {
                crate::wet::blot(doc, i, &before, self.bbox, self.s.size);
            } else if !self.erase {
                crate::wet::finish_stroke(doc, i, &before, self.bbox, &self.s);
            }
        }
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
        if self.in_gap() {
            self.path_last.clear();
            return;
        }
        self.draw_dab(doc, pt, f, sa);
    }

    /// The pen pressure after the brush's own response curve.
    fn curve(&self, p: f32) -> f32 {
        let c = self.s.pressure_curve.clamp(-1.0, 1.0);
        let p = p.clamp(0.0, 1.0);
        if c == 0.0 { p } else { p.powf(2f32.powf(c * 1.5)) }
    }

    fn radius_at(&self, p: f32) -> f32 {
        let s = &self.s;
        let f = if s.size_pressure { s.min_size + (1.0 - s.min_size) * self.curve(p) } else { 1.0 };
        s.size / 2.0 * f
    }

    /// 1 = full width; pinch points make the stroke narrow every `pinch_every` pixels.
    fn pinch_at(&self, d: f32) -> f32 {
        let s = &self.s;
        if s.pinch <= 0.0 {
            return 1.0;
        }
        1.0 - s.pinch.clamp(0.0, 1.0) * 0.9 * (0.5 - 0.5 * (TAU * d / s.pinch_every.max(10.0)).cos())
    }

    /// True while the stroke is in a gap of a dashed line.
    fn in_gap(&self) -> bool {
        let s = &self.s;
        if s.gap_len <= 0.0 {
            return false;
        }
        // dab centers paint for `paint` pixels and rest for gap + size, so the dash you see is
        // `dash_len` long and the gap you see is `gap_len` long
        let paint = (s.dash_len - s.size).max(s.size * s.spacing.max(0.05));
        self.stroke_len % (paint + s.gap_len + s.size) > paint
    }

    /// The brush color moved toward the second color (in OKLab, so the middle stays clean).
    fn transition_color(&self, c: [f32; 3], p: f32, d: f32) -> [f32; 3] {
        let s = &self.s;
        let mut t = match s.transition {
            Transition::Off => return c,
            Transition::Length => d / s.trans_len.max(20.0),
            Transition::Speed => self.speed / (s.size * 0.6).max(4.0),
            Transition::Pressure => self.curve(p),
            Transition::Cycle => 0.5 - 0.5 * (TAU * d / s.trans_len.max(20.0)).cos(),
        }
        .clamp(0.0, 1.0);
        if s.trans_back {
            t = 1.0 - t;
        }
        mix_oklab(c, s.trans_color, t)
    }

    /// Paint running out: the color shifts (darker or lighter, warmer, duller) and thins along the stroke.
    fn drift(&self, c: [f32; 3], d: f32) -> ([f32; 3], f32) {
        let s = &self.s;
        if s.drift == 0.0 || self.erase {
            return (c, 1.0);
        }
        let run = smoothstep(0.0, 1.0, d / s.drift_len.max(50.0));
        let k = s.drift.clamp(-1.0, 1.0) * run;
        let [l, a, b] = rgb_to_oklab(c);
        let (sn, cs) = (0.14 * k).sin_cos();
        let (a2, b2) = (a * cs - b * sn, a * sn + b * cs); // a little warmer (or cooler)
        let dull = 1.0 - 0.3 * k.abs();
        let out = oklab_to_rgb([(l - 0.22 * k).clamp(0.0, 1.0), a2 * dull, b2 * dull]);
        (out, 1.0 - 0.25 * k.max(0.0))
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
        let (ts, te) = (self.s.tip_start.clamp(0.0, 1.0), self.s.tip_end.clamp(0.0, 1.0));
        let mut f = ts + (1.0 - ts) * ease(d / l);
        if total.is_finite() {
            f = f.min(te + (1.0 - te) * ease((total - d) / l));
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
        let r = (self.radius_at(pt.p) * factor * self.pinch_at(self.cur_d)).max(0.3);
        let dir = self.direction(pt.x, pt.y);
        let press = if self.s.opacity_pressure { 0.25 + 0.75 * self.curve(pt.p) } else { 1.0 };

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
        let (base, grad_alpha) = match &self.gradient {
            Some(g) => g.at(d),
            None => (self.color, 1.0),
        };
        let base = self.transition_color(base, pt.p, d);
        let color = self.jitter_color(base, d);
        let (color, thin) = self.drift(color, d);
        let color = warm_cool(color, self.s.tint);
        let alpha = self.s.flow * press * dens * grad_alpha * thin;
        if self.s.dots > 0.0 {
            let dots = self.s.dots.clamp(0.0, 1.0);
            for _ in 0..self.s.dot_count.max(1) {
                let rr = (size * (self.s.dot_min + self.rng.next() * (self.s.dot_max - self.s.dot_min).max(0.0)).max(0.01)).max(0.5);
                let (pa, pd) = (self.rng.next() * TAU, self.rng.next().sqrt() * self.s.dot_spread);
                let (px, py) = (x + pa.cos() * pd * size, y + pa.sin() * pd * size);
                if self.s.dot_cluster > 0.0 {
                    // freckles and blush come in patches: a slow noise decides where dots may land
                    let n = noise(px / (self.s.size * 1.1).max(8.0), py / (self.s.size * 1.1).max(8.0), self.s.seed as i32 + 11);
                    let keep = 1.0 + (smoothstep(0.38, 0.7, n) - 1.0) * self.s.dot_cluster.clamp(0.0, 1.0);
                    if self.rng.next() > keep {
                        continue;
                    }
                }
                let c = if self.s.dot_colors > 0.0 {
                    let t = self.rng.next() * self.s.dot_colors.clamp(0.0, 1.0);
                    shade_oklab(mix_oklab(color, self.s.trans_color, t), (self.rng.signed()) * 0.05 * self.s.dot_colors)
                } else {
                    color
                };
                if dots >= 1.0 {
                    self.stamp_dot(doc, px, py, rr, alpha, c, pt.p);
                } else {
                    self.queue.push(QueuedDot { x: px, y: py, r: rr, alpha, color: c, p: pt.p, d });
                }
            }
            if dots >= 1.0 {
                self.threads(doc, x, y, size, dir, d, color, alpha, pt.p);
                self.flush_queue(doc, false);
                return;
            }
        }
        let alpha = if self.s.dots > 0.0 { alpha * (1.0 - self.s.dots.clamp(0.0, 1.0)) } else { alpha };
        if self.s.strands > 0 {
            self.hair(doc, x, y, size, dir, d, color, alpha, pt.p);
            let fill = self.s.strand_fill.clamp(0.0, 1.0);
            if fill > 0.0 {
                let v = self.pick_variant();
                self.stamp_buffer(doc, x, y, size.max(0.4), ang, alpha * fill, color, pt.p, v);
            }
            self.threads(doc, x, y, size, dir, d, color, alpha, pt.p);
            self.flush_queue(doc, false);
            return;
        }
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
        self.threads(doc, x, y, size, dir, d, color, alpha, pt.p);
        self.flush_queue(doc, false);
    }

    /// Lines and waves inside the stroke: thin threads that follow the stroke and swing sideways.
    #[allow(clippy::too_many_arguments)]
    fn threads(&mut self, _doc: &mut Document, x: f32, y: f32, size: f32, dir: f32, d: f32, color: [f32; 3], alpha: f32, p: f32) {
        let n = self.s.lines.min(12);
        if n == 0 || self.s.line_strength <= 0.0 {
            return;
        }
        let c = shade_oklab(color, self.s.line_shade.clamp(-1.0, 1.0) * 0.3);
        let (nx, ny) = ((dir + PI / 2.0).cos(), (dir + PI / 2.0).sin());
        let lw = (self.s.line_width * size).max(0.5);
        for i in 0..n {
            let v = (i as f32 + 0.5) / n as f32 * 2.0 - 1.0;
            let wave = (TAU * d / self.s.line_len.max(8.0) + i as f32 * 1.7).sin() * self.s.line_wave;
            let off = (v * 0.8 + wave) * size;
            self.queue_path(100 + i as usize, x + nx * off, y + ny * off, lw, alpha * self.s.line_strength, c, p, d);
        }
    }

    /// Strands: many fine hairs along the stroke, in clumps, with curl, wobble, shine and stray hairs.
    #[allow(clippy::too_many_arguments)]
    fn hair(&mut self, doc: &mut Document, x: f32, y: f32, size: f32, dir: f32, d: f32, color: [f32; 3], alpha: f32, p: f32) {
        let _ = doc;
        let n = self.s.strands.min(40) as usize;
        let seed = self.s.seed as i32 * 7 + 3;
        let (nx, ny) = ((dir + PI / 2.0).cos(), (dir + PI / 2.0).sin());
        let clumps = self.s.clump_count.clamp(1, 8) as i32;
        for i in 0..n {
            let hi = |k: i32| hash(i as i32, k, seed);
            // strands begin at different places, so the ends are not lined up
            let start = hi(1) * size * 1.6;
            if d < start {
                self.path_last.remove(&i);
                continue;
            }
            let center = ((i as i32 % clumps) as f32 + 0.5) / clumps as f32 * 2.0 - 1.0;
            let spread = hi(2) * 2.0 - 1.0;
            let tight = center + (hi(3) - 0.5) * 0.35;
            let v = (spread + (tight - spread) * self.s.clump.clamp(0.0, 1.0)).clamp(-1.0, 1.0);
            let wobble = (TAU * d / (60.0 + hi(4) * 120.0) + hi(5) * TAU).sin() * self.s.strand_wobble * 0.25;
            let curl = (TAU * d / self.s.curl_len.max(20.0) + hi(6) * TAU).sin() * self.s.curl * 0.9;
            let off = (v * 0.85 + wobble + curl) * size;
            let along = ((d - start) / (size * 10.0)).min(1.0);
            let w = (self.s.strand_width * size * (0.6 + 0.8 * hi(7)) * (1.0 - 0.45 * along)).max(0.35);
            let band = (d / (size * 3.0)).sin() * 0.5 + hi(8) * 0.2;
            let shine = self.s.sheen.clamp(0.0, 1.0) * 0.22 * (-((v - band) * (v - band)) / 0.08).exp();
            let col = shade_oklab(color, (hi(9) - 0.5) * 0.14 + shine);
            self.queue_path(i, x + nx * off, y + ny * off, w, alpha * (0.75 + 0.25 * hi(10)), col, p, d);
        }
        // stray hairs lifting off the edge
        let fly = self.s.flyaways.clamp(0.0, 1.0);
        if fly > 0.0 && self.rng.next() < fly * 0.05 {
            let side = if self.rng.next() < 0.5 { -1.0 } else { 1.0 };
            let (mut hx, mut hy) = (x + nx * side * size * 0.85, y + ny * side * size * 0.85);
            let ang0 = dir + side * (0.5 + self.rng.next() * 1.0);
            let len = size * (0.7 + self.rng.next() * 1.8);
            let curve = self.rng.signed() * 1.3;
            let steps = ((len / 1.4).ceil() as usize).clamp(4, 120);
            let col = shade_oklab(color, 0.05);
            for k in 0..steps {
                let t = k as f32 / steps as f32;
                let a = ang0 + curve * t;
                hx += a.cos() * 1.4;
                hy += a.sin() * 1.4;
                self.queue.push(QueuedDot { x: hx, y: hy, r: 0.45 + 0.35 * (1.0 - t), alpha: alpha * (0.85 - 0.45 * t), color: col, p, d });
            }
        }
    }

    /// Add a point of a strand or thread, with the points between it and the last one so the line is unbroken.
    #[allow(clippy::too_many_arguments)]
    fn queue_path(&mut self, key: usize, x: f32, y: f32, r: f32, alpha: f32, color: [f32; 3], p: f32, d: f32) {
        if let Some(&(lx, ly)) = self.path_last.get(&key) {
            let dist = ((x - lx).powi(2) + (y - ly).powi(2)).sqrt();
            let step = (r * 0.7).max(0.6);
            if dist > step && dist < r * 60.0 + 40.0 {
                let n = ((dist / step).ceil() as usize).min(200);
                for k in 1..n {
                    let t = k as f32 / n as f32;
                    self.queue.push(QueuedDot { x: lx + (x - lx) * t, y: ly + (y - ly) * t, r, alpha, color, p, d });
                }
            }
        }
        self.path_last.insert(key, (x, y));
        self.queue.push(QueuedDot { x, y, r, alpha, color, p, d });
    }

    /// Paint the waiting dots and threads: those the stroke has moved far enough away from, or all of them.
    fn flush_queue(&mut self, doc: &mut Document, all: bool) {
        let limit = self.cur_d - self.s.size * 1.3;
        let mut keep = Vec::new();
        for q in std::mem::take(&mut self.queue) {
            if all || q.d <= limit {
                self.stamp_dot(doc, q.x, q.y, q.r, q.alpha, q.color, q.p);
            } else {
                keep.push(q);
            }
        }
        self.queue = keep;
    }

    /// A small round soft dab (dots and threads).
    #[allow(clippy::too_many_arguments)]
    fn stamp_dot(&mut self, doc: &mut Document, x: f32, y: f32, r: f32, alpha: f32, color: [f32; 3], p: f32) {
        self.stamp_buffer_with(doc, x, y, r, 0.0, alpha, color, p, 0, true);
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
                let mut mv = sample(m, u, v);
                if s.terminator > 0.0 {
                    // a clear step between light and shade, the softer the lower the setting
                    let half = 0.5 * (1.0 - s.terminator.clamp(0.0, 1.0) * 0.92);
                    mv = smoothstep(0.5 - half, 0.5 + half, mv);
                }
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

    /// Where a dab and its symmetry copies land: (x, y, angle). Copies that fall on the
    /// original (on the axis) are left out so the line doesn't get heavier there.
    fn copies(&self, x: f32, y: f32, r: f32, ang: f32) -> Vec<(f32, f32, f32)> {
        let mut out = vec![(x, y, ang)];
        for m in &self.mirrors {
            let (mx, my) = m.point(x, y);
            if ((mx - x).powi(2) + (my - y).powi(2)).sqrt() < (r * 0.25).max(0.5) {
                continue;
            }
            out.push((mx, my, m.angle(ang)));
        }
        out
    }

    #[allow(clippy::too_many_arguments)]
    fn stamp_buffer(&mut self, doc: &mut Document, x: f32, y: f32, r: f32, ang: f32, alpha: f32, color: [f32; 3], p: f32, variant: u32) {
        self.stamp_buffer_with(doc, x, y, r, ang, alpha, color, p, variant, false);
    }

    #[allow(clippy::too_many_arguments)]
    fn stamp_buffer_with(&mut self, doc: &mut Document, x: f32, y: f32, r: f32, ang: f32, alpha: f32, color: [f32; 3], p: f32, variant: u32, dot: bool) {
        let (w, h) = (doc.w, doc.h);
        for (x, y, ang) in self.copies(x, y, r, ang) {
            let Some(live) = doc.live.as_mut() else { return };
            let tiles = &mut live.tiles;
            let settings = if dot { &self.dot_s } else { &self.s };
            let rect = Self::for_dab_pixels(settings, w, h, x, y, r, ang, variant, |key, i, _, _, mv| {
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
            let avg = [acc[0] / n, acc[1] / n, acc[2] / n, acc[3] / n];
            if self.s.paint != Paint::Normal && avg[3] > 0.001 {
                let rgb = [(avg[0] / avg[3]).clamp(0.0, 1.0), (avg[1] / avg[3]).clamp(0.0, 1.0), (avg[2] / avg[3]).clamp(0.0, 1.0)];
                // real paint keeps its load: picking up from bare canvas changes the color, not how much paint there is
                let old_a = self.mix_p[3];
                let mut m = self.mixer.mix_premult(self.mix_p, avg, &Pigment::new(rgb), amount);
                if m[3] > 0.0 && m[3] < old_a {
                    let k = old_a / m[3];
                    m = [m[0] * k, m[1] * k, m[2] * k, old_a];
                }
                self.mix_p = m;
            } else {
                for j in 0..4 {
                    self.mix_p[j] += (avg[j] - self.mix_p[j]) * amount;
                }
            }
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn mix_dab(&mut self, doc: &mut Document, x: f32, y: f32, r: f32, ang: f32, amount: f32, _p: f32, sa: SampleAll) {
        let s_mix = self.s.mix;
        if ((x - self.last_sample.0).powi(2) + (y - self.last_sample.1).powi(2)).sqrt() >= (r * 0.4).max(1.0) {
            // how much fresh paint comes back: slowly (unless mix is 100%), never, or all of it when dipped
            let reload = match self.s.dip {
                Dip::Gradual => (1.0 - s_mix) * 0.3,
                Dip::Once if self.s.paint != Paint::Normal => 0.0,
                Dip::Once => (1.0 - s_mix) * 0.3,
                Dip::Every => {
                    if self.cur_d - self.last_dip >= self.s.dip_every.max(10.0) {
                        self.last_dip = self.cur_d;
                        1.0
                    } else {
                        0.0
                    }
                }
            };
            let fresh = [self.color[0], self.color[1], self.color[2], 1.0];
            if self.s.paint != Paint::Normal && reload > 0.0 {
                let fp = Pigment::new(self.color);
                self.mix_p = self.mixer.mix_premult(self.mix_p, fresh, &fp, reload);
            } else {
                for j in 0..4 {
                    self.mix_p[j] += (fresh[j] - self.mix_p[j]) * reload;
                }
            }
            self.pick_up(doc, x, y, r, s_mix * 0.3, s_mix < 0.9, sa);
            self.last_sample = (x, y);
        }
        let a = self.mix_p[3].clamp(0.0, 1.0);
        let mut rgb = if a > 0.001 { [self.mix_p[0] / a, self.mix_p[1] / a, self.mix_p[2] / a] } else { [0.0; 3] };
        rgb = self.jitter_color(rgb.map(|v| v.clamp(0.0, 1.0)), self.cur_d);
        // skin and shine: a touch of warmth, light through the skin, gloss
        // a blender deposits again and again, so its warmth is a gentler nudge than a paint brush's
        rgb = warm_cool(rgb, self.s.tint * 0.35);
        rgb = subsurface(rgb, self.s.sss);
        rgb = shade_oklab(rgb, self.s.gloss.clamp(0.0, 1.0) * 0.08);
        let f = self.s.opacity * self.s.flow * amount * (1.0 - 0.55 * self.s.water);
        let gran = self.s.granulation;
        let lock_alpha = doc.layers[self.li].lock_alpha;
        let tex = ((self.s.texture > 0.0 || gran > 0.0) && !self.s.dir_texture()).then(|| textures::for_brush(self.s.tex_kind, &self.s.tex_image));
        let (texture, tex_scale) = (self.s.texture, self.s.tex_scale);
        let v = self.pick_variant();
        let (w, h) = (doc.w, doc.h);
        let li = self.li;
        let sel = doc.selection.clone();
        let real = self.s.paint != Paint::Normal;
        let paint_pg = real.then(|| Pigment::new(rgb));
        let (keep, comb, temp) = (self.s.keep_detail.clamp(0.0, 1.0), self.s.comb.clamp(0.0, 1.0), self.s.temp_blend);
        let local = (keep > 0.0 || comb > 0.0).then(|| Local::build(&doc.layers[self.li], w, h, x, y, r * 1.42 + 2.0, if keep > 0.0 { (r * 0.35).max(1.5) } else { 0.0 }, if comb > 0.0 { (r * 0.6).clamp(3.0, 14.0) } else { 0.0 }));
        // combing: the deposit follows the strands under the brush
        let ang = match (&local, self.s.follow_direction) {
            (Some(l), true) => l.flow.map(|(fx, fy)| fy.atan2(fx)).unwrap_or(ang),
            _ => ang,
        };
        for (x, y, ang) in self.copies(x, y, r, ang) {
            let before = &mut self.before;
            let mixer = &mut self.mixer;
            let layer = &mut doc.layers[li];
            let rect = Self::for_dab_pixels(&self.s, w, h, x, y, r, ang, v, |key, i, px, py, mv| {
                let mut m = (mv * f).min(1.0);
                if let Some(sel) = &sel {
                    match sel.tile(key) {
                        Some(t) if t[i] > 0 => m *= t[i] as f32 / 255.0,
                        _ => return,
                    }
                }
                before.entry(key).or_insert_with(|| layer.tiles.get(&key).cloned());
                if lock_alpha && !layer.tiles.contains_key(&key) {
                    return;
                }
                let mut dens = 1.0;
                if let Some(t) = tex {
                    let sa = textures::sample_a(t, px as f32 + 0.5, py as f32 + 0.5, tex_scale);
                    m *= 1.0 - texture.min(1.0) * sa;
                    if gran > 0.0 {
                        let v = 0.5 - sa;
                        m = (m * (1.0 + gran * 0.6 * v)).clamp(0.0, 1.0);
                        dens = 1.0 + gran * 0.45 * v;
                    }
                }
                let tile = layer.tile_mut(key);
                let o = blend::to_px(tile[i]);
                let k = 1.0 - m;
                let paint: Px = [(rgb[0] * dens).clamp(0.0, 1.0) * a, (rgb[1] * dens).clamp(0.0, 1.0) * a, (rgb[2] * dens).clamp(0.0, 1.0) * a, a];
                let mut paint = paint;
                if let Some(l) = &local {
                    if let Some(c) = l.comb_at(px, py).filter(|c| c[3] > 0.02) {
                        // pulled along the strands: what is already there, smeared in the flow direction
                        paint = [0, 1, 2, 3].map(|j| paint[j] + (c[j] - paint[j]) * comb);
                    }
                }
                let mut out = match &paint_pg {
                    Some(pg) if o[3] > 0.0 && comb == 0.0 => mixer.mix_premult(o, paint, pg, m),
                    _ => [o[0] * k + paint[0] * m, o[1] * k + paint[1] * m, o[2] * k + paint[2] * m, o[3] * k + paint[3] * m],
                };
                if keep > 0.0 && o[3] > 0.0 {
                    // change the soft color, keep the fine texture: the old detail is added back
                    if let Some(lp) = local.as_ref().and_then(|l| l.low_at(px, py)) {
                        let a = out[3];
                        for j in 0..3 {
                            let soft = lp[j] * k + paint[j] * m;
                            out[j] = (soft + (o[j] - lp[j]) * keep).clamp(0.0, a.max(0.0));
                        }
                    }
                }
                if temp && o[3] > 0.0 {
                    // temperature: warm and cool move toward the brush's, the lights and darks stay
                    let old = [o[0] / o[3], o[1] / o[3], o[2] / o[3]].map(|v| v.clamp(0.0, 1.0));
                    let [lo, ao, bo] = crate::oklab::rgb_to_oklab(old);
                    let [_, ap, bp] = crate::oklab::rgb_to_oklab(rgb);
                    let c = crate::oklab::oklab_to_rgb([lo, ao + (ap - ao) * m, bo + (bp - bo) * m]);
                    out = [c[0] * o[3], c[1] * o[3], c[2] * o[3], o[3]];
                }
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
                self.last_dip = 0.0;
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
        self.queue.clear();
        self.path_last.clear();
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
        let mut f = edge_factors(&cov, w, h, x0, y0, &s);
        if s.fray != Fray::Off {
            let angle = match (self.dabs.first(), self.dabs.last()) {
                (Some(a), Some(b)) if (b.x - a.x).abs() + (b.y - a.y).abs() > 4.0 => (b.y - a.y).atan2(b.x - a.x),
                _ => 0.0,
            };
            let g = fray_factors(&cov, w, h, x0, y0, &s, angle);
            for (k, v) in f.iter_mut().zip(g) {
                *k *= v;
            }
        }
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

/// Two colors mixed in OKLab (t = how much of b).
pub fn mix_oklab(a: [f32; 3], b: [f32; 3], t: f32) -> [f32; 3] {
    if t <= 0.0 {
        return a;
    }
    let (la, lb) = (rgb_to_oklab(a), rgb_to_oklab(b));
    oklab_to_rgb([la[0] + (lb[0] - la[0]) * t, la[1] + (lb[1] - la[1]) * t, la[2] + (lb[2] - la[2]) * t])
}

/// A color nudged warmer (positive) or cooler (negative).
pub fn warm_cool(c: [f32; 3], t: f32) -> [f32; 3] {
    if t == 0.0 {
        return c;
    }
    let [l, a, b] = rgb_to_oklab(c);
    let t = t.clamp(-1.0, 1.0);
    oklab_to_rgb([l, a + 0.02 * t, b + 0.035 * t])
}

/// Light through skin: mid tones get a little more red.
pub fn subsurface(c: [f32; 3], amount: f32) -> [f32; 3] {
    if amount <= 0.0 {
        return c;
    }
    let [l, a, b] = rgb_to_oklab(c);
    let bell = (-((l - 0.72) / 0.22).powi(2)).exp();
    oklab_to_rgb([l, a + 0.025 * amount * bell, b - 0.005 * amount * bell])
}

/// A color made lighter (positive) or darker (negative) in OKLab.
pub fn shade_oklab(c: [f32; 3], dl: f32) -> [f32; 3] {
    if dl == 0.0 {
        return c;
    }
    let [l, a, b] = rgb_to_oklab(c);
    oklab_to_rgb([(l + dl).clamp(0.0, 1.0), a, b])
}

/// The edge fray engine, for the whole stroke: factors 0..1 that break up the edge.
fn fray_factors(cov: &[f32], w: usize, h: usize, x: i32, y: i32, s: &BrushSettings, angle: f32) -> Vec<f32> {
    let mut f = vec![1.0f32; w * h];
    let cover: Vec<f32> = cov.iter().map(|c| (c * 3.0).min(1.0)).collect();
    let size = s.size.max(4.0);
    let mut near = cover.clone();
    blur(&mut near, w, h, (size * 0.1).max(1.5) * 2.0);
    let amt = s.fray_amount.clamp(0.0, 1.0);
    let len = s.fray_len.clamp(1.0, 80.0);
    let seed = s.seed as i32 * 17 + 3;
    let (sn, cs) = angle.sin_cos();
    for j in 0..h {
        for i in 0..w {
            let p = j * w + i;
            if cov[p] <= 0.0 {
                continue;
            }
            // how far into the edge band this pixel is: 0.5 = right on the outline
            let n0 = near[p];
            let band = amt * 0.45 + 0.05;
            if n0 >= 0.5 + band {
                continue;
            }
            let (gx, gy) = ((x + i as i32) as f32, (y + j as i32) as f32);
            let k = match s.fray {
                Fray::Off => 1.0,
                Fray::Serrated => {
                    // small teeth: a fast, sharp noise bites into the edge
                    let n = noise(gx / (len * 0.5), gy / (len * 0.5), seed) - 0.5;
                    smoothstep(0.48, 0.52, n0 + n * amt * 1.8)
                }
                Fray::Split => {
                    // streaks along the stroke: noise stretched in the direction of travel
                    let (u, v) = (gx * cs + gy * sn, -gx * sn + gy * cs);
                    let n = noise(u / (len * 9.0), v / (len * 0.45), seed + 5) - 0.5;
                    smoothstep(0.47, 0.53, n0 + n * amt * 2.4)
                }
                Fray::Knotted => {
                    // big lumps and bites, hard edged
                    let n = fbm(gx / (len * 2.5), gy / (len * 2.5), seed + 9) - 0.5;
                    smoothstep(0.495, 0.505, n0 + n * amt * 2.2)
                }
            };
            f[p] = k;
        }
    }
    f
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

    // ---------- real paint ----------

    fn color_line(doc: &mut Document, s: &BrushSettings, color: Color32, y: f32) {
        let mut st = Stroke::begin(doc, 0, s, color, false, 1.0, false, Sample { x: 100.0, y, p: 1.0 }, &none);
        let pts: Vec<Sample> = (1..=80).map(|i| Sample { x: 100.0 + i as f32 * 5.0, y, p: 1.0 }).collect();
        for c in pts.chunks(5) {
            st.add(doc, c, &none);
        }
        st.finish(doc, &none);
    }

    fn solid() -> BrushSettings {
        BrushSettings { size: 50.0, hardness: 1.0, spacing: 0.1, size_pressure: false, stabilize: 0.0, ..BrushSettings::default() }
    }

    fn at(doc: &Document, x: i32, y: i32) -> [f32; 3] {
        let p = crate::files::layer_rgba(&doc.layers[0], x, y, 1, 1).get_pixel(0, 0).0;
        [p[0] as f32 / 255.0, p[1] as f32 / 255.0, p[2] as f32 / 255.0]
    }

    fn chroma(c: [f32; 3]) -> f32 {
        let l = crate::oklab::rgb_to_oklab(c);
        l[1].hypot(l[2])
    }

    const YELLOW: Color32 = Color32::from_rgb(250, 224, 25);
    const BLUE: Color32 = Color32::from_rgb(25, 50, 217);

    #[test]
    fn real_paint_makes_green_where_plain_paint_makes_gray() {
        let result = |paint: Paint| {
            let mut d = Document::new(600, 400);
            color_line(&mut d, &solid(), YELLOW, 200.0);
            color_line(&mut d, &BrushSettings { opacity: 0.5, paint, ..solid() }, BLUE, 200.0);
            at(&d, 300, 200)
        };
        let (plain, real) = (result(Paint::Normal), result(Paint::Mix));
        assert!(real[1] > real[0] + 0.2 && real[1] > real[2] - 0.03, "real paint is green (teal): {real:?}");
        assert!(chroma(real) > chroma(plain) * 1.4, "livelier than plain RGB: {real:?} vs {plain:?}");
        // where there is nothing below, both give exactly the brush color
        let mut d = Document::new(600, 400);
        color_line(&mut d, &BrushSettings { paint: Paint::Mix, ..solid() }, BLUE, 100.0);
        let c = at(&d, 300, 100);
        assert!((c[2] - 217.0 / 255.0).abs() < 0.02, "plain blue on empty canvas: {c:?}");
    }

    #[test]
    fn a_glaze_over_dry_paint_makes_it_deeper() {
        let mut d = Document::new(600, 400);
        color_line(&mut d, &solid(), YELLOW, 200.0);
        let before = at(&d, 300, 200);
        color_line(&mut d, &BrushSettings { opacity: 0.6, paint: Paint::Glaze, ..solid() }, BLUE, 200.0);
        let after = at(&d, 300, 200);
        let l = |c: [f32; 3]| crate::oklab::rgb_to_oklab(c)[0];
        assert!(l(after) < l(before) - 0.1, "deeper: {before:?} -> {after:?}");
        assert!(after[1] > after[2], "a greenish glaze rather than blue paint: {after:?}");
    }

    #[test]
    fn a_mixing_brush_carries_paint_along_as_pigment() {
        let result = |paint: Paint, dip: Dip| {
            let mut d = Document::new(600, 400);
            // a band of yellow, then a blue mixing brush dragged through it
            color_line(&mut d, &BrushSettings { size: 120.0, ..solid() }, YELLOW, 200.0);
            let s = BrushSettings { mix: 0.6, paint, dip, dip_every: 150.0, ..solid() };
            color_line(&mut d, &s, BLUE, 200.0);
            (at(&d, 220, 200), at(&d, 420, 200))
        };
        let (plain_early, _) = result(Paint::Normal, Dip::Once);
        let (real_early, real_late) = result(Paint::Mix, Dip::Once);
        let greenness = |c: [f32; 3]| c[1] - c[0].max(c[2]);
        assert!(greenness(real_early) > greenness(plain_early) + 0.03, "pigment pickup is greener: {real_early:?} vs {plain_early:?}");
        // loaded once, the brush runs out of blue and turns into what it picked up: yellower than a re-dipped brush
        let (_, redipped) = result(Paint::Mix, Dip::Every);
        assert!(redipped[2] > real_late[2] + 0.02, "re-dipping brings the blue back: {redipped:?} vs {real_late:?}");
    }

    #[test]
    fn real_paint_presets_paint_and_old_brushes_are_unchanged() {
        for name in ["Real oil", "Real gouache", "Glaze"] {
            let pr = presets().into_iter().find(|p| p.name == name).unwrap_or_else(|| panic!("{name} exists"));
            assert_ne!(pr.s.paint, Paint::Normal);
            let mut d = Document::new(700, 400);
            color_line(&mut d, &solid(), YELLOW, 200.0);
            line_at(&mut d, &pr.s, false, 200.0);
            assert!(painted(&d) > 100, "{name} paints");
        }
        assert!(presets().iter().filter(|p| !matches!(p.name, "Real oil" | "Real gouache" | "Glaze" | "Wet watercolor" | "Wet ink wash" | "Granulating wash")).all(|p| p.s.paint == Paint::Normal), "nothing else changed");
    }

    // ---------- brush designer v2 ----------

    fn line_p(doc: &mut Document, s: &BrushSettings, color: Color32, y: f32, p: f32, x1: f32) {
        let mut st = Stroke::begin(doc, 0, s, color, false, 1.0, true, Sample { x: 100.0, y, p }, &none);
        let pts: Vec<Sample> = (1..=((x1 - 100.0) / 5.0) as usize).map(|i| Sample { x: 100.0 + i as f32 * 5.0, y, p }).collect();
        for c in pts.chunks(5) {
            st.add(doc, c, &none);
        }
        st.finish(doc, &none);
    }

    fn column(doc: &Document, x: i32, y: i32) -> usize {
        (y - 80..y + 80).filter(|&yy| crate::files::layer_rgba(&doc.layers[0], x, yy, 1, 1).get_pixel(0, 0).0[3] > 128).count()
    }

    fn lum(c: [f32; 3]) -> f32 {
        crate::oklab::rgb_to_oklab(c)[0]
    }

    fn designer() -> BrushSettings {
        BrushSettings { size: 40.0, hardness: 1.0, spacing: 0.08, size_pressure: false, stabilize: 0.0, ..BrushSettings::default() }
    }

    #[test]
    fn pinch_points_narrow_the_stroke_regularly() {
        let mut d = Document::new(700, 300);
        line_p(&mut d, &BrushSettings { pinch: 0.9, pinch_every: 160.0, ..designer() }, BLUE, 150.0, 1.0, 560.0);
        let (narrow, wide) = (column(&d, 180, 150), column(&d, 260, 150));
        assert!(narrow * 2 < wide, "pinched at one place, full at the next: {narrow} vs {wide}");
        assert!(column(&d, 340, 150) * 2 < wide + 4 && column(&d, 340, 150) >= 1, "and pinched again after one more period");
    }

    #[test]
    fn the_pressure_curve_changes_how_hard_you_have_to_press() {
        let width = |curve: f32| {
            let mut d = Document::new(700, 300);
            line_p(&mut d, &BrushSettings { size_pressure: true, min_size: 0.1, pressure_curve: curve, ..designer() }, BLUE, 150.0, 0.5, 400.0);
            column(&d, 250, 150)
        };
        let (light, normal, firm) = (width(-1.0), width(0.0), width(1.0));
        assert!(light > normal && normal > firm, "light touch {light}, normal {normal}, firm {firm}");
    }

    #[test]
    fn start_and_end_width_are_chosen_per_brush() {
        let start = |tip: f32| {
            let mut d = Document::new(700, 300);
            line_p(&mut d, &BrushSettings { ends: Ends::Taper, taper: 0.5, tip_start: tip, ..designer() }, BLUE, 150.0, 1.0, 500.0);
            column(&d, 104, 150)
        };
        assert!(start(0.6) > start(0.06) + 8, "a blunt start versus a pointed one: {} {}", start(0.6), start(0.06));
    }

    #[test]
    fn the_color_moves_toward_the_second_color_along_the_stroke() {
        let mut d = Document::new(800, 300);
        let s = BrushSettings { transition: Transition::Length, trans_color: [0.1, 0.8, 0.3], trans_len: 300.0, ..designer() };
        line_p(&mut d, &s, Color32::from_rgb(220, 30, 30), 150.0, 1.0, 600.0);
        let (a, b) = (at(&d, 115, 150), at(&d, 560, 150));
        assert!(a[0] > 0.7 && a[1] < 0.3, "starts as the brush color: {a:?}");
        assert!(b[1] > 0.55 && b[0] < 0.3, "ends as the second color: {b:?}");
        // the other way round
        let mut d = Document::new(800, 300);
        line_p(&mut d, &BrushSettings { trans_back: true, ..s.clone() }, Color32::from_rgb(220, 30, 30), 150.0, 1.0, 600.0);
        assert!(at(&d, 115, 150)[1] > 0.55, "backwards: starts with the second color");
        // pressure: light touch = brush color, firm = second color
        let by_press = |p: f32| {
            let mut d = Document::new(800, 300);
            line_p(&mut d, &BrushSettings { transition: Transition::Pressure, trans_color: [0.1, 0.8, 0.3], ..designer() }, Color32::from_rgb(220, 30, 30), 150.0, p, 400.0);
            at(&d, 300, 150)
        };
        assert!(by_press(0.05)[0] > 0.6 && by_press(1.0)[1] > 0.6, "pressure picks the color");
    }

    #[test]
    fn paint_that_runs_out_darkens_or_lightens() {
        let end_lum = |drift: f32| {
            let mut d = Document::new(800, 300);
            line_p(&mut d, &BrushSettings { drift, drift_len: 300.0, ..designer() }, Color32::from_rgb(200, 90, 60), 150.0, 1.0, 600.0);
            (lum(at(&d, 130, 150)), lum(at(&d, 560, 150)))
        };
        let (s0, e0) = end_lum(0.0);
        let (sd, ed) = end_lum(0.9);
        let (sl, el) = end_lum(-0.9);
        assert!((s0 - e0).abs() < 0.02 && (sd - s0).abs() < 0.05 && (sl - s0).abs() < 0.05, "all start the same");
        assert!(ed < s0 - 0.1 && el > s0 + 0.1, "darker or lighter at the end: {ed} / {el} vs {s0}");
    }

    #[test]
    fn dots_replace_the_solid_line_and_lines_run_inside_the_stroke() {
        let painted_in = |s: &BrushSettings| {
            let mut d = Document::new(800, 300);
            line_p(&mut d, s, BLUE, 150.0, 1.0, 600.0);
            (d.layers[0].tiles.values().flat_map(|t| t.iter()).filter(|c| c.a() > 100).count(), d)
        };
        let (solid_n, _) = painted_in(&designer());
        let (dots_n, _) = painted_in(&BrushSettings { dots: 1.0, spacing: 0.5, dot_min: 0.05, dot_max: 0.12, dot_count: 2, dot_spread: 0.8, ..designer() });
        assert!(dots_n > 40 && dots_n * 3 < solid_n, "separate dots, far less paint than a solid line: {dots_n} vs {solid_n}");
        // threads: darker lines inside a solid stroke
        let (_, d) = painted_in(&BrushSettings { lines: 3, line_shade: -1.0, line_strength: 1.0, line_width: 0.15, line_wave: 0.0, ..designer() });
        let base = lum(at(&d, 300, 150 - 19));
        let darkest = (-18..18).map(|dy| lum(at(&d, 300, 150 + dy))).fold(1.0, f32::min);
        assert!(darkest < base - 0.08, "a darker thread inside: {darkest} vs {base}");
        // waves: the thread moves up and down along the stroke
        let (_, d) = painted_in(&BrushSettings { lines: 1, line_shade: -1.0, line_strength: 1.0, line_width: 0.12, line_wave: 0.5, line_len: 120.0, ..designer() });
        let dark_y = |x: i32| (-18..18).min_by(|a, b| lum(at(&d, x, 150 + a)).total_cmp(&lum(at(&d, x, 150 + b)))).unwrap();
        let ys: Vec<i32> = (0..8).map(|k| dark_y(150 + k * 15)).collect();
        assert!(ys.iter().max().unwrap() - ys.iter().min().unwrap() > 8, "the thread waves: {ys:?}");
    }

    #[test]
    fn dashes_leave_gaps_and_fray_breaks_the_edge() {
        let mut d = Document::new(800, 300);
        line_p(&mut d, &BrushSettings { dash_len: 40.0, gap_len: 40.0, ..designer() }, BLUE, 150.0, 1.0, 560.0);
        let empty = (110..540).filter(|&x| column(&d, x, 150) == 0).count();
        assert!(empty > 150, "a dashed line has empty stretches: {empty}");
        assert!(column(&d, 101, 150) > 20, "and painted ones");
        // fray: the outline of the stroke is rougher
        let rough = |s: &BrushSettings| {
            let mut d = Document::new(800, 300);
            line_p(&mut d, s, BLUE, 150.0, 1.0, 600.0);
            let tops: Vec<i32> = (150..550)
                .map(|x| (100..200).find(|&y| crate::files::layer_rgba(&d.layers[0], x, y, 1, 1).get_pixel(0, 0).0[3] > 128).unwrap_or(150))
                .collect();
            tops.windows(2).map(|w| (w[0] - w[1]).abs()).sum::<i32>()
        };
        let smooth = rough(&designer());
        for fray in [Fray::Serrated, Fray::Split, Fray::Knotted] {
            let r = rough(&BrushSettings { fray, fray_amount: 0.9, fray_len: 6.0, ..designer() });
            assert!(r > smooth + 10, "{fray:?} is rougher than a smooth edge: {r} vs {smooth}");
        }
    }

    #[test]
    fn new_brushes_exist_and_old_ones_are_unchanged() {
        for name in ["Bark", "Leopard dots", "Skin pores", "Running dry", "Color ribbon", "Speed ink", "Bamboo", "Dashed pen", "Serrated pen", "Split brush", "Worn chalk"] {
            let pr = presets().into_iter().find(|p| p.name == name).unwrap_or_else(|| panic!("{name} exists"));
            let mut d = Document::new(800, 400);
            line_at(&mut d, &pr.s, false, 200.0);
            assert!(painted(&d) > 100, "{name} paints");
        }
        let plain = BrushSettings::default();
        assert_eq!((plain.pinch, plain.dots, plain.lines, plain.gap_len, plain.drift, plain.pressure_curve), (0.0, 0.0, 0, 0.0, 0.0, 0.0));
        assert!(plain.transition == Transition::Off && plain.fray == Fray::Off, "everything is off by default");
    }

    // ---------- hair and skin ----------

    /// Runs of painted pixels down one column (how many separate strands the stroke is made of).
    fn runs(doc: &Document, x: i32, y: i32, alpha: u8) -> usize {
        let mut n = 0;
        let mut inside = false;
        for yy in y - 60..y + 60 {
            let on = crate::files::layer_rgba(&doc.layers[0], x, yy, 1, 1).get_pixel(0, 0).0[3] > alpha;
            if on && !inside {
                n += 1;
            }
            inside = on;
        }
        n
    }

    fn hairy() -> BrushSettings {
        BrushSettings { strands: 8, strand_width: 0.05, clump: 0.0, strand_wobble: 0.0, hardness: 1.0, ..designer() }
    }

    #[test]
    fn strands_make_a_stroke_of_separate_hairs_and_zero_strands_is_solid() {
        let mut d = Document::new(800, 300);
        line_p(&mut d, &BrushSettings { size: 80.0, ..hairy() }, BLUE, 150.0, 1.0, 600.0);
        assert!(runs(&d, 500, 150, 100) >= 4, "several separate strands: {}", runs(&d, 500, 150, 100));
        let mut d = Document::new(800, 300);
        line_p(&mut d, &BrushSettings { size: 80.0, strands: 0, ..hairy() }, BLUE, 150.0, 1.0, 600.0);
        assert_eq!(runs(&d, 500, 150, 100), 1, "no strands: a solid line");
        // an underpaint fills the space between the strands
        let mut d = Document::new(800, 300);
        line_p(&mut d, &BrushSettings { size: 80.0, strand_fill: 1.0, ..hairy() }, BLUE, 150.0, 1.0, 600.0);
        assert_eq!(runs(&d, 500, 150, 100), 1, "full underpaint: no gaps");
    }

    #[test]
    fn curl_makes_a_strand_wind_and_flyaways_leave_the_edge() {
        let single = |curl: f32| {
            let mut d = Document::new(800, 300);
            line_p(&mut d, &BrushSettings { size: 80.0, strands: 1, strand_width: 0.06, curl, curl_len: 100.0, ..hairy() }, BLUE, 150.0, 1.0, 600.0);
            let ys: Vec<i32> = (300..560)
                .step_by(10)
                .filter_map(|x| (60..240).find(|&y| crate::files::layer_rgba(&d.layers[0], x, y, 1, 1).get_pixel(0, 0).0[3] > 100))
                .collect();
            ys.iter().max().unwrap_or(&0) - ys.iter().min().unwrap_or(&0)
        };
        assert!(single(0.9) > single(0.0) + 15, "a curly strand swings across the stroke: {} vs {}", single(0.9), single(0.0));
        // stray hairs reach outside the stroke's own width
        let outside = |fly: f32| {
            let mut d = Document::new(800, 300);
            line_p(&mut d, &BrushSettings { size: 50.0, strands: 3, strand_width: 0.04, flyaways: fly, ..hairy() }, BLUE, 150.0, 1.0, 650.0);
            d.layers[0].tiles.values().enumerate().map(|(_, t)| t.iter().enumerate().filter(|(i, c)| c.a() > 30 && ((*i / TILE) as i32 + 0) > 0).count()).sum::<usize>();
            let mut n = 0;
            for y in 0..300 {
                for x in 100..700 {
                    if (y as i32 - 150).abs() > 30 && crate::files::layer_rgba(&d.layers[0], x, y, 1, 1).get_pixel(0, 0).0[3] > 30 {
                        n += 1;
                    }
                }
            }
            n
        };
        assert!(outside(1.0) > outside(0.0) + 40, "flyaways leave the edge: {} vs {}", outside(1.0), outside(0.0));
    }

    #[test]
    fn shine_runs_across_the_strands() {
        let spread = |sheen: f32| {
            let mut d = Document::new(800, 300);
            line_p(&mut d, &BrushSettings { size: 90.0, strands: 12, strand_width: 0.05, sheen, ..hairy() }, Color32::from_rgb(90, 60, 40), 150.0, 1.0, 650.0);
            let ls: Vec<f32> = (120..650).step_by(3).flat_map(|x| (110..190).filter(move |_| true).map(move |y| (x, y))).filter_map(|(x, y)| {
                let p = crate::files::layer_rgba(&d.layers[0], x, y, 1, 1).get_pixel(0, 0).0;
                (p[3] > 200).then(|| lum([p[0] as f32 / 255.0, p[1] as f32 / 255.0, p[2] as f32 / 255.0]))
            }).collect();
            ls.iter().cloned().fold(0.0, f32::max)
        };
        assert!(spread(1.0) > spread(0.0) + 0.05, "a shine makes some strands lighter: {} vs {}", spread(1.0), spread(0.0));
    }

    fn stripes(doc: &mut Document, band: usize) {
        for y in 0..doc.h as usize {
            for x in 0..doc.w as usize {
                let c = if y / band % 2 == 0 { Color32::from_rgb(220, 40, 40) } else { Color32::from_rgb(30, 40, 200) };
                doc.layers[0].tile_mut(((x / TILE) as i32, (y / TILE) as i32))[(y % TILE) * TILE + x % TILE] = c;
            }
        }
    }

    #[test]
    fn a_comb_blender_pulls_along_the_strands_not_across_them() {
        let contrast_after = |comb: f32| {
            let mut d = Document::new(400, 400);
            stripes(&mut d, 3);
            let s = BrushSettings { size: 60.0, mix: 1.0, comb, hardness: 0.7, spacing: 0.06, flow: 1.0, size_pressure: false, stabilize: 0.0, mix_refer: crate::brush::MixRefer::Layer, ..BrushSettings::default() };
            // a vertical stroke across horizontal stripes
            let mut st = Stroke::begin(&mut d, 0, &s, Color32::BLACK, false, 1.0, false, Sample { x: 200.0, y: 100.0, p: 1.0 }, &none);
            let pts: Vec<Sample> = (1..=60).map(|i| Sample { x: 200.0, y: 100.0 + i as f32 * 4.0, p: 1.0 }).collect();
            for c in pts.chunks(5) {
                st.add(&mut d, c, &none);
            }
            st.finish(&mut d, &none);
            // how different neighbouring rows still are in the middle of the stroke
            (200..260).map(|y| {
                let (a, b) = (at(&d, 200, y), at(&d, 200, y + 3));
                (a[0] - b[0]).abs() + (a[2] - b[2]).abs()
            }).sum::<f32>()
        };
        let (plain, combed) = (contrast_after(0.0), contrast_after(1.0));
        assert!(combed > plain * 1.5, "the combed stripes stay distinct, the plain smudge mixes them: {combed} vs {plain}");
    }

    #[test]
    fn terminator_makes_the_edge_between_light_and_shade_clearer() {
        let edge_width = |term: f32| {
            let mut d = Document::new(500, 300);
            line_p(&mut d, &BrushSettings { size: 100.0, hardness: 0.1, terminator: term, ..designer() }, BLUE, 150.0, 1.0, 400.0);
            // pixels between 10% and 90% paint across the stroke's edge
            (80..160).filter(|&y| {
                let a = crate::files::layer_rgba(&d.layers[0], 300, y, 1, 1).get_pixel(0, 0).0[3];
                (25..230).contains(&a)
            }).count()
        };
        assert!(edge_width(1.0) * 2 < edge_width(0.0), "a clear edge versus the soft airbrush: {} vs {}", edge_width(1.0), edge_width(0.0));
    }

    #[test]
    fn a_skin_blender_keeps_the_pores_and_a_plain_blender_loses_them() {
        let detail_after = |keep: f32| {
            let mut d = Document::new(400, 300);
            // skin with fine noisy pores
            for y in 0..300usize {
                for x in 0..400usize {
                    let n = crate::brush::noise::hash(x as i32, y as i32, 5);
                    let v = 190.0 + (n - 0.5) * 60.0;
                    d.layers[0].tile_mut(((x / TILE) as i32, (y / TILE) as i32))[(y % TILE) * TILE + x % TILE] = Color32::from_rgb(v as u8, (v * 0.78) as u8, (v * 0.68) as u8);
                }
            }
            let s = BrushSettings { size: 80.0, mix: 0.9, keep_detail: keep, hardness: 0.5, spacing: 0.05, size_pressure: false, stabilize: 0.0, mix_refer: crate::brush::MixRefer::Layer, ..BrushSettings::default() };
            let mut st = Stroke::begin(&mut d, 0, &s, Color32::from_rgb(200, 150, 130), false, 1.0, false, Sample { x: 100.0, y: 150.0, p: 1.0 }, &none);
            let pts: Vec<Sample> = (1..=50).map(|i| Sample { x: 100.0 + i as f32 * 4.0, y: 150.0, p: 1.0 }).collect();
            for c in pts.chunks(5) {
                st.add(&mut d, c, &none);
            }
            st.finish(&mut d, &none);
            // variance of the red channel in the middle of the stroke
            let vals: Vec<f32> = (150..250).flat_map(|x| (140..160).map(move |y| (x, y))).map(|(x, y)| at(&d, x, y)[0]).collect();
            let mean = vals.iter().sum::<f32>() / vals.len() as f32;
            vals.iter().map(|v| (v - mean).powi(2)).sum::<f32>() / vals.len() as f32
        };
        let (plain, kept) = (detail_after(0.0), detail_after(1.0));
        assert!(kept > plain * 3.0, "the pores survive the blend: {kept} vs {plain}");
    }

    #[test]
    fn a_temperature_blender_moves_hue_but_not_lightness_and_skin_glow_warms() {
        let mut d = Document::new(400, 300);
        for y in 0..300usize {
            for x in 0..400usize {
                let c = if x < 200 { Color32::from_rgb(215, 150, 120) } else { Color32::from_rgb(150, 160, 200) };
                d.layers[0].tile_mut(((x / TILE) as i32, (y / TILE) as i32))[(y % TILE) * TILE + x % TILE] = c;
            }
        }
        let (l0, l1) = (lum(at(&d, 100, 150)), lum(at(&d, 300, 150)));
        let s = BrushSettings { size: 80.0, mix: 1.0, temp_blend: true, hardness: 0.6, spacing: 0.05, size_pressure: false, stabilize: 0.0, mix_refer: crate::brush::MixRefer::Layer, ..BrushSettings::default() };
        let mut st = Stroke::begin(&mut d, 0, &s, Color32::from_rgb(190, 150, 150), false, 1.0, false, Sample { x: 120.0, y: 150.0, p: 1.0 }, &none);
        let pts: Vec<Sample> = (1..=40).map(|i| Sample { x: 120.0 + i as f32 * 4.0, y: 150.0, p: 1.0 }).collect();
        for c in pts.chunks(5) {
            st.add(&mut d, c, &none);
        }
        st.finish(&mut d, &none);
        let (a, b) = (at(&d, 100, 150), at(&d, 300, 150));
        assert!((lum(a) - l0).abs() < 0.03 && (lum(b) - l1).abs() < 0.03, "lights and darks are kept: {} {} / {l0} {l1}", lum(a), lum(b));
        assert!(a != at(&d, 100, 100) || b != at(&d, 300, 100), "but the colors moved");
        // skin glow: mid tones get a little more red
        let skin = [225.0 / 255.0, 170.0 / 255.0, 150.0 / 255.0];
        let glow = subsurface(skin, 1.0);
        assert!(glow[0] - glow[1] > skin[0] - skin[1], "redder: {glow:?} vs {skin:?}");
        assert!(warm_cool(skin, 1.0)[2] < skin[2] && warm_cool(skin, -1.0)[2] > skin[2], "warm = less blue, cool = more");
    }

    #[test]
    fn freckles_come_in_patches() {
        let patchiness = |cluster: f32| {
            let mut d = Document::new(900, 300);
            let s = BrushSettings { size: 60.0, dots: 1.0, spacing: 0.3, dot_min: 0.05, dot_max: 0.1, dot_count: 4, dot_spread: 1.2, dot_cluster: cluster, size_pressure: false, hardness: 0.8, ..designer() };
            line_p(&mut d, &s, Color32::from_rgb(120, 70, 50), 150.0, 1.0, 850.0);
            // painted pixels in 30x30 cells along the stroke: how uneven are they?
            let counts: Vec<f32> = (0..24)
                .map(|c| {
                    let x0 = 130 + c * 30;
                    (0..60).flat_map(|dx| (0..60).map(move |dy| (dx, dy))).filter(|(dx, dy)| {
                        crate::files::layer_rgba(&d.layers[0], x0 + dx / 2, 120 + dy / 2, 1, 1).get_pixel(0, 0).0[3] > 60
                    }).count() as f32
                })
                .collect();
            let mean = counts.iter().sum::<f32>() / counts.len() as f32;
            (counts.iter().map(|v| (v - mean).powi(2)).sum::<f32>() / counts.len() as f32).sqrt() / mean.max(1.0)
        };
        assert!(patchiness(1.0) > patchiness(0.0) * 1.3, "clustered dots are patchier than an even sprinkle: {} vs {}", patchiness(1.0), patchiness(0.0));
    }

    #[test]
    fn there_are_hair_and_skin_brushes_and_they_all_paint() {
        let hair: Vec<_> = presets().into_iter().filter(|p| p.category == "Hair").collect();
        let skin: Vec<_> = presets().into_iter().filter(|p| p.category == "Skin").collect();
        assert!(hair.len() >= 10 && skin.len() >= 10, "ten ready brushes each: {} hair, {} skin", hair.len(), skin.len());
        for pr in hair.iter().chain(skin.iter()) {
            let mut d = Document::new(800, 400);
            // some paint to blend, for the blenders
            color_line(&mut d, &BrushSettings { size: 60.0, ..solid() }, Color32::from_rgb(190, 130, 100), 200.0);
            let before = painted(&d);
            line_at(&mut d, &pr.s, false, 200.0);
            assert!(painted(&d) >= before, "{} paints or blends", pr.name);
            let mut e = Document::new(800, 400);
            line_at(&mut e, &pr.s, false, 200.0);
            if pr.s.mix < 0.5 {
                assert!(painted(&e) > 30, "{} paints on empty canvas", pr.name);
            }
        }
    }

    // ---------- picture tips ----------

    #[test]
    fn a_picture_tip_stamps_the_picture() {
        // a plus-shaped picture: a bar across and a bar down
        let img = image::RgbaImage::from_fn(60, 60, |x, y| if (26..34).contains(&x) || (26..34).contains(&y) { image::Rgba([0, 0, 0, 255]) } else { image::Rgba([255, 255, 255, 255]) });
        let tip = crate::brush::tipimage::TipImage::from_picture(&img).unwrap();
        let s = BrushSettings { tip: Tip::Image, tip_image: Some(tip), size: 80.0, hardness: 1.0, spacing: 5.0, size_pressure: false, stabilize: 0.0, ..BrushSettings::default() };
        let mut d = Document::new(300, 300);
        let st = Stroke::begin(&mut d, 0, &s, BLUE, false, 1.0, false, Sample { x: 150.0, y: 150.0, p: 1.0 }, &none);
        st.finish(&mut d, &none);
        let alpha = |x: i32, y: i32| crate::files::layer_rgba(&d.layers[0], x, y, 1, 1).get_pixel(0, 0).0[3];
        assert!(alpha(150, 150) > 200, "the middle of the plus");
        assert!(alpha(150, 125) > 200 && alpha(125, 150) > 200, "its arms");
        assert!(alpha(125, 125) < 40 && alpha(175, 175) < 40, "and the empty corners: the picture, not a square");
        // a brush saved with a picture tip comes back painting the same
        let json = crate::brush::userlib::to_json(&[crate::brush::userlib::UserBrush { settings: s.clone(), ..Default::default() }]);
        let back = crate::brush::userlib::from_json(&json).unwrap().remove(0).settings;
        assert_eq!(back.tip_image, s.tip_image);
        assert!(crate::brush::tips::tip_mask(&back, 0).mips[0].a.iter().sum::<f32>() > 100.0);
    }

    #[test]
    fn a_texture_of_your_own_shapes_the_paint_and_old_brushes_are_unchanged() {
        // stripes: dark bars (paint stays) between white gaps (paint is held back), 8 pixels each
        let img = image::RgbaImage::from_fn(64, 64, |x, _| if (x / 8) % 2 == 0 { image::Rgba([0, 0, 0, 255]) } else { image::Rgba([255, 255, 255, 255]) });
        let tex = crate::brush::tipimage::TipImage::from_picture(&img).unwrap();
        let s = BrushSettings { tex_kind: crate::brush::TexKind::Custom, tex_image: Some(tex), texture: 1.0, tex_scale: 0.25, ..designer() };
        let mut d = Document::new(500, 300);
        line_p(&mut d, &s, BLUE, 150.0, 1.0, 400.0);
        let a = |x: i32| crate::files::layer_rgba(&d.layers[0], x, 150, 1, 1).get_pixel(0, 0).0[3];
        let row: Vec<u8> = (200..232).map(|x| a(x)).collect();
        let (hi, lo) = (row.iter().max().unwrap(), row.iter().min().unwrap());
        assert!(*hi > 200 && *hi as i32 - *lo as i32 > 100, "stripes in the paint: {row:?}");
        // without the picture the brush falls back to plain paper
        let mut bad = s.clone();
        bad.tex_image = None;
        assert_eq!(bad.sanitized().tex_kind, crate::brush::TexKind::Paper);
    }
}
