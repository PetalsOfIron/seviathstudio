// SPDX-License-Identifier: GPL-3.0-or-later
// Vector layers (port of the first app's vector.ts): lines are kept as points with pen pressure,
// not as pixels. The layer's pixels are drawn from the lines, so the line width can change later,
// lines can be cut cleanly with the vector eraser, nodes can be moved, and the pressure of every
// point can be edited. Lines keep the order of their points, which later steps (animation,
// inbetweening) need.

use std::collections::HashMap;
use std::sync::Arc;

use eframe::egui::Color32;
use rayon::prelude::*;
use serde::{Deserialize, Serialize};

use crate::blend;
use crate::doc::{Blend, Layer, TILE, TileKey};

/// One point of a line: x, y, pen pressure 0..1.
pub type VPt = [f32; 3];

#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub enum LineEnds {
    Round,
    Taper,
    Flat,
}

#[derive(Clone, Copy, PartialEq, Debug, Serialize, Deserialize)]
pub struct LineStyle {
    pub color: [u8; 3],
    /// width at full pressure, in pixels
    pub size: f32,
    /// 0..1: width at the lightest pressure, as a share of `size`
    pub min_size: f32,
    pub size_pressure: bool,
    pub ends: LineEnds,
    /// how long the pointed ends are
    pub taper: f32,
    /// 0..1
    pub opacity: f32,
}

impl Default for LineStyle {
    fn default() -> Self {
        Self { color: [34, 34, 34], size: 6.0, min_size: 0.15, size_pressure: true, ends: LineEnds::Round, taper: 0.5, opacity: 1.0 }
    }
}

#[derive(Clone, PartialEq, Debug, Serialize, Deserialize)]
pub struct VecStroke {
    pub pts: Vec<VPt>,
    pub style: LineStyle,
}

/// A flat color shape (from Auto-trace): one or more outlines filled with one color.
/// An outline inside another one is a hole.
#[derive(Clone, PartialEq, Debug, Serialize, Deserialize)]
pub struct VecFill {
    pub rings: Vec<Vec<[f32; 2]>>,
    pub color: [u8; 3],
}

#[derive(Clone, PartialEq, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct VecData {
    pub strokes: Vec<VecStroke>,
    /// Line width multiplier for the whole layer (1 = as drawn).
    pub width: f32,
    /// Color shapes, drawn under the lines.
    pub fills: Vec<VecFill>,
}

impl Default for VecData {
    fn default() -> Self {
        Self { strokes: Vec::new(), width: 1.0, fills: Vec::new() }
    }
}

impl VecData {
    /// Project files come from outside: keep numbers sane and sizes bounded.
    pub fn sanitized(mut self) -> Self {
        self.width = if self.width.is_finite() { self.width.clamp(0.05, 20.0) } else { 1.0 };
        self.strokes.truncate(200_000);
        self.strokes.retain_mut(|s| {
            s.pts.truncate(100_000);
            s.pts.retain(|p| p.iter().all(|v| v.is_finite()));
            for p in &mut s.pts {
                p[2] = p[2].clamp(0.0, 1.0);
            }
            let st = &mut s.style;
            st.size = if st.size.is_finite() { st.size.clamp(0.1, 2000.0) } else { 6.0 };
            st.min_size = st.min_size.clamp(0.0, 1.0);
            st.taper = st.taper.clamp(0.0, 3.0);
            st.opacity = st.opacity.clamp(0.0, 1.0);
            s.pts.len() >= 2
        });
        self.fills.truncate(5_000);
        self.fills.retain_mut(|f| {
            f.rings.truncate(20_000);
            f.rings.retain_mut(|r| {
                r.truncate(200_000);
                r.retain(|p| p[0].is_finite() && p[1].is_finite());
                r.len() >= 3
            });
            !f.rings.is_empty()
        });
        self
    }
}

/// The vector data of a layer, shared between history snapshots until it is changed.
pub fn data_mut(layer: &mut Layer) -> &mut VecData {
    Arc::make_mut(layer.vector.get_or_insert_with(|| Arc::new(VecData::default())))
}

pub fn same(a: &Option<Arc<VecData>>, b: &Option<Arc<VecData>>) -> bool {
    match (a, b) {
        (None, None) => true,
        (Some(x), Some(y)) => Arc::ptr_eq(x, y),
        _ => false,
    }
}

// ---------- Shaping a drawn line ----------

/// Ramer-Douglas-Peucker: drops points that change the shape by less than `eps` pixels.
/// Pressure changes count a little, so tapers survive.
pub fn simplify(pts: &[VPt], eps: f32) -> Vec<VPt> {
    if pts.len() <= 2 {
        return pts.to_vec();
    }
    let mut keep = vec![false; pts.len()];
    keep[0] = true;
    keep[pts.len() - 1] = true;
    let mut stack = vec![(0usize, pts.len() - 1)];
    while let Some((a, b)) = stack.pop() {
        let (pa, pb) = (pts[a], pts[b]);
        let (dx, dy) = (pb[0] - pa[0], pb[1] - pa[1]);
        let len = dx.hypot(dy).max(1e-6);
        let (mut worst, mut at) = (-1.0f32, 0usize);
        for i in a + 1..b {
            let q = pts[i];
            let d = ((q[0] - pa[0]) * dy - (q[1] - pa[1]) * dx).abs() / len;
            let pd = (q[2] - (pa[2] + (pb[2] - pa[2]) * (i - a) as f32 / (b - a) as f32)).abs() * 4.0;
            let e = d.max(pd);
            if e > worst {
                worst = e;
                at = i;
            }
        }
        if worst > eps && at > 0 {
            keep[at] = true;
            stack.push((a, at));
            stack.push((at, b));
        }
    }
    pts.iter().zip(keep).filter(|(_, k)| *k).map(|(p, _)| *p).collect()
}

fn round2(v: f32) -> f32 {
    (v * 100.0).round() / 100.0
}

