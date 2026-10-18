// SPDX-License-Identifier: GPL-3.0-or-later
// The Parts tab: layers on the left, the picture in the middle with color codes, tags on the right.
// "L" is the character's own left, which is on the right side of the picture.

use std::collections::HashMap;

use eframe::egui::{self, Color32, Pos2, Rect, RichText, Sense, Stroke, TextureHandle, TextureOptions, Ui, Vec2, pos2, vec2};

use super::SeviathApp;
use crate::doc::{Document, LayerId, TILE};
use crate::history::DocState;
use crate::parts::{self, Check, Group, Level, Part, Suggestion, Template};
use crate::tabs::Tab;
use crate::theme::*;
use crate::widgets::{self, section_title};

const PREVIEW: u32 = 1024;

#[derive(Default)]
pub struct PartsState {
    pub selected: Vec<LayerId>,
    anchor: Option<LayerId>,
    pub suggestions: Vec<Suggestion>,
    /// Look at layers that already have a tag too.
    redo_tagged: bool,
    codes: bool,
    zoom: f32,
    pan: Vec2,
    base: Option<TextureHandle>,
    overlay: Option<TextureHandle>,
    /// The document revision the pictures were made from.
    made_for: Option<(u64, usize, u32, u32)>,
    scale: f32,
    bounds: HashMap<(LayerId, u64), Option<(i32, i32, i32, i32)>>,
    templates: Vec<Template>,
    templates_loaded: bool,
    tpl_name: String,
    message: Option<String>,
    /// A layer the checklist asked to look at.
    flash: Option<LayerId>,
}

fn layer_pixel(l: &crate::doc::Layer, x: i32, y: i32) -> bool {
    let t = TILE as i32;
    l.tiles.get(&(x.div_euclid(t), y.div_euclid(t))).is_some_and(|tile| tile[(y.rem_euclid(t) * t + x.rem_euclid(t)) as usize].a() > 8)
}

/// The color codes: every tagged layer tinted in its part's color, at preview size.
fn overlay_image(d: &Document, scale: f32, w: u32, h: u32) -> egui::ColorImage {
    let mut img = egui::ColorImage::filled([w as usize, h as usize], Color32::TRANSPARENT);
    let t = TILE as i32;
    // bottom layer first, so the top layer shows where they overlap
    for l in d.layers.iter().rev().filter(|l| l.visible && l.part.is_some()) {
        let c = l.part.map(|p| p.color()).unwrap_or(Color32::WHITE);
        let tint = Color32::from_rgba_premultiplied((c.r() as f32 * 0.55) as u8, (c.g() as f32 * 0.55) as u8, (c.b() as f32 * 0.55) as u8, 140);
        for (key, tile) in &l.tiles {
            for j in 0..t {
                for i in 0..t {
                    if tile[(j * t + i) as usize].a() > 8 {
                        let (x, y) = (((key.0 * t + i) as f32 * scale) as usize, ((key.1 * t + j) as f32 * scale) as usize);
                        if x < w as usize && y < h as usize {
                            img.pixels[y * w as usize + x] = tint;
                        }
                    }
                }
            }
        }
    }
    // the preview is smaller than the page: close the pinholes between sampled pixels
    if scale < 0.999 {
        let src = img.pixels.clone();
        let wi = w as usize;
        for y in 1..h as usize - 1 {
            for x in 1..wi - 1 {
                if src[y * wi + x] == Color32::TRANSPARENT {
                    let n = [src[y * wi + x - 1], src[y * wi + x + 1], src[(y - 1) * wi + x], src[(y + 1) * wi + x]];
                    if let Some(c) = n.iter().find(|c| **c != Color32::TRANSPARENT) {
                        if n.iter().filter(|c| **c != Color32::TRANSPARENT).count() >= 2 {
                            img.pixels[y * wi + x] = *c;
                        }
                    }
                }
            }
        }
    }
    img
}

impl SeviathApp {
    /// Change the document from the Parts tab as one undo step.
    fn parts_edit(&mut self, label: &str, now: f64, f: impl FnOnce(&mut Document)) {
        let before: DocState = self.draw.doc.snapshot();
        f(&mut self.draw.doc);
        if self.draw.doc.same_as(&before) {
            return;
        }
        let after = self.draw.doc.snapshot();
        self.draw.doc.mark_all_dirty();
        self.recorder_step(&after);
        self.log_step(label, now);
        self.history.push(label.to_owned(), Tab::Parts, before, after, now);
    }

