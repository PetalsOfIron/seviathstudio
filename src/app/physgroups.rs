// SPDX-License-Identifier: GPL-3.0-or-later
// The physics settings window of the Physics tab, built the way Live2D's is: any number of groups, and each group has its inputs (which
// sliders push the chain: across, up and down, or turning, how much, and whether it is the other way round), its pendulum (the weights
// of the chain: length, how freely it swings, how slowly it follows, how strongly it is pulled down) and its outputs (which sliders the
// chain moves, from which weight, how much). Several groups move the same sliders freely. The numbers are the ones the Live2D physics of
// `rig/paramphys.rs` uses, so what is made here swings as it would in a Live2D model (and files from Live2D open in the same window).

use eframe::egui::{self, RichText, Ui, vec2};

use super::SeviathApp;
use crate::rig::Id;
use crate::rig::paramphys::{Kind, ParamPhysics, PhysInput, PhysOutput, PhysSetting, PhysVertex, Range};
use crate::theme::*;
use crate::widgets;

/// Which page of the window is shown.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum Tab {
    #[default]
    Inputs,
    Pendulum,
    Outputs,
}

#[derive(Default)]
pub struct GroupsUi {
    pub sel: usize,
    pub tab: Tab,
}

/// A new group: a chain of weights hanging from the top, pushed by the head and the body, to move one slider. `inputs` and `output` are the
/// ids (Live2D ids) of the sliders.
fn new_group(name: &str, count: usize, length: f32, mobility: f32, delay: f32, accel: f32, inputs: &[(String, Kind, f32)], output: Option<(String, Kind, f32)>) -> PhysSetting {
    let mut vertices = vec![PhysVertex { pos: [0.0, 0.0], mobility: 1.0, delay: 1.0, acceleration: 1.0, radius: 0.0 }];
    for i in 1..=count {
        vertices.push(PhysVertex { pos: [0.0, length * i as f32], mobility, delay, acceleration: accel, radius: length });
    }
    PhysSetting {
        name: name.to_owned(),
        inputs: inputs.iter().map(|(id, kind, w)| PhysInput { id: id.clone(), weight: *w, kind: *kind, reflect: false }).collect(),
        outputs: output.map(|(id, kind, scale)| PhysOutput { id, vertex: count, scale, weight: 100.0, kind, reflect: false }).into_iter().collect(),
        vertices,
        position: Range { min: -10.0, default: 0.0, max: 10.0 },
        angle: Range { min: -10.0, default: 0.0, max: 10.0 },
    }
}

/// The pendulum of a group: the weights hang one under the other, each `radius` below the one before.
fn rehang(vertices: &mut [PhysVertex]) {
    for i in 1..vertices.len() {
        let above = vertices[i - 1].pos;
        vertices[i].pos = [above[0], above[1] + vertices[i].radius.max(0.1)];
    }
}

impl SeviathApp {
    /// The slider that has this Live2D id, or (when the id is empty) none.
    fn param_of_cid(&self, cid: &str) -> Option<Id> {
        self.draw.doc.rig.params.iter().find(|p| !p.cid.is_empty() && p.cid == cid).map(|p| p.id)
    }

    /// Give a slider the id physics reads it by (a plain version of its name), when it has none; returns the id.
    fn ensure_cid(&mut self, pid: Id, now: f64) -> String {
        if let Some(p) = self.draw.doc.rig.param(pid) {
            if !p.cid.is_empty() {
                return p.cid.clone();
            }
        }
        let name = self.draw.doc.rig.param(pid).map(|p| p.name.clone()).unwrap_or_default();
        let base: String = name.chars().filter(|c| c.is_alphanumeric()).collect();
        let mut cid = if base.is_empty() { format!("Param{pid}") } else { base };
        // two sliders with the same name do not share an id
        if self.draw.doc.rig.params.iter().any(|p| p.id != pid && p.cid == cid) {
            cid = format!("{cid}{pid}");
        }
        let c2 = cid.clone();
        self.rig_edit("Slider id for physics", now, |r| {
            if let Some(p) = r.params.iter_mut().find(|p| p.id == pid) {
                p.cid = c2;
            }
        });
        cid
    }