fn rounded(pts: Vec<VPt>) -> Vec<VPt> {
    pts.into_iter().map(|p| [round2(p[0]), round2(p[1]), (p[2] * 1000.0).round() / 1000.0]).collect()
}

/// Raw pen points into a clean line. `smooth` 0..1: 0 keeps your hand exactly, 1 gives very
/// smooth curves with few points.
pub fn clean_line(raw: &[VPt], smooth: f32) -> Vec<VPt> {
    let mut pts: Vec<VPt> = Vec::with_capacity(raw.len());
    for (i, q) in raw.iter().enumerate() {
        if i == 0 || (q[0] - raw[i - 1][0]).hypot(q[1] - raw[i - 1][1]) > 0.05 {
            pts.push(*q);
        }
    }
    if pts.len() == 1 {
        let p = pts[0];
        pts.push([p[0] + 0.01, p[1], p[2]]);
    }
    // even out the shake (the ends stay where you put them)
    for _ in 0..(smooth.clamp(0.0, 1.0) * 10.0).round() as usize {
        let src = pts.clone();
        for i in 1..src.len().saturating_sub(1) {
            for k in 0..3 {
                pts[i][k] = (src[i - 1][k] + 2.0 * src[i][k] + src[i + 1][k]) / 4.0;
            }
        }
    }
    rounded(simplify(&pts, 0.12 + smooth * 1.2))
}

/// A smooth curve through the points (Catmull-Rom), a point about every `step` pixels.
pub fn densify(pts: &[VPt], step: f32) -> Vec<VPt> {
    if pts.len() < 2 {
        return pts.to_vec();
    }
    let n = pts.len();
    let mut out = vec![pts[0]];
    for i in 0..n - 1 {
        let (p0, p1, p2, p3) = (pts[i.saturating_sub(1)], pts[i], pts[i + 1], pts[(i + 2).min(n - 1)]);
        let m = ((p2[0] - p1[0]).hypot(p2[1] - p1[1]) / step).ceil().max(1.0) as usize;
        for j in 1..=m {
            let t = j as f32 / m as f32;
            let (t2, t3) = (t * t, t * t * t);
            let cr = |a: f32, b: f32, c: f32, d: f32| 0.5 * (2.0 * b + (-a + c) * t + (2.0 * a - 5.0 * b + 4.0 * c - d) * t2 + (-a + 3.0 * b - 3.0 * c + d) * t3);
            out.push([cr(p0[0], p1[0], p2[0], p3[0]), cr(p0[1], p1[1], p2[1], p3[1]), p1[2] + (p2[2] - p1[2]) * t]);
        }
    }
    out
}

// ---------- Drawing ----------

/// A round dab on the line: position, radius, and how far along the line it is.
#[derive(Clone, Copy, Debug)]
pub struct Disc {
    pub x: f32,
    pub y: f32,
    pub r: f32,
}

fn taper_factor(d: f32, total: f32, st: &LineStyle) -> f32 {
    if st.ends != LineEnds::Taper {
        return 1.0;
    }
    let l = (st.taper * st.size * 15.0).max(4.0);
    let ease = |t: f32| 1.0 - (1.0 - t.clamp(0.0, 1.0)).powi(2);
    let tip = 0.06;
    let start = tip + (1.0 - tip) * ease(d / l);
    if total.is_finite() { start.min(tip + (1.0 - tip) * ease((total - d) / l)) } else { start }
}

/// The line as round dabs. `live`: the end is not known yet, so only the start is tapered.
pub fn outline(pts: &[VPt], st: &LineStyle, width: f32, live: bool) -> Vec<Disc> {
    let step = (st.size * width / 3.0).clamp(0.75, 2.5);
    let dense = densify(pts, step);
    let mut dist = Vec::with_capacity(dense.len());
    let mut total = 0.0;
    for (i, p) in dense.iter().enumerate() {
        if i > 0 {
            total += (p[0] - dense[i - 1][0]).hypot(p[1] - dense[i - 1][1]);
        }
        dist.push(total);
    }
    let total = if live { f32::INFINITY } else { total };
    dense
        .iter()
        .zip(dist)
        .map(|(p, d)| {
            let f = if st.size_pressure { st.min_size + (1.0 - st.min_size) * p[2] } else { 1.0 };
            Disc { x: p[0], y: p[1], r: (st.size * f / 2.0 * width * taper_factor(d, total, st)).max(0.25) }
        })
        .collect()
}

pub type Bounds = (i32, i32, i32, i32);

pub fn stroke_bounds(s: &VecStroke, width: f32) -> Bounds {
    let (mut x0, mut y0, mut x1, mut y1) = (f32::MAX, f32::MAX, f32::MIN, f32::MIN);
    for p in &s.pts {
        x0 = x0.min(p[0]);
        y0 = y0.min(p[1]);
        x1 = x1.max(p[0]);
        y1 = y1.max(p[1]);
    }
    let pad = s.style.size * width / 2.0 + 3.0;
    ((x0 - pad).floor() as i32, (y0 - pad).floor() as i32, (x1 + pad).ceil() as i32, (y1 + pad).ceil() as i32)
}

pub fn union(a: Option<Bounds>, b: Bounds) -> Bounds {
    match a {
        None => b,
        Some(a) => (a.0.min(b.0), a.1.min(b.1), a.2.max(b.2), a.3.max(b.3)),
    }
}

fn intersects(a: Bounds, b: Bounds) -> bool {
    a.0 < b.2 && b.0 < a.2 && a.1 < b.3 && b.1 < a.3
}

/// Anti-aliased coverage (0..1) of a chain of discs over `clip`.
struct Cov {
    x0: i32,
    y0: i32,
    w: usize,
    h: usize,
    v: Vec<f32>,
}

