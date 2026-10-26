// SPDX-License-Identifier: GPL-3.0-or-later
// "Rig for games" (Rig tab): turns the drawing and its rig into a 3D character with a skeleton and expressions, shows it
// turning round, and saves it as a VRM file (1.0 or 0.x) for games and VRM apps. No tracking is needed to move it:
// the skeleton and the expressions are what a game drives.

use crate::widgets::HelpExt;
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
    /// The preview follows the face that the Stream tab tracks (the head turn and the expressions).
    pub follow: bool,
    /// The expression shown in the preview.
    pub expr: Option<String>,
    pub model: Option<Model3d>,
    /// Which AI makes the 3D shape, and whether the setup also fetches Hunyuan3D (not licensed in the EU, the UK and South Korea).
    pub shape_model: crate::ai::ShapeModel,
    pub setup_hunyuan: bool,
    pub message: Option<String>,
    /// What the SVRM check found in the last file that was made (things a game maker should know).
    pub svrm_notes: Vec<String>,
    /// Face shapes from a real model, and where they came from.
    pub template: Option<(String, std::sync::Arc<crate::vrm::template::Template>)>,
    /// The command of Python for the AI helpers, which finder to use (0 auto, 1 MediaPipe, 2 Grounding DINO), and the job that runs.
    pub ai_python: String,
    /// Guide lines on the preview, and a change of the texture parts waiting to be built (when the mouse is let go).
    pub guides: bool,
    pub(super) tex_dirty: bool,
    paint_rev: u64,
    /// Build the character with a plain texture, to paint it by hand with the brushes of the drawing app.
    pub mesh_only: bool,
    /// Downloading the AI files: the job, what it is doing now, and whether the licences were accepted.
    setup_job: Option<std::sync::mpsc::Receiver<Result<(), String>>>,
    setup_step: Option<std::sync::mpsc::Receiver<String>>,
    setup_text: String,
    pub setup_agree: bool,
    /// Painting straight on the 3D character: on or off, the brush, the paint kept for the project, the last place of the stroke, how many
    /// times the picture of the preview must be made again, and the paint before each stroke (to undo).
    pub paint3d: bool,
    pub brush: crate::vrm::paint3d::Brush,
    overlay: Option<image::RgbaImage>,
    last_pt: Option<[f32; 2]>,
    paint_gen: u64,
    paint_undo: Vec<String>,
    paint_rebuild: bool,
    pub ai_backend: usize,
    ai_job: Option<std::sync::mpsc::Receiver<Result<crate::vrm::face::FaceMarks, String>>>,
    /// The shape an AI made of the picture (the character is built on it), and the job that makes it.
    pub shape: Option<std::sync::Arc<crate::vrm::shape::Shape>>,
    shape_job: Option<std::sync::mpsc::Receiver<Result<Vec<u8>, String>>>,
    tex: Option<((i32, Option<String>, u64, bool, u64, bool), egui::TextureHandle)>,
    /// How many times the model was made (so the preview picture knows to change).
    made: u64,
}

impl GameUi {
    /// How many times the character was made (so a picture of it knows to change).
    pub(super) fn made_count(&self) -> u64 {
        self.made
    }
}

impl Default for GameUi {
    fn default() -> Self {
        Self { name: String::new(), author: String::new(), height_m: 1.6, puff: 1.0, yaw: 0.0, follow: false, expr: None, model: None, shape_model: Default::default(), setup_hunyuan: false, message: None, svrm_notes: Vec::new(), template: None, ai_python: default_python(), guides: false, tex_dirty: false, paint_rev: 0, mesh_only: false, setup_job: None, setup_step: None, setup_text: String::new(), setup_agree: false, paint3d: false, brush: Default::default(), overlay: None, last_pt: None, paint_gen: 0, paint_undo: Vec::new(), paint_rebuild: false, ai_backend: 0, ai_job: None, shape: None, shape_job: None, tex: None, made: 0 }
    }
}

impl SeviathApp {
    fn game_options(&self) -> Options {
        let g = &self.rigs.game;
        let name = if g.name.trim().is_empty() { self.files.name.clone() } else { g.name.clone() };
        Options { name, author: if g.author.trim().is_empty() { "Unknown".into() } else { g.author.clone() }, height_m: g.height_m, puff: g.puff, template: g.template.as_ref().map(|t| t.1.clone()), shape: g.shape.clone(), mesh_only: g.mesh_only, ..Default::default() }
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
        let rig = self.draw.doc.rig.clone();
        let layers: std::collections::HashMap<u64, String> = self.draw.doc.layers.iter().map(|l| (l.id, l.name.clone())).collect();
        let svrm = crate::vrm::svrm::describe(&rig, &|id| layers.get(&id).cloned());
        let notes = crate::vrm::svrm::report(&svrm);
        let m = self.rigs.game.model.as_ref().ok_or("There is no character to save.")?;
        let bytes = crate::vrm::export::export_with(m, fmt, Some(&svrm))?;
        let summary = validate(&bytes)?;
        self.rigs.game.svrm_notes = notes;
        Ok((bytes, summary))
    }

