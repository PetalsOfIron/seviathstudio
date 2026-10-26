// SPDX-License-Identifier: GPL-3.0-or-later
// Draw tab: the gradient tool. Drag on the canvas to lay out a gradient; it stays live (drag the
// two points, change colors, shape or blend mode) until you apply it with Enter. Also the
// gradient map and the settings for painting with a gradient along a stroke.

use eframe::egui::{
    self, Color32, CornerRadius, Mesh, Pos2, Rect, Response, RichText, Sense, Shape, Stroke, StrokeKind, TextureHandle, TextureOptions, Ui, pos2,
    vec2,
};

use crate::widgets::HelpExt;
use super::{DrawState, checker};
use crate::blend::from_px;
use crate::doc::Blend;
use crate::gradient::{Gradient, Repeat, Shape as GShape, Stop, presets};
use crate::theme::*;
use crate::widgets::{self, section_title, slider_row};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Drag {
    P0,
    P1,
}

pub struct GradState {
    pub g: Gradient,
    /// The two points of the gradient being laid out (canvas pixels).
    pub edit: Option<(Pos2, Pos2)>,
    drag: Option<Drag>,
    pub sel_stop: usize,
    preview: Option<(PreviewKey, TextureHandle)>,
    /// Brushes take their color from the gradient along each stroke.
    pub brush_on: bool,
    /// How many pixels of stroke one pass of the gradient takes.
    pub brush_len: f32,
}

type PreviewKey = (Gradient, Pos2, Pos2, (i32, i32, i32, i32), u64);

impl Default for GradState {
    fn default() -> Self {
        Self { g: Gradient::default(), edit: None, drag: None, sel_stop: 0, preview: None, brush_on: false, brush_len: 600.0 }
    }
}

fn mesh_quad(tex: egui::TextureId, corners: [Pos2; 4]) -> Mesh {
    let mut m = Mesh::with_texture(tex);
    let uv = [pos2(0.0, 0.0), pos2(1.0, 0.0), pos2(1.0, 1.0), pos2(0.0, 1.0)];
    for (p, uv) in corners.iter().zip(uv) {
        m.vertices.push(egui::epaint::Vertex { pos: *p, uv, color: Color32::WHITE });
    }
    m.add_triangle(0, 1, 2);
    m.add_triangle(0, 2, 3);
    m
}

impl DrawState {
    pub fn gradient_editing(&self) -> bool {
        self.grad.edit.is_some()
    }

    /// The area gradients work on: the selection's box, or the whole canvas.
    fn work_bounds(&self) -> (i32, i32, i32, i32) {
        let full = (0, 0, self.doc.w as i32, self.doc.h as i32);
        match self.doc.selection.as_ref().and_then(|s| s.bounds()) {
            Some((x0, y0, x1, y1)) => (x0.max(0), y0.max(0), x1.min(full.2), y1.min(full.3)),
            None => full,
        }
    }

    pub fn apply_gradient(&mut self) {
        let Some((p0, p1)) = self.grad.edit.take() else { return };
        self.grad.drag = None;
        let li = self.active_idx();
        if !self.doc.layers[li].paintable() {
            self.notice = Some("Pick a pixel layer first.");
            return;
        }
        let bounds = self.work_bounds();
        let sel = self.doc.selection.clone();
        let name = self.doc.layers[li].name.clone();
        let keys = self.grad.g.apply(&mut self.doc.layers[li], p0, p1, sel.as_ref(), bounds);
        for k in keys {
            self.doc.mark_tile_dirty(k);
        }
        self.label_hint = Some(format!("Gradient · {name}"));
    }

    pub fn cancel_gradient(&mut self) {
        self.grad.edit = None;
        self.grad.drag = None;
    }

    /// Gradient map: the active layer's colors become gradient colors by lightness.
    pub fn gradient_map(&mut self) {
        let li = self.active_idx();
        if !self.doc.layers[li].paintable() {
            self.notice = Some("Pick a pixel layer first.");
            return;
        }
        let bounds = self.work_bounds();
        let sel = self.doc.selection.clone();
        let name = self.doc.layers[li].name.clone();
        let keys = self.grad.g.map_lightness(&mut self.doc.layers[li], sel.as_ref(), bounds);
        if keys.is_empty() {
            self.notice = Some("Nothing to recolor: this layer is empty here.");
            return;
        }
        for k in keys {
            self.doc.mark_tile_dirty(k);
        }
        self.label_hint = Some(format!("Gradient map · {name}"));
    }

