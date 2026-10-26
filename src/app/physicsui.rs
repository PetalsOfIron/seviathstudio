// SPDX-License-Identifier: GPL-3.0-or-later
// The Physics tab: pick parts, say what they are (hair, cloth, earring…), and they swing and lag when the head or body
// moves. The preview is the Rig tab's picture; a test panel moves the head so you can see it.

use crate::widgets::HelpExt;
use std::collections::HashMap;

use eframe::egui::{self, Color32, RichText, Sense, Ui, vec2};

use super::SeviathApp;
use crate::doc::LayerId;
use crate::parts;
use crate::rig::physics::{self, Collider, Kind as PKind, Material, PhysChain, PhysSim, Wind};
use crate::rig::{Id, Kind, Rig, Values};
use crate::theme::*;
use crate::widgets::{self, section_title};

/// The test motions: (name, slider, degrees, beats a second, seconds).
pub const MOTIONS: [(&str, &str, f32, f32, f32); 4] = [("Shake the head", "Angle X", 28.0, 1.6, 2.5), ("Nod", "Angle Y", 22.0, 1.4, 2.5), ("Tilt", "Angle Z", 20.0, 1.2, 2.5), ("Slow turn", "Angle X", 30.0, 0.3, 6.0)];

pub struct PhysUi {
    pub on: bool,
    pub sim: PhysSim,
    pub offs: HashMap<(Id, usize), Vec<[f32; 2]>>,
    pub sel: Option<usize>,
    pub kind: PKind,
    pub material: Material,
    /// The test motion running: index and seconds so far.
    pub motion: Option<(usize, f32)>,
    pub message: Option<String>,
    /// The swinging of a Live2D model (parameter physics): where its chains are, and the slider values it gives now.
    /// The physics settings window (groups, inputs, pendulum, outputs).
    pub groups: super::physgroups::GroupsUi,
    pub param_state: crate::rig::paramphys::PhysState,
    pub param_out: HashMap<Id, f32>,
}

impl Default for PhysUi {
    fn default() -> Self {
        Self { groups: Default::default(), on: true, sim: PhysSim::default(), offs: HashMap::new(), sel: None, kind: PKind::Hair, material: Material::Normal, motion: None, message: None, param_state: Default::default(), param_out: HashMap::new() }
    }
}

/// The sliders of a rig by their Live2D id, for the parameter physics: the values read are the ones the user set, the values written
/// are collected.
struct RigSliders<'a> {
    rig: &'a crate::rig::Rig,
    values: &'a Values,
    out: HashMap<Id, f32>,
}

impl crate::rig::paramphys::Sliders for RigSliders<'_> {
    fn get(&self, id: &str) -> Option<(f32, f32, f32, f32)> {
        let p = self.rig.params.iter().find(|p| !p.cid.is_empty() && p.cid == id)?;
        let v = self.out.get(&p.id).copied().or_else(|| self.values.0.get(&p.id).copied()).unwrap_or(p.default);
        Some((v, p.min, p.max, p.default))
    }

    fn set(&mut self, id: &str, value: f32) {
        if let Some(p) = self.rig.params.iter().find(|p| !p.cid.is_empty() && p.cid == id) {
            self.out.insert(p.id, value);
        }
    }
}

impl super::rigtab::RigState {
    /// The slider values to show: the ones the user set, and over them what the Live2D physics gives.
    pub fn effective_values(&self) -> Values {
        let mut v = self.values.clone();
        for (id, x) in &self.phys.param_out {
            v.set(*id, *x);
        }
        v
    }

