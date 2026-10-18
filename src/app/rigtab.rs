// SPDX-License-Identifier: GPL-3.0-or-later
// The Rig tab: parts on the left, the moving picture in the middle, meshes, changers and sliders on the right.
// The picture is drawn through the meshes, so a slider moves the drawing the way the changers say.

use std::collections::HashMap;
use std::hash::{Hash, Hasher};
use std::sync::Arc;

use eframe::egui::{self, Color32, Pos2, Rect, RichText, Sense, Stroke, TextureHandle, TextureOptions, Ui, Vec2, pos2, vec2};

use super::SeviathApp;
use crate::doc::{Document, LayerId};
use crate::parts;
use crate::rig::raster::{self, Item, LayerTex};
use crate::rig::{self, ArtMesh, Form, Id, Kind, Rig, Values};
use crate::tabs::Tab;
use crate::theme::*;
use crate::widgets::{self, section_title};

const MAX_PREVIEW: f32 = 900.0;
/// Frames a second when recording a pose.
const RECORD_FPS: f32 = 30.0;

#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
enum Mode {
    #[default]
    Look,
    /// The next click on the picture is the pivot of the selected Rotation changer.
    Pivot,
    /// Drag on the picture from the head of a new bone to its tail.
    Bone,
    /// Paint the weights of the picked bone (the picture is shown at rest).
    Weight,
}

/// What dragging does in weight mode.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum WTool {
    Brush(rig::skin::Brush),
    /// Draw an outline: everything inside goes to the bone.
    Lasso,
    /// Draw an outline: everything inside is taken from the bone.
    LassoRemove,
}

impl Default for WTool {
    fn default() -> Self {
        WTool::Brush(rig::skin::Brush::Add)
    }
}

impl RigState {
    /// Show the shape being edited instead of the saved one.
    fn put_edit(&self, rig: &Rig, pose: &mut rig::Pose) {
        let Some((id, off)) = &self.warp_edit else { return };
        match rig.changer(*id).map(|c| &c.kind) {
            Some(Kind::Warp { .. }) => {
                pose.warp.insert(*id, off.clone());
            }
            Some(Kind::Line { .. }) => {
                pose.line.insert(*id, off.clone());
            }
            Some(Kind::Soft { .. }) => {
                pose.soft.insert(*id, (off.first().copied().unwrap_or([0.0, 0.0]), self.soft_grow));
            }
            _ => {}
        }
    }

    #[cfg(test)]
    pub fn test_warp_edit(&mut self) -> Option<&mut Vec<[f32; 2]>> {
        self.warp_edit.as_mut().map(|w| &mut w.1)
    }
}

#[derive(Default)]
pub struct RigState {
    pub selected: Vec<LayerId>,
    anchor: Option<LayerId>,
    density: f32,
    wire: bool,
    pub sel_changer: Option<Id>,
    pub sel_param: Option<Id>,
    pub values: Values,
    mode: Mode,
    /// A Warp being edited: the changer and its control point shifts as they are on screen now.
    warp_edit: Option<(Id, Vec<[f32; 2]>)>,
    /// How much a Soft changer being edited grows.
    pub soft_grow: f32,
    /// A second slider: the shape is then saved for the two sliders together (a grid).
    pub sel_param2: Option<Id>,
    skin_detail: f32,
    pub heat: bool,
    skin_tex: Option<(u64, Option<Arc<LayerTex>>)>,
    heat_tex: Option<((usize, u64), TextureHandle)>,
    bone_from: Option<[f32; 2]>,
    bone_chain: bool,
    pub wtool: WTool,
    pub face_opts: rig::autorig::FaceOptions,
    pub body_opts: rig::autorig::BodyOptions,
    pub gizmo: bool,
    pub gtarget: usize,
    pub snap: f32,
    gdrag: Option<(rig::gizmo::Axis, Pos2, f32)>,
    pub warnings: Vec<String>,
    /// Where the rings were drawn last: centre and radius on the screen (for the tests).
    pub gizmo_geom: Option<(Pos2, f32)>,
    /// Recording the sliders: when it started, the time of the last frame, and the frames so far.
    recording: Option<(f64, f64, Vec<Vec<(Id, f32)>>)>,
    /// Playing a clip: which one and when it started.
    playing: Option<(usize, f64)>,
    pub issues: Vec<rig::check::Issue>,
    checked: bool,
    pub pro: bool,
    play: bool,
    wopts: rig::skin::BrushOpts,
    feather: f32,
    lasso_pts: Vec<[f32; 2]>,
    last_dab: Option<[f32; 2]>,
    weight_before: Option<crate::history::DocState>,
    weight_gen: u64,
    pub mirror_axis: f32,
    pub mirror_to: Option<Id>,
    tag_pick: Option<parts::Part>,
    glue_reach: f32,
    pub new_param: (String, f32, f32),
    turn_deg: f32,
    cols_rows: (u32, u32),
    tex: HashMap<LayerId, (u64, Option<Arc<LayerTex>>)>,
    image: Option<TextureHandle>,
    rendered: Option<u64>,
    /// pixels of the picture per page pixel
    scale: f32,
    zoom: f32,
    pan: Vec2,
    message: Option<String>,
    inited: bool,
}

fn hash_of(items: &[u64]) -> u64 {
    let mut h = std::collections::hash_map::DefaultHasher::new();
    items.hash(&mut h);
    h.finish()
}

impl SeviathApp {
    /// Change the rig as one undo step of the Rig tab.
    pub(super) fn rig_edit(&mut self, label: &str, now: f64, f: impl FnOnce(&mut Rig)) {
        let before = self.draw.doc.snapshot();
        f(Arc::make_mut(&mut self.draw.doc.rig));
        if self.draw.doc.same_as(&before) {
            return;
        }
        let after = self.draw.doc.snapshot();
        self.log_step(label, now);
        self.history.push(label.to_owned(), Tab::Rig, before, after, now);
    }

    #[cfg(test)]
    pub fn test_rig(&mut self) -> &mut RigState {
        &mut self.rigs
    }

    /// Paint one dab (a test stand-in for dragging), as one undo step.
    #[cfg(test)]
    pub fn test_paint(&mut self, at: [f32; 2], brush: rig::skin::Brush, opts: rig::skin::BrushOpts) {
        let Some(bi) = self.rigs.sel_changer.and_then(|c| self.draw.doc.rig.skin.as_ref().and_then(|s| s.bone_index(c))) else { return };
        let before = self.draw.doc.snapshot();
        if let Some(sk) = self.skin_mut() {
            sk.paint(bi, at[0], at[1], brush, &opts);
        }
        self.rigs.weight_gen += 1;
        let after = self.draw.doc.snapshot();
        self.history.push("Paint weights".into(), Tab::Rig, before, after, 0.0);
    }

    #[cfg(test)]
    pub fn test_weight_mode(&mut self, on: bool) {
        self.rigs.mode = if on { Mode::Weight } else { Mode::Look };
    }

    #[cfg(test)]
    pub fn test_add_bone(&mut self, head: [f32; 2], tail: [f32; 2]) {
        self.add_bone(head, tail, 0.0);
    }

    #[cfg(test)]
    pub fn test_rig_edit(&mut self, f: impl FnOnce(&mut Rig)) {
        self.rig_edit("Rig edit", 0.0, f);
    }

    /// How far the first mesh's vertices are from where they were (page pixels), for the tests.
    #[cfg(test)]
    pub fn test_moved(&mut self, layer: LayerId) -> f32 {
        let items = self.rig_items();
        items.iter().find(|i| i.0 == layer).map(|(_, _, rest, now, ..)| rest.iter().zip(now).map(|(a, b)| (a[0] - b[0]).hypot(a[1] - b[1])).fold(0.0, f32::max)).unwrap_or(0.0)
    }

    fn layer_tex(&mut self, id: LayerId) -> Option<Arc<LayerTex>> {
        let d = &self.draw.doc;
        let l = d.layers.iter().find(|l| l.id == id)?;
        if let Some((rev, t)) = self.rigs.tex.get(&id) {
            if *rev == l.rev {
                return t.clone();
            }
        }
        let t = LayerTex::of(l, d.w, d.h).map(Arc::new);
        self.rigs.tex.insert(id, (l.rev, t.clone()));
        t
    }

    /// The layers of the skin drawn together as one picture.
    fn skin_texture(&mut self) -> Option<Arc<LayerTex>> {
        let skin = self.draw.doc.rig.skin.clone()?;
        let d = &self.draw.doc;
        let mut key: Vec<u64> = vec![d.w as u64, d.h as u64];
        for l in d.layers.iter().filter(|l| skin.layers.contains(&l.id)) {
            key.extend([l.id, l.rev, l.visible as u64, l.opacity.to_bits() as u64, format!("{:?}", l.blend).len() as u64]);
        }
        let key = hash_of(&key);
        if let Some((k, t)) = &self.rigs.skin_tex {
            if *k == key {
                return t.clone();
            }
        }
        let t = skin_composite(d, &skin.layers).map(|img| Arc::new(LayerTex::from_rgba(&img)));
        self.rigs.skin_tex = Some((key, t.clone()));
        t
    }

    /// Everything that is drawn, bottom first: (layer, mesh or a plain box, opacity).
    fn rig_items(&mut self) -> Vec<(LayerId, Arc<LayerTex>, Vec<[f32; 2]>, Vec<[f32; 2]>, Vec<[u32; 3]>, f32)> {
        let rig = self.draw.doc.rig.clone();
        let painting = self.rigs.mode == Mode::Weight;
        let mut pose = if painting { rig.pose(&Values::default()) } else { rig.pose(&self.rigs.values) };
        // a Warp being edited shows the shape being dragged
        self.rigs.put_edit(&rig, &mut pose);
        let all = rig.deformed_all(&pose);
        let ids: Vec<(LayerId, f32)> = self.draw.doc.layers.iter().rev().filter(|l| l.visible && matches!(l.kind, crate::doc::LayerKind::Raster | crate::doc::LayerKind::Vector)).map(|l| (l.id, l.opacity / 100.0)).collect();
        let mut out = Vec::new();
        let skin = rig.skin.clone();
        let skin_tex = skin.as_ref().and_then(|_| self.skin_texture());
        let mut skin_done = false;
        for (id, op) in ids {
            // the layers of the skin are drawn as one picture, where the lowest of them is
            if let (Some(sk), Some(tex)) = (&skin, &skin_tex) {
                if sk.layers.contains(&id) {
                    if !skin_done {
                        skin_done = true;
                        out.push((u64::MAX, tex.clone(), sk.verts.clone(), sk.skinned(&rig, &pose), sk.tris.clone(), 1.0));
                    }
                    continue;
                }
            }
            let Some(tex) = self.layer_tex(id) else { continue };
            match rig.meshes.iter().position(|m| m.layer == id) {
                Some(mi) => {
                    let m = &rig.meshes[mi];
                    out.push((id, tex, m.verts.clone(), all[mi].clone(), m.tris.clone(), op));
                }
                None => {
                    // no mesh: stays where it is
                    let (x0, y0, x1, y1) = (tex.x0 as f32, tex.y0 as f32, (tex.x0 + tex.w as i32) as f32, (tex.y0 + tex.h as i32) as f32);
                    let v = vec![[x0, y0], [x1, y0], [x1, y1], [x0, y1]];
                    out.push((id, tex, v.clone(), v, vec![[0, 1, 2], [0, 2, 3]], op));
                }
            }
        }
        out
    }

    fn rig_render(&mut self, ctx: &egui::Context) {
        let d = &self.draw.doc;
        let mut key: Vec<u64> = vec![d.w as u64, d.h as u64, Arc::as_ptr(&d.rig) as usize as u64];
        for l in &d.layers {
            key.extend([l.id, l.rev, l.visible as u64, l.opacity.to_bits() as u64]);
        }
        key.push(self.rigs.weight_gen);
        key.push((self.rigs.mode == Mode::Weight) as u64);
        let mut vals: Vec<(Id, u32)> = self.rigs.values.0.iter().map(|(k, v)| (*k, v.to_bits())).collect();
        vals.sort();
        for (k, v) in vals {
            key.extend([k as u64, v as u64]);
        }
        if let Some((id, off)) = &self.rigs.warp_edit {
            key.push(*id as u64);
            for o in off {
                key.extend([o[0].to_bits() as u64, o[1].to_bits() as u64]);
            }
            key.push(self.rigs.soft_grow.to_bits() as u64);
        }
        let key = hash_of(&key);
        if self.rigs.rendered == Some(key) && self.rigs.image.is_some() {
            return;
        }
        let (w, h) = (d.w as f32, d.h as f32);
        let scale = (MAX_PREVIEW / w.max(h)).min(1.0);
        let (pw, ph) = (((w * scale) as usize).max(8), ((h * scale) as usize).max(8));
        let items = self.rig_items();
        let list: Vec<Item> = items.iter().map(|(_, tex, rest, now, tris, op)| Item { tex, rest, now: now.clone(), tris, opacity: *op }).collect();
        let paper = Color32::from_rgb(244, 242, 236);
        let px = raster::render(&list, pw, ph, scale, Some(paper));
        let img = egui::ColorImage::new([pw, ph], px);
        self.rigs.image = Some(ctx.load_texture("rig_preview", img, TextureOptions::LINEAR));
        self.rigs.scale = scale;
        self.rigs.rendered = Some(key);
    }

