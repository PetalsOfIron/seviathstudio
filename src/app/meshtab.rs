// SPDX-License-Identifier: GPL-3.0-or-later
// The 3D tab. Four pages. From a text: a text makes pictures (one object or one standing character) and the picked picture becomes a textured 3D mesh.
// From a picture or drawing: your own picture, or the drawing of the Draw tab, becomes the mesh the same way. Rig by hand: bones are drawn in the mesh and
// the weights (which bone moves which part, and how much) are painted, with the tools of a 3D program. Animations: 22 ready motions and a maker for the
// hand-rigged mesh, saved in the game file (.glb). The AI only makes pictures from a text and 3D meshes from a picture (`aigen.rs`, `ai.rs`,
// `tools/ai/`); on this computer, one job at a time. Everything else is done by hand.

use crate::widgets::HelpExt;
use std::sync::mpsc::{Receiver, TryRecvError};

use eframe::egui::{self, Color32, RichText, Sense, TextureHandle, TextureOptions, Ui, vec2};
use image::RgbaImage;

use super::SeviathApp;
use crate::ai::ShapeModel;
use crate::aigen::{AssetRequest, PictureRequest, generate_pictures, make_asset};
use crate::assetview::{AssetMesh, View};
use crate::generate::*;
use crate::theme::*;
use crate::widgets::{self, section_title};

#[derive(Clone, Copy, PartialEq, Eq, Default)]
pub enum Page {
    #[default]
    Bases,
    Edit,
    FromText,
    FromImage,
    Rig,
    Parts,
    Animate,
    Animations,
}

/// What the mesh is of: a thing (a prop, a weapon, a plant, a building) or a standing character.
#[derive(Clone, Copy, PartialEq, Eq, Default, Debug)]
pub enum Kind {
    #[default]
    Prop,
    Character,
}

struct Pic {
    img: RgbaImage,
    tex: Option<TextureHandle>,
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
type MeshJob = Receiver<Result<Vec<u8>, String>>;

pub struct MeshUi {
    pub page: Page,
    pub kind: Kind,
    pub look: Look,
    pub asset: AssetLook,
    pub own: Own,
    pub anim: super::animpage::AnimState,
    pub hand: super::handrig::HandRig,
    pub setup: super::aisetupui::AiSetup,
    pub bases: super::basestab::BasesUi,
    pics: Vec<Pic>,
    pub picked: Option<usize>,
    pic_job: Option<PicJob>,
    pub seed_used: Option<u32>,
    mesh_job: Option<MeshJob>,
    pub glb: Option<Vec<u8>>,
    pub mesh: Option<AssetMesh>,
    /// How the mesh is turned (yaw, pitch) and whether the triangles show.
    pub view: (f32, f32),
    pub wire: bool,
    /// Colours, the directions of the surface, or depth.
    pub view_mode: View,
    view_tex: Option<((i32, i32, bool, u64, View), TextureHandle)>,
    /// Counts the meshes that were made (a new one starts a new rig).
    pub made: u64,
    pub message: Option<String>,
    /// Pictures, then the mesh, one after the other without a click in between.
    auto: bool,
    /// What is installed (the base AI files, the picture AI, CharacterGen), and when it was looked at (the folders are not read every frame).
    ready: Option<(f64, [bool; 3])>,
    /// A character's height in metres, and the AI that makes its shape.
    pub char_height: f32,
    pub shape_model: ShapeModel,
    /// The command that starts Python with the AI packages.
    pub python: String,
}

impl Default for MeshUi {
    fn default() -> Self {
        MeshUi {
            page: Page::default(),
            kind: Kind::default(),
            look: Look::default(),
            asset: AssetLook::default(),
            own: Own::default(),
            anim: Default::default(),
            hand: Default::default(),
            setup: Default::default(),
            bases: Default::default(),
            pics: Vec::new(),
            picked: None,
            pic_job: None,
            seed_used: None,
            mesh_job: None,
            glb: None,
            mesh: None,
            view: (0.0, 0.0),
            wire: false,
            view_mode: View::default(),
            view_tex: None,
            made: 0,
            message: None,
            auto: false,
            ready: None,
            char_height: 1.7,
            shape_model: if crate::aisetup::status().charactergen { ShapeModel::CharacterGen } else { ShapeModel::TripoSr },
            python: super::aisetupui::default_python(),
        }
    }
}

impl MeshUi {
    /// The name of the files that are saved: from the words, or what kind of thing it is.
    pub fn file_name(&self) -> String {
        match self.kind {
            Kind::Prop => asset_file_name(&self.asset),
            Kind::Character => {
                let words: Vec<String> = self.look.who.split_whitespace().take(3).map(|w| w.chars().filter(|c| c.is_alphanumeric()).collect::<String>()).filter(|w| !w.is_empty()).collect();
                if words.is_empty() { "character".into() } else { words.join("_").to_lowercase() }
            }
        }
    }

    /// The picked picture.
    fn picked_picture(&self) -> Option<RgbaImage> {
        self.picked.and_then(|i| self.pics.get(i)).map(|p| p.img.clone())
    }

