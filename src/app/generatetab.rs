// SPDX-License-Identifier: GPL-3.0-or-later
// The Generate tab. Two pages: Game assets (a text makes pictures of one object, the picked picture becomes a 3D model with a real
// texture, a real size and its pivot, as a .glb for a game engine) and VRM characters (a text makes pictures of a character in the
// standard A-pose, the picked picture becomes a rigged, textured 3D character with the Rig tab's own steps, saved as VRM or GLB).
// The AI runs on this computer (`aigen.rs`, `tools/ai/`), one job at a time.

use crate::widgets::HelpExt;
use std::sync::mpsc::{Receiver, TryRecvError};

use eframe::egui::{self, Color32, RichText, Sense, TextureHandle, TextureOptions, Ui, vec2};
use image::RgbaImage;

use super::SeviathApp;
use crate::aigen::{AssetRequest, PictureRequest, generate_pictures, make_asset};
use crate::assetview::{AssetMesh, View};
use crate::generate::*;
use crate::theme::*;
use crate::vrm::export::Format;
use crate::widgets::{self, section_title};

#[derive(Clone, Copy, PartialEq, Eq, Default)]
pub enum Page {
    #[default]
    Assets,
    Characters,
    Animations,
}

struct Pic {
    img: RgbaImage,
    tex: Option<TextureHandle>,
}

/// Where the making of a character stands (the steps after the picture was picked).
#[derive(Clone, Copy, PartialEq, Eq, Default, Debug)]
pub enum Stage {
    #[default]
    Idle,
    Shape,
    /// The AI works out the skin weights of the shape.
    Skin,
}

/// Models and style files of your own (any picture model that is free to use for what you make).
pub struct Own {
    pub checkpoint: String,
    pub lora: String,
    pub lora_weight: f32,
}

impl Default for Own {
    fn default() -> Self {
        Own { checkpoint: String::new(), lora: String::new(), lora_weight: 0.8 }
    }
}

type PicJob = Receiver<Result<(Vec<RgbaImage>, u32), String>>;
type AssetJob = Receiver<Result<Vec<u8>, String>>;

#[derive(Default)]
pub struct GenerateUi {
    pub page: Page,
    pub look: Look,
    pub asset: AssetLook,
    pub own: Own,
    pub anim: super::animpage::AnimState,
    pics: Vec<Pic>,
    pub picked: Option<usize>,
    pic_job: Option<PicJob>,
    pub seed_used: Option<u32>,
    asset_job: Option<AssetJob>,
    pub glb: Option<Vec<u8>>,
    pub mesh: Option<AssetMesh>,
    /// How the model is turned (yaw, pitch) and whether the triangles show.
    pub view: (f32, f32),
    pub wire: bool,
    /// Colours, the directions of the surface, or depth.
    pub view_mode: View,
    view_tex: Option<((i32, i32, bool, u64, View), TextureHandle)>,
    made: u64,
    pub message: Option<String>,
    /// Pictures, then the 3D model, one after the other without a click in between.
    auto: bool,
    pub stage: Stage,
    /// What is installed (the base AI files, the picture AI, CharacterGen), and when it was looked at (the folders are not read every frame).
    ready: Option<(f64, [bool; 3])>,
    /// The open project may be replaced by the character that is made (when it has unsaved changes).
    pub replace_ok: bool,
    /// Make the character's shape with the AI (otherwise the round card of the Rig tab).
    pub flat_card: bool,
}

impl SeviathApp {
    pub(super) fn generate_ui(&mut self, ui: &mut Ui) {
        let ctx = ui.ctx().clone();
        let now = ctx.input(|i| i.time);
        egui::Panel::top("gen_top").frame(egui::Frame::new().fill(PANEL).inner_margin(egui::Margin::same(8))).show(ui, |ui| self.gen_top(ui));
        egui::Panel::left("gen_left").default_size(330.0).size_range(260.0..=520.0).resizable(true).frame(egui::Frame::new().fill(PANEL).inner_margin(egui::Margin::same(10))).show(ui, |ui| {
            egui::ScrollArea::vertical().id_salt("gen_left_scroll").show(ui, |ui| match self.genr.page {
                Page::Assets => self.gen_assets_left(ui),
                Page::Characters => self.gen_characters_left(ui),
                Page::Animations => self.gen_animations_left(ui),
            });
        });
        egui::Panel::right("gen_right").default_size(310.0).size_range(250.0..=480.0).resizable(true).frame(egui::Frame::new().fill(PANEL).inner_margin(egui::Margin::same(10))).show(ui, |ui| {
            egui::ScrollArea::vertical().id_salt("gen_right_scroll").show(ui, |ui| match self.genr.page {
                Page::Assets => self.gen_assets_right(ui, now),
                Page::Characters => self.gen_characters_right(ui, now),
                Page::Animations => self.gen_animations_right(ui),
            });
        });
        egui::CentralPanel::default().frame(egui::Frame::new().fill(Color32::from_rgb(14, 20, 18)).inner_margin(egui::Margin::same(10))).show(ui, |ui| {
            egui::ScrollArea::vertical().id_salt("gen_center_scroll").show(ui, |ui| {
                if self.genr.page != Page::Animations {
                    self.gen_pictures_ui(ui);
                    ui.add_space(8.0);
                }
                match self.genr.page {
                    Page::Assets => self.gen_asset_view(ui),
                    Page::Characters => self.gen_character_view(ui),
                    Page::Animations => self.gen_animations_center(ui),
                }
            });
        });
    }