    /// Pointer for the gradient tool. Returns true: the tool always owns the pointer.
    pub(super) fn grad_input(&mut self, resp: &Response, ctx: &egui::Context) -> bool {
        let (pressed, down, shift, latest) = ctx.input(|i| (i.pointer.primary_pressed(), i.pointer.primary_down(), i.modifiers.shift, i.pointer.latest_pos()));
        let over = resp.hovered() || resp.is_pointer_button_down_on();
        let view = self.view;
        if pressed && over {
            if let Some(sp) = latest {
                let near = |d: Pos2| view.doc_to_screen(d).distance(sp) < 12.0;
                match self.grad.edit {
                    Some((p0, _)) if near(p0) => self.grad.drag = Some(Drag::P0),
                    Some((_, p1)) if near(p1) => self.grad.drag = Some(Drag::P1),
                    _ => {
                        if !self.doc.layers[self.active_idx()].paintable() {
                            self.notice = Some("Pick a pixel layer first.");
                        } else {
                            let d = view.screen_to_doc(sp);
                            self.grad.edit = Some((d, d));
                            self.grad.drag = Some(Drag::P1);
                        }
                    }
                }
            }
        }
        if let (Some(which), Some(sp)) = (self.grad.drag, latest) {
            let mut d = view.screen_to_doc(sp);
            if let Some((p0, p1)) = self.grad.edit.as_mut() {
                if shift && which == Drag::P1 {
                    // keep the angle on 15 degree steps
                    let v = d - *p0;
                    let step = 15f32.to_radians();
                    let a = (v.y.atan2(v.x) / step).round() * step;
                    d = *p0 + vec2(a.cos(), a.sin()) * v.length();
                }
                match which {
                    Drag::P0 => *p0 = d,
                    Drag::P1 => *p1 = d,
                }
            }
            if !down {
                self.grad.drag = None;
                // a click without dragging is not a gradient
                if let Some((p0, p1)) = self.grad.edit {
                    if (p1 - p0).length() * view.zoom < 3.0 {
                        self.grad.edit = None;
                    }
                }
            }
        }
        true
    }

    /// The live gradient on the canvas: preview picture, the line between the two points, stop dots.
    pub(super) fn grad_overlay(&mut self, ctx: &egui::Context, p: &egui::Painter) {
        let Some((p0, p1)) = self.grad.edit else { return };
        let bounds = self.work_bounds();
        let sel_id = self.doc.selection.as_ref().map(|s| s.id).unwrap_or(0);
        let key: PreviewKey = (self.grad.g.clone(), p0, p1, bounds, sel_id);
        if self.grad.preview.as_ref().map(|(k, _)| *k != key).unwrap_or(true) {
            let img = self.grad.g.preview(p0, p1, bounds, self.doc.selection.as_ref(), 420);
            match &mut self.grad.preview {
                Some((k, t)) => {
                    t.set(img, TextureOptions::LINEAR);
                    *k = key;
                }
                None => self.grad.preview = Some((key, ctx.load_texture("gradient_preview", img, TextureOptions::LINEAR))),
            }
        }
        if let Some((_, tex)) = &self.grad.preview {
            let (x0, y0, x1, y1) = (bounds.0 as f32, bounds.1 as f32, bounds.2 as f32, bounds.3 as f32);
            let corners = [pos2(x0, y0), pos2(x1, y0), pos2(x1, y1), pos2(x0, y1)].map(|c| self.view.doc_to_screen(c));
            p.add(Shape::mesh(mesh_quad(tex.id(), corners)));
        }
        let (s0, s1) = (self.view.doc_to_screen(p0), self.view.doc_to_screen(p1));
        let dark = Stroke::new(3.0, Color32::from_black_alpha(170));
        let light = Stroke::new(1.3, Color32::WHITE);
        p.line_segment([s0, s1], dark);
        p.line_segment([s0, s1], light);
        // the colors along the line (for linear and mirrored; the others measure from the first point)
        for st in self.grad.g.sorted() {
            let c = s0 + (s1 - s0) * st.pos;
            let col = Color32::from_rgb(st.color[0], st.color[1], st.color[2]);
            p.circle_filled(c, 5.5, Color32::BLACK);
            p.circle_filled(c, 4.0, col);
        }
        for (pt, name) in [(s0, "first"), (s1, "second")] {
            p.circle_filled(pt, 8.0, Color32::from_black_alpha(160));
            p.circle_stroke(pt, 7.0, Stroke::new(2.0, if name == "first" { VENOM } else { GOLD }));
        }
    }