    /// The window: groups, then inputs, pendulum and outputs of the picked group.
    pub(super) fn physics_groups_body(&mut self, ui: &mut Ui, now: f64) {
        let rig = self.draw.doc.rig.clone();
        let mut pp: ParamPhysics = rig.param_physics.clone();
        ui.label(RichText::new("Each group is a chain that swings. Its inputs are the sliders that push it (the head turning, the body moving), its outputs are the sliders it moves (the angle of a strand of hair, the sway of a skirt). Make as many groups as you like: they all work together.").size(11.5).color(TEXT_DIM));
        let n = pp.settings.len();
        let mut sel = self.rigs.phys.groups.sel.min(n.saturating_sub(1));
        // -- the groups
        let mut add_preset: Option<usize> = None;
        let mut duplicate = false;
        let mut delete = false;
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing = vec2(4.0, 4.0);
            ui.label(RichText::new("Group").size(12.0).color(TEXT_DIM));
            egui::ComboBox::from_id_salt("phys_group").selected_text(pp.settings.get(sel).map(|s| s.name.clone()).unwrap_or_else(|| "none yet".into())).width(170.0).show_ui(ui, |ui| {
                for (i, s) in pp.settings.iter().enumerate() {
                    if ui.selectable_label(sel == i, &s.name).clicked() {
                        sel = i;
                    }
                }
            });
            egui::ComboBox::from_id_salt("phys_group_add").selected_text("Add...").width(110.0).show_ui(ui, |ui| {
                for (i, (name, _)) in PRESETS.iter().enumerate() {
                    if ui.selectable_label(false, *name).clicked() {
                        add_preset = Some(i);
                    }
                }
            });
            if ui.add_enabled(n > 0, egui::Button::new("Duplicate")).clicked() {
                duplicate = true;
            }
            if ui.add_enabled(n > 0, egui::Button::new("Delete")).clicked() {
                delete = true;
            }
        });
        if let Some(pi) = add_preset {
            let g = self.preset_group(pi, &rig);
            pp.settings.push(g);
            sel = pp.settings.len() - 1;
        }
        if duplicate {
            if let Some(g) = pp.settings.get(sel).cloned() {
                let mut g = g;
                g.name = format!("{} copy", g.name);
                pp.settings.push(g);
                sel = pp.settings.len() - 1;
            }
        }
        if delete && sel < pp.settings.len() {
            pp.settings.remove(sel);
            sel = sel.saturating_sub(1);
        }
        self.rigs.phys.groups.sel = sel;
        if pp.settings.is_empty() {
            ui.add_space(6.0);
            ui.label(RichText::new("No groups yet. Press Add and pick what should swing (hair, a skirt, a hanging thing), or Empty to build one yourself.").size(12.0));
        }
        if let Some(g) = pp.settings.get_mut(sel) {
            ui.horizontal(|ui| {
                ui.label(RichText::new("Name").size(12.0).color(TEXT_DIM));
                ui.add(egui::TextEdit::singleline(&mut g.name).desired_width(200.0).char_limit(40));
            });
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing = vec2(4.0, 4.0);
                for (t, label) in [(Tab::Inputs, "Input settings"), (Tab::Pendulum, "Pendulum settings"), (Tab::Outputs, "Output settings")] {
                    if widgets::chip(ui, label, self.rigs.phys.groups.tab == t).clicked() {
                        self.rigs.phys.groups.tab = t;
                    }
                }
            });
            ui.separator();
            let params: Vec<(Id, String, String)> = rig.params.iter().map(|p| (p.id, p.name.clone(), p.cid.clone())).collect();
            let mut need_cid: Vec<Id> = Vec::new();
            match self.rigs.phys.groups.tab {
                Tab::Inputs => {
                    let mut remove: Option<usize> = None;
                    for (i, inp) in g.inputs.iter_mut().enumerate() {
                        ui.horizontal(|ui| {
                            slider_pick(ui, ("pin", i), &params, &mut inp.id, &mut need_cid);
                            kind_pick(ui, ("pink", i), &mut inp.kind);
                            ui.label(RichText::new("effect").size(11.0).color(TEXT_DIM));
                            ui.add(egui::DragValue::new(&mut inp.weight).range(0.0..=100.0).speed(1.0).suffix("%"));
                            ui.checkbox(&mut inp.reflect, "Reflect").on_hover_text("The other way round");
                            if ui.small_button("Delete").clicked() {
                                remove = Some(i);
                            }
                        });
                    }
                    if let Some(i) = remove {
                        g.inputs.remove(i);
                    }
                    if ui.button("Add an input").clicked() {
                        let first = params.first().map(|p| p.2.clone()).unwrap_or_default();
                        g.inputs.push(PhysInput { id: first, weight: 100.0, kind: Kind::X, reflect: false });
                        if let Some(p) = params.first() {
                            need_cid.push(p.0);
                        }
                    }
                    ui.add_space(4.0);
                    ui.label(RichText::new("Input normalization: how far a slider counts").size(12.0).strong());
                    range_row(ui, "Angle", &mut g.angle);
                    range_row(ui, "Position", &mut g.position);
                }
                Tab::Pendulum => {
                    ui.label(RichText::new("The weights of the chain, from the top. The first one is fixed to the inputs.").size(11.5).color(TEXT_DIM));
                    let mut remove: Option<usize> = None;
                    let mut dup: Option<usize> = None;
                    egui::Grid::new("phys_pendulum").num_columns(6).spacing([8.0, 4.0]).show(ui, |ui| {
                        for h in ["No", "Length", "Shaking influence", "Reaction time", "Convergence", ""] {
                            ui.label(RichText::new(h).size(11.0).color(TEXT_DIM));
                        }
                        ui.end_row();
                        for (i, v) in g.vertices.iter_mut().enumerate() {
                            ui.label(format!("{i}"));
                            if i == 0 {
                                ui.label(RichText::new("-").color(TEXT_DIM));
                            } else {
                                ui.add(egui::DragValue::new(&mut v.radius).range(0.1..=50.0).speed(0.1).max_decimals(2));
                            }
                            ui.add(egui::DragValue::new(&mut v.mobility).range(0.0..=1.0).speed(0.01).max_decimals(2)).on_hover_text("0 holds still, 1 swings freely");
                            ui.add(egui::DragValue::new(&mut v.delay).range(0.0..=3.0).speed(0.01).max_decimals(2)).on_hover_text("Bigger follows faster");
                            ui.add(egui::DragValue::new(&mut v.acceleration).range(0.0..=5.0).speed(0.01).max_decimals(2)).on_hover_text("How strongly it is pulled down");
                            ui.horizontal(|ui| {
                                if i > 0 && ui.small_button("Duplicate").clicked() {
                                    dup = Some(i);
                                }
                                if i > 1 && ui.small_button("Delete").clicked() {
                                    remove = Some(i);
                                }
                            });
                            ui.end_row();
                        }
                    });
                    if let Some(i) = dup {
                        let v = g.vertices[i].clone();
                        g.vertices.insert(i + 1, v);
                    }
                    if let Some(i) = remove {
                        g.vertices.remove(i);
                        for o in g.outputs.iter_mut() {
                            o.vertex = o.vertex.min(g.vertices.len() - 1);
                        }
                    }
                    if ui.button("Add a weight").clicked() {
                        let last = g.vertices.last().cloned().unwrap_or(PhysVertex { pos: [0.0, 0.0], mobility: 0.9, delay: 0.9, acceleration: 1.0, radius: 3.0 });
                        g.vertices.push(PhysVertex { radius: if last.radius > 0.0 { last.radius } else { 3.0 }, ..last });
                    }
                    rehang(&mut g.vertices);
                    // the chain, drawn from the top: how long each weight hangs
                    let (r, _) = ui.allocate_exact_size(vec2(160.0, 170.0), egui::Sense::hover());
                    let p = ui.painter_at(r);
                    p.rect_filled(r, 4.0, egui::Color32::from_gray(26));
                    let reach: f32 = g.vertices.iter().skip(1).map(|v| v.radius).sum::<f32>().max(1.0);
                    let k = (r.height() - 24.0) / reach;
                    let mut at = egui::pos2(r.center().x, r.top() + 10.0);
                    let mut last = at;
                    for (i, v) in g.vertices.iter().enumerate() {
                        if i > 0 {
                            at.y += v.radius * k;
                            p.line_segment([last, at], egui::Stroke::new(1.5, egui::Color32::from_gray(150)));
                        }
                        p.circle_filled(at, if i == 0 { 4.0 } else { 3.5 }, if i == 0 { egui::Color32::WHITE } else { egui::Color32::from_rgb(70, 140, 240) });
                        p.text(at + vec2(7.0, 0.0), egui::Align2::LEFT_CENTER, format!("{i}"), egui::FontId::proportional(10.0), TEXT_DIM);
                        last = at;
                    }
                }
                Tab::Outputs => {
                    let weights = g.vertices.len();
                    let mut remove: Option<usize> = None;
                    for (i, o) in g.outputs.iter_mut().enumerate() {
                        ui.horizontal(|ui| {
                            ui.label(RichText::new("weight").size(11.0).color(TEXT_DIM));
                            ui.add(egui::DragValue::new(&mut o.vertex).range(1..=weights.saturating_sub(1).max(1)));
                            slider_pick(ui, ("pout", i), &params, &mut o.id, &mut need_cid);
                            kind_pick(ui, ("poutk", i), &mut o.kind);
                        });
                        ui.horizontal(|ui| {
                            ui.add_space(24.0);
                            ui.label(RichText::new("effect").size(11.0).color(TEXT_DIM));
                            ui.add(egui::DragValue::new(&mut o.weight).range(0.0..=100.0).speed(1.0).suffix("%"));
                            ui.label(RichText::new("scale").size(11.0).color(TEXT_DIM));
                            ui.add(egui::DragValue::new(&mut o.scale).range(-500.0..=500.0).speed(0.5)).on_hover_text("How far the slider goes for a swing of this size");
                            ui.checkbox(&mut o.reflect, "Reflect");
                            if ui.small_button("Delete").clicked() {
                                remove = Some(i);
                            }
                        });
                    }
                    if let Some(i) = remove {
                        g.outputs.remove(i);
                    }
                    ui.horizontal_wrapped(|ui| {
                        if ui.button("Increase output").on_hover_text("Every output reaches 25 percent further").clicked() {
                            for o in g.outputs.iter_mut() {
                                o.scale = (o.scale * 1.25).clamp(-500.0, 500.0);
                            }
                        }
                        if ui.button("Decrease output").clicked() {
                            for o in g.outputs.iter_mut() {
                                o.scale = (o.scale / 1.25).clamp(-500.0, 500.0);
                            }
                        }
                        if ui.button("Reset magnification").on_hover_text("Every output back to a scale of 10").clicked() {
                            for o in g.outputs.iter_mut() {
                                o.scale = 10.0 * o.scale.signum();
                            }
                        }
                    });
                    if ui.button("Add an output").clicked() {
                        let first = params.first().map(|p| p.2.clone()).unwrap_or_default();
                        g.outputs.push(PhysOutput { id: first, vertex: weights.saturating_sub(1).max(1), scale: 10.0, weight: 100.0, kind: Kind::Angle, reflect: false });
                        if let Some(p) = params.first() {
                            need_cid.push(p.0);
                        }
                    }
                }
            }
            for pid in need_cid {
                self.ensure_cid(pid, now);
            }
        }
        // -- all groups together
        ui.separator();
        ui.horizontal_wrapped(|ui| {
            ui.checkbox(&mut pp.on, "Physics of the groups on");
            egui::ComboBox::from_id_salt("phys_fps").selected_text(format!("{} fps", pp.fps.round())).width(80.0).show_ui(ui, |ui| {
                for f in [30.0, 60.0, 90.0, 120.0] {
                    ui.selectable_value(&mut pp.fps, f, format!("{f} fps"));
                }
            });
            ui.label(RichText::new("Strength").size(12.0).color(TEXT_DIM));
            ui.add(egui::Slider::new(&mut pp.strength, 0.0..=2.0).show_value(false)).on_hover_text("1 is as set up");
            ui.label(RichText::new("Gravity").size(12.0).color(TEXT_DIM));
            ui.add(egui::DragValue::new(&mut pp.gravity[1]).range(-3.0..=3.0).speed(0.05).max_decimals(2));
            ui.label(RichText::new("Wind").size(12.0).color(TEXT_DIM));
            ui.add(egui::DragValue::new(&mut pp.wind[0]).range(-5.0..=5.0).speed(0.05).max_decimals(2));
            if ui.small_button("Settle").on_hover_text("The chains hang still again").clicked() {
                self.rigs.phys.param_state = Default::default();
            }
        });
        // the groups' ids may have been made just now
        let fixed = self.draw.doc.rig.clone();
        let _ = fixed;
        if pp != rig.param_physics {
            self.rig_edit("Physics groups", now, |r| r.param_physics = pp);
        }
    }

    /// A group from a preset, with the sliders of this rig that fit (the head and body angles) as its inputs.
    fn preset_group(&mut self, i: usize, rig: &crate::rig::Rig) -> PhysSetting {
        let (name, _) = PRESETS[i];
        // the sliders by what they are called
        let find = |names: &[&str]| rig.params.iter().find(|p| names.iter().any(|n| p.name.to_lowercase().replace(' ', "") == *n)).map(|p| p.id);
        let head_x = find(&["anglex"]);
        let head_z = find(&["anglez"]);
        let body_z = find(&["bodytilt", "bodyz", "bodyanglez"]);
        let mut inputs: Vec<(String, Kind, f32)> = Vec::new();
        let mut cids: Vec<Id> = Vec::new();
        for (pid, kind, w) in [(head_x, Kind::X, 100.0), (head_z, Kind::Angle, 80.0), (body_z, Kind::Angle, 60.0)] {
            if let Some(pid) = pid {
                cids.push(pid);
                inputs.push((format!("{pid}"), kind, w));
            }
        }
        // the ids of the sliders (made when they have none)
        let now = 0.0;
        let mut real: Vec<(String, Kind, f32)> = Vec::new();
        for ((_, kind, w), pid) in inputs.iter().zip(&cids) {
            let cid = self.ensure_cid(*pid, now);
            real.push((cid, *kind, *w));
        }
        // the slider that is moved: one called like the group, or a new one
        let out_name = format!("{name} sway");
        let out_cid = match self.draw.doc.rig.params.iter().find(|p| p.name == out_name).map(|p| p.id) {
            Some(pid) => self.ensure_cid(pid, now),
            None => {
                let mut pid = 0;
                self.rig_edit("Slider for the physics", now, |r| pid = r.add_param(&out_name, -30.0, 30.0, 0.0));
                self.ensure_cid(pid, now)
            }
        };
        let (count, length, mobility, delay, accel) = match i {
            0 => (3, 3.0, 0.9, 0.9, 1.5),
            1 => (4, 3.5, 0.95, 0.85, 1.2),
            2 => (2, 3.0, 0.8, 0.8, 2.0),
            _ => (3, 3.0, 0.9, 0.9, 1.5),
        };
        if i == 3 {
            return new_group("Empty", count, length, mobility, delay, accel, &[], None);
        }
        new_group(name, count, length, mobility, delay, accel, &real, Some((out_cid, Kind::Angle, 12.0)))
    }
}

