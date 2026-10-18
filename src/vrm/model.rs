// SPDX-License-Identifier: GPL-3.0-or-later
// A 3D character made from the drawing and its rig, for games: every part becomes a rounded "card" with depth, a humanoid
// skeleton is placed from the Parts tags, every vertex gets skin weights, and the face sliders of the rig become
// expression shapes (blink, mouth vowels, smile, look direction…). Coordinates are metres, Y up, the character faces +Z
// and its left side is +X (as glTF and VRM 1.0 want); the VRM 0.x writer turns it round.

use std::collections::HashMap;

use image::RgbaImage;

use crate::doc::{Document, LayerId};
use crate::parts::Part;
use crate::rig::raster::LayerTex;
use crate::rig::{Rig, Values};

/// How the 3D character is made.
#[derive(Clone, Debug)]
pub struct Options {
    pub name: String,
    pub author: String,
    /// Height of the character in metres.
    pub height_m: f32,
    /// How round the parts are, 0 = flat cards, 1 = the default roundness, up to 2.
    pub puff: f32,
    /// Distance in metres between neighbouring layers in the stack (so the front part is in front).
    pub layer_gap: f32,
    /// Largest side of a texture in pixels (bigger pictures are made smaller).
    pub max_texture: u32,
    /// Make finger bones (a thumb and four fingers of three bones on each hand).
    pub fingers: bool,
}

impl Default for Options {
    fn default() -> Self {
        Self { name: "Character".into(), author: "Unknown".into(), height_m: 1.6, puff: 1.0, layer_gap: 0.003, max_texture: 2048, fingers: true }
    }
}

/// A shape that moves the vertices of one part (an expression).
pub struct Morph {
    pub name: String,
    pub deltas: Vec<[f32; 3]>,
}

pub struct Part3d {
    pub name: String,
    pub layer: LayerId,
    pub verts: Vec<[f32; 3]>,
    pub normals: Vec<[f32; 3]>,
    pub uvs: Vec<[f32; 2]>,
    pub tris: Vec<[u32; 3]>,
    pub joints: Vec<[u16; 4]>,
    pub weights: Vec<[f32; 4]>,
    pub image: RgbaImage,
    pub morphs: Vec<Morph>,
}

pub struct Bone {
    /// The VRM humanoid name ("hips", "leftUpperArm"…), or the name of a joint of a swinging chain.
    pub name: String,
    /// Is it one of the humanoid bones (not a joint of hair or cloth)?
    pub humanoid: bool,
    pub parent: Option<usize>,
    /// Where it is, in the model's space.
    pub pos: [f32; 3],
    /// Where its piece of the body ends (for the skin weights).
    pub tip: [f32; 3],
}

/// One expression: which shapes of which parts it moves, and by how much.
pub struct Expression {
    /// VRM preset name ("blink", "aa", "happy"…) or a custom name (ARKit style: "jawOpen").
    pub name: String,
    pub binds: Vec<(usize, usize, f32)>,
}

/// A chain of joints that swings by itself in a game (hair, cloth, a tail): the physics of the Physics tab.
pub struct Spring {
    pub name: String,
    /// The bones of the chain, from the root out.
    pub joints: Vec<usize>,
    pub hit_radius: f32,
    pub stiffness: f32,
    pub gravity: f32,
    pub drag: f32,
}

/// A ball on a bone that swinging chains do not go through.
pub struct Collider {
    pub bone: usize,
    pub offset: [f32; 3],
    pub radius: f32,
}

pub struct Model3d {
    pub name: String,
    pub author: String,
    pub parts: Vec<Part3d>,
    pub bones: Vec<Bone>,
    pub expressions: Vec<Expression>,
    pub springs: Vec<Spring>,
    pub colliders: Vec<Collider>,
    /// Metres per page pixel.
    pub scale: f32,
}

/// The VRM humanoid bones we make: (name, parent, the bone its piece runs to).
const BONES: [(&str, Option<&str>, Option<&str>); 23] = [
    ("hips", None, Some("spine")),
    ("spine", Some("hips"), Some("chest")),
    ("chest", Some("spine"), Some("neck")),
    ("neck", Some("chest"), Some("head")),
    ("head", Some("neck"), None),
    ("leftEye", Some("head"), None),
    ("rightEye", Some("head"), None),
    ("leftShoulder", Some("chest"), Some("leftUpperArm")),
    ("leftUpperArm", Some("leftShoulder"), Some("leftLowerArm")),
    ("leftLowerArm", Some("leftUpperArm"), Some("leftHand")),
    ("leftHand", Some("leftLowerArm"), None),
    ("rightShoulder", Some("chest"), Some("rightUpperArm")),
    ("rightUpperArm", Some("rightShoulder"), Some("rightLowerArm")),
    ("rightLowerArm", Some("rightUpperArm"), Some("rightHand")),
    ("rightHand", Some("rightLowerArm"), None),
    ("leftUpperLeg", Some("hips"), Some("leftLowerLeg")),
    ("leftLowerLeg", Some("leftUpperLeg"), Some("leftFoot")),
    ("leftFoot", Some("leftLowerLeg"), Some("leftToes")),
    ("leftToes", Some("leftFoot"), None),
    ("rightUpperLeg", Some("hips"), Some("rightLowerLeg")),
    ("rightLowerLeg", Some("rightUpperLeg"), Some("rightFoot")),
    ("rightFoot", Some("rightLowerLeg"), Some("rightToes")),
    ("rightToes", Some("rightFoot"), None),
];