/// `flat`: no round caps at the two ends of the chain (when `caps` says the ends are real ends).
fn coverage(discs: &[Disc], flat: bool, clip: Bounds) -> Option<Cov> {
    if discs.is_empty() {
        return None;
    }
    let (mut x0, mut y0, mut x1, mut y1) = (f32::MAX, f32::MAX, f32::MIN, f32::MIN);
    for d in discs {
        x0 = x0.min(d.x - d.r);
        y0 = y0.min(d.y - d.r);
        x1 = x1.max(d.x + d.r);
        y1 = y1.max(d.y + d.r);
    }
    let b = ((x0 - 1.0).floor() as i32, (y0 - 1.0).floor() as i32, (x1 + 1.0).ceil() as i32, (y1 + 1.0).ceil() as i32);
    let (bx0, by0, bx1, by1) = (b.0.max(clip.0), b.1.max(clip.1), b.2.min(clip.2), b.3.min(clip.3));
    if bx1 <= bx0 || by1 <= by0 {
        return None;
    }
    let (w, h) = ((bx1 - bx0) as usize, (by1 - by0) as usize);
    let mut v = vec![0f32; w * h];
    // flat ends: everything behind the first dab and beyond the last one is cut off square
    let cut = if flat && discs.len() >= 2 {
        let dir = |a: Disc, b: Disc| {
            let (dx, dy) = (b.x - a.x, b.y - a.y);
            let l = dx.hypot(dy).max(1e-6);
            (dx / l, dy / l)
        };
        let n = discs.len();
        Some((discs[0], dir(discs[0], discs[1]), discs[n - 1], dir(discs[n - 2], discs[n - 1])))
    } else {
        None
    };
    let mut put = |a: Disc, b: Disc| {
        let (sx0, sy0) = (((a.x - a.r).min(b.x - b.r) - 1.0).floor() as i32, ((a.y - a.r).min(b.y - b.r) - 1.0).floor() as i32);
        let (sx1, sy1) = (((a.x + a.r).max(b.x + b.r) + 1.0).ceil() as i32, ((a.y + a.r).max(b.y + b.r) + 1.0).ceil() as i32);
        let (px0, py0, px1, py1) = (sx0.max(bx0), sy0.max(by0), sx1.min(bx1), sy1.min(by1));
        let (dx, dy) = (b.x - a.x, b.y - a.y);
        let l2 = dx * dx + dy * dy;
        for y in py0..py1 {
            let row = (y - by0) as usize * w;
            for x in px0..px1 {
                let (fx, fy) = (x as f32 + 0.5, y as f32 + 0.5);
                let t = if l2 > 1e-9 { ((fx - a.x) * dx + (fy - a.y) * dy) / l2 } else { 0.0 };
                if let Some((d0, u0, d1, u1)) = cut {
                    if (fx - d0.x) * u0.0 + (fy - d0.y) * u0.1 < 0.0 || (fx - d1.x) * u1.0 + (fy - d1.y) * u1.1 > 0.0 {
                        continue;
                    }
                }
                let t = t.clamp(0.0, 1.0);
                let r = a.r + (b.r - a.r) * t;
                let dist = (fx - (a.x + dx * t)).hypot(fy - (a.y + dy * t));
                let c = (r - dist + 0.5).clamp(0.0, 1.0);
                let cell = &mut v[row + (x - bx0) as usize];
                if c > *cell {
                    *cell = c;
                }
            }
        }
    };
    if discs.len() == 1 {
        put(discs[0], discs[0]);
    }
    let n = discs.len();
    for i in 1..n {
        put(discs[i - 1], discs[i]);
    }
    Some(Cov { x0: bx0, y0: by0, w, h, v })
}

/// Put a coverage buffer onto a layer in `color` at `opacity`. Returns false when nothing landed.
fn composite(layer: &mut Layer, cov: &Cov, color: [u8; 3], opacity: f32) {
    let t = TILE as i32;
    let src = [color[0] as f32 / 255.0, color[1] as f32 / 255.0, color[2] as f32 / 255.0];
    let (x1, y1) = (cov.x0 + cov.w as i32, cov.y0 + cov.h as i32);
    let mut touched = false;
    for ty in cov.y0.div_euclid(t)..=(y1 - 1).div_euclid(t) {
        for tx in cov.x0.div_euclid(t)..=(x1 - 1).div_euclid(t) {
            let (bx, by) = (tx * t, ty * t);
            let (px0, py0, px1, py1) = (cov.x0.max(bx), cov.y0.max(by), x1.min(bx + t), y1.min(by + t));
            // anything to paint in this tile?
            let any = (py0..py1).any(|y| {
                let row = (y - cov.y0) as usize * cov.w;
                (px0..px1).any(|x| cov.v[row + (x - cov.x0) as usize] > 0.003)
            });
            if !any {
                continue;
            }
            touched = true;
            let tile = layer.tile_mut((tx, ty));
            for y in py0..py1 {
                let row = (y - cov.y0) as usize * cov.w;
                for x in px0..px1 {
                    let c = cov.v[row + (x - cov.x0) as usize] * opacity;
                    if c <= 0.003 {
                        continue;
                    }
                    let k = ((y - by) * t + (x - bx)) as usize;
                    let out = blend::over(blend::to_px(tile[k]), [src[0] * c, src[1] * c, src[2] * c, c], 1.0, Blend::Normal);
                    tile[k] = blend::from_px(out);
                }
            }
        }
    }
    if touched {
        layer.rev += 1;
    }
}

/// Draw one finished line onto the layer's pixels.
pub fn draw_stroke(layer: &mut Layer, s: &VecStroke, width: f32, clip: Bounds) {
    if s.pts.len() < 2 {
        return;
    }
    let discs = outline(&s.pts, &s.style, width, false);
    if let Some(cov) = coverage(&discs, s.style.ends == LineEnds::Flat, clip) {
        composite(layer, &cov, s.style.color, s.style.opacity);
    }
}

/// Draw part of a line that is still being drawn: the dabs from `from` on.
pub fn draw_live(layer: &mut Layer, discs: &[Disc], from: usize, st: &LineStyle, clip: Bounds) {
    let from = from.saturating_sub(1).min(discs.len().saturating_sub(1));
    if let Some(cov) = coverage(&discs[from..], false, clip) {
        composite(layer, &cov, st.color, st.opacity);
    }
}

