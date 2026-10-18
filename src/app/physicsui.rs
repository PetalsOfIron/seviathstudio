// SPDX-License-Identifier: GPL-3.0-or-later
// The Physics tab: pick parts, say what they are (hair, cloth, earring…), and they swing and lag when the head or body
// moves. The preview is the Rig tab's picture; a test panel moves the head so you can see it.

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
    pub offs: HashMap<Id, Vec<[f32; 2]>>,
    pub sel: Option<usize>,
    pub kind: PKind,
    pub material: Material,
    /// The test motion running: index and seconds so far.
    pub motion: Option<(usize, f32)>,
    pub message: Option<String>,
}

impl Default for PhysUi {
    fn default() -> Self {
        Self { on: true, sim: PhysSim::default(), offs: HashMap::new(), sel: None, kind: PKind::Hair, material: Material::Normal, motion: None, message: None }
    }
}

impl super::rigtab::RigState {
    /// Add what the physics does to a pose (after the sliders and IK).
    pub fn add_phys(&self, pose: &mut crate::rig::Pose) {
        if !self.phys.on {
            return;
        }
        for (id, offs) in &self.phys.offs {
            let e = pose.line.entry(*id).or_insert_with(|| vec![[0.0, 0.0]; offs.len()]);
            if e.len() != offs.len() {
                continue;
            }
            for (a, o) in e.iter_mut().zip(offs) {
                a[0] += o[0];
                a[1] += o[1];
            }
        }
    }
}

impl SeviathApp {
    /// Move the simulation on by `dt` seconds (also runs the test motion).
    pub fn phys_advance(&mut self, dt: f32) {
        let rig = self.draw.doc.rig.clone();
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
        if rig.physics.is_empty() || !self.rigs.phys.on {
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
        egui::Panel::left("phys_layers").default_size(230.0).size_range(190.0..=380.0).resizable(true).frame(egui::Frame::new().fill(PANEL).inner_margin(egui::Margin::same(10))).show(ui, |ui| {
            section_title(ui, "Parts");
            ui.label(RichText::new("Pick the parts that should move.").size(11.5).color(TEXT_DIM));
            egui::ScrollArea::vertical().id_salt("phys_parts").show(ui, |ui| {
                for l in self.draw.doc.layers.iter().filter(|l| parts::is_part_layer(l)) {
                    let on = self.rigs.selected.contains(&l.id);
                    let (rect, resp) = ui.allocate_exact_size(vec2(ui.available_width(), 24.0), Sense::click());
                    if on {
                        ui.painter().rect_filled(rect, 4.0, HOVER);
                    }
                    let dot = l.part.map(|p| p.color()).unwrap_or(Color32::from_gray(70));
                    ui.painter().circle_filled(rect.left_center() + vec2(10.0, 0.0), 5.0, dot);
                    ui.painter().text(rect.left_center() + vec2(22.0, 0.0), egui::Align2::LEFT_CENTER, &l.name, egui::FontId::proportional(13.0), TEXT);
                    if let Some(c) = self.draw.doc.rig.physics.iter().find(|c| self.draw.doc.rig.mesh_of(l.id).and_then(|m| m.parent) == Some(c.changer)) {
                        ui.painter().text(rect.right_center() - vec2(6.0, 0.0), egui::Align2::RIGHT_CENTER, c.kind.label(), egui::FontId::proportional(11.0), VENOM);
                    }
                    if resp.clicked() {
                        let m = ctx.input(|i| i.modifiers);
                        picked = Some((l.id, m.command || m.ctrl, m.shift));
                    }
                }
            });
        });
        egui::Panel::right("phys_tools").default_size(330.0).size_range(280.0..=500.0).resizable(true).frame(egui::Frame::new().fill(PANEL).inner_margin(egui::Margin::same(10))).show(ui, |ui| {
            egui::ScrollArea::vertical().id_salt("phys_right").show(ui, |ui| self.physics_side(ui, now));
        });
        egui::CentralPanel::default().frame(egui::Frame::new().fill(Color32::from_rgb(14, 20, 18))).show(ui, |ui| self.rig_view(ui, now, &mut picked));
        if let Some((id, c, s)) = picked.filter(|p| p.0 != u64::MAX) {
            self.rig_select(id, c, s);
            // picking a part that has physics shows its numbers
            let rig = &self.draw.doc.rig;
            self.rigs.phys.sel = rig.mesh_of(id).and_then(|m| m.parent).and_then(|p| rig.physics.iter().position(|c| c.changer == p));
        }
    }

    fn physics_side(&mut self, ui: &mut Ui, now: f64) {
        section_title(ui, "Physics");
        ui.horizontal_wrapped(|ui| {
            ui.checkbox(&mut self.rigs.phys.on, "Physics on").on_hover_text("Off: the picture only follows the sliders");
            if ui.button("Reset").on_hover_text("Things hang still again").clicked() {
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
                if widgets::chip(ui, k.label(), self.rigs.phys.kind == k).on_hover_text(k.about()).clicked() {
                    self.rigs.phys.kind = k;
                }
            }
        });
        ui.horizontal(|ui| {
            ui.label(RichText::new("Material").size(12.0).color(TEXT_DIM));
            for m in Material::ALL {
                if widgets::chip(ui, m.label(), self.rigs.phys.material == m).on_hover_text("Light cloth catches the wind. Heavy cloth hangs and moves slowly.").clicked() {
                    self.rigs.phys.material = m;
                }
            }
        });
        ui.horizontal_wrapped(|ui| {
            if ui.button(RichText::new("Make it move").strong()).on_hover_text("Adds a curve along each picked part and makes it swing").clicked() {
                self.add_physics(now);
            }
            if ui.button("Hair and accessories from the tags").on_hover_text("Every part tagged as hair gets hair physics, every accessory a hanging one").clicked() {
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
                    if widgets::chip(ui, &format!("{} ({})", c.name, c.kind.label()), self.rigs.phys.sel == Some(i)).clicked() {
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
                if widgets::chip(ui, k.label(), w.wind == k).clicked() {
                    w.wind = k;
                }
            }
        });
        ui.horizontal(|ui| {
            ui.label(RichText::new("Wind comes").size(12.0).color(TEXT_DIM));
            if widgets::chip(ui, "From the left", w.wind_dir > 0.0).clicked() {
                w.wind_dir = 1.0;
            }
            if widgets::chip(ui, "From the right", w.wind_dir < 0.0).clicked() {
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
                    if ui.button(*name).on_hover_text(if has { "Moves the head" } else { "Needs the face rig first" }).clicked() {
                        self.rigs.phys.motion = Some((i, 0.0));
                    }
                });
            }
        });
    }

