// SPDX-License-Identifier: GPL-3.0-or-later
// Auto-rig: from the Parts tags, "Rig face" builds a whole face rig in one go: head turn (X and Y, with parallax so the
// nose moves more than the cheek) and tilt (Z), blinking eyes, brows, and a mouth that opens and makes A, I, U, E and O.
// It is a rule-based first version: meshes, changers, sliders and keyforms made by the same pieces you can edit by hand.

use serde::{Deserialize, Serialize};

use super::{Form, Id, Kind, Rig, mesh};
use crate::doc::{Layer, LayerId};
use crate::files::layer_bounds;
use crate::parts::Part;

/// What was made by the automation. Things in `locked` are left alone when it runs again.
#[derive(Clone, PartialEq, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct AutoGroup {
    pub name: String,
    pub changers: Vec<Id>,
    pub params: Vec<Id>,
}

/// Everything the automation made (all groups together), the groups it came in, and what you locked.
#[derive(Clone, PartialEq, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct AutoRig {
    pub changers: Vec<Id>,
    pub params: Vec<Id>,
    pub locked: Vec<Id>,
    pub groups: Vec<AutoGroup>,
}

impl AutoRig {
    fn refresh(&mut self) {
        self.changers = self.groups.iter().flat_map(|g| g.changers.clone()).collect();
        self.params = self.groups.iter().flat_map(|g| g.params.clone()).collect();
    }
}

/// Note what a run made as one group (keeping the locked leftovers of an earlier run of the same group).
fn record(rig: &mut Rig, group: &str, changers: Vec<Id>, params: Vec<Id>) {
    let mut a = rig.auto.take().unwrap_or_default();
    match a.groups.iter_mut().find(|g| g.name == group) {
        Some(g) => {
            g.changers.extend(changers);
            g.params.extend(params);
        }
        None => a.groups.push(AutoGroup { name: group.to_owned(), changers, params }),
    }
    a.refresh();
    rig.auto = Some(a);
}

#[derive(Clone, Copy, Debug)]
pub struct FaceOptions {
    /// How strong the head turn is (1 = normal).
    pub turn: f32,
    /// One slider for both eyes (off: one each, for a face that is not symmetrical).
    pub together_eyes: bool,
    pub together_brows: bool,
    /// A, I, U, E, O for the mouth.
    pub vowels: bool,
    /// How fine the meshes are, 0..1.
    pub detail: f32,
}

impl Default for FaceOptions {
    fn default() -> Self {
        Self { turn: 1.0, together_eyes: true, together_brows: true, vowels: true, detail: 0.5 }
    }
}

#[derive(Clone, Debug, Default)]
pub struct Report {
    pub meshes: usize,
    pub changers: usize,
    pub params: usize,
    pub notes: Vec<String>,
}

/// How far in front of the face a part sits: the nearer it is, the more it moves when the head turns.
fn depth(p: Part) -> f32 {
    match p {
        Part::HairBack => 0.1,
        Part::Face => 0.5,
        Part::EarL | Part::EarR => 0.2,
        Part::HairSideL | Part::HairSideR => 0.6,
        Part::EyeL | Part::EyeR | Part::EyeballL | Part::EyeballR | Part::Blush => 0.9,
        Part::BrowL | Part::BrowR => 0.95,
        Part::Mouth | Part::MouthInside => 1.0,
        Part::Nose => 1.2,
        Part::HairFront => 1.15,
        _ => 0.5,
    }
}

fn is_face_part(p: Part) -> bool {
    matches!(
        p,
        Part::Face | Part::EyeL | Part::EyeR | Part::EyeballL | Part::EyeballR | Part::BrowL | Part::BrowR | Part::Nose | Part::Mouth | Part::MouthInside | Part::EarL | Part::EarR | Part::Blush | Part::HairFront | Part::HairSideL | Part::HairSideR | Part::HairBack
    )
}

fn union(a: [f32; 4], b: [f32; 4]) -> [f32; 4] {
    [a[0].min(b[0]), a[1].min(b[1]), a[2].max(b[2]), a[3].max(b[3])]
}

fn grow(r: [f32; 4], by: f32) -> [f32; 4] {
    [r[0] - by, r[1] - by, r[2] + by, r[3] + by]
}

/// Remove what an earlier run of this group made (except what is locked).
pub fn clear_generated(rig: &mut Rig, group: &str) {
    let Some(mut auto) = rig.auto.take() else { return };
    let Some(gi) = auto.groups.iter().position(|g| g.name == group) else {
        rig.auto = Some(auto);
        return;
    };
    let drop: Vec<Id> = auto.groups[gi].changers.iter().copied().filter(|c| !auto.locked.contains(c)).collect();
    rig.changers.retain(|c| !drop.contains(&c.id));
    rig.bindings.retain(|b| !drop.contains(&b.changer));
    rig.iks.retain(|k| !drop.contains(&k.upper) && !drop.contains(&k.lower));
    for c in rig.changers.iter_mut() {
        if c.parent.is_some_and(|p| drop.contains(&p)) {
            c.parent = None;
        }
    }
    for m in rig.meshes.iter_mut() {
        if m.parent.is_some_and(|p| drop.contains(&p)) {
            m.parent = None;
        }
    }
    // a slider goes when nothing uses it any more
    let used: Vec<Id> = rig.bindings.iter().flat_map(|b| b.params.clone()).chain(rig.iks.iter().flat_map(|k| [k.px, k.py])).collect();
    let dead: Vec<Id> = auto.groups[gi].params.iter().copied().filter(|p| !used.contains(p)).collect();
    rig.params.retain(|p| !dead.contains(&p.id));
    auto.groups[gi].changers.retain(|c| !drop.contains(c));
    auto.groups[gi].params.retain(|p| !dead.contains(p));
    auto.refresh();
    rig.auto = Some(auto);
}

struct Maker<'a> {
    rig: &'a mut Rig,
    changers: Vec<Id>,
    params: Vec<Id>,
}

impl Maker<'_> {
    fn param(&mut self, name: &str, lo: f32, hi: f32, def: f32) -> Id {
        let id = self.rig.add_param(name, lo, hi, def);
        self.params.push(id);
        id
    }

    fn changer(&mut self, name: &str, kind: Kind, parent: Option<Id>) -> Id {
        let id = self.rig.add_changer(name, kind, parent);
        self.changers.push(id);
        id
    }

    /// A box that only slides: all four corners move by the same amount.
    fn slide(&mut self, changer: Id, param: Id, at: f32, d: [f32; 2]) {
        self.rig.set_keyform(changer, param, at, Form::Warp { offsets: vec![d; 4] });
    }
}