/// What can be added.
const PRESETS: [(&str, &str); 4] = [("Hair", "hair"), ("Skirt", "skirt"), ("Hanging thing", "thing"), ("Empty", "")];

/// Pick the slider (by its Live2D id) from the sliders there are; a slider that has no id yet gets one afterwards.
fn slider_pick(ui: &mut Ui, id: (&str, usize), params: &[(Id, String, String)], cid: &mut String, need: &mut Vec<Id>) {
    let current = params.iter().find(|p| !p.2.is_empty() && p.2 == *cid).map(|p| p.1.clone()).unwrap_or_else(|| if cid.is_empty() { "pick a slider".to_string() } else { format!("{cid} (not in this rig)") });
    egui::ComboBox::from_id_salt(id).selected_text(current).width(150.0).show_ui(ui, |ui| {
        for (pid, name, c) in params {
            if ui.selectable_label(!c.is_empty() && c == cid, name).clicked() {
                if c.is_empty() {
                    need.push(*pid);
                    // the id the slider is about to get, as `ensure_cid` makes it
                    let base: String = name.chars().filter(|ch| ch.is_alphanumeric()).collect();
                    *cid = if base.is_empty() { format!("Param{pid}") } else { base };
                } else {
                    *cid = c.clone();
                }
            }
        }
    });
}

