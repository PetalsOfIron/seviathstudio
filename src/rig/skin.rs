// SPDX-License-Identifier: GPL-3.0-or-later
// Uncut rigging: the whole drawing (or the layers you pick) stays in one piece, covered by one mesh. Bones
// (Rotation changers with a tail) move it, and weights say how much each part of the picture follows each bone.
// The weights are painted on the page itself (one grey plane per bone, at a coarser grid), so a part of the
// drawing keeps its weights when you redraw it. Every point goes where the weighted mix of its bones' moves says.

use serde::{Deserialize, Serialize};

use super::{Id, Pose, Rig};
use crate::doc::LayerId;

/// One bone: a Rotation changer (its pivot is the head of the bone) and where the tail is at rest.
#[derive(Clone, PartialEq, Debug, Serialize, Deserialize)]
pub struct SkinBone {
    pub changer: Id,
    pub tail: [f32; 2],
}

#[derive(Clone, PartialEq, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Skin {
    /// The layers that are drawn through this mesh.
    pub layers: Vec<LayerId>,
    pub verts: Vec<[f32; 2]>,
    pub tris: Vec<[u32; 3]>,
    pub bones: Vec<SkinBone>,
    /// Page pixels per weight cell, and the size of the weight grid.
    pub cell: u32,
    pub gw: u32,
    pub gh: u32,
    /// One plane per bone (same order as `bones`), 0..255, row by row.
    #[serde(with = "planes_text")]
    pub planes: Vec<Vec<u8>>,
    /// Where the eyes and the mouth are, when the user has set them (else they are guessed from the head).
    pub face: Option<crate::vrm::face::FaceMarks>,
}

/// Weight planes are written as small grey PNG pictures (as text), so empty or smooth planes cost almost nothing.
mod planes_text {
    use serde::{Deserialize, Deserializer, Serialize, Serializer};

    /// The plane as a grey PNG. The size is not known here, so it is stored as one row (the length is kept).
    pub fn to_png(plane: &[u8]) -> Vec<u8> {
        let mut out = Vec::new();
        let w = plane.len().clamp(1, 1 << 15) as u32;
        let h = plane.len().div_ceil(w as usize).max(1) as u32;
        let mut data = plane.to_vec();
        data.resize((w * h) as usize, 0);
        let mut enc = png::Encoder::new(&mut out, w, h);
        enc.set_color(png::ColorType::Grayscale);
        enc.set_depth(png::BitDepth::Eight);
        enc.set_compression(png::Compression::Balanced);
        if let Ok(mut wr) = enc.write_header() {
            let _ = wr.write_image_data(&data);
        }
        // the true length goes in front (the picture is padded to a full rectangle)
        let mut v = (plane.len() as u32).to_le_bytes().to_vec();
        v.extend(out);
        v
    }

    pub fn from_png(bytes: &[u8], limit: usize) -> Option<Vec<u8>> {
        if bytes.len() < 5 {
            return None;
        }
        let len = u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]) as usize;
        if len > limit {
            return None;
        }
        let mut dec = png::Decoder::new(std::io::Cursor::new(&bytes[4..]));
        dec.set_limits(png::Limits { bytes: limit + (1 << 16) });
        let mut r = dec.read_info().ok()?;
        let mut buf = vec![0u8; r.output_buffer_size()?];
        let info = r.next_frame(&mut buf).ok()?;
        buf.truncate(info.buffer_size());
        if buf.len() < len {
            return None;
        }
        buf.truncate(len);
        Some(buf)
    }

    pub fn serialize<S: Serializer>(planes: &[Vec<u8>], s: S) -> Result<S::Ok, S::Error> {
        planes.iter().map(|p| crate::brush::tipimage::base64_encode(&to_png(p))).collect::<Vec<String>>().serialize(s)
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<Vec<Vec<u8>>, D::Error> {
        let texts = Vec::<String>::deserialize(d)?;
        Ok(texts.iter().filter_map(|t| crate::brush::tipimage::base64_decode(t).and_then(|b| from_png(&b, 16 << 20))).collect())
    }
}

/// The most weight cells on a side (a big page uses bigger cells).
pub const MAX_GRID: u32 = 1024;
pub const MAX_BONES: usize = 64;

impl Skin {
    pub fn new(layers: Vec<LayerId>, verts: Vec<[f32; 2]>, tris: Vec<[u32; 3]>, doc_w: u32, doc_h: u32) -> Skin {
        let cell = (doc_w.max(doc_h)).div_ceil(MAX_GRID).max(1);
        Skin { layers, verts, tris, bones: Vec::new(), cell, gw: doc_w.div_ceil(cell), gh: doc_h.div_ceil(cell), planes: Vec::new(), face: None }
    }