    /// A mesh arrived (made, or opened from a file): it is what the pages show, and the rig starts again for it.
    pub fn set_mesh(&mut self, mesh: AssetMesh, glb: Vec<u8>) {
        self.hand.start_for(&mesh);
        self.mesh = Some(mesh);
        self.glb = Some(glb);
        self.made += 1;
        self.view = (0.0, 12.0);
    }
}

impl SeviathApp {
    pub(super) fn mesh_ui(&mut self, ui: &mut Ui) {
        egui::Panel::top("mesh_top").frame(egui::Frame::new().fill(PANEL).inner_margin(egui::Margin::same(8))).show(ui, |ui| self.mesh_top(ui));
        if matches!(self.m3.page, Page::Bases | Page::Edit | Page::Rig | Page::Parts | Page::Animate) {
            self.mesh_rig_ui(ui);
            return;
        }
        egui::Panel::left("mesh_left").default_size(330.0).size_range(260.0..=520.0).resizable(true).frame(egui::Frame::new().fill(PANEL).inner_margin(egui::Margin::same(10))).show(ui, |ui| {
            egui::ScrollArea::vertical().id_salt("mesh_left_scroll").show(ui, |ui| match self.m3.page {
                Page::FromText => self.mesh_text_left(ui),
                Page::FromImage => self.mesh_image_left(ui),
                _ => self.mesh_animations_left(ui),
            });
        });
        egui::Panel::right("mesh_right").default_size(310.0).size_range(250.0..=480.0).resizable(true).frame(egui::Frame::new().fill(PANEL).inner_margin(egui::Margin::same(10))).show(ui, |ui| {
            egui::ScrollArea::vertical().id_salt("mesh_right_scroll").show(ui, |ui| match self.m3.page {
                Page::FromText | Page::FromImage => self.mesh_make_and_save(ui),
                _ => self.mesh_animations_right(ui),
            });
        });
        egui::CentralPanel::default().frame(egui::Frame::new().fill(Color32::from_rgb(14, 20, 18)).inner_margin(egui::Margin::same(10))).show(ui, |ui| {
            egui::ScrollArea::vertical().id_salt("mesh_center_scroll").show(ui, |ui| {
                if self.m3.page != Page::Animations {
                    self.mesh_pictures_ui(ui);
                    ui.add_space(8.0);
                    self.mesh_view_ui(ui);
                } else {
                    self.mesh_animations_center(ui);
                }
            });
        });
    }

    fn mesh_top(&mut self, ui: &mut Ui) {
        ui.horizontal_wrapped(|ui| {
            ui.label(RichText::new("3D").size(16.0).strong());
            ui.add_space(8.0);
            for (p, label, tip) in [
                (Page::Bases, "Bases", "The ready bases of the races: pick one, make the other sex, change its looks and paint it"),
                (Page::Edit, "Redraw and sculpt", "Redraw, sculpt, cut and add parts to the mesh that is open (made from a text, a picture or a drawing, or a base), like in Blender"),
                (Page::FromText, "From a text", "Write what you want: pictures are made, and the picture you pick becomes a 3D mesh"),
                (Page::FromImage, "From a picture or drawing", "Your own picture, or your drawing from the Draw tab, becomes a 3D mesh"),
                (Page::Rig, "Bones and weights", "Draw the bones in the mesh and paint how much each bone moves every part, by hand"),
                (Page::Parts, "Parts (no weights)", "The simple way to rig: draw the bones, then say which part of the mesh each bone moves, by painting or by a loop"),
                (Page::Animate, "Animate by hand", "Turn the bones frame by frame, keep the keys and play the animation on the mesh"),
                (Page::Animations, "Ready animations", "22 ready animations and an animation maker for the rigged mesh, saved in the game file"),
            ] {
                if widgets::chip(ui, label, self.m3.page == p).help(tip).clicked() {
                    self.m3.page = p;
                }
            }
            ui.add_space(12.0);
            if let Some(what) = self.mesh_busy() {
                ui.spinner();
                ui.label(RichText::new(what).size(12.0).color(GOLD));
            } else if let Some(m) = &self.m3.message {
                ui.label(RichText::new(m).size(12.0).color(TEXT_DIM));
            }
        });
        if matches!(self.m3.page, Page::FromText | Page::FromImage) {
            // what is installed, and the way to get the rest with one button
            let now = ui.input(|i| i.time);
            if self.m3.ready.is_none_or(|(t, _)| now - t > 3.0) {
                let st = crate::aisetup::status();
                self.m3.ready = Some((now, [st.ready(), crate::setupall::picture_ready(), st.charactergen]));
            }
            let ready = self.m3.ready.map(|r| r.1).unwrap_or([false; 3]);
            let text_page = self.m3.page == Page::FromText;
            ui.horizontal_wrapped(|ui| {
                for (ok, name, tip, needed) in [
                    (ready[0], "AI files", "Python, PyTorch and TripoSR: they make the mesh of a thing.", true),
                    (ready[1], "Picture AI", "Stable Diffusion XL and its pose model: they make the pictures from a text.", text_page),
                    (ready[2], "CharacterGen", "The better AI for the mesh of a character.", true),
                ] {
                    if !needed {
                        continue;
                    }
                    let col = if ok { EMERALD } else { GOLD };
                    ui.label(RichText::new(format!("{} {name}", if ok { "Ready:" } else { "Missing:" })).size(11.5).color(col)).on_hover_text(tip);
                }
                if ready.iter().any(|r| !*r) && ui.small_button("Get everything").help("Opens the Downloads window: the first card there downloads and installs all of it with one button (about 45 GB, free). It also shows how much each part is.").clicked() {
                    self.open_downloads();
                }
            });
            self.ai_setup_ui(ui);
        }
    }

