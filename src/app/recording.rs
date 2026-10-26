// SPDX-License-Identifier: GPL-3.0-or-later
// The Recorder window and the red "REC" light in the top bar. It films only the picture (see recorder.rs).

use crate::widgets::HelpExt;
use eframe::egui::{self, Align2, Color32, RichText, vec2};

use super::SeviathApp;
use crate::recorder::{self, QUALITIES, RecConfig, RecMode, Session};
use crate::theme::*;
use crate::widgets;

#[derive(Default)]
pub struct RecUi {
    pub open: bool,
    pub cfg: RecConfig,
    pub session: Option<Session>,
    message: Option<String>,
}

fn clock(secs: f64) -> String {
    let s = secs.max(0.0) as u64;
    format!("{:02}:{:02}", s / 60, s % 60)
}

impl SeviathApp {
    pub(super) fn open_recorder(&mut self) {
        self.rec.open = true;
    }

    #[cfg(test)]
    pub fn test_open_recorder(&mut self) {
        self.open_recorder();
    }

    #[cfg(test)]
    pub fn test_recording(&self) -> Option<(u32, bool)> {
        self.rec.session.as_ref().map(|s| (s.frames(), s.paused))
    }

    /// Each finished change is one frame of a timelapse.
    pub(super) fn recorder_step(&mut self, after: &crate::history::DocState) {
        if let Some(s) = self.rec.session.as_mut() {
            if s.cfg.mode == RecMode::Timelapse {
                s.push(self.draw.doc.w, self.draw.doc.h, after.clone());
            }
        }
    }

    /// Every frame of the app.
    pub(super) fn recorder_tick(&mut self, ctx: &egui::Context) {
        let Some(s) = self.rec.session.as_mut() else { return };
        let now = ctx.input(|i| i.time);
        let d = &self.draw.doc;
        let rev = d.revision;
        s.tick(now, rev, || (d.w, d.h, d.snapshot_live()));
        if let Some(e) = s.error() {
            self.rec.message = Some(e);
            if let Some(s) = self.rec.session.take() {
                s.cancel();
            }
            return;
        }
        // keep the clock and the real-time frames going while nothing else moves
        let fps = s.cfg.fps.max(1) as f32;
        ctx.request_repaint_after(std::time::Duration::from_secs_f32(if s.cfg.mode == RecMode::RealTime { 1.0 / fps } else { 0.5 }));
    }

    /// The red light in the top bar.
    pub(super) fn rec_light(&mut self, ui: &mut egui::Ui) {
        let Some(s) = self.rec.session.as_ref() else { return };
        let (txt, col) = if s.paused { (format!("PAUSED {}", clock(s.elapsed)), TEXT_DIM) } else { (format!("REC {}", clock(s.elapsed)), Color32::from_rgb(235, 70, 70)) };
        if ui.add(egui::Button::new(RichText::new(txt).color(col).strong()).corner_radius(14)).help("The recorder (it films only the picture)").clicked() {
            self.rec.open = true;
        }
    }

    fn stop_recording(&mut self, save_to: Option<std::path::PathBuf>) {
        let Some(s) = self.rec.session.take() else { return };
        let last = Some((self.draw.doc.w, self.draw.doc.h, self.draw.doc.snapshot_live()));
        let name = self.files.name.clone();
        let kind = if s.cfg.mode == RecMode::Timelapse { "timelapse" } else { "recording" };
        match s.finish(last) {
            Err(e) => self.rec.message = Some(e),
            Ok(done) => {
                let dest = save_to.or_else(|| {
                    let dir = self.files.path.as_ref().and_then(|p| p.parent().map(std::path::Path::to_path_buf)).unwrap_or_else(|| self.settings.projects_folder());
                    rfd::FileDialog::new().set_title("Save the video").set_directory(dir).set_file_name(format!("{name}-{kind}.mp4")).add_filter("MP4 video", &["mp4"]).save_file()
                });
                self.rec.message = Some(match dest {
                    Some(p) => match recorder::deliver(&done.path, &p) {
                        Ok(()) => format!("Saved: {} frames, {:.1} s{}.", done.frames, done.seconds, if done.dropped > 0 { format!(", {} frames skipped (the computer was too busy)", done.dropped) } else { String::new() }),
                        Err(e) => e,
                    },
                    None => {
                        let _ = std::fs::remove_file(&done.path);
                        "Not saved: the video was thrown away.".into()
                    }
                });
            }
        }
    }

    #[cfg(test)]
    pub fn test_stop_recording(&mut self, to: std::path::PathBuf) {
        self.stop_recording(Some(to));
    }