    pub fn add_bone(&mut self, changer: Id, tail: [f32; 2]) {
        if self.bones.len() >= MAX_BONES {
            return;
        }
        self.bones.push(SkinBone { changer, tail });
        self.planes.push(vec![0u8; (self.gw * self.gh) as usize]);
    }

    pub fn remove_bone(&mut self, changer: Id) {
        if let Some(i) = self.bones.iter().position(|b| b.changer == changer) {
            self.bones.remove(i);
            if i < self.planes.len() {
                self.planes.remove(i);
            }
        }
    }

    pub fn bone_index(&self, changer: Id) -> Option<usize> {
        self.bones.iter().position(|b| b.changer == changer)
    }

    /// The weight of a bone at a page position, 0..1 (smooth between cells).
    pub fn weight_at(&self, bone: usize, x: f32, y: f32) -> f32 {
        let Some(plane) = self.planes.get(bone) else { return 0.0 };
        let c = self.cell as f32;
        let (fx, fy) = (x / c - 0.5, y / c - 0.5);
        let (ix, iy) = (fx.floor(), fy.floor());
        let (tx, ty) = (fx - ix, fy - iy);
        let (ix, iy) = (ix as i32, iy as i32);
        let at = |gx: i32, gy: i32| -> f32 {
            let (gx, gy) = (gx.clamp(0, self.gw as i32 - 1), gy.clamp(0, self.gh as i32 - 1));
            plane.get((gy as u32 * self.gw + gx as u32) as usize).copied().unwrap_or(0) as f32 / 255.0
        };
        (at(ix, iy) * (1.0 - tx) + at(ix + 1, iy) * tx) * (1.0 - ty) + (at(ix, iy + 1) * (1.0 - tx) + at(ix + 1, iy + 1) * tx) * ty
    }

    /// The weights of every bone at a point, scaled so they never add up to more than 1.
    pub fn weights_at(&self, x: f32, y: f32) -> Vec<f32> {
        let mut w: Vec<f32> = (0..self.bones.len()).map(|b| self.weight_at(b, x, y)).collect();
        let sum: f32 = w.iter().sum();
        if sum > 1.0 {
            for v in &mut w {
                *v /= sum;
            }
        }
        w
    }

    /// Where every mesh vertex goes for a pose: the weighted mix of what each bone would do to it; the
    /// weight that no bone claims keeps the vertex where it is.
    pub fn skinned(&self, rig: &Rig, pose: &Pose) -> Vec<[f32; 2]> {
        self.verts
            .iter()
            .map(|v| {
                let w = self.weights_at(v[0], v[1]);
                let rest = 1.0 - w.iter().sum::<f32>();
                let mut p = [v[0] * rest, v[1] * rest];
                for (b, wb) in self.bones.iter().zip(&w) {
                    if *wb > 0.0 {
                        let q = rig.deform_point(pose, Some(b.changer), *v);
                        p[0] += q[0] * wb;
                        p[1] += q[1] * wb;
                    }
                }
                p
            })
            .collect()
    }

    /// Automatic weights from where the bones are: close to a bone it follows that bone fully,
    /// and the pull fades out with distance (about 70% of the bone's length, at least 40 px).
    pub fn auto_weights(&mut self, rig: &Rig) {
        let n = self.bones.len();
        let heads: Vec<[f32; 2]> = self.bones.iter().map(|b| match rig.changer(b.changer).map(|c| &c.kind) {
            Some(super::Kind::Rotation { pivot }) => *pivot,
            _ => b.tail,
        }).collect();
        let reach: Vec<f32> = (0..n).map(|i| (((self.bones[i].tail[0] - heads[i][0]).hypot(self.bones[i].tail[1] - heads[i][1])) * 0.7).max(40.0)).collect();
        let c = self.cell as f32;
        for gy in 0..self.gh {
            for gx in 0..self.gw {
                let (x, y) = ((gx as f32 + 0.5) * c, (gy as f32 + 0.5) * c);
                let mut s: Vec<f32> = Vec::with_capacity(n);
                for i in 0..n {
                    let d = dist_to_segment([x, y], heads[i], self.bones[i].tail);
                    let k = (1.0 - d / reach[i]).max(0.0);
                    s.push(k * k);
                }
                let sum: f32 = s.iter().sum();
                let norm = if sum > 1.0 { sum } else { 1.0 };
                for i in 0..n {
                    self.planes[i][(gy * self.gw + gx) as usize] = ((s[i] / norm) * 255.0).round() as u8;
                }
            }
        }
    }

