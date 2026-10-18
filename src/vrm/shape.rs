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
    let (w, h) = (img.width() as i32, img.height() as i32);
    let mut open: Vec<bool> = img.pixels().map(|p| p.0[3] < 128).collect();
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
    for p in img.pixels_mut() {
        p.0[3] = 255;
    }
}

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
    let mut tris = shape.tris.clone();
    // finer around the eyes and the mouth, so they can close and open
    let marks = if opt.face { face_marks(rig) } else { None };
    if let Some(m) = &marks {
        let (c, dist, fine) = (m.eye_center(), m.eye_distance(), (m.eye_w / 4.5).max(1.5));
        let m = *m;
        let inside = move |p: [f32; 2]| (p[0] - c[0]).abs() < dist * 0.9 + m.eye_w && p[1] > c[1] - 1.5 * m.eye_w && p[1] < m.mouth[1] + 0.9 * m.mouth_w;
        let (p2, t2) = crate::rig::refine::subdivide_xyz(&pts, &tris, &move |p| if inside(p) { fine } else { f32::MAX }, 600_000);
        pts = p2;
        tris = t2;
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
    let (cx0, cy0) = ((vb[0].floor() as i64 - 2).max(0) as u32, (vb[1].floor() as i64 - 2).max(0) as u32);
    let (cx1, cy1) = (((vb[2].ceil() as u32) + 2).min(page.width()), ((vb[3].ceil() as u32) + 2).min(page.height()));
    if cx1 <= cx0 + 2 || cy1 <= cy0 + 2 {
        return None;
    }
    let crop = image::imageops::crop_imm(&page, cx0, cy0, cx1 - cx0, cy1 - cy0).to_image();
    let (tw, th) = (crop.width() as f32, crop.height() as f32);
    let big = crop.width().max(crop.height());
    let mut front = if big > opt.max_texture {
        let k = opt.max_texture as f32 / big as f32;
        image::imageops::resize(&crop, ((tw * k).round() as u32).max(1), ((th * k).round() as u32).max(1), image::imageops::FilterType::Triangle)
    } else {
        crop
    };
    let mask: Vec<bool> = front.pixels().map(|p| p.0[3] >= 128).collect();
    spread_colour(&mut front);
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
    for t in &tris {
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
                let sh = if neck { [head_x - pts[t[k] as usize][0], 0.0] } else { shift };
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
    let verts: Vec<[f32; 3]> = made.iter().map(|(i, _, _)| model_pts[*i as usize]).collect();
    let uvs: Vec<[f32; 2]> = made
        .iter()
        .map(|(i, back, side)| {
            let p = pts[*i as usize];
            let sh = side.unwrap_or([0.0, 0.0]);
            let (u, v) = (((p[0] + sh[0] - cx0 as f32) / tw).clamp(0.0, 1.0), ((p[1] + sh[1] - cy0 as f32) / th).clamp(0.0, 1.0));
            match (side.is_some(), *back) {
                (false, false) => [u * fwf / awf, v * fhf / ahf],
                (false, true) => [(fwf + (1.0 - u) * fwf) / awf, v * fhf / ahf],
                (true, false) => [u * swf / awf, (fhf + v * shf) / ahf],
                (true, true) => [(swf + (1.0 - u) * swf) / awf, (fhf + v * shf) / ahf],
            }
        })
        .collect();
    let normals = normals_of(&verts, &out_tris);
    // skin weights: what is painted for the skeleton at the place of the point on the picture
    let ix_of: Vec<Option<usize>> = sk.bones.iter().map(|b| rig.changer(b.changer).and_then(|c| slot.get(c.name.as_str()).copied())).collect();
    // The painted weights belong to the drawing; an AI shape is never quite the drawing (an arm a little higher, a leg a little
    // wider), and where it differs the painted weights would say "body" for a point that is really on the arm. So the weights
    // are also worked out from the distance to the bones, and where the two do not agree on the limb the distance wins.
    let weights_of: Vec<Vec<(usize, f32)>> = (0..model_pts.len())
        .map(|i| {
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
    let part = Part3d { name: "Body".to_string(), layer: sk.layers.first().copied().unwrap_or(0), verts, normals, uvs, tris: out_tris, joints, weights, image: atlas, morphs: Vec::new() };
    let bones: Vec<Bone> = bones;
    let mut model = Model3d { name: opt.name.clone(), author: opt.author.clone(), parts: vec![part], bones, expressions: Vec::new(), springs: Vec::new(), colliders: Vec::new(), scale };
    if let Some(m) = &marks {
        add_face(&mut model, m, xc, vb[3], opt.template.as_deref(), true);
    }
    normalize_to_t_pose(&mut model);
    Some(model)
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
