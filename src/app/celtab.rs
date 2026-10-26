// SPDX-License-Identifier: GPL-3.0-or-later
// Frame by frame animation in the Animate tab. A cel track is a folder of layers: every layer is one drawing (a cel), drawn in the Draw
// tab, and the track says which drawing shows from which frame on. The Animate tab shows the picture put together from the layers
// (the drawings of the frame, onion skin of the frames around it), plays it, and saves it like the rig animation. A character that is
// both rigged and drawn frame by frame works as well: the rig (posed by the sliders and their tracks) is the base and the drawings are over it.

use crate::widgets::HelpExt;
use std::hash::{Hash, Hasher};

use eframe::egui::{self, Color32, Pos2, Rect, RichText, Sense, Stroke, Ui, pos2, vec2};

use super::SeviathApp;
use super::animtab::ROW;
use crate::doc::LayerKind;
use crate::theme::*;
use crate::widgets::{self, section_title};

/// Put `top` over `base` (both the same size, not premultiplied).
pub(super) fn composite_over(base: &mut image::RgbaImage, top: &image::RgbaImage) {
    for (b, t) in base.pixels_mut().zip(top.pixels()) {
        let ta = t.0[3] as f32 / 255.0;
        if ta <= 0.0 {
            continue;
        }
        let ba = b.0[3] as f32 / 255.0;
        let oa = ta + ba * (1.0 - ta);
        for k in 0..3 {
            b.0[k] = ((t.0[k] as f32 * ta + b.0[k] as f32 * ba * (1.0 - ta)) / oa).round().clamp(0.0, 255.0) as u8;
        }
        b.0[3] = (oa * 255.0).round() as u8;
    }
}

/// A texture (the whole of it) on four corners, which may be turned and scaled; the default texture is a plain colour.
pub(super) fn tex_quad(p: &egui::Painter, tex: egui::TextureId, corners: [Pos2; 4], tint: Color32) {
    let mut m = egui::Mesh::with_texture(tex);
    let uvs = [pos2(0.0, 0.0), pos2(1.0, 0.0), pos2(1.0, 1.0), pos2(0.0, 1.0)];
    for (pos, uv) in corners.iter().zip(uvs) {
        m.vertices.push(egui::epaint::Vertex { pos: *pos, uv: if tex == egui::TextureId::default() { egui::epaint::WHITE_UV } else { uv }, color: tint });
    }
    m.add_triangle(0, 1, 2);
    m.add_triangle(0, 2, 3);
    p.add(egui::Shape::mesh(m));
}

/// The biggest side of the pictures the preview keeps.
const PREVIEW_SIDE: f32 = 1100.0;
const MAX_CACHED: usize = 90;

impl SeviathApp {
    pub(super) fn cel_mode(&self) -> bool {
        !self.draw.doc.rig.anim.cel_tracks.is_empty()
    }

    /// The character is rigged too: its picture (posed by the sliders) is the base, and the cel drawings are drawn over it.
    pub(super) fn has_rig(&self) -> bool {
        !self.draw.doc.rig.meshes.is_empty()
    }

    /// Whether each drawing shows at a frame (drawings whose layer is gone are left out).
    fn cel_vis(&self, frame: u32) -> Vec<(u64, bool)> {
        self.draw.doc.rig.anim.cel_visibility(frame).into_iter().filter(|(id, _)| self.draw.doc.index_of(*id).is_some()).collect()
    }

    /// Show the drawings of a frame on the layers (the layers go back as they were when the animation stops).
    pub(super) fn cel_apply(&mut self, frame: f32) {
        let vis = self.cel_vis(frame.round().max(0.0) as u32);
        if vis.is_empty() {
            return;
        }
        if self.rigs.anim.base.is_none() {
            self.rigs.anim.base = Some(self.draw.doc.layers.iter().map(|l| (l.id, (l.opacity, l.visible))).collect());
        }
        let mut changed = false;
        for (id, v) in vis {
            if let Some(l) = self.draw.doc.layers.iter_mut().find(|l| l.id == id) {
                if l.visible != v {
                    l.visible = v;
                    changed = true;
                }
            }
        }
        if changed {
            self.draw.doc.mark_all_dirty();
        }
    }

    /// What the layers look like with these drawings shown and the paper left out: a key for the picture cache.
    fn cel_key(&self, vis: &[(u64, bool)]) -> u64 {
        let mut h = std::collections::hash_map::DefaultHasher::new();
        for l in &self.draw.doc.layers {
            let shown = vis.iter().find(|(id, _)| *id == l.id).map(|(_, v)| *v).unwrap_or(l.visible);
            // a layer that is not shown does not matter, so drawing on one drawing leaves the pictures of the other frames as they are
            let rev = if shown { l.rev } else { 0 };
            (l.id, shown, l.opacity.to_bits(), rev, l.kind as u8, l.parent).hash(&mut h);
        }
        (self.draw.doc.w, self.draw.doc.h).hash(&mut h);
        h.finish()
    }

