// SPDX-License-Identifier: GPL-3.0-or-later
// Move & transform: the selected pixels (or the whole layer) are lifted off the layer and
// float until applied. The floating picture is placed by its 4 corners, so one model covers
// move, scale, rotate, flip, skew and perspective. Applying resamples it back into tiles
// (bilinear, with extra samples when it got smaller); a pure move by whole pixels is copied
// exactly. The selection moves along.

use std::collections::HashMap;

use eframe::egui::{Color32, Pos2, Vec2, pos2, vec2};
use rayon::prelude::*;

use crate::blend::{self, Px};
use crate::doc::{Document, LayerId, TILE, TileData, TileKey};
use crate::select::{Dense, Selection};

/// Maps the unit square onto a 4-corner shape (corners: top-left, top-right, bottom-right, bottom-left).
#[derive(Clone, Copy, Debug)]
pub struct Homography([f32; 9]);

impl Homography {
    pub fn square_to_quad(q: &[Pos2; 4]) -> Homography {
        let sx = q[0].x - q[1].x + q[2].x - q[3].x;
        let sy = q[0].y - q[1].y + q[2].y - q[3].y;
        let (g, h) = if sx.abs() < 1e-6 && sy.abs() < 1e-6 {
            (0.0, 0.0)
        } else {
            let (dx1, dx2) = (q[1].x - q[2].x, q[3].x - q[2].x);
            let (dy1, dy2) = (q[1].y - q[2].y, q[3].y - q[2].y);
            let den = dx1 * dy2 - dx2 * dy1;
            if den.abs() < 1e-9 { (0.0, 0.0) } else { ((sx * dy2 - dx2 * sy) / den, (dx1 * sy - sx * dy1) / den) }
        };
        let a = q[1].x - q[0].x + g * q[1].x;
        let b = q[3].x - q[0].x + h * q[3].x;
        let d = q[1].y - q[0].y + g * q[1].y;
        let e = q[3].y - q[0].y + h * q[3].y;
        Homography([a, b, q[0].x, d, e, q[0].y, g, h, 1.0])
    }

    pub fn map(&self, u: f32, v: f32) -> Pos2 {
        let m = &self.0;
        let w = m[6] * u + m[7] * v + m[8];
        pos2((m[0] * u + m[1] * v + m[2]) / w, (m[3] * u + m[4] * v + m[5]) / w)
    }

    pub fn inverse(&self) -> Option<Homography> {
        let m = &self.0;
        let c00 = m[4] * m[8] - m[5] * m[7];
        let c01 = m[5] * m[6] - m[3] * m[8];
        let c02 = m[3] * m[7] - m[4] * m[6];
        let det = m[0] * c00 + m[1] * c01 + m[2] * c02;
        if det.abs() < 1e-12 {
            return None;
        }
        let k = 1.0 / det;
        Some(Homography([
            c00 * k,
            (m[2] * m[7] - m[1] * m[8]) * k,
            (m[1] * m[5] - m[2] * m[4]) * k,
            c01 * k,
            (m[0] * m[8] - m[2] * m[6]) * k,
            (m[2] * m[3] - m[0] * m[5]) * k,
            c02 * k,
            (m[1] * m[6] - m[0] * m[7]) * k,
            (m[0] * m[4] - m[1] * m[3]) * k,
        ]))
    }
}

pub struct Floating {
    pub layer: LayerId,
    /// The lifted pixels (premultiplied), `w × h`, taken from canvas position `(x, y)`.
    pub px: Vec<Color32>,
    pub x: i32,
    pub y: i32,
    pub w: usize,
    pub h: usize,
    /// Selection coverage of the lifted area (None = no selection: the whole layer moves).
    pub mask: Option<Vec<u8>>,
    /// Where the picture is now: top-left, top-right, bottom-right, bottom-left (canvas pixels).
    pub corners: [Pos2; 4],
    /// The layer's tiles before lifting (for Cancel).
    orig: HashMap<TileKey, TileData>,
}

