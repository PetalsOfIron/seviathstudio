// SPDX-License-Identifier: GPL-3.0-or-later
// Draw tab: the color randomizer window. Pick what you are coloring and a harmony, lock the
// swatches you like, reroll the rest, then apply to the selection (or the whole layer).

use crate::widgets::HelpExt;
use eframe::egui::{self, Align2, Color32, CornerRadius, RichText, Sense, Stroke, StrokeKind, vec2};

use super::DrawState;
use crate::fill;
use crate::gradient::Gradient;
use crate::icons::{self, Icon};
use crate::randomizer::{self, Harmony, Intent};
use crate::select::Dense;
use crate::theme::*;
use crate::widgets::{self, section_title};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ApplyMode {
    Flat,
    Gradient,
    Speckle,
    Recolor,
}

impl ApplyMode {
    const ALL: [(ApplyMode, &'static str, &'static str); 4] = [
        (ApplyMode::Flat, "Flat", "Fill with the chosen swatch."),
        (ApplyMode::Gradient, "Gradient", "A smooth blend through all the swatches."),
        (ApplyMode::Speckle, "Speckle", "Soft patches and fine grain in all the swatches, like real pigment."),
        (ApplyMode::Recolor, "Recolor flats", "Each flat area of your drawing gets a swatch. Lineart and shading stay."),
    ];
}

pub struct RandState {
    pub open: bool,
    intent: Intent,
    harmony: Harmony,
    pub seed: u32,
    count: usize,
    swatches: Vec<[u8; 3]>,
    locked: Vec<bool>,
    chosen: usize,
    mode: ApplyMode,
    /// gradient direction in degrees, 0 = top to bottom
    angle: f32,
    /// speckle patch size in pixels
    cell: f32,
    dirty: bool,
}

impl Default for RandState {
    fn default() -> Self {
        Self {
            open: false,
            intent: Intent::Anything,
            harmony: Harmony::Free,
            seed: 1,
            count: 5,
            swatches: Vec::new(),
            locked: Vec::new(),
            chosen: 0,
            mode: ApplyMode::Flat,
            angle: 0.0,
            cell: 40.0,
            dirty: true,
        }
    }
}

impl RandState {
    fn regenerate(&mut self) {
        self.locked.resize(self.count, false);
        let keep: Vec<Option<[u8; 3]>> = (0..self.count).map(|i| (self.locked[i] && i < self.swatches.len()).then(|| self.swatches[i])).collect();
        self.swatches = randomizer::generate(self.intent, self.harmony, self.seed, self.count, &keep);
        self.chosen = self.chosen.min(self.count - 1);
        self.dirty = false;
    }

    /// A new seed that is not just the next number.
    fn reroll(&mut self) {
        self.seed = self.seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223) % 1_000_000;
        self.dirty = true;
    }
}

