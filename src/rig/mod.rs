// SPDX-License-Identifier: GPL-3.0-or-later
// The rig: art meshes (a layer cut into triangles), changers that bend them (Rotation and Warp so far),
// parameters (sliders like "head turn") and keyforms (the shape a changer has at certain slider values).
// `pose` turns slider values into changer shapes, `deformed` moves a mesh by its changers. The renderer
// (raster.rs) draws the layers through the moved meshes.

pub mod mesh;
pub mod raster;

use std::collections::{HashMap, HashSet};

use serde::{Deserialize, Serialize};

use crate::doc::LayerId;

pub type Id = u32;

/// A layer cut into triangles. Vertices are page positions at rest; the layer's pixels are looked up at
/// the rest position, so moving a vertex drags the picture with it.
#[derive(Clone, PartialEq, Debug, Serialize, Deserialize)]
pub struct ArtMesh {
    pub id: Id,
    pub layer: LayerId,
    pub verts: Vec<[f32; 2]>,
    pub tris: Vec<[u32; 3]>,
    /// The changer that moves this mesh.
    pub parent: Option<Id>,
}

#[derive(Clone, PartialEq, Debug, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Kind {
    /// Turns everything around a point.
    Rotation { pivot: [f32; 2] },
    /// A grid of control points over a rectangle (x0, y0, x1, y1); moving a point bends the picture.
    Warp { rect: [f32; 4], cols: u32, rows: u32 },
}

#[derive(Clone, PartialEq, Debug, Serialize, Deserialize)]
pub struct Changer {
    pub id: Id,
    pub name: String,
    pub kind: Kind,
    /// A changer inside another one follows it.
    pub parent: Option<Id>,
}

#[derive(Clone, PartialEq, Debug, Serialize, Deserialize)]
pub struct Param {
    pub id: Id,
    pub name: String,
    pub min: f32,
    pub max: f32,
    pub default: f32,
}

/// The shape of a changer: degrees for a Rotation, the offsets of the control points for a Warp.
#[derive(Clone, PartialEq, Debug, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Form {
    Rot { deg: f32 },
    Warp { offsets: Vec<[f32; 2]> },
}

/// A changer's shapes at chosen values of one parameter (or of two, as a grid).
/// `keys[axis]` are the slider values, `forms` has one shape per combination (the second axis is the outer loop).
#[derive(Clone, PartialEq, Debug, Serialize, Deserialize)]
pub struct Binding {
    pub changer: Id,
    pub params: Vec<Id>,
    pub keys: Vec<Vec<f32>>,
    pub forms: Vec<Form>,
}

#[derive(Clone, PartialEq, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Rig {
    pub meshes: Vec<ArtMesh>,
    pub changers: Vec<Changer>,
    pub params: Vec<Param>,
    pub bindings: Vec<Binding>,
    next_id: Id,
}

/// The slider values now (a parameter that isn't set sits at its default).
#[derive(Clone, Debug, Default)]
pub struct Values(pub HashMap<Id, f32>);

/// What every changer looks like for a set of slider values.
#[derive(Clone, Debug, Default)]
pub struct Pose {
    pub rot: HashMap<Id, f32>,
    pub warp: HashMap<Id, Vec<[f32; 2]>>,
}

pub const MAX_WARP: u32 = 24;
const MAX_DEPTH: usize = 16;

/// Where a value falls between sorted keys: (lower, upper, share of the upper).
pub fn axis_weights(keys: &[f32], v: f32) -> (usize, usize, f32) {
    if keys.is_empty() {
        return (0, 0, 0.0);
    }
    if v <= keys[0] {
        return (0, 0, 0.0);
    }
    let last = keys.len() - 1;
    if v >= keys[last] {
        return (last, last, 0.0);
    }
    let i = keys.iter().rposition(|k| *k <= v).unwrap_or(0).min(last - 1);
    let span = keys[i + 1] - keys[i];
    (i, i + 1, if span > 1e-9 { (v - keys[i]) / span } else { 0.0 })
}