    fn gen_top(&mut self, ui: &mut Ui) {
        ui.horizontal_wrapped(|ui| {
            ui.label(RichText::new("Generate").size(16.0).strong());
            ui.add_space(8.0);
            for (p, label, tip) in [(Page::Assets, "Game assets", "Props, weapons, plants, buildings and more: a textured 3D model for a game engine"), (Page::Characters, "VRM characters", "A rigged, textured 3D character, saved as VRM or GLB"), (Page::Animations, "Animations", "22 ready animations and an animation maker for the 3D character, saved in the game file")] {
                if widgets::chip(ui, label, self.genr.page == p).help(tip).clicked() {
                    self.genr.page = p;
                }
            }
            ui.add_space(12.0);
            let busy = self.gen_busy();
            if let Some(what) = busy {
                ui.spinner();
                ui.label(RichText::new(what).size(12.0).color(GOLD));
            } else if let Some(m) = &self.genr.message {
                ui.label(RichText::new(m).size(12.0).color(TEXT_DIM));
            }
        });
        // what is installed, and the way to get the rest with one button
        let now = ui.input(|i| i.time);
        if self.genr.ready.is_none_or(|(t, _)| now - t > 3.0) {
            let st = crate::aisetup::status();
            self.genr.ready = Some((now, [st.ready(), crate::setupall::picture_ready(), st.charactergen]));
        }
        let ready = self.genr.ready.map(|r| r.1).unwrap_or([false; 3]);
        ui.horizontal_wrapped(|ui| {
            for (ok, name, tip) in [(ready[0], "AI files", "Python, PyTorch and TripoSR (game assets and the first shape AI)."), (ready[1], "Picture AI", "Stable Diffusion XL and its pose model: they make the pictures from a text."), (ready[2], "CharacterGen", "The better shape AI for characters.")] {
                let col = if ok { EMERALD } else { GOLD };
                ui.label(RichText::new(format!("{} {name}", if ok { "Ready:" } else { "Missing:" })).size(11.5).color(col)).on_hover_text(tip);
            }
            if ready.iter().any(|r| !*r) && ui.small_button("Get everything").help("Opens the Downloads window: the first card there downloads and installs all of it with one button (about 45 GB, free). It also shows how much each part is.").clicked() {
                self.open_downloads();
            }
        });
        self.ai_setup_ui(ui);
    }

    /// What is being made now, in words (None when nothing runs).
    pub(super) fn gen_busy(&self) -> Option<String> {
        if self.genr.pic_job.is_some() {
            Some("Making pictures... the first time the AI models are downloaded (about 10 GB), after that it takes a minute or two.".into())
        } else if self.genr.asset_job.is_some() {
            Some("Making the 3D model... about a minute.".into())
        } else if self.genr.stage == Stage::Skin {
            Some("The AI is working out the skin weights... about twenty seconds.".into())
        } else if self.genr.stage == Stage::Shape || self.shape_busy() {
            Some("Making the 3D shape of the character... about a minute.".into())
        } else {
            None
        }
    }

    // ------------------------------------------------------------------ jobs

    /// Takes the answers of the AI jobs, whichever tab is open (so a job that was started is not left waiting when you look at another tab).
    pub(super) fn gen_poll(&mut self, ctx: &egui::Context, now: f64) {
        let pics = self.genr.pic_job.as_ref().map(|rx| rx.try_recv());
        match pics {
            Some(Ok(Ok((imgs, seed)))) => {
                self.genr.pic_job = None;
                let n = imgs.len();
                self.genr.pics = imgs.into_iter().map(|img| Pic { img, tex: None }).collect();
                self.genr.picked = Some(0);
                self.genr.seed_used = Some(seed);
                self.genr.message = Some(format!("{n} pictures made (the first seed was {seed}). Click the one you like."));
                if std::mem::take(&mut self.genr.auto) {
                    self.gen_make_model(now);
                }
            }
            Some(Ok(Err(e))) => {
                self.genr.pic_job = None;
                self.genr.auto = false;
                self.genr.message = Some(format!("The AI could not make the pictures: {e}"));
            }
            Some(Err(TryRecvError::Empty)) => ctx.request_repaint_after(std::time::Duration::from_millis(600)),
            Some(Err(TryRecvError::Disconnected)) => {
                self.genr.pic_job = None;
                self.genr.auto = false;
            }
            None => {}
        }
        let asset = self.genr.asset_job.as_ref().map(|rx| rx.try_recv());
        match asset {
            Some(Ok(Ok(bytes))) => {
                self.genr.asset_job = None;
                match AssetMesh::from_glb(&bytes) {
                    Ok(m) => {
                        let s = m.stats();
                        self.genr.message = Some(format!("The 3D model is made: {} triangles, {:.2} x {:.2} x {:.2} m.", s.triangles, s.size[0], s.size[1], s.size[2]));
                        self.genr.mesh = Some(m);
                        self.genr.glb = Some(bytes);
                        self.genr.made += 1;
                        self.genr.view = (30.0, 12.0);
                    }
                    Err(e) => self.genr.message = Some(format!("The model could not be read: {e}")),
                }
            }
            Some(Ok(Err(e))) => {
                self.genr.asset_job = None;
                self.genr.message = Some(format!("The AI could not make the 3D model: {e}"));
            }
            Some(Err(TryRecvError::Empty)) => ctx.request_repaint_after(std::time::Duration::from_millis(600)),
            Some(Err(TryRecvError::Disconnected)) => self.genr.asset_job = None,
            None => {}
        }
        if self.genr.stage == Stage::Skin {
            let r = self.rigs.game.skin_job.as_ref().map(|rx| rx.try_recv());
            match r {
                Some(Ok(res)) => {
                    self.rigs.game.skin_job = None;
                    self.genr.stage = Stage::Idle;
                    let note = match res {
                        Ok(s) => {
                            self.rigs.game.ai_skin = Some(std::sync::Arc::new(s));
                            "The skin weights come from the AI."
                        }
                        Err(e) => {
                            self.rigs.game.ai_skin = None;
                            self.genr.message = Some(format!("The AI skin weights failed ({e}); the weights of the drawing are used."));
                            ""
                        }
                    };
                    self.game_make();
                    if !note.is_empty() {
                        self.genr.message = self.rigs.game.message.clone().map(|m| format!("{m} {note}"));
                    }
                }
                Some(Err(std::sync::mpsc::TryRecvError::Empty)) => ctx.request_repaint_after(std::time::Duration::from_millis(600)),
                Some(Err(_)) | None => {
                    self.rigs.game.skin_job = None;
                    self.genr.stage = Stage::Idle;
                }
            }
        }
        // the character: the shape comes from the AI, then the character is built on it
        if self.genr.stage == Stage::Shape {
            self.shape_poll(ctx);
            if !self.shape_busy() {
                self.genr.stage = Stage::Idle;
                if self.rigs.game.shape.is_some() {
                    if self.rigs.game.use_ai_skin && crate::aigen::mia_ready() {
                        let path = crate::ai::helper_folder().join("shape.glb");
                        let (tx, rx) = std::sync::mpsc::channel();
                        std::thread::spawn(move || {
                            let _ = tx.send(crate::aigen::mia_skin(&path));
                        });
                        self.rigs.game.skin_job = Some(rx);
                        self.genr.stage = Stage::Skin;
                        self.genr.message = Some("The AI shape is made. The AI is working out the skin weights...".into());
                    } else {
                        self.game_make();
                        self.genr.message = self.rigs.game.message.clone();
                    }
                } else {
                    self.genr.message = self.rigs.game.message.clone().or(Some("The AI shape was not made.".into()));
                }
            } else {
                ctx.request_repaint_after(std::time::Duration::from_millis(600));
            }
        }
    }

