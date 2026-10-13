// SPDX-License-Identifier: GPL-3.0-or-later
// Selections: a soft mask (0..255 per pixel) kept in the same 256×256 tiles as the layers.
// Missing tiles are "not selected". Built from rectangles, ellipses, lassos and the magic wand,
// combined (new / add / subtract / intersect), inverted, grown, shrunk and feathered.
// Also the shared pixel helpers for flood fills and distance-based growing.

use std::collections::{HashMap, HashSet};
use std::sync::{Arc, OnceLock};
use std::sync::atomic::{AtomicU64, Ordering};

use eframe::egui::{Pos2, pos2};
use rayon::prelude::*;

use crate::doc::{TILE, TileKey};

pub type MaskTile = Arc<Vec<u8>>;

static NEXT_ID: AtomicU64 = AtomicU64::new(1);

fn new_id() -> u64 {
    NEXT_ID.fetch_add(1, Ordering::Relaxed)
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Combine {
    New,
    Add,
    Subtract,
    Intersect,
}

impl Combine {
    pub const ALL: [(Combine, &'static str); 4] =
        [(Combine::New, "New"), (Combine::Add, "Add"), (Combine::Subtract, "Subtract"), (Combine::Intersect, "Intersect")];
}

#[derive(Clone, Debug)]
pub struct Selection {
    /// Changes whenever the selection changes (history compares it).
    pub id: u64,
    pub tiles: HashMap<TileKey, MaskTile>,
    /// Bounds, worked out once (a selection never changes after it is made).
    bounds: Arc<OnceLock<Option<(i32, i32, i32, i32)>>>,
}

/// A rectangle of the canvas as one plain buffer.
pub struct Dense {
    pub x0: i32,
    pub y0: i32,
    pub w: usize,
    pub h: usize,
    pub px: Vec<u8>,
}

impl Selection {
    fn make(tiles: HashMap<TileKey, MaskTile>) -> Selection {
        Selection { id: new_id(), tiles, bounds: Arc::default() }
    }

    #[cfg_attr(not(test), allow(dead_code))]
    pub fn at(&self, x: i32, y: i32) -> u8 {
        let t = TILE as i32;
        match self.tiles.get(&(x.div_euclid(t), y.div_euclid(t))) {
            Some(m) => m[(y.rem_euclid(t) * t + x.rem_euclid(t)) as usize],
            None => 0,
        }
    }

    pub fn tile(&self, key: TileKey) -> Option<&[u8]> {
        self.tiles.get(&key).map(|t| t.as_slice())
    }

    /// Everything on the canvas.
    pub fn all(w: u32, h: u32) -> Selection {
        let d = Dense { x0: 0, y0: 0, w: w as usize, h: h as usize, px: vec![255; w as usize * h as usize] };
        Selection::from_dense(&d, w, h).expect("canvas is not empty")
    }

    /// Smallest pixel rectangle `(x0, y0, x1, y1)` holding everything selected.
    pub fn bounds(&self) -> Option<(i32, i32, i32, i32)> {
        *self.bounds.get_or_init(|| self.find_bounds())
    }

    fn find_bounds(&self) -> Option<(i32, i32, i32, i32)> {
        let t = TILE as i32;
        let found: Vec<(i32, i32, i32, i32)> = self
            .tiles
            .par_iter()
            .filter_map(|(k, m)| {
                let (mut x0, mut y0, mut x1, mut y1) = (i32::MAX, i32::MAX, i32::MIN, i32::MIN);
                for j in 0..t {
                    for i in 0..t {
                        if m[(j * t + i) as usize] != 0 {
                            x0 = x0.min(i);
                            x1 = x1.max(i + 1);
                            y0 = y0.min(j);
                            y1 = y1.max(j + 1);
                        }
                    }
                }
                (x0 < x1).then_some((k.0 * t + x0, k.1 * t + y0, k.0 * t + x1, k.1 * t + y1))
            })
            .collect();
        found.into_iter().reduce(|a, b| (a.0.min(b.0), a.1.min(b.1), a.2.max(b.2), a.3.max(b.3)))
    }

    pub fn dense(&self, x0: i32, y0: i32, w: usize, h: usize) -> Dense {
        let mut px = vec![0u8; w * h];
        let t = TILE as i32;
        px.par_chunks_mut(w).enumerate().for_each(|(j, row)| {
            let y = y0 + j as i32;
            for (i, v) in row.iter_mut().enumerate() {
                let x = x0 + i as i32;
                if let Some(m) = self.tiles.get(&(x.div_euclid(t), y.div_euclid(t))) {
                    *v = m[(y.rem_euclid(t) * t + x.rem_euclid(t)) as usize];
                }
            }
        });
        Dense { x0, y0, w, h, px }
    }

    /// Tiles from a buffer (clipped to the canvas). None when nothing is selected.
    pub fn from_dense(d: &Dense, doc_w: u32, doc_h: u32) -> Option<Selection> {
        let t = TILE as i32;
        let (x0, y0) = (d.x0.max(0), d.y0.max(0));
        let x1 = (d.x0 + d.w as i32).min(doc_w as i32);
        let y1 = (d.y0 + d.h as i32).min(doc_h as i32);
        if x1 <= x0 || y1 <= y0 {
            return None;
        }
        let keys: Vec<TileKey> = (y0.div_euclid(t)..=(y1 - 1).div_euclid(t))
            .flat_map(|ty| (x0.div_euclid(t)..=(x1 - 1).div_euclid(t)).map(move |tx| (tx, ty)))
            .collect();
        let full: MaskTile = Arc::new(vec![255u8; TILE * TILE]);
        let tiles: HashMap<TileKey, MaskTile> = keys
            .into_par_iter()
            .filter_map(|(tx, ty)| {
                let mut m = vec![0u8; TILE * TILE];
                let (mut any, mut all) = (false, true);
                for j in 0..t {
                    let y = ty * t + j;
                    for i in 0..t {
                        let x = tx * t + i;
                        let v = if x >= x0 && x < x1 && y >= y0 && y < y1 {
                            d.px[(y - d.y0) as usize * d.w + (x - d.x0) as usize]
                        } else {
                            0
                        };
                        m[(j * t + i) as usize] = v;
                        any |= v != 0;
                        all &= v == 255;
                    }
                }
                any.then(|| ((tx, ty), if all { full.clone() } else { Arc::new(m) }))
            })
            .collect();
        (!tiles.is_empty()).then(|| Selection::make(tiles))
    }

    /// A filled polygon (nonzero winding), soft-edged when `smooth`.
    pub fn polygon(pts: &[Pos2], doc_w: u32, doc_h: u32, smooth: bool) -> Option<Selection> {
        let d = raster_polygon(pts, doc_w, doc_h, smooth)?;
        Selection::from_dense(&d, doc_w, doc_h)
    }

    /// `new` combined into `old`.
    pub fn combine(old: Option<&Selection>, new: Option<Selection>, how: Combine) -> Option<Selection> {
        let old = match (how, old) {
            (Combine::New, _) | (_, None) => {
                return match how {
                    Combine::Subtract | Combine::Intersect if old.is_none() => None,
                    _ => new,
                };
            }
            (_, Some(o)) => o,
        };
        let empty = Selection { id: 0, tiles: HashMap::new(), bounds: Arc::default() };
        let new = new.unwrap_or(empty);
        let keys: HashSet<TileKey> = old.tiles.keys().chain(new.tiles.keys()).copied().collect();
        let zero = vec![0u8; TILE * TILE];
        let tiles: HashMap<TileKey, MaskTile> = keys
            .into_par_iter()
            .filter_map(|k| {
                let a = old.tiles.get(&k).map(|t| t.as_slice()).unwrap_or(&zero);
                let b = new.tiles.get(&k).map(|t| t.as_slice()).unwrap_or(&zero);
                let m: Vec<u8> = a
                    .iter()
                    .zip(b)
                    .map(|(&a, &b)| match how {
                        Combine::Add | Combine::New => a.max(b),
                        Combine::Subtract => ((a as u32 * (255 - b as u32) + 127) / 255) as u8,
                        Combine::Intersect => ((a as u32 * b as u32 + 127) / 255) as u8,
                    })
                    .collect();
                m.iter().any(|&v| v != 0).then(|| (k, Arc::new(m)))
            })
            .collect();
        (!tiles.is_empty()).then(|| Selection::make(tiles))
    }

    /// Everything that was not selected (no selection → nothing, like "select all" inverted).
    pub fn inverted(sel: Option<&Selection>, doc_w: u32, doc_h: u32) -> Option<Selection> {
        let Some(s) = sel else { return None };
        let mut d = s.dense(0, 0, doc_w as usize, doc_h as usize);
        d.px.par_iter_mut().for_each(|v| *v = 255 - *v);
        Selection::from_dense(&d, doc_w, doc_h)
    }

    /// Grow (`r > 0`) or shrink (`r < 0`) by `r` pixels with round corners.
    pub fn grown(&self, r: f32, doc_w: u32, doc_h: u32) -> Option<Selection> {
        let (x0, y0, x1, y1) = self.bounds()?;
        let pad = r.abs().ceil() as i32 + 2;
        let (bx0, by0) = ((x0 - pad).max(0), (y0 - pad).max(0));
        let (bx1, by1) = ((x1 + pad).min(doc_w as i32), (y1 + pad).min(doc_h as i32));
        let (w, h) = ((bx1 - bx0) as usize, (by1 - by0) as usize);
        let mut d = self.dense(bx0, by0, w, h);
        if r >= 0.0 {
            let inside: Vec<bool> = d.px.iter().map(|&v| v >= 128).collect();
            let dist = distance(&inside, w, h);
            d.px.par_iter_mut().zip(dist.par_iter()).for_each(|(v, &dd)| {
                let c = ((r + 0.5 - dd.sqrt()).clamp(0.0, 1.0) * 255.0).round() as u8;
                *v = (*v).max(c);
            });
        } else {
            // distance to the nearest unselected pixel; the canvas edge counts as unselected
            let (pw, ph) = (w + 2, h + 2);
            let mut outside = vec![true; pw * ph];
            for j in 0..h {
                for i in 0..w {
                    outside[(j + 1) * pw + i + 1] = d.px[j * w + i] < 128;
                }
            }
            let dist = distance(&outside, pw, ph);
            let rr = -r;
            d.px.par_chunks_mut(w).enumerate().for_each(|(j, row)| {
                for (i, v) in row.iter_mut().enumerate() {
                    let dd = dist[(j + 1) * pw + i + 1].sqrt();
                    let c = ((dd - rr - 0.5).clamp(0.0, 1.0) * 255.0).round() as u8;
                    *v = (*v).min(c);
                }
            });
        }
        Selection::from_dense(&d, doc_w, doc_h)
    }

    /// Soft edge of about `r` pixels.
    pub fn feathered(&self, r: f32, doc_w: u32, doc_h: u32) -> Option<Selection> {
        if r < 0.5 {
            return Some(self.clone());
        }
        let (x0, y0, x1, y1) = self.bounds()?;
        let pad = (r * 2.0).ceil() as i32 + 2;
        let (bx0, by0) = ((x0 - pad).max(0), (y0 - pad).max(0));
        let (bx1, by1) = ((x1 + pad).min(doc_w as i32), (y1 + pad).min(doc_h as i32));
        let (w, h) = ((bx1 - bx0) as usize, (by1 - by0) as usize);
        let d = self.dense(bx0, by0, w, h);
        let mut f: Vec<f32> = d.px.iter().map(|&v| v as f32).collect();
        gaussian(&mut f, w, h, r / 2.0);
        let px = f.iter().map(|&v| v.round().clamp(0.0, 255.0) as u8).collect();
        Selection::from_dense(&Dense { px, ..d }, doc_w, doc_h)
    }

    /// The outline (where the mask crosses 50%) as closed lines in canvas pixels, for the marching ants.
    pub fn outline(&self) -> Vec<Vec<Pos2>> {
        let Some((x0, y0, x1, y1)) = self.bounds() else { return Vec::new() };
        let (bx0, by0) = (x0 - 1, y0 - 1);
        let (w, h) = ((x1 - x0 + 2) as usize, (y1 - y0 + 2) as usize);
        let d = self.dense(bx0, by0, w, h);
        let on = |x: i32, y: i32| x >= 0 && y >= 0 && (x as usize) < w && (y as usize) < h && d.px[y as usize * w + x as usize] >= 128;
        // unit edges, directed so "selected" is on the left: chains then close by themselves
        let mut next: HashMap<(i32, i32), Vec<(i32, i32)>> = HashMap::new();
        let mut add = |a: (i32, i32), b: (i32, i32)| next.entry(a).or_default().push(b);
        for y in 0..=h as i32 {
            for x in 0..=w as i32 {
                let c = on(x, y);
                if c != on(x, y - 1) {
                    // horizontal edge on top of (x, y)
                    if c { add((x + 1, y), (x, y)) } else { add((x, y), (x + 1, y)) }
                }
                if c != on(x - 1, y) {
                    if c { add((x, y), (x, y + 1)) } else { add((x, y + 1), (x, y)) }
                }
            }
        }
        let mut lines = Vec::new();
        let starts: Vec<(i32, i32)> = next.keys().copied().collect();
        for s in starts {
            while let Some(first) = next.get_mut(&s).and_then(|v| v.pop()) {
                let mut pts = vec![s];
                let mut cur = first;
                let mut guard = 0;
                while cur != s && guard < 10_000_000 {
                    pts.push(cur);
                    match next.get_mut(&cur).and_then(|v| v.pop()) {
                        Some(n) => cur = n,
                        None => break,
                    }
                    guard += 1;
                }
                // keep only the corners (the loop is closed, so look across the start too)
                let n = pts.len();
                let mut simple: Vec<Pos2> = Vec::with_capacity(n / 2 + 1);
                for i in 0..n {
                    let (a, p, b) = (pts[(i + n - 1) % n], pts[i], pts[(i + 1) % n]);
                    if (a.0 == p.0 && p.0 == b.0) || (a.1 == p.1 && p.1 == b.1) {
                        continue;
                    }
                    simple.push(pos2((p.0 + bx0) as f32, (p.1 + by0) as f32));
                }
                if let Some(&f) = simple.first() {
                    simple.push(f);
                }
                lines.push(simple);
            }
        }
        lines
    }
}

/// Scanline polygon fill with 4 sub-rows and exact horizontal coverage.
pub fn raster_polygon(pts: &[Pos2], doc_w: u32, doc_h: u32, smooth: bool) -> Option<Dense> {
    if pts.len() < 3 {
        return None;
    }
    let (mut minx, mut miny, mut maxx, mut maxy) = (f32::MAX, f32::MAX, f32::MIN, f32::MIN);
    for p in pts {
        minx = minx.min(p.x);
        miny = miny.min(p.y);
        maxx = maxx.max(p.x);
        maxy = maxy.max(p.y);
    }
    let x0 = (minx.floor() as i32).max(0);
    let y0 = (miny.floor() as i32).max(0);
    let x1 = (maxx.ceil() as i32).min(doc_w as i32);
    let y1 = (maxy.ceil() as i32).min(doc_h as i32);
    if x1 <= x0 || y1 <= y0 {
        return None;
    }
    let (w, h) = ((x1 - x0) as usize, (y1 - y0) as usize);
    let edges: Vec<(Pos2, Pos2)> = (0..pts.len()).map(|i| (pts[i], pts[(i + 1) % pts.len()])).filter(|(a, b)| a.y != b.y).collect();
    let mut px = vec![0u8; w * h];
    const SUB: usize = 4;
    px.par_chunks_mut(w).enumerate().for_each(|(j, row)| {
        let mut cov = vec![0f32; w];
        let mut xs: Vec<(f32, i32)> = Vec::new();
        for s in 0..SUB {
            let sy = (y0 + j as i32) as f32 + (s as f32 + 0.5) / SUB as f32;
            xs.clear();
            for (a, b) in &edges {
                let (lo, hi, dir) = if a.y < b.y { (a, b, 1) } else { (b, a, -1) };
                if sy >= lo.y && sy < hi.y {
                    let t = (sy - lo.y) / (hi.y - lo.y);
                    xs.push((lo.x + (hi.x - lo.x) * t, dir));
                }
            }
            xs.sort_by(|a, b| a.0.total_cmp(&b.0));
            let mut wind = 0;
            for k in 0..xs.len() {
                let before = wind;
                wind += xs[k].1;
                if before == 0 && wind != 0 {
                    // span starts here; find where winding returns to zero
                    let mut w2 = wind;
                    let mut e = k + 1;
                    while e < xs.len() {
                        w2 += xs[e].1;
                        if w2 == 0 {
                            break;
                        }
                        e += 1;
                    }
                    let (xa, xb) = (xs[k].0 - x0 as f32, xs.get(e).map(|v| v.0).unwrap_or(maxx) - x0 as f32);
                    let (xa, xb) = (xa.max(0.0), xb.min(w as f32));
                    if xb > xa {
                        let (ia, ib) = (xa.floor() as usize, (xb.ceil() as usize).min(w));
                        for (i, c) in cov.iter_mut().enumerate().take(ib).skip(ia) {
                            let part = (xb.min(i as f32 + 1.0) - xa.max(i as f32)).clamp(0.0, 1.0);
                            *c += part / SUB as f32;
                        }
                    }
                }
            }
        }
        for (v, c) in row.iter_mut().zip(&cov) {
            let c = c.min(1.0);
            *v = if smooth { (c * 255.0).round() as u8 } else if c >= 0.5 { 255 } else { 0 };
        }
    });
    Some(Dense { x0, y0, w, h, px })
}

/// Squared distance from every pixel to the nearest `true` pixel (Felzenszwalb & Huttenlocher).
pub fn distance(set: &[bool], w: usize, h: usize) -> Vec<f32> {
    const INF: f32 = 1e20;
    let mut g: Vec<f32> = set.iter().map(|&s| if s { 0.0 } else { INF }).collect();
    // columns
    let mut cols: Vec<Vec<f32>> = (0..w).into_par_iter().map(|x| edt_1d(&(0..h).map(|y| g[y * w + x]).collect::<Vec<_>>())).collect();
    for (x, c) in cols.iter_mut().enumerate() {
        for (y, v) in c.iter().enumerate() {
            g[y * w + x] = *v;
        }
    }
    // rows
    g.par_chunks_mut(w).for_each(|row| {
        let r = edt_1d(row);
        row.copy_from_slice(&r);
    });
    g
}

fn edt_1d(f: &[f32]) -> Vec<f32> {
    let n = f.len();
    let mut d = vec![0f32; n];
    if n == 0 {
        return d;
    }
    let mut v = vec![0usize; n];
    let mut z = vec![0f32; n + 1];
    let mut k = 0;
    z[0] = f32::NEG_INFINITY;
    z[1] = f32::INFINITY;
    let inter = |q: usize, p: usize| ((f[q] + (q * q) as f32) - (f[p] + (p * p) as f32)) / (2.0 * q as f32 - 2.0 * p as f32);
    for q in 1..n {
        let mut s = inter(q, v[k]);
        while s <= z[k] && k > 0 {
            k -= 1;
            s = inter(q, v[k]);
        }
        if s <= z[k] {
            // only possible at k == 0 with z[0] = -inf; keep the parabola order valid
            v[0] = q;
            z[1] = f32::INFINITY;
            continue;
        }
        k += 1;
        v[k] = q;
        z[k] = s;
        z[k + 1] = f32::INFINITY;
    }
    k = 0;
    for (q, out) in d.iter_mut().enumerate() {
        while z[k + 1] < q as f32 {
            k += 1;
        }
        let p = v[k];
        *out = (q as f32 - p as f32).powi(2) + f[p];
    }
    d
}

/// Three box blurs ≈ a gaussian with this sigma, in place.
pub fn gaussian(f: &mut [f32], w: usize, h: usize, sigma: f32) {
    if sigma < 0.3 {
        return;
    }
    let r = (((12.0 * sigma * sigma / 3.0 + 1.0).sqrt() - 1.0) / 2.0).round().max(1.0) as usize;
    for _ in 0..3 {
        f.par_chunks_mut(w).for_each(|row| box_1d(row, r));
        let mut cols: Vec<Vec<f32>> = (0..w)
            .into_par_iter()
            .map(|x| {
                let mut c: Vec<f32> = (0..h).map(|y| f[y * w + x]).collect();
                box_1d(&mut c, r);
                c
            })
            .collect();
        for (x, c) in cols.iter_mut().enumerate() {
            for (y, v) in c.iter().enumerate() {
                f[y * w + x] = *v;
            }
        }
    }
}

fn box_1d(v: &mut [f32], r: usize) {
    let n = v.len();
    if n == 0 {
        return;
    }
    let src = v.to_vec();
    let at = |i: isize| src[i.clamp(0, n as isize - 1) as usize];
    let k = 1.0 / (2 * r + 1) as f32;
    let mut acc: f32 = (-(r as isize)..=r as isize).map(at).sum();
    for i in 0..n {
        v[i] = acc * k;
        acc += at(i as isize + r as isize + 1) - at(i as isize - r as isize);
    }
}

/// Scanline flood fill: the pixels `ok` connected to `(sx, sy)` (4-neighbours).
pub fn flood(ok: &[bool], w: usize, h: usize, sx: usize, sy: usize) -> Vec<bool> {
    let mut out = vec![false; w * h];
    if sx >= w || sy >= h || !ok[sy * w + sx] {
        return out;
    }
    let mut stack = vec![(sx, sy)];
    while let Some((x, y)) = stack.pop() {
        let row = y * w;
        if out[row + x] || !ok[row + x] {
            continue;
        }
        let mut a = x;
        while a > 0 && ok[row + a - 1] && !out[row + a - 1] {
            a -= 1;
        }
        let mut b = x;
        while b + 1 < w && ok[row + b + 1] && !out[row + b + 1] {
            b += 1;
        }
        for i in a..=b {
            out[row + i] = true;
        }
        for ny in [y.wrapping_sub(1), y + 1] {
            if ny >= h {
                continue;
            }
            let nrow = ny * w;
            let mut i = a;
            while i <= b {
                if ok[nrow + i] && !out[nrow + i] {
                    stack.push((i, ny));
                    while i <= b && ok[nrow + i] {
                        i += 1;
                    }
                } else {
                    i += 1;
                }
            }
        }
    }
    out
}

/// Pixels whose color is within `tol` (0..255) of `c` (transparent matches transparent).
pub fn similar(img: &image::RgbaImage, c: [u8; 4], tol: u8) -> Vec<bool> {
    img.as_raw()
        .par_chunks(4)
        .map(|p| {
            if c[3] == 0 {
                return p[3] <= tol;
            }
            (0..4).all(|k| (p[k] as i32 - c[k] as i32).unsigned_abs() <= tol as u32)
        })
        .collect()
}

/// Magic wand: the area around `(x, y)` with a similar color (or every such pixel when not contiguous).
pub fn wand(img: &image::RgbaImage, x: u32, y: u32, tol: u8, contiguous: bool, smooth: bool) -> Option<Selection> {
    let (w, h) = (img.width() as usize, img.height() as usize);
    if x as usize >= w || y as usize >= h {
        return None;
    }
    let c = img.get_pixel(x, y).0;
    let ok = similar(img, c, tol);
    let region = if contiguous { flood(&ok, w, h, x as usize, y as usize) } else { ok };
    let mut px: Vec<u8> = region.iter().map(|&b| if b { 255 } else { 0 }).collect();
    if smooth {
        // a half-pixel soft edge so the outline isn't jagged when painted into
        let mut f: Vec<f32> = px.iter().map(|&v| v as f32).collect();
        gaussian(&mut f, w, h, 0.5);
        for (v, f) in px.iter_mut().zip(&f) {
            if *v == 255 {
                *v = (f.round() as u8).max(128);
            } else {
                *v = (f.round() as u8).min(127);
            }
        }
    }
    Selection::from_dense(&Dense { x0: 0, y0: 0, w, h, px }, w as u32, h as u32)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rect(x0: f32, y0: f32, x1: f32, y1: f32) -> Vec<Pos2> {
        vec![pos2(x0, y0), pos2(x1, y0), pos2(x1, y1), pos2(x0, y1)]
    }

    #[test]
    fn rectangle_exact_and_soft_edges() {
        let s = Selection::polygon(&rect(10.0, 20.0, 300.0, 40.0), 400, 400, true).unwrap();
        assert_eq!(s.bounds(), Some((10, 20, 300, 40)));
        assert_eq!(s.at(10, 20), 255);
        assert_eq!(s.at(9, 20), 0);
        assert_eq!(s.at(299, 39), 255);
        let half = Selection::polygon(&rect(10.5, 20.0, 50.0, 40.0), 400, 400, true).unwrap();
        assert!((half.at(10, 25) as i32 - 128).abs() <= 1);
    }

    #[test]
    fn combine_modes() {
        let a = Selection::polygon(&rect(0.0, 0.0, 100.0, 100.0), 300, 300, false);
        let b = Selection::polygon(&rect(50.0, 50.0, 150.0, 150.0), 300, 300, false);
        let add = Selection::combine(a.as_ref(), b.clone(), Combine::Add).unwrap();
        assert_eq!(add.bounds(), Some((0, 0, 150, 150)));
        let sub = Selection::combine(a.as_ref(), b.clone(), Combine::Subtract).unwrap();
        assert_eq!((sub.at(10, 10), sub.at(60, 60)), (255, 0));
        let int = Selection::combine(a.as_ref(), b, Combine::Intersect).unwrap();
        assert_eq!(int.bounds(), Some((50, 50, 100, 100)));
        let inv = Selection::inverted(Some(&int), 300, 300).unwrap();
        assert_eq!((inv.at(60, 60), inv.at(0, 0), inv.at(299, 299)), (0, 255, 255));
    }

    #[test]
    fn grow_shrink_feather() {
        let a = Selection::polygon(&rect(100.0, 100.0, 200.0, 200.0), 400, 400, false).unwrap();
        let g = a.grown(10.0, 400, 400).unwrap();
        assert_eq!(g.bounds(), Some((90, 90, 210, 210)));
        let s = a.grown(-10.0, 400, 400).unwrap();
        assert_eq!(s.bounds(), Some((110, 110, 190, 190)));
        let all = Selection::all(400, 400).grown(-5.0, 400, 400).unwrap();
        assert_eq!(all.bounds(), Some((5, 5, 395, 395)));
        let f = a.feathered(8.0, 400, 400).unwrap();
        assert!(f.at(100, 150) > 60 && f.at(100, 150) < 200, "{}", f.at(100, 150));
        assert_eq!(f.at(150, 150), 255);
    }

    #[test]
    fn outline_is_closed() {
        let a = Selection::polygon(&rect(10.0, 10.0, 20.0, 30.0), 100, 100, false).unwrap();
        let lines = a.outline();
        assert_eq!(lines.len(), 1);
        assert_eq!(lines[0].first(), lines[0].last());
        assert_eq!(lines[0].len(), 5); // 4 corners + back to start
    }

    #[test]
    fn distance_transform() {
        let mut set = vec![false; 25];
        set[12] = true;
        let d = distance(&set, 5, 5);
        assert_eq!(d[12], 0.0);
        assert_eq!(d[0], 8.0);
        assert_eq!(d[14], 4.0);
    }

    #[test]
    fn wand_stops_at_lines() {
        let mut img = image::RgbaImage::from_pixel(50, 50, image::Rgba([255, 255, 255, 255]));
        for y in 0..50 {
            img.put_pixel(25, y, image::Rgba([0, 0, 0, 255]));
        }
        let s = wand(&img, 5, 5, 30, true, false).unwrap();
        assert_eq!(s.bounds(), Some((0, 0, 25, 50)));
        let all = wand(&img, 5, 5, 30, false, false).unwrap();
        assert_eq!(all.bounds(), Some((0, 0, 50, 50)));
        assert_eq!(all.at(25, 3), 0);
    }
}
