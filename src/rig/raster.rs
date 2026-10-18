// SPDX-License-Identifier: GPL-3.0-or-later
// A software renderer for rig previews: draws each layer through its (moved) triangle mesh, looking the pixels up at the
// rest position. Normal blending only. Rows are split between the CPU's threads.

use eframe::egui::Color32;
use rayon::prelude::*;

use crate::doc::{Layer, TILE};
use crate::files::layer_bounds;

/// A layer's pixels in one piece (premultiplied, 0..1), over its bounding box.
pub struct LayerTex {
    pub x0: i32,
    pub y0: i32,
    pub w: usize,
    pub h: usize,
    px: Vec<[f32; 4]>,
}

impl LayerTex {
    pub fn of(l: &Layer, doc_w: u32, doc_h: u32) -> Option<LayerTex> {
        let (x0, y0, x1, y1) = layer_bounds(l, doc_w, doc_h)?;
        let (w, h) = ((x1 - x0) as usize, (y1 - y0) as usize);
        let mut px = vec![[0.0f32; 4]; w * h];
        let t = TILE as i32;
        for (key, tile) in &l.tiles {
            for j in 0..t {
                let y = key.1 * t + j - y0;
                if y < 0 || y >= h as i32 {
                    continue;
                }
                for i in 0..t {
                    let x = key.0 * t + i - x0;
                    if x < 0 || x >= w as i32 {
                        continue;
                    }
                    let c = tile[(j * t + i) as usize];
                    if c.a() != 0 {
                        px[y as usize * w + x as usize] = [c.r() as f32 / 255.0, c.g() as f32 / 255.0, c.b() as f32 / 255.0, c.a() as f32 / 255.0];
                    }
                }
            }
        }
        Some(LayerTex { x0, y0, w, h, px })
    }

    /// A texture from a picture that covers the page (not premultiplied).
    pub fn from_rgba(img: &image::RgbaImage) -> LayerTex {
        let (w, h) = (img.width() as usize, img.height() as usize);
        let px = img
            .pixels()
            .map(|p| {
                let a = p[3] as f32 / 255.0;
                [p[0] as f32 / 255.0 * a, p[1] as f32 / 255.0 * a, p[2] as f32 / 255.0 * a, a]
            })
            .collect();
        LayerTex { x0: 0, y0: 0, w, h, px }
    }

    /// How solid a texel is (0..1).
    pub fn alpha_at(&self, x: usize, y: usize) -> f32 {
        self.px.get(y * self.w + x).map(|p| p[3]).unwrap_or(0.0)
    }

    /// Bilinear lookup at a page position; see-through outside.
    fn sample(&self, x: f32, y: f32) -> [f32; 4] {
        let (fx, fy) = (x - self.x0 as f32 - 0.5, y - self.y0 as f32 - 0.5);
        let (ix, iy) = (fx.floor(), fy.floor());
        let (tx, ty) = (fx - ix, fy - iy);
        let (ix, iy) = (ix as i32, iy as i32);
        let at = |x: i32, y: i32| -> [f32; 4] {
            if x < 0 || y < 0 || x >= self.w as i32 || y >= self.h as i32 {
                [0.0; 4]
            } else {
                self.px[y as usize * self.w + x as usize]
            }
        };
        let (a, b, c, d) = (at(ix, iy), at(ix + 1, iy), at(ix, iy + 1), at(ix + 1, iy + 1));
        let mut o = [0.0f32; 4];
        for k in 0..4 {
            o[k] = (a[k] * (1.0 - tx) + b[k] * tx) * (1.0 - ty) + (c[k] * (1.0 - tx) + d[k] * tx) * ty;
        }
        o
    }
}

/// One layer to draw: its pixels, its mesh at rest and where the mesh is now.
pub struct Item<'a> {
    pub tex: &'a LayerTex,
    pub rest: &'a [[f32; 2]],
    pub now: Vec<[f32; 2]>,
    pub tris: &'a [[u32; 3]],
    /// 0..1
    pub opacity: f32,
}