fn kind_pick(ui: &mut Ui, id: (&str, usize), kind: &mut Kind) {
    egui::ComboBox::from_id_salt(id).selected_text(match kind {
        Kind::X => "Position X",
        Kind::Y => "Position Y",
        Kind::Angle => "Angle",
    }).width(90.0).show_ui(ui, |ui| {
        ui.selectable_value(kind, Kind::X, "Position X");
        ui.selectable_value(kind, Kind::Y, "Position Y");
        ui.selectable_value(kind, Kind::Angle, "Angle");
    });
}

fn range_row(ui: &mut Ui, label: &str, r: &mut Range) {
    ui.horizontal(|ui| {
        ui.label(RichText::new(label).size(12.0).color(TEXT_DIM));
        ui.label(RichText::new("min").size(11.0).color(TEXT_DIM));
        ui.add(egui::DragValue::new(&mut r.min).speed(0.5));
        ui.label(RichText::new("center").size(11.0).color(TEXT_DIM));
        ui.add(egui::DragValue::new(&mut r.default).speed(0.5));
        ui.label(RichText::new("max").size(11.0).color(TEXT_DIM));
        ui.add(egui::DragValue::new(&mut r.max).speed(0.5));
    });
}

#[cfg(test)]
impl SeviathApp {
    /// Add a group from a preset (0 hair, 1 skirt, 2 hanging thing, 3 empty), as the Add menu does.
    pub fn test_add_physics_group(&mut self, preset: usize) {
        let rig = self.draw.doc.rig.clone();
        let g = self.preset_group(preset, &rig);
        let mut pp = self.draw.doc.rig.param_physics.clone();
        pp.settings.push(g);
        self.rig_edit("Physics groups", 0.0, |r| r.param_physics = pp);
    }

    /// The slider values that the physics of the groups gives now, by slider name.
    pub fn test_physics_out(&self) -> Vec<(String, f32)> {
        let rig = &self.draw.doc.rig;
        self.rigs.phys.param_out.iter().filter_map(|(id, v)| rig.param(*id).map(|p| (p.name.clone(), *v))).collect()
    }
}