fn lerp_form(a: &Form, b: &Form, t: f32) -> Form {
    match (a, b) {
        (Form::Rot { deg: x }, Form::Rot { deg: y }) => Form::Rot { deg: x + (y - x) * t },
        (Form::Warp { offsets: x }, Form::Warp { offsets: y }) if x.len() == y.len() => Form::Warp { offsets: x.iter().zip(y).map(|(p, q)| [p[0] + (q[0] - p[0]) * t, p[1] + (q[1] - p[1]) * t]).collect() },
        _ => a.clone(),
    }
}

impl Values {
    pub fn set(&mut self, id: Id, v: f32) {
        self.0.insert(id, v);
    }
}

impl Rig {
    pub fn is_empty(&self) -> bool {
        self.meshes.is_empty() && self.changers.is_empty() && self.params.is_empty()
    }

    pub fn fresh_id(&mut self) -> Id {
        self.next_id += 1;
        self.next_id
    }

    pub fn param(&self, id: Id) -> Option<&Param> {
        self.params.iter().find(|p| p.id == id)
    }

    pub fn changer(&self, id: Id) -> Option<&Changer> {
        self.changers.iter().find(|c| c.id == id)
    }

    pub fn mesh_of(&self, layer: LayerId) -> Option<&ArtMesh> {
        self.meshes.iter().find(|m| m.layer == layer)
    }

    pub fn value(&self, vals: &Values, id: Id) -> f32 {
        vals.0.get(&id).copied().or_else(|| self.param(id).map(|p| p.default)).unwrap_or(0.0)
    }

    pub fn add_param(&mut self, name: &str, min: f32, max: f32, default: f32) -> Id {
        let id = self.fresh_id();
        self.params.push(Param { id, name: name.chars().filter(|c| !c.is_control()).take(40).collect(), min, max, default: default.clamp(min, max) });
        id
    }

    pub fn add_changer(&mut self, name: &str, kind: Kind, parent: Option<Id>) -> Id {
        let id = self.fresh_id();
        self.changers.push(Changer { id, name: name.chars().filter(|c| !c.is_control()).take(40).collect(), kind, parent });
        id
    }

    /// The rest-position control points of a Warp (row by row).
    pub fn warp_rest(rect: [f32; 4], cols: u32, rows: u32) -> Vec<[f32; 2]> {
        let mut v = Vec::with_capacity(((cols + 1) * (rows + 1)) as usize);
        for r in 0..=rows {
            for c in 0..=cols {
                v.push([rect[0] + (rect[2] - rect[0]) * c as f32 / cols as f32, rect[1] + (rect[3] - rect[1]) * r as f32 / rows as f32]);
            }
        }
        v
    }

    /// The shape a changer has when nothing is moved.
    pub fn rest_form(&self, changer: Id) -> Option<Form> {
        Some(match &self.changer(changer)?.kind {
            Kind::Rotation { .. } => Form::Rot { deg: 0.0 },
            Kind::Warp { cols, rows, .. } => Form::Warp { offsets: vec![[0.0, 0.0]; ((cols + 1) * (rows + 1)) as usize] },
        })
    }

    /// A slider that turns a Rotation: at `min` it is turned by `-deg_at_max`, at the default not at all,
    /// at `max` by `deg_at_max` (use a negative number to turn the other way).
    pub fn bind_rotation(&mut self, changer: Id, param: Id, deg_at_max: f32) {
        let Some(p) = self.param(param).cloned() else { return };
        let (lo, hi) = (p.min, p.max);
        let mid = p.default;
        let share = |v: f32| if v >= mid { if hi > mid { (v - mid) / (hi - mid) } else { 0.0 } } else if mid > lo { (v - mid) / (mid - lo) } else { 0.0 };
        let mut keys = vec![lo, mid, hi];
        keys.dedup();
        let forms = keys.iter().map(|k| Form::Rot { deg: deg_at_max * share(*k) }).collect();
        self.bindings.push(Binding { changer, params: vec![param], keys: vec![keys], forms });
    }

