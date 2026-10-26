// SPDX-License-Identifier: GPL-3.0-or-later
// A 3D character from a shape that an AI made of the drawing (Hunyuan3D, `tools/ai/shape3d.py`): the shape is fitted onto the
// drawing so both have the same outline, the drawing is laid on it from the front (and, mirrored, from the back), the skin
// weights are the ones painted on the picture for the skeleton of the auto-rig, and the face gets the same blend shapes as
// the card made from the drawing. Wings, shards and tails that stay rigid simply follow the bone they hang on.

use std::collections::HashMap;

use image::RgbaImage;

use super::gltf::read_glb;
use super::model::{Bone, Model3d, Options, Part3d, UncutSetup, add_face, face_marks, head_box, limb_group, normalize_to_t_pose, normals_of, point_weights, seg_dist, uncut_setup};
use crate::doc::Document;

/// A mesh without colours: points (x right, y up, z towards the viewer) and triangles.
#[derive(Clone, Debug)]
pub struct Shape {
    pub verts: Vec<[f32; 3]>,
    pub tris: Vec<[u32; 3]>,
}

impl Shape {
    /// Read the points and triangles of every mesh in a glTF binary.
    pub fn from_glb(bytes: &[u8]) -> Result<Shape, String> {
        let g = read_glb(bytes)?;
        let (mut verts, mut tris): (Vec<[f32; 3]>, Vec<[u32; 3]>) = (Vec::new(), Vec::new());
        for m in g.json["meshes"].as_array().ok_or("The file has no meshes.")? {
            for p in m["primitives"].as_array().ok_or("A mesh has no primitives.")? {
                let pos = g.vec3s(p["attributes"]["POSITION"].as_u64().ok_or("A mesh has no positions.")? as usize)?;
                let base = verts.len() as u32;
                let idx = match p["indices"].as_u64() {
                    Some(i) => g.index_list(i as usize)?,
                    None => (0..pos.len() as u32).collect(),
                };
                if idx.iter().any(|i| *i as usize >= pos.len()) {
                    return Err("A triangle points past the last point.".into());
                }
                tris.extend(idx.chunks_exact(3).map(|c| [base + c[0], base + c[1], base + c[2]]));
                verts.extend(pos);
            }
        }
        if verts.len() < 100 || tris.is_empty() || verts.iter().any(|v| v.iter().any(|x| !x.is_finite())) {
            return Err("The shape is empty or has points that are not numbers.".into());
        }
        Ok(Shape { verts, tris })
    }
}

/// Paint the transparent texels with the colour next to them, so a triangle that leans away from the viewer (the side of an
/// arm) still finds colour at the edge of the outline instead of a hole.
fn spread_colour(img: &mut RgbaImage) {
    let open: Vec<bool> = img.pixels().map(|p| p.0[3] < 128).collect();
    fill_open(img, open);
    for p in img.pixels_mut() {
        p.0[3] = 255;
    }
}

/// Fill the places marked `open` with the colours around them: first copied inward from the edge, then smoothed by letting every
/// filled place take the average of its neighbours again and again, so a gap becomes a soft passage and not blocks and streaks.
pub(super) fn fill_open(img: &mut RgbaImage, mut open: Vec<bool>) {
    let (w, h) = (img.width() as i32, img.height() as i32);
    let was_open = open.clone();
    for _ in 0..24 {
        let mut changed = Vec::new();
        for y in 0..h {
            for x in 0..w {
                if !open[(y * w + x) as usize] {
                    continue;
                }
                let (mut sum, mut n) = ([0u32; 3], 0u32);
                for (dx, dy) in [(1, 0), (-1, 0), (0, 1), (0, -1)] {
                    let (nx, ny) = (x + dx, y + dy);
                    if nx >= 0 && ny >= 0 && nx < w && ny < h && !open[(ny * w + nx) as usize] {
                        let p = img.get_pixel(nx as u32, ny as u32).0;
                        for k in 0..3 {
                            sum[k] += p[k] as u32;
                        }
                        n += 1;
                    }
                }
                if n > 0 {
                    changed.push((x, y, [(sum[0] / n) as u8, (sum[1] / n) as u8, (sum[2] / n) as u8]));
                }
            }
        }
        if changed.is_empty() {
            break;
        }
        for (x, y, c) in changed {
            img.put_pixel(x as u32, y as u32, image::Rgba([c[0], c[1], c[2], 255]));
            open[(y * w + x) as usize] = false;
        }
    }
    let n = (w * h) as usize;
    let mut cur: Vec<[f32; 3]> = img.pixels().map(|p| [p.0[0] as f32, p.0[1] as f32, p.0[2] as f32]).collect();
    let mut next = cur.clone();
    for _ in 0..40 {
        for y in 0..h {
            for x in 0..w {
                let i = (y * w + x) as usize;
                if !was_open[i] {
                    continue;
                }
                let mut sum = [0.0f32; 3];
                let mut c = 0.0f32;
                for (dx, dy) in [(1, 0), (-1, 0), (0, 1), (0, -1)] {
                    let (nx, ny) = (x + dx, y + dy);
                    if nx >= 0 && ny >= 0 && nx < w && ny < h {
                        let v = cur[(ny * w + nx) as usize];
                        sum[0] += v[0];
                        sum[1] += v[1];
                        sum[2] += v[2];
                        c += 1.0;
                    }
                }
                next[i] = [sum[0] / c, sum[1] / c, sum[2] / c];
            }
        }
        std::mem::swap(&mut cur, &mut next);
    }
    for i in 0..n {
        if was_open[i] {
            let (x, y) = ((i as i32 % w) as u32, (i as i32 / w) as u32);
            let a = img.get_pixel(x, y).0[3];
            img.put_pixel(x, y, image::Rgba([cur[i][0] as u8, cur[i][1] as u8, cur[i][2] as u8, a.max(255)]));
        }
    }
}

/// How deep the head stays compared with what the AI made (1 is as made).
const HEAD_DEPTH: f32 = 1.0;

