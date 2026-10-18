// SPDX-License-Identifier: GPL-3.0-or-later
// The Rig tab: parts on the left, the moving picture in the middle, meshes, changers and sliders on the right.
// The picture is drawn through the meshes, so a slider moves the drawing the way the changers say.

use std::collections::HashMap;
use std::hash::{Hash, Hasher};
use std::sync::Arc;

use eframe::egui::{self, Color32, Pos2, Rect, RichText, Sense, Stroke, TextureHandle, TextureOptions, Ui, Vec2, pos2, vec2};

use super::SeviathApp;
use crate::doc::LayerId;
use crate::parts;
use crate::rig::raster::{self, Item, LayerTex};
use crate::rig::{self, ArtMesh, Form, Id, Kind, Rig, Values};
use crate::tabs::Tab;
use crate::theme::*;
use crate::widgets::{self, section_title};

const MAX_PREVIEW: f32 = 900.0;

#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
enum Mode {
    #[default]
    Look,
    /// The next click on the picture is the pivot of the selected Rotation changer.
    Pivot,
}

impl RigState {
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
    new_param: (String, f32, f32),
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

    /// Everything that is drawn, bottom first: (layer, mesh or a plain box, opacity).
    fn rig_items(&mut self) -> Vec<(LayerId, Arc<LayerTex>, Vec<[f32; 2]>, Vec<[f32; 2]>, Vec<[u32; 3]>, f32)> {
        let rig = self.draw.doc.rig.clone();
        let mut pose = rig.pose(&self.rigs.values);
        // a Warp being edited shows the shape being dragged
        if let Some((id, off)) = &self.rigs.warp_edit {
            pose.warp.insert(*id, off.clone());
        }
        let ids: Vec<(LayerId, f32)> = self.draw.doc.layers.iter().rev().filter(|l| l.visible && matches!(l.kind, crate::doc::LayerKind::Raster | crate::doc::LayerKind::Vector)).map(|l| (l.id, l.opacity / 100.0)).collect();
        let mut out = Vec::new();
        for (id, op) in ids {
            let Some(tex) = self.layer_tex(id) else { continue };
            match rig.mesh_of(id) {
                Some(m) => {
                    let now = rig.deformed(m, &pose);
                    out.push((id, tex, m.verts.clone(), now, m.tris.clone(), op));
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

        if let Some((id, c, s)) = picked {
            self.rig_select(id, c, s);
        }
    }

    fn rig_side(&mut self, ui: &mut Ui, now: f64) {
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
                if widgets::chip(ui, &format!("{name} ({})", if *is_rot { "rotation" } else { "warp" }), on).clicked() {
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
        if let (Some(cid), Some(pid)) = (self.rigs.sel_changer, self.rigs.sel_param) {
            self.rig_keyforms(ui, now, cid, pid);
        } else {
            ui.label(RichText::new("Pick a changer and a slider to say how the slider moves it.").size(11.5).color(TEXT_DIM));
        }
    }

    fn rig_keyforms(&mut self, ui: &mut Ui, now: f64, cid: Id, pid: Id) {
        let rig = self.draw.doc.rig.clone();
        let (Some(c), Some(p)) = (rig.changer(cid).cloned(), rig.param(pid).cloned()) else { return };
        section_title(ui, &format!("{} and {}", c.name, p.name));
        let value = rig.value(&self.rigs.values, pid);
        match c.kind {
            Kind::Rotation { .. } => {
                ui.horizontal(|ui| {
                    ui.label(RichText::new("Turns by").size(12.0).color(TEXT_DIM));
                    ui.add(egui::DragValue::new(&mut self.rigs.turn_deg).speed(0.5).suffix(" deg"));
                    ui.label(RichText::new("at the slider's end").size(12.0).color(TEXT_DIM));
                });
                if ui.button("Bind: turn with the slider").on_hover_text("At the low end it turns the other way, in the middle it doesn't turn").clicked() {
                    let deg = self.rigs.turn_deg;
                    self.rig_edit("Bind rotation to slider", now, |r| {
                        r.bindings.retain(|b| !(b.changer == cid && b.params == [pid]));
                        r.bind_rotation(cid, pid, deg);
                    });
                }
                ui.horizontal(|ui| {
                    if ui.button("Set the turn at this value").on_hover_text("Adds one keyform at the slider's value now").clicked() {
                        let deg = self.rigs.turn_deg;
                        self.rig_edit("Rotation keyform", now, |r| r.set_keyform(cid, pid, value, Form::Rot { deg }));
                    }
                });
            }
            Kind::Warp { cols, rows, .. } => {
                let editing = self.rigs.warp_edit.as_ref().is_some_and(|w| w.0 == cid);
                ui.label(RichText::new("Drag the grid points on the picture, then save the shape at this slider value.").size(11.5).color(TEXT_DIM));
                ui.horizontal(|ui| {
                    if !editing {
                        if ui.button("Edit the shape").clicked() {
                            // start from what is on screen now
                            let pose = rig.pose(&self.rigs.values);
                            let off = pose.warp.get(&cid).cloned().unwrap_or_else(|| vec![[0.0, 0.0]; ((cols + 1) * (rows + 1)) as usize]);
                            self.rigs.warp_edit = Some((cid, off));
                        }
                    } else {
                        if ui.button(RichText::new("Save the shape at this value").strong()).clicked() {
                            if let Some((_, off)) = self.rigs.warp_edit.take() {
                                self.rig_edit("Warp keyform", now, |r| r.set_keyform(cid, pid, value, Form::Warp { offsets: off }));
                            }
                        }
                        if ui.button("Cancel").clicked() {
                            self.rigs.warp_edit = None;
                        }
                    }
                });
            }
        }
        if let Some(b) = rig.bindings.iter().find(|b| b.changer == cid && b.params == [pid]) {
            ui.label(RichText::new(format!("Keyforms at: {}", b.keys[0].iter().map(|k| format!("{k:.1}")).collect::<Vec<_>>().join(", "))).size(11.5).color(TEXT_DIM));
            if ui.small_button("Remove these keyforms").clicked() {
                self.rig_edit("Remove keyforms", now, |r| r.bindings.retain(|x| !(x.changer == cid && x.params == [pid])));
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
        let to_page = |s: Pos2| [(s.x - page.min.x) / sc, (s.y - page.min.y) / sc];
        let rig = self.draw.doc.rig.clone();
        let mut pose = rig.pose(&self.rigs.values);
        if let Some((id, off)) = &self.rigs.warp_edit {
            pose.warp.insert(*id, off.clone());
        }
        // triangles of the picked parts
        if self.rigs.wire {
            for m in &rig.meshes {
                if !self.rigs.selected.contains(&m.layer) {
                    continue;
                }
                let now_v = rig.deformed(m, &pose);
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
            }
        }
        if let (Some((i, d)), Some((_, off))) = (drag_handle, self.rigs.warp_edit.as_mut()) {
            if let Some(o) = off.get_mut(i) {
                o[0] += d.x;
                o[1] += d.y;
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