    // ---------- Options panel ----------

    pub(super) fn gradient_options(&mut self, ui: &mut Ui) {
        let hint = |ui: &mut Ui, t: &str| {
            ui.label(RichText::new(t).size(11.5).color(TEXT_DIM));
        };
        hint(ui, "Drag on the canvas. Drag the two round handles to adjust; Shift keeps the angle on 15 degree steps. Enter applies, Esc cancels.");
        ui.add_space(4.0);
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing = vec2(4.0, 4.0);
            for (s, label, about) in GShape::ALL {
                if widgets::chip(ui, label, self.grad.g.shape == s).help(about).clicked() {
                    self.grad.g.shape = s;
                }
            }
        });
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing = vec2(4.0, 4.0);
            for (r, label) in Repeat::ALL {
                if widgets::chip(ui, label, self.grad.g.repeat == r).help("Pick what the gradient does after its end.").clicked() {
                    self.grad.g.repeat = r;
                }
            }
        });
        ui.add_space(4.0);

        section_title(ui, "Colors");
        stop_bar(ui, &mut self.grad.g, &mut self.grad.sel_stop);
        let n = self.grad.g.stops.len();
        self.grad.sel_stop = self.grad.sel_stop.min(n.saturating_sub(1));
        let sel = self.grad.sel_stop;
        if let Some(s) = self.grad.g.stops.get_mut(sel) {
            ui.horizontal(|ui| {
                ui.label(RichText::new(format!("Color {} of {n}", sel + 1)).size(12.0).color(TEXT_DIM));
                egui::color_picker::color_edit_button_srgb(ui, &mut s.color);
                let mut pct = s.pos * 100.0;
                if ui.add(egui::DragValue::new(&mut pct).range(0.0..=100.0).suffix("%").max_decimals(0)).changed() {
                    s.pos = pct / 100.0;
                }
            });
            let mut a = s.alpha * 100.0;
            if slider_row(ui, "Opacity", &mut a, 0.0..=100.0, "%").changed() {
                s.alpha = a / 100.0;
            }
        }
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 4.0;
            if ui.add_enabled(n > 2, egui::Button::new("Remove color")).clicked() {
                self.grad.g.stops.remove(sel);
                self.grad.sel_stop = sel.saturating_sub(1);
            }
            if ui.add_enabled(n < 12, egui::Button::new("Add color")).help("Or click on the bar").clicked() {
                let t = self.grad.g.stops.get(sel).map(|s| (s.pos + 0.1).min(1.0)).unwrap_or(0.5);
                let c = self.grad.g.color_at(t);
                self.grad.g.stops.push(Stop { pos: t, color: [(c[0] * 255.0) as u8, (c[1] * 255.0) as u8, (c[2] * 255.0) as u8], alpha: c[3] });
                self.grad.sel_stop = self.grad.g.stops.len() - 1;
            }
            if ui.button("Reverse").help("Turn the order round.").clicked() {
                for s in &mut self.grad.g.stops {
                    s.pos = 1.0 - s.pos;
                }
            }
        });
        ui.add_space(2.0);
        ui.label(RichText::new("Ready-made").size(12.0).color(TEXT_DIM));
        let pc = [self.primary.r(), self.primary.g(), self.primary.b()];
        let sc = [self.secondary.r(), self.secondary.g(), self.secondary.b()];
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing = vec2(4.0, 4.0);
            for (name, stops) in presets(pc, sc) {
                if widgets::chip(ui, name, false).help("Use this.").clicked() {
                    self.grad.g.stops = stops;
                    self.grad.sel_stop = 0;
                }
            }
        });
        ui.add_space(4.0);
        ui.checkbox(&mut self.grad.g.smooth, "Clean color blending").help("Blends in OKLab, so the middle stays colorful instead of turning gray");
        ui.checkbox(&mut self.grad.g.dither, "Smooth out banding").help("A tiny noise so slow gradients show no visible steps");
        ui.add_space(2.0);
        let mut op = self.grad.g.opacity * 100.0;
        if slider_row(ui, "Opacity", &mut op, 1.0..=100.0, "%").changed() {
            self.grad.g.opacity = op / 100.0;
        }
        ui.horizontal(|ui| {
            ui.label(RichText::new("Mix").color(TEXT_DIM).size(12.5));
            egui::ComboBox::from_id_salt("grad_blend").selected_text(self.grad.g.blend.name()).width(150.0).height(380.0).show_ui(ui, |ui| {
                for b in Blend::ALL {
                    ui.selectable_value(&mut self.grad.g.blend, b, b.name()).help(b.about());
                }
            });
        });
        ui.label(RichText::new("Mix decides how the gradient meets the paint under it: Normal covers it, Multiply darkens, Overlay and Soft light tint it.").size(11.0).color(TEXT_DIM));
        ui.add_space(6.0);
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 4.0;
            let live = self.grad.edit.is_some();
            if ui.add_enabled(live, egui::Button::new(RichText::new("Apply").strong())).help("Enter").clicked() {
                self.apply_gradient();
            }
            if ui.add_enabled(live, egui::Button::new("Cancel")).help("Esc").clicked() {
                self.cancel_gradient();
            }
            if ui.button("Gradient map").help("Recolor this layer: dark parts get the first color, light parts the last").clicked() {
                self.gradient_map();
            }
        });
        ui.add_space(4.0);
        ui.label(RichText::new("The gradient also colors brush strokes: see Gradient color in the brush settings.").size(11.0).color(TEXT_DIM));
    }

    /// The small group in the brush settings.
    pub(super) fn gradient_brush_section(grad: &mut GradState, ui: &mut Ui) {
        ui.checkbox(&mut grad.brush_on, "Color the stroke with the gradient").help("Uses the colors set in the Gradient tool");
        if grad.brush_on {
            slider_row(ui, "One pass", &mut grad.brush_len, 50.0..=4000.0, " px");
            let sg = grad.g.clone();
            // the bar shows what a stroke will look like
            let (r, _) = ui.allocate_exact_size(vec2(ui.available_width(), 14.0), Sense::hover());
            checker(ui.painter(), r);
            let s = sg.along_stroke(grad.brush_len);
            let n = 48;
            let mut mesh = Mesh::default();
            for i in 0..=n {
                let t = i as f32 / n as f32;
                let (c, a) = s.at(t * grad.brush_len);
                let col = from_px([c[0] * a, c[1] * a, c[2] * a, a]);
                let x = r.left() + r.width() * t;
                let base = mesh.vertices.len() as u32;
                mesh.vertices.push(egui::epaint::Vertex { pos: pos2(x, r.top()), uv: egui::epaint::WHITE_UV, color: col });
                mesh.vertices.push(egui::epaint::Vertex { pos: pos2(x, r.bottom()), uv: egui::epaint::WHITE_UV, color: col });
                if i > 0 {
                    mesh.add_triangle(base - 2, base - 1, base);
                    mesh.add_triangle(base - 1, base, base + 1);
                }
            }
            ui.painter().add(Shape::mesh(mesh));
            ui.painter().rect_stroke(r, CornerRadius::same(3), Stroke::new(1.0, LINE), StrokeKind::Outside);
            ui.label(RichText::new("Every color holds for a while before it blends into the next one.").size(11.0).color(TEXT_DIM));
        }
    }
}

