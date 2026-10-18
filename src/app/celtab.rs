// SPDX-License-Identifier: GPL-3.0-or-later
// Frame by frame animation in the Animate tab. A cel track is a folder of layers: every layer is one drawing (a cel), drawn in the Draw
// tab, and the track says which drawing shows from which frame on. The Animate tab shows the picture put together from the layers
// (the drawings of the frame, onion skin of the frames around it), plays it, and saves it like the rig animation. A character that is
// both rigged and drawn frame by frame is not mixed yet: with a cel track the picture is the layers as they are drawn.

use std::hash::{Hash, Hasher};

use eframe::egui::{self, Color32, Pos2, Rect, RichText, Stroke, Ui, pos2, vec2};

use super::SeviathApp;
use super::animtab::ROW;
use crate::doc::LayerKind;
use crate::theme::*;
use crate::widgets::{self, section_title};

/// The biggest side of the pictures the preview keeps.
const PREVIEW_SIDE: f32 = 1100.0;
const MAX_CACHED: usize = 90;

impl SeviathApp {
    pub(super) fn cel_mode(&self) -> bool {
        !self.draw.doc.rig.anim.cel_tracks.is_empty()
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
            (l.id, shown, l.opacity.to_bits(), l.rev, l.kind as u8, l.parent).hash(&mut h);
        }
        (self.draw.doc.w, self.draw.doc.h).hash(&mut h);
        h.finish()
    }

    /// The picture of the layers with `vis` set (and the paper hidden), at full size. The layers are put back.
    fn cel_flatten(&mut self, vis: &[(u64, bool)]) -> image::RgbaImage {
        let saved: Vec<(u64, bool)> = self.draw.doc.layers.iter().map(|l| (l.id, l.visible)).collect();
        for l in self.draw.doc.layers.iter_mut() {
            if l.kind == LayerKind::Paper {
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
        let p = ui.painter_at(rect);
        p.rect_filled(page, 0.0, Color32::from_rgb(244, 242, 236));
        let frame = self.rigs.anim.frame.round().max(0.0) as u32;
        let onion = self.draw.doc.rig.anim.onion;
        let uv = Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0));
        if onion.on && !self.rigs.anim.playing {
            for (n, dir, tint) in [(onion.before, -1i64, (255u8, 110u8, 110u8)), (onion.after, 1, (90, 200, 120))] {
                for k in (1..=n as i64).rev() {
                    let f = frame as i64 + dir * k;
                    if f < 0 || f >= self.draw.doc.rig.anim.length as i64 {
                        continue;
                    }
                    let alpha = (onion.opacity * (1.0 - (k - 1) as f32 / (n as f32 + 0.5)) * 255.0).clamp(0.0, 255.0) as u8;
                    if let Some(t) = self.cel_texture(&ctx, f as u32) {
                        p.image(t.id(), page, uv, Color32::from_rgba_unmultiplied(tint.0, tint.1, tint.2, alpha));
                    }
                }
            }
        }
        if let Some(t) = self.cel_texture(&ctx, frame) {
            p.image(t.id(), page, uv, Color32::WHITE);
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
        if ui.button("New cel track").on_hover_text("A folder with its first drawing").clicked() {
            self.cel_new_track();
        }
        let frame = self.rigs.anim.frame.round().max(0.0) as u32;
        let mut remove: Option<usize> = None;
        for (ti, t) in a.cel_tracks.iter().enumerate() {
            ui.add_space(4.0);
            let mut name = t.name.clone();
            let mut muted = t.muted;
            ui.horizontal(|ui| {
                ui.add(egui::TextEdit::singleline(&mut name).desired_width(120.0));
                ui.checkbox(&mut muted, "Off").on_hover_text("The track leaves the layers alone");
                if ui.small_button("Delete").on_hover_text("Takes the track away; the layers stay").clicked() {
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
                if ui.button("Draw it").on_hover_text("Open the drawing that shows here in the Draw tab").clicked() {
                    self.cel_draw_it(ti);
                }
                if ui.button("New drawing here").on_hover_text("A new blank layer, shown from the playhead on").clicked() {
                    self.cel_new_cel(ti);
                }
                if ui.button("Blank here").clicked() {
                    self.cel_set_here(ti, None);
                }
                if ui.button("Remove the change here").on_hover_text("Takes away the exposure that starts at the playhead").clicked() {
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
                    if widgets::chip(ui, &(i + 1).to_string(), t.shown_at(frame) == Some(*c)).on_hover_text("Shown from the playhead on").clicked() {
                        self.cel_set_here(ti, Some(*c));
                    }
                }
            });
            ui.horizontal_wrapped(|ui| {
                ui.label(RichText::new("Timing").size(12.0).color(TEXT_DIM));
                if ui.small_button("Add a frame here").on_hover_text("Everything from the playhead on moves one frame later").clicked() {
                    if let Some(tr) = self.anim_mut("Add a frame").cel_tracks.get_mut(ti) {
                        tr.shift(frame, 1);
                    }
                    self.rigs.anim.applied = None;
                }
                if ui.small_button("Take a frame away here").clicked() {
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
                ui.checkbox(&mut o.on, "Onion skin").on_hover_text("The frames before (red) and after (green) show faintly while you scrub");
                ui.label(RichText::new("before").size(12.0).color(TEXT_DIM));
                ui.add(egui::DragValue::new(&mut o.before).range(0..=6));
                ui.label(RichText::new("after").size(12.0).color(TEXT_DIM));
                ui.add(egui::DragValue::new(&mut o.after).range(0..=6));
                ui.add(egui::Slider::new(&mut o.opacity, 0.05..=0.9).show_value(false)).on_hover_text("How strong");
            });
            if o != a.onion {
                self.anim_mut("Onion skin").onion = o;
            }
        }
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