    /// The picture of the layers with `vis` set (and the paper hidden), at full size. The layers are put back.
    fn cel_flatten(&mut self, vis: &[(u64, bool)]) -> image::RgbaImage {
        let saved: Vec<(u64, bool)> = self.draw.doc.layers.iter().map(|l| (l.id, l.visible)).collect();
        // the layers of a rigged character are drawn by the rig, not here
        let rigged: std::collections::HashSet<u64> = self.draw.doc.rig.meshes.iter().map(|m| m.layer).collect();
        for l in self.draw.doc.layers.iter_mut() {
            if l.kind == LayerKind::Paper || rigged.contains(&l.id) {
                l.visible = false;
            } else if let Some((_, v)) = vis.iter().find(|(id, _)| *id == l.id) {
                l.visible = *v;
            }
        }
        let img = crate::files::flatten(&self.draw.doc);
        for (l, (_, v)) in self.draw.doc.layers.iter_mut().zip(saved) {
            l.visible = v;
        }
        img
    }

    /// The picture of a frame for the preview (kept, so playing a second time is quick).
    fn cel_texture(&mut self, ctx: &egui::Context, frame: u32) -> Option<egui::TextureHandle> {
        let vis = self.cel_vis(frame);
        let key = self.cel_key(&vis);
        if let Some(t) = self.rigs.anim.cel_cache.get(&key) {
            return Some(t.clone());
        }
        if self.rigs.anim.cel_cache.len() >= MAX_CACHED {
            self.rigs.anim.cel_cache.clear();
        }
        let full = self.cel_flatten(&vis);
        let k = (PREVIEW_SIDE / full.width().max(full.height()) as f32).min(1.0);
        let img = if k < 1.0 { image::imageops::resize(&full, ((full.width() as f32 * k) as u32).max(1), ((full.height() as f32 * k) as u32).max(1), image::imageops::FilterType::Triangle) } else { full };
        let ci = egui::ColorImage::from_rgba_unmultiplied([img.width() as usize, img.height() as usize], img.as_raw());
        let tex = ctx.load_texture(format!("cel_{key}"), ci, egui::TextureOptions::LINEAR);
        self.rigs.anim.cel_cache.insert(key, tex.clone());
        Some(tex)
    }

    /// The picture of the frame that is applied now, `size` pixels, with `paper` behind it (None: see-through). For saving.
    pub(super) fn cel_picture(&mut self, size: (u32, u32), paper: Option<Color32>) -> image::RgbaImage {
        let vis: Vec<(u64, bool)> = Vec::new();
        let full = self.cel_flatten(&vis);
        let mut img = image::imageops::resize(&full, size.0, size.1, image::imageops::FilterType::Triangle);
        if let Some(p) = paper {
            let [pr, pg, pb, _] = p.to_array();
            for px in img.pixels_mut() {
                let a = px.0[3] as u32;
                for (c, back) in px.0[..3].iter_mut().zip([pr, pg, pb]) {
                    *c = ((*c as u32 * a + back as u32 * (255 - a) + 127) / 255) as u8;
                }
                px.0[3] = 255;
            }
        }
        img
    }

    /// The preview of a cel animation: paper, the frames around the one shown as onion skin, then the frame itself.
    pub(super) fn cel_preview(&mut self, ui: &mut Ui, rect: Rect) {
        let ctx = ui.ctx().clone();
        let (w, h) = (self.draw.doc.w as f32, self.draw.doc.h as f32);
        let fit = ((rect.width() - 24.0) / w).min((rect.height() - 24.0) / h).max(0.01) * self.rigs.anim.view_zoom;
        let page = Rect::from_center_size(rect.center(), vec2(w, h) * fit);
        let p = ui.painter_at(rect).with_clip_rect(page.intersect(rect));
        let corners = self.camera_corners(page);
        tex_quad(&p, egui::TextureId::default(), corners, Color32::from_rgb(244, 242, 236));
        if self.has_rig() {
            if let Some(t) = self.rigs.image.clone() {
                tex_quad(&p, t.id(), corners, Color32::WHITE);
            }
        }
        let frame = self.rigs.anim.frame.round().max(0.0) as u32;
        let onion = self.draw.doc.rig.anim.onion;
        if onion.on && !self.rigs.anim.playing {
            for (n, dir, tint) in [(onion.before, -1i64, (255u8, 110u8, 110u8)), (onion.after, 1, (90, 200, 120))] {
                for k in (1..=n as i64).rev() {
                    let f = frame as i64 + dir * k;
                    if f < 0 || f >= self.draw.doc.rig.anim.length as i64 {
                        continue;
                    }
                    let alpha = (onion.opacity * (1.0 - (k - 1) as f32 / (n as f32 + 0.5)) * 255.0).clamp(0.0, 255.0) as u8;
                    if let Some(t) = self.cel_texture(&ctx, f as u32) {
                        tex_quad(&p, t.id(), corners, Color32::from_rgba_unmultiplied(tint.0, tint.1, tint.2, alpha));
                    }
                }
            }
        }
        if let Some(t) = self.cel_texture(&ctx, frame) {
            tex_quad(&p, t.id(), corners, Color32::WHITE);
        }
    }

