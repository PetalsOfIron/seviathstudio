// SPDX-License-Identifier: GPL-3.0-or-later
// Wet paint. A brush with water leaves the paint wet for a while (20 seconds); when the pen lifts,
// wet paint bleeds: its pigment spreads into the paint around it, in pigment space, so wet blue
// next to wet yellow turns green at the seam instead of gray. Painting into paint that is still
// wet bleeds the same way. The edge of a wet wash darkens a little, as real puddles do.
//
// The bleed is worked out once, when the pen lifts, as part of that stroke (so undo takes it away
// with the stroke). What stays wet is remembered per layer, for a few seconds, and is not saved.

use std::collections::HashMap;

use rayon::prelude::*;

use crate::blend::{self, Px};
use crate::brush::BrushSettings;
use crate::doc::{Document, LayerId, TILE, TileKey};
use crate::pigment::{self, N};

/// How long paint stays wet, in seconds.
pub const WET_SECONDS: f32 = 20.0;
/// Biggest area (pixels) that is bled in one go.
const MAX_AREA: usize = 700_000;

/// When each pixel of each layer dries (seconds on the document clock).
#[derive(Default)]
pub struct WetMap {
    planes: HashMap<(LayerId, TileKey), Vec<f32>>,
}

impl WetMap {
    /// 0 = dry, 1 = soaking.
    pub fn level(&self, layer: LayerId, key: TileKey, i: usize, now: f32) -> f32 {
        self.planes.get(&(layer, key)).map(|p| ((p[i] - now) / WET_SECONDS).clamp(0.0, 1.0)).unwrap_or(0.0)
    }

    fn set(&mut self, layer: LayerId, key: TileKey, i: usize, level: f32, now: f32) {
        let p = self.planes.entry((layer, key)).or_insert_with(|| vec![0.0; TILE * TILE]);
        p[i] = if level <= 0.0 { 0.0 } else { now + level * WET_SECONDS };
    }

    /// Forget tiles that have dried.
    pub fn prune(&mut self, now: f32) {
        self.planes.retain(|_, p| p.iter().any(|d| *d > now));
    }

    pub fn is_empty(&self) -> bool {
        self.planes.is_empty()
    }
}

struct Region {
    x0: i32,
    y0: i32,
    w: usize,
    h: usize,
}

fn tile_index(x: i32, y: i32) -> (TileKey, usize) {
    let t = TILE as i32;
    ((x.div_euclid(t), y.div_euclid(t)), (y.rem_euclid(t) * t + x.rem_euclid(t)) as usize)
}

fn gather(doc: &Document, li: usize, r: &Region) -> Vec<Px> {
    let layer = &doc.layers[li];
    let mut out = vec![[0.0; 4]; r.w * r.h];
    for y in 0..r.h {
        for x in 0..r.w {
            let (key, i) = tile_index(r.x0 + x as i32, r.y0 + y as i32);
            if let Some(t) = layer.tiles.get(&key) {
                out[y * r.w + x] = blend::to_px(t[i]);
            }
        }
    }
    out
}

/// Pixels of the region that differ from how the layer looked before the stroke.
fn changed(doc: &Document, li: usize, before: &HashMap<TileKey, Option<crate::doc::TileData>>, r: &Region) -> Vec<bool> {
    let layer = &doc.layers[li];
    let mut out = vec![false; r.w * r.h];
    for (key, b) in before {
        let now = layer.tiles.get(key);
        if let (Some(a), Some(b)) = (now, b) {
            if std::sync::Arc::ptr_eq(a, b) {
                continue;
            }
        }
        for j in 0..TILE {
            for i in 0..TILE {
                let (x, y) = (key.0 * TILE as i32 + i as i32 - r.x0, key.1 * TILE as i32 + j as i32 - r.y0);
                if x < 0 || y < 0 || x >= r.w as i32 || y >= r.h as i32 {
                    continue;
                }
                let k = j * TILE + i;
                let (pa, pb) = (now.map(|t| t[k]), b.as_ref().map(|t| t[k]));
                if pa != pb {
                    out[y as usize * r.w + x as usize] = true;
                }
            }
        }
    }
    out
}

