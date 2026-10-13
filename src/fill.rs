// SPDX-License-Identifier: GPL-3.0-or-later
// Paint bucket (port of the first app's fill.ts, plan §20) and lasso fill.
// The bucket looks at one layer or the whole picture, closes small gaps in the lineart so the
// color doesn't leak out, and slides the color a few pixels under the lines so no white seam
// is left. Everything respects the selection.

use std::collections::HashSet;

use eframe::egui::Color32;
use rayon::prelude::*;

use crate::blend;
use crate::doc::{Layer, TILE, TileKey};
use crate::select::{self, Dense, Selection};

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FillOpts {
    /// 0..255: how different a color may be and still be filled.
    pub tolerance: u8,
    /// Close gaps in the lines up to about twice this many pixels.
    pub gap: f32,
    /// Pixels the color grows under the lineart.
    pub expand: f32,
    /// Look at all visible layers (else only the layer being filled).
    pub all_layers: bool,
    /// 0..1
    pub opacity: f32,
}

impl Default for FillOpts {
    fn default() -> Self {
        Self { tolerance: 40, gap: 2.0, expand: 2.0, all_layers: true, opacity: 1.0 }
    }
}

/// The area a click at `(x, y)` fills, as coverage over the whole canvas. None when outside.
pub fn bucket(img: &image::RgbaImage, x: i32, y: i32, o: &FillOpts) -> Option<Dense> {
    let (w, h) = (img.width() as usize, img.height() as usize);
    if x < 0 || y < 0 || x as usize >= w || y as usize >= h {
        return None;
    }
    let (sx, sy) = (x as usize, y as usize);
    let c = img.get_pixel(sx as u32, sy as u32).0;
    let open = select::similar(img, c, o.tolerance);
    // 1. close gaps: only pixels farther than `gap` from any line may be reached
    let mut allowed = open.clone();
    let line_dist = if o.gap > 0.0 {
        let lines: Vec<bool> = open.par_iter().map(|&b| !b).collect();
        Some(select::distance(&lines, w, h))
    } else {
        None
    };
    if o.gap > 0.0 {
        let g2 = o.gap * o.gap;
        let d = line_dist.as_ref().unwrap();
        allowed.par_iter_mut().zip(d.par_iter()).for_each(|(a, &dd)| *a = *a && dd > g2);
        if !allowed[sy * w + sx] {
            allowed = open.clone(); // clicked right next to a line: no gap closing
        }
    }
    let region = select::flood(&allowed, w, h, sx, sy);
    if !region.iter().any(|&b| b) {
        return None;
    }
    // 2. gap closing made the area thinner: grow it back through open pixels only (so it
    //    never jumps across a line), then let it slide `expand` px under the lines
    let mut filled = region;
    if o.gap > 0.0 {
        grow_through(&mut filled, &open, w, h, o.gap.ceil() as usize);
    }
    let mut cov: Vec<u8> = filled.iter().map(|&b| if b { 255 } else { 0 }).collect();
    if o.expand > 0.0 {
        let from = select::distance(&filled, w, h);
        let e = o.expand;
        cov.par_iter_mut().enumerate().for_each(|(i, v)| {
            if *v == 0 && !open[i] {
                let d = from[i].sqrt();
                if d <= e {
                    *v = 255;
                } else if d < e + 1.0 {
                    *v = ((e + 1.0 - d) * 255.0).round().max(1.0) as u8;
                }
            }
        });
    }
    Some(Dense { x0: 0, y0: 0, w, h, px: cov })
}

/// Grow `set` by up to `steps` pixels (8 neighbours), only into pixels where `ok`.
fn grow_through(set: &mut [bool], ok: &[bool], w: usize, h: usize, steps: usize) {
    let mut front: Vec<usize> = (0..w * h).filter(|&i| set[i]).collect();
    for _ in 0..steps {
        let mut next = Vec::new();
        for &i in &front {
            let (x, y) = ((i % w) as isize, (i / w) as isize);
            for (dx, dy) in [(-1, 0), (1, 0), (0, -1), (0, 1), (-1, -1), (1, -1), (-1, 1), (1, 1)] {
                let (nx, ny) = (x + dx, y + dy);
                if nx < 0 || ny < 0 || nx >= w as isize || ny >= h as isize {
                    continue;
                }
                let n = ny as usize * w + nx as usize;
                if !set[n] && ok[n] {
                    set[n] = true;
                    next.push(n);
                }
            }
        }
        if next.is_empty() {
            break;
        }
        front = next;
    }
}