pub fn build_from_shape(doc: &Document, shape: &Shape, opt: &Options) -> Option<Model3d> {
    let UncutSetup { page, vb, height_px: _, scale, xc, bones, slot, frames } = uncut_setup(doc, opt)?;
    let rig = &*doc.rig;
    let sk = rig.skin.as_ref()?;
    let to3 = |p: [f32; 2], z: f32| -> [f32; 3] { [(p[0] - xc) * scale, (vb[3] - p[1]) * scale, z] };
    // fit the shape onto the outline of the drawing: the same width and height, the depth as much as the mean of the two
    let (mut lo, mut hi) = ([f32::MAX; 3], [f32::MIN; 3]);
    for v in &shape.verts {
        for k in 0..3 {
            lo[k] = lo[k].min(v[k]);
            hi[k] = hi[k].max(v[k]);
        }
    }
    if hi[0] - lo[0] < 1e-6 || hi[1] - lo[1] < 1e-6 {
        return None;
    }
    let (kx, ky) = ((vb[2] - vb[0]) / (hi[0] - lo[0]), (vb[3] - vb[1]) / (hi[1] - lo[1]));
    let (kz, zmid) = ((kx + ky) / 2.0, (lo[2] + hi[2]) / 2.0);
    let mut pts: Vec<[f32; 3]> = shape.verts.iter().map(|p| [vb[0] + (p[0] - lo[0]) * kx, vb[1] + (hi[1] - p[1]) * ky, (p[2] - zmid) * kz]).collect();
    // An AI makes the head deeper than a drawn one (the hair stands out behind it), and then the face looks small and pushed
    // to the front when the head is turned. The head is made shallower: full depth at the neck, less above the chin.
    if let Some(hb) = head_box(rig) {
        let hh = hb[2] / 2.0;
        let chin = hb[1] + 0.1 * hh + hh;
        for p in &mut pts {
            let t = ((chin + 0.35 * hh - p[1]) / (0.35 * hh)).clamp(0.0, 1.0);
            let t = t * t * (3.0 - 2.0 * t);
            p[2] *= 1.0 - (1.0 - HEAD_DEPTH) * t;
        }
    }
    let mut tris = shape.tris.clone();
    // finer around the eyes and the mouth, so they can close and open
    let marks_drawn = if opt.face { face_marks(rig) } else { None };
    // the parts of the picture the user has moved: the blend shapes and the mouth follow where the features now are
    let tex = sk.tex.clone();
    let marks = marks_drawn.map(|m| tex.effective(&m));
    if let Some(m) = &marks {
        let (c, dist, fine) = (m.eye_center(), m.eye_distance(), (m.eye_w / 4.5).max(1.5));
        let m = *m;
        let inside = move |p: [f32; 2]| (p[0] - c[0]).abs() < dist * 0.9 + m.eye_w && p[1] > c[1] - 1.5 * m.eye_w && p[1] < m.mouth[1] + 0.9 * m.mouth_w;
        let (p2, t2) = crate::rig::refine::subdivide_xyz(&pts, &tris, &move |p| if inside(p) { fine } else { f32::MAX }, 600_000);
        pts = p2;
        tris = t2;
    }
    // The jaw of the AI's face and the jaw of the drawing end at different heights, and turned to the side the drawn face then sits
    // too high on the shape. Where the chin of the shape ends (its profile drops away toward the neck) and where the chin of
    // the drawing ends (the dark line of the chin below the mouth) are measured, and the drawn face is moved down (or up) by the
    // difference, fading out above the eyes and below the chin. The shape itself stays as it is.
    let mut face_down: Option<[f32; 4]> = None;
    let mut mesh_chin: Option<f32> = None;
    if let Some(m) = &marks_drawn {
        let (c, dist) = (m.eye_center(), m.eye_distance());
        let eye_y = (m.eye_l[1] + m.eye_r[1]) / 2.0;
        let d = (m.mouth[1] - eye_y).max(8.0);
        // the chin of the drawing: the darkest row under the mouth, in the middle of the face
        let lum = |x: f32, y: f32| -> Option<f32> {
            let (xi, yi) = (x as i32, y as i32);
            if xi < 0 || yi < 0 || xi >= page.width() as i32 || yi >= page.height() as i32 {
                return None;
            }
            let p = page.get_pixel(xi as u32, yi as u32).0;
            (p[3] >= 128).then(|| 0.3 * p[0] as f32 + 0.59 * p[1] as f32 + 0.11 * p[2] as f32)
        };
        let mut rows: Vec<(f32, f32)> = Vec::new();
        let mut y = m.mouth[1] + 0.35 * d;
        while y < m.mouth[1] + 1.5 * d {
            let vals: Vec<f32> = (-4..=4).filter_map(|k| lum(c[0] + k as f32 * dist * 0.06, y)).collect();
            if vals.len() >= 5 {
                rows.push((y, vals.iter().sum::<f32>() / vals.len() as f32));
            }
            y += 1.0;
        }
        let drawn = rows.iter().min_by(|a, b| a.1.total_cmp(&b.1)).copied();
        let mut sorted: Vec<f32> = rows.iter().map(|r| r.1).collect();
        sorted.sort_by(|a, b| a.total_cmp(b));
        let median = sorted.get(sorted.len() / 2).copied().unwrap_or(0.0);
        // the chin of the shape: the lowest row of its profile (the front-most points) that is still part of the face
        let profile = |yy: f32| pts.iter().filter(|p| (p[0] - c[0]).abs() < dist * 0.18 && (p[1] - yy).abs() < 1.5).map(|p| p[2]).fold(f32::MIN, f32::max);
        let (z_mouth, z_low) = (profile(m.mouth[1]), profile(m.mouth[1] + 2.4 * d));
        let level = 0.5 * (z_mouth + z_low);
        let mut shape_chin = None;
        let mut yy = m.mouth[1];
        while yy < m.mouth[1] + 2.4 * d {
            let z = profile(yy);
            if z != f32::MIN && z >= level {
                shape_chin = Some(yy);
            }
            yy += 1.0;
        }
        if let (Some((y_d, dark)), Some(y_g)) = (drawn, shape_chin) {
            if std::env::var("SEVIATH_DEBUG_FACE").is_ok() {
                eprintln!("chin of the drawing at y {y_d:.1} (dark {dark:.0}, median {median:.0}), chin of the shape at y {y_g:.1}; eyes {eye_y:.1}, mouth {:.1}", m.mouth[1]);
            }
            mesh_chin = Some(y_g);
            if median - dark > 18.0 {
                // how far the drawing must move down, where it is whole, and the chin
                face_down = Some([(y_g - y_d).clamp(-0.8 * d, 0.8 * d), eye_y - 0.5 * d, y_g, d]);
            }
        }
    }
    // the triangles must face outward: if most of them face the middle, turn them all round
    let centre = [(vb[0] + vb[2]) / 2.0, (vb[1] + vb[3]) / 2.0, 0.0];
    let mut outward = 0.0f64;
    for t in &tris {
        let (a, b, c) = (pts[t[0] as usize], pts[t[1] as usize], pts[t[2] as usize]);
        let (u, v) = ([b[0] - a[0], b[1] - a[1], b[2] - a[2]], [c[0] - a[0], c[1] - a[1], c[2] - a[2]]);
        // the picture's y points down, so the cross product turns the other way
        let n = [u[1] * v[2] - u[2] * v[1], u[2] * v[0] - u[0] * v[2], u[0] * v[1] - u[1] * v[0]];
        let m = [(a[0] + b[0] + c[0]) / 3.0 - centre[0], (a[1] + b[1] + c[1]) / 3.0 - centre[1], (a[2] + b[2] + c[2]) / 3.0 - centre[2]];
        outward += (n[0] * m[0] + n[1] * m[1] + n[2] * m[2]) as f64;
    }
    // the model's y goes up while the picture's goes down, which turns the winding once more
    if outward > 0.0 {
        for t in &mut tris {
            t.swap(1, 2);
        }
    }
    let model_pts: Vec<[f32; 3]> = pts.iter().map(|p| to3([p[0], p[1]], p[2] * scale)).collect();
    // the texture: the drawing from the front, and mirrored from the back, side by side
    let painted = paint_layer_image(doc, &tex);
    let Front { tile: mut front, mask, cx0, cy0, tw, th } = make_front(&page, vb, rig, &tex, &marks_drawn, opt.max_texture, true, painted.as_ref(), opt.mesh_only)?;
    let mut back = image::imageops::flip_horizontal(&front);
    // the back of the head is hair (or the top of the head), not the face of the front turned round
    if let Some(hb) = head_box(rig) {
        let k = front.width() as f32 / tw;
        let hh = hb[2] / 2.0;
        let (fx0, fx1) = ((((hb[0] - cx0 as f32) * k).max(0.0)) as u32, (((hb[0] + hb[2] - cx0 as f32) * k) as u32).min(front.width()));
        let fy0 = (((hb[1] + 0.1 * hh - cy0 as f32) * k).max(0.0)) as u32;
        let fy1 = (fy0 + (hh * k) as u32).min(front.height());
        // the colour of the top of the head: the middle of the upper third of it
        let band_y1 = fy0 + (fy1.saturating_sub(fy0)) / 3;
        let (mut sum, mut n) = ([0u64; 3], 0u64);
        for y in fy0..band_y1.max(fy0 + 1).min(front.height()) {
            for x in (fx0 + (fx1.saturating_sub(fx0)) / 4)..(fx1 - (fx1.saturating_sub(fx0)) / 4).max(fx0 + 1) {
                if x < front.width() && mask[(y * front.width() + x) as usize] {
                    let p = front.get_pixel(x, y).0;
                    for c in 0..3 {
                        sum[c] += p[c] as u64;
                    }
                    n += 1;
                }
            }
        }
        if n > 20 {
            let hair = [(sum[0] / n) as u8, (sum[1] / n) as u8, (sum[2] / n) as u8];
            let w = front.width();
            for y in fy0..fy1 {
                for x in fx0..fx1.min(w) {
                    if mask[(y * w + x) as usize] {
                        back.put_pixel(w - 1 - x, y, image::Rgba([hair[0], hair[1], hair[2], 255]));
                    }
                }
            }
        }
    }
    for p in back.pixels_mut() {
        for k in 0..3 {
            p.0[k] = (p.0[k] as f32 * 0.82) as u8;
        }
    }
    // The atlas: the front and the back side by side, and under them a small blurred copy of each, for the sides.
    let (fw, fh) = (front.width(), front.height());
    let (sw, sh) = ((fw / 2).max(1), (fh / 2).max(1));
    let soft = |img: &RgbaImage| image::imageops::blur(&image::imageops::resize(img, sw, sh, image::imageops::FilterType::Triangle), (0.012 * sh as f32).max(2.0));
    let (soft_front, soft_back) = (soft(&front), soft(&back));
    let (aw, ah) = (fw * 2, fh + sh);
    let mut atlas = RgbaImage::from_pixel(aw, ah, image::Rgba([200, 180, 170, 255]));
    image::imageops::replace(&mut atlas, &front, 0, 0);
    image::imageops::replace(&mut atlas, &back, fw as i64, 0);
    image::imageops::replace(&mut atlas, &soft_front, 0, fh as i64);
    image::imageops::replace(&mut atlas, &soft_back, sw as i64, fh as i64);
    // what was painted straight on the 3D character
    if let Some(p) = tex.atlas_paint_image() {
        if p.dimensions() == atlas.dimensions() {
            for (d, s) in atlas.pixels_mut().zip(p.pixels()) {
                let a = s.0[3] as f32 / 255.0;
                if a > 0.0 {
                    for c in 0..3 {
                        d.0[c] = (d.0[c] as f32 * (1.0 - a) + s.0[c] as f32 * a) as u8;
                    }
                }
            }
        }
    }
    let (awf, ahf, fwf, fhf, swf, shf) = (aw as f32, ah as f32, fw as f32, fh as f32, sw as f32, sh as f32);
    // The points are made once for the front and once for the back. A triangle that leans away from the viewer (the side of
    // an arm, under the arm) would stretch the edge of the drawing, the dark outline, across itself: it gets points of its
    // own whose picture is taken a little way inside the outline, so the sides come out as plain skin or cloth.
    let mut copy_of: HashMap<(u32, bool), u32> = HashMap::new();
    let mut made: Vec<(u32, bool, Option<[f32; 2]>)> = Vec::new();
    let mut out_tris: Vec<[u32; 3]> = Vec::with_capacity(tris.len());
    let inward_px = 0.02 * (vb[3] - vb[1]);
    let head_x = head_box(rig).map(|h| h[0] + h[2] / 2.0).unwrap_or((vb[0] + vb[2]) / 2.0);
    let head_bottom = head_box(rig).map(|h| h[1] + 0.1 * (h[2] / 2.0) + h[2] / 2.0 + 0.02 * (vb[3] - vb[1])).unwrap_or(vb[1]);
    // What the AI made outside the outline of the drawing has no picture to wear (a wing in another place, a skirt that is wider): it would
    // be stretched colour. Triangles whose three points and middle all lie outside the drawn outline (a few pixels wider) are left out.
    let (mw, mh) = (fw as i32, fh as i32);
    let kk = fwf / tw;
    let reach = ((0.006 * fwf.max(fhf)).round() as i32).max(3);
    let mut near_drawing = vec![false; mask.len()];
    {
        // dilate the outline with two passes (along the rows, then along the columns)
        let mut rows = vec![false; mask.len()];
        for y in 0..mh {
            let mut last: i32 = i32::MIN / 2;
            let mut next: i32 = i32::MAX / 2;
            let row: Vec<i32> = (0..mw).filter(|x| mask[(y * mw + x) as usize]).collect();
            let mut at = 0usize;
            for x in 0..mw {
                while at < row.len() && row[at] < x {
                    last = row[at];
                    at += 1;
                }
                next = if at < row.len() { row[at] } else { i32::MAX / 2 };
                rows[(y * mw + x) as usize] = (x - last).abs() <= reach || (next - x).abs() <= reach;
            }
            let _ = (last, next);
        }
        for x in 0..mw {
            let col: Vec<i32> = (0..mh).filter(|y| rows[(y * mw + x) as usize]).collect();
            let mut at = 0usize;
            let mut last: i32 = i32::MIN / 2;
            for y in 0..mh {
                while at < col.len() && col[at] < y {
                    last = col[at];
                    at += 1;
                }
                let next = if at < col.len() { col[at] } else { i32::MAX / 2 };
                near_drawing[(y * mw + x) as usize] = (y - last).abs() <= reach || (next - y).abs() <= reach;
            }
        }
    }
    let on_drawing = |p: [f32; 3]| -> bool {
        let (x, y) = (((p[0] - cx0 as f32) * kk) as i32, ((p[1] - cy0 as f32) * kk) as i32);
        x >= 0 && y >= 0 && x < mw && y < mh && near_drawing[(y * mw + x) as usize]
    };
    let mut left_out = 0usize;
    for t in &tris {
        let (pa, pb, pc) = (pts[t[0] as usize], pts[t[1] as usize], pts[t[2] as usize]);
        let mid = [(pa[0] + pb[0] + pc[0]) / 3.0, (pa[1] + pb[1] + pc[1]) / 3.0, 0.0];
        if !(on_drawing(pa) || on_drawing(pb) || on_drawing(pc) || on_drawing(mid)) {
            left_out += 1;
            continue;
        }
        let (a, b, c) = (model_pts[t[0] as usize], model_pts[t[1] as usize], model_pts[t[2] as usize]);
        let (u, v) = ([b[0] - a[0], b[1] - a[1], b[2] - a[2]], [c[0] - a[0], c[1] - a[1], c[2] - a[2]]);
        let n = [u[1] * v[2] - u[2] * v[1], u[2] * v[0] - u[0] * v[2], u[0] * v[1] - u[1] * v[0]];
        let len = (n[0] * n[0] + n[1] * n[1] + n[2] * n[2]).sqrt().max(1e-12);
        let is_back = n[2] < 0.0;
        // the head and its hair keep their picture on curved parts; the body only where it really turns away
        let cy = (pts[t[0] as usize][1] + pts[t[1] as usize][1] + pts[t[2] as usize][1]) / 3.0;
        let near_head = cy < head_bottom + 0.05 * (vb[3] - vb[1]);
        let side = n[2].abs() / len < if near_head { 0.38 } else { 0.62 };
        let mut o = [0u32; 3];
        if side {
            // inward on the page: against the way the surface faces (the model's y is up, the page's y is down)
            let (dx, dy) = (-n[0], n[1]);
            let l = dx.hypot(dy).max(1e-12);
            // the neck and the head are narrow: a short step only, or it lands on the hair or the background
            let step = if near_head { inward_px * 0.25 } else { inward_px };
            let shift = [dx / l * step, dy / l * step];
            for k in 0..3 {
                // under the chin the sides of the neck are hidden behind the hair in the drawing: take the skin down its middle
                let neck = cy > head_bottom - 0.01 * (vb[3] - vb[1]) && near_head;
                let mut sh = if neck { [head_x - pts[t[k] as usize][0], 0.0] } else { shift };
                // a step that would land outside the drawing (the tip of a wing, the edge of a skirt) takes colour from nowhere: no step
                let p = pts[t[k] as usize];
                let (sx, sy) = (((p[0] + sh[0] - cx0 as f32) * kk) as i32, ((p[1] + sh[1] - cy0 as f32) * kk) as i32);
                if sx < 0 || sy < 0 || sx >= mw || sy >= mh || !mask[(sy * mw + sx) as usize] {
                    sh = [0.0, 0.0];
                }
                made.push((t[k], is_back, Some(sh)));
                o[k] = (made.len() - 1) as u32;
            }
        } else {
            for k in 0..3 {
                o[k] = *copy_of.entry((t[k], is_back)).or_insert_with(|| {
                    made.push((t[k], is_back, None));
                    (made.len() - 1) as u32
                });
            }
        }
        out_tris.push(o);
    }
    if std::env::var("SEVIATH_DEBUG_FACE").is_ok() {
        eprintln!("triangles outside the drawn outline left out: {left_out} of {}", tris.len());
    }
    let verts: Vec<[f32; 3]> = made.iter().map(|(i, _, _)| model_pts[*i as usize]).collect();
    // how far the drawing is moved down at a height of the picture
    let face_shift = |y: f32| -> f32 {
        match face_down {
            Some([dy, top, chin, d]) => {
                let (fade_in, fade_out) = (1.2 * d, 0.9 * d);
                let up = ((y - (top - fade_in)) / fade_in).clamp(0.0, 1.0);
                let down = ((chin + fade_out - y) / fade_out).clamp(0.0, 1.0);
                let sm = |t: f32| t * t * (3.0 - 2.0 * t);
                dy * sm(up) * sm(down)
            }
            None => 0.0,
        }
    };
    let uvs: Vec<[f32; 2]> = made
        .iter()
        .map(|(i, back, side)| {
            let p = pts[*i as usize];
            let sh = side.unwrap_or([0.0, 0.0]);
            let p = [p[0], p[1] - face_shift(p[1])];
            let (u, v) = (((p[0] + sh[0] - cx0 as f32) / tw).clamp(0.0, 1.0), ((p[1] + sh[1] - cy0 as f32) / th).clamp(0.0, 1.0));
            match (side.is_some(), *back) {
                (false, false) => [u * fwf / awf, v * fhf / ahf],
                (false, true) => [(fwf + (1.0 - u) * fwf) / awf, v * fhf / ahf],
                (true, false) => [u * swf / awf, (fhf + v * shf) / ahf],
                (true, true) => [(swf + (1.0 - u) * swf) / awf, (fhf + v * shf) / ahf],
            }
        })
        .collect();
    if std::env::var("SEVIATH_DEBUG_FACE").is_ok() {
        let outside = made.iter().filter(|(i, _, side)| {
            let p = pts[*i as usize];
            let sh = side.unwrap_or([0.0, 0.0]);
            let (u, v) = ((p[0] + sh[0] - cx0 as f32) / tw, (p[1] + sh[1] - cy0 as f32) / th);
            !(0.0..=1.0).contains(&u) || !(0.0..=1.0).contains(&v)
        }).count();
        let (mut stretched, mut total) = (0usize, 0usize);
        for t in &out_tris {
            let uv = |k: usize| uvs[t[k] as usize];
            let (a, b, c) = (uv(0), uv(1), uv(2));
            let area_uv = ((b[0] - a[0]) * (c[1] - a[1]) - (c[0] - a[0]) * (b[1] - a[1])).abs() * 0.5;
            let (p, q, r) = (verts[t[0] as usize], verts[t[1] as usize], verts[t[2] as usize]);
            let (u, w) = ([q[0] - p[0], q[1] - p[1], q[2] - p[2]], [r[0] - p[0], r[1] - p[1], r[2] - p[2]]);
            let n = [u[1] * w[2] - u[2] * w[1], u[2] * w[0] - u[0] * w[2], u[0] * w[1] - u[1] * w[0]];
            let area3 = 0.5 * (n[0] * n[0] + n[1] * n[1] + n[2] * n[2]).sqrt();
            total += 1;
            if area3 > 1e-9 && area_uv / area3 < 1e-3 {
                stretched += 1;
            }
        }
        eprintln!("points whose picture lies outside the drawing's frame: {outside} of {}; triangles whose picture is a thin sliver: {stretched} of {total}", made.len());
    }
    let normals = normals_of(&verts, &out_tris);
    // skin weights: what is painted for the skeleton at the place of the point on the picture
    let ix_of: Vec<Option<usize>> = sk.bones.iter().map(|b| rig.changer(b.changer).and_then(|c| slot.get(c.name.as_str()).copied())).collect();
    // The painted weights belong to the drawing; an AI shape is never quite the drawing (an arm a little higher, a leg a little
    // wider), and where it differs the painted weights would say "body" for a point that is really on the arm. So the weights
    // are also worked out from the distance to the bones, and where the two do not agree on the limb the distance wins.
    let n_shape = shape.verts.len();
    let ai = opt.ai_skin.as_ref().filter(|a| a.weights.len() == n_shape);
    let weights_of: Vec<Vec<(usize, f32)>> = (0..model_pts.len())
        .map(|i| {
            // the weights an AI worked out for the shape itself (the points made later, around the eyes and the mouth, are the head's)
            if let Some(ai) = ai.filter(|_| i < n_shape) {
                let w = ai.weights_for(i, &slot, 0);
                if !w.is_empty() {
                    return w;
                }
            }
            let plane = point_weights(rig, sk, &bones, &slot, &frames, &ix_of, [pts[i][0], pts[i][1]], model_pts[i]);
            let near = distance_weights(&bones, model_pts[i]);
            let limb = dominant_limb(&bones, &near);
            // the painted weights where they agree on the limb, and then only the bones of that limb (a fingertip must not
            // follow the chest a little); else the distance weights
            let mut chosen: Vec<(usize, f32)> = if dominant_limb(&bones, &plane) == limb { plane.into_iter().filter(|(b, _)| limb_group(&bones[*b].name) == limb).collect() } else { Vec::new() };
            let total: f32 = chosen.iter().map(|c| c.1).sum();
            if total < 0.5 {
                chosen = near.into_iter().filter(|(b, _)| limb_group(&bones[*b].name) == limb).collect();
            }
            let total: f32 = chosen.iter().map(|c| c.1).sum::<f32>().max(1e-9);
            chosen.iter_mut().for_each(|c| c.1 /= total);
            chosen
        })
        .collect();
    // Every point follows one limb only, so where an arm meets the body the skin would tear when the arm moves. The weights are smoothed
    // along the surface: near a joint a point follows both bones a little, and far from it only its own.
    let weights_of = smooth_weights(&tris, weights_of, 14);
    let (mut joints, mut weights) = (Vec::with_capacity(verts.len()), Vec::with_capacity(verts.len()));
    for (orig, _, _) in &made {
        let mut acc = weights_of[*orig as usize].clone();
        if acc.iter().map(|a| a.1).sum::<f32>() < 0.001 {
            acc = vec![(0, 1.0)];
        }
        acc.sort_by(|a, b| b.1.total_cmp(&a.1));
        acc.truncate(4);
        let total: f32 = acc.iter().map(|a| a.1).sum::<f32>().max(1e-6);
        let (mut j, mut o) = ([0u16; 4], [0.0f32; 4]);
        for (i, (b, x)) in acc.iter().enumerate() {
            j[i] = *b as u16;
            o[i] = x / total;
        }
        joints.push(j);
        weights.push(o);
    }
    let mut part = Part3d { name: "Body".to_string(), layer: sk.layers.first().copied().unwrap_or(0), verts, normals, uvs, tris: out_tris, joints, weights, image: atlas, morphs: Vec::new() };
    // The AI joins what touches (a hand on a hip, two legs at the thigh, a sleeve on a skirt) into one surface. Pieces that move on their own
    // cannot share triangles: a triangle between two different limbs would stretch into a long line when they move apart. The body can join a
    // limb (that is a joint), a limb cannot join another limb.
    let cut = split_fused_limbs(&mut part, &bones);
    if std::env::var("SEVIATH_DEBUG_HAIR").is_ok() {
        eprintln!("limbs: {cut} triangles between two different limbs were cut");
    }
    let bones: Vec<Bone> = bones;
    let mut model = Model3d { name: opt.name.clone(), author: opt.author.clone(), parts: vec![part], bones, expressions: Vec::new(), springs: Vec::new(), colliders: Vec::new(), scale, landmarks: Vec::new(), hair_chains: Vec::new(), t_pose: false };
    if let Some(m) = &marks {
        // The face is a part of its own, as on a VRoid model: only it carries the blend shapes, so the shapes do not have to
        // be written for every point of the body. Its triangles are the ones that face the front inside the face box.
        let (c, dist) = (m.eye_center(), m.eye_distance());
        let m2 = *m;
        let in_face = move |p: [f32; 2]| (p[0] - c[0]).abs() < dist * 0.9 + m2.eye_w * 1.4 && p[1] > c[1] - 2.2 * m2.eye_w && p[1] < m2.mouth[1] + 1.4 * m2.mouth_w;
        let (face_part, rest) = split_part(&model.parts[0], |t| {
            let v = t.iter().map(|i| model.parts[0].verts[*i as usize]).collect::<Vec<_>>();
            let cen = [(v[0][0] + v[1][0] + v[2][0]) / 3.0 / scale + xc, vb[3] - (v[0][1] + v[1][1] + v[2][1]) / 3.0 / scale];
            let n = model.parts[0].normals[t[0] as usize];
            in_face(cen) && n[2] > 0.25
        });
        if let Some(face_part) = face_part {
            model.parts = vec![rest, face_part];
            add_face(&mut model, m, xc, vb[3], opt.template.as_deref(), true, 1, &tex);
        } else {
            add_face(&mut model, m, xc, vb[3], opt.template.as_deref(), true, 0, &tex);
        }
    }
    // the places of the features, for the guide lines of the preview: where the picture puts them, and where the shape has its own
    // nose and chin
    if let Some(m) = &marks {
        let (c, dist) = (m.eye_center(), m.eye_distance());
        let eye_y = (m.eye_l[1] + m.eye_r[1]) / 2.0;
        let d = (m.mouth[1] - eye_y).max(8.0);
        let front_z = |x: f32, y: f32| pts.iter().filter(|p| (p[0] - x).abs() < dist * 0.12 && (p[1] - y).abs() < d * 0.12).map(|p| p[2]).fold(f32::MIN, f32::max);
        let put = |name: &str, p: [f32; 2], model: &mut Model3d| {
            let z = front_z(p[0], p[1]);
            model.landmarks.push((name.to_string(), [(p[0] - xc) * scale, (vb[3] - p[1]) * scale, if z == f32::MIN { 0.0 } else { z * scale }]));
        };
        put("eye_l", m.eye_l, &mut model);
        put("eye_r", m.eye_r, &mut model);
        put("nose", super::texedit::TexEdit::nose_at(m), &mut model);
        if let Some(m0) = &marks_drawn {
            let base = [tex.head[0] + tex.face[0], tex.head[1] + tex.face[1]];
            for (name, left, off) in [("brow_l", true, tex.brow_l), ("brow_r", false, tex.brow_r)] {
                let b = super::texedit::TexEdit::brow_at(m0, left);
                put(name, [b[0] + base[0] + off[0], b[1] + base[1] + off[1]], &mut model);
            }
        }
        put("mouth", m.mouth, &mut model);
        let near = |p: &&[f32; 3]| (p[0] - c[0]).abs() < dist * 0.35 && p[1] > eye_y - 0.2 * d && p[1] < m.mouth[1] + 0.4 * d;
        if let Some(tip) = pts.iter().filter(near).max_by(|a, b| a[2].total_cmp(&b[2])) {
            model.landmarks.push(("mesh_nose".to_string(), [(tip[0] - xc) * scale, (vb[3] - tip[1]) * scale, tip[2] * scale]));
        }
        if let Some(y) = mesh_chin {
            put("mesh_chin", [c[0], y], &mut model);
        }
    }
    // the hair: found on the drawing, a part of its own on bones that swing
    if opt.hair {
        if let Some(hb) = head_box(rig) {
            if let Some(mut mask) = super::hair::hair_mask(&page, hb) {
                mask.apply_dabs(&sk.hair.dabs);
                super::hair::add_hair(&mut model, &mask, |v| [v[0] / scale + xc, vb[3] - v[1] / scale], &sk.hair, (xc, vb[3], scale));
            }
        }
    }
    if opt.t_pose {
        normalize_to_t_pose(&mut model);
    }
    Some(model)
}

