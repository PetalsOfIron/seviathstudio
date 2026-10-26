// SPDX-License-Identifier: GPL-3.0-or-later
// Layer panel: tree with folders, drag & drop, rename, thumbnails, and the active layer's settings.

use eframe::egui::{
    self, Align, Align2, Color32, CornerRadius, CursorIcon, FontId, Id, Key, Layout, Modifiers, Order, Rect,
    RichText, Sense, Shape, Stroke, StrokeKind, Ui, Vec2, pos2, vec2,
};

use crate::widgets::HelpExt;
use super::{DrawState, checker};
use crate::doc::{Blend, Drop, LayerId, LayerKind, TAG_COLORS};
use crate::icons::{self, Icon};
use crate::theme::*;
use crate::widgets::{self, icon_button, section_title, slider_row};

const ROW_H: f32 = 46.0;
const INDENT: f32 = 16.0;

impl DrawState {
    pub(super) fn active_idx(&self) -> usize {
        self.doc.index_of(self.active).unwrap_or(0)
    }

    /// Keep the selection on an existing layer (after deletes, merges, undo…).
    pub(super) fn fix_active(&mut self) {
        if self.doc.index_of(self.active).is_none() {
            self.active = self.doc.layers.first().map(|l| l.id).unwrap_or(0);
        }
    }

    fn next_name(&mut self, base: &str) -> String {
        self.layer_counter += 1;
        format!("{base} {}", self.layer_counter)
    }

    pub(super) fn add_layer(&mut self, kind: LayerKind) {
        let name = match kind {
            LayerKind::Group => self.next_name("Folder"),
            LayerKind::Vector => self.next_name("Vector"),
            _ => self.next_name("Layer"),
        };
        let l = self.doc.new_layer(&name, kind);
        self.active = self.doc.insert_layer(l, self.active);
    }

    pub(super) fn duplicate_active(&mut self) {
        if let Some(id) = self.doc.duplicate(self.active) {
            self.active = id;
        }
    }

    pub(super) fn merge_down_active(&mut self) {
        match self.doc.merge_down(self.active) {
            Ok(id) => self.active = id,
            Err(e) => self.notice = Some(e),
        }
    }

    pub(super) fn group_active(&mut self) {
        let name = self.next_name("Folder");
        if let Some(f) = self.doc.group(self.active, &name) {
            self.active = f;
        }
    }

    pub(super) fn toggle_clip_active(&mut self) {
        let i = self.active_idx();
        if self.doc.layers[i].kind != LayerKind::Paper {
            self.doc.layers[i].clip = !self.doc.layers[i].clip;
            self.doc.mark_all_dirty();
        }
    }

    /// What affects the composited picture; when it changes the canvas is redrawn.
    fn layer_sig(&self) -> (Vec<(LayerId, bool, u32, Blend, bool)>, Color32) {
        let v = self.doc.layers.iter().map(|l| (l.id, l.visible, l.opacity.to_bits(), l.blend, l.clip)).collect();
        (v, self.doc.paper)
    }

    pub(super) fn layers_section(&mut self, ui: &mut Ui) {
        self.track_layer_use();
        let before = self.layer_sig();
        self.layers_section_inner(ui);
        self.fix_active();
        if self.layer_sig() != before {
            self.doc.mark_all_dirty();
        }
    }

