// SPDX-License-Identifier: GPL-3.0-or-later
// What the paint under one dab looks like, for the blender brushes: a low-pass copy (so a blend
// can change the soft color and leave the fine texture alone: skin pores survive) and a copy
// smeared along the local flow (so a comb blender pulls along the strands instead of across them).

use super::raster::blur;
use crate::blend::{self, Px};
use crate::doc::{Layer, TILE};

pub struct Local {
    x0: i32,
    y0: i32,
    w: usize,
    h: usize,
    /// The layer here, premultiplied.
    src: Vec<Px>,
    low: Option<Vec<Px>>,
    comb: Option<Vec<Px>>,
    /// Unit direction of the strands under the brush, when there are any.
    pub flow: Option<(f32, f32)>,
}

fn lum(p: &Px) -> f32 {
    0.3 * p[0] + 0.59 * p[1] + 0.11 * p[2] + 0.5 * p[3]
}

impl Local {
    /// `reach`: how far from (cx, cy) a dab covers. `low`: blur radius for the low-pass copy (0 = none).
    /// `comb`: smear length along the flow (0 = none).
    pub fn build(layer: &Layer, doc_w: u32, doc_h: u32, cx: f32, cy: f32, reach: f32, low: f32, comb: f32) -> Local {
        let line = comb.clamp(0.0, 16.0).ceil() as i32;
        let pad = (low * 2.2).ceil() as i32 + line + 3;
        let x0 = ((cx - reach).floor() as i32 - pad).max(0);
        let y0 = ((cy - reach).floor() as i32 - pad).max(0);
        let x1 = ((cx + reach).ceil() as i32 + pad).min(doc_w as i32);
        let y1 = ((cy + reach).ceil() as i32 + pad).min(doc_h as i32);
        let (w, h) = ((x1 - x0).max(0) as usize, (y1 - y0).max(0) as usize);
        let mut src = vec![[0.0f32; 4]; w * h];
        let t = TILE as i32;
        for y in 0..h {
            for x in 0..w {
                let (px, py) = (x0 + x as i32, y0 + y as i32);
                if let Some(tile) = layer.tiles.get(&(px.div_euclid(t), py.div_euclid(t))) {
                    src[y * w + x] = blend::to_px(tile[(py.rem_euclid(t) * t + px.rem_euclid(t)) as usize]);
                }
            }
        }
        let mut out = Local { x0, y0, w, h, src, low: None, comb: None, flow: None };
        if w < 3 || h < 3 {
            return out;
        }
        if low > 0.0 {
            let mut planes: Vec<Vec<f32>> = (0..4).map(|c| out.src.iter().map(|p| p[c]).collect()).collect();
            for p in planes.iter_mut() {
                blur(p, w, h, low * 2.0);
            }
            out.low = Some((0..w * h).map(|i| [planes[0][i], planes[1][i], planes[2][i], planes[3][i]]).collect());
        }
        if comb > 0.0 {
            out.flow = out.flow_under(cx, cy, reach);
            if let Some((fx, fy)) = out.flow {
                let mut sm = vec![[0.0f32; 4]; w * h];
                for y in 0..h {
                    for x in 0..w {
                        let mut acc = [0.0f32; 4];
                        let mut n = 0.0;
                        for k in -line..=line {
                            let (sx, sy) = ((x as f32 + fx * k as f32).round() as i32, (y as f32 + fy * k as f32).round() as i32);
                            if sx < 0 || sy < 0 || sx >= w as i32 || sy >= h as i32 {
                                continue;
                            }
                            let p = out.src[sy as usize * w + sx as usize];
                            for c in 0..4 {
                                acc[c] += p[c];
                            }
                            n += 1.0;
                        }
                        if n > 0.0 {
                            sm[y * w + x] = acc.map(|v| v / n);
                        }
                    }
                }
                out.comb = Some(sm);
            }
        }
        out
    }

