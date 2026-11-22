// SPDX-License-Identifier: GPL-3.0-or-later
// The timeline of the Draw tab: frame by frame animation with every layer as a frame, laid out as a strip of frames with their pictures and how long each one is
// held. It works on the first cel track (a folder of drawings and the frames they are shown at); "Frames from the layers" makes one from the layers you have.

use eframe::egui::{self, Color32, Margin, Rect, RichText, Sense, Stroke, Ui, pos2, vec2};

use super::SeviathApp;
use crate::doc::LayerKind;
use crate::theme::*;
use crate::widgets::{self, HelpExt};

const CELL_W: f32 = 92.0;
const THUMB_H: f32 = 62.0;

impl SeviathApp {
    /// The timeline at the bottom of the Draw tab: a thin strip with a button when it is closed, the frames and their buttons when it is open.
    pub(super) fn draw_timeline(&mut self, ui: &mut Ui) {
        let ctx = ui.ctx().clone();
        if !self.rigs.anim.timeline {
            return;
        }
        self.rig_init();
        self.timeline_playback(&ctx);
        egui::Panel::bottom("draw_timeline").default_size(190.0).size_range(120.0..=460.0).resizable(true).frame(egui::Frame::new().fill(PANEL).inner_margin(Margin::same(8))).show(ui, |ui| {
            self.timeline_body(ui, &ctx);
        });
        self.anim_flush(&ctx);
    }

    /// Playing: the frame moves on and the layers of that frame are shown.
    fn timeline_playback(&mut self, ctx: &egui::Context) {
        if self.rigs.anim.playing {
            let dt = ctx.input(|i| i.unstable_dt).clamp(0.0, 0.1);
            let (f, d) = self.draw.doc.rig.anim.advance(self.rigs.anim.frame, self.rigs.anim.dir, dt);
            let end = self.draw.doc.rig.anim.play_range().1 as f32;
            self.rigs.anim.frame = f;
            self.rigs.anim.dir = d;
            if !self.draw.doc.rig.anim.looping && d > 0.0 && f >= end {
                self.rigs.anim.playing = false;
            }
            self.cel_apply(f);
            ctx.request_repaint();
        }
    }

    /// Go to a frame: its drawing is shown and picked to draw on.
    fn timeline_go(&mut self, f: u32) {
        let last = self.draw.doc.rig.anim.length.saturating_sub(1);
        let f = f.min(last);
        self.rigs.anim.frame = f as f32;
        self.rigs.anim.playing = false;
        self.cel_apply(f as f32);
        self.rigs.anim.applied = None;
        let pick = self.draw.doc.rig.anim.cel_tracks.first().and_then(|t| t.shown_at(f));
        if let Some(c) = pick {
            self.draw.set_active(c);
        }
    }

