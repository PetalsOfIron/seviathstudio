// SPDX-License-Identifier: GPL-3.0-or-later
// The Animate tab: a timeline under the picture of the character. Tracks move the sliders of the rig (and how see-through or shown a
// layer is) over time with keys; the playhead plays them at a frame rate (physics swings with it), a slider moved while Auto key is on
// makes a key, and the animation can be saved as a PNG sequence, a GIF or an MP4.

use std::collections::HashMap;
use std::path::PathBuf;

use eframe::egui::{self, Color32, Pos2, Rect, RichText, Sense, Stroke, Ui, pos2, vec2};

use super::SeviathApp;
use crate::anim::{Animation, Ease, Target};
use crate::history::DocState;
use crate::rig::Id;
use crate::theme::*;
use crate::widgets::{self, section_title};

const ROW: f32 = 22.0;
const RULER: f32 = 26.0;
const NAMES: f32 = 190.0;

enum Drag {
    Scrub,
    Key { track: usize, key: usize },
    Pan { x0: f32, scroll0: f32 },
}

struct ExportJob {
    kind: u8,
    frames: Vec<u32>,
    next: usize,
    size: (u32, u32),
    transparent: bool,
    dir: PathBuf,
    path: PathBuf,
    gif: Option<image::codecs::gif::GifEncoder<std::fs::File>>,
    mp4: Option<crate::recorder::Writer>,
    base_values: crate::rig::Values,
    error: Option<String>,
}

pub struct AnimUi {
    pub frame: f32,
    pub playing: bool,
    pub dir: f32,
    applied: Option<f32>,
    pub sel: Option<(usize, usize)>,
    pub sel_track: Option<usize>,
    pub zoom: f32,
    pub scroll: f32,
    pub autokey: bool,
    pub message: Option<String>,
    drag: Option<Drag>,
    ctx_key: Option<(usize, usize)>,
    pub export_kind: u8,
    pub export_side: u32,
    pub export_work_area: bool,
    pub export_transparent: bool,
    job: Option<ExportJob>,
    /// What the layers looked like before the animation moved them, to put back when it stops.
    base: Option<HashMap<u64, (f32, bool)>>,
    /// A change being made (a key dragged, a number dragged): the document before it, kept as one undo step when the mouse is let go.
    pending: Option<(String, DocState)>,
    pub view_zoom: f32,
}

impl Default for AnimUi {
    fn default() -> Self {
        AnimUi {
            frame: 0.0,
            playing: false,
            dir: 1.0,
            applied: None,
            sel: None,
            sel_track: None,
            zoom: 9.0,
            scroll: 0.0,
            autokey: false,
            message: None,
            drag: None,
            ctx_key: None,
            export_kind: 0,
            export_side: 720,
            export_work_area: false,
            export_transparent: true,
            job: None,
            base: None,
            pending: None,
            view_zoom: 1.0,
        }
    }
}

fn target_color(t: &Target) -> Color32 {
    match t {
        Target::Param(_) => Color32::from_rgb(90, 200, 255),
        Target::Opacity(_) => Color32::from_rgb(255, 200, 60),
        Target::Visible(_) => Color32::from_rgb(255, 120, 170),
    }
}

impl SeviathApp {
    /// Change the animation; the change is kept as one undo step when the mouse is let go.
    fn anim_mut(&mut self, label: &str) -> &mut Animation {
        if self.rigs.anim.pending.is_none() {
            self.rigs.anim.pending = Some((label.to_string(), self.draw.doc.snapshot()));
        }
        &mut std::sync::Arc::make_mut(&mut self.draw.doc.rig).anim
    }

    fn anim_flush(&mut self, ctx: &egui::Context) {
        if self.rigs.anim.pending.is_some() && !ctx.input(|i| i.pointer.any_down()) {
            if let Some((label, before)) = self.rigs.anim.pending.take() {
                if !self.draw.doc.same_as(&before) {
                    let after = self.draw.doc.snapshot();
                    let now = ctx.input(|i| i.time);
                    self.log_step(&label, now);
                    self.history.push(label, super::Tab::Animate, before, after, now);
                }
            }
        }
    }

    /// Put what the tracks say at `frame` on the rig and the layers.
    pub(super) fn apply_anim(&mut self, frame: f32) {
        let samples = self.draw.doc.rig.anim.sample(frame);
        if samples.iter().any(|(t, _)| !matches!(t, Target::Param(_))) && self.rigs.anim.base.is_none() {
            self.rigs.anim.base = Some(self.draw.doc.layers.iter().map(|l| (l.id, (l.opacity, l.visible))).collect());
        }
        let limits: HashMap<Id, (f32, f32)> = self.draw.doc.rig.params.iter().map(|p| (p.id, (p.min, p.max))).collect();
        for (t, v) in samples {
            match t {
                Target::Param(id) => {
                    if let Some((lo, hi)) = limits.get(&id) {
                        self.rigs.values.set(id, v.clamp(*lo, *hi));
                    }
                }
                Target::Opacity(l) => {
                    if let Some(layer) = self.draw.doc.layers.iter_mut().find(|x| x.id == l) {
                        layer.opacity = v.clamp(0.0, 100.0);
                    }
                }
                Target::Visible(l) => {
                    if let Some(layer) = self.draw.doc.layers.iter_mut().find(|x| x.id == l) {
                        layer.visible = v >= 0.5;
                    }
                }
            }
        }
        self.rigs.anim.applied = Some(frame);
    }

    /// Put the layers back as they were before the animation moved them.
    fn anim_restore_layers(&mut self) {
        if let Some(base) = self.rigs.anim.base.take() {
            for l in self.draw.doc.layers.iter_mut() {
                if let Some((o, v)) = base.get(&l.id) {
                    l.opacity = *o;
                    l.visible = *v;
                }
            }
        }
    }