/// The weights of every point mixed with the weights of the points next to it along the triangles, again and again (each point keeps its four
/// biggest shares). Points that belong to one bone entirely and are far from the others do not change.
pub fn smooth_weights(tris: &[[u32; 3]], weights: Vec<Vec<(usize, f32)>>, rounds: usize) -> Vec<Vec<(usize, f32)>> {
    let n = weights.len();
    let mut next_to: Vec<Vec<u32>> = vec![Vec::new(); n];
    for t in tris {
        for k in 0..3 {
            let (a, b) = (t[k] as usize, t[(k + 1) % 3]);
            if a < n && (b as usize) < n {
                next_to[a].push(b);
                next_to[b as usize].push(a as u32);
            }
        }
    }
    for list in &mut next_to {
        list.sort_unstable();
        list.dedup();
    }
    let mut w = weights;
    for _ in 0..rounds {
        let mut out: Vec<Vec<(usize, f32)>> = Vec::with_capacity(n);
        for i in 0..n {
            if next_to[i].is_empty() {
                out.push(w[i].clone());
                continue;
            }
            let mut acc: Vec<(usize, f32)> = Vec::with_capacity(8);
            let (own, other) = (0.5f32, 0.5 / next_to[i].len() as f32);
            for (b, x) in &w[i] {
                acc.push((*b, x * own));
            }
            for &j in &next_to[i] {
                for (b, x) in &w[j as usize] {
                    match acc.iter_mut().find(|a| a.0 == *b) {
                        Some(a) => a.1 += x * other,
                        None => acc.push((*b, x * other)),
                    }
                }
            }
            acc.sort_by(|a, b| b.1.total_cmp(&a.1));
            acc.truncate(4);
            let sum: f32 = acc.iter().map(|a| a.1).sum::<f32>().max(1e-9);
            acc.iter_mut().for_each(|a| a.1 /= sum);
            out.push(acc);
        }
        w = out;
    }
    w
}

