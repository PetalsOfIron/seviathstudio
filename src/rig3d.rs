// SPDX-License-Identifier: GPL-3.0-or-later
// Rigging a 3D mesh by hand, the way Blender does it, without any AI: bones that are drawn in the picture of the mesh (a head, a tail, a parent),
// weights (how much each bone moves each point of the mesh) that are given by an envelope round the bone or painted with a brush, cleaned up
// (at most four bones for a point, adding up to one) and smoothed, a pose (a turn for every bone) that moves the mesh, and the file for a game.
// Everything here is plain numbers, so it is tested without a screen (`rig3d_tests`).

use std::collections::HashMap;

use crate::motion::euler_apply;

/// One bone: it turns about its head; its tail only shows where it points (and how long it is).
#[derive(Clone, Debug, PartialEq)]
pub struct Bone3 {
    pub name: String,
    pub parent: Option<usize>,
    pub head: [f32; 3],
    pub tail: [f32; 3],
}

/// How a brush changes the weights.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Paint {
    /// More of this bone.
    Add,
    /// Less of this bone.
    Subtract,
    /// The weights of a point become more like those of the points next to it.
    Smooth,
}

/// The turn of every bone (x, y and z in degrees, as the animations give them) and how far the whole figure is moved.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Pose {
    pub rot: Vec<[f32; 3]>,
    pub offset: [f32; 3],
}

impl Pose {
    /// The pose of an animation frame for a rig whose bones have the names of the animations (the others stay still). `relative`: the figure is built
    /// with the arms down, so the frame is taken as turns from the relaxed stand.
    pub fn from_frame(rig: &Rig3, frame: &crate::motion::Frame, relative: bool) -> Pose {
        let f = if relative { frame.minus_rest() } else { frame.clone() };
        let rot = rig.bones.iter().map(|b| crate::motion::BONES.iter().find(|n| n.eq_ignore_ascii_case(&b.name)).map(|n| f.get(n)).unwrap_or([0.0; 3])).collect();
        Pose { rot, offset: f.hips }
    }
}

/// The bones of a mesh and the weights of its points.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Rig3 {
    pub bones: Vec<Bone3>,
    /// For every point of the mesh: the bones that move it and how much each does (at most four, adding up to one; nothing is a point that no
    /// bone moves).
    pub weights: Vec<Vec<(u16, f32)>>,
}

fn sub(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}

fn dot(a: [f32; 3], b: [f32; 3]) -> f32 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

/// The distance from a point to the segment of a bone (head to tail), and how far along the bone the nearest place is (0 head, 1 tail).
/// Are two places the same place (to a tenth of a millimetre)?
fn near(a: [f32; 3], b: [f32; 3]) -> bool {
    dot(sub(a, b), sub(a, b)) < 1e-8
}

pub fn distance_to_bone(p: [f32; 3], head: [f32; 3], tail: [f32; 3]) -> (f32, f32) {
    let ab = sub(tail, head);
    let len2 = dot(ab, ab).max(1e-12);
    let t = (dot(sub(p, head), ab) / len2).clamp(0.0, 1.0);
    let q = [head[0] + ab[0] * t, head[1] + ab[1] * t, head[2] + ab[2] * t];
    (dot(sub(p, q), sub(p, q)).sqrt(), t)
}

/// Left becomes right and right left in a name ("leftArm" and "Arm.L", "Arm_R" and so on); a name without a side is returned as it is.
pub fn mirrored_name(name: &str) -> String {
    let swap = |s: &str, a: &str, b: &str| -> Option<String> {
        if let Some(r) = s.strip_prefix(a) {
            return Some(format!("{b}{r}"));
        }
        if let Some(r) = s.strip_prefix(&a.to_lowercase()) {
            return Some(format!("{}{r}", b.to_lowercase()));
        }
        None
    };
    for (a, b) in [("Left", "Right"), ("Right", "Left")] {
        if let Some(n) = swap(name, a, b) {
            return n;
        }
    }
    for (a, b) in [(".L", ".R"), (".R", ".L"), ("_L", "_R"), ("_R", "_L"), ("-L", "-R"), ("-R", "-L")] {
        if let Some(r) = name.strip_suffix(a) {
            return format!("{r}{b}");
        }
    }
    name.to_owned()
}

impl Rig3 {
    pub fn new(points: usize) -> Rig3 {
        Rig3 { bones: Vec::new(), weights: vec![Vec::new(); points] }
    }

