// SPDX-License-Identifier: GPL-3.0-or-later
// The Effects part of the layer panel (shadow, glow, outline, overlays on a pixel layer) and the
// settings of an adjustment layer. Everything here is non-destructive: see effects.rs.

use eframe::egui::{self, RichText, Ui};

use super::DrawState;
use super::filters_ui::params_editor;
use crate::doc::LayerKind;
use crate::effects::{self, Adjust, Effect, FxKind, Place};
use crate::filters;
use crate::theme::*;
use crate::widgets::{self, section_title};

fn row(ui: &mut Ui, label: &str, add: impl FnOnce(&mut Ui)) {
    ui.horizontal(|ui| {
        ui.label(RichText::new(label).size(12.5).color(TEXT_DIM));
        add(ui);
    });
}

fn slider(ui: &mut Ui, label: &str, v: &mut f32, range: std::ops::RangeInclusive<f32>, suffix: &str) {
    row(ui, label, |ui| {
        ui.add(egui::Slider::new(v, range).suffix(suffix).max_decimals(2));
    });
}

fn percent(ui: &mut Ui, label: &str, v: &mut f32) {
    row(ui, label, |ui| {
        let mut p = *v * 100.0;
        if ui.add(egui::Slider::new(&mut p, 0.0..=100.0).suffix("%").max_decimals(0)).changed() {
            *v = p / 100.0;
        }
    });
}

fn color(ui: &mut Ui, label: &str, c: &mut [u8; 3]) {
    row(ui, label, |ui| {
        egui::color_picker::color_edit_button_srgb(ui, c);
    });
}

/// The controls of one effect.
fn effect_controls(ui: &mut Ui, e: &mut FxKind) {
    match e {
        FxKind::Shadow { color: c, opacity, dx, dy, blur, spread } => {
            color(ui, "Color", c);
            percent(ui, "Strength", opacity);
            slider(ui, "Right", dx, -effects::SHADOW_OFFSET..=effects::SHADOW_OFFSET, " px");
            slider(ui, "Down", dy, -effects::SHADOW_OFFSET..=effects::SHADOW_OFFSET, " px");
            slider(ui, "Soft", blur, 0.0..=effects::SHADOW_BLUR, " px");
            percent(ui, "Grow", spread);
        }
        FxKind::Glow { color: c, opacity, size, spread } => {
            color(ui, "Color", c);
            percent(ui, "Strength", opacity);
            slider(ui, "Size", size, 0.0..=effects::GLOW_SIZE, " px");
            percent(ui, "Grow", spread);
        }
        FxKind::Outline { color: c, opacity, width, place } => {
            color(ui, "Color", c);
            percent(ui, "Strength", opacity);
            slider(ui, "Width", width, 0.0..=effects::OUTLINE_WIDTH, " px");
            row(ui, "Where", |ui| {
                for (p, name) in Place::ALL {
                    if widgets::chip(ui, name, *place == p).clicked() {
                        *place = p;
                    }
                }
            });
        }
        FxKind::Color { color: c, opacity } => {
            color(ui, "Color", c);
            percent(ui, "Strength", opacity);
        }
        FxKind::Gradient { from, to, angle, opacity } => {
            color(ui, "From", from);
            color(ui, "To", to);
            slider(ui, "Angle", angle, -180.0..=180.0, " deg");
            percent(ui, "Strength", opacity);
        }
    }
}

impl DrawState {
    /// New adjustment layer above the selected layer.
    pub fn add_adjustment(&mut self, id: &str) {
        let Some(a) = Adjust::new(id) else { return };
        let mut l = self.doc.new_layer(a.label(), LayerKind::Adjust);
        l.adjust = Some(a);
        self.active = self.doc.insert_layer(l, self.active);
        self.doc.mark_all_dirty();
    }

    /// Effects of the selected pixel layer, or the settings of the selected adjustment layer.
    pub(super) fn effects_section(&mut self, ui: &mut Ui) {
        let Some(i) = self.doc.index_of(self.active) else { return };
        match self.doc.layers[i].kind {
            LayerKind::Adjust => {
                ui.add_space(6.0);
                section_title(ui, "Adjustment");
                let layer = &mut self.doc.layers[i];
                let Some(adj) = layer.adjust.clone() else { return };
                let mut now = adj.clone();
                ui.horizontal_wrapped(|ui| {
                    ui.spacing_mut().item_spacing = egui::vec2(4.0, 4.0);
                    for fid in effects::ADJUSTMENTS {
                        let Some(d) = filters::find(fid) else { continue };
                        if widgets::chip(ui, d.label, now.id == fid).on_hover_text(d.about).clicked() && now.id != fid {
                            now = Adjust::new(fid).unwrap_or_else(|| now.clone());
                        }
                    }
                });
                if let Some(def) = filters::find(now.id) {
                    ui.label(RichText::new(def.about).size(11.5).color(TEXT_DIM));
                    params_editor(ui, def, &mut now.params);
                }
                ui.label(RichText::new("Changes everything below it in the same folder. Hide or delete it to go back.").size(11.0).color(TEXT_DIM));
                if now != adj {
                    // the layer's name follows the adjustment while it still has the default name
                    if layer.name == adj.label() {
                        layer.name = now.label().to_owned();
                    }
                    layer.adjust = Some(now);
                    self.doc.mark_all_dirty();
                }
            }
            LayerKind::Raster | LayerKind::Vector => {
                ui.add_space(6.0);
                let before = self.doc.layers[i].fx.clone();
                let mut fx = before.clone();
                ui.horizontal(|ui| {
                    section_title(ui, "Effects");
                    ui.menu_button("Add effect", |ui| {
                        for p in Effect::presets() {
                            if ui.button(p.name()).clicked() && fx.len() < effects::MAX_EFFECTS {
                                fx.push(p);
                                ui.close();
                            }
                        }
                    });
                });
                if fx.is_empty() {
                    ui.label(RichText::new("Shadow, glow, outline and color overlays that never touch your paint.").size(11.5).color(TEXT_DIM));
                }
                let mut remove = None;
                let mut up = None;
                for (n, e) in fx.iter_mut().enumerate() {
                    egui::Frame::new().fill(CARD).corner_radius(6).inner_margin(6).show(ui, |ui| {
                        ui.horizontal(|ui| {
                            let name = e.name();
                            ui.checkbox(&mut e.on, RichText::new(name).strong());
                            if n > 0 && ui.small_button("Up").on_hover_text("Draw this effect earlier").clicked() {
                                up = Some(n);
                            }
                            if ui.small_button("Remove").clicked() {
                                remove = Some(n);
                            }
                        });
                        if e.on {
                            effect_controls(ui, &mut e.kind);
                        }
                    });
                }
                if let Some(n) = remove {
                    fx.remove(n);
                }
                if let Some(n) = up {
                    fx.swap(n, n - 1);
                }
                if fx != before {
                    self.doc.layers[i].fx = fx;
                    self.doc.layers[i].rev += 1;
                    self.doc.mark_all_dirty();
                }
            }
            _ => {}
        }
    }
}
