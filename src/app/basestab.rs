// SPDX-License-Identifier: GPL-3.0-or-later
// The Bases page of the 3D tab: the ready bases of the races (a folder of .glb files that are only points and triangles) are the start of a game character.
// A base is loaded, turned into the other sex when asked (`bodyshape.rs`), unwrapped so it has a texture (`unwrap.rs`), and its looks (skin colour, tone,
// a pattern) are baked as a variable first layer of the texture (`features.rs`). The face is not drawn: it is painted by hand with the tools of the page, like
// everything else, and the mesh goes on to the Parts or Bones pages. A button makes the other sex of every base and saves the files. No AI.

use std::path::{Path, PathBuf};
use std::sync::mpsc::{Receiver, TryRecvError};

use eframe::egui::{self, RichText, Ui};
use image::RgbaImage;

use super::SeviathApp;
use crate::assetview::AssetMesh;
use crate::bodyshape::{self, Shift};
use crate::features::{self, Look, Pattern};
use crate::theme::*;
use crate::widgets::{self, HelpExt, section_title};

/// The share of the texture that is kept free for the colours of the strokes of the mesh brushes.
pub const STRIP: f32 = 0.125;

pub struct BaseEntry {
    pub name: String,
    pub path: PathBuf,
    pub kb: u64,
}

type LoadJob = Receiver<Result<(AssetMesh, Vec<u8>, RgbaImage), String>>;
type BakeJob = Receiver<RgbaImage>;

pub struct BasesUi {
    pub folder: String,
    pub list: Vec<BaseEntry>,
    scanned: Option<String>,
    pub chosen: Option<usize>,
    /// Make the other sex of the base, and how far.
    pub other_sex: bool,
    pub amount: f32,
    /// The size of the texture.
    pub size: u32,
    pub look: Look,
    pub seed: u32,
    load_job: Option<LoadJob>,
    bake_job: Option<BakeJob>,
    /// The looks changed and the texture is baked again when the pointer is let go.
    dirty: bool,
    /// The texture as the looks made it last (what is painted by hand is what differs from it).
    baked: Option<RgbaImage>,
    batch: Option<Receiver<String>>,
    pub message: Option<String>,
    /// Which mesh (`MeshUi::made`) came from a base: only that one has its looks baked by the sliders.
    base_made: u64,
    /// The height for "Set it", and the colour for "Colour the whole mesh".
    pub height: f32,
    pub fill: [u8; 3],
}

impl Default for BasesUi {
    fn default() -> Self {
        let folder = dirs::download_dir().map(|d| d.join("Bases").join("Readybases").join("Races").to_string_lossy().to_string()).unwrap_or_default();
        BasesUi {
            folder,
            list: Vec::new(),
            scanned: None,
            chosen: None,
            other_sex: false,
            amount: 1.0,
            size: 2048,
            look: Look::default(),
            seed: 1,
            load_job: None,
            bake_job: None,
            dirty: false,
            baked: None,
            batch: None,
            message: None,
            base_made: 0,
            height: 1.8,
            fill: [200, 200, 205],
        }
    }
}

/// The bases in a folder: every .glb, by name.
pub fn scan(folder: &Path) -> Vec<BaseEntry> {
    let mut v: Vec<BaseEntry> = std::fs::read_dir(folder)
        .map(|rd| {
            rd.filter_map(|e| e.ok())
                .filter(|e| e.path().extension().is_some_and(|x| x.eq_ignore_ascii_case("glb")))
                .map(|e| BaseEntry { name: e.path().file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_default(), kb: e.metadata().map(|m| m.len() / 1024).unwrap_or(0), path: e.path() })
                .collect()
        })
        .unwrap_or_default();
    v.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));
    v
}