    fn tag_selected(&mut self, part: Option<Part>, now: f64) {
        let sel = self.parts.selected.clone();
        if sel.is_empty() {
            return;
        }
        let label = match part {
            Some(p) => format!("Tag · {}", p.label()),
            None => "Clear tag".to_owned(),
        };
        self.parts_edit(&label, now, |d| {
            for l in d.layers.iter_mut().filter(|l| sel.contains(&l.id) && parts::is_part_layer(l)) {
                l.part = part;
            }
        });
    }

    #[cfg(test)]
    pub fn test_parts(&mut self) -> &mut PartsState {
        &mut self.parts
    }

    #[cfg(test)]
    pub fn test_tag(&mut self, ids: &[LayerId], part: Option<Part>) {
        self.parts.selected = ids.to_vec();
        self.tag_selected(part, 0.0);
    }

    /// Make the pictures for the middle when the drawing changed.
    fn parts_pictures(&mut self, ctx: &egui::Context) {
        let d = &self.draw.doc;
        let key = (d.revision, d.layers.iter().filter(|l| l.part.is_some()).count() + d.layers.len() * 1000, d.w, d.h);
        let tags_sig = d.layers.iter().fold(0usize, |a, l| a.wrapping_mul(31).wrapping_add(l.part.map(|p| p as usize + 1).unwrap_or(0)).wrapping_add(l.visible as usize));
        let key = (key.0, key.1 ^ tags_sig, key.2, key.3);
        if self.parts.made_for == Some(key) && self.parts.base.is_some() {
            return;
        }
        let img = crate::files::thumbnail(d, PREVIEW);
        let scale = img.width() as f32 / d.w as f32;
        let (w, h) = (img.width(), img.height());
        let base = egui::ColorImage::from_rgba_unmultiplied([w as usize, h as usize], img.as_raw());
        let over = overlay_image(d, scale, w, h);
        self.parts.base = Some(ctx.load_texture("parts_base", base, TextureOptions::LINEAR));
        self.parts.overlay = Some(ctx.load_texture("parts_codes", over, TextureOptions::LINEAR));
        self.parts.scale = scale;
        self.parts.made_for = Some(key);
        self.parts.bounds.clear();
    }

    /// The topmost visible tagged-or-not pixel layer under a point of the page.
    fn part_layer_at(&self, x: i32, y: i32) -> Option<LayerId> {
        self.draw.doc.layers.iter().find(|l| l.visible && parts::is_part_layer(l) && layer_pixel(l, x, y)).map(|l| l.id)
    }

    fn select_layer(&mut self, id: LayerId, ctrl: bool, shift: bool) {
        let p = &mut self.parts;
        let order: Vec<LayerId> = self.draw.doc.layers.iter().filter(|l| parts::is_part_layer(l)).map(|l| l.id).collect();
        if shift && p.anchor.is_some() {
            let (a, b) = (order.iter().position(|x| Some(*x) == p.anchor), order.iter().position(|x| *x == id));
            if let (Some(a), Some(b)) = (a, b) {
                p.selected = order[a.min(b)..=a.max(b)].to_vec();
            }
        } else if ctrl {
            if let Some(i) = p.selected.iter().position(|x| *x == id) {
                p.selected.remove(i);
            } else {
                p.selected.push(id);
            }
            p.anchor = Some(id);
        } else {
            p.selected = vec![id];
            p.anchor = Some(id);
        }
        self.draw.focus_layer(id);
    }

