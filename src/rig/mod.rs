// SPDX-License-Identifier: GPL-3.0-or-later
// The rig: art meshes (a layer cut into triangles), changers that bend them (Rotation and Warp so far),
// parameters (sliders like "head turn") and keyforms (the shape a changer has at certain slider values).
// `pose` turns slider values into changer shapes, `deformed` moves a mesh by its changers. The renderer
// (raster.rs) draws the layers through the moved meshes.

pub mod autorig;
pub mod changers;
pub mod check;
pub mod gizmo;
pub mod mesh;
pub mod paramphys;
pub mod physics;
pub mod refine;
pub mod raster;
pub mod skin;
pub mod uncut;
pub mod views;

use std::collections::{HashMap, HashSet};

use serde::{Deserialize, Serialize};

use crate::doc::LayerId;

pub use changers::PathSpot;

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
    /// A curve with control points; the picture near it bends along the curve (hair strands, tails, ribbons).
    /// `width`: how far from the curve the picture follows it fully; it fades out over the same distance again.
    Line { pts: Vec<[f32; 2]>, width: f32 },
    /// A soft push: everything within `radius` of the centre moves, most in the middle (cheeks, eyes, belly).
    Soft { center: [f32; 2], radius: f32 },
    /// Several curves put anywhere on a part, each dragged at its own spots (Live2D and Inochi path deformers, but many).
    Paths { paths: Vec<PathSpot> },
    /// Puppet-warp pins: drag a pin and the picture follows smoothly; pins you leave alone hold it still.
    Pins { pts: Vec<[f32; 2]> },
    /// Shifts everything (Inochi translate).
    Move,
    /// Stretches everything about a point (Inochi scale, Live2D rotation scale).
    Scale { center: [f32; 2] },
    /// Turns everything over, through flat (Live2D reflect).
    Flip { center: [f32; 2], vertical: bool },
    /// Opacity of everything in it (Live2D keyform and part opacity).
    Fade,
    /// Moves everything forward or back in the stack of layers (Live2D draw order, Inochi z-sort).
    Order,
    /// A wave that runs through everything, moved along by a slider.
    Wave { center: [f32; 2] },
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
    /// The Live2D id of the slider ("ParamAngleX"), for the files of a Live2D model; empty when it has none.
    #[serde(default)]
    pub cid: String,
}

/// The shape of a changer: degrees for a Rotation, the offsets of the control points for a Warp.
#[derive(Clone, PartialEq, Debug, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Form {
    Rot { deg: f32 },
    Warp { offsets: Vec<[f32; 2]> },
    /// How far each control point of a Line moved, and how much the picture turns at each point (degrees; empty = none).
    Line {
        offsets: Vec<[f32; 2]>,
        #[serde(default)]
        turns: Vec<f32>,
    },
    /// How far a Soft changer pushes, and how much it grows (0.2 = 20% bigger in the middle).
    Soft { offset: [f32; 2], grow: f32 },
    /// The numbers of Move, Scale, Flip, Fade, Order and Wave (see `changers::val_names`).
    Vals { v: Vec<f32> },
    /// How far each pin moved.
    Pins { offsets: Vec<[f32; 2]> },
    /// How far each point of each curve moved, and how much the picture turns at each point.
    Paths {
        offsets: Vec<Vec<[f32; 2]>>,
        #[serde(default)]
        turns: Vec<Vec<f32>>,
    },
}

/// How a value moves between two keys.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Ease {
    #[default]
    Linear,
    /// Slow at both ends.
    Smooth,
    /// Slow to start.
    In,
    /// Slow to finish.
    Out,
}

impl Ease {
    pub const ALL: [Ease; 4] = [Ease::Linear, Ease::Smooth, Ease::In, Ease::Out];

    pub fn label(self) -> &'static str {
        match self {
            Ease::Linear => "Even",
            Ease::Smooth => "Smooth",
            Ease::In => "Slow start",
            Ease::Out => "Slow finish",
        }
    }

    pub fn apply(self, t: f32) -> f32 {
        let t = t.clamp(0.0, 1.0);
        match self {
            Ease::Linear => t,
            Ease::Smooth => t * t * (3.0 - 2.0 * t),
            Ease::In => t * t,
            Ease::Out => 1.0 - (1.0 - t) * (1.0 - t),
        }
    }
}

/// A changer's shapes at chosen values of one parameter (or of two, as a grid).
/// `keys[axis]` are the slider values, `forms` has one shape per combination (the second axis is the outer loop).
#[derive(Clone, PartialEq, Debug, Serialize, Deserialize)]
pub struct Binding {
    pub changer: Id,
    pub params: Vec<Id>,
    pub keys: Vec<Vec<f32>>,
    pub forms: Vec<Form>,
    #[serde(default)]
    pub ease: Ease,
}

/// Two-bone inverse kinematics: sliders for where the tip (a hand, a foot) should be, and the two Rotation changers
/// (upper and lower, the lower one inside the upper) turn so the tip gets there, as far as the bones reach.
#[derive(Clone, PartialEq, Debug, Serialize, Deserialize)]
pub struct Ik {
    pub upper: Id,
    pub lower: Id,
    /// Rest positions: the root (shoulder), the joint (elbow) and the tip (hand).
    pub root: [f32; 2],
    pub joint: [f32; 2],
    pub tip: [f32; 2],
    /// Sliders: how far the tip is moved to the right and down, in page pixels.
    pub px: Id,
    pub py: Id,
}

/// The turns (degrees, on top of the rest) that bring the tip of a two-bone arm to `target`.
pub fn solve_two_bone(root: [f32; 2], joint: [f32; 2], tip: [f32; 2], target: [f32; 2]) -> (f32, f32) {
    let v1 = [joint[0] - root[0], joint[1] - root[1]];
    let v2 = [tip[0] - joint[0], tip[1] - joint[1]];
    let (l1, l2) = (v1[0].hypot(v1[1]).max(1e-3), v2[0].hypot(v2[1]).max(1e-3));
    let rest1 = v1[1].atan2(v1[0]);
    let rest_rel = v2[1].atan2(v2[0]) - rest1;
    // which way the elbow bends
    let s = if v1[0] * v2[1] - v1[1] * v2[0] >= 0.0 { 1.0 } else { -1.0 };
    let t = [target[0] - root[0], target[1] - root[1]];
    let t_len = t[0].hypot(t[1]);
    let straight = t_len >= l1 + l2 - 1e-3;
    let dist = t_len.clamp((l1 - l2).abs() + 1e-3, l1 + l2 - 1e-3);
    let ang_t = t[1].atan2(t[0]);
    let cos_a = ((l1 * l1 + dist * dist - l2 * l2) / (2.0 * l1 * dist)).clamp(-1.0, 1.0);
    let alpha = if straight { 0.0 } else { cos_a.acos() };
    let th1 = ang_t - s * alpha;
    let elbow = [root[0] + l1 * th1.cos(), root[1] + l1 * th1.sin()];
    let reach = [root[0] + dist * ang_t.cos(), root[1] + dist * ang_t.sin()];
    let th2 = (reach[1] - elbow[1]).atan2(reach[0] - elbow[0]);
    let wrap = |a: f32| {
        let mut a = a;
        while a > std::f32::consts::PI {
            a -= std::f32::consts::TAU;
        }
        while a < -std::f32::consts::PI {
            a += std::f32::consts::TAU;
        }
        a
    };
    (wrap(th1 - rest1).to_degrees(), wrap((th2 - th1) - rest_rel).to_degrees())
}

/// A named set of slider values (a pose you made and want back).
#[derive(Clone, PartialEq, Debug, Serialize, Deserialize)]
pub struct SavedPose {
    pub name: String,
    /// (slider, value); sliders not listed are at their default.
    pub values: Vec<(Id, f32)>,
}

/// A recording of slider values over time: one entry per frame, `fps` frames a second.
#[derive(Clone, PartialEq, Debug, Serialize, Deserialize)]
pub struct Clip {
    pub name: String,
    pub fps: f32,
    pub frames: Vec<Vec<(Id, f32)>>,
}

impl Clip {
    pub fn seconds(&self) -> f32 {
        self.frames.len() as f32 / self.fps.max(1.0)
    }

    /// The frame to show `t` seconds in (the clip loops).
    pub fn frame_at(&self, t: f32) -> Option<&Vec<(Id, f32)>> {
        if self.frames.is_empty() {
            return None;
        }
        let i = ((t.max(0.0) * self.fps.max(1.0)) as usize) % self.frames.len();
        self.frames.get(i)
    }
}

/// Two sliders joined into one square pad (like eyeball left-right and up-down, or mouth form and mouth open).
/// The first slider is the pad's horizontal line, the second its vertical line.
#[derive(Clone, PartialEq, Debug, Serialize, Deserialize)]
pub struct Pad {
    pub name: String,
    pub x: Id,
    pub y: Id,
}

/// A seam: vertices of two meshes that sit together at rest are pulled together when the meshes move apart.
#[derive(Clone, PartialEq, Debug, Serialize, Deserialize)]
pub struct Glue {
    pub a: Id,
    pub b: Id,
    /// (vertex of a, vertex of b)
    pub pairs: Vec<[u32; 2]>,
    /// 0 = loose, 1 = fully joined
    pub weight: f32,
}