/// The base as a mesh to paint: the other sex when asked, unwrapped, with the looks baked.
fn prepare(path: &Path, other: Option<Shift>, size: u32, look: &Look, seed: u32) -> Result<(AssetMesh, Vec<u8>, RgbaImage), String> {
    let bytes = std::fs::read(path).map_err(|e| format!("The base could not be read: {e}"))?;
    let mut mesh = AssetMesh::from_glb(&bytes)?;
    if let Some(s) = other {
        mesh = bodyshape::other_sex(&mesh, s);
    }
    let mut mesh = crate::unwrap::unwrap(&mesh, &crate::unwrap::Options { size, reserve: STRIP, ..Default::default() });
    let tex = features::bake(&mesh, size, look, seed);
    mesh.texture = Some(tex.clone());
    let glb = crate::meshfile::write_glb(&mesh, None, &[], 30.0);
    Ok((mesh, glb, tex))
}

/// The looks are baked again without losing the strokes of the brush: the texels that differ from the last bake are the strokes, and they stay.
pub fn keep_strokes(new: RgbaImage, last: &RgbaImage, now: &RgbaImage) -> RgbaImage {
    if new.dimensions() != last.dimensions() || new.dimensions() != now.dimensions() {
        return new;
    }
    let mut out = new;
    for ((o, l), c) in out.pixels_mut().zip(last.pixels()).zip(now.pixels()) {
        if l != c {
            *o = *c;
        }
    }
    out
}

impl SeviathApp {
    /// The jobs of the page, whichever tab is open.
    pub(super) fn bases_poll(&mut self, ctx: &egui::Context) {
        let job = self.m3.bases.load_job.as_ref().map(|rx| rx.try_recv());
        match job {
            Some(Ok(Ok((mesh, glb, tex)))) => {
                self.m3.bases.load_job = None;
                self.m3.bases.baked = Some(tex);
                let s = mesh.stats();
                self.m3.set_mesh(mesh, glb);
                self.m3.bases.base_made = self.m3.made;
                self.m3.hand.tool = super::handrig::Tool::Turn;
                self.m3.hand.strip_v0 = Some(1.0 - STRIP);
                self.m3.bases.message = Some(format!("The base is ready to paint: {} triangles, texture {} x {}. Paint it, draw hair with the mesh brushes, or go on to the Parts or Bones pages.", s.triangles, self.m3.bases.size, self.m3.bases.size));
            }
            Some(Ok(Err(e))) => {
                self.m3.bases.load_job = None;
                self.m3.bases.message = Some(e);
            }
            Some(Err(TryRecvError::Empty)) => ctx.request_repaint_after(std::time::Duration::from_millis(300)),
            Some(Err(TryRecvError::Disconnected)) => self.m3.bases.load_job = None,
            None => {}
        }
        let bake = self.m3.bases.bake_job.as_ref().map(|rx| rx.try_recv());
        match bake {
            Some(Ok(img)) => {
                self.m3.bases.bake_job = None;
                if let (Some(mesh), Some(last)) = (self.m3.mesh.as_mut(), self.m3.bases.baked.take()) {
                    let now = mesh.texture.clone().unwrap_or_else(|| img.clone());
                    mesh.texture = Some(keep_strokes(img.clone(), &last, &now));
                    self.m3.bases.baked = Some(img);
                    self.m3.hand.texture_changed();
                }
            }
            Some(Err(TryRecvError::Empty)) => ctx.request_repaint_after(std::time::Duration::from_millis(200)),
            Some(Err(TryRecvError::Disconnected)) => self.m3.bases.bake_job = None,
            None => {}
        }
        let msg = self.m3.bases.batch.as_ref().map(|rx| {
            let mut last = None;
            let mut gone = false;
            loop {
                match rx.try_recv() {
                    Ok(m) => last = Some(m),
                    Err(TryRecvError::Empty) => break,
                    Err(TryRecvError::Disconnected) => {
                        gone = true;
                        break;
                    }
                }
            }
            (last, gone)
        });
        if let Some((last, gone)) = msg {
            if let Some(m) = last {
                self.m3.bases.message = Some(m);
            }
            if gone {
                self.m3.bases.batch = None;
            } else {
                ctx.request_repaint_after(std::time::Duration::from_millis(400));
            }
        }
    }