impl Floating {
    /// Lift the selected pixels (or everything) off layer `li`. None when there is nothing to move.
    pub fn lift(doc: &mut Document, li: usize) -> Option<Floating> {
        let (dw, dh) = (doc.w, doc.h);
        let sel = doc.selection.clone();
        let layer = &mut doc.layers[li];
        let (x0, y0, x1, y1) = match &sel {
            Some(s) => {
                let (sx0, sy0, sx1, sy1) = s.bounds()?;
                let (lx0, ly0, lx1, ly1) = crate::files::layer_bounds(layer, dw, dh)?;
                if sx0.max(lx0) >= sx1.min(lx1) || sy0.max(ly0) >= sy1.min(ly1) {
                    return None; // no paint inside the selection
                }
                // the whole selection floats (so it moves along in one piece)
                (sx0, sy0, sx1, sy1)
            }
            None => crate::files::layer_bounds(layer, dw, dh)?,
        };
        let (w, h) = ((x1 - x0) as usize, (y1 - y0) as usize);
        let mask = sel.as_ref().map(|s| s.dense(x0, y0, w, h).px);
        let orig = layer.tiles.clone();
        let t = TILE as i32;
        let mut px = vec![Color32::TRANSPARENT; w * h];
        px.par_chunks_mut(w).enumerate().for_each(|(j, row)| {
            let y = y0 + j as i32;
            for (i, out) in row.iter_mut().enumerate() {
                let x = x0 + i as i32;
                if let Some(tile) = orig.get(&(x.div_euclid(t), y.div_euclid(t))) {
                    let c = tile[(y.rem_euclid(t) * t + x.rem_euclid(t)) as usize];
                    let m = mask.as_ref().map(|m| m[j * w + i]).unwrap_or(255);
                    *out = if m == 255 { c } else { c.gamma_multiply(m as f32 / 255.0) };
                }
            }
        });
        let (fx0, fy0, fx1, fy1) = (x0 as f32, y0 as f32, x1 as f32, y1 as f32);
        let corners = [pos2(fx0, fy0), pos2(fx1, fy0), pos2(fx1, fy1), pos2(fx0, fy1)];
        let f = Floating { layer: layer.id, px, x: x0, y: y0, w, h, mask, corners, orig };
        // take them away from the layer
        for (key, tile) in clear_lifted(&f) {
            match tile {
                Some(t) => {
                    layer.tiles.insert(key, t);
                }
                None => {
                    layer.tiles.remove(&key);
                }
            }
        }
        layer.rev += 1;
        doc.mark_dirty_rect(x0, y0, x1, y1);
        Some(f)
    }

    pub fn center(&self) -> Pos2 {
        let s = self.corners.iter().fold(Vec2::ZERO, |a, c| a + c.to_vec2());
        (s / 4.0).to_pos2()
    }

    /// Canvas bounds of the shape as it is now.
    pub fn bounds(&self) -> (i32, i32, i32, i32) {
        let (mut x0, mut y0, mut x1, mut y1) = (f32::MAX, f32::MAX, f32::MIN, f32::MIN);
        for c in &self.corners {
            x0 = x0.min(c.x);
            y0 = y0.min(c.y);
            x1 = x1.max(c.x);
            y1 = y1.max(c.y);
        }
        (x0.floor() as i32, y0.floor() as i32, x1.ceil() as i32, y1.ceil() as i32)
    }

    /// Moved by whole pixels only (same size, not turned): copied exactly when applied.
    fn integer_move(&self) -> Option<(i32, i32)> {
        let dx = self.corners[0].x - self.x as f32;
        let dy = self.corners[0].y - self.y as f32;
        let ok = (dx - dx.round()).abs() < 1e-3 && (dy - dy.round()).abs() < 1e-3;
        let (w, h) = (self.w as f32, self.h as f32);
        let expect = [vec2(0.0, 0.0), vec2(w, 0.0), vec2(w, h), vec2(0.0, h)];
        let same = self.corners.iter().zip(expect).all(|(c, e)| (*c - (self.corners[0] + e)).length() < 1e-3);
        (ok && same).then(|| (dx.round() as i32, dy.round() as i32))
    }

    pub fn flip_h(&mut self) {
        let c = self.corners;
        self.corners = [c[1], c[0], c[3], c[2]];
    }

    pub fn flip_v(&mut self) {
        let c = self.corners;
        self.corners = [c[3], c[2], c[1], c[0]];
    }

    pub fn rotate(&mut self, rad: f32) {
        let ctr = self.center();
        let (s, c) = rad.sin_cos();
        for p in &mut self.corners {
            let d = *p - ctr;
            *p = ctr + vec2(d.x * c - d.y * s, d.x * s + d.y * c);
        }
    }

