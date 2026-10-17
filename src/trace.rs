// SPDX-License-Identifier: GPL-3.0-or-later
// Auto-trace (port of the first app's trace.ts): turns a pixel drawing into vector lines and
// color shapes. Lines are traced along their middle ("center line"), so the result is real
// editable lines with their width kept as pen pressure, not outlines like most tracers make.
// Colors are traced as flat shapes (like a color trace in a vector program).

use std::collections::{HashMap, HashSet};

use rayon::prelude::*;

use crate::vector::VPt;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum TraceMode {
    Lines,
    Colors,
    Both,
    Silhouette,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum LineColor {
    /// the average color of the pixels under each line
    Original,
    Single,
}

#[derive(Clone, PartialEq, Debug)]
pub struct TraceOptions {
    pub mode: TraceMode,
    /// 0..1: how dark (or solid) a pixel must be to count as line
    pub threshold: f32,
    /// 0..1: line smoothness
    pub smooth: f32,
    /// specks smaller than this many pixels are ignored
    pub detail: usize,
    /// pixels: small gaps in lines are closed
    pub close_gaps: usize,
    /// pixels: shorter lines are dropped
    pub min_length: f32,
    pub line_color: LineColor,
    pub single_color: [u8; 3],
    /// thickness changes become pen pressure
    pub keep_width: bool,
    /// number of colors for color shapes
    pub colors: usize,
    /// skip white paper and transparent areas
    pub ignore_background: bool,
    /// smaller color spots are merged into their neighbours
    pub min_area: usize,
    /// 0..1: smoothness of color shape edges
    pub color_smooth: f32,
}

impl Default for TraceOptions {
    fn default() -> Self {
        Self {
            mode: TraceMode::Lines,
            threshold: 0.45,
            smooth: 0.4,
            detail: 12,
            close_gaps: 0,
            min_length: 6.0,
            line_color: LineColor::Original,
            single_color: [0, 0, 0],
            keep_width: true,
            colors: 8,
            ignore_background: true,
            min_area: 30,
            color_smooth: 0.5,
        }
    }
}

impl TraceOptions {
    /// The same settings for a copy of the picture scaled by `k` (pixel sizes shrink with it).
    pub fn scaled(&self, k: f32) -> TraceOptions {
        TraceOptions {
            detail: (self.detail as f32 * k * k).round() as usize,
            min_area: (self.min_area as f32 * k * k).round() as usize,
            min_length: self.min_length * k,
            close_gaps: (self.close_gaps as f32 * k).round() as usize,
            ..self.clone()
        }
    }
}

pub struct Preset {
    pub name: &'static str,
    pub about: &'static str,
    pub opts: fn(&TraceOptions) -> TraceOptions,
}

/// Ready-made settings (the color choices of the current settings are kept).
pub const PRESETS: [Preset; 5] = [
    Preset {
        name: "Ink lineart",
        about: "Clean lines with their thickness. For inked or digital lineart.",
        opts: |o| TraceOptions { mode: TraceMode::Lines, threshold: 0.45, smooth: 0.4, detail: 12, close_gaps: 0, keep_width: true, ..keep_colors(o) },
    },
    Preset {
        name: "Pencil sketch",
        about: "Softer, lighter lines; small specks and gaps are cleaned up.",
        opts: |o| TraceOptions { mode: TraceMode::Lines, threshold: 0.3, smooth: 0.65, detail: 30, close_gaps: 1, keep_width: true, ..keep_colors(o) },
    },
    Preset {
        name: "Flat colors",
        about: "Color areas as flat shapes, like a poster.",
        opts: |o| TraceOptions { mode: TraceMode::Colors, colors: 8, color_smooth: 0.5, min_area: 40, ..keep_colors(o) },
    },
    Preset {
        name: "Lineart + colors",
        about: "Lines as editable lines, colors as shapes under them.",
        opts: |o| TraceOptions { mode: TraceMode::Both, threshold: 0.5, smooth: 0.4, colors: 8, color_smooth: 0.5, ..keep_colors(o) },
    },
    Preset {
        name: "Silhouette / logo",
        about: "Everything drawn becomes one solid shape in one color.",
        opts: |o| TraceOptions { mode: TraceMode::Silhouette, threshold: 0.4, color_smooth: 0.4, min_area: 20, ..keep_colors(o) },
    },
];

fn keep_colors(o: &TraceOptions) -> TraceOptions {
    TraceOptions { line_color: o.line_color, single_color: o.single_color, ..TraceOptions::default() }
}

pub struct TracedLine {
    pub pts: Vec<VPt>,
    /// the width at full pressure
    pub size: f32,
    pub color: [u8; 3],
    pub pressure: bool,
}

pub struct TracedFill {
    pub rings: Vec<Vec<[f32; 2]>>,
    pub color: [u8; 3],
    pub area: f32,
}

#[derive(Default)]
pub struct TraceResult {
    pub lines: Vec<TracedLine>,
    pub fills: Vec<TracedFill>,
}

type Progress<'a> = &'a (dyn Fn(f32) + Sync);

// ---------- Pixel masks ----------

/// Is this a drawing on a see-through layer (a lineart layer), or a flat picture (scan, photo)?
fn has_transparency(d: &[u8]) -> bool {
    let (mut clear, mut n) = (0usize, 0usize);
    let mut i = 3;
    while i < d.len() {
        n += 1;
        if d[i] < 128 {
            clear += 1;
        }
        i += 4 * 7;
    }
    clear as f32 > n as f32 * 0.05
}

fn line_mask(d: &[u8], w: usize, h: usize, o: &TraceOptions) -> Vec<u8> {
    // on a see-through layer everything painted counts (lineart layers can be any color);
    // on a flat picture, dark pixels count (colored areas are not lines)
    let only_alpha = matches!(o.mode, TraceMode::Lines | TraceMode::Silhouette) && has_transparency(d);
    (0..w * h)
        .into_par_iter()
        .map(|p| {
            let i = p * 4;
            let a = d[i + 3] as f32 / 255.0;
            let ink = if only_alpha { a } else { a * (1.0 - d[i].max(d[i + 1]).max(d[i + 2]) as f32 / 255.0) };
            (ink > o.threshold) as u8
        })
        .collect()
}

/// Grow (true) or shrink (false) the mask by one pixel, four directions.
fn morph(m: &[u8], w: usize, h: usize, grow: bool) -> Vec<u8> {
    let mut out = m.to_vec();
    let v = grow as u8;
    for y in 0..h {
        for x in 0..w {
            let p = y * w + x;
            if m[p] == v {
                continue;
            }
            if (x > 0 && m[p - 1] == v) || (x + 1 < w && m[p + 1] == v) || (y > 0 && m[p - w] == v) || (y + 1 < h && m[p + w] == v) {
                out[p] = v;
            }
        }
    }
    out
}

/// Remove connected pieces smaller than `min_area`.
fn despeckle(m: &mut [u8], w: usize, h: usize, min_area: usize) {
    if min_area <= 1 {
        return;
    }
    let mut seen = vec![false; w * h];
    let mut stack: Vec<usize> = Vec::new();
    let mut piece: Vec<usize> = Vec::new();
    for s in 0..w * h {
        if m[s] == 0 || seen[s] {
            continue;
        }
        piece.clear();
        stack.push(s);
        seen[s] = true;
        while let Some(p) = stack.pop() {
            piece.push(p);
            let (x, y) = ((p % w) as isize, (p / w) as isize);
            for dy in -1..=1isize {
                for dx in -1..=1isize {
                    let (nx, ny) = (x + dx, y + dy);
                    if nx < 0 || ny < 0 || nx >= w as isize || ny >= h as isize {
                        continue;
                    }
                    let q = ny as usize * w + nx as usize;
                    if m[q] != 0 && !seen[q] {
                        seen[q] = true;
                        stack.push(q);
                    }
                }
            }
        }
        if piece.len() < min_area {
            for &p in &piece {
                m[p] = 0;
            }
        }
    }
}

/// Distance from each line pixel to the nearest empty pixel (chamfer 3-4, in pixels).
fn distance(m: &[u8], w: usize, h: usize) -> Vec<f32> {
    const INF: f32 = 1e9;
    let mut d: Vec<f32> = m.iter().map(|&v| if v != 0 { INF } else { 0.0 }).collect();
    for y in 0..h {
        for x in 0..w {
            let p = y * w + x;
            if d[p] == 0.0 {
                continue;
            }
            let mut v = d[p];
            v = if x > 0 { v.min(d[p - 1] + 3.0) } else { v.min(3.0) };
            if y > 0 {
                v = v.min(d[p - w] + 3.0);
                if x > 0 {
                    v = v.min(d[p - w - 1] + 4.0);
                }
                if x + 1 < w {
                    v = v.min(d[p - w + 1] + 4.0);
                }
            } else {
                v = v.min(3.0);
            }
            d[p] = v;
        }
    }
    for y in (0..h).rev() {
        for x in (0..w).rev() {
            let p = y * w + x;
            if d[p] == 0.0 {
                continue;
            }
            let mut v = d[p];
            v = if x + 1 < w { v.min(d[p + 1] + 3.0) } else { v.min(3.0) };
            if y + 1 < h {
                v = v.min(d[p + w] + 3.0);
                if x + 1 < w {
                    v = v.min(d[p + w + 1] + 4.0);
                }
                if x > 0 {
                    v = v.min(d[p + w - 1] + 4.0);
                }
            } else {
                v = v.min(3.0);
            }
            d[p] = v;
        }
    }
    d.par_iter_mut().for_each(|v| *v /= 3.0);
    d
}

/// Zhang-Suen thinning: shrinks every line to one pixel wide, keeping its middle.
fn thin(m: &mut [u8], w: usize, h: usize) {
    let mut list: Vec<usize> = Vec::new();
    for p in 0..w * h {
        let (x, y) = (p % w, p / w);
        if m[p] != 0 && x > 0 && y > 0 && x < w - 1 && y < h - 1 {
            list.push(p);
        } else if m[p] != 0 {
            m[p] = 0; // keep a one pixel empty border
        }
    }
    let mut remove: Vec<usize> = Vec::new();
    let mut changed = true;
    while changed {
        changed = false;
        for step in 0..2 {
            remove.clear();
            for &p in &list {
                if m[p] == 0 {
                    continue;
                }
                let (p2, p3, p4, p5) = (m[p - w], m[p - w + 1], m[p + 1], m[p + w + 1]);
                let (p6, p7, p8, p9) = (m[p + w], m[p + w - 1], m[p - 1], m[p - w - 1]);
                let b = p2 + p3 + p4 + p5 + p6 + p7 + p8 + p9;
                if !(2..=6).contains(&b) {
                    continue;
                }
                let t = |a: u8, c: u8| (a == 0 && c == 1) as u8;
                let a = t(p2, p3) + t(p3, p4) + t(p4, p5) + t(p5, p6) + t(p6, p7) + t(p7, p8) + t(p8, p9) + t(p9, p2);
                if a != 1 {
                    continue;
                }
                let ok = if step == 0 { p2 * p4 * p6 == 0 && p4 * p6 * p8 == 0 } else { p2 * p4 * p8 == 0 && p2 * p6 * p8 == 0 };
                if ok {
                    remove.push(p);
                }
            }
            if !remove.is_empty() {
                changed = true;
            }
            for &p in &remove {
                m[p] = 0;
            }
        }
        list.retain(|&p| m[p] != 0);
    }
}

// ---------- Following the one-pixel lines ----------

struct SkelPath {
    path: Vec<usize>,
    ends: [bool; 2],
}

fn skeleton_paths(s: &[u8], w: usize, h: usize) -> Vec<SkelPath> {
    let wi = w as isize;
    let nb: [isize; 8] = [-wi - 1, -wi, -wi + 1, -1, 1, wi - 1, wi, wi + 1];
    let at = |p: usize, o: isize| (p as isize + o) as usize;
    let mut deg = vec![0u8; w * h];
    let mut pix: Vec<usize> = Vec::new();
    // degree = how many separate branches touch the pixel (not how many neighbours it has:
    // a diagonal "staircase" pixel has 3 neighbours but is still just part of a line)
    let ring: [isize; 8] = [-wi, -wi + 1, 1, wi + 1, wi, wi - 1, -1, -wi - 1];
    for p in w..w * h - w {
        if s[p] == 0 {
            continue;
        }
        pix.push(p);
        let (mut groups, mut any) = (0u8, 0u8);
        for k in 0..8 {
            let a = s[at(p, ring[k])];
            let b = s[at(p, ring[(k + 1) % 8])];
            if a != 0 {
                any += 1;
            }
            if a == 0 && b != 0 {
                groups += 1;
            }
        }
        deg[p] = if any == 8 { 0 } else if groups == 0 && any > 0 { 1 } else { groups };
    }
    let mut visited = vec![false; w * h];
    let mut out: Vec<SkelPath> = Vec::new();
    let is_node = |deg: &[u8], p: usize| deg[p] != 2;
    let mut used: HashSet<usize> = HashSet::new(); // edges already walked: from * 8 + direction

    let walk = |visited: &mut Vec<bool>, deg: &[u8], start: usize, first: usize| -> Vec<usize> {
        let mut path = vec![start, first];
        let (mut prev, mut cur) = (start, first);
        while !is_node(deg, cur) {
            visited[cur] = true;
            let mut next = None;
            // prefer straight neighbours, never step back next to where we came from
            for o in [-wi, 1, wi, -1, -wi - 1, -wi + 1, wi - 1, wi + 1] {
                let q = at(cur, o);
                if s[q] == 0 || q == prev || (q == start && path.len() < 3) {
                    continue;
                }
                if visited[q] && !is_node(deg, q) {
                    continue;
                }
                next = Some(q);
                break;
            }
            let Some(q) = next else { break };
            prev = cur;
            cur = q;
            path.push(cur);
            if cur == start {
                break;
            }
        }
        if !is_node(deg, cur) {
            visited[cur] = true;
        }
        path
    };

    // 1. lines that start or end at line ends or crossings
    for &p in &pix {
        if !is_node(&deg, p) || deg[p] == 0 {
            continue;
        }
        for k in 0..8 {
            let q = at(p, nb[k]);
            if s[q] == 0 || used.contains(&(p * 8 + k)) {
                continue;
            }
            if !is_node(&deg, q) && visited[q] {
                continue;
            }
            used.insert(p * 8 + k);
            if is_node(&deg, q) {
                // two crossing pixels next to each other: mark both directions
                if let Some(back) = nb.iter().position(|&o| o == -nb[k]) {
                    used.insert(q * 8 + back);
                }
                continue;
            }
            let path = walk(&mut visited, &deg, p, q);
            let end = *path.last().unwrap();
            if is_node(&deg, end) && path.len() > 1 {
                let diff = path[path.len() - 2] as isize - end as isize;
                if let Some(back) = nb.iter().position(|&o| o == diff) {
                    used.insert(end * 8 + back);
                }
            }
            out.push(SkelPath { ends: [deg[p] == 1, deg[end] == 1], path });
        }
    }
    // 2. closed loops (no ends at all)
    for &p in &pix {
        if visited[p] || is_node(&deg, p) {
            continue;
        }
        let Some(q) = nb.iter().map(|&o| at(p, o)).find(|&q| s[q] != 0) else { continue };
        visited[p] = true;
        let path = walk(&mut visited, &deg, p, q);
        out.push(SkelPath { path, ends: [false, false] });
    }
    // single dots
    for &p in &pix {
        if deg[p] == 0 {
            out.push(SkelPath { path: vec![p, p], ends: [true, true] });
        }
    }
    out
}

/// Douglas-Peucker on parallel x / y lists: the indices of the points to keep.
fn rdp(xs: &[f32], ys: &[f32], eps: f32) -> Vec<usize> {
    let n = xs.len();
    if n <= 2 {
        return (0..n).collect();
    }
    let mut keep = vec![false; n];
    keep[0] = true;
    keep[n - 1] = true;
    let mut stack = vec![(0usize, n - 1)];
    while let Some((a, b)) = stack.pop() {
        let (dx, dy) = (xs[b] - xs[a], ys[b] - ys[a]);
        let len = dx.hypot(dy);
        let (mut worst, mut at) = (-1.0f32, 0usize);
        for i in a + 1..b {
            let d = if len < 1e-6 { (xs[i] - xs[a]).hypot(ys[i] - ys[a]) } else { ((xs[i] - xs[a]) * dy - (ys[i] - ys[a]) * dx).abs() / len };
            if d > worst {
                worst = d;
                at = i;
            }
        }
        if worst > eps && at > 0 {
            keep[at] = true;
            stack.push((a, at));
            stack.push((at, b));
        }
    }
    (0..n).filter(|&i| keep[i]).collect()
}

fn trace_lines(d: &[u8], w: usize, h: usize, o: &TraceOptions, report: Progress) -> Vec<TracedLine> {
    let mut m = line_mask(d, w, h, o);
    for _ in 0..o.close_gaps {
        m = morph(&m, w, h, true);
    }
    for _ in 0..o.close_gaps {
        m = morph(&m, w, h, false);
    }
    despeckle(&mut m, w, h, o.detail);
    report(0.2);
    let dist = distance(&m, w, h);
    report(0.35);
    thin(&mut m, w, h);
    report(0.6);
    let paths = skeleton_paths(&m, w, h);
    report(0.75);

    let results: Vec<Option<TracedLine>> = paths
        .par_iter()
        .map(|sp| {
            let mut xs: Vec<f32> = sp.path.iter().map(|&p| (p % w) as f32 + 0.5).collect();
            let mut ys: Vec<f32> = sp.path.iter().map(|&p| (p / w) as f32 + 0.5).collect();
            let mut rs: Vec<f32> = sp.path.iter().map(|&p| dist[p].max(0.6)).collect();
            let len: f32 = (1..xs.len()).map(|i| (xs[i] - xs[i - 1]).hypot(ys[i] - ys[i - 1])).sum();
            let avg_r = rs.iter().sum::<f32>() / rs.len() as f32;
            // little side branches that thinning makes at the ends of thick lines
            let spur = (sp.ends[0] || sp.ends[1]) && !(sp.ends[0] && sp.ends[1]) && len < avg_r * 2.2 + 1.0;
            if spur || (len < o.min_length && !(sp.ends[0] && sp.ends[1] && len >= 1.0 && avg_r > o.min_length / 2.0)) {
                return None;
            }
            // smooth (the ends stay put)
            for _ in 0..(o.smooth * 6.0).round() as usize {
                let (sx, sy, sr) = (xs.clone(), ys.clone(), rs.clone());
                for i in 1..xs.len().saturating_sub(1) {
                    xs[i] = (sx[i - 1] + 2.0 * sx[i] + sx[i + 1]) / 4.0;
                    ys[i] = (sy[i - 1] + 2.0 * sy[i] + sy[i + 1]) / 4.0;
                    rs[i] = (sr[i - 1] + 2.0 * sr[i] + sr[i + 1]) / 4.0;
                }
            }
            let keep = rdp(&xs, &ys, 0.35 + o.smooth * 1.2);
            let max_r = rs.iter().cloned().fold(0.0f32, f32::max);
            // line color: the average of the original pixels under the line
            let mut color = o.single_color;
            if o.line_color == LineColor::Original {
                let (mut r, mut g, mut b, mut n) = (0u32, 0u32, 0u32, 0u32);
                for &p in &sp.path {
                    let i = p * 4;
                    if d[i + 3] < 20 {
                        continue;
                    }
                    r += d[i] as u32;
                    g += d[i + 1] as u32;
                    b += d[i + 2] as u32;
                    n += 1;
                }
                if n > 0 {
                    color = [(r / n) as u8, (g / n) as u8, (b / n) as u8];
                }
            }
            let mut pts: Vec<VPt> = keep
                .iter()
                .map(|&i| {
                    let pr = if o.keep_width { (rs[i] / max_r).clamp(0.15, 1.0) } else { 1.0 };
                    [(xs[i] * 100.0).round() / 100.0, (ys[i] * 100.0).round() / 100.0, (pr * 1000.0).round() / 1000.0]
                })
                .collect();
            if pts.len() == 1 {
                let p = pts[0];
                pts.push([p[0] + 0.01, p[1], p[2]]);
            }
            Some(TracedLine { pts, size: (max_r * 2.0).max(1.0), color, pressure: o.keep_width })
        })
        .collect();
    results.into_iter().flatten().collect()
}

// ---------- Color shapes ----------

fn dist2(a: &[f32; 3], b: &[f32; 3]) -> f32 {
    (a[0] - b[0]).powi(2) + (a[1] - b[1]).powi(2) + (a[2] - b[2]).powi(2)
}

fn kmeans(samples: &[[f32; 3]], k: usize) -> Vec<[f32; 3]> {
    // k-means++ start: spread-out first colors
    let mut centers = vec![samples[samples.len() / 2]];
    while centers.len() < k && centers.len() < samples.len() {
        let (mut best, mut best_d) = (0usize, -1.0f32);
        let step = (samples.len() / 3000).max(1);
        let mut i = 0;
        while i < samples.len() {
            let dmin = centers.iter().map(|c| dist2(&samples[i], c)).fold(f32::MAX, f32::min);
            if dmin > best_d {
                best_d = dmin;
                best = i;
            }
            i += step;
        }
        if best_d <= 0.0 {
            break;
        }
        centers.push(samples[best]);
    }
    for _ in 0..10 {
        let mut sum = vec![[0f32; 4]; centers.len()];
        for s in samples {
            let (mut bi, mut bd) = (0usize, f32::MAX);
            for (c, ce) in centers.iter().enumerate() {
                let dd = dist2(s, ce);
                if dd < bd {
                    bd = dd;
                    bi = c;
                }
            }
            sum[bi][0] += s[0];
            sum[bi][1] += s[1];
            sum[bi][2] += s[2];
            sum[bi][3] += 1.0;
        }
        for (c, s) in sum.iter().enumerate() {
            if s[3] > 0.0 {
                centers[c] = [s[0] / s[3], s[1] / s[3], s[2] / s[3]];
            }
        }
    }
    centers
}

fn trace_colors(d: &[u8], w: usize, h: usize, o: &TraceOptions, line_m: Option<&[u8]>, report: Progress) -> Vec<TracedFill> {
    let n = w * h;
    let is_bg = |i: usize| {
        if d[i + 3] < 128 {
            return true;
        }
        if !o.ignore_background {
            return false;
        }
        let mx = d[i].max(d[i + 1]).max(d[i + 2]);
        let mn = d[i].min(d[i + 1]).min(d[i + 2]);
        mn > 238 && mx - mn < 12 // white paper
    };
    // 1. pick the colors
    let mut samples: Vec<[f32; 3]> = Vec::new();
    let step = (n / 40_000).max(1);
    let mut p = 0;
    while p < n {
        let i = p * 4;
        if !(is_bg(i) || line_m.is_some_and(|m| m[p] != 0)) {
            samples.push([d[i] as f32, d[i + 1] as f32, d[i + 2] as f32]);
        }
        p += step;
    }
    if samples.is_empty() {
        return Vec::new();
    }
    let centers = kmeans(&samples, o.colors.clamp(1, 32));
    report(0.3);

    // 2. every pixel gets its nearest color (-1 = background, -2 = under a line)
    let mut label: Vec<i16> = (0..n)
        .into_par_iter()
        .map(|p| {
            let i = p * 4;
            if is_bg(i) {
                return -1;
            }
            if line_m.is_some_and(|m| m[p] != 0) {
                return -2;
            }
            let px = [d[i] as f32, d[i + 1] as f32, d[i + 2] as f32];
            let (mut bi, mut bd) = (0usize, f32::MAX);
            for (c, ce) in centers.iter().enumerate() {
                let dd = dist2(&px, ce);
                if dd < bd {
                    bd = dd;
                    bi = c;
                }
            }
            bi as i16
        })
        .collect();
    // colors continue under the lines, so lines and colors fit without gaps
    if line_m.is_some() {
        let mut changed = true;
        let mut guard = 0;
        while changed && guard < 64 {
            guard += 1;
            changed = false;
            let mut next = label.clone();
            for p in 0..n {
                if label[p] != -2 {
                    continue;
                }
                let (x, y) = (p % w, p / w);
                let cand = [if x > 0 { label[p - 1] } else { -1 }, if x + 1 < w { label[p + 1] } else { -1 }, if y > 0 { label[p - w] } else { -1 }, if y + 1 < h { label[p + w] } else { -1 }];
                if let Some(&c) = cand.iter().find(|&&v| v >= 0) {
                    next[p] = c;
                    changed = true;
                }
            }
            label = next;
        }
        for v in label.iter_mut() {
            if *v == -2 {
                *v = -1;
            }
        }
    }
    // 3. smooth: each pixel takes the most common color around it
    for _ in 0..(o.color_smooth * 3.0).round() as usize {
        let mut next = label.clone();
        let mut count: HashMap<i16, u8> = HashMap::new();
        for y in 1..h.saturating_sub(1) {
            for x in 1..w.saturating_sub(1) {
                let p = y * w + x;
                count.clear();
                for dy in [-1isize, 0, 1] {
                    for dx in [-1isize, 0, 1] {
                        let v = label[(p as isize + dy * w as isize + dx) as usize];
                        *count.entry(v).or_insert(0) += 1;
                    }
                }
                let (mut best, mut bc) = (label[p], 0u8);
                for (&v, &c) in &count {
                    if c > bc || (c == bc && v < best) {
                        bc = c;
                        best = v;
                    }
                }
                if bc >= 5 {
                    next[p] = best;
                }
            }
        }
        label = next;
    }
    report(0.5);
    // 4. tiny spots join the color around them
    let mut seen = vec![false; n];
    let mut stack: Vec<usize> = Vec::new();
    let mut piece: Vec<usize> = Vec::new();
    for s in 0..n {
        if seen[s] || label[s] < 0 {
            continue;
        }
        let l = label[s];
        piece.clear();
        stack.push(s);
        seen[s] = true;
        let mut around: HashMap<i16, usize> = HashMap::new();
        while let Some(p) = stack.pop() {
            piece.push(p);
            let (x, y) = ((p % w) as isize, (p / w) as isize);
            for (nx, ny) in [(x - 1, y), (x + 1, y), (x, y - 1), (x, y + 1)] {
                if nx < 0 || ny < 0 || nx >= w as isize || ny >= h as isize {
                    continue;
                }
                let q = ny as usize * w + nx as usize;
                if label[q] == l {
                    if !seen[q] {
                        seen[q] = true;
                        stack.push(q);
                    }
                } else {
                    *around.entry(label[q]).or_insert(0) += 1;
                }
            }
        }
        if piece.len() < o.min_area {
            let mut best = -1i16;
            let mut bc = 0usize;
            let mut cand: Vec<(&i16, &usize)> = around.iter().collect();
            cand.sort();
            for (&v, &c) in cand {
                if c > bc {
                    bc = c;
                    best = v;
                }
            }
            for &p in &piece {
                label[p] = best;
            }
        }
    }
    report(0.65);

    // 5. outlines of every color area, following the pixel edges
    let w1 = w + 1;
    let mut edges_of: Vec<HashMap<u32, Vec<u32>>> = vec![HashMap::new(); centers.len()];
    for y in 0..h {
        for x in 0..w {
            let c = label[y * w + x];
            if c < 0 {
                continue;
            }
            let (tl, tr) = ((y * w1 + x) as u32, (y * w1 + x + 1) as u32);
            let (bl, br) = (tl + w1 as u32, tr + w1 as u32);
            let m = &mut edges_of[c as usize];
            let mut add = |a: u32, b: u32| m.entry(a).or_default().push(b);
            if y == 0 || label[(y - 1) * w + x] != c {
                add(tl, tr);
            }
            if x == w - 1 || label[y * w + x + 1] != c {
                add(tr, br);
            }
            if y == h - 1 || label[(y + 1) * w + x] != c {
                add(br, bl);
            }
            if x == 0 || label[y * w + x - 1] != c {
                add(bl, tl);
            }
        }
    }
    let eps = 0.5 + o.color_smooth;
    let mut fills: Vec<TracedFill> = edges_of
        .into_par_iter()
        .enumerate()
        .filter_map(|(c, mut m)| {
            let mut rings: Vec<Vec<[f32; 2]>> = Vec::new();
            let mut area_sum = 0.0f32;
            let mut starts: Vec<u32> = m.keys().copied().collect();
            starts.sort_unstable();
            for start_key in starts {
                loop {
                    let Some(mut b) = m.get_mut(&start_key).and_then(|l| l.pop()) else { break };
                    let mut a = start_key;
                    let (mut xs, mut ys) = (vec![(a % w1 as u32) as f32], vec![(a / w1 as u32) as f32]);
                    let mut guard = 0usize;
                    while b != start_key && guard < 4 * n {
                        guard += 1;
                        xs.push((b % w1 as u32) as f32);
                        ys.push((b / w1 as u32) as f32);
                        let Some(nexts) = m.get_mut(&b) else { break };
                        if nexts.is_empty() {
                            break;
                        }
                        // at a corner where two areas touch diagonally, turn the same way every time
                        let mut k = nexts.len() - 1;
                        if nexts.len() > 1 {
                            let (dx, dy) = ((b % w1 as u32) as f32 - (a % w1 as u32) as f32, (b / w1 as u32) as f32 - (a / w1 as u32) as f32);
                            k = nexts
                                .iter()
                                .position(|&nn| {
                                    let (ndx, ndy) = ((nn % w1 as u32) as f32 - (b % w1 as u32) as f32, (nn / w1 as u32) as f32 - (b / w1 as u32) as f32);
                                    dx * ndy - dy * ndx > 0.0
                                })
                                .unwrap_or(0);
                        }
                        a = b;
                        b = nexts.remove(k);
                    }
                    if xs.len() < 4 {
                        continue;
                    }
                    // ring area (to drop crumbs and to sort)
                    let mut ar = 0.0f32;
                    for i in 0..xs.len() {
                        let j = (i + 1) % xs.len();
                        ar += xs[i] * ys[j] - xs[j] * ys[i];
                    }
                    let ar = ar.abs() / 2.0;
                    if ar < (o.min_area as f32 / 4.0).max(2.0) {
                        continue;
                    }
                    // simplify, then round off the pixel steps (Chaikin)
                    xs.push(xs[0]);
                    ys.push(ys[0]);
                    let keep = rdp(&xs, &ys, eps);
                    let mut px: Vec<f32> = keep.iter().map(|&i| xs[i]).collect();
                    let mut py: Vec<f32> = keep.iter().map(|&i| ys[i]).collect();
                    px.pop();
                    py.pop();
                    let rounds = if o.color_smooth > 0.25 { 2 } else if o.color_smooth > 0.0 { 1 } else { 0 };
                    for _ in 0..rounds {
                        if px.len() < 3 {
                            break;
                        }
                        let (mut nx, mut ny) = (Vec::new(), Vec::new());
                        for i in 0..px.len() {
                            let j = (i + 1) % px.len();
                            nx.push(0.75 * px[i] + 0.25 * px[j]);
                            nx.push(0.25 * px[i] + 0.75 * px[j]);
                            ny.push(0.75 * py[i] + 0.25 * py[j]);
                            ny.push(0.25 * py[i] + 0.75 * py[j]);
                        }
                        px = nx;
                        py = ny;
                    }
                    if px.len() < 3 {
                        continue;
                    }
                    rings.push(px.iter().zip(&py).map(|(x, y)| [(x * 100.0).round() / 100.0, (y * 100.0).round() / 100.0]).collect());
                    area_sum += ar;
                }
            }
            if rings.is_empty() {
                return None;
            }
            let ce = centers[c];
            Some(TracedFill { rings, color: [ce[0].round().clamp(0.0, 255.0) as u8, ce[1].round().clamp(0.0, 255.0) as u8, ce[2].round().clamp(0.0, 255.0) as u8], area: area_sum })
        })
        .collect();
    // big areas first, small details on top
    fills.sort_by(|a, b| b.area.total_cmp(&a.area));
    report(0.9);
    fills
}

/// Trace unmultiplied RGBA pixels. `report` gets progress 0..1.
pub fn trace(d: &[u8], w: usize, h: usize, o: &TraceOptions, report: Progress) -> TraceResult {
    let mut result = TraceResult::default();
    if w < 3 || h < 3 || d.len() < w * h * 4 {
        return result;
    }
    match o.mode {
        TraceMode::Silhouette => {
            // a silhouette is one solid shape: trace it as a color area in a single color
            let so = TraceOptions { mode: TraceMode::Colors, colors: 1, ignore_background: true, ..o.clone() };
            let m = line_mask(d, w, h, o);
            let mut solid = vec![0u8; d.len()];
            for p in 0..w * h {
                if m[p] != 0 {
                    solid[p * 4..p * 4 + 4].copy_from_slice(&[o.single_color[0], o.single_color[1], o.single_color[2], 255]);
                }
            }
            result.fills = trace_colors(&solid, w, h, &so, None, report);
            report(1.0);
            return result;
        }
        TraceMode::Lines | TraceMode::Both => {
            let both = o.mode == TraceMode::Both;
            result.lines = trace_lines(d, w, h, o, &|p| report(if both { p * 0.5 } else { p }));
        }
        TraceMode::Colors => {}
    }
    if matches!(o.mode, TraceMode::Colors | TraceMode::Both) {
        let both = o.mode == TraceMode::Both;
        let lm = both.then(|| line_mask(d, w, h, o));
        result.fills = trace_colors(d, w, h, o, lm.as_deref(), &|p| report(if both { 0.5 + p * 0.5 } else { p }));
    }
    report(1.0);
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rgba(w: usize, h: usize, f: impl Fn(usize, usize) -> [u8; 4]) -> Vec<u8> {
        (0..w * h).flat_map(|p| f(p % w, p / w)).collect()
    }

    fn none(_: f32) {}

    #[test]
    fn a_thick_line_becomes_one_line_with_its_width() {
        let d = rgba(160, 60, |x, y| if (20..140).contains(&x) && (26..34).contains(&y) { [30, 20, 20, 255] } else { [0; 4] });
        let r = trace(&d, 160, 60, &TraceOptions::default(), &none);
        assert_eq!(r.lines.len(), 1, "one line");
        let l = &r.lines[0];
        assert!((l.size - 8.0).abs() < 2.5, "width kept: {}", l.size);
        let (first, last) = (l.pts[0], *l.pts.last().unwrap());
        let (a, b) = if first[0] < last[0] { (first, last) } else { (last, first) };
        assert!(a[0] < 30.0 && b[0] > 130.0, "spans the line: {a:?} {b:?}");
        assert!(l.pts.iter().all(|p| (p[1] - 30.0).abs() < 1.5), "runs along the middle");
        assert_eq!(l.color, [30, 20, 20], "color as drawn");
        assert!(l.pts.len() <= 4, "few points on a straight line: {}", l.pts.len());
    }

    #[test]
    fn thickness_becomes_pressure() {
        // a wedge: thin on the left, thick on the right
        let d = rgba(200, 60, |x, y| {
            let half = 1.0 + x as f32 / 200.0 * 9.0;
            if (10..190).contains(&x) && (y as f32 - 30.0).abs() < half { [0, 0, 0, 255] } else { [0; 4] }
        });
        let r = trace(&d, 200, 60, &TraceOptions { smooth: 0.1, ..Default::default() }, &none);
        let l = r.lines.iter().max_by_key(|l| l.pts.len()).unwrap();
        let (a, b) = (l.pts[0], *l.pts.last().unwrap());
        let (thin, thick) = if a[0] < b[0] { (a, b) } else { (b, a) };
        assert!(thin[2] < thick[2] - 0.3, "pressure rises with the width: {thin:?} {thick:?}");
        let flat = trace(&d, 200, 60, &TraceOptions { keep_width: false, ..Default::default() }, &none);
        assert!(flat.lines[0].pts.iter().all(|p| p[2] == 1.0));
    }

    #[test]
    fn specks_and_crossings() {
        // a plus sign and a speck
        let d = rgba(120, 120, |x, y| {
            let plus = ((50..56).contains(&x) && (20..100).contains(&y)) || ((50..56).contains(&y) && (20..100).contains(&x));
            let speck = (100..102).contains(&x) && (100..102).contains(&y);
            if plus || speck { [0, 0, 0, 255] } else { [0; 4] }
        });
        let r = trace(&d, 120, 120, &TraceOptions::default(), &none);
        assert!(r.lines.len() >= 2 && r.lines.len() <= 4, "the plus is a few lines: {}", r.lines.len());
        assert!(r.lines.iter().all(|l| l.pts.iter().all(|p| p[0] < 95.0 || p[1] < 95.0)), "the speck is ignored");
    }

    #[test]
    fn a_ring_is_a_closed_line() {
        let d = rgba(140, 140, |x, y| {
            let r = ((x as f32 - 70.0).powi(2) + (y as f32 - 70.0).powi(2)).sqrt();
            if (44.0..52.0).contains(&r) { [0, 0, 0, 255] } else { [0; 4] }
        });
        let r = trace(&d, 140, 140, &TraceOptions { smooth: 0.1, ..Default::default() }, &none);
        assert_eq!(r.lines.len(), 1);
        let l = &r.lines[0];
        let radii: Vec<f32> = l.pts.iter().map(|p| (p[0] - 70.0).hypot(p[1] - 70.0)).collect();
        assert!(radii.iter().all(|r| (r - 48.0).abs() < 2.0), "{radii:?}");
        let (a, b) = (l.pts[0], *l.pts.last().unwrap());
        assert!((a[0] - b[0]).hypot(a[1] - b[1]) < 5.0, "the ends meet");
    }

    #[test]
    fn color_areas_become_shapes() {
        let d = rgba(120, 80, |x, y| {
            if (10..50).contains(&x) && (10..70).contains(&y) {
                [220, 30, 30, 255]
            } else if (60..110).contains(&x) && (10..40).contains(&y) {
                [30, 60, 220, 255]
            } else {
                [255, 255, 255, 255]
            }
        });
        let o = TraceOptions { mode: TraceMode::Colors, colors: 3, ..Default::default() };
        let r = trace(&d, 120, 80, &o, &none);
        assert_eq!(r.fills.len(), 2, "red and blue; the white paper is left out");
        let red = r.fills.iter().find(|f| f.color[0] > 150).unwrap();
        let blue = r.fills.iter().find(|f| f.color[2] > 150).unwrap();
        assert!((red.area - 40.0 * 60.0).abs() < 120.0 && (blue.area - 50.0 * 30.0).abs() < 100.0, "{} {}", red.area, blue.area);
        assert!(r.fills[0].area >= r.fills[1].area, "big areas first");
        // keeping the paper makes a third shape
        let all = trace(&d, 120, 80, &TraceOptions { ignore_background: false, ..o.clone() }, &none);
        assert_eq!(all.fills.len(), 3);
    }

    #[test]
    fn silhouette_is_one_solid_shape() {
        let d = rgba(100, 100, |x, y| if ((x as f32 - 50.0).powi(2) + (y as f32 - 50.0).powi(2)).sqrt() < 30.0 { [200, 100, 50, 255] } else { [0; 4] });
        let o = TraceOptions { mode: TraceMode::Silhouette, single_color: [5, 6, 7], ..Default::default() };
        let r = trace(&d, 100, 100, &o, &none);
        assert!(r.lines.is_empty());
        assert_eq!(r.fills.len(), 1);
        assert_eq!(r.fills[0].color, [5, 6, 7]);
        assert!((r.fills[0].area - std::f32::consts::PI * 900.0).abs() < 150.0, "{}", r.fills[0].area);
    }

    #[test]
    fn both_makes_lines_over_color() {
        // a blue box with a black outline on white paper
        let d = rgba(100, 100, |x, y| {
            let edge = (20..80).contains(&x) && (20..80).contains(&y) && !((24..76).contains(&x) && (24..76).contains(&y));
            if edge {
                [0, 0, 0, 255]
            } else if (24..76).contains(&x) && (24..76).contains(&y) {
                [60, 90, 230, 255]
            } else {
                [255, 255, 255, 255]
            }
        });
        let o = TraceOptions { mode: TraceMode::Both, colors: 2, threshold: 0.5, ..Default::default() };
        let r = trace(&d, 100, 100, &o, &none);
        assert!(!r.lines.is_empty() && !r.fills.is_empty());
        assert!(r.fills.iter().any(|f| f.color[2] > 150), "the blue fill");
    }

    #[test]
    fn settings_shrink_with_the_picture() {
        let o = TraceOptions { detail: 100, min_area: 400, min_length: 20.0, close_gaps: 4, ..Default::default() };
        let s = o.scaled(0.5);
        assert_eq!((s.detail, s.min_area, s.min_length, s.close_gaps), (25, 100, 10.0, 2));
        assert_eq!((PRESETS[2].opts)(&o).mode, TraceMode::Colors);
        assert_eq!(PRESETS.len(), 5);
    }

    #[test]
    fn tiny_or_empty_pictures_do_not_crash() {
        assert!(trace(&[0; 16], 2, 2, &TraceOptions::default(), &none).lines.is_empty());
        let blank = rgba(50, 50, |_, _| [0; 4]);
        let r = trace(&blank, 50, 50, &TraceOptions { mode: TraceMode::Both, ..Default::default() }, &none);
        assert!(r.lines.is_empty() && r.fills.is_empty());
    }
}
