// SPDX-License-Identifier: GPL-3.0-or-later
// Draw tab: the Filter window (blurs, stylize, color, line art, color adjustments) with a live
// preview on a small copy, and the check-view button (black & white, inverted, color blindness).

use std::sync::Arc;
use std::sync::mpsc::{Receiver, TryRecvError, channel};

use eframe::egui::{self, Align2, Color32, ColorImage, CornerRadius, RichText, Sense, Stroke, StrokeKind, TextureHandle, TextureOptions, Ui, pos2, vec2};

use super::{DrawState, checker};
use crate::check::CheckView;
use crate::doc::{LayerId, LayerKind};
use crate::filters::{self, FILTERS, FilterDef, Group, Kind, Params};
use crate::icons::Icon;
use crate::theme::*;
use crate::widgets::{self, icon_button, section_title};

#[derive(Clone, Copy, PartialEq, Eq)]
enum View {
    Result,
    Original,
}

/// How far around the drawing the filter may read and write, for filters that spread paint.
const MARGIN: i32 = 200;

pub struct FilterUi {
    pub open: bool,
    group: Group,
    def: &'static FilterDef,
    params: Params,
    layer: LayerId,
    layer_name: String,
    /// The part of the layer being filtered (unmultiplied pixels) and where it sits on the canvas.
    src: Option<Arc<image::RgbaImage>>,
    origin: (i32, i32),
    small: Option<(Arc<image::RgbaImage>, f32)>,
    last: Option<(&'static str, Params)>,
    due: f64,
    job: Option<Receiver<image::RgbaImage>>,
    result: Option<image::RgbaImage>,
    preview: Option<TextureHandle>,
    view: View,
    redraw: bool,
    applying: Option<Receiver<image::RgbaImage>>,
    pub check_open: bool,
}

impl Default for FilterUi {
    fn default() -> Self {
        let def = &FILTERS[0];
        Self {
            open: false,
            group: def.group,
            def,
            params: Params::defaults(def),
            layer: 0,
            layer_name: String::new(),
            src: None,
            origin: (0, 0),
            small: None,
            last: None,
            due: 0.0,
            job: None,
            result: None,
            preview: None,
            view: View::Result,
            redraw: false,
            applying: None,
            check_open: false,
        }
    }
}

fn spawn(src: Arc<image::RgbaImage>, id: &'static str, params: Params, scale: f32) -> Receiver<image::RgbaImage> {
    let (tx, rx) = channel();
    std::thread::spawn(move || {
        let _ = tx.send(filters::run(id, &src, &params, scale));
    });
    rx
}

impl DrawState {
    /// Open the Filter window for the active pixel layer, with `id` selected.
    pub fn open_filter(&mut self, id: &str) {
        let Some(def) = filters::find(id) else { return };
        let li = self.active_idx();
        let layer = &self.doc.layers[li];
        if layer.kind != LayerKind::Raster {
            self.notice = Some("Filters work on pixel layers. Pick one, or trace a vector layer to pixels first.");
            return;
        }
        if layer.locked {
            self.notice = Some("This layer is locked.");
            return;
        }
        let (dw, dh) = (self.doc.w as i32, self.doc.h as i32);
        let Some(mut b) = crate::files::layer_bounds(layer, self.doc.w, self.doc.h) else {
            self.notice = Some("This layer is empty: nothing to filter.");
            return;
        };
        if let Some(sb) = self.doc.selection.as_ref().and_then(|s| s.bounds()) {
            b = (b.0.max(sb.0), b.1.max(sb.1), b.2.min(sb.2), b.3.min(sb.3));
            if b.0 >= b.2 || b.1 >= b.3 {
                self.notice = Some("Nothing of this layer is inside the selection.");
                return;
            }
        }
        let m = if matches!(def.id, "blur" | "motion" | "glow") { MARGIN } else { 0 };
        let (x0, y0, x1, y1) = ((b.0 - m).max(0), (b.1 - m).max(0), (b.2 + m).min(dw), (b.3 + m).min(dh));
        if (x1 - x0) as u64 * (y1 - y0) as u64 > 100_000_000 {
            self.notice = Some("This is too big to filter in one go. Select a part of it first.");
            return;
        }
        let img = crate::files::layer_rgba(layer, x0, y0, (x1 - x0) as u32, (y1 - y0) as u32);
        let f = &mut self.filter;
        f.open = true;
        f.layer = layer.id;
        f.layer_name = layer.name.clone();
        f.src = Some(Arc::new(img));
        f.origin = (x0, y0);
        f.small = None;
        f.def = def;
        f.group = def.group;
        f.params = Params::defaults(def);
        f.last = None;
        f.result = None;
        f.preview = None;
        f.job = None;
        f.applying = None;
        f.view = View::Result;
    }