    /// Add what the physics does to a pose (after the sliders and IK).
    pub fn add_phys(&self, rig: &crate::rig::Rig, pose: &mut crate::rig::Pose) {
        if !self.phys.on {
            return;
        }
        for ((id, path), offs) in &self.phys.offs {
            let Some(c) = rig.changer(*id) else { continue };
            match &c.kind {
                crate::rig::Kind::Line { .. } => {
                    let e = pose.line.entry(*id).or_insert_with(|| vec![[0.0, 0.0]; offs.len()]);
                    if e.len() == offs.len() {
                        for (a, o) in e.iter_mut().zip(offs) {
                            a[0] += o[0];
                            a[1] += o[1];
                        }
                    }
                }
                crate::rig::Kind::Paths { paths } => {
                    let e = pose.paths.entry(*id).or_insert_with(|| paths.iter().map(|p| vec![[0.0, 0.0]; p.pts.len()]).collect());
                    if let Some(e) = e.get_mut(*path).filter(|e| e.len() == offs.len()) {
                        for (a, o) in e.iter_mut().zip(offs) {
                            a[0] += o[0];
                            a[1] += o[1];
                        }
                    }
                }
                _ => {}
            }
        }
    }
}

impl SeviathApp {
    /// Move the simulation on by `dt` seconds (also runs the test motion).
    pub fn phys_advance(&mut self, dt: f32) {
        let rig = self.draw.doc.rig.clone();
        // the swinging of a Live2D model moves sliders
        if self.rigs.phys.on && rig.param_physics.on && !rig.param_physics.is_empty() {
            let mut s = RigSliders { rig: &rig, values: &self.rigs.values, out: HashMap::new() };
            rig.param_physics.step(&mut self.rigs.phys.param_state, &mut s, dt);
            self.rigs.phys.param_out = s.out;
        } else {
            self.rigs.phys.param_out.clear();
        }
        if !self.rigs.phys.on || rig.physics.is_empty() {
            self.rigs.phys.offs.clear();
            self.rigs.phys.motion = None;
            return;
        }
        if let Some((mi, t)) = self.rigs.phys.motion {
            let (_, name, deg, beats, secs) = MOTIONS[mi];
            let t = t + dt;
            let param = rig.params.iter().find(|p| p.name == name).map(|p| (p.id, p.default));
            if let Some((id, def)) = param {
                if t >= secs {
                    self.rigs.values.set(id, def);
                    self.rigs.phys.motion = None;
                } else {
                    // a smooth start so it does not jump
                    let ease = (t / 0.3).min(1.0);
                    self.rigs.values.set(id, def + deg * ease * (std::f32::consts::TAU * beats * t).sin());
                    self.rigs.phys.motion = Some((mi, t));
                }
            } else {
                self.rigs.phys.motion = None;
            }
        }
        let base = rig.pose(&self.rigs.values);
        let rest = rig.pose(&Values::default());
        self.rigs.phys.offs = self.rigs.phys.sim.step(&rig, &base, &rest, dt);
    }

    /// Called every frame the picture is shown.
    pub fn phys_tick(&mut self, ctx: &egui::Context) {
        let rig = &self.draw.doc.rig;
        if (rig.physics.is_empty() && !(rig.param_physics.on && !rig.param_physics.is_empty())) || !self.rigs.phys.on {
            return;
        }
        let dt = ctx.input(|i| i.unstable_dt).clamp(0.0, 0.05);
        self.phys_advance(dt);
        ctx.request_repaint();
    }