/// Separate the limbs that the AI joined (the left arm, the right arm, the left leg, the right leg) without a gap: returns how many triangles
/// changed. The limb of a point is the one of the bone that moves it most.
pub fn split_fused_limbs(part: &mut Part3d, bones: &[Bone]) -> usize {
    let limb: Vec<usize> = (0..part.verts.len())
        .map(|i| {
            let best = (0..4).max_by(|a, b| part.weights[i][*a].total_cmp(&part.weights[i][*b])).unwrap_or(0);
            if part.weights[i][best] <= 0.0 {
                return 0;
            }
            bones.get(part.joints[i][best] as usize).map(|b| limb_group(&b.name)).unwrap_or(0)
        })
        .collect();
    super::model::separate_groups(part, &limb)
}

/// Weights from how close a point is to each bone (the bones are the segments from their head to their tail): the near ones
/// share it, the nearest by far.
fn distance_weights(bones: &[Bone], p: [f32; 3]) -> Vec<(usize, f32)> {
    let mut d: Vec<(usize, f32)> = bones.iter().enumerate().map(|(i, b)| (i, seg_dist(p, b.pos, b.tip))).collect();
    d.sort_by(|a, b| a.1.total_cmp(&b.1));
    d.truncate(4);
    let mut w: Vec<(usize, f32)> = d.iter().map(|(i, dist)| (*i, 1.0 / (dist + 0.015).powi(4))).collect();
    let sum: f32 = w.iter().map(|x| x.1).sum::<f32>().max(1e-12);
    for x in &mut w {
        x.1 /= sum;
    }
    w
}