    pub(super) fn filter_window(&mut self, ctx: &egui::Context) {
        if !self.filter.open {
            return;
        }
        let now = ctx.input(|i| i.time);
        let mut open = true;
        let mut apply = false;
        egui::Window::new(RichText::new(format!("Filter · {}", self.filter.layer_name)).strong())
            .open(&mut open)
            .collapsible(false)
            .resizable(false)
            .pivot(Align2::CENTER_CENTER)
            .default_pos(ctx.content_rect().center())
            .show(ctx, |ui| {
                let f = &mut self.filter;
                let busy = f.applying.is_some();
                ui.horizontal_top(|ui| {
                    ui.vertical(|ui| {
                        ui.set_width(300.0);
                        ui.add_enabled_ui(!busy, |ui| {
                            ui.horizontal_wrapped(|ui| {
                                ui.spacing_mut().item_spacing = vec2(4.0, 4.0);
                                for (g, label) in Group::ALL {
                                    if widgets::chip(ui, label, f.group == g).clicked() {
                                        f.group = g;
                                        if f.def.group != g {
                                            if let Some(d) = FILTERS.iter().find(|d| d.group == g) {
                                                f.def = d;
                                                f.params = Params::defaults(d);
                                            }
                                        }
                                    }
                                }
                            });
                            ui.horizontal_wrapped(|ui| {
                                ui.spacing_mut().item_spacing = vec2(4.0, 4.0);
                                for d in FILTERS.iter().filter(|d| d.group == f.group) {
                                    if widgets::chip(ui, d.label, f.def.id == d.id).on_hover_text(d.about).clicked() && f.def.id != d.id {
                                        f.def = d;
                                        f.params = Params::defaults(d);
                                    }
                                }
                            });
                            ui.label(RichText::new(f.def.about).size(11.5).color(TEXT_DIM));
                            ui.add_space(4.0);
                            section_title(ui, f.def.label);
                            params_editor(ui, f.def, &mut f.params);
                        });
                    });
                    ui.vertical(|ui| {
                        ui.horizontal(|ui| {
                            ui.spacing_mut().item_spacing.x = 4.0;
                            for (v, label) in [(View::Result, "Result"), (View::Original, "Original")] {
                                if widgets::chip(ui, label, f.view == v).clicked() && f.view != v {
                                    f.view = v;
                                    f.redraw = true;
                                }
                            }
                        });
                        let size = vec2(460.0, 400.0);
                        let (rect, _) = ui.allocate_exact_size(size, Sense::hover());
                        ui.painter().rect_filled(rect, CornerRadius::same(6), BG);
                        if let Some(tex) = &f.preview {
                            let s = tex.size_vec2();
                            let k = (size.x / s.x).min(size.y / s.y);
                            let r = egui::Rect::from_center_size(rect.center(), s * k);
                            checker(ui.painter(), r);
                            ui.painter().image(tex.id(), r, egui::Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)), Color32::WHITE);
                        }
                        ui.label(RichText::new("The preview is a small copy of the layer; applying uses full detail.").size(11.0).color(TEXT_DIM));
                    });
                });
                ui.separator();
                ui.horizontal(|ui| {
                    let busy = self.filter.applying.is_some();
                    if busy {
                        ui.spinner();
                        ui.label("Applying…");
                    }
                    if ui.add_enabled(!busy, egui::Button::new("Cancel")).clicked() {
                        self.filter.open = false;
                    }
                    let name = self.filter.def.label;
                    if ui.add_enabled(!busy, egui::Button::new(RichText::new(format!("Apply {name}")).strong())).clicked() {
                        apply = true;
                    }
                });
            });
        if !open && self.filter.applying.is_none() {
            self.filter.open = false;
        }
        let f = &mut self.filter;
        if !f.open {
            return;
        }