/// Paint `color` into a layer where the coverage says (× selection, × opacity).
/// `erase` takes paint away instead. Returns the tiles that changed.
pub fn paint(layer: &mut Layer, d: &Dense, color: Color32, opacity: f32, sel: Option<&Selection>, erase: bool) -> Vec<TileKey> {
    let t = TILE as i32;
    // tiles with any coverage
    let mut keys: HashSet<TileKey> = HashSet::new();
    for (j, row) in d.px.chunks(d.w).enumerate() {
        let y = d.y0 + j as i32;
        let mut x = 0;
        while x < d.w {
            if row[x] != 0 {
                let px = d.x0 + x as i32;
                keys.insert((px.div_euclid(t), y.div_euclid(t)));
                // skip to the next tile
                x = ((px.div_euclid(t) + 1) * t - d.x0) as usize;
            } else {
                x += 1;
            }
        }
    }
    let lock_alpha = layer.lock_alpha;
    let src = blend::to_px(color);
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
                let dy = y - d.y0;
                if dy < 0 || dy >= d.h as i32 {
                    continue;
                }
                for i in 0..t {
                    let x = key.0 * t + i;
                    let dx = x - d.x0;
                    if dx < 0 || dx >= d.w as i32 {
                        continue;
                    }
                    let c = d.px[dy as usize * d.w + dx as usize];
                    if c == 0 {
                        continue;
                    }
                    let k = (j * t + i) as usize;
                    let mut a = c as f32 / 255.0 * opacity;
                    if let Some(m) = mask {
                        a *= m[k] as f32 / 255.0;
                    }
                    if a <= 0.0 {
                        continue;
                    }
                    let o = blend::to_px(px[k]);
                    let out = if erase {
                        let f = 1.0 - a;
                        [o[0] * f, o[1] * f, o[2] * f, o[3] * f]
                    } else if lock_alpha {
                        [o[0] + (src[0] * o[3] - o[0]) * a, o[1] + (src[1] * o[3] - o[1]) * a, o[2] + (src[2] * o[3] - o[2]) * a, o[3]]
                    } else {
                        let f = 1.0 - a;
                        [src[0] * a + o[0] * f, src[1] * a + o[1] * f, src[2] * a + o[2] * f, a + o[3] * f]
                    };
                    px[k] = blend::from_px(out);
                    changed = true;
                }
            }
            changed.then_some((key, px))
        })
        .collect();
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::doc::{Document, LayerKind};

    /// A white picture with a black ring that has a 3 px gap.
    fn ring(gap: bool) -> image::RgbaImage {
        let mut img = image::RgbaImage::from_pixel(100, 100, image::Rgba([255, 255, 255, 255]));
        for y in 0..100u32 {
            for x in 0..100u32 {
                let d = ((x as f32 - 50.0).powi(2) + (y as f32 - 50.0).powi(2)).sqrt();
                if (28.0..31.0).contains(&d) && !(gap && x >= 49 && x <= 51 && y > 50) {
                    img.put_pixel(x, y, image::Rgba([0, 0, 0, 255]));
                }
            }
        }
        img
    }

    #[test]
    fn fills_inside_the_ring_only() {
        let img = ring(false);
        let o = FillOpts { gap: 0.0, expand: 0.0, ..Default::default() };
        let d = bucket(&img, 50, 50, &o).unwrap();
        assert_eq!(d.px[50 * 100 + 50], 255);
        assert_eq!(d.px[2 * 100 + 2], 0);
    }

    #[test]
    fn gap_closing_stops_leaks() {
        let img = ring(true);
        let leak = bucket(&img, 50, 50, &FillOpts { gap: 0.0, expand: 0.0, ..Default::default() }).unwrap();
        assert_eq!(leak.px[2 * 100 + 2], 255, "without gap closing the fill leaks out");
        let closed = bucket(&img, 50, 50, &FillOpts { gap: 3.0, expand: 1.0, ..Default::default() }).unwrap();
        assert_eq!(closed.px[2 * 100 + 2], 0, "with gap closing it stays inside");
        assert_eq!(closed.px[50 * 100 + 50], 255);
        // the color reaches the line and slides under it
        assert_eq!(closed.px[50 * 100 + 50 - 28], 255);
    }

    #[test]
    fn paint_respects_selection() {
        let mut doc = Document::new(300, 300);
        let d = Dense { x0: 0, y0: 0, w: 300, h: 300, px: vec![255; 300 * 300] };
        let sel = Selection::polygon(&[eframe::egui::pos2(0.0, 0.0), eframe::egui::pos2(100.0, 0.0), eframe::egui::pos2(100.0, 300.0), eframe::egui::pos2(0.0, 300.0)], 300, 300, false);
        assert_eq!(doc.layers[0].kind, LayerKind::Raster);
        paint(&mut doc.layers[0], &d, Color32::RED, 1.0, sel.as_ref(), false);
        let img = crate::files::layer_rgba(&doc.layers[0], 0, 0, 300, 300);
        assert_eq!(img.get_pixel(50, 50).0, [255, 0, 0, 255]);
        assert_eq!(img.get_pixel(150, 50).0[3], 0);
        assert_eq!(doc.layers[0].tiles.len(), 2); // only the left column of tiles
    }
}
