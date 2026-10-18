// SPDX-License-Identifier: GPL-3.0-or-later
// Makes a triangle mesh for a layer from its see-through shape: the outline is traced, simplified, and the inside
// is filled with points and cut into triangles (a Delaunay triangulation that keeps the outline as it is).

use spade::{ConstrainedDelaunayTriangulation, Point2, Triangulation};

use crate::doc::Layer;
use crate::files::layer_bounds;
use crate::trace::{self, TraceMode, TraceOptions};

/// The longest side the outline is traced at (bigger layers are traced smaller and the outline scaled back).
const TRACE_SIDE: f32 = 900.0;

fn inside(rings: &[Vec<[f32; 2]>], x: f32, y: f32) -> bool {
    let mut inside = false;
    for r in rings {
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

/// Ramer-Douglas-Peucker on a closed ring.
fn simplify_ring(r: &[[f32; 2]], eps: f32) -> Vec<[f32; 2]> {
    fn rdp(pts: &[[f32; 2]], eps: f32, out: &mut Vec<[f32; 2]>) {
        if pts.len() < 3 {
            out.push(pts[0]);
            return;
        }
        let (a, b) = (pts[0], *pts.last().unwrap());
        let (dx, dy) = (b[0] - a[0], b[1] - a[1]);
        let len = dx.hypot(dy).max(1e-6);
        let (mut worst, mut wi) = (0.0, 0);
        for (i, p) in pts.iter().enumerate().take(pts.len() - 1).skip(1) {
            let d = ((p[0] - a[0]) * dy - (p[1] - a[1]) * dx).abs() / len;
            if d > worst {
                worst = d;
                wi = i;
            }
        }
        if worst > eps {
            rdp(&pts[..=wi], eps, out);
            rdp(&pts[wi..], eps, out);
        } else {
            out.push(a);
        }
    }
    if r.len() < 6 {
        return r.to_vec();
    }
    let mid = r.len() / 2;
    let mut out = Vec::new();
    rdp(&r[..=mid], eps, &mut out);
    let mut second = r[mid..].to_vec();
    second.push(r[0]);
    rdp(&second, eps, &mut out);
    out
}

/// The outlines of a layer's paint in page pixels (an outline inside another is a hole).
pub fn outlines(l: &Layer, doc_w: u32, doc_h: u32, simplify_px: f32) -> Vec<Vec<[f32; 2]>> {
    let Some((x0, y0, x1, y1)) = layer_bounds(l, doc_w, doc_h) else { return Vec::new() };
    let (bw, bh) = ((x1 - x0) as usize, (y1 - y0) as usize);
    let k = (TRACE_SIDE / bw.max(bh) as f32).min(1.0);
    let (mw, mh) = (((bw as f32 * k).ceil() as usize + 6).max(8), ((bh as f32 * k).ceil() as usize + 6).max(8));
    // a black picture with the layer's see-through amount, with a margin of 3 pixels around
    let mut rgba = vec![0u8; mw * mh * 4];
    let t = crate::doc::TILE as i32;
    for (key, tile) in &l.tiles {
        for j in 0..t {
            for i in 0..t {
                let a = tile[(j * t + i) as usize].a();
                if a == 0 {
                    continue;
                }
                let (px, py) = (key.0 * t + i - x0, key.1 * t + j - y0);
                if px < 0 || py < 0 || px >= bw as i32 || py >= bh as i32 {
                    continue;
                }
                let (mx, my) = (((px as f32 * k) as usize) + 3, ((py as f32 * k) as usize) + 3);
                let o = (my.min(mh - 1) * mw + mx.min(mw - 1)) * 4 + 3;
                rgba[o] = rgba[o].max(a);
            }
        }
    }
    let o = TraceOptions { mode: TraceMode::Silhouette, threshold: 0.06, color_smooth: 0.2, min_area: ((6.0 * k * k) as usize).max(2), single_color: [0, 0, 0], ..TraceOptions::default() };
    let res = trace::trace(&rgba, mw, mh, &o, &|_| {});
    let eps = (simplify_px * k).max(0.6);
    let mut rings = Vec::new();
    for f in res.fills {
        for r in f.rings {
            let r = simplify_ring(&r, eps);
            if r.len() >= 3 {
                rings.push(r.iter().map(|p| [x0 as f32 + (p[0] - 3.0) / k, y0 as f32 + (p[1] - 3.0) / k]).collect());
            }
        }
    }
    rings
}

/// Mesh a layer. `density` 0..1: 0 = few big triangles, 1 = many small ones.
pub fn generate(l: &Layer, doc_w: u32, doc_h: u32, density: f32) -> Option<(Vec<[f32; 2]>, Vec<[u32; 3]>)> {
    let d = density.clamp(0.0, 1.0);
    let rings = outlines(l, doc_w, doc_h, 6.0 - 4.5 * d);
    if rings.is_empty() {
        return None;
    }
    let spacing = 170.0 - 130.0 * d;
    let mut t = ConstrainedDelaunayTriangulation::<Point2<f64>>::new();
    let mut boundary: Vec<[f32; 2]> = Vec::new();
    for r in &rings {
        let mut handles = Vec::new();
        for p in r {
            if let Ok(h) = t.insert(Point2::new(p[0] as f64, p[1] as f64)) {
                handles.push(h);
                boundary.push(*p);
            }
        }
        for i in 0..handles.len() {
            let (a, b) = (handles[i], handles[(i + 1) % handles.len()]);
            if a != b && t.can_add_constraint(a, b) {
                t.add_constraint(a, b);
            }
        }
    }
    // points inside, on a grid, away from the outline
    let (mut bx0, mut by0, mut bx1, mut by1) = (f32::MAX, f32::MAX, f32::MIN, f32::MIN);
    for p in &boundary {
        bx0 = bx0.min(p[0]);
        by0 = by0.min(p[1]);
        bx1 = bx1.max(p[0]);
        by1 = by1.max(p[1]);
    }
    let near = |x: f32, y: f32| boundary.iter().any(|p| (p[0] - x).hypot(p[1] - y) < spacing * 0.55);
    let mut y = by0 + spacing * 0.5;
    let mut row = 0;
    while y < by1 {
        // every second row is shifted half a step, so the triangles come out even
        let mut x = bx0 + spacing * if row % 2 == 0 { 0.5 } else { 1.0 };
        while x < bx1 {
            if inside(&rings, x, y) && !near(x, y) {
                let _ = t.insert(Point2::new(x as f64, y as f64));
            }
            x += spacing;
        }
        y += spacing * 0.866;
        row += 1;
    }
    let mut verts: Vec<[f32; 2]> = vec![[0.0, 0.0]; t.num_vertices()];
    for v in t.vertices() {
        let p = v.position();
        verts[v.fix().index()] = [p.x as f32, p.y as f32];
    }
    let mut tris = Vec::new();
    for f in t.inner_faces() {
        let vs = f.vertices();
        let idx = [vs[0].fix().index() as u32, vs[1].fix().index() as u32, vs[2].fix().index() as u32];
        let (a, b, c) = (verts[idx[0] as usize], verts[idx[1] as usize], verts[idx[2] as usize]);
        let area = ((b[0] - a[0]) * (c[1] - a[1]) - (c[0] - a[0]) * (b[1] - a[1])).abs() / 2.0;
        let centre = [(a[0] + b[0] + c[0]) / 3.0, (a[1] + b[1] + c[1]) / 3.0];
        if area > 1e-3 && inside(&rings, centre[0], centre[1]) {
            tris.push(idx);
        }
    }
    // keep only the vertices that are used
    let mut used = vec![u32::MAX; verts.len()];
    let mut out_v = Vec::new();
    for t3 in &mut tris {
        for i in t3.iter_mut() {
            if used[*i as usize] == u32::MAX {
                used[*i as usize] = out_v.len() as u32;
                out_v.push(verts[*i as usize]);
            }
            *i = used[*i as usize];
        }
    }
    (!tris.is_empty()).then_some((out_v, tris))
}

#[cfg(test)]
pub fn mesh_area(verts: &[[f32; 2]], tris: &[[u32; 3]]) -> f32 {
    tris.iter()
        .map(|t| {
            let (a, b, c) = (verts[t[0] as usize], verts[t[1] as usize], verts[t[2] as usize]);
            ((b[0] - a[0]) * (c[1] - a[1]) - (c[0] - a[0]) * (b[1] - a[1])).abs() / 2.0
        })
        .sum()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::doc::{Document, LayerKind};
    use eframe::egui::Color32;

    fn disc_layer(d: &mut Document, cx: i32, cy: i32, r_out: i32, r_in: i32) -> Layer {
        let mut l = d.new_layer("Part", LayerKind::Raster);
        let t = crate::doc::TILE as i32;
        for y in cy - r_out - 2..=cy + r_out + 2 {
            for x in cx - r_out - 2..=cx + r_out + 2 {
                let r = (((x - cx).pow(2) + (y - cy).pow(2)) as f32).sqrt();
                if r <= r_out as f32 && r >= r_in as f32 {
                    let tile = l.tiles.entry((x.div_euclid(t), y.div_euclid(t))).or_insert_with(|| std::sync::Arc::new(vec![Color32::TRANSPARENT; crate::doc::TILE * crate::doc::TILE]));
                    std::sync::Arc::make_mut(tile)[(y.rem_euclid(t) * t + x.rem_euclid(t)) as usize] = Color32::from_rgb(200, 60, 60);
                }
            }
        }
        l
    }

    #[test]
    fn a_disc_becomes_triangles_that_cover_it() {
        let mut d = Document::new(800, 800);
        let l = disc_layer(&mut d, 400, 400, 200, 0);
        let (v, t) = generate(&l, d.w, d.h, 0.5).expect("a mesh");
        let area = mesh_area(&v, &t);
        let disc = std::f32::consts::PI * 200.0 * 200.0;
        assert!((area - disc).abs() / disc < 0.06, "area {area} vs {disc}");
        assert!(t.len() > 20, "{} triangles", t.len());
        assert!(t.iter().flatten().all(|i| (*i as usize) < v.len()));
        // the vertices lie on or inside the disc (a little slack for the outline)
        assert!(v.iter().all(|p| (p[0] - 400.0).hypot(p[1] - 400.0) < 206.0));
        // every vertex is used by a triangle
        let mut used = vec![false; v.len()];
        for tr in &t {
            for i in tr {
                used[*i as usize] = true;
            }
        }
        assert!(used.iter().all(|u| *u));
    }

    #[test]
    fn denser_means_more_triangles() {
        let mut d = Document::new(800, 800);
        let l = disc_layer(&mut d, 400, 400, 250, 0);
        let few = generate(&l, d.w, d.h, 0.0).unwrap().1.len();
        let many = generate(&l, d.w, d.h, 1.0).unwrap().1.len();
        assert!(many > few * 4, "{few} -> {many}");
    }

    #[test]
    fn a_ring_keeps_its_hole() {
        let mut d = Document::new(800, 800);
        let l = disc_layer(&mut d, 400, 400, 250, 120);
        let (v, t) = generate(&l, d.w, d.h, 0.6).expect("a mesh");
        let area = mesh_area(&v, &t);
        let ring = std::f32::consts::PI * (250.0 * 250.0 - 120.0 * 120.0);
        assert!((area - ring).abs() / ring < 0.08, "area {area} vs {ring}");
        // no triangle sits in the middle of the hole
        for tr in &t {
            let c: Vec<[f32; 2]> = tr.iter().map(|i| v[*i as usize]).collect();
            let m = [(c[0][0] + c[1][0] + c[2][0]) / 3.0, (c[0][1] + c[1][1] + c[2][1]) / 3.0];
            assert!((m[0] - 400.0).hypot(m[1] - 400.0) > 110.0, "{m:?}");
        }
    }

    #[test]
    fn an_empty_layer_has_no_mesh_and_a_big_layer_is_traced_at_a_smaller_size() {
        let mut d = Document::new(4000, 4000);
        let e = d.new_layer("Empty", LayerKind::Raster);
        assert!(generate(&e, d.w, d.h, 0.5).is_none());
        let l = disc_layer(&mut d, 2000, 2000, 1500, 0);
        let (v, t) = generate(&l, d.w, d.h, 0.3).expect("a mesh");
        let disc = std::f32::consts::PI * 1500.0 * 1500.0;
        assert!((mesh_area(&v, &t) - disc).abs() / disc < 0.05);
        assert!(v.iter().all(|p| (0.0..=4000.0).contains(&p[0]) && (0.0..=4000.0).contains(&p[1])));
    }
}