    fn gen_make_pictures(&mut self) {
        if self.gen_busy().is_some() {
            return;
        }
        let g = &self.genr;
        let req = match g.page {
            Page::Assets => {
                let (prompt, negative) = g.asset.prompt();
                PictureRequest { prompt, negative, count: g.asset.count, seed: g.asset.seed, steps: g.asset.steps, guidance: g.asset.guidance, size: (1024, 1024), pose: "none", checkpoint: nonempty(&g.own.checkpoint), lora: nonempty(&g.own.lora).map(|f| (f, g.own.lora_weight)) }
            }
            Page::Animations => return,
            Page::Characters => {
                let (prompt, negative) = g.look.prompt();
                PictureRequest { prompt, negative, count: g.look.count, seed: g.look.seed, steps: g.look.steps, guidance: g.look.guidance, size: g.look.size_px(), pose: "a-pose", checkpoint: nonempty(&g.own.checkpoint), lora: nonempty(&g.own.lora).map(|f| (f, g.own.lora_weight)) }
            }
        };
        let python = self.rigs.game.ai_python.clone();
        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let _ = tx.send(generate_pictures(&python, &req));
        });
        self.genr.pic_job = Some(rx);
        self.genr.message = None;
    }

    fn gen_make_model(&mut self, _now: f64) {
        let Some(img) = self.genr.picked.and_then(|i| self.genr.pics.get(i)).map(|p| p.img.clone()) else {
            self.genr.message = Some("Pick a picture first.".into());
            return;
        };
        if self.gen_busy().is_some() {
            return;
        }
        let a = &self.genr.asset;
        let req = AssetRequest { height_m: a.height_m, triangles: a.triangles, texture: a.texture, detail: a.detail, pivot_bottom: a.pivot_bottom, normal_map: a.normal_map };
        let python = self.rigs.game.ai_python.clone();
        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let _ = tx.send(make_asset(&python, &img, &req));
        });
        self.genr.asset_job = Some(rx);
    }

    /// The picked picture becomes the open project (cut out of its background and rigged with the automatic rig), then the AI makes its
    /// 3D shape and the character is built on it.
    fn gen_make_character(&mut self, now: f64) {
        let Some(img) = self.genr.picked.and_then(|i| self.genr.pics.get(i)).map(|p| p.img.clone()) else {
            self.genr.message = Some("Pick a picture first.".into());
            return;
        };
        if self.gen_busy().is_some() {
            return;
        }
        if self.is_dirty() && !self.genr.replace_ok {
            self.genr.message = Some("The open project has changes that are not saved. Save it first, or tick \"Replace the open project\".".into());
            return;
        }
        let cut = crate::rig::uncut::cut_out_background(&img);
        let (w, h) = (cut.width().max(64), cut.height().max(64));
        let doc = crate::doc::Document::new(w, h);
        self.load_document(doc, None, "Generated character".into());
        self.tab = super::Tab::Generate;
        self.draw.import_picture_layer(&cut, "Generated character");
        let id = self.draw.doc.layers.iter().find(|l| l.name == "Generated character").map(|l| l.id);
        self.rigs.selected = id.into_iter().collect();
        self.rigs.game.model = None;
        self.rigs.game.shape = None;
        self.rig_the_uncut(now);
        if self.draw.doc.rig.skin.is_none() {
            self.genr.message = Some(self.rigs.message.clone().unwrap_or("The body could not be found in the picture. Try another picture (one body, standing, seen from the front).".into()));
            return;
        }
        let ready = crate::aisetup::status().shape_ready(self.rigs.game.shape_model);
        if !self.genr.flat_card && ready {
            self.shape_start();
            if self.shape_busy() {
                self.genr.stage = Stage::Shape;
                self.genr.message = Some("The skeleton is made. The AI is making the 3D shape...".into());
                return;
            }
        }
        self.game_make();
        self.genr.message = self.rigs.game.message.clone();
    }

    // ------------------------------------------------------------------ the middle

    fn gen_pictures_ui(&mut self, ui: &mut Ui) {
        section_title(ui, "Pictures");
        if self.genr.pics.is_empty() {
            ui.label(RichText::new("Write what you want on the left and press Make pictures on the right. Or use a picture you have.").size(12.0).color(TEXT_DIM));
        }
        let (w, h) = if self.genr.page == Page::Assets { (170.0, 170.0) } else { (130.0, 190.0) };
        ui.horizontal_wrapped(|ui| {
            for i in 0..self.genr.pics.len() {
                let on = self.genr.picked == Some(i);
                if self.genr.pics[i].tex.is_none() {
                    let p = &self.genr.pics[i].img;
                    let small = image::imageops::thumbnail(p, (p.width() / 2).max(1), (p.height() / 2).max(1));
                    let color = egui::ColorImage::from_rgba_unmultiplied([small.width() as usize, small.height() as usize], small.as_raw());
                    self.genr.pics[i].tex = Some(ui.ctx().load_texture(format!("gen_pic_{i}"), color, TextureOptions::LINEAR));
                }
                let tex = self.genr.pics[i].tex.as_ref().unwrap();
                let r = ui.add(egui::Image::new((tex.id(), vec2(w, h))).fit_to_exact_size(vec2(w, h)).corner_radius(6.0).sense(Sense::click()));
                if on {
                    ui.painter().rect_stroke(r.rect, 6.0, egui::Stroke::new(2.5, EMERALD), egui::StrokeKind::Outside);
                }
                if r.clicked() {
                    self.genr.picked = Some(i);
                }
            }
        });
        ui.horizontal_wrapped(|ui| {
            if ui.button("Use a picture I have...").help("A picture of one object (or one standing character) on a plain background").clicked() {
                if let Some(p) = rfd::FileDialog::new().set_title("A picture").add_filter("Pictures", &["png", "jpg", "jpeg", "webp"]).pick_file() {
                    match image::open(&p) {
                        Ok(i) => {
                            self.genr.pics = vec![Pic { img: i.to_rgba8(), tex: None }];
                            self.genr.picked = Some(0);
                            self.genr.message = Some(format!("Using {}.", p.display()));
                        }
                        Err(e) => self.genr.message = Some(format!("The picture could not be opened: {e}")),
                    }
                }
            }
            if let (Some(i), true) = (self.genr.picked, !self.genr.pics.is_empty()) {
                if ui.button("Save the picked picture...").help("Save the picked picture as a PNG file.").clicked() {
                    if let Some(p) = rfd::FileDialog::new().set_title("Save the picture").set_file_name("generated.png").add_filter("PNG picture", &["png"]).save_file() {
                        let _ = self.genr.pics[i].img.save(&p);
                    }
                }
            }
        });
    }

    fn gen_asset_view(&mut self, ui: &mut Ui) {
        section_title(ui, "3D model");
        let Some(mesh) = &self.genr.mesh else {
            ui.label(RichText::new("The model shows here when it is made.").size(12.0).color(TEXT_DIM));
            return;
        };
        let side = ui.available_width().clamp(320.0, 560.0).min(560.0);
        let (yaw, pitch) = self.genr.view;
        let key = (yaw.round() as i32, pitch.round() as i32, self.genr.wire, self.genr.made, self.genr.view_mode);
        if self.genr.view_tex.as_ref().map(|t| t.0) != Some(key) {
            let n = side as u32 / 4 * 4;
            let img = mesh.render_view(yaw, pitch, n, n, self.genr.wire, self.genr.view_mode);
            let color = egui::ColorImage::from_rgba_unmultiplied([img.width() as usize, img.height() as usize], img.as_raw());
            self.genr.view_tex = Some((key, ui.ctx().load_texture("gen_asset_view", color, TextureOptions::LINEAR)));
        }
        let stats = mesh.stats();
        if let Some((_, tex)) = &self.genr.view_tex {
            let resp = ui.add(egui::Image::new((tex.id(), vec2(side, side))).corner_radius(6.0).sense(Sense::drag()));
            if resp.dragged() {
                let d = resp.drag_delta();
                self.genr.view.0 -= d.x * 0.6;
                self.genr.view.1 = (self.genr.view.1 + d.y * 0.4).clamp(-89.0, 89.0);
            }
            resp.help("Drag to turn it");
        }
        ui.horizontal_wrapped(|ui| {
            for (v, label, tip) in [(View::Textured, "Colours", "The model with its texture."), (View::Normals, "Normals", "The direction of the surface in colours: red to the right, green up, blue to the viewer."), (View::Depth, "Depth", "How far every point is: white is near, black is far.")] {
                if widgets::chip(ui, label, self.genr.view_mode == v).help(tip).clicked() {
                    self.genr.view_mode = v;
                }
            }
        });
        ui.horizontal_wrapped(|ui| {
            ui.checkbox(&mut self.genr.wire, "Show the triangles").help("Draw the triangle edges of the model over it.");
            if ui.small_button("Front").help("Look at it from the front.").clicked() {
                self.genr.view = (0.0, 0.0);
            }
            if ui.small_button("Side").help("Look at it from the side.").clicked() {
                self.genr.view = (90.0, 0.0);
            }
            if ui.small_button("Back").help("Go back one step.").clicked() {
                self.genr.view = (180.0, 0.0);
            }
            if ui.small_button("Top").help("Look at it from above.").clicked() {
                self.genr.view = (0.0, 89.0);
            }
        });
        let over = stats.triangles as u32 > self.genr.asset.triangles + self.genr.asset.triangles / 10;
        ui.label(
            RichText::new(format!(
                "{} triangles{}, {} points, {:.2} x {:.2} x {:.2} m, texture {}",
                stats.triangles,
                if over { " (more than asked for)" } else { "" },
                stats.vertices,
                stats.size[0],
                stats.size[1],
                stats.size[2],
                stats.texture.map(|t| format!("{} x {}", t.0, t.1)).unwrap_or("none".into())
            ))
            .size(11.5)
            .color(if over { GOLD } else { TEXT_DIM }),
        );
    }

    fn gen_character_view(&mut self, ui: &mut Ui) {
        section_title(ui, "3D character");
        if self.rigs.game.model.is_none() {
            ui.label(RichText::new("The character shows here when it is made. It is the character of the Rig tab (3D character page), so everything there works on it too.").size(12.0).color(TEXT_DIM));
            return;
        }
        self.game_preview_ui(ui);
        ui.horizontal(|ui| {
            ui.label(RichText::new("Turn").size(12.0).color(TEXT_DIM));
            ui.add(egui::Slider::new(&mut self.rigs.game.yaw, -180.0..=180.0).suffix(" deg"));
        });
    }

    // ------------------------------------------------------------------ the left side

    fn gen_text(ui: &mut Ui, label: &str, hint: &str, text: &mut String, rows: usize) {
        ui.label(RichText::new(label).size(12.0).color(TEXT_DIM));
        ui.add(egui::TextEdit::multiline(text).desired_rows(rows).desired_width(f32::INFINITY).hint_text(hint));
    }

    fn gen_assets_left(&mut self, ui: &mut Ui) {
        section_title(ui, "What do you want");
        ui.label(RichText::new("Kind").size(12.0).color(TEXT_DIM));
        ui.horizontal_wrapped(|ui| {
            for (i, k) in ASSET_KINDS.iter().enumerate() {
                if widgets::chip(ui, k.label, self.genr.asset.kind == i).help(format!("Usual height {} m", k.height_m)).clicked() {
                    self.genr.asset.set_kind(i);
                }
            }
        });
        ui.label(RichText::new("Style").size(12.0).color(TEXT_DIM));
        ui.horizontal_wrapped(|ui| {
            for (i, s) in ASSET_STYLES.iter().enumerate() {
                if widgets::chip(ui, s.label, self.genr.asset.style == i).help(s.hint).clicked() {
                    self.genr.asset.style = i;
                }
            }
        });
        Self::gen_text(ui, "What is it", "an old iron lantern with a glass and a hook", &mut self.genr.asset.what, 3);
        Self::gen_text(ui, "Colors, materials, details", "rusty dark metal, warm orange glow, small chains", &mut self.genr.asset.details, 2);
        ui.label(RichText::new("Keep out").size(12.0).color(TEXT_DIM));
        ui.add(egui::TextEdit::singleline(&mut self.genr.asset.avoid).desired_width(f32::INFINITY).hint_text("things that must not be there"));
        ui.add_space(6.0);
        section_title(ui, "Pictures");
        let a = &mut self.genr.asset;
        gen_knobs(ui, &mut a.count, &mut a.steps, &mut a.guidance, &mut a.seed);
        ui.add_space(6.0);
        section_title(ui, "3D model");
        ui.horizontal(|ui| {
            ui.label(RichText::new("Height").size(12.0).color(TEXT_DIM));
            ui.add(egui::DragValue::new(&mut a.height_m).range(0.02..=100.0).speed(0.02).suffix(" m"));
        });
        ui.label(RichText::new("Triangles the game can afford").size(12.0).color(TEXT_DIM));
        ui.horizontal_wrapped(|ui| {
            for (n, label) in [(1000, "1 000"), (3000, "3 000"), (8000, "8 000"), (20000, "20 000"), (50000, "50 000")] {
                if widgets::chip(ui, label, a.triangles == n).help("How many triangles the game can afford for this model.").clicked() {
                    a.triangles = n;
                }
            }
        });
        ui.add(egui::Slider::new(&mut a.triangles, 300..=100_000).logarithmic(true));
        ui.label(RichText::new("Texture").size(12.0).color(TEXT_DIM));
        ui.horizontal_wrapped(|ui| {
            for n in [512, 1024, 2048, 4096] {
                if widgets::chip(ui, &format!("{n}"), a.texture == n).help("Pick this choice.").clicked() {
                    a.texture = n;
                }
            }
        });
        ui.label(RichText::new("How fine the shape is").size(12.0).color(TEXT_DIM));
        ui.horizontal_wrapped(|ui| {
            for (n, label) in [(192, "Fast"), (256, "Normal"), (320, "Fine")] {
                if widgets::chip(ui, label, a.detail == n).help("How fine the 3D shape is made: finer takes longer.").clicked() {
                    a.detail = n;
                }
            }
        });
        ui.checkbox(&mut a.normal_map, "Bake a normal map").help("Makes a normal map from the fine shape before the triangles are reduced, so the shape (dents, folds, carving) is still seen in the light with few triangles. Game engines use it with the colour texture.");
        ui.checkbox(&mut a.pivot_bottom, "Hold it at the middle of its bottom").help("The model's zero point is where it touches the ground, so it stands on the floor in the game. Off: its middle.");
        ui.add_space(6.0);
        let (p, n) = self.genr.asset.prompt();
        self.gen_own_and_prompt(ui, &p, &n);
    }

    fn gen_characters_left(&mut self, ui: &mut Ui) {
        section_title(ui, "Style");
        ui.horizontal_wrapped(|ui| {
            for (i, s) in STYLES.iter().enumerate() {
                if widgets::chip(ui, s.label, self.genr.look.style == i).help(s.hint).clicked() {
                    self.genr.look.style = i;
                }
            }
        });
        ui.add_space(6.0);
        section_title(ui, "The character, in words");
        Self::gen_text(ui, "Who", "a girl with long pink hair and green eyes", &mut self.genr.look.who, 2);
        Self::gen_text(ui, "Outfit", "a green hooded coat, white trousers, boots", &mut self.genr.look.outfit, 2);
        Self::gen_text(ui, "Body", "slim and tall", &mut self.genr.look.body, 1);
        Self::gen_text(ui, "More", "a calm look, a small bag", &mut self.genr.look.more, 2);
        ui.label(RichText::new("Keep out").size(12.0).color(TEXT_DIM));
        ui.add(egui::TextEdit::singleline(&mut self.genr.look.avoid).desired_width(f32::INFINITY).hint_text("things that must not be there"));
        ui.label(RichText::new("The pose is always the standard A-pose, seen from the front, so the character moves in a game.").size(11.5).color(TEXT_DIM));
        ui.add_space(6.0);
        section_title(ui, "Pictures");
        ui.horizontal_wrapped(|ui| {
            for (i, s) in SIZES.iter().enumerate() {
                if widgets::chip(ui, s.0, self.genr.look.size == i).help(format!("{} x {}", s.1, s.2)).clicked() {
                    self.genr.look.size = i;
                }
            }
        });
        let l = &mut self.genr.look;
        gen_knobs(ui, &mut l.count, &mut l.steps, &mut l.guidance, &mut l.seed);
        ui.add_space(6.0);
        let (p, n) = self.genr.look.prompt();
        self.gen_own_and_prompt(ui, &p, &n);
    }

    /// Your own picture model and style file, and the text the AI is given.
    fn gen_own_and_prompt(&mut self, ui: &mut Ui, prompt: &str, negative: &str) {
        egui::CollapsingHeader::new(RichText::new("The text the AI gets").size(12.5).strong()).id_salt("gen_prompt").default_open(false).show(ui, |ui| {
            ui.label(RichText::new(prompt).size(11.0).color(TEXT_DIM));
            ui.label(RichText::new(format!("Not wanted: {negative}")).size(11.0).color(TEXT_DIM));
        });
        egui::CollapsingHeader::new(RichText::new("Your own picture model").size(12.5).strong()).id_salt("gen_own").default_open(false).show(ui, |ui| {
            ui.label(RichText::new("Stable Diffusion XL (free to use, also for work) makes the pictures. A model of your own that is free to use for what you make can replace it: pick its .safetensors file. A style file (LoRA) adds a look.").size(11.0).color(TEXT_DIM));
            for (label, which) in [("Model file", 0), ("Style file", 1)] {
                ui.horizontal(|ui| {
                    ui.label(RichText::new(label).size(12.0).color(TEXT_DIM));
                    let text = if which == 0 { &mut self.genr.own.checkpoint } else { &mut self.genr.own.lora };
                    ui.add(egui::TextEdit::singleline(text).desired_width(150.0));
                    if ui.small_button("Pick...").help("Choose a file.").clicked() {
                        if let Some(p) = rfd::FileDialog::new().add_filter("Model files", &["safetensors"]).pick_file() {
                            *text = p.to_string_lossy().to_string();
                        }
                    }
                });
            }
            ui.add(egui::Slider::new(&mut self.genr.own.lora_weight, 0.1..=1.5).text("Style strength"));
            ui.label(RichText::new("The pose of a character is held exactly (a pose model) only with Stable Diffusion XL.").size(11.0).color(TEXT_DIM));
        });
    }

    // ------------------------------------------------------------------ the right side

    fn gen_assets_right(&mut self, ui: &mut Ui, now: f64) {
        let busy = self.gen_busy().is_some();
        let words = self.genr.asset.has_words();
        section_title(ui, "1  Pictures");
        if ui.add_enabled(!busy && words, egui::Button::new(RichText::new("Make pictures").strong())).help("Needs a few words about what it is").clicked() {
            self.gen_make_pictures();
        }
        if !words {
            ui.label(RichText::new("Write what it is on the left first.").size(11.5).color(GOLD));
        }
        section_title(ui, "2  3D model");
        if ui.add_enabled(!busy && self.genr.picked.is_some(), egui::Button::new(RichText::new("Make the 3D model").strong())).help("From the picked picture: a textured model of the size and the triangle count you set").clicked() {
            self.gen_make_model(now);
        }
        if ui.add_enabled(!busy && words, egui::Button::new("Make it all from the text")).help("Makes the pictures, takes the first one and makes the 3D model of it").clicked() {
            self.genr.auto = true;
            self.gen_make_pictures();
        }
        section_title(ui, "3  Save");
        let have = self.genr.glb.is_some();
        if ui.add_enabled(have, egui::Button::new(RichText::new("Save the model (.glb)").strong())).help("One file with the shape, the texture and the size. Unity, Unreal (with a plugin), Godot and Blender open it.").clicked() {
            if let Some(bytes) = &self.genr.glb {
                let name = asset_file_name(&self.genr.asset);
                if let Some(p) = rfd::FileDialog::new().set_title("Save the model").set_directory(self.settings.projects_folder()).set_file_name(format!("{name}.glb")).add_filter("glTF binary", &["glb"]).save_file() {
                    self.genr.message = Some(match std::fs::write(&p, bytes) {
                        Ok(()) => format!("Saved {}.", p.display()),
                        Err(e) => format!("Could not save the file: {e}"),
                    });
                }
            }
        }
        if ui.add_enabled(have && self.genr.mesh.as_ref().is_some_and(|m| m.texture.is_some()), egui::Button::new("Save the texture (.png)")).clicked() {
            if let Some(tex) = self.genr.mesh.as_ref().and_then(|m| m.texture.as_ref()) {
                if let Some(p) = rfd::FileDialog::new().set_title("Save the texture").set_file_name(format!("{}_color.png", asset_file_name(&self.genr.asset))).add_filter("PNG picture", &["png"]).save_file() {
                    let _ = tex.save(&p);
                }
            }
        }
        if ui.add_enabled(have && self.genr.mesh.as_ref().is_some_and(|m| m.normal.is_some()), egui::Button::new("Save the normal map (.png)")).help("The normal map that was baked from the fine shape. Use it with the colour texture in a game engine or in Blender.").clicked() {
            if let Some(n) = self.genr.mesh.as_ref().and_then(|m| m.normal.as_ref()) {
                if let Some(p) = rfd::FileDialog::new().set_title("Save the normal map").set_file_name(format!("{}_normal.png", asset_file_name(&self.genr.asset))).add_filter("PNG picture", &["png"]).save_file() {
                    let _ = n.save(&p);
                }
            }
        }
        if ui.add_enabled(have, egui::Button::new("Save the depth map of this view (.png)")).help("A grey picture of how far every point is from this side (white is near). Useful to steer a picture AI that follows depth, or to check the shape.").clicked() {
            if let Some(mesh) = &self.genr.mesh {
                if let Some(p) = rfd::FileDialog::new().set_title("Save the depth map").set_file_name(format!("{}_depth.png", asset_file_name(&self.genr.asset))).add_filter("PNG picture", &["png"]).save_file() {
                    let (yaw, pitch) = self.genr.view;
                    let _ = mesh.render_view(yaw, pitch, 1024, 1024, false, View::Depth).save(&p);
                }
            }
        }
        if ui.add_enabled(have, egui::Button::new("Save the normals of this view (.png)")).help("A picture of the direction of the surface seen from this side. Useful to steer a picture AI that follows normal maps.").clicked() {
            if let Some(mesh) = &self.genr.mesh {
                if let Some(p) = rfd::FileDialog::new().set_title("Save the normals of this view").set_file_name(format!("{}_view_normals.png", asset_file_name(&self.genr.asset))).add_filter("PNG picture", &["png"]).save_file() {
                    let (yaw, pitch) = self.genr.view;
                    let _ = mesh.render_view(yaw, pitch, 1024, 1024, false, View::Normals).save(&p);
                }
            }
        }
        if ui.add_enabled(have, egui::Button::new("Save the model (.obj with texture)")).help("For tools that do not read .glb").clicked() {
            self.gen_save_obj();
        }
        ui.add_space(8.0);
        ui.label(RichText::new("The model is made from one picture, so the back is a guess of the AI. Turn it round before you use it. The triangle count is a target: the model may have a little more or less.").size(11.0).color(TEXT_DIM));
    }

    fn gen_save_obj(&mut self) {
        let Some(mesh) = &self.genr.mesh else { return };
        let name = asset_file_name(&self.genr.asset);
        let Some(p) = rfd::FileDialog::new().set_title("Save the model").set_directory(self.settings.projects_folder()).set_file_name(format!("{name}.obj")).add_filter("OBJ model", &["obj"]).save_file() else { return };
        let obj = obj_text(mesh, &name);
        let res = std::fs::write(&p, obj).and_then(|_| {
            if let Some(t) = &mesh.texture {
                t.save(p.with_extension("png")).map_err(|e| std::io::Error::other(e.to_string()))?;
            }
            std::fs::write(p.with_extension("mtl"), format!("newmtl {name}\nKd 1 1 1\nmap_Kd {name}.png\n"))
        });
        self.genr.message = Some(match res {
            Ok(()) => format!("Saved {} with its texture next to it.", p.display()),
            Err(e) => format!("Could not save the file: {e}"),
        });
    }

    fn gen_characters_right(&mut self, ui: &mut Ui, now: f64) {
        let busy = self.gen_busy().is_some();
        let words = self.genr.look.has_words();
        section_title(ui, "1  Pictures");
        if ui.add_enabled(!busy && words, egui::Button::new(RichText::new("Make pictures").strong())).help("Pictures of the character in the standard A-pose").clicked() {
            self.gen_make_pictures();
        }
        if !words {
            ui.label(RichText::new("Write who it is on the left first.").size(11.5).color(GOLD));
        }
        section_title(ui, "2  Rigged 3D character");
        let dirty = self.is_dirty();
        if dirty {
            ui.checkbox(&mut self.genr.replace_ok, "Replace the open project").help("The character is made in a new project: the open one has changes that are not saved.");
        }
        let ready = crate::aisetup::status().shape_ready(self.rigs.game.shape_model);
        if crate::aigen::mia_ready() {
            ui.checkbox(&mut self.rigs.game.use_ai_skin, "AI skin weights").help("Make-It-Animatable looks at the 3D shape and says how every bone moves every point: wings, skirts and hair bend better than with the weights painted on the picture. About twenty seconds more.");
        }
        ui.checkbox(&mut self.genr.flat_card, "Flat card instead of an AI shape").help("No AI shape: the drawing is wrapped on a round card. Quick, but it has no depth.");
        if !ready && !self.genr.flat_card {
            ui.label(RichText::new("The AI shape files are missing (press Download the AI files at the top), or tick the flat card.").size(11.5).color(GOLD));
        }
        if ui.add_enabled(!busy && self.genr.picked.is_some(), egui::Button::new(RichText::new("Make the 3D character").strong())).help("Finds the skeleton in the picked picture, makes the 3D shape with the AI, lays the picture on it, and gives it the skeleton, the skin and the face shapes").clicked() {
            self.gen_make_character(now);
        }
        if ui.add_enabled(!busy && words, egui::Button::new("Make it all from the text")).help("Makes the pictures, then the character from the first one").clicked() {
            self.genr.message = Some("Make the pictures first, pick one, then Make the 3D character.".into());
            self.gen_make_pictures();
        }
        section_title(ui, "3  Save");
        let have = self.rigs.game.model.is_some();
        ui.horizontal(|ui| {
            ui.label(RichText::new("Name").size(12.0).color(TEXT_DIM));
            ui.add(egui::TextEdit::singleline(&mut self.rigs.game.name).desired_width(160.0));
        });
        for (fmt, label, tip) in [
            (Format::Vrm1, "Save as VRM 1.0", "For VRM apps, VTuber programs and engines with a VRM loader"),
            (Format::Vrm0, "Save as VRM 0.x", "For older VRM apps"),
            (Format::Glb, "Save as GLB for games", "A plain glTF binary with the skeleton and the face shapes"),
        ] {
            if ui.add_enabled(have, egui::Button::new(label)).help(tip).clicked() {
                self.game_save(fmt);
            }
        }
        ui.horizontal(|ui| {
            if ui.add_enabled(have, egui::Button::new("Edit in the Rig tab")).help("Fix the face, the hair, the joints and paint it by hand").clicked() {
                self.tab = super::Tab::Rig;
                self.rigs.bench.view3d = true;
            }
            if ui.add_enabled(have, egui::Button::new("Animate it")).clicked() {
                self.tab = super::Tab::Animate;
            }
        });
        if let Some(m) = &self.rigs.game.message {
            ui.label(RichText::new(m).size(11.5).color(TEXT_DIM));
        }
    }

    #[cfg(test)]
    pub fn test_tab_is_generate(&self) -> bool {
        self.tab == super::Tab::Generate
    }

    #[cfg(test)]
    pub fn test_game_glb_with(&mut self, clips: &[crate::motion::Clip], fps: f32) -> Result<(Vec<u8>, crate::vrm::export::Summary), String> {
        self.game_glb_with(clips, fps)
    }

    #[cfg(test)]
    pub fn test_gen(&mut self) -> &mut GenerateUi {
        &mut self.genr
    }

    #[cfg(test)]
    pub fn test_gen_set_pictures(&mut self, imgs: Vec<RgbaImage>) {
        self.genr.pics = imgs.into_iter().map(|img| Pic { img, tex: None }).collect();
        self.genr.picked = Some(0);
    }

    #[cfg(test)]
    pub fn test_gen_show_asset(&mut self, glb: Vec<u8>) {
        self.genr.mesh = AssetMesh::from_glb(&glb).ok();
        self.genr.glb = Some(glb);
        self.genr.made += 1;
        self.genr.view = (30.0, 12.0);
    }

    #[cfg(test)]
    pub fn test_gen_make_character(&mut self) {
        self.gen_make_character(0.0);
    }
}