/// The limb (body, an arm, a leg) that most of a set of weights lies on.
fn dominant_limb(bones: &[Bone], w: &[(usize, f32)]) -> usize {
    let mut g = [0.0f32; 5];
    for (b, x) in w {
        g[limb_group(&bones[*b].name)] += x;
    }
    (0..5).max_by(|a, b| g[*a].total_cmp(&g[*b])).unwrap_or(0)
}

/// Split a part in two by its triangles: the ones `take` says yes to make the first part (None when there are fewer than a few),
/// the others the second. Points are copied to the part that uses them.
fn split_part(part: &Part3d, take: impl Fn(&[u32; 3]) -> bool) -> (Option<Part3d>, Part3d) {
    let mut sides: [(HashMap<u32, u32>, Vec<u32>, Vec<[u32; 3]>); 2] = [(HashMap::new(), Vec::new(), Vec::new()), (HashMap::new(), Vec::new(), Vec::new())];
    for t in &part.tris {
        let k = if take(t) { 0 } else { 1 };
        let (map, order, tris) = &mut sides[k];
        let mut o = [0u32; 3];
        for c in 0..3 {
            o[c] = *map.entry(t[c]).or_insert_with(|| {
                order.push(t[c]);
                (order.len() - 1) as u32
            });
        }
        tris.push(o);
    }
    let make = |name: &str, (_, order, tris): &(HashMap<u32, u32>, Vec<u32>, Vec<[u32; 3]>)| Part3d {
        name: name.to_string(),
        layer: part.layer,
        verts: order.iter().map(|i| part.verts[*i as usize]).collect(),
        normals: order.iter().map(|i| part.normals[*i as usize]).collect(),
        uvs: order.iter().map(|i| part.uvs[*i as usize]).collect(),
        tris: tris.clone(),
        joints: order.iter().map(|i| part.joints[*i as usize]).collect(),
        weights: order.iter().map(|i| part.weights[*i as usize]).collect(),
        image: part.image.clone(),
        morphs: Vec::new(),
    };
    let face = if sides[0].2.len() >= 20 { Some(make("Face", &sides[0])) } else { None };
    (face, make("Body", &sides[1]))
}