    pub fn add_bone(&mut self, name: &str, parent: Option<usize>, head: [f32; 3], tail: [f32; 3]) -> usize {
        let parent = parent.filter(|p| *p < self.bones.len());
        self.bones.push(Bone3 { name: name.chars().filter(|c| !c.is_control()).take(40).collect(), parent, head, tail });
        self.bones.len() - 1
    }

    /// Move where a bone starts. The tail of its parent, when it was joined to it there, goes along (the chain stays whole).
    pub fn move_head(&mut self, i: usize, to: [f32; 3]) {
        if i >= self.bones.len() {
            return;
        }
        let old = self.bones[i].head;
        if let Some(p) = self.bones[i].parent {
            if near(self.bones[p].tail, old) {
                self.bones[p].tail = to;
            }
        }
        self.bones[i].head = to;
    }

    /// Move where a bone ends. The bones that start there (the ones that were joined to it) go along.
    pub fn move_tail(&mut self, i: usize, to: [f32; 3]) {
        if i >= self.bones.len() {
            return;
        }
        let old = self.bones[i].tail;
        for c in self.children(i) {
            if near(self.bones[c].head, old) {
                self.bones[c].head = to;
            }
        }
        self.bones[i].tail = to;
    }

    /// Move a whole bone and the bones below it (what Blender does with a grab of a bone and its children).
    pub fn move_with_children(&mut self, i: usize, by: [f32; 3]) {
        if i >= self.bones.len() {
            return;
        }
        let mut stack = vec![i];
        let mut seen = vec![false; self.bones.len()];
        while let Some(b) = stack.pop() {
            if std::mem::replace(&mut seen[b], true) {
                continue;
            }
            for k in 0..3 {
                self.bones[b].head[k] += by[k];
                self.bones[b].tail[k] += by[k];
            }
            stack.extend(self.children(b));
        }
    }

    /// The bones of a person, with the names the animations use, placed by the size of the mesh (feet on the floor at `lo[1]`, the front towards +z, the
    /// character's left towards +x), the arms down at a slant (an A-pose). A start to move into place, not an answer: every joint is moved by hand.
    pub fn humanoid(points: usize, lo: [f32; 3], hi: [f32; 3]) -> Rig3 {
        let h = (hi[1] - lo[1]).max(1e-3);
        let cx = (lo[0] + hi[0]) / 2.0;
        let cz = (lo[2] + hi[2]) / 2.0;
        let at = |x: f32, y: f32, z: f32| -> [f32; 3] { [cx + x * h, lo[1] + y * h, cz + z * h] };
        let mut r = Rig3::new(points);
        let hips = r.add_bone("hips", None, at(0.0, 0.53, 0.0), at(0.0, 0.58, 0.0));
        let spine = r.add_bone("spine", Some(hips), at(0.0, 0.58, 0.0), at(0.0, 0.66, 0.0));
        let chest = r.add_bone("chest", Some(spine), at(0.0, 0.66, 0.0), at(0.0, 0.72, 0.0));
        let upper = r.add_bone("upperChest", Some(chest), at(0.0, 0.72, 0.0), at(0.0, 0.79, 0.0));
        let neck = r.add_bone("neck", Some(upper), at(0.0, 0.79, 0.0), at(0.0, 0.84, 0.0));
        r.add_bone("head", Some(neck), at(0.0, 0.84, 0.0), at(0.0, 0.98, 0.0));
        for (side, sx) in [("left", 1.0f32), ("right", -1.0)] {
            let shoulder = r.add_bone(&format!("{side}Shoulder"), Some(upper), at(0.0, 0.77, 0.0), at(sx * 0.06, 0.77, 0.0));
            let arm = r.add_bone(&format!("{side}UpperArm"), Some(shoulder), at(sx * 0.06, 0.77, 0.0), at(sx * 0.16, 0.65, 0.0));
            let fore = r.add_bone(&format!("{side}LowerArm"), Some(arm), at(sx * 0.16, 0.65, 0.0), at(sx * 0.25, 0.54, 0.0));
            r.add_bone(&format!("{side}Hand"), Some(fore), at(sx * 0.25, 0.54, 0.0), at(sx * 0.29, 0.48, 0.0));
            let leg = r.add_bone(&format!("{side}UpperLeg"), Some(hips), at(sx * 0.05, 0.52, 0.0), at(sx * 0.05, 0.28, 0.0));
            let shin = r.add_bone(&format!("{side}LowerLeg"), Some(leg), at(sx * 0.05, 0.28, 0.0), at(sx * 0.05, 0.05, 0.0));
            let foot = r.add_bone(&format!("{side}Foot"), Some(shin), at(sx * 0.05, 0.05, 0.0), at(sx * 0.05, 0.0, 0.06));
            r.add_bone(&format!("{side}Toes"), Some(foot), at(sx * 0.05, 0.0, 0.06), at(sx * 0.05, 0.0, 0.1));
        }
        r
    }

