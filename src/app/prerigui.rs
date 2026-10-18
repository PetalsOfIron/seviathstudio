// SPDX-License-Identifier: GPL-3.0-or-later
// The pre-rig improver window: goes through the problems one at a time, zoomed in, with before and after.
// Nothing changes until you press Fix, and every fix is an undo step.

use std::collections::HashSet;

use eframe::egui::{self, Align2, Color32, ColorImage, Rect, RichText, Stroke, TextureHandle, TextureOptions, vec2};

use super::SeviathApp;
use crate::doc::{Layer, LayerId};
use crate::parts;
use crate::prerig::{self, Kind, Problem, Sens};
use crate::tabs::Tab;
use crate::theme::*;

pub struct PreRig {
    pub open: bool,
    pub sens: f32,
    pub only_tagged: bool,
    pub problems: Vec<Problem>,
    pub at: usize,
    ignored: HashSet<(LayerId, Kind, i32, i32)>,
    checked: bool,
    before: Option<TextureHandle>,
    after: Option<TextureHandle>,
    shown: Option<(usize, u64, usize)>,
    /// The square of the page that is shown: x, y, side.
    crop: (i32, i32, i32),
    message: Option<String>,
}

impl Default for PreRig {
    fn default() -> Self {
        Self { open: false, sens: 0.4, only_tagged: true, problems: Vec::new(), at: 0, ignored: HashSet::new(), checked: false, before: None, after: None, shown: None, crop: (0, 0, 32), message: None }
    }
}

/// A square of one layer over a dark checkerboard, so see-through and light edges can be seen.
fn crop_image(l: &Layer, x0: i32, y0: i32, side: i32) -> ColorImage {
    let mut img = ColorImage::filled([side as usize, side as usize], Color32::TRANSPARENT);
    for y in 0..side {
        for x in 0..side {
            let back = if ((x + y) / 4) % 2 == 0 { 34u8 } else { 52u8 };
            let c = prerig::px_get(l, x0 + x, y0 + y);
            let a = c.a() as u32;
            let mix = |v: u8, b: u8| (v as u32 + b as u32 * (255 - a) / 255).min(255) as u8;
            img.pixels[(y * side + x) as usize] = Color32::from_rgb(mix(c.r(), back), mix(c.g(), back), mix(c.b(), back));
        }
    }
    img
}

impl SeviathApp {
    pub(super) fn open_prerig(&mut self) {
        self.prerig.open = true;
        if !self.prerig.checked {
            self.prerig_check();
        }
    }

    #[cfg(test)]
    pub fn test_prerig(&mut self) -> &mut PreRig {
        &mut self.prerig
    }

    #[cfg(test)]
    pub fn test_open_prerig(&mut self) {
        self.open_prerig();
    }

    fn prerig_layers(&self) -> Vec<LayerId> {
        let any_tag = self.draw.doc.layers.iter().any(|l| l.part.is_some());
        self.draw.doc.layers.iter().filter(|l| parts::is_part_layer(l) && !l.tiles.is_empty() && (!self.prerig.only_tagged || !any_tag || l.part.is_some())).map(|l| l.id).collect()
    }

    /// Look at the drawing (this only looks).
    pub(super) fn prerig_check(&mut self) {
        let ids = self.prerig_layers();
        let d = &self.draw.doc;
        let found = prerig::detect(&d.layers, &ids, d.w, d.h, Sens::from_slider(self.prerig.sens));
        let p = &mut self.prerig;
        p.problems = found.into_iter().filter(|x| !p.ignored.contains(&x.key())).collect();
        p.at = 0;
        p.checked = true;
        p.shown = None;
        p.message = Some(if p.problems.is_empty() { "Nothing found.".into() } else { format!("{} things found in {} layers.", p.problems.len(), p.problems.iter().map(|x| x.layer).collect::<HashSet<_>>().len()) });
    }

    /// After a fix: look at that layer again.
    fn prerig_recheck_layer(&mut self, id: LayerId) {
        let d = &self.draw.doc;
        let Some(l) = d.layers.iter().find(|l| l.id == id) else {
            self.prerig.problems.retain(|p| p.layer != id);
            return;
        };
        let fresh = prerig::detect_layer(l, d.w, d.h, Sens::from_slider(self.prerig.sens));
        let p = &mut self.prerig;
        let pos = p.problems.iter().position(|x| x.layer == id).unwrap_or(p.at.min(p.problems.len()));
        p.problems.retain(|x| x.layer != id);
        let fresh: Vec<Problem> = fresh.into_iter().filter(|x| !p.ignored.contains(&x.key())).collect();
        for (i, f) in fresh.into_iter().enumerate() {
            p.problems.insert((pos + i).min(p.problems.len()), f);
        }
        p.at = p.at.min(p.problems.len().saturating_sub(1));
        p.shown = None;
    }