    /// Files come from outside: keep every size consistent.
    pub fn sanitized(mut self) -> Option<Skin> {
        self.cell = self.cell.clamp(1, 4096);
        self.gw = self.gw.clamp(1, MAX_GRID + 1);
        self.gh = self.gh.clamp(1, MAX_GRID + 1);
        self.bones.truncate(MAX_BONES);
        self.verts.truncate(200_000);
        let nv = self.verts.len() as u32;
        self.tris.retain(|t| t.iter().all(|i| *i < nv));
        self.tris.truncate(400_000);
        self.layers.truncate(2000);
        if self.verts.iter().any(|v| !v[0].is_finite() || !v[1].is_finite()) || self.tris.is_empty() {
            return None;
        }
        let want = (self.gw * self.gh) as usize;
        self.planes.truncate(self.bones.len());
        for p in &mut self.planes {
            p.resize(want, 0);
        }
        while self.planes.len() < self.bones.len() {
            self.planes.push(vec![0; want]);
        }
        Some(self)
    }
}

/// What the weight brush does.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Brush {
    /// Give the bone more weight.
    Add,
    /// Take weight away.
    Subtract,
    /// Even out the weight with what is around it.
    Smooth,
    /// Set the weight to a value.
    Flood,
}

#[derive(Clone, Copy, Debug)]
pub struct BrushOpts {
    /// Page pixels.
    pub radius: f32,
    /// 0..1: how much each dab changes (for Flood: the weight it sets).
    pub strength: f32,
    /// 0 = soft all the way out, 1 = full strength up to the edge.
    pub hardness: f32,
    /// Weight given to this bone is taken from the others, so a point never follows more than 100%.
    pub keep_normalized: bool,
}

impl Default for BrushOpts {
    fn default() -> Self {
        Self { radius: 120.0, strength: 0.5, hardness: 0.3, keep_normalized: true }
    }
}

fn falloff(d: f32, radius: f32, hardness: f32) -> f32 {
    let t = d / radius.max(1e-3);
    if t >= 1.0 {
        0.0
    } else if t <= hardness {
        1.0
    } else {
        let k = 1.0 - (t - hardness) / (1.0 - hardness).max(1e-3);
        k * k * (3.0 - 2.0 * k)
    }
}

impl Skin {
    /// Scale the other bones down where this bone's weight would push the total over 1.
    fn keep_total(&mut self, bone: usize, idx: usize) {
        let mine = self.planes[bone][idx] as f32 / 255.0;
        let others: f32 = (0..self.planes.len()).filter(|b| *b != bone).map(|b| self.planes[b][idx] as f32 / 255.0).sum();
        if others + mine > 1.0 && others > 0.0 {
            let k = ((1.0 - mine) / others).max(0.0);
            for b in 0..self.planes.len() {
                if b != bone {
                    self.planes[b][idx] = (self.planes[b][idx] as f32 * k).round() as u8;
                }
            }
        }
    }

    /// One dab of the weight brush at a page position. Returns true when something changed.
    pub fn paint(&mut self, bone: usize, x: f32, y: f32, brush: Brush, o: &BrushOpts) -> bool {
        if bone >= self.planes.len() {
            return false;
        }
        let c = self.cell as f32;
        let (gx0, gx1) = ((((x - o.radius) / c).floor() as i32).max(0), (((x + o.radius) / c).ceil() as i32).min(self.gw as i32 - 1));
        let (gy0, gy1) = ((((y - o.radius) / c).floor() as i32).max(0), (((y + o.radius) / c).ceil() as i32).min(self.gh as i32 - 1));
        let gw = self.gw as i32;
        let before = self.planes[bone].clone();
        let mut touched = Vec::new();
        for gy in gy0..=gy1 {
            for gx in gx0..=gx1 {
                let (cx, cy) = ((gx as f32 + 0.5) * c, (gy as f32 + 0.5) * c);
                let f = falloff((cx - x).hypot(cy - y), o.radius, o.hardness);
                if f <= 0.0 {
                    continue;
                }
                let idx = (gy * gw + gx) as usize;
                let v = before[idx] as f32 / 255.0;
                let nv = match brush {
                    Brush::Add => v + o.strength * f,
                    Brush::Subtract => v - o.strength * f,
                    Brush::Smooth => {
                        let mut sum = 0.0;
                        let mut n = 0.0;
                        for dy in -1..=1 {
                            for dx in -1..=1 {
                                let (ax, ay) = ((gx + dx).clamp(0, gw - 1), (gy + dy).clamp(0, self.gh as i32 - 1));
                                sum += before[(ay * gw + ax) as usize] as f32 / 255.0;
                                n += 1.0;
                            }
                        }
                        v + (sum / n - v) * (o.strength * f).clamp(0.0, 1.0)
                    }
                    Brush::Flood => v + (o.strength - v) * f,
                }
                .clamp(0.0, 1.0);
                self.planes[bone][idx] = (nv * 255.0).round() as u8;
                touched.push(idx);
            }
        }
        if o.keep_normalized && matches!(brush, Brush::Add | Brush::Flood) {
            for idx in &touched {
                self.keep_total(bone, *idx);
            }
        }
        self.planes[bone] != before
    }