fn blur_plane(src: &[f32], w: usize, h: usize, radius: usize) -> Vec<f32> {
    let mut cur = src.to_vec();
    for _ in 0..2 {
        cur = box_h(&cur, w, h, radius);
        cur = box_v(&cur, w, h, radius);
    }
    cur
}

fn box_h(src: &[f32], w: usize, h: usize, r: usize) -> Vec<f32> {
    let mut out = vec![0.0; src.len()];
    out.par_chunks_mut(w).zip(src.par_chunks(w)).take(h).for_each(|(o, s)| {
        for (x, v) in o.iter_mut().enumerate() {
            // at the border only the pixels that exist are averaged, so the border is not a false edge
            let (a, b) = (x.saturating_sub(r), (x + r).min(w - 1));
            let sum: f32 = s[a..=b].iter().sum();
            *v = sum / (b - a + 1) as f32;
        }
    });
    out
}

fn box_v(src: &[f32], w: usize, h: usize, r: usize) -> Vec<f32> {
    let mut out = vec![0.0; src.len()];
    out.par_chunks_mut(w).enumerate().for_each(|(y, o)| {
        let (a, b) = (y.saturating_sub(r), (y + r).min(h - 1));
        for (x, v) in o.iter_mut().enumerate() {
            let mut sum = 0.0;
            for yy in a..=b {
                sum += src[yy * w + x];
            }
            *v = sum / (b - a + 1) as f32;
        }
    });
    out
}