fn nonempty(s: &str) -> Option<String> {
    let t = s.trim();
    (!t.is_empty()).then(|| t.to_owned())
}

/// The sliders every picture page has: how many, how detailed, how closely it follows the words, and the seed.
fn gen_knobs(ui: &mut Ui, count: &mut usize, steps: &mut u32, guidance: &mut f32, seed: &mut Option<u32>) {
    ui.horizontal(|ui| {
        ui.label(RichText::new("How many").size(12.0).color(TEXT_DIM));
        for n in [1usize, 2, 4, 6] {
            if widgets::chip(ui, &format!("{n}"), *count == n).clicked() {
                *count = n;
            }
        }
    });
    ui.add(egui::Slider::new(steps, 15..=50).text("Detail")).help("More steps: a finer picture, and it takes longer");
    ui.add(egui::Slider::new(guidance, 3.0..=12.0).text("Follows the words")).help("Higher follows the words strictly, lower is freer");
    ui.horizontal(|ui| {
        let mut keep = seed.is_some();
        if ui.checkbox(&mut keep, "Keep the seed").help("The same seed and the same words give the same pictures").changed() {
            *seed = keep.then_some(1234);
        }
        if let Some(s) = seed {
            ui.add(egui::DragValue::new(s).speed(1.0));
        }
    });
}