    fn prerig_fix(&mut self, which: Vec<usize>, now: f64, what: &str) {
        let mut by_layer: Vec<(LayerId, Vec<Problem>)> = Vec::new();
        for i in which {
            if let Some(p) = self.prerig.problems.get(i).cloned() {
                match by_layer.iter_mut().find(|(l, _)| *l == p.layer) {
                    Some((_, v)) => v.push(p),
                    None => by_layer.push((p.layer, vec![p])),
                }
            }
        }
        let mut total = 0;
        for (id, list) in by_layer {
            let name = self.draw.doc.layers.iter().find(|l| l.id == id).map(|l| l.name.clone()).unwrap_or_default();
            let mut n = 0;
            self.parts_edit(&format!("Fix · {what} · {name}"), now, |d| {
                if let Some(l) = d.layers.iter_mut().find(|l| l.id == id) {
                    for p in &list {
                        n += prerig::apply(l, p);
                    }
                }
            });
            total += n;
            self.prerig_recheck_layer(id);
        }
        self.prerig.message = Some(format!("Fixed {total} pixels."));
    }

    fn prerig_pictures(&mut self, ctx: &egui::Context) {
        let p = &self.prerig;
        let Some(prob) = p.problems.get(p.at).cloned() else {
            self.prerig.before = None;
            self.prerig.after = None;
            return;
        };
        let key = (p.at, self.draw.doc.revision, p.problems.len());
        if p.shown == Some(key) && p.before.is_some() {
            return;
        }
        let Some(layer) = self.draw.doc.layers.iter().find(|l| l.id == prob.layer).cloned() else { return };
        let (fx0, fy0, fx1, fy1) = prob.focus;
        let side = ((fx1 - fx0).max(fy1 - fy0) + 16).clamp(24, 128);
        let (cx, cy) = ((fx0 + fx1) / 2, (fy0 + fy1) / 2);
        let (x0, y0) = (cx - side / 2, cy - side / 2);
        let mut fixed = layer.clone();
        prerig::apply(&mut fixed, &prob);
        let opts = TextureOptions::NEAREST;
        let before = ctx.load_texture("prerig_before", crop_image(&layer, x0, y0, side), opts);
        let after = ctx.load_texture("prerig_after", crop_image(&fixed, x0, y0, side), opts);
        let p = &mut self.prerig;
        p.before = Some(before);
        p.after = Some(after);
        p.crop = (x0, y0, side);
        p.shown = Some(key);
    }