    fn anim_playback(&mut self, ctx: &egui::Context) {
        if self.rigs.anim.job.is_some() {
            return;
        }
        if self.rigs.anim.playing {
            let dt = ctx.input(|i| i.unstable_dt).clamp(0.0, 0.1);
            let (f, d) = self.draw.doc.rig.anim.advance(self.rigs.anim.frame, self.rigs.anim.dir, dt);
            let end = self.draw.doc.rig.anim.play_range().1 as f32;
            let looping = self.draw.doc.rig.anim.looping;
            self.rigs.anim.frame = f;
            self.rigs.anim.dir = d;
            if !looping && d > 0.0 && f >= end {
                self.rigs.anim.playing = false;
            }
            ctx.request_repaint();
        }
        let f = self.rigs.anim.frame;
        if self.rigs.anim.playing || self.rigs.anim.applied != Some(f) {
            self.apply_anim(f);
        }
    }

    pub(super) fn anim_ui(&mut self, ui: &mut Ui) {
        let ctx = ui.ctx().clone();
        let now = ctx.input(|i| i.time);
        self.rig_init();
        self.anim_keys(&ctx);
        self.anim_playback(&ctx);
        self.phys_tick(&ctx);
        self.anim_export_step(&ctx);
        self.rig_render(&ctx);

        egui::Panel::bottom("anim_timeline").default_size(300.0).size_range(190.0..=760.0).resizable(true).frame(egui::Frame::new().fill(PANEL).inner_margin(egui::Margin::same(8))).show(ui, |ui| self.anim_timeline_ui(ui));
        egui::Panel::right("anim_side").default_size(320.0).size_range(260.0..=480.0).resizable(true).frame(egui::Frame::new().fill(PANEL).inner_margin(egui::Margin::same(10))).show(ui, |ui| {
            egui::ScrollArea::vertical().id_salt("anim_right").show(ui, |ui| self.anim_side_ui(ui, now));
        });
        egui::CentralPanel::default().frame(egui::Frame::new().fill(Color32::from_rgb(14, 20, 18))).show(ui, |ui| self.anim_preview(ui));
        self.anim_flush(&ctx);
    }

    /// Space plays, the arrows step, K puts a key, Delete takes it away.
    fn anim_keys(&mut self, ctx: &egui::Context) {
        if ctx.egui_wants_keyboard_input() {
            return;
        }
        let (space, left, right, home, end, k, del) = ctx.input_mut(|i| {
            use egui::{Key, Modifiers};
            (i.consume_key(Modifiers::NONE, Key::Space), i.consume_key(Modifiers::NONE, Key::ArrowLeft), i.consume_key(Modifiers::NONE, Key::ArrowRight), i.consume_key(Modifiers::NONE, Key::Home), i.consume_key(Modifiers::NONE, Key::End), i.consume_key(Modifiers::NONE, Key::K), i.consume_key(Modifiers::NONE, Key::Delete))
        });
        let (a, b) = self.draw.doc.rig.anim.play_range();
        if space {
            self.anim_toggle_play();
        }
        if left || right {
            self.rigs.anim.playing = false;
            let f = self.rigs.anim.frame.round() + if right { 1.0 } else { -1.0 };
            self.rigs.anim.frame = f.clamp(0.0, self.draw.doc.rig.anim.length.saturating_sub(1) as f32);
        }
        if home {
            self.rigs.anim.frame = a as f32;
        }
        if end {
            self.rigs.anim.frame = b as f32;
        }
        if k {
            self.anim_key_selected();
        }
        if del {
            self.anim_delete_selected();
        }
    }

    fn anim_toggle_play(&mut self) {
        let (a, b) = self.draw.doc.rig.anim.play_range();
        if !self.rigs.anim.playing && (self.rigs.anim.frame >= b as f32 || self.rigs.anim.frame < a as f32) {
            self.rigs.anim.frame = a as f32;
            self.rigs.anim.dir = 1.0;
        }
        self.rigs.anim.playing = !self.rigs.anim.playing;
    }

    fn anim_stop(&mut self) {
        self.rigs.anim.playing = false;
        self.rigs.anim.frame = self.draw.doc.rig.anim.play_range().0 as f32;
        self.rigs.anim.dir = 1.0;
        self.anim_restore_layers();
        self.rigs.anim.applied = None;
    }

    /// A key on the picked track at the playhead, with the value the slider has now.
    fn anim_key_selected(&mut self) {
        let Some(ti) = self.rigs.anim.sel_track else { return };
        let frame = self.rigs.anim.frame.round();
        let Some(t) = self.draw.doc.rig.anim.tracks.get(ti).cloned() else { return };
        let value = match t.target {
            Target::Param(id) => self.draw.doc.rig.param(id).map(|p| self.rigs.values.0.get(&id).copied().unwrap_or(p.default)),
            Target::Opacity(l) => self.draw.doc.layers.iter().find(|x| x.id == l).map(|x| x.opacity),
            Target::Visible(l) => self.draw.doc.layers.iter().find(|x| x.id == l).map(|x| if x.visible { 1.0 } else { 0.0 }),
        };
        if let Some(v) = value {
            let k = self.anim_mut("Key").tracks[ti].set_key(frame, v, Ease::Linear);
            self.rigs.anim.sel = Some((ti, k));
        }
    }

    fn anim_delete_selected(&mut self) {
        if let Some((ti, ki)) = self.rigs.anim.sel.take() {
            let a = self.anim_mut("Delete a key");
            if let Some(t) = a.tracks.get_mut(ti) {
                if ki < t.keys.len() {
                    t.keys.remove(ki);
                }
            }
        }
    }

