// SPDX-License-Identifier: GPL-3.0-or-later
// "Rig for games" (Rig tab): turns the drawing and its rig into a 3D character with a skeleton and expressions, shows it
// turning round, and saves it as a VRM file (1.0 or 0.x) for games and VRM apps. No tracking is needed to move it:
// the skeleton and the expressions are what a game drives.

use eframe::egui::{self, RichText, TextureOptions, Ui, vec2};

use super::SeviathApp;
use crate::theme::*;
use crate::vrm::export::{Format, Summary, export, validate};
use crate::vrm::model::{Model3d, Options, build_any};
use crate::vrm::preview;
use crate::widgets::{self, section_title};

pub struct GameUi {
    pub name: String,
    pub author: String,
    pub height_m: f32,
    pub puff: f32,
    pub yaw: f32,
    /// The expression shown in the preview.
    pub expr: Option<String>,
    pub model: Option<Model3d>,
    pub message: Option<String>,
    /// Face shapes from a real model, and where they came from.
    pub template: Option<(String, std::sync::Arc<crate::vrm::template::Template>)>,
    /// The command of Python for the AI helpers, which finder to use (0 auto, 1 MediaPipe, 2 Grounding DINO), and the job that runs.
    pub ai_python: String,
    pub ai_backend: usize,
    ai_job: Option<std::sync::mpsc::Receiver<Result<crate::vrm::face::FaceMarks, String>>>,
    tex: Option<((i32, Option<String>, u64), egui::TextureHandle)>,
    /// How many times the model was made (so the preview picture knows to change).
    made: u64,
}

impl Default for GameUi {
    fn default() -> Self {
        Self { name: String::new(), author: String::new(), height_m: 1.6, puff: 1.0, yaw: 0.0, expr: None, model: None, message: None, template: None, ai_python: default_python(), ai_backend: 0, ai_job: None, tex: None, made: 0 }
    }
}

impl SeviathApp {
    fn game_options(&self) -> Options {
        let g = &self.rigs.game;
        let name = if g.name.trim().is_empty() { self.files.name.clone() } else { g.name.clone() };
        Options { name, author: if g.author.trim().is_empty() { "Unknown".into() } else { g.author.clone() }, height_m: g.height_m, puff: g.puff, template: g.template.as_ref().map(|t| t.1.clone()), ..Default::default() }
    }

    /// Make the 3D character from the document and its rig.
    pub fn game_make(&mut self) {
        let opt = self.game_options();
        match build_any(&self.draw.doc, &opt) {
            Some(m) => {
                let expr = m.expressions.len();
                let (parts, bones) = (m.parts.len(), m.bones.len());
                self.rigs.game.model = Some(m);
                self.rigs.game.made += 1;
                let guessed = self.draw.doc.rig.skin.as_ref().is_some_and(|k| k.face.is_none());
                if guessed {
                    // show the guessed eyes and mouth on the picture, ready to be dragged
                    self.rigs.face_edit = true;
                }
                let hint = if guessed { " The eyes and the mouth are only guessed: drag them into place on the picture (Set the face)." } else { "" };
                self.rigs.game.message = Some(format!("A 3D character with {parts} parts, {bones} bones and {expr} expressions.{hint}"));
            }
            None => {
                self.rigs.game.model = None;
                self.rigs.game.message = Some("Nothing to build from: make meshes for the parts in the Rig tab first.".into());
            }
        }
    }

    /// The VRM file bytes for the character (made first when it was not yet), with the check of the file.
    pub fn game_bytes(&mut self, fmt: Format) -> Result<(Vec<u8>, Summary), String> {
        if self.rigs.game.model.is_none() {
            self.game_make();
        }
        let m = self.rigs.game.model.as_ref().ok_or("There is no character to save.")?;
        let bytes = export(m, fmt)?;
        let summary = validate(&bytes)?;
        Ok((bytes, summary))
    }