/// A file name from what the asset is.
pub fn asset_file_name(a: &AssetLook) -> String {
    let base = if a.what.trim().is_empty() { a.kind().label.to_lowercase() } else { a.what.trim().to_lowercase() };
    let words: Vec<String> = base.split_whitespace().take(3).map(|w| w.chars().filter(|c| c.is_alphanumeric()).collect::<String>()).filter(|w| !w.is_empty()).collect();
    if words.is_empty() { "asset".into() } else { words.join("_") }
}

/// The model as a Wavefront .obj text (points, texture coordinates, triangles) that uses `<name>.mtl`.
pub fn obj_text(m: &AssetMesh, name: &str) -> String {
    let mut s = format!("mtllib {name}.mtl\nusemtl {name}\n");
    for v in &m.verts {
        s.push_str(&format!("v {} {} {}\n", v[0], v[1], v[2]));
    }
    for t in &m.uvs {
        s.push_str(&format!("vt {} {}\n", t[0], 1.0 - t[1]));
    }
    for t in &m.tris {
        s.push_str(&format!("f {0}/{0} {1}/{1} {2}/{2}\n", t[0] + 1, t[1] + 1, t[2] + 1));
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_file_name_comes_from_what_it_is() {
        let mut a = AssetLook::default();
        assert_eq!(asset_file_name(&a), "prop");
        a.what = "An old iron lantern, with a hook!".into();
        assert_eq!(asset_file_name(&a), "an_old_iron");
    }

    #[test]
    fn the_obj_has_every_point_texture_coordinate_and_triangle() {
        let m = AssetMesh { verts: vec![[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]], uvs: vec![[0.0, 0.0], [1.0, 0.0], [0.0, 1.0]], tris: vec![[0, 1, 2]], texture: None, normal: None };
        let t = obj_text(&m, "lamp");
        assert_eq!(t.lines().filter(|l| l.starts_with("v ")).count(), 3);
        assert_eq!(t.lines().filter(|l| l.starts_with("vt ")).count(), 3);
        assert!(t.contains("f 1/1 2/2 3/3") && t.contains("mtllib lamp.mtl"));
    }
}