    pub(super) fn physics_ui(&mut self, ui: &mut Ui) {
        let ctx = ui.ctx().clone();
        let now = ctx.input(|i| i.time);
        self.rig_init();
        self.phys_tick(&ctx);
        self.rig_render(&ctx);
        let mut picked: Option<(LayerId, bool, bool)> = None;
        egui::Panel::top("phys_toolbar").frame(egui::Frame::new().fill(PANEL).inner_margin(egui::Margin::same(8))).show(ui, |ui| self.rig_toolbar(ui, false, true, false));
        egui::Panel::left("phys_layers").default_size(270.0).size_range(210.0..=440.0).resizable(true).frame(egui::Frame::new().fill(PANEL).inner_margin(egui::Margin::same(10))).show(ui, |ui| {
            picked = self.rig_left_panel(ui, &ctx, now, "phys", true, true);
        });
        egui::Panel::left("phys_params").default_size(250.0).size_range(190.0..=420.0).resizable(true).frame(egui::Frame::new().fill(PANEL).inner_margin(egui::Margin::same(10))).show(ui, |ui| {
            self.rig_params_column(ui, now, "phys");
        });
        egui::Panel::right("phys_tools").default_size(330.0).size_range(280.0..=500.0).resizable(true).frame(egui::Frame::new().fill(PANEL).inner_margin(egui::Margin::same(10))).show(ui, |ui| {
            egui::ScrollArea::vertical().id_salt("phys_right").show(ui, |ui| self.physics_side(ui, now));
        });
        egui::CentralPanel::default().frame(egui::Frame::new().fill(Color32::from_rgb(14, 20, 18))).show(ui, |ui| self.rig_view(ui, now, &mut picked));
        self.rig_tool_window(&ctx, now);
        if let Some((id, c, s)) = picked.filter(|p| p.0 != u64::MAX) {
            self.rig_select(id, c, s);
            // picking a part that has physics shows its numbers
            let rig = &self.draw.doc.rig;
            self.rigs.phys.sel = rig.mesh_of(id).and_then(|m| m.parent).and_then(|p| rig.physics.iter().position(|c| c.changer == p));
        }
    }