    /// The Draw tab shows the frames around the playhead faintly, to draw the next drawing by.
    pub(super) fn cel_onion_for_draw(&mut self, ctx: &egui::Context) {
        self.draw.onion.clear();
        let o = self.draw.doc.rig.anim.onion;
        if !self.cel_mode() || !o.on {
            return;
        }
        let frame = self.rigs.anim.frame.round().max(0.0) as u32;
        let here = self.cel_key(&self.cel_vis(frame));
        let mut seen = vec![here];
        for (n, dir, tint) in [(o.before, -1i64, (255u8, 110u8, 110u8)), (o.after, 1, (90, 200, 120))] {
            for k in 1..=n as i64 {
                let f = frame as i64 + dir * k;
                if f < 0 || f >= self.draw.doc.rig.anim.length as i64 {
                    continue;
                }
                // a frame that looks like one that is already there (a held drawing) is not put again
                let key = self.cel_key(&self.cel_vis(f as u32));
                if seen.contains(&key) {
                    continue;
                }
                seen.push(key);
                let alpha = (o.opacity * (1.0 - (k - 1) as f32 / (n as f32 + 0.5)) * 255.0).clamp(0.0, 255.0) as u8;
                if let Some(t) = self.cel_texture(ctx, f as u32) {
                    self.draw.onion.push((t, Color32::from_rgba_unmultiplied(tint.0, tint.1, tint.2, alpha)));
                }
            }
        }
    }

    /// A small bar over the Draw tab canvas: which frame it is, and buttons to go to the one before or after (the drawing that shows
    /// there is picked to draw on).
    pub(super) fn cel_frame_bar(&mut self, ctx: &egui::Context) {
        if !self.cel_mode() {
            return;
        }
        let frame = self.rigs.anim.frame.round().max(0.0) as i64;
        let last = self.draw.doc.rig.anim.length as i64 - 1;
        let mut go: Option<i64> = None;
        let mut onion = self.draw.doc.rig.anim.onion.on;
        egui::Area::new(egui::Id::new("cel_frame_bar")).anchor(egui::Align2::CENTER_TOP, vec2(0.0, 64.0)).order(egui::Order::Foreground).show(ctx, |ui| {
            egui::Frame::new().fill(PANEL.gamma_multiply(0.94)).corner_radius(8).inner_margin(egui::Margin::symmetric(8, 4)).stroke(Stroke::new(1.0, Color32::from_gray(60))).show(ui, |ui| {
                ui.horizontal(|ui| {
                    if ui.button("Frame before").help("Go to the frame before and pick its drawing").clicked() {
                        go = Some((frame - 1).max(0));
                    }
                    ui.label(RichText::new(format!("Frame {} of {}", frame + 1, last + 1)).monospace().size(12.5));
                    if ui.button("Frame after").help("Go to the frame after and pick its drawing").clicked() {
                        go = Some((frame + 1).min(last));
                    }
                    ui.checkbox(&mut onion, "Onion skin").help("Show the frames before and after faintly, to help you draw the next one.");
                });
            });
        });
        if onion != self.draw.doc.rig.anim.onion.on {
            self.anim_mut("Onion skin").onion.on = onion;
        }
        if let Some(f) = go {
            self.rigs.anim.frame = f as f32;
            self.rigs.anim.playing = false;
            self.cel_apply(f as f32);
            // the drawing that shows in the first track is the one to draw on
            let pick = self.draw.doc.rig.anim.cel_tracks.iter().find(|t| !t.muted).and_then(|t| t.shown_at(f as u32));
            if let Some(c) = pick {
                self.draw.set_active(c);
            }
        }
    }

    // ---- the track rows on the timeline ----

    /// The cel track whose row is at this point (rows come after the rows of the slider tracks).
    pub(super) fn cel_row_at(&self, pos: Pos2, rows_top: f32) -> Option<usize> {
        let first = self.draw.doc.rig.anim.tracks.len() as f32;
        let ci = ((pos.y - rows_top) / ROW - first).floor();
        (ci >= 0.0 && (ci as usize) < self.draw.doc.rig.anim.cel_tracks.len()).then_some(ci as usize)
    }