    /// Premultiplied pixel with bilinear filtering at source coordinates (pixel centers at +0.5).
    fn sample(&self, u: f32, v: f32) -> Px {
        let (fx, fy) = (u - 0.5, v - 0.5);
        let (x0, y0) = (fx.floor(), fy.floor());
        let (tx, ty) = (fx - x0, fy - y0);
        let (x0, y0) = (x0 as i64, y0 as i64);
        let get = |x: i64, y: i64| -> Px {
            if x < 0 || y < 0 || x >= self.w as i64 || y >= self.h as i64 {
                [0.0; 4]
            } else {
                blend::to_px(self.px[y as usize * self.w + x as usize])
            }
        };
        let (a, b, c, d) = (get(x0, y0), get(x0 + 1, y0), get(x0, y0 + 1), get(x0 + 1, y0 + 1));
        [0, 1, 2, 3].map(|k| (a[k] * (1.0 - tx) + b[k] * tx) * (1.0 - ty) + (c[k] * (1.0 - tx) + d[k] * tx) * ty)
    }

    fn sample_mask(&self, u: f32, v: f32) -> f32 {
        let Some(m) = &self.mask else { return 0.0 };
        let (fx, fy) = (u - 0.5, v - 0.5);
        let (x0, y0) = (fx.floor(), fy.floor());
        let (tx, ty) = (fx - x0, fy - y0);
        let (x0, y0) = (x0 as i64, y0 as i64);
        let get = |x: i64, y: i64| -> f32 {
            if x < 0 || y < 0 || x >= self.w as i64 || y >= self.h as i64 { 0.0 } else { m[y as usize * self.w + x as usize] as f32 }
        };
        (get(x0, y0) * (1.0 - tx) + get(x0 + 1, y0) * tx) * (1.0 - ty) + (get(x0, y0 + 1) * (1.0 - tx) + get(x0 + 1, y0 + 1) * tx) * ty
    }