    fn timeline_body(&mut self, ui: &mut Ui, ctx: &egui::Context) {
        let a = self.draw.doc.rig.anim.clone();
        let cur = self.rigs.anim.frame.round().max(0.0) as u32;
        let last = a.length.saturating_sub(1);
        let playing = self.rigs.anim.playing;
        let mut go: Option<u32> = None;
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing = vec2(4.0, 4.0);
            if widgets::chip(ui, "Hide", false).help("Close the timeline").clicked() {
                self.rigs.anim.timeline = false;
                self.rigs.anim.playing = false;
            }
            ui.separator();
            if widgets::chip(ui, "First", false).help("Go to the first frame").clicked() {
                go = Some(0);
            }
            if widgets::chip(ui, "Back", false).help("Go one frame back and pick its drawing").clicked() {
                go = Some(cur.saturating_sub(1));
            }
            if widgets::chip(ui, if playing { "Stop" } else { "Play" }, playing).help("Plays the animation at its speed, from here").clicked() {
                if playing {
                    self.rigs.anim.playing = false;
                } else {
                    if cur >= a.play_range().1 {
                        self.rigs.anim.frame = a.play_range().0 as f32;
                    }
                    self.rigs.anim.dir = 1.0;
                    self.rigs.anim.playing = true;
                }
            }
            if widgets::chip(ui, "Next", false).help("Go one frame forward and pick its drawing").clicked() {
                go = Some(cur + 1);
            }
            if widgets::chip(ui, "Last", false).help("Go to the last frame").clicked() {
                go = Some(last);
            }
            ui.label(RichText::new(format!("Frame {} of {}", cur + 1, last + 1)).monospace().size(12.5));
            let mut looping = a.looping;
            if ui.checkbox(&mut looping, "Loop").help("Starts again when it reaches the end").changed() {
                self.anim_mut("Loop").looping = looping;
            }
            let mut fps = a.fps;
            if ui.add(egui::DragValue::new(&mut fps).speed(0.2).range(1.0..=60.0).suffix(" per second")).help("How many frames are shown every second").changed() {
                self.anim_mut("Speed").fps = fps;
            }
            let mut onion = a.onion.on;
            if ui.checkbox(&mut onion, "Onion skin").help("Shows the frames before (red) and after (green) faintly, to draw the next one by").changed() {
                self.anim_mut("Onion skin").onion.on = onion;
            }
        });
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing = vec2(4.0, 4.0);
            if ui.button("Frames from the layers").help("Makes one frame of every layer of the picture (the bottom layer first): each layer is shown in its own frame. Folders, the paper and the layers of a rig are left out.").clicked() {
                self.timeline_from_layers();
            }
            if ui.button("New frame").help("A new empty drawing after the last frame").clicked() {
                self.timeline_new_frame(false);
            }
            if ui.button("Copy the frame").help("A new frame after the last one, with a copy of the drawing that shows here").clicked() {
                self.timeline_new_frame(true);
            }
            if ui.button("Longer").help("Holds this frame one more step before the next one shows").clicked() {
                self.timeline_hold(1);
            }
            if ui.button("Shorter").help("Holds this frame one step less").clicked() {
                self.timeline_hold(-1);
            }
            if ui.button("Take the frame out").help("The frame is taken out of the timeline (its layer stays in the picture)").clicked() {
                self.timeline_remove_frame();
            }
        });
        let Some(t) = a.cel_tracks.first().cloned() else {
            ui.label(RichText::new("No frames yet. Press Frames from the layers to make a frame of every layer, or New frame to start.").size(12.0).color(TEXT_DIM));
            if let Some(f) = go {
                self.timeline_go(f);
            }
            return;
        };
        let here = t.exposure_at(cur);
        let fps = a.fps.max(1.0);
        let mut clicked: Option<u32> = None;
        egui::ScrollArea::horizontal().id_salt("draw_timeline_strip").auto_shrink([false, false]).show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing = vec2(4.0, 0.0);
                for (ei, e) in t.exposures.iter().enumerate() {
                    let end = t.exposures.get(ei + 1).map(|n| n.frame).unwrap_or(a.length.max(e.frame + 1));
                    let hold = end.saturating_sub(e.frame).max(1);
                    let (rect, resp) = ui.allocate_exact_size(vec2(CELL_W, THUMB_H + 34.0), Sense::click());
                    if !ui.is_rect_visible(rect) {
                        continue;
                    }
                    let on = here == Some(ei);
                    let p = ui.painter();
                    p.rect_filled(rect, 4.0, if on { Color32::from_rgb(52, 46, 30) } else { RAISED });
                    p.rect_stroke(rect, 4.0, Stroke::new(if on { 2.0 } else { 1.0 }, if on { Color32::from_rgb(255, 170, 90) } else { Color32::from_gray(55) }), egui::StrokeKind::Inside);
                    let pic = Rect::from_min_size(rect.min + vec2(4.0, 4.0), vec2(CELL_W - 8.0, THUMB_H));
                    let (dw, dh) = (self.draw.doc.w.max(1) as f32, self.draw.doc.h.max(1) as f32);
                    let k = (pic.width() / dw).min(pic.height() / dh);
                    let page = Rect::from_center_size(pic.center(), vec2(dw, dh) * k);
                    p.rect_filled(page, 0.0, Color32::from_rgb(244, 242, 236));
                    if e.cel.is_some() {
                        if let Some(tex) = self.cel_texture(ctx, e.frame) {
                            ui.painter().image(tex.id(), page, Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)), Color32::WHITE);
                        }
                    } else {
                        ui.painter().text(page.center(), egui::Align2::CENTER_CENTER, "blank", egui::FontId::proportional(11.0), Color32::from_gray(120));
                    }
                    let name = e.cel.and_then(|c| self.draw.doc.layers.iter().find(|l| l.id == c)).map(|l| l.name.clone()).unwrap_or_default();
                    let p = ui.painter();
                    p.text(pos2(rect.left() + 6.0, rect.bottom() - 18.0), egui::Align2::LEFT_CENTER, format!("{}  {}", ei + 1, name), egui::FontId::proportional(11.0), TEXT);
                    p.text(pos2(rect.left() + 6.0, rect.bottom() - 7.0), egui::Align2::LEFT_CENTER, format!("{:.2} s  ({} step{})", hold as f32 / fps, hold, if hold == 1 { "" } else { "s" }), egui::FontId::proportional(10.0), TEXT_DIM);
                    if resp.clicked() {
                        clicked = Some(e.frame);
                    }
                    resp.on_hover_text("Click to go to this frame and draw on its drawing");
                }
            });
        });
        if let Some(f) = clicked.or(go) {
            self.timeline_go(f);
        }
    }

    /// One frame for every layer: the bottom one first. Folders, the paper and the layers a rig draws are left out.
    pub(super) fn timeline_from_layers(&mut self) {
        let rigged: std::collections::HashSet<u64> = self.draw.doc.rig.meshes.iter().map(|m| m.layer).collect();
        let ids: Vec<u64> = self.draw.doc.layers.iter().rev().filter(|l| matches!(l.kind, LayerKind::Raster | LayerKind::Vector) && l.parent.is_none() && !rigged.contains(&l.id)).map(|l| l.id).collect();
        if ids.is_empty() {
            self.rigs.anim.message = Some("There are no layers to make frames of.".into());
            return;
        }
        let n = ids.len() as u32;
        let anim = self.anim_mut("Frames from the layers");
        anim.cel_tracks.clear();
        let ti = anim.add_cel_track("Frames", ids[0], ids[0]);
        let t = &mut anim.cel_tracks[ti];
        t.exposures.clear();
        t.cels = ids.clone();
        for (i, id) in ids.iter().enumerate() {
            t.set_exposure(i as u32, Some(*id));
        }
        anim.length = if anim.tracks.is_empty() { n } else { anim.length.max(n) };
        self.rigs.anim.applied = None;
        self.timeline_go(0);
    }

    /// A new drawing (empty, or a copy of the one that shows here) after the last frame of the first track; made if there is no track yet.
    pub(super) fn timeline_new_frame(&mut self, copy: bool) {
        let cur = self.rigs.anim.frame.round().max(0.0) as u32;
        let shown = self.draw.doc.rig.anim.cel_tracks.first().and_then(|t| t.shown_at(cur));
        let source = if copy { shown.and_then(|c| self.draw.doc.layers.iter().find(|l| l.id == c)).map(|l| l.tiles.clone()) } else { None };
        let first_track = self.draw.doc.rig.anim.cel_tracks.is_empty();
        let _ = self.anim_mut("New frame");
        let doc = &mut self.draw.doc;
        let number = doc.rig.anim.cel_tracks.first().map(|t| t.cels.len()).unwrap_or(0) + 1;
        let mut layer = doc.new_layer(&number.to_string(), LayerKind::Raster);
        if let Some(tiles) = source {
            layer.tiles = tiles;
            layer.rev += 1;
        }
        layer.visible = false;
        // above the drawing it follows, or on top of everything for the first one
        let above = doc.rig.anim.cel_tracks.first().and_then(|t| t.cels.last().copied()).or(doc.layers.first().map(|l| l.id)).unwrap_or(0);
        let id = doc.insert_layer(layer, above);
        doc.mark_all_dirty();
        let anim = &mut std::sync::Arc::make_mut(&mut doc.rig).anim;
        if first_track {
            anim.add_cel_track("Frames", id, id);
            anim.length = 1;
            self.rigs.anim.applied = None;
            self.timeline_go(0);
            return;
        }
        let fps_hold = anim.cel_tracks[0].exposures.last().map(|e| anim.length.saturating_sub(e.frame).max(1)).unwrap_or(1);
        let start = anim.cel_tracks[0].exposures.last().map(|e| e.frame + fps_hold).unwrap_or(0);
        let t = &mut anim.cel_tracks[0];
        t.cels.push(id);
        t.set_exposure(start, Some(id));
        anim.length = if anim.tracks.is_empty() { start + 1 } else { anim.length.max(start + 1) };
        self.rigs.anim.applied = None;
        self.timeline_go(start);
    }

    /// The frame under the playhead is held `by` steps longer (or shorter): the frames after it move.
    fn timeline_hold(&mut self, by: i32) {
        let cur = self.rigs.anim.frame.round().max(0.0) as u32;
        let Some(ei) = self.draw.doc.rig.anim.cel_tracks.first().and_then(|t| t.exposure_at(cur)) else { return };
        let anim = self.anim_mut("Hold a frame");
        let length = anim.length;
        let t = &mut anim.cel_tracks[0];
        let (start, count) = (t.exposures[ei].frame, t.exposures.len());
        let end = t.exposures.get(ei + 1).map(|n| n.frame).unwrap_or(length.max(start + 1));
        if by < 0 && end - start <= 1 {
            return;
        }
        for e in t.exposures.iter_mut().skip(ei + 1) {
            e.frame = (e.frame as i64 + by as i64).max(0) as u32;
        }
        let new_end = (end as i64 + by as i64).max(1) as u32;
        anim.length = if ei + 1 == count { length.max(new_end) } else { (length as i64 + by as i64).max(1) as u32 };
        self.rigs.anim.applied = None;
    }

    /// The frame under the playhead is taken out and the ones after it move up; its layer stays.
    fn timeline_remove_frame(&mut self) {
        let cur = self.rigs.anim.frame.round().max(0.0) as u32;
        let Some(ei) = self.draw.doc.rig.anim.cel_tracks.first().and_then(|t| t.exposure_at(cur)) else { return };
        if self.draw.doc.rig.anim.cel_tracks[0].exposures.len() <= 1 {
            return;
        }
        let anim = self.anim_mut("Take a frame out");
        let length = anim.length;
        let t = &mut anim.cel_tracks[0];
        let start = t.exposures[ei].frame;
        let end = t.exposures.get(ei + 1).map(|n| n.frame).unwrap_or(length.max(start + 1));
        let hold = end - start;
        t.exposures.remove(ei);
        for e in t.exposures.iter_mut().skip(ei) {
            e.frame = e.frame.saturating_sub(hold);
        }
        let new_length = length.saturating_sub(hold).max(1);
        anim.length = new_length;
        self.rigs.anim.applied = None;
        self.timeline_go(start.min(new_length - 1));
    }
}