/// Build the face rig from the tagged layers. Returns what was made, or why nothing could be.
pub fn rig_face(rig: &mut Rig, layers: &[Layer], doc_w: u32, doc_h: u32, o: &FaceOptions) -> Result<Report, String> {
    let items: Vec<(&Layer, Part, [f32; 4])> = layers
        .iter()
        .filter(|l| l.visible)
        .filter_map(|l| {
            let p = l.part.filter(|p| is_face_part(*p))?;
            let (x0, y0, x1, y1) = layer_bounds(l, doc_w, doc_h)?;
            Some((l, p, [x0 as f32, y0 as f32, x1 as f32, y1 as f32]))
        })
        .collect();
    let has = |p: Part| items.iter().any(|i| i.1 == p);
    if !has(Part::Face) && !has(Part::EyeL) && !has(Part::EyeR) && !has(Part::EyeballL) && !has(Part::EyeballR) {
        return Err("Tag the face first: the face base and the eyes (Parts tab, or the Tag everything for me button).".into());
    }
    let rect_of = |parts: &[Part]| -> Option<[f32; 4]> { items.iter().filter(|i| parts.contains(&i.1)).map(|i| i.2).reduce(union) };
    let head = rect_of(&[Part::Face]).or_else(|| rect_of(&[Part::EyeL, Part::EyeR, Part::Nose, Part::Mouth, Part::BrowL, Part::BrowR])).unwrap_or([0.0, 0.0, doc_w as f32, doc_h as f32]);
    let (hw, hh) = ((head[2] - head[0]).max(10.0), (head[3] - head[1]).max(10.0));
    let mut report = Report::default();

    clear_generated(rig, "face");
    // meshes for every face layer
    let ids: Vec<LayerId> = items.iter().map(|i| i.0.id).collect();
    for (l, _, _) in &items {
        if let Some((verts, tris)) = mesh::generate(l, doc_w, doc_h, o.detail) {
            let mid = rig.fresh_id();
            rig.meshes.retain(|m| m.layer != l.id);
            rig.meshes.push(super::ArtMesh { id: mid, layer: l.id, verts, tris, parent: None });
            report.meshes += 1;
        } else {
            report.notes.push(format!("“{}” has no paint, so it has no mesh.", l.name));
        }
    }
    let mut mk = Maker { rig, changers: Vec::new(), params: Vec::new() };

    // head turn and tilt
    let mut pads: Vec<(String, Id, Id)> = Vec::new();
    let ax = mk.param("Angle X", -30.0, 30.0, 0.0);
    let ay = mk.param("Angle Y", -30.0, 30.0, 0.0);
    pads.push(("Head".into(), ax, ay));
    let az = mk.param("Angle Z", -30.0, 30.0, 0.0);
    let pivot = [(head[0] + head[2]) / 2.0, head[3] - 0.08 * hh];
    let tilt = mk.changer("Head tilt", Kind::Rotation { pivot }, None);
    mk.rig.bind_rotation(tilt, az, 15.0 * o.turn);
    // every kind of part slides a little, by how near it is
    let mut parallax: Vec<(Part, Id)> = Vec::new();
    for part in crate::parts::Part::ALL.iter().copied().filter(|p| is_face_part(*p) && has(*p)) {
        let Some(r) = rect_of(&[part]) else { continue };
        let c = mk.changer(&format!("Turn {}", part.label()), Kind::Warp { rect: grow(r, 4.0), cols: 1, rows: 1 }, Some(tilt));
        let d = depth(part);
        let (dx, dy) = (d * 0.14 * hw * o.turn, d * 0.10 * hh * o.turn);
        mk.slide(c, ax, 30.0, [dx, 0.0]);
        mk.slide(c, ax, -30.0, [-dx, 0.0]);
        mk.slide(c, ay, 30.0, [0.0, -dy]);
        mk.slide(c, ay, -30.0, [0.0, dy]);
        parallax.push((part, c));
    }
    let par = |p: Part| parallax.iter().find(|x| x.0 == p).map(|x| x.1);
    let mut parent_of: Vec<(Part, Option<Id>)> = parallax.iter().map(|(p, c)| (*p, Some(*c))).collect();
    let set_parent = |parent_of: &mut Vec<(Part, Option<Id>)>, p: Part, c: Id| {
        if let Some(e) = parent_of.iter_mut().find(|e| e.0 == p) {
            e.1 = Some(c);
        }
    };

    // eyes: closing, and smiling (the lower lid comes up)
    let eye_param_all = if o.together_eyes && (has(Part::EyeL) || has(Part::EyeR)) { Some(mk.param("Eyes open", 0.0, 1.0, 1.0)) } else { None };
    let smile_all = if o.together_eyes && (has(Part::EyeL) || has(Part::EyeR)) { Some(mk.param("Eyes smile", 0.0, 1.0, 0.0)) } else { None };
    for (part, name) in [(Part::EyeL, "Eye L open"), (Part::EyeR, "Eye R open")] {
        if let (Some(r), Some(p)) = (rect_of(&[part]), par(part)) {
            let prm = match eye_param_all {
                Some(a) => a,
                None => mk.param(name, 0.0, 1.0, 1.0),
            };
            let c = mk.changer(&format!("{} blink", part.label()), Kind::Warp { rect: grow(r, 2.0), cols: 1, rows: 1 }, Some(p));
            let h = (r[3] - r[1]) + 4.0;
            // closed: the top edge comes down to the bottom, leaving a line
            mk.rig.set_keyform(c, prm, 0.0, Form::Warp { offsets: vec![[0.0, 0.9 * h], [0.0, 0.9 * h], [0.0, 0.0], [0.0, 0.0]] });
            // smiling: the lower edge comes up in a happy curve
            let sm = match smile_all {
                Some(a) => a,
                None => mk.param(&format!("{} smile", part.label()), 0.0, 1.0, 0.0),
            };
            mk.rig.set_keyform(c, sm, 1.0, Form::Warp { offsets: vec![[0.0, 0.1 * h], [0.0, 0.1 * h], [0.0, -0.45 * h], [0.0, -0.45 * h]] });
            set_parent(&mut parent_of, part, c);
        }
    }
    // eyeballs: look left-right and up-down (one pair of sliders for both, or one pair each)
    let ball_all = if o.together_eyes && (has(Part::EyeballL) || has(Part::EyeballR)) {
        let (x, y) = (mk.param("Eyeball X", -1.0, 1.0, 0.0), mk.param("Eyeball Y", -1.0, 1.0, 0.0));
        pads.push(("Eyeballs".into(), x, y));
        Some((x, y))
    } else {
        None
    };
    for (part, eye, side) in [(Part::EyeballL, Part::EyeL, "L"), (Part::EyeballR, Part::EyeR, "R")] {
        if let (Some(r), Some(p)) = (rect_of(&[part]), par(part)) {
            let (px, py) = match ball_all {
                Some(a) => a,
                None => {
                    let (x, y) = (mk.param(&format!("Eyeball {side} X"), -1.0, 1.0, 0.0), mk.param(&format!("Eyeball {side} Y"), -1.0, 1.0, 0.0));
                    pads.push((format!("Eyeball {side}"), x, y));
                    (x, y)
                }
            };
            // how far it can look: a share of the eye it sits in
            let e = rect_of(&[eye]).unwrap_or_else(|| grow(r, (r[2] - r[0]) * 0.5));
            let (ew, eh) = ((e[2] - e[0]).max(1.0), (e[3] - e[1]).max(1.0));
            let c = mk.changer(&format!("{} look", part.label()), Kind::Warp { rect: grow(r, 2.0), cols: 1, rows: 1 }, Some(p));
            let (dx, dy) = (0.22 * ew, 0.30 * eh);
            mk.slide(c, px, 1.0, [dx, 0.0]);
            mk.slide(c, px, -1.0, [-dx, 0.0]);
            mk.slide(c, py, 1.0, [0.0, -dy]);
            mk.slide(c, py, -1.0, [0.0, dy]);
            set_parent(&mut parent_of, part, c);
        }
    }
    // brows: up and down
    let brow_all = if o.together_brows && (has(Part::BrowL) || has(Part::BrowR)) { Some(mk.param("Brows", 0.0, 1.0, 0.0)) } else { None };
    for (part, name) in [(Part::BrowL, "Brow L"), (Part::BrowR, "Brow R")] {
        if let (Some(r), Some(p)) = (rect_of(&[part]), par(part)) {
            let prm = match brow_all {
                Some(a) => a,
                None => mk.param(name, 0.0, 1.0, 0.0),
            };
            let c = mk.changer(&format!("{} raise", part.label()), Kind::Warp { rect: grow(r, 2.0), cols: 1, rows: 1 }, Some(p));
            let up = 0.5 * (r[3] - r[1]) + 0.03 * hh;
            // two key points: the normal place (left end) and raised (right end)
            mk.slide(c, prm, 1.0, [0.0, -up]);
            set_parent(&mut parent_of, part, c);
        }
    }
    // mouth: open, and the five vowels
    if let Some(mr) = rect_of(&[Part::Mouth, Part::MouthInside]) {
        let open = mk.param("Mouth open", 0.0, 1.0, 0.0);
        let form = mk.param("Mouth form", -1.0, 1.0, 0.0);
        pads.push(("Mouth".into(), form, open));
        let vowels: Vec<(Id, &str, (f32, f32, f32))> = if o.vowels {
            [("A", (0.05, 0.9, 0.1)), ("I", (0.18, 0.12, 0.0)), ("U", (-0.22, 0.35, 0.05)), ("E", (0.12, 0.45, 0.0)), ("O", (-0.18, 0.7, 0.1))].iter().map(|(n, s)| (mk.param(&format!("Mouth {n}"), 0.0, 1.0, 0.0), *n, *s)).collect()
        } else {
            Vec::new()
        };
        let (mw, mh) = (mr[2] - mr[0], mr[3] - mr[1]);
        // the 3 x 3 control points: rows top to bottom, columns left to right
        let shape = |corner: f32, bottom: f32, top: f32| -> Vec<[f32; 2]> {
            let mut v = Vec::new();
            for r in 0..3 {
                for c in 0..3 {
                    let x = match c {
                        0 => -corner * mw,
                        2 => corner * mw,
                        _ => 0.0,
                    };
                    let mid = c == 1;
                    let y = match r {
                        2 => bottom * mh * if mid { 1.0 } else { 0.5 },
                        0 => -top * mh * if mid { 1.0 } else { 0.5 },
                        _ => bottom * mh * if mid { 0.5 } else { 0.25 },
                    };
                    v.push([x, y]);
                }
            }
            v
        };
        for part in [Part::Mouth, Part::MouthInside] {
            let (Some(_), Some(p)) = (rect_of(&[part]), par(part)) else { continue };
            let c = mk.changer(&format!("{} shape", part.label()), Kind::Warp { rect: grow(mr, 2.0), cols: 2, rows: 2 }, Some(p));
            mk.rig.set_keyform(c, open, 1.0, Form::Warp { offsets: shape(0.0, 0.8, 0.0) });
            // mouth form has three key points: sad, the original drawing in the middle, smiling
            let curve = |k: f32| -> Vec<[f32; 2]> {
                let mut v = Vec::new();
                for r in 0..3 {
                    for col in 0..3 {
                        let corner = col != 1;
                        let y = match (r, corner) {
                            (0, true) => -0.25 * k * mh,
                            (1, true) => -0.35 * k * mh,
                            (2, true) => -0.20 * k * mh,
                            (2, false) => 0.10 * k * mh,
                            _ => 0.0,
                        };
                        v.push([0.0, y]);
                    }
                }
                v
            };
            mk.rig.set_keyform(c, form, 1.0, Form::Warp { offsets: curve(1.0) });
            mk.rig.set_keyform(c, form, -1.0, Form::Warp { offsets: curve(-0.8) });
            for (prm, _, (corner, bottom, top)) in &vowels {
                mk.rig.set_keyform(c, *prm, 1.0, Form::Warp { offsets: shape(*corner, *bottom, *top) });
            }
            set_parent(&mut parent_of, part, c);
        }
        if !has(Part::MouthInside) {
            report.notes.push("The mouth has no inside (teeth and tongue): tag one to see the mouth open.".into());
        }
    }
    // every mesh goes into the last changer of its part's chain
    let (changers, params) = (mk.changers, mk.params);
    let rig = mk.rig;
    for (l, part, _) in &items {
        let parent = parent_of.iter().find(|e| e.0 == *part).and_then(|e| e.1);
        if let Some(m) = rig.meshes.iter_mut().find(|m| m.layer == l.id) {
            m.parent = parent;
        }
    }
    report.changers = changers.len();
    report.params = params.len();
    // pads: earlier ones that point at sliders that are gone are dropped, then the new ones are added
    let alive: Vec<Id> = rig.params.iter().map(|p| p.id).collect();
    rig.pads.retain(|p| alive.contains(&p.x) && alive.contains(&p.y));
    for (name, x, y) in pads {
        rig.pads.push(super::Pad { name, x, y });
    }
    record(rig, "face", changers, params);
    let _ = ids;
    Ok(report)
}