    fn game_save(&mut self, fmt: Format) {
        let (bytes, summary) = match self.game_bytes(fmt) {
            Ok(b) => b,
            Err(e) => {
                self.rigs.game.message = Some(e);
                return;
            }
        };
        let stem = self.rigs.game.model.as_ref().map(|m| m.name.clone()).unwrap_or_else(|| "character".into());
        let (suffix, ext, kind) = match fmt {
            Format::Vrm1 => ("", "vrm", "VRM model"),
            Format::Vrm0 => ("0", "vrm", "VRM model"),
            Format::Glb => ("", "glb", "glTF binary"),
        };
        let picked = rfd::FileDialog::new().set_title("Save the 3D character").set_directory(self.settings.projects_folder()).set_file_name(format!("{stem}{suffix}.{ext}")).add_filter(kind, &[ext]).save_file();
        if let Some(path) = picked {
            self.rigs.game.message = Some(match std::fs::write(&path, &bytes) {
                Ok(()) => format!("Saved {} ({}): {} parts, {} bones, {} expressions, checked.", path.display(), match summary.version { 1 => "VRM 1.0", 0 => "VRM 0.x", _ => "GLB for games" }, summary.parts, summary.bones, summary.expressions),
                Err(e) => format!("Could not save the file: {e}"),
            });
        }
    }

    /// Find the face with an AI helper (MediaPipe for real faces, Grounding DINO for drawings), in the background.
    fn face_ai_row(&mut self, ui: &mut Ui) {
        // the answer, when the helper is done
        if let Some(rx) = &self.rigs.game.ai_job {
            match rx.try_recv() {
                Ok(Ok(marks)) => {
                    if let Some(sk) = std::sync::Arc::make_mut(&mut self.draw.doc.rig).skin.as_mut() {
                        sk.face = Some(marks);
                    }
                    self.rigs.game.model = None;
                    self.rigs.game.ai_job = None;
                    self.rigs.face_edit = true;
                    self.rigs.game.message = Some("The AI found the eyes and the mouth. Check them on the picture, drag them if they are off, then make the 3D character.".into());
                }
                Ok(Err(e)) => {
                    self.rigs.game.ai_job = None;
                    self.rigs.game.message = Some(format!("The AI could not find the face: {e}"));
                }
                Err(std::sync::mpsc::TryRecvError::Empty) => ui.ctx().request_repaint_after(std::time::Duration::from_millis(300)),
                Err(std::sync::mpsc::TryRecvError::Disconnected) => self.rigs.game.ai_job = None,
            }
        }
        let busy = self.rigs.game.ai_job.is_some();
        ui.horizontal_wrapped(|ui| {
            ui.label(RichText::new("Find the face with").size(12.0).color(TEXT_DIM));
            for (i, n) in ["Auto", "MediaPipe (real faces)", "Grounding DINO (drawings)"].iter().enumerate() {
                if widgets::chip(ui, n, self.rigs.game.ai_backend == i).clicked() {
                    self.rigs.game.ai_backend = i;
                }
            }
        });
        ui.horizontal_wrapped(|ui| {
            ui.label(RichText::new("Python").size(12.0).color(TEXT_DIM));
            ui.add(egui::TextEdit::singleline(&mut self.rigs.game.ai_python).desired_width(150.0)).on_hover_text("The command that starts Python with the AI packages installed (see the README, AI helpers)");
            let label = if busy { "Looking..." } else { "Find the face with AI" };
            if ui.add_enabled(!busy, egui::Button::new(label)).on_hover_text("Runs a helper program on the picture. It needs Python and its packages installed; without them the app keeps its own guess.").clicked() {
                let rig = self.draw.doc.rig.clone();
                let page = rig.skin.as_ref().and_then(|sk| crate::rig::uncut::composite(&self.draw.doc, &sk.layers));
                match (page, crate::vrm::model::head_box(&rig)) {
                    (Some(page), Some(crop)) => {
                        let (python, backend) = (self.rigs.game.ai_python.clone(), ["auto", "mediapipe", "dino"][self.rigs.game.ai_backend.min(2)]);
                        let (tx, rx) = std::sync::mpsc::channel();
                        std::thread::spawn(move || {
                            let _ = tx.send(crate::ai::find_face(&python, &page, crop, backend));
                        });
                        self.rigs.game.ai_job = Some(rx);
                        self.rigs.game.message = Some("The AI is looking for the face. The first run can take a while (it loads the model).".into());
                    }
                    _ => self.rigs.game.message = Some("Rig the whole picture first (Auto-rig), then the face can be found.".into()),
                }
            }
        });
    }