    pub(super) fn prerig_window(&mut self, ctx: &egui::Context) {
        if !self.prerig.open {
            return;
        }
        self.prerig_pictures(ctx);
        let now = ctx.input(|i| i.time);
        let mut open = true;
        let (mut check, mut fix_one, mut ignore, mut manual, mut fix_layer_kind, mut fix_all) = (false, false, false, false, false, false);
        let mut step: i32 = 0;
        egui::Window::new(RichText::new("Pre-rig improver").strong())
            .open(&mut open)
            .collapsible(false)
            .resizable(false)
            .pivot(Align2::CENTER_CENTER)
            .default_pos(ctx.content_rect().center())
            .show(ctx, |ui| {
                ui.set_width(780.0);
                ui.label(RichText::new("Looks for things that go wrong when a part is deformed. Nothing changes until you press Fix, and every fix can be undone.").size(11.5).color(TEXT_DIM));
                ui.add_space(4.0);
                ui.horizontal_wrapped(|ui| {
                    ui.label(RichText::new("Sensitivity").size(12.0).color(TEXT_DIM));
                    let r = ui.add(egui::Slider::new(&mut self.prerig.sens, 0.0..=1.0).show_value(false)).on_hover_text("Low: only the obvious. High: also small details like freckles");
                    ui.label(RichText::new(if self.prerig.sens < 0.3 { "only the obvious" } else if self.prerig.sens < 0.7 { "normal" } else { "small details too" }).size(11.5).color(TEXT_DIM));
                    ui.checkbox(&mut self.prerig.only_tagged, "Only tagged layers");
                    if ui.button("Check the drawing").clicked() || r.drag_stopped() {
                        check = true;
                    }
                });
                if let Some(m) = &self.prerig.message {
                    ui.label(RichText::new(m).size(11.5).color(TEXT_DIM));
                }
                let n = self.prerig.problems.len();
                if n == 0 {
                    return;
                }
                let at = self.prerig.at.min(n - 1);
                let prob = self.prerig.problems[at].clone();
                let lname = self.draw.doc.layers.iter().find(|l| l.id == prob.layer).map(|l| l.name.clone()).unwrap_or_default();
                ui.separator();
                ui.horizontal(|ui| {
                    ui.label(RichText::new(format!("{}  in  {}", prob.kind.label(), lname)).strong().size(15.0));
                    ui.label(RichText::new(format!("{} of {n}  ·  {} pixels", at + 1, prob.px.len())).size(12.0).color(TEXT_DIM));
                });
                ui.label(RichText::new(prob.kind.explain()).size(12.0).color(TEXT_DIM));
                ui.add_space(4.0);
                ui.horizontal(|ui| {
                    for (title, tex, mark) in [("Before", &self.prerig.before, true), ("After", &self.prerig.after, false)] {
                        ui.vertical(|ui| {
                            ui.label(RichText::new(title).size(12.0).color(TEXT_DIM));
                            let (r, _) = ui.allocate_exact_size(vec2(370.0, 370.0), egui::Sense::hover());
                            if let Some(t) = tex {
                                ui.painter().image(t.id(), r, Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)), Color32::WHITE);
                            }
                            if mark {
                                // the pixels that will change, outlined
                                let (x0, y0, side) = self.prerig.crop;
                                let k = r.width() / side as f32;
                                for (x, y) in prob.px.iter().take(4000) {
                                    let rr = Rect::from_min_size(r.min + vec2((x - x0) as f32 * k, (y - y0) as f32 * k), vec2(k, k));
                                    if r.contains_rect(rr) {
                                        ui.painter().rect_stroke(rr, 0.0, Stroke::new(1.0, Color32::from_rgb(255, 70, 70)), egui::StrokeKind::Inside);
                                    }
                                }
                            }
                        });
                    }
                });
                ui.horizontal_wrapped(|ui| {
                    if ui.button("Previous").clicked() {
                        step = -1;
                    }
                    if ui.button("Next").clicked() {
                        step = 1;
                    }
                    ui.separator();
                    if ui.button(RichText::new("Fix").strong()).clicked() {
                        fix_one = true;
                    }
                    if ui.button("Ignore").on_hover_text("Leave it as it is and don't mention it again").clicked() {
                        ignore = true;
                    }
                    if ui.button("Fix manually").on_hover_text("Go to this place in the Draw tab").clicked() {
                        manual = true;
                    }
                    ui.separator();
                    if ui.button(format!("Fix all “{}” here", prob.kind.label().to_lowercase())).clicked() {
                        fix_layer_kind = true;
                    }
                    if ui.button("Fix everything").clicked() {
                        fix_all = true;
                    }
                });
            });
        if check {
            self.prerig_check();
        }
        let n = self.prerig.problems.len();
        if n > 0 {
            let at = self.prerig.at.min(n - 1);
            if step != 0 {
                self.prerig.at = (at as i32 + step).rem_euclid(n as i32) as usize;
            }
            if fix_one {
                self.prerig_fix(vec![at], now, &self.prerig.problems[at].kind.label().to_lowercase());
            }
            if fix_layer_kind {
                let (l, k) = (self.prerig.problems[at].layer, self.prerig.problems[at].kind);
                let which: Vec<usize> = self.prerig.problems.iter().enumerate().filter(|(_, p)| p.layer == l && p.kind == k).map(|(i, _)| i).collect();
                self.prerig_fix(which, now, &k.label().to_lowercase());
            }
            if fix_all {
                let which: Vec<usize> = (0..n).collect();
                self.prerig_fix(which, now, "everything");
            }
            if ignore {
                let key = self.prerig.problems[at].key();
                self.prerig.ignored.insert(key);
                self.prerig.problems.remove(at);
                self.prerig.at = at.min(self.prerig.problems.len().saturating_sub(1));
                self.prerig.shown = None;
            }
            if manual {
                let p = self.prerig.problems[at].clone();
                let (cx, cy) = ((p.focus.0 + p.focus.2) as f32 / 2.0, (p.focus.1 + p.focus.3) as f32 / 2.0);
                self.draw.look_at(p.layer, cx, cy, 8.0);
                self.tab = Tab::Draw;
            }
        }
        self.prerig.open = open;
    }
}