    /// Add the shape of a changer at the current value of a parameter as a keyform (replacing one at the same value).
    /// For a changer that has no binding with this parameter yet, one is made with the rest shape at the default.
    pub fn set_keyform(&mut self, changer: Id, param: Id, at: f32, form: Form) {
        let Some(p) = self.param(param).cloned() else { return };
        let Some(rest) = self.rest_form(changer) else { return };
        let bi = match self.bindings.iter().position(|b| b.changer == changer && b.params == [param]) {
            Some(i) => i,
            None => {
                let keys = if (p.default - at).abs() < 1e-4 { vec![p.default] } else { vec![p.default] };
                self.bindings.push(Binding { changer, params: vec![param], keys: vec![keys], forms: vec![rest.clone()] });
                self.bindings.len() - 1
            }
        };
        let b = &mut self.bindings[bi];
        if let Some(i) = b.keys[0].iter().position(|k| (k - at).abs() < 1e-4) {
            b.forms[i] = form;
        } else {
            let i = b.keys[0].iter().position(|k| *k > at).unwrap_or(b.keys[0].len());
            b.keys[0].insert(i, at);
            b.forms.insert(i, form);
        }
    }

    fn binding_form(&self, b: &Binding, vals: &Values) -> Option<Form> {
        match b.params.len() {
            1 => {
                let (i0, i1, t) = axis_weights(&b.keys[0], self.value(vals, b.params[0]));
                Some(lerp_form(b.forms.get(i0)?, b.forms.get(i1)?, t))
            }
            2 => {
                let nx = b.keys[0].len();
                let (x0, x1, tx) = axis_weights(&b.keys[0], self.value(vals, b.params[0]));
                let (y0, y1, ty) = axis_weights(&b.keys[1], self.value(vals, b.params[1]));
                let at = |x: usize, y: usize| b.forms.get(y * nx + x);
                let top = lerp_form(at(x0, y0)?, at(x1, y0)?, tx);
                let bottom = lerp_form(at(x0, y1)?, at(x1, y1)?, tx);
                Some(lerp_form(&top, &bottom, ty))
            }
            _ => None,
        }
    }

    /// Every changer's shape for these slider values (all bindings of a changer add up).
    pub fn pose(&self, vals: &Values) -> Pose {
        let mut pose = Pose::default();
        for b in &self.bindings {
            let Some(f) = self.binding_form(b, vals) else { continue };
            match f {
                Form::Rot { deg } => *pose.rot.entry(b.changer).or_insert(0.0) += deg,
                Form::Warp { offsets } => {
                    let e = pose.warp.entry(b.changer).or_insert_with(|| vec![[0.0, 0.0]; offsets.len()]);
                    if e.len() == offsets.len() {
                        for (a, o) in e.iter_mut().zip(&offsets) {
                            a[0] += o[0];
                            a[1] += o[1];
                        }
                    }
                }
            }
        }
        pose
    }