#[derive(Clone, PartialEq, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Rig {
    pub meshes: Vec<ArtMesh>,
    pub changers: Vec<Changer>,
    pub params: Vec<Param>,
    pub bindings: Vec<Binding>,
    pub glues: Vec<Glue>,
    pub iks: Vec<Ik>,
    pub pads: Vec<Pad>,
    pub poses: Vec<SavedPose>,
    pub clips: Vec<Clip>,
    /// The uncut mode: one mesh over a group of layers, moved by bones with painted weights.
    pub skin: Option<skin::Skin>,
    /// What the automatic rigging made (and what you locked).
    pub auto: Option<autorig::AutoRig>,
    /// Things that swing and lag (hair, cloth, chains), each moving one Line changer.
    pub physics: Vec<physics::PhysChain>,
    pub colliders: Vec<physics::Collider>,
    pub world: physics::World,
    /// Warp changers that bend smoothly (a curve through the control points) instead of in straight cells.
    pub smooth: Vec<Id>,
    /// Live2D style physics that moves sliders (from a model's physics file), and what else came with the runtime files of a model:
    /// its expressions, its motions as they were, and the settings VTube Studio saved for it.
    pub param_physics: paramphys::ParamPhysics,
    pub expressions: Vec<crate::import::runtime::Expression>,
    pub live_motions: Vec<crate::import::runtime::Motion>,
    pub vtube: Option<crate::import::runtime::Vtube>,
    /// The timeline of the character (Animate tab).
    pub anim: crate::anim::Animation,
    /// How tracking (the face from a camera, a phone or a program) moves the sliders (Stream tab).
    pub tracking: crate::track::TrackMapping,
    /// Expressions made in this program (sliders saved under a name) and the keys, pad buttons and MIDI notes that switch expressions.
    pub own_exprs: Vec<crate::expressions::OwnExpr>,
    pub hotkeys: Vec<crate::hotkeys::Binding>,
    /// Views of the character (front, side, back) that one slider switches between.
    pub views: views::Views,
    /// Layers that are cut to the shape of a part (a skin patch to the face), so they never go outside it.
    pub clip_to: Vec<(LayerId, crate::parts::Part)>,
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
    pub line: HashMap<Id, Vec<[f32; 2]>>,
    pub soft: HashMap<Id, ([f32; 2], f32)>,
    pub vals: HashMap<Id, Vec<f32>>,
    pub pins: HashMap<Id, Vec<[f32; 2]>>,
    pub paths: HashMap<Id, Vec<Vec<[f32; 2]>>>,
    /// The turn at every point of a Line, and of every curve of a Path changer.
    pub line_turns: HashMap<Id, Vec<f32>>,
    pub path_turns: HashMap<Id, Vec<Vec<f32>>>,
}

pub const MAX_WARP: u32 = 24;
pub const MAX_LINE_POINTS: usize = 24;
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

/// Two lists of turns (empty = all zero) blended; empty when both are.
fn lerp_turns(a: &[f32], b: &[f32], n: usize, t: f32) -> Vec<f32> {
    if a.is_empty() && b.is_empty() {
        return Vec::new();
    }
    (0..n).map(|i| {
        let (x, y) = (a.get(i).copied().unwrap_or(0.0), b.get(i).copied().unwrap_or(0.0));
        x + (y - x) * t
    }).collect()
}

fn lerp_form(a: &Form, b: &Form, t: f32) -> Form {
    match (a, b) {
        (Form::Rot { deg: x }, Form::Rot { deg: y }) => Form::Rot { deg: x + (y - x) * t },
        (Form::Warp { offsets: x }, Form::Warp { offsets: y }) if x.len() == y.len() => Form::Warp { offsets: x.iter().zip(y).map(|(p, q)| [p[0] + (q[0] - p[0]) * t, p[1] + (q[1] - p[1]) * t]).collect() },
        (Form::Line { offsets: x, turns: tx }, Form::Line { offsets: y, turns: ty }) if x.len() == y.len() => Form::Line { offsets: x.iter().zip(y).map(|(p, q)| [p[0] + (q[0] - p[0]) * t, p[1] + (q[1] - p[1]) * t]).collect(), turns: lerp_turns(tx, ty, x.len(), t) },
        (Form::Vals { v: x }, Form::Vals { v: y }) if x.len() == y.len() => Form::Vals { v: x.iter().zip(y).map(|(p, q)| p + (q - p) * t).collect() },
        (Form::Pins { offsets: x }, Form::Pins { offsets: y }) if x.len() == y.len() => Form::Pins { offsets: x.iter().zip(y).map(|(p, q)| [p[0] + (q[0] - p[0]) * t, p[1] + (q[1] - p[1]) * t]).collect() },
        (Form::Paths { offsets: x, turns: tx }, Form::Paths { offsets: y, turns: ty }) if x.len() == y.len() && x.iter().zip(y).all(|(a, b)| a.len() == b.len()) => Form::Paths {
            offsets: x.iter().zip(y).map(|(a, b)| a.iter().zip(b).map(|(p, q)| [p[0] + (q[0] - p[0]) * t, p[1] + (q[1] - p[1]) * t]).collect()).collect(),
            turns: if tx.is_empty() && ty.is_empty() { Vec::new() } else { x.iter().enumerate().map(|(k, a)| lerp_turns(tx.get(k).map(Vec::as_slice).unwrap_or(&[]), ty.get(k).map(Vec::as_slice).unwrap_or(&[]), a.len(), t)).collect() },
        },
        (Form::Soft { offset: a, grow: g }, Form::Soft { offset: b, grow: h }) => Form::Soft { offset: [a[0] + (b[0] - a[0]) * t, a[1] + (b[1] - a[1]) * t], grow: g + (h - g) * t },
        _ => a.clone(),
    }
}

/// A smooth curve through the control points (Catmull-Rom), `per` samples between neighbours.
pub fn curve_samples(pts: &[[f32; 2]], per: usize) -> Vec<[f32; 2]> {
    let n = pts.len();
    if n < 2 {
        return pts.to_vec();
    }
    let at = |i: isize| pts[i.clamp(0, n as isize - 1) as usize];
    let mut out = Vec::with_capacity((n - 1) * per + 1);
    for i in 0..n - 1 {
        let (p0, p1, p2, p3) = (at(i as isize - 1), at(i as isize), at(i as isize + 1), at(i as isize + 2));
        for k in 0..per {
            let t = k as f32 / per as f32;
            let (t2, t3) = (t * t, t * t * t);
            let f = |a: f32, b: f32, c: f32, d: f32| 0.5 * ((2.0 * b) + (-a + c) * t + (2.0 * a - 5.0 * b + 4.0 * c - d) * t2 + (-a + 3.0 * b - 3.0 * c + d) * t3);
            out.push([f(p0[0], p1[0], p2[0], p3[0]), f(p0[1], p1[1], p2[1], p3[1])]);
        }
    }
    out.push(pts[n - 1]);
    out
}

/// Move a point along with a bent curve: it keeps its place along the curve and its distance to the side.
pub(crate) fn bend_along(rest: &[[f32; 2]], offsets: &[[f32; 2]], turns: &[f32], width: f32, p: [f32; 2]) -> ([f32; 2], f32) {
    if rest.len() < 2 || offsets.len() != rest.len() {
        return (p, 0.0);
    }
    let moved: Vec<[f32; 2]> = rest.iter().zip(offsets).map(|(a, o)| [a[0] + o[0], a[1] + o[1]]).collect();
    let (r, d) = (curve_samples(rest, 8), curve_samples(&moved, 8));
    // the nearest place on the rest curve
    let (mut best, mut best_i, mut best_u) = (f32::MAX, 0usize, 0.0f32);
    for i in 0..r.len() - 1 {
        let (a, b) = (r[i], r[i + 1]);
        let (dx, dy) = (b[0] - a[0], b[1] - a[1]);
        let len2 = (dx * dx + dy * dy).max(1e-9);
        let u = (((p[0] - a[0]) * dx + (p[1] - a[1]) * dy) / len2).clamp(0.0, 1.0);
        let dist = (p[0] - (a[0] + dx * u)).hypot(p[1] - (a[1] + dy * u));
        if dist < best {
            best = dist;
            best_i = i;
            best_u = u;
        }
    }
    let (a, b) = (r[best_i], r[best_i + 1]);
    let (dx, dy) = (b[0] - a[0], b[1] - a[1]);
    let len = (dx * dx + dy * dy).sqrt().max(1e-6);
    // which side of the curve the point is on (the normal is the tangent turned a quarter)
    let side = ((p[0] - (a[0] + dx * best_u)) * (-dy / len) + (p[1] - (a[1] + dy * best_u)) * (dx / len)).signum() * best;
    let (da, db) = (d[best_i], d[best_i + 1]);
    let along = [da[0] + (db[0] - da[0]) * best_u, da[1] + (db[1] - da[1]) * best_u];
    let (tx, ty) = (db[0] - da[0], db[1] - da[1]);
    let tl = (tx * tx + ty * ty).sqrt().max(1e-6);
    let mut new = [along[0] + (-ty / tl) * side, along[1] + (tx / tl) * side];
    // the picture turns about the curve by the turn at this place (between the turns of the two control points around it)
    if turns.len() == rest.len() {
        let at = (best_i as f32 + best_u) / 8.0;
        let k = (at.floor() as usize).min(rest.len() - 2);
        let f = (at - k as f32).clamp(0.0, 1.0);
        let deg = turns[k] + (turns[k + 1] - turns[k]) * f;
        if deg != 0.0 {
            let (s, c) = deg.to_radians().sin_cos();
            let (vx, vy) = (new[0] - along[0], new[1] - along[1]);
            new = [along[0] + vx * c - vy * s, along[1] + vx * s + vy * c];
        }
    }
    // full strength close to the curve, fading out beyond `width`
    let w = width.max(1.0);
    let fade = if best <= w {
        1.0
    } else {
        let t = ((best - w) / w).min(1.0);
        1.0 - t * t * (3.0 - 2.0 * t)
    };
    ([p[0] + (new[0] - p[0]) * fade, p[1] + (new[1] - p[1]) * fade], fade)
}