/// Erase the layer's pixels inside `bounds` (the vector lines are redrawn there afterwards).
fn clear_region(layer: &mut Layer, bounds: Bounds) {
    let t = TILE as i32;
    let (x0, y0, x1, y1) = bounds;
    if x1 <= x0 || y1 <= y0 {
        return;
    }
    for ty in y0.div_euclid(t)..=(y1 - 1).div_euclid(t) {
        for tx in x0.div_euclid(t)..=(x1 - 1).div_euclid(t) {
            let key = (tx, ty);
            if !layer.tiles.contains_key(&key) {
                continue;
            }
            let (bx, by) = (tx * t, ty * t);
            let (px0, py0, px1, py1) = (x0.max(bx), y0.max(by), x1.min(bx + t), y1.min(by + t));
            let whole = px0 == bx && py0 == by && px1 == bx + t && py1 == by + t;
            if whole {
                layer.tiles.remove(&key);
                continue;
            }
            let tile = layer.tile_mut(key);
            for y in py0..py1 {
                for x in px0..px1 {
                    tile[((y - by) * t + (x - bx)) as usize] = Color32::TRANSPARENT;
                }
            }
            if tile.iter().all(|c| c.a() == 0) {
                layer.tiles.remove(&key);
            }
        }
    }
}

/// Redraw the part of a vector layer inside `bounds` from its lines.
pub fn render_region(layer: &mut Layer, bounds: Bounds) {
    let Some(data) = layer.vector.clone() else {
        clear_region(layer, bounds);
        layer.rev += 1;
        return;
    };
    clear_region(layer, bounds);
    for f in &data.fills {
        if intersects(fill_bounds(f), bounds) {
            draw_fill(layer, f, bounds);
        }
    }
    for s in &data.strokes {
        if intersects(stroke_bounds(s, data.width), bounds) {
            draw_stroke(layer, s, data.width, bounds);
        }
    }
    layer.rev += 1;
}

/// Redraw a whole vector layer.
pub fn render_all(layer: &mut Layer, doc_w: u32, doc_h: u32) {
    render_region(layer, (0, 0, doc_w as i32, doc_h as i32));
}

// ---------- Color shapes ----------

pub fn fill_bounds(f: &VecFill) -> Bounds {
    let (mut x0, mut y0, mut x1, mut y1) = (f32::MAX, f32::MAX, f32::MIN, f32::MIN);
    for r in &f.rings {
        for p in r {
            x0 = x0.min(p[0]);
            y0 = y0.min(p[1]);
            x1 = x1.max(p[0]);
            y1 = y1.max(p[1]);
        }
    }
    ((x0 - 2.0).floor() as i32, (y0 - 2.0).floor() as i32, (x1 + 2.0).ceil() as i32, (y1 + 2.0).ceil() as i32)
}

/// Is (x, y) inside the shape (even-odd, so inner outlines are holes)?
pub fn inside_fill(f: &VecFill, x: f32, y: f32) -> bool {
    let mut inside = false;
    for r in &f.rings {
        let mut j = r.len() - 1;
        for i in 0..r.len() {
            let (a, b) = (r[i], r[j]);
            if (a[1] > y) != (b[1] > y) && x < (b[0] - a[0]) * (y - a[1]) / (b[1] - a[1]) + a[0] {
                inside = !inside;
            }
            j = i;
        }
    }
    inside
}

/// Anti-aliased even-odd coverage of a shape over `clip`.
fn fill_coverage(f: &VecFill, clip: Bounds) -> Option<Cov> {
    let b = fill_bounds(f);
    let (bx0, by0, bx1, by1) = (b.0.max(clip.0), b.1.max(clip.1), b.2.min(clip.2), b.3.min(clip.3));
    if bx1 <= bx0 || by1 <= by0 {
        return None;
    }
    let (w, h) = ((bx1 - bx0) as usize, (by1 - by0) as usize);
    // edges, listed for every pixel row they cross
    let mut edges: Vec<(f32, f32, f32, f32)> = Vec::new(); // (x at top, top y, x at bottom, bottom y)
    for r in &f.rings {
        for i in 0..r.len() {
            let (a, c) = (r[i], r[(i + 1) % r.len()]);
            if a[1] == c[1] {
                continue;
            }
            edges.push(if a[1] < c[1] { (a[0], a[1], c[0], c[1]) } else { (c[0], c[1], a[0], a[1]) });
        }
    }
    let mut rows: Vec<Vec<u32>> = vec![Vec::new(); h];
    for (k, e) in edges.iter().enumerate() {
        let r0 = (e.1.floor() as i32 - by0).max(0);
        let r1 = (e.3.ceil() as i32 - by0).min(h as i32);
        for r in r0..r1 {
            rows[r as usize].push(k as u32);
        }
    }
    const SUB: usize = 4;
    let mut v = vec![0f32; w * h];
    v.par_chunks_mut(w).enumerate().for_each(|(j, row)| {
        let mut xs: Vec<f32> = Vec::new();
        for s in 0..SUB {
            let sy = (by0 + j as i32) as f32 + (s as f32 + 0.5) / SUB as f32;
            xs.clear();
            for &k in &rows[j] {
                let e = edges[k as usize];
                if sy >= e.1 && sy < e.3 {
                    xs.push(e.0 + (e.2 - e.0) * (sy - e.1) / (e.3 - e.1));
                }
            }
            xs.sort_by(|a, b| a.total_cmp(b));
            for pair in xs.chunks_exact(2) {
                let (xa, xb) = ((pair[0] - bx0 as f32).max(0.0), (pair[1] - bx0 as f32).min(w as f32));
                if xb <= xa {
                    continue;
                }
                for (i, c) in row.iter_mut().enumerate().take((xb.ceil() as usize).min(w)).skip(xa.floor() as usize) {
                    *c += (xb.min(i as f32 + 1.0) - xa.max(i as f32)).clamp(0.0, 1.0) / SUB as f32;
                }
            }
        }
        for c in row.iter_mut() {
            // a little extra at the edge hides the thin seams between neighbouring shapes
            *c = (*c * 1.5).min(1.0);
        }
    });
    Some(Cov { x0: bx0, y0: by0, w, h, v })
}

