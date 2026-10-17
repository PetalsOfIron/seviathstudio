// SPDX-License-Identifier: GPL-3.0-or-later
// The Effects part of the layer panel (shadow, glow, outline, overlays on a pixel layer) and the
// settings of an adjustment layer. Everything here is non-destructive: see effects.rs.

use eframe::egui::{self, Color32, RichText, Ui};

use super::DrawState;
use super::filters_ui::params_editor;
use crate::doc::LayerKind;
use crate::effects::{self, Adjust, Effect, FxKind, Place};
use crate::filters;
use crate::light::Light;
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

    /// New depth layer above the selected layer: paint white for near, black for far.
    pub fn add_depth_layer(&mut self) {
        let mut l = self.doc.new_layer("Depth", LayerKind::Raster);
        l.depth = true;
        self.active = self.doc.insert_layer(l, self.active);
        self.doc.mark_all_dirty();
    }

    /// New light layer above the selected layer.
    pub fn add_light_layer(&mut self) {
        let mut l = self.doc.new_layer("Light", LayerKind::Light);
        l.light = Some(Light::default());
        self.active = self.doc.insert_layer(l, self.active);
        self.doc.mark_all_dirty();
    }

    /// The light's position: drag the dot inside the disc (middle = straight above, edge = low).
    fn light_pad(ui: &mut Ui, dir: &mut [f32; 2]) {
        let size = 120.0;
        let (rect, resp) = ui.allocate_exact_size(egui::vec2(size, size), egui::Sense::click_and_drag());
        let (c, r) = (rect.center(), size / 2.0 - 4.0);
        let p = ui.painter_at(rect);
        p.circle_filled(c, r, BG);
        p.circle_stroke(c, r, egui::Stroke::new(1.5, LINE));
        p.circle_stroke(c, r * 0.5, egui::Stroke::new(1.0, LINE));
        p.line_segment([c - egui::vec2(r, 0.0), c + egui::vec2(r, 0.0)], egui::Stroke::new(1.0, LINE));
        p.line_segment([c - egui::vec2(0.0, r), c + egui::vec2(0.0, r)], egui::Stroke::new(1.0, LINE));
        if let Some(pt) = resp.interact_pointer_pos() {
            if resp.dragged() || resp.clicked() {
                *dir = Light::clamp_dir([(pt.x - c.x) / r, (pt.y - c.y) / r]);
            }
        }
        let at = c + egui::vec2(dir[0], dir[1]) * r;
        p.line_segment([c, at], egui::Stroke::new(1.5, GOLD.gamma_multiply(0.6)));
        p.circle_filled(at, 7.0, GOLD);
        p.circle_stroke(at, 7.0, egui::Stroke::new(1.5, Color32::BLACK.gamma_multiply(0.5)));
        resp.on_hover_text("Drag the sun. The middle is straight above the drawing, the edge is low and from the side.");
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
            LayerKind::Light => {
                ui.add_space(6.0);
                section_title(ui, "Light");
                let has_depth = {
                    let at = i;
                    self.doc.layers[at + 1..].iter().any(|d| d.depth && d.visible)
                };
                if !has_depth {
                    ui.label(RichText::new("Nothing to shade yet. Add a Depth layer below this one and paint it: white is near, black is far.").size(11.5).color(GOLD));
                }
                let before = self.doc.layers[i].light.clone().unwrap_or_default();
                let mut l = before.clone();
                ui.horizontal(|ui| {
                    Self::light_pad(ui, &mut l.dir);
                    ui.vertical(|ui| {
                        color(ui, "Shadow", &mut l.shadow);
                        color(ui, "Highlight", &mut l.highlight);
                        if ui.small_button("Reset").clicked() {
                            l = Light::default();
                        }
                    });
                });
                percent(ui, "Strength", &mut l.strength);
                slider(ui, "Depth", &mut l.depth, 1.0..=crate::light::MAX_DEPTH_PX, " px");
                slider(ui, "Soften", &mut l.soften, 0.0..=crate::light::MAX_SOFTEN, " px");
                slider(ui, "Contrast", &mut l.contrast, 0.2..=8.0, "");
                ui.label(RichText::new("Shades the Depth layers below it in the same folder. It is a layer like any other: hide it, fade it or change its blend mode.").size(11.0).color(TEXT_DIM));
                if l != before {
                    self.doc.layers[i].light = Some(l.sanitized());
                    self.doc.layers[i].rev += 1;
                    self.doc.mark_all_dirty();
                }
            }
            LayerKind::Raster if self.doc.layers[i].depth => {
                ui.add_space(6.0);
                section_title(ui, "Depth");
                ui.label(RichText::new("Paint white where things are near, black where they are far. Soft brushes make round shapes.").size(11.5).color(TEXT_DIM));
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 4.0;
                    for (name, v) in [("Near", 255u8), ("Middle", 128), ("Far", 0)] {
                        if widgets::chip(ui, name, false).on_hover_text("Set the paint color to this depth").clicked() {
                            self.primary = Color32::from_gray(v);
                        }
                    }
                });
                ui.checkbox(&mut self.doc.layers[i].depth, "This is a depth layer").on_hover_text("Off: it becomes a normal layer again");
                ui.label(RichText::new("Hidden in the drawing unless it is selected. Add a Light layer above it to see the shading.").size(11.0).color(TEXT_DIM));
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
