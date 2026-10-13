// SPDX-License-Identifier: GPL-3.0-or-later
// Color picker in the Seviath style: a full-width saturation/brightness square, a hue bar,
// and old → new swatches. Keeps the hue when the color goes gray so dragging back feels right.

use eframe::egui::{
    self, Color32, CornerRadius, CursorIcon, Mesh, Pos2, Rect, Sense, Stroke, StrokeKind, Ui, ecolor::Hsva, pos2, vec2,
};

use crate::theme::*;

/// Returns true when the color changed.
pub fn picker(ui: &mut Ui, color: &mut Color32, hsv: &mut Hsva) -> bool {
    // Follow outside changes (swatches, hex field, swap) without losing the hue on grays.
    if Color32::from(*hsv) != *color {
        let fresh = Hsva::from(*color);
        hsv.s = fresh.s;
        hsv.v = fresh.v;
        if fresh.s > 0.001 && fresh.v > 0.001 {
            hsv.h = fresh.h;
        }
    }
    let before = *color;
    let w = ui.available_width();

    // Saturation × brightness square.
    let (sq, resp) = ui.allocate_exact_size(vec2(w, (w * 0.62).round()), Sense::click_and_drag());
    let resp = resp.on_hover_cursor(CursorIcon::Crosshair);
    if let Some(p) = resp.interact_pointer_pos() {
        hsv.s = ((p.x - sq.left()) / sq.width()).clamp(0.0, 1.0);
        hsv.v = 1.0 - ((p.y - sq.top()) / sq.height()).clamp(0.0, 1.0);
    }
    let painter = ui.painter();
    let n = 24;
    painter.add(grid_mesh(sq, n, n, |u, v| Color32::from(Hsva::new(hsv.h, u, 1.0 - v, 1.0))));
    painter.rect_stroke(sq, CornerRadius::same(4), Stroke::new(1.0, LINE), StrokeKind::Outside);
    let knob = pos2(sq.left() + hsv.s * sq.width(), sq.top() + (1.0 - hsv.v) * sq.height());
    painter.circle_stroke(knob, 7.0, Stroke::new(2.5, Color32::BLACK));
    painter.circle_stroke(knob, 7.0, Stroke::new(1.5, Color32::WHITE));

    ui.add_space(4.0);

    // Hue bar.
    let (bar, bresp) = ui.allocate_exact_size(vec2(w, 14.0), Sense::click_and_drag());
    if let Some(p) = bresp.interact_pointer_pos() {
        hsv.h = ((p.x - bar.left()) / bar.width()).clamp(0.0, 0.9999);
    }
    let painter = ui.painter();
    painter.add(grid_mesh(bar, 36, 1, |u, _| Color32::from(Hsva::new(u, 1.0, 1.0, 1.0))));
    painter.rect_stroke(bar, CornerRadius::same(4), Stroke::new(1.0, LINE), StrokeKind::Outside);
    let hx = bar.left() + hsv.h * bar.width();
    let marker = Rect::from_center_size(pos2(hx, bar.center().y), vec2(6.0, bar.height() + 6.0));
    painter.rect_filled(marker, CornerRadius::same(3), Color32::from(Hsva::new(hsv.h, 1.0, 1.0, 1.0)));
    painter.rect_stroke(marker, CornerRadius::same(3), Stroke::new(2.0, Color32::WHITE), StrokeKind::Outside);

    *color = Color32::from(*hsv);
    *color != before
}

/// A rectangle split into a grid of colored quads; `f(u, v)` gives the color at each corner (0..1).
fn grid_mesh(r: Rect, nx: usize, ny: usize, f: impl Fn(f32, f32) -> Color32) -> Mesh {
    let mut m = Mesh::default();
    for j in 0..=ny {
        for i in 0..=nx {
            let u = i as f32 / nx as f32;
            let v = j as f32 / ny as f32;
            m.colored_vertex(Pos2::new(r.left() + u * r.width(), r.top() + v * r.height()), f(u, v));
        }
    }
    let row = (nx + 1) as u32;
    for j in 0..ny as u32 {
        for i in 0..nx as u32 {
            let a = j * row + i;
            m.add_triangle(a, a + 1, a + row);
            m.add_triangle(a + 1, a + row + 1, a + row);
        }
    }
    m
}

/// Old/new swatch pair: shows the color you started with next to the current one.
pub fn compare(ui: &mut Ui, old: Color32, new: Color32) -> egui::Response {
    let (r, resp) = ui.allocate_exact_size(vec2(44.0, 22.0), Sense::click());
    let p = ui.painter();
    let (a, b) = r.split_left_right_at_fraction(0.5);
    p.rect_filled(a, CornerRadius { nw: 5, sw: 5, ne: 0, se: 0 }, old);
    p.rect_filled(b, CornerRadius { nw: 0, sw: 0, ne: 5, se: 5 }, new);
    p.rect_stroke(r, CornerRadius::same(5), Stroke::new(1.0, LINE), StrokeKind::Outside);
    resp.on_hover_text("Left: color before · Right: now. Click to go back.")
}