    fn anim_preview(&mut self, ui: &mut Ui) {
        let rect = ui.available_rect_before_wrap();
        let resp = ui.allocate_rect(rect, Sense::click_and_drag());
        if resp.hovered() {
            let scroll = ui.input(|i| i.smooth_scroll_delta.y);
            if scroll != 0.0 && ui.input(|i| i.modifiers.command) {
                self.rigs.anim.view_zoom = (self.rigs.anim.view_zoom * (1.0 + scroll * 0.002)).clamp(0.3, 8.0);
            }
        }
        let p = ui.painter_at(rect);
        if let Some(t) = &self.rigs.image {
            let tex = t.size_vec2();
            let fit = ((rect.width() - 24.0) / tex.x).min((rect.height() - 24.0) / tex.y).max(0.05) * self.rigs.anim.view_zoom;
            let r = Rect::from_center_size(rect.center(), tex * fit);
            p.image(t.id(), r, Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)), Color32::WHITE);
        } else {
            p.text(rect.center(), egui::Align2::CENTER_CENTER, "Make a rig first (Rig tab) to animate it", egui::FontId::proportional(14.0), TEXT_DIM);
        }
        let a = &self.draw.doc.rig.anim;
        let f = self.rigs.anim.frame;
        p.text(rect.left_top() + vec2(12.0, 10.0), egui::Align2::LEFT_TOP, format!("Frame {} of {}   {:.2} s", f.round() as i32, a.length, f / a.fps.max(1.0)), egui::FontId::proportional(13.0), TEXT);
        if let Some(job) = &self.rigs.anim.job {
            p.text(rect.center_top() + vec2(0.0, 12.0), egui::Align2::CENTER_TOP, format!("Saving: frame {} of {}", job.next, job.frames.len()), egui::FontId::proportional(14.0), GOLD);
        }
    }

    // ---- the timeline ----

    fn anim_x(&self, key_left: f32, frame: f32) -> f32 {
        key_left + (frame - self.rigs.anim.scroll) * self.rigs.anim.zoom
    }

    fn anim_timeline_ui(&mut self, ui: &mut Ui) {
        let ctx = ui.ctx().clone();
        // the transport
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing = vec2(6.0, 4.0);
            let playing = self.rigs.anim.playing;
            if widgets::chip(ui, if playing { "Pause" } else { "Play" }, playing).on_hover_text("Space").clicked() {
                self.anim_toggle_play();
            }
            if ui.button("Stop").on_hover_text("Back to the start, and the layers as they were").clicked() {
                self.anim_stop();
            }
            if ui.button("Previous key").clicked() {
                let f = self.rigs.anim.frame;
                if let Some(k) = self.draw.doc.rig.anim.tracks.iter().flat_map(|t| t.keys.iter().map(|k| k.frame)).filter(|k| *k < f - 0.5).fold(None, |a: Option<f32>, k| Some(a.map_or(k, |x| x.max(k)))) {
                    self.rigs.anim.frame = k;
                    self.rigs.anim.playing = false;
                }
            }
            if ui.button("Next key").clicked() {
                let f = self.rigs.anim.frame;
                if let Some(k) = self.draw.doc.rig.anim.tracks.iter().flat_map(|t| t.keys.iter().map(|k| k.frame)).filter(|k| *k > f + 0.5).fold(None, |a: Option<f32>, k| Some(a.map_or(k, |x| x.min(k)))) {
                    self.rigs.anim.frame = k;
                    self.rigs.anim.playing = false;
                }
            }
            let mut a = self.draw.doc.rig.anim.clone();
            ui.checkbox(&mut a.looping, "Loop");
            ui.add_enabled(a.looping, egui::Checkbox::new(&mut a.ping_pong, "Back and forth"));
            ui.label(RichText::new("Frames a second").size(12.0).color(TEXT_DIM));
            ui.add(egui::DragValue::new(&mut a.fps).range(1.0..=120.0).speed(0.2));
            ui.label(RichText::new("Length").size(12.0).color(TEXT_DIM));
            ui.add(egui::DragValue::new(&mut a.length).range(2..=100_000).speed(0.5).suffix(" frames"));
            let here = self.rigs.anim.frame.round() as u32;
            if ui.small_button("Start here").on_hover_text("The part that plays starts at the playhead").clicked() {
                let end = a.play_range().1.max(here + 1);
                a.range = Some([here, end]);
            }
            if ui.small_button("End here").on_hover_text("The part that plays ends at the playhead").clicked() {
                let start = a.play_range().0.min(here.saturating_sub(1));
                a.range = Some([start, here.max(1)]);
            }
            if ui.small_button("Play all").clicked() {
                a.range = None;
            }
            if a != self.draw.doc.rig.anim {
                *self.anim_mut("Animation settings") = a;
            }
            let autokey = self.rigs.anim.autokey;
            if widgets::chip(ui, "Auto key", autokey).on_hover_text("A slider you move makes a key at the playhead").clicked() {
                self.rigs.anim.autokey = !autokey;
            }
            let (f, a) = (self.rigs.anim.frame, &self.draw.doc.rig.anim);
            ui.label(RichText::new(format!("Frame {}  ({:.2} s of {:.2} s)", f.round() as i32, f / a.fps.max(1.0), a.seconds())).monospace().size(12.0));
        });
        ui.add_space(4.0);

        // the tracks and their keys
        let avail = ui.available_rect_before_wrap();
        let tracks_n = self.draw.doc.rig.anim.tracks.len();
        let area = Rect::from_min_size(avail.min, vec2(avail.width(), avail.height().max(80.0)));
        let key_left = area.left() + NAMES;
        let rows_top = area.top() + RULER;
        let (resp, painter) = ui.allocate_painter(area.size(), Sense::click_and_drag());
        painter.rect_filled(area, 4.0, Color32::from_rgb(18, 24, 22));
        let zoom = self.rigs.anim.zoom;
        let scroll_now = self.rigs.anim.scroll;
        let xof = move |f: f32| key_left + (f - scroll_now) * zoom;
        let a = self.draw.doc.rig.anim.clone();
        // the ruler: a tick for frames, seconds by the label
        let step = {
            let mut s = 1u32;
            for c in [1u32, 2, 5, 10, 12, 24, 48, 120, 240, 600, 1200] {
                s = c;
                if c as f32 * zoom >= 44.0 {
                    break;
                }
            }
            s
        };
        painter.rect_filled(Rect::from_min_max(area.min, pos2(area.right(), rows_top)), 4.0, Color32::from_rgb(26, 34, 31));
        let first = (self.rigs.anim.scroll.floor() as i32).max(0);
        let last = (self.rigs.anim.scroll + (area.right() - key_left) / zoom).ceil() as i32;
        for f in first..=last.min(a.length as i32) {
            if f as u32 % step != 0 {
                continue;
            }
            let x = self.anim_x(key_left, f as f32);
            if x < key_left || x > area.right() {
                continue;
            }
            painter.line_segment([pos2(x, rows_top - 8.0), pos2(x, rows_top)], Stroke::new(1.0, TEXT_DIM));
            painter.text(pos2(x + 3.0, area.top() + 4.0), egui::Align2::LEFT_TOP, format!("{f}"), egui::FontId::proportional(10.5), TEXT_DIM);
        }
        // the work area and the end
        let (r0, r1) = a.play_range();
        let (rx0, rx1) = (self.anim_x(key_left, r0 as f32).max(key_left), self.anim_x(key_left, r1 as f32 + 1.0).min(area.right()));
        if rx1 > rx0 {
            painter.rect_filled(Rect::from_min_max(pos2(rx0, rows_top - 4.0), pos2(rx1, rows_top)), 0.0, Color32::from_rgba_unmultiplied(60, 200, 130, 90));
        }
        let endx = self.anim_x(key_left, a.length as f32);
        if endx >= key_left && endx <= area.right() {
            painter.line_segment([pos2(endx, rows_top), pos2(endx, area.bottom())], Stroke::new(1.0, Color32::from_rgb(120, 60, 60)));
        }
        // the rows
        let selected_track = self.rigs.anim.sel_track;
        for (ti, t) in a.tracks.iter().enumerate() {
            let y = rows_top + ti as f32 * ROW;
            if y > area.bottom() {
                break;
            }
            let row = Rect::from_min_max(pos2(area.left(), y), pos2(area.right(), y + ROW));
            if selected_track == Some(ti) {
                painter.rect_filled(row, 0.0, Color32::from_rgba_unmultiplied(255, 255, 255, 14));
            } else if ti % 2 == 1 {
                painter.rect_filled(row, 0.0, Color32::from_rgba_unmultiplied(255, 255, 255, 5));
            }
            let col = target_color(&t.target);
            painter.circle_filled(pos2(area.left() + 10.0, y + ROW / 2.0), 4.0, if t.muted { Color32::from_gray(80) } else { col });
            painter.text(pos2(area.left() + 20.0, y + ROW / 2.0), egui::Align2::LEFT_CENTER, &t.name, egui::FontId::proportional(12.0), if t.muted { TEXT_DIM } else { TEXT });
            // a line between the keys, to see how long a value holds
            for w in t.keys.windows(2) {
                let (x0, x1) = (self.anim_x(key_left, w[0].frame).max(key_left), self.anim_x(key_left, w[1].frame).min(area.right()));
                if x1 > x0 {
                    painter.line_segment([pos2(x0, y + ROW / 2.0), pos2(x1, y + ROW / 2.0)], Stroke::new(1.0, Color32::from_rgba_unmultiplied(col.r(), col.g(), col.b(), 70)));
                }
            }
            for (ki, k) in t.keys.iter().enumerate() {
                let x = self.anim_x(key_left, k.frame);
                if x < key_left - 6.0 || x > area.right() + 6.0 {
                    continue;
                }
                let on = self.rigs.anim.sel == Some((ti, ki));
                let c = pos2(x, y + ROW / 2.0);
                let d = if on { 7.0 } else { 5.5 };
                painter.add(egui::Shape::convex_polygon(vec![c + vec2(0.0, -d), c + vec2(d, 0.0), c + vec2(0.0, d), c + vec2(-d, 0.0)], if on { Color32::WHITE } else { col }, Stroke::new(1.0, Color32::BLACK)));
            }
        }
        if tracks_n == 0 {
            painter.text(pos2(key_left + 16.0, rows_top + 14.0), egui::Align2::LEFT_CENTER, "No tracks yet. Add one on the right: a slider, or a layer.", egui::FontId::proportional(12.5), TEXT_DIM);
        }
        // the markers and the playhead
        let px = self.anim_x(key_left, self.rigs.anim.frame);
        if px >= key_left && px <= area.right() {
            painter.line_segment([pos2(px, area.top()), pos2(px, area.bottom())], Stroke::new(1.5, Color32::from_rgb(255, 90, 90)));
            painter.add(egui::Shape::convex_polygon(vec![pos2(px - 6.0, area.top()), pos2(px + 6.0, area.top()), pos2(px, area.top() + 10.0)], Color32::from_rgb(255, 90, 90), Stroke::NONE));
        }
        painter.line_segment([pos2(key_left, area.top()), pos2(key_left, area.bottom())], Stroke::new(1.0, Color32::from_gray(50)));

        // the mouse
        let at = |pos: Pos2| -> Option<(usize, Option<usize>)> {
            if pos.y < rows_top || pos.x < area.left() {
                return None;
            }
            let ti = ((pos.y - rows_top) / ROW) as usize;
            let t = a.tracks.get(ti)?;
            let k = t.keys.iter().position(|k| (xof(k.frame) - pos.x).abs() < 7.0 && pos.x >= key_left - 4.0);
            Some((ti, k))
        };
        let frame_at = move |x: f32| ((x - key_left) / zoom + scroll_now).max(0.0);
        if resp.hovered() {
            let (scroll, zoom_in) = ui.input(|i| (i.smooth_scroll_delta, i.modifiers.command));
            if zoom_in && scroll.y != 0.0 {
                let pos = ui.input(|i| i.pointer.hover_pos()).unwrap_or(area.center());
                let f = frame_at(pos.x);
                self.rigs.anim.zoom = (zoom * (1.0 + scroll.y * 0.003)).clamp(1.5, 80.0);
                self.rigs.anim.scroll = (f - (pos.x - key_left) / self.rigs.anim.zoom).max(0.0);
            } else if scroll.x != 0.0 || scroll.y != 0.0 {
                self.rigs.anim.scroll = (self.rigs.anim.scroll - (scroll.x + scroll.y) / zoom).max(0.0);
            }
        }
        if resp.drag_started_by(egui::PointerButton::Middle) {
            self.rigs.anim.drag = Some(Drag::Pan { x0: ui.input(|i| i.pointer.press_origin()).map_or(0.0, |p| p.x), scroll0: self.rigs.anim.scroll });
        } else if resp.drag_started_by(egui::PointerButton::Primary) {
            if let Some(origin) = ui.input(|i| i.pointer.press_origin()) {
                self.rigs.anim.drag = if origin.y < rows_top && origin.x >= key_left {
                    Some(Drag::Scrub)
                } else {
                    match at(origin) {
                        Some((ti, Some(ki))) => {
                            self.rigs.anim.sel = Some((ti, ki));
                            self.rigs.anim.sel_track = Some(ti);
                            Some(Drag::Key { track: ti, key: ki })
                        }
                        _ => None,
                    }
                };
            }
        }
        if resp.dragged() {
            if let Some(pos) = resp.interact_pointer_pos() {
                match &self.rigs.anim.drag {
                    Some(Drag::Scrub) => {
                        self.rigs.anim.frame = frame_at(pos.x).round().min(a.length.saturating_sub(1) as f32);
                        self.rigs.anim.playing = false;
                    }
                    Some(Drag::Key { track, key }) => {
                        let (track, key) = (*track, *key);
                        let f = frame_at(pos.x).round();
                        let moved = self.anim_mut("Move a key").tracks.get_mut(track).map(|t| t.move_key(key, f));
                        if let Some(nk) = moved {
                            self.rigs.anim.drag = Some(Drag::Key { track, key: nk });
                            self.rigs.anim.sel = Some((track, nk));
                        }
                    }
                    Some(Drag::Pan { x0, scroll0 }) => {
                        self.rigs.anim.scroll = (scroll0 - (pos.x - x0) / zoom).max(0.0);
                    }
                    None => {}
                }
            }
        }
        if resp.drag_stopped() {
            self.rigs.anim.drag = None;
        }
        // a click: on the ruler the playhead goes there, on a key it is picked, on a row the track is picked
        if resp.clicked_by(egui::PointerButton::Primary) {
            if let Some(pos) = resp.interact_pointer_pos() {
                if pos.y < rows_top && pos.x >= key_left {
                    self.rigs.anim.frame = frame_at(pos.x).round().min(a.length.saturating_sub(1) as f32);
                    self.rigs.anim.playing = false;
                } else if let Some((ti, k)) = at(pos) {
                    self.rigs.anim.sel_track = Some(ti);
                    self.rigs.anim.sel = k.map(|ki| (ti, ki));
                    if let Some(ki) = k {
                        self.rigs.anim.frame = a.tracks[ti].keys[ki].frame;
                    }
                } else {
                    self.rigs.anim.sel = None;
                }
            }
        }
        // a double click on a row puts a key there
        if resp.double_clicked() {
            if let Some(pos) = resp.interact_pointer_pos() {
                if let Some((ti, None)) = at(pos) {
                    if pos.x >= key_left {
                        self.rigs.anim.frame = frame_at(pos.x).round();
                        self.rigs.anim.sel_track = Some(ti);
                        self.anim_key_selected();
                    }
                }
            }
        }
        if resp.secondary_clicked() || ui.input(|i| i.pointer.secondary_pressed()) && resp.hovered() {
            if let Some(pos) = ui.input(|i| i.pointer.hover_pos()) {
                self.rigs.anim.ctx_key = at(pos).and_then(|(ti, k)| k.map(|ki| (ti, ki)));
            }
        }
        resp.context_menu(|ui| {
            let Some((ti, ki)) = self.rigs.anim.ctx_key else {
                ui.label(RichText::new("Right click a key").size(12.0).color(TEXT_DIM));
                return;
            };
            ui.label(RichText::new("From this key to the next").size(12.0).color(TEXT_DIM));
            for (e, name) in Ease::NAMED {
                if ui.button(name).clicked() {
                    if let Some(k) = self.anim_mut("Easing").tracks.get_mut(ti).and_then(|t| t.keys.get_mut(ki)) {
                        k.ease = e;
                    }
                    ui.close();
                }
            }
            if ui.button("Curve").on_hover_text("A curve with two handles, set on the right").clicked() {
                if let Some(k) = self.anim_mut("Easing").tracks.get_mut(ti).and_then(|t| t.keys.get_mut(ki)) {
                    k.ease = Ease::Bezier([0.42, 0.0, 0.58, 1.0]);
                }
                ui.close();
            }
            ui.separator();
            if ui.button("Delete the key").clicked() {
                if let Some(t) = self.anim_mut("Delete a key").tracks.get_mut(ti) {
                    if ki < t.keys.len() {
                        t.keys.remove(ki);
                    }
                }
                self.rigs.anim.sel = None;
                ui.close();
            }
        });
        let _ = ctx;
    }

    // ---- the right side ----

    fn anim_side_ui(&mut self, ui: &mut Ui, now: f64) {
        let _ = now;
        section_title(ui, "Sliders at the playhead");
        ui.label(RichText::new("Move a slider to pose the character at this frame. With Auto key on (under the timeline) the move makes a key; otherwise use Key this slider.").size(11.5).color(TEXT_DIM));
        let params: Vec<(Id, String, f32, f32, f32)> = self.draw.doc.rig.params.iter().map(|p| (p.id, p.name.clone(), p.min, p.max, p.default)).collect();
        if params.is_empty() {
            ui.label(RichText::new("The rig has no sliders yet. The Rig tab makes them (Auto-rig, or Rig the whole picture).").size(11.5).color(TEXT_DIM));
        }
        let frame = self.rigs.anim.frame.round();
        let autokey = self.rigs.anim.autokey;
        let mut key_it: Vec<(Id, String, f32)> = Vec::new();
        for (id, name, lo, hi, def) in &params {
            let mut v = self.rigs.values.0.get(id).copied().unwrap_or(*def);
            let before = v;
            let keyed = self.draw.doc.rig.anim.track(Target::Param(*id)).and_then(|t| self.draw.doc.rig.anim.tracks[t].key_at(frame)).is_some();
            ui.horizontal(|ui| {
                ui.label(RichText::new(name).size(12.0).color(if keyed { GOLD } else { TEXT }));
                let w = (ui.available_width() - 60.0).max(60.0);
                ui.spacing_mut().slider_width = w;
                ui.add(egui::Slider::new(&mut v, *lo..=*hi).show_value(false));
                if ui.small_button(if keyed { "Keyed" } else { "Key" }).on_hover_text("Key this slider at the playhead").clicked() {
                    key_it.push((*id, name.clone(), v));
                }
            });
            if (v - before).abs() > 1e-6 {
                self.rigs.values.set(*id, v);
                if autokey {
                    key_it.push((*id, name.clone(), v));
                }
            }
        }
        for (id, name, v) in key_it {
            let a = self.anim_mut("Key a slider");
            let t = a.track_for(Target::Param(id), &name);
            let k = a.tracks[t].set_key(frame, v, Ease::Linear);
            self.rigs.anim.sel = Some((t, k));
            self.rigs.anim.sel_track = Some(t);
            self.rigs.anim.applied = Some(self.rigs.anim.frame);
        }
        // the key that is picked
        if let Some((ti, ki)) = self.rigs.anim.sel {
            let key = self.draw.doc.rig.anim.tracks.get(ti).and_then(|t| t.keys.get(ki).map(|k| (t.name.clone(), *k)));
            if let Some((name, mut k)) = key {
                section_title(ui, "The key");
                ui.label(RichText::new(&name).size(12.0).color(TEXT_DIM));
                let before = k;
                ui.horizontal(|ui| {
                    ui.label(RichText::new("Frame").size(12.0).color(TEXT_DIM));
                    ui.add(egui::DragValue::new(&mut k.frame).range(0.0..=100_000.0).speed(0.2));
                    ui.label(RichText::new("Value").size(12.0).color(TEXT_DIM));
                    ui.add(egui::DragValue::new(&mut k.value).speed(0.01));
                });
                ui.horizontal_wrapped(|ui| {
                    for (e, label) in Ease::NAMED {
                        if widgets::chip(ui, label, k.ease == e).clicked() {
                            k.ease = e;
                        }
                    }
                    if widgets::chip(ui, "Curve", matches!(k.ease, Ease::Bezier(_))).clicked() && !matches!(k.ease, Ease::Bezier(_)) {
                        k.ease = Ease::Bezier([0.42, 0.0, 0.58, 1.0]);
                    }
                });
                if let Ease::Bezier(h) = &mut k.ease {
                    ui.horizontal(|ui| {
                        for (i, n) in ["x1", "y1", "x2", "y2"].iter().enumerate() {
                            ui.label(RichText::new(*n).size(11.5).color(TEXT_DIM));
                            ui.add(egui::DragValue::new(&mut h[i]).range(if i % 2 == 0 { 0.0..=1.0 } else { -1.0..=2.0 }).speed(0.01));
                        }
                    });
                    let (r, _) = ui.allocate_exact_size(vec2(120.0, 90.0), Sense::hover());
                    let p = ui.painter();
                    p.rect_filled(r, 3.0, Color32::from_rgb(18, 24, 22));
                    let e = Ease::Bezier(*h);
                    let pts: Vec<Pos2> = (0..=30).map(|i| { let t = i as f32 / 30.0; pos2(r.left() + t * r.width(), r.bottom() - 6.0 - e.at(t) * (r.height() - 12.0)) }).collect();
                    p.add(egui::Shape::line(pts, Stroke::new(1.5, VENOM)));
                }
                if k != before {
                    let frame_changed = (k.frame - before.frame).abs() > 1e-6;
                    let a = self.anim_mut("Edit a key");
                    if let Some(t) = a.tracks.get_mut(ti) {
                        if frame_changed {
                            let n = t.move_key(ki, k.frame);
                            t.keys[n].value = k.value;
                            t.keys[n].ease = k.ease;
                            self.rigs.anim.sel = Some((ti, n));
                        } else if let Some(slot) = t.keys.get_mut(ki) {
                            *slot = k;
                        }
                    }
                    self.rigs.anim.applied = None;
                }
                if ui.small_button("Delete the key").clicked() {
                    self.anim_delete_selected();
                }
            }
        }
        // tracks
        section_title(ui, "Tracks");
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing = vec2(4.0, 4.0);
            let layers: Vec<(u64, String)> = self.draw.doc.layers.iter().filter(|l| matches!(l.kind, crate::doc::LayerKind::Raster | crate::doc::LayerKind::Vector)).map(|l| (l.id, l.name.clone())).collect();
            egui::ComboBox::from_id_salt("anim_add_layer").selected_text("Add a layer track").width(150.0).show_ui(ui, |ui| {
                for (id, name) in &layers {
                    for (target, label) in [(Target::Opacity(*id), format!("{name}: see-through")), (Target::Visible(*id), format!("{name}: shown"))] {
                        if ui.selectable_label(false, &label).clicked() {
                            let frame = self.rigs.anim.frame.round();
                            let (cur_op, cur_vis) = self.draw.doc.layers.iter().find(|l| l.id == *id).map(|l| (l.opacity, if l.visible { 1.0 } else { 0.0 })).unwrap_or((100.0, 1.0));
                            let a = self.anim_mut("Add a layer track");
                            let t = a.track_for(target, &label);
                            a.tracks[t].set_key(frame, if matches!(target, Target::Opacity(_)) { cur_op } else { cur_vis }, if matches!(target, Target::Visible(_)) { Ease::Step } else { Ease::Linear });
                            self.rigs.anim.sel_track = Some(t);
                        }
                    }
                }
            });
        });
        let tracks: Vec<(usize, String, bool, usize)> = self.draw.doc.rig.anim.tracks.iter().enumerate().map(|(i, t)| (i, t.name.clone(), t.muted, t.keys.len())).collect();
        let mut remove: Option<usize> = None;
        let mut mute: Option<usize> = None;
        for (i, name, muted, n) in &tracks {
            ui.horizontal(|ui| {
                if widgets::chip(ui, name, self.rigs.anim.sel_track == Some(*i)).clicked() {
                    self.rigs.anim.sel_track = Some(*i);
                }
                ui.label(RichText::new(format!("{n} keys")).size(11.0).color(TEXT_DIM));
                if ui.small_button(if *muted { "Off" } else { "On" }).on_hover_text("Mute or unmute the track").clicked() {
                    mute = Some(*i);
                }
                if ui.small_button("Delete").clicked() {
                    remove = Some(*i);
                }
            });
        }
        if let Some(i) = mute {
            let t = &mut self.anim_mut("Mute a track").tracks[i];
            t.muted = !t.muted;
            self.rigs.anim.applied = None;
        }
        if let Some(i) = remove {
            self.anim_mut("Delete a track").remove_track(i);
            self.rigs.anim.sel = None;
            self.rigs.anim.sel_track = None;
        }
        // clips: recorded poses and the motions of a Live2D model
        let clips: Vec<(usize, String, f32)> = self.draw.doc.rig.clips.iter().enumerate().map(|(i, c)| (i, c.name.clone(), c.seconds())).collect();
        if !clips.is_empty() {
            section_title(ui, "Clips");
            ui.label(RichText::new("Recorded poses and motions that came with a model. Add one at the playhead: it becomes tracks with only the keys that matter.").size(11.5).color(TEXT_DIM));
            for (i, name, secs) in clips {
                ui.horizontal(|ui| {
                    ui.label(RichText::new(format!("{name} ({secs:.1} s)")).size(12.0));
                    if ui.small_button("Add").clicked() {
                        let clip = self.draw.doc.rig.clips[i].clone();
                        let start = self.rigs.anim.frame.round();
                        let rig = self.draw.doc.rig.clone();
                        let names = move |id: Id| rig.param(id).map(|p| p.name.clone()).unwrap_or_else(|| "Slider".into());
                        let rig2 = self.draw.doc.rig.clone();
                        let ranges = move |id: Id| rig2.param(id).map(|p| p.max - p.min).unwrap_or(1.0);
                        let n = self.anim_mut("Add a clip").add_clip(&clip, start, &names, &ranges);
                        self.rigs.anim.message = Some(format!("{n} slider tracks added from {name}."));
                        self.rigs.anim.applied = None;
                    }
                });
            }
        }
        self.anim_export_ui(ui);
        if let Some(m) = &self.rigs.anim.message {
            ui.label(RichText::new(m).size(11.5).color(TEXT_DIM));
        }
    }

    // ---- saving the animation ----

    fn anim_export_ui(&mut self, ui: &mut Ui) {
        section_title(ui, "Save the animation");
        ui.horizontal_wrapped(|ui| {
            for (i, n) in ["PNG pictures", "GIF", "MP4 video"].iter().enumerate() {
                if widgets::chip(ui, n, self.rigs.anim.export_kind == i as u8).clicked() {
                    self.rigs.anim.export_kind = i as u8;
                }
            }
        });
        ui.horizontal_wrapped(|ui| {
            ui.label(RichText::new("Biggest side").size(12.0).color(TEXT_DIM));
            ui.add(egui::DragValue::new(&mut self.rigs.anim.export_side).range(64..=4096).speed(4.0).suffix(" px"));
            ui.checkbox(&mut self.rigs.anim.export_work_area, "Only the part that plays");
            if self.rigs.anim.export_kind != 2 {
                ui.checkbox(&mut self.rigs.anim.export_transparent, "See-through background");
            }
        });
        let busy = self.rigs.anim.job.is_some();
        if busy {
            let (n, total) = self.rigs.anim.job.as_ref().map(|j| (j.next, j.frames.len())).unwrap_or((0, 1));
            ui.add(egui::ProgressBar::new(n as f32 / total.max(1) as f32).text(format!("Frame {n} of {total}")));
            if ui.button("Cancel").clicked() {
                self.anim_export_cancel();
            }
        } else if ui.button("Save...").on_hover_text("Plays the animation frame by frame, with the physics, and writes it").clicked() {
            self.anim_export_start();
        }
    }

    fn anim_export_start(&mut self) {
        let a = self.draw.doc.rig.anim.clone();
        let (first, last) = if self.rigs.anim.export_work_area { a.play_range() } else { (0, a.length.saturating_sub(1)) };
        let frames: Vec<u32> = (first..=last).collect();
        if frames.is_empty() {
            return;
        }
        let kind = self.rigs.anim.export_kind;
        let (w, h) = (self.draw.doc.w, self.draw.doc.h);
        let size = if kind == 2 { crate::recorder::video_size(w, h, self.rigs.anim.export_side) } else {
            let k = (self.rigs.anim.export_side as f32 / w.max(h) as f32).min(1.0);
            (((w as f32 * k).round() as u32).max(8), ((h as f32 * k).round() as u32).max(8))
        };
        let name = self.files.name.clone();
        let mut job = ExportJob { kind, frames, next: 0, size, transparent: self.rigs.anim.export_transparent && kind != 2, dir: PathBuf::new(), path: PathBuf::new(), gif: None, mp4: None, base_values: self.rigs.values.clone(), error: None };
        match kind {
            0 => {
                let Some(dir) = rfd::FileDialog::new().set_title("Folder for the pictures").set_directory(self.settings.projects_folder()).pick_folder() else { return };
                job.dir = dir;
            }
            1 => {
                let Some(p) = rfd::FileDialog::new().set_title("Save the GIF").set_directory(self.settings.projects_folder()).set_file_name(format!("{name}.gif")).add_filter("GIF", &["gif"]).save_file() else { return };
                match std::fs::File::create(&p) {
                    Ok(f) => {
                        let mut enc = image::codecs::gif::GifEncoder::new(f);
                        let _ = enc.set_repeat(image::codecs::gif::Repeat::Infinite);
                        job.gif = Some(enc);
                        job.path = p;
                    }
                    Err(e) => {
                        self.rigs.anim.message = Some(format!("Could not make the file: {e}"));
                        return;
                    }
                }
            }
            _ => {
                let Some(p) = rfd::FileDialog::new().set_title("Save the video").set_directory(self.settings.projects_folder()).set_file_name(format!("{name}.mp4")).add_filter("MP4 video", &["mp4"]).save_file() else { return };
                match crate::recorder::Writer::new(&p, size, a.fps.round().max(1.0) as u32) {
                    Ok(w) => {
                        job.mp4 = Some(w);
                        job.path = p;
                    }
                    Err(e) => {
                        self.rigs.anim.message = Some(e);
                        return;
                    }
                }
            }
        }
        self.rigs.anim.playing = false;
        self.rigs.anim.job = Some(job);
    }

    fn anim_export_cancel(&mut self) {
        if let Some(job) = self.rigs.anim.job.take() {
            self.rigs.values = job.base_values;
            self.rigs.anim.message = Some("Saving was cancelled.".into());
            self.rigs.anim.applied = None;
        }
    }

    /// Do a few frames of the saving each time the picture is shown, so the program stays alive.
    fn anim_export_step(&mut self, ctx: &egui::Context) {
        let Some(mut job) = self.rigs.anim.job.take() else { return };
        let t0 = std::time::Instant::now();
        let fps = self.draw.doc.rig.anim.fps.max(1.0);
        let mut done = false;
        while t0.elapsed().as_millis() < 60 {
            let Some(f) = job.frames.get(job.next).copied() else {
                done = true;
                break;
            };
            self.apply_anim(f as f32);
            // the physics goes on frame by frame
            let mut left = 1.0 / fps;
            while left > 1e-6 {
                let dt = left.min(1.0 / 60.0);
                self.phys_advance(dt);
                left -= dt;
            }
            let paper = if job.transparent { None } else { Some(Color32::WHITE) };
            let Some(img) = self.rig_still(job.size, paper) else {
                job.error = Some("There is nothing to draw: the rig has no meshes yet.".into());
                done = true;
                break;
            };
            let r = match job.kind {
                0 => img.save(job.dir.join(format!("frame_{:04}.png", f + 1))).map_err(|e| e.to_string()),
                1 => match job.gif.as_mut() {
                    Some(enc) => enc.encode_frame(image::Frame::from_parts(img, 0, 0, image::Delay::from_numer_denom_ms(1000, fps.round() as u32))).map_err(|e| e.to_string()),
                    None => Ok(()),
                },
                _ => match job.mp4.as_mut() {
                    Some(w) => {
                        let mut rgb = Vec::with_capacity((img.width() * img.height() * 3) as usize);
                        for p in img.pixels() {
                            let a = p[3] as u32;
                            for c in &p.0[..3] {
                                rgb.push(((*c as u32 * a + 255 * (255 - a) + 127) / 255) as u8);
                            }
                        }
                        w.write(rgb)
                    }
                    None => Ok(()),
                },
            };
            if let Err(e) = r {
                job.error = Some(e);
                done = true;
                break;
            }
            job.next += 1;
            if job.next >= job.frames.len() {
                done = true;
                break;
            }
        }
        if done {
            self.rigs.values = job.base_values.clone();
            self.rigs.anim.applied = None;
            let n = job.next;
            if let Some(w) = job.mp4.take() {
                w.finish();
            }
            drop(job.gif.take());
            self.rigs.anim.message = Some(match job.error {
                Some(e) => format!("Saving stopped: {e}"),
                None => match job.kind {
                    0 => format!("Saved {n} pictures in {}.", job.dir.display()),
                    _ => format!("Saved {} ({n} frames).", job.path.display()),
                },
            });
        } else {
            self.rigs.anim.job = Some(job);
            ctx.request_repaint();
        }
    }

    #[cfg(test)]
    pub fn test_anim(&mut self) -> &mut AnimUi {
        &mut self.rigs.anim
    }

    /// Save to a folder without the dialog: used by the tests.
    #[cfg(test)]
    pub fn test_anim_export_png(&mut self, dir: PathBuf, side: u32) {
        let a = self.draw.doc.rig.anim.clone();
        let (w, h) = (self.draw.doc.w, self.draw.doc.h);
        let k = (side as f32 / w.max(h) as f32).min(1.0);
        let size = (((w as f32 * k).round() as u32).max(8), ((h as f32 * k).round() as u32).max(8));
        let job = ExportJob { kind: 0, frames: (0..a.length).collect(), next: 0, size, transparent: false, dir, path: PathBuf::new(), gif: None, mp4: None, base_values: self.rigs.values.clone(), error: None };
        self.rigs.anim.job = Some(job);
    }

    /// Save a GIF (kind 1) or an MP4 (kind 2) to a path without the dialog: used by the tests.
    #[cfg(test)]
    pub fn test_anim_export_file(&mut self, kind: u8, path: PathBuf, side: u32) {
        let a = self.draw.doc.rig.anim.clone();
        let (w, h) = (self.draw.doc.w, self.draw.doc.h);
        let size = if kind == 2 { crate::recorder::video_size(w, h, side) } else {
            let k = (side as f32 / w.max(h) as f32).min(1.0);
            (((w as f32 * k).round() as u32).max(8), ((h as f32 * k).round() as u32).max(8))
        };
        let mut job = ExportJob { kind, frames: (0..a.length).collect(), next: 0, size, transparent: kind == 1, dir: PathBuf::new(), path: path.clone(), gif: None, mp4: None, base_values: self.rigs.values.clone(), error: None };
        if kind == 1 {
            let mut enc = image::codecs::gif::GifEncoder::new(std::fs::File::create(&path).unwrap());
            let _ = enc.set_repeat(image::codecs::gif::Repeat::Infinite);
            job.gif = Some(enc);
        } else {
            job.mp4 = Some(crate::recorder::Writer::new(&path, size, a.fps.round().max(1.0) as u32).unwrap());
        }
        self.rigs.anim.job = Some(job);
    }

    #[cfg(test)]
    pub fn test_still(&mut self, side: u32) -> Option<image::RgbaImage> {
        let (w, h) = (self.draw.doc.w, self.draw.doc.h);
        let k = (side as f32 / w.max(h) as f32).min(1.0);
        self.rig_still((((w as f32 * k).round() as u32).max(8), ((h as f32 * k).round() as u32).max(8)), Some(Color32::WHITE))
    }

    #[cfg(test)]
    pub fn test_anim_busy(&self) -> bool {
        self.rigs.anim.job.is_some()
    }
}
