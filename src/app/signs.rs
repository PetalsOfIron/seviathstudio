// SPDX-License-Identifier: GPL-3.0-or-later
// The Signatures window: draw your signature on a pad (pressure and timing are kept), keep it in one of
// four slots, put it on the picture (stamp or written live), and set the rule that signs your exports.

use eframe::egui::{self, Align2, Color32, Event, Pos2, RichText, Sense, Stroke, Vec2, vec2};

use super::SeviathApp;
use crate::signature::{self, AutoSign, Corner, SigStroke, Signature, Slot};
use crate::theme::*;
use crate::vector::LineEnds;
use crate::widgets::{self, section_title};

pub const PAD: Vec2 = vec2(520.0, 200.0);

pub struct SignState {
    pub open: bool,
    slot: Slot,
    /// What is on the pad (pad pixels, times counted from the first touch).
    pub pad: Signature,
    saved: [bool; 4],
    loaded: bool,
    /// Time of the first touch of this signature.
    t0: Option<f64>,
    drawing: bool,
    clean: f32,
    message: Option<String>,
    pad_rect: egui::Rect,
}

impl Default for SignState {
    fn default() -> Self {
        Self { open: false, slot: Slot::Full, pad: Signature::default(), saved: [false; 4], loaded: false, t0: None, drawing: false, clean: 0.5, message: None, pad_rect: egui::Rect::NOTHING }
    }
}

impl SignState {
    fn refresh(&mut self) {
        let d = signature::dir();
        for (i, s) in Slot::ALL.iter().enumerate() {
            self.saved[i] = signature::load_in(&d, *s).is_some();
        }
        self.loaded = true;
    }

    fn pick(&mut self, slot: Slot) {
        self.slot = slot;
        let style = self.pad.style;
        self.pad = signature::load_in(&signature::dir(), slot).unwrap_or(Signature { style, ..Signature::default() });
        self.t0 = None;
        self.message = None;
    }
}

impl SeviathApp {
    #[cfg(test)]
    pub fn test_open_signatures(&mut self) {
        self.open_signatures();
    }

    #[cfg(test)]
    pub fn test_sign_pad(&self) -> (egui::Rect, Signature) {
        (self.signs.pad_rect, self.signs.pad.clone())
    }

    pub(super) fn open_signatures(&mut self) {
        self.signs.loaded = false;
        self.signs.open = true;
    }