    fn layers_section_inner(&mut self, ui: &mut Ui) {
        ui.horizontal(|ui| {
            section_title(ui, "Layers");
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                ui.spacing_mut().item_spacing.x = 2.0;
                let is_paper = self.doc.layers[self.active_idx()].kind == LayerKind::Paper;
                if icon_button(ui, Icon::Trash, false, 26.0, "Delete layer (a folder goes with its contents)").help("Delete the layer. A folder goes with everything in it.").clicked() {
                    if self.is_multi() {
                        self.batch_delete();
                    } else if is_paper {
                        self.notice = Some("The paper can't be deleted — hide it with the eye instead.");
                    } else {
                        self.delete_layer(self.active);
                    }
                }
                if icon_button(ui, Icon::Folder, false, 26.0, "New folder").help("Make a new folder.").clicked() {
                    self.add_layer(LayerKind::Group);
                }
                if icon_button(ui, Icon::Pen, false, 26.0, "New vector layer").help("Add a new vector layer, for smooth lines you can change later.").clicked() {
                    self.add_layer(LayerKind::Vector);
                }
                if icon_button(ui, Icon::Plus, false, 26.0, "New layer (Ctrl+Shift+N)").help("Add a new empty layer (Ctrl+Shift+N).").clicked() {
                    self.add_layer(LayerKind::Raster);
                }
            });
        });

        // Search, color labels, filter chips and sorting, for big files with many layers.
        let search = self.filter_bar(ui);
        self.favorites_row(ui);
        self.batch_bar(ui);
        let flat = self.layer_filtering();
        if flat && self.drag_layer.is_some() {
            self.drag_layer = None;
        }
        let order = self.layer_order();
        let order_ids: Vec<LayerId> = order.iter().map(|&i| self.doc.layers[i].id).collect();
        // typing in the search box jumps to the first match; arrows walk the matches; Esc clears
        if search.changed() && !order_ids.contains(&self.active) {
            if let Some(first) = order_ids.first() {
                self.active = *first;
            }
        }
        if search.has_focus() {
            let (up, down) = ui.ctx().input_mut(|i| (i.consume_key(Modifiers::NONE, Key::ArrowUp), i.consume_key(Modifiers::NONE, Key::ArrowDown)));
            let pos = order_ids.iter().position(|id| *id == self.active);
            if down {
                self.active = order_ids.get(pos.map(|p| p + 1).unwrap_or(0)).copied().unwrap_or(self.active);
            }
            if up {
                self.active = pos.and_then(|p| p.checked_sub(1)).and_then(|p| order_ids.get(p)).copied().unwrap_or(self.active);
            }
        }
        // Esc (the text box itself already lets go of focus on Esc): clear the search
        if (search.has_focus() || search.lost_focus()) && ui.ctx().input(|i| i.key_pressed(Key::Escape)) {
            self.lui.filter.query.clear();
            search.surrender_focus();
        }

        let w = ui.available_width();
        let mut rows: Vec<(LayerId, Rect, LayerKind)> = Vec::new();
        let mut drag_released = false;
        let ctx = ui.ctx().clone();

        for i in order {
            let depth = if flat { 0.0 } else { self.doc.depth(i) as f32 };
            let (row, resp) = ui.allocate_exact_size(vec2(w, ROW_H), Sense::click_and_drag());
            let id = self.doc.layers[i].id;
            let kind = self.doc.layers[i].kind;
            rows.push((id, row, kind));
            let selected = id == self.active;
            let picked = self.lui.multi.contains(&id);
            let dragging_this = self.drag_layer == Some(id);

            // Background and selection.
            {
                let p = ui.painter();
                let bg = if selected {
                    EMERALD_DEEP.gamma_multiply(0.55)
                } else if picked {
                    EMERALD_DEEP.gamma_multiply(0.3)
                } else if resp.hovered() {
                    HOVER
                } else {
                    RAISED
                };
                p.rect_filled(row, CornerRadius::same(8), if dragging_this { bg.gamma_multiply(0.4) } else { bg });
                if selected || picked {
                    p.rect_stroke(row, CornerRadius::same(8), Stroke::new(1.0, if selected { EMERALD } else { EMERALD_DEEP }), StrokeKind::Inside);
                }
                let tag = self.doc.layers[i].tag as usize;
                if tag > 0 {
                    let stripe = Rect::from_min_max(row.left_top() + vec2(1.0, 6.0), row.left_bottom() + vec2(5.0, -6.0));
                    p.rect_filled(stripe, CornerRadius::same(2), TAG_COLORS[tag].0);
                }
            }

            let x0 = row.left() + 30.0 + depth * INDENT;
            // Folder open/close arrow.
            if kind == LayerKind::Group {
                let ar = Rect::from_center_size(pos2(x0 + 6.0, row.center().y), Vec2::splat(16.0));
                let aresp = ui.interact(ar, ui.id().with(("fold", id)), Sense::click());
                let open = self.doc.layers[i].expanded;
                let c = ar.center();
                let col = if aresp.hovered() { VENOM } else { TEXT_DIM };
                let tri = if open {
                    vec![c + vec2(-4.0, -2.0), c + vec2(4.0, -2.0), c + vec2(0.0, 3.0)]
                } else {
                    vec![c + vec2(-2.0, -4.0), c + vec2(3.0, 0.0), c + vec2(-2.0, 4.0)]
                };
                ui.painter().add(Shape::convex_polygon(tri, col, Stroke::NONE));
                if aresp.help("Open / close folder").clicked() {
                    self.doc.layers[i].expanded = !open;
                }
            }
            let clip_x = if self.doc.layers[i].clip { 12.0 } else { 0.0 };
            let thumb = Rect::from_min_size(pos2(x0 + 14.0 + clip_x, row.top() + 6.0), Vec2::splat(34.0));
            if self.doc.layers[i].clip {
                // Hooked arrow pointing down to the layer it is clipped to.
                let p = ui.painter();
                let hx = thumb.left() - 7.0;
                p.line_segment([pos2(hx, row.top() + 12.0), pos2(hx, row.bottom() - 4.0)], Stroke::new(2.0, VENOM));
                p.add(Shape::convex_polygon(
                    vec![pos2(hx - 3.5, row.bottom() - 8.0), pos2(hx + 3.5, row.bottom() - 8.0), pos2(hx, row.bottom() - 3.0)],
                    VENOM,
                    Stroke::NONE,
                ));
            }

            // Thumbnail.
            let tex = {
                let (dw, dh) = (self.doc.w, self.doc.h);
                let layer = &self.doc.layers[i];
                self.renderer.layer_thumb(&ctx, layer, dw, dh)
            };
            {
                let layer = &self.doc.layers[i];
                let p = ui.painter();
                match layer.kind {
                    LayerKind::Paper => {
                        p.rect_filled(thumb, CornerRadius::same(4), self.doc.paper);
                    }
                    LayerKind::Group => {
                        p.rect_filled(thumb, CornerRadius::same(4), CARD);
                        icons::paint(p, thumb.shrink(7.0), Icon::Folder, GOLD);
                    }
                    LayerKind::Adjust => {
                        p.rect_filled(thumb, CornerRadius::same(4), CARD);
                        icons::paint(p, thumb.shrink(7.0), Icon::Gradient, VENOM);
                    }
                    LayerKind::Light => {
                        p.rect_filled(thumb, CornerRadius::same(4), CARD);
                        p.circle_filled(thumb.center(), 8.0, GOLD);
                        p.circle_stroke(thumb.center(), 11.0, Stroke::new(1.5, GOLD.gamma_multiply(0.6)));
                    }
                    LayerKind::Raster | LayerKind::Vector => {
                        checker(p, thumb);
                        if let Some(t) = tex {
                            let a = self.doc.w as f32 / self.doc.h as f32;
                            let r = if a >= 1.0 {
                                Rect::from_center_size(thumb.center(), vec2(thumb.width(), thumb.width() / a))
                            } else {
                                Rect::from_center_size(thumb.center(), vec2(thumb.height() * a, thumb.height()))
                            };
                            p.image(t, r, Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)), Color32::WHITE);
                        }
                    }
                }
                p.rect_stroke(thumb, CornerRadius::same(4), Stroke::new(1.0, LINE), StrokeKind::Outside);
            }

            // Name (or rename box) and details.
            let tx = thumb.right() + 10.0;
            let name_rect = Rect::from_min_max(pos2(tx, row.top() + 5.0), pos2(row.right() - 28.0, row.top() + 25.0));
            let renaming = matches!(&self.renaming, Some((rid, _)) if *rid == id);
            if renaming {
                let mut done = false;
                if let Some((_, text)) = &mut self.renaming {
                    let r = ui.put(name_rect, egui::TextEdit::singleline(text).font(FontId::proportional(13.0)));
                    if !r.has_focus() && !r.lost_focus() {
                        r.request_focus();
                    }
                    if r.lost_focus() {
                        done = true;
                    }
                }
                if done {
                    let (rid, text) = self.renaming.take().unwrap();
                    let t = text.trim();
                    if !t.is_empty() {
                        if let Some(k) = self.doc.index_of(rid) {
                            self.doc.layers[k].name = t.chars().take(80).collect();
                        }
                    }
                }
            } else {
                let layer = &self.doc.layers[i];
                let name_col = if layer.visible { TEXT } else { TEXT_DIM };
                ui.painter().with_clip_rect(name_rect.expand(2.0)).text(
                    pos2(tx, row.top() + 15.0),
                    Align2::LEFT_CENTER,
                    &layer.name,
                    FontId::proportional(13.0),
                    name_col,
                );
            }
            {
                let layer = &self.doc.layers[i];
                let mut sub = match layer.kind {
                    LayerKind::Paper => "Background".to_owned(),
                    LayerKind::Group => {
                        let n = self.doc.block_len(i) - 1;
                        format!("Folder · {} · {n} inside", layer.blend.name())
                    }
                    LayerKind::Vector => format!("Vector · {}", layer.blend.name()),
                    LayerKind::Raster if layer.depth => "Depth · hidden unless selected".to_owned(),
                    LayerKind::Light => format!("Light · {}", layer.blend.name()),
                    LayerKind::Raster => layer.blend.name().to_owned(),
                    LayerKind::Adjust => format!("Adjustment · {}", layer.adjust.as_ref().map(|a| a.label()).unwrap_or("none")),
                };
                if layer.text.is_some() {
                    sub = "Text".to_owned();
                } else if let Some(f) = &layer.lightfx {
                    sub = format!("{} · light effect", f.kind.name());
                } else if let Some(m) = &layer.material {
                    sub = format!("Material · {}", m.name);
                } else if layer.fx_active() {
                    sub += " · fx";
                }
                if layer.opacity < 100.0 {
                    sub += &format!(" · {:.0}%", layer.opacity);
                }
                let p = ui.painter().with_clip_rect(Rect::from_min_max(pos2(tx, row.top()), pos2(row.right() - 24.0, row.bottom())));
                p.text(pos2(tx, row.top() + 32.0), Align2::LEFT_CENTER, sub, FontId::proportional(11.0), TEXT_DIM);
                if layer.locked {
                    let lr = Rect::from_center_size(pos2(row.right() - 14.0, row.top() + 14.0), Vec2::splat(14.0));
                    icons::paint(ui.painter(), lr, Icon::Lock, GOLD);
                }
                if layer.fav {
                    let sr = Rect::from_center_size(pos2(row.right() - 14.0, row.bottom() - 13.0), Vec2::splat(14.0));
                    icons::paint(ui.painter(), sr, Icon::Star, GOLD);
                }
            }

            // Eye toggle, always in the first column.
            let eye = Rect::from_center_size(pos2(row.left() + 15.0, row.center().y), Vec2::splat(24.0));
            let eresp = ui.interact(eye, ui.id().with(("eye", id)), Sense::click());
            let visible = self.doc.layers[i].visible;
            let ecol = if eresp.hovered() { VENOM } else if visible { TEXT } else { TEXT_DIM };
            icons::paint(ui.painter(), eye.shrink(4.0), if visible { Icon::Eye } else { Icon::EyeOff }, ecol);
            if eresp.help("Show / hide").clicked() {
                self.doc.layers[i].visible = !visible;
            } else if resp.double_clicked() && kind != LayerKind::Paper {
                self.renaming = Some((id, self.doc.layers[i].name.clone()));
            } else if resp.clicked() {
                let mods = ctx.input(|i| i.modifiers);
                if mods.command {
                    // Ctrl + click: add or remove this layer from the picked ones
                    if self.lui.multi.is_empty() {
                        self.lui.multi.insert(self.active);
                    }
                    if !self.lui.multi.remove(&id) {
                        self.lui.multi.insert(id);
                    }
                    self.active = id;
                } else if mods.shift {
                    // Shift + click: everything from the active layer to this one
                    let a = order_ids.iter().position(|x| *x == self.active).unwrap_or(0);
                    let b = order_ids.iter().position(|x| *x == id).unwrap_or(0);
                    let (lo, hi) = (a.min(b), a.max(b));
                    let doc = &self.doc;
                    self.lui.multi =
                        order_ids[lo..=hi].iter().copied().filter(|x| doc.index_of(*x).is_some_and(|k| doc.layers[k].kind != LayerKind::Paper)).collect();
                } else {
                    self.lui.multi.clear();
                    self.active = id;
                }
            }

            // Right-click menu.
            resp.context_menu(|ui| {
                if !self.lui.multi.contains(&id) {
                    self.lui.multi.clear();
                }
                self.active = id;
                if ui.button("New layer above").help("Add a new layer above this one.").clicked() {
                    self.new_layer_beside(id, true, false);
                    ui.close();
                }
                if kind != LayerKind::Paper {
                    if ui.button("New layer below").help("Add a new layer below this one.").clicked() {
                        self.new_layer_beside(id, false, false);
                        ui.close();
                    }
                    if ui.button("New layer clipped to this").help("A shading layer that only shows where this layer has paint").clicked() {
                        self.new_layer_beside(id, true, true);
                        ui.close();
                    }
                    if kind == LayerKind::Raster && ui.button("Select pixels of this layer").help("Select the painted pixels of this layer.").clicked() {
                        self.select_layer_pixels(id);
                        ui.close();
                    }
                    if kind == LayerKind::Raster && ui.button("Trace to vector lines…").help("Auto-trace: turn this drawing into editable lines and color shapes").clicked() {
                        self.open_trace(id);
                        ui.close();
                    }
                    let fav = self.doc.layers[i].fav;
                    if ui.button(if fav { "Remove from favorites" } else { "Add to favorites" }).help("Take this out of your favorites.").clicked() {
                        self.toggle_favorite(id);
                        ui.close();
                    }
                    ui.separator();
                    if ui.button("Rename").help("Give it another name.").clicked() {
                        self.renaming = Some((id, self.doc.layers[i].name.clone()));
                        ui.close();
                    }
                    if ui.button("Duplicate  (Ctrl+J)").help("Make a copy of the layer (Ctrl+J).").clicked() {
                        self.duplicate_active();
                        ui.close();
                    }
                    if ui.button("Merge down  (Ctrl+E)").help("Join this layer with the one below it (Ctrl+E).").clicked() {
                        self.merge_down_active();
                        ui.close();
                    }
                    if ui.button("Into new folder  (Ctrl+G)").help("Put the picked layers in a new folder (Ctrl+G).").clicked() {
                        self.group_active();
                        ui.close();
                    }
                    let clip_label = if self.doc.layers[i].clip { "Release clipping mask" } else { "Clip to layer below" };
                    if ui.button(clip_label).help("Add this recorded clip to the animation as tracks.").clicked() {
                        self.toggle_clip_active();
                        ui.close();
                    }
                    ui.separator();
                }
                ui.label(RichText::new("Color label").size(12.0).color(TEXT_DIM));
                ui.horizontal(|ui| {
                    for (t, (c, name)) in TAG_COLORS.iter().enumerate() {
                        let (r, cresp) = ui.allocate_exact_size(Vec2::splat(18.0), Sense::click());
                        if t == 0 {
                            ui.painter().circle_stroke(r.center(), 6.0, Stroke::new(1.2, TEXT_DIM));
                            ui.painter().line_segment(
                                [r.center() + vec2(-4.0, 4.0), r.center() + vec2(4.0, -4.0)],
                                Stroke::new(1.2, TEXT_DIM),
                            );
                        } else {
                            ui.painter().circle_filled(r.center(), 7.0, *c);
                        }
                        let current = self.doc.index_of(id).map(|k| self.doc.layers[k].tag as usize);
                        if current == Some(t) {
                            ui.painter().circle_stroke(r.center(), 8.5, Stroke::new(1.5, TEXT));
                        }
                        if cresp.help(*name).clicked() {
                            if let Some(k) = self.doc.index_of(id) {
                                self.doc.layers[k].tag = t as u8;
                            }
                            ui.close();
                        }
                    }
                });
                if kind != LayerKind::Paper {
                    ui.separator();
                    if ui.button(RichText::new("Delete").color(Color32::from_rgb(232, 110, 110))).help("Delete this. Undo brings it back.").clicked() {
                        self.delete_layer(id);
                        ui.close();
                    }
                }
            });
            // The menu may have changed the list: stop drawing rows for this frame.
            if self.doc.layers.get(i).map(|l| l.id) != Some(id) {
                break;
            }

            // Drag & drop (not while the list is filtered or sorted).
            if resp.drag_started() && kind != LayerKind::Paper && !flat {
                self.drag_layer = Some(id);
                self.active = id;
            }
            if resp.drag_stopped() && self.drag_layer == Some(id) {
                drag_released = true;
            }
            ui.add_space(2.0);
        }

        self.layer_rows = rows.iter().map(|(id, r, _)| (*id, *r)).collect();
        if let Some(dragged) = self.drag_layer {
            ctx.set_cursor_icon(CursorIcon::Grabbing);
            let target = ctx.pointer_latest_pos().and_then(|p| drop_target(&rows, p));
            if let Some((drop, rect)) = target {
                let p = ui.painter();
                match drop {
                    Drop::Above(_) => {
                        p.line_segment([rect.left_top() + vec2(4.0, -1.0), rect.right_top() + vec2(-4.0, -1.0)], Stroke::new(2.5, VENOM));
                    }
                    Drop::Below(_) => {
                        p.line_segment(
                            [rect.left_bottom() + vec2(4.0, 1.0), rect.right_bottom() + vec2(-4.0, 1.0)],
                            Stroke::new(2.5, VENOM),
                        );
                    }
                    Drop::Into(_) => {
                        p.rect_stroke(rect, CornerRadius::same(8), Stroke::new(2.0, VENOM), StrokeKind::Inside);
                    }
                }
            }
            // A little name tag follows the pointer.
            if let (Some(pos), Some(k)) = (ctx.pointer_latest_pos(), self.doc.index_of(dragged)) {
                let lp = ctx.layer_painter(egui::LayerId::new(Order::Tooltip, Id::new("layer_drag")));
                let galley = lp.layout_no_wrap(self.doc.layers[k].name.clone(), FontId::proportional(12.5), TEXT);
                let r = Rect::from_min_size(pos + vec2(14.0, 6.0), galley.size() + vec2(16.0, 8.0));
                lp.rect_filled(r, CornerRadius::same(6), EMERALD_DEEP);
                lp.galley(r.min + vec2(8.0, 4.0), galley, TEXT);
            }
            if drag_released || !ctx.input(|i| i.pointer.any_down()) {
                if let Some((drop, _)) = target {
                    if !self.doc.move_layer(dragged, drop) {
                        self.notice = Some("Can't put a layer there.");
                    }
                }
                self.drag_layer = None;
            }
        }

        // Quick actions for the selected layer.
        ui.add_space(4.0);
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing = vec2(4.0, 4.0);
            if widgets::chip(ui, "Duplicate", false).help("Copy this layer (Ctrl+J)").clicked() {
                self.duplicate_active();
            }
            if widgets::chip(ui, "Merge down", false).help("Bake into the layer below (Ctrl+E)").clicked() {
                self.merge_down_active();
            }
            if widgets::chip(ui, "Into folder", false).help("Put this layer in a new folder (Ctrl+G)").clicked() {
                self.group_active();
            }
            ui.menu_button("Adjust", |ui| {
                for fid in crate::effects::ADJUSTMENTS {
                    if let Some(d) = crate::filters::find(fid) {
                        if ui.button(d.label).help(d.about).clicked() {
                            self.add_adjustment(fid);
                            ui.close();
                        }
                    }
                }
            })
            .response
            .help("New adjustment layer: changes the colors of everything below it, and you can undo it any time");
            if widgets::chip(ui, "Depth", false).help("New depth layer: paint white where things are near and black where they are far").clicked() {
                self.add_depth_layer();
            }
            if widgets::chip(ui, "Material", false).help("New material layer: paint a shape and it shines as metal, glass, velvet, neon or a gem").clicked() {
                self.add_material_layer();
            }
            ui.menu_button("Light FX", |ui| {
                for k in [crate::lightfx::LightKind::Shadow, crate::lightfx::LightKind::Glow, crate::lightfx::LightKind::Rays] {
                    if ui.button(k.name()).help(k.about()).clicked() {
                        self.add_light_effect(k);
                        ui.close();
                    }
                }
            })
            .response
            .help("Shadow, glow or rays: paint a shape and the light works on it, live");
            if widgets::chip(ui, "Light", false).help("New light layer: shades the depth layers below it, and you can move the light any time").clicked() {
                self.add_light_layer();
            }
            let clipped = self.doc.layers[self.active_idx()].clip;
            if widgets::chip(ui, "Clip", clipped)
                .help("Clipping mask: show only where the layer below has paint (Ctrl+Alt+G)")
                .clicked()
            {
                self.toggle_clip_active();
            }
        });

        // Settings of the selected layer.
        ui.add_space(6.0);
        let ai = self.active_idx();
        if self.doc.layers[ai].kind == LayerKind::Paper {
            ui.horizontal(|ui| {
                ui.label(RichText::new("Paper color").color(TEXT_DIM).size(12.5));
                egui::color_picker::color_edit_button_srgba(ui, &mut self.doc.paper, egui::color_picker::Alpha::Opaque);
            });
            ui.label(RichText::new("Hide the paper (eye) to work on transparency.").color(TEXT_DIM).size(11.5));
            return;
        }
        let layer = &mut self.doc.layers[ai];
        if layer.kind != LayerKind::Adjust {
            ui.horizontal(|ui| {
                ui.label(RichText::new("Blend").color(TEXT_DIM).size(12.5));
                egui::ComboBox::from_id_salt("blend")
                    .selected_text(layer.blend.name())
                    .width(150.0)
                    .height(420.0)
                    .show_ui(ui, |ui| {
                        if layer.kind == LayerKind::Group {
                            ui.selectable_value(&mut layer.blend, Blend::PassThrough, Blend::PassThrough.name())
                                .help(Blend::PassThrough.about());
                        }
                        for b in Blend::ALL {
                            ui.selectable_value(&mut layer.blend, b, b.name()).help(b.about());
                        }
                    });
            });
            ui.label(RichText::new(layer.blend.about()).color(TEXT_DIM).size(11.5));
        }
        slider_row(ui, "Opacity", &mut layer.opacity, 0.0..=100.0, "%");
        ui.horizontal_wrapped(|ui| {
            if layer.kind == LayerKind::Raster {
                ui.checkbox(&mut layer.lock_alpha, "Lock alpha").help("Paint only where there is already paint");
            }
            ui.checkbox(&mut layer.locked, "Lock").help("No painting or moving");
        });
        ui.label(RichText::new("Double-click a layer to rename. Drag to reorder or drop onto a folder.").color(TEXT_DIM).size(11.0));
        self.effects_section(ui);
    }
}

/// Which drop position the pointer is over: folders take the middle half as "into".
fn drop_target(rows: &[(LayerId, Rect, LayerKind)], p: egui::Pos2) -> Option<(Drop, Rect)> {
    let (id, rect, kind) = rows.iter().find(|(_, r, _)| p.y >= r.top() - 1.0 && p.y <= r.bottom() + 1.0).copied().or_else(|| {
        // Below the last row: drop above the paper (the bottom-most place allowed).
        let last = rows.last()?;
        (p.y > last.1.bottom()).then_some(*last)
    })?;
    let t = ((p.y - rect.top()) / rect.height()).clamp(0.0, 1.0);
    let drop = match kind {
        LayerKind::Paper => Drop::Above(id),
        LayerKind::Group if (0.25..0.75).contains(&t) => Drop::Into(id),
        _ if t < 0.5 => Drop::Above(id),
        _ => Drop::Below(id),
    };
    Some((drop, rect))
}