impl Values {
    pub fn set(&mut self, id: Id, v: f32) {
        self.0.insert(id, v);
    }
}

impl Rig {
    pub fn is_empty(&self) -> bool {
        self.meshes.is_empty() && self.changers.is_empty() && self.params.is_empty() && self.skin.is_none() && self.auto.is_none() && self.iks.is_empty() && self.poses.is_empty() && self.clips.is_empty() && self.pads.is_empty() && self.physics.is_empty() && self.colliders.is_empty()
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
        self.params.push(Param { id, name: name.chars().filter(|c| !c.is_control()).take(40).collect(), min, max, default: default.clamp(min, max), cid: String::new() });
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

    /// Give a Warp changer a finer (or coarser) grid: every shape saved for it is taken over, the new points on the old surface. False when the
    /// changer is not a Warp or the size is not allowed.
    pub fn resize_warp(&mut self, cid: Id, cols: u32, rows: u32) -> bool {
        if !(1..=MAX_WARP).contains(&cols) || !(1..=MAX_WARP).contains(&rows) {
            return false;
        }
        let Some(c) = self.changers.iter_mut().find(|c| c.id == cid) else { return false };
        let Kind::Warp { cols: oc, rows: or, .. } = &mut c.kind else { return false };
        let (old_c, old_r) = (*oc, *or);
        *oc = cols;
        *or = rows;
        for b in self.bindings.iter_mut().filter(|b| b.changer == cid) {
            for f in b.forms.iter_mut() {
                if let Form::Warp { offsets } = f {
                    if offsets.len() != ((old_c + 1) * (old_r + 1)) as usize {
                        continue;
                    }
                    let at = |c: u32, r: u32| offsets[(r * (old_c + 1) + c) as usize];
                    let mut out = Vec::with_capacity(((cols + 1) * (rows + 1)) as usize);
                    for r in 0..=rows {
                        for cc in 0..=cols {
                            // where the new point is on the old grid
                            let (x, y) = (cc as f32 / cols as f32 * old_c as f32, r as f32 / rows as f32 * old_r as f32);
                            let (x0, y0) = ((x.floor() as u32).min(old_c), (y.floor() as u32).min(old_r));
                            let (x1, y1) = ((x0 + 1).min(old_c), (y0 + 1).min(old_r));
                            let (tx, ty) = (x - x0 as f32, y - y0 as f32);
                            let (a, b, c2, d) = (at(x0, y0), at(x1, y0), at(x0, y1), at(x1, y1));
                            out.push([0, 1].map(|k| (a[k] * (1.0 - tx) + b[k] * tx) * (1.0 - ty) + (c2[k] * (1.0 - tx) + d[k] * tx) * ty));
                        }
                    }
                    *offsets = out;
                }
            }
        }
        true
    }

    /// The shape a changer has when nothing is moved.
    pub fn rest_form(&self, changer: Id) -> Option<Form> {
        Some(match &self.changer(changer)?.kind {
            Kind::Rotation { .. } => Form::Rot { deg: 0.0 },
            Kind::Warp { cols, rows, .. } => Form::Warp { offsets: vec![[0.0, 0.0]; ((cols + 1) * (rows + 1)) as usize] },
            Kind::Line { pts, .. } => Form::Line { offsets: vec![[0.0, 0.0]; pts.len()], turns: Vec::new() },
            Kind::Soft { .. } => Form::Soft { offset: [0.0, 0.0], grow: 0.0 },
            Kind::Paths { paths } => Form::Paths { offsets: paths.iter().map(|p| vec![[0.0, 0.0]; p.pts.len()]).collect(), turns: Vec::new() },
            Kind::Pins { pts } => Form::Pins { offsets: vec![[0.0, 0.0]; pts.len()] },
            k => Form::Vals { v: changers::rest_vals(k).unwrap_or_default() },
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
        self.bindings.push(Binding { changer, params: vec![param], keys: vec![keys], forms, ease: Ease::Linear });
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
                self.bindings.push(Binding { changer, params: vec![param], keys: vec![keys], forms: vec![rest.clone()], ease: Ease::Linear });
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
                let t = b.ease.apply(t);
                Some(lerp_form(b.forms.get(i0)?, b.forms.get(i1)?, t))
            }
            2 => {
                let nx = b.keys[0].len();
                let (x0, x1, tx) = axis_weights(&b.keys[0], self.value(vals, b.params[0]));
                let tx = b.ease.apply(tx);
                let (y0, y1, ty) = axis_weights(&b.keys[1], self.value(vals, b.params[1]));
                let ty = b.ease.apply(ty);
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
                Form::Line { offsets, turns } => {
                    let e = pose.line.entry(b.changer).or_insert_with(|| vec![[0.0, 0.0]; offsets.len()]);
                    if e.len() == offsets.len() {
                        for (a, o) in e.iter_mut().zip(&offsets) {
                            a[0] += o[0];
                            a[1] += o[1];
                        }
                        if turns.len() == offsets.len() {
                            let t = pose.line_turns.entry(b.changer).or_insert_with(|| vec![0.0; offsets.len()]);
                            for (a, x) in t.iter_mut().zip(&turns) {
                                *a += x;
                            }
                        }
                    }
                }
                Form::Vals { v } => {
                    let rest = self.changer(b.changer).and_then(|c| changers::rest_vals(&c.kind)).unwrap_or_default();
                    if rest.len() == v.len() {
                        let e = pose.vals.entry(b.changer).or_insert_with(|| rest.clone());
                        for ((a, x), r) in e.iter_mut().zip(&v).zip(&rest) {
                            *a += x - r;
                        }
                    }
                }
                Form::Pins { offsets } => {
                    let e = pose.pins.entry(b.changer).or_insert_with(|| vec![[0.0, 0.0]; offsets.len()]);
                    if e.len() == offsets.len() {
                        for (a, o) in e.iter_mut().zip(&offsets) {
                            a[0] += o[0];
                            a[1] += o[1];
                        }
                    }
                }
                Form::Paths { offsets, turns } => {
                    let e = pose.paths.entry(b.changer).or_insert_with(|| offsets.iter().map(|o| vec![[0.0, 0.0]; o.len()]).collect());
                    if e.len() == offsets.len() && e.iter().zip(&offsets).all(|(a, b)| a.len() == b.len()) {
                        for (ea, oa) in e.iter_mut().zip(&offsets) {
                            for (a, o) in ea.iter_mut().zip(oa) {
                                a[0] += o[0];
                                a[1] += o[1];
                            }
                        }
                        if turns.len() == offsets.len() && turns.iter().zip(&offsets).all(|(t, o)| t.len() == o.len()) {
                            let t = pose.path_turns.entry(b.changer).or_insert_with(|| offsets.iter().map(|o| vec![0.0; o.len()]).collect());
                            for (ta, tb) in t.iter_mut().zip(&turns) {
                                for (a, x) in ta.iter_mut().zip(tb) {
                                    *a += x;
                                }
                            }
                        }
                    }
                }
                Form::Soft { offset, grow } => {
                    let e = pose.soft.entry(b.changer).or_insert(([0.0, 0.0], 0.0));
                    e.0[0] += offset[0];
                    e.0[1] += offset[1];
                    e.1 += grow;
                }
            }
        }
        for ik in &self.iks {
            let target = [ik.tip[0] + self.value(vals, ik.px), ik.tip[1] + self.value(vals, ik.py)];
            let (a, b) = solve_two_bone(ik.root, ik.joint, ik.tip, target);
            *pose.rot.entry(ik.upper).or_insert(0.0) += a;
            *pose.rot.entry(ik.lower).or_insert(0.0) += b;
        }
        pose
    }

    /// Does a shape of a movement fit a changer (a turn fits a turning changer, the points of a warp fit a warp of the same grid...)?
    fn form_fits(form: &Form, kind: &Kind, like: &Kind) -> bool {
        match (form, kind) {
            (Form::Rot { .. }, Kind::Rotation { .. }) => true,
            (Form::Warp { offsets }, Kind::Warp { cols, rows, .. }) => offsets.len() == (*cols as usize + 1) * (*rows as usize + 1),
            (Form::Line { offsets, .. }, Kind::Line { pts, .. }) => offsets.len() == pts.len(),
            (Form::Soft { .. }, Kind::Soft { .. }) => true,
            (Form::Pins { offsets }, Kind::Pins { pts }) => offsets.len() == pts.len(),
            (Form::Paths { offsets, .. }, Kind::Paths { paths }) => offsets.len() == paths.len() && offsets.iter().zip(paths).all(|(o, p)| o.len() == p.pts.len()),
            (Form::Vals { .. }, k) => std::mem::discriminant(k) == std::mem::discriminant(like) && matches!(k, Kind::Move | Kind::Scale { .. } | Kind::Flip { .. } | Kind::Fade | Kind::Order | Kind::Wave { .. }),
            _ => false,
        }
    }

    /// The other changers move the same way as this one, with the same sliders and the same keys: the movement of `from` is copied to every changer of `to`
    /// that it fits (the same kind, the same number of points). With `mirror` the movement is turned to the other side (an arm on the other side turns the other way). What a
    /// target did with these sliders before is replaced. Returns how many changers got it.
    pub fn copy_movement(&mut self, from: Id, to: &[Id], mirror: bool) -> usize {
        let Some(src) = self.changer(from).map(|c| c.kind.clone()) else { return 0 };
        let sources: Vec<Binding> = self.bindings.iter().filter(|b| b.changer == from).cloned().collect();
        if sources.is_empty() {
            return 0;
        }
        let mut done = 0;
        for &t in to.iter().filter(|t| **t != from) {
            let Some(kind) = self.changer(t).map(|c| c.kind.clone()) else { continue };
            if !sources.iter().all(|b| b.forms.iter().all(|f| Self::form_fits(f, &kind, &src))) {
                continue;
            }
            for b in &sources {
                self.bindings.retain(|x| !(x.changer == t && x.params == b.params));
                let mut c = b.clone();
                c.changer = t;
                if mirror {
                    for f in &mut c.forms {
                        Self::mirror_form(f, &kind);
                    }
                }
                self.bindings.push(c);
            }
            done += 1;
        }
        done
    }

    /// A shape of a movement turned to the other side (left and right swapped): what a changer on the other side does with the same slider.
    fn mirror_form(f: &mut Form, kind: &Kind) {
        match (f, kind) {
            (Form::Rot { deg }, _) => *deg = -*deg,
            (Form::Warp { offsets }, Kind::Warp { cols, rows, .. }) => {
                let cols = *cols as usize;
                let old = offsets.clone();
                for r in 0..=*rows as usize {
                    for col in 0..=cols {
                        let o = old[r * (cols + 1) + (cols - col)];
                        offsets[r * (cols + 1) + col] = [-o[0], o[1]];
                    }
                }
            }
            (Form::Line { offsets, turns }, _) => {
                offsets.iter_mut().for_each(|o| o[0] = -o[0]);
                turns.iter_mut().for_each(|t| *t = -*t);
            }
            (Form::Soft { offset, .. }, _) => offset[0] = -offset[0],
            (Form::Pins { offsets }, _) => offsets.iter_mut().for_each(|o| o[0] = -o[0]),
            (Form::Paths { offsets, turns }, _) => {
                offsets.iter_mut().flatten().for_each(|o| o[0] = -o[0]);
                turns.iter_mut().flatten().for_each(|t| *t = -*t);
            }
            (Form::Vals { v }, Kind::Move) => v[0] = -v[0],
            (Form::Vals { v }, Kind::Wave { .. }) if v.len() > 3 => v[3] = 180.0 - v[3],
            _ => {}
        }
    }

    /// A copy of a changer on the other side of the vertical line x = `axis_x`, moved by the same sliders
    /// (turns the other way, bends the other way). Returns its id.
    pub fn mirror_changer(&mut self, id: Id, axis_x: f32, name: &str) -> Option<Id> {
        let c = self.changer(id)?.clone();
        let flip = |p: [f32; 2]| [2.0 * axis_x - p[0], p[1]];
        let kind = match &c.kind {
            Kind::Rotation { pivot } => Kind::Rotation { pivot: flip(*pivot) },
            Kind::Warp { rect, cols, rows } => Kind::Warp { rect: [2.0 * axis_x - rect[2], rect[1], 2.0 * axis_x - rect[0], rect[3]], cols: *cols, rows: *rows },
            Kind::Line { pts, width } => Kind::Line { pts: pts.iter().map(|p| flip(*p)).collect(), width: *width },
            Kind::Soft { center, radius } => Kind::Soft { center: flip(*center), radius: *radius },
            Kind::Paths { paths } => Kind::Paths { paths: paths.iter().map(|sp| PathSpot { pts: sp.pts.iter().map(|p| flip(*p)).collect(), width: sp.width }).collect() },
            Kind::Pins { pts } => Kind::Pins { pts: pts.iter().map(|p| flip(*p)).collect() },
            Kind::Move => Kind::Move,
            Kind::Scale { center } => Kind::Scale { center: flip(*center) },
            Kind::Flip { center, vertical } => Kind::Flip { center: flip(*center), vertical: *vertical },
            Kind::Fade => Kind::Fade,
            Kind::Order => Kind::Order,
            Kind::Wave { center } => Kind::Wave { center: flip(*center) },
        };
        let new = self.add_changer(name, kind.clone(), c.parent);
        let copies: Vec<Binding> = self.bindings.iter().filter(|b| b.changer == id).cloned().collect();
        for mut b in copies {
            b.changer = new;
            for f in &mut b.forms {
                Self::mirror_form(f, &kind);
            }
            self.bindings.push(b);
        }
        Some(new)
    }

    /// The key points of a slider: where a changer has a saved shape for it, plus the original (default) point, in order.
    /// For a slider with two key points the original drawing is at one end, with three it is in the middle.
    pub fn key_points(&self, param: Id) -> Vec<f32> {
        let Some(p) = self.param(param) else { return Vec::new() };
        let mut v: Vec<f32> = self.bindings.iter().filter(|b| b.params.contains(&param)).flat_map(|b| b.keys[b.params.iter().position(|x| *x == param).unwrap_or(0)].clone()).collect();
        v.push(p.default);
        v.sort_by(|a, b| a.total_cmp(b));
        v.dedup_by(|a, b| (*a - *b).abs() < 1e-4);
        v
    }

    /// The points drawn on a slider: its key points plus both ends and the original, in order (like the parameter
    /// palettes of Live2D and Inochi2D: two points for an open-close slider, three for an angle).
    pub fn display_points(&self, param: Id) -> Vec<f32> {
        let Some(p) = self.param(param) else { return Vec::new() };
        let mut v = self.key_points(param);
        v.extend([p.min, p.max, p.default]);
        v.sort_by(|a, b| a.total_cmp(b));
        v.dedup_by(|a, b| (*a - *b).abs() < 1e-4);
        v
    }

    /// Has a shape been saved at this point of the slider? (The original drawing does not count: it is always there.)
    pub fn key_is_set(&self, param: Id, value: f32) -> bool {
        let default = self.param(param).map(|p| p.default).unwrap_or(0.0);
        if (value - default).abs() < 1e-4 {
            return false;
        }
        self.bindings.iter().any(|b| b.params.len() == 1 && b.params[0] == param && b.keys[0].iter().any(|k| (k - value).abs() < 1e-4))
    }

    /// Put a key point of a slider back to the original drawing, for every changer that uses it.
    pub fn reset_key(&mut self, param: Id, value: f32) {
        let ids: Vec<(usize, Id)> = self.bindings.iter().enumerate().filter(|(_, b)| b.params == [param]).map(|(i, b)| (i, b.changer)).collect();
        for (bi, changer) in ids {
            let Some(rest) = self.rest_form(changer) else { continue };
            let b = &mut self.bindings[bi];
            if let Some(k) = b.keys[0].iter().position(|k| (k - value).abs() < 1e-4) {
                b.forms[k] = rest;
            }
        }
    }

    /// Take a key point away (the default point stays: it is the original drawing).
    pub fn remove_key(&mut self, param: Id, value: f32) {
        let default = self.param(param).map(|p| p.default).unwrap_or(0.0);
        if (value - default).abs() < 1e-4 {
            self.reset_key(param, value);
            return;
        }
        for b in self.bindings.iter_mut().filter(|b| b.params == [param]) {
            if let Some(k) = b.keys[0].iter().position(|k| (k - value).abs() < 1e-4) {
                b.keys[0].remove(k);
                b.forms.remove(k);
            }
        }
        self.bindings.retain(|b| !(b.params.len() == 1 && b.keys[0].is_empty()));
    }

    /// The slider values that are not at their default, ready to keep.
    pub fn values_to_keep(&self, vals: &Values) -> Vec<(Id, f32)> {
        self.params.iter().filter_map(|p| vals.0.get(&p.id).filter(|v| (**v - p.default).abs() > 1e-4).map(|v| (p.id, *v))).collect()
    }

    /// The other way: sliders not listed are at their default.
    pub fn values_from(&self, kept: &[(Id, f32)]) -> Values {
        let mut v = Values::default();
        for (id, x) in kept {
            if let Some(p) = self.param(*id) {
                v.set(*id, x.clamp(p.min, p.max));
            }
        }
        v
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
                if self.smooth.contains(&c.id) && off.len() == ((cols + 1) * (rows + 1)) as usize {
                    let d = refine::smooth_shift(off, *cols, *rows, u, v);
                    return [p[0] + d[0], p[1] + d[1]];
                }
                let idx = |c2: u32, r2: u32| (r2 * (cols + 1) + c2) as usize;
                let (a, b2, c3, d) = (off[idx(i, j)], off[idx(i + 1, j)], off[idx(i, j + 1)], off[idx(i + 1, j + 1)]);
                let mix = |k: usize| (a[k] * (1.0 - fu) + b2[k] * fu) * (1.0 - fv) + (c3[k] * (1.0 - fu) + d[k] * fu) * fv;
                [p[0] + mix(0), p[1] + mix(1)]
            }
            Kind::Line { pts, width } => {
                let Some(off) = pose.line.get(&c.id) else { return p };
                bend_along(pts, off, pose.line_turns.get(&c.id).map(Vec::as_slice).unwrap_or(&[]), *width, p).0
            }
            Kind::Paths { paths } => match pose.paths.get(&c.id) {
                Some(off) => changers::blend_paths(paths, off, pose.path_turns.get(&c.id).map(Vec::as_slice).unwrap_or(&[]), p),
                None => p,
            },
            Kind::Pins { pts } => match pose.pins.get(&c.id) {
                Some(off) => changers::pin_shift(pts, off, p),
                None => p,
            },
            Kind::Move | Kind::Scale { .. } | Kind::Flip { .. } | Kind::Wave { .. } => match pose.vals.get(&c.id) {
                Some(v) => changers::apply_vals(&c.kind, v, p),
                None => p,
            },
            Kind::Fade | Kind::Order => p,
            Kind::Soft { center, radius } => {
                let Some((offset, grow)) = pose.soft.get(&c.id) else { return p };
                let d = ((p[0] - center[0]).hypot(p[1] - center[1]) / radius.max(1e-3)).min(1.0);
                let f = 0.5 * (1.0 + (std::f32::consts::PI * d).cos());
                [p[0] + offset[0] * f + (p[0] - center[0]) * grow * f, p[1] + offset[1] * f + (p[1] - center[1]) * grow * f]
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

    /// Every mesh moved by its changers, then the seams pulled together. Same order as `meshes`.
    pub fn deformed_all(&self, pose: &Pose) -> Vec<Vec<[f32; 2]>> {
        let mut all: Vec<Vec<[f32; 2]>> = self.meshes.iter().map(|m| self.deformed(m, pose)).collect();
        for g in &self.glues {
            let (Some(ia), Some(ib)) = (self.meshes.iter().position(|m| m.id == g.a), self.meshes.iter().position(|m| m.id == g.b)) else { continue };
            if ia == ib {
                continue;
            }
            for pr in &g.pairs {
                let (pa, pb) = (all[ia].get(pr[0] as usize).copied(), all[ib].get(pr[1] as usize).copied());
                if let (Some(pa), Some(pb)) = (pa, pb) {
                    let mid = [(pa[0] + pb[0]) / 2.0, (pa[1] + pb[1]) / 2.0];
                    let k = g.weight.clamp(0.0, 1.0);
                    all[ia][pr[0] as usize] = [pa[0] + (mid[0] - pa[0]) * k, pa[1] + (mid[1] - pa[1]) * k];
                    all[ib][pr[1] as usize] = [pb[0] + (mid[0] - pb[0]) * k, pb[1] + (mid[1] - pb[1]) * k];
                }
            }
        }
        all
    }

    /// Glue two meshes along the places where their vertices are within `reach` of each other at rest.
    /// Returns how many vertex pairs were joined (0 = the meshes don't touch).
    pub fn make_glue(&mut self, a: Id, b: Id, reach: f32, weight: f32) -> usize {
        let (Some(ma), Some(mb)) = (self.meshes.iter().find(|m| m.id == a), self.meshes.iter().find(|m| m.id == b)) else { return 0 };
        let mut pairs = Vec::new();
        for (i, va) in ma.verts.iter().enumerate() {
            let near = mb.verts.iter().enumerate().map(|(j, vb)| (j, (va[0] - vb[0]).hypot(va[1] - vb[1]))).filter(|(_, d)| *d <= reach).min_by(|x, y| x.1.total_cmp(&y.1));
            if let Some((j, _)) = near {
                pairs.push([i as u32, j as u32]);
            }
        }
        if pairs.is_empty() {
            return 0;
        }
        let n = pairs.len();
        self.glues.retain(|g| !((g.a == a && g.b == b) || (g.a == b && g.b == a)));
        self.glues.push(Glue { a, b, pairs, weight: weight.clamp(0.0, 1.0) });
        n
    }

    /// Put a new key into a binding's axis. The forms around it are blended, so nothing on screen changes.
    fn insert_axis_key(&mut self, bi: usize, axis: usize, v: f32) {
        let b = &mut self.bindings[bi];
        if b.keys[axis].iter().any(|k| (k - v).abs() < 1e-4) {
            return;
        }
        let nx = b.keys[0].len();
        let ny = if b.keys.len() > 1 { b.keys[1].len() } else { 1 };
        let (i0, i1, t) = axis_weights(&b.keys[axis], v);
        let at = if v <= b.keys[axis][0] { 0 } else if v >= *b.keys[axis].last().unwrap() { b.keys[axis].len() } else { i1 };
        let get = |forms: &[Form], x: usize, y: usize| forms[y * nx + x].clone();
        let mut out: Vec<Form> = Vec::new();
        let (nnx, nny) = if axis == 0 { (nx + 1, ny) } else { (nx, ny + 1) };
        for y in 0..nny {
            for x in 0..nnx {
                let (sx, sy) = (x, y);
                let cell = if axis == 0 {
                    if x < at { get(&b.forms, sx, sy) } else if x == at { lerp_form(&get(&b.forms, i0.min(nx - 1), sy), &get(&b.forms, i1.min(nx - 1), sy), t) } else { get(&b.forms, sx - 1, sy) }
                } else if y < at {
                    get(&b.forms, sx, sy)
                } else if y == at {
                    lerp_form(&get(&b.forms, sx, i0.min(ny - 1)), &get(&b.forms, sx, i1.min(ny - 1)), t)
                } else {
                    get(&b.forms, sx, sy - 1)
                };
                out.push(cell);
            }
        }
        b.keys[axis].insert(at, v);
        b.forms = out;
    }

    /// The shape of a changer when two sliders have these values (a grid of shapes, like head left-right by up-down).
    pub fn set_keyform2(&mut self, changer: Id, px: Id, py: Id, vx: f32, vy: f32, form: Form) {
        let (Some(a), Some(b)) = (self.param(px).cloned(), self.param(py).cloned()) else { return };
        let Some(rest) = self.rest_form(changer) else { return };
        let bi = match self.bindings.iter().position(|x| x.changer == changer && x.params == [px, py]) {
            Some(i) => i,
            None => {
                let (kx, ky) = (vec![a.default], vec![b.default]);
                self.bindings.push(Binding { changer, params: vec![px, py], keys: vec![kx, ky], forms: vec![rest], ease: Ease::Linear });
                self.bindings.len() - 1
            }
        };
        self.insert_axis_key(bi, 0, vx);
        self.insert_axis_key(bi, 1, vy);
        let bd = &mut self.bindings[bi];
        let nx = bd.keys[0].len();
        let (ix, iy) = (bd.keys[0].iter().position(|k| (k - vx).abs() < 1e-4).unwrap_or(0), bd.keys[1].iter().position(|k| (k - vy).abs() < 1e-4).unwrap_or(0));
        bd.forms[iy * nx + ix] = form;
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
            Kind::Line { pts, width } => {
                pts.truncate(MAX_LINE_POINTS);
                *width = if fin(*width) { width.clamp(1.0, 5000.0) } else { 50.0 };
                pts.len() >= 2 && pts.iter().all(|p| fin(p[0]) && fin(p[1]))
            }
            Kind::Soft { center, radius } => {
                *radius = if fin(*radius) { radius.clamp(1.0, 10000.0) } else { 100.0 };
                center.iter().all(|v| fin(*v))
            }
            Kind::Paths { paths } => {
                paths.truncate(changers::MAX_PATHS);
                for sp in paths.iter_mut() {
                    sp.pts.truncate(MAX_LINE_POINTS);
                    sp.width = if fin(sp.width) { sp.width.clamp(1.0, 5000.0) } else { 50.0 };
                }
                paths.retain(|sp| sp.pts.len() >= 2 && sp.pts.iter().all(|p| fin(p[0]) && fin(p[1])));
                !paths.is_empty()
            }
            Kind::Pins { pts } => {
                pts.truncate(changers::MAX_PINS);
                !pts.is_empty() && pts.iter().all(|p| fin(p[0]) && fin(p[1]))
            }
            Kind::Scale { center } | Kind::Wave { center } => center.iter().all(|v| fin(*v)),
            Kind::Flip { center, .. } => center.iter().all(|v| fin(*v)),
            Kind::Move | Kind::Fade | Kind::Order => true,
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
                (Form::Line { offsets, turns }, Kind::Line { pts, .. }) => offsets.len() == pts.len() && offsets.iter().all(|o| fin(o[0]) && fin(o[1])) && (turns.is_empty() || (turns.len() == pts.len() && turns.iter().all(|t| fin(*t)))),
                (Form::Soft { offset, grow }, Kind::Soft { .. }) => fin(offset[0]) && fin(offset[1]) && fin(*grow),
                (Form::Pins { offsets }, Kind::Pins { pts }) => offsets.len() == pts.len() && offsets.iter().all(|o| fin(o[0]) && fin(o[1])),
                (Form::Paths { offsets, turns }, Kind::Paths { paths }) => offsets.len() == paths.len() && offsets.iter().zip(paths).all(|(o, sp)| o.len() == sp.pts.len() && o.iter().all(|v| fin(v[0]) && fin(v[1]))) && (turns.is_empty() || (turns.len() == paths.len() && turns.iter().zip(paths).all(|(t, sp)| t.len() == sp.pts.len() && t.iter().all(|x| fin(*x))))),
                (Form::Vals { v }, k) => changers::rest_vals(k).is_some_and(|r| r.len() == v.len()) && v.iter().all(|x| fin(*x)),
                _ => false,
            })
        });
        // the skin keeps only bones that are still Rotation changers
        let rot_ids: HashSet<Id> = self.changers.iter().filter(|c| matches!(c.kind, Kind::Rotation { .. })).map(|c| c.id).collect();
        self.skin = self.skin.take().and_then(|sk| sk.sanitized()).map(|mut sk| {
            let keep: Vec<bool> = sk.bones.iter().map(|b| rot_ids.contains(&b.changer)).collect();
            let mut i = 0;
            sk.bones.retain(|_| {
                i += 1;
                keep[i - 1]
            });
            let mut i = 0;
            sk.planes.retain(|_| {
                i += 1;
                keep.get(i - 1).copied().unwrap_or(false)
            });
            sk
        });
        let counts: HashMap<Id, usize> = self.meshes.iter().map(|m| (m.id, m.verts.len())).collect();
        self.glues.truncate(2000);
        self.glues.retain_mut(|g| {
            let (Some(na), Some(nb)) = (counts.get(&g.a).copied(), counts.get(&g.b).copied()) else { return false };
            g.weight = if g.weight.is_finite() { g.weight.clamp(0.0, 1.0) } else { 1.0 };
            g.pairs.retain(|p| (p[0] as usize) < na && (p[1] as usize) < nb);
            g.pairs.truncate(100_000);
            g.a != g.b && !g.pairs.is_empty()
        });
        {
            let rot_ids: HashSet<Id> = self.changers.iter().filter(|c| matches!(c.kind, Kind::Rotation { .. })).map(|c| c.id).collect();
            let pids: HashSet<Id> = self.params.iter().map(|p| p.id).collect();
            self.iks.truncate(64);
            self.iks.retain(|k| rot_ids.contains(&k.upper) && rot_ids.contains(&k.lower) && k.upper != k.lower && pids.contains(&k.px) && pids.contains(&k.py) && [k.root, k.joint, k.tip].iter().all(|p| p[0].is_finite() && p[1].is_finite()));
        }
        {
            let pids: HashSet<Id> = self.params.iter().map(|p| p.id).collect();
            let clean = |v: &mut Vec<(Id, f32)>| {
                v.retain(|(id, x)| pids.contains(id) && x.is_finite());
                v.truncate(500);
            };
            self.poses.truncate(200);
            for p in &mut self.poses {
                clean(&mut p.values);
                p.name = p.name.chars().filter(|c| !c.is_control()).take(40).collect();
            }
            self.clips.truncate(50);
            for c in &mut self.clips {
                c.fps = if c.fps.is_finite() { c.fps.clamp(1.0, 120.0) } else { 30.0 };
                c.frames.truncate(20_000);
                for f in &mut c.frames {
                    clean(f);
                }
                c.name = c.name.chars().filter(|c| !c.is_control()).take(40).collect();
            }
        }
        {
            let warps: HashSet<Id> = self.changers.iter().filter(|c| matches!(c.kind, Kind::Warp { .. })).map(|c| c.id).collect();
            self.smooth.retain(|id| warps.contains(id));
            self.smooth.sort_unstable();
            self.smooth.dedup();
        }
        {
            let lines: HashSet<Id> = self.changers.iter().filter(|c| matches!(c.kind, Kind::Line { .. })).map(|c| c.id).collect();
            self.physics.truncate(500);
            self.physics.retain(|p| lines.contains(&p.changer));
            for p in &mut self.physics {
                let d = physics::PhysChain::new("", p.kind, p.material, p.changer);
                let fix = |v: &mut f32, lo: f32, hi: f32, dflt: f32| *v = if v.is_finite() { v.clamp(lo, hi) } else { dflt };
                fix(&mut p.stiff_root, 0.0, 1.0, d.stiff_root);
                fix(&mut p.stiff_tip, 0.0, 1.0, d.stiff_tip);
                fix(&mut p.damping, 0.0, 0.98, d.damping);
                fix(&mut p.gravity, -2.0, 4.0, d.gravity);
                fix(&mut p.wind, 0.0, 4.0, d.wind);
                p.name = p.name.chars().filter(|c| !c.is_control()).take(40).collect();
            }
            let all: HashSet<Id> = self.changers.iter().map(|c| c.id).collect();
            self.colliders.truncate(64);
            self.colliders.retain_mut(|c| {
                c.name = c.name.chars().filter(|c| !c.is_control()).take(40).collect();
                if c.changer.is_some_and(|i| !all.contains(&i)) {
                    c.changer = None;
                }
                c.center.iter().all(|v| v.is_finite()) && c.radius.is_finite() && c.radius > 0.0
            });
            let w = &mut self.world;
            w.gravity = if w.gravity.is_finite() { w.gravity.clamp(0.0, 20000.0) } else { 2400.0 };
            if w.gravity <= 0.0 {
                w.gravity = 2400.0;
            }
            w.wind_dir = if w.wind_dir.is_finite() { w.wind_dir.clamp(-1.0, 1.0) } else { 1.0 };
        }
        {
            let pids: HashSet<Id> = self.params.iter().map(|p| p.id).collect();
            self.pads.truncate(200);
            self.pads.retain(|p| p.x != p.y && pids.contains(&p.x) && pids.contains(&p.y));
            for p in &mut self.pads {
                p.name = p.name.chars().filter(|c| !c.is_control()).take(40).collect();
            }
        }
        if let Some(a) = self.auto.as_mut() {
            let cids: HashSet<Id> = self.changers.iter().map(|c| c.id).collect();
            let pids: HashSet<Id> = self.params.iter().map(|p| p.id).collect();
            a.changers.retain(|c| cids.contains(c));
            a.params.retain(|p| pids.contains(p));
            a.locked.retain(|c| cids.contains(c));
        }
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
        r.bindings.push(Binding { changer: c, params: vec![x, y], keys: vec![vec![-1.0, 1.0], vec![-1.0, 1.0]], forms: vec![Form::Rot { deg: -40.0 }, Form::Rot { deg: -20.0 }, Form::Rot { deg: 20.0 }, Form::Rot { deg: 40.0 }], ease: Ease::Linear });
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
        r.bindings.push(Binding { changer: 777, params: vec![tilt], keys: vec![vec![0.0]], forms: vec![Form::Rot { deg: 1.0 }], ease: Ease::Linear });
        r.bindings.push(Binding { changer: head, params: vec![tilt], keys: vec![vec![0.0, 1.0]], forms: vec![Form::Rot { deg: 1.0 }], ease: Ease::Linear });
        r.bindings.push(Binding { changer: head, params: vec![tilt], keys: vec![vec![0.0]], forms: vec![Form::Warp { offsets: vec![] }], ease: Ease::Linear });
        r.bindings.push(Binding { changer: head, params: vec![tilt], keys: vec![vec![f32::NAN]], forms: vec![Form::Rot { deg: 1.0 }], ease: Ease::Linear });
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

    #[test]
    fn a_line_changer_bends_the_picture_along_its_curve() {
        let mut r = Rig::default();
        // a vertical strand from (100,0) to (100,300) with 4 control points
        let pts = vec![[100.0, 0.0], [100.0, 100.0], [100.0, 200.0], [100.0, 300.0]];
        let line = r.add_changer("Strand", Kind::Line { pts: pts.clone(), width: 40.0 }, None);
        let p = r.add_param("Sway", 0.0, 1.0, 0.0);
        // the tip (last point) swings 60 to the right, the middle points a little
        r.set_keyform(line, p, 1.0, Form::Line { offsets: vec![[0.0, 0.0], [5.0, 0.0], [25.0, 0.0], [60.0, 0.0]], turns: vec![] });
        let mut v = Values::default();
        let at_rest = r.deform_point(&r.pose(&v), Some(line), [110.0, 300.0]);
        assert!((at_rest[0] - 110.0).abs() < 0.01 && (at_rest[1] - 300.0).abs() < 0.01);
        v.set(p, 1.0);
        let pose = r.pose(&v);
        // a point on the strand follows the curve
        let tip = r.deform_point(&pose, Some(line), [100.0, 300.0]);
        assert!((tip[0] - 160.0).abs() < 0.5 && (tip[1] - 300.0).abs() < 1.0, "{tip:?}");
        // a point 10 to the side of the tip keeps its side distance
        let side = r.deform_point(&pose, Some(line), [110.0, 300.0]);
        assert!((side[0] - tip[0] - 10.0).abs() < 1.0, "{side:?} {tip:?}");
        // the root does not move
        let root = r.deform_point(&pose, Some(line), [100.0, 0.0]);
        assert!((root[0] - 100.0).abs() < 0.5);
        // far from the strand (more than twice the width) nothing follows
        let far = r.deform_point(&pose, Some(line), [300.0, 300.0]);
        assert!((far[0] - 300.0).abs() < 0.01, "{far:?}");
        // half way between near and far it follows a part of the way
        let mid = r.deform_point(&pose, Some(line), [160.0, 300.0]);
        assert!(mid[0] > 160.0 && mid[0] < 160.0 + 60.0, "{mid:?}");
        // the curve is smooth: samples pass through the control points
        let cs = curve_samples(&pts, 8);
        assert_eq!(cs.len(), 3 * 8 + 1);
        assert!(cs.iter().any(|c| (c[1] - 100.0).abs() < 0.01));
    }

    #[test]
    fn a_soft_changer_pushes_and_grows_most_in_the_middle() {
        let mut r = Rig::default();
        let soft = r.add_changer("Cheek", Kind::Soft { center: [100.0, 100.0], radius: 50.0 }, None);
        let p = r.add_param("Puff", 0.0, 1.0, 0.0);
        r.set_keyform(soft, p, 1.0, Form::Soft { offset: [10.0, 0.0], grow: 0.2 });
        let mut v = Values::default();
        v.set(p, 1.0);
        let pose = r.pose(&v);
        let mid = r.deform_point(&pose, Some(soft), [100.0, 100.0]);
        assert!((mid[0] - 110.0).abs() < 0.01 && (mid[1] - 100.0).abs() < 0.01, "the centre is pushed by the offset: {mid:?}");
        let edge = r.deform_point(&pose, Some(soft), [150.0, 100.0]);
        assert!((edge[0] - 150.0).abs() < 0.01, "outside the radius nothing moves: {edge:?}");
        let half = r.deform_point(&pose, Some(soft), [125.0, 100.0]);
        assert!(half[0] > 125.0 && half[0] < 150.0, "in between it is soft: {half:?}");
        // it grows: points near the centre move away from it
        let grown = r.deform_point(&pose, Some(soft), [110.0, 100.0]);
        assert!(grown[0] - 110.0 > 10.0 * 0.9, "{grown:?}");
    }

    #[test]
    fn glue_pulls_the_seam_of_two_meshes_together() {
        let mut r = Rig::default();
        // two squares that touch along x = 100
        let sq = |id, layer, x0: f32| ArtMesh { id, layer, verts: vec![[x0, 0.0], [x0 + 100.0, 0.0], [x0 + 100.0, 100.0], [x0, 100.0]], tris: vec![[0, 1, 2], [0, 2, 3]], parent: None };
        r.meshes.push(sq(1, 1, 0.0));
        r.meshes.push(sq(2, 2, 100.0));
        let c = r.add_changer("Turn", Kind::Rotation { pivot: [0.0, 0.0] }, None);
        r.meshes[1].parent = Some(c);
        let p = r.add_param("Turn", 0.0, 90.0, 0.0);
        r.bind_rotation(c, p, 90.0);
        let mut v = Values::default();
        v.set(p, 10.0);
        let pose = r.pose(&v);
        let loose = r.deformed_all(&pose);
        // the seam: mesh 1's vertices 1 and 2 against mesh 2's vertices 0 and 3
        let gap = |m: &Vec<Vec<[f32; 2]>>| (m[0][2][0] - m[1][3][0]).hypot(m[0][2][1] - m[1][3][1]);
        assert!(gap(&loose) > 10.0, "without glue the seam opens: {}", gap(&loose));
        assert_eq!(r.make_glue(1, 2, 5.0, 1.0), 2, "the two vertices on the seam are paired");
        let glued = r.deformed_all(&pose);
        assert!(gap(&glued) < 0.01, "glued: {}", gap(&glued));
        r.glues[0].weight = 0.5;
        let half = r.deformed_all(&pose);
        assert!((gap(&half) - gap(&loose) / 2.0).abs() < 0.1, "half glued is half open");
        // meshes that don't touch make no glue
        r.meshes[1].verts.iter_mut().for_each(|v| v[0] += 500.0);
        assert_eq!(r.make_glue(1, 2, 5.0, 1.0), 0);
        // a glue to a missing mesh is dropped when cleaning
        r.glues.push(Glue { a: 1, b: 99, pairs: vec![[0, 0]], weight: 1.0 });
        let s = r.sanitized();
        assert!(s.glues.iter().all(|g| g.b != 99));
    }

    #[test]
    fn easing_changes_the_way_between_keys_and_two_slider_grids_grow_key_by_key() {
        assert_eq!(Ease::Linear.apply(0.5), 0.5);
        assert_eq!(Ease::Smooth.apply(0.5), 0.5);
        assert!(Ease::Smooth.apply(0.25) < 0.25 && Ease::Smooth.apply(0.75) > 0.75);
        assert!(Ease::In.apply(0.5) < 0.5 && Ease::Out.apply(0.5) > 0.5);
        let (mut r, head, tilt) = rot_rig();
        let mut v = Values::default();
        v.set(tilt, 15.0);
        let even = r.pose(&v).rot[&head];
        r.bindings[0].ease = Ease::In;
        assert!(r.pose(&v).rot[&head] < even, "slow start: {} vs {even}", r.pose(&v).rot[&head]);
        // a grid of two sliders: put keys in one at a time
        let mut g = Rig::default();
        let c = g.add_changer("Head", Kind::Rotation { pivot: [0.0, 0.0] }, None);
        let x = g.add_param("X", -30.0, 30.0, 0.0);
        let y = g.add_param("Y", -30.0, 30.0, 0.0);
        g.set_keyform2(c, x, y, 30.0, 0.0, Form::Rot { deg: 20.0 });
        g.set_keyform2(c, x, y, 0.0, 30.0, Form::Rot { deg: 10.0 });
        g.set_keyform2(c, x, y, 30.0, 30.0, Form::Rot { deg: 40.0 });
        let b = &g.bindings[0];
        assert_eq!(b.keys[0], [0.0, 30.0]);
        assert_eq!(b.keys[1], [0.0, 30.0]);
        assert_eq!(b.forms.len(), 4);
        let mut vals = Values::default();
        vals.set(x, 30.0);
        vals.set(y, 30.0);
        assert_eq!(g.pose(&vals).rot[&c], 40.0);
        vals.set(y, 0.0);
        assert_eq!(g.pose(&vals).rot[&c], 20.0);
        vals.set(x, 0.0);
        vals.set(y, 30.0);
        assert_eq!(g.pose(&vals).rot[&c], 10.0);
        vals.set(x, 15.0);
        vals.set(y, 15.0);
        assert!((g.pose(&vals).rot[&c] - (0.0 + 20.0 + 10.0 + 40.0) / 4.0).abs() < 1e-4, "the middle blends all four");
        // adding a key in the middle keeps what was on screen
        g.set_keyform2(c, x, y, 15.0, 15.0, Form::Rot { deg: 50.0 });
        assert_eq!(g.bindings[0].keys[0], [0.0, 15.0, 30.0]);
        vals.set(x, 30.0);
        vals.set(y, 30.0);
        assert_eq!(g.pose(&vals).rot[&c], 40.0, "the corners are unchanged");
        vals.set(x, 15.0);
        vals.set(y, 15.0);
        assert_eq!(g.pose(&vals).rot[&c], 50.0);
    }

    #[test]
    fn the_new_changers_survive_json_and_cleaning() {
        let mut r = Rig::default();
        let l = r.add_changer("Strand", Kind::Line { pts: vec![[0.0, 0.0], [0.0, 50.0], [0.0, 100.0]], width: 20.0 }, None);
        let s = r.add_changer("Cheek", Kind::Soft { center: [5.0, 5.0], radius: 30.0 }, None);
        let p = r.add_param("A", 0.0, 1.0, 0.0);
        r.set_keyform(l, p, 1.0, Form::Line { offsets: vec![[1.0, 0.0]; 3], turns: vec![] });
        r.set_keyform(s, p, 1.0, Form::Soft { offset: [2.0, 1.0], grow: 0.1 });
        r.bindings[0].ease = Ease::Smooth;
        let back: Rig = serde_json::from_str(&serde_json::to_string(&r).unwrap()).unwrap();
        assert_eq!(back, r);
        assert_eq!(back.clone().sanitized(), r.clone().sanitized());
        assert_eq!(back.bindings[0].ease, Ease::Smooth);
        // a line with a wrong number of offsets and a soft with nan are dropped
        let mut bad = r.clone();
        bad.bindings.push(Binding { changer: l, params: vec![p], keys: vec![vec![0.0]], forms: vec![Form::Line { offsets: vec![[0.0, 0.0]], turns: vec![] }], ease: Ease::Linear });
        bad.bindings.push(Binding { changer: s, params: vec![p], keys: vec![vec![0.0]], forms: vec![Form::Soft { offset: [f32::NAN, 0.0], grow: 0.0 }], ease: Ease::Linear });
        assert_eq!(bad.sanitized().bindings.len(), 2);
        // old project files (a binding without an easing) still open
        let old = r#"{"changer":1,"params":[2],"keys":[[0.0]],"forms":[{"type":"rot","deg":0.0}]}"#;
        let b: Binding = serde_json::from_str(old).unwrap();
        assert_eq!(b.ease, Ease::Linear);
    }

    #[test]
    fn two_bone_ik_brings_the_tip_to_the_target() {
        let (root, joint, tip) = ([100.0, 100.0], [100.0, 200.0], [100.0, 300.0]);
        // at rest nothing turns
        let (a, b) = solve_two_bone(root, joint, tip, tip);
        assert!(a.abs() < 0.01 && b.abs() < 0.01, "{a} {b}");
        // build the arm as changers and check where the tip really goes, for several targets
        let mut r = Rig::default();
        let upper = r.add_changer("Upper", Kind::Rotation { pivot: root }, None);
        let lower = r.add_changer("Lower", Kind::Rotation { pivot: joint }, Some(upper));
        let px = r.add_param("X", -300.0, 300.0, 0.0);
        let py = r.add_param("Y", -300.0, 300.0, 0.0);
        r.iks.push(Ik { upper, lower, root, joint, tip, px, py });
        for (dx, dy) in [(80.0, -60.0), (-120.0, -100.0), (0.0, -150.0), (100.0, -120.0), (-50.0, -40.0)] {
            let mut v = Values::default();
            v.set(px, dx);
            v.set(py, dy);
            let pose = r.pose(&v);
            let got = r.deform_point(&pose, Some(lower), tip);
            let want = [tip[0] + dx, tip[1] + dy];
            assert!((got[0] - want[0]).abs() < 0.5 && (got[1] - want[1]).abs() < 0.5, "target {want:?}, got {got:?}");
            // the elbow stays one bone length from the shoulder
            let elbow = r.deform_point(&pose, Some(upper), joint);
            assert!(((elbow[0] - root[0]).hypot(elbow[1] - root[1]) - 100.0).abs() < 0.1);
        }
        // a target that is too far: the arm stretches out straight toward it
        let mut v = Values::default();
        v.set(px, 300.0);
        v.set(py, -300.0);
        let pose = r.pose(&v);
        let got = r.deform_point(&pose, Some(lower), tip);
        let reach = ((got[0] - root[0]).hypot(got[1] - root[1])).round();
        assert!((reach - 200.0).abs() <= 1.0, "reaches as far as the arm is long: {reach}");
        // the elbow bends the same way as at rest (no flipping through the shoulder)
        let near = {
            let mut w = Values::default();
            w.set(px, 10.0);
            w.set(py, -100.0);
            let p = r.pose(&w);
            r.deform_point(&p, Some(upper), joint)
        };
        assert!(near[0].is_finite());
        // cleaning keeps a good IK and drops one that points at nothing
        let mut bad = r.clone();
        bad.iks.push(Ik { upper, lower: 9999, root, joint, tip, px, py });
        assert_eq!(bad.sanitized().iks.len(), 1);
    }

    #[test]
    fn the_movement_of_one_changer_is_copied_to_the_others_that_fit() {
        let mut r = Rig::default();
        let a = r.add_changer("Arm L", Kind::Rotation { pivot: [700.0, 300.0] }, None);
        let b = r.add_changer("Arm R", Kind::Rotation { pivot: [300.0, 300.0] }, None);
        let w = r.add_changer("Cheek", Kind::Warp { rect: [0.0, 0.0, 10.0, 10.0], cols: 1, rows: 1 }, None);
        let p = r.add_param("Wave", -1.0, 1.0, 0.0);
        r.bind_rotation(a, p, 40.0);
        let n = r.copy_movement(a, &[b, w, a], false);
        assert_eq!(n, 1, "the other rotation gets it, the warp does not fit, and a changer is not copied onto itself");
        let mut v = Values::default();
        v.set(p, 1.0);
        let pose = r.pose(&v);
        let (pa, pb) = (r.deform_point(&pose, Some(a), [800.0, 300.0]), r.deform_point(&pose, Some(b), [400.0, 300.0]));
        // both turn the same way, by the same angle, each round its own pivot
        assert!(((pa[0] - 700.0) - (pb[0] - 300.0)).abs() < 0.01 && (pa[1] - pb[1]).abs() < 0.01, "{pa:?} {pb:?}");
        assert!(r.bindings.iter().all(|x| x.changer != w));
        // doing it again does not add a second binding
        r.copy_movement(a, &[b], false);
        assert_eq!(r.bindings.iter().filter(|x| x.changer == b).count(), 1);
    }

    #[test]
    fn mirrored_movement_turns_the_other_way() {
        let mut r = Rig::default();
        let a = r.add_changer("Arm L", Kind::Rotation { pivot: [700.0, 300.0] }, None);
        let b = r.add_changer("Arm R", Kind::Rotation { pivot: [300.0, 300.0] }, None);
        let p = r.add_param("Wave", -1.0, 1.0, 0.0);
        r.bind_rotation(a, p, 40.0);
        assert_eq!(r.copy_movement(a, &[b], true), 1);
        let deg = |r: &Rig, c: Id| -> Option<f32> {
            r.bindings.iter().find(|x| x.changer == c).and_then(|x| x.forms.iter().filter_map(|f| if let Form::Rot { deg } = f { Some(*deg) } else { None }).find(|d| d.abs() > 0.0))
        };
        let (da, db) = (deg(&r, a).unwrap(), deg(&r, b).unwrap());
        assert!(da.abs() > 1.0 && (da + db).abs() < 1e-4, "{da} {db}: the other way round");
    }

    #[test]
    fn a_changer_can_be_copied_to_the_other_side_and_turns_the_other_way() {
        let mut r = Rig::default();
        let arm = r.add_changer("Arm L", Kind::Rotation { pivot: [700.0, 300.0] }, None);
        let p = r.add_param("Wave", -1.0, 1.0, 0.0);
        r.bind_rotation(arm, p, 40.0);
        let other = r.mirror_changer(arm, 500.0, "Arm R").unwrap();
        match r.changer(other).unwrap().kind {
            Kind::Rotation { pivot } => assert_eq!(pivot, [300.0, 300.0]),
            _ => panic!(),
        }
        let mut v = Values::default();
        v.set(p, 1.0);
        let pose = r.pose(&v);
        // the left arm's hand (to the right of the pivot, going down when turned clockwise) and the mirror image
        let l = r.deform_point(&pose, Some(arm), [800.0, 300.0]);
        let rr = r.deform_point(&pose, Some(other), [200.0, 300.0]);
        assert!((l[0] - 500.0 - (500.0 - rr[0])).abs() < 0.01 && (l[1] - rr[1]).abs() < 0.01, "{l:?} {rr:?}");
        // a warp is mirrored with its points (and the shape too)
        let w = r.add_changer("Cheek L", Kind::Warp { rect: [600.0, 400.0, 800.0, 500.0], cols: 2, rows: 1 }, None);
        let mut off = vec![[0.0, 0.0]; 6];
        off[2] = [30.0, 5.0]; // the top right point moves right and down
        r.set_keyform(w, p, 1.0, Form::Warp { offsets: off });
        let w2 = r.mirror_changer(w, 500.0, "Cheek R").unwrap();
        match &r.changer(w2).unwrap().kind {
            Kind::Warp { rect, .. } => assert_eq!(*rect, [200.0, 400.0, 400.0, 500.0]),
            _ => panic!(),
        }
        let b = r.bindings.iter().find(|b| b.changer == w2).unwrap();
        match &b.forms[b.forms.len() - 1] {
            Form::Warp { offsets } => assert_eq!(offsets[0], [-30.0, 5.0], "the top left point moves left and down"),
            _ => panic!(),
        }
        // nothing to copy: None
        assert!(r.mirror_changer(9999, 500.0, "x").is_none());
    }

    #[test]
    fn a_warp_gets_a_finer_grid_and_keeps_its_shapes() {
        let mut r = Rig::default();
        let p = r.add_param("x", -1.0, 1.0, 0.0);
        let c = r.add_changer("W", Kind::Warp { rect: [0.0, 0.0, 100.0, 100.0], cols: 1, rows: 1 }, None);
        // the right edge goes down by 10 at the end of the slider
        r.set_keyform(c, p, 1.0, Form::Warp { offsets: vec![[0.0, 0.0], [0.0, 10.0], [0.0, 0.0], [0.0, 10.0]] });
        assert!(r.resize_warp(c, 4, 2));
        let b = r.bindings.iter().find(|b| b.changer == c).unwrap();
        let Form::Warp { offsets } = b.forms.last().unwrap() else { panic!() };
        assert_eq!(offsets.len(), 15);
        // along the top row: 0, 2.5, 5, 7.5, 10
        let top: Vec<f32> = offsets[..5].iter().map(|o| o[1]).collect();
        for (got, want) in top.iter().zip([0.0, 2.5, 5.0, 7.5, 10.0]) {
            assert!((got - want).abs() < 1e-4, "{top:?}");
        }
        assert!(!r.resize_warp(c, 0, 2) && !r.resize_warp(c, 4, MAX_WARP + 1));
    }
}