    pub(super) fn cel_rows_paint(&self, painter: &egui::Painter, area: Rect, key_left: f32, rows_top: f32) {
        let a = &self.draw.doc.rig.anim;
        for (ci, t) in a.cel_tracks.iter().enumerate() {
            let y = rows_top + (a.tracks.len() + ci) as f32 * ROW;
            if y > area.bottom() {
                break;
            }
            let row = Rect::from_min_max(pos2(area.left(), y), pos2(area.right(), y + ROW));
            if self.rigs.anim.sel_cel.map(|s| s.0) == Some(ci) {
                painter.rect_filled(row, 0.0, Color32::from_rgba_unmultiplied(255, 255, 255, 14));
            }
            painter.circle_filled(pos2(area.left() + 10.0, y + ROW / 2.0), 4.0, if t.muted { Color32::from_gray(80) } else { Color32::from_rgb(255, 160, 90) });
            painter.text(pos2(area.left() + 20.0, y + ROW / 2.0), egui::Align2::LEFT_CENTER, &t.name, egui::FontId::proportional(12.0), if t.muted { TEXT_DIM } else { TEXT });
            for (ei, e) in t.exposures.iter().enumerate() {
                let end = t.exposures.get(ei + 1).map(|n| n.frame).unwrap_or(a.length.max(e.frame + 1));
                let (x0, x1) = (self.anim_x(key_left, e.frame as f32).max(key_left), self.anim_x(key_left, end as f32).min(area.right()));
                if x1 <= x0 {
                    continue;
                }
                let block = Rect::from_min_max(pos2(x0, y + 2.0), pos2(x1 - 1.0, y + ROW - 2.0));
                let on = self.rigs.anim.sel_cel == Some((ci, ei));
                let fill = match e.cel {
                    Some(c) => {
                        let n = t.number_of(c).unwrap_or(0);
                        let hue = (n as f32 * 0.17).fract();
                        let col: Color32 = egui::ecolor::Hsva::new(hue, 0.45, 0.62, 1.0).into();
                        (col, n.to_string())
                    }
                    None => (Color32::from_gray(48), "blank".to_string()),
                };
                painter.rect_filled(block, 3.0, fill.0);
                painter.rect_stroke(block, 3.0, Stroke::new(if on { 2.0 } else { 1.0 }, if on { Color32::WHITE } else { Color32::BLACK }), egui::StrokeKind::Inside);
                if block.width() > 14.0 {
                    painter.text(block.left_center() + vec2(4.0, 0.0), egui::Align2::LEFT_CENTER, fill.1, egui::FontId::proportional(11.0), Color32::WHITE);
                }
            }
        }
    }

    /// A click on a cel row: the playhead goes there and the exposure under it is picked.
    pub(super) fn cel_row_click(&mut self, ci: usize, frame: f32) {
        let f = frame.round().max(0.0) as u32;
        self.rigs.anim.frame = f as f32;
        self.rigs.anim.playing = false;
        let ex = self.draw.doc.rig.anim.cel_tracks.get(ci).and_then(|t| t.exposure_at(f));
        self.rigs.anim.sel_cel = ex.map(|e| (ci, e));
        self.rigs.anim.sel_track = None;
        self.rigs.anim.sel = None;
    }