    fn rig_select(&mut self, id: LayerId, ctrl: bool, shift: bool) {
        let order: Vec<LayerId> = self.draw.doc.layers.iter().filter(|l| parts::is_part_layer(l)).map(|l| l.id).collect();
        let r = &mut self.rigs;
        if shift && r.anchor.is_some() {
            let (a, b) = (order.iter().position(|x| Some(*x) == r.anchor), order.iter().position(|x| *x == id));
            if let (Some(a), Some(b)) = (a, b) {
                r.selected = order[a.min(b)..=a.max(b)].to_vec();
            }
        } else if ctrl {
            match r.selected.iter().position(|x| *x == id) {
                Some(i) => {
                    r.selected.remove(i);
                }
                None => r.selected.push(id),
            }
            r.anchor = Some(id);
        } else {
            r.selected = vec![id];
            r.anchor = Some(id);
        }
    }

    /// The bounds of the selected layers' paint: (x0, y0, x1, y1).
    fn selection_box(&self) -> Option<[f32; 4]> {
        let d = &self.draw.doc;
        let mut b: Option<[f32; 4]> = None;
        for l in d.layers.iter().filter(|l| self.rigs.selected.contains(&l.id)) {
            if let Some((x0, y0, x1, y1)) = crate::files::layer_bounds(l, d.w, d.h) {
                let r = [x0 as f32, y0 as f32, x1 as f32, y1 as f32];
                b = Some(match b {
                    Some(o) => [o[0].min(r[0]), o[1].min(r[1]), o[2].max(r[2]), o[3].max(r[3])],
                    None => r,
                });
            }
        }
        b
    }

    /// One mesh over the given layers (kept bones and weights stay when the mesh is made again).
    fn make_skin(&mut self, ids: Vec<LayerId>, now: f64) {
        let d = &self.draw.doc;
        let Some(img) = skin_composite(d, &ids) else {
            self.rigs.message = Some("Nothing to mesh: no visible layer has paint.".into());
            return;
        };
        let mut scratch = Document::new(d.w, d.h);
        let mut tmp = scratch.new_layer("skin", crate::doc::LayerKind::Raster);
        tmp.tiles = crate::files::tiles_from_rgba(&img, 0, 0, d.w, d.h);
        let (w, h) = (d.w, d.h);
        let Some((verts, tris)) = rig::mesh::generate(&tmp, w, h, self.rigs.skin_detail) else {
            self.rigs.message = Some("Nothing to mesh: no visible layer has paint.".into());
            return;
        };
        let nt = tris.len();
        self.rig_edit("Make the uncut skin", now, |r| match r.skin.as_mut() {
            Some(s) => {
                s.layers = ids.clone();
                s.verts = verts;
                s.tris = tris;
            }
            None => r.skin = Some(rig::skin::Skin::new(ids.clone(), verts, tris, w, h)),
        });
        self.rigs.skin_tex = None;
        self.rigs.message = Some(format!("One mesh with {nt} triangles over {} layers.", ids.len()));
    }

    fn add_bone(&mut self, head: [f32; 2], tail: [f32; 2], now: f64) {
        let n = self.draw.doc.rig.skin.as_ref().map(|s| s.bones.len()).unwrap_or(0) + 1;
        let parent = if self.rigs.bone_chain {
            self.rigs.sel_changer.filter(|c| self.draw.doc.rig.skin.as_ref().is_some_and(|s| s.bone_index(*c).is_some()))
        } else {
            None
        };
        let mut new_id = 0;
        self.rig_edit("Add a bone", now, |r| {
            let id = r.add_changer(&format!("Bone {n}"), Kind::Rotation { pivot: head }, parent);
            if let Some(s) = r.skin.as_mut() {
                s.add_bone(id, tail);
            }
            new_id = id;
        });
        self.rigs.sel_changer = Some(new_id);
    }

    fn make_meshes(&mut self, ids: Vec<LayerId>, now: f64) {
        let density = self.rigs.density;
        let d = &self.draw.doc;
        let made: Vec<(LayerId, Option<(Vec<[f32; 2]>, Vec<[u32; 3]>)>)> = ids.iter().map(|id| (*id, d.layers.iter().find(|l| l.id == *id).and_then(|l| rig::mesh::generate(l, d.w, d.h, density)))).collect();
        let n = made.iter().filter(|m| m.1.is_some()).count();
        if n == 0 {
            self.rigs.message = Some("Nothing to mesh: pick layers that have paint.".into());
            return;
        }
        self.rig_edit(&format!("Make {n} meshes"), now, |r| {
            for (layer, m) in made {
                let Some((verts, tris)) = m else { continue };
                let parent = r.mesh_of(layer).and_then(|o| o.parent);
                r.meshes.retain(|o| o.layer != layer);
                let id = r.fresh_id();
                r.meshes.push(ArtMesh { id, layer, verts, tris, parent });
            }
        });
        self.rigs.message = Some(format!("{n} meshes made."));
    }

    pub(super) fn rig_ui(&mut self, ui: &mut Ui) {
        let ctx = ui.ctx().clone();
        let now = ctx.input(|i| i.time);
        if !self.rigs.inited {
            self.rigs.density = 0.5;
            self.rigs.wire = true;
            self.rigs.turn_deg = 30.0;
            self.rigs.cols_rows = (3, 3);
            self.rigs.glue_reach = 30.0;
            self.rigs.skin_detail = 0.6;
            self.rigs.bone_chain = true;
            self.rigs.wopts = rig::skin::BrushOpts::default();
            self.rigs.face_opts = rig::autorig::FaceOptions::default();
            self.rigs.body_opts = rig::autorig::BodyOptions::default();
            self.rigs.gizmo = true;
            self.rigs.feather = 60.0;
            self.rigs.mirror_axis = self.draw.doc.w as f32 / 2.0;
            self.rigs.tag_pick = Some(parts::Part::ArmL);
            self.rigs.new_param = ("Head tilt".into(), -30.0, 30.0);
            self.rigs.zoom = 1.0;
            self.rigs.inited = true;
        }
        self.rigs.selected.retain(|id| self.draw.doc.index_of(*id).is_some());
        {
            let rig = &self.draw.doc.rig;
            if self.rigs.sel_changer.is_some_and(|c| rig.changer(c).is_none()) {
                self.rigs.sel_changer = None;
                self.rigs.warp_edit = None;
            }
            if self.rigs.sel_param.is_some_and(|p| rig.param(p).is_none()) {
                self.rigs.sel_param = None;
            }
            if self.rigs.sel_param2.is_some_and(|p| rig.param(p).is_none()) {
                self.rigs.sel_param2 = None;
            }
        }
        // playing a recorded clip
        if let Some((ci, t0)) = self.rigs.playing {
            let now = ctx.input(|i| i.time);
            let frame = self.draw.doc.rig.clips.get(ci).and_then(|c| c.frame_at((now - t0) as f32)).cloned();
            match frame {
                Some(f) => {
                    self.rigs.values = self.draw.doc.rig.values_from(&f);
                    ctx.request_repaint();
                }
                None => self.rigs.playing = None,
            }
        }
        // recording the sliders while you pose
        if let Some((t0, last, frames)) = self.rigs.recording.as_mut() {
            let now = ctx.input(|i| i.time);
            if now - *last >= 1.0 / RECORD_FPS as f64 {
                *last = now;
                let kept = self.draw.doc.rig.values_to_keep(&self.rigs.values);
                frames.push(kept);
            }
            let _ = t0;
            ctx.request_repaint();
        }
        if self.rigs.play {
            // every slider swings between its ends, each at its own pace
            let t = ctx.input(|i| i.time) as f32;
            let params: Vec<(Id, f32, f32, f32)> = self.draw.doc.rig.params.iter().map(|p| (p.id, p.min, p.max, p.default)).collect();
            for (i, (id, lo, hi, def)) in params.iter().enumerate() {
                let k = (t * (0.7 + 0.13 * i as f32) + i as f32).sin();
                let v = if k >= 0.0 { def + (hi - def) * k } else { def + (def - lo) * k };
                self.rigs.values.set(*id, v);
            }
            ctx.request_repaint();
        }
        self.rig_render(&ctx);
        let mut picked: Option<(LayerId, bool, bool)> = None;

        egui::Panel::left("rig_layers").default_size(230.0).size_range(190.0..=380.0).resizable(true).frame(egui::Frame::new().fill(PANEL).inner_margin(egui::Margin::same(10))).show(ui, |ui| {
            section_title(ui, "Parts");
            ui.label(RichText::new("Pick the parts to mesh or move.").size(11.5).color(TEXT_DIM));
            egui::ScrollArea::vertical().id_salt("rig_parts").show(ui, |ui| {
                for l in self.draw.doc.layers.iter().filter(|l| parts::is_part_layer(l)) {
                    let on = self.rigs.selected.contains(&l.id);
                    let (rect, resp) = ui.allocate_exact_size(vec2(ui.available_width(), 24.0), Sense::click());
                    if on {
                        ui.painter().rect_filled(rect, 4.0, HOVER);
                    }
                    let dot = l.part.map(|p| p.color()).unwrap_or(Color32::from_gray(70));
                    ui.painter().circle_filled(rect.left_center() + vec2(10.0, 0.0), 5.0, dot);
                    ui.painter().text(rect.left_center() + vec2(22.0, 0.0), egui::Align2::LEFT_CENTER, &l.name, egui::FontId::proportional(13.0), if l.visible { TEXT } else { TEXT_DIM });
                    if self.draw.doc.rig.mesh_of(l.id).is_some() {
                        ui.painter().text(rect.right_center() - vec2(6.0, 0.0), egui::Align2::RIGHT_CENTER, "mesh", egui::FontId::proportional(11.0), VENOM);
                    }
                    if resp.clicked() {
                        let m = ctx.input(|i| i.modifiers);
                        picked = Some((l.id, m.command || m.ctrl, m.shift));
                    }
                }
            });
        });

        egui::Panel::right("rig_tools").default_size(330.0).size_range(280.0..=500.0).resizable(true).frame(egui::Frame::new().fill(PANEL).inner_margin(egui::Margin::same(10))).show(ui, |ui| {
            egui::ScrollArea::vertical().id_salt("rig_right").show(ui, |ui| self.rig_side(ui, now));
        });

        egui::CentralPanel::default().frame(egui::Frame::new().fill(Color32::from_rgb(14, 20, 18))).show(ui, |ui| self.rig_view(ui, now, &mut picked));

        if let Some((id, c, s)) = picked.filter(|p| p.0 != u64::MAX) {
            self.rig_select(id, c, s);
        }
    }

    fn rig_side(&mut self, ui: &mut Ui, now: f64) {
        ui.horizontal(|ui| {
            ui.label(RichText::new("Mode").size(12.0).color(TEXT_DIM));
            if widgets::chip(ui, "Beginner", !self.rigs.pro).on_hover_text("Just the automatic rig and the sliders").clicked() {
                self.rigs.pro = false;
            }
            if widgets::chip(ui, "Pro", self.rigs.pro).on_hover_text("Meshes, changers, glue, the uncut skin and keyforms").clicked() {
                self.rigs.pro = true;
            }
        });
        self.rig_auto_section(ui, now);
        self.rig_quality_section(ui, now);
        self.rig_pose_section(ui, now);
        if self.rigs.pro {
            self.rig_pro_sections(ui, now);
        }
        self.rig_sliders_section(ui, now);
    }