/// The picture of the front of the character as laid on the shape: the drawing cut to the figure, the empty places filled, the
/// parts the user moved or picked again put right, and (when `with_paint`) what was painted by hand over it.
pub(super) struct Front {
    pub tile: RgbaImage,
    /// Which pixels were part of the drawing before the empty places were filled.
    pub mask: Vec<bool>,
    pub cx0: u32,
    pub cy0: u32,
    pub tw: f32,
    pub th: f32,
}

pub(super) fn make_front(page: &RgbaImage, vb: [f32; 4], rig: &crate::rig::Rig, tex: &super::texedit::TexEdit, marks_drawn: &Option<super::face::FaceMarks>, max_texture: u32, with_paint: bool, painted: Option<&RgbaImage>, plain: bool) -> Option<Front> {
    let (cx0, cy0) = ((vb[0].floor() as i64 - 2).max(0) as u32, (vb[1].floor() as i64 - 2).max(0) as u32);
    let (cx1, cy1) = (((vb[2].ceil() as u32) + 2).min(page.width()), ((vb[3].ceil() as u32) + 2).min(page.height()));
    if cx1 <= cx0 + 2 || cy1 <= cy0 + 2 {
        return None;
    }
    let crop = image::imageops::crop_imm(page, cx0, cy0, cx1 - cx0, cy1 - cy0).to_image();
    let (tw, th) = (crop.width() as f32, crop.height() as f32);
    let big = crop.width().max(crop.height());
    let mut front = if big > max_texture {
        let k = max_texture as f32 / big as f32;
        image::imageops::resize(&crop, ((tw * k).round() as u32).max(1), ((th * k).round() as u32).max(1), image::imageops::FilterType::Triangle)
    } else {
        crop
    };
    let mask: Vec<bool> = front.pixels().map(|p| p.0[3] >= 128).collect();
    spread_colour(&mut front);
    if plain {
        // a mesh to paint by hand: one neutral colour everywhere
        for p in front.pixels_mut() {
            p.0 = [205, 190, 180, 255];
        }
    }
    // the parts the user moved: cut out, the skin filled in, put down again
    if let Some(m0) = marks_drawn {
        let k = front.width() as f32 / tw;
        let mut t = tex.clone();
        if !with_paint {
            t.paint.clear();
        }
        t.apply(&mut front, m0, head_box(rig), [cx0 as f32, cy0 as f32], k);
    } else if with_paint {
        tex.apply_paint(&mut front);
    }
    // what was painted on the layer of the document with the brushes of the drawing app, over the whole result
    if let (true, Some(p)) = (with_paint, painted) {
        let crop = image::imageops::crop_imm(p, cx0, cy0, (p.width().saturating_sub(cx0)).min(tw as u32), (p.height().saturating_sub(cy0)).min(th as u32)).to_image();
        if crop.width() > 0 && crop.height() > 0 {
            let over = image::imageops::resize(&crop, front.width(), front.height(), image::imageops::FilterType::Triangle);
            for (d, s) in front.pixels_mut().zip(over.pixels()) {
                let a = s.0[3] as f32 / 255.0;
                if a > 0.0 {
                    for c in 0..3 {
                        d.0[c] = (d.0[c] as f32 * (1.0 - a) + s.0[c] as f32 * a) as u8;
                    }
                }
            }
        }
    }
    Some(Front { tile: front, mask, cx0, cy0, tw, th })
}