    /// Give a bone a whole area: inside the outline the weight is full, fading out over `feather` pixels
    /// inside the edge. With `remove` the weight is taken away instead.
    pub fn lasso(&mut self, bone: usize, poly: &[[f32; 2]], feather: f32, remove: bool) {
        if bone >= self.planes.len() || poly.len() < 3 {
            return;
        }
        let c = self.cell as f32;
        let (mut x0, mut y0, mut x1, mut y1) = (f32::MAX, f32::MAX, f32::MIN, f32::MIN);
        for p in poly {
            x0 = x0.min(p[0]);
            y0 = y0.min(p[1]);
            x1 = x1.max(p[0]);
            y1 = y1.max(p[1]);
        }
        let (gx0, gx1) = (((x0 / c).floor() as i32).max(0), ((x1 / c).ceil() as i32).min(self.gw as i32 - 1));
        let (gy0, gy1) = (((y0 / c).floor() as i32).max(0), ((y1 / c).ceil() as i32).min(self.gh as i32 - 1));
        for gy in gy0..=gy1 {
            for gx in gx0..=gx1 {
                let (cx, cy) = ((gx as f32 + 0.5) * c, (gy as f32 + 0.5) * c);
                if !point_in_poly([cx, cy], poly) {
                    continue;
                }
                // how far inside the outline: full strength beyond `feather`
                let edge = poly_edge_distance([cx, cy], poly);
                let f = if feather > 0.5 { (edge / feather).clamp(0.0, 1.0) } else { 1.0 };
                let f = f * f * (3.0 - 2.0 * f);
                let idx = (gy as u32 * self.gw + gx as u32) as usize;
                let v = self.planes[bone][idx] as f32 / 255.0;
                let nv = if remove { v * (1.0 - f) } else { v + (1.0 - v) * f };
                self.planes[bone][idx] = (nv * 255.0).round() as u8;
                if !remove {
                    self.keep_total(bone, idx);
                }
            }
        }
    }

    /// Set the whole plane of a bone to one value.
    pub fn flood_all(&mut self, bone: usize, value: f32) {
        if let Some(p) = self.planes.get_mut(bone) {
            let v = (value.clamp(0.0, 1.0) * 255.0).round() as u8;
            p.iter_mut().for_each(|c| *c = v);
        }
        if value > 0.0 && bone < self.planes.len() {
            for i in 0..(self.gw * self.gh) as usize {
                self.keep_total(bone, i);
            }
        }
    }

    /// Copy one bone's weights, mirrored across the vertical line x = `axis_x`, onto another bone.
    pub fn mirror_plane(&mut self, from: usize, to: usize, axis_x: f32) {
        if from >= self.planes.len() || to >= self.planes.len() || from == to {
            return;
        }
        let c = self.cell as f32;
        let mut out = vec![0u8; self.planes[to].len()];
        for gy in 0..self.gh {
            for gx in 0..self.gw {
                let cx = (gx as f32 + 0.5) * c;
                let mx = 2.0 * axis_x - cx;
                let mgx = (mx / c).floor() as i32;
                if mgx >= 0 && mgx < self.gw as i32 {
                    out[(gy * self.gw + gx) as usize] = self.planes[from][(gy * self.gw + mgx as u32) as usize];
                }
            }
        }
        self.planes[to] = out;
        for i in 0..(self.gw * self.gh) as usize {
            self.keep_total(to, i);
        }
    }

    /// Never let a point follow more than 100%: where the total is over, every bone is scaled down together.
    pub fn limit_all(&mut self) {
        for i in 0..(self.gw * self.gh) as usize {
            let sum: f32 = self.planes.iter().map(|p| p[i] as f32).sum();
            if sum > 255.0 {
                let k = 255.0 / sum;
                for p in &mut self.planes {
                    p[i] = (p[i] as f32 * k).round() as u8;
                }
            }
        }
    }

    /// Where any bone has weight, make the weights add up to exactly 100% (nothing is left to stay behind).
    pub fn fill_to_one(&mut self) {
        for i in 0..(self.gw * self.gh) as usize {
            let sum: f32 = self.planes.iter().map(|p| p[i] as f32).sum();
            if sum > 0.0 {
                let k = 255.0 / sum;
                for p in &mut self.planes {
                    p[i] = (p[i] as f32 * k).round().min(255.0) as u8;
                }
            }
        }
    }