    /// Move a point by one changer (not its parents).
    fn apply_one(&self, c: &Changer, pose: &Pose, p: [f32; 2]) -> [f32; 2] {
        match &c.kind {
            Kind::Rotation { pivot } => {
                let deg = pose.rot.get(&c.id).copied().unwrap_or(0.0);
                if deg == 0.0 {
                    return p;
                }
                let (s, co) = deg.to_radians().sin_cos();
                let (dx, dy) = (p[0] - pivot[0], p[1] - pivot[1]);
                [pivot[0] + dx * co - dy * s, pivot[1] + dx * s + dy * co]
            }
            Kind::Warp { rect, cols, rows } => {
                let Some(off) = pose.warp.get(&c.id) else { return p };
                let (w, h) = ((rect[2] - rect[0]).max(1e-3), (rect[3] - rect[1]).max(1e-3));
                // where the point is in the grid; outside the box it takes the shift of the nearest edge
                let u = ((p[0] - rect[0]) / w * *cols as f32).clamp(0.0, *cols as f32);
                let v = ((p[1] - rect[1]) / h * *rows as f32).clamp(0.0, *rows as f32);
                let (i, j) = ((u.floor() as u32).min(cols - 1), (v.floor() as u32).min(rows - 1));
                let (fu, fv) = (u - i as f32, v - j as f32);
                let idx = |c2: u32, r2: u32| (r2 * (cols + 1) + c2) as usize;
                let (a, b2, c3, d) = (off[idx(i, j)], off[idx(i + 1, j)], off[idx(i, j + 1)], off[idx(i + 1, j + 1)]);
                let mix = |k: usize| (a[k] * (1.0 - fu) + b2[k] * fu) * (1.0 - fv) + (c3[k] * (1.0 - fu) + d[k] * fu) * fv;
                [p[0] + mix(0), p[1] + mix(1)]
            }
        }
    }

    /// Move a point by a changer and then by the changers it sits in.
    pub fn deform_point(&self, pose: &Pose, changer: Option<Id>, mut p: [f32; 2]) -> [f32; 2] {
        let mut cur = changer;
        let mut depth = 0;
        while let Some(id) = cur {
            let Some(c) = self.changer(id) else { break };
            p = self.apply_one(c, pose, p);
            cur = c.parent;
            depth += 1;
            if depth > MAX_DEPTH {
                break;
            }
        }
        p
    }

    pub fn deformed(&self, mesh: &ArtMesh, pose: &Pose) -> Vec<[f32; 2]> {
        mesh.verts.iter().map(|v| self.deform_point(pose, mesh.parent, *v)).collect()
    }