    /// The X-sheet: a row for every frame and a column for every cel track. A cell shows the drawing that starts there, or a line while a
    /// drawing is held. Click a cell, then type a number to show that drawing from there (0 for blank), or Delete to take the change away.
    pub(super) fn cel_xsheet_ui(&mut self, ui: &mut Ui) {
        let a = self.draw.doc.rig.anim.clone();
        if a.cel_tracks.is_empty() {
            ui.label(RichText::new("The X-sheet shows the cel tracks. Make a cel track on the right first (Frame by frame).").size(12.0).color(TEXT_DIM));
            return;
        }
        let (row_h, col_w, num_w) = (20.0, 78.0, 56.0);
        let cur = self.rigs.anim.frame.round().max(0.0) as u32;
        ui.horizontal(|ui| {
            ui.add_space(num_w);
            for t in &a.cel_tracks {
                ui.add_sized([col_w, 18.0], egui::Label::new(RichText::new(&t.name).size(11.5).color(if t.muted { TEXT_DIM } else { TEXT })).truncate());
            }
        });
        let mut clicked: Option<(usize, u32)> = None;
        let fps = a.fps.round().max(1.0) as u32;
        egui::ScrollArea::vertical().id_salt("xsheet_rows").auto_shrink([false, false]).show_rows(ui, row_h, a.length as usize, |ui, range| {
            for f in range {
                let f = f as u32;
                let (rect, resp) = ui.allocate_exact_size(vec2(num_w + col_w * a.cel_tracks.len() as f32, row_h), Sense::click());
                let p = ui.painter();
                if f == cur {
                    p.rect_filled(rect, 0.0, Color32::from_rgba_unmultiplied(255, 90, 90, 40));
                } else if (f / fps) % 2 == 1 {
                    p.rect_filled(rect, 0.0, Color32::from_rgba_unmultiplied(255, 255, 255, 6));
                }
                p.text(rect.left_center() + vec2(8.0, 0.0), egui::Align2::LEFT_CENTER, format!("{}", f + 1), egui::FontId::monospace(11.5), if f % fps == 0 { TEXT } else { TEXT_DIM });
                for (ci, t) in a.cel_tracks.iter().enumerate() {
                    let cell = Rect::from_min_size(rect.min + vec2(num_w + col_w * ci as f32, 0.0), vec2(col_w, row_h));
                    let picked = self.rigs.anim.sel_x == Some((ci, f));
                    if picked {
                        p.rect_stroke(cell.shrink(1.0), 2.0, Stroke::new(1.5, Color32::WHITE), egui::StrokeKind::Inside);
                    }
                    p.line_segment([cell.left_top(), cell.left_bottom()], Stroke::new(1.0, Color32::from_gray(45)));
                    match t.exposures.iter().find(|e| e.frame == f) {
                        Some(e) => {
                            let (txt, col) = match e.cel.and_then(|c| t.number_of(c)) {
                                Some(n) => (n.to_string(), Color32::from_rgb(255, 200, 120)),
                                None => ("X".to_string(), Color32::from_gray(150)),
                            };
                            p.text(cell.center(), egui::Align2::CENTER_CENTER, txt, egui::FontId::monospace(13.0), col);
                        }
                        None if t.exposure_at(f).is_some() => {
                            p.line_segment([cell.center_top() + vec2(0.0, 2.0), cell.center_bottom() - vec2(0.0, 2.0)], Stroke::new(1.0, Color32::from_gray(110)));
                        }
                        None => {}
                    }
                }
                if resp.clicked() {
                    if let Some(pos) = resp.interact_pointer_pos() {
                        let ci = (((pos.x - rect.min.x - num_w) / col_w).floor().max(0.0) as usize).min(a.cel_tracks.len() - 1);
                        clicked = Some((ci, f));
                    }
                }
            }
        });
        if let Some((ci, f)) = clicked {
            self.rigs.anim.sel_x = Some((ci, f));
            self.rigs.anim.frame = f as f32;
            self.rigs.anim.playing = false;
        }
        // typing a number puts that drawing at the picked cell
        let Some((ci, f)) = self.rigs.anim.sel_x else { return };
        if ui.ctx().egui_wants_keyboard_input() {
            return;
        }
        let typed: String = ui.ctx().input(|i| i.events.iter().filter_map(|e| if let egui::Event::Text(t) = e { Some(t.clone()) } else { None }).collect());
        for ch in typed.chars().filter(|c| c.is_ascii_digit()) {
            let n = ch.to_digit(10).unwrap_or(0) as usize;
            let cel = if n == 0 { None } else { a.cel_tracks.get(ci).and_then(|t| t.cels.get(n - 1)).copied() };
            if n != 0 && cel.is_none() {
                continue;
            }
            if let Some(t) = self.anim_mut("X-sheet").cel_tracks.get_mut(ci) {
                t.set_exposure(f, cel);
            }
            self.rigs.anim.applied = None;
        }
    }

    // ---- making and changing tracks ----

    fn cel_new_track(&mut self) {
        let n = self.draw.doc.rig.anim.cel_tracks.len() + 1;
        let _ = self.anim_mut("New cel track");
        let doc = &mut self.draw.doc;
        let mut folder = doc.new_layer(&format!("Cels {n}"), LayerKind::Group);
        folder.expanded = true;
        let fid = folder.id;
        doc.layers.insert(0, folder);
        let cel = doc.new_layer("1", LayerKind::Raster);
        let cid = doc.insert_layer(cel, fid);
        doc.mark_all_dirty();
        let ti = std::sync::Arc::make_mut(&mut doc.rig).anim.add_cel_track(&format!("Cels {n}"), fid, cid);
        self.rigs.anim.sel_cel = Some((ti, 0));
        self.rigs.anim.applied = None;
    }

