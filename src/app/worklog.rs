// SPDX-License-Identifier: GPL-3.0-or-later
// Keeps the process log of the open picture (see processlog.rs) and shows it in the Work history window.

use eframe::egui::{self, Align2, RichText};

use super::SeviathApp;
use crate::processlog::{self, MAX_GAP_SECS, duration_text};
use crate::theme::*;
use crate::widgets::section_title;

#[derive(Default)]
pub struct WorkClock {
    pub open: bool,
    /// Time (egui seconds) of the last logged step.
    last_step: Option<f64>,
    /// The reference being shown and for how long so far.
    shown: Option<(String, f64)>,
    last_frame: Option<f64>,
}

fn unix_now() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
}

impl SeviathApp {
    pub(super) fn open_work_history(&mut self) {
        self.flush_reference_time();
        self.work.open = true;
    }

    #[cfg(test)]
    pub fn test_open_work_history(&mut self) {
        self.open_work_history();
    }

    /// A finished step becomes a line in the log. The work time is the time since the last step,
    /// but breaks longer than a minute don't count.
    pub(super) fn log_step(&mut self, label: &str, now: f64) {
        let gap = self.work.last_step.map(|t| (now - t).clamp(0.0, MAX_GAP_SECS)).unwrap_or(5.0);
        self.work.last_step = Some(now);
        let (kind, layer) = processlog::classify(label);
        self.draw.doc.log.add(unix_now(), kind, &layer, label, gap as f32);
    }

    /// Adds up how long a reference picture is on show; it becomes one log line when it stops being shown.
    pub(super) fn reference_clock(&mut self, ctx: &egui::Context) {
        let now = ctx.input(|i| i.time);
        let dt = self.work.last_frame.replace(now).map(|t| (now - t).clamp(0.0, 0.5)).unwrap_or(0.0);
        let current = self.draw.shown_reference();
        match (&mut self.work.shown, current) {
            (Some((name, secs)), Some(c)) if *name == c => *secs += dt,
            (slot, c) => {
                let old = slot.take();
                *slot = c.map(|n| (n, 0.0));
                if let Some((name, secs)) = old {
                    self.log_reference(&name, secs);
                }
            }
        }
    }

    fn log_reference(&mut self, name: &str, secs: f64) {
        if secs >= 2.0 {
            self.draw.doc.log.add(unix_now(), "reference", name, "Reference shown", secs as f32);
        }
    }

    /// Puts the reference time counted so far into the log (before saving or looking at the history).
    pub(super) fn flush_reference_time(&mut self) {
        if let Some((name, secs)) = self.work.shown.take() {
            self.log_reference(&name, secs);
            self.work.shown = Some((name, 0.0));
        }
    }

    pub(super) fn work_history_window(&mut self, ctx: &egui::Context) {
        if !self.work.open {
            return;
        }
        let mut open = true;
        let log = &self.draw.doc.log;
        let chain = log.verify();
        let s = log.summary();
        let mut copy = false;
        egui::Window::new(RichText::new("Work history").strong())
            .open(&mut open)
            .collapsible(false)
            .resizable(false)
            .pivot(Align2::CENTER_CENTER)
            .default_pos(ctx.content_rect().center())
            .show(ctx, |ui| {
                ui.set_width(460.0);
                ui.label(RichText::new("Drawn in SeviathStudio").size(18.0).strong());
                ui.label(RichText::new("What happened while this picture was made. It is kept inside the project file.").size(11.5).color(TEXT_DIM));
                ui.add_space(4.0);
                match chain {
                    Ok(()) if s.entries == 0 => {
                        ui.label("Nothing is written yet. Draw something and it shows up here.");
                    }
                    Ok(()) => {
                        ui.label(RichText::new(format!("History is intact: {} entries, each one tied to the one before it.", s.entries)).color(VENOM));
                    }
                    Err(i) => {
                        ui.label(RichText::new(format!("History was changed after it was written (first problem: entry {}).", i + 1)).color(egui::Color32::from_rgb(235, 110, 90)));
                    }
                }
                ui.label(RichText::new("This protects against careless edits. The app is open source, so a person who rewrites the whole file with their own program can fake it.").size(11.0).color(TEXT_DIM));
                if s.entries > 0 {
                    section_title(ui, "In short");
                    ui.label(format!("Work time: {}    Strokes: {}    Imported pictures: {}", duration_text(s.work_secs), s.strokes, s.imports.len()));
                    if let (Some(a), Some(b)) = (s.first, s.last) {
                        ui.label(RichText::new(format!("From {} to {}", date_text(a), date_text(b))).size(11.5).color(TEXT_DIM));
                    }
                    section_title(ui, "Layers");
                    egui::ScrollArea::vertical().id_salt("wh_layers").max_height(140.0).show(ui, |ui| {
                        for (name, strokes, secs) in &s.layers {
                            ui.label(format!("{name}:  {strokes} strokes, {}", duration_text(*secs)));
                        }
                    });
                    section_title(ui, "Imported pictures");
                    if s.imports.is_empty() {
                        ui.label(RichText::new("None. Everything was drawn here.").color(TEXT_DIM));
                    }
                    for n in s.imports.iter().take(30) {
                        ui.label(n);
                    }
                    section_title(ui, "References that were shown");
                    if s.references.is_empty() {
                        ui.label(RichText::new("None").color(TEXT_DIM));
                    }
                    for (n, t) in s.references.iter().take(30) {
                        ui.label(format!("{n}:  {}", duration_text(*t)));
                    }
                    section_title(ui, "AI tools used");
                    ui.label(RichText::new(if s.ai.is_empty() { "None".to_owned() } else { s.ai.join(", ") }).color(TEXT_DIM));
                    ui.add_space(6.0);
                    if ui.button("Copy this as text").clicked() {
                        copy = true;
                    }
                }
            });
        if copy {
            ctx.copy_text(summary_text(&self.files.name, &s, chain.is_ok()));
        }
        self.work.open = open;
    }
}

fn date_text(t: u64) -> String {
    chrono::DateTime::from_timestamp(t as i64, 0).map(|d| d.with_timezone(&chrono::Local).format("%Y-%m-%d %H:%M").to_string()).unwrap_or_default()
}

/// The history as plain text (to paste next to a picture).
pub fn summary_text(name: &str, s: &processlog::Summary, intact: bool) -> String {
    let mut t = format!("{name}: drawn in SeviathStudio\nWork time {}, {} strokes, {} imported pictures\n", duration_text(s.work_secs), s.strokes, s.imports.len());
    if !s.references.is_empty() {
        t += &format!("References shown: {}\n", s.references.iter().map(|r| format!("{} ({})", r.0, duration_text(r.1))).collect::<Vec<_>>().join(", "));
    }
    t += &format!("AI tools: {}\n", if s.ai.is_empty() { "none".to_owned() } else { s.ai.join(", ") });
    t += if intact { "History check: intact (tamper-evident, not forge-proof)\n" } else { "History check: CHANGED after it was written\n" };
    t
}