    pub(super) fn parts_ui(&mut self, ui: &mut Ui) {
        let ctx = ui.ctx().clone();
        let now = ctx.input(|i| i.time);
        if !self.parts.templates_loaded {
            self.parts.templates = parts::list_templates_in(&parts::templates_dir());
            self.parts.templates_loaded = true;
            self.parts.codes = true;
        }
        self.parts.selected.retain(|id| self.draw.doc.index_of(*id).is_some());
        self.parts_pictures(&ctx);
        let (mut act_tag, mut act_clear) = (None::<Part>, false);
        let mut picked: Option<(LayerId, bool, bool)> = None;

        egui::Panel::left("parts_layers").default_size(250.0).size_range(200.0..=420.0).resizable(true).frame(egui::Frame::new().fill(PANEL).inner_margin(egui::Margin::same(10))).show(ui, |ui| {
            section_title(ui, "Layers");
            let tagged = self.draw.doc.layers.iter().filter(|l| l.part.is_some()).count();
            let total = self.draw.doc.layers.iter().filter(|l| parts::is_part_layer(l) && (!l.tiles.is_empty() || l.vector.is_some() || l.part.is_some())).count();
            ui.label(RichText::new(format!("{tagged} of {total} tagged")).size(11.5).color(TEXT_DIM));
            egui::ScrollArea::vertical().id_salt("parts_layer_list").show(ui, |ui| {
                for l in self.draw.doc.layers.iter().filter(|l| parts::is_part_layer(l)) {
                    let on = self.parts.selected.contains(&l.id);
                    let (rect, resp) = ui.allocate_exact_size(vec2(ui.available_width(), 24.0), Sense::click());
                    if on {
                        ui.painter().rect_filled(rect, 4.0, HOVER);
                    } else if resp.hovered() {
                        ui.painter().rect_filled(rect, 4.0, CARD);
                    }
                    if self.parts.flash == Some(l.id) {
                        ui.painter().rect_stroke(rect, 4.0, Stroke::new(1.5, VENOM), egui::StrokeKind::Inside);
                    }
                    let dot = l.part.map(|p| p.color()).unwrap_or(Color32::from_gray(70));
                    ui.painter().circle_filled(rect.left_center() + vec2(10.0, 0.0), 5.0, dot);
                    let name_col = if l.visible { TEXT } else { TEXT_DIM };
                    ui.painter().text(rect.left_center() + vec2(22.0, 0.0), egui::Align2::LEFT_CENTER, &l.name, egui::FontId::proportional(13.0), name_col);
                    if let Some(p) = l.part {
                        ui.painter().text(rect.right_center() - vec2(6.0, 0.0), egui::Align2::RIGHT_CENTER, p.label(), egui::FontId::proportional(11.0), p.color());
                    }
                    if resp.clicked() {
                        let m = ctx.input(|i| i.modifiers);
                        picked = Some((l.id, m.command || m.ctrl, m.shift));
                    }
                }
            });
        });

        egui::Panel::right("parts_tags").default_size(310.0).size_range(260.0..=480.0).resizable(true).frame(egui::Frame::new().fill(PANEL).inner_margin(egui::Margin::same(10))).show(ui, |ui| {
            egui::ScrollArea::vertical().id_salt("parts_right").show(ui, |ui| self.parts_side(ui, now, &mut act_tag, &mut act_clear, &mut picked));
        });

        egui::CentralPanel::default().frame(egui::Frame::new().fill(Color32::from_rgb(14, 20, 18))).show(ui, |ui| self.parts_model(ui, &mut picked));

        if let Some((id, c, s)) = picked {
            self.select_layer(id, c, s);
            self.parts.flash = None;
        }
        if let Some(p) = act_tag {
            self.tag_selected(Some(p), now);
        }
        if act_clear {
            self.tag_selected(None, now);
        }
    }