    pub(super) fn game_save(&mut self, fmt: Format) {
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

    /// The parts of the picture on an AI shape, each moved by hand (the head, the face, each eye, the nose, the mouth), how soft their
    /// edges are, and the settings of the dark inside of the mouth. The preview can show guide lines at the features.
    fn texture_parts_ui(&mut self, ui: &mut Ui) {
        if self.rigs.game.shape.is_none() {
            return;
        }
        let Some(mut t) = self.draw.doc.rig.skin.as_ref().map(|s| s.tex.clone()) else { return };
        let before = t.clone();
        section_title(ui, "Texture parts");
        ui.label(RichText::new("Each part of the picture can be moved on the 3D face, in pixels of the drawing (x right, y down). The place it leaves is filled with skin. Guide lines: blue for where the texture has the eyes, nose and mouth, pink for the nose and the chin of the shape itself.").size(11.5).color(TEXT_DIM));
        ui.horizontal_wrapped(|ui| {
            if widgets::chip(ui, "Show guide lines", self.rigs.game.guides).help("Lines on the preview at the eyes, the nose and the mouth of the texture (blue) and the nose and the chin of the shape (pink)").clicked() {
                self.rigs.game.guides = !self.rigs.game.guides;
            }
            if ui.small_button("Reset all").help("Put every setting back to the default.").clicked() {
                t = crate::vrm::texedit::TexEdit::default();
            }
        });
        let mut row = |ui: &mut Ui, label: &str, v: &mut [f32; 2]| {
            ui.horizontal(|ui| {
                ui.label(RichText::new(label).size(12.0).color(TEXT_DIM));
                ui.add(egui::DragValue::new(&mut v[0]).range(-300.0..=300.0).speed(0.25).prefix("x ").suffix(" px"));
                ui.add(egui::DragValue::new(&mut v[1]).range(-300.0..=300.0).speed(0.25).prefix("y ").suffix(" px"));
                if ui.small_button("0").help("Put it back").clicked() {
                    *v = [0.0, 0.0];
                }
            });
        };
        let picking = self.rigs.pick_part;
        let mut start_pick: Option<Option<crate::vrm::texedit::Part>> = None;
        for part in crate::vrm::texedit::Part::ALL {
            row(ui, part.label(), t.offset_mut(part));
            // pick the right piece of the drawing for this part again
            ui.horizontal(|ui| {
                ui.add_space(14.0);
                let on = picking == Some(part);
                if widgets::chip(ui, if on { "Drag a circle round it on the picture..." } else { "Pick it again" }, on).help("The app found this part in the wrong place? Pick the right piece of the drawing: it is cut out and put where the part belongs, and the old one is painted over with skin").clicked() {
                    start_pick = Some(if on { None } else { Some(part) });
                }
                if t.pick_of(part).is_some() && ui.small_button("Forget the pick").help("Let go of the picked picture.").clicked() {
                    t.picks.retain(|k| k.part != part);
                }
            });
        }
        if let Some(p) = start_pick {
            self.rigs.pick_part = p;
            self.rigs.pick_from = None;
        }
        let mut paint_now = false;
        let mut see_again = false;
        ui.horizontal_wrapped(|ui| {
            if widgets::chip(ui, "Mesh only: plain texture", self.rigs.game.mesh_only).help("Builds the character on the AI shape with one plain colour instead of your drawing, so the whole texture is painted by hand with the brushes of the drawing app (the drawing shows faintly on a locked layer to paint from)").clicked() {
                self.rigs.game.mesh_only = !self.rigs.game.mesh_only;
                self.rigs.game.tex_dirty = true;
            }
            if ui.button("Paint the texture").help("Opens the Draw tab with the picture of the front of the character as it is now. Paint on the layer Texture paint with any brush of the drawing app (blend, smudge, the eyedropper picks colours from the texture), then come back: the paint is laid over the texture").clicked() {
                paint_now = true;
            }
            if ui.small_button("Show the moved parts again").help("Renews the locked layer under your paint with the parts as they are placed now").clicked() {
                see_again = true;
            }
        });
        ui.checkbox(&mut t.replace_old, "Paint the old part over with skin").help("When a part is picked again, the place where the app thought it was is covered");
        ui.horizontal(|ui| {
            ui.label(RichText::new("Soft edges").size(12.0).color(TEXT_DIM));
            ui.add(egui::Slider::new(&mut t.feather, 0.05..=0.7).show_value(false)).help("How softly a moved part blends into the skin around it");
        });
        ui.label(RichText::new("The dark inside of the mouth").size(12.0).color(TEXT_DIM));
        ui.horizontal(|ui| {
            ui.label(RichText::new("Open").size(12.0).color(TEXT_DIM));
            ui.add(egui::DragValue::new(&mut t.mouth_open).range(0.0..=3.0).speed(0.01));
            ui.label(RichText::new("Wide").size(12.0).color(TEXT_DIM));
            ui.add(egui::DragValue::new(&mut t.mouth_width).range(0.1..=3.0).speed(0.01));
            ui.label(RichText::new("Colour").size(12.0).color(TEXT_DIM));
            ui.color_edit_button_srgb(&mut t.mouth_color);
        });
        row(ui, "Inside moved", &mut t.mouth_move);
        if t != before {
            if let Some(sk) = std::sync::Arc::make_mut(&mut self.draw.doc.rig).skin.as_mut() {
                sk.tex = t;
            }
            self.rigs.game.tex_dirty = true;
        }
        if paint_now || see_again {
            let opt = self.game_options();
            let before = self.draw.doc.snapshot();
            let guides: Vec<(String, [f32; 3])> = self.rigs.game.model.as_ref().map(|m| m.landmarks.clone()).unwrap_or_default();
            if let Some(paint_id) = crate::vrm::shape::make_paint_layers(&mut self.draw.doc, &opt, Some(&guides)) {
                let after = self.draw.doc.snapshot();
                self.history.push("Layers to paint the texture".to_owned(), crate::app::Tab::Rig, before, after, 0.0);
                self.rigs.game.message = Some("The Draw tab shows the texture on a locked layer, with lines for the places of the features: blue the eyes, green the eyebrows, orange the nose, red the mouth, pink the nose and chin of the 3D shape. Paint on the layer Texture paint; come back to the Rig tab to see it on the character.".into());
                if paint_now {
                    self.draw.focus_layer(paint_id);
                    self.tab = crate::app::Tab::Draw;
                }
            } else {
                self.rigs.game.message = Some("The texture cannot be shown yet: make the 3D character first.".into());
            }
        }
        // painting on the layer of the drawing app changes the character: remake it when it did
        if let Some(l) = self.draw.doc.layers.iter().find(|l| l.id == self.draw.doc.rig.skin.as_ref().map(|s| s.tex.paint_layer).unwrap_or(0)) {
            if self.rigs.game.paint_rev != l.rev {
                self.rigs.game.paint_rev = l.rev;
                self.rigs.game.tex_dirty = true;
            }
        }
        // the character is made again when the mouse is let go (it takes a moment)
        if self.rigs.game.tex_dirty && !ui.input(|i| i.pointer.any_down()) {
            self.rigs.game.tex_dirty = false;
            self.game_make();
        }
    }

    /// The AI tools are not part of the app. When they are missing this says so and offers a button that downloads them.
    pub(super) fn ai_setup_ui(&mut self, ui: &mut Ui) {
        // what the job says
        if let Some(rx) = &self.rigs.game.setup_step {
            while let Ok(s) = rx.try_recv() {
                self.rigs.game.setup_text = s;
            }
        }
        if let Some(rx) = &self.rigs.game.setup_job {
            match rx.try_recv() {
                Ok(r) => {
                    self.rigs.game.setup_job = None;
                    self.rigs.game.setup_step = None;
                    self.rigs.game.message = Some(match r {
                        Ok(()) => "The AI files are installed. The models themselves (a few GB) are fetched the first time the shape or the face is made.".to_string(),
                        Err(e) => format!("The download did not finish: {e}"),
                    });
                }
                Err(std::sync::mpsc::TryRecvError::Empty) => ui.ctx().request_repaint_after(std::time::Duration::from_millis(700)),
                Err(std::sync::mpsc::TryRecvError::Disconnected) => self.rigs.game.setup_job = None,
            }
        }
        let status = crate::aisetup::status();
        let running = self.rigs.game.setup_job.is_some();
        if status.ready() && !running {
            return;
        }
        ui.group(|ui| {
            ui.label(RichText::new("The AI files are missing").strong());
            if running {
                ui.label(RichText::new(format!("{}...", self.rigs.game.setup_text)).size(12.0).color(TEXT));
                let last = crate::aisetup::tail(&crate::aisetup::log_path(), 1);
                if !last.is_empty() {
                    ui.label(RichText::new(last.chars().take(110).collect::<String>()).size(10.5).color(TEXT_DIM));
                }
                ui.label(RichText::new("This takes a while. The full log is in the folder of the AI files (setup.log).").size(11.0).color(TEXT_DIM));
                return;
            }
            ui.label(RichText::new(format!("Missing: {}. They are not part of the app: they are large and have their own licences, so they are downloaded to {}.", status.missing().join(", "), crate::aisetup::root().display())).size(11.5).color(TEXT_DIM));
            ui.checkbox(&mut self.rigs.game.setup_agree, "I accept the licences: PyTorch (BSD), MediaPipe and Transformers (Apache 2.0), TripoSR (MIT).").help("Tick this to say you accept the licences of the AI files. The download needs it.");
            ui.checkbox(&mut self.rigs.game.setup_hunyuan, "Also Hunyuan3D 2 (a finer shape, but its Tencent licence does not apply in the European Union, the United Kingdom and South Korea; accept it only where it is allowed)").help("Also download Hunyuan3D 2, which makes a finer 3D shape. Its licence does not apply in the EU, the UK and South Korea.");
            ui.label(RichText::new("About 3 GB now (Python packages and PyTorch with CUDA, so an NVIDIA graphics card is needed), and a few GB more the first time the AI models run. It needs Python 3.10, 3.11 or 3.12 and an internet connection.").size(11.0).color(TEXT_DIM));
            if ui.add_enabled(self.rigs.game.setup_agree, egui::Button::new(RichText::new("Download the AI files").strong())).clicked() {
                let (tx, rx) = std::sync::mpsc::channel::<String>();
                let (dtx, drx) = std::sync::mpsc::channel::<Result<(), String>>();
                let with_hunyuan = self.rigs.game.setup_hunyuan;
                std::thread::spawn(move || {
                    let _ = dtx.send(crate::aisetup::install(&tx, with_hunyuan));
                });
                self.rigs.game.setup_job = Some(drx);
                self.rigs.game.setup_step = Some(rx);
                self.rigs.game.setup_text = "Starting".into();
            }
        });
    }

    /// Start making the 3D shape of the picture with the chosen AI, in the background.
    pub(super) fn shape_start(&mut self) {
        let model = self.rigs.game.shape_model;
        let page = self.draw.doc.rig.skin.as_ref().and_then(|sk| crate::rig::uncut::composite(&self.draw.doc, &sk.layers)).map(|p| crate::rig::uncut::cut_out_background(&p));
        match page {
            Some(page) => {
                let python = self.rigs.game.ai_python.clone();
                let (tx, rx) = std::sync::mpsc::channel();
                std::thread::spawn(move || {
                    let _ = tx.send(crate::ai::make_shape(&python, &page, model, true));
                });
                self.rigs.game.shape_job = Some(rx);
                self.rigs.game.message = Some("The AI is making the 3D shape. The first run downloads the model; after that it takes a minute or two.".into());
            }
            None => self.rigs.game.message = Some("Rig the whole picture first (Auto-rig), then the shape can be made.".into()),
        }
    }

    /// Make the 3D shape of the character with an AI helper (Hunyuan3D 2 mini), in the background. The character is then built on
    /// that shape (the drawing is laid on it) instead of on a card.
    /// Take the answer of the shape job when it is there.
    pub(super) fn shape_poll(&mut self, ctx: &egui::Context) {
        if let Some(rx) = &self.rigs.game.shape_job {
            match rx.try_recv() {
                Ok(Ok(bytes)) => {
                    self.rigs.game.shape_job = None;
                    self.rigs.game.message = Some(match crate::vrm::shape::Shape::from_glb(&bytes) {
                        Ok(sh) => {
                            let (v, t) = (sh.verts.len(), sh.tris.len());
                            self.rigs.game.shape = Some(std::sync::Arc::new(sh));
                            self.rigs.game.model = None;
                            format!("The AI made a 3D shape ({v} points, {t} triangles). Make the 3D character to put your drawing on it.")
                        }
                        Err(e) => format!("The AI's shape could not be read: {e}"),
                    });
                }
                Ok(Err(e)) => {
                    self.rigs.game.shape_job = None;
                    self.rigs.game.message = Some(format!("The AI could not make the shape: {e}"));
                }
                Err(std::sync::mpsc::TryRecvError::Empty) => ctx.request_repaint_after(std::time::Duration::from_millis(500)),
                Err(std::sync::mpsc::TryRecvError::Disconnected) => self.rigs.game.shape_job = None,
            }
        }
    }

    pub(super) fn shape_busy(&self) -> bool {
        self.rigs.game.shape_job.is_some()
    }

    fn shape_ai_row(&mut self, ui: &mut Ui) {
        self.shape_poll(ui.ctx());
        let busy = self.rigs.game.shape_job.is_some();
        ui.horizontal_wrapped(|ui| {
            ui.label(RichText::new("Shape AI").size(12.0).color(TEXT_DIM));
            if widgets::chip(ui, "TripoSR", self.rigs.game.shape_model == crate::ai::ShapeModel::TripoSr).help("MIT licence, usable everywhere, about 6 GB of video memory, about 20 seconds. A rougher shape.").clicked() {
                self.rigs.game.shape_model = crate::ai::ShapeModel::TripoSr;
            }
            if widgets::chip(ui, "Hunyuan3D", self.rigs.game.shape_model == crate::ai::ShapeModel::Hunyuan).help("A finer shape, but Tencent's licence does not apply in the European Union, the United Kingdom and South Korea.").clicked() {
                self.rigs.game.shape_model = crate::ai::ShapeModel::Hunyuan;
            }
        });
        ui.horizontal_wrapped(|ui| {
            let label = if busy { "Making the shape..." } else { "Make the 3D shape with AI" };
            let model = self.rigs.game.shape_model;
            if ui.add_enabled(!busy && crate::aisetup::status().shape_ready(model), egui::Button::new(label)).help("The chosen AI makes a 3D shape of the whole character from the picture (about a minute on the graphics card, one job at a time). Your drawing is then laid on it. It needs the AI files (see the card above when they are missing).").clicked() {
                self.shape_start();
            }
            if self.rigs.game.shape.is_some() && ui.small_button("Use the card instead").help("Forget the AI shape: the character is a round card made from the drawing").clicked() {
                self.rigs.game.shape = None;
                self.rigs.game.model = None;
            }
        });
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
                if widgets::chip(ui, n, self.rigs.game.ai_backend == i).help("Pick which AI finds the face.").clicked() {
                    self.rigs.game.ai_backend = i;
                }
            }
        });
        ui.horizontal_wrapped(|ui| {
            ui.label(RichText::new("Python").size(12.0).color(TEXT_DIM));
            ui.add(egui::TextEdit::singleline(&mut self.rigs.game.ai_python).desired_width(150.0)).help("The command that starts Python with the AI packages installed (see the README, AI helpers)");
            let label = if busy { "Looking..." } else { "Find the face with AI" };
            if ui.add_enabled(!busy && crate::aisetup::status().ready(), egui::Button::new(label)).help("Runs a helper program on the picture. It needs Python and its packages installed; without them the app keeps its own guess.").clicked() {
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
                ui.add(egui::Slider::new(&mut g.puff, 0.0..=2.0).show_value(false)).help("0 is flat cards, 1 is the usual, 2 is very round");
            });
        }
        if self.draw.doc.rig.skin.is_some() {
            self.ai_setup_ui(ui);
            self.shape_ai_row(ui);
            self.face_ai_row(ui);
            self.texture_parts_ui(ui);
            self.hair_ui(ui);
            ui.horizontal_wrapped(|ui| {
                if widgets::chip(ui, "Set the face", self.rigs.face_edit).help("Shows the eyes and the mouth on the picture: drag them into place so the blink, the smile and the mouth move the right parts").clicked() {
                    self.rigs.face_edit = !self.rigs.face_edit;
                }
                if widgets::chip(ui, "Click the eyes and mouth", self.rigs.face_click > 0).help("Click the first eye, the other eye and then the middle of the mouth on the picture").clicked() {
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
            if ui.button(label).help("Pick a VRM 1.0 model with a VRoid-style face (Fcl_* shapes): its blink, smile, brows and mouth shapes are moved onto this face").clicked() {
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
            if ui.button(RichText::new("Make the 3D character").strong()).help("Builds it from the parts with a mesh (the layers' order gives the depth), the Parts tags (the skeleton) and the face sliders (the expressions)").clicked() {
                self.game_make();
            }
            let has = self.rigs.game.model.is_some();
            ui.label(RichText::new("VTubers").size(12.0).color(TEXT_DIM));
            if ui.add_enabled(has, egui::Button::new("Save as VRM 1.0")).help("For VTuber apps: the newer format").clicked() {
                self.game_save(Format::Vrm1);
            }
            if ui.add_enabled(has, egui::Button::new("Save as VRM 0.x")).help("For VTuber apps: the older format many apps still use").clicked() {
                self.game_save(Format::Vrm0);
            }
            ui.label(RichText::new("Games").size(12.0).color(TEXT_DIM));
            if ui.add_enabled(has, egui::Button::new("Save as GLB")).help("For game engines (Unity, Unreal, Godot, Bevy): the skeleton, the skin and the blend shapes in one plain glTF file").clicked() {
                self.game_save(Format::Glb);
            }
        });
        // the engines: the free plugins that read the file we save
        ui.horizontal_wrapped(|ui| {
            ui.label(RichText::new("Unreal Engine").size(12.0).color(TEXT_DIM));
            if ui.button("Get VRM4U (free plugin)").help("VRM4U (MIT licence) puts a VRM file into Unreal Engine 4.20 to 5.4 with its bones, face shapes, spring bones and a humanoid rig: save as VRM 1.0 here, turn the plugin on in your Unreal project and drag the .vrm file into the Content Browser. Opens github.com/ruyo/VRM4U").clicked() {
                ui.ctx().open_url(egui::OpenUrl::new_tab("https://github.com/ruyo/VRM4U"));
            }
        });
        // the bones as sliders: to pose the 3D character by hand, and to key the poses in the Animate tab
        ui.horizontal_wrapped(|ui| {
            let has = self.rigs.game.model.is_some();
            if ui.add_enabled(has, egui::Button::new("Sliders for the bones")).help("One slider for each main bone (arms, legs, spine, head): move them to pose the 3D character, and key them in the Animate tab (its 3D view button shows the pose)").clicked() {
                self.make_bone_sliders(_now);
            }
            if let Some(n) = self.bone_slider_count().filter(|n| *n > 0) {
                ui.label(RichText::new(format!("{n} bone sliders (3D bones, on the left)")).size(11.0).color(TEXT_DIM));
            }
        });
        if let Some(m) = &self.rigs.game.message {
            ui.label(RichText::new(m).size(11.5).color(TEXT_DIM));
        }
        for n in &self.rigs.game.svrm_notes {
            ui.label(RichText::new(format!("For game makers: {n}")).size(11.0).color(GOLD));
        }
        let Some(model) = &self.rigs.game.model else { return };
        // the preview: turn it round, and try the expressions
        ui.horizontal(|ui| {
            ui.label(RichText::new("Turn").size(12.0).color(TEXT_DIM));
            ui.add(egui::Slider::new(&mut self.rigs.game.yaw, -90.0..=90.0).suffix(" deg"));
        });
        ui.checkbox(&mut self.rigs.game.follow, "Follow the tracking").help("The 3D character turns its head and blinks, opens the mouth and smiles with the face that the Stream tab follows (turn a tracking source on there)");
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
        self.paint3d_controls(ui);
    }

    /// The bones that get a slider.
    const SLIDER_BONES: [&'static str; 12] = ["spine", "chest", "neck", "head", "leftUpperArm", "leftLowerArm", "rightUpperArm", "rightLowerArm", "leftUpperLeg", "leftLowerLeg", "rightUpperLeg", "rightLowerLeg"];

    /// How many of the bone sliders (3D bones) the rig has.
    pub(super) fn bone_slider_count(&self) -> Option<usize> {
        Some(self.draw.doc.rig.params.iter().filter(|p| p.name.starts_with("3D ")).count())
    }

    /// A slider for each main bone of the 3D character (turn in degrees, in the picture plane), made once.
    pub(super) fn make_bone_sliders(&mut self, now: f64) {
        let Some(model) = &self.rigs.game.model else { return };
        let have: Vec<String> = self.draw.doc.rig.params.iter().map(|p| p.name.clone()).collect();
        let wanted: Vec<&str> = Self::SLIDER_BONES.iter().copied().filter(|b| model.bone(b).is_some() && !have.contains(&format!("3D {b}"))).collect();
        if wanted.is_empty() {
            self.rigs.game.message = Some("The bone sliders are there already (3D bones, on the left).".into());
            return;
        }
        let n = wanted.len();
        self.rig_edit("Sliders for the bones", now, |r| {
            for b in &wanted {
                r.add_param(&format!("3D {b}"), -90.0, 90.0, 0.0);
            }
        });
        self.rigs.game.message = Some(format!("{n} bone sliders are made: 3D bones, on the left. Move them to pose the 3D character, and key them in the Animate tab."));
    }

    /// The turn of every bone that has a slider, from the sliders as they are now.
    pub(super) fn bone_pose_from_sliders(&self) -> Vec<(String, f32)> {
        let rig = &self.draw.doc.rig;
        rig.params
            .iter()
            .filter_map(|p| p.name.strip_prefix("3D ").map(|b| (b.to_owned(), rig.value(&self.rigs.values, p.id))))
            .filter(|(_, v)| v.abs() > 0.01)
            .collect()
    }

    #[cfg(test)]
    pub fn bone_pose_for_test(&self) -> Vec<(String, f32)> {
        self.bone_pose_from_sliders()
    }

    /// The 3D character as a picture in the middle of the Rig tab (3D page): turn it with the slider on the right, drag on it to paint when painting is on.
    pub(super) fn game_preview_ui(&mut self, ui: &mut Ui) {
        if self.rigs.game.model.is_none() {
            ui.add_space(30.0);
            ui.vertical_centered(|ui| {
                ui.label(RichText::new("No 3D character yet").size(16.0).strong());
                ui.label(RichText::new("Press Make the 3D character on the right.").size(12.0).color(TEXT_DIM));
            });
            return;
        }
        let paint = self.rigs.game.paint3d;
        let avail = ui.available_size();
        let ph = ((avail.y - 12.0).clamp(480.0, 1100.0) / 8.0) as u32 * 8;
        let pw = (ph as f32 * 0.625) as u32 / 8 * 8;
        // following the face: the head turn and the expressions come from the tracking (steps of a tenth, so the picture is not made again for
        // a change nobody can see)
        let tracked = if self.rigs.game.follow { Some(tracked_look(&self.rigs.stream.inputs)) } else { None };
        if tracked.is_some() {
            ui.ctx().request_repaint();
        }
        // the poses of the bone sliders (3D bones): hand posing, and the keys of the Animate tab
        let slider_pose: Vec<(String, f32)> = self.bone_pose_from_sliders();
        let slider_digest = format!("{:?}", slider_pose.iter().map(|(n, v)| (n.as_str(), (v / 2.0).round() as i32)).collect::<Vec<_>>());
        let (yaw_now, expr_key) = match &tracked {
            Some((yaw, list, pose)) => (
                (*yaw / 2.0).round() as i32 * 2,
                Some(format!("{:?}{:?}{slider_digest}", list.iter().map(|(n, v)| (*n, (v * 10.0).round() as i32)).collect::<Vec<_>>(), pose.iter().map(|(n, v)| (*n, (v / 3.0).round() as i32)).collect::<Vec<_>>())),
            ),
            None => (self.rigs.game.yaw.round() as i32, Some(format!("{:?}{slider_digest}", self.rigs.game.expr))),
        };
        let key = (yaw_now, expr_key, self.rigs.game.made, self.rigs.game.guides, self.rigs.game.paint_gen, paint);
        if self.rigs.game.tex.as_ref().map(|t| &t.0) != Some(&key) {
            if let Some(m) = &self.rigs.game.model {
                let active: Vec<(&str, f32)> = match &tracked {
                    Some((_, list, _)) => list.iter().map(|(n, v)| (*n, (v * 10.0).round() / 10.0)).collect(),
                    None => self.rigs.game.expr.as_deref().map(|e| vec![(e, 1.0)]).unwrap_or_default(),
                };
                // the bones the sliders turn, and over them the arms of the tracked body
                let mut pose: Vec<(&str, f32)> = slider_pose.iter().map(|(n, v)| (n.as_str(), (v / 2.0).round() * 2.0)).collect();
                if let Some((_, _, tp)) = &tracked {
                    for (n, v) in tp {
                        let v = (v / 3.0).round() * 3.0;
                        match pose.iter_mut().find(|p| p.0 == *n) {
                            Some(p) => p.1 = v,
                            None => pose.push((*n, v)),
                        }
                    }
                }
                let img = if pose.is_empty() { preview::render_guides(m, key.0 as f32, pw, ph, &active, key.3) } else { preview::render_posed(m, key.0 as f32, pw, ph, &active, &pose) };
                let color = egui::ColorImage::from_rgba_unmultiplied([img.width() as usize, img.height() as usize], img.as_raw());
                let tex = ui.ctx().load_texture("game_preview", color, TextureOptions::LINEAR);
                self.rigs.game.tex = Some((key, tex));
            }
        }
        let shown = self.rigs.game.tex.as_ref().map(|(_, t)| t.id());
        if let Some(id) = shown {
            let size = vec2(pw as f32, ph as f32);
            let resp = ui.add(egui::Image::new((id, size)).sense(egui::Sense::drag()));
            if paint {
                self.paint3d_input(&resp, (pw, ph));
            }
        }
    }

    /// The hair of a character on an AI shape: the strands (bone chains that swing in a game or a VRM app) and the hair brush.
    fn hair_ui(&mut self, ui: &mut Ui) {
        if self.rigs.game.shape.is_none() {
            return;
        }
        section_title(ui, "Hair");
        ui.label(RichText::new("The hair is its own part on bone chains (strands). Show them on the picture to drag their joints, add a strand by clicking its joints from the scalp down, delete one, change how it swings, or correct where the hair is with the hair brush.").size(11.5).color(TEXT_DIM));
        ui.horizontal_wrapped(|ui| {
            if widgets::chip(ui, "Edit the hair", self.rigs.hair_edit).clicked() {
                self.rigs.hair_edit = !self.rigs.hair_edit;
                self.rigs.hair_new.clear();
                if self.rigs.hair_edit {
                    // the strands the app made are the start
                    self.hair_adopt_auto();
                }
            }
            if ui.small_button("Reset the hair").help("Forget every change: the app makes the strands and finds the hair again").clicked() {
                if let Some(sk) = std::sync::Arc::make_mut(&mut self.draw.doc.rig).skin.as_mut() {
                    sk.hair = Default::default();
                }
                self.rigs.hair_sel = None;
                self.rigs.hair_tex = None;
                self.rigs.hair_dirty = true;
            }
        });
        if !self.rigs.hair_edit {
            return;
        }
        ui.horizontal_wrapped(|ui| {
            for (i, name, tip) in [(0u8, "Move joints", "Drag the joints of the strands on the picture"), (1, "Add a strand", "Click the joints of a new strand from the scalp down, then Finish"), (2, "Hair brush +", "Paint over what the app took for something else: it becomes hair"), (3, "Hair brush -", "Paint over what is not hair: it stops being hair")] {
                if widgets::chip(ui, name, self.rigs.hair_tool == i).help(tip).clicked() {
                    self.rigs.hair_tool = i;
                    self.rigs.hair_new.clear();
                }
            }
        });
        if self.rigs.hair_tool == 1 {
            ui.horizontal_wrapped(|ui| {
                ui.label(RichText::new(format!("{} joints so far", self.rigs.hair_new.len())).size(12.0).color(TEXT_DIM));
                if ui.add_enabled(self.rigs.hair_new.len() >= 2, egui::Button::new("Finish the strand")).clicked() {
                    let joints = std::mem::take(&mut self.rigs.hair_new);
                    if let Some(sk) = std::sync::Arc::make_mut(&mut self.draw.doc.rig).skin.as_mut() {
                        sk.hair.chains.push(crate::vrm::hairedit::HairChain { joints, ..Default::default() });
                        self.rigs.hair_sel = Some(sk.hair.chains.len() - 1);
                    }
                    self.rigs.hair_dirty = true;
                }
                if ui.small_button("Clear").clicked() {
                    self.rigs.hair_new.clear();
                }
            });
        }
        if self.rigs.hair_tool >= 2 {
            ui.horizontal(|ui| {
                ui.label(RichText::new("Brush size").size(12.0).color(TEXT_DIM));
                ui.add(egui::DragValue::new(&mut self.rigs.hair_r).range(3.0..=300.0).speed(0.5).suffix(" px"));
            });
        }
        // the picked strand: how it hangs and swings
        let sel = self.rigs.hair_sel;
        if let Some(sk) = std::sync::Arc::make_mut(&mut self.draw.doc.rig).skin.as_mut() {
            if let Some(c) = sel.and_then(|i| sk.hair.chains.get_mut(i)) {
                let before = c.clone();
                ui.label(RichText::new(format!("Strand {} ({} joints)", sel.unwrap() + 1, c.joints.len())).size(12.0).color(TEXT_DIM));
                ui.horizontal(|ui| {
                    ui.label(RichText::new("Hangs").size(12.0).color(TEXT_DIM));
                    for (v, name) in [(1.0f32, "front"), (0.0, "middle"), (-1.0, "back")] {
                        if widgets::chip(ui, name, (c.layer - v).abs() < 0.01).clicked() {
                            c.layer = v;
                        }
                    }
                });
                ui.horizontal_wrapped(|ui| {
                    ui.label(RichText::new("Stiff").size(12.0).color(TEXT_DIM));
                    ui.add(egui::DragValue::new(&mut c.stiffness).range(0.0..=1.5).speed(0.01));
                    ui.label(RichText::new("Gravity").size(12.0).color(TEXT_DIM));
                    ui.add(egui::DragValue::new(&mut c.gravity).range(0.0..=1.0).speed(0.01));
                    ui.label(RichText::new("Drag").size(12.0).color(TEXT_DIM));
                    ui.add(egui::DragValue::new(&mut c.drag).range(0.0..=1.0).speed(0.01));
                });
                let changed = *c != before;
                let mut delete = false;
                if ui.small_button("Delete this strand").clicked() {
                    delete = true;
                }
                if delete {
                    sk.hair.chains.remove(sel.unwrap());
                    self.rigs.hair_sel = None;
                    self.rigs.hair_dirty = true;
                } else if changed {
                    self.rigs.hair_dirty = true;
                }
            }
        }
        // the character is made again when the mouse is let go
        if self.rigs.hair_dirty && !ui.input(|i| i.pointer.any_down()) {
            self.rigs.hair_dirty = false;
            self.game_make();
        }
    }

    /// Start editing from the strands the app made: they are copied into the project.
    fn hair_adopt_auto(&mut self) {
        let auto = self.rigs.game.model.as_ref().map(|m| m.hair_chains.clone()).unwrap_or_default();
        if let Some(sk) = std::sync::Arc::make_mut(&mut self.draw.doc.rig).skin.as_mut() {
            if sk.hair.chains.is_empty() {
                sk.hair.chains = auto;
            }
        }
    }

    /// The hair on the canvas: the hair found (green), the strands with their joints, and the hair brush.
    pub(super) fn hair_canvas(&mut self, ui: &mut Ui, resp: &egui::Response, p: &egui::Painter, to_screen: &dyn Fn([f32; 2]) -> egui::Pos2, to_page: &dyn Fn(egui::Pos2) -> [f32; 2], sc: f32) {
        use egui::{Color32, Stroke};
        let dabs_now = self.draw.doc.rig.skin.as_ref().map(|s| s.hair.dabs.len()).unwrap_or(0);
        // the picture of the hair: found on the drawing, corrected by the dabs
        if self.rigs.hair_base.is_none() {
            let opt = self.game_options();
            if let Some(setup) = crate::vrm::model::uncut_setup(&self.draw.doc, &opt) {
                if let Some(hb) = crate::vrm::model::head_box(&self.draw.doc.rig) {
                    self.rigs.hair_base = crate::vrm::hair::hair_mask(&setup.page, hb);
                }
            }
        }
        let stale = self.rigs.hair_tex.as_ref().map(|t| t.0) != Some(dabs_now);
        if stale {
            if let Some(base) = &self.rigs.hair_base {
                let mut m = base.clone();
                if let Some(sk) = self.draw.doc.rig.skin.as_ref() {
                    m.apply_dabs(&sk.hair.dabs);
                }
                let (img, at) = m.to_image();
                let ci = egui::ColorImage::from_rgba_unmultiplied([img.width() as usize, img.height() as usize], img.as_raw());
                let tex = ui.ctx().load_texture("hair_mask", ci, egui::TextureOptions::NEAREST);
                self.rigs.hair_tex = Some((dabs_now, tex, at));
            }
        }
        if let Some((_, tex, at)) = &self.rigs.hair_tex {
            let size = tex.size_vec2();
            let rect = egui::Rect::from_min_max(to_screen([at[0] as f32, at[1] as f32]), to_screen([at[0] as f32 + size.x, at[1] as f32 + size.y]));
            p.image(tex.id(), rect, egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)), Color32::WHITE);
        }
        // the strands
        let chains: Vec<crate::vrm::hairedit::HairChain> = self.draw.doc.rig.skin.as_ref().map(|s| s.hair.chains.clone()).unwrap_or_default();
        let green = Color32::from_rgb(120, 255, 170);
        let amber = Color32::from_rgb(255, 200, 60);
        let mut drag: Option<(usize, usize, [f32; 2])> = None;
        let mut stopped = false;
        for (ci, c) in chains.iter().enumerate() {
            let on = self.rigs.hair_sel == Some(ci);
            let col = if on { amber } else { green };
            let pts: Vec<egui::Pos2> = c.joints.iter().map(|j| to_screen(*j)).collect();
            for w in pts.windows(2) {
                p.line_segment([w[0], w[1]], Stroke::new(if on { 3.0 } else { 2.0 }, col));
            }
            for (ji, s) in pts.iter().enumerate() {
                p.circle_filled(*s, if ji == 0 { 4.0 } else { 5.5 }, if ji == 0 { Color32::WHITE } else { col });
                if self.rigs.hair_tool == 0 {
                    let r = ui.interact(egui::Rect::from_center_size(*s, egui::vec2(16.0, 16.0)), egui::Id::new(("hair_joint", ci, ji)), egui::Sense::drag());
                    if r.drag_started() || r.clicked() {
                        self.rigs.hair_sel = Some(ci);
                    }
                    if r.dragged() {
                        if let Some(pp) = r.interact_pointer_pos() {
                            drag = Some((ci, ji, to_page(pp)));
                        }
                    }
                    if r.drag_stopped() {
                        stopped = true;
                    }
                }
            }
        }
        if let Some((ci, ji, at)) = drag {
            if let Some(c) = std::sync::Arc::make_mut(&mut self.draw.doc.rig).skin.as_mut().and_then(|s| s.hair.chains.get_mut(ci)) {
                if let Some(j) = c.joints.get_mut(ji) {
                    *j = at;
                }
            }
        }
        if stopped {
            self.rigs.hair_dirty = true;
        }
        // a new strand: click its joints from the scalp down
        if self.rigs.hair_tool == 1 {
            if resp.clicked_by(egui::PointerButton::Primary) {
                if let Some(pos) = resp.interact_pointer_pos() {
                    self.rigs.hair_new.push(to_page(pos));
                }
            }
            let pts: Vec<egui::Pos2> = self.rigs.hair_new.iter().map(|j| to_screen(*j)).collect();
            for w in pts.windows(2) {
                p.line_segment([w[0], w[1]], Stroke::new(2.0, Color32::WHITE));
            }
            for s in &pts {
                p.circle_filled(*s, 5.0, Color32::WHITE);
            }
        }
        // the hair brush
        if self.rigs.hair_tool >= 2 {
            if let Some(pos) = ui.input(|i| i.pointer.hover_pos()).filter(|q| resp.rect.contains(*q)) {
                p.circle_stroke(pos, self.rigs.hair_r * sc, Stroke::new(1.5, if self.rigs.hair_tool == 2 { green } else { Color32::from_rgb(255, 90, 90) }));
            }
            if resp.dragged_by(egui::PointerButton::Primary) || resp.clicked_by(egui::PointerButton::Primary) {
                if let Some(pos) = resp.interact_pointer_pos() {
                    let at = to_page(pos);
                    let r = self.rigs.hair_r;
                    let add = self.rigs.hair_tool == 2;
                    if let Some(sk) = std::sync::Arc::make_mut(&mut self.draw.doc.rig).skin.as_mut() {
                        let far = sk.hair.dabs.last().is_none_or(|d| (d.at[0] - at[0]).hypot(d.at[1] - at[1]) > r * 0.4 || d.add != add);
                        if far {
                            sk.hair.dabs.push(crate::vrm::hairedit::HairDab { at, radius: r, add });
                        }
                    }
                    self.rigs.hair_dirty = true;
                }
            }
        }
    }

    /// The controls for painting straight on the 3D character.
    fn paint3d_controls(&mut self, ui: &mut Ui) {
        if self.rigs.game.model.is_none() {
            return;
        }
        use crate::vrm::paint3d::Tool;
        ui.horizontal_wrapped(|ui| {
            if widgets::chip(ui, "Paint on the 3D character", self.rigs.game.paint3d).help("Paint straight on the model in the preview, as in VRoid: drag on it. The preview gets bigger. Turn the model with the Turn slider to reach the sides").clicked() {
                self.rigs.game.paint3d = !self.rigs.game.paint3d;
                self.rigs.game.last_pt = None;
            }
        });
        if !self.rigs.game.paint3d {
            return;
        }
        let b = &mut self.rigs.game.brush;
        ui.horizontal_wrapped(|ui| {
            for (tool, name, tip) in [(Tool::Brush, "Brush", "Paint with a colour"), (Tool::Eraser, "Eraser", "Take away what was painted by hand"), (Tool::Blend, "Blend", "Smudge: the colour becomes the average of what is around, so edges soften"), (Tool::Dropper, "Eyedropper", "Pick the colour under the pointer")] {
                if widgets::chip(ui, name, b.tool == tool).help(tip).clicked() {
                    b.tool = tool;
                }
            }
            ui.color_edit_button_srgb(&mut b.colour);
        });
        ui.horizontal_wrapped(|ui| {
            ui.label(RichText::new("Size").size(12.0).color(TEXT_DIM));
            ui.add(egui::DragValue::new(&mut b.size).range(1.0..=200.0).speed(0.3));
            ui.label(RichText::new("Hard").size(12.0).color(TEXT_DIM));
            ui.add(egui::DragValue::new(&mut b.hardness).range(0.0..=1.0).speed(0.01));
            ui.label(RichText::new("Opacity").size(12.0).color(TEXT_DIM));
            ui.add(egui::DragValue::new(&mut b.opacity).range(0.05..=1.0).speed(0.01));
        });
        ui.horizontal_wrapped(|ui| {
            if ui.add_enabled(!self.rigs.game.paint_undo.is_empty(), egui::Button::new("Undo the last stroke")).clicked() {
                if let Some(prev) = self.rigs.game.paint_undo.pop() {
                    if let Some(sk) = std::sync::Arc::make_mut(&mut self.draw.doc.rig).skin.as_mut() {
                        sk.tex.atlas_paint = prev;
                    }
                    self.rigs.game.overlay = None;
                    self.game_make();
                }
            }
            if ui.button("Clear all paint").clicked() {
                if let Some(sk) = std::sync::Arc::make_mut(&mut self.draw.doc.rig).skin.as_mut() {
                    self.rigs.game.paint_undo.push(sk.tex.atlas_paint.clone());
                    sk.tex.atlas_paint.clear();
                }
                self.rigs.game.overlay = None;
                self.game_make();
            }
        });
    }

    /// Paint where the pointer drags on the preview.
    fn paint3d_input(&mut self, resp: &egui::Response, size: (u32, u32)) {
        use crate::vrm::paint3d;
        let Some(pos) = resp.interact_pointer_pos() else {
            if resp.drag_stopped() {
                self.paint3d_end();
            }
            return;
        };
        let rel = pos - resp.rect.min;
        let hit = self.rigs.game.model.as_ref().and_then(|m| preview::pick(m, self.rigs.game.yaw.round(), size.0, size.1, rel.x, rel.y));
        if resp.drag_started() {
            if let Some(sk) = self.draw.doc.rig.skin.as_ref() {
                self.rigs.game.paint_undo.push(sk.tex.atlas_paint.clone());
                self.rigs.game.paint_undo.truncate(12);
            }
            self.rigs.game.last_pt = None;
        }
        let Some((part, uv)) = hit else {
            self.rigs.game.last_pt = None;
            return;
        };
        let Some(model) = self.rigs.game.model.as_mut() else { return };
        let dims = model.parts[0].image.dimensions();
        if model.parts.get(part).map(|p| p.image.dimensions()) != Some(dims) {
            self.rigs.game.last_pt = None;
            return;
        }
        if self.rigs.game.overlay.as_ref().map(|o| o.dimensions()) != Some(dims) {
            let stored = self.draw.doc.rig.skin.as_ref().and_then(|s| s.tex.atlas_paint_image()).filter(|i| i.dimensions() == dims);
            self.rigs.game.overlay = Some(stored.unwrap_or_else(|| image::RgbaImage::new(dims.0, dims.1)));
        }
        let at = [uv[0] * dims.0 as f32, uv[1] * dims.1 as f32];
        let brush = self.rigs.game.brush.clone();
        let overlay = self.rigs.game.overlay.as_mut().unwrap();
        let (picked, rebuild) = match self.rigs.game.last_pt {
            Some(last) => paint3d::stroke(model, overlay, &brush, last, at),
            None => paint3d::dab(model, overlay, &brush, at[0], at[1]),
        };
        if let Some(c) = picked {
            self.rigs.game.brush.colour = c;
        }
        self.rigs.game.paint_rebuild |= rebuild;
        self.rigs.game.last_pt = Some(at);
        self.rigs.game.paint_gen += 1;
        if resp.drag_stopped() {
            self.paint3d_end();
        }
    }

    /// The stroke is over: the paint is kept with the project (and the model made again when an eraser was used).
    fn paint3d_end(&mut self) {
        self.rigs.game.last_pt = None;
        if self.rigs.game.brush.tool == crate::vrm::paint3d::Tool::Dropper {
            return;
        }
        if let Some(o) = self.rigs.game.overlay.clone() {
            if let Some(sk) = std::sync::Arc::make_mut(&mut self.draw.doc.rig).skin.as_mut() {
                sk.tex.set_atlas_paint(&o);
            }
        }
        if std::mem::take(&mut self.rigs.game.paint_rebuild) {
            self.game_make();
        }
    }

    #[cfg(test)]
    pub fn test_game_make(&mut self) {
        self.game_make();
    }

    #[cfg(test)]
    pub fn test_hair_edit(&mut self, on: bool) {
        self.rigs.hair_edit = on;
        if on {
            self.hair_adopt_auto();
        }
    }

    #[cfg(test)]
    pub fn test_game_bytes(&mut self, fmt: Format) -> Result<(Vec<u8>, Summary), String> {
        self.game_bytes(fmt)
    }

    #[cfg(test)]
    pub fn test_game(&mut self) -> &mut GameUi {
        &mut self.rigs.game
    }
}