    /// The direction the strands under the brush run in (structure tensor), if they run in one.
    fn flow_under(&self, cx: f32, cy: f32, reach: f32) -> Option<(f32, f32)> {
        let (mut jxx, mut jyy, mut jxy) = (0.0f32, 0.0f32, 0.0f32);
        let r = reach.max(3.0);
        let (xa, xb) = (((cx - r) as i32 - self.x0).max(1), ((cx + r) as i32 - self.x0).min(self.w as i32 - 2));
        let (ya, yb) = (((cy - r) as i32 - self.y0).max(1), ((cy + r) as i32 - self.y0).min(self.h as i32 - 2));
        for y in ya..=yb {
            for x in xa..=xb {
                let at = |xx: i32, yy: i32| lum(&self.src[yy as usize * self.w + xx as usize]);
                let gx = at(x + 1, y) - at(x - 1, y);
                let gy = at(x, y + 1) - at(x, y - 1);
                jxx += gx * gx;
                jyy += gy * gy;
                jxy += gx * gy;
            }
        }
        let tr = jxx + jyy;
        if tr < 1e-4 {
            return None;
        }
        let coherence = ((jxx - jyy).powi(2) + 4.0 * jxy * jxy).sqrt() / tr;
        if coherence < 0.2 {
            return None;
        }
        // the gradient points across the strands: the strands run at a right angle to it
        let theta = 0.5 * (2.0 * jxy).atan2(jxx - jyy);
        Some((-theta.sin(), theta.cos()))
    }

    fn at(&self, plane: &[Px], x: i32, y: i32) -> Option<Px> {
        let (lx, ly) = (x - self.x0, y - self.y0);
        (lx >= 0 && ly >= 0 && (lx as usize) < self.w && (ly as usize) < self.h).then(|| plane[ly as usize * self.w + lx as usize])
    }

    /// The layer's soft color at a pixel (fine texture removed).
    pub fn low_at(&self, x: i32, y: i32) -> Option<Px> {
        self.low.as_ref().and_then(|l| self.at(l, x, y))
    }

    /// The layer here, smeared along the strands.
    pub fn comb_at(&self, x: i32, y: i32) -> Option<Px> {
        self.comb.as_ref().and_then(|l| self.at(l, x, y))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use eframe::egui::Color32;

    fn striped(horizontal: bool) -> Layer {
        let mut d = crate::doc::Document::new(200, 200);
        for y in 0..200usize {
            for x in 0..200usize {
                let band = if horizontal { y } else { x } / 3 % 2 == 0;
                let c = if band { Color32::from_rgb(220, 40, 40) } else { Color32::from_rgb(30, 40, 200) };
                d.layers[0].tile_mut(((x / TILE) as i32, (y / TILE) as i32))[(y % TILE) * TILE + x % TILE] = c;
            }
        }
        d.layers.remove(0)
    }

    #[test]
    fn the_flow_follows_the_strands_and_flat_paint_has_none() {
        let h = Local::build(&striped(true), 200, 200, 100.0, 100.0, 25.0, 0.0, 6.0);
        let (fx, fy) = h.flow.expect("horizontal stripes have a flow");
        assert!(fx.abs() > 0.95 && fy.abs() < 0.3, "runs left to right: {fx} {fy}");
        let v = Local::build(&striped(false), 200, 200, 100.0, 100.0, 25.0, 0.0, 6.0);
        let (fx, fy) = v.flow.unwrap();
        assert!(fy.abs() > 0.95 && fx.abs() < 0.3, "runs up and down: {fx} {fy}");
        let mut flat = crate::doc::Document::new(200, 200);
        for p in flat.layers[0].tile_mut((0, 0)).iter_mut() {
            *p = Color32::from_rgb(120, 120, 120);
        }
        assert!(Local::build(&flat.layers[0], 200, 200, 100.0, 100.0, 25.0, 0.0, 6.0).flow.is_none(), "no strands, no flow");
    }

    #[test]
    fn combing_keeps_the_strands_and_the_low_pass_removes_them() {
        let layer = striped(true);
        let l = Local::build(&layer, 200, 200, 100.0, 100.0, 25.0, 5.0, 6.0);
        // along the strands the color is kept (red stays red); across them a plain blur would mix to purple
        let c = l.comb_at(100, 100).unwrap();
        let orig = blend::to_px(layer.tiles[&(0, 0)][100 * TILE + 100]);
        assert!((c[0] - orig[0]).abs() < 0.12 && (c[2] - orig[2]).abs() < 0.12, "combed {c:?} vs {orig:?}");
        let low = l.low_at(100, 100).unwrap();
        assert!(low[0] > 0.25 && low[2] > 0.25, "the low-pass copy mixes both stripes: {low:?}");
        assert!(l.low_at(5, 5).is_none() || l.low_at(5, 5).is_some(), "lookups outside never panic");
    }
}