    /// Put the picture down where it is now. Returns false when the layer is gone.
    pub fn apply(self, doc: &mut Document) -> bool {
        let Some(li) = doc.index_of(self.layer) else { return false };
        let (dw, dh) = (doc.w as i32, doc.h as i32);
        let (bx0, by0, bx1, by1) = self.bounds();
        let (bx0, by0, bx1, by1) = (bx0.max(0), by0.max(0), bx1.min(dw), by1.min(dh));
        let exact = self.integer_move();
        let h = Homography::square_to_quad(&self.corners);
        let Some(inv) = h.inverse() else { return false };
        // more samples per pixel when the picture got smaller (no sparkly holes)
        let area = |q: &[Pos2; 4]| {
            let mut a = 0.0;
            for i in 0..4 {
                let (p, n) = (q[i], q[(i + 1) % 4]);
                a += p.x * n.y - n.x * p.y;
            }
            (a / 2.0).abs()
        };
        let shrink = area(&self.corners) / (self.w * self.h).max(1) as f32;
        let ss = if exact.is_some() { 1 } else if shrink < 0.3 { 3 } else if shrink < 0.8 { 2 } else { 1 };
        let (sw, sh) = (self.w as f32, self.h as f32);
        let place = |x: i32, y: i32| -> (Px, f32) {
            if let Some((dx, dy)) = exact {
                let (u, v) = (x - self.x - dx, y - self.y - dy);
                if u < 0 || v < 0 || u >= self.w as i32 || v >= self.h as i32 {
                    return ([0.0; 4], 0.0);
                }
                let k = v as usize * self.w + u as usize;
                return (blend::to_px(self.px[k]), self.mask.as_ref().map(|m| m[k] as f32).unwrap_or(0.0));
            }
            let mut acc = [0.0f32; 4];
            let mut macc = 0.0;
            let n = (ss * ss) as f32;
            for sy in 0..ss {
                for sx in 0..ss {
                    let p = inv.map(x as f32 + (sx as f32 + 0.5) / ss as f32, y as f32 + (sy as f32 + 0.5) / ss as f32);
                    if p.x < -0.001 || p.y < -0.001 || p.x > 1.001 || p.y > 1.001 {
                        continue;
                    }
                    let c = self.sample(p.x * sw, p.y * sh);
                    for k in 0..4 {
                        acc[k] += c[k];
                    }
                    macc += self.sample_mask(p.x * sw, p.y * sh);
                }
            }
            ([acc[0] / n, acc[1] / n, acc[2] / n, acc[3] / n], macc / n)
        };
        let mut new_mask = Dense { x0: bx0, y0: by0, w: (bx1 - bx0).max(0) as usize, h: (by1 - by0).max(0) as usize, px: Vec::new() };
        new_mask.px = vec![0; new_mask.w * new_mask.h];
        let t = TILE as i32;
        let layer = &mut doc.layers[li];
        if bx1 > bx0 && by1 > by0 {
            let keys: Vec<TileKey> = (by0.div_euclid(t)..=(by1 - 1).div_euclid(t))
                .flat_map(|ty| (bx0.div_euclid(t)..=(bx1 - 1).div_euclid(t)).map(move |tx| (tx, ty)))
                .collect();
            let tiles = &layer.tiles;
            let lock_alpha = layer.lock_alpha;
            let done: Vec<(TileKey, Option<Vec<Color32>>, Vec<(usize, u8)>)> = keys
                .par_iter()
                .map(|&key| {
                    let mut px: Vec<Color32> = tiles.get(&key).map(|t| (**t).clone()).unwrap_or_else(|| vec![Color32::TRANSPARENT; TILE * TILE]);
                    let mut masks = Vec::new();
                    let mut changed = false;
                    for j in 0..t {
                        let y = key.1 * t + j;
                        if y < by0 || y >= by1 {
                            continue;
                        }
                        for i in 0..t {
                            let x = key.0 * t + i;
                            if x < bx0 || x >= bx1 {
                                continue;
                            }
                            let (c, m) = place(x, y);
                            if m > 0.5 {
                                masks.push(((y - by0) as usize * (bx1 - bx0) as usize + (x - bx0) as usize, m.round().min(255.0) as u8));
                            }
                            if c[3] <= 0.0002 {
                                continue;
                            }
                            let k = (j * t + i) as usize;
                            let d = blend::to_px(px[k]);
                            let out = if lock_alpha {
                                d // a layer with locked transparency keeps its shape
                            } else {
                                let f = 1.0 - c[3];
                                [c[0] + d[0] * f, c[1] + d[1] * f, c[2] + d[2] * f, c[3] + d[3] * f]
                            };
                            px[k] = blend::from_px(out);
                            changed = true;
                        }
                    }
                    (key, changed.then_some(px), masks)
                })
                .collect();
            for (key, px, masks) in done {
                for (i, m) in masks {
                    new_mask.px[i] = m;
                }
                if let Some(px) = px {
                    if px.iter().all(|c| c.a() == 0) {
                        layer.tiles.remove(&key);
                    } else {
                        *layer.tile_mut(key) = px;
                    }
                }
            }
        }
        layer.rev += 1;
        if self.mask.is_some() {
            doc.selection = Selection::from_dense(&new_mask, doc.w, doc.h);
        }
        doc.mark_all_dirty();
        true
    }

    /// Put everything back as it was.
    pub fn cancel(self, doc: &mut Document) {
        if let Some(li) = doc.index_of(self.layer) {
            doc.layers[li].tiles = self.orig;
            doc.layers[li].rev += 1;
            doc.mark_all_dirty();
        }
    }

    /// The lifted picture as an image for the on-screen preview (at most `max` px wide/high).
    pub fn preview(&self, max: usize) -> eframe::egui::ColorImage {
        let k = (max as f32 / self.w.max(self.h) as f32).min(1.0);
        let (pw, ph) = (((self.w as f32 * k).round() as usize).max(1), ((self.h as f32 * k).round() as usize).max(1));
        let mut out = vec![Color32::TRANSPARENT; pw * ph];
        out.par_chunks_mut(pw).enumerate().for_each(|(j, row)| {
            for (i, o) in row.iter_mut().enumerate() {
                let (u, v) = (((i as f32 + 0.5) / k) as usize, ((j as f32 + 0.5) / k) as usize);
                *o = self.px[v.min(self.h - 1) * self.w + u.min(self.w - 1)];
            }
        });
        eframe::egui::ColorImage { size: [pw, ph], pixels: out, source_size: eframe::egui::vec2(pw as f32, ph as f32) }
    }
}