    pub fn children(&self, i: usize) -> Vec<usize> {
        (0..self.bones.len()).filter(|k| self.bones[*k].parent == Some(i)).collect()
    }

    /// Is `a` the same bone as `b`, or somewhere above it.
    fn is_above(&self, a: usize, b: usize) -> bool {
        let mut at = Some(b);
        let mut steps = 0;
        while let Some(i) = at {
            if i == a {
                return true;
            }
            at = self.bones.get(i).and_then(|x| x.parent);
            steps += 1;
            if steps > self.bones.len() + 1 {
                return false;
            }
        }
        false
    }

    /// Give a bone another parent (or none). Not allowed when it would make a circle. Returns whether it was done.
    pub fn set_parent(&mut self, bone: usize, parent: Option<usize>) -> bool {
        if bone >= self.bones.len() || parent.is_some_and(|p| p >= self.bones.len() || self.is_above(bone, p)) {
            return false;
        }
        self.bones[bone].parent = parent;
        true
    }

    /// Take a bone away: its children go to its parent, and what it moved goes to its parent too (or to nothing when it had none).
    pub fn delete_bone(&mut self, i: usize) {
        if i >= self.bones.len() {
            return;
        }
        let up = self.bones[i].parent;
        for b in &mut self.bones {
            if b.parent == Some(i) {
                b.parent = up;
            }
        }
        let remap = |k: usize| if k > i { k - 1 } else { k };
        for b in &mut self.bones {
            b.parent = b.parent.map(remap);
        }
        for w in &mut self.weights {
            let mut moved: Vec<(u16, f32)> = Vec::new();
            for (b, x) in w.iter() {
                let b = *b as usize;
                let target = if b == i { up } else { Some(b) };
                if let Some(t) = target {
                    let t = remap(t) as u16;
                    match moved.iter_mut().find(|m| m.0 == t) {
                        Some(m) => m.1 += x,
                        None => moved.push((t, *x)),
                    }
                }
            }
            *w = moved;
        }
        self.bones.remove(i);
    }

    /// A bone on the other side of the middle (x becomes -x), with the name turned round, under the mirror of the parent when it has one.
    pub fn mirror_bone(&mut self, i: usize) -> Option<usize> {
        let b = self.bones.get(i)?.clone();
        let parent = b.parent.map(|p| {
            let pn = mirrored_name(&self.bones[p].name);
            self.bones.iter().position(|o| o.name == pn && pn != self.bones[p].name).unwrap_or(p)
        });
        let name = mirrored_name(&b.name);
        if name == b.name {
            return None;
        }
        Some(self.add_bone(&name, parent, [-b.head[0], b.head[1], b.head[2]], [-b.tail[0], b.tail[1], b.tail[2]]))
    }

    /// The weights of one bone for a point.
    pub fn weight_of(&self, point: usize, bone: usize) -> f32 {
        self.weights.get(point).and_then(|w| w.iter().find(|x| x.0 as usize == bone)).map(|x| x.1).unwrap_or(0.0)
    }

    fn set_weight(&mut self, point: usize, bone: usize, value: f32) {
        let w = &mut self.weights[point];
        match w.iter_mut().find(|x| x.0 as usize == bone) {
            Some(x) => x.1 = value.max(0.0),
            None => {
                if value > 0.0 {
                    w.push((bone as u16, value));
                }
            }
        }
    }

    /// Weights from the distance to a bone: full inside `inner`, none beyond `outer`, a smooth fall between. The bone's old weights are replaced.
    pub fn assign_envelope(&mut self, verts: &[[f32; 3]], bone: usize, inner: f32, outer: f32) {
        if bone >= self.bones.len() {
            return;
        }
        let (h, t) = (self.bones[bone].head, self.bones[bone].tail);
        for (i, p) in verts.iter().enumerate().take(self.weights.len()) {
            let (d, _) = distance_to_bone(*p, h, t);
            let w = if d <= inner { 1.0 } else if d >= outer { 0.0 } else {
                let u = 1.0 - (d - inner) / (outer - inner).max(1e-6);
                u * u * (3.0 - 2.0 * u)
            };
            self.set_weight(i, bone, w);
        }
        self.limit(4);
    }