    /// What is being made now, in words (None when nothing runs).
    pub(super) fn mesh_busy(&self) -> Option<String> {
        if self.m3.pic_job.is_some() {
            Some("Making pictures... the first time the AI models are downloaded (about 10 GB), after that it takes a minute or two.".into())
        } else if self.m3.mesh_job.is_some() {
            Some(match (self.m3.kind, self.m3.shape_model) {
                (Kind::Character, ShapeModel::CharacterGen) => "Making the 3D mesh of the character... about seven minutes. Close games and other programs that use the graphics card.".into(),
                _ => "Making the 3D mesh... about a minute.".into(),
            })
        } else {
            None
        }
    }

    // ------------------------------------------------------------------ jobs

    /// Takes the answers of the AI jobs, whichever tab is open (so a job that was started is not left waiting when you look at another tab).
    pub(super) fn mesh_poll(&mut self, ctx: &egui::Context) {
        self.bases_poll(ctx);
        let pics = self.m3.pic_job.as_ref().map(|rx| rx.try_recv());
        match pics {
            Some(Ok(Ok((imgs, seed)))) => {
                self.m3.pic_job = None;
                let n = imgs.len();
                self.m3.pics = imgs.into_iter().map(|img| Pic { img, tex: None }).collect();
                self.m3.picked = Some(0);
                self.m3.seed_used = Some(seed);
                self.m3.message = Some(format!("{n} pictures made (the first seed was {seed}). Click the one you like."));
                if std::mem::take(&mut self.m3.auto) {
                    self.mesh_make_model();
                }
            }
            Some(Ok(Err(e))) => {
                self.m3.pic_job = None;
                self.m3.auto = false;
                self.m3.message = Some(format!("The AI could not make the pictures: {e}"));
            }
            Some(Err(TryRecvError::Empty)) => ctx.request_repaint_after(std::time::Duration::from_millis(600)),
            Some(Err(TryRecvError::Disconnected)) => {
                self.m3.pic_job = None;
                self.m3.auto = false;
            }
            None => {}
        }
        let job = self.m3.mesh_job.as_ref().map(|rx| rx.try_recv());
        match job {
            Some(Ok(Ok(bytes))) => {
                self.m3.mesh_job = None;
                match AssetMesh::from_glb(&bytes) {
                    Ok(m) => {
                        let s = m.stats();
                        self.m3.message = Some(format!("The 3D mesh is made: {} triangles, {:.2} x {:.2} x {:.2} m. Rig it by hand on the Rig by hand page.", s.triangles, s.size[0], s.size[1], s.size[2]));
                        self.m3.set_mesh(m, bytes);
                    }
                    Err(e) => self.m3.message = Some(format!("The mesh could not be read: {e}")),
                }
            }
            Some(Ok(Err(e))) => {
                self.m3.mesh_job = None;
                self.m3.message = Some(format!("The AI could not make the 3D mesh: {e}"));
            }
            Some(Err(TryRecvError::Empty)) => ctx.request_repaint_after(std::time::Duration::from_millis(600)),
            Some(Err(TryRecvError::Disconnected)) => self.m3.mesh_job = None,
            None => {}
        }
    }