    /// Give a bone the weights of a picture: `mask` (0..255 per cell, as big as the grid) is softened by
    /// `blur` cells and becomes the bone's weight, taken from the others.
    pub fn assign_mask(&mut self, bone: usize, mask: &[u8], blur: u32) {
        if bone >= self.planes.len() || mask.len() != (self.gw * self.gh) as usize {
            return;
        }
        let (w, h) = (self.gw as i32, self.gh as i32);
        let r = blur as i32;
        let mut soft = vec![0u8; mask.len()];
        for y in 0..h {
            for x in 0..w {
                let (mut sum, mut n) = (0u32, 0u32);
                for dy in -r..=r {
                    for dx in -r..=r {
                        let (ax, ay) = (x + dx, y + dy);
                        if ax >= 0 && ay >= 0 && ax < w && ay < h {
                            sum += mask[(ay * w + ax) as usize] as u32;
                        }
                        n += 1;
                    }
                }
                soft[(y * w + x) as usize] = (sum / n.max(1)) as u8;
            }
        }
        for (i, v) in soft.iter().enumerate() {
            self.planes[bone][i] = *v;
            self.keep_total(bone, i);
        }
    }
}

fn point_in_poly(p: [f32; 2], poly: &[[f32; 2]]) -> bool {
    let mut inside = false;
    let mut j = poly.len() - 1;
    for i in 0..poly.len() {
        let (a, b) = (poly[i], poly[j]);
        if (a[1] > p[1]) != (b[1] > p[1]) && p[0] < (b[0] - a[0]) * (p[1] - a[1]) / (b[1] - a[1]) + a[0] {
            inside = !inside;
        }
        j = i;
    }
    inside
}

fn poly_edge_distance(p: [f32; 2], poly: &[[f32; 2]]) -> f32 {
    (0..poly.len()).map(|i| dist_to_segment(p, poly[i], poly[(i + 1) % poly.len()])).fold(f32::MAX, f32::min)
}

pub fn dist_to_segment(p: [f32; 2], a: [f32; 2], b: [f32; 2]) -> f32 {
    let (dx, dy) = (b[0] - a[0], b[1] - a[1]);
    let l2 = dx * dx + dy * dy;
    let t = if l2 < 1e-9 { 0.0 } else { (((p[0] - a[0]) * dx + (p[1] - a[1]) * dy) / l2).clamp(0.0, 1.0) };
    (p[0] - (a[0] + dx * t)).hypot(p[1] - (a[1] + dy * t))
}

use super::Kind;

/// How close two ends of bones must be to count as one joint (page pixels).
pub const JOINT_JOIN: f32 = 2.5;

/// The joints of a skeleton: the head of every bone and the tail of every bone, where several meet in one place they are one
/// joint. Each is a place on the page and the names of the bone ends there ("leftLowerArm head", "leftUpperArm tail").
pub fn joints(rig: &Rig) -> Vec<([f32; 2], Vec<String>)> {
    let Some(sk) = rig.skin.as_ref() else { return Vec::new() };
    let mut out: Vec<([f32; 2], Vec<String>)> = Vec::new();
    let mut put = |p: [f32; 2], label: String| match out.iter_mut().find(|j| (j.0[0] - p[0]).hypot(j.0[1] - p[1]) <= JOINT_JOIN) {
        Some(j) => j.1.push(label),
        None => out.push((p, vec![label])),
    };
    for b in &sk.bones {
        let Some(c) = rig.changer(b.changer) else { continue };
        if let Kind::Rotation { pivot } = c.kind {
            put(pivot, format!("{} head", c.name));
        }
        put(b.tail, format!("{} tail", c.name));
    }
    out
}