    /// Every point is moved by the nearest bones (the bones with a weight to share by their distance). A start to work from.
    pub fn assign_by_distance(&mut self, verts: &[[f32; 3]]) {
        if self.bones.is_empty() {
            return;
        }
        for (i, p) in verts.iter().enumerate().take(self.weights.len()) {
            let mut d: Vec<(usize, f32)> = self.bones.iter().enumerate().map(|(k, b)| (k, distance_to_bone(*p, b.head, b.tail).0)).collect();
            d.sort_by(|a, b| a.1.total_cmp(&b.1));
            let nearest = d[0].1.max(1e-4);
            self.weights[i] = d.iter().take(4).filter(|x| x.1 < nearest * 1.8 + 0.01).map(|x| (x.0 as u16, 1.0 / (x.1 + nearest * 0.25).powi(3))).collect();
        }
        self.normalize(4);
    }

    /// A brush at a place on the mesh: the points within `radius` change by `strength` (0 to 1) times how near they are to the middle of the brush.
    pub fn paint(&mut self, verts: &[[f32; 3]], near: &[Vec<u32>], bone: usize, center: [f32; 3], radius: f32, strength: f32, mode: Paint) {
        if bone >= self.bones.len() || radius <= 0.0 {
            return;
        }
        let touched: Vec<(usize, f32)> = verts
            .iter()
            .enumerate()
            .take(self.weights.len())
            .filter_map(|(i, p)| {
                let d = dot(sub(*p, center), sub(*p, center)).sqrt();
                (d < radius).then(|| (i, { let u = 1.0 - d / radius; u * u * (3.0 - 2.0 * u) }))
            })
            .collect();
        self.paint_points(near, bone, &touched, strength, mode);
    }

    /// The same brush for points that were picked some other way (on the screen): each comes with how much of the brush it gets (0 to 1).
    pub fn paint_points(&mut self, near: &[Vec<u32>], bone: usize, touched: &[(usize, f32)], strength: f32, mode: Paint) {
        if bone >= self.bones.len() {
            return;
        }
        match mode {
            Paint::Add | Paint::Subtract => {
                let sign = if mode == Paint::Add { 1.0 } else { -1.0 };
                for &(i, f) in touched {
                    let now = self.weight_of(i, bone);
                    self.set_weight(i, bone, (now + sign * strength * f).clamp(0.0, 1.0));
                }
                self.normalize(4);
            }
            Paint::Smooth => {
                // each touched point moves a part of the way to the average of its neighbours
                let old = self.weights.clone();
                for &(i, f) in touched {
                    let nb = near.get(i).map(|n| n.as_slice()).unwrap_or(&[]);
                    if nb.is_empty() {
                        continue;
                    }
                    let mut avg: HashMap<u16, f32> = HashMap::new();
                    for j in nb {
                        for (b, x) in &old[*j as usize] {
                            *avg.entry(*b).or_default() += x / nb.len() as f32;
                        }
                    }
                    let k = (strength * f).clamp(0.0, 1.0);
                    let mut mixed: HashMap<u16, f32> = HashMap::new();
                    for (b, x) in &old[i] {
                        *mixed.entry(*b).or_default() += x * (1.0 - k);
                    }
                    for (b, x) in avg {
                        *mixed.entry(b).or_default() += x * k;
                    }
                    self.weights[i] = mixed.into_iter().collect();
                    self.weights[i].sort_by(|a, b| a.0.cmp(&b.0));
                }
                self.normalize(4);
            }
        }
    }

    /// Take one bone's weights away from every point.
    pub fn clear_bone(&mut self, bone: usize) {
        for w in &mut self.weights {
            w.retain(|x| x.0 as usize != bone);
        }
        self.normalize(4);
    }

    /// At most `keep` bones for a point (the biggest shares), and shares that add up to more than one are made to add up to one. A smaller sum is
    /// kept as it is (an envelope can give a part of a point to a bone); `normalize` and the skinning make it one.
    pub fn limit(&mut self, keep: usize) {
        for w in &mut self.weights {
            w.retain(|x| x.1 > 1e-4 && x.1.is_finite());
            w.sort_by(|a, b| b.1.total_cmp(&a.1));
            w.truncate(keep.max(1));
            let sum: f32 = w.iter().map(|x| x.1).sum();
            if sum > 1.0 {
                w.iter_mut().for_each(|x| x.1 /= sum);
            }
        }
    }