pub fn draw_fill(layer: &mut Layer, f: &VecFill, clip: Bounds) {
    if let Some(cov) = fill_coverage(f, clip) {
        composite(layer, &cov, f.color, 1.0);
    }
}

// ---------- Erasing ----------

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum EraseMode {
    /// Remove only the touched part: the rest stays as clean separate lines.
    Cut,
    /// Remove every line the eraser touches.
    Whole,
}

/// The eraser path as (x, y, radius), filled in so fast movements skip nothing.
fn fill_path(path: &[(f32, f32, f32)]) -> Vec<(f32, f32, f32)> {
    let Some(first) = path.first() else { return Vec::new() };
    let mut out = vec![*first];
    for w in path.windows(2) {
        let (a, b) = (w[0], w[1]);
        let n = ((b.0 - a.0).hypot(b.1 - a.1) / (a.2.min(b.2) * 0.5).max(0.5)).ceil().max(1.0) as usize;
        for j in 1..=n {
            let t = j as f32 / n as f32;
            out.push((a.0 + (b.0 - a.0) * t, a.1 + (b.1 - a.1) * t, a.2 + (b.2 - a.2) * t));
        }
    }
    out
}

/// Run the eraser over the lines. Returns the new list and the area that changed, or None when
/// nothing was touched.
pub fn erase(strokes: &[VecStroke], path: &[(f32, f32, f32)], mode: EraseMode, width: f32) -> Option<(Vec<VecStroke>, Bounds)> {
    let er = fill_path(path);
    if er.is_empty() {
        return None;
    }
    let (mut ex0, mut ey0, mut ex1, mut ey1) = (f32::MAX, f32::MAX, f32::MIN, f32::MIN);
    for e in &er {
        ex0 = ex0.min(e.0 - e.2);
        ey0 = ey0.min(e.1 - e.2);
        ex1 = ex1.max(e.0 + e.2);
        ey1 = ey1.max(e.1 + e.2);
    }
    let ebox = (ex0.floor() as i32, ey0.floor() as i32, ex1.ceil() as i32, ey1.ceil() as i32);
    let mut changed: Option<Bounds> = None;
    let mut out = Vec::with_capacity(strokes.len());
    for s in strokes {
        let sb = stroke_bounds(s, width);
        if !intersects(sb, ebox) {
            out.push(s.clone());
            continue;
        }
        let line_r = s.style.size * width / 2.0;
        let dense = densify(&s.pts, 1.0);
        let hit: Vec<bool> = dense
            .par_iter()
            .map(|q| er.iter().any(|e| (q[0] - e.0).hypot(q[1] - e.1) <= e.2 + if mode == EraseMode::Whole { line_r } else { 0.0 }))
            .collect();
        if !hit.iter().any(|h| *h) {
            out.push(s.clone());
            continue;
        }
        changed = Some(union(changed, sb));
        if mode == EraseMode::Whole {
            continue;
        }
        let mut piece: Vec<VPt> = Vec::new();
        let flush = |piece: &mut Vec<VPt>, out: &mut Vec<VecStroke>| {
            if piece.len() >= 2 {
                out.push(VecStroke { pts: rounded(simplify(piece, 0.2)), style: s.style });
            }
            piece.clear();
        };
        for (q, h) in dense.iter().zip(&hit) {
            if *h {
                flush(&mut piece, &mut out);
            } else {
                piece.push(*q);
            }
        }
        flush(&mut piece, &mut out);
    }
    changed.map(|b| (out, b))
}

// ---------- Editing existing lines ----------

/// The "remove whole lines" eraser also removes color shapes it passes over.
/// Returns the remaining shapes and the area that changed.
pub fn erase_fills(fills: &[VecFill], path: &[(f32, f32, f32)]) -> Option<(Vec<VecFill>, Bounds)> {
    let er = fill_path(path);
    let mut changed: Option<Bounds> = None;
    let mut out = Vec::with_capacity(fills.len());
    for f in fills {
        if er.iter().any(|e| inside_fill(f, e.0, e.1)) {
            changed = Some(union(changed, fill_bounds(f)));
        } else {
            out.push(f.clone());
        }
    }
    changed.map(|b| (out, b))
}

/// The top-most line under (x, y), within `tol` pixels of its edge.
pub fn hit_stroke(strokes: &[VecStroke], x: f32, y: f32, tol: f32, width: f32) -> Option<usize> {
    for (k, s) in strokes.iter().enumerate().rev() {
        let b = stroke_bounds(s, width);
        if x < b.0 as f32 - tol || y < b.1 as f32 - tol || x > b.2 as f32 + tol || y > b.3 as f32 + tol {
            continue;
        }
        let r = s.style.size * width / 2.0 + tol;
        if densify(&s.pts, 2.0).iter().any(|q| (q[0] - x).hypot(q[1] - y) <= r) {
            return Some(k);
        }
    }
    None
}

/// The nodes worth showing as handles: on dense freehand lines only nodes at least `min_dist`
/// apart (plus both ends), so the handles don't cover the line.
pub fn key_nodes(s: &VecStroke, min_dist: f32) -> Vec<usize> {
    let n = s.pts.len();
    if n <= 2 {
        return (0..n).collect();
    }
    let mut out = vec![0];
    let (mut lx, mut ly) = (s.pts[0][0], s.pts[0][1]);
    let end = s.pts[n - 1];
    for i in 1..n - 1 {
        let q = s.pts[i];
        if (q[0] - lx).hypot(q[1] - ly) >= min_dist && (q[0] - end[0]).hypot(q[1] - end[1]) >= min_dist * 0.6 {
            out.push(i);
            lx = q[0];
            ly = q[1];
        }
    }
    out.push(n - 1);
    out
}