    fn physics_chain_settings(&mut self, ui: &mut Ui, now: f64, i: usize) {
        let Some(c) = self.draw.doc.rig.physics.get(i).cloned() else { return };
        section_title(ui, &format!("{}: {}", c.name, c.kind.label()));
        let mut n = c.clone();
        let row = |ui: &mut Ui, label: &str, tip: &str, v: &mut f32, hi: f32| {
            ui.horizontal(|ui| {
                ui.label(RichText::new(label).size(12.0).color(TEXT_DIM)).on_hover_text(tip);
                ui.add(egui::Slider::new(v, 0.0..=hi).max_decimals(2));
            });
        };
        ui.checkbox(&mut n.enabled, "This one moves");
        row(ui, "Stiff at the root", "How strongly the base is held in place", &mut n.stiff_root, 1.0);
        row(ui, "Stiff at the tip", "How strongly the end is held in place: low is floppy", &mut n.stiff_tip, 1.0);
        row(ui, "Slows down", "Higher: stops swinging sooner", &mut n.damping, 0.98);
        row(ui, "Gravity", "How much it hangs down", &mut n.gravity, 3.0);
        row(ui, "Wind", "How much the wind moves it", &mut n.wind, 3.0);
        ui.horizontal(|ui| {
            ui.label(RichText::new("Material").size(12.0).color(TEXT_DIM));
            for m in Material::ALL {
                if widgets::chip(ui, m.label(), n.material == m).clicked() {
                    n = PhysChain { name: c.name.clone(), enabled: c.enabled, ..PhysChain::new(&c.name, c.kind, m, c.changer) };
                }
            }
        });
        let mut similar = false;
        let mut remove = false;
        ui.horizontal_wrapped(|ui| {
            if ui.button("Same for all of this type").on_hover_text("Copies these numbers to every other thing of the same type").clicked() {
                similar = true;
            }
            if ui.button("Remove").on_hover_text("Takes the physics off this part").clicked() {
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
            if ui.button("Face and body from the tags").on_hover_text("A circle over the face base and the torso").clicked() {
                let cs = self.colliders_from_tags();
                if cs.is_empty() {
                    self.rigs.phys.message = Some("Tag a face base or a torso first (Parts tab).".into());
                } else {
                    self.rig_edit("Colliders from the tags", now, |r| r.colliders = cs);
                }
            }
            if !self.draw.doc.rig.colliders.is_empty() && ui.button("Clear").clicked() {
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