    /// Face rigging from the tags, in one click.
    fn rig_auto_section(&mut self, ui: &mut Ui, now: f64) {
        section_title(ui, "Auto-rig");
        ui.label(RichText::new("Head turn, eyes, brows and a mouth that makes A I U E O, built from the Parts tags.").size(11.5).color(TEXT_DIM));
        ui.horizontal_wrapped(|ui| {
            if ui.button(RichText::new("This is a face").strong()).on_hover_text("Tags the picked parts (or every layer) by their names, then rigs the face").clicked() {
                self.this_is_a_face(now);
            }
            if ui.button("Rig the face from the tags").on_hover_text("Replaces what the auto-rig made before, except what you locked").clicked() {
                self.rig_the_face(now);
            }
            if ui.button(if self.rigs.play { "Stop the test motion" } else { "Test motion" }).on_hover_text("Every slider moves by itself, so you can see what the rig does").clicked() {
                self.rigs.play = !self.rigs.play;
                if !self.rigs.play {
                    self.rigs.values = Values::default();
                }
            }
        });
        ui.horizontal_wrapped(|ui| {
            if ui.button("Rig the body").on_hover_text("Torso tilt and breathing, arms and legs that swing, hands that reach. Do the face first so the head leans with the body.").clicked() {
                self.rig_the_body(now);
            }
            if ui.button("Hair sway").on_hover_text("Curves down the hair parts and one slider").clicked() {
                self.rig_the_hair(now);
            }
            ui.checkbox(&mut self.rigs.body_opts.ik, "Hands reach (IK)");
        });
        let o = &mut self.rigs.face_opts;
        ui.horizontal(|ui| {
            ui.label(RichText::new("Head turn strength").size(12.0).color(TEXT_DIM));
            ui.add(egui::Slider::new(&mut o.turn, 0.2..=2.5).show_value(false));
        });
        ui.horizontal_wrapped(|ui| {
            ui.checkbox(&mut o.together_eyes, "Eyes together").on_hover_text("Off: one slider for each eye (for a face that is not symmetrical)");
            ui.checkbox(&mut o.together_brows, "Brows together");
            ui.checkbox(&mut o.vowels, "Mouth A I U E O");
        });
        if let Some(m) = &self.rigs.message {
            ui.label(RichText::new(m).size(11.5).color(TEXT_DIM));
        }
        // lock: the automation leaves a changer you made by hand alone
        if let (Some(cid), Some(a)) = (self.rigs.sel_changer, self.draw.doc.rig.auto.as_ref()) {
            if a.changers.contains(&cid) {
                let mut locked = a.locked.contains(&cid);
                if ui.checkbox(&mut locked, "Keep my edits of this changer (lock)").on_hover_text("The next auto-rig leaves it as it is").changed() {
                    self.rig_edit("Lock a changer", now, |r| {
                        if let Some(a) = r.auto.as_mut() {
                            a.locked.retain(|c| *c != cid);
                            if locked {
                                a.locked.push(cid);
                            }
                        }
                    });
                }
            }
        }
    }