    /// Pictures (frames of a video, the frames of a GIF, an image sequence) as the drawings of a new cel track, one after the other from
    /// the playhead. Every picture is fitted into the canvas. Returns how many drawings were made.
    pub(super) fn cel_import_pictures(&mut self, pictures: Vec<image::RgbaImage>, name: &str) -> usize {
        if pictures.is_empty() {
            return 0;
        }
        let start = self.rigs.anim.frame.round().max(0.0) as u32;
        let _ = self.anim_mut("Pictures as drawings");
        let (w, h) = (self.draw.doc.w, self.draw.doc.h);
        let doc = &mut self.draw.doc;
        let mut folder = doc.new_layer(name, LayerKind::Group);
        folder.expanded = false;
        let fid = folder.id;
        doc.layers.insert(0, folder);
        let mut ids = Vec::new();
        // the first drawing goes in the folder, the next ones above it, so the numbers run from the bottom up
        for (i, pic) in pictures.iter().enumerate() {
            let k = (w as f32 / pic.width().max(1) as f32).min(h as f32 / pic.height().max(1) as f32);
            let (nw, nh) = (((pic.width() as f32 * k).round() as u32).clamp(1, w), ((pic.height() as f32 * k).round() as u32).clamp(1, h));
            let fitted = if (nw, nh) == pic.dimensions() { pic.clone() } else { image::imageops::resize(pic, nw, nh, image::imageops::FilterType::Triangle) };
            let mut layer = doc.new_layer(&(i + 1).to_string(), LayerKind::Raster);
            layer.tiles = crate::files::tiles_from_rgba(&fitted, ((w - nw) / 2) as i32, ((h - nh) / 2) as i32, w, h);
            layer.visible = false;
            layer.rev += 1;
            ids.push(doc.insert_layer(layer, fid));
        }
        doc.mark_all_dirty();
        let n = ids.len() as u32;
        let rig = std::sync::Arc::make_mut(&mut doc.rig);
        let ti = rig.anim.add_cel_track(name, fid, ids[0]);
        {
            let t = &mut rig.anim.cel_tracks[ti];
            t.exposures.clear();
            t.cels = ids.clone();
            for (i, id) in ids.iter().enumerate() {
                t.set_exposure(start + i as u32, Some(*id));
            }
        }
        if rig.anim.length < start + n {
            rig.anim.length = start + n;
        }
        self.rigs.anim.sel_cel = Some((ti, 0));
        self.rigs.anim.applied = None;
        ids.len()
    }