    /// Project files come from outside: drop what points nowhere and bound every size.
    pub fn sanitized(mut self) -> Self {
        let fin = |v: f32| v.is_finite();
        self.meshes.truncate(2000);
        self.changers.truncate(2000);
        self.params.truncate(2000);
        self.bindings.truncate(8000);
        self.params.retain(|p| fin(p.min) && fin(p.max) && fin(p.default) && p.min <= p.max);
        for p in &mut self.params {
            p.default = p.default.clamp(p.min, p.max);
            p.name = p.name.chars().filter(|c| !c.is_control()).take(40).collect();
        }
        self.changers.retain_mut(|c| match &mut c.kind {
            Kind::Rotation { pivot } => pivot.iter().all(|v| fin(*v)),
            Kind::Warp { rect, cols, rows } => {
                *cols = (*cols).clamp(1, MAX_WARP);
                *rows = (*rows).clamp(1, MAX_WARP);
                rect.iter().all(|v| fin(*v)) && rect[2] > rect[0] && rect[3] > rect[1]
            }
        });
        let ids: HashSet<Id> = self.changers.iter().map(|c| c.id).collect();
        // a changer whose parent is missing or that would loop sits at the top
        let parents: HashMap<Id, Option<Id>> = self.changers.iter().map(|c| (c.id, c.parent)).collect();
        for c in &mut self.changers {
            let mut cur = c.parent;
            let mut ok = true;
            let mut depth = 0;
            while let Some(p) = cur {
                if !ids.contains(&p) || p == c.id || depth > MAX_DEPTH {
                    ok = false;
                    break;
                }
                cur = parents.get(&p).copied().flatten();
                depth += 1;
            }
            if !ok {
                c.parent = None;
            }
        }
        for m in &mut self.meshes {
            if m.parent.is_some_and(|p| !ids.contains(&p)) {
                m.parent = None;
            }
            m.verts.truncate(200_000);
            let n = m.verts.len() as u32;
            m.tris.retain(|t| t.iter().all(|i| *i < n));
            m.tris.truncate(400_000);
        }
        self.meshes.retain(|m| m.verts.iter().all(|v| v[0].is_finite() && v[1].is_finite()) && !m.tris.is_empty());
        let pids: HashSet<Id> = self.params.iter().map(|p| p.id).collect();
        let cmap: HashMap<Id, usize> = self.changers.iter().enumerate().map(|(i, c)| (c.id, i)).collect();
        let kinds: Vec<Kind> = self.changers.iter().map(|c| c.kind.clone()).collect();
        self.bindings.retain_mut(|b| {
            let Some(ci) = cmap.get(&b.changer) else { return false };
            if b.params.is_empty() || b.params.len() > 2 || b.params.len() != b.keys.len() || !b.params.iter().all(|p| pids.contains(p)) {
                return false;
            }
            for k in &mut b.keys {
                k.retain(|v| v.is_finite());
                k.sort_by(|a, c| a.total_cmp(c));
                k.dedup();
                k.truncate(64);
                if k.is_empty() {
                    return false;
                }
            }
            let want: usize = b.keys.iter().map(Vec::len).product();
            if b.forms.len() != want {
                return false;
            }
            b.forms.iter().all(|f| match (f, &kinds[*ci]) {
                (Form::Rot { deg }, Kind::Rotation { .. }) => fin(*deg),
                (Form::Warp { offsets }, Kind::Warp { cols, rows, .. }) => offsets.len() == ((cols + 1) * (rows + 1)) as usize && offsets.iter().all(|o| fin(o[0]) && fin(o[1])),
                _ => false,
            })
        });
        let top = self.meshes.iter().map(|m| m.id).chain(self.changers.iter().map(|c| c.id)).chain(self.params.iter().map(|p| p.id)).max().unwrap_or(0);
        self.next_id = self.next_id.max(top);
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rot_rig() -> (Rig, Id, Id) {
        let mut r = Rig::default();
        let head = r.add_changer("Head", Kind::Rotation { pivot: [100.0, 100.0] }, None);
        let tilt = r.add_param("Head tilt", -30.0, 30.0, 0.0);
        r.bind_rotation(head, tilt, 30.0);
        (r, head, tilt)
    }

    fn near(a: [f32; 2], b: [f32; 2]) -> bool {
        (a[0] - b[0]).abs() < 0.01 && (a[1] - b[1]).abs() < 0.01
    }

    #[test]
    fn a_rotation_follows_its_slider() {
        let (r, head, tilt) = rot_rig();
        let mut v = Values::default();
        assert!(near(r.deform_point(&r.pose(&v), Some(head), [150.0, 100.0]), [150.0, 100.0]), "rest at the default");
        v.set(tilt, 30.0);
        let p = r.deform_point(&r.pose(&v), Some(head), [200.0, 100.0]);
        assert!(near(p, [100.0 + 100.0 * 30f32.to_radians().cos(), 100.0 + 100.0 * 30f32.to_radians().sin()]), "{p:?}");
        v.set(tilt, 15.0);
        let q = r.deform_point(&r.pose(&v), Some(head), [200.0, 100.0]);
        assert!((q[1] - (100.0 + 100.0 * 15f32.to_radians().sin())).abs() < 0.01, "half way is 15 degrees: {q:?}");
        v.set(tilt, -30.0);
        assert!(r.deform_point(&r.pose(&v), Some(head), [200.0, 100.0])[1] < 100.0, "the other way");
        // beyond the ends it stays at the end
        v.set(tilt, 500.0);
        let e = r.deform_point(&r.pose(&v), Some(head), [200.0, 100.0]);
        assert!(near(e, r.deform_point(&{ let mut w = Values::default(); w.set(tilt, 30.0); r.pose(&w) }, Some(head), [200.0, 100.0])));
    }

    #[test]
    fn a_changer_inside_another_follows_it() {
        let (mut r, head, tilt) = rot_rig();
        let ear = r.add_changer("Ear", Kind::Rotation { pivot: [150.0, 100.0] }, Some(head));
        let flap = r.add_param("Ear flap", 0.0, 1.0, 0.0);
        r.bind_rotation(ear, flap, 90.0);
        let mut v = Values::default();
        v.set(flap, 1.0);
        // the ear's tip turns up around the ear's pivot
        let p = r.deform_point(&r.pose(&v), Some(ear), [200.0, 100.0]);
        assert!(near(p, [150.0, 150.0]), "{p:?}");
        // and when the head tilts, the ear (and its tip) goes with the head
        v.set(tilt, 30.0);
        let q = r.deform_point(&r.pose(&v), Some(ear), [200.0, 100.0]);
        let (s, c) = 30f32.to_radians().sin_cos();
        let (dx, dy) = (150.0 - 100.0, 150.0 - 100.0);
        assert!(near(q, [100.0 + dx * c - dy * s, 100.0 + dx * s + dy * c]), "{q:?}");
    }

    #[test]
    fn a_warp_moves_the_picture_with_its_control_points_and_extends_past_the_box() {
        let mut r = Rig::default();
        let w = r.add_changer("Cheek", Kind::Warp { rect: [0.0, 0.0, 100.0, 100.0], cols: 2, rows: 2 }, None);
        let p = r.add_param("Puff", 0.0, 1.0, 0.0);
        // the middle control point (index 4) goes 20 right at 1
        let mut offsets = vec![[0.0, 0.0]; 9];
        offsets[4] = [20.0, 0.0];
        r.set_keyform(w, p, 1.0, Form::Warp { offsets });
        let mut v = Values::default();
        v.set(p, 1.0);
        let pose = r.pose(&v);
        assert!(near(r.deform_point(&pose, Some(w), [50.0, 50.0]), [70.0, 50.0]));
        assert!(near(r.deform_point(&pose, Some(w), [0.0, 0.0]), [0.0, 0.0]), "corners stay");
        assert!((r.deform_point(&pose, Some(w), [25.0, 50.0])[0] - 35.0).abs() < 0.01, "between the points it blends");
        v.set(p, 0.5);
        assert!(near(r.deform_point(&r.pose(&v), Some(w), [50.0, 50.0]), [60.0, 50.0]), "half way");
        // outside the box a point takes the shift of the edge (no tear)
        let out = r.deform_point(&pose, Some(w), [50.0, 150.0]);
        assert!(near(out, [50.0, 150.0]), "the bottom edge control point did not move: {out:?}");
    }

    #[test]
    fn two_sliders_make_a_grid_of_shapes() {
        let mut r = Rig::default();
        let c = r.add_changer("Head", Kind::Rotation { pivot: [0.0, 0.0] }, None);
        let x = r.add_param("Angle X", -1.0, 1.0, 0.0);
        let y = r.add_param("Angle Y", -1.0, 1.0, 0.0);
        // corners: (-1,-1)=-40, (1,-1)=-20, (-1,1)=20, (1,1)=40 (keys at -1 and 1 only)
        r.bindings.push(Binding { changer: c, params: vec![x, y], keys: vec![vec![-1.0, 1.0], vec![-1.0, 1.0]], forms: vec![Form::Rot { deg: -40.0 }, Form::Rot { deg: -20.0 }, Form::Rot { deg: 20.0 }, Form::Rot { deg: 40.0 }] });
        let mut v = Values::default();
        v.set(x, 1.0);
        v.set(y, 1.0);
        assert_eq!(r.pose(&v).rot[&c], 40.0);
        v.set(x, -1.0);
        assert_eq!(r.pose(&v).rot[&c], 20.0);
        v.set(x, 0.0);
        v.set(y, 0.0);
        assert!((r.pose(&v).rot[&c]).abs() < 1e-4, "the middle is the average of the four corners");
        v.set(x, 1.0);
        v.set(y, 0.0);
        assert!((r.pose(&v).rot[&c] - 10.0).abs() < 1e-4);
    }

    #[test]
    fn keyforms_are_added_in_order_and_replaced_at_the_same_value() {
        let (mut r, head, tilt) = rot_rig();
        r.set_keyform(head, tilt, 10.0, Form::Rot { deg: 99.0 });
        let b = &r.bindings[0];
        assert_eq!(b.keys[0], [-30.0, 0.0, 10.0, 30.0]);
        r.set_keyform(head, tilt, 10.0, Form::Rot { deg: 5.0 });
        assert_eq!(r.bindings[0].forms.len(), 4);
        assert_eq!(r.bindings[0].forms[2], Form::Rot { deg: 5.0 });
        let mut v = Values::default();
        v.set(tilt, 10.0);
        assert_eq!(r.pose(&v).rot[&head], 5.0);
        // a changer without a binding gets one starting from its rest shape
        let other = r.add_changer("Other", Kind::Rotation { pivot: [0.0, 0.0] }, None);
        r.set_keyform(other, tilt, 20.0, Form::Rot { deg: 8.0 });
        assert_eq!(r.bindings.last().unwrap().keys[0], [0.0, 20.0]);
        v.set(tilt, 10.0);
        assert!((r.pose(&v).rot[&other] - 4.0).abs() < 1e-4);
    }

    #[test]
    fn bad_files_are_cleaned() {
        let (mut r, head, tilt) = rot_rig();
        // a loop of parents, a missing parent, a binding to nothing, a form of the wrong kind, wrong sizes
        let a = r.add_changer("A", Kind::Rotation { pivot: [0.0, 0.0] }, None);
        let b = r.add_changer("B", Kind::Rotation { pivot: [0.0, 0.0] }, Some(a));
        r.changers.iter_mut().find(|c| c.id == a).unwrap().parent = Some(b);
        let lost = r.add_changer("Lost", Kind::Rotation { pivot: [0.0, 0.0] }, Some(9999));
        r.bindings.push(Binding { changer: 777, params: vec![tilt], keys: vec![vec![0.0]], forms: vec![Form::Rot { deg: 1.0 }] });
        r.bindings.push(Binding { changer: head, params: vec![tilt], keys: vec![vec![0.0, 1.0]], forms: vec![Form::Rot { deg: 1.0 }] });
        r.bindings.push(Binding { changer: head, params: vec![tilt], keys: vec![vec![0.0]], forms: vec![Form::Warp { offsets: vec![] }] });
        r.bindings.push(Binding { changer: head, params: vec![tilt], keys: vec![vec![f32::NAN]], forms: vec![Form::Rot { deg: 1.0 }] });
        r.meshes.push(ArtMesh { id: 50, layer: 1, verts: vec![[0.0, 0.0], [1.0, 0.0], [0.0, 1.0]], tris: vec![[0, 1, 2], [0, 1, 9]], parent: Some(5555) });
        r.changers.push(Changer { id: 60, name: "Huge".into(), kind: Kind::Warp { rect: [0.0, 0.0, 10.0, 10.0], cols: 9999, rows: 0 }, parent: None });
        let s = r.sanitized();
        assert!(s.changer(a).unwrap().parent.is_none() || s.changer(b).unwrap().parent.is_none(), "the loop is broken");
        assert_eq!(s.changer(lost).unwrap().parent, None);
        assert_eq!(s.bindings.len(), 1, "only the good binding is left");
        assert_eq!(s.meshes[0].tris, vec![[0, 1, 2]]);
        assert_eq!(s.meshes[0].parent, None);
        match s.changer(60).unwrap().kind {
            Kind::Warp { cols, rows, .. } => assert!(cols <= MAX_WARP && rows >= 1),
            _ => panic!(),
        }
    }

    #[test]
    fn a_rig_survives_json() {
        let (r, ..) = rot_rig();
        let back: Rig = serde_json::from_str(&serde_json::to_string(&r).unwrap()).unwrap();
        assert_eq!(back, r);
        assert_eq!(back.sanitized(), r.clone().sanitized());
    }
}