    /// The rotation gizmo's panel: numbers, snapping, spin to the side, poses and recordings.
    fn rig_pose_section(&mut self, ui: &mut Ui, now: f64) {
        use rig::gizmo::{Axis, SPIN};
        section_title(ui, "Pose");
        let rig = self.draw.doc.rig.clone();
        let targets = rig::gizmo::targets(&rig);
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing = vec2(4.0, 4.0);
            ui.checkbox(&mut self.rigs.gizmo, "Turn rings on the picture");
            for (i, t) in targets.iter().enumerate() {
                if widgets::chip(ui, &t.name, self.rigs.gtarget == i).clicked() {
                    self.rigs.gtarget = i;
                }
            }
        });
        let Some(t) = targets.get(self.rigs.gtarget.min(targets.len().saturating_sub(1))).cloned() else {
            ui.label(RichText::new("The turn rings need the head or body sliders: use the auto-rig first.").size(11.5).color(TEXT_DIM));
            self.rig_pose_store(ui, now);
            return;
        };
        let mut changed = false;
        for (axis, label) in [(Axis::X, "Left and right"), (Axis::Y, "Up and down"), (Axis::Z, "Tilt")] {
            let Some(id) = t.param(axis) else { continue };
            let (lo, hi) = rig.param(id).map(|p| (p.min, p.max)).unwrap_or((-1.0, 1.0));
            let mut v = rig.value(&self.rigs.values, id);
            ui.horizontal(|ui| {
                ui.label(RichText::new(label).size(12.0).color(TEXT_DIM));
                if ui.add(egui::DragValue::new(&mut v).speed(0.3).range(lo..=hi).max_decimals(1)).changed() {
                    rig::gizmo::set_value(&rig, &mut self.rigs.values, id, v, self.rigs.snap);
                    changed = true;
                }
            });
        }
        ui.horizontal(|ui| {
            ui.label(RichText::new("Snap").size(12.0).color(TEXT_DIM));
            for (st, name) in [(0.0, "Off"), (5.0, "5"), (15.0, "15")] {
                if widgets::chip(ui, name, (self.rigs.snap - st).abs() < 1e-4).clicked() {
                    self.rigs.snap = st;
                }
            }
        });
        if t.x.is_some() {
            ui.horizontal_wrapped(|ui| {
                ui.spacing_mut().item_spacing = vec2(4.0, 4.0);
                for (name, share) in SPIN {
                    if widgets::chip(ui, name, false).clicked() {
                        rig::gizmo::spin(&rig, &mut self.rigs.values, &t, share);
                        changed = true;
                    }
                }
            });
        }
        if changed {
            self.refresh_warnings();
        }
        for w in &self.rigs.warnings {
            ui.label(RichText::new(format!("Warning: {w}")).size(11.5).color(Color32::from_rgb(235, 170, 80)));
        }
        self.rig_pose_store(ui, now);
    }

    fn refresh_warnings(&mut self) {
        let d = &self.draw.doc;
        self.rigs.warnings = rig::check::check_at(&d.rig, &d.layers, d.w, &self.rigs.values);
    }

    /// Key this pose (K): keep the sliders as a pose you can come back to.
    pub(super) fn rig_key_pose(&mut self, now: f64) {
        let rig = self.draw.doc.rig.clone();
        let kept = rig.values_to_keep(&self.rigs.values);
        let n = rig.poses.len() + 1;
        self.rig_edit("Key this pose", now, |r| r.poses.push(rig::SavedPose { name: format!("Pose {n}"), values: kept }));
        self.rigs.message = Some(format!("Pose {n} kept."));
    }

    #[cfg(test)]
    pub fn test_key_pose(&mut self) {
        self.rig_key_pose(0.0);
    }

    fn rig_pose_store(&mut self, ui: &mut Ui, now: f64) {
        let rig = self.draw.doc.rig.clone();
        ui.horizontal_wrapped(|ui| {
            if ui.button("Key this pose (K)").on_hover_text("Keeps the sliders as they are now").clicked() {
                self.rig_key_pose(now);
            }
            let rec = self.rigs.recording.is_some();
            if ui.button(if rec { "Stop recording" } else { "Record while posing" }).on_hover_text("Records the sliders as you move them, to use in the Animate tab").clicked() {
                if rec {
                    if let Some((t0, _, frames)) = self.rigs.recording.take() {
                        if frames.len() >= 2 {
                            let n = rig.clips.len() + 1;
                            self.rig_edit("Record a motion", now, |r| r.clips.push(rig::Clip { name: format!("Motion {n}"), fps: RECORD_FPS, frames }));
                            self.rigs.message = Some(format!("Motion {n} recorded ({:.1} s).", (now - t0).max(0.0)));
                        }
                    }
                } else {
                    self.rigs.playing = None;
                    self.rigs.recording = Some((now, now, Vec::new()));
                }
            }
        });
        if let Some((t0, _, frames)) = &self.rigs.recording {
            ui.label(RichText::new(format!("Recording: {:.1} s, {} frames", (now - t0).max(0.0), frames.len())).size(11.5).color(Color32::from_rgb(235, 90, 90)));
        }
        let mut recall: Option<usize> = None;
        let mut del_pose: Option<usize> = None;
        for (i, p) in rig.poses.iter().enumerate() {
            ui.horizontal(|ui| {
                ui.label(RichText::new(&p.name).size(12.5));
                if ui.small_button("Go to").clicked() {
                    recall = Some(i);
                }
                if ui.small_button("Delete").clicked() {
                    del_pose = Some(i);
                }
            });
        }
        if let Some(i) = recall {
            self.rigs.values = rig.values_from(&rig.poses[i].values);
            self.refresh_warnings();
        }
        if let Some(i) = del_pose {
            self.rig_edit("Delete a pose", now, |r| {
                if i < r.poses.len() {
                    r.poses.remove(i);
                }
            });
        }
        let mut play: Option<usize> = None;
        let mut del_clip: Option<usize> = None;
        for (i, c) in rig.clips.iter().enumerate() {
            let on = self.rigs.playing.is_some_and(|p| p.0 == i);
            ui.horizontal(|ui| {
                ui.label(RichText::new(format!("{} ({:.1} s)", c.name, c.seconds())).size(12.5));
                if ui.small_button(if on { "Stop" } else { "Play" }).clicked() {
                    play = Some(if on { usize::MAX } else { i });
                }
                if ui.small_button("Delete").clicked() {
                    del_clip = Some(i);
                }
            });
        }
        if let Some(i) = play {
            self.rigs.playing = if i == usize::MAX { None } else { Some((i, now)) };
            if i == usize::MAX {
                self.rigs.values = Values::default();
            }
        }
        if let Some(i) = del_clip {
            self.rigs.playing = None;
            self.rig_edit("Delete a motion", now, |r| {
                if i < r.clips.len() {
                    r.clips.remove(i);
                }
            });
        }
    }

    /// The quality checker: moves every slider to its ends and lists what goes wrong, with Fix buttons.
    fn rig_quality_section(&mut self, ui: &mut Ui, now: f64) {
        section_title(ui, "Quality check");
        ui.horizontal(|ui| {
            if ui.button("Check the rig").on_hover_text("Moves every slider to its ends and looks for gaps, folds, parts without a mesh and sliders that do nothing").clicked() {
                self.run_check();
            }
            if self.rigs.checked && self.rigs.issues.is_empty() {
                ui.label(RichText::new("Nothing found.").color(VENOM));
            }
        });
        let issues = self.rigs.issues.clone();
        if issues.iter().filter(|i| i.fix.is_some()).count() > 1 && ui.button("Fix everything that can be fixed").clicked() {
            let (layers, w, h) = (self.draw.doc.layers.clone(), self.draw.doc.w, self.draw.doc.h);
            self.rig_edit("Fix the rig problems", now, |r| {
                for i in issues.iter().filter(|i| i.fix.is_some()) {
                    rig::check::fix(r, i, &layers, w, h);
                }
            });
            self.run_check();
        }
        let mut fix: Option<usize> = None;
        for (i, is) in issues.iter().enumerate() {
            ui.horizontal_wrapped(|ui| {
                ui.label(RichText::new(&is.text).size(12.0));
                if let Some(label) = is.fix {
                    if ui.small_button(label).clicked() {
                        fix = Some(i);
                    }
                }
            });
        }
        if let Some(i) = fix {
            let (layers, w, h) = (self.draw.doc.layers.clone(), self.draw.doc.w, self.draw.doc.h);
            let issue = issues[i].clone();
            self.rig_edit("Fix a rig problem", now, |r| {
                rig::check::fix(r, &issue, &layers, w, h);
            });
            self.run_check();
        }
    }

    fn run_check(&mut self) {
        let d = &self.draw.doc;
        self.rigs.issues = rig::check::check(&d.rig, &d.layers, d.w);
        self.rigs.checked = true;
    }

    #[cfg(test)]
    pub fn test_check(&mut self) -> usize {
        self.run_check();
        self.rigs.issues.len()
    }

    fn rig_the_body(&mut self, now: f64) {
        let (layers, w, h, o) = (self.draw.doc.layers.clone(), self.draw.doc.w, self.draw.doc.h, self.rigs.body_opts);
        let mut result: Option<Result<rig::autorig::Report, String>> = None;
        self.rig_edit("Rig the body", now, |r| {
            let mut tmp = r.clone();
            let res = rig::autorig::rig_body(&mut tmp, &layers, w, h, &o);
            if res.is_ok() {
                *r = tmp;
            }
            result = Some(res);
        });
        self.rigs.values = Values::default();
        self.rigs.message = Some(match result {
            Some(Ok(rep)) => format!("Body rigged: {} meshes, {} changers, {} sliders. {}", rep.meshes, rep.changers, rep.params, rep.notes.join(" ")),
            Some(Err(e)) => e,
            None => String::new(),
        });
    }

    fn rig_the_hair(&mut self, now: f64) {
        let (layers, w, h) = (self.draw.doc.layers.clone(), self.draw.doc.w, self.draw.doc.h);
        let mut result: Option<Result<rig::autorig::Report, String>> = None;
        self.rig_edit("Rig the hair", now, |r| {
            let mut tmp = r.clone();
            let res = rig::autorig::rig_hair(&mut tmp, &layers, w, h);
            if res.is_ok() {
                *r = tmp;
            }
            result = Some(res);
        });
        self.rigs.message = Some(match result {
            Some(Ok(rep)) => format!("Hair rigged: {} changers. Run the face first if you want the hair to turn with the head.", rep.changers),
            Some(Err(e)) => e,
            None => String::new(),
        });
    }

    #[cfg(test)]
    pub fn test_rig_body(&mut self) {
        self.rig_the_body(0.0);
    }

    #[cfg(test)]
    pub fn test_rig_hair(&mut self) {
        self.rig_the_hair(0.0);
    }

    fn rig_the_face(&mut self, now: f64) {
        let (layers, w, h, o) = (self.draw.doc.layers.clone(), self.draw.doc.w, self.draw.doc.h, self.rigs.face_opts);
        let mut result: Option<Result<rig::autorig::Report, String>> = None;
        self.rig_edit("Rig the face", now, |r| {
            // a failed run must change nothing
            let mut tmp = r.clone();
            let res = rig::autorig::rig_face(&mut tmp, &layers, w, h, &o);
            if res.is_ok() {
                *r = tmp;
            }
            result = Some(res);
        });
        self.rigs.values = Values::default();
        self.rigs.message = Some(match result {
            Some(Ok(rep)) => {
                self.rigs.sel_param = self.draw.doc.rig.params.first().map(|p| p.id);
                format!("Face rigged: {} meshes, {} changers, {} sliders. {}", rep.meshes, rep.changers, rep.params, rep.notes.join(" "))
            }
            Some(Err(e)) => e,
            None => String::new(),
        });
    }

    /// Tag the picked parts (or every layer) by their names, then rig the face.
    fn this_is_a_face(&mut self, now: f64) {
        let ids: Vec<LayerId> = if self.rigs.selected.is_empty() { self.draw.doc.layers.iter().map(|l| l.id).collect() } else { self.rigs.selected.clone() };
        let layers: Vec<crate::doc::Layer> = self.draw.doc.layers.iter().filter(|l| ids.contains(&l.id)).cloned().collect();
        let found = parts::suggest_with(&layers, self.draw.doc.w, self.draw.doc.h, false, &self.parts.rules);
        let take: Vec<parts::Suggestion> = found.into_iter().filter(|s| s.confidence >= 0.3).collect();
        if !take.is_empty() {
            let n = take.len();
            self.parts_edit(&format!("Tag {n} layers (this is a face)"), now, |d| {
                for s in &take {
                    if let Some(l) = d.layers.iter_mut().find(|l| l.id == s.layer) {
                        l.part = Some(s.part);
                    }
                }
            });
        }
        self.rig_the_face(now);
    }

    #[cfg(test)]
    pub fn test_rig_the_face(&mut self) {
        self.rig_the_face(0.0);
    }

    fn rig_pro_sections(&mut self, ui: &mut Ui, now: f64) {
        // ---- meshes ----
        section_title(ui, "Meshes");
        ui.label(RichText::new("A mesh cuts a layer into triangles, so it can bend.").size(11.5).color(TEXT_DIM));
        ui.horizontal(|ui| {
            ui.label(RichText::new("Detail").size(12.0).color(TEXT_DIM));
            ui.add(egui::Slider::new(&mut self.rigs.density, 0.0..=1.0).show_value(false));
        });
        let sel = self.rigs.selected.clone();
        ui.horizontal_wrapped(|ui| {
            if ui.add_enabled(!sel.is_empty(), egui::Button::new("Make mesh for the picked parts")).clicked() {
                self.make_meshes(sel.clone(), now);
            }
            if ui.button("Mesh all tagged parts").clicked() {
                let ids: Vec<LayerId> = self.draw.doc.layers.iter().filter(|l| parts::is_part_layer(l) && l.part.is_some()).map(|l| l.id).collect();
                self.make_meshes(ids, now);
            }
        });
        ui.horizontal(|ui| {
            if ui.add_enabled(!sel.is_empty(), egui::Button::new("Remove mesh")).clicked() {
                self.rig_edit("Remove meshes", now, |r| r.meshes.retain(|m| !sel.contains(&m.layer)));
            }
            ui.checkbox(&mut self.rigs.wire, "Show the triangles");
        });
        if let Some(m) = &self.rigs.message {
            ui.label(RichText::new(m).size(11.5).color(TEXT_DIM));
        }
        let (nm, nt) = (self.draw.doc.rig.meshes.len(), self.draw.doc.rig.meshes.iter().map(|m| m.tris.len()).sum::<usize>());
        ui.label(RichText::new(format!("{nm} meshes, {nt} triangles. Parts without a mesh stay where they are.")).size(11.5).color(TEXT_DIM));

        // ---- uncut skin ----
        section_title(ui, "Uncut skin: the whole drawing in one piece");
        ui.label(RichText::new("No cutting: one mesh over the drawing, bones on top, and weights say how much each part follows each bone.").size(11.5).color(TEXT_DIM));
        ui.horizontal(|ui| {
            ui.label(RichText::new("Detail").size(12.0).color(TEXT_DIM));
            ui.add(egui::Slider::new(&mut self.rigs.skin_detail, 0.0..=1.0).show_value(false));
        });
        let has_skin = self.draw.doc.rig.skin.is_some();
        ui.horizontal_wrapped(|ui| {
            let label = if has_skin { "Make the skin mesh again" } else { "Make one mesh of the drawing" };
            if ui.button(label).on_hover_text("Uses the picked parts, or every visible layer when none is picked").clicked() {
                let ids: Vec<LayerId> = if sel.is_empty() {
                    self.draw.doc.layers.iter().filter(|l| l.visible && matches!(l.kind, crate::doc::LayerKind::Raster | crate::doc::LayerKind::Vector)).map(|l| l.id).collect()
                } else {
                    sel.clone()
                };
                self.make_skin(ids, now);
            }
            if has_skin && ui.button("Remove the skin").clicked() {
                self.rig_edit("Remove the skin", now, |r| r.skin = None);
                self.rigs.skin_tex = None;
            }
        });
        if has_skin {
            let on = self.rigs.mode == Mode::Bone;
            ui.horizontal_wrapped(|ui| {
                if widgets::chip(ui, if on { "Drag on the picture: head to tail…" } else { "Add a bone" }, on).clicked() {
                    self.rigs.mode = if on { Mode::Look } else { Mode::Bone };
                }
                ui.checkbox(&mut self.rigs.bone_chain, "Inside the picked bone");
            });
            let bones: Vec<(Id, String)> = self.draw.doc.rig.skin.as_ref().map(|s| s.bones.iter().filter_map(|b| self.draw.doc.rig.changer(b.changer).map(|c| (b.changer, c.name.clone()))).collect()).unwrap_or_default();
            ui.horizontal_wrapped(|ui| {
                ui.spacing_mut().item_spacing = vec2(4.0, 4.0);
                for (id, name) in &bones {
                    if widgets::chip(ui, name, self.rigs.sel_changer == Some(*id)).clicked() {
                        self.rigs.sel_changer = Some(*id);
                        self.rigs.warp_edit = None;
                    }
                }
            });
            let picked_bone = self.rigs.sel_changer.filter(|c| bones.iter().any(|b| b.0 == *c));
            ui.horizontal_wrapped(|ui| {
                if ui.add_enabled(!bones.is_empty(), egui::Button::new("Weights from the bones")).on_hover_text("Close to a bone it follows that bone, fading out with distance").clicked() {
                    self.rig_edit("Automatic weights", now, |r| {
                        let rc = r.clone();
                        if let Some(s) = r.skin.as_mut() {
                            s.auto_weights(&rc);
                        }
                    });
                }
                if ui.add_enabled(picked_bone.is_some(), egui::Button::new("Slider for this bone")).on_hover_text("A slider that turns the picked bone").clicked() {
                    if let Some(bid) = picked_bone {
                        let name = self.draw.doc.rig.changer(bid).map(|c| c.name.clone()).unwrap_or_default();
                        let mut pid = 0;
                        self.rig_edit("Slider for a bone", now, |r| {
                            pid = r.add_param(&format!("{name} turn"), -45.0, 45.0, 0.0);
                            r.bind_rotation(bid, pid, 45.0);
                        });
                        self.rigs.sel_param = Some(pid);
                    }
                }
                if ui.add_enabled(picked_bone.is_some(), egui::Button::new("Delete the bone")).clicked() {
                    if let Some(bid) = picked_bone {
                        self.rig_edit("Delete a bone", now, |r| {
                            r.changers.retain(|c| c.id != bid);
                            r.bindings.retain(|b| b.changer != bid);
                            for c in r.changers.iter_mut().filter(|c| c.parent == Some(bid)) {
                                c.parent = None;
                            }
                            if let Some(s) = r.skin.as_mut() {
                                s.remove_bone(bid);
                            }
                        });
                        self.rigs.sel_changer = None;
                    }
                }
            });
            ui.checkbox(&mut self.rigs.heat, "Show the picked bone's weights (blue little, red a lot)");
            self.rig_weight_tools(ui, now, picked_bone, &bones);
        }

        // ---- changers ----
        section_title(ui, "Changers");
        ui.horizontal_wrapped(|ui| {
            if ui.add_enabled(!sel.is_empty(), egui::Button::new("New Rotation changer")).on_hover_text("Turns the picked parts around a point (the middle of them to start with)").clicked() {
                if let Some(b) = self.selection_box() {
                    let pivot = [(b[0] + b[2]) / 2.0, (b[1] + b[3]) / 2.0];
                    let n = self.draw.doc.rig.changers.len() + 1;
                    let mut new_id = 0;
                    let selc = sel.clone();
                    self.rig_edit("New Rotation changer", now, |r| {
                        let id = r.add_changer(&format!("Rotation {n}"), Kind::Rotation { pivot }, None);
                        for m in r.meshes.iter_mut().filter(|m| selc.contains(&m.layer)) {
                            m.parent = Some(id);
                        }
                        new_id = id;
                    });
                    self.rigs.sel_changer = Some(new_id);
                }
            }
            if ui.add_enabled(!sel.is_empty(), egui::Button::new("New Warp changer")).on_hover_text("A grid over the picked parts: move its points to bend them").clicked() {
                if let Some(b) = self.selection_box() {
                    let (cols, rows) = self.rigs.cols_rows;
                    let n = self.draw.doc.rig.changers.len() + 1;
                    let mut new_id = 0;
                    let selc = sel.clone();
                    self.rig_edit("New Warp changer", now, |r| {
                        let id = r.add_changer(&format!("Warp {n}"), Kind::Warp { rect: b, cols, rows }, None);
                        for m in r.meshes.iter_mut().filter(|m| selc.contains(&m.layer)) {
                            m.parent = Some(id);
                        }
                        new_id = id;
                    });
                    self.rigs.sel_changer = Some(new_id);
                }
            }
        });
        ui.horizontal_wrapped(|ui| {
            if ui.add_enabled(!sel.is_empty(), egui::Button::new("New Line changer")).on_hover_text("A curve along the picked parts: they bend along it (hair strands, tails, ribbons)").clicked() {
                if let Some(b) = self.selection_box() {
                    let x = (b[0] + b[2]) / 2.0;
                    let pts: Vec<[f32; 2]> = (0..5).map(|i| [x, b[1] + (b[3] - b[1]) * i as f32 / 4.0]).collect();
                    let width = ((b[2] - b[0]) / 2.0).max(20.0);
                    let n = self.draw.doc.rig.changers.len() + 1;
                    let mut new_id = 0;
                    let selc = sel.clone();
                    self.rig_edit("New Line changer", now, |r| {
                        let id = r.add_changer(&format!("Line {n}"), Kind::Line { pts, width }, None);
                        for m in r.meshes.iter_mut().filter(|m| selc.contains(&m.layer)) {
                            m.parent = Some(id);
                        }
                        new_id = id;
                    });
                    self.rigs.sel_changer = Some(new_id);
                }
            }
            if ui.add_enabled(!sel.is_empty(), egui::Button::new("New Soft changer")).on_hover_text("A soft push around a point (cheeks, eyes, belly)").clicked() {
                if let Some(b) = self.selection_box() {
                    let center = [(b[0] + b[2]) / 2.0, (b[1] + b[3]) / 2.0];
                    let radius = ((b[2] - b[0]).max(b[3] - b[1]) / 2.0).max(20.0);
                    let n = self.draw.doc.rig.changers.len() + 1;
                    let mut new_id = 0;
                    let selc = sel.clone();
                    self.rig_edit("New Soft changer", now, |r| {
                        let id = r.add_changer(&format!("Soft {n}"), Kind::Soft { center, radius }, None);
                        for m in r.meshes.iter_mut().filter(|m| selc.contains(&m.layer)) {
                            m.parent = Some(id);
                        }
                        new_id = id;
                    });
                    self.rigs.sel_changer = Some(new_id);
                }
            }
        });
        ui.horizontal(|ui| {
            ui.label(RichText::new("Warp grid").size(12.0).color(TEXT_DIM));
            ui.add(egui::DragValue::new(&mut self.rigs.cols_rows.0).range(1..=rig::MAX_WARP).suffix(" across"));
            ui.add(egui::DragValue::new(&mut self.rigs.cols_rows.1).range(1..=rig::MAX_WARP).suffix(" down"));
        });
        let changers: Vec<(Id, String, bool, Option<Id>)> = self.draw.doc.rig.changers.iter().map(|c| (c.id, c.name.clone(), matches!(c.kind, Kind::Rotation { .. }), c.parent)).collect();
        let mut delete: Option<Id> = None;
        let mut set_parent: Option<(Id, Option<Id>)> = None;
        for (id, name, is_rot, parent) in &changers {
            let on = self.rigs.sel_changer == Some(*id);
            ui.horizontal(|ui| {
                if widgets::chip(ui, &format!("{name} ({})", kind_word(&self.draw.doc.rig, *id, *is_rot)), on).clicked() {
                    self.rigs.sel_changer = Some(*id);
                    self.rigs.warp_edit = None;
                }
                if on {
                    egui::ComboBox::from_id_salt("changer_parent").selected_text(match parent {
                        Some(p) => format!("inside {}", changers.iter().find(|c| c.0 == *p).map(|c| c.1.as_str()).unwrap_or("?")),
                        None => "on its own".into(),
                    }).show_ui(ui, |ui| {
                        if ui.selectable_label(parent.is_none(), "on its own").clicked() {
                            set_parent = Some((*id, None));
                        }
                        for (oid, oname, ..) in changers.iter().filter(|c| c.0 != *id) {
                            if ui.selectable_label(*parent == Some(*oid), format!("inside {oname}")).clicked() {
                                set_parent = Some((*id, Some(*oid)));
                            }
                        }
                    });
                    if ui.small_button("Delete").clicked() {
                        delete = Some(*id);
                    }
                }
            });
        }
        if let Some((id, p)) = set_parent {
            // a changer can't go inside itself or inside one of its own children
            let rig = self.draw.doc.rig.clone();
            let mut cur = p;
            let mut loops = false;
            while let Some(c) = cur {
                if c == id {
                    loops = true;
                    break;
                }
                cur = rig.changer(c).and_then(|c| c.parent);
            }
            if loops {
                self.rigs.message = Some("A changer can't go inside one of its own children.".into());
            } else {
                self.rig_edit("Changer inside another", now, |r| {
                    if let Some(c) = r.changers.iter_mut().find(|c| c.id == id) {
                        c.parent = p;
                    }
                });
            }
        }
        if let Some(id) = delete {
            self.rig_edit("Delete changer", now, |r| {
                r.changers.retain(|c| c.id != id);
                r.bindings.retain(|b| b.changer != id);
                for c in r.changers.iter_mut().filter(|c| c.parent == Some(id)) {
                    c.parent = None;
                }
                for m in r.meshes.iter_mut().filter(|m| m.parent == Some(id)) {
                    m.parent = None;
                }
            });
            self.rigs.sel_changer = None;
        }
        // move parts into the selected changer
        if let Some(cid) = self.rigs.sel_changer {
            ui.horizontal_wrapped(|ui| {
                ui.label(RichText::new("Copy to the other side, line at x").size(12.0).color(TEXT_DIM));
                ui.add(egui::DragValue::new(&mut self.rigs.mirror_axis).speed(2.0));
                if ui.button("Copy").on_hover_text("A copy on the other side that turns and bends the other way, moved by the same sliders").clicked() {
                    let axis = self.rigs.mirror_axis;
                    let name = self.draw.doc.rig.changer(cid).map(|c| format!("{} (other side)", c.name)).unwrap_or_default();
                    let mut new_id = None;
                    self.rig_edit("Copy a changer to the other side", now, |r| new_id = r.mirror_changer(cid, axis, &name));
                    if new_id.is_some() {
                        self.rigs.sel_changer = new_id;
                    }
                }
            });
            if ui.add_enabled(!sel.is_empty(), egui::Button::new("Put the picked parts into this changer")).clicked() {
                let selc = sel.clone();
                self.rig_edit("Parts into changer", now, |r| {
                    for m in r.meshes.iter_mut().filter(|m| selc.contains(&m.layer)) {
                        m.parent = Some(cid);
                    }
                });
            }
            if matches!(self.draw.doc.rig.changer(cid).map(|c| &c.kind), Some(Kind::Rotation { .. })) {
                let on = self.rigs.mode == Mode::Pivot;
                if widgets::chip(ui, if on { "Click the picture for the pivot…" } else { "Move the pivot" }, on).clicked() {
                    self.rigs.mode = if on { Mode::Look } else { Mode::Pivot };
                }
            }
        }

        // ---- glue ----
        section_title(ui, "Glue");
        ui.label(RichText::new("Keeps the seam of two parts closed when they move differently.").size(11.5).color(TEXT_DIM));
        ui.horizontal(|ui| {
            ui.label(RichText::new("Reach").size(12.0).color(TEXT_DIM));
            ui.add(egui::DragValue::new(&mut self.rigs.glue_reach).speed(1.0).range(2.0..=400.0).suffix(" px"));
            let two = sel.len() == 2 && sel.iter().all(|l| self.draw.doc.rig.mesh_of(*l).is_some());
            if ui.add_enabled(two, egui::Button::new("Glue the two picked parts")).on_disabled_hover_text("Pick two parts that have a mesh").clicked() {
                let ids: Vec<Id> = sel.iter().filter_map(|l| self.draw.doc.rig.mesh_of(*l).map(|m| m.id)).collect();
                let reach = self.rigs.glue_reach;
                let mut n = 0;
                self.rig_edit("Glue parts", now, |r| n = r.make_glue(ids[0], ids[1], reach, 1.0));
                self.rigs.message = Some(if n == 0 { "The two parts don't touch within that reach.".into() } else { format!("Glued along {n} points.") });
            }
        });
        let glues: Vec<(usize, String, f32)> = self.draw.doc.rig.glues.iter().enumerate().map(|(i, g)| {
            let name = |id: Id| self.draw.doc.rig.meshes.iter().find(|m| m.id == id).and_then(|m| self.draw.doc.layers.iter().find(|l| l.id == m.layer)).map(|l| l.name.clone()).unwrap_or_else(|| "?".into());
            (i, format!("{} and {}", name(g.a), name(g.b)), g.weight)
        }).collect();
        let mut del_glue: Option<usize> = None;
        let mut new_weight: Option<(usize, f32)> = None;
        for (i, label, w) in &glues {
            ui.horizontal(|ui| {
                ui.label(RichText::new(label).size(12.0));
                let mut v = *w;
                if ui.add(egui::Slider::new(&mut v, 0.0..=1.0).show_value(false)).drag_stopped() {
                    new_weight = Some((*i, v));
                }
                if ui.small_button("Delete").clicked() {
                    del_glue = Some(*i);
                }
            });
        }
        if let Some((i, v)) = new_weight {
            self.rig_edit("Glue strength", now, |r| {
                if let Some(g) = r.glues.get_mut(i) {
                    g.weight = v;
                }
            });
        }
        if let Some(i) = del_glue {
            self.rig_edit("Remove glue", now, |r| {
                if i < r.glues.len() {
                    r.glues.remove(i);
                }
            });
        }

    }

    fn rig_sliders_section(&mut self, ui: &mut Ui, now: f64) {
        // ---- sliders ----
        section_title(ui, "Sliders");
        let params: Vec<(Id, String, f32, f32, f32)> = self.draw.doc.rig.params.iter().map(|p| (p.id, p.name.clone(), p.min, p.max, p.default)).collect();
        let mut del_param: Option<Id> = None;
        for (id, name, lo, hi, def) in &params {
            let mut v = self.draw.doc.rig.value(&self.rigs.values, *id);
            let on = self.rigs.sel_param == Some(*id);
            ui.horizontal(|ui| {
                if widgets::chip(ui, name, on).clicked() {
                    self.rigs.sel_param = Some(*id);
                }
                if ui.add(egui::Slider::new(&mut v, *lo..=*hi).max_decimals(1)).changed() {
                    self.rigs.values.set(*id, v);
                }
                if ui.small_button("0").on_hover_text("Back to the middle").clicked() {
                    self.rigs.values.set(*id, *def);
                }
                if on && ui.small_button("Delete").clicked() {
                    del_param = Some(*id);
                }
            });
        }
        if let Some(id) = del_param {
            self.rig_edit("Delete slider", now, |r| {
                r.params.retain(|p| p.id != id);
                r.bindings.retain(|b| !b.params.contains(&id));
            });
            self.rigs.values.0.remove(&id);
            self.rigs.sel_param = None;
        }
        ui.horizontal(|ui| {
            ui.add(egui::TextEdit::singleline(&mut self.rigs.new_param.0).desired_width(110.0).char_limit(40));
            ui.add(egui::DragValue::new(&mut self.rigs.new_param.1).speed(0.5).prefix("from "));
            ui.add(egui::DragValue::new(&mut self.rigs.new_param.2).speed(0.5).prefix("to "));
            if ui.button("Add slider").clicked() && self.rigs.new_param.1 < self.rigs.new_param.2 {
                let (name, lo, hi) = self.rigs.new_param.clone();
                let mid = if lo < 0.0 && hi > 0.0 { 0.0 } else { lo };
                let mut new_id = 0;
                self.rig_edit("New slider", now, |r| new_id = r.add_param(&name, lo, hi, mid));
                self.rigs.sel_param = Some(new_id);
            }
        });

        // ---- keyforms of the picked changer and slider ----
        if !self.rigs.pro {
            ui.label(RichText::new("Pro mode shows the meshes, changers and keyforms behind these sliders.").size(11.5).color(TEXT_DIM));
        } else if let (Some(cid), Some(pid)) = (self.rigs.sel_changer, self.rigs.sel_param) {
            self.rig_keyforms(ui, now, cid, pid);
        } else {
            ui.label(RichText::new("Pick a changer and a slider to say how the slider moves it.").size(11.5).color(TEXT_DIM));
        }
    }

    /// The skin of the document, to change (a step is added to the history by the caller).
    fn skin_mut(&mut self) -> Option<&mut rig::skin::Skin> {
        Arc::make_mut(&mut self.draw.doc.rig).skin.as_mut()
    }

    /// Manual weights: the weight brush, lasso, mirror, normalize and weights from tags.
    fn rig_weight_tools(&mut self, ui: &mut Ui, now: f64, picked: Option<Id>, bones: &[(Id, String)]) {
        section_title(ui, "Paint the weights yourself");
        let on = self.rigs.mode == Mode::Weight;
        ui.horizontal_wrapped(|ui| {
            if widgets::chip(ui, if on { "Painting: pick a tool and drag on the picture" } else { "Paint weights" }, on).on_hover_text("Shows the picture at rest, so you paint where things really are").clicked() {
                self.rigs.mode = if on { Mode::Look } else { Mode::Weight };
                if !on {
                    self.rigs.heat = true;
                }
            }
        });
        if picked.is_none() {
            ui.label(RichText::new("Pick a bone above first.").size(11.5).color(TEXT_DIM));
        }
        use rig::skin::Brush;
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing = vec2(4.0, 4.0);
            for (t, name) in [
                (WTool::Brush(Brush::Add), "Add"),
                (WTool::Brush(Brush::Subtract), "Subtract"),
                (WTool::Brush(Brush::Smooth), "Smooth"),
                (WTool::Brush(Brush::Flood), "Flood"),
                (WTool::Lasso, "Lasso: give"),
                (WTool::LassoRemove, "Lasso: take away"),
            ] {
                if widgets::chip(ui, name, self.rigs.wtool == t).clicked() {
                    self.rigs.wtool = t;
                    self.rigs.mode = Mode::Weight;
                    self.rigs.heat = true;
                }
            }
        });
        ui.horizontal(|ui| {
            ui.label(RichText::new("Size").size(12.0).color(TEXT_DIM));
            ui.add(egui::Slider::new(&mut self.rigs.wopts.radius, 10.0..=600.0).suffix(" px").logarithmic(true));
        });
        ui.horizontal(|ui| {
            ui.label(RichText::new(if matches!(self.rigs.wtool, WTool::Brush(Brush::Flood)) { "Weight" } else { "Strength" }).size(12.0).color(TEXT_DIM));
            ui.add(egui::Slider::new(&mut self.rigs.wopts.strength, 0.01..=1.0));
        });
        ui.horizontal(|ui| {
            ui.label(RichText::new("Hardness").size(12.0).color(TEXT_DIM));
            ui.add(egui::Slider::new(&mut self.rigs.wopts.hardness, 0.0..=1.0));
            ui.label(RichText::new("Lasso edge").size(12.0).color(TEXT_DIM));
            ui.add(egui::DragValue::new(&mut self.rigs.feather).speed(1.0).range(0.0..=400.0).suffix(" px"));
        });
        ui.checkbox(&mut self.rigs.wopts.keep_normalized, "Keep each point's total at 100%").on_hover_text("Weight given to this bone is taken from the other bones");
        let b_idx = picked.and_then(|c| self.draw.doc.rig.skin.as_ref().and_then(|s| s.bone_index(c)));
        ui.horizontal_wrapped(|ui| {
            if ui.add_enabled(b_idx.is_some(), egui::Button::new("Flood the whole bone")).on_hover_text("Sets the bone's weight everywhere to the strength above").clicked() {
                let v = self.rigs.wopts.strength;
                let bi = b_idx.unwrap_or(0);
                self.rig_edit("Flood weights", now, |r| {
                    if let Some(s) = r.skin.as_mut() {
                        s.flood_all(bi, v);
                    }
                });
                self.rigs.weight_gen += 1;
            }
            if ui.button("Clear this bone").clicked() {
                if let Some(bi) = b_idx {
                    self.rig_edit("Clear weights", now, |r| {
                        if let Some(s) = r.skin.as_mut() {
                            s.flood_all(bi, 0.0);
                        }
                    });
                    self.rigs.weight_gen += 1;
                }
            }
            if ui.button("Limit to 100%").on_hover_text("Where the bones add up to more than everything, scale them down together").clicked() {
                self.rig_edit("Limit weights", now, |r| {
                    if let Some(s) = r.skin.as_mut() {
                        s.limit_all();
                    }
                });
                self.rigs.weight_gen += 1;
            }
            if ui.button("Add up to exactly 100%").on_hover_text("Where any bone has weight, nothing is left to stay behind").clicked() {
                self.rig_edit("Normalize weights", now, |r| {
                    if let Some(s) = r.skin.as_mut() {
                        s.fill_to_one();
                    }
                });
                self.rigs.weight_gen += 1;
            }
        });
        // mirror left to right
        ui.horizontal_wrapped(|ui| {
            ui.label(RichText::new("Mirror onto").size(12.0).color(TEXT_DIM));
            let target = self.rigs.mirror_to.filter(|t| bones.iter().any(|b| b.0 == *t) && Some(*t) != picked);
            egui::ComboBox::from_id_salt("mirror_to").selected_text(target.and_then(|t| bones.iter().find(|b| b.0 == t)).map(|b| b.1.clone()).unwrap_or_else(|| "pick a bone".into())).show_ui(ui, |ui| {
                for (id, name) in bones.iter().filter(|b| Some(b.0) != picked) {
                    if ui.selectable_label(target == Some(*id), name).clicked() {
                        self.rigs.mirror_to = Some(*id);
                    }
                }
            });
            ui.label(RichText::new("line at x").size(12.0).color(TEXT_DIM));
            ui.add(egui::DragValue::new(&mut self.rigs.mirror_axis).speed(2.0));
            if ui.add_enabled(b_idx.is_some() && target.is_some(), egui::Button::new("Mirror")).on_hover_text("Copies this bone's weights, flipped left to right, onto the other bone").clicked() {
                let (axis, to) = (self.rigs.mirror_axis, target.and_then(|t| self.draw.doc.rig.skin.as_ref().and_then(|s| s.bone_index(t))));
                if let (Some(from), Some(to)) = (b_idx, to) {
                    self.rig_edit("Mirror weights", now, |r| {
                        if let Some(s) = r.skin.as_mut() {
                            s.mirror_plane(from, to, axis);
                        }
                    });
                    self.rigs.weight_gen += 1;
                }
            }
        });
        // from tags
        ui.horizontal_wrapped(|ui| {
            ui.label(RichText::new("From a tag").size(12.0).color(TEXT_DIM));
            let tags: Vec<parts::Part> = parts::Part::ALL.iter().copied().filter(|p| self.draw.doc.layers.iter().any(|l| l.part == Some(*p))).collect();
            let cur = self.rigs.tag_pick.filter(|t| tags.contains(t)).or_else(|| tags.first().copied());
            egui::ComboBox::from_id_salt("tag_pick").selected_text(cur.map(|t| t.label()).unwrap_or("no tags yet")).show_ui(ui, |ui| {
                for t in &tags {
                    if ui.selectable_label(cur == Some(*t), t.label()).clicked() {
                        self.rigs.tag_pick = Some(*t);
                    }
                }
            });
            if ui.add_enabled(b_idx.is_some() && cur.is_some(), egui::Button::new("Give the bone the parts with this tag")).on_hover_text("Weight where the layers with this tag have paint, softened at the edge. Tags are set in the Parts tab.").clicked() {
                if let (Some(bi), Some(tag)) = (b_idx, cur) {
                    let mask = self.tag_mask(tag);
                    self.rig_edit("Weights from a tag", now, |r| {
                        if let Some(s) = r.skin.as_mut() {
                            s.assign_mask(bi, &mask, 2);
                        }
                    });
                    self.rigs.weight_gen += 1;
                }
            }
        });
    }

    /// Where the layers with a tag have paint, as a picture the size of the weight grid.
    fn tag_mask(&self, tag: parts::Part) -> Vec<u8> {
        let Some(sk) = self.draw.doc.rig.skin.as_ref() else { return Vec::new() };
        let mut mask = vec![0u8; (sk.gw * sk.gh) as usize];
        for l in self.draw.doc.layers.iter().filter(|l| l.part == Some(tag) && l.visible) {
            for gy in 0..sk.gh {
                for gx in 0..sk.gw {
                    let (x, y) = (((gx as f32 + 0.5) * sk.cell as f32) as i32, ((gy as f32 + 0.5) * sk.cell as f32) as i32);
                    if crate::prerig::px_get(l, x, y).a() > 20 {
                        mask[(gy * sk.gw + gx) as usize] = 255;
                    }
                }
            }
        }
        mask
    }

    fn rig_keyforms(&mut self, ui: &mut Ui, now: f64, cid: Id, pid: Id) {
        let rig = self.draw.doc.rig.clone();
        let (Some(c), Some(p)) = (rig.changer(cid).cloned(), rig.param(pid).cloned()) else { return };
        let second = self.rigs.sel_param2.filter(|q| *q != pid).and_then(|q| rig.param(q).cloned());
        section_title(ui, &format!("{} and {}{}", c.name, p.name, second.as_ref().map(|q| format!(" and {}", q.name)).unwrap_or_default()));
        // a second slider makes the shape depend on both (a grid, like head left-right by up-down)
        ui.horizontal(|ui| {
            ui.label(RichText::new("With a second slider").size(12.0).color(TEXT_DIM));
            egui::ComboBox::from_id_salt("second_param").selected_text(second.as_ref().map(|q| q.name.clone()).unwrap_or_else(|| "none".into())).show_ui(ui, |ui| {
                if ui.selectable_label(second.is_none(), "none").clicked() {
                    self.rigs.sel_param2 = None;
                }
                for q in rig.params.iter().filter(|q| q.id != pid) {
                    if ui.selectable_label(second.as_ref().map(|s| s.id) == Some(q.id), &q.name).clicked() {
                        self.rigs.sel_param2 = Some(q.id);
                    }
                }
            });
        });
        let value = rig.value(&self.rigs.values, pid);
        let value2 = second.as_ref().map(|q| rig.value(&self.rigs.values, q.id));
        let pid2 = second.as_ref().map(|q| q.id);
        // saving a shape at the current slider value(s)
        let save = |this: &mut Self, label: &str, form: Form| {
            this.rig_edit(label, now, |r| match (pid2, value2) {
                (Some(q), Some(v2)) => r.set_keyform2(cid, pid, q, value, v2, form),
                _ => r.set_keyform(cid, pid, value, form),
            });
        };
        match c.kind.clone() {
            Kind::Rotation { .. } => {
                ui.horizontal(|ui| {
                    ui.label(RichText::new("Turns by").size(12.0).color(TEXT_DIM));
                    ui.add(egui::DragValue::new(&mut self.rigs.turn_deg).speed(0.5).suffix(" deg"));
                    ui.label(RichText::new("at the slider's end").size(12.0).color(TEXT_DIM));
                });
                if second.is_none() && ui.button("Bind: turn with the slider").on_hover_text("At the low end it turns the other way, in the middle it doesn't turn").clicked() {
                    let deg = self.rigs.turn_deg;
                    self.rig_edit("Bind rotation to slider", now, |r| {
                        r.bindings.retain(|b| !(b.changer == cid && b.params == [pid]));
                        r.bind_rotation(cid, pid, deg);
                    });
                }
                if ui.button(if second.is_some() { "Set the turn at these two values" } else { "Set the turn at this value" }).on_hover_text("Adds one keyform at the slider value(s) now").clicked() {
                    let deg = self.rigs.turn_deg;
                    save(self, "Rotation keyform", Form::Rot { deg });
                }
            }
            Kind::Warp { .. } | Kind::Line { .. } | Kind::Soft { .. } => {
                let editing = self.rigs.warp_edit.as_ref().is_some_and(|w| w.0 == cid);
                let is_soft = matches!(c.kind, Kind::Soft { .. });
                ui.label(RichText::new(if is_soft { "Drag the dot on the picture, set how much it grows, then save the shape." } else { "Drag the points on the picture, then save the shape at this slider value." }).size(11.5).color(TEXT_DIM));
                if editing && is_soft {
                    ui.horizontal(|ui| {
                        ui.label(RichText::new("Grows by").size(12.0).color(TEXT_DIM));
                        ui.add(egui::DragValue::new(&mut self.rigs.soft_grow).speed(0.01).range(-0.9..=3.0).fixed_decimals(2));
                    });
                }
                ui.horizontal(|ui| {
                    if !editing {
                        if ui.button("Edit the shape").clicked() {
                            // start from what is on screen now
                            let pose = rig.pose(&self.rigs.values);
                            let off: Vec<[f32; 2]> = match &c.kind {
                                Kind::Warp { cols, rows, .. } => pose.warp.get(&cid).cloned().unwrap_or_else(|| vec![[0.0, 0.0]; ((cols + 1) * (rows + 1)) as usize]),
                                Kind::Line { pts, .. } => pose.line.get(&cid).cloned().unwrap_or_else(|| vec![[0.0, 0.0]; pts.len()]),
                                _ => {
                                    let (o, g) = pose.soft.get(&cid).copied().unwrap_or(([0.0, 0.0], 0.0));
                                    self.rigs.soft_grow = g;
                                    vec![o]
                                }
                            };
                            self.rigs.warp_edit = Some((cid, off));
                        }
                    } else {
                        if ui.button(RichText::new("Save the shape at this value").strong()).clicked() {
                            if let Some((_, off)) = self.rigs.warp_edit.take() {
                                let form = match &c.kind {
                                    Kind::Warp { .. } => Form::Warp { offsets: off },
                                    Kind::Line { .. } => Form::Line { offsets: off },
                                    _ => Form::Soft { offset: off.first().copied().unwrap_or([0.0, 0.0]), grow: self.rigs.soft_grow },
                                };
                                save(self, "Keyform", form);
                            }
                        }
                        if ui.button("Cancel").clicked() {
                            self.rigs.warp_edit = None;
                        }
                    }
                });
            }
        }
        let keys_for: Vec<Id> = match pid2 {
            Some(q) => vec![pid, q],
            None => vec![pid],
        };
        if let Some(b) = rig.bindings.iter().find(|b| b.changer == cid && b.params == keys_for) {
            let label: Vec<String> = b.keys.iter().map(|k| k.iter().map(|v| format!("{v:.0}")).collect::<Vec<_>>().join(", ")).collect();
            ui.label(RichText::new(format!("Keyforms at: {}", label.join("  by  "))).size(11.5).color(TEXT_DIM));
            ui.horizontal_wrapped(|ui| {
                ui.label(RichText::new("Between keys").size(12.0).color(TEXT_DIM));
                for e in rig::Ease::ALL {
                    if widgets::chip(ui, e.label(), b.ease == e).clicked() {
                        let ks = keys_for.clone();
                        self.rig_edit("Easing", now, |r| {
                            if let Some(x) = r.bindings.iter_mut().find(|x| x.changer == cid && x.params == ks) {
                                x.ease = e;
                            }
                        });
                    }
                }
            });
            if ui.small_button("Remove these keyforms").clicked() {
                let ks = keys_for.clone();
                self.rig_edit("Remove keyforms", now, |r| r.bindings.retain(|x| !(x.changer == cid && x.params == ks)));
            }
        }
    }

    fn rig_view(&mut self, ui: &mut Ui, now: f64, picked: &mut Option<(LayerId, bool, bool)>) {
        let rect = ui.available_rect_before_wrap();
        let resp = ui.allocate_rect(rect, Sense::click_and_drag());
        let (dw, dh) = (self.draw.doc.w as f32, self.draw.doc.h as f32);
        let fit = ((rect.width() - 24.0) / dw).min((rect.height() - 24.0) / dh).max(0.01);
        let sc = fit * self.rigs.zoom.max(0.1);
        let page = Rect::from_center_size(rect.center() + self.rigs.pan, vec2(dw * sc, dh * sc));
        if resp.hovered() {
            let (scroll, pos) = ui.input(|i| (i.smooth_scroll_delta.y, i.pointer.hover_pos()));
            if let (true, Some(p)) = (scroll != 0.0, pos) {
                let k = (1.0 + scroll * 0.002).clamp(0.8, 1.25);
                let nz = (self.rigs.zoom * k).clamp(0.3, 12.0);
                let k = nz / self.rigs.zoom;
                self.rigs.pan = (p - rect.center()) - (p - rect.center() - self.rigs.pan) * k;
                self.rigs.zoom = nz;
            }
        }
        if resp.dragged_by(egui::PointerButton::Secondary) || resp.dragged_by(egui::PointerButton::Middle) {
            self.rigs.pan += resp.drag_delta();
        }
        let p = ui.painter_at(rect);
        p.rect_filled(page.expand(1.0), 0.0, Color32::from_gray(40));
        if let Some(t) = &self.rigs.image {
            p.image(t.id(), page, Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)), Color32::WHITE);
        }
        let to_screen = |v: [f32; 2]| pos2(page.min.x + v[0] * sc, page.min.y + v[1] * sc);
        self.heat_overlay(ui.ctx(), &p, page);
        let to_page = |s: Pos2| [(s.x - page.min.x) / sc, (s.y - page.min.y) / sc];
        let rig = self.draw.doc.rig.clone();
        let mut pose = if self.rigs.mode == Mode::Weight { rig.pose(&Values::default()) } else { rig.pose(&self.rigs.values) };
        self.rigs.put_edit(&rig, &mut pose);
        let all = rig.deformed_all(&pose);
        // triangles of the picked parts
        if self.rigs.wire {
            for (mi, m) in rig.meshes.iter().enumerate() {
                if !self.rigs.selected.contains(&m.layer) {
                    continue;
                }
                let now_v = &all[mi];
                let stroke = Stroke::new(1.0, Color32::from_rgba_unmultiplied(40, 200, 120, 170));
                for t in &m.tris {
                    let (a, b, c) = (to_screen(now_v[t[0] as usize]), to_screen(now_v[t[1] as usize]), to_screen(now_v[t[2] as usize]));
                    p.line_segment([a, b], stroke);
                    p.line_segment([b, c], stroke);
                    p.line_segment([c, a], stroke);
                }
            }
        }
        // the picked changer: a pivot or a grid with handles
        let mut drag_handle: Option<(usize, Vec2)> = None;
        if let Some(c) = self.rigs.sel_changer.and_then(|id| rig.changer(id)) {
            match &c.kind {
                Kind::Rotation { pivot } => {
                    let at = to_screen(rig.deform_point(&pose, c.parent, *pivot));
                    p.circle_stroke(at, 9.0, Stroke::new(2.0, Color32::from_rgb(255, 200, 60)));
                    p.circle_filled(at, 3.0, Color32::from_rgb(255, 200, 60));
                }
                Kind::Warp { rect: r, cols, rows } => {
                    let rest = Rig::warp_rest(*r, *cols, *rows);
                    let off = pose.warp.get(&c.id).cloned().unwrap_or_else(|| vec![[0.0, 0.0]; rest.len()]);
                    let editing = self.rigs.warp_edit.as_ref().is_some_and(|w| w.0 == c.id);
                    let pos_of = |i: usize| to_screen(rig.deform_point(&pose, c.parent, [rest[i][0] + off[i][0], rest[i][1] + off[i][1]]));
                    for row in 0..=*rows as usize {
                        for col in 0..=*cols as usize {
                            let i = row * (*cols as usize + 1) + col;
                            let a = pos_of(i);
                            if col < *cols as usize {
                                p.line_segment([a, pos_of(i + 1)], Stroke::new(1.0, Color32::from_rgb(255, 200, 60)));
                            }
                            if row < *rows as usize {
                                p.line_segment([a, pos_of(i + *cols as usize + 1)], Stroke::new(1.0, Color32::from_rgb(255, 200, 60)));
                            }
                        }
                    }
                    for i in 0..rest.len() {
                        let a = pos_of(i);
                        let hr = Rect::from_center_size(a, vec2(12.0, 12.0));
                        p.rect_filled(hr, 3.0, if editing { Color32::from_rgb(255, 200, 60) } else { Color32::from_rgb(120, 110, 60) });
                        if editing {
                            let r = ui.interact(hr, egui::Id::new(("warp_handle", c.id, i)), Sense::drag());
                            if r.dragged() {
                                drag_handle = Some((i, r.drag_delta() / sc));
                            }
                        }
                    }
                }
                Kind::Line { pts, width } => {
                    let off = pose.line.get(&c.id).cloned().unwrap_or_else(|| vec![[0.0, 0.0]; pts.len()]);
                    let editing = self.rigs.warp_edit.as_ref().is_some_and(|w| w.0 == c.id);
                    let moved: Vec<[f32; 2]> = pts.iter().zip(&off).map(|(a, o)| [a[0] + o[0], a[1] + o[1]]).collect();
                    let curve: Vec<Pos2> = rig::curve_samples(&moved, 8).iter().map(|v| to_screen(rig.deform_point(&pose, c.parent, *v))).collect();
                    for w in curve.windows(2) {
                        p.line_segment([w[0], w[1]], Stroke::new(2.0, Color32::from_rgb(255, 200, 60)));
                    }
                    // how far from the curve the picture follows it
                    if let Some(first) = curve.first() {
                        p.circle_stroke(*first, width * sc, Stroke::new(1.0, Color32::from_rgba_unmultiplied(255, 200, 60, 80)));
                    }
                    for (i, v) in moved.iter().enumerate() {
                        let a = to_screen(rig.deform_point(&pose, c.parent, *v));
                        let hr = Rect::from_center_size(a, vec2(12.0, 12.0));
                        p.rect_filled(hr, 6.0, if editing { Color32::from_rgb(255, 200, 60) } else { Color32::from_rgb(120, 110, 60) });
                        if editing {
                            let r = ui.interact(hr, egui::Id::new(("line_handle", c.id, i)), Sense::drag());
                            if r.dragged() {
                                drag_handle = Some((i, r.drag_delta() / sc));
                            }
                        }
                    }
                }
                Kind::Soft { center, radius } => {
                    let (o, _) = pose.soft.get(&c.id).copied().unwrap_or(([0.0, 0.0], 0.0));
                    let editing = self.rigs.warp_edit.as_ref().is_some_and(|w| w.0 == c.id);
                    let at = to_screen(rig.deform_point(&pose, c.parent, [center[0] + o[0], center[1] + o[1]]));
                    p.circle_stroke(to_screen(rig.deform_point(&pose, c.parent, *center)), radius * sc, Stroke::new(1.0, Color32::from_rgba_unmultiplied(255, 200, 60, 120)));
                    let hr = Rect::from_center_size(at, vec2(14.0, 14.0));
                    p.circle_filled(at, 7.0, if editing { Color32::from_rgb(255, 200, 60) } else { Color32::from_rgb(120, 110, 60) });
                    if editing {
                        let r = ui.interact(hr, egui::Id::new(("soft_handle", c.id)), Sense::drag());
                        if r.dragged() {
                            drag_handle = Some((0, r.drag_delta() / sc));
                        }
                    }
                }
            }
        }
        if let (Some((i, d)), Some((_, off))) = (drag_handle, self.rigs.warp_edit.as_mut()) {
            if let Some(o) = off.get_mut(i) {
                o[0] += d.x;
                o[1] += d.y;
            }
        }
        // the turn rings
        if self.rigs.gizmo && self.rigs.mode == Mode::Look {
            self.gizmo_input(ui, &resp, &p, &to_screen, sc);
        }
        // the bones of the skin
        if let Some(sk) = &rig.skin {
            for b in &sk.bones {
                let Some(c) = rig.changer(b.changer) else { continue };
                let Kind::Rotation { pivot } = &c.kind else { continue };
                let (head, tail) = (to_screen(rig.deform_point(&pose, c.parent, *pivot)), to_screen(rig.deform_point(&pose, Some(c.id), b.tail)));
                let on = self.rigs.sel_changer == Some(c.id);
                let col = if on { Color32::from_rgb(255, 200, 60) } else { Color32::from_rgb(90, 200, 255) };
                p.line_segment([head, tail], Stroke::new(if on { 4.0 } else { 3.0 }, col));
                p.circle_filled(head, 5.0, col);
                p.circle_stroke(tail, 4.0, Stroke::new(2.0, col));
            }
        }
        // paint the weights of the picked bone
        if self.rigs.mode == Mode::Weight {
            self.weight_input(ui, &resp, &p, &to_page, &to_screen, sc, now);
        }
        // add a bone: drag from its head to its tail
        if self.rigs.mode == Mode::Bone {
            if resp.drag_started_by(egui::PointerButton::Primary) {
                if let Some(pos) = ui.input(|i| i.pointer.press_origin()) {
                    self.rigs.bone_from = Some(to_page(pos));
                }
            }
            if let (Some(from), Some(pos)) = (self.rigs.bone_from, ui.input(|i| i.pointer.latest_pos())) {
                p.line_segment([to_screen(from), pos], Stroke::new(3.0, Color32::from_rgb(255, 200, 60)));
                if resp.drag_stopped() {
                    let tail = to_page(pos);
                    if (tail[0] - from[0]).hypot(tail[1] - from[1]) > 10.0 {
                        self.add_bone(from, tail, now);
                    }
                    self.rigs.bone_from = None;
                    self.rigs.mode = Mode::Look;
                }
            }
        }
        // click: place the pivot, or pick the part under the pointer
        if resp.clicked_by(egui::PointerButton::Primary) {
            if let Some(pos) = resp.interact_pointer_pos().filter(|q| page.contains(*q)) {
                let pg = to_page(pos);
                if self.rigs.mode == Mode::Pivot {
                    if let Some(cid) = self.rigs.sel_changer {
                        // the point on the rest picture that is under the pointer (undo the changer's own pose)
                        self.rig_edit("Move the pivot", now, |r| {
                            if let Some(c) = r.changers.iter_mut().find(|c| c.id == cid) {
                                c.kind = Kind::Rotation { pivot: pg };
                            }
                        });
                    }
                    self.rigs.mode = Mode::Look;
                } else if let Some(id) = self.rig_pick(pg) {
                    let m = ui.input(|i| i.modifiers);
                    *picked = Some((id, m.command || m.ctrl, m.shift));
                }
            }
        }
        egui::Area::new(egui::Id::new("rig_bar")).fixed_pos(rect.min + vec2(10.0, 8.0)).order(egui::Order::Foreground).show(ui.ctx(), |ui| {
            ui.horizontal(|ui| {
                if ui.small_button("Fit").clicked() {
                    self.rigs.zoom = 1.0;
                    self.rigs.pan = Vec2::ZERO;
                }
                if ui.small_button("All sliders to the middle").clicked() {
                    self.rigs.values = Values::default();
                }
                ui.label(RichText::new("Wheel: zoom. Right drag: move. Blend modes are not shown here.").size(11.0).color(TEXT_DIM));
            });
        });
    }

    /// The three rings of the rotation gizmo: left-right, up-down and tilt. Drag a ring to move its slider.
    fn gizmo_input(&mut self, ui: &mut Ui, resp: &egui::Response, p: &egui::Painter, to_screen: &dyn Fn([f32; 2]) -> Pos2, sc: f32) {
        use rig::gizmo::Axis;
        let rig = self.draw.doc.rig.clone();
        let targets = rig::gizmo::targets(&rig);
        let Some(t) = targets.get(self.rigs.gtarget.min(targets.len().saturating_sub(1))) else { return };
        let centre = to_screen(t.pivot);
        let r = 70.0f32.max(60.0 * sc * 4.0).min(140.0);
        self.rigs.gizmo_geom = Some((centre, r));
        // the rings as outlines (an ellipse for the two turns, a circle for the tilt)
        let ring = |axis: Axis| -> Vec<Pos2> {
            (0..=48)
                .map(|i| {
                    let a = i as f32 / 48.0 * std::f32::consts::TAU;
                    match axis {
                        Axis::Z => centre + vec2(a.cos() * r, a.sin() * r),
                        Axis::X => centre + vec2(a.cos() * r * 0.3, a.sin() * r),
                        Axis::Y => centre + vec2(a.cos() * r, a.sin() * r * 0.3),
                    }
                })
                .collect()
        };
        let colour = |a: Axis| match a {
            Axis::X => Color32::from_rgb(235, 90, 90),
            Axis::Y => Color32::from_rgb(110, 220, 110),
            Axis::Z => Color32::from_rgb(90, 150, 255),
        };
        for axis in [Axis::Z, Axis::X, Axis::Y] {
            if t.param(axis).is_none() {
                continue;
            }
            let on = self.rigs.gdrag.as_ref().is_some_and(|d| d.0 == axis);
            let pts = ring(axis);
            for w in pts.windows(2) {
                p.line_segment([w[0], w[1]], Stroke::new(if on { 4.0 } else { 2.0 }, colour(axis)));
            }
        }
        let dist_to = |axis: Axis, pos: Pos2| ring(axis).iter().map(|q| (q.x - pos.x).hypot(q.y - pos.y)).fold(f32::MAX, f32::min);
        if resp.drag_started_by(egui::PointerButton::Primary) {
            if let Some(pos) = ui.input(|i| i.pointer.press_origin()) {
                let best = [Axis::X, Axis::Y, Axis::Z].into_iter().filter(|a| t.param(*a).is_some()).map(|a| (a, dist_to(a, pos))).filter(|(_, d)| *d < 12.0).min_by(|a, b| a.1.total_cmp(&b.1));
                if let Some((axis, _)) = best {
                    let id = t.param(axis).unwrap_or(0);
                    self.rigs.gdrag = Some((axis, pos, rig.value(&self.rigs.values, id)));
                }
            }
        }
        if let (Some((axis, start, v0)), true) = (self.rigs.gdrag, resp.dragged_by(egui::PointerButton::Primary)) {
            if let (Some(pos), Some(id)) = (ui.input(|i| i.pointer.latest_pos()), t.param(axis)) {
                let half = rig::gizmo::half_range(&rig, id);
                let delta = match axis {
                    Axis::X => rig::gizmo::x_delta(pos.x - start.x, half),
                    Axis::Y => rig::gizmo::y_delta(pos.y - start.y, half),
                    Axis::Z => rig::gizmo::z_delta([start.x, start.y], [pos.x, pos.y], [centre.x, centre.y], half),
                };
                rig::gizmo::set_value(&rig, &mut self.rigs.values, id, v0 + delta, self.rigs.snap);
            }
        }
        if resp.drag_stopped() && self.rigs.gdrag.take().is_some() {
            self.refresh_warnings();
        }
    }

    /// Painting weights with the mouse or pen on the picture.
    #[allow(clippy::too_many_arguments)]
    fn weight_input(&mut self, ui: &mut Ui, resp: &egui::Response, p: &egui::Painter, to_page: &dyn Fn(Pos2) -> [f32; 2], to_screen: &dyn Fn([f32; 2]) -> Pos2, sc: f32, now: f64) {
        let bone = self.rigs.sel_changer.and_then(|c| self.draw.doc.rig.skin.as_ref().and_then(|s| s.bone_index(c)));
        let radius = self.rigs.wopts.radius;
        let hover = resp.hover_pos();
        let brush_tool = matches!(self.rigs.wtool, WTool::Brush(_));
        if let (Some(h), true) = (hover, brush_tool) {
            p.circle_stroke(h, radius * sc, Stroke::new(1.5, Color32::from_rgb(255, 200, 60)));
        }
        let Some(bi) = bone else { return };
        if resp.drag_started_by(egui::PointerButton::Primary) {
            self.rigs.weight_before = Some(self.draw.doc.snapshot());
            self.rigs.last_dab = None;
            self.rigs.lasso_pts.clear();
        }
        if resp.dragged_by(egui::PointerButton::Primary) {
            if let Some(pos) = ui.input(|i| i.pointer.latest_pos()) {
                let pg = to_page(pos);
                match self.rigs.wtool {
                    WTool::Brush(brush) => {
                        // dabs along the way, so a fast stroke has no gaps
                        let step = (radius * 0.25).max(2.0);
                        let from = self.rigs.last_dab.unwrap_or(pg);
                        let dist = (pg[0] - from[0]).hypot(pg[1] - from[1]);
                        let n = (dist / step).floor() as usize;
                        let o = self.rigs.wopts;
                        let mut changed = false;
                        for i in 0..=n {
                            let t = if n == 0 { 1.0 } else { i as f32 / n as f32 };
                            let at = [from[0] + (pg[0] - from[0]) * t, from[1] + (pg[1] - from[1]) * t];
                            if let Some(sk) = self.skin_mut() {
                                changed |= sk.paint(bi, at[0], at[1], brush, &o);
                            }
                        }
                        if changed {
                            self.rigs.weight_gen += 1;
                        }
                        self.rigs.last_dab = Some(pg);
                    }
                    WTool::Lasso | WTool::LassoRemove => {
                        if self.rigs.lasso_pts.last().is_none_or(|l| (l[0] - pg[0]).hypot(l[1] - pg[1]) > 4.0 / sc) {
                            self.rigs.lasso_pts.push(pg);
                        }
                    }
                }
            }
        }
        if matches!(self.rigs.wtool, WTool::Lasso | WTool::LassoRemove) && self.rigs.lasso_pts.len() > 1 {
            let pts: Vec<Pos2> = self.rigs.lasso_pts.iter().map(|q| to_screen(*q)).collect();
            for w in pts.windows(2) {
                p.line_segment([w[0], w[1]], Stroke::new(2.0, Color32::from_rgb(255, 200, 60)));
            }
        }
        if resp.drag_stopped() {
            if let WTool::Lasso | WTool::LassoRemove = self.rigs.wtool {
                let poly = std::mem::take(&mut self.rigs.lasso_pts);
                let (feather, remove) = (self.rigs.feather, self.rigs.wtool == WTool::LassoRemove);
                if poly.len() >= 3 {
                    if let Some(sk) = self.skin_mut() {
                        sk.lasso(bi, &poly, feather, remove);
                    }
                    self.rigs.weight_gen += 1;
                }
            }
            // the whole stroke is one step of the history
            if let Some(before) = self.rigs.weight_before.take() {
                let after = self.draw.doc.snapshot();
                if !self.draw.doc.same_as(&before) {
                    let label = match self.rigs.wtool {
                        WTool::Brush(b) => format!("Paint weights · {b:?}"),
                        WTool::Lasso => "Lasso weights".to_owned(),
                        WTool::LassoRemove => "Lasso weights away".to_owned(),
                    };
                    self.log_step(&label, now);
                    self.history.push(label, Tab::Rig, before, after, now);
                }
            }
            self.rigs.last_dab = None;
        }
    }

    /// The weights of the picked bone as a blue-to-red haze over the picture.
    fn heat_overlay(&mut self, ctx: &egui::Context, p: &egui::Painter, page: Rect) {
        if !self.rigs.heat {
            return;
        }
        let Some(sk) = self.draw.doc.rig.skin.as_ref() else { return };
        let Some(bi) = self.rigs.sel_changer.and_then(|c| sk.bone_index(c)) else { return };
        let Some(plane) = sk.planes.get(bi) else { return };
        let cover = self.rigs.skin_tex.as_ref().and_then(|t| t.1.clone());
        let key = (bi, plane.iter().map(|v| *v as u64).sum::<u64>() ^ ((plane.len() as u64) << 32) ^ self.rigs.skin_tex.as_ref().map(|t| t.0).unwrap_or(0));
        if self.rigs.heat_tex.as_ref().map(|t| t.0) != Some(key) {
            let ramp = |w: f32| -> Color32 {
                if w < 0.02 {
                    return Color32::TRANSPARENT;
                }
                // blue, green, yellow, red
                let (r, g, b) = if w < 0.5 { (0.0, w * 2.0, 1.0 - w * 2.0) } else { ((w - 0.5) * 2.0, 1.0 - (w - 0.5) * 2.0, 0.0) };
                Color32::from_rgba_unmultiplied((r * 255.0) as u8, (g * 255.0) as u8, (b * 255.0) as u8, 150)
            };
            // only where the drawing is
            let px: Vec<Color32> = plane
                .iter()
                .enumerate()
                .map(|(i, v)| {
                    let (gx, gy) = ((i as u32 % sk.gw) as f32, (i as u32 / sk.gw) as f32);
                    let (x, y) = (((gx + 0.5) * sk.cell as f32) as usize, ((gy + 0.5) * sk.cell as f32) as usize);
                    match &cover {
                        Some(t) if t.alpha_at(x, y) < 0.1 => Color32::TRANSPARENT,
                        _ => ramp(*v as f32 / 255.0),
                    }
                })
                .collect();
            let img = egui::ColorImage::new([sk.gw as usize, sk.gh as usize], px);
            let t = ctx.load_texture("rig_heat", img, TextureOptions::LINEAR);
            self.rigs.heat_tex = Some((key, t));
        }
        if let Some((_, t)) = &self.rigs.heat_tex {
            // the grid covers the page in whole cells: draw only the part that is the page
            let (cw, ch) = (sk.gw as f32 * sk.cell as f32, sk.gh as f32 * sk.cell as f32);
            let (dw, dh) = (self.draw.doc.w as f32, self.draw.doc.h as f32);
            let uv = Rect::from_min_max(pos2(0.0, 0.0), pos2(dw / cw, dh / ch));
            p.image(t.id(), page, uv, Color32::WHITE);
        }
    }

    /// The topmost part under a page point (looking at where things are now).
    fn rig_pick(&mut self, pg: [f32; 2]) -> Option<LayerId> {
        let items = self.rig_items();
        for (id, tex, rest, now, tris, _) in items.iter().rev() {
            for t in tris {
                let (a, b, c) = (now[t[0] as usize], now[t[1] as usize], now[t[2] as usize]);
                let d = (b[1] - c[1]) * (a[0] - c[0]) + (c[0] - b[0]) * (a[1] - c[1]);
                if d.abs() < 1e-9 {
                    continue;
                }
                let l0 = ((b[1] - c[1]) * (pg[0] - c[0]) + (c[0] - b[0]) * (pg[1] - c[1])) / d;
                let l1 = ((c[1] - a[1]) * (pg[0] - c[0]) + (a[0] - c[0]) * (pg[1] - c[1])) / d;
                let l2 = 1.0 - l0 - l1;
                if l0 >= 0.0 && l1 >= 0.0 && l2 >= 0.0 {
                    let (ra, rb, rc) = (rest[t[0] as usize], rest[t[1] as usize], rest[t[2] as usize]);
                    let (u, v) = (ra[0] * l0 + rb[0] * l1 + rc[0] * l2, ra[1] * l0 + rb[1] * l1 + rc[1] * l2);
                    let (ix, iy) = (u.floor() as i32 - tex.x0, v.floor() as i32 - tex.y0);
                    if ix >= 0 && iy >= 0 && (ix as usize) < tex.w && (iy as usize) < tex.h && tex.alpha_at(ix as usize, iy as usize) > 0.2 {
                        return Some(*id);
                    }
                }
            }
        }
        None
    }
}

/// "rotation", "warp", "line" or "soft".
fn kind_word(rig: &Rig, id: Id, is_rot: bool) -> &'static str {
    if is_rot {
        return "rotation";
    }
    match rig.changer(id).map(|c| &c.kind) {
        Some(Kind::Warp { .. }) => "warp",
        Some(Kind::Line { .. }) => "line",
        Some(Kind::Soft { .. }) => "soft",
        _ => "rotation",
    }
}

/// The given layers drawn together on a see-through page.
fn skin_composite(d: &Document, ids: &[LayerId]) -> Option<image::RgbaImage> {
    let layers: Vec<crate::doc::Layer> = d
        .layers
        .iter()
        .filter(|l| ids.contains(&l.id) && l.visible && matches!(l.kind, crate::doc::LayerKind::Raster | crate::doc::LayerKind::Vector))
        .cloned()
        .map(|mut l| {
            l.parent = None;
            l
        })
        .collect();
    if layers.is_empty() {
        return None;
    }
    let tmp = Document::from_parts(d.w, d.h, Color32::TRANSPARENT, layers);
    Some(crate::files::flatten(&tmp))
}