    #[cfg(test)]
    pub fn test_face_click(&mut self, step: u8) {
        self.rigs.face_click = step;
        self.rigs.face_edit = true;
    }

    pub(super) fn rig_game_section(&mut self, ui: &mut Ui, _now: f64) {
        section_title(ui, "Rig for games");
        ui.label(RichText::new("Turns the drawing and its rig into a 3D character with a skeleton and expressions, saved as a VRM file for games and VRM apps. It needs no camera: a game moves the skeleton and the expressions itself.").size(11.5).color(TEXT_DIM));
        {
            let g = &mut self.rigs.game;
            ui.horizontal(|ui| {
                ui.label(RichText::new("Name").size(12.0).color(TEXT_DIM));
                ui.add(egui::TextEdit::singleline(&mut g.name).desired_width(110.0).hint_text("project name"));
                ui.label(RichText::new("Author").size(12.0).color(TEXT_DIM));
                ui.add(egui::TextEdit::singleline(&mut g.author).desired_width(100.0));
            });
            ui.horizontal(|ui| {
                ui.label(RichText::new("Height").size(12.0).color(TEXT_DIM));
                ui.add(egui::DragValue::new(&mut g.height_m).range(0.3..=3.0).speed(0.01).suffix(" m"));
                ui.label(RichText::new("Roundness").size(12.0).color(TEXT_DIM));
                ui.add(egui::Slider::new(&mut g.puff, 0.0..=2.0).show_value(false)).on_hover_text("0 is flat cards, 1 is the usual, 2 is very round");
            });
        }
        if self.draw.doc.rig.skin.is_some() {
            self.face_ai_row(ui);
            ui.horizontal_wrapped(|ui| {
                if widgets::chip(ui, "Set the face", self.rigs.face_edit).on_hover_text("Shows the eyes and the mouth on the picture: drag them into place so the blink, the smile and the mouth move the right parts").clicked() {
                    self.rigs.face_edit = !self.rigs.face_edit;
                }
                if widgets::chip(ui, "Click the eyes and mouth", self.rigs.face_click > 0).on_hover_text("Click the first eye, the other eye and then the middle of the mouth on the picture").clicked() {
                    self.rigs.face_click = if self.rigs.face_click > 0 { 0 } else { 1 };
                    self.rigs.face_edit = true;
                }
                if let Some(mut f) = crate::vrm::model::face_marks(&self.draw.doc.rig) {
                    let before = f;
                    ui.label(RichText::new("Eye").size(12.0).color(TEXT_DIM));
                    ui.add(egui::DragValue::new(&mut f.eye_w).range(4.0..=600.0).speed(0.5));
                    ui.label(RichText::new("Mouth").size(12.0).color(TEXT_DIM));
                    ui.add(egui::DragValue::new(&mut f.mouth_w).range(4.0..=800.0).speed(0.5));
                    if f != before {
                        if let Some(sk) = std::sync::Arc::make_mut(&mut self.draw.doc.rig).skin.as_mut() {
                            sk.face = Some(f);
                        }
                        self.rigs.game.model = None;
                    }
                }
            });
        }
        ui.horizontal_wrapped(|ui| {
            let label = match &self.rigs.game.template {
                Some((n, _)) => format!("Face shapes from {n}"),
                None => "Face shapes from a model...".to_string(),
            };
            if ui.button(label).on_hover_text("Pick a VRM 1.0 model with a VRoid-style face (Fcl_* shapes): its blink, smile, brows and mouth shapes are moved onto this face").clicked() {
                if let Some(path) = rfd::FileDialog::new().set_title("A VRM model with face shapes").set_directory(self.settings.projects_folder()).add_filter("VRM model", &["vrm"]).pick_file() {
                    self.rigs.game.message = Some(match std::fs::read(&path).map_err(|e| e.to_string()).and_then(|b| crate::vrm::template::Template::from_vrm(&b)) {
                        Ok(t) => {
                            let n = t.names().len();
                            let name = path.file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_default();
                            self.rigs.game.template = Some((name.clone(), std::sync::Arc::new(t)));
                            self.rigs.game.model = None;
                            format!("Took {n} face shapes from {name}. Make the 3D character again to use them.")
                        }
                        Err(e) => format!("Could not use that model: {e}"),
                    });
                }
            }
            if self.rigs.game.template.is_some() && ui.small_button("Use the built-in shapes").clicked() {
                self.rigs.game.template = None;
                self.rigs.game.model = None;
            }
        });
        ui.horizontal_wrapped(|ui| {
            if ui.button(RichText::new("Make the 3D character").strong()).on_hover_text("Builds it from the parts with a mesh (the layers' order gives the depth), the Parts tags (the skeleton) and the face sliders (the expressions)").clicked() {
                self.game_make();
            }
            let has = self.rigs.game.model.is_some();
            ui.label(RichText::new("VTubers").size(12.0).color(TEXT_DIM));
            if ui.add_enabled(has, egui::Button::new("Save as VRM 1.0")).on_hover_text("For VTuber apps: the newer format").clicked() {
                self.game_save(Format::Vrm1);
            }
            if ui.add_enabled(has, egui::Button::new("Save as VRM 0.x")).on_hover_text("For VTuber apps: the older format many apps still use").clicked() {
                self.game_save(Format::Vrm0);
            }
            ui.label(RichText::new("Games").size(12.0).color(TEXT_DIM));
            if ui.add_enabled(has, egui::Button::new("Save as GLB")).on_hover_text("For game engines (Unity, Unreal, Godot, Bevy): the skeleton, the skin and the blend shapes in one plain glTF file").clicked() {
                self.game_save(Format::Glb);
            }
        });
        if let Some(m) = &self.rigs.game.message {
            ui.label(RichText::new(m).size(11.5).color(TEXT_DIM));
        }
        let Some(model) = &self.rigs.game.model else { return };
        // the preview: turn it round, and try the expressions
        ui.horizontal(|ui| {
            ui.label(RichText::new("Turn").size(12.0).color(TEXT_DIM));
            ui.add(egui::Slider::new(&mut self.rigs.game.yaw, -90.0..=90.0).suffix(" deg"));
        });
        let names: Vec<String> = model.expressions.iter().map(|e| e.name.clone()).collect();
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing = vec2(4.0, 4.0);
            if widgets::chip(ui, "Neutral", self.rigs.game.expr.is_none()).clicked() {
                self.rigs.game.expr = None;
            }
            for n in &names {
                if widgets::chip(ui, n, self.rigs.game.expr.as_deref() == Some(n.as_str())).clicked() {
                    self.rigs.game.expr = Some(n.clone());
                }
            }
        });
        let key = (self.rigs.game.yaw.round() as i32, self.rigs.game.expr.clone(), self.rigs.game.made);
        if self.rigs.game.tex.as_ref().map(|t| &t.0) != Some(&key) {
            if let Some(m) = &self.rigs.game.model {
                let active: Vec<(&str, f32)> = self.rigs.game.expr.as_deref().map(|e| vec![(e, 1.0)]).unwrap_or_default();
                let img = preview::render(m, key.0 as f32, 300, 480, &active);
                let color = egui::ColorImage::from_rgba_unmultiplied([img.width() as usize, img.height() as usize], img.as_raw());
                let tex = ui.ctx().load_texture("game_preview", color, TextureOptions::LINEAR);
                self.rigs.game.tex = Some((key, tex));
            }
        }
        if let Some((_, t)) = &self.rigs.game.tex {
            ui.image((t.id(), vec2(300.0, 480.0)));
        }
    }

    #[cfg(test)]
    pub fn test_game(&mut self) -> &mut GameUi {
        &mut self.rigs.game
    }
}

/// The Python that has the AI packages: the one in the SeviathAI folder of the user when it is there, else plain `python`.
fn default_python() -> String {
    let venv = dirs::home_dir().map(|h| h.join("SeviathAI").join("venv").join("Scripts").join("python.exe"));
    match venv {
        Some(p) if p.exists() => p.to_string_lossy().to_string(),
        _ => "python".to_string(),
    }
}