/// The layer's tiles inside the lifted area with the lifted pixels taken out (same math as `lift`).
fn clear_lifted(f: &Floating) -> Vec<(TileKey, Option<TileData>)> {
    let t = TILE as i32;
    let (x0, y0, x1, y1) = (f.x, f.y, f.x + f.w as i32, f.y + f.h as i32);
    let mut out = Vec::new();
    for ty in y0.div_euclid(t)..=(y1 - 1).div_euclid(t) {
        for tx in x0.div_euclid(t)..=(x1 - 1).div_euclid(t) {
            let key = (tx, ty);
            let Some(orig) = f.orig.get(&key) else { continue };
            let mut tile = (**orig).clone();
            for j in 0..t {
                let y = ty * t + j;
                if y < y0 || y >= y1 {
                    continue;
                }
                for i in 0..t {
                    let x = tx * t + i;
                    if x < x0 || x >= x1 {
                        continue;
                    }
                    let k = (j * t + i) as usize;
                    let m = f.mask.as_ref().map(|m| m[(y - y0) as usize * f.w + (x - x0) as usize]).unwrap_or(255);
                    tile[k] = if m == 255 { Color32::TRANSPARENT } else { tile[k].gamma_multiply(1.0 - m as f32 / 255.0) };
                }
            }
            out.push((key, if tile.iter().all(|c| c.a() == 0) { None } else { Some(std::sync::Arc::new(tile)) }));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::files::{layer_rgba, tiles_from_rgba};

    fn doc_with_square() -> Document {
        let mut d = Document::new(400, 300);
        let img = image::RgbaImage::from_pixel(40, 30, image::Rgba([200, 30, 30, 255]));
        d.layers[0].tiles = tiles_from_rgba(&img, 20, 10, 400, 300);
        d
    }

    #[test]
    fn homography_round_trip() {
        let q = [pos2(10.0, 20.0), pos2(200.0, 40.0), pos2(180.0, 220.0), pos2(30.0, 160.0)];
        let h = Homography::square_to_quad(&q);
        let p = h.map(1.0, 1.0);
        assert!((p - q[2]).length() < 1e-3);
        let inv = h.inverse().unwrap();
        let back = inv.map(p.x, p.y);
        assert!((back.x - 1.0).abs() < 1e-3 && (back.y - 1.0).abs() < 1e-3);
    }

    #[test]
    fn move_by_whole_pixels_is_exact() {
        let mut d = doc_with_square();
        let mut f = Floating::lift(&mut d, 0).unwrap();
        assert!(d.layers[0].tiles.is_empty(), "lifted pixels leave the layer");
        for c in &mut f.corners {
            *c += vec2(100.0, 50.0);
        }
        assert!(f.apply(&mut d));
        let img = layer_rgba(&d.layers[0], 0, 0, 400, 300);
        assert_eq!(img.get_pixel(120, 60).0, [200, 30, 30, 255]);
        assert_eq!(img.get_pixel(159, 89).0, [200, 30, 30, 255]);
        assert_eq!(img.get_pixel(160, 60).0[3], 0);
        assert_eq!(img.get_pixel(25, 15).0[3], 0, "old place is empty");
    }

    #[test]
    fn scale_and_selection_move_along() {
        let mut d = doc_with_square();
        d.selection = Selection::polygon(&[pos2(20.0, 10.0), pos2(40.0, 10.0), pos2(40.0, 40.0), pos2(20.0, 40.0)], 400, 300, false);
        let mut f = Floating::lift(&mut d, 0).unwrap();
        assert_eq!((f.w, f.h), (20, 30));
        // the unselected right half stays on the layer
        assert_eq!(layer_rgba(&d.layers[0], 0, 0, 400, 300).get_pixel(50, 20).0[3], 255);
        f.corners = [pos2(200.0, 100.0), pos2(240.0, 100.0), pos2(240.0, 160.0), pos2(200.0, 160.0)];
        assert!(f.apply(&mut d));
        let img = layer_rgba(&d.layers[0], 0, 0, 400, 300);
        assert_eq!(img.get_pixel(220, 130).0[3], 255);
        assert_eq!(img.get_pixel(50, 20).0[3], 255);
        assert_eq!(img.get_pixel(25, 20).0[3], 0);
        let s = d.selection.as_ref().unwrap();
        assert_eq!(s.at(220, 130), 255);
        assert_eq!(s.at(25, 20), 0);
    }

    #[test]
    fn cancel_restores() {
        let mut d = doc_with_square();
        let before = d.layers[0].tiles.clone();
        let mut f = Floating::lift(&mut d, 0).unwrap();
        f.rotate(0.5);
        f.cancel(&mut d);
        assert_eq!(d.layers[0].tiles.len(), before.len());
        assert!(d.layers[0].tiles.iter().all(|(k, t)| std::sync::Arc::ptr_eq(t, &before[k])));
    }
}