        // settings changed: filter the small copy again a moment later
        let key = (f.def.id, f.params.clone());
        if f.last.as_ref() != Some(&key) {
            f.last = Some(key);
            f.due = now + 0.18;
            ctx.request_repaint_after_secs(0.25);
        }
        if f.due > 0.0 && now >= f.due && f.job.is_none() {
            f.due = 0.0;
            if let Some(src) = f.src.clone() {
                if f.small.is_none() {
                    let scale = (460.0 / src.width().max(src.height()) as f32).min(1.0);
                    let img = if scale < 0.999 {
                        image::imageops::resize(&*src, ((src.width() as f32 * scale).round() as u32).max(1), ((src.height() as f32 * scale).round() as u32).max(1), image::imageops::FilterType::Triangle)
                    } else {
                        (*src).clone()
                    };
                    f.small = Some((Arc::new(img), scale));
                }
                let (small, scale) = f.small.clone().unwrap();
                f.job = Some(spawn(small, f.def.id, f.params.clone(), scale));
            }
        } else if f.due > 0.0 {
            ctx.request_repaint_after_secs(0.08);
        }
        if let Some(rx) = &f.job {
            match rx.try_recv() {
                Ok(img) => {
                    f.result = Some(img);
                    f.job = None;
                    f.redraw = true;
                }
                Err(TryRecvError::Empty) => ctx.request_repaint_after_secs(0.08),
                Err(TryRecvError::Disconnected) => f.job = None,
            }
        }
        if f.redraw {
            f.redraw = false;
            if let Some((small, _)) = &f.small {
                let shown = if f.view == View::Original { Some(small.as_ref()) } else { f.result.as_ref() };
                if let Some(img) = shown {
                    let ci = ColorImage::from_rgba_unmultiplied([img.width() as usize, img.height() as usize], img.as_raw());
                    match &mut f.preview {
                        Some(t) => t.set(ci, TextureOptions::LINEAR),
                        None => f.preview = Some(ctx.load_texture("filter_preview", ci, TextureOptions::LINEAR)),
                    }
                }
            }
        }