/// The node of this line nearest to (x, y) within `tol`, among the handles `key_nodes` shows.
pub fn nearest_node(s: &VecStroke, x: f32, y: f32, tol: f32, spacing: f32) -> Option<usize> {
    let mut best = None;
    let mut bd = tol;
    for k in key_nodes(s, spacing) {
        let d = (s.pts[k][0] - x).hypot(s.pts[k][1] - y);
        if d <= bd {
            bd = d;
            best = Some(k);
        }
    }
    best
}

/// Move node `k` by (dx, dy). With `soft` > 0 the nodes beside it (up to `soft` pixels away along
/// the line) follow smoothly, so freehand lines bend instead of spiking.
pub fn move_node(s: &VecStroke, k: usize, dx: f32, dy: f32, soft: f32) -> VecStroke {
    let mut pts = s.pts.clone();
    let shift = |pts: &mut Vec<VPt>, i: usize, w: f32| {
        pts[i][0] = round2(s.pts[i][0] + dx * w);
        pts[i][1] = round2(s.pts[i][1] + dy * w);
    };
    shift(&mut pts, k, 1.0);
    if soft > 0.0 {
        for step in [-1i32, 1] {
            let mut d = 0.0;
            let mut i = k as i32 + step;
            while i >= 0 && (i as usize) < s.pts.len() {
                let j = (i - step) as usize;
                d += (s.pts[i as usize][0] - s.pts[j][0]).hypot(s.pts[i as usize][1] - s.pts[j][1]);
                if d >= soft {
                    break;
                }
                let t = 1.0 - (d / soft) * (d / soft);
                shift(&mut pts, i as usize, t * t);
                i += step;
            }
        }
    }
    VecStroke { pts, style: s.style }
}

pub fn move_stroke(s: &VecStroke, dx: f32, dy: f32) -> VecStroke {
    VecStroke { pts: s.pts.iter().map(|p| [round2(p[0] + dx), round2(p[1] + dy), p[2]]).collect(), style: s.style }
}

/// The line without node `k` (a line keeps at least 2 nodes).
pub fn delete_node(s: &VecStroke, k: usize) -> VecStroke {
    if s.pts.len() <= 2 {
        return s.clone();
    }
    let mut pts = s.pts.clone();
    pts.remove(k);
    VecStroke { pts, style: s.style }
}

/// A new node on the line where it passes nearest to (x, y). Returns the line and the node's index.
pub fn insert_node(s: &VecStroke, x: f32, y: f32) -> (VecStroke, usize) {
    let (mut best, mut bd, mut bt) = (0usize, f32::MAX, 0.0f32);
    for i in 0..s.pts.len().saturating_sub(1) {
        let (a, b) = (s.pts[i], s.pts[i + 1]);
        let (vx, vy) = (b[0] - a[0], b[1] - a[1]);
        let l2 = (vx * vx + vy * vy).max(1e-9);
        let t = (((x - a[0]) * vx + (y - a[1]) * vy) / l2).clamp(0.0, 1.0);
        let d = (a[0] + vx * t - x).hypot(a[1] + vy * t - y);
        if d < bd {
            bd = d;
            best = i;
            bt = t;
        }
    }
    let (a, b) = (s.pts[best], s.pts[(best + 1).min(s.pts.len() - 1)]);
    let mut pts = s.pts.clone();
    pts.insert(best + 1, [round2(a[0] + (b[0] - a[0]) * bt), round2(a[1] + (b[1] - a[1]) * bt), a[2] + (b[2] - a[2]) * bt]);
    (VecStroke { pts, style: s.style }, best + 1)
}

/// Smoother curve (amount 0..1).
pub fn smooth_stroke(s: &VecStroke, amount: f32) -> VecStroke {
    VecStroke { pts: clean_line(&densify(&s.pts, 3.0), amount), style: s.style }
}

/// Fewer nodes, the same look.
pub fn simplify_stroke(s: &VecStroke, eps: f32) -> VecStroke {
    VecStroke { pts: rounded(simplify(&s.pts, eps)), style: s.style }
}

/// A node every `step` pixels, so the pressure can be edited finely.
pub fn densify_stroke(s: &VecStroke, step: f32) -> VecStroke {
    VecStroke { pts: rounded(densify(&s.pts, step)), style: s.style }
}

pub fn set_pressures(s: &VecStroke, pressures: &[f32]) -> VecStroke {
    let mut pts = s.pts.clone();
    for (p, v) in pts.iter_mut().zip(pressures) {
        p[2] = (v.clamp(0.0, 1.0) * 1000.0).round() / 1000.0;
    }
    VecStroke { pts, style: s.style }
}

/// Where each node sits along the line, 0..1 (for the pressure graph).
pub fn node_fractions(s: &VecStroke) -> Vec<f32> {
    let mut d = vec![0.0f32];
    for i in 1..s.pts.len() {
        d.push(d[i - 1] + (s.pts[i][0] - s.pts[i - 1][0]).hypot(s.pts[i][1] - s.pts[i - 1][1]));
    }
    let total = d.last().copied().unwrap_or(1.0).max(1e-6);
    d.iter().map(|v| v / total).collect()
}

/// Used by tests and the pen: tiles of a layer as plain alpha for a pixel.
#[cfg(test)]
pub fn alpha_at(layer: &Layer, x: i32, y: i32) -> u8 {
    let t = TILE as i32;
    layer.tiles.get(&(x.div_euclid(t), y.div_euclid(t))).map(|tile| tile[(y.rem_euclid(t) * t + x.rem_euclid(t)) as usize].a()).unwrap_or(0)
}