    /// At most `keep` bones for a point (the biggest shares), the shares adding up to one; a point with nothing stays with nothing.
    pub fn normalize(&mut self, keep: usize) {
        for w in &mut self.weights {
            w.retain(|x| x.1 > 1e-4 && x.1.is_finite());
            w.sort_by(|a, b| b.1.total_cmp(&a.1));
            w.truncate(keep.max(1));
            let sum: f32 = w.iter().map(|x| x.1).sum();
            if sum > 1e-9 {
                w.iter_mut().for_each(|x| x.1 /= sum);
            }
        }
    }

    /// How many points no bone moves.
    pub fn unweighted(&self) -> usize {
        self.weights.iter().filter(|w| w.is_empty()).count()
    }

    /// The transform of every bone for a pose: a point of the rest mesh goes to `m * p + t` for `(m, t)` of its bone. A bone turns about its head, in the
    /// frame of its parent, after the parent has moved it; the figure as a whole is moved by the offset.
    fn matrices(&self, pose: &Pose) -> Vec<([[f32; 3]; 3], [f32; 3])> {
        type Affine = ([[f32; 3]; 3], [f32; 3]);
        let id: [[f32; 3]; 3] = [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]];
        fn rot_matrix(r: [f32; 3]) -> [[f32; 3]; 3] {
            // the columns are what the axes become
            let c0 = euler_apply(r, [1.0, 0.0, 0.0]);
            let c1 = euler_apply(r, [0.0, 1.0, 0.0]);
            let c2 = euler_apply(r, [0.0, 0.0, 1.0]);
            [[c0[0], c1[0], c2[0]], [c0[1], c1[1], c2[1]], [c0[2], c1[2], c2[2]]]
        }
        fn mul(a: &[[f32; 3]; 3], b: &[[f32; 3]; 3]) -> [[f32; 3]; 3] {
            let mut m = [[0.0; 3]; 3];
            for i in 0..3 {
                for j in 0..3 {
                    m[i][j] = (0..3).map(|k| a[i][k] * b[k][j]).sum();
                }
            }
            m
        }
        fn apply(m: &[[f32; 3]; 3], p: [f32; 3]) -> [f32; 3] {
            [m[0][0] * p[0] + m[0][1] * p[1] + m[0][2] * p[2], m[1][0] * p[0] + m[1][1] * p[1] + m[1][2] * p[2], m[2][0] * p[0] + m[2][1] * p[1] + m[2][2] * p[2]]
        }
        fn make(rig: &Rig3, pose: &Pose, b: usize, done: &mut Vec<Option<Affine>>, depth: usize) -> Affine {
            if let Some(a) = done[b] {
                return a;
            }
            let id: [[f32; 3]; 3] = [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]];
            let (pm, pt) = match rig.bones[b].parent {
                Some(p) if depth < rig.bones.len() => make(rig, pose, p, done, depth + 1),
                _ => (id, pose.offset),
            };
            let r = rot_matrix(pose.rot.get(b).copied().unwrap_or([0.0; 3]));
            let h = rig.bones[b].head;
            let rh = apply(&r, h);
            let local_t = [h[0] - rh[0], h[1] - rh[1], h[2] - rh[2]];
            let t0 = apply(&pm, local_t);
            let a: Affine = (mul(&pm, &r), [t0[0] + pt[0], t0[1] + pt[1], t0[2] + pt[2]]);
            done[b] = Some(a);
            a
        }
        let mut done: Vec<Option<Affine>> = vec![None; self.bones.len()];
        (0..self.bones.len()).map(|b| make(self, pose, b, &mut done, 0)).collect::<Vec<_>>().into_iter().map(|a| if a.0 == [[0.0; 3]; 3] { (id, pose.offset) } else { a }).collect()
    }

    /// Where the head and the tail of every bone are in a pose.
    pub fn posed_bones(&self, pose: &Pose) -> Vec<([f32; 3], [f32; 3])> {
        let mats = self.matrices(pose);
        let at = |m: &([[f32; 3]; 3], [f32; 3]), p: [f32; 3]| -> [f32; 3] {
            let (r, t) = m;
            [r[0][0] * p[0] + r[0][1] * p[1] + r[0][2] * p[2] + t[0], r[1][0] * p[0] + r[1][1] * p[1] + r[1][2] * p[2] + t[1], r[2][0] * p[0] + r[2][1] * p[1] + r[2][2] * p[2] + t[2]]
        };
        self.bones.iter().zip(&mats).map(|(b, m)| (at(m, b.head), at(m, b.tail))).collect()
    }

    /// The mesh as the pose moves it (linear blend skinning).
    pub fn skin(&self, verts: &[[f32; 3]], pose: &Pose) -> Vec<[f32; 3]> {
        if self.bones.is_empty() {
            return verts.to_vec();
        }
        let mats = self.matrices(pose);
        verts
            .iter()
            .enumerate()
            .map(|(i, p)| {
                let w = self.weights.get(i).map(|w| w.as_slice()).unwrap_or(&[]);
                if w.is_empty() {
                    return [p[0] + pose.offset[0], p[1] + pose.offset[1], p[2] + pose.offset[2]];
                }
                let sum: f32 = w.iter().map(|x| x.1).sum::<f32>().max(1e-9);
                let mut acc = [0.0f32; 3];
                for (b, x) in w {
                    let (m, t) = &mats[*b as usize];
                    for r in 0..3 {
                        acc[r] += x / sum * (m[r][0] * p[0] + m[r][1] * p[1] + m[r][2] * p[2] + t[r]);
                    }
                }
                acc
            })
            .collect()
    }
}