        if apply {
            if let Some(src) = f.src.clone() {
                f.applying = Some(spawn(src, f.def.id, f.params.clone(), 1.0));
            }
        }
        if let Some(rx) = &f.applying {
            match rx.try_recv() {
                Ok(out) => {
                    let (id, origin, label) = (f.layer, f.origin, f.def.label);
                    f.applying = None;
                    f.open = false;
                    self.finish_filter(id, &out, origin, label);
                }
                Err(TryRecvError::Empty) => ctx.request_repaint_after_secs(0.08),
                Err(TryRecvError::Disconnected) => {
                    f.applying = None;
                    self.notice = Some("The filter failed.");
                }
            }
        }
    }

    fn finish_filter(&mut self, id: LayerId, out: &image::RgbaImage, origin: (i32, i32), label: &str) {
        let Some(li) = self.doc.index_of(id) else { return };
        let sel = self.doc.selection.clone();
        let name = self.doc.layers[li].name.clone();
        let keys = filters::put_back(&mut self.doc.layers[li], out, origin.0, origin.1, sel.as_ref());
        if keys.is_empty() {
            self.notice = Some("Nothing changed.");
            return;
        }
        for k in keys {
            self.doc.mark_tile_dirty(k);
        }
        self.label_hint = Some(format!("{label} · {name}"));
    }

    /// The eye button in the view bar: black & white, inverted and color-blindness views.
    pub(super) fn check_button(&mut self, ui: &mut Ui) {
        let on = self.check != CheckView::Normal;
        let tip = format!("Check view: {}", CheckView::ALL.iter().find(|c| c.0 == self.check).map(|c| c.1).unwrap_or(""));
        let r = icon_button(ui, Icon::Eye, on || self.filter.check_open, 28.0, &tip);
        if r.clicked() {
            self.filter.check_open = !self.filter.check_open;
        }
        if !self.filter.check_open {
            return;
        }
        let mut open = true;
        egui::Window::new(RichText::new("Check view").strong())
            .open(&mut open)
            .collapsible(false)
            .resizable(false)
            .pivot(Align2::CENTER_BOTTOM)
            .default_pos(r.rect.center_top() - vec2(0.0, 12.0))
            .show(ui.ctx(), |ui| {
                ui.set_width(260.0);
                ui.label(RichText::new("Only the display changes, never your drawing.").size(11.5).color(TEXT_DIM));
                ui.horizontal_wrapped(|ui| {
                    ui.spacing_mut().item_spacing = vec2(4.0, 4.0);
                    for (c, label, about) in CheckView::ALL {
                        if widgets::chip(ui, label, self.check == c).on_hover_text(about).clicked() {
                            self.check = c;
                        }
                    }
                });
            });
        self.filter.check_open = open;
    }
}

/// The settings of one filter: sliders, choices, colors and the curve.
pub(super) fn params_editor(ui: &mut Ui, def: &'static FilterDef, params: &mut Params) {
    for pd in def.params {
        match &pd.kind {
            Kind::Slider { min, max, step, suffix, .. } => {
                ui.horizontal(|ui| {
                    ui.label(RichText::new(pd.label).size(12.5).color(TEXT_DIM));
                    let v = params.v.entry(pd.key).or_insert(0.0);
                    let mut s = egui::Slider::new(v, *min..=*max).step_by(*step as f64).suffix(*suffix);
                    if *step >= 1.0 {
                        s = s.max_decimals(0);
                    }
                    ui.add(s);
                });
            }
            Kind::Choice { options, .. } => {
                ui.horizontal(|ui| {
                    ui.label(RichText::new(pd.label).size(12.5).color(TEXT_DIM));
                    let cur = params.f(pd.key) as usize;
                    for (i, o) in options.iter().enumerate() {
                        if widgets::chip(ui, o, cur == i).clicked() {
                            params.v.insert(pd.key, i as f32);
                        }
                    }
                });
            }
            Kind::Check { .. } => {
                let mut b = params.b(pd.key);
                if ui.checkbox(&mut b, pd.label).changed() {
                    params.v.insert(pd.key, b as u8 as f32);
                }
            }
            Kind::Color { .. } => {
                ui.horizontal(|ui| {
                    ui.label(RichText::new(pd.label).size(12.5).color(TEXT_DIM));
                    let c = params.colors.entry(pd.key).or_insert([0, 0, 0]);
                    egui::color_picker::color_edit_button_srgb(ui, c);
                });
            }
        }
    }
    if def.curve {
        curve_editor(ui, &mut params.curve);
        if ui.button("Straighten the curve").clicked() {
            params.curve = vec![[0.0, 0.0], [1.0, 1.0]];
        }
    }
    if def.group == Group::Adjust || !def.params.is_empty() {
        if ui.small_button("Reset").clicked() {
            *params = Params::defaults(def);
        }
    }
}