    /// Load the chosen base (in the background).
    fn bases_load(&mut self, index: usize) {
        let b = &self.m3.bases;
        let Some(entry) = b.list.get(index) else { return };
        let (path, stem) = (entry.path.clone(), entry.name.clone());
        let other = b.other_sex.then(|| Shift { amount: b.amount, from_male: !bodyshape::is_female(&stem) });
        let (size, look, seed) = (b.size, b.look.clone(), b.seed);
        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let _ = tx.send(prepare(&path, other, size, &look, seed));
        });
        self.m3.bases.chosen = Some(index);
        self.m3.bases.load_job = Some(rx);
        self.m3.bases.message = Some(format!("Loading {stem}..."));
    }

    /// The looks changed: bake the texture again (in the background).
    fn bases_rebake(&mut self) {
        let Some(mesh) = &self.m3.mesh else { return };
        if self.m3.bases.bake_job.is_some() {
            self.m3.bases.dirty = true;
            return;
        }
        let (mut m, size, look, seed) = (mesh.clone(), self.m3.bases.size, self.m3.bases.look.clone(), self.m3.bases.seed);
        m.texture = None;
        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let _ = tx.send(features::bake(&m, size, &look, seed));
        });
        self.m3.bases.bake_job = Some(rx);
        self.m3.bases.dirty = false;
    }

    /// The other sex of every base, saved as new base files.
    fn bases_make_all(&mut self) {
        let Some(out) = rfd::FileDialog::new().set_title("A folder for the other sex of every base").set_directory(&self.m3.bases.folder).pick_folder() else { return };
        let files: Vec<(String, PathBuf)> = self.m3.bases.list.iter().map(|b| (b.name.clone(), b.path.clone())).collect();
        let amount = self.m3.bases.amount;
        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let total = files.len();
            let mut done = 0;
            for (i, (name, path)) in files.iter().enumerate() {
                let Some(other) = bodyshape::other_name(name) else {
                    let _ = tx.send(format!("{name}: the name does not say male or female, skipped."));
                    continue;
                };
                let _ = tx.send(format!("Making {other} ({} of {total})...", i + 1));
                let res = std::fs::read(path).map_err(|e| e.to_string()).and_then(|b| AssetMesh::from_glb(&b)).map(|m| bodyshape::other_sex(&m, Shift { amount, from_male: !bodyshape::is_female(name) })).and_then(|m| std::fs::write(out.join(format!("{other}.glb")), crate::meshfile::write_glb(&m, None, &[], 30.0)).map_err(|e| e.to_string()));
                match res {
                    Ok(()) => done += 1,
                    Err(e) => {
                        let _ = tx.send(format!("{name}: {e}"));
                    }
                }
            }
            let _ = tx.send(format!("Saved {done} bases of the other sex in {}.", out.display()));
        });
        self.m3.bases.batch = Some(rx);
    }

    /// Load a base of the folder by name and wait until it is there (the real bases of the user, when they are on this computer).
    #[cfg(test)]
    pub fn test_bases_load(&mut self, name: &str, other_sex: bool) -> bool {
        self.m3.bases.list = scan(Path::new(&self.m3.bases.folder));
        self.m3.bases.scanned = Some(self.m3.bases.folder.clone());
        let Some(i) = self.m3.bases.list.iter().position(|b| b.name == name) else { return false };
        self.m3.bases.other_sex = other_sex;
        self.bases_load(i);
        let ctx = egui::Context::default();
        let start = std::time::Instant::now();
        while self.m3.bases.load_job.is_some() && start.elapsed().as_secs() < 120 {
            std::thread::sleep(std::time::Duration::from_millis(100));
            self.bases_poll(&ctx);
        }
        self.m3.mesh.is_some()
    }

    #[cfg(test)]
    pub fn test_mesh_add_part(&mut self, part: crate::attach::Part) {
        if let Some(mut mesh) = self.m3.mesh.take() {
            self.m3.hand.attach_start(&mut mesh, part);
            self.m3.mesh = Some(mesh);
        }
    }

    #[cfg(test)]
    pub fn test_mesh_drop_part(&mut self) {
        if let Some(mut mesh) = self.m3.mesh.take() {
            self.m3.hand.attach_drop(&mut mesh);
            self.m3.mesh = Some(mesh);
        }
    }

    /// A part made from one of its presets and kept (with the bones of a person to hang from).
    #[cfg(test)]
    pub fn test_preset_part(&mut self, part: crate::attach::Part, preset: &str) {
        if let Some(mut mesh) = self.m3.mesh.take() {
            let (lo, hi) = mesh.bounds();
            if self.m3.hand.rig.bones.is_empty() {
                let mut r = crate::rig3d::Rig3::humanoid(mesh.verts.len(), lo, hi);
                r.assign_nearest_parts(&mesh.verts);
                self.m3.hand.use_rig(r);
            }
            self.m3.hand.attach_start(&mut mesh, part);
            if let (Some((p, _, v0, t0)), Some(params)) = (self.m3.hand.attach, part.presets().into_iter().find(|x| x.0 == preset).map(|x| x.1)) {
                self.m3.hand.attach = Some((p, params, v0, t0));
                self.m3.hand.attach_apply(&mut mesh);
            }
            self.m3.hand.attach_keep(&mesh);
            self.m3.mesh = Some(mesh);
        }
    }

    #[cfg(test)]
    pub fn test_open_hair_editor(&mut self) {
        if let Some(mut mesh) = self.m3.mesh.take() {
            self.m3.hand.open_hair_editor(&mut mesh);
            self.m3.mesh = Some(mesh);
        }
    }

    #[cfg(test)]
    pub fn test_mesh_keep_part(&mut self) {
        if let Some(mesh) = self.m3.mesh.take() {
            self.m3.hand.attach_keep(&mesh);
            self.m3.mesh = Some(mesh);
        }
    }

    /// Open a mesh file the way the button does (without the dialog).
    #[cfg(test)]
    pub fn test_mesh_import(&mut self, path: &std::path::Path) -> Result<(), String> {
        let i = crate::meshimport::load(path)?;
        let glb = crate::meshfile::write_glb(&i.mesh, None, &[], 30.0);
        self.m3.set_mesh(i.mesh, glb);
        if let Some(r) = i.rig {
            self.m3.hand.use_rig(r);
        }
        Ok(())
    }

    #[cfg(test)]
    pub fn test_bases_load_named(&mut self, name: &str) -> bool {
        self.test_bases_load(name, false)
    }

    #[cfg(test)]
    pub fn test_mesh_bases_load_other(&mut self) -> bool {
        self.test_bases_load("Midlanderbasefemale", true)
    }

    /// Open a file that has a mesh in it (glb, gltf, fbx, obj, stl, ply) to edit it: a base that is wrong, a model from another program.
    pub(super) fn mesh_import_dialog(&mut self) {
        let Some(p) = rfd::FileDialog::new().set_title("Open a mesh").add_filter("Meshes", crate::meshimport::EXTENSIONS).pick_file() else { return };
        match crate::meshimport::load(&p) {
            Ok(i) => {
                let glb = crate::meshfile::write_glb(&i.mesh, None, &[], 30.0);
                let stats = i.mesh.stats();
                self.m3.set_mesh(i.mesh, glb);
                if let Some(r) = i.rig {
                    self.m3.hand.use_rig(r);
                }
                self.m3.bases.message = Some(format!("Opened {}: {} triangles. {}It can be edited now: paint, sculpt, cut, add parts.", p.display(), stats.triangles, i.note));
            }
            Err(e) => self.m3.bases.message = Some(e),
        }
    }

    /// The left side of the page for a mesh that was made from a text, a picture or a drawing (or opened from a file): what it is, and how to get it ready for
    /// drawing on it.
    pub(super) fn edit_panel(&mut self, ui: &mut Ui) {
        section_title(ui, "Redraw and sculpt");
        if ui.button("Open a mesh from a file...").help("A .glb, .gltf, .fbx, .obj, .stl or .ply file: a base that is wrong, a model from another program. It can be edited like any mesh.").clicked() {
            self.mesh_import_dialog();
        }
        if let Some(m) = &self.m3.bases.message {
            ui.label(RichText::new(m).size(11.0).color(TEXT_DIM));
        }
        let Some(mesh) = self.m3.mesh.as_mut() else {
            ui.label(RichText::new("Make a mesh on the From a text or From a picture pages, or open one from a file.").size(11.5).color(TEXT_DIM));
            return;
        };
        let s = mesh.stats();
        ui.label(RichText::new(format!("{} triangles, {} points{}.", s.triangles, s.vertices, s.texture.map(|t| format!(", texture {} x {}", t.0, t.1)).unwrap_or_else(|| ", no texture".into()))).size(11.5).color(TEXT_DIM));
        ui.label(RichText::new("Paint it, sculpt it, draw hair and ropes on it, add parts, cut pieces off: everything the tools on the right do, as in Blender. The strip of texture for the colours of strokes and parts is added when you need it.").size(11.5).color(TEXT_DIM));
        // the size and the way it stands
        ui.add_space(6.0);
        section_title(ui, "Size and direction");
        let hand = &mut self.m3.hand;
        ui.horizontal(|ui| {
            ui.label(RichText::new("Height").size(12.0).color(TEXT_DIM));
            ui.add(egui::DragValue::new(&mut self.m3.bases.height).range(0.05..=60.0).speed(0.01).suffix(" m"));
            if ui.button("Set it").help("Scales the mesh (and its bones) to this height and puts its feet on the floor").clicked() {
                let (lo, hi) = mesh.bounds();
                let k = self.m3.bases.height / (hi[1] - lo[1]).max(1e-6);
                let (cx, cz) = ((lo[0] + hi[0]) / 2.0, (lo[2] + hi[2]) / 2.0);
                hand.transform(mesh, |p| [(p[0] - cx) * k, (p[1] - lo[1]) * k, (p[2] - cz) * k]);
            }
        });
        ui.horizontal_wrapped(|ui| {
            if ui.button("Z is up: turn it").help("A mesh from a program where the z axis is up (CAD, 3ds Max, Blender without the export setting) is turned to stand").clicked() {
                hand.transform(mesh, |p| [p[0], p[2], -p[1]]);
            }
            if ui.button("Turn it round").help("The back becomes the front").clicked() {
                hand.transform(mesh, |p| [-p[0], p[1], -p[2]]);
            }
            if ui.button("Stand it on the floor").clicked() {
                let (lo, hi) = mesh.bounds();
                let (cx, cz) = ((lo[0] + hi[0]) / 2.0, (lo[2] + hi[2]) / 2.0);
                hand.transform(mesh, |p| [p[0] - cx, p[1] - lo[1], p[2] - cz]);
            }
        });
        // colour
        ui.add_space(6.0);
        section_title(ui, "Colour");
        ui.horizontal(|ui| {
            ui.color_edit_button_srgb(&mut self.m3.bases.fill);
            if ui.button("Colour the whole mesh").help("Every part of the picture of the mesh gets this colour (paint over it after that: the paint tool, the recolour of a picked colour, the mesh brushes).").clicked() {
                let c = self.m3.bases.fill;
                if !hand.fill_color(mesh, c) {
                    self.m3.bases.message = Some("The mesh has no texture yet: unwrap it first.".into());
                }
            }
        });
        if mesh.texture.is_none() {
            ui.add_space(4.0);
            ui.label(RichText::new("This mesh has no texture (its colours are not in a picture), so it cannot be painted yet.").size(11.5).color(GOLD));
            if ui.button("Unwrap it and give it a plain texture").help("Cuts the mesh into pieces laid flat (it gets more points on the cuts) and gives it a grey texture to paint on. The bones are lost: do it before rigging.").clicked() {
                let mut m = crate::unwrap::unwrap(mesh, &crate::unwrap::Options { size: 2048, reserve: STRIP, ..Default::default() });
                m.texture = Some(RgbaImage::from_pixel(2048, 2048, image::Rgba([170, 170, 175, 255])));
                let glb = crate::meshfile::write_glb(&m, None, &[], 30.0);
                self.m3.set_mesh(m, glb);
                self.m3.hand.strip_v0 = Some(1.0 - STRIP);
                self.m3.bases.message = Some("Unwrapped. It has a plain texture to paint on now.".into());
            }
        }
    }

    /// The left side of the page: the bases, the other sex, and the looks.
    pub(super) fn bases_panel(&mut self, ui: &mut Ui) {
        let busy = self.m3.bases.load_job.is_some() || self.m3.bases.batch.is_some();
        if self.m3.bases.scanned.as_deref() != Some(self.m3.bases.folder.as_str()) {
            self.m3.bases.list = scan(Path::new(&self.m3.bases.folder));
            self.m3.bases.scanned = Some(self.m3.bases.folder.clone());
            self.m3.bases.chosen = None;
        }
        section_title(ui, "Bases");
        ui.horizontal(|ui| {
            ui.add(egui::TextEdit::singleline(&mut self.m3.bases.folder).desired_width(190.0));
            if ui.small_button("Pick...").help("The folder with the .glb bases").clicked() {
                if let Some(p) = rfd::FileDialog::new().set_title("The folder of the bases").pick_folder() {
                    self.m3.bases.folder = p.to_string_lossy().to_string();
                }
            }
        });
        if self.m3.bases.list.is_empty() {
            ui.label(RichText::new("No .glb files in this folder.").size(11.5).color(GOLD));
        }
        if ui.button("Import a base from a file...").help("A .glb, .gltf, .fbx, .obj, .stl or .ply file, to edit it").clicked() {
            self.mesh_import_dialog();
        }
        egui::ScrollArea::vertical().id_salt("bases_list").max_height(190.0).show(ui, |ui| {
            for i in 0..self.m3.bases.list.len() {
                let (name, kb) = (self.m3.bases.list[i].name.clone(), self.m3.bases.list[i].kb);
                if ui.selectable_label(self.m3.bases.chosen == Some(i), format!("{name}  ({:.1} MB)", kb as f32 / 1024.0)).clicked() && !busy {
                    self.bases_load(i);
                }
            }
        });
        ui.add_space(4.0);
        ui.checkbox(&mut self.m3.bases.other_sex, "Make the other sex of the base").help("Shoulders, chest, waist, hips and neck are changed by proportion: a woman base becomes a man and the other way round. Pick the base again to make it.");
        if self.m3.bases.other_sex {
            ui.add(egui::Slider::new(&mut self.m3.bases.amount, 0.3..=1.5).text("How far"));
        }
        ui.horizontal(|ui| {
            ui.label(RichText::new("Texture").size(12.0).color(TEXT_DIM));
            for n in [1024u32, 2048, 4096] {
                if widgets::chip(ui, &format!("{n}"), self.m3.bases.size == n).help("The size of the texture picture. Pick the base again to change it.").clicked() {
                    self.m3.bases.size = n;
                }
            }
        });
        if ui.add_enabled(!busy && !self.m3.bases.list.is_empty(), egui::Button::new("Make the other sex of every base...")).help("Saves a new .glb for every base in a folder you pick: Midlanderbasefemale gets Midlanderbasemale, and so on. They are bases like the others (no texture).").clicked() {
            self.bases_make_all();
        }
        if let Some(m) = &self.m3.bases.message {
            ui.label(RichText::new(m).size(11.5).color(TEXT_DIM));
        }
        if self.m3.mesh.is_none() || self.m3.bases.base_made != self.m3.made {
            if self.m3.mesh.is_some() {
                ui.add_space(6.0);
                ui.label(RichText::new("This mesh did not come from a base, so there are no looks to change. Use the Redraw and sculpt page to edit it.").size(11.5).color(TEXT_DIM));
            }
            return;
        }
        ui.add_space(8.0);
        section_title(ui, "Looks");
        let mut changed = false;
        ui.horizontal(|ui| {
            if ui.button("Random").help("Another skin colour and tone").clicked() {
                self.m3.bases.seed = self.m3.bases.seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
                self.m3.bases.look = features::random_look(self.m3.bases.seed, &self.m3.bases.look);
                changed = true;
            }
            if ui.button("Standard").help("The starting looks").clicked() {
                self.m3.bases.look = Look::default();
                changed = true;
            }
        });
        let look = &mut self.m3.bases.look;
        let mut released = false;
        let mut color = |ui: &mut Ui, label: &str, c: &mut [u8; 3], changed: &mut bool| {
            ui.horizontal(|ui| {
                ui.label(RichText::new(label).size(12.0).color(TEXT_DIM));
                *changed |= ui.color_edit_button_srgb(c).changed();
            });
        };
        color(ui, "Skin", &mut look.skin, &mut changed);
        let mut slider = |ui: &mut Ui, v: &mut f32, r: std::ops::RangeInclusive<f32>, label: &str, changed: &mut bool, released: &mut bool| {
            let resp = ui.add(egui::Slider::new(v, r).text(label));
            *changed |= resp.changed();
            *released |= resp.drag_stopped() || (resp.changed() && !resp.dragged());
        };
        slider(ui, &mut look.tone, 0.0..=1.0, "Darker to the feet", &mut changed, &mut released);
        ui.label(RichText::new("Pattern").size(12.0).color(TEXT_DIM));
        ui.horizontal_wrapped(|ui| {
            for p in Pattern::ALL {
                if widgets::chip(ui, p.label(), look.pattern == p).clicked() {
                    look.pattern = p;
                    changed = true;
                }
            }
        });
        if look.pattern != Pattern::None {
            color(ui, "Pattern colour", &mut look.pattern_color, &mut changed);
            slider(ui, &mut look.pattern_size, 0.3..=3.0, "Pattern size", &mut changed, &mut released);
            slider(ui, &mut look.pattern_strength, 0.0..=1.0, "Pattern strength", &mut changed, &mut released);
        }
        ui.label(RichText::new("The face is not drawn: paint it by hand with the brush on the right.").size(11.0).color(TEXT_DIM));
        if changed && released {
            self.m3.bases.dirty = true;
        }
        let pointer_down = ui.input(|i| i.pointer.any_down());
        if self.m3.bases.dirty && !pointer_down {
            self.bases_rebake();
        }
        if self.m3.bases.dirty || self.m3.bases.bake_job.is_some() {
            ui.label(RichText::new("Painting the looks...").size(11.0).color(GOLD));
        }
        ui.label(RichText::new("What you painted with the brush stays when the looks change.").size(11.0).color(TEXT_DIM));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_strokes_of_the_brush_stay_when_the_looks_are_baked_again() {
        let last = RgbaImage::from_pixel(4, 4, image::Rgba([100, 100, 100, 255]));
        let mut now = last.clone();
        now.put_pixel(1, 1, image::Rgba([255, 0, 0, 255]));
        let new = RgbaImage::from_pixel(4, 4, image::Rgba([10, 200, 10, 255]));
        let out = keep_strokes(new, &last, &now);
        assert_eq!(out.get_pixel(1, 1).0, [255, 0, 0, 255], "the stroke stays");
        assert_eq!(out.get_pixel(2, 2).0, [10, 200, 10, 255], "the rest has the new looks");
    }

    #[test]
    fn the_bases_of_a_folder_are_listed_by_name() {
        let dir = std::env::temp_dir().join("sevi_bases_test");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        for n in ["b_base.glb", "A_base.glb", "note.txt"] {
            std::fs::write(dir.join(n), b"x").unwrap();
        }
        let l = scan(&dir);
        assert_eq!(l.iter().map(|b| b.name.as_str()).collect::<Vec<_>>(), vec!["A_base", "b_base"]);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_base_becomes_a_textured_mesh_with_the_looks_on_it() {
        let (mesh, _) = crate::meshfile::tests_figure();
        let dir = std::env::temp_dir().join("sevi_bases_test2");
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("Figurebasefemale.glb");
        let mut plain = mesh.clone();
        plain.texture = None;
        plain.normal = None;
        std::fs::write(&path, crate::meshfile::write_glb(&plain, None, &[], 30.0)).unwrap();
        let (m, glb, tex) = prepare(&path, Some(Shift { amount: 1.0, from_male: false }), 256, &Look::default(), 1).unwrap();
        assert_eq!(m.texture.as_ref().unwrap().dimensions(), (256, 256));
        assert_eq!(tex.dimensions(), (256, 256));
        assert!(AssetMesh::from_glb(&glb).unwrap().texture.is_some());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