impl DrawState {
    pub(super) fn randomizer_window(&mut self, ctx: &egui::Context) {
        if !self.rand.open {
            return;
        }
        if self.rand.dirty || self.rand.swatches.len() != self.rand.count {
            self.rand.regenerate();
        }
        let mut open = true;
        let mut apply = false;
        let mut save_palette = false;
        let mut use_main: Option<usize> = None;
        egui::Window::new(RichText::new("Color randomizer").strong())
            .open(&mut open)
            .collapsible(false)
            .resizable(false)
            .pivot(Align2::RIGHT_TOP)
            .default_pos(ctx.content_rect().right_top() + vec2(-310.0, 70.0))
            .show(ctx, |ui| {
                ui.set_width(380.0);
                let r = &mut self.rand;
                let mut changed = false;
                section_title(ui, "What are you coloring?");
                ui.horizontal_wrapped(|ui| {
                    ui.spacing_mut().item_spacing = vec2(4.0, 4.0);
                    for (i, label, about) in Intent::ALL {
                        if widgets::chip(ui, label, r.intent == i).help(about).clicked() {
                            r.intent = i;
                            changed = true;
                        }
                    }
                });
                ui.add_space(2.0);
                section_title(ui, "Harmony");
                ui.horizontal_wrapped(|ui| {
                    ui.spacing_mut().item_spacing = vec2(4.0, 4.0);
                    for (h, label, about) in Harmony::ALL {
                        if widgets::chip(ui, label, r.harmony == h).help(about).clicked() {
                            r.harmony = h;
                            changed = true;
                        }
                    }
                });
                ui.add_space(4.0);
                section_title(ui, "Swatches");
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 6.0;
                    let w = ((ui.available_width() - 6.0 * (r.count as f32 - 1.0)) / r.count as f32).min(64.0);
                    for i in 0..r.count {
                        let c = r.swatches[i];
                        let (rect, resp) = ui.allocate_exact_size(vec2(w, 64.0), Sense::click());
                        let p = ui.painter();
                        p.rect_filled(rect, CornerRadius::same(8), Color32::from_rgb(c[0], c[1], c[2]));
                        let on = r.chosen == i;
                        p.rect_stroke(rect, CornerRadius::same(8), Stroke::new(if on { 2.5 } else { 1.0 }, if on { VENOM } else { LINE }), StrokeKind::Inside);
                        // lock button in the corner
                        let lock = egui::Rect::from_min_size(rect.right_top() + vec2(-22.0, 4.0), vec2(18.0, 18.0));
                        let lresp = ui.interact(lock, ui.id().with(("rlock", i)), Sense::click());
                        let lp = ui.painter();
                        lp.rect_filled(lock, CornerRadius::same(5), Color32::from_black_alpha(if r.locked[i] { 170 } else { 60 }));
                        icons::paint(lp, lock.shrink(3.0), Icon::Lock, if r.locked[i] { GOLD } else { Color32::from_white_alpha(150) });
                        if lresp.help("Keep this color when you reroll").clicked() {
                            r.locked[i] = !r.locked[i];
                        }
                        let resp = resp.help(format!("#{:02X}{:02X}{:02X} · click to choose it for a flat fill · right-click: use as main color", c[0], c[1], c[2]));
                        if resp.clicked() {
                            r.chosen = i;
                        }
                        resp.context_menu(|ui| {
                            if ui.button("Use as main color").help("Use this as the color you paint with.").clicked() {
                                use_main = Some(i);
                                ui.close();
                            }
                        });
                    }
                });
                ui.horizontal(|ui| {
                    if ui.button(RichText::new("Reroll").strong()).help("New colors for every swatch that is not locked").clicked() {
                        r.reroll();
                    }
                    ui.label(RichText::new("Seed").color(TEXT_DIM).size(12.0));
                    if ui.add(egui::DragValue::new(&mut r.seed).range(0..=999_999)).help("The same seed always gives the same colors").changed() {
                        changed = true;
                    }
                    ui.label(RichText::new("Swatches").color(TEXT_DIM).size(12.0));
                    if ui.add(egui::DragValue::new(&mut r.count).range(3..=8)).changed() {
                        changed = true;
                    }
                });
                if changed {
                    r.dirty = true;
                }
                ui.add_space(4.0);
                section_title(ui, "Apply");
                ui.horizontal_wrapped(|ui| {
                    ui.spacing_mut().item_spacing = vec2(4.0, 4.0);
                    for (m, label, about) in ApplyMode::ALL {
                        if widgets::chip(ui, label, r.mode == m).help(about).clicked() {
                            r.mode = m;
                        }
                    }
                });
                match r.mode {
                    ApplyMode::Flat => {
                        ui.label(RichText::new("Fills with the swatch you clicked.").size(11.5).color(TEXT_DIM));
                    }
                    ApplyMode::Gradient => {
                        ui.horizontal(|ui| {
                            ui.label("Direction");
                            ui.add(egui::Slider::new(&mut r.angle, -180.0..=180.0).suffix("°").max_decimals(0));
                        });
                    }
                    ApplyMode::Speckle => {
                        ui.horizontal(|ui| {
                            ui.label("Patch size");
                            ui.add(egui::Slider::new(&mut r.cell, 6.0..=300.0).logarithmic(true).suffix(" px").max_decimals(0));
                        });
                    }
                    ApplyMode::Recolor => {
                        ui.label(RichText::new("Works on the layer you are on. Dark lines stay as they are.").size(11.5).color(TEXT_DIM));
                    }
                }
                let target = if self.doc.selection.is_some() { "the selection" } else { "the whole layer" };
                ui.add_space(4.0);
                ui.horizontal(|ui| {
                    if ui.button(RichText::new(format!("Apply to {target}")).strong()).help("Use this on the picked layer or selection.").clicked() {
                        apply = true;
                    }
                    if ui.button("Save as palette").help("Add these colors to your swatches").clicked() {
                        save_palette = true;
                    }
                });
            });
        self.rand.open = open;
        if let Some(i) = use_main {
            let c = self.rand.swatches[i];
            self.primary = Color32::from_rgb(c[0], c[1], c[2]);
        }
        if save_palette {
            let colors = self.rand.swatches.clone();
            for c in colors {
                let col = Color32::from_rgb(c[0], c[1], c[2]);
                if !self.swatches.contains(&col) {
                    if self.swatches.len() >= self.set.swatches_max {
                        self.swatches.remove(0);
                    }
                    self.swatches.push(col);
                }
            }
            self.notice = Some("Added to your swatches.");
        }
        if apply {
            self.randomizer_apply();
        }
    }

    fn randomizer_apply(&mut self) {
        let Some(li) = self.paintable_layer() else { return };
        let (w, h) = (self.doc.w as i32, self.doc.h as i32);
        let bounds = match self.doc.selection.as_ref().and_then(|s| s.bounds()) {
            Some((x0, y0, x1, y1)) => (x0.max(0), y0.max(0), x1.min(w), y1.min(h)),
            None => (0, 0, w, h),
        };
        let sel = self.doc.selection.clone();
        let colors = self.rand.swatches.clone();
        let name = self.doc.layers[li].name.clone();
        let seed = self.rand.seed;
        let layer = &mut self.doc.layers[li];
        let (keys, label) = match self.rand.mode {
            ApplyMode::Flat => {
                let (bw, bh) = ((bounds.2 - bounds.0).max(0) as usize, (bounds.3 - bounds.1).max(0) as usize);
                let d = match &sel {
                    Some(s) => s.dense(bounds.0, bounds.1, bw, bh),
                    None => Dense { x0: 0, y0: 0, w: bw, h: bh, px: vec![255; bw * bh] },
                };
                let c = colors[self.rand.chosen.min(colors.len() - 1)];
                (fill::paint(layer, &d, Color32::from_rgb(c[0], c[1], c[2]), 1.0, None, false), "Random color fill")
            }
            ApplyMode::Gradient => {
                let g: Gradient = randomizer::palette_gradient(&colors);
                let a = self.rand.angle.to_radians();
                let dir = vec2(a.sin(), a.cos());
                let (bw, bh) = ((bounds.2 - bounds.0) as f32, (bounds.3 - bounds.1) as f32);
                let half = (bw * dir.x.abs() + bh * dir.y.abs()) / 2.0;
                let c = egui::pos2((bounds.0 + bounds.2) as f32 / 2.0, (bounds.1 + bounds.3) as f32 / 2.0);
                (g.apply(layer, c - dir * half, c + dir * half, sel.as_ref(), bounds), "Random color gradient")
            }
            ApplyMode::Speckle => (randomizer::speckle(layer, &colors, sel.as_ref(), bounds, self.rand.cell, seed), "Random color speckle"),
            ApplyMode::Recolor => match randomizer::recolor_flats(layer, &colors, sel.as_ref(), bounds, seed) {
                Ok(k) => (k, "Recolor flats"),
                Err(e) => {
                    self.tools.notice = Some(e);
                    return;
                }
            },
        };
        if keys.is_empty() {
            self.notice = Some("Nothing changed: there was nothing to color here.");
            return;
        }
        for k in keys {
            self.doc.mark_tile_dirty(k);
        }
        self.label_hint = Some(format!("{label} · {name}"));
    }
}

#[cfg(test)]
impl DrawState {
    /// Open the window with an intent (index into Intent::ALL) and a seed.
    pub fn test_rand_open(&mut self, intent: usize, seed: u32) {
        self.rand.open = true;
        self.rand.intent = Intent::ALL[intent].0;
        self.rand.seed = seed;
        self.rand.dirty = true;
    }

    pub fn rand_swatches(&self) -> Vec<[u8; 3]> {
        self.rand.swatches.clone()
    }

    /// Apply with a mode: 0 flat, 1 gradient, 2 speckle, 3 recolor.
    pub fn test_rand_apply(&mut self, mode: usize) {
        self.rand.mode = ApplyMode::ALL[mode].0;
        self.randomizer_apply();
    }
}