/// A tone curve you can bend: click to add a point, drag to move, right-click to remove.
fn curve_editor(ui: &mut Ui, pts: &mut Vec<[f32; 2]>) {
    let side = ui.available_width().min(240.0);
    let (rect, resp) = ui.allocate_exact_size(vec2(side, side), Sense::click_and_drag());
    let to_screen = |p: [f32; 2]| pos2(rect.left() + p[0] * rect.width(), rect.bottom() - p[1] * rect.height());
    let from_screen = |q: egui::Pos2| [((q.x - rect.left()) / rect.width()).clamp(0.0, 1.0), (1.0 - (q.y - rect.top()) / rect.height()).clamp(0.0, 1.0)];
    let id = ui.id().with("curve_drag");
    let mut drag: Option<usize> = ui.data(|d| d.get_temp(id));
    if let Some(pp) = resp.interact_pointer_pos() {
        if resp.drag_started() || resp.clicked() {
            drag = pts.iter().position(|p| to_screen(*p).distance(pp) < 9.0);
            if drag.is_none() && resp.clicked_by(egui::PointerButton::Primary) {
                let q = from_screen(pp);
                pts.push(q);
                pts.sort_by(|a, b| a[0].total_cmp(&b[0]));
                drag = pts.iter().position(|p| *p == q);
            }
        }
        if resp.secondary_clicked() {
            if let Some(i) = pts.iter().position(|p| to_screen(*p).distance(pp) < 9.0) {
                if i != 0 && i != pts.len() - 1 && pts.len() > 2 {
                    pts.remove(i);
                }
            }
        }
        if let (true, Some(i)) = (resp.dragged(), drag) {
            let mut q = from_screen(pp);
            // keep the order: a point stays between its neighbours (the two ends only move up and down)
            if i == 0 {
                q[0] = 0.0;
            } else if i == pts.len() - 1 {
                q[0] = 1.0;
            } else {
                q[0] = q[0].clamp(pts[i - 1][0] + 0.01, pts[i + 1][0] - 0.01);
            }
            pts[i] = q;
        }
    }
    if !resp.dragged() {
        drag = None;
    }
    ui.data_mut(|d| d.insert_temp(id, drag));
    let p = ui.painter_at(rect);
    p.rect_filled(rect, CornerRadius::same(6), BG);
    for g in [0.25, 0.5, 0.75] {
        p.line_segment([pos2(rect.left() + g * rect.width(), rect.top()), pos2(rect.left() + g * rect.width(), rect.bottom())], Stroke::new(1.0, LINE));
        p.line_segment([pos2(rect.left(), rect.bottom() - g * rect.height()), pos2(rect.right(), rect.bottom() - g * rect.height())], Stroke::new(1.0, LINE));
    }
    p.line_segment([to_screen([0.0, 0.0]), to_screen([1.0, 1.0])], Stroke::new(1.0, TEXT_DIM.gamma_multiply(0.5)));
    let lut = filters::curve_lut(pts);
    let line: Vec<egui::Pos2> = (0..=64).map(|i| to_screen([i as f32 / 64.0, lut[(i * 255 / 64).min(255)] as f32 / 255.0])).collect();
    p.add(egui::Shape::line(line, Stroke::new(2.0, VENOM)));
    for pt in pts.iter() {
        let c = to_screen(*pt);
        p.circle_filled(c, 4.5, Color32::WHITE);
        p.circle_stroke(c, 4.5, Stroke::new(1.5, Color32::BLACK));
    }
    p.rect_stroke(rect, CornerRadius::same(6), Stroke::new(1.0, LINE), StrokeKind::Outside);
}

#[cfg(test)]
impl DrawState {
    pub fn filter_ui_mut(&mut self) -> &mut FilterUi {
        &mut self.filter
    }

    pub fn filter_ready(&self) -> bool {
        self.filter.result.is_some() && self.filter.job.is_none() && self.filter.preview.is_some()
    }

    pub fn filter_open_now(&self) -> bool {
        self.filter.open
    }

    pub fn test_apply_filter(&mut self) {
        // the same as pressing the Apply button
        if let Some(src) = self.filter.src.clone() {
            self.filter.applying = Some(spawn(src, self.filter.def.id, self.filter.params.clone(), 1.0));
        }
    }

    pub fn check_view(&self) -> CheckView {
        self.check
    }
}

#[cfg(test)]
impl FilterUi {
    pub fn set_param(&mut self, key: &'static str, v: f32) {
        self.params.v.insert(key, v);
    }
}