/// The edited picture of the front of the character without the hand painting, for the painter: (the picture, the tile pixels per
/// page pixel, the place of its top left on the page).
pub fn edited_front_tile(doc: &Document, opt: &Options) -> Option<(RgbaImage, f32, [f32; 2])> {
    let UncutSetup { page, vb, .. } = uncut_setup(doc, opt)?;
    let rig = &*doc.rig;
    let sk = rig.skin.as_ref()?;
    let marks_drawn = face_marks(rig);
    let f = make_front(&page, vb, rig, &sk.tex, &marks_drawn, opt.max_texture, false, None, opt.mesh_only)?;
    let k = f.tile.width() as f32 / f.tw;
    Some((f.tile, k, [f.cx0 as f32, f.cy0 as f32]))
}

/// What is painted on the paint layer of the document (the whole page, transparent where nothing is).
fn paint_layer_image(doc: &Document, tex: &super::texedit::TexEdit) -> Option<RgbaImage> {
    if tex.paint_layer == 0 {
        return None;
    }
    crate::rig::uncut::composite(doc, &[tex.paint_layer])
}

/// Make (or renew) the two layers the texture is painted on in the drawing app: a locked layer that shows the edited picture of the
/// front of the character in its place on the page, and above it an empty layer to paint on. Returns the layer to paint on.
pub fn make_paint_layers(doc: &mut Document, opt: &Options, guides: Option<&[(String, [f32; 3])]>) -> Option<u64> {
    let (tile, k, origin) = edited_front_tile(doc, opt)?;
    let (cw, ch) = ((tile.width() as f32 / k).round().max(1.0) as u32, (tile.height() as f32 / k).round().max(1.0) as u32);
    let mut on_page = image::imageops::resize(&tile, cw, ch, image::imageops::FilterType::Triangle);
    let tex = doc.rig.skin.as_ref()?.tex.clone();
    // lines at the places of the features, so they can be painted where they belong
    if let (Some(lm), Some(m0)) = (guides, face_marks(&doc.rig)) {
        let setup = uncut_setup(doc, opt)?;
        draw_guides(&mut on_page, origin, &m0, &tex, lm, (setup.xc, setup.vb[3], setup.scale));
    }
    let (ox, oy) = (origin[0] as i32, origin[1] as i32);
    // the view layer: renewed every time
    doc.layers.retain(|l| l.id != tex.view_layer);
    let mut view = doc.new_layer("Texture view (locked)", crate::doc::LayerKind::Raster);
    view.tiles = crate::files::tiles_from_rgba(&on_page, ox, oy, doc.w, doc.h);
    view.rev += 1;
    view.locked = true;
    let view_id = view.id;
    // the paint layer: kept when it is there
    let paint_id = if doc.layers.iter().any(|l| l.id == tex.paint_layer) {
        tex.paint_layer
    } else {
        let paint = doc.new_layer("Texture paint", crate::doc::LayerKind::Raster);
        let id = paint.id;
        doc.layers.insert(0, paint);
        id
    };
    // the view goes just under the paint layer
    let at = doc.layers.iter().position(|l| l.id == paint_id).map(|i| i + 1).unwrap_or(0);
    doc.layers.insert(at, view);
    // on a mesh to paint by hand the drawing itself shows, faint and locked, to paint from
    if opt.mesh_only {
        doc.layers.retain(|l| l.name != "Drawing (reference)");
        if let Some(page) = crate::rig::uncut::composite(doc, &doc.rig.skin.as_ref()?.layers.clone()) {
            let mut reference = doc.new_layer("Drawing (reference)", crate::doc::LayerKind::Raster);
            reference.tiles = crate::files::tiles_from_rgba(&page, 0, 0, doc.w, doc.h);
            reference.rev += 1;
            reference.locked = true;
            reference.opacity = 45.0;
            let above = doc.layers.iter().position(|l| l.id == view_id).map(|i| i + 1).unwrap_or(0);
            doc.layers.insert(above, reference);
        }
    }
    doc.mark_all_dirty();
    let rig = std::sync::Arc::make_mut(&mut doc.rig);
    if let Some(sk) = rig.skin.as_mut() {
        sk.tex.view_layer = view_id;
        sk.tex.paint_layer = paint_id;
    }
    Some(paint_id)
}