// ---------- body, arms, legs and hair ----------

#[derive(Clone, Copy, Debug)]
pub struct BodyOptions {
    /// Arms and legs swing from their top end.
    pub limbs: bool,
    /// Hand sliders that bring the hand to a place (two-bone IK) instead of turning each part by hand.
    pub ik: bool,
    /// Breathing and a slight tilt of the torso.
    pub torso: bool,
    pub detail: f32,
}

impl Default for BodyOptions {
    fn default() -> Self {
        Self { limbs: true, ik: true, torso: true, detail: 0.4 }
    }
}

fn is_body_part(p: Part) -> bool {
    matches!(p, Part::Neck | Part::Torso | Part::ArmL | Part::ArmR | Part::HandL | Part::HandR | Part::LegL | Part::LegR | Part::Clothing)
}

/// The two ends of a box along its long side (top and bottom middle for a tall box, left and right middle for a wide one).
fn ends(r: [f32; 4]) -> [[f32; 2]; 2] {
    let (cx, cy) = ((r[0] + r[2]) / 2.0, (r[1] + r[3]) / 2.0);
    if r[3] - r[1] >= r[2] - r[0] {
        [[cx, r[1]], [cx, r[3]]]
    } else {
        [[r[0], cy], [r[2], cy]]
    }
}

/// Of a limb's two ends, the one farthest from `from` (the hand, the foot).
fn far_end(r: [f32; 4], from: [f32; 2]) -> [f32; 2] {
    let e = ends(r);
    if (e[0][0] - from[0]).hypot(e[0][1] - from[1]) >= (e[1][0] - from[0]).hypot(e[1][1] - from[1]) { e[0] } else { e[1] }
}