/// Draw the items (bottom first) into a `w` x `h` picture, `scale` pixels per page pixel.
/// `backdrop` is what shows through (None = see-through).
pub fn render(items: &[Item], w: usize, h: usize, scale: f32, backdrop: Option<Color32>) -> Vec<Color32> {
    // the screen rows of every triangle, once
    struct Prep {
        item: usize,
        tri: usize,
        y0: f32,
        y1: f32,
    }
    let mut preps: Vec<Prep> = Vec::new();
    for (ii, it) in items.iter().enumerate() {
        for (ti, t) in it.tris.iter().enumerate() {
            let ys = [it.now[t[0] as usize][1], it.now[t[1] as usize][1], it.now[t[2] as usize][1]];
            preps.push(Prep { item: ii, tri: ti, y0: ys.iter().cloned().fold(f32::MAX, f32::min) * scale, y1: ys.iter().cloned().fold(f32::MIN, f32::max) * scale });
        }
    }
    let mut out = vec![Color32::TRANSPARENT; w * h];
    let band = 32usize;
    out.par_chunks_mut(w * band).enumerate().for_each(|(bi, rows)| {
        let (by0, nrows) = (bi * band, rows.len() / w);
        let mut acc: Vec<[f32; 4]> = vec![[0.0; 4]; w * nrows];
        for p in &preps {
            if p.y1 < by0 as f32 - 1.0 || p.y0 > (by0 + nrows) as f32 + 1.0 {
                continue;
            }
            let it = &items[p.item];
            let t = it.tris[p.tri];
            let s = |i: u32| {
                let v = it.now[i as usize];
                [v[0] * scale, v[1] * scale]
            };
            let (a, mut b, mut c) = (s(t[0]), s(t[1]), s(t[2]));
            let (ra, mut rb, mut rc) = (it.rest[t[0] as usize], it.rest[t[1] as usize], it.rest[t[2] as usize]);
            let mut area = (b[0] - a[0]) * (c[1] - a[1]) - (c[0] - a[0]) * (b[1] - a[1]);
            if area.abs() < 1e-6 {
                continue;
            }
            if area < 0.0 {
                std::mem::swap(&mut b, &mut c);
                std::mem::swap(&mut rb, &mut rc);
                area = -area;
            }
            let (minx, maxx) = (a[0].min(b[0]).min(c[0]), a[0].max(b[0]).max(c[0]));
            let x_lo = (minx.floor() as i32).max(0);
            let x_hi = (maxx.ceil() as i32).min(w as i32 - 1);
            let y_lo = (p.y0.floor() as i32).max(by0 as i32);
            let y_hi = (p.y1.ceil() as i32).min((by0 + nrows) as i32 - 1);
            // a pixel on a shared edge belongs to one triangle only (the top-left rule)
            let owns = |wv: f32, dx: f32, dy: f32| wv > 0.0 || (wv == 0.0 && (dy > 0.0 || (dy == 0.0 && dx < 0.0)));
            for y in y_lo..=y_hi {
                for x in x_lo..=x_hi {
                    let pt = [x as f32 + 0.5, y as f32 + 0.5];
                    let w0 = (c[0] - b[0]) * (pt[1] - b[1]) - (c[1] - b[1]) * (pt[0] - b[0]);
                    let w1 = (a[0] - c[0]) * (pt[1] - c[1]) - (a[1] - c[1]) * (pt[0] - c[0]);
                    let w2 = (b[0] - a[0]) * (pt[1] - a[1]) - (b[1] - a[1]) * (pt[0] - a[0]);
                    if !(owns(w0, c[0] - b[0], c[1] - b[1]) && owns(w1, a[0] - c[0], a[1] - c[1]) && owns(w2, b[0] - a[0], b[1] - a[1])) {
                        continue;
                    }
                    let (l0, l1, l2) = (w0 / area, w1 / area, w2 / area);
                    let (u, v) = (ra[0] * l0 + rb[0] * l1 + rc[0] * l2, ra[1] * l0 + rb[1] * l1 + rc[1] * l2);
                    let src = it.tex.sample(u, v);
                    if src[3] <= 0.0 {
                        continue;
                    }
                    let k = it.opacity;
                    let d = &mut acc[(y as usize - by0) * w + x as usize];
                    let inv = 1.0 - src[3] * k;
                    for i in 0..4 {
                        d[i] = src[i] * k + d[i] * inv;
                    }
                }
            }
        }
        let back = backdrop.map(|c| [c.r() as f32 / 255.0, c.g() as f32 / 255.0, c.b() as f32 / 255.0, c.a() as f32 / 255.0]);
        for (i, px) in rows.iter_mut().enumerate() {
            let mut c = acc[i];
            if let Some(b) = back {
                let inv = 1.0 - c[3];
                for k in 0..4 {
                    c[k] += b[k] * inv;
                }
            }
            let q = |v: f32| (v.clamp(0.0, 1.0) * 255.0 + 0.5) as u8;
            *px = Color32::from_rgba_premultiplied(q(c[0]), q(c[1]), q(c[2]), q(c[3]));
        }
    });
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::doc::{Document, LayerKind};
    use crate::rig::{Kind, Rig, Values};

    /// A layer with a solid rectangle (x0..x1, y0..y1) in the given color.
    fn rect_layer(d: &mut Document, x0: i32, y0: i32, x1: i32, y1: i32, c: Color32) -> Layer {
        let mut l = d.new_layer("R", LayerKind::Raster);
        let t = TILE as i32;
        for y in y0..y1 {
            for x in x0..x1 {
                let tile = l.tiles.entry((x.div_euclid(t), y.div_euclid(t))).or_insert_with(|| std::sync::Arc::new(vec![Color32::TRANSPARENT; TILE * TILE]));
                std::sync::Arc::make_mut(tile)[(y.rem_euclid(t) * t + x.rem_euclid(t)) as usize] = c;
            }
        }
        l
    }

    fn quad(x0: f32, y0: f32, x1: f32, y1: f32) -> (Vec<[f32; 2]>, Vec<[u32; 3]>) {
        (vec![[x0, y0], [x1, y0], [x1, y1], [x0, y1]], vec![[0, 1, 2], [0, 2, 3]])
    }

    #[test]
    fn an_unmoved_mesh_draws_the_layer_as_it_is_without_seams() {
        let mut d = Document::new(100, 100);
        let red = Color32::from_rgb(200, 30, 30);
        let l = rect_layer(&mut d, 20, 20, 60, 50, red);
        let tex = LayerTex::of(&l, 100, 100).unwrap();
        let (v, t) = quad(20.0, 20.0, 60.0, 50.0);
        let out = render(&[Item { tex: &tex, rest: &v, now: v.clone(), tris: &t, opacity: 1.0 }], 100, 100, 1.0, None);
        for y in 0..100usize {
            for x in 0..100usize {
                let inside = (20..60).contains(&x) && (20..50).contains(&y);
                let c = out[y * 100 + x];
                if inside {
                    assert_eq!(c, red, "({x},{y}) = {c:?}: no gaps along the diagonal and no double drawing");
                } else {
                    assert_eq!(c.a(), 0, "({x},{y})");
                }
            }
        }
    }

    #[test]
    fn a_rotation_moves_the_picture() {
        let mut d = Document::new(200, 200);
        let blue = Color32::from_rgb(20, 60, 220);
        // a bar to the right of the pivot (100,100): after a 90 degree turn it is below the pivot
        let l = rect_layer(&mut d, 120, 96, 180, 104, blue);
        let tex = LayerTex::of(&l, 200, 200).unwrap();
        let (v, t) = quad(120.0, 96.0, 180.0, 104.0);
        let mut rig = Rig::default();
        let c = rig.add_changer("Turn", Kind::Rotation { pivot: [100.0, 100.0] }, None);
        let p = rig.add_param("Angle", 0.0, 90.0, 0.0);
        rig.bind_rotation(c, p, 90.0);
        let mut vals = Values::default();
        vals.set(p, 90.0);
        let pose = rig.pose(&vals);
        let now: Vec<[f32; 2]> = v.iter().map(|q| rig.deform_point(&pose, Some(c), *q)).collect();
        let out = render(&[Item { tex: &tex, rest: &v, now, tris: &t, opacity: 1.0 }], 200, 200, 1.0, None);
        assert_eq!(out[150 * 200 + 100], blue, "the bar now hangs below the pivot");
        assert_eq!(out[100 * 200 + 150].a(), 0, "and it is gone from where it was");
    }

    #[test]
    fn layers_are_drawn_bottom_first_and_opacity_and_backdrop_work() {
        let mut d = Document::new(60, 60);
        let a = rect_layer(&mut d, 10, 10, 40, 40, Color32::from_rgb(255, 0, 0));
        let b = rect_layer(&mut d, 20, 20, 50, 50, Color32::from_rgb(0, 0, 255));
        let (ta, tb) = (LayerTex::of(&a, 60, 60).unwrap(), LayerTex::of(&b, 60, 60).unwrap());
        let (va, ia) = quad(10.0, 10.0, 40.0, 40.0);
        let (vb, ib) = quad(20.0, 20.0, 50.0, 50.0);
        let items = [
            Item { tex: &ta, rest: &va, now: va.clone(), tris: &ia, opacity: 1.0 },
            Item { tex: &tb, rest: &vb, now: vb.clone(), tris: &ib, opacity: 0.5 },
        ];
        let out = render(&items, 60, 60, 1.0, Some(Color32::WHITE));
        assert_eq!(out[15 * 60 + 15], Color32::from_rgb(255, 0, 0));
        let mix = out[30 * 60 + 30];
        assert!(mix.b() > 100 && mix.r() > 100, "half blue over red: {mix:?}");
        let blue_over_white = out[45 * 60 + 45];
        assert!(blue_over_white.r() > 100 && blue_over_white.b() > 200, "{blue_over_white:?}");
        assert_eq!(out[5 * 60 + 5], Color32::WHITE);
    }

    #[test]
    fn a_scaled_preview_has_the_right_size_and_content() {
        let mut d = Document::new(200, 200);
        let l = rect_layer(&mut d, 0, 0, 200, 200, Color32::from_rgb(10, 200, 10));
        let tex = LayerTex::of(&l, 200, 200).unwrap();
        let (v, t) = quad(0.0, 0.0, 200.0, 200.0);
        let out = render(&[Item { tex: &tex, rest: &v, now: v.clone(), tris: &t, opacity: 1.0 }], 50, 50, 0.25, None);
        assert_eq!(out.len(), 2500);
        assert!(out.iter().all(|c| *c == Color32::from_rgb(10, 200, 10)));
    }
}