/// What wet paint does when the pen lifts. `before`: the layer's tiles as they were before the stroke.
/// `bbox`: where the stroke went (document pixels, x0 y0 x1 y1).
pub fn finish_stroke(doc: &mut Document, li: usize, before: &HashMap<TileKey, Option<crate::doc::TileData>>, bbox: [f32; 4], s: &BrushSettings) {
    let (water, bleed, pool) = (s.water.clamp(0.0, 1.0), s.bleed.clamp(0.0, 1.0), s.pool.clamp(0.0, 1.0));
    if (water <= 0.0 && bleed <= 0.0 && pool <= 0.0) || bbox[0] > bbox[2] {
        return;
    }
    let passes = if bleed > 0.0 { 1 + (bleed * 8.0).round() as i32 } else { 0 };
    let pad = (s.size * 0.6 + passes as f32 + 3.0).ceil() as i32;
    let x0 = (bbox[0].floor() as i32 - pad).max(0);
    let y0 = (bbox[1].floor() as i32 - pad).max(0);
    let x1 = (bbox[2].ceil() as i32 + pad).min(doc.w as i32);
    let y1 = (bbox[3].ceil() as i32 + pad).min(doc.h as i32);
    if x1 <= x0 || y1 <= y0 {
        return;
    }
    let r = Region { x0, y0, w: (x1 - x0) as usize, h: (y1 - y0) as usize };
    if r.w * r.h > MAX_AREA {
        return;
    }
    let (layer_id, now) = (doc.layers[li].id, doc.clock as f32);
    let mut px = gather(doc, li, &r);
    let touched = changed(doc, li, before, &r);

    // how wet each pixel is: what was wet already, plus the stroke's own water
    let own = water.max(bleed * 0.5);
    let mut wet: Vec<f32> = (0..r.w * r.h)
        .map(|k| {
            let (key, i) = tile_index(r.x0 + (k % r.w) as i32, r.y0 + (k / r.w) as i32);
            let old = doc.wet.level(layer_id, key, i, now);
            if touched[k] { old.max(own) } else { old }
        })
        .collect();

    // bleed: pigment spreads between wet neighbours
    if passes > 0 {
        let (w, h) = (r.w, r.h);
        // per pixel: K/S times coverage for each band, then coverage
        let mut planes: Vec<[f32; N + 1]> = px
            .par_iter()
            .map(|p| {
                let mut o = [0.0f32; N + 1];
                if p[3] > 0.002 {
                    let rgb = [(p[0] / p[3]).clamp(0.0, 1.0), (p[1] / p[3]).clamp(0.0, 1.0), (p[2] / p[3]).clamp(0.0, 1.0)];
                    let ks = pigment::Pigment::new(rgb).ks();
                    for i in 0..N {
                        o[i] = ks[i] * p[3];
                    }
                    o[N] = p[3];
                }
                o
            })
            .collect();
        for _ in 0..passes {
            // the water creeps outward a little each pass
            let spread: Vec<f32> = (0..w * h)
                .into_par_iter()
                .map(|k| {
                    let (x, y) = (k % w, k / w);
                    let mut m = wet[k];
                    if x > 0 {
                        m = m.max(wet[k - 1] * 0.88);
                    }
                    if x + 1 < w {
                        m = m.max(wet[k + 1] * 0.88);
                    }
                    if y > 0 {
                        m = m.max(wet[k - w] * 0.88);
                    }
                    if y + 1 < h {
                        m = m.max(wet[k + w] * 0.88);
                    }
                    m
                })
                .collect();
            wet = spread;
            let src = planes.clone();
            planes.par_iter_mut().enumerate().for_each(|(k, o)| {
                let wk = wet[k] * bleed * 0.85;
                if wk <= 0.001 {
                    return;
                }
                let (x, y) = ((k % w) as i32, (k / w) as i32);
                let mut acc = [0.0f32; N + 1];
                let mut n = 0.0;
                for dy in -1..=1 {
                    for dx in -1..=1 {
                        let (xx, yy) = ((x + dx).clamp(0, w as i32 - 1) as usize, (y + dy).clamp(0, h as i32 - 1) as usize);
                        for (a, v) in acc.iter_mut().zip(src[yy * w + xx].iter()) {
                            *a += v;
                        }
                        n += 1.0;
                    }
                }
                for i in 0..=N {
                    o[i] += (acc[i] / n - o[i]) * wk;
                }
            });
        }
        px = planes
            .par_iter()
            .map(|o| {
                let a = o[N];
                if a <= 0.002 {
                    return [0.0; 4];
                }
                let mut ks = [0.0f32; N];
                for i in 0..N {
                    ks[i] = o[i] / a;
                }
                let rgb = pigment::rgb_from_ks(&ks);
                [rgb[0] * a, rgb[1] * a, rgb[2] * a, a.min(1.0)]
            })
            .collect();
    }

    // edge darkening: pigment pools where the wash ends
    if pool > 0.0 {
        let alpha: Vec<f32> = px.iter().map(|p| p[3]).collect();
        let radius = ((s.size * 0.08).round() as usize).clamp(1, 12);
        let soft = blur_plane(&alpha, r.w, r.h, radius);
        for (k, ((p, a), b)) in px.iter_mut().zip(&alpha).zip(&soft).enumerate() {
            // only paint that is wet now pools; older dry paint inside the area is left alone
            let rim = ((a - b) * 4.0).clamp(0.0, 1.0) * (wet[k] * 2.0).min(1.0);
            if rim > 0.0 && p[3] > 0.0 {
                let k = 1.0 - 0.45 * pool * rim;
                p[0] *= k;
                p[1] *= k;
                p[2] *= k;
            }
        }
    }

    // write it back, and remember what is wet
    let (lid, rev_bump) = (layer_id, 1);
    for y in 0..r.h {
        for x in 0..r.w {
            let k = y * r.w + x;
            let (key, i) = tile_index(r.x0 + x as i32, r.y0 + y as i32);
            let v = px[k];
            let layer = &mut doc.layers[li];
            if v[3] <= 0.0 && !layer.tiles.contains_key(&key) {
                continue;
            }
            let t = layer.tile_mut(key);
            let nv = blend::from_px(v);
            if t[i] != nv {
                t[i] = nv;
            }
            doc.wet.set(lid, key, i, if v[3] > 0.01 { wet[k] } else { 0.0 }, now);
        }
    }
    doc.layers[li].rev += rev_bump;
    doc.mark_dirty_rect(r.x0, r.y0, r.x0 + r.w as i32, r.y0 + r.h as i32);
}