    fn parts_side(&mut self, ui: &mut Ui, now: f64, act_tag: &mut Option<Part>, act_clear: &mut bool, picked: &mut Option<(LayerId, bool, bool)>) {
        // 1. tags
        section_title(ui, "Tag the selected layers");
        let n = self.parts.selected.len();
        ui.label(RichText::new(if n == 0 { "Pick layers on the left or click the picture.".to_owned() } else { format!("{n} selected") }).size(11.5).color(TEXT_DIM));
        let current: Option<Part> = self.parts.selected.first().and_then(|id| self.draw.doc.layers.iter().find(|l| l.id == *id)).and_then(|l| l.part);
        for g in Group::ALL {
            ui.label(RichText::new(g.label()).size(12.0).color(TEXT_DIM));
            ui.horizontal_wrapped(|ui| {
                ui.spacing_mut().item_spacing = vec2(4.0, 4.0);
                for p in Part::ALL.iter().filter(|p| p.group() == g) {
                    if widgets::chip(ui, p.label(), current == Some(*p)).on_hover_text(if p.is_left() { "L is the character's left: the right side of the picture" } else { "" }).clicked() {
                        *act_tag = Some(*p);
                    }
                }
            });
        }
        if ui.add_enabled(n > 0, egui::Button::new("Clear the tag")).clicked() {
            *act_clear = true;
        }

        // 2. suggestions
        section_title(ui, "Suggestions");
        ui.horizontal(|ui| {
            if ui.button("Suggest tags").on_hover_text("From the layer names, their shape and where they are on the page").clicked() {
                let again = self.parts.redo_tagged;
                self.parts.suggestions = parts::suggest(&self.draw.doc.layers, self.draw.doc.w, self.draw.doc.h, again);
                self.parts.message = Some(if self.parts.suggestions.is_empty() { "Nothing to suggest.".into() } else { format!("{} suggestions.", self.parts.suggestions.len()) });
            }
            ui.checkbox(&mut self.parts.redo_tagged, "Also tagged ones");
        });
        if let Some(m) = &self.parts.message {
            ui.label(RichText::new(m).size(11.5).color(TEXT_DIM));
        }
        let mut accept: Vec<Suggestion> = Vec::new();
        let mut dismiss: Option<usize> = None;
        for (i, s) in self.parts.suggestions.iter().enumerate() {
            let name = self.draw.doc.layers.iter().find(|l| l.id == s.layer).map(|l| l.name.clone()).unwrap_or_default();
            ui.horizontal(|ui| {
                ui.label(RichText::new(if s.confidence >= 0.7 { "sure" } else { "maybe" }).size(11.0).color(if s.confidence >= 0.7 { VENOM } else { TEXT_DIM }));
                let r = ui.label(format!("{name}: {}", s.part.label()));
                r.on_hover_text(&s.reason);
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.small_button("No").clicked() {
                        dismiss = Some(i);
                    }
                    if ui.small_button("Yes").clicked() {
                        accept.push(s.clone());
                    }
                });
            });
        }
        if !self.parts.suggestions.is_empty() {
            ui.horizontal(|ui| {
                if ui.button("Accept the sure ones").clicked() {
                    accept = self.parts.suggestions.iter().filter(|s| s.confidence >= 0.7).cloned().collect();
                }
                if ui.button("Accept all").clicked() {
                    accept = self.parts.suggestions.clone();
                }
            });
        }
        if let Some(i) = dismiss {
            self.parts.suggestions.remove(i);
        }
        if !accept.is_empty() {
            let n = accept.len();
            self.parts_edit(&format!("Tag {n} layers (suggested)"), now, |d| {
                for s in &accept {
                    if let Some(l) = d.layers.iter_mut().find(|l| l.id == s.layer) {
                        l.part = Some(s.part);
                    }
                }
            });
            let done: Vec<LayerId> = accept.iter().map(|s| s.layer).collect();
            self.parts.suggestions.retain(|s| !done.contains(&s.layer));
        }

        // 3. pairs
        let pairs = parts::pairs(&self.draw.doc.layers);
        if !pairs.is_empty() {
            section_title(ui, "Left and right");
            for (left, l, r) in &pairs {
                let right = left.mirror().unwrap_or(*left);
                ui.horizontal(|ui| {
                    let base = left.label().trim_end_matches(" L").to_owned();
                    ui.label(format!("{base}:"));
                    for (part, id) in [(*left, l), (right, r)] {
                        match id {
                            Some(id) => {
                                if ui.add(egui::Button::new(RichText::new(part.label()).color(part.color())).small()).clicked() {
                                    *picked = Some((*id, false, false));
                                }
                            }
                            None => {
                                ui.label(RichText::new(format!("{} missing", part.label())).color(Color32::from_rgb(235, 110, 90)).size(12.0));
                            }
                        }
                    }
                });
            }
        }

        // 4. checklist
        section_title(ui, "Checklist");
        let checks: Vec<Check> = parts::checklist(&self.draw.doc.layers);
        if checks.is_empty() {
            ui.label(RichText::new("Nothing is missing.").color(VENOM));
        }
        for c in &checks {
            ui.horizontal_wrapped(|ui| {
                ui.label(RichText::new(if c.level == Level::Needs { "Must" } else { "Tip" }).size(11.0).color(if c.level == Level::Needs { Color32::from_rgb(235, 110, 90) } else { TEXT_DIM }));
                ui.label(RichText::new(&c.text).size(12.5));
                if let Some(id) = c.jump {
                    if ui.small_button("Show").on_hover_text("Pick the layer here").clicked() {
                        *picked = Some((id, false, false));
                        self.parts.flash = Some(id);
                    }
                    if ui.small_button("Draw tab").on_hover_text("Go to this layer in the Draw tab").clicked() {
                        self.draw.focus_layer(id);
                        self.tab = Tab::Draw;
                    }
                }
            });
        }

        // 5. templates
        section_title(ui, "Tag templates");
        ui.horizontal(|ui| {
            ui.add(egui::TextEdit::singleline(&mut self.parts.tpl_name).hint_text("Template name").desired_width(140.0).char_limit(40));
            if ui.button("Save").on_hover_text("Keeps the tags by layer name, to use on another project").clicked() {
                let t = parts::template_from(&self.parts.tpl_name, &self.draw.doc.layers);
                self.parts.message = Some(match parts::save_template_in(&parts::templates_dir(), &t) {
                    Ok(()) => {
                        self.parts.templates = parts::list_templates_in(&parts::templates_dir());
                        format!("Template “{}” saved.", t.name)
                    }
                    Err(e) => e,
                });
            }
        });
        let mut apply: Option<Template> = None;
        let mut delete: Option<String> = None;
        for t in &self.parts.templates {
            ui.horizontal(|ui| {
                ui.label(format!("{} ({} tags)", t.name, t.tags.len()));
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.small_button("Delete").clicked() {
                        delete = Some(t.name.clone());
                    }
                    if ui.small_button("Use").on_hover_text("Tags the layers that have the same names").clicked() {
                        apply = Some(t.clone());
                    }
                });
            });
        }
        if let Some(name) = delete {
            parts::delete_template_in(&parts::templates_dir(), &name);
            self.parts.templates = parts::list_templates_in(&parts::templates_dir());
        }
        if let Some(t) = apply {
            let mut n = 0;
            self.parts_edit(&format!("Use template · {}", t.name), now, |d| n = parts::apply_template(&mut d.layers, &t, false));
            self.parts.message = Some(format!("{n} layers tagged from “{}”.", t.name));
        }

        // 6. ready
        ui.add_space(8.0);
        let must = checks.iter().any(|c| c.level == Level::Needs);
        let ready = self.draw.doc.meta.parts_ready;
        ui.horizontal(|ui| {
            let label = if ready { "Ready to rig" } else { "Mark as ready to rig" };
            if ui.add_enabled(!must && !ready, egui::Button::new(RichText::new(label).strong())).on_disabled_hover_text(if ready { "Tags can still change later" } else { "Fix the lines marked Must first" }).clicked() {
                self.parts_edit("Ready to rig", now, |d| d.meta.parts_ready = true);
            }
            if ready && ui.small_button("Not ready").clicked() {
                self.parts_edit("Not ready to rig", now, |d| d.meta.parts_ready = false);
            }
        });
        if must {
            ui.label(RichText::new("Some lines say Must: tag those layers first.").size(11.5).color(TEXT_DIM));
        }
    }

    fn parts_model(&mut self, ui: &mut Ui, picked: &mut Option<(LayerId, bool, bool)>) {
        let rect = ui.available_rect_before_wrap();
        let resp = ui.allocate_rect(rect, Sense::click_and_drag());
        let (dw, dh) = (self.draw.doc.w as f32, self.draw.doc.h as f32);
        if self.parts.zoom <= 0.0 {
            self.parts.zoom = 1.0;
        }
        let fit = ((rect.width() - 24.0) / dw).min((rect.height() - 24.0) / dh).max(0.01);
        let sc = fit * self.parts.zoom;
        let centre = rect.center() + self.parts.pan;
        let page = Rect::from_center_size(centre, vec2(dw * sc, dh * sc));
        // wheel zoom around the pointer, right or middle drag moves the picture
        if resp.hovered() {
            let (scroll, pos) = ui.input(|i| (i.smooth_scroll_delta.y, i.pointer.hover_pos()));
            if scroll != 0.0 {
                if let Some(p) = pos {
                    let k = (1.0 + scroll * 0.002).clamp(0.8, 1.25);
                    let nz = (self.parts.zoom * k).clamp(0.3, 12.0);
                    let k = nz / self.parts.zoom;
                    self.parts.pan = (p - rect.center()) - (p - rect.center() - self.parts.pan) * k;
                    self.parts.zoom = nz;
                }
            }
        }
        if resp.dragged_by(egui::PointerButton::Secondary) || resp.dragged_by(egui::PointerButton::Middle) {
            self.parts.pan += resp.drag_delta();
        }
        let p = ui.painter_at(rect);
        p.rect_filled(page.expand(1.0), 0.0, Color32::from_gray(40));
        if let Some(t) = &self.parts.base {
            p.image(t.id(), page, Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)), Color32::WHITE);
        }
        if self.parts.codes {
            if let Some(t) = &self.parts.overlay {
                p.image(t.id(), page, Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)), Color32::WHITE);
            }
        }
        // the selected layers: a frame around what they hold
        let rev = self.draw.doc.revision;
        let sel = self.parts.selected.clone();
        for id in sel {
            let key = (id, rev);
            let b = match self.parts.bounds.get(&key) {
                Some(b) => *b,
                None => {
                    let b = self.draw.doc.layers.iter().find(|l| l.id == id).and_then(|l| crate::files::layer_bounds(l, self.draw.doc.w, self.draw.doc.h));
                    self.parts.bounds.insert(key, b);
                    b
                }
            };
            if let Some((x0, y0, x1, y1)) = b {
                let r = Rect::from_min_max(page.min + vec2(x0 as f32, y0 as f32) * sc, page.min + vec2(x1 as f32, y1 as f32) * sc);
                p.rect_stroke(r, 2.0, Stroke::new(2.0, VENOM), egui::StrokeKind::Outside);
            }
        }
        // hover: the name of the layer under the pointer; click picks it
        let to_page = |s: Pos2| ((s.x - page.min.x) / sc, (s.y - page.min.y) / sc);
        if let Some(pos) = resp.hover_pos().filter(|p| page.contains(*p)) {
            let (x, y) = to_page(pos);
            if let Some(id) = self.part_layer_at(x as i32, y as i32) {
                if let Some(l) = self.draw.doc.layers.iter().find(|l| l.id == id) {
                    let tag = l.part.map(|p| p.label()).unwrap_or("no tag");
                    egui::Area::new(egui::Id::new("parts_tip")).order(egui::Order::Tooltip).fixed_pos(pos + vec2(14.0, 14.0)).show(ui.ctx(), |ui| {
                        egui::Frame::popup(ui.style()).show(ui, |ui| ui.label(format!("{} ({tag})", l.name)));
                    });
                }
            }
        }
        if resp.clicked_by(egui::PointerButton::Primary) {
            if let Some(pos) = resp.interact_pointer_pos().filter(|p| page.contains(*p)) {
                let (x, y) = to_page(pos);
                if let Some(id) = self.part_layer_at(x as i32, y as i32) {
                    let m = ui.input(|i| i.modifiers);
                    *picked = Some((id, m.command || m.ctrl, m.shift));
                }
            }
        }
        // the small bar on top of the picture
        egui::Area::new(egui::Id::new("parts_bar")).fixed_pos(rect.min + vec2(10.0, 8.0)).order(egui::Order::Foreground).show(ui.ctx(), |ui| {
            ui.horizontal(|ui| {
                ui.checkbox(&mut self.parts.codes, "Color codes");
                if ui.small_button("Fit").clicked() {
                    self.parts.zoom = 1.0;
                    self.parts.pan = Vec2::ZERO;
                }
                ui.label(RichText::new("Wheel: zoom. Right drag: move.").size(11.0).color(TEXT_DIM));
            });
        });
        // the key to the colors
        egui::Area::new(egui::Id::new("parts_key")).fixed_pos(rect.left_bottom() + vec2(10.0, -44.0)).order(egui::Order::Foreground).show(ui.ctx(), |ui| {
            ui.horizontal(|ui| {
                for g in Group::ALL {
                    let c = Part::ALL.iter().find(|p| p.group() == g).map(|p| p.color()).unwrap_or(Color32::WHITE);
                    let (r, _) = ui.allocate_exact_size(vec2(10.0, 10.0), Sense::hover());
                    ui.painter().rect_filled(r, 2.0, c);
                    ui.label(RichText::new(g.label()).size(11.0).color(TEXT_DIM));
                }
            });
        });
    }
}
