// SPDX-License-Identifier: GPL-3.0-or-later
// Small custom widgets shared by all tabs, styled in the Seviath theme.

use eframe::egui::{
    self, Align2, Color32, CornerRadius, FontId, Response, Sense, Stroke, StrokeKind, Ui, Vec2, pos2, vec2,
};

use crate::icons::{self, Icon};
use crate::theme::*;

/// A hover help text on a widget, and (when the setting is on) a small ! at its corner so people can see where help is. Use this instead of
/// `on_hover_text` on anything that is clicked or dragged.
pub trait HelpExt {
    fn help(self, text: impl Into<egui::WidgetText>) -> Self;
}

impl HelpExt for Response {
    fn help(self, text: impl Into<egui::WidgetText>) -> Self {
        if self.ctx.data(|d| d.get_temp::<bool>(egui::Id::new("help_marks"))).unwrap_or(true) && (self.sense.senses_click() || self.sense.senses_drag()) {
            let rc = self.interact_rect;
            if rc.is_positive() && rc.width() >= 24.0 && rc.height() >= 14.0 {
                let c = pos2(rc.right() - 2.0, rc.top() + 2.0);
                let p = self.ctx.layer_painter(self.layer_id);
                p.circle_filled(c, 4.3, Color32::from_rgba_unmultiplied(214, 168, 76, 215));
                p.text(c + vec2(0.0, 0.3), Align2::CENTER_CENTER, "!", FontId::proportional(8.0), Color32::from_rgb(20, 18, 10));
            }
        }
        self.on_hover_text(text)
    }
}

/// Square icon button. `selected` draws the emerald fill and a small fang marker on the left.
pub fn icon_button(ui: &mut Ui, icon: Icon, selected: bool, size: f32, tip: &str) -> Response {
    let (rect, resp) = ui.allocate_exact_size(Vec2::splat(size), Sense::click());
    if ui.is_rect_visible(rect) {
        let p = ui.painter();
        let (bg, fg) = if selected {
            (EMERALD_DEEP, VENOM)
        } else if resp.hovered() {
            (HOVER, TEXT)
        } else {
            (Color32::TRANSPARENT, TEXT_DIM)
        };
        p.rect_filled(rect, CornerRadius::same(CORNER), bg);
        if selected {
            p.rect_stroke(rect, CornerRadius::same(CORNER), Stroke::new(1.0, EMERALD), StrokeKind::Inside);
            fang(p, pos2(rect.left() - 3.0, rect.center().y), 5.0);
        }
        icons::paint(p, rect.shrink(size * 0.24), icon, fg);
    }
    resp.help(tip)
}

/// A small downward fang, used as the "selected" marker.
pub fn fang(p: &egui::Painter, at: egui::Pos2, h: f32) {
    p.add(egui::Shape::convex_polygon(
        vec![at + vec2(0.0, -h), at + vec2(h * 0.6, 0.0), at + vec2(0.0, h)],
        VENOM,
        Stroke::NONE,
    ));
}

/// Main tab button: icon + label, colored underline with fangs when active.
pub fn tab_button(ui: &mut Ui, icon: Icon, label: &str, accent: Color32, selected: bool, tip: &str) -> Response {
    let font = FontId::proportional(14.0);
    let text_w = ui.ctx().fonts_mut(|f| f.layout_no_wrap(label.to_owned(), font.clone(), TEXT).size().x);
    let size = vec2(text_w + 44.0, 34.0);
    let (rect, resp) = ui.allocate_exact_size(size, Sense::click());
    if ui.is_rect_visible(rect) {
        let p = ui.painter();
        let hovered = resp.hovered();
        if selected {
            p.rect_filled(rect, CornerRadius::same(9), accent.gamma_multiply(0.16));
        } else if hovered {
            p.rect_filled(rect, CornerRadius::same(9), HOVER);
        }
        let fg = if selected { accent } else if hovered { TEXT } else { TEXT_DIM };
        let ir = egui::Rect::from_center_size(pos2(rect.left() + 18.0, rect.center().y), Vec2::splat(16.0));
        icons::paint(p, ir, icon, fg);
        p.text(
            pos2(rect.left() + 32.0, rect.center().y),
            Align2::LEFT_CENTER,
            label,
            font,
            if selected { TEXT } else { fg },
        );
        if selected {
            let y = rect.bottom() - 1.5;
            p.line_segment([pos2(rect.left() + 8.0, y), pos2(rect.right() - 8.0, y)], Stroke::new(2.5, accent));
            // Two fangs hanging from the underline.
            for x in [rect.center().x - 5.0, rect.center().x + 5.0] {
                p.add(egui::Shape::convex_polygon(
                    vec![pos2(x - 2.0, y), pos2(x + 2.0, y), pos2(x, y + 4.0)],
                    accent,
                    Stroke::NONE,
                ));
            }
        }
    }
    resp.help(tip)
}

/// Pill-shaped toggle chip (brush categories, filters, segmented choices).
pub fn chip(ui: &mut Ui, label: &str, selected: bool) -> Response {
    let font = FontId::proportional(12.5);
    let text_w = ui.ctx().fonts_mut(|f| f.layout_no_wrap(label.to_owned(), font.clone(), TEXT).size().x);
    let (rect, resp) = ui.allocate_exact_size(vec2(text_w + 18.0, 22.0), Sense::click());
    if ui.is_rect_visible(rect) {
        let p = ui.painter();
        let r = CornerRadius::same(11);
        if selected {
            p.rect_filled(rect, r, EMERALD);
        } else {
            p.rect_filled(rect, r, if resp.hovered() { HOVER } else { RAISED });
        }
        p.text(rect.center(), Align2::CENTER_CENTER, label, font, if selected { ON_ACCENT } else { TEXT });
    }
    resp
}

/// Segmented control: one chip per option, returns true when the value changed.
pub fn segmented<T: PartialEq + Copy>(ui: &mut Ui, value: &mut T, options: &[(T, &str)]) -> bool {
    let mut changed = false;
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 4.0;
        for (v, label) in options {
            if chip(ui, label, *value == *v).clicked() && *value != *v {
                *value = *v;
                changed = true;
            }
        }
    });
    changed
}

/// Section title in the side panels, with a gold scale dot.
pub fn section_title(ui: &mut Ui, title: &str) {
    ui.horizontal(|ui| {
        let (r, _) = ui.allocate_exact_size(vec2(8.0, 14.0), Sense::hover());
        ui.painter().circle_filled(r.center(), 3.0, GOLD);
        ui.label(egui::RichText::new(title).size(13.5).strong().color(TEXT));
    });
}

/// Labeled slider row with a fixed label column so all sliders line up.
pub fn slider_row(ui: &mut Ui, label: &str, value: &mut f32, range: std::ops::RangeInclusive<f32>, suffix: &str) -> Response {
    ui.horizontal(|ui| {
        let (r, _) = ui.allocate_exact_size(vec2(72.0, 18.0), Sense::hover());
        ui.painter().text(r.left_center(), Align2::LEFT_CENTER, label, FontId::proportional(12.5), TEXT_DIM);
        ui.add(egui::Slider::new(value, range).suffix(suffix))
    })
    .inner
}