    /// The frames of a video, made by ffmpeg when the computer has it (it is looked for on the path): `fps` pictures a second.
    fn cel_read_video(path: &std::path::Path, fps: f32) -> Result<Vec<image::RgbaImage>, String> {
        let dir = std::env::temp_dir().join(format!("seviath_video_{}_{}", std::process::id(), std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis()).unwrap_or(0)));
        std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
        let mut cmd = std::process::Command::new("ffmpeg");
        cmd.arg("-v").arg("error").arg("-i").arg(path).arg("-vf").arg(format!("fps={}", fps.max(1.0))).arg("-frames:v").arg("1200").arg(dir.join("f_%05d.png"));
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            cmd.creation_flags(0x0800_0000);
        }
        let out = cmd.output().map_err(|_| "Reading a video needs ffmpeg, which this computer does not have (or it is not on the path). Install it, or turn the video into a GIF or pictures first.".to_string())?;
        if !out.status.success() {
            let _ = std::fs::remove_dir_all(&dir);
            return Err(format!("ffmpeg could not read the video: {}", String::from_utf8_lossy(&out.stderr).lines().last().unwrap_or("")));
        }
        let mut files: Vec<_> = std::fs::read_dir(&dir).map_err(|e| e.to_string())?.flatten().map(|e| e.path()).collect();
        files.sort();
        let pics: Result<Vec<_>, String> = files.iter().map(|p| image::open(p).map(|i| i.to_rgba8()).map_err(|e| e.to_string())).collect();
        let _ = std::fs::remove_dir_all(&dir);
        pics
    }

    /// The pictures of a file: the frames of a GIF, of a video (with ffmpeg), or the one picture.
    fn cel_read_pictures(path: &std::path::Path) -> Result<Vec<image::RgbaImage>, String> {
        use image::AnimationDecoder;
        let ext = path.extension().map(|e| e.to_string_lossy().to_lowercase()).unwrap_or_default();
        if matches!(ext.as_str(), "mp4" | "mov" | "webm" | "mkv" | "avi" | "m4v") {
            return Self::cel_read_video(path, 12.0);
        }
        let is_gif = path.extension().is_some_and(|e| e.eq_ignore_ascii_case("gif"));
        if is_gif {
            let f = std::fs::File::open(path).map_err(|e| e.to_string())?;
            let dec = image::codecs::gif::GifDecoder::new(std::io::BufReader::new(f)).map_err(|e| e.to_string())?;
            let frames: Result<Vec<_>, _> = dec.into_frames().take(2000).collect();
            return Ok(frames.map_err(|e| e.to_string())?.into_iter().map(|f| f.into_buffer()).collect());
        }
        Ok(vec![image::open(path).map_err(|e| format!("{}: {e}", path.display()))?.to_rgba8()])
    }

    /// Ask for pictures and make drawings of them.
    fn cel_import_dialog(&mut self) {
        let Some(mut files) = rfd::FileDialog::new().set_title("Pictures or a GIF").set_directory(self.settings.projects_folder()).add_filter("Pictures, GIF and video", &["png", "jpg", "jpeg", "webp", "bmp", "gif", "mp4", "mov", "webm", "mkv", "avi", "m4v"]).pick_files() else { return };
        // an image sequence is in the order of its names
        files.sort();
        let name = files.first().and_then(|p| p.file_stem()).map(|s| s.to_string_lossy().to_string()).unwrap_or_else(|| "Pictures".into());
        let mut all = Vec::new();
        for f in &files {
            match Self::cel_read_pictures(f) {
                Ok(mut v) => all.append(&mut v),
                Err(e) => {
                    self.rigs.anim.message = Some(e);
                    return;
                }
            }
            if all.len() > 2000 {
                break;
            }
        }
        let n = self.cel_import_pictures(all, &name);
        self.rigs.anim.message = Some(format!("{n} pictures are drawings of a new track now. Trace over them in the Draw tab on a new layer, or use them as they are."));
    }

    /// A new blank drawing in the folder of the track, shown from the playhead on.
    fn cel_new_cel(&mut self, ti: usize) {
        let Some(t) = self.draw.doc.rig.anim.cel_tracks.get(ti).cloned() else { return };
        let frame = self.rigs.anim.frame.round().max(0.0) as u32;
        let _ = self.anim_mut("New cel");
        let doc = &mut self.draw.doc;
        let cel = doc.new_layer(&(t.cels.len() + 1).to_string(), LayerKind::Raster);
        let cid = if doc.index_of(t.folder).is_some() { doc.insert_layer(cel, t.folder) } else { doc.insert_layer(cel, t.cels.first().copied().unwrap_or(0)) };
        doc.mark_all_dirty();
        let rig = std::sync::Arc::make_mut(&mut doc.rig);
        if let Some(tr) = rig.anim.cel_tracks.get_mut(ti) {
            tr.cels.push(cid);
            tr.set_exposure(frame, Some(cid));
            self.rigs.anim.sel_cel = tr.exposure_at(frame).map(|e| (ti, e));
        }
        self.rigs.anim.applied = None;
    }

    fn cel_set_here(&mut self, ti: usize, cel: Option<u64>) {
        let frame = self.rigs.anim.frame.round().max(0.0) as u32;
        if let Some(t) = self.anim_mut("Show a drawing").cel_tracks.get_mut(ti) {
            t.set_exposure(frame, cel);
            self.rigs.anim.sel_cel = t.exposure_at(frame).map(|e| (ti, e));
        }
        self.rigs.anim.applied = None;
    }

    /// Go to the Draw tab with the drawing that shows at the playhead picked, to draw on it.
    fn cel_draw_it(&mut self, ti: usize) {
        let frame = self.rigs.anim.frame.round().max(0.0) as u32;
        let Some(c) = self.draw.doc.rig.anim.cel_tracks.get(ti).and_then(|t| t.shown_at(frame)) else {
            self.rigs.anim.message = Some("Nothing shows here: put a drawing here first (New drawing here, or Show drawing).".into());
            return;
        };
        self.rigs.anim.playing = false;
        self.cel_apply(frame as f32);
        self.draw.set_active(c);
        self.tab = crate::tabs::Tab::Draw;
    }

    pub(super) fn cel_side_ui(&mut self, ui: &mut Ui) {
        section_title(ui, "Frame by frame");
        let a = self.draw.doc.rig.anim.clone();
        if a.cel_tracks.is_empty() {
            ui.label(RichText::new("Draw every picture of the animation yourself. A cel track makes a folder of layers: each layer is one drawing, drawn in the Draw tab, and the track says which one shows at which frame.").size(11.5).color(TEXT_DIM));
        }
        ui.horizontal_wrapped(|ui| {
            if ui.button("New cel track").help("A folder with its first drawing").clicked() {
                self.cel_new_track();
            }
            if ui.button("Pictures as drawings...").help("The frames of a GIF, or pictures in the order of their names, one drawing each").clicked() {
                self.cel_import_dialog();
            }
        });
        let frame = self.rigs.anim.frame.round().max(0.0) as u32;
        let mut remove: Option<usize> = None;
        for (ti, t) in a.cel_tracks.iter().enumerate() {
            ui.add_space(4.0);
            let mut name = t.name.clone();
            let mut muted = t.muted;
            ui.horizontal(|ui| {
                ui.add(egui::TextEdit::singleline(&mut name).desired_width(120.0));
                ui.checkbox(&mut muted, "Off").help("The track leaves the layers alone");
                if ui.small_button("Delete").help("Takes the track away; the layers stay").clicked() {
                    remove = Some(ti);
                }
            });
            if name != t.name || muted != t.muted {
                if let Some(tr) = self.anim_mut("Cel track").cel_tracks.get_mut(ti) {
                    tr.name = name;
                    tr.muted = muted;
                }
            }
            let shown = t.shown_at(frame).and_then(|c| t.number_of(c));
            ui.label(RichText::new(format!("At frame {frame}: {}", shown.map(|n| format!("drawing {n}")).unwrap_or_else(|| "blank".into()))).size(11.5).color(TEXT_DIM));
            ui.horizontal_wrapped(|ui| {
                if ui.button("Draw it").help("Open the drawing that shows here in the Draw tab").clicked() {
                    self.cel_draw_it(ti);
                }
                if ui.button("New drawing here").help("A new blank layer, shown from the playhead on").clicked() {
                    self.cel_new_cel(ti);
                }
                if ui.button("Blank here").help("Show nothing in this frame.").clicked() {
                    self.cel_set_here(ti, None);
                }
                if ui.button("Remove the change here").help("Takes away the exposure that starts at the playhead").clicked() {
                    if let Some(tr) = self.anim_mut("Remove exposure").cel_tracks.get_mut(ti) {
                        tr.remove_exposure(frame);
                    }
                    self.rigs.anim.sel_cel = None;
                    self.rigs.anim.applied = None;
                }
            });
            ui.horizontal_wrapped(|ui| {
                ui.label(RichText::new("Show drawing").size(12.0).color(TEXT_DIM));
                for (i, c) in t.cels.iter().enumerate() {
                    if widgets::chip(ui, &(i + 1).to_string(), t.shown_at(frame) == Some(*c)).help("Shown from the playhead on").clicked() {
                        self.cel_set_here(ti, Some(*c));
                    }
                }
            });
            ui.horizontal_wrapped(|ui| {
                ui.label(RichText::new("Timing").size(12.0).color(TEXT_DIM));
                if ui.small_button("Add a frame here").help("Everything from the playhead on moves one frame later").clicked() {
                    if let Some(tr) = self.anim_mut("Add a frame").cel_tracks.get_mut(ti) {
                        tr.shift(frame, 1);
                    }
                    self.rigs.anim.applied = None;
                }
                if ui.small_button("Take a frame away here").help("Remove one frame here, so the animation is shorter.").clicked() {
                    if let Some(tr) = self.anim_mut("Take a frame away").cel_tracks.get_mut(ti) {
                        tr.shift(frame, -1);
                    }
                    self.rigs.anim.applied = None;
                }
            });
        }
        if let Some(i) = remove {
            self.anim_mut("Delete cel track").cel_tracks.remove(i);
            self.rigs.anim.sel_cel = None;
        }
        if !a.cel_tracks.is_empty() {
            ui.add_space(4.0);
            let mut o = a.onion;
            ui.horizontal_wrapped(|ui| {
                ui.checkbox(&mut o.on, "Onion skin").help("The frames before (red) and after (green) show faintly while you scrub");
                ui.label(RichText::new("before").size(12.0).color(TEXT_DIM));
                ui.add(egui::DragValue::new(&mut o.before).range(0..=6));
                ui.label(RichText::new("after").size(12.0).color(TEXT_DIM));
                ui.add(egui::DragValue::new(&mut o.after).range(0..=6));
                ui.add(egui::Slider::new(&mut o.opacity, 0.05..=0.9).show_value(false)).help("How strong");
            });
            if o != a.onion {
                self.anim_mut("Onion skin").onion = o;
            }
        }
    }

    #[cfg(test)]
    pub fn test_cel_import(&mut self, pictures: Vec<image::RgbaImage>) -> usize {
        self.cel_import_pictures(pictures, "Frames")
    }

    #[cfg(test)]
    pub fn test_cel_read(path: &std::path::Path) -> Result<Vec<image::RgbaImage>, String> {
        Self::cel_read_pictures(path)
    }

    #[cfg(test)]
    pub fn test_cel_new_track(&mut self) {
        self.cel_new_track();
    }

    #[cfg(test)]
    pub fn test_cel_new_cel(&mut self, ti: usize) {
        self.cel_new_cel(ti);
    }

    #[cfg(test)]
    pub fn test_cel_preview_picture(&mut self, ctx: &egui::Context, frame: u32) -> Option<egui::TextureHandle> {
        self.cel_texture(ctx, frame)
    }
}

#[cfg(test)]
impl SeviathApp {
    /// The colour of a pixel of the picture put together for the frame that is applied now (None outside the canvas).
    pub fn test_still_cels(&mut self, x: i32, y: i32) -> Option<[u8; 4]> {
        let img = self.cel_flatten(&[]);
        (x >= 0 && y >= 0 && (x as u32) < img.width() && (y as u32) < img.height()).then(|| img.get_pixel(x as u32, y as u32).0)
    }
}
