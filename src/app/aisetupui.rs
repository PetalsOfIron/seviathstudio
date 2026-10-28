// SPDX-License-Identifier: GPL-3.0-or-later
// The AI files of the 3D tab are not part of the app. When they are missing this says so and offers a button that downloads them (Python packages,
// PyTorch and TripoSR). The pictures and the mesh are made with them, on this computer.

use crate::widgets::HelpExt;
use eframe::egui::{self, RichText, Ui};

use super::SeviathApp;
use crate::theme::*;

/// Downloading the AI files: the job, what it is doing now, and whether the licences were accepted.
#[derive(Default)]
pub struct AiSetup {
    job: Option<std::sync::mpsc::Receiver<Result<(), String>>>,
    step: Option<std::sync::mpsc::Receiver<String>>,
    text: String,
    pub agree: bool,
}

/// The Python that has the AI packages: the one in the SeviathAI folder of the user when it is there, else plain `python`.
pub(super) fn default_python() -> String {
    let p = crate::aisetup::venv_python(&crate::aisetup::root());
    if p.exists() { p.to_string_lossy().to_string() } else { "python".to_string() }
}

impl SeviathApp {
    pub(super) fn ai_setup_ui(&mut self, ui: &mut Ui) {
        // what the job says
        if let Some(rx) = &self.m3.setup.step {
            while let Ok(s) = rx.try_recv() {
                self.m3.setup.text = s;
            }
        }
        if let Some(rx) = &self.m3.setup.job {
            match rx.try_recv() {
                Ok(r) => {
                    self.m3.setup.job = None;
                    self.m3.setup.step = None;
                    self.m3.message = Some(match r {
                        Ok(()) => "The AI files are installed. The models themselves (a few GB) are fetched the first time the mesh is made.".to_string(),
                        Err(e) => format!("The download did not finish: {e}"),
                    });
                }
                Err(std::sync::mpsc::TryRecvError::Empty) => ui.ctx().request_repaint_after(std::time::Duration::from_millis(700)),
                Err(std::sync::mpsc::TryRecvError::Disconnected) => self.m3.setup.job = None,
            }
        }
        let status = crate::aisetup::status();
        let running = self.m3.setup.job.is_some();
        if status.ready() && !running {
            return;
        }
        ui.group(|ui| {
            ui.label(RichText::new("The AI files are missing").strong());
            if running {
                ui.label(RichText::new(format!("{}...", self.m3.setup.text)).size(12.0).color(TEXT));
                let last = crate::aisetup::tail(&crate::aisetup::log_path(), 1);
                if !last.is_empty() {
                    ui.label(RichText::new(last.chars().take(110).collect::<String>()).size(10.5).color(TEXT_DIM));
                }
                ui.label(RichText::new("This takes a while. The full log is in the folder of the AI files (setup.log).").size(11.0).color(TEXT_DIM));
                return;
            }
            ui.label(RichText::new(format!("Missing: {}. They are not part of the app: they are large and have their own licences, so they are downloaded to {}.", status.missing().join(", "), crate::aisetup::root().display())).size(11.5).color(TEXT_DIM));
            ui.checkbox(&mut self.m3.setup.agree, "I accept the licences: PyTorch (BSD), Transformers (Apache 2.0), TripoSR (MIT).").help("Tick this to say you accept the licences of the AI files. The download needs it.");
            ui.label(RichText::new("About 3 GB now (Python packages and PyTorch with CUDA, so an NVIDIA graphics card is needed), and a few GB more the first time the AI models run. It needs Python 3.10, 3.11 or 3.12 and an internet connection.").size(11.0).color(TEXT_DIM));
            if ui.add_enabled(self.m3.setup.agree, egui::Button::new(RichText::new("Download the AI files").strong())).clicked() {
                let (tx, rx) = std::sync::mpsc::channel::<String>();
                let (dtx, drx) = std::sync::mpsc::channel::<Result<(), String>>();
                std::thread::spawn(move || {
                    let _ = dtx.send(crate::aisetup::install(&tx));
                });
                self.m3.setup.job = Some(drx);
                self.m3.setup.step = Some(rx);
                self.m3.setup.text = "Starting".into();
            }
        });
    }
}
