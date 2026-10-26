// SPDX-License-Identifier: GPL-3.0-or-later
// File, Downloads: the list of everything the program uses but does not contain (the AI models, the tracking models, the Unreal plugin,
// OBS Studio), with what is already on this computer and a button for each that is not. One download at a time; what is being
// done is shown, and what is done says where it is. See downloads.rs.

use crate::widgets::HelpExt;
use std::sync::mpsc::{Receiver, channel};

use eframe::egui::{self, Align2, RichText};

use super::SeviathApp;
use crate::downloads::{self, Kind};
use crate::theme::*;

#[derive(Default)]
pub struct DownloadsUi {
    pub open: bool,
    job: Option<(Kind, Receiver<Result<String, String>>)>,
    step: Option<Receiver<String>>,
    text: String,
    message: Option<String>,
}

impl SeviathApp {
    pub(super) fn open_downloads(&mut self) {
        self.downloads.open = true;
    }

    fn downloads_poll(&mut self, ctx: &egui::Context) {
        if let Some(rx) = &self.downloads.step {
            while let Ok(s) = rx.try_recv() {
                self.downloads.text = s;
            }
        }
        if let Some((_, rx)) = &self.downloads.job {
            ctx.request_repaint_after(std::time::Duration::from_millis(300));
            if let Ok(r) = rx.try_recv() {
                self.downloads.job = None;
                self.downloads.step = None;
                self.downloads.message = Some(match r {
                    Ok(m) => m,
                    Err(e) => e,
                });
            }
        }
    }

    pub(super) fn downloads_window(&mut self, ctx: &egui::Context) {
        self.downloads_poll(ctx);
        if !self.downloads.open {
            return;
        }
        let mut open = true;
        egui::Window::new(RichText::new("Downloads").strong()).open(&mut open).collapsible(false).default_width(560.0).pivot(Align2::CENTER_CENTER).default_pos(ctx.content_rect().center()).show(ctx, |ui| {
            ui.set_width(530.0);
            ui.label(RichText::new("Things the program uses but does not contain. They come from the pages of their makers, one at a time, and nothing is run after a download.").size(11.5).color(TEXT_DIM));
            ui.add_space(6.0);
            let busy = self.downloads.job.is_some();
            let mut start: Option<Kind> = None;
            let mut show: Option<std::path::PathBuf> = None;
            for it in downloads::items() {
                let have = downloads::installed(it.kind);
                ui.group(|ui| {
                    ui.horizontal(|ui| {
                        ui.label(RichText::new(it.name).strong());
                        ui.label(RichText::new(format!("{}, {}", it.size, it.licence)).size(11.0).color(TEXT_DIM));
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            let this = self.downloads.job.as_ref().is_some_and(|j| j.0 == it.kind);
                            if this {
                                ui.label(RichText::new("Downloading...").color(GOLD));
                            } else if have {
                                if ui.small_button("Show").help("Show it.").clicked() {
                                    show = downloads::place(it.kind);
                                }
                                ui.label(RichText::new("On this computer").size(11.5).color(TEXT_DIM));
                            } else if ui.add_enabled(!busy, egui::Button::new("Download")).clicked() {
                                start = Some(it.kind);
                            }
                        });
                    });
                    ui.label(RichText::new(it.what).size(11.5).color(TEXT_DIM));
                    if let Some(w) = it.warning {
                        ui.label(RichText::new(w).size(11.5).color(GOLD));
                    }
                });
            }
            if busy {
                ui.label(RichText::new(format!("{}...", if self.downloads.text.is_empty() { "Starting" } else { &self.downloads.text })).size(12.0));
            }
            if let Some(m) = &self.downloads.message {
                ui.label(RichText::new(m).size(11.5).color(GOLD));
            }
            if let Some(p) = show {
                // opens the folder (or the folder of the file) in the file browser
                let dir = if p.is_file() { p.parent().map(|d| d.to_path_buf()).unwrap_or(p.clone()) } else { p.clone() };
                ui.ctx().open_url(egui::OpenUrl::new_tab(format!("file:///{}", dir.display().to_string().replace('\\', "/"))));
            }
            if let Some(kind) = start {
                let (tx, rx) = channel::<String>();
                let (dtx, drx) = channel::<Result<String, String>>();
                std::thread::spawn(move || {
                    let _ = dtx.send(downloads::download(kind, &tx));
                });
                self.downloads.job = Some((kind, drx));
                self.downloads.step = Some(rx);
                self.downloads.text = "Starting".into();
                self.downloads.message = None;
            }
        });
        self.downloads.open = open;
    }
}