    fn physics_side(&mut self, ui: &mut Ui, now: f64) {
        self.live2d_files_ui(ui, now);
        section_title(ui, "Physics");
        ui.horizontal_wrapped(|ui| {
            ui.checkbox(&mut self.rigs.phys.on, "Physics on").help("Off: the picture only follows the sliders");
            if ui.button("Reset").help("Things hang still again").clicked() {
                self.rigs.phys.sim.reset();
                self.rigs.phys.offs.clear();
            }
        });
        // what to add
        section_title(ui, "Make parts move");
        ui.label(RichText::new("Pick parts on the left, say what they are, then press Make it move.").size(11.5).color(TEXT_DIM));
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing = vec2(4.0, 4.0);
            for k in PKind::ALL {
                if widgets::chip(ui, k.label(), self.rigs.phys.kind == k).help(k.about()).clicked() {
                    self.rigs.phys.kind = k;
                }
            }
        });
        ui.horizontal(|ui| {
            ui.label(RichText::new("Material").size(12.0).color(TEXT_DIM));
            for m in Material::ALL {
                if widgets::chip(ui, m.label(), self.rigs.phys.material == m).help("Light cloth catches the wind. Heavy cloth hangs and moves slowly.").clicked() {
                    self.rigs.phys.material = m;
                }
            }
        });
        ui.horizontal_wrapped(|ui| {
            if ui.button(RichText::new("Make it move").strong()).help("Adds a curve along each picked part and makes it swing").clicked() {
                self.add_physics(now);
            }
            if ui.button("Hair and accessories from the tags").help("Every part tagged as hair gets hair physics, every accessory a hanging one").clicked() {
                self.add_physics_from_tags(now);
            }
        });
        if let Some(m) = &self.rigs.phys.message {
            ui.label(RichText::new(m).size(11.5).color(TEXT_DIM));
        }
        // the things that move
        let rig = self.draw.doc.rig.clone();
        if !rig.physics.is_empty() {
            section_title(ui, "Moving things");
            for (i, c) in rig.physics.iter().enumerate() {
                ui.horizontal(|ui| {
                    if widgets::chip(ui, &format!("{} ({})", c.name, c.kind.label()), self.rigs.phys.sel == Some(i)).help("Pick this choice.").clicked() {
                        self.rigs.phys.sel = Some(i);
                    }
                });
            }
        }
        if let Some(i) = self.rigs.phys.sel.filter(|i| *i < rig.physics.len()) {
            self.physics_chain_settings(ui, now, i);
        }
        // the world
        section_title(ui, "Wind and gravity");
        let mut w = rig.world.clone();
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing = vec2(4.0, 4.0);
            for k in Wind::ALL {
                if widgets::chip(ui, k.label(), w.wind == k).help("Pick how the wind blows.").clicked() {
                    w.wind = k;
                }
            }
        });
        ui.horizontal(|ui| {
            ui.label(RichText::new("Wind comes").size(12.0).color(TEXT_DIM));
            if widgets::chip(ui, "From the left", w.wind_dir > 0.0).help("The light comes from the left.").clicked() {
                w.wind_dir = 1.0;
            }
            if widgets::chip(ui, "From the right", w.wind_dir < 0.0).help("The light comes from the right.").clicked() {
                w.wind_dir = -1.0;
            }
        });
        ui.horizontal(|ui| {
            ui.label(RichText::new("Gravity").size(12.0).color(TEXT_DIM));
            let mut pct = w.gravity / 2400.0 * 100.0;
            if ui.add(egui::Slider::new(&mut pct, 0.0..=300.0).suffix("%").show_value(true)).changed() {
                w.gravity = (pct / 100.0 * 2400.0).max(24.0);
            }
        });
        if w != rig.world {
            self.rig_edit("Wind and gravity", now, |r| r.world = w);
        }
        self.physics_colliders(ui, now);
        // the test panel
        section_title(ui, "Test");
        ui.label(RichText::new("Moves the head by itself so you can watch the hair and cloth.").size(11.5).color(TEXT_DIM));
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing = vec2(4.0, 4.0);
            for (i, (name, slider, ..)) in MOTIONS.iter().enumerate() {
                let has = rig.params.iter().any(|p| p.name == *slider);
                ui.add_enabled_ui(has, |ui| {
                    if ui.button(*name).help(if has { "Moves the head" } else { "Needs the face rig first" }).clicked() {
                        self.rigs.phys.motion = Some((i, 0.0));
                    }
                });
            }
        });
    }

    /// The files of a Live2D model (physics, expressions, motions, VTube Studio settings) put onto this rig.
    fn live2d_files_ui(&mut self, ui: &mut Ui, now: f64) {
        section_title(ui, "Live2D files");
        ui.label(RichText::new("The swinging, expressions and motions of a Live2D model (a .model3.json with its files, as in a VTube Studio folder) can be put onto this rig. Sliders are matched by their Live2D ids.").size(11.5).color(TEXT_DIM));
        ui.horizontal_wrapped(|ui| {
            if ui.button("Add the files of a Live2D model...").help("Pick the .model3.json: its physics, expressions, motions and VTube Studio settings are read").clicked() {
                if let Some(path) = rfd::FileDialog::new().set_title("A Live2D model (.model3.json)").set_directory(self.settings.projects_folder()).add_filter("Live2D model", &["json"]).pick_file() {
                    match crate::import::runtime::load_folder(&path) {
                        Ok(files) => {
                            let mut report = crate::import::runtime::Report::default();
                            self.rig_edit("Live2D files", now, |r| report = crate::import::runtime::apply(r, &files));
                            self.rigs.phys.param_state = Default::default();
                            let mut m = format!("Read {}: {} swinging settings, {} expressions, {} motions as clips, {} sliders matched.", path.file_name().and_then(|n| n.to_str()).unwrap_or("the model"), report.physics_settings, report.expressions, report.motions, report.sliders_matched);
                            if !report.sliders_missing.is_empty() {
                                m += &format!(" The rig has no slider for: {}.", report.sliders_missing.iter().take(8).cloned().collect::<Vec<_>>().join(", "));
                            }
                            for n in report.notes.iter().take(3) {
                                m += &format!(" {n}");
                            }
                            self.rigs.phys.message = Some(m);
                        }
                        Err(e) => self.rigs.phys.message = Some(e),
                    }
                }
            }
        });
        let rig = self.draw.doc.rig.clone();
        if !rig.param_physics.is_empty() {
            let mut pp = rig.param_physics.clone();
            ui.horizontal_wrapped(|ui| {
                ui.checkbox(&mut pp.on, "Live2D physics on").help("Turn the physics of the imported Live2D model on or off.");
                ui.label(RichText::new("Strength").size(12.0).color(TEXT_DIM));
                ui.add(egui::Slider::new(&mut pp.strength, 0.0..=2.0).show_value(false)).help("1 is as the model was set up");
                if ui.small_button("Settle").help("The chains hang still again").clicked() {
                    self.rigs.phys.param_state = Default::default();
                }
            });
            if pp != rig.param_physics {
                self.rig_edit("Live2D physics", now, |r| r.param_physics = pp);
            }
            let names: Vec<String> = rig.param_physics.settings.iter().map(|s| s.name.clone()).collect();
            ui.label(RichText::new(format!("{} swinging settings: {}", names.len(), names.join(", "))).size(11.0).color(TEXT_DIM));
        }
        if !rig.expressions.is_empty() {
            ui.label(RichText::new("Expressions").size(12.0).color(TEXT_DIM));
            ui.horizontal_wrapped(|ui| {
                ui.spacing_mut().item_spacing = vec2(4.0, 4.0);
                for e in &rig.expressions {
                    if ui.small_button(&e.name).help("Put this expression on the sliders (All sliders to the middle in the Rig tab takes it off)").clicked() {
                        for ep in &e.params {
                            if let Some(p) = rig.params.iter().find(|p| p.cid == ep.id) {
                                let cur = self.rigs.values.0.get(&p.id).copied().unwrap_or(p.default);
                                if let Some(v) = e.blend(&ep.id, cur) {
                                    self.rigs.values.set(p.id, v.clamp(p.min, p.max));
                                }
                            }
                        }
                    }
                }
            });
        }
        if let Some(m) = &self.rigs.phys.message {
            ui.label(RichText::new(m).size(11.5).color(TEXT_DIM));
        }
    }

    fn physics_chain_settings(&mut self, ui: &mut Ui, now: f64, i: usize) {
        let Some(c) = self.draw.doc.rig.physics.get(i).cloned() else { return };
        section_title(ui, &format!("{}: {}", c.name, c.kind.label()));
        let mut n = c.clone();
        let row = |ui: &mut Ui, label: &str, tip: &str, v: &mut f32, hi: f32| {
            ui.horizontal(|ui| {
                ui.label(RichText::new(label).size(12.0).color(TEXT_DIM)).help(tip);
                ui.add(egui::Slider::new(v, 0.0..=hi).max_decimals(2));
            });
        };
        ui.checkbox(&mut n.enabled, "This one moves").help("This part moves with the slider.");
        row(ui, "Stiff at the root", "How strongly the base is held in place", &mut n.stiff_root, 1.0);
        row(ui, "Stiff at the tip", "How strongly the end is held in place: low is floppy", &mut n.stiff_tip, 1.0);
        row(ui, "Slows down", "Higher: stops swinging sooner", &mut n.damping, 0.98);
        row(ui, "Gravity", "How much it hangs down", &mut n.gravity, 3.0);
        row(ui, "Wind", "How much the wind moves it", &mut n.wind, 3.0);
        ui.horizontal(|ui| {
            ui.label(RichText::new("Material").size(12.0).color(TEXT_DIM));
            for m in Material::ALL {
                if widgets::chip(ui, m.label(), n.material == m).help("Pick what the layer is made of: it changes how light shows on it.").clicked() {
                    n = PhysChain { name: c.name.clone(), enabled: c.enabled, ..PhysChain::new(&c.name, c.kind, m, c.changer) };
                }
            }
        });
        let mut similar = false;
        let mut remove = false;
        ui.horizontal_wrapped(|ui| {
            if ui.button("Same for all of this type").help("Copies these numbers to every other thing of the same type").clicked() {
                similar = true;
            }
            if ui.button("Remove").help("Takes the physics off this part").clicked() {
                remove = true;
            }
        });
        if remove {
            self.rig_edit("Remove physics", now, |r| remove_physics(r, i));
            self.rigs.phys.sel = None;
            return;
        }
        if n != c || similar {
            self.rig_edit("Physics settings", now, |r| {
                if similar {
                    for o in r.physics.iter_mut().filter(|o| o.kind == n.kind) {
                        (o.stiff_root, o.stiff_tip, o.damping, o.gravity, o.wind, o.material) = (n.stiff_root, n.stiff_tip, n.damping, n.gravity, n.wind, n.material);
                    }
                }
                r.physics[i] = n.clone();
            });
        }
    }

    fn physics_colliders(&mut self, ui: &mut Ui, now: f64) {
        section_title(ui, "Things that stop the hair");
        ui.label(RichText::new("Hair and cloth do not go through these round shapes.").size(11.5).color(TEXT_DIM));
        ui.horizontal_wrapped(|ui| {
            if ui.button("Face and body from the tags").help("A circle over the face base and the torso").clicked() {
                let cs = self.colliders_from_tags();
                if cs.is_empty() {
                    self.rigs.phys.message = Some("Tag a face base or a torso first (Parts tab).".into());
                } else {
                    self.rig_edit("Colliders from the tags", now, |r| r.colliders = cs);
                }
            }
            if !self.draw.doc.rig.colliders.is_empty() && ui.button("Clear").help("Take everything away from this.").clicked() {
                self.rig_edit("Clear colliders", now, |r| r.colliders.clear());
            }
        });
        for c in &self.draw.doc.rig.colliders {
            ui.label(RichText::new(format!("{}  radius {:.0}", c.name, c.radius)).size(11.5).color(TEXT_DIM));
        }
    }

    /// Circles over the layers tagged face, torso and neck.
    pub fn colliders_from_tags(&self) -> Vec<Collider> {
        let rig = &self.draw.doc.rig;
        let mut out = Vec::new();
        for l in self.draw.doc.layers.iter() {
            let Some(part) = l.part else { continue };
            let radius_k = match part {
                parts::Part::Face => 0.45,
                parts::Part::Torso => 0.35,
                _ => continue,
            };
            let Some(m) = rig.mesh_of(l.id) else { continue };
            let (mut lo, mut hi) = ([f32::MAX; 2], [f32::MIN; 2]);
            for v in &m.verts {
                for k in 0..2 {
                    lo[k] = lo[k].min(v[k]);
                    hi[k] = hi[k].max(v[k]);
                }
            }
            if lo[0] > hi[0] {
                continue;
            }
            // the changer that moves the face goes with the circle
            out.push(Collider { name: l.name.clone(), center: [(lo[0] + hi[0]) / 2.0, (lo[1] + hi[1]) / 2.0], radius: ((hi[0] - lo[0]).min(hi[1] - lo[1]) * radius_k * 1.1).max(10.0), changer: m.parent });
        }
        out
    }

    /// Add physics to the picked parts.
    pub fn add_physics(&mut self, now: f64) {
        let (kind, mat) = (self.rigs.phys.kind, self.rigs.phys.material);
        let ids = self.rigs.selected.clone();
        self.add_physics_to(&ids, kind, mat, now);
    }

    pub fn add_physics_from_tags(&mut self, now: f64) {
        let mut hair = Vec::new();
        let mut acc = Vec::new();
        for l in self.draw.doc.layers.iter() {
            match l.part {
                Some(parts::Part::HairFront | parts::Part::HairSideL | parts::Part::HairSideR | parts::Part::HairBack) => hair.push(l.id),
                Some(parts::Part::Accessory) => acc.push(l.id),
                _ => {}
            }
        }
        if hair.is_empty() && acc.is_empty() {
            self.rigs.phys.message = Some("No part is tagged as hair or accessory. Tag them in the Parts tab.".into());
            return;
        }
        let mat = self.rigs.phys.material;
        self.add_physics_to(&hair, PKind::Hair, mat, now);
        self.add_physics_to(&acc, PKind::Hanging, mat, now);
    }

    pub fn add_physics_to(&mut self, ids: &[LayerId], kind: PKind, mat: Material, now: f64) {
        // a part needs a mesh to move
        let missing: Vec<LayerId> = ids.iter().copied().filter(|id| self.draw.doc.rig.mesh_of(*id).is_none()).collect();
        if !missing.is_empty() {
            self.make_meshes(missing, now);
        }
        let rig = self.draw.doc.rig.clone();
        let (dw, dh) = (self.draw.doc.w as f32, self.draw.doc.h as f32);
        let names: Vec<(LayerId, String)> = ids.iter().filter_map(|id| self.draw.doc.layers.iter().find(|l| l.id == *id).map(|l| (*id, l.name.clone()))).collect();
        let mut made = 0;
        self.rig_edit("Make it move", now, |r| {
            for (layer, name) in &names {
                let Some(mi) = r.meshes.iter().position(|m| m.layer == *layer) else { continue };
                if r.physics.iter().any(|c| r.meshes[mi].parent == Some(c.changer)) {
                    continue;
                }
                let (mut lo, mut hi) = ([f32::MAX; 2], [f32::MIN; 2]);
                for v in &r.meshes[mi].verts {
                    for k in 0..2 {
                        lo[k] = lo[k].min(v[k]);
                        hi[k] = hi[k].max(v[k]);
                    }
                }
                if lo[0] > hi[0] {
                    continue;
                }
                let rect = [lo[0], lo[1], hi[0], hi[1]];
                let vertical = hi[1] - lo[1] >= hi[0] - lo[0];
                let cx = (lo[0] + hi[0]) / 2.0;
                // fixed at the top for tall parts, toward the middle of the page for wide ones
                let root_at = if vertical { [cx, lo[1] - 10000.0] } else { [dw / 2.0, (lo[1] + hi[1]) / 2.0] };
                let length = (hi[1] - lo[1]).max(hi[0] - lo[0]);
                let n = physics::segments_for(kind, length, dh * 0.18);
                let old_parent = r.meshes[mi].parent;
                let line = physics::make_line(r, name, rect, root_at, n, old_parent);
                r.meshes[mi].parent = Some(line);
                r.physics.push(PhysChain::new(name, kind, mat, line));
                made += 1;
            }
        });
        let _ = rig;
        self.rigs.phys.sel = self.draw.doc.rig.physics.len().checked_sub(1).filter(|_| made > 0);
        self.rigs.phys.message = Some(if made > 0 { format!("{made} {} now. Press a test motion.", if made == 1 { "part moves" } else { "parts move" }) } else { "Nothing added: pick parts that have paint and do not move yet.".into() });
    }

    #[cfg(test)]
    pub fn test_phys(&mut self) -> &mut PhysUi {
        &mut self.rigs.phys
    }
}

/// Take the physics off one thing: the part goes back to the parent of its curve.
pub fn remove_physics(r: &mut Rig, i: usize) {
    if i >= r.physics.len() {
        return;
    }
    let line = r.physics.remove(i).changer;
    let up = r.changer(line).and_then(|c| c.parent);
    for m in r.meshes.iter_mut().filter(|m| m.parent == Some(line)) {
        m.parent = up;
    }
    for c in r.changers.iter_mut().filter(|c| c.parent == Some(line)) {
        c.parent = up;
    }
    r.bindings.retain(|b| b.changer != line);
    r.colliders.iter_mut().filter(|c| c.changer == Some(line)).for_each(|c| c.changer = up);
    r.changers.retain(|c| c.id != line);
    let _ = Kind::Rotation { pivot: [0.0, 0.0] };
}