/// Of a limb's two ends, the one nearest to `to` (where it joins the body).
fn near_end(r: [f32; 4], to: [f32; 2]) -> [f32; 2] {
    let e = ends(r);
    if (e[0][0] - to[0]).hypot(e[0][1] - to[1]) <= (e[1][0] - to[0]).hypot(e[1][1] - to[1]) { e[0] } else { e[1] }
}

/// Build the body rig from the tags: torso tilt and breathing, arms and legs that swing, and hands that can be brought
/// to a place (IK). Run the face first: the head then goes with the body.
pub fn rig_body(rig: &mut Rig, layers: &[Layer], doc_w: u32, doc_h: u32, o: &BodyOptions) -> Result<Report, String> {
    let items: Vec<(&Layer, Part, [f32; 4])> = layers
        .iter()
        .filter(|l| l.visible)
        .filter_map(|l| {
            let p = l.part.filter(|p| is_body_part(*p))?;
            let (x0, y0, x1, y1) = layer_bounds(l, doc_w, doc_h)?;
            Some((l, p, [x0 as f32, y0 as f32, x1 as f32, y1 as f32]))
        })
        .collect();
    let has = |p: Part| items.iter().any(|i| i.1 == p);
    if !has(Part::Torso) && !has(Part::ArmL) && !has(Part::ArmR) && !has(Part::LegL) && !has(Part::LegR) {
        return Err("Tag the body first: the torso, arms or legs (Parts tab, or the Tag everything for me button).".into());
    }
    let rect_of = |p: Part| -> Option<[f32; 4]> { items.iter().filter(|i| i.1 == p).map(|i| i.2).reduce(union) };
    let torso = rect_of(Part::Torso).or_else(|| items.iter().map(|i| i.2).reduce(union)).unwrap_or([0.0, 0.0, doc_w as f32, doc_h as f32]);
    let (tw, th) = ((torso[2] - torso[0]).max(10.0), (torso[3] - torso[1]).max(10.0));
    let centre = [(torso[0] + torso[2]) / 2.0, (torso[1] + torso[3]) / 2.0];
    let mut report = Report::default();

    clear_generated(rig, "body");
    for (l, _, _) in &items {
        if let Some((verts, tris)) = mesh::generate(l, doc_w, doc_h, o.detail) {
            let mid = rig.fresh_id();
            rig.meshes.retain(|m| m.layer != l.id);
            rig.meshes.push(super::ArtMesh { id: mid, layer: l.id, verts, tris, parent: None });
            report.meshes += 1;
        }
    }
    let mut mk = Maker { rig, changers: Vec::new(), params: Vec::new() };
    // the whole body leans around its feet
    let lean_param = mk.param("Body tilt", -10.0, 10.0, 0.0);
    let body = mk.changer("Body tilt", Kind::Rotation { pivot: [centre[0], torso[3]] }, None);
    if o.torso {
        mk.rig.bind_rotation(body, lean_param, 6.0);
    }
    // breathing: the chest swells a little
    let breath_param = mk.param("Breath", 0.0, 1.0, 0.0);
    let breath = mk.changer("Breath", Kind::Soft { center: [centre[0], torso[1] + th * 0.3], radius: tw.max(th * 0.5) * 0.8 }, Some(body));
    if o.torso {
        mk.rig.set_keyform(breath, breath_param, 1.0, Form::Soft { offset: [0.0, -0.008 * th], grow: 0.015 });
    }
    let mut parent_of: Vec<(Part, Option<Id>)> = Vec::new();
    let mut parent_for = |p: Part, c: Option<Id>| parent_of.push((p, c));
    parent_for(Part::Torso, Some(breath));
    parent_for(Part::Neck, Some(body));
    parent_for(Part::Clothing, Some(body));

    if o.limbs {
        for (arm, hand, side) in [(Part::ArmL, Part::HandL, "L"), (Part::ArmR, Part::HandR, "R")] {
            let Some(ar) = rect_of(arm) else { continue };
            let root = near_end(ar, [centre[0], torso[1]]);
            let swing = mk.param(&format!("Arm {side}"), -45.0, 45.0, 0.0);
            let upper = mk.changer(&format!("Arm {side} swing"), Kind::Rotation { pivot: root }, Some(body));
            mk.rig.bind_rotation(upper, swing, if side == "L" { 45.0 } else { -45.0 });
            parent_for(arm, Some(upper));
            if let Some(hr) = rect_of(hand) {
                let wrist = near_end(hr, root);
                let tip = far_end(hr, wrist);
                let turn = mk.param(&format!("Hand {side}"), -40.0, 40.0, 0.0);
                let lower = mk.changer(&format!("Hand {side} turn"), Kind::Rotation { pivot: wrist }, Some(upper));
                mk.rig.bind_rotation(lower, turn, if side == "L" { 40.0 } else { -40.0 });
                parent_for(hand, Some(lower));
                if o.ik {
                    let reach = (tip[0] - root[0]).hypot(tip[1] - root[1]).max(50.0);
                    let px = mk.param(&format!("Hand {side} reach X"), -reach, reach, 0.0);
                    let py = mk.param(&format!("Hand {side} reach Y"), -reach, reach, 0.0);
                    mk.rig.iks.push(super::Ik { upper, lower, root, joint: wrist, tip, px, py });
                }
            }
        }
        for (leg, side) in [(Part::LegL, "L"), (Part::LegR, "R")] {
            let Some(lr) = rect_of(leg) else { continue };
            let root = near_end(lr, [centre[0], torso[3]]);
            let swing = mk.param(&format!("Leg {side}"), -30.0, 30.0, 0.0);
            let c = mk.changer(&format!("Leg {side} swing"), Kind::Rotation { pivot: root }, Some(body));
            mk.rig.bind_rotation(c, swing, if side == "L" { 30.0 } else { -30.0 });
            parent_for(leg, Some(c));
        }
    }
    // the head goes with the body
    let head_tilt = mk.rig.changers.iter().find(|c| c.name == "Head tilt").map(|c| c.id);
    if let Some(h) = head_tilt {
        if let Some(c) = mk.rig.changers.iter_mut().find(|c| c.id == h) {
            c.parent = Some(body);
        }
    } else {
        report.notes.push("There is no face rig yet: rig the face first and the head will lean with the body.".into());
    }
    let (changers, params) = (mk.changers, mk.params);
    let rig = mk.rig;
    for (l, part, _) in &items {
        let parent = parent_of.iter().find(|e| e.0 == *part).and_then(|e| e.1);
        if let Some(m) = rig.meshes.iter_mut().find(|m| m.layer == l.id) {
            m.parent = parent;
        }
    }
    report.changers = changers.len();
    report.params = params.len();
    record(rig, "body", changers, params);
    Ok(report)
}

