// SPDX-License-Identifier: GPL-3.0-or-later
// References in the Sub view (and a bigger pop-out window): add pictures, look at them in any
// zoom/turn/mirror/gray, take colors with a click, make a palette, and lay one over the canvas.

use eframe::egui::{
    self, Align2, Color32, CornerRadius, CursorIcon, Event, FontId, Id, Mesh, MouseWheelUnit, Order, Painter, Pos2, Rect, RichText, Sense,
    Shape, Stroke, StrokeKind, Ui, Vec2, epaint::Vertex, pos2, vec2,
};

use super::{DrawState, SubTab, hex_of};
use crate::theme::*;
use crate::widgets;

const PICTURES: &[&str] = &["png", "jpg", "jpeg", "webp", "bmp", "gif"];

/// A textured quad with the given texture coordinates at its corners (TL, TR, BR, BL).
fn quad(p: &Painter, tex: egui::TextureId, corners: [Pos2; 4], uvs: [Pos2; 4], tint: Color32) {
    let mut m = Mesh::with_texture(tex);
    for (pos, uv) in corners.iter().zip(uvs) {
        m.vertices.push(Vertex { pos: *pos, uv, color: tint });
    }
    m.add_triangle(0, 1, 2);
    m.add_triangle(0, 2, 3);
    p.add(Shape::mesh(m));
}

impl DrawState {
    fn add_reference_paths(&mut self, paths: &[std::path::PathBuf]) {
        let mut failed = false;
        for p in paths {
            if self.refs.add_path(p).is_err() {
                failed = true;
            }
        }
        if failed {
            self.notice = Some("Some files couldn't be opened as pictures.");
        }
        if !paths.is_empty() {
            self.subview_open = true;
            self.subview_tab = SubTab::References;
        }
    }

    /// Pictures dropped onto the window become references.
    pub(super) fn handle_dropped_files(&mut self, ctx: &egui::Context) {
        let (dropped, hovering) = ctx.input(|i| (i.raw.dropped_files.clone(), !i.raw.hovered_files.is_empty()));
        if hovering {
            let screen = ctx.content_rect();
            let lp = ctx.layer_painter(egui::LayerId::new(Order::Foreground, Id::new("drop_hint")));
            lp.rect_filled(screen, 0.0, Color32::from_black_alpha(120));
            lp.rect_stroke(screen.shrink(12.0), CornerRadius::same(16), Stroke::new(3.0, VENOM), StrokeKind::Inside);
            lp.text(screen.center(), Align2::CENTER_CENTER, "Drop pictures to add them as references", FontId::proportional(22.0), TEXT);
        }
        if dropped.is_empty() {
            return;
        }
        let mut paths = Vec::new();
        for f in dropped {
            let p = f.path().to_path_buf();
            let ext = p.extension().map(|e| e.to_string_lossy().to_lowercase()).unwrap_or_default();
            if ["seviath", "psd", "ora", "kra"].contains(&ext.as_str()) {
                self.open_requests.push(p); // project files open as documents
                continue;
            }
            let ok = p.extension().map(|e| PICTURES.contains(&e.to_string_lossy().to_lowercase().as_str())).unwrap_or(false);
            if ok {
                paths.push(p);
            } else {
                self.notice = Some("Only pictures (PNG, JPG, WebP, BMP, GIF) can be references for now.");
            }
        }
        self.add_reference_paths(&paths);
    }