/// Move the joint at `from` to `to`: every bone head and every bone tail that sits there goes along. The bones that hang
/// from the joint keep their own lengths only by the other end staying where it is, so the bones on both sides change.
pub fn move_joint(rig: &mut Rig, from: [f32; 2], to: [f32; 2]) -> bool {
    let near = |p: [f32; 2]| (p[0] - from[0]).hypot(p[1] - from[1]) <= JOINT_JOIN;
    let ids: Vec<(Id, [f32; 2])> = match rig.skin.as_ref() {
        Some(sk) => sk.bones.iter().map(|b| (b.changer, b.tail)).collect(),
        None => return false,
    };
    let mut moved = false;
    for (id, tail) in &ids {
        if let Some(c) = rig.changers.iter_mut().find(|c| c.id == *id) {
            if let Kind::Rotation { pivot } = c.kind {
                if near(pivot) {
                    c.kind = Kind::Rotation { pivot: to };
                    moved = true;
                }
            }
        }
        if near(*tail) {
            if let Some(b) = rig.skin.as_mut().and_then(|s| s.bones.iter_mut().find(|b| b.changer == *id)) {
                b.tail = to;
                moved = true;
            }
        }
    }
    moved
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rig::Values;

    /// A 400 x 400 page with a column mesh (two triangles) and two bones along it: a spine bone (top half) and an arm bone.
    fn setup() -> (Rig, Skin, Id, Id, Id) {
        let mut rig = Rig::default();
        let a = rig.add_changer("Upper", Kind::Rotation { pivot: [200.0, 100.0] }, None);
        let b = rig.add_changer("Lower", Kind::Rotation { pivot: [200.0, 300.0] }, None);
        let p = rig.add_param("Turn", -90.0, 90.0, 0.0);
        rig.bind_rotation(a, p, 90.0);
        // a grid of vertices over the page so the skinning can be seen
        let mut verts = Vec::new();
        for y in 0..=8 {
            for x in 0..=8 {
                verts.push([x as f32 * 50.0, y as f32 * 50.0]);
            }
        }
        let mut s = Skin::new(vec![1], verts, vec![[0, 1, 9]], 400, 400);
        s.add_bone(a, [200.0, 200.0]);
        s.add_bone(b, [200.0, 400.0]);
        (rig, s, a, b, p)
    }

    #[test]
    fn the_weight_grid_has_the_right_size_and_weights_are_smooth() {
        let (_, mut s, ..) = setup();
        assert_eq!((s.cell, s.gw, s.gh), (1, 400, 400));
        let big = Skin::new(vec![], vec![[0.0, 0.0]], vec![], 4000, 3000);
        assert_eq!((big.cell, big.gw, big.gh), (4, 1000, 750));
        // paint one cell full: the weight at its centre is 1 and fades toward the next cell
        s.planes[0][(10 * 400 + 10) as usize] = 255;
        assert!((s.weight_at(0, 10.5, 10.5) - 1.0).abs() < 1e-4);
        assert!(s.weight_at(0, 11.0, 10.5) > 0.4 && s.weight_at(0, 11.0, 10.5) < 0.6);
        assert_eq!(s.weight_at(0, 100.5, 100.5), 0.0);
        assert_eq!(s.weight_at(5, 10.5, 10.5), 0.0, "a bone that doesn't exist has no weight");
    }

    #[test]
    fn a_point_follows_the_weighted_mix_of_its_bones() {
        let (rig, mut s, a, _, p) = setup();
        // everything fully weighted to the upper bone
        s.planes[0].iter_mut().for_each(|v| *v = 255);
        let mut vals = Values::default();
        vals.set(p, 90.0);
        let pose = rig.pose(&vals);
        let out = s.skinned(&rig, &pose);
        // the vertex at (300,100) is 100 right of the pivot (200,100): turned 90 degrees it is 100 below
        let i = 2 * 9 + 6;
        assert_eq!(s.verts[i], [300.0, 100.0]);
        assert!((out[i][0] - 200.0).abs() < 0.01 && (out[i][1] - 200.0).abs() < 0.01, "{:?}", out[i]);
        // half weight: half way between where it was and where the bone sends it
        s.planes[0].iter_mut().for_each(|v| *v = 128);
        let half = s.skinned(&rig, &pose);
        let k = 128.0 / 255.0;
        assert!((half[i][0] - (300.0 + (200.0 - 300.0) * k)).abs() < 0.1, "{:?}", half[i]);
        // no weight: nothing moves
        s.planes[0].iter_mut().for_each(|v| *v = 0);
        assert_eq!(s.skinned(&rig, &pose)[i], s.verts[i]);
        let _ = a;
    }

    #[test]
    fn weights_that_add_up_to_more_than_one_are_scaled_down() {
        let (rig, mut s, a, b, p) = setup();
        s.planes[0].iter_mut().for_each(|v| *v = 255);
        s.planes[1].iter_mut().for_each(|v| *v = 255);
        let w = s.weights_at(100.0, 100.0);
        assert!((w.iter().sum::<f32>() - 1.0).abs() < 1e-4 && (w[0] - 0.5).abs() < 1e-4);
        let mut vals = Values::default();
        vals.set(p, 90.0);
        // the lower bone has no slider, so only the upper one moves: half way
        let out = s.skinned(&rig, &rig.pose(&vals));
        let direct = rig.deform_point(&rig.pose(&vals), Some(a), s.verts[20]);
        assert!((out[20][0] - (s.verts[20][0] + direct[0]) / 2.0).abs() < 0.1);
        let _ = b;
    }

    #[test]
    fn a_child_bone_goes_with_its_parent() {
        let (mut rig, mut s, a, _, p) = setup();
        let child = rig.add_changer("Forearm", Kind::Rotation { pivot: [200.0, 200.0] }, Some(a));
        s.add_bone(child, [200.0, 280.0]);
        // all weight on the forearm; the upper bone turns 90 degrees about (200,100)
        let ci = s.bone_index(child).unwrap();
        s.planes[ci].iter_mut().for_each(|v| *v = 255);
        let mut vals = Values::default();
        vals.set(p, 90.0);
        let out = s.skinned(&rig, &rig.pose(&vals));
        let i = 4 * 9 + 4; // (200,200), the forearm's own pivot
        assert_eq!(s.verts[i], [200.0, 200.0]);
        // the elbow is 100 below the shoulder pivot: a quarter turn puts it 100 to the left
        assert!((out[i][0] - 100.0).abs() < 0.01 && (out[i][1] - 100.0).abs() < 0.01, "{:?}", out[i]);
    }

    #[test]
    fn automatic_weights_follow_the_bones_and_fade_with_distance() {
        let (rig, mut s, ..) = setup();
        s.auto_weights(&rig);
        // on the upper bone (its head is at 200,100; tail at 200,200): fully its own
        let w = s.weights_at(200.0, 150.0);
        assert!(w[0] > 0.95 && w[1] < 0.05, "{w:?}");
        // on the lower bone
        let w = s.weights_at(200.0, 350.0);
        assert!(w[1] > 0.95 && w[0] < 0.05, "{w:?}");
        // far from both bones nothing follows
        let far = s.weights_at(390.0, 5.0);
        assert!(far.iter().sum::<f32>() < 0.05, "{far:?}");
        // between them it is a mix, and never more than everything
        let mid = s.weights_at(200.0, 250.0);
        assert!(mid.iter().sum::<f32>() <= 1.0001 && mid[0] > 0.05 && mid[1] > 0.05, "{mid:?}");
        // off to the side it fades
        let side_near = s.weights_at(220.0, 150.0)[0];
        let side_far = s.weights_at(260.0, 150.0)[0];
        assert!(side_near > side_far && side_far >= 0.0);
    }

    #[test]
    fn a_skin_survives_json_with_its_weights_and_odd_files_are_cleaned() {
        let (rig, mut s, ..) = setup();
        s.auto_weights(&rig);
        let text = serde_json::to_string(&s).unwrap();
        assert!(text.len() < 150_000, "weights are written as small pictures: {} bytes", text.len());
        let back: Skin = serde_json::from_str(&text).unwrap();
        assert_eq!(back, s);
        // a plane of the wrong size is fixed, a missing plane is added, a bad triangle is dropped
        let mut bad = s.clone();
        bad.planes[0].truncate(10);
        bad.planes.pop();
        bad.tris.push([0, 1, 9999]);
        let ok = bad.sanitized().unwrap();
        assert_eq!(ok.planes.len(), ok.bones.len());
        assert!(ok.planes.iter().all(|p| p.len() == (ok.gw * ok.gh) as usize));
        assert_eq!(ok.tris.len(), 1);
        // nothing left to draw: no skin
        let mut empty = s.clone();
        empty.tris.clear();
        assert!(empty.sanitized().is_none());
        // removing a bone removes its plane
        let mut r = s.clone();
        let c = r.bones[0].changer;
        r.remove_bone(c);
        assert_eq!((r.bones.len(), r.planes.len()), (1, 1));
        let _ = rig;
    }

    fn blank() -> Skin {
        let mut s = Skin::new(vec![1], vec![[0.0, 0.0], [10.0, 0.0], [0.0, 10.0]], vec![[0, 1, 2]], 200, 100);
        s.add_bone(1, [0.0, 0.0]);
        s.add_bone(2, [0.0, 0.0]);
        s
    }

    #[test]
    fn the_weight_brush_adds_subtracts_smooths_and_floods() {
        let mut s = blank();
        let o = BrushOpts { radius: 20.0, strength: 0.5, hardness: 1.0, keep_normalized: true };
        // add: half weight in the middle of the dab, nothing outside it
        assert!(s.paint(0, 100.0, 50.0, Brush::Add, &o));
        assert!((s.weight_at(0, 100.5, 50.5) - 0.5).abs() < 0.02);
        assert_eq!(s.weight_at(0, 100.5, 85.5), 0.0, "nothing outside the radius");
        // again: it adds up, and stops at full
        for _ in 0..4 {
            s.paint(0, 100.0, 50.0, Brush::Add, &o);
        }
        assert!((s.weight_at(0, 100.5, 50.5) - 1.0).abs() < 0.01);
        // subtract
        s.paint(0, 100.0, 50.0, Brush::Subtract, &o);
        assert!((s.weight_at(0, 100.5, 50.5) - 0.5).abs() < 0.02);
        // smooth evens out a spike: a single full cell next to empty ones
        let mut sp = blank();
        sp.planes[0][(50 * 200 + 100) as usize] = 255;
        let before = sp.planes[0][(50 * 200 + 100) as usize];
        sp.paint(0, 100.0, 50.0, Brush::Smooth, &BrushOpts { radius: 10.0, strength: 1.0, hardness: 1.0, keep_normalized: false });
        assert!(sp.planes[0][(50 * 200 + 100) as usize] < before, "the spike is lower");
        assert!(sp.planes[0][(50 * 200 + 101) as usize] > 0, "and its neighbours got some");
        // flood sets a value
        let mut f = blank();
        f.paint(0, 100.0, 50.0, Brush::Flood, &BrushOpts { radius: 30.0, strength: 0.8, hardness: 1.0, keep_normalized: true });
        assert!((f.weight_at(0, 100.5, 50.5) - 0.8).abs() < 0.02);
        // a bone that does not exist or a dab off the page does nothing
        assert!(!f.paint(9, 100.0, 50.0, Brush::Add, &o));
        assert!(!f.paint(0, -500.0, -500.0, Brush::Add, &o));
    }

    #[test]
    fn adding_weight_to_one_bone_takes_it_from_the_others() {
        let mut s = blank();
        s.planes[1].iter_mut().for_each(|v| *v = 255);
        let o = BrushOpts { radius: 15.0, strength: 0.7, hardness: 1.0, keep_normalized: true };
        s.paint(0, 100.0, 50.0, Brush::Add, &o);
        let i = (50 * 200 + 100) as usize;
        let (a, b) = (s.planes[0][i] as f32 / 255.0, s.planes[1][i] as f32 / 255.0);
        assert!((a + b - 1.0).abs() < 0.02, "{a} + {b}");
        assert!(a > 0.6 && b < 0.4);
        // without it the total can go over (and is then scaled when used)
        let mut t = blank();
        t.planes[1].iter_mut().for_each(|v| *v = 255);
        t.paint(0, 100.0, 50.0, Brush::Add, &BrushOpts { keep_normalized: false, ..o });
        assert_eq!(t.planes[1][i], 255);
        t.limit_all();
        let sum = t.planes[0][i] as u32 + t.planes[1][i] as u32;
        assert!((253..=257).contains(&sum), "limited to 100%: {sum}");
    }

    #[test]
    fn a_lasso_gives_an_area_to_a_bone_with_a_soft_edge() {
        let mut s = blank();
        let square = [[40.0, 20.0], [160.0, 20.0], [160.0, 80.0], [40.0, 80.0]];
        s.lasso(0, &square, 20.0, false);
        assert!(s.weight_at(0, 100.5, 50.5) > 0.99, "full inside");
        assert!(s.weight_at(0, 42.5, 50.5) < 0.3, "soft at the edge");
        assert_eq!(s.weight_at(0, 10.5, 50.5), 0.0, "nothing outside");
        // another bone lassoed over the same place takes it over
        s.lasso(1, &square, 0.0, false);
        assert!(s.weight_at(1, 100.5, 50.5) > 0.99 && s.weight_at(0, 100.5, 50.5) < 0.01);
        // and a lasso can take weight away
        s.lasso(1, &[[80.0, 40.0], [120.0, 40.0], [120.0, 60.0], [80.0, 60.0]], 0.0, true);
        assert!(s.weight_at(1, 100.5, 50.5) < 0.01 && s.weight_at(1, 60.5, 50.5) > 0.99);
        // a lasso with fewer than three points does nothing
        let before = s.planes.clone();
        s.lasso(0, &[[0.0, 0.0], [10.0, 10.0]], 0.0, false);
        assert_eq!(s.planes, before);
    }

    #[test]
    fn weights_are_mirrored_across_a_line_and_flooded_and_filled() {
        let mut s = blank();
        s.lasso(0, &[[20.0, 20.0], [60.0, 20.0], [60.0, 80.0], [20.0, 80.0]], 0.0, false);
        s.mirror_plane(0, 1, 100.0);
        assert!(s.weight_at(1, 160.5, 50.5) > 0.99, "the left arm's weights are on the right now");
        assert!(s.weight_at(1, 40.5, 50.5) < 0.01);
        // flood the whole plane
        s.flood_all(0, 0.25);
        assert!((s.weight_at(0, 100.5, 50.5) - 0.25).abs() < 0.01);
        // fill to one: where something has weight, it adds up to everything
        s.fill_to_one();
        let sum: f32 = (0..2).map(|b| s.weight_at(b, 160.5, 50.5)).sum();
        assert!((sum - 1.0).abs() < 0.02, "{sum}");
    }

    #[test]
    fn a_mask_becomes_a_soft_weight() {
        let mut s = blank();
        let mut mask = vec![0u8; (s.gw * s.gh) as usize];
        for y in 30..70 {
            for x in 60..140 {
                mask[(y * s.gw + x) as usize] = 255;
            }
        }
        s.assign_mask(0, &mask, 3);
        assert!(s.weight_at(0, 100.5, 50.5) > 0.99);
        let edge = s.weight_at(0, 60.5, 50.5);
        assert!(edge > 0.2 && edge < 0.8, "soft edge: {edge}");
        assert!(s.weight_at(0, 150.5, 50.5) == 0.0);
        // a mask of the wrong size is refused
        let before = s.planes.clone();
        s.assign_mask(0, &[1, 2, 3], 1);
        assert_eq!(s.planes, before);
    }
}
