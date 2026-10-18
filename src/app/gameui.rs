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
    tex: Option<((i32, Option<String>, u64), egui::TextureHandle)>,
    /// How many times the model was made (so the preview picture knows to change).
    made: u64,
}

impl Default for GameUi {
    fn default() -> Self {
        Self { name: String::new(), author: String::new(), height_m: 1.6, puff: 1.0, yaw: 0.0, expr: None, model: None, message: None, tex: None, made: 0 }
    }
}

impl SeviathApp {
    fn game_options(&self) -> Options {
        let g = &self.rigs.game;
        let name = if g.name.trim().is_empty() { self.files.name.clone() } else { g.name.clone() };
        Options { name, author: if g.author.trim().is_empty() { "Unknown".into() } else { g.author.clone() }, height_m: g.height_m, puff: g.puff, ..Default::default() }
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
                self.rigs.game.message = Some(format!("A 3D character with {parts} parts, {bones} bones and {expr} expressions."));
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
        let suffix = if fmt == Format::Vrm1 { "" } else { "0" };
        let picked = rfd::FileDialog::new().set_title("Save the 3D character").set_directory(self.settings.projects_folder()).set_file_name(format!("{stem}{suffix}.vrm")).add_filter("VRM model", &["vrm"]).save_file();
        if let Some(path) = picked {
            self.rigs.game.message = Some(match std::fs::write(&path, &bytes) {
                Ok(()) => format!("Saved {} (VRM {}): {} parts, {} bones, {} expressions, checked.", path.display(), if summary.version == 1 { "1.0" } else { "0.x" }, summary.parts, summary.bones, summary.expressions),
                Err(e) => format!("Could not save the file: {e}"),
            });
        }
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
            ui.horizontal_wrapped(|ui| {
                if widgets::chip(ui, "Set the face", self.rigs.face_edit).on_hover_text("Shows the eyes and the mouth on the picture: drag them into place so the blink, the smile and the mouth move the right parts").clicked() {
                    self.rigs.face_edit = !self.rigs.face_edit;
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
            if ui.button(RichText::new("Make the 3D character").strong()).on_hover_text("Builds it from the parts with a mesh (the layers' order gives the depth), the Parts tags (the skeleton) and the face sliders (the expressions)").clicked() {
                self.game_make();
            }
            let has = self.rigs.game.model.is_some();
            if ui.add_enabled(has, egui::Button::new("Save as VRM 1.0")).on_hover_text("The newer format").clicked() {
                self.game_save(Format::Vrm1);
            }
            if ui.add_enabled(has, egui::Button::new("Save as VRM 0.x")).on_hover_text("The older format many apps still use").clicked() {
                self.game_save(Format::Vrm0);
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