    /// The References tab: tools, the picture, and the thumbnails of all references.
    pub(super) fn references_panel(&mut self, ui: &mut Ui, w: f32, h: f32, big: bool) {
        // ---- tools
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing = vec2(3.0, 3.0);
            if widgets::chip(ui, "+ Add", false).on_hover_text("Add pictures (or drop them onto the window)").clicked() {
                if let Some(paths) = rfd::FileDialog::new().set_title("Add reference pictures").add_filter("Pictures", PICTURES).pick_files() {
                    self.add_reference_paths(&paths);
                }
            }
            let has = self.refs.current().is_some();
            if has {
                let r = self.refs.current_mut().unwrap();
                let (gray, flip) = (r.gray, r.flip);
                let mut changed = false;
                if widgets::chip(ui, "Gray", gray).on_hover_text("Look at the values only").clicked() {
                    r.gray = !gray;
                    changed = true;
                }
                if widgets::chip(ui, "Mirror", flip).clicked() {
                    r.flip = !flip;
                    changed = true;
                }
                if widgets::chip(ui, "Turn", false).on_hover_text("Turn a quarter clockwise").clicked() {
                    r.rot = (r.rot + 1) % 4;
                    changed = true;
                }
                if widgets::chip(ui, "Fit", false).clicked() {
                    r.zoom = 1.0;
                    r.pan = Vec2::ZERO;
                    changed = true;
                }
                if widgets::chip(ui, "Grid", self.refs.grid).on_hover_text("Thirds grid for proportions").clicked() {
                    self.refs.grid = !self.refs.grid;
                    changed = true;
                }
                if widgets::chip(ui, "On canvas", self.refs.overlay).on_hover_text("Show this picture see-through over your page").clicked() {
                    self.refs.overlay = !self.refs.overlay;
                    changed = true;
                }
                if widgets::chip(ui, "Palette", false).on_hover_text("Add the picture's main colors to your swatches").clicked() {
                    let colors = self.refs.current().map(|r| r.palette(6)).unwrap_or_default();
                    let mut added = 0;
                    for c in colors {
                        if !self.swatches.contains(&c) {
                            if self.swatches.len() >= self.set.swatches_max {
                                self.swatches.remove(0);
                            }
                            self.swatches.push(c);
                            added += 1;
                        }
                    }
                    self.notice = Some(if added > 0 { "Palette added to your swatches." } else { "Those colors are already in your swatches." });
                }
                if !big && widgets::chip(ui, "Pop out", self.ref_popout).on_hover_text("Bigger reference window you can move anywhere").clicked() {
                    self.ref_popout = !self.ref_popout;
                }
                if widgets::chip(ui, "Remove", false).on_hover_text("Remove this reference").clicked() {
                    if let Some(id) = self.refs.current {
                        self.refs.remove(id);
                    }
                }
                if changed {
                    self.refs.save();
                }
            }
        });
        if self.refs.overlay && self.refs.current().is_some() {
            ui.horizontal(|ui| {
                ui.label(RichText::new("On canvas").size(11.5).color(TEXT_DIM));
                if ui.add(egui::Slider::new(&mut self.refs.overlay_opacity, 0.05..=1.0).custom_formatter(|v, _| format!("{:.0}%", v * 100.0))).drag_stopped() {
                    self.refs.save();
                }
            });
        }
        ui.add_space(3.0);

        // ---- the picture
        let (rect, resp) = ui.allocate_exact_size(vec2(w, h), Sense::click_and_drag());
        if !big {
            self.ref_view_rect = Some(rect);
        }
        let p = ui.painter_at(rect);
        p.rect_filled(rect, CornerRadius::same(6), BG);
        let ctx = ui.ctx().clone();
        let mut pick: Option<Color32> = None;
        let grid = self.refs.grid;
        if let Some(r) = self.refs.current_mut() {
            let shown = r.shown_size();
            let fit = ((rect.width() - 8.0) / shown.x).min((rect.height() - 8.0) / shown.y);
            // wheel zooms at the pointer (and doesn't scroll the panel)
            if let Some(hp) = resp.hover_pos() {
                let mut f = 1.0f32;
                ctx.input(|i| {
                    for e in &i.events {
                        if let Event::MouseWheel { unit, delta, .. } = e {
                            let k = match unit {
                                MouseWheelUnit::Point => 1.0,
                                MouseWheelUnit::Line => 40.0,
                                MouseWheelUnit::Page => 400.0,
                            };
                            f *= (delta.y * k * 0.0045).exp();
                        }
                    }
                });
                if f != 1.0 {
                    let nz = (r.zoom * f).clamp(0.05, 40.0);
                    let f = nz / r.zoom;
                    let rel = hp - rect.center();
                    r.pan = rel - (rel - r.pan) * f;
                    r.zoom = nz;
                    ctx.input_mut(|i| i.smooth_scroll_delta = Vec2::ZERO);
                }
            }
            if resp.dragged() {
                r.pan += resp.drag_delta();
            }
            let shown_rect = Rect::from_center_size(rect.center() + r.pan, shown * fit * r.zoom);
            let tex = r.texture(&ctx);
            let corners = [shown_rect.left_top(), shown_rect.right_top(), shown_rect.right_bottom(), shown_rect.left_bottom()];
            quad(&p, tex, corners, r.corner_uvs(), Color32::WHITE);
            if grid {
                let st = Stroke::new(1.0, VENOM.gamma_multiply(0.8));
                for k in 1..3 {
                    let t = k as f32 / 3.0;
                    let x = shown_rect.left() + shown_rect.width() * t;
                    let y = shown_rect.top() + shown_rect.height() * t;
                    p.line_segment([pos2(x, shown_rect.top()), pos2(x, shown_rect.bottom())], st);
                    p.line_segment([pos2(shown_rect.left(), y), pos2(shown_rect.right(), y)], st);
                }
                p.rect_stroke(shown_rect, 0.0, st, StrokeKind::Middle);
            }
            if resp.clicked() {
                if let Some(pt) = resp.interact_pointer_pos() {
                    pick = r.color_at((pt.x - shown_rect.left()) / shown_rect.width(), (pt.y - shown_rect.top()) / shown_rect.height());
                }
            }
            if let Some(hp) = resp.hover_pos() {
                // a loupe ring showing the color under the pointer
                if let Some(c) = r.color_at((hp.x - shown_rect.left()) / shown_rect.width(), (hp.y - shown_rect.top()) / shown_rect.height()) {
                    p.circle_filled(hp + vec2(16.0, -16.0), 9.0, c);
                    p.circle_stroke(hp + vec2(16.0, -16.0), 9.0, Stroke::new(1.5, Color32::WHITE));
                }
            }
            let name = r.name.clone();
            p.text(rect.left_bottom() + vec2(6.0, -6.0), Align2::LEFT_BOTTOM, name, FontId::proportional(11.0), TEXT_DIM);
        } else {
            p.text(rect.center() - vec2(0.0, 8.0), Align2::CENTER_CENTER, "Drop pictures here", FontId::proportional(13.0), TEXT_DIM);
            p.text(rect.center() + vec2(0.0, 10.0), Align2::CENTER_CENTER, "or press + Add", FontId::proportional(11.5), TEXT_DIM);
        }
        if resp.drag_stopped() {
            self.refs.save();
        }
        resp.on_hover_text("Click: take the color · Drag: move · Wheel: zoom").on_hover_cursor(CursorIcon::Crosshair);
        if let Some(c) = pick {
            self.primary = c;
            self.hex = hex_of(c);
        }

        // ---- thumbnails of all references
        if !self.refs.refs.is_empty() {
            ui.add_space(3.0);
            let mut select = None;
            let mut remove = None;
            egui::ScrollArea::horizontal().id_salt(if big { "ref_thumbs_big" } else { "ref_thumbs" }).show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 4.0;
                    let current = self.refs.current;
                    for r in self.refs.refs.iter_mut() {
                        let (cell, resp) = ui.allocate_exact_size(Vec2::splat(44.0), Sense::click());
                        let p = ui.painter();
                        p.rect_filled(cell, CornerRadius::same(5), RAISED);
                        let s = r.shown_size();
                        let k = (40.0 / s.x).min(40.0 / s.y);
                        let tr = Rect::from_center_size(cell.center(), s * k);
                        let tex = r.texture(&ctx);
                        quad(p, tex, [tr.left_top(), tr.right_top(), tr.right_bottom(), tr.left_bottom()], r.corner_uvs(), Color32::WHITE);
                        if current == Some(r.id) {
                            p.rect_stroke(cell, CornerRadius::same(5), Stroke::new(2.0, VENOM), StrokeKind::Inside);
                        }
                        let id = r.id;
                        let resp = resp.on_hover_text(r.name.clone());
                        if resp.clicked() {
                            select = Some(id);
                        }
                        resp.context_menu(|ui| {
                            if ui.button("Remove").clicked() {
                                remove = Some(id);
                                ui.close();
                            }
                        });
                    }
                });
            });
            if let Some(id) = select {
                self.refs.current = Some(id);
                self.refs.save();
            }
            if let Some(id) = remove {
                self.refs.remove(id);
            }
        }
    }

    /// The bigger reference window ("Pop out").
    pub(super) fn reference_window(&mut self, ctx: &egui::Context) {
        if !self.ref_popout {
            return;
        }
        let mut open = true;
        egui::Window::new(RichText::new("Reference").strong())
            .open(&mut open)
            .default_size(vec2(520.0, 620.0))
            .resizable(true)
            .collapsible(true)
            .show(ctx, |ui| {
                let w = ui.available_width().max(200.0);
                let h = (ui.available_height() - 110.0).max(160.0);
                self.references_panel(ui, w, h, true);
            });
        self.ref_popout = open;
    }

    /// The current reference laid see-through over the page (fit inside it, turning with the view).
    pub(super) fn reference_overlay(&mut self, p: &Painter) {
        if !self.refs.overlay {
            return;
        }
        let opacity = self.refs.overlay_opacity;
        let (dw, dh) = (self.doc.w as f32, self.doc.h as f32);
        let view = self.view;
        let ctx = p.ctx().clone();
        let Some(r) = self.refs.current_mut() else { return };
        let s = r.shown_size();
        let k = (dw / s.x).min(dh / s.y);
        let (w, h) = (s.x * k, s.y * k);
        let (x0, y0) = ((dw - w) / 2.0, (dh - h) / 2.0);
        let corners = [pos2(x0, y0), pos2(x0 + w, y0), pos2(x0 + w, y0 + h), pos2(x0, y0 + h)].map(|c| view.doc_to_screen(c));
        let tex = r.texture(&ctx);
        quad(p, tex, corners, r.corner_uvs(), Color32::WHITE.gamma_multiply(opacity));
    }
}