/// What the tracked face does, as the head turn (degrees) and the expressions of a VRM (blink, aa, happy, look...), from the named inputs of
/// the Stream tab. Only what is tracked is in the list.
pub fn tracked_look(inputs: &[(&'static str, f32)]) -> (f32, Vec<(&'static str, f32)>, Vec<(&'static str, f32)>) {
    let get = |n: &str| inputs.iter().find(|i| i.0 == n).map(|i| i.1);
    let mut list: Vec<(&'static str, f32)> = Vec::new();
    let (l, r) = (get("EyeOpenLeft"), get("EyeOpenRight"));
    match (l, r) {
        (Some(l), Some(r)) => {
            let (bl, br) = (1.0 - l.clamp(0.0, 1.0), 1.0 - r.clamp(0.0, 1.0));
            if (bl - br).abs() < 0.25 {
                list.push(("blink", (bl + br) / 2.0));
            } else {
                list.push(("blinkLeft", bl));
                list.push(("blinkRight", br));
            }
        }
        _ => {
            if let Some(o) = get("EyeOpen") {
                list.push(("blink", 1.0 - o.clamp(0.0, 1.0)));
            }
        }
    }
    if let Some(m) = get("MouthOpen") {
        list.push(("aa", m.clamp(0.0, 1.0)));
    }
    if let Some(s) = get("MouthSmile") {
        list.push(("happy", s.clamp(0.0, 1.0) * 0.8));
    }
    if let Some(y) = get("EyeY") {
        list.push((if y >= 0.0 { "lookUp" } else { "lookDown" }, y.abs().min(1.0)));
    }
    if let Some(x) = get("EyeX") {
        list.push((if x >= 0.0 { "lookRight" } else { "lookLeft" }, x.abs().min(1.0)));
    }
    let yaw = get("FaceAngleX").unwrap_or(0.0).clamp(-45.0, 45.0);
    // the arms: the model's arms stand out sideways (T pose), the tracked angle is 0 hanging down, 90 out, 180 up; a bone turns
    // counter-clockwise seen from the front, the character's left arm is on the right of the picture
    let mut pose: Vec<(&'static str, f32)> = Vec::new();
    for (side, up_n, low_n, up_b, low_b, sign) in [("Left", "ArmLeftAngle", "ForearmLeftAngle", "leftUpperArm", "leftLowerArm", 1.0f32), ("Right", "ArmRightAngle", "ForearmRightAngle", "rightUpperArm", "rightLowerArm", -1.0f32)] {
        let _ = side;
        if let Some(u) = get(up_n) {
            pose.push((up_b, sign * (u - 90.0)));
            if let Some(f) = get(low_n) {
                pose.push((low_b, sign * (f - u)));
            }
        }
    }
    (yaw, list, pose)
}

/// The Python that has the AI packages: the one in the SeviathAI folder of the user when it is there, else plain `python`.
fn default_python() -> String {
    let p = crate::aisetup::venv_python(&crate::aisetup::root());
    if p.exists() { p.to_string_lossy().to_string() } else { "python".to_string() }
}

#[cfg(test)]
mod tracked_tests {
    use super::tracked_look;

    #[test]
    fn the_tracked_face_becomes_a_head_turn_and_expressions() {
        let inputs: Vec<(&'static str, f32)> = vec![("FaceAngleX", 20.0), ("EyeOpenLeft", 0.0), ("EyeOpenRight", 0.1), ("MouthOpen", 0.6), ("MouthSmile", 1.0)];
        let (yaw, list, _) = tracked_look(&inputs);
        assert_eq!(yaw, 20.0);
        let g = |n: &str| list.iter().find(|x| x.0 == n).map(|x| x.1);
        assert!(g("blink").unwrap() > 0.9, "both eyes shut: blink");
        assert_eq!(g("aa"), Some(0.6));
        assert!((g("happy").unwrap() - 0.8).abs() < 1e-5);
        // one eye shut: a wink
        let (_, wink, _) = tracked_look(&[("EyeOpenLeft", 0.0), ("EyeOpenRight", 1.0)]);
        assert!(wink.iter().any(|x| x.0 == "blinkLeft" && x.1 > 0.9) && wink.iter().any(|x| x.0 == "blinkRight" && x.1 < 0.1));
        // nothing tracked: no turn, no expressions
        assert_eq!(tracked_look(&[]), (0.0, vec![], vec![]));
        // arms: the left one straight out stays as the T pose, the right one hanging down turns a quarter turn the other way
        let (_, _, pose) = tracked_look(&[("ArmLeftAngle", 90.0), ("ForearmLeftAngle", 180.0), ("ArmRightAngle", 0.0)]);
        let p = |n: &str| pose.iter().find(|x| x.0 == n).map(|x| x.1);
        assert_eq!(p("leftUpperArm"), Some(0.0));
        assert_eq!(p("leftLowerArm"), Some(90.0), "the forearm up from an arm that is out");
        assert_eq!(p("rightUpperArm"), Some(90.0), "hanging down is the other way round on the other side");
    }
}