/// A blotting brush dries what it touches (and lifts a little water-soaked paint with the eraser).
pub fn blot(doc: &mut Document, li: usize, before: &HashMap<TileKey, Option<crate::doc::TileData>>, bbox: [f32; 4], size: f32) {
    if bbox[0] > bbox[2] {
        return;
    }
    let pad = (size * 0.6).ceil() as i32 + 2;
    let x0 = (bbox[0].floor() as i32 - pad).max(0);
    let y0 = (bbox[1].floor() as i32 - pad).max(0);
    let x1 = (bbox[2].ceil() as i32 + pad).min(doc.w as i32);
    let y1 = (bbox[3].ceil() as i32 + pad).min(doc.h as i32);
    if x1 <= x0 || y1 <= y0 || ((x1 - x0) as usize * (y1 - y0) as usize) > MAX_AREA {
        return;
    }
    let r = Region { x0, y0, w: (x1 - x0) as usize, h: (y1 - y0) as usize };
    let touched = changed(doc, li, before, &r);
    let (lid, now) = (doc.layers[li].id, doc.clock as f32);
    for (k, t) in touched.iter().enumerate() {
        if *t {
            let (key, i) = tile_index(r.x0 + (k % r.w) as i32, r.y0 + (k / r.w) as i32);
            doc.wet.set(lid, key, i, 0.0, now);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::brush::engine::{Sample, Stroke};
    use eframe::egui::Color32;

    fn none(_: i32, _: i32) -> Option<Color32> {
        None
    }

    fn solid() -> BrushSettings {
        BrushSettings { size: 40.0, hardness: 1.0, spacing: 0.1, size_pressure: false, stabilize: 0.0, ..BrushSettings::default() }
    }

    fn paint(doc: &mut Document, s: &BrushSettings, color: Color32, x0: f32, x1: f32, y: f32) {
        let mut st = Stroke::begin(doc, 0, s, color, false, 1.0, false, Sample { x: x0, y, p: 1.0 }, &none);
        let n = ((x1 - x0) / 5.0) as usize;
        let pts: Vec<Sample> = (1..=n).map(|i| Sample { x: x0 + i as f32 * 5.0, y, p: 1.0 }).collect();
        for c in pts.chunks(5) {
            st.add(doc, c, &none);
        }
        st.finish(doc, &none);
    }

    fn at(doc: &Document, x: i32, y: i32) -> [f32; 4] {
        let p = crate::files::layer_rgba(&doc.layers[0], x, y, 1, 1).get_pixel(0, 0).0;
        [p[0] as f32 / 255.0, p[1] as f32 / 255.0, p[2] as f32 / 255.0, p[3] as f32 / 255.0]
    }

    const YELLOW: Color32 = Color32::from_rgb(250, 224, 25);
    const BLUE: Color32 = Color32::from_rgb(25, 50, 217);

    #[test]
    fn wet_neighbours_bleed_into_each_other_as_pigment() {
        let wet = BrushSettings { paint: crate::brush::Paint::Mix, water: 0.8, bleed: 0.9, ..solid() };
        let dry = BrushSettings { paint: crate::brush::Paint::Mix, ..solid() };
        let result = |s: &BrushSettings| {
            let mut d = Document::new(500, 300);
            d.clock = 10.0;
            // yellow above, blue directly under it, touching
            paint(&mut d, s, YELLOW, 100.0, 400.0, 130.0);
            paint(&mut d, s, BLUE, 100.0, 400.0, 170.0);
            // the seam between them is at y = 150
            (at(&d, 250, 150), at(&d, 250, 100), at(&d, 250, 200))
        };
        let (seam_wet, _, _) = result(&wet);
        let (seam_dry, _, _) = result(&dry);
        let green = |c: [f32; 4]| c[1] - c[0].max(c[2]);
        assert!(green(seam_wet) > green(seam_dry) + 0.04, "the wet seam is greener: wet {seam_wet:?} dry {seam_dry:?}");
        // and the paint stays pigment, not gray: the seam keeps some color
        let l = crate::oklab::rgb_to_oklab([seam_wet[0], seam_wet[1], seam_wet[2]]);
        assert!(l[1].hypot(l[2]) > 0.05, "{seam_wet:?}");
    }

    #[test]
    fn bleed_spreads_paint_a_little_beyond_the_stroke_and_only_when_wet() {
        let wet = BrushSettings { water: 0.8, bleed: 0.9, ..solid() };
        let mut d = Document::new(500, 300);
        d.clock = 5.0;
        paint(&mut d, &solid(), Color32::from_rgb(40, 60, 200), 100.0, 300.0, 150.0);
        let dry_edge = at(&d, 200, 150 + 22)[3];
        let mut d2 = Document::new(500, 300);
        d2.clock = 5.0;
        paint(&mut d2, &wet, Color32::from_rgb(40, 60, 200), 100.0, 300.0, 150.0);
        let wet_edge = at(&d2, 200, 150 + 22)[3];
        assert!(dry_edge < 0.05, "a dry stroke ends where it ends: {dry_edge}");
        assert!(wet_edge > 0.03 && wet_edge > dry_edge, "a wet stroke feathers outward: {wet_edge} vs {dry_edge}");
        assert!(at(&d2, 200, 150)[3] > 0.4, "and keeps its middle (thinned by the water)");
    }

    #[test]
    fn paint_dries_and_a_blot_dries_it_sooner() {
        let wet = BrushSettings { water: 0.9, ..solid() };
        let mut d = Document::new(500, 300);
        d.clock = 0.0;
        paint(&mut d, &wet, BLUE, 100.0, 300.0, 150.0);
        let (id, key, i) = (d.layers[0].id, (0, 0), 0usize);
        let (key2, i2) = tile_index(200, 150);
        assert!(d.wet.level(id, key2, i2, 1.0) > 0.7, "soaking just after");
        assert!(d.wet.level(id, key2, i2, 19.0) < d.wet.level(id, key2, i2, 5.0), "it dries over time");
        assert_eq!(d.wet.level(id, key2, i2, WET_SECONDS + 1.0), 0.0, "and is dry after twenty seconds");
        assert_eq!(d.wet.level(id, key, i, 1.0), 0.0, "far away nothing is wet");
        d.wet.prune(WET_SECONDS + 1.0);
        assert!(d.wet.is_empty());
        // a blot sets what it touches dry
        let mut d = Document::new(500, 300);
        paint(&mut d, &wet, BLUE, 100.0, 300.0, 150.0);
        let before: HashMap<TileKey, Option<crate::doc::TileData>> = d.layers[0].tiles.iter().map(|(k, t)| (*k, Some(t.clone()))).collect();
        for t in d.layers[0].tiles.values_mut() {
            let mut v = (**t).clone();
            v[0] = Color32::RED; // pretend the blot changed something in every tile
            *t = std::sync::Arc::new(v);
        }
        blot(&mut d, 0, &before, [0.0, 0.0, 255.0, 255.0], 20.0);
        assert_eq!(d.wet.level(d.layers[0].id, (0, 0), 0, 0.0), 0.0);
    }

    #[test]
    fn edges_pool_darker_than_the_middle_of_a_wash() {
        let s = BrushSettings { water: 0.6, pool: 1.0, ..solid() };
        let mut d = Document::new(500, 300);
        paint(&mut d, &s, Color32::from_rgb(60, 120, 200), 100.0, 400.0, 150.0);
        let (mid, rim) = (at(&d, 250, 150), at(&d, 250, 150 - 17));
        let l = |c: [f32; 4]| (c[0] + c[1] + c[2]) / 3.0;
        assert!(l(rim) < l(mid) - 0.02, "darker at the edge: rim {rim:?} middle {mid:?}");
    }

    #[test]
    fn brushes_without_water_are_untouched_by_all_this() {
        let s = solid();
        assert_eq!((s.water, s.bleed, s.pool, s.granulation), (0.0, 0.0, 0.0, 0.0));
        let mut d = Document::new(500, 300);
        paint(&mut d, &s, BLUE, 100.0, 300.0, 150.0);
        assert!(d.wet.is_empty(), "nothing is marked wet");
    }

    #[test]
    fn no_straight_seam_where_the_bled_area_ends() {
        let s = BrushSettings { water: 0.6, pool: 1.0, bleed: 0.6, ..solid() };
        let mut d = Document::new(500, 300);
        paint(&mut d, &s, Color32::from_rgb(60, 120, 200), 200.0, 400.0, 150.0);
        // the region of the wet pass ends somewhere left of the stroke: nothing may change along that line
        let img = crate::files::flatten(&d);
        for x in 100..160u32 {
            assert_eq!(img.get_pixel(x, 150).0, [255, 255, 255, 255], "x={x} is far from the stroke and stays paper");
        }
        // along the stroke the rim darkening is smooth: neighbouring columns differ little
        let row: Vec<i32> = (210..390u32).map(|x| img.get_pixel(x, 150 - 18).0[0] as i32).collect();
        assert!(row.windows(2).all(|w| (w[0] - w[1]).abs() < 12), "{row:?}");
    }

    /// A sheet of wet strokes to look at: target/screens/wet_sheet.png
    #[test]
    fn wet_sheet() {
        let preset = |n: &str| crate::brush::presets().into_iter().chain(crate::brush::eraser_presets()).find(|p| p.name == n).unwrap().s;
        let mut d = Document::new(900, 560);
        d.layers[1].visible = true;
        let wavy = |d: &mut Document, s: &BrushSettings, c: Color32, erase: bool, x0: f32, x1: f32, y: f32, amp: f32| {
            let mut st = Stroke::begin(d, 0, s, c, erase, 1.0, false, Sample { x: x0, y, p: 1.0 }, &none);
            let n = ((x1 - x0) / 5.0) as usize;
            let pts: Vec<Sample> = (1..=n).map(|i| Sample { x: x0 + i as f32 * 5.0, y: y + (i as f32 * 0.12).sin() * amp, p: 1.0 }).collect();
            for c in pts.chunks(5) {
                st.add(d, c, &none);
            }
            st.finish(d, &none);
        };
        d.clock = 1.0;
        let wc = preset("Wet watercolor");
        wavy(&mut d, &wc, Color32::from_rgb(40, 90, 200), false, 60.0, 440.0, 80.0, 12.0);
        wavy(&mut d, &wc, YELLOW, false, 60.0, 440.0, 125.0, 12.0); // touches the blue while it is wet
        wavy(&mut d, &preset("Granulating wash"), Color32::from_rgb(190, 50, 60), false, 480.0, 860.0, 100.0, 15.0);
        wavy(&mut d, &preset("Wet ink wash"), Color32::from_rgb(25, 25, 35), false, 60.0, 440.0, 260.0, 20.0);
        wavy(&mut d, &preset("Wet ink wash"), Color32::from_rgb(25, 25, 35), false, 480.0, 860.0, 260.0, 20.0);
        wavy(&mut d, &preset("Lift"), Color32::BLACK, true, 480.0, 860.0, 255.0, 6.0);
        wavy(&mut d, &preset("Scrape"), Color32::BLACK, true, 60.0, 440.0, 262.0, 4.0);
        // the same two colors with a plain dry brush for comparison
        let dry = BrushSettings { size: 60.0, hardness: 0.35, opacity: 0.75, flow: 0.6, ..BrushSettings::default() };
        wavy(&mut d, &dry, Color32::from_rgb(40, 90, 200), false, 60.0, 440.0, 400.0, 12.0);
        wavy(&mut d, &dry, YELLOW, false, 60.0, 440.0, 445.0, 12.0);
        // the wet pair again, without paper texture, to tell bleeding from paper
        let plain = BrushSettings { texture: 0.0, granulation: 0.0, ..wc.clone() };
        wavy(&mut d, &plain, Color32::from_rgb(40, 90, 200), false, 480.0, 860.0, 400.0, 12.0);
        wavy(&mut d, &plain, YELLOW, false, 480.0, 860.0, 445.0, 12.0);
        let img = crate::files::flatten(&d);
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("target").join("screens");
        std::fs::create_dir_all(&dir).unwrap();
        img.save(dir.join("wet_sheet.png")).unwrap();
    }
}