/// For every point the points that share a triangle with it (the points that are the same place count as one, so a seam of the picture does not
/// cut the surface in two): what smoothing uses.
pub fn neighbours(verts: &[[f32; 3]], tris: &[[u32; 3]]) -> Vec<Vec<u32>> {
    let key = |p: [f32; 3]| ((p[0] * 4000.0).round() as i32, (p[1] * 4000.0).round() as i32, (p[2] * 4000.0).round() as i32);
    let mut first: HashMap<(i32, i32, i32), u32> = HashMap::new();
    let rep: Vec<u32> = verts.iter().enumerate().map(|(i, p)| *first.entry(key(*p)).or_insert(i as u32)).collect();
    let mut by_place: Vec<Vec<u32>> = vec![Vec::new(); verts.len()];
    for t in tris {
        for k in 0..3 {
            let (a, b) = (rep[t[k] as usize], rep[t[(k + 1) % 3] as usize]);
            if a != b {
                by_place[a as usize].push(b);
                by_place[b as usize].push(a);
            }
        }
    }
    for n in &mut by_place {
        n.sort_unstable();
        n.dedup();
    }
    (0..verts.len()).map(|i| by_place[rep[i] as usize].clone()).collect()
}

#[cfg(test)]
mod rig3d_tests {
    use super::*;

    /// A strip of points along y from 0 to 2 (a leg): bones along it.
    fn leg() -> (Vec<[f32; 3]>, Rig3) {
        let verts: Vec<[f32; 3]> = (0..=20).map(|i| [0.0, i as f32 * 0.1, 0.0]).collect();
        let mut r = Rig3::new(verts.len());
        let hips = r.add_bone("hips", None, [0.0, 1.0, 0.0], [0.0, 1.1, 0.0]);
        let thigh = r.add_bone("leftUpperLeg", Some(hips), [0.0, 1.0, 0.0], [0.0, 0.5, 0.0]);
        let _calf = r.add_bone("leftLowerLeg", Some(thigh), [0.0, 0.5, 0.0], [0.0, 0.0, 0.0]);
        (verts, r)
    }

    #[test]
    fn the_parent_cannot_make_a_circle_and_a_deleted_bone_hands_its_work_to_its_parent() {
        let (verts, mut r) = leg();
        assert!(!r.set_parent(0, Some(2)), "the hips cannot be under the calf");
        assert!(!r.set_parent(1, Some(1)), "a bone cannot be its own parent");
        assert!(r.set_parent(2, Some(0)));
        r.assign_by_distance(&verts);
        let before = r.unweighted();
        r.delete_bone(1);
        assert_eq!(r.bones.len(), 2);
        assert_eq!(r.bones[1].parent, Some(0), "the calf went up to the hips");
        assert_eq!(r.bones[1].name, "leftLowerLeg");
        assert!(r.weights.iter().all(|w| w.iter().all(|x| (x.0 as usize) < 2)), "no weight points at a bone that is gone");
        assert_eq!(r.unweighted(), before, "what the thigh moved is moved by the hips now");
    }

    #[test]
    fn the_envelope_gives_full_weight_near_the_bone_and_none_far_away() {
        let (verts, mut r) = leg();
        r.assign_envelope(&verts, 2, 0.1, 0.4);
        assert!((r.weight_of(0, 2) - 1.0).abs() < 1e-5, "the foot is on the calf");
        assert_eq!(r.weight_of(20, 2), 0.0, "the top is far from the calf");
        let mid = r.weight_of(8, 2);
        assert!(mid > 0.0 && mid < 1.0, "the fall is smooth between: {mid}");
        assert!(r.weights.iter().all(|w| w.iter().map(|x| x.1).sum::<f32>() <= 1.0 + 1e-5 && w.len() <= 4));
    }