    #[cfg(test)]
    pub fn test_recorder_message(&self) -> Option<String> {
        self.rec.message.clone()
    }

    pub(super) fn recorder_window(&mut self, ctx: &egui::Context) {
        if !self.rec.open {
            return;
        }
        let mut open = true;
        let (mut start, mut pause, mut stop, mut discard) = (false, false, false, false);
        egui::Window::new(RichText::new("Recorder").strong())
            .open(&mut open)
            .collapsible(false)
            .resizable(false)
            .pivot(Align2::CENTER_CENTER)
            .default_pos(ctx.content_rect().center())
            .show(ctx, |ui| {
                ui.set_width(430.0);
                ui.label(RichText::new("Films only your picture (all layers and the paper), not the app. Panels, the cursor and other windows are never in the video.").size(11.5).color(TEXT_DIM));
                ui.add_space(4.0);
                let busy = self.rec.session.is_some();
                ui.add_enabled_ui(!busy, |ui| {
                    let c = &mut self.rec.cfg;
                    ui.horizontal_wrapped(|ui| {
                        ui.spacing_mut().item_spacing = vec2(4.0, 4.0);
                        ui.label(RichText::new("Kind").size(12.0).color(TEXT_DIM));
                        if widgets::chip(ui, "Timelapse", c.mode == RecMode::Timelapse).help("One frame after every finished change: hours become a short clip").clicked() {
                            c.mode = RecMode::Timelapse;
                            c.fps = 12;
                        }
                        if widgets::chip(ui, "Real time", c.mode == RecMode::RealTime).help("Frames at a steady rate while you draw, strokes appear as you paint them").clicked() {
                            c.mode = RecMode::RealTime;
                            c.fps = 15;
                        }
                    });
                    ui.horizontal_wrapped(|ui| {
                        ui.spacing_mut().item_spacing = vec2(4.0, 4.0);
                        ui.label(RichText::new("Size").size(12.0).color(TEXT_DIM));
                        for (px, name) in QUALITIES {
                            if widgets::chip(ui, name, c.max_side == px).clicked() {
                                c.max_side = px;
                            }
                        }
                    });
                    ui.horizontal(|ui| {
                        ui.label(RichText::new(if c.mode == RecMode::Timelapse { "Speed" } else { "Frames per second" }).size(12.0).color(TEXT_DIM));
                        ui.add(egui::Slider::new(&mut c.fps, 4..=30).suffix(if c.mode == RecMode::Timelapse { " changes per second" } else { " fps" }));
                    });
                    let (w, h) = recorder::video_size(self.draw.doc.w, self.draw.doc.h, c.max_side);
                    ui.label(RichText::new(format!("The video will be {w} x {h} px.")).size(11.5).color(TEXT_DIM));
                });
                ui.add_space(4.0);
                match self.rec.session.as_ref() {
                    None => {
                        ui.horizontal(|ui| {
                            if ui.button(RichText::new("Start recording").strong()).clicked() {
                                start = true;
                            }
                            if let Some(m) = &self.rec.message {
                                ui.label(RichText::new(m).size(11.5).color(TEXT_DIM));
                            }
                        });
                    }
                    Some(s) => {
                        ui.label(RichText::new(format!("{}   {} frames   video so far {:.1} s", if s.paused { "Paused" } else { "Recording" }, s.frames(), s.video_seconds())).size(13.0));
                        ui.horizontal(|ui| {
                            if ui.button(if s.paused { "Continue" } else { "Pause" }).clicked() {
                                pause = true;
                            }
                            if ui.button(RichText::new("Stop and save…").strong()).clicked() {
                                stop = true;
                            }
                            if ui.button("Throw away").clicked() {
                                discard = true;
                            }
                        });
                    }
                }
            });
        let now = ctx.input(|i| i.time);
        if start {
            self.rec.message = None;
            let first = self.draw.doc.snapshot_live();
            match Session::start(self.rec.cfg, self.draw.doc.w, self.draw.doc.h, first, now) {
                Ok(s) => self.rec.session = Some(s),
                Err(e) => self.rec.message = Some(e),
            }
        }
        if pause {
            if let Some(s) = self.rec.session.as_mut() {
                let p = !s.paused;
                s.pause(p, now);
            }
        }
        if stop {
            self.stop_recording(None);
        }
        if discard {
            if let Some(s) = self.rec.session.take() {
                s.cancel();
            }
            self.rec.message = Some("Thrown away.".into());
        }
        self.rec.open = open;
    }
}