impl Model3d {
    pub fn bone(&self, name: &str) -> Option<usize> {
        self.bones.iter().position(|b| b.name == name)
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Cat {
    Head,
    Neck,
    Torso,
    Arm(bool),
    Hand(bool),
    Leg(bool),
    Any,
}

struct Info {
    layer: LayerId,
    name: String,
    tag: Option<Part>,
    mesh: usize,
    bb: [f32; 4],
}

fn bbox(verts: &[[f32; 2]]) -> [f32; 4] {
    let mut b = [f32::MAX, f32::MAX, f32::MIN, f32::MIN];
    for v in verts {
        b[0] = b[0].min(v[0]);
        b[1] = b[1].min(v[1]);
        b[2] = b[2].max(v[0]);
        b[3] = b[3].max(v[1]);
    }
    b
}

fn merge(a: Option<[f32; 4]>, b: [f32; 4]) -> Option<[f32; 4]> {
    Some(match a {
        Some(a) => [a[0].min(b[0]), a[1].min(b[1]), a[2].max(b[2]), a[3].max(b[3])],
        None => b,
    })
}

fn category(tag: Option<Part>, side_left: bool) -> Cat {
    match tag {
        Some(Part::Face | Part::EyeL | Part::EyeR | Part::EyeballL | Part::EyeballR | Part::BrowL | Part::BrowR | Part::Nose | Part::Mouth | Part::MouthInside | Part::EarL | Part::EarR | Part::Blush | Part::HairFront | Part::HairSideL | Part::HairSideR | Part::HairBack) => Cat::Head,
        Some(Part::Neck) => Cat::Neck,
        Some(Part::Torso) => Cat::Torso,
        Some(Part::ArmL | Part::ArmR) => Cat::Arm(side_left),
        Some(Part::HandL | Part::HandR) => Cat::Hand(side_left),
        Some(Part::LegL | Part::LegR) => Cat::Leg(side_left),
        _ => Cat::Any,
    }
}

/// How high the stack of parts is at every place of the page: each part sits on what is under it.
struct HeightMap {
    x0: f32,
    y0: f32,
    cell: f32,
    w: usize,
    h: usize,
    z: Vec<f32>,
}

impl HeightMap {
    fn new(bb: [f32; 4], cell: f32) -> HeightMap {
        let (w, h) = ((((bb[2] - bb[0]) / cell).ceil() as usize + 3).min(4000), (((bb[3] - bb[1]) / cell).ceil() as usize + 3).min(4000));
        HeightMap { x0: bb[0] - cell, y0: bb[1] - cell, cell, w, h, z: vec![0.0; w * h] }
    }

    /// The highest point around a place (a small neighbourhood, so the edge of a part sits on what is next to it).
    fn at(&self, p: [f32; 2]) -> f32 {
        let (cx, cy) = (((p[0] - self.x0) / self.cell) as i64, ((p[1] - self.y0) / self.cell) as i64);
        let mut best = 0.0f32;
        for dy in -1..=1 {
            for dx in -1..=1 {
                let (x, y) = (cx + dx, cy + dy);
                if x >= 0 && y >= 0 && (x as usize) < self.w && (y as usize) < self.h {
                    best = best.max(self.z[y as usize * self.w + x as usize]);
                }
            }
        }
        best
    }

    /// Put a triangle (page positions, heights) on the map, keeping the higher where there already is something.
    fn splat(&mut self, t: [([f32; 2], f32); 3]) {
        let (a, b, c) = (t[0], t[1], t[2]);
        let det = (b.0[1] - c.0[1]) * (a.0[0] - c.0[0]) + (c.0[0] - b.0[0]) * (a.0[1] - c.0[1]);
        if det.abs() < 1e-9 {
            return;
        }
        let lo = |f: fn(f32, f32) -> f32, k: usize| f(f(a.0[k], b.0[k]), c.0[k]);
        let (x0, x1) = (((lo(f32::min, 0) - self.x0) / self.cell).floor().max(0.0) as usize, (((lo(f32::max, 0) - self.x0) / self.cell).ceil() as usize).min(self.w - 1));
        let (y0, y1) = (((lo(f32::min, 1) - self.y0) / self.cell).floor().max(0.0) as usize, (((lo(f32::max, 1) - self.y0) / self.cell).ceil() as usize).min(self.h - 1));
        for y in y0..=y1 {
            for x in x0..=x1 {
                let (px, py) = (self.x0 + (x as f32 + 0.5) * self.cell, self.y0 + (y as f32 + 0.5) * self.cell);
                let l1 = ((b.0[1] - c.0[1]) * (px - c.0[0]) + (c.0[0] - b.0[0]) * (py - c.0[1])) / det;
                let l2 = ((c.0[1] - a.0[1]) * (px - c.0[0]) + (a.0[0] - c.0[0]) * (py - c.0[1])) / det;
                let l3 = 1.0 - l1 - l2;
                if l1 < -0.05 || l2 < -0.05 || l3 < -0.05 {
                    continue;
                }
                let z = a.1 * l1 + b.1 * l2 + c.1 * l3;
                let cell = &mut self.z[y * self.w + x];
                *cell = cell.max(z);
            }
        }
    }
}

fn seg_dist(p: [f32; 3], a: [f32; 3], b: [f32; 3]) -> f32 {
    let ab = [b[0] - a[0], b[1] - a[1], b[2] - a[2]];
    let l2 = ab[0] * ab[0] + ab[1] * ab[1] + ab[2] * ab[2];
    let t = if l2 < 1e-12 { 0.0 } else { (((p[0] - a[0]) * ab[0] + (p[1] - a[1]) * ab[1] + (p[2] - a[2]) * ab[2]) / l2).clamp(0.0, 1.0) };
    let q = [a[0] + ab[0] * t, a[1] + ab[1] * t, a[2] + ab[2] * t];
    ((p[0] - q[0]).powi(2) + (p[1] - q[1]).powi(2) + (p[2] - q[2]).powi(2)).sqrt()
}

/// Skin weights of a point: the bones it may follow, the nearer bone pieces counting more; at most four.
fn weights_for(p: [f32; 3], cands: &[usize], bones: &[Bone], sigma: f32) -> ([u16; 4], [f32; 4]) {
    if cands.len() == 1 {
        return ([cands[0] as u16, 0, 0, 0], [1.0, 0.0, 0.0, 0.0]);
    }
    let mut w: Vec<(usize, f32)> = cands.iter().map(|&b| (b, (-(seg_dist(p, bones[b].pos, bones[b].tip) / sigma).powi(2)).exp())).collect();
    w.sort_by(|a, b| b.1.total_cmp(&a.1));
    w.truncate(4);
    let sum: f32 = w.iter().map(|x| x.1).sum();
    if sum < 1e-6 {
        let nearest = cands.iter().copied().min_by(|a, b| seg_dist(p, bones[*a].pos, bones[*a].tip).total_cmp(&seg_dist(p, bones[*b].pos, bones[*b].tip))).unwrap_or(0);
        return ([nearest as u16, 0, 0, 0], [1.0, 0.0, 0.0, 0.0]);
    }
    let (mut j, mut o) = ([0u16; 4], [0.0f32; 4]);
    for (i, (b, x)) in w.iter().enumerate() {
        j[i] = *b as u16;
        o[i] = x / sum;
    }
    (j, o)
}

/// Distance of every vertex to the outline of the mesh (page pixels).
fn edge_distance(verts: &[[f32; 2]], tris: &[[u32; 3]]) -> Vec<f32> {
    let mut count: HashMap<(u32, u32), u32> = HashMap::new();
    for t in tris {
        for k in 0..3 {
            let (a, b) = (t[k], t[(k + 1) % 3]);
            *count.entry((a.min(b), a.max(b))).or_default() += 1;
        }
    }
    let edges: Vec<([f32; 2], [f32; 2])> = count.iter().filter(|(_, c)| **c == 1).map(|(k, _)| (verts[k.0 as usize], verts[k.1 as usize])).collect();
    if edges.is_empty() {
        return vec![0.0; verts.len()];
    }
    verts
        .iter()
        .map(|p| {
            edges
                .iter()
                .map(|(a, b)| {
                    let (dx, dy) = (b[0] - a[0], b[1] - a[1]);
                    let l2 = dx * dx + dy * dy;
                    let t = if l2 < 1e-9 { 0.0 } else { (((p[0] - a[0]) * dx + (p[1] - a[1]) * dy) / l2).clamp(0.0, 1.0) };
                    (p[0] - (a[0] + dx * t)).hypot(p[1] - (a[1] + dy * t))
                })
                .fold(f32::MAX, f32::min)
        })
        .collect()
}

/// Smooth normals from the triangles.
fn normals_of(verts: &[[f32; 3]], tris: &[[u32; 3]]) -> Vec<[f32; 3]> {
    let mut n = vec![[0.0f32; 3]; verts.len()];
    for t in tris {
        let (a, b, c) = (verts[t[0] as usize], verts[t[1] as usize], verts[t[2] as usize]);
        let (u, v) = ([b[0] - a[0], b[1] - a[1], b[2] - a[2]], [c[0] - a[0], c[1] - a[1], c[2] - a[2]]);
        let f = [u[1] * v[2] - u[2] * v[1], u[2] * v[0] - u[0] * v[2], u[0] * v[1] - u[1] * v[0]];
        for i in t {
            for k in 0..3 {
                n[*i as usize][k] += f[k];
            }
        }
    }
    for v in &mut n {
        let l = (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt();
        *v = if l > 1e-12 { [v[0] / l, v[1] / l, v[2] / l] } else { [0.0, 0.0, 1.0] };
    }
    n
}

/// The picture of a layer as a normal (not premultiplied) image, made smaller when it is bigger than `max`.
fn layer_image(tex: &LayerTex, max: u32) -> RgbaImage {
    let img = tex.to_rgba();
    let big = img.width().max(img.height());
    if big <= max || max < 16 {
        return img;
    }
    let k = max as f32 / big as f32;
    image::imageops::resize(&img, ((img.width() as f32 * k).round() as u32).max(1), ((img.height() as f32 * k).round() as u32).max(1), image::imageops::FilterType::Triangle)
}

fn find_param(rig: &Rig, words: &[&str]) -> Option<crate::rig::Id> {
    rig.params.iter().find(|p| {
        let n = p.name.to_lowercase();
        words.iter().all(|w| n.contains(w))
    }).map(|p| p.id)
}

/// Which end of a slider is "more": the end away from its default.
fn far_end(rig: &Rig, id: crate::rig::Id) -> f32 {
    rig.param(id).map(|p| if (p.max - p.default).abs() >= (p.default - p.min).abs() { p.max } else { p.min }).unwrap_or(0.0)
}

/// Build the 3D character. `None` when there is nothing with a mesh to build from.
pub fn build(doc: &Document, opt: &Options) -> Option<Model3d> {
    let rig = &*doc.rig;
    // the parts: meshes of visible layers, bottom first
    let mut infos: Vec<Info> = Vec::new();
    for l in doc.layers.iter().rev() {
        if !l.visible {
            continue;
        }
        let Some(mi) = rig.meshes.iter().position(|m| m.layer == l.id) else { continue };
        let m = &rig.meshes[mi];
        if m.verts.is_empty() || m.tris.is_empty() {
            continue;
        }
        infos.push(Info { layer: l.id, name: l.name.clone(), tag: l.part, mesh: mi, bb: bbox(&m.verts) });
    }
    if infos.is_empty() {
        return None;
    }
    // the frame of the character: the middle, the feet and the size
    let all = infos.iter().fold(None, |a, i| merge(a, i.bb))?;
    let (height_px, xc0) = ((all[3] - all[1]).max(1.0), (all[0] + all[2]) / 2.0);
    let scale = opt.height_m / height_px;
    let to3 = |p: [f32; 2], z: f32| -> [f32; 3] { [(p[0] - xc0) * scale, (all[3] - p[1]) * scale, z] };
    // where the parts say the body is
    let by = |pred: &dyn Fn(&Info) -> bool| infos.iter().filter(|i| pred(i)).fold(None, |a, i| merge(a, i.bb));
    let face = by(&|i| i.tag == Some(Part::Face));
    let head = by(&|i| matches!(category(i.tag, false), Cat::Head) && i.tag != Some(Part::HairBack));
    let torso = by(&|i| i.tag == Some(Part::Torso));
    let xc = face.map(|b| (b[0] + b[2]) / 2.0).unwrap_or(xc0);
    let hb = head.unwrap_or([xc - 0.09 * height_px, all[1] + 0.02 * height_px, xc + 0.09 * height_px, all[1] + 0.15 * height_px]);
    let neck_y = face.map(|b| b[3]).unwrap_or(hb[3]);
    let tb = torso.unwrap_or([xc - 0.12 * height_px, neck_y, xc + 0.12 * height_px, neck_y + 0.33 * height_px]);
    let th = (tb[3] - tb[1]).max(1.0);
    let lerp = |a: f32, b: f32, t: f32| a + (b - a) * t;
    let hips2 = [xc, lerp(tb[1], tb[3], 0.92)];
    let spine2 = [xc, lerp(tb[1], tb[3], 0.62)];
    let chest2 = [xc, lerp(tb[1], tb[3], 0.28)];
    let neck2 = [xc, neck_y];
    let head2 = [(hb[0] + hb[2]) / 2.0, (hb[1] + hb[3]) / 2.0];
    let top2 = [head2[0], hb[1]];
    let half = ((tb[2] - tb[0]) * 0.42).max(0.07 * height_px);
    // the sides: left in VRM is +X, the right of the picture
    let side_parts = |left: bool, tags: &[Part]| by(&|i| i.tag.is_some_and(|t| tags.contains(&t)) && (((i.bb[0] + i.bb[2]) / 2.0 > xc) == left));
    let mut joints: HashMap<String, [f32; 2]> = HashMap::new();
    let mut tips: HashMap<String, [f32; 2]> = HashMap::new();
    let ybottom = all[3];
    for left in [true, false] {
        let sgn = if left { 1.0 } else { -1.0 };
        let pre = if left { "left" } else { "right" };
        let n = |s: &str| -> String { format!("{pre}{s}") };
        let shoulder = [xc + sgn * half, tb[1] + 0.12 * th];
        let arm = side_parts(left, &[Part::ArmL, Part::ArmR]);
        let hand = side_parts(left, &[Part::HandL, Part::HandR]);
        let wrist = match (arm, hand) {
            (_, Some(h)) => [(h[0] + h[2]) / 2.0, h[1]],
            (Some(a), None) => [(a[0] + a[2]) / 2.0, a[3]],
            _ => [shoulder[0] + sgn * 0.02 * height_px, ybottom - 0.45 * height_px],
        };
        let tip = hand.map(|h| [(h[0] + h[2]) / 2.0, h[3]]).unwrap_or([wrist[0], wrist[1] + 0.07 * height_px]);
        let elbow = [lerp(shoulder[0], wrist[0], 0.45), lerp(shoulder[1], wrist[1], 0.45)];
        joints.insert(n("Shoulder"), [xc + sgn * half * 0.4, shoulder[1] - 0.01 * th]);
        joints.insert(n("UpperArm"), shoulder);
        joints.insert(n("LowerArm"), elbow);
        joints.insert(n("Hand"), wrist);
        tips.insert(n("Hand"), tip);
        let leg = side_parts(left, &[Part::LegL, Part::LegR]);
        let hip = [xc + sgn * 0.22 * (tb[2] - tb[0]), hips2[1]];
        let ankle = match leg {
            Some(l) => [(l[0] + l[2]) / 2.0, l[3] - 0.04 * height_px],
            None => [hip[0], ybottom - 0.04 * height_px],
        };
        let knee = [lerp(hip[0], ankle[0], 0.40), lerp(hip[1], ankle[1], 0.40)];
        joints.insert(n("UpperLeg"), hip);
        joints.insert(n("LowerLeg"), knee);
        joints.insert(n("Foot"), ankle);
        tips.insert(n("Toes"), [ankle[0], ybottom]);
        joints.insert(n("Toes"), [ankle[0], ybottom - 0.01 * height_px]);
        // the eyes
        let eye = by(&|i| matches!(i.tag, Some(Part::EyeballL | Part::EyeballR | Part::EyeL | Part::EyeR)) && (((i.bb[0] + i.bb[2]) / 2.0 > xc) == left));
        joints.insert(n("Eye"), eye.map(|b| [(b[0] + b[2]) / 2.0, (b[1] + b[3]) / 2.0]).unwrap_or([head2[0] + sgn * 0.2 * (hb[2] - hb[0]), head2[1]]));
    }
    joints.insert("hips".into(), hips2);
    joints.insert("spine".into(), spine2);
    joints.insert("chest".into(), chest2);
    joints.insert("neck".into(), neck2);
    joints.insert("head".into(), head2);
    tips.insert("head".into(), top2);
    // the bones in the model's space
    let mut bones: Vec<Bone> = Vec::new();
    for (name, parent, _) in BONES {
        let p2 = joints.get(name).copied().unwrap_or(hips2);
        // the face is the middle of the depth, so the skeleton sits there
        let z = if name.ends_with("Toes") { 0.04 } else { 0.0 };
        bones.push(Bone { name: name.to_string(), humanoid: true, parent: parent.and_then(|p| BONES.iter().position(|b| b.0 == p)), pos: to3(p2, z), tip: to3(p2, z) });
    }
    for i in 0..bones.len() {
        let next = BONES[i].2.and_then(|n| BONES.iter().position(|b| b.0 == n));
        bones[i].tip = match (next, tips.get(bones[i].name.as_str())) {
            (Some(n), _) => bones[n].pos,
            (None, Some(t)) => to3(*t, 0.0),
            // eyes and fingertips-less bones: a short piece forward
            (None, None) => [bones[i].pos[0], bones[i].pos[1], bones[i].pos[2] + 0.02],
        };
    }
    let sigma = 0.035 * opt.height_m / 1.6;
    // swinging chains from the physics, and the balls they do not go through
    let rest_pose = rig.pose(&Values::default());
    let mut springs: Vec<Spring> = Vec::new();
    let mut spring_of_changer: HashMap<crate::rig::Id, Vec<usize>> = HashMap::new();
    let humanoid_count = bones.len();
    for pc in rig.physics.iter().filter(|c| c.enabled && c.kind != crate::rig::physics::Kind::Solid) {
        let Some(ch) = rig.changer(pc.changer) else { continue };
        let (pts, width) = match &ch.kind {
            crate::rig::Kind::Line { pts, width } => (pts.clone(), *width),
            crate::rig::Kind::Paths { paths } => match paths.get(pc.path) {
                Some(sp) => (sp.pts.clone(), sp.width),
                None => continue,
            },
            _ => continue,
        };
        if pts.len() < 2 || springs.len() >= 120 {
            continue;
        }
        let world: Vec<[f32; 2]> = pts.iter().map(|p| rig.deform_point(&rest_pose, Some(ch.id), *p)).collect();
        // fixed to the humanoid bone that is nearest to its root
        let root3 = to3(world[0], 0.0);
        let attach = (0..humanoid_count).min_by(|a, b| seg_dist(root3, bones[*a].pos, bones[*a].tip).total_cmp(&seg_dist(root3, bones[*b].pos, bones[*b].tip))).unwrap_or(0);
        let tag: String = pc.name.chars().filter(|c| c.is_ascii_alphanumeric()).take(16).collect();
        let first = bones.len();
        for (i, p) in world.iter().enumerate() {
            let here = to3(*p, 0.0);
            let next = to3(world[(i + 1).min(world.len() - 1)], 0.0);
            bones.push(Bone { name: format!("J_Sec_{}_{}_{}", tag, springs.len(), i), humanoid: false, parent: Some(if i == 0 { attach } else { first + i - 1 }), pos: here, tip: if i + 1 < world.len() { next } else { [here[0], here[1] - 0.02, here[2]] } });
        }
        let mean = (pc.stiff_root + pc.stiff_tip) / 2.0;
        spring_of_changer.entry(pc.changer).or_default().push(springs.len());
        springs.push(Spring { name: format!("{} {}", pc.name, springs.len() + 1), joints: (first..bones.len()).collect(), hit_radius: (width * scale * 0.4).clamp(0.01, 0.08), stiffness: 0.3 + 2.0 * mean, gravity: (pc.gravity * 0.4).clamp(0.0, 2.0), drag: pc.damping.clamp(0.05, 0.9) });
    }
    let mut colliders: Vec<Collider> = Vec::new();
    for c in rig.colliders.iter().take(32) {
        let at = to3(rig.deform_point(&rest_pose, c.changer, c.center), 0.0);
        // a ball sits on the joint it is nearest to (the head for a face, the chest for a body)
        let near = |b: usize| (bones[b].pos[0] - at[0]).powi(2) + (bones[b].pos[1] - at[1]).powi(2) + (bones[b].pos[2] - at[2]).powi(2);
        let bone = (0..humanoid_count).filter(|b| !bones[*b].name.ends_with("Eye")).min_by(|a, b| near(*a).total_cmp(&near(*b))).unwrap_or(0);
        let p = bones[bone].pos;
        colliders.push(Collider { bone, offset: [at[0] - p[0], at[1] - p[1], 0.0], radius: (c.radius * scale).clamp(0.01, 1.0) });
    }
    let bone_ix = |n: &str| bones.iter().position(|b| b.name == n).unwrap_or(0);
    // the parts
    let mut heights = HeightMap::new(all, (height_px / 700.0).max(1.0));
    let mut parts: Vec<Part3d> = Vec::new();
    let mut index_of_mesh: HashMap<usize, usize> = HashMap::new();
    for (rank, info) in infos.iter().enumerate() {
        let m = &rig.meshes[info.mesh];
        let Some(tex) = LayerTex::of(doc.layers.iter().find(|l| l.id == info.layer)?, doc.w, doc.h) else { continue };
        let side_left = (info.bb[0] + info.bb[2]) / 2.0 > xc;
        let cat = category(info.tag, side_left);
        // depth: the stack order, plus roundness inside the outline
        let d = edge_distance(&m.verts, &m.tris);
        let (w_px, h_px) = (info.bb[2] - info.bb[0], info.bb[3] - info.bb[1]);
        let reach = (w_px.min(h_px) * 0.3).max(4.0);
        let round = (opt.puff * 0.1 * (w_px.min(h_px) * scale)).clamp(0.0, 0.025);
        let _ = rank;
        // every vertex sits on what is under it, a little above, and the inside of the part is raised in a round shape
        let mut verts: Vec<[f32; 3]> = m
            .verts
            .iter()
            .zip(&d)
            .map(|(p, d)| {
                let t = (d / reach).min(1.0);
                to3(*p, heights.at(*p) + opt.layer_gap + round * (1.0 - (1.0 - t) * (1.0 - t)).max(0.0).sqrt())
            })
            .collect();
        for t in &m.tris {
            heights.splat([(m.verts[t[0] as usize], verts[t[0] as usize][2]), (m.verts[t[1] as usize], verts[t[1] as usize][2]), (m.verts[t[2] as usize], verts[t[2] as usize][2])]);
        }
        // the front faces +Z
        let mut tris = m.tris.clone();
        for t in &mut tris {
            let (a, b, c) = (verts[t[0] as usize], verts[t[1] as usize], verts[t[2] as usize]);
            if (b[0] - a[0]) * (c[1] - a[1]) - (b[1] - a[1]) * (c[0] - a[0]) < 0.0 {
                t.swap(1, 2);
            }
        }
        let normals = normals_of(&verts, &tris);
        let uvs: Vec<[f32; 2]> = m.verts.iter().map(|p| [(p[0] - tex.x0 as f32) / tex.w as f32, (p[1] - tex.y0 as f32) / tex.h as f32]).collect();
        // a part that hangs from a swinging chain follows the joints of that chain
        let chain_joints: Vec<usize> = {
            let mut v = Vec::new();
            let (mut cur, mut depth) = (m.parent, 0);
            while let Some(id) = cur {
                if let Some(sp) = spring_of_changer.get(&id) {
                    for s in sp {
                        v.extend(springs[*s].joints.iter().copied());
                    }
                }
                cur = rig.changer(id).and_then(|c| c.parent);
                depth += 1;
                if depth > 16 {
                    break;
                }
            }
            v
        };
        // which bones the part follows
        let cands: Vec<usize> = if !chain_joints.is_empty() { chain_joints.clone() } else { match cat {
            Cat::Head => vec![bone_ix("head")],
            Cat::Neck => vec![bone_ix("chest"), bone_ix("neck"), bone_ix("head")],
            Cat::Torso => vec![bone_ix("hips"), bone_ix("spine"), bone_ix("chest"), bone_ix("neck")],
            Cat::Arm(l) => {
                let p = if l { "left" } else { "right" };
                vec![bone_ix("chest"), bone_ix(&format!("{p}UpperArm")), bone_ix(&format!("{p}LowerArm")), bone_ix(&format!("{p}Hand"))]
            }
            Cat::Hand(l) => {
                let p = if l { "left" } else { "right" };
                vec![bone_ix(&format!("{p}LowerArm")), bone_ix(&format!("{p}Hand"))]
            }
            Cat::Leg(l) => {
                let p = if l { "left" } else { "right" };
                vec![bone_ix("hips"), bone_ix(&format!("{p}UpperLeg")), bone_ix(&format!("{p}LowerLeg")), bone_ix(&format!("{p}Foot"))]
            }
            Cat::Any => (0..humanoid_count).filter(|i| !bones[*i].name.ends_with("Eye")).collect(),
        } };
        let (mut joints_v, mut weights_v) = (Vec::with_capacity(verts.len()), Vec::with_capacity(verts.len()));
        for v in &verts {
            let (j, w) = weights_for(*v, &cands, &bones, if chain_joints.is_empty() { sigma } else { sigma * 0.6 });
            joints_v.push(j);
            weights_v.push(w);
        }
        verts.shrink_to_fit();
        index_of_mesh.insert(info.mesh, parts.len());
        parts.push(Part3d { name: info.name.clone(), layer: info.layer, verts, normals, uvs, tris, joints: joints_v, weights: weights_v, image: layer_image(&tex, opt.max_texture), morphs: Vec::new() });
    }
    if parts.is_empty() {
        return None;
    }
    // the middle of the depth is the face, where the skeleton is
    let centre = {
        let face_part = parts.iter().find(|p| infos.iter().any(|i| i.layer == p.layer && i.tag == Some(Part::Face))).unwrap_or(&parts[0]);
        face_part.verts.iter().map(|v| v[2]).sum::<f32>() / face_part.verts.len().max(1) as f32
    };
    for p in &mut parts {
        for v in &mut p.verts {
            v[2] -= centre;
        }
    }
    // the swinging joints and balls are in the middle of the depth too
    let mut model = Model3d { name: opt.name.clone(), author: opt.author.clone(), parts, bones, expressions: Vec::new(), springs, colliders, scale };
    make_expressions(&mut model, rig, &index_of_mesh, scale);
    Some(model)
}

/// Build from either: the cut parts when the rig has meshes, or the one uncut picture with its skeleton.
pub fn build_any(doc: &Document, opt: &Options) -> Option<Model3d> {
    let cut = doc.rig.meshes.iter().any(|m| doc.layers.iter().any(|l| l.id == m.layer && l.visible));
    let uncut = doc.rig.skin.as_ref().is_some_and(|s| s.bones.len() >= 5 && !s.verts.is_empty());
    if cut {
        build(doc, opt)
    } else if uncut {
        build_uncut(doc, opt)
    } else {
        None
    }
}

/// A 3D character from the uncut picture: its mesh is one round card, the bones of the skin are the skeleton, and the
/// painted weights are the skin weights.
pub fn build_uncut(doc: &Document, opt: &Options) -> Option<Model3d> {
    let rig = &*doc.rig;
    let sk = rig.skin.as_ref()?;
    let page = crate::rig::uncut::cut_out_background(&crate::rig::uncut::composite(doc, &sk.layers)?);
    // the bones by their humanoid names
    let named: HashMap<String, (usize, [f32; 2], [f32; 2])> = sk
        .bones
        .iter()
        .enumerate()
        .filter_map(|(i, b)| {
            let c = rig.changer(b.changer)?;
            match c.kind {
                crate::rig::Kind::Rotation { pivot } => Some((c.name.clone(), (i, pivot, b.tail))),
                _ => None,
            }
        })
        .collect();
    if !["hips", "head"].iter().all(|n| named.contains_key(*n)) {
        return None;
    }
    let vb = bbox(&sk.verts);
    let height_px = (vb[3] - vb[1]).max(1.0);
    let scale = opt.height_m / height_px;
    let xc = (vb[0] + vb[2]) / 2.0;
    let to3 = |p: [f32; 2], z: f32| -> [f32; 3] { [(p[0] - xc) * scale, (vb[3] - p[1]) * scale, z] };
    // the bones that exist
    let mut bones: Vec<Bone> = Vec::new();
    let mut slot: HashMap<String, usize> = HashMap::new();
    for (name, parent, _) in BONES {
        let pos2 = match (named.get(name), name) {
            (Some(b), _) => b.1,
            (None, "leftToes") => match named.get("leftFoot") {
                Some(f) => f.2,
                None => continue,
            },
            (None, "rightToes") => match named.get("rightFoot") {
                Some(f) => f.2,
                None => continue,
            },
            (None, n) if n.ends_with("Eye") => continue,
            // a missing limb bone sits where its parent is
            (None, _) => match parent.and_then(|p| slot.get(p)).map(|i| bones[*i].pos) {
                Some(p) => {
                    slot.insert(name.to_string(), bones.len());
                    bones.push(Bone { name: name.to_string(), humanoid: true, parent: parent.and_then(|p| slot.get(p).copied()), pos: p, tip: p });
                    continue;
                }
                None => continue,
            },
        };
        let z = if name.ends_with("Toes") { 0.03 } else { 0.0 };
        let p3 = to3(pos2, z);
        slot.insert(name.to_string(), bones.len());
        bones.push(Bone { name: name.to_string(), humanoid: true, parent: parent.and_then(|p| slot.get(p).copied()), pos: p3, tip: p3 });
    }
    // fingers: a thumb and four fingers of three bones along each hand
    let mut frames: [Option<super::hands::Frame>; 2] = [None, None];
    if opt.fingers {
        for (si, left) in [true, false].iter().enumerate() {
            let hand = if *left { "leftHand" } else { "rightHand" };
            let (Some(&hi), Some(h)) = (slot.get(hand), named.get(hand)) else { continue };
            let (wrist, tip) = (bones[hi].pos, to3(h.2, 0.0));
            let fr = super::hands::frame(wrist, tip, *left);
            for (mut b, (_, parent)) in fr.bones(*left).into_iter().zip(super::hands::finger_names(*left)) {
                b.parent = slot.get(parent.as_str()).copied();
                slot.insert(b.name.clone(), bones.len());
                bones.push(b);
            }
            frames[si] = Some(fr);
        }
    }
    for i in 0..bones.len() {
        let Some(def) = BONES.iter().find(|b| b.0 == bones[i].name) else { continue };
        let next = def.2.and_then(|n| slot.get(n).copied());
        let own = named.get(bones[i].name.as_str()).map(|b| b.2);
        bones[i].tip = match (next, own) {
            (Some(n), _) => bones[n].pos,
            (None, Some(t)) => to3(t, 0.0),
            _ => [bones[i].pos[0], bones[i].pos[1] - 0.02, bones[i].pos[2]],
        };
    }
    // the picture: the part of the page the mesh covers
    let (cx0, cy0) = ((vb[0].floor() as i64 - 2).max(0) as u32, (vb[1].floor() as i64 - 2).max(0) as u32);
    let (cx1, cy1) = (((vb[2].ceil() as u32) + 2).min(page.width()), ((vb[3].ceil() as u32) + 2).min(page.height()));
    if cx1 <= cx0 + 2 || cy1 <= cy0 + 2 {
        return None;
    }
    let crop = image::imageops::crop_imm(&page, cx0, cy0, cx1 - cx0, cy1 - cy0).to_image();
    let (tw, th) = (crop.width() as f32, crop.height() as f32);
    let big = crop.width().max(crop.height());
    let image = if big > opt.max_texture {
        let k = opt.max_texture as f32 / big as f32;
        image::imageops::resize(&crop, ((tw * k).round() as u32).max(1), ((th * k).round() as u32).max(1), image::imageops::FilterType::Triangle)
    } else {
        crop
    };
    // a finer mesh, so it bends smoothly in a game
    let (mesh_v, mesh_t) = crate::rig::refine::subdivide(&sk.verts, &sk.tris, (0.012 * height_px).max(3.0), 40_000);
    // depth: round inside the outline, the edge flat
    let d = edge_distance(&mesh_v, &mesh_t);
    let reach = (0.045 * height_px).max(4.0);
    let round = (opt.puff * 0.03 * opt.height_m / 1.6).clamp(0.0, 0.05);
    let mut verts: Vec<[f32; 3]> = mesh_v.iter().zip(&d).map(|(p, d)| to3(*p, round * (1.0 - (1.0 - (d / reach).min(1.0)).powi(2)).max(0.0).sqrt())).collect();
    let mut tris = mesh_t.clone();
    for t in &mut tris {
        let (a, b, c) = (verts[t[0] as usize], verts[t[1] as usize], verts[t[2] as usize]);
        if (b[0] - a[0]) * (c[1] - a[1]) - (b[1] - a[1]) * (c[0] - a[0]) < 0.0 {
            t.swap(1, 2);
        }
    }
    let normals = normals_of(&verts, &tris);
    let uvs: Vec<[f32; 2]> = mesh_v.iter().map(|p| [(p[0] - cx0 as f32) / tw, (p[1] - cy0 as f32) / th]).collect();
    // the weights painted on the skin, on the bones of the model; what no bone claims goes to the nearest bone
    let ix_of: Vec<Option<usize>> = sk
        .bones
        .iter()
        .map(|b| rig.changer(b.changer).and_then(|c| slot.get(c.name.as_str()).copied()))
        .collect();
    let mut raw: Vec<Vec<(usize, f32)>> = Vec::with_capacity(verts.len());
    for (v, p3) in mesh_v.iter().zip(&verts) {
        let w = sk.weights_at(v[0], v[1]);
        let mut acc: Vec<(usize, f32)> = Vec::new();
        for (sb, wb) in w.iter().enumerate() {
            if *wb > 0.001 {
                if let Some(mb) = ix_of[sb] {
                    match acc.iter_mut().find(|a| a.0 == mb) {
                        Some(a) => a.1 += wb,
                        None => acc.push((mb, *wb)),
                    }
                }
            }
        }
        let sum: f32 = acc.iter().map(|a| a.1).sum();
        if sum < 0.999 {
            let near = (0..bones.len()).min_by(|a, b| seg_dist(*p3, bones[*a].pos, bones[*a].tip).total_cmp(&seg_dist(*p3, bones[*b].pos, bones[*b].tip))).unwrap_or(0);
            match acc.iter_mut().find(|a| a.0 == near) {
                Some(a) => a.1 += 1.0 - sum,
                None => acc.push((near, 1.0 - sum)),
            }
        }
        // a hand's weight is shared between the palm and the fingers
        for (si, left) in [true, false].iter().enumerate() {
            let hand = if *left { "leftHand" } else { "rightHand" };
            if let (Some(fr), Some(&hi)) = (&frames[si], slot.get(hand)) {
                if let Some(pos) = acc.iter().position(|a| a.0 == hi) {
                    let wh = acc[pos].1;
                    acc.remove(pos);
                    for (n, share) in fr.split([p3[0], p3[1]], *left) {
                        if let Some(&bi) = slot.get(n.as_str()) {
                            match acc.iter_mut().find(|a| a.0 == bi) {
                                Some(a) => a.1 += wh * share,
                                None => acc.push((bi, wh * share)),
                            }
                        }
                    }
                }
            }
        }
        raw.push(acc);
    }
    // The figure is one picture, so an arm and the chest next to it are one piece of mesh and would stretch across the seam
    // when the arm swings. Cut the mesh into pieces by the bones: every triangle belongs to the arm, leg or body it mostly
    // follows, and each piece gets its own copy of the points on the seam (the same place at rest, so no crack, but free to
    // move apart).
    let bone_group: Vec<usize> = bones.iter().map(|b| limb_group(&b.name)).collect();
    let share: Vec<[f32; 5]> = raw
        .iter()
        .map(|acc| {
            let mut g = [0.0f32; 5];
            for (b, x) in acc {
                g[bone_group[*b]] += x;
            }
            g
        })
        .collect();
    let tri_group: Vec<usize> = tris
        .iter()
        .map(|t| {
            let mut g = [0.0f32; 5];
            for i in t {
                for k in 0..5 {
                    g[k] += share[*i as usize][k];
                }
            }
            (0..5).max_by(|a, b| g[*a].total_cmp(&g[*b])).unwrap_or(0)
        })
        .collect();
    let mut copy_of: HashMap<(u32, usize), u32> = HashMap::new();
    let mut piece_verts: Vec<(u32, usize)> = Vec::new();
    let mut piece_tris: Vec<[u32; 3]> = Vec::new();
    for (t, g) in tris.iter().zip(&tri_group) {
        let mut n = [0u32; 3];
        for k in 0..3 {
            n[k] = *copy_of.entry((t[k], *g)).or_insert_with(|| {
                piece_verts.push((t[k], *g));
                (piece_verts.len() - 1) as u32
            });
        }
        piece_tris.push(n);
    }
    let (verts, uvs, normals): (Vec<[f32; 3]>, Vec<[f32; 2]>, Vec<[f32; 3]>) = (
        piece_verts.iter().map(|(i, _)| verts[*i as usize]).collect(),
        piece_verts.iter().map(|(i, _)| uvs[*i as usize]).collect(),
        piece_verts.iter().map(|(i, _)| normals[*i as usize]).collect(),
    );
    let tris = piece_tris;
    let (mut joints, mut weights) = (Vec::with_capacity(verts.len()), Vec::with_capacity(verts.len()));
    for (orig, group) in &piece_verts {
        // only the bones of the piece (the arm bones for an arm, the body bones for the body)
        let mut acc: Vec<(usize, f32)> = raw[*orig as usize].iter().filter(|(b, _)| bone_group[*b] == *group).cloned().collect();
        if acc.iter().map(|a| a.1).sum::<f32>() < 0.001 {
            let first = (0..bones.len()).find(|b| bone_group[*b] == *group).unwrap_or(0);
            acc = vec![(first, 1.0)];
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
    let part = Part3d { name: "Body".to_string(), layer: sk.layers.first().copied().unwrap_or(0), verts, normals, uvs, tris, joints, weights, image, morphs: Vec::new() };
    let mut model = Model3d { name: opt.name.clone(), author: opt.author.clone(), parts: vec![part], bones, expressions: Vec::new(), springs: Vec::new(), colliders: Vec::new(), scale };
    normalize_to_t_pose(&mut model);
    Some(model)
}

/// Which piece of the body a bone belongs to: 0 the body, 1 the left arm, 2 the right arm, 3 the left leg, 4 the right leg.
fn limb_group(name: &str) -> usize {
    let finger = ["Thumb", "Index", "Middle", "Ring", "Little"].iter().any(|f| name.contains(f));
    if finger {
        return if name.starts_with("left") { 1 } else { 2 };
    }
    match name {
        "leftUpperArm" | "leftLowerArm" | "leftHand" => 1,
        "rightUpperArm" | "rightLowerArm" | "rightHand" => 2,
        "leftUpperLeg" | "leftLowerLeg" | "leftFoot" | "leftToes" => 3,
        "rightUpperLeg" | "rightLowerLeg" | "rightFoot" | "rightToes" => 4,
        _ => 0,
    }
}

/// Games and VRM apps expect the rest pose to be a T pose: arms straight out sideways, legs straight down. Whatever way the
/// figure was drawn, turn the arm bones about the shoulders and the leg bones about the hips into that pose, and carry the
/// parts that follow them (by their skin weights). A pose from tracking or an idle animation then starts from where the
/// app thinks it does, so arms do not come out raised or folded.
pub fn normalize_to_t_pose(m: &mut Model3d) {
    // (the bones turned, the joint they turn about, the joint that should end up in the target direction, the direction)
    let jobs: [(&[&str], &str, &str, [f32; 2]); 4] = [
        (&["leftUpperArm", "leftLowerArm", "leftHand", "@leftfingers"], "leftUpperArm", "leftHand", [1.0, 0.0]),
        (&["rightUpperArm", "rightLowerArm", "rightHand", "@rightfingers"], "rightUpperArm", "rightHand", [-1.0, 0.0]),
        (&["leftUpperLeg", "leftLowerLeg", "leftFoot", "leftToes"], "leftUpperLeg", "leftFoot", [0.0, -1.0]),
        (&["rightUpperLeg", "rightLowerLeg", "rightFoot", "rightToes"], "rightUpperLeg", "rightFoot", [0.0, -1.0]),
    ];
    // for every bone: the turn (angle, about which point), or none
    let mut turn: Vec<Option<(f32, [f32; 3])>> = vec![None; m.bones.len()];
    for (names, about, to, dir) in jobs {
        let (Some(a), Some(b)) = (m.bone(about), m.bone(to)) else { continue };
        let (pa, pb) = (m.bones[a].pos, m.bones[b].pos);
        let v = [pb[0] - pa[0], pb[1] - pa[1]];
        if v[0].hypot(v[1]) < 1e-4 {
            continue;
        }
        let ang = dir[1].atan2(dir[0]) - v[1].atan2(v[0]);
        let ang = (ang + std::f32::consts::PI).rem_euclid(std::f32::consts::TAU) - std::f32::consts::PI;
        if ang.abs() < 1e-3 {
            continue;
        }
        for n in names {
            if let Some(side) = n.strip_prefix('@').and_then(|s| s.strip_suffix("fingers")) {
                for (i, b) in m.bones.iter().enumerate() {
                    if b.name.starts_with(side) && ["Thumb", "Index", "Middle", "Ring", "Little"].iter().any(|f| b.name.contains(f)) {
                        turn[i] = Some((ang, pa));
                    }
                }
            } else if let Some(i) = m.bone(n) {
                turn[i] = Some((ang, pa));
            }
        }
    }
    if turn.iter().all(|t| t.is_none()) {
        return;
    }
    let rot = |t: &(f32, [f32; 3]), p: [f32; 3]| -> [f32; 3] {
        let (s, c) = t.0.sin_cos();
        let (x, y) = (p[0] - t.1[0], p[1] - t.1[1]);
        [t.1[0] + x * c - y * s, t.1[1] + x * s + y * c, p[2]]
    };
    for part in &mut m.parts {
        for i in 0..part.verts.len() {
            let v = part.verts[i];
            let mut acc = [0.0f32; 3];
            for k in 0..4 {
                let w = part.weights[i][k];
                if w <= 0.0 {
                    continue;
                }
                let q = match &turn[part.joints[i][k] as usize] {
                    Some(t) => rot(t, v),
                    None => v,
                };
                for a in 0..3 {
                    acc[a] += q[a] * w;
                }
            }
            part.verts[i] = acc;
            // the normals turn the same way (by the main bone)
            let main = (0..4).max_by(|a, b| part.weights[i][*a].total_cmp(&part.weights[i][*b])).unwrap_or(0);
            if let Some((ang, _)) = &turn[part.joints[i][main] as usize] {
                let (s, c) = ang.sin_cos();
                let n = part.normals[i];
                part.normals[i] = [n[0] * c - n[1] * s, n[0] * s + n[1] * c, n[2]];
            }
        }
    }
    for i in 0..m.bones.len() {
        if let Some(t) = &turn[i] {
            let (p, q) = (m.bones[i].pos, m.bones[i].tip);
            m.bones[i].pos = rot(t, p);
            m.bones[i].tip = rot(t, q);
        }
    }
}

/// What an expression sets: (words that find the slider, "far" = the end away from the default, or a fraction of that).
type Setting = (&'static [&'static str], f32);

/// The expressions the face sliders of the rig can make: (name, sliders).
const EXPRESSIONS: &[(&str, &[Setting])] = &[
    ("blink", &[(&["eyes", "open"], 1.0)]),
    ("aa", &[(&["mouth", "a"], 1.0)]),
    ("ih", &[(&["mouth", "i"], 1.0)]),
    ("ou", &[(&["mouth", "u"], 1.0)]),
    ("ee", &[(&["mouth", "e"], 1.0)]),
    ("oh", &[(&["mouth", "o"], 1.0)]),
    ("happy", &[(&["eyes", "smile"], 1.0), (&["mouth", "form"], 1.0)]),
    ("relaxed", &[(&["eyes", "smile"], 0.5), (&["mouth", "form"], 0.5)]),
    ("sad", &[(&["mouth", "form"], -1.0)]),
    ("surprised", &[(&["brows"], 1.0), (&["mouth", "open"], 0.6)]),
    ("jawOpen", &[(&["mouth", "open"], 1.0)]),
    ("browInnerUp", &[(&["brows"], 1.0)]),
];

/// ARKit names that the same shapes also answer to (for face tracking): (ARKit name, our expression).
pub const ARKIT_ALIASES: &[(&str, &str)] = &[
    ("eyeBlinkLeft", "blinkLeft"),
    ("eyeBlinkRight", "blinkRight"),
    ("jawOpen", "jawOpen"),
    ("mouthSmileLeft", "happy"),
    ("mouthSmileRight", "happy"),
    ("mouthFrownLeft", "sad"),
    ("mouthFrownRight", "sad"),
    ("browInnerUp", "browInnerUp"),
    ("eyeLookUpLeft", "lookUp"),
    ("eyeLookUpRight", "lookUp"),
    ("eyeLookDownLeft", "lookDown"),
    ("eyeLookDownRight", "lookDown"),
];

fn make_expressions(model: &mut Model3d, rig: &Rig, index_of_mesh: &HashMap<usize, usize>, scale: f32) {
    if rig.params.is_empty() {
        return;
    }
    let rest_pose = rig.pose(&Values::default());
    let rest: HashMap<usize, Vec<[f32; 2]>> = index_of_mesh.keys().map(|mi| (*mi, rig.deformed(&rig.meshes[*mi], &rest_pose))).collect();
    // how a set of slider values moves every part: the shifts in metres
    let shifts = |vals: &Values| -> HashMap<usize, Vec<[f32; 3]>> {
        let pose = rig.pose(vals);
        let mut out = HashMap::new();
        for (mi, pi) in index_of_mesh {
            let now = rig.deformed(&rig.meshes[*mi], &pose);
            let d: Vec<[f32; 3]> = now.iter().zip(&rest[mi]).map(|(a, b)| [(a[0] - b[0]) * scale, -(a[1] - b[1]) * scale, 0.0]).collect();
            if d.iter().any(|v| v[0].abs().max(v[1].abs()) > 0.0004) {
                out.insert(*pi, d);
            }
        }
        out
    };
    let mut made: Vec<(String, HashMap<usize, Vec<[f32; 3]>>)> = Vec::new();
    for (name, sets) in EXPRESSIONS {
        let mut vals = Values::default();
        let mut found = 0;
        for (words, amount) in *sets {
            let Some(id) = find_param(rig, words) else { continue };
            let Some(p) = rig.param(id) else { continue };
            let far = far_end(rig, id);
            // a negative amount goes the other way round the default
            let towards = if *amount >= 0.0 { far } else { p.default - (far - p.default) };
            let v = p.default + (towards - p.default) * amount.abs().min(1.0);
            vals.set(id, v.clamp(p.min, p.max));
            found += 1;
        }
        if found == 0 {
            continue;
        }
        let d = shifts(&vals);
        if !d.is_empty() {
            made.push((name.to_string(), d));
        }
    }
    // where the eyes look: named by the way the eyeball parts really move (character's left is +X)
    for (words, names) in [(&["eyeball", "x"], ["lookRight", "lookLeft"]), (&["eyeball", "y"], ["lookDown", "lookUp"])] {
        let Some(id) = find_param(rig, words) else { continue };
        let Some(p) = rig.param(id) else { continue };
        for end in [p.max, p.min] {
            let mut vals = Values::default();
            vals.set(id, end);
            let d = shifts(&vals);
            if d.is_empty() {
                continue;
            }
            let axis = if words[1] == "x" { 0 } else { 1 };
            let (sum, n): (f32, usize) = d.values().flat_map(|v| v.iter().map(move |x| x[axis])).fold((0.0, 0), |a, x| if x.abs() > 1e-5 { (a.0 + x, a.1 + 1) } else { a });
            let name = if sum > 0.0 { names[1] } else { names[0] };
            let _ = n;
            made.push((name.to_string(), d));
        }
    }
    // each eye on its own: the sliders of one eye move the parts on one side only
    for p in rig.params.iter().filter(|p| {
        let n = p.name.to_lowercase();
        n.contains("eye") && n.contains("open") && !n.contains("eyes")
    }) {
        let mut vals = Values::default();
        vals.set(p.id, far_end(rig, p.id));
        let d = shifts(&vals);
        if d.is_empty() {
            continue;
        }
        let (mut sx, mut w) = (0.0f32, 0.0f32);
        for (pi, ds) in &d {
            for (v, dv) in model.parts[*pi].verts.iter().zip(ds) {
                let m = dv[0].abs() + dv[1].abs();
                sx += v[0] * m;
                w += m;
            }
        }
        let left = w > 0.0 && sx / w > 0.0;
        made.push((if left { "blinkLeft" } else { "blinkRight" }.to_string(), d));
    }
    // store the shapes in the parts and bind the expressions
    for (name, d) in made {
        if model.expressions.iter().any(|e| e.name == name) {
            continue;
        }
        let mut binds = Vec::new();
        for (pi, deltas) in d {
            let mi = model.parts[pi].morphs.len();
            model.parts[pi].morphs.push(Morph { name: name.clone(), deltas });
            binds.push((pi, mi, 1.0));
        }
        model.expressions.push(Expression { name, binds });
    }
}