    fn mesh_make_pictures(&mut self) {
        if self.mesh_busy().is_some() {
            return;
        }
        let g = &self.m3;
        let req = match g.kind {
            Kind::Prop => {
                let (prompt, negative) = g.asset.prompt();
                PictureRequest { prompt, negative, count: g.asset.count, seed: g.asset.seed, steps: g.asset.steps, guidance: g.asset.guidance, size: (1024, 1024), pose: "none", checkpoint: nonempty(&g.own.checkpoint), lora: nonempty(&g.own.lora).map(|f| (f, g.own.lora_weight)) }
            }
            Kind::Character => {
                let (prompt, negative) = g.look.prompt();
                PictureRequest { prompt, negative, count: g.look.count, seed: g.look.seed, steps: g.look.steps, guidance: g.look.guidance, size: g.look.size_px(), pose: "a-pose", checkpoint: nonempty(&g.own.checkpoint), lora: nonempty(&g.own.lora).map(|f| (f, g.own.lora_weight)) }
            }
        };
        let python = self.m3.python.clone();
        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let _ = tx.send(generate_pictures(&python, &req));
        });
        self.m3.pic_job = Some(rx);
        self.m3.message = None;
    }

    /// The picked picture becomes the mesh (a thing with TripoSR, a character with CharacterGen or TripoSR), in the background.
    fn mesh_make_model(&mut self) {
        let Some(img) = self.m3.picked_picture() else {
            self.m3.message = Some("Pick a picture first.".into());
            return;
        };
        if self.mesh_busy().is_some() {
            return;
        }
        let a = &self.m3.asset;
        let (kind, model) = (self.m3.kind, self.m3.shape_model);
        let height = if kind == Kind::Character { self.m3.char_height } else { a.height_m };
        let req = AssetRequest { height_m: height, triangles: a.triangles, texture: a.texture, detail: a.detail, pivot_bottom: kind == Kind::Character || a.pivot_bottom, normal_map: a.normal_map };
        let python = self.m3.python.clone();
        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let _ = tx.send(if kind == Kind::Character && model == ShapeModel::CharacterGen { character_glb(&python, &img, height) } else { make_asset(&python, &img, &req) });
        });
        self.m3.mesh_job = Some(rx);
        self.m3.message = None;
    }

    // ------------------------------------------------------------------ the middle

    fn mesh_pictures_ui(&mut self, ui: &mut Ui) {
        section_title(ui, if self.m3.page == Page::FromText { "Pictures" } else { "Your picture" });
        if self.m3.pics.is_empty() {
            let hint = if self.m3.page == Page::FromText { "Write what you want on the left and press Make pictures on the right. Or use a picture you have." } else { "Open a picture on the left, or use your drawing from the Draw tab." };
            ui.label(RichText::new(hint).size(12.0).color(TEXT_DIM));
        }
        let (w, h) = if self.m3.kind == Kind::Prop { (170.0, 170.0) } else { (130.0, 190.0) };
        ui.horizontal_wrapped(|ui| {
            for i in 0..self.m3.pics.len() {
                let on = self.m3.picked == Some(i);
                if self.m3.pics[i].tex.is_none() {
                    let p = &self.m3.pics[i].img;
                    let small = image::imageops::thumbnail(p, (p.width() / 2).max(1), (p.height() / 2).max(1));
                    let color = egui::ColorImage::from_rgba_unmultiplied([small.width() as usize, small.height() as usize], small.as_raw());
                    self.m3.pics[i].tex = Some(ui.ctx().load_texture(format!("mesh_pic_{i}"), color, TextureOptions::LINEAR));
                }
                let tex = self.m3.pics[i].tex.as_ref().unwrap();
                let r = ui.add(egui::Image::new((tex.id(), vec2(w, h))).fit_to_exact_size(vec2(w, h)).corner_radius(6.0).sense(Sense::click()));
                if on {
                    ui.painter().rect_stroke(r.rect, 6.0, egui::Stroke::new(2.5, EMERALD), egui::StrokeKind::Outside);
                }
                if r.clicked() {
                    self.m3.picked = Some(i);
                }
            }
        });
        if let (Some(i), true) = (self.m3.picked, !self.m3.pics.is_empty()) {
            if ui.button("Save the picked picture...").help("Save the picked picture as a PNG file.").clicked() {
                if let Some(p) = rfd::FileDialog::new().set_title("Save the picture").set_file_name("picture.png").add_filter("PNG picture", &["png"]).save_file() {
                    let _ = self.m3.pics[i].img.save(&p);
                }
            }
        }
    }

    fn mesh_view_ui(&mut self, ui: &mut Ui) {
        section_title(ui, "3D mesh");
        let Some(mesh) = &self.m3.mesh else {
            ui.label(RichText::new("The mesh shows here when it is made.").size(12.0).color(TEXT_DIM));
            return;
        };
        let side = ui.available_width().clamp(320.0, 560.0).min(560.0);
        let (yaw, pitch) = self.m3.view;
        let key = (yaw.round() as i32, pitch.round() as i32, self.m3.wire, self.m3.made, self.m3.view_mode);
        if self.m3.view_tex.as_ref().map(|t| t.0) != Some(key) {
            let n = side as u32 / 4 * 4;
            let img = mesh.render_view(yaw, pitch, n, n, self.m3.wire, self.m3.view_mode);
            let color = egui::ColorImage::from_rgba_unmultiplied([img.width() as usize, img.height() as usize], img.as_raw());
            self.m3.view_tex = Some((key, ui.ctx().load_texture("mesh_view", color, TextureOptions::LINEAR)));
        }
        let stats = mesh.stats();
        if let Some((_, tex)) = &self.m3.view_tex {
            let resp = ui.add(egui::Image::new((tex.id(), vec2(side, side))).corner_radius(6.0).sense(Sense::drag()));
            if resp.dragged() {
                let d = resp.drag_delta();
                self.m3.view.0 -= d.x * 0.6;
                self.m3.view.1 = (self.m3.view.1 + d.y * 0.4).clamp(-89.0, 89.0);
            }
            resp.help("Drag to turn it");
        }
        ui.horizontal_wrapped(|ui| {
            for (v, label, tip) in [(View::Textured, "Colours", "The mesh with its texture."), (View::Normals, "Normals", "The direction of the surface in colours: red to the right, green up, blue to the viewer."), (View::Depth, "Depth", "How far every point is: white is near, black is far.")] {
                if widgets::chip(ui, label, self.m3.view_mode == v).help(tip).clicked() {
                    self.m3.view_mode = v;
                }
            }
        });
        ui.horizontal_wrapped(|ui| {
            ui.checkbox(&mut self.m3.wire, "Show the triangles").help("Draw the triangle edges of the mesh over it.");
            for (label, tip, to) in [("Front", "Look at it from the front.", (0.0, 0.0)), ("Side", "Look at it from the side.", (90.0, 0.0)), ("Back", "Look at it from behind.", (180.0, 0.0)), ("Top", "Look at it from above.", (0.0, 89.0))] {
                if ui.small_button(label).help(tip).clicked() {
                    self.m3.view = to;
                }
            }
        });
        let over = self.m3.kind == Kind::Prop && stats.triangles as u32 > self.m3.asset.triangles + self.m3.asset.triangles / 10;
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

    // ------------------------------------------------------------------ the left side

    fn mesh_text(ui: &mut Ui, label: &str, hint: &str, text: &mut String, rows: usize) {
        ui.label(RichText::new(label).size(12.0).color(TEXT_DIM));
        ui.add(egui::TextEdit::multiline(text).desired_rows(rows).desired_width(f32::INFINITY).hint_text(hint));
    }

    /// A thing, or a character.
    fn mesh_kind_row(&mut self, ui: &mut Ui) {
        ui.horizontal_wrapped(|ui| {
            for (k, label, tip) in [(Kind::Prop, "A thing", "A prop, a weapon, a plant, a building: a textured mesh for a game engine"), (Kind::Character, "A character", "One standing character, in the standard A-pose, to rig by hand")] {
                if widgets::chip(ui, label, self.m3.kind == k).help(tip).clicked() {
                    self.m3.kind = k;
                }
            }
        });
    }

    fn mesh_text_left(&mut self, ui: &mut Ui) {
        section_title(ui, "What do you want");
        self.mesh_kind_row(ui);
        ui.add_space(4.0);
        match self.m3.kind {
            Kind::Prop => self.mesh_prop_words(ui),
            Kind::Character => self.mesh_character_words(ui),
        }
        ui.add_space(6.0);
        section_title(ui, "Pictures");
        match self.m3.kind {
            Kind::Prop => {
                let a = &mut self.m3.asset;
                mesh_knobs(ui, &mut a.count, &mut a.steps, &mut a.guidance, &mut a.seed);
            }
            Kind::Character => {
                ui.horizontal_wrapped(|ui| {
                    for (i, s) in SIZES.iter().enumerate() {
                        if widgets::chip(ui, s.0, self.m3.look.size == i).help(format!("{} x {}", s.1, s.2)).clicked() {
                            self.m3.look.size = i;
                        }
                    }
                });
                let l = &mut self.m3.look;
                mesh_knobs(ui, &mut l.count, &mut l.steps, &mut l.guidance, &mut l.seed);
            }
        }
        ui.add_space(6.0);
        let (p, n) = if self.m3.kind == Kind::Prop { self.m3.asset.prompt() } else { self.m3.look.prompt() };
        self.mesh_own_and_prompt(ui, &p, &n);
        ui.add_space(6.0);
        self.mesh_settings_ui(ui);
    }

    fn mesh_prop_words(&mut self, ui: &mut Ui) {
        ui.label(RichText::new("Kind").size(12.0).color(TEXT_DIM));
        ui.horizontal_wrapped(|ui| {
            for (i, k) in ASSET_KINDS.iter().enumerate() {
                if widgets::chip(ui, k.label, self.m3.asset.kind == i).help(format!("Usual height {} m", k.height_m)).clicked() {
                    self.m3.asset.set_kind(i);
                }
            }
        });
        ui.label(RichText::new("Style").size(12.0).color(TEXT_DIM));
        ui.horizontal_wrapped(|ui| {
            for (i, s) in ASSET_STYLES.iter().enumerate() {
                if widgets::chip(ui, s.label, self.m3.asset.style == i).help(s.hint).clicked() {
                    self.m3.asset.style = i;
                }
            }
        });
        Self::mesh_text(ui, "What is it", "an old iron lantern with a glass and a hook", &mut self.m3.asset.what, 3);
        Self::mesh_text(ui, "Colors, materials, details", "rusty dark metal, warm orange glow, small chains", &mut self.m3.asset.details, 2);
        ui.label(RichText::new("Keep out").size(12.0).color(TEXT_DIM));
        ui.add(egui::TextEdit::singleline(&mut self.m3.asset.avoid).desired_width(f32::INFINITY).hint_text("things that must not be there"));
    }

    fn mesh_character_words(&mut self, ui: &mut Ui) {
        ui.label(RichText::new("Style").size(12.0).color(TEXT_DIM));
        ui.horizontal_wrapped(|ui| {
            for (i, s) in STYLES.iter().enumerate() {
                if widgets::chip(ui, s.label, self.m3.look.style == i).help(s.hint).clicked() {
                    self.m3.look.style = i;
                }
            }
        });
        Self::mesh_text(ui, "Who", "a girl with long pink hair and green eyes", &mut self.m3.look.who, 2);
        Self::mesh_text(ui, "Outfit", "a green hooded coat, white trousers, boots", &mut self.m3.look.outfit, 2);
        Self::mesh_text(ui, "Body", "slim and tall", &mut self.m3.look.body, 1);
        Self::mesh_text(ui, "More", "a calm look, a small bag", &mut self.m3.look.more, 2);
        ui.label(RichText::new("Keep out").size(12.0).color(TEXT_DIM));
        ui.add(egui::TextEdit::singleline(&mut self.m3.look.avoid).desired_width(f32::INFINITY).hint_text("things that must not be there"));
        ui.label(RichText::new("The pose is always the standard A-pose, seen from the front, so the character is easy to rig.").size(11.5).color(TEXT_DIM));
    }

    fn mesh_image_left(&mut self, ui: &mut Ui) {
        section_title(ui, "Your picture or drawing");
        ui.label(RichText::new("One object, or one standing character seen from the front, on a plain background, gives the best mesh.").size(11.5).color(TEXT_DIM));
        if ui.button("Open a picture...").help("A picture file from this computer (png, jpg or webp)").clicked() {
            if let Some(p) = rfd::FileDialog::new().set_title("A picture").add_filter("Pictures", &["png", "jpg", "jpeg", "webp"]).pick_file() {
                match image::open(&p) {
                    Ok(i) => {
                        self.m3.pics = vec![Pic { img: i.to_rgba8(), tex: None }];
                        self.m3.picked = Some(0);
                        self.m3.message = Some(format!("Using {}.", p.display()));
                    }
                    Err(e) => self.m3.message = Some(format!("The picture could not be opened: {e}")),
                }
            }
        }
        if ui.button("Use my drawing from the Draw tab").help("The drawing on the canvas, all its visible layers on the paper, becomes the picture").clicked() {
            let img = crate::files::flatten(&self.draw.doc);
            self.m3.pics = vec![Pic { img, tex: None }];
            self.m3.picked = Some(0);
            self.m3.message = Some("Using the drawing of the Draw tab.".into());
        }
        ui.add_space(6.0);
        section_title(ui, "What is it");
        self.mesh_kind_row(ui);
        ui.add_space(6.0);
        self.mesh_settings_ui(ui);
    }

    /// The size and the quality of the mesh.
    fn mesh_settings_ui(&mut self, ui: &mut Ui) {
        section_title(ui, "3D mesh");
        match self.m3.kind {
            Kind::Character => {
                ui.horizontal(|ui| {
                    ui.label(RichText::new("Height").size(12.0).color(TEXT_DIM));
                    ui.add(egui::DragValue::new(&mut self.m3.char_height).range(0.2..=5.0).speed(0.01).suffix(" m"));
                });
                ui.label(RichText::new("Mesh AI").size(12.0).color(TEXT_DIM));
                ui.horizontal_wrapped(|ui| {
                    if widgets::chip(ui, "CharacterGen", self.m3.shape_model == ShapeModel::CharacterGen).help("Apache 2.0 licence. The better mesh for a character, with its colours. About seven minutes, and it needs the graphics card free: close games first.").clicked() {
                        self.m3.shape_model = ShapeModel::CharacterGen;
                    }
                    if widgets::chip(ui, "TripoSR", self.m3.shape_model == ShapeModel::TripoSr).help("MIT licence. A rougher mesh in about a minute.").clicked() {
                        self.m3.shape_model = ShapeModel::TripoSr;
                    }
                });
            }
            Kind::Prop => {
                ui.horizontal(|ui| {
                    ui.label(RichText::new("Height").size(12.0).color(TEXT_DIM));
                    ui.add(egui::DragValue::new(&mut self.m3.asset.height_m).range(0.02..=100.0).speed(0.02).suffix(" m"));
                });
            }
        }
        if self.m3.kind == Kind::Character && self.m3.shape_model == ShapeModel::CharacterGen {
            ui.label(RichText::new("CharacterGen makes its own triangles and colours.").size(11.5).color(TEXT_DIM));
            return;
        }
        let a = &mut self.m3.asset;
        ui.label(RichText::new("Triangles the game can afford").size(12.0).color(TEXT_DIM));
        ui.horizontal_wrapped(|ui| {
            for (n, label) in [(1000, "1 000"), (3000, "3 000"), (8000, "8 000"), (20000, "20 000"), (50000, "50 000")] {
                if widgets::chip(ui, label, a.triangles == n).help("How many triangles the game can afford for this mesh.").clicked() {
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
        if self.m3.kind == Kind::Prop {
            ui.checkbox(&mut self.m3.asset.pivot_bottom, "Hold it at the middle of its bottom").help("The mesh's zero point is where it touches the ground, so it stands on the floor in the game. Off: its middle.");
        }
    }

    /// Your own picture model and style file, and the text the AI is given.
    fn mesh_own_and_prompt(&mut self, ui: &mut Ui, prompt: &str, negative: &str) {
        egui::CollapsingHeader::new(RichText::new("The text the AI gets").size(12.5).strong()).id_salt("mesh_prompt").default_open(false).show(ui, |ui| {
            ui.label(RichText::new(prompt).size(11.0).color(TEXT_DIM));
            ui.label(RichText::new(format!("Not wanted: {negative}")).size(11.0).color(TEXT_DIM));
        });
        egui::CollapsingHeader::new(RichText::new("Your own picture model").size(12.5).strong()).id_salt("mesh_own").default_open(false).show(ui, |ui| {
            ui.label(RichText::new("Stable Diffusion XL (free to use, also for work) makes the pictures. A model of your own that is free to use for what you make can replace it: pick its .safetensors file. A style file (LoRA) adds a look.").size(11.0).color(TEXT_DIM));
            for (label, which) in [("Model file", 0), ("Style file", 1)] {
                ui.horizontal(|ui| {
                    ui.label(RichText::new(label).size(12.0).color(TEXT_DIM));
                    let text = if which == 0 { &mut self.m3.own.checkpoint } else { &mut self.m3.own.lora };
                    ui.add(egui::TextEdit::singleline(text).desired_width(150.0));
                    if ui.small_button("Pick...").help("Choose a file.").clicked() {
                        if let Some(p) = rfd::FileDialog::new().add_filter("Model files", &["safetensors"]).pick_file() {
                            *text = p.to_string_lossy().to_string();
                        }
                    }
                });
            }
            ui.add(egui::Slider::new(&mut self.m3.own.lora_weight, 0.1..=1.5).text("Style strength"));
            ui.label(RichText::new("The pose of a character is held exactly (a pose model) only with Stable Diffusion XL.").size(11.0).color(TEXT_DIM));
        });
    }

    // ------------------------------------------------------------------ the right side

    fn mesh_make_and_save(&mut self, ui: &mut Ui) {
        let busy = self.mesh_busy().is_some();
        let text_page = self.m3.page == Page::FromText;
        if text_page {
            let words = if self.m3.kind == Kind::Prop { self.m3.asset.has_words() } else { self.m3.look.has_words() };
            section_title(ui, "1  Pictures");
            if ui.add_enabled(!busy && words, egui::Button::new(RichText::new("Make pictures").strong())).help("Needs a few words about what it is").clicked() {
                self.mesh_make_pictures();
            }
            if !words {
                ui.label(RichText::new("Write what it is on the left first.").size(11.5).color(GOLD));
            }
            section_title(ui, "2  3D mesh");
            if ui.add_enabled(!busy && self.m3.picked.is_some(), egui::Button::new(RichText::new("Make the 3D mesh").strong())).help("From the picked picture").clicked() {
                self.mesh_make_model();
            }
            if ui.add_enabled(!busy && words, egui::Button::new("Make it all from the text")).help("Makes the pictures, takes the first one and makes the 3D mesh of it").clicked() {
                self.m3.auto = true;
                self.mesh_make_pictures();
            }
            section_title(ui, "3  Save");
        } else {
            section_title(ui, "1  3D mesh");
            if ui.add_enabled(!busy && self.m3.picked.is_some(), egui::Button::new(RichText::new("Make the 3D mesh").strong())).help("From your picture or drawing").clicked() {
                self.mesh_make_model();
            }
            section_title(ui, "2  Save");
        }
        let have = self.m3.glb.is_some();
        if ui.add_enabled(have, egui::Button::new(RichText::new("Save the mesh (.glb)").strong())).help("One file with the shape, the texture and the size. Unity, Unreal (with a plugin), Godot and Blender open it.").clicked() {
            let made = self.m3.mesh.as_ref().filter(|_| self.m3.hand.painted).map(|m| crate::meshfile::write_glb(m, None, &[], 30.0));
            if let Some(bytes) = made.as_ref().or(self.m3.glb.as_ref()) {
                let name = self.m3.file_name();
                if let Some(p) = rfd::FileDialog::new().set_title("Save the mesh").set_directory(self.settings.projects_folder()).set_file_name(format!("{name}.glb")).add_filter("glTF binary", &["glb"]).save_file() {
                    self.m3.message = Some(match std::fs::write(&p, bytes) {
                        Ok(()) => format!("Saved {}.", p.display()),
                        Err(e) => format!("Could not save the file: {e}"),
                    });
                }
            }
        }
        if ui.add_enabled(have, egui::Button::new(RichText::new("Rig it by hand").strong())).help("Go to the Rig by hand page with this mesh").clicked() {
            self.m3.page = Page::Rig;
        }
        if ui.add_enabled(have && self.m3.mesh.as_ref().is_some_and(|m| m.texture.is_some()), egui::Button::new("Save the texture (.png)")).clicked() {
            if let Some(tex) = self.m3.mesh.as_ref().and_then(|m| m.texture.as_ref()) {
                if let Some(p) = rfd::FileDialog::new().set_title("Save the texture").set_file_name(format!("{}_color.png", self.m3.file_name())).add_filter("PNG picture", &["png"]).save_file() {
                    let _ = tex.save(&p);
                }
            }
        }
        if ui.add_enabled(have && self.m3.mesh.as_ref().is_some_and(|m| m.normal.is_some()), egui::Button::new("Save the normal map (.png)")).help("The normal map that was baked from the fine shape. Use it with the colour texture in a game engine or in Blender.").clicked() {
            if let Some(n) = self.m3.mesh.as_ref().and_then(|m| m.normal.as_ref()) {
                if let Some(p) = rfd::FileDialog::new().set_title("Save the normal map").set_file_name(format!("{}_normal.png", self.m3.file_name())).add_filter("PNG picture", &["png"]).save_file() {
                    let _ = n.save(&p);
                }
            }
        }
        for (label, tip, view, suffix) in [
            ("Save the depth map of this view (.png)", "A grey picture of how far every point is from this side (white is near). Useful to check the shape.", View::Depth, "depth"),
            ("Save the normals of this view (.png)", "A picture of the direction of the surface seen from this side.", View::Normals, "view_normals"),
        ] {
            if ui.add_enabled(have, egui::Button::new(label)).help(tip).clicked() {
                if let Some(mesh) = &self.m3.mesh {
                    if let Some(p) = rfd::FileDialog::new().set_title(label).set_file_name(format!("{}_{suffix}.png", self.m3.file_name())).add_filter("PNG picture", &["png"]).save_file() {
                        let (yaw, pitch) = self.m3.view;
                        let _ = mesh.render_view(yaw, pitch, 1024, 1024, false, view).save(&p);
                    }
                }
            }
        }
        if ui.add_enabled(have, egui::Button::new("Save the mesh (.obj with texture)")).help("For tools that do not read .glb").clicked() {
            self.mesh_save_obj();
        }
        ui.add_space(8.0);
        ui.label(RichText::new("The mesh is made from one picture, so the back is a guess of the AI. Turn it round before you use it. The triangle count is a target: the mesh may have a little more or less.").size(11.0).color(TEXT_DIM));
    }

    fn mesh_save_obj(&mut self) {
        let Some(mesh) = &self.m3.mesh else { return };
        let name = self.m3.file_name();
        let Some(p) = rfd::FileDialog::new().set_title("Save the mesh").set_directory(self.settings.projects_folder()).set_file_name(format!("{name}.obj")).add_filter("OBJ model", &["obj"]).save_file() else { return };
        let obj = obj_text(mesh, &name);
        let res = std::fs::write(&p, obj).and_then(|_| {
            if let Some(t) = &mesh.texture {
                t.save(p.with_extension("png")).map_err(|e| std::io::Error::other(e.to_string()))?;
            }
            std::fs::write(p.with_extension("mtl"), format!("newmtl {name}\nKd 1 1 1\nmap_Kd {name}.png\n"))
        });
        self.m3.message = Some(match res {
            Ok(()) => format!("Saved {} with its texture next to it.", p.display()),
            Err(e) => format!("Could not save the file: {e}"),
        });
    }

    #[cfg(test)]
    pub fn test_tab_is_mesh3d(&self) -> bool {
        self.tab == super::Tab::Mesh3d
    }

    #[cfg(test)]
    pub fn test_mesh(&mut self) -> &mut MeshUi {
        &mut self.m3
    }

    #[cfg(test)]
    pub fn test_mesh_set_pictures(&mut self, imgs: Vec<RgbaImage>) {
        self.m3.pics = imgs.into_iter().map(|img| Pic { img, tex: None }).collect();
        self.m3.picked = Some(0);
    }

    #[cfg(test)]
    pub fn test_mesh_show(&mut self, glb: Vec<u8>) {
        if let Ok(m) = AssetMesh::from_glb(&glb) {
            self.m3.set_mesh(m, glb);
        }
    }
}

/// The character made by CharacterGen from a picture: its mesh, standing on the floor at the height asked for, as a .glb.
fn character_glb(python: &str, picture: &RgbaImage, height_m: f32) -> Result<Vec<u8>, String> {
    let bytes = crate::ai::make_shape(python, picture, ShapeModel::CharacterGen)?;
    let mut mesh = AssetMesh::from_glb(&bytes)?;
    mesh.stand_on_floor(height_m);
    Ok(crate::meshfile::write_glb(&mesh, None, &[], 30.0))
}

fn nonempty(s: &str) -> Option<String> {
    let t = s.trim();
    (!t.is_empty()).then(|| t.to_owned())
}

/// The sliders every picture page has: how many, how detailed, how closely it follows the words, and the seed.
fn mesh_knobs(ui: &mut Ui, count: &mut usize, steps: &mut u32, guidance: &mut f32, seed: &mut Option<u32>) {
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

/// The mesh as a Wavefront .obj text (points, texture coordinates, triangles) that uses `<name>.mtl`.
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
        let mut ui = MeshUi::default();
        ui.kind = Kind::Character;
        assert_eq!(ui.file_name(), "character");
        ui.look.who = "A girl, with long pink hair".into();
        assert_eq!(ui.file_name(), "a_girl_with");
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