/// Shared helper: a snapshot of the pixels of a layer (cheap: tiles are reference counted).
pub fn snapshot(layer: &Layer) -> HashMap<TileKey, crate::doc::TileData> {
    layer.tiles.clone()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::doc::{Document, LayerKind};

    fn line(pts: &[(f32, f32)], style: LineStyle) -> VecStroke {
        VecStroke { pts: pts.iter().map(|p| [p.0, p.1, 1.0]).collect(), style }
    }

    fn vec_layer() -> (Document, usize) {
        let mut d = Document::new(300, 200);
        let l = d.new_layer("V", LayerKind::Vector);
        d.layers.insert(0, l);
        (d, 0)
    }

    fn style(size: f32) -> LineStyle {
        LineStyle { size, size_pressure: false, ..Default::default() }
    }

    #[test]
    fn a_line_has_the_right_thickness() {
        let (mut d, i) = vec_layer();
        let s = line(&[(20.0, 100.0), (200.0, 100.0)], style(10.0));
        draw_stroke(&mut d.layers[i], &s, 1.0, (0, 0, 300, 200));
        let l = &d.layers[i];
        assert_eq!(alpha_at(l, 100, 100), 255);
        assert_eq!(alpha_at(l, 100, 96), 255);
        assert!(alpha_at(l, 100, 94) < 40, "outside the 10 px line");
        assert!(alpha_at(l, 100, 106) < 40);
        assert!(alpha_at(l, 18, 100) > 100, "round cap reaches past the end point");
    }

    #[test]
    fn flat_ends_have_no_cap() {
        let (mut d, i) = vec_layer();
        let mut st = style(10.0);
        st.ends = LineEnds::Flat;
        draw_stroke(&mut d.layers[i], &line(&[(20.0, 100.0), (200.0, 100.0)], st), 1.0, (0, 0, 300, 200));
        let l = &d.layers[i];
        assert!(alpha_at(l, 17, 100) == 0, "nothing before the start");
        assert!(alpha_at(l, 22, 100) > 200);
        assert!(alpha_at(l, 203, 100) == 0, "nothing after the end");
    }

    #[test]
    fn pointed_ends_are_thin() {
        let (mut d, i) = vec_layer();
        let mut st = style(20.0);
        st.ends = LineEnds::Taper;
        draw_stroke(&mut d.layers[i], &line(&[(20.0, 100.0), (280.0, 100.0)], st), 1.0, (0, 0, 300, 200));
        let l = &d.layers[i];
        let thick = |x: i32| (80..120).filter(|&y| alpha_at(l, x, y) > 128).count();
        assert!(thick(22) < 6, "the tip is thin: {}", thick(22));
        assert!(thick(150) >= 18, "the middle is full width: {}", thick(150));
        assert!(thick(278) < 6);
    }

    #[test]
    fn pressure_changes_width() {
        let (mut d, i) = vec_layer();
        let st = LineStyle { size: 20.0, min_size: 0.1, size_pressure: true, ..Default::default() };
        let s = VecStroke { pts: vec![[20.0, 100.0, 0.0], [140.0, 100.0, 0.5], [260.0, 100.0, 1.0]], style: st };
        draw_stroke(&mut d.layers[i], &s, 1.0, (0, 0, 300, 200));
        let l = &d.layers[i];
        let thick = |x: i32| (70..130).filter(|&y| alpha_at(l, x, y) > 128).count();
        assert!(thick(40) < thick(140) && thick(140) < thick(250), "{} {} {}", thick(40), thick(140), thick(250));
    }

    #[test]
    fn opacity_does_not_double_where_a_line_crosses_itself() {
        let (mut d, i) = vec_layer();
        let mut st = style(12.0);
        st.opacity = 0.5;
        // a loop that crosses itself at (100, 100)
        let s = line(&[(40.0, 60.0), (160.0, 140.0), (160.0, 60.0), (40.0, 140.0)], st);
        draw_stroke(&mut d.layers[i], &s, 1.0, (0, 0, 300, 200));
        let l = &d.layers[i];
        assert!((alpha_at(l, 100, 100) as i32 - 128).abs() <= 2, "{}", alpha_at(l, 100, 100));
        assert!((alpha_at(l, 70, 78) as i32 - 128).abs() <= 2);
    }

    #[test]
    fn layer_width_scales_every_line() {
        let (mut d, i) = vec_layer();
        let data = data_mut(&mut d.layers[i]);
        data.strokes.push(line(&[(20.0, 100.0), (200.0, 100.0)], style(10.0)));
        render_all(&mut d.layers[i], 300, 200);
        assert!(alpha_at(&d.layers[i], 100, 93) < 40);
        data_mut(&mut d.layers[i]).width = 2.0;
        render_all(&mut d.layers[i], 300, 200);
        assert_eq!(alpha_at(&d.layers[i], 100, 93), 255, "twice as wide now");
        assert_eq!(alpha_at(&d.layers[i], 100, 93 - 10), 0);
    }

    #[test]
    fn region_render_matches_full_render() {
        let (mut d, i) = vec_layer();
        let data = data_mut(&mut d.layers[i]);
        data.strokes.push(line(&[(10.0, 20.0), (280.0, 30.0)], style(14.0)));
        data.strokes.push(line(&[(40.0, 150.0), (250.0, 60.0)], style(8.0)));
        render_all(&mut d.layers[i], 300, 200);
        let full = crate::files::layer_rgba(&d.layers[i], 0, 0, 300, 200);
        // change one line, redraw only around it: the rest must be identical
        let data = data_mut(&mut d.layers[i]);
        data.strokes[1].style.color = [200, 30, 30];
        let b = stroke_bounds(&data.strokes[1].clone(), 1.0);
        render_region(&mut d.layers[i], b);
        let part = crate::files::layer_rgba(&d.layers[i], 0, 0, 300, 200);
        render_all(&mut d.layers[i], 300, 200);
        let whole = crate::files::layer_rgba(&d.layers[i], 0, 0, 300, 200);
        assert_eq!(part, whole, "redrawing a region gives the same picture as redrawing everything");
        assert_ne!(full, whole);
    }

    #[test]
    fn eraser_cuts_or_removes() {
        let strokes = vec![line(&[(20.0, 100.0), (280.0, 100.0)], style(6.0)), line(&[(20.0, 20.0), (280.0, 20.0)], style(6.0))];
        let path = [(150.0, 100.0, 10.0)];
        let (cut, area) = erase(&strokes, &path, EraseMode::Cut, 1.0).unwrap();
        assert_eq!(cut.len(), 3, "the touched line became two lines");
        assert!(cut[0].pts.last().unwrap()[0] < 142.0 && cut[1].pts[0][0] > 158.0);
        assert!(area.1 < 100 && area.3 > 100);
        let (whole, _) = erase(&strokes, &path, EraseMode::Whole, 1.0).unwrap();
        assert_eq!(whole.len(), 1);
        assert!(erase(&strokes, &[(150.0, 60.0, 10.0)], EraseMode::Cut, 1.0).is_none(), "nothing touched");
        // a quick stroke across a line is not skipped
        let (fast, _) = erase(&strokes, &[(100.0, 150.0, 6.0), (100.0, 50.0, 6.0)], EraseMode::Whole, 1.0).unwrap();
        assert_eq!(fast.len(), 1);
    }

    #[test]
    fn color_shapes_have_holes_and_sit_under_lines() {
        let (mut d, i) = vec_layer();
        let ring = |x0: f32, y0: f32, x1: f32, y1: f32| vec![[x0, y0], [x1, y0], [x1, y1], [x0, y1]];
        let data = data_mut(&mut d.layers[i]);
        data.fills.push(VecFill { rings: vec![ring(20.0, 20.0, 180.0, 160.0), ring(70.0, 60.0, 130.0, 120.0)], color: [200, 40, 40] });
        data.strokes.push(line(&[(10.0, 90.0), (190.0, 90.0)], style(8.0)));
        render_all(&mut d.layers[i], 300, 200);
        let l = &d.layers[i];
        assert_eq!(alpha_at(l, 40, 40), 255, "inside the shape");
        assert_eq!(alpha_at(l, 100, 110), 0, "the hole is empty");
        assert_eq!(alpha_at(l, 250, 100), 0, "outside");
        let img = crate::files::layer_rgba(l, 0, 0, 300, 200);
        assert_eq!(img.get_pixel(40, 40).0, [200, 40, 40, 255]);
        assert_eq!(img.get_pixel(100, 90).0[..3], [34, 34, 34], "the line is on top, even across the hole");
        let f = &l.vector.as_ref().unwrap().fills[0];
        assert!(inside_fill(f, 40.0, 40.0) && !inside_fill(f, 100.0, 100.0));
        let (left, area) = erase_fills(&l.vector.as_ref().unwrap().fills, &[(40.0, 40.0, 5.0)]).unwrap();
        assert!(left.is_empty() && area.0 <= 20);
        assert!(erase_fills(&l.vector.as_ref().unwrap().fills, &[(100.0, 100.0, 5.0)]).is_none(), "the hole is not the shape");
    }

    #[test]
    fn node_editing() {
        let s = line(&[(0.0, 0.0), (100.0, 0.0), (200.0, 0.0)], style(4.0));
        let m = move_node(&s, 1, 0.0, 30.0, 0.0);
        assert_eq!((m.pts[1][1], m.pts[0][1], m.pts[2][1]), (30.0, 0.0, 0.0));
        let soft = move_node(&densify_stroke(&s, 10.0), 10, 0.0, 30.0, 60.0);
        assert!(soft.pts[9][1] > 5.0 && soft.pts[9][1] < 30.0, "neighbours follow: {}", soft.pts[9][1]);
        let (ins, k) = insert_node(&s, 50.0, 5.0);
        assert_eq!((ins.pts.len(), k), (4, 1));
        assert_eq!(ins.pts[1][0], 50.0);
        assert_eq!(delete_node(&ins, 1).pts.len(), 3);
        assert_eq!(delete_node(&line(&[(0.0, 0.0), (1.0, 1.0)], style(4.0)), 0).pts.len(), 2, "a line keeps two nodes");
        assert_eq!(hit_stroke(&[s.clone()], 100.0, 3.0, 2.0, 1.0), Some(0));
        assert_eq!(hit_stroke(&[s.clone()], 100.0, 40.0, 2.0, 1.0), None);
        assert_eq!(nearest_node(&s, 98.0, 3.0, 8.0, 0.0), Some(1));
        let moved = move_stroke(&s, 5.0, -5.0);
        assert_eq!((moved.pts[0][0], moved.pts[0][1]), (5.0, -5.0));
    }

    #[test]
    fn cleaning_a_shaky_line_keeps_the_shape_with_fewer_points() {
        let raw: Vec<VPt> = (0..200).map(|i| [i as f32 * 1.5, 100.0 + ((i * 7) % 5) as f32 * 0.3, 0.8]).collect();
        let gentle = clean_line(&raw, 0.1);
        let strong = clean_line(&raw, 0.9);
        assert!(gentle.len() < raw.len() && strong.len() <= gentle.len(), "{} {} {}", raw.len(), gentle.len(), strong.len());
        assert!(strong.len() <= 6);
        assert_eq!((strong[0][0], strong.last().unwrap()[0]), (0.0, 298.5), "the ends stay");
        let p = set_pressures(&line(&[(0.0, 0.0), (1.0, 0.0)], style(4.0)), &[0.2, 0.9]);
        assert_eq!((p.pts[0][2], p.pts[1][2]), (0.2, 0.9));
    }

    #[test]
    fn sanitizing_drops_garbage() {
        let d = VecData {
            strokes: vec![
                VecStroke { pts: vec![[0.0, 0.0, 2.0], [f32::NAN, 0.0, 0.5], [5.0, 5.0, 0.5]], style: LineStyle { size: f32::INFINITY, ..Default::default() } },
                VecStroke { pts: vec![[0.0, 0.0, 1.0]], style: Default::default() },
            ],
            width: f32::NAN,
            fills: vec![VecFill { rings: vec![vec![[0.0, 0.0], [1.0, 1.0]]], color: [0, 0, 0] }],
        }
        .sanitized();
        assert_eq!(d.strokes.len(), 1);
        assert_eq!((d.width, d.strokes[0].pts.len(), d.strokes[0].pts[0][2]), (1.0, 2, 1.0));
        assert!(d.strokes[0].style.size.is_finite());
        assert!(d.fills.is_empty(), "a shape needs three points");
    }
}
