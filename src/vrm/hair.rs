// SPDX-License-Identifier: GPL-3.0-or-later
// Hair that moves, for a character built on an AI shape. The hair is found on the drawing (grown from the top of the head by
// colour), made a part of its own that sits a little outside the head, and hung on chains of bones that a game or a VRM app
// swings with its spring bones. The head under it keeps the hair colour, so nothing opens up when the hair swings.

use image::RgbaImage;

use super::model::{Collider, Model3d, Part3d, Spring};

pub struct HairMask {
    x0: i32,
    y0: i32,
    w: i32,
    h: i32,
    bits: Vec<bool>,
}

impl HairMask {
    pub fn at(&self, x: f32, y: f32) -> bool {
        let (ix, iy) = (x.round() as i32 - self.x0, y.round() as i32 - self.y0);
        ix >= 0 && iy >= 0 && ix < self.w && iy < self.h && self.bits[(iy * self.w + ix) as usize]
    }

    pub fn count(&self) -> usize {
        self.bits.iter().filter(|b| **b).count()
    }
}

/// The hair on the picture: the pixels joined to the top of the head that have the colour of the hair there. `head` is the box of
/// the head (x, y, width, height) from `head_box`.
pub fn hair_mask(page: &RgbaImage, head: [f32; 4]) -> Option<HairMask> {
    let hh = head[2] / 2.0;
    let (cx, top) = (head[0] + hh, head[1] + 0.1 * hh);
    let (x0, y0) = (((cx - 1.9 * hh) as i32).max(0), ((top - 0.2 * hh) as i32).max(0));
    let (x1, y1) = (((cx + 1.9 * hh) as i32).min(page.width() as i32), ((top + 6.0 * hh) as i32).min(page.height() as i32));
    let (w, h) = (x1 - x0, y1 - y0);
    if w < 8 || h < 8 {
        return None;
    }
    let px = |x: i32, y: i32| page.get_pixel((x0 + x) as u32, (y0 + y) as u32).0;
    // the colour of the hair: the upper part of the head, in the middle
    let (mut sum, mut n) = ([0u64; 3], 0u64);
    for y in ((top - y0 as f32 + 0.1 * hh) as i32)..((top - y0 as f32 + 0.4 * hh) as i32) {
        for x in ((cx - x0 as f32 - 0.4 * hh) as i32)..((cx - x0 as f32 + 0.4 * hh) as i32) {
            if x >= 0 && y >= 0 && x < w && y < h && px(x, y)[3] >= 128 {
                let p = px(x, y);
                for c in 0..3 {
                    sum[c] += p[c] as u64;
                }
                n += 1;
            }
        }
    }
    if n < 50 {
        return None;
    }
    let mean = [(sum[0] / n) as f32, (sum[1] / n) as f32, (sum[2] / n) as f32];
    let lum = 0.3 * mean[0] + 0.59 * mean[1] + 0.11 * mean[2];
    // dark hair has dark shadows and bright highlights; light hair is close to the colour of skin
    let tol = (90.0 - 0.25 * lum).clamp(45.0, 85.0);
    let close = |p: [u8; 4]| {
        p[3] >= 128 && {
            let d = [p[0] as f32 - mean[0], p[1] as f32 - mean[1], p[2] as f32 - mean[2]];
            (d[0] * d[0] + d[1] * d[1] + d[2] * d[2]).sqrt() < tol
        }
    };
    let mut bits = vec![false; (w * h) as usize];
    let mut stack: Vec<(i32, i32)> = Vec::new();
    for y in ((top - y0 as f32 + 0.1 * hh) as i32)..((top - y0 as f32 + 0.4 * hh) as i32) {
        for x in ((cx - x0 as f32 - 0.4 * hh) as i32)..((cx - x0 as f32 + 0.4 * hh) as i32) {
            if x >= 0 && y >= 0 && x < w && y < h && close(px(x, y)) && !bits[(y * w + x) as usize] {
                bits[(y * w + x) as usize] = true;
                stack.push((x, y));
            }
        }
    }
    while let Some((x, y)) = stack.pop() {
        for (dx, dy) in [(1, 0), (-1, 0), (0, 1), (0, -1)] {
            let (nx, ny) = (x + dx, y + dy);
            if nx >= 0 && ny >= 0 && nx < w && ny < h && !bits[(ny * w + nx) as usize] && close(px(nx, ny)) {
                bits[(ny * w + nx) as usize] = true;
                stack.push((nx, ny));
            }
        }
    }
    let mask = HairMask { x0, y0, w, h, bits };
    // a few pixels are not hair
    (mask.count() as f32 > 0.02 * (hh * hh)).then_some(mask)
}