    #[test]
    fn every_point_follows_the_nearest_bones_and_the_weights_add_up_to_one() {
        let (verts, mut r) = leg();
        r.assign_by_distance(&verts);
        assert_eq!(r.unweighted(), 0);
        for w in &r.weights {
            assert!((w.iter().map(|x| x.1).sum::<f32>() - 1.0).abs() < 1e-4 && w.len() <= 4);
        }
        // the foot follows the calf and the hips follow the hips
        assert!(r.weight_of(0, 2) > 0.8 && r.weight_of(10, 0) > 0.3);
    }

    #[test]
    fn a_brush_adds_and_takes_away_and_smooths_and_keeps_the_rules() {
        let (verts, mut r) = leg();
        let tris: Vec<[u32; 3]> = (0..20u32).map(|i| [i, i + 1, i]).collect();
        let near = neighbours(&verts, &tris);
        r.assign_by_distance(&verts);
        let before = r.weight_of(14, 2);
        r.paint(&verts, &near, 2, verts[14], 0.3, 0.6, Paint::Add);
        assert!(r.weight_of(14, 2) > before + 0.2, "more calf where the brush went");
        assert_eq!(r.weight_of(2, 2), r.weight_of(2, 2));
        r.paint(&verts, &near, 2, verts[14], 0.3, 1.0, Paint::Subtract);
        assert!(r.weight_of(14, 2) < 0.2, "and less again");
        // smoothing makes a hard step between the calf and the thigh softer
        for w in r.weights.iter_mut() {
            w.clear();
        }
        for i in 0..=20 {
            r.weights[i] = vec![(if i < 10 { 2 } else { 1 }, 1.0)];
        }
        let step = (r.weight_of(9, 1) - r.weight_of(10, 1)).abs();
        r.paint(&verts, &near, 1, verts[10], 0.4, 1.0, Paint::Smooth);
        assert!((r.weight_of(9, 1) - r.weight_of(10, 1)).abs() < step, "the step is softer");
        for w in &r.weights {
            assert!(w.is_empty() || (w.iter().map(|x| x.1).sum::<f32>() - 1.0).abs() < 1e-4);
        }
        r.clear_bone(2);
        assert!(r.weights.iter().all(|w| w.iter().all(|x| x.0 != 2)));
    }

    #[test]
    fn turning_a_bone_moves_what_it_moves_about_its_head_and_the_rest_stays() {
        let (verts, mut r) = leg();
        // the foot follows the calf, the top the hips, the middle the thigh
        for i in 0..=20 {
            r.weights[i] = vec![(if i < 5 { 2 } else if i < 15 { 1 } else { 0 }, 1.0)];
        }
        let rest = r.skin(&verts, &Pose { rot: vec![[0.0; 3]; 3], offset: [0.0; 3] });
        for (a, b) in rest.iter().zip(&verts) {
            assert!((a[0] - b[0]).abs() < 1e-5 && (a[1] - b[1]).abs() < 1e-5, "the rest pose changes nothing");
        }
        // the thigh swings forward by 90 degrees (a negative x is forward for a hanging leg): the foot ends up in front, at the height of the knee line
        let pose = Pose { rot: vec![[0.0; 3], [-90.0, 0.0, 0.0], [0.0; 3]], offset: [0.0; 3] };
        let moved = r.skin(&verts, &pose);
        assert!(moved[0][2] > 0.8, "the foot is in front: {:?}", moved[0]);
        assert!((moved[0][1] - 1.0).abs() < 0.25, "and level with the hip: {:?}", moved[0]);
        assert!((moved[20][1] - 2.0).abs() < 0.2 && moved[20][2].abs() < 0.2, "the top does not move: {:?}", moved[20]);
        // a child turns with its parent: the hips turn and the whole leg goes with them
        let hips = Pose { rot: vec![[0.0, 0.0, 90.0], [0.0; 3], [0.0; 3]], offset: [0.0; 3] };
        let turned = r.skin(&verts, &hips);
        assert!(turned[0][0] > 0.8 && (turned[0][1] - 1.0).abs() < 0.1, "the foot swung out to the side (a positive z turns towards +x), level with the hip: {:?}", turned[0]);
        // the whole figure moved
        let up = r.skin(&verts, &Pose { rot: vec![[0.0; 3]; 3], offset: [0.0, 0.5, 0.0] });
        assert!((up[5][1] - 1.0).abs() < 1e-4);
    }