    pub(super) fn signatures_window(&mut self, ctx: &egui::Context) {
        if !self.signs.open {
            return;
        }
        if !self.signs.loaded {
            self.signs.refresh();
            let slot = self.signs.slot;
            self.signs.pick(slot);
        }
        let mut open = true;
        let mut rule = self.draw.doc.meta.auto_sign;
        let mut stamp = false;
        let mut live = false;
        let mut save = false;
        let mut delete = false;
        let mouse_p = self.settings.mouse_pressure;
        egui::Window::new(RichText::new("Signatures").strong())
            .open(&mut open)
            .collapsible(false)
            .resizable(false)
            .pivot(Align2::CENTER_CENTER)
            .default_pos(ctx.content_rect().center())
            .show(ctx, |ui| {
                ui.set_width(PAD.x + 16.0);
                let s = &mut self.signs;
                ui.label(RichText::new("Sign on the pad below. Your pressure and your timing are kept, so it can be written again just like you did.").size(11.5).color(TEXT_DIM));
                ui.add_space(4.0);
                ui.horizontal_wrapped(|ui| {
                    ui.spacing_mut().item_spacing = vec2(4.0, 4.0);
                    for (i, sl) in Slot::ALL.iter().enumerate() {
                        let label = if s.saved[i] { format!("{} (saved)", sl.label()) } else { sl.label().to_owned() };
                        if widgets::chip(ui, &label, s.slot == *sl).clicked() {
                            s.pick(*sl);
                        }
                    }
                });
                pad(ui, s, ctx, &self.settings, mouse_p);
                ui.horizontal_wrapped(|ui| {
                    if ui.button("Undo line").clicked() {
                        s.pad.strokes.pop();
                    }
                    if ui.button("Clear").clicked() {
                        s.pad.strokes.clear();
                        s.t0 = None;
                    }
                    ui.separator();
                    ui.label(RichText::new("Clean").size(12.0).color(TEXT_DIM));
                    ui.add(egui::Slider::new(&mut s.clean, 0.0..=1.0).show_value(false)).on_hover_text("Smooths the wobble and evens out the weight, but keeps your shapes");
                    if ui.button("Clean it").clicked() && !s.pad.is_empty() {
                        s.pad = s.pad.cleaned(s.clean);
                    }
                });
                ui.horizontal_wrapped(|ui| {
                    ui.label(RichText::new("Color").size(12.0).color(TEXT_DIM));
                    egui::color_picker::color_edit_button_srgb(ui, &mut s.pad.style.color);
                    ui.label(RichText::new("Line").size(12.0).color(TEXT_DIM));
                    ui.add(egui::Slider::new(&mut s.pad.style.size, 1.0..=30.0).suffix(" px"));
                    for (e, n) in [(LineEnds::Round, "Round"), (LineEnds::Taper, "Pointed"), (LineEnds::Flat, "Flat")] {
                        if widgets::chip(ui, n, s.pad.style.ends == e).clicked() {
                            s.pad.style.ends = e;
                        }
                    }
                });
                ui.horizontal(|ui| {
                    if ui.button(RichText::new("Save in this slot").strong()).clicked() {
                        save = true;
                    }
                    if ui.button("Delete this slot").clicked() {
                        delete = true;
                    }
                    if let Some(m) = &s.message {
                        ui.label(RichText::new(m).size(11.5).color(TEXT_DIM));
                    }
                });

                section_title(ui, "Put it on the picture");
                ui.horizontal_wrapped(|ui| {
                    ui.spacing_mut().item_spacing = vec2(4.0, 4.0);
                    ui.label(RichText::new("Place").size(12.0).color(TEXT_DIM));
                    for c in Corner::ALL {
                        if widgets::chip(ui, c.label(), rule.corner == c).clicked() {
                            rule.corner = c;
                        }
                    }
                });
                size_row(ui, &mut rule);
                ui.horizontal(|ui| {
                    let ok = !s.pad.is_empty();
                    if ui.add_enabled(ok, egui::Button::new("Stamp it")).on_hover_text("A new layer you can still move, scale and turn").clicked() {
                        stamp = true;
                    }
                    if ui.add_enabled(ok, egui::Button::new("Write it live")).on_hover_text("It is written on the picture again, with your timing (Esc: finish at once)").clicked() {
                        live = true;
                    }
                });

                section_title(ui, "Sign every export");
                ui.checkbox(&mut rule.on, "Add my signature to exports of this project").on_hover_text("PNG, JPEG, WebP, PSD and OpenRaster. Turn it off here for one export.");
                ui.horizontal_wrapped(|ui| {
                    ui.spacing_mut().item_spacing = vec2(4.0, 4.0);
                    ui.label(RichText::new("Which").size(12.0).color(TEXT_DIM));
                    for (i, sl) in Slot::ALL.iter().enumerate() {
                        let label = if s.saved[i] { sl.label().to_owned() } else { format!("{} (empty)", sl.label()) };
                        if widgets::chip(ui, &label, rule.slot == *sl).clicked() {
                            rule.slot = *sl;
                        }
                    }
                });
                ui.label(RichText::new("The place and size above are used for exports too. Only saved slots are used.").size(11.0).color(TEXT_DIM));
            });
        if save {
            let sig = self.signs.pad.clone().normalized();
            self.signs.message = Some(if sig.is_empty() {
                "Draw something first.".into()
            } else {
                match signature::save_in(&signature::dir(), self.signs.slot, &sig) {
                    Ok(()) => {
                        self.signs.pad = sig;
                        self.signs.refresh();
                        "Saved.".into()
                    }
                    Err(e) => format!("Couldn't save: {e}"),
                }
            });
        }
        if delete {
            signature::delete_in(&signature::dir(), self.signs.slot);
            self.signs.pad.strokes.clear();
            self.signs.refresh();
            self.signs.message = Some("Deleted.".into());
        }
        let rule = rule.sanitized();
        if rule != self.draw.doc.meta.auto_sign {
            self.draw.apply_external("Signing rule", |d| d.doc.meta.auto_sign = rule);
        }
        if stamp || live {
            let sig = self.signs.pad.clone().normalized();
            if stamp {
                self.draw.stamp_signature(&sig, &rule);
            } else {
                self.draw.start_sign_replay(sig, &rule);
            }
            self.tab = crate::tabs::Tab::Draw;
        }
        self.signs.open = open;
    }
}