struct Chain {
    x: f32,
    z: f32,
    top: f32,
    len: f32,
    bones: [usize; 3],
}

const COLUMNS: usize = 6;
const REACH: [f32; 4] = [0.0, 0.33, 0.66, 1.0];

/// Make the hair of a model a part of its own with bone chains. `page_of` gives the place on the picture of a point of the
/// body (page pixels).
pub fn add_hair(model: &mut Model3d, mask: &HairMask, page_of: impl Fn(&[f32; 3]) -> [f32; 2]) -> bool {
    let Some(head) = model.bone("head") else { return false };
    // the points and triangles of the body and the face together (the face carries the blend shapes, the hair covers both)
    let (mut b_verts, mut b_normals, mut b_uvs, mut b_tris): (Vec<[f32; 3]>, Vec<[f32; 3]>, Vec<[f32; 2]>, Vec<[u32; 3]>) = (Vec::new(), Vec::new(), Vec::new(), Vec::new());
    let (b_layer, b_image) = (model.parts[0].layer, model.parts[0].image.clone());
    for p in model.parts.iter().filter(|p| p.name == "Body" || p.name == "Face") {
        let base = b_verts.len() as u32;
        b_verts.extend(p.verts.iter().copied());
        b_normals.extend(p.normals.iter().copied());
        b_uvs.extend(p.uvs.iter().copied());
        b_tris.extend(p.tris.iter().map(|t| [t[0] + base, t[1] + base, t[2] + base]));
    }
    let is_hair: Vec<bool> = b_verts.iter().map(|v| { let p = page_of(v); mask.at(p[0], p[1]) }).collect();
    let tris: Vec<[u32; 3]> = b_tris.iter().copied().filter(|t| t.iter().all(|i| is_hair[*i as usize])).collect();
    if tris.len() < 50 {
        return false;
    }
    // the hair vertices that are used, and the bounds of the hair
    let mut used: Vec<u32> = tris.iter().flatten().copied().collect();
    used.sort_unstable();
    used.dedup();
    let (mut lo, mut hi) = ([f32::MAX; 3], [f32::MIN; 3]);
    for i in &used {
        let v = b_verts[*i as usize];
        for k in 0..3 {
            lo[k] = lo[k].min(v[k]);
            hi[k] = hi[k].max(v[k]);
        }
    }
    let top = hi[1];
    // the chains: a few columns across the hair, in front and behind
    let mut chains: Vec<Chain> = Vec::new();
    let colw = (hi[0] - lo[0]) / COLUMNS as f32;
    if colw < 1e-4 {
        return false;
    }
    for layer in [1.0f32, -1.0] {
        let layer_z: Vec<f32> = used.iter().map(|i| b_verts[*i as usize]).filter(|v| v[2] * layer > 0.0).map(|v| v[2]).collect();
        if layer_z.is_empty() {
            continue;
        }
        let zmean = layer_z.iter().sum::<f32>() / layer_z.len() as f32 * 0.6;
        for c in 0..COLUMNS {
            let xc = lo[0] + (c as f32 + 0.5) * colw;
            let bottom = used
                .iter()
                .map(|i| b_verts[*i as usize])
                .filter(|v| (v[0] - xc).abs() <= colw * 0.5 && v[2] * layer > 0.0)
                .map(|v| v[1])
                .fold(f32::MAX, f32::min);
            if bottom == f32::MAX || top - bottom < 0.05 {
                continue;
            }
            chains.push(Chain { x: xc, z: zmean, top, len: top - bottom, bones: [0; 3] });
        }
    }
    if chains.is_empty() {
        return false;
    }
    // the bones, and the springs that swing them
    let head_pos = model.bones[head].pos;
    for (ci, ch) in chains.iter_mut().enumerate() {
        let mut parent = head;
        let mut idx = Vec::new();
        for k in 1..=3 {
            let pos = [ch.x, ch.top - ch.len * REACH[k], ch.z];
            let next = [ch.x, ch.top - ch.len * REACH.get(k + 1).copied().unwrap_or(REACH[3] + 0.25).min(REACH[3] + 0.25), ch.z];
            model.bones.push(super::model::Bone { name: format!("hair{ci}_{k}"), humanoid: false, parent: Some(parent), pos, tip: next });
            parent = model.bones.len() - 1;
            idx.push(parent);
        }
        ch.bones = [idx[0], idx[1], idx[2]];
        model.springs.push(Spring { name: format!("hair{ci}"), joints: idx.clone(), hit_radius: 0.012, stiffness: 0.55, gravity: 0.08, drag: 0.35 });
    }
    let _ = head_pos;
    // a ball for the head, which the hair does not go through
    let hp = model.bones[head].pos;
    model.colliders.push(Collider { bone: head, offset: [0.0, (top - hp[1]) * 0.5, 0.0], radius: ((top - hp[1]) * 0.55).max(0.07) });
    // the part: the hair triangles, a little outside the head
    let mut remap = std::collections::HashMap::new();
    let (mut verts, mut normals, mut uvs, mut joints, mut weights) = (Vec::new(), Vec::new(), Vec::new(), Vec::new(), Vec::new());
    let mut out_tris: Vec<[u32; 3]> = Vec::with_capacity(tris.len());
    for t in &tris {
        let mut o = [0u32; 3];
        for k in 0..3 {
            let i = t[k];
            o[k] = *remap.entry(i).or_insert_with(|| {
                let (v, n) = (b_verts[i as usize], b_normals[i as usize]);
                let p = [v[0] + n[0] * 0.003, v[1] + n[1] * 0.003, v[2] + n[2] * 0.003];
                // the two nearest chains share the point; along the chain the bones share it by how far down it is
                let mut near: Vec<(usize, f32)> = chains.iter().enumerate().map(|(ci, c)| (ci, (p[0] - c.x).powi(2) + (p[2] - c.z).powi(2))).collect();
                near.sort_by(|a, b| a.1.total_cmp(&b.1));
                near.truncate(2);
                let mut acc: Vec<(usize, f32)> = Vec::new();
                let total: f32 = near.iter().map(|n| 1.0 / (n.1 + 1e-4)).sum();
                for (ci, d2) in near {
                    let c = &chains[ci];
                    let share = 1.0 / (d2 + 1e-4) / total;
                    let f = ((c.top - p[1]) / c.len).clamp(0.0, 1.0);
                    let slot = (0..3).find(|k| f <= REACH[k + 1]).unwrap_or(2);
                    let t = ((f - REACH[slot]) / (REACH[slot + 1] - REACH[slot])).clamp(0.0, 1.0);
                    let lower = c.bones[slot];
                    let upper = if slot == 0 { head } else { c.bones[slot - 1] };
                    // a point between two bones follows the upper one less and the lower one more as it goes down
                    for (b, w) in [(upper, 1.0 - t), (lower, t)] {
                        match acc.iter_mut().find(|a| a.0 == b) {
                            Some(a) => a.1 += w * share,
                            None => acc.push((b, w * share)),
                        }
                    }
                }
                acc.sort_by(|a, b| b.1.total_cmp(&a.1));
                acc.truncate(4);
                let sum: f32 = acc.iter().map(|a| a.1).sum::<f32>().max(1e-9);
                let (mut j, mut w) = ([0u16; 4], [0.0f32; 4]);
                for (k, (b, x)) in acc.iter().enumerate() {
                    j[k] = *b as u16;
                    w[k] = x / sum;
                }
                verts.push(p);
                normals.push(n);
                uvs.push(b_uvs[i as usize]);
                joints.push(j);
                weights.push(w);
                (verts.len() - 1) as u32
            });
        }
        out_tris.push(o);
    }
    let part = Part3d { name: "Hair".to_string(), layer: b_layer, verts, normals, uvs, tris: out_tris, joints, weights, image: b_image, morphs: Vec::new() };
    model.parts.push(part);
    true
}