    #[test]
    fn a_bone_is_mirrored_with_its_name_turned_round() {
        let mut r = Rig3::new(0);
        let chest = r.add_bone("chest", None, [0.0, 1.4, 0.0], [0.0, 1.5, 0.0]);
        let a = r.add_bone("leftUpperArm", Some(chest), [0.2, 1.4, 0.0], [0.5, 1.4, 0.0]);
        let m = r.mirror_bone(a).unwrap();
        assert_eq!(r.bones[m].name, "rightUpperArm");
        assert_eq!(r.bones[m].head, [-0.2, 1.4, 0.0]);
        assert_eq!(r.bones[m].parent, Some(chest));
        assert_eq!(r.mirror_bone(chest), None, "the middle has no other side");
        for (a, b) in [("Arm.L", "Arm.R"), ("hand_R", "hand_L"), ("RightFoot", "LeftFoot"), ("leftHand", "rightHand"), ("spine", "spine")] {
            assert_eq!(mirrored_name(a), b);
        }
    }

    #[test]
    fn neighbours_join_points_that_are_the_same_place() {
        // two triangles that share an edge by place only (the points are written twice, as a seam of the picture does)
        let verts = vec![[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [1.0, 1.0, 0.0]];
        let tris = vec![[0, 1, 2], [3, 5, 4]];
        let n = neighbours(&verts, &tris);
        assert!(n[3].contains(&0) || n[3].contains(&1), "the second copy of a point knows the first triangle: {:?}", n[3]);
        assert!(n[1].len() >= 2);
    }

    #[test]
    fn joined_bones_move_together() {
        let mut r = Rig3::new(0);
        let a = r.add_bone("a", None, [0.0, 0.0, 0.0], [0.0, 1.0, 0.0]);
        let b = r.add_bone("b", Some(a), [0.0, 1.0, 0.0], [0.0, 2.0, 0.0]);
        let c = r.add_bone("c", Some(a), [1.0, 0.0, 0.0], [1.0, 1.0, 0.0]);
        r.move_tail(a, [0.5, 1.2, 0.0]);
        assert_eq!(r.bones[b].head, [0.5, 1.2, 0.0], "the child that was joined follows");
        assert_eq!(r.bones[c].head, [1.0, 0.0, 0.0], "the one that was not joined stays");
        r.move_head(b, [0.6, 1.3, 0.0]);
        assert_eq!(r.bones[a].tail, [0.6, 1.3, 0.0], "the parent's tail follows the head");
        r.move_with_children(a, [0.0, 0.0, 1.0]);
        assert_eq!(r.bones[b].tail, [0.0, 2.0, 1.0]);
        assert_eq!(r.bones[c].head, [1.0, 0.0, 1.0]);
    }

    #[test]
    fn the_humanoid_has_the_names_of_the_animations_and_fits_the_mesh() {
        let r = Rig3::humanoid(0, [-0.4, 0.0, -0.2], [0.4, 1.8, 0.2]);
        for n in crate::motion::BONES {
            assert!(r.bones.iter().any(|b| b.name == n), "{n}");
        }
        let hips = r.bones.iter().find(|b| b.name == "hips").unwrap();
        assert!((hips.head[1] - 0.53 * 1.8).abs() < 1e-4 && hips.head[0].abs() < 1e-6);
        let left = r.bones.iter().find(|b| b.name == "leftUpperArm").unwrap();
        let right = r.bones.iter().find(|b| b.name == "rightUpperArm").unwrap();
        assert!(left.head[0] > 0.0 && right.head[0] < 0.0 && (left.head[0] + right.head[0]).abs() < 1e-5);
        for (i, b) in r.bones.iter().enumerate() {
            assert!(b.parent.is_none_or(|p| p < i), "parents come first");
        }
    }

    #[test]
    fn a_frame_turns_the_bones_with_the_names() {
        let r = Rig3::humanoid(0, [0.0; 3], [1.0, 2.0, 1.0]);
        let mut f = crate::motion::Frame::default();
        f.add("leftUpperLeg", [-30.0, 0.0, 0.0]);
        let p = Pose::from_frame(&r, &f, false);
        let i = r.bones.iter().position(|b| b.name == "leftUpperLeg").unwrap();
        assert_eq!(p.rot[i], [-30.0, 0.0, 0.0]);
        assert_eq!(p.rot[0], [0.0; 3]);
        let rel = Pose::from_frame(&r, &crate::motion::Frame::rest(), true);
        assert!(rel.rot.iter().all(|v| *v == [0.0; 3]), "the relaxed stand is the rest of a figure built with its arms down");
    }
}