/// The gradient as a bar with a handle for each color. Click the bar to add one, drag handles to
/// move them, right-click a handle to remove it.
fn stop_bar(ui: &mut Ui, g: &mut Gradient, sel: &mut usize) {
    let w = ui.available_width();
    let (bar, bresp) = ui.allocate_exact_size(vec2(w, 26.0), Sense::click());
    let p = ui.painter();
    checker(p, bar);
    let n = 64;
    let mut mesh = Mesh::default();
    for i in 0..=n {
        let t = i as f32 / n as f32;
        let c = g.color_at(t);
        let col = from_px([c[0] * c[3], c[1] * c[3], c[2] * c[3], c[3]]);
        let x = bar.left() + bar.width() * t;
        let base = mesh.vertices.len() as u32;
        mesh.vertices.push(egui::epaint::Vertex { pos: pos2(x, bar.top()), uv: egui::epaint::WHITE_UV, color: col });
        mesh.vertices.push(egui::epaint::Vertex { pos: pos2(x, bar.bottom()), uv: egui::epaint::WHITE_UV, color: col });
        if i > 0 {
            mesh.add_triangle(base - 2, base - 1, base);
            mesh.add_triangle(base - 1, base, base + 1);
        }
    }
    p.add(Shape::mesh(mesh));
    p.rect_stroke(bar, CornerRadius::same(4), Stroke::new(1.0, LINE), StrokeKind::Outside);
    ui.add_space(14.0);
    let handles_y = bar.bottom() + 1.0;
    let mut remove = None;
    for i in 0..g.stops.len() {
        let x = bar.left() + bar.width() * g.stops[i].pos;
        let hr = Rect::from_min_size(pos2(x - 6.0, handles_y), vec2(12.0, 14.0));
        let r = ui.interact(hr, ui.id().with(("gstop", i)), Sense::click_and_drag());
        if r.clicked() || r.drag_started() {
            *sel = i;
        }
        if r.dragged() {
            if let Some(pp) = r.interact_pointer_pos() {
                g.stops[i].pos = ((pp.x - bar.left()) / bar.width()).clamp(0.0, 1.0);
            }
        }
        if r.secondary_clicked() && g.stops.len() > 2 {
            remove = Some(i);
        }
        let s = g.stops[i];
        let p = ui.painter();
        let on = *sel == i;
        p.rect_filled(hr, CornerRadius::same(3), Color32::from_rgb(s.color[0], s.color[1], s.color[2]));
        p.rect_stroke(hr, CornerRadius::same(3), Stroke::new(if on { 2.0 } else { 1.0 }, if on { VENOM } else { TEXT_DIM }), StrokeKind::Inside);
        r.help("Drag to move · right-click to remove");
    }
    if let Some(i) = remove {
        g.stops.remove(i);
        *sel = (*sel).min(g.stops.len() - 1);
    }
    if bresp.clicked() && g.stops.len() < 12 {
        if let Some(pp) = bresp.interact_pointer_pos() {
            let t = ((pp.x - bar.left()) / bar.width()).clamp(0.0, 1.0);
            let c = g.color_at(t);
            g.stops.push(Stop { pos: t, color: [(c[0] * 255.0).round() as u8, (c[1] * 255.0).round() as u8, (c[2] * 255.0).round() as u8], alpha: c[3] });
            *sel = g.stops.len() - 1;
        }
    }
}

#[cfg(test)]
impl DrawState {
    pub fn grad_mut(&mut self) -> &mut GradState {
        &mut self.grad
    }

    /// The pixel of layer `li` under a screen position, as unmultiplied RGBA.
    pub fn pixel_rgba(&self, li: usize, at: Pos2) -> [u8; 4] {
        let p = self.view.screen_to_doc(at);
        crate::files::layer_rgba(&self.doc.layers[li], p.x as i32, p.y as i32, 1, 1).get_pixel(0, 0).0
    }

    /// The strongest paint within 12 canvas pixels of a screen position (strokes are soft-edged).
    pub fn max_paint_color(&self, li: usize, at: Pos2) -> [u8; 4] {
        let p = self.view.screen_to_doc(at);
        let img = crate::files::layer_rgba(&self.doc.layers[li], p.x as i32 - 12, p.y as i32 - 12, 24, 24);
        img.pixels().map(|p| p.0).max_by_key(|p| p[3]).unwrap()
    }
}