fn size_row(ui: &mut egui::Ui, rule: &mut AutoSign) {
    ui.horizontal_wrapped(|ui| {
        ui.label(RichText::new("Width").size(12.0).color(TEXT_DIM));
        ui.add(egui::Slider::new(&mut rule.size, 2.0..=60.0).suffix(" %"));
        ui.label(RichText::new("Opacity").size(12.0).color(TEXT_DIM));
        ui.add(egui::Slider::new(&mut rule.opacity, 5.0..=100.0).suffix(" %"));
        ui.label(RichText::new("From edge").size(12.0).color(TEXT_DIM));
        ui.add(egui::Slider::new(&mut rule.margin, 0.0..=15.0).suffix(" %"));
    });
}

/// The drawing pad. Lines are kept in pad pixels.
fn pad(ui: &mut egui::Ui, s: &mut SignState, ctx: &egui::Context, settings: &crate::settings::Settings, mouse_p: f32) {
    let (rect, resp) = ui.allocate_exact_size(PAD, Sense::click_and_drag());
    let origin = rect.min;
    s.pad_rect = rect;
    let p = ui.painter_at(rect);
    p.rect_filled(rect, 4.0, Color32::from_rgb(246, 244, 238));
    let base = rect.min.y + rect.height() * 0.72;
    p.line_segment([Pos2::new(rect.min.x + 14.0, base), Pos2::new(rect.max.x - 14.0, base)], Stroke::new(1.0, Color32::from_rgb(214, 210, 200)));
    let now = ctx.input(|i| i.time);
    if resp.drag_started() {
        s.drawing = true;
        s.pad.strokes.push(SigStroke::default());
        s.t0.get_or_insert(now);
        if let Some(o) = ctx.input(|i| i.pointer.press_origin()) {
            add_point(s, o - origin, mouse_p, now);
        }
    }
    if s.drawing {
        let (events, dt) = ctx.input(|i| (i.events.clone(), i.stable_dt as f64));
        let mut pts: Vec<(Pos2, f32)> = Vec::new();
        for e in &events {
            match e {
                Event::Touch { pos, force: Some(f), .. } => pts.push((*pos, settings.pressure(*f))),
                Event::PointerMoved(pos) => pts.push((*pos, mouse_p)),
                _ => {}
            }
        }
        // a pen sends both: keep the ones with pressure
        if pts.iter().any(|(_, p)| (*p - mouse_p).abs() > 1e-4) {
            let pen: Vec<_> = events.iter().filter_map(|e| if let Event::Touch { pos, force: Some(f), .. } = e { Some((*pos, settings.pressure(*f))) } else { None }).collect();
            if !pen.is_empty() {
                pts = pen;
            }
        }
        let n = pts.len().max(1) as f64;
        for (k, (pos, pr)) in pts.into_iter().enumerate() {
            let at = now - dt * (n - 1.0 - k as f64) / n;
            add_point(s, pos - origin, pr, at);
        }
        if resp.drag_stopped() || !ctx.input(|i| i.pointer.primary_down()) {
            s.drawing = false;
            // a single dot is not a line
            if s.pad.strokes.last().is_some_and(|l| l.pts.len() < 2) {
                s.pad.strokes.pop();
            }
        }
    }
    let st = s.pad.style;
    let c = Color32::from_rgb(st.color[0], st.color[1], st.color[2]);
    for line in &s.pad.strokes {
        for w in line.pts.windows(2) {
            let pr = (w[0][2] + w[1][2]) / 2.0;
            let width = st.size * (st.min_size + (1.0 - st.min_size) * pr).max(0.1);
            p.line_segment([origin + vec2(w[0][0], w[0][1]), origin + vec2(w[1][0], w[1][1])], Stroke::new(width, c));
        }
    }
    if s.pad.strokes.is_empty() {
        p.text(rect.center(), Align2::CENTER_CENTER, "Sign here", egui::FontId::proportional(18.0), Color32::from_rgb(190, 186, 176));
    }
}

fn add_point(s: &mut SignState, at: Vec2, pressure: f32, now: f64) {
    let t = (now - s.t0.unwrap_or(now)).max(0.0) as f32;
    if let Some(l) = s.pad.strokes.last_mut() {
        if l.pts.last().is_some_and(|q| (q[0] - at.x).abs() < 0.3 && (q[1] - at.y).abs() < 0.3) {
            return;
        }
        l.pts.push([at.x, at.y, pressure.clamp(0.0, 1.0), t]);
    }
}