/// Draw the guide lines into the picture the texture is painted over: where the eyes (blue), the eyebrows (green), the nose (orange)
/// and the mouth (red) of the texture belong, and the nose and the chin of the 3D shape itself (pink). `origin` is where the picture
/// sits on the page; `frame` is (the middle of the figure, its bottom, metres per pixel) to bring the landmarks of the model back to
/// the page.
fn draw_guides(img: &mut RgbaImage, origin: [f32; 2], m0: &super::face::FaceMarks, tex: &super::texedit::TexEdit, landmarks: &[(String, [f32; 3])], frame: (f32, f32, f32)) {
    use super::texedit::TexEdit;
    let m = tex.effective(m0);
    let base = [tex.head[0] + tex.face[0], tex.head[1] + tex.face[1]];
    let brow = |left: bool| {
        let b = TexEdit::brow_at(m0, left);
        let o = if left { tex.brow_l } else { tex.brow_r };
        [b[0] + base[0] + o[0], b[1] + base[1] + o[1]]
    };
    let nose = TexEdit::nose_at(m0);
    let nose = [nose[0] + base[0] + tex.nose[0], nose[1] + base[1] + tex.nose[1]];
    let cx = m.eye_center()[0];
    let wide = m.eye_distance() * 1.25;
    let mut line = |y: f32, col: [u8; 4], img: &mut RgbaImage| {
        let ly = (y - origin[1]).round() as i32;
        for x in ((cx - wide - origin[0]) as i32)..((cx + wide - origin[0]) as i32) {
            for t in 0..2 {
                let (px, py) = (x, ly + t);
                if px >= 0 && py >= 0 && (px as u32) < img.width() && (py as u32) < img.height() {
                    img.put_pixel(px as u32, py as u32, Rgba(col));
                }
            }
        }
    };
    use image::Rgba;
    line((brow(true)[1] + brow(false)[1]) / 2.0, [40, 200, 80, 255], img);
    line((m.eye_l[1] + m.eye_r[1]) / 2.0, [0, 150, 255, 255], img);
    line(nose[1], [255, 150, 0, 255], img);
    line(m.mouth[1], [255, 40, 40, 255], img);
    // the sides of the eyes and the middle of the mouth, as ticks
    for (p, col) in [(m.eye_l, [0, 150, 255, 255]), (m.eye_r, [0, 150, 255, 255]), (m.mouth, [255, 40, 40, 255]), (brow(true), [40, 200, 80, 255]), (brow(false), [40, 200, 80, 255]), (nose, [255, 150, 0, 255])] {
        for dy in -10..=10 {
            let (px, py) = ((p[0] - origin[0]).round() as i32, (p[1] - origin[1]).round() as i32 + dy);
            if px >= 0 && py >= 0 && (px as u32) < img.width() && (py as u32) < img.height() {
                img.put_pixel(px as u32, py as u32, Rgba(col));
            }
        }
    }
    // the nose and the chin of the shape itself
    for (name, p) in landmarks {
        if name == "mesh_nose" || name == "mesh_chin" {
            line(frame.1 - p[1] / frame.2, [255, 0, 200, 255], img);
        }
    }
    let _ = frame.0;
}

#[cfg(test)]
mod smooth_tests {
    use super::*;

    #[test]
    fn smoothing_blends_the_weights_at_a_seam_and_leaves_the_far_ends() {
        // a strip of points in a row: the first half follows bone 0, the second half bone 1
        let n = 40usize;
        let tris: Vec<[u32; 3]> = (0..n as u32 - 2).map(|i| [i, i + 1, i + 2]).collect();
        let w: Vec<Vec<(usize, f32)>> = (0..n).map(|i| vec![(if i < n / 2 { 0 } else { 1 }, 1.0)]).collect();
        let s = smooth_weights(&tris, w, 6);
        let share = |i: usize, b: usize| s[i].iter().find(|a| a.0 == b).map(|a| a.1).unwrap_or(0.0);
        assert_eq!(share(0, 0), 1.0, "the far end is untouched");
        assert_eq!(share(n - 1, 1), 1.0);
        assert!(share(n / 2 - 1, 1) > 0.15 && share(n / 2, 0) > 0.15, "the seam is blended");
        assert!(s.iter().all(|r| (r.iter().map(|a| a.1).sum::<f32>() - 1.0).abs() < 1e-4 && r.len() <= 4));
    }
}

#[cfg(test)]
mod split_tests {
    use super::*;

    #[test]
    fn triangles_between_two_limbs_are_cut_but_a_limb_may_join_the_body() {
        let bone = |name: &str| Bone { name: name.into(), humanoid: true, parent: None, pos: [0.0; 3], tip: [0.0; 3] };
        let bones = vec![bone("hips"), bone("leftUpperLeg"), bone("rightUpperLeg"), bone("leftUpperArm")];
        let mut part = Part3d {
            name: "Body".into(),
            layer: 0,
            verts: vec![[0.0; 3]; 6],
            normals: vec![[0.0, 0.0, 1.0]; 6],
            uvs: vec![[0.0; 2]; 6],
            // 0 body, 1 left leg, 2 right leg, 3 left arm, 4 body, 5 left leg
            tris: vec![[0, 1, 4], [1, 2, 0], [1, 5, 4], [3, 1, 0], [1, 5, 1]],
            joints: vec![[0, 0, 0, 0], [1, 0, 0, 0], [2, 0, 0, 0], [3, 0, 0, 0], [0, 0, 0, 0], [1, 0, 0, 0]],
            weights: vec![[1.0, 0.0, 0.0, 0.0]; 6],
            image: image::RgbaImage::new(2, 2),
            morphs: Vec::new(),
        };
        let cut = split_fused_limbs(&mut part, &bones);
        // [1,2,0] joins the legs and [3,1,0] joins an arm and a leg: both are separated; a body with a leg, and a leg with itself, stay
        assert_eq!(cut, 2);
        assert_eq!(part.tris[0], [0, 1, 4]);
        assert_eq!(part.tris[2], [1, 5, 4]);
        for t in [part.tris[1], part.tris[3]] {
            let limbs: Vec<usize> = t.iter().map(|i| limb_group(&bones[part.joints[*i as usize][0] as usize].name)).filter(|g| *g != 0).collect();
            assert!(limbs.windows(2).all(|w| w[0] == w[1]), "a triangle follows one limb only: {limbs:?}");
        }
        // nothing is lost: the copies are in the same places as the points they copy
        assert_eq!(part.verts.len(), 6 + 2 * 1 + 0, "{}", part.verts.len());
    }
}