/// Hair that sways: a curve down each hair part (root at the top, tip at the bottom) and one slider.
pub fn rig_hair(rig: &mut Rig, layers: &[Layer], doc_w: u32, doc_h: u32) -> Result<Report, String> {
    let items: Vec<(&Layer, Part, [f32; 4])> = layers
        .iter()
        .filter(|l| l.visible)
        .filter_map(|l| {
            let p = l.part.filter(|p| matches!(p, Part::HairFront | Part::HairSideL | Part::HairSideR | Part::HairBack))?;
            let (x0, y0, x1, y1) = layer_bounds(l, doc_w, doc_h)?;
            Some((l, p, [x0 as f32, y0 as f32, x1 as f32, y1 as f32]))
        })
        .collect();
    if items.is_empty() {
        return Err("Tag the hair first: hair front, sides or back (Parts tab).".into());
    }
    let mut report = Report::default();
    clear_generated(rig, "hair");
    let mut mk = Maker { rig, changers: Vec::new(), params: Vec::new() };
    let sway = mk.param("Hair sway", -1.0, 1.0, 0.0);
    for (l, part, r) in &items {
        // a mesh if there is none yet
        if mk.rig.mesh_of(l.id).is_none() {
            if let Some((verts, tris)) = mesh::generate(l, doc_w, doc_h, 0.4) {
                let mid = mk.rig.fresh_id();
                mk.rig.meshes.push(super::ArtMesh { id: mid, layer: l.id, verts, tris, parent: None });
                report.meshes += 1;
            } else {
                continue;
            }
        }
        let (w, h) = (r[2] - r[0], r[3] - r[1]);
        let x = (r[0] + r[2]) / 2.0;
        let pts: Vec<[f32; 2]> = (0..5).map(|i| [x, r[1] + h * i as f32 / 4.0]).collect();
        let existing = mk.rig.mesh_of(l.id).and_then(|m| m.parent);
        let line = mk.changer(&format!("{} sway", part.label()), Kind::Line { pts, width: (w * 0.6).max(20.0) }, existing);
        // the tip swings most; the sides of the hair swing opposite ways
        let dir = if matches!(part, Part::HairSideL) { 1.0 } else if matches!(part, Part::HairSideR) { -1.0 } else { 1.0 };
        let amp = 0.22 * w * dir;
        let offsets: Vec<[f32; 2]> = (0..5).map(|i| [amp * (i as f32 / 4.0).powi(2), 0.0]).collect();
        mk.rig.set_keyform(line, sway, 1.0, Form::Line { offsets: offsets.clone() });
        mk.rig.set_keyform(line, sway, -1.0, Form::Line { offsets: offsets.iter().map(|o| [-o[0], o[1]]).collect() });
        if let Some(m) = mk.rig.meshes.iter_mut().find(|m| m.layer == l.id) {
            m.parent = Some(line);
        }
    }
    let (changers, params) = (mk.changers, mk.params);
    report.changers = changers.len();
    report.params = params.len();
    record(rig, "hair", changers, params);
    Ok(report)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::doc::{Document, LayerKind, TILE};
    use crate::rig::Values;
    use eframe::egui::Color32;
    use std::sync::Arc;

    fn add(d: &mut Document, name: &str, part: Option<Part>, b: (usize, usize, usize, usize)) {
        let mut l = d.new_layer(name, LayerKind::Raster);
        for ty in b.1 / TILE..=(b.3 - 1) / TILE {
            for tx in b.0 / TILE..=(b.2 - 1) / TILE {
                let mut px = vec![Color32::TRANSPARENT; TILE * TILE];
                for j in 0..TILE {
                    for i in 0..TILE {
                        let (x, y) = (tx * TILE + i, ty * TILE + j);
                        if x >= b.0 && x < b.2 && y >= b.1 && y < b.3 {
                            px[j * TILE + i] = Color32::from_rgb(200, 120, 120);
                        }
                    }
                }
                l.tiles.insert((tx as i32, ty as i32), Arc::new(px));
            }
        }
        l.part = part;
        d.layers.insert(0, l);
    }

    /// A face on a 1000 x 1000 page.
    fn face() -> Document {
        let mut d = Document::new(1000, 1000);
        add(&mut d, "Hair back", Some(Part::HairBack), (250, 80, 750, 700));
        add(&mut d, "Face", Some(Part::Face), (300, 200, 700, 700));
        add(&mut d, "Eye L", Some(Part::EyeL), (550, 400, 650, 450));
        add(&mut d, "Eye R", Some(Part::EyeR), (350, 400, 450, 450));
        add(&mut d, "Brow L", Some(Part::BrowL), (550, 340, 650, 360));
        add(&mut d, "Brow R", Some(Part::BrowR), (350, 340, 450, 360));
        add(&mut d, "Nose", Some(Part::Nose), (480, 470, 520, 540));
        add(&mut d, "Mouth inside", Some(Part::MouthInside), (440, 580, 560, 610));
        add(&mut d, "Mouth", Some(Part::Mouth), (440, 580, 560, 610));
        add(&mut d, "Hair front", Some(Part::HairFront), (280, 150, 720, 300));
        add(&mut d, "Background", None, (0, 0, 1000, 1000));
        d
    }

    fn param(r: &Rig, name: &str) -> Id {
        r.params.iter().find(|p| p.name == name).unwrap_or_else(|| panic!("a slider called {name}: {:?}", r.params.iter().map(|p| p.name.clone()).collect::<Vec<_>>())).id
    }

    fn mesh_of<'a>(d: &'a Document, r: &'a Rig, name: &str) -> &'a super::super::ArtMesh {
        let id = d.layers.iter().find(|l| l.name == name).unwrap().id;
        r.mesh_of(id).unwrap_or_else(|| panic!("a mesh for {name}"))
    }

    fn extent(v: &[[f32; 2]]) -> (f32, f32, f32, f32) {
        let f = |k: usize, m: bool| v.iter().map(|p| p[k]).fold(if m { f32::MIN } else { f32::MAX }, |a, b| if m { a.max(b) } else { a.min(b) });
        (f(0, false), f(1, false), f(0, true), f(1, true))
    }

    #[test]
    fn rig_face_makes_meshes_sliders_and_changers_from_the_tags() {
        let d = face();
        let mut r = Rig::default();
        let rep = rig_face(&mut r, &d.layers, d.w, d.h, &FaceOptions::default()).unwrap();
        assert_eq!(rep.meshes, 10, "every tagged face layer has a mesh, the background does not");
        assert!(r.mesh_of(d.layers.iter().find(|l| l.name == "Background").unwrap().id).is_none());
        for n in ["Angle X", "Angle Y", "Angle Z", "Eyes open", "Eyes smile", "Brows", "Mouth open", "Mouth form", "Mouth A", "Mouth I", "Mouth U", "Mouth E", "Mouth O"] {
            param(&r, n);
        }
        assert_eq!(rep.params, 13);
        // two sliders are joined into a pad for the head and for the mouth
        let pad = |name: &str| r.pads.iter().find(|p| p.name == name).unwrap_or_else(|| panic!("a pad {name}")).clone();
        assert_eq!((pad("Head").x, pad("Head").y), (param(&r, "Angle X"), param(&r, "Angle Y")));
        assert_eq!((pad("Mouth").x, pad("Mouth").y), (param(&r, "Mouth form"), param(&r, "Mouth open")));
        assert!(rep.changers >= 10, "{}", rep.changers);
        assert!(r.mesh_of(d.layers.iter().find(|l| l.name == "Mouth").unwrap().id).unwrap().parent.is_some());
        // it all survives cleaning
        let cleaned = r.clone().sanitized();
        assert_eq!(cleaned.changers.len(), r.changers.len());
        assert_eq!(cleaned.bindings.len(), r.bindings.len());
    }

    #[test]
    fn turning_the_head_moves_near_parts_more_than_far_ones() {
        let d = face();
        let mut r = Rig::default();
        rig_face(&mut r, &d.layers, d.w, d.h, &FaceOptions::default()).unwrap();
        let ax = param(&r, "Angle X");
        let mut v = Values::default();
        v.set(ax, 30.0);
        let pose = r.pose(&v);
        let shift = |name: &str| {
            let m = mesh_of(&d, &r, name);
            let now = r.deformed(m, &pose);
            now.iter().zip(&m.verts).map(|(a, b)| a[0] - b[0]).sum::<f32>() / now.len() as f32
        };
        let (nose, face, back) = (shift("Nose"), shift("Face"), shift("Hair back"));
        assert!(nose > face && face > back && back > 0.0, "nose {nose}, face {face}, hair back {back}");
        v.set(ax, -30.0);
        let left = r.pose(&v);
        let m = mesh_of(&d, &r, "Nose");
        assert!(r.deformed(m, &left)[0][0] < m.verts[0][0], "the other way");
        // Angle Y moves things up and down
        let ay = param(&r, "Angle Y");
        let mut w = Values::default();
        w.set(ay, 30.0);
        let m = mesh_of(&d, &r, "Eye L");
        assert!(r.deformed(m, &r.pose(&w))[0][1] < m.verts[0][1], "looking up moves the eyes up");
    }

    #[test]
    fn eyes_close_brows_move_and_the_mouth_opens_with_its_inside() {
        let d = face();
        let mut r = Rig::default();
        rig_face(&mut r, &d.layers, d.w, d.h, &FaceOptions::default()).unwrap();
        let (open, brows, a, i) = (param(&r, "Eyes open"), param(&r, "Brows"), param(&r, "Mouth A"), param(&r, "Mouth I"));
        let rest_height = |name: &str| {
            let m = mesh_of(&d, &r, name);
            let e = extent(&m.verts);
            e.3 - e.1
        };
        let mut v = Values::default();
        assert!((v.0.get(&open).is_none()), "nothing set: the default is open");
        let closed_h = |v: &Values, name: &str| {
            let m = mesh_of(&d, &r, name);
            let e = extent(&r.deformed(m, &r.pose(v)));
            e.3 - e.1
        };
        assert!((closed_h(&v, "Eye L") - rest_height("Eye L")).abs() < 0.5, "open at rest");
        v.set(open, 0.0);
        assert!(closed_h(&v, "Eye L") < rest_height("Eye L") * 0.25 && closed_h(&v, "Eye R") < rest_height("Eye R") * 0.25, "both eyes close together");
        // brows go up
        let mut b = Values::default();
        b.set(brows, 1.0);
        let m = mesh_of(&d, &r, "Brow L");
        assert!(r.deformed(m, &r.pose(&b))[0][1] < m.verts[0][1] - 10.0);
        // A opens the mouth wide and the inside follows
        let mut va = Values::default();
        va.set(a, 1.0);
        for name in ["Mouth", "Mouth inside"] {
            assert!(closed_h(&va, name) > rest_height(name) + 20.0, "{name} opens");
        }
        // I is wide but not open: the corners go out
        let mut vi = Values::default();
        vi.set(i, 1.0);
        let m = mesh_of(&d, &r, "Mouth");
        let (w0, w1) = (extent(&m.verts), extent(&r.deformed(m, &r.pose(&vi))));
        assert!(w1.2 - w1.0 > w0.2 - w0.0 + 10.0 && (w1.3 - w1.1) < (w0.3 - w0.1) + 10.0);
    }

    #[test]
    fn tilting_turns_the_whole_head_together() {
        let d = face();
        let mut r = Rig::default();
        rig_face(&mut r, &d.layers, d.w, d.h, &FaceOptions::default()).unwrap();
        let az = param(&r, "Angle Z");
        let mut v = Values::default();
        v.set(az, 30.0);
        let pose = r.pose(&v);
        let m = mesh_of(&d, &r, "Hair front");
        let moved = r.deformed(m, &pose).iter().zip(&m.verts).map(|(a, b)| (a[0] - b[0]).hypot(a[1] - b[1])).fold(0.0, f32::max);
        assert!(moved > 20.0, "{moved}");
    }

    #[test]
    fn options_change_what_is_made_and_a_second_run_replaces_the_first_but_not_locked_things() {
        let d = face();
        let mut r = Rig::default();
        rig_face(&mut r, &d.layers, d.w, d.h, &FaceOptions { together_eyes: false, together_brows: false, vowels: false, ..FaceOptions::default() }).unwrap();
        for n in ["Eye L open", "Eye R open", "Brow L", "Brow R", "Mouth open"] {
            param(&r, n);
        }
        assert!(r.params.iter().all(|p| !p.name.starts_with("Mouth A")), "no vowels");
        let first_changers = r.changers.len();
        // run again: no duplicates
        rig_face(&mut r, &d.layers, d.w, d.h, &FaceOptions { together_eyes: false, together_brows: false, vowels: false, ..FaceOptions::default() }).unwrap();
        assert_eq!(r.changers.len(), first_changers);
        assert_eq!(r.meshes.len(), 10);
        // lock the head tilt: it stays (and so does the slider it uses) when the rig is made again
        let tilt = r.changers.iter().find(|c| c.name == "Head tilt").unwrap().id;
        r.auto.as_mut().unwrap().locked.push(tilt);
        rig_face(&mut r, &d.layers, d.w, d.h, &FaceOptions::default()).unwrap();
        assert!(r.changer(tilt).is_some(), "the locked changer is kept");
        assert!(r.auto.as_ref().unwrap().locked.contains(&tilt));
        let z_sliders = r.params.iter().filter(|p| p.name == "Angle Z").count();
        assert!(z_sliders >= 1);
        // nothing tagged: a clear reason
        let mut empty = Rig::default();
        let mut e = Document::new(100, 100);
        add(&mut e, "Layer", None, (10, 10, 50, 50));
        let err = rig_face(&mut empty, &e.layers, 100, 100, &FaceOptions::default()).unwrap_err();
        assert!(err.contains("Tag the face"));
        assert!(empty.is_empty());
    }

    #[test]
    fn the_strength_option_scales_the_head_turn() {
        let d = face();
        let mut weak = Rig::default();
        let mut strong = Rig::default();
        rig_face(&mut weak, &d.layers, d.w, d.h, &FaceOptions { turn: 0.5, ..FaceOptions::default() }).unwrap();
        rig_face(&mut strong, &d.layers, d.w, d.h, &FaceOptions { turn: 2.0, ..FaceOptions::default() }).unwrap();
        let shift = |r: &Rig| {
            let ax = r.params.iter().find(|p| p.name == "Angle X").unwrap().id;
            let mut v = Values::default();
            v.set(ax, 30.0);
            let m = mesh_of(&d, r, "Nose");
            r.deformed(m, &r.pose(&v))[0][0] - m.verts[0][0]
        };
        assert!(shift(&strong) > shift(&weak) * 3.0, "{} vs {}", shift(&strong), shift(&weak));
    }

    /// A body on a 1000 x 1000 page: torso, two arms with hands, two legs, and hair.
    fn body() -> Document {
        let mut d = face();
        add(&mut d, "Torso", Some(Part::Torso), (380, 700, 620, 900));
        add(&mut d, "Arm L", Some(Part::ArmL), (620, 700, 680, 860));
        add(&mut d, "Hand L", Some(Part::HandL), (620, 860, 680, 900));
        add(&mut d, "Arm R", Some(Part::ArmR), (320, 700, 380, 860));
        add(&mut d, "Hand R", Some(Part::HandR), (320, 860, 380, 900));
        add(&mut d, "Leg L", Some(Part::LegL), (500, 900, 620, 990));
        add(&mut d, "Leg R", Some(Part::LegR), (380, 900, 500, 990));
        d
    }

    #[test]
    fn rig_body_makes_tilt_breath_limbs_and_hands_that_reach() {
        let d = body();
        let mut r = Rig::default();
        rig_face(&mut r, &d.layers, d.w, d.h, &FaceOptions::default()).unwrap();
        let head_tilt = r.changers.iter().find(|c| c.name == "Head tilt").unwrap().id;
        let rep = rig_body(&mut r, &d.layers, d.w, d.h, &BodyOptions::default()).unwrap();
        assert_eq!(rep.meshes, 7);
        for n in ["Body tilt", "Breath", "Arm L", "Arm R", "Hand L", "Hand R", "Leg L", "Leg R", "Hand L reach X", "Hand L reach Y", "Hand R reach X"] {
            param(&r, n);
        }
        assert_eq!(r.iks.len(), 2, "one arm each");
        // the head goes with the body now
        let body_tilt = r.changers.iter().find(|c| c.name == "Body tilt").unwrap().id;
        assert_eq!(r.changer(head_tilt).unwrap().parent, Some(body_tilt));
        // the arm swings from its top (the shoulder), the leg from its top
        let swing = param(&r, "Arm L");
        let mut v = Values::default();
        v.set(swing, 45.0);
        let pose = r.pose(&v);
        let m = mesh_of(&d, &r, "Arm L");
        let now = r.deformed(m, &pose);
        let (bottom_move, top_move) = {
            let (mut bi, mut ti) = (0, 0);
            for (i, p) in m.verts.iter().enumerate() {
                if p[1] > m.verts[bi][1] {
                    bi = i;
                }
                if p[1] < m.verts[ti][1] {
                    ti = i;
                }
            }
            ((now[bi][0] - m.verts[bi][0]).hypot(now[bi][1] - m.verts[bi][1]), (now[ti][0] - m.verts[ti][0]).hypot(now[ti][1] - m.verts[ti][1]))
        };
        assert!(bottom_move > 3.0 * top_move.max(1.0), "the hand end swings, the shoulder end hardly: {bottom_move} vs {top_move}");
        // the right arm swings the other way for the same kind of slider
        let rs = param(&r, "Arm R");
        let mut w = Values::default();
        w.set(rs, 45.0);
        let mr = mesh_of(&d, &r, "Arm R");
        let dx_l = r.deformed(m, &pose).iter().zip(&m.verts).map(|(a, b)| a[0] - b[0]).sum::<f32>();
        let dx_r = r.deformed(mr, &r.pose(&w)).iter().zip(&mr.verts).map(|(a, b)| a[0] - b[0]).sum::<f32>();
        assert!(dx_l * dx_r < 0.0, "mirrored: {dx_l} {dx_r}");
        // the hand follows an IK slider: moving the target right moves the hand
        let (rx, ry) = (param(&r, "Hand L reach X"), param(&r, "Hand L reach Y"));
        let mut u = Values::default();
        u.set(rx, 40.0);
        u.set(ry, -20.0);
        let hand = mesh_of(&d, &r, "Hand L");
        let moved = r.deformed(hand, &r.pose(&u)).iter().zip(&hand.verts).map(|(a, b)| (a[0] - b[0]).hypot(a[1] - b[1])).fold(0.0, f32::max);
        assert!(moved > 10.0, "{moved}");
        // without IK there are no reach sliders
        let mut plain = Rig::default();
        rig_body(&mut plain, &d.layers, d.w, d.h, &BodyOptions { ik: false, ..BodyOptions::default() }).unwrap();
        assert!(plain.iks.is_empty() && !plain.params.iter().any(|p| p.name.contains("reach")));
        // running it again replaces the body and keeps the face
        let n = r.changers.len();
        rig_body(&mut r, &d.layers, d.w, d.h, &BodyOptions::default()).unwrap();
        assert_eq!(r.changers.len(), n);
        assert_eq!(r.auto.as_ref().unwrap().groups.len(), 2);
        // and cleaning keeps it all
        assert_eq!(r.clone().sanitized().iks.len(), 2);
        // nothing tagged: a clear reason
        let mut e = Rig::default();
        assert!(rig_body(&mut e, &face().layers, 1000, 1000, &BodyOptions::default()).unwrap_err().contains("Tag the body"));
    }

    #[test]
    fn hair_sways_most_at_the_tips_and_the_two_sides_swing_apart() {
        let mut d = face();
        add(&mut d, "Hair side L", Some(Part::HairSideL), (700, 200, 760, 700));
        add(&mut d, "Hair side R", Some(Part::HairSideR), (240, 200, 300, 700));
        let mut r = Rig::default();
        rig_face(&mut r, &d.layers, d.w, d.h, &FaceOptions::default()).unwrap();
        let rep = rig_hair(&mut r, &d.layers, d.w, d.h).unwrap();
        assert!(rep.changers >= 4 && rep.params == 1, "{rep:?}");
        let sway = param(&r, "Hair sway");
        let mut v = Values::default();
        v.set(sway, 1.0);
        let pose = r.pose(&v);
        let shift = |name: &str, top: bool| {
            let m = mesh_of(&d, &r, name);
            let now = r.deformed(m, &pose);
            let i = (0..m.verts.len()).max_by(|a, b| if top { m.verts[*b][1].total_cmp(&m.verts[*a][1]) } else { m.verts[*a][1].total_cmp(&m.verts[*b][1]) }).unwrap();
            now[i][0] - m.verts[i][0]
        };
        let front = mesh_of(&d, &r, "Hair front").parent.is_some();
        assert!(front);
        assert!(shift("Hair front", false).abs() > 3.0 * shift("Hair front", true).abs().max(0.5), "the tip swings more than the root");
        let (l, rt) = (shift("Hair side L", false), shift("Hair side R", false));
        assert!(l * rt < 0.0, "the sides swing apart: {l} {rt}");
        // the hair keeps the head turn: the face rig is still there
        assert!(r.params.iter().any(|p| p.name == "Angle X"));
        // no hair tagged: a reason
        let mut e = Rig::default();
        let mut only = Document::new(100, 100);
        add(&mut only, "x", None, (10, 10, 50, 50));
        assert!(rig_hair(&mut e, &only.layers, 100, 100).unwrap_err().contains("Tag the hair"));
    }

    #[test]
    fn sliders_have_two_or_three_key_points_with_the_original_drawing_at_the_default() {
        let d = face();
        let mut r = Rig::default();
        rig_face(&mut r, &d.layers, d.w, d.h, &FaceOptions::default()).unwrap();
        let keys = |n: &str| r.key_points(param(&r, n));
        // three key points: left, the original drawing in the middle, right
        assert_eq!(keys("Angle X"), [-30.0, 0.0, 30.0]);
        assert_eq!(keys("Mouth form"), [-1.0, 0.0, 1.0]);
        // two key points: the original drawing at the left (closed mouth, brows in their place)
        assert_eq!(keys("Mouth open"), [0.0, 1.0]);
        assert_eq!(keys("Brows"), [0.0, 1.0]);
        // eyes: open is the original drawing, at the right end
        assert_eq!(keys("Eyes open"), [0.0, 1.0]);
        assert_eq!(keys("Eyes smile"), [0.0, 1.0], "two key points, the original drawing at the left");
        // what is drawn on a slider: the ends and the original too, and which points have a saved shape
        let ax = param(&r, "Angle X");
        assert_eq!(r.display_points(ax), [-30.0, 0.0, 30.0]);
        assert!(r.key_is_set(ax, 30.0) && r.key_is_set(ax, -30.0) && !r.key_is_set(ax, 0.0), "the original drawing is never 'set'");
        let bare = r.params.iter().find(|p| p.name == "Head tilt").map(|p| p.id);
        assert!(bare.is_none());
        let eyes = r.param(param(&r, "Eyes open")).unwrap();
        assert_eq!((eyes.min, eyes.default, eyes.max), (0.0, 1.0, 1.0));
        let brows = r.param(param(&r, "Brows")).unwrap();
        assert_eq!((brows.min, brows.default, brows.max), (0.0, 0.0, 1.0));
    }

    #[test]
    fn the_middle_key_can_be_changed_and_put_back_to_the_original_drawing() {
        let d = face();
        let mut r = Rig::default();
        rig_face(&mut r, &d.layers, d.w, d.h, &FaceOptions::default()).unwrap();
        let ax = param(&r, "Angle X");
        let nose = r.changers.iter().find(|c| c.name == "Turn Nose").unwrap().id;
        // edit the middle key: the nose is moved at the middle of the slider
        r.set_keyform(nose, ax, 0.0, Form::Warp { offsets: vec![[20.0, 0.0]; 4] });
        let first = mesh_of(&d, &r, "Nose").verts[0][0];
        let shift = |r: &Rig| {
            let m = mesh_of(&d, r, "Nose");
            r.deformed(m, &r.pose(&Values::default()))[0][0] - first
        };
        let at_mid = shift(&r);
        assert!((at_mid - 20.0).abs() < 0.5, "the middle key is editable: {at_mid}");
        // put it back to the original drawing
        r.reset_key(ax, 0.0);
        let back = shift(&r);
        assert!(back.abs() < 0.01, "{back}");
        // the ends can be removed, the middle one stays (it is the original drawing)
        r.remove_key(ax, 30.0);
        assert_eq!(r.key_points(ax), [-30.0, 0.0], "a key point belongs to the slider: it goes for every changer");
        r.remove_key(ax, 0.0);
        assert!(r.key_points(ax).contains(&0.0));
    }

    #[test]
    fn eyeballs_look_around_inside_the_eye_and_the_pads_join_the_sliders() {
        let mut d = face();
        add(&mut d, "iris L", Some(Part::EyeballL), (585, 405, 615, 445));
        add(&mut d, "iris R", Some(Part::EyeballR), (385, 405, 415, 445));
        let mut r = Rig::default();
        rig_face(&mut r, &d.layers, d.w, d.h, &FaceOptions::default()).unwrap();
        let (bx, by) = (param(&r, "Eyeball X"), param(&r, "Eyeball Y"));
        let pad = r.pads.iter().find(|p| p.name == "Eyeballs").expect("eyeball X and Y are one pad");
        assert_eq!((pad.x, pad.y), (bx, by));
        let ball = mesh_of(&d, &r, "iris L");
        let mut v = Values::default();
        v.set(bx, 1.0);
        let now = r.deformed(ball, &r.pose(&v));
        let dx = now[0][0] - ball.verts[0][0];
        assert!(dx > 5.0 && dx < 30.0, "it looks right, but stays inside the eye: {dx}");
        let eye = mesh_of(&d, &r, "Eye L");
        assert!((r.deformed(eye, &r.pose(&v))[0][0] - eye.verts[0][0]).abs() < 1.0, "the eye itself stays");
        v.set(by, 1.0);
        assert!(r.deformed(ball, &r.pose(&v))[0][1] < ball.verts[0][1], "up looks up");
        // one eyeball slider pair per eye when the eyes are separate
        let mut sep = Rig::default();
        rig_face(&mut sep, &d.layers, d.w, d.h, &FaceOptions { together_eyes: false, ..FaceOptions::default() }).unwrap();
        assert!(sep.params.iter().any(|p| p.name == "Eyeball L X") && sep.params.iter().any(|p| p.name == "Eyeball R Y"));
        assert!(sep.pads.iter().any(|p| p.name == "Eyeball L") && sep.pads.iter().any(|p| p.name == "Eyeball R"));
        // making the rig again does not pile up pads
        let n = r.pads.len();
        rig_face(&mut r, &d.layers, d.w, d.h, &FaceOptions::default()).unwrap();
        assert_eq!(r.pads.len(), n);
        // and cleaning keeps them
        assert_eq!(r.clone().sanitized().pads.len(), n);
        // a pad that points at a slider that is gone is dropped
        let mut bad = r.clone();
        bad.pads.push(crate::rig::Pad { name: "x".into(), x: 9999, y: bx });
        assert_eq!(bad.sanitized().pads.len(), n);
    }

    #[test]
    fn mouth_form_smiles_and_frowns_and_the_brows_only_go_up() {
        let d = face();
        let mut r = Rig::default();
        rig_face(&mut r, &d.layers, d.w, d.h, &FaceOptions::default()).unwrap();
        let form = param(&r, "Mouth form");
        let m = mesh_of(&d, &r, "Mouth");
        // the leftmost vertices are the corners of the mouth
        let corner = (0..m.verts.len()).min_by(|a, b| m.verts[*a][0].total_cmp(&m.verts[*b][0])).unwrap();
        let mut v = Values::default();
        v.set(form, 1.0);
        assert!(r.deformed(m, &r.pose(&v))[corner][1] < m.verts[corner][1] - 3.0, "smile: the corners go up");
        v.set(form, -1.0);
        assert!(r.deformed(m, &r.pose(&v))[corner][1] > m.verts[corner][1] + 3.0, "sad: the corners go down");
        v.set(form, 0.0);
        assert!((r.deformed(m, &r.pose(&v))[corner][1] - m.verts[corner][1]).abs() < 0.01, "the middle is the original drawing");
    }
}
