// SPDX-License-Identifier: GPL-3.0-or-later
// Everything about files in the app: New / Open / Save / Save as / Export / Import picture,
// background saving and loading, autosave recovery copies, the "save changes?" guard,
// the New project dialog and the project manager.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::thread::JoinHandle;
use std::time::SystemTime;

use eframe::egui::{
    self, Align2, Color32, CornerRadius, FontId, RichText, Sense, Stroke, StrokeKind, TextureHandle, TextureOptions, Ui, pos2, vec2,
};

use super::SeviathApp;
use crate::doc::Document;
use crate::files::template::{self, NewProject};
use crate::files::{self, Flat, kra, ora, project, psd};
use crate::history::DocState;
use crate::settings::Settings;
use crate::theme::*;
use crate::widgets;

/// What to do once unsaved changes are dealt with.
#[derive(Clone)]
pub enum Pending {
    New(NewProject),
    Open(PathBuf),
    Close,
}

enum Done {
    Saved { path: PathBuf, generation: u64, then: Option<Pending> },
    Exported(PathBuf),
    Autosaved,
    Opened { doc: Box<Document>, path: PathBuf, project: bool },
}

struct Job {
    label: String,
    handle: JoinHandle<Result<Done, String>>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum ManagerTab {
    Projects,
    Recent,
    Recovery,
}

pub struct FileState {
    /// The .seviath file this document belongs to (None = not saved yet, or opened from PSD/ORA/picture).
    pub path: Option<PathBuf>,
    pub name: String,
    pub saved_generation: u64,
    autosaved_generation: u64,
    last_autosave: f64,
    job: Option<Job>,
    pub pending: Option<Pending>,
    close_ok: bool,
    title: String,
    pub new_open: bool,
    pub(super) np: super::newproject::NewState,
    manager_query: String,
    infos: HashMap<PathBuf, (SystemTime, Option<project::Info>)>,
    pub manager_open: bool,
    manager_tab: ManagerTab,
    thumbs: HashMap<PathBuf, (SystemTime, Option<TextureHandle>)>,
    rename: Option<(PathBuf, String)>,
}

impl Default for FileState {
    fn default() -> Self {
        Self {
            path: None,
            name: "Untitled".into(),
            saved_generation: 0,
            autosaved_generation: 0,
            last_autosave: 0.0,
            job: None,
            pending: None,
            close_ok: false,
            title: String::new(),
            new_open: false,
            np: Default::default(),
            manager_query: String::new(),
            infos: HashMap::new(),
            manager_open: false,
            manager_tab: ManagerTab::Projects,
            thumbs: HashMap::new(),
            rename: None,
        }
    }
}

impl FileState {
    pub(super) fn starting(show_projects: bool) -> Self {
        Self { manager_open: show_projects, ..Default::default() }
    }
}

fn kind_of(path: &Path) -> String {
    path.extension().map(|e| e.to_string_lossy().to_lowercase()).unwrap_or_default()
}

fn stem(path: &Path) -> String {
    path.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_else(|| "Untitled".into())
}

/// Load any supported file into a document (runs on a background thread).
fn load_any(path: &Path) -> Result<(Document, bool), String> {
    match kind_of(path).as_str() {
        "seviath" => project::load(path).map(|d| (d, true)),
        "psd" => {
            let bytes = std::fs::read(path).map_err(|e| e.to_string())?;
            psd::read(&bytes).map(|d| (d, false))
        }
        "ora" => ora::read(path).map(|d| (d, false)),
        "kra" => kra::read(path).map(|d| (d, false)),
        "cmo3" | "moc3" | "json" => {
            let moc = kind_of(path) != "cmo3";
            let imp = if moc { crate::import::moc3::import_file(path)? } else { crate::import::cmo3::import_file(path)? };
            let mut doc = imp.doc;
            let r = &imp.report;
            let mut notes = format!("Opened from a Live2D model ({}): {} parts, {} sliders, {} warps, {} rotations, {} shapes fitted.", imp.name, r.meshes, r.params, r.warps, r.rotations, r.morphs);
            for n in &r.notes {
                notes.push_str("
- ");
                notes.push_str(n);
            }
            doc.meta.notes = notes;
            Ok((doc, false))
        }
        _ => {
            let img = files::read_picture(path)?;
            Ok((files::document_from_picture(&img, &stem(path)), false))
        }
    }
}

fn doc_from_state(w: u32, h: u32, s: DocState) -> Document {
    let mut d = Document::from_parts(w, h, s.paper, s.layers);
    d.filter_presets = s.presets;
    d.meta = s.meta;
    d.rig = s.rig;
    d
}

/// Keep only the newest `keep` recovery copies.
fn prune_recovery(dir: &Path, keep: usize) {
    let Ok(rd) = std::fs::read_dir(dir) else { return };
    let mut files: Vec<(SystemTime, PathBuf)> = rd
        .flatten()
        .filter(|e| kind_of(&e.path()) == "seviath")
        .filter_map(|e| Some((e.metadata().ok()?.modified().ok()?, e.path())))
        .collect();
    files.sort_by(|a, b| b.0.cmp(&a.0));
    for (_, p) in files.into_iter().skip(keep) {
        let _ = std::fs::remove_file(p);
    }
}

fn list_seviath(dir: &Path) -> Vec<(PathBuf, SystemTime, u64)> {
    let Ok(rd) = std::fs::read_dir(dir) else { return Vec::new() };
    let mut v: Vec<(PathBuf, SystemTime, u64)> = rd
        .flatten()
        .filter(|e| kind_of(&e.path()) == "seviath")
        .filter_map(|e| {
            let m = e.metadata().ok()?;
            Some((e.path(), m.modified().ok()?, m.len()))
        })
        .collect();
    v.sort_by(|a, b| b.1.cmp(&a.1));
    v
}

fn when(t: SystemTime) -> String {
    let dt: chrono::DateTime<chrono::Local> = t.into();
    dt.format("%Y-%m-%d %H:%M").to_string()
}

fn size_text(bytes: u64) -> String {
    if bytes >= 1 << 20 { format!("{:.1} MB", bytes as f64 / (1u64 << 20) as f64) } else { format!("{} KB", (bytes >> 10).max(1)) }
}

impl SeviathApp {
    pub(super) fn is_dirty(&self) -> bool {
        self.history.generation != self.files.saved_generation
    }

    pub(super) fn busy(&self) -> Option<&str> {
        self.files.job.as_ref().map(|j| j.label.as_str())
    }

    fn start(&mut self, label: impl Into<String>, f: impl FnOnce() -> Result<Done, String> + Send + 'static) {
        if self.files.job.is_some() {
            return;
        }
        self.files.job = Some(Job { label: label.into(), handle: std::thread::spawn(f) });
    }

    /// Do `p` now, or ask about unsaved changes first.
    pub(super) fn request(&mut self, p: Pending) {
        if self.is_dirty() {
            self.files.pending = Some(p);
        } else {
            self.perform(p);
        }
    }

    fn perform(&mut self, p: Pending) {
        match p {
            Pending::New(np) => {
                let doc = template::build(&np);
                self.load_document(doc, None, np.name);
            }
            Pending::Open(path) => {
                let label = format!("Opening {}…", stem(&path));
                self.start(label, move || {
                    let (doc, project) = load_any(&path)?;
                    Ok(Done::Opened { doc: Box::new(doc), path, project })
                });
            }
            Pending::Close => {
                self.files.close_ok = true;
            }
        }
    }

    fn load_document(&mut self, doc: Document, path: Option<PathBuf>, name: String) {
        self.draw.replace_document(doc);
        self.history.clear();
        self.files.saved_generation = self.history.generation;
        self.files.autosaved_generation = self.history.generation;
        self.files.path = path;
        self.files.name = name;
        self.tab = crate::tabs::Tab::Draw;
    }

    pub(super) fn open_dialog(&mut self) {
        let dir = self.settings.projects_folder();
        let picked = rfd::FileDialog::new()
            .set_title("Open")
            .set_directory(&dir)
            .add_filter("Everything SeviathStudio opens", &["seviath", "psd", "ora", "kra", "cmo3", "moc3", "json", "png", "jpg", "jpeg", "webp", "bmp", "gif"])
            .add_filter("SeviathStudio project", &["seviath"])
            .add_filter("Photoshop", &["psd"])
            .add_filter("Krita", &["kra"])
            .add_filter("Live2D Cubism model", &["cmo3", "moc3", "json"])
            .add_filter("OpenRaster (Krita, GIMP, MyPaint)", &["ora"])
            .add_filter("Pictures", &["png", "jpg", "jpeg", "webp", "bmp", "gif"])
            .pick_file();
        if let Some(p) = picked {
            self.request(Pending::Open(p));
        }
    }

    /// Save to the project file, or ask where when there is none yet.
    pub(super) fn save(&mut self, then: Option<Pending>) {
        match self.files.path.clone() {
            Some(p) => self.save_to(p, then),
            None => self.save_as(then),
        }
    }

    pub(super) fn save_as(&mut self, then: Option<Pending>) {
        let dir = self.settings.projects_folder();
        let _ = std::fs::create_dir_all(&dir);
        let picked = rfd::FileDialog::new()
            .set_title("Save project")
            .set_directory(&dir)
            .set_file_name(format!("{}.seviath", self.files.name))
            .add_filter("SeviathStudio project", &["seviath"])
            .save_file();
        if let Some(mut p) = picked {
            if kind_of(&p) != project::EXT {
                p.set_extension(project::EXT);
            }
            self.save_to(p, then);
        }
    }

    fn save_to(&mut self, path: PathBuf, then: Option<Pending>) {
        // a floating transform is put down first, so nothing is missing from the file
        self.draw.apply_transform();
        let state = self.draw.doc.snapshot();
        let (w, h) = (self.draw.doc.w, self.draw.doc.h);
        let generation = self.history.generation;
        self.flush_reference_time();
        let log = self.draw.doc.log.clone();
        let label = format!("Saving {}…", stem(&path));
        self.start(label, move || {
            project::save_full(w, h, state.paper, &state.layers, &state.presets, &state.meta, &log, &state.rig, &path)?;
            Ok(Done::Saved { path, generation, then })
        });
    }

    pub(super) fn export_dialog(&mut self) {
        let dir = self.files.path.as_ref().and_then(|p| p.parent().map(Path::to_path_buf)).unwrap_or_else(|| self.settings.projects_folder());
        let picked = rfd::FileDialog::new()
            .set_title("Export")
            .set_directory(&dir)
            .set_file_name(format!("{}.png", self.files.name))
            .add_filter("PNG picture", &["png"])
            .add_filter("JPEG picture", &["jpg", "jpeg"])
            .add_filter("WebP picture", &["webp"])
            .add_filter("Photoshop (layers)", &["psd"])
            .add_filter("OpenRaster (layers)", &["ora"])
            .save_file();
        let Some(path) = picked else { return };
        self.draw.apply_transform();
        let state = self.draw.doc.snapshot();
        let (w, h) = (self.draw.doc.w, self.draw.doc.h);
        let q = self.settings.jpeg_quality;
        let credit = self.settings.export_credit;
        let rule = self.draw.doc.meta.auto_sign;
        let sign = if rule.on { crate::signature::load_in(&crate::signature::dir(), rule.slot) } else { None };
        let label = format!("Exporting {}…", path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default());
        self.start(label, move || {
            let mut doc = doc_from_state(w, h, state);
            if let Some(s) = &sign {
                s.add_layer(&mut doc, &rule, "Signature");
            }
            match kind_of(&path).as_str() {
                "jpg" | "jpeg" => files::export_flat(&doc, &path, Flat::Jpeg, q, credit)?,
                "webp" => files::export_flat(&doc, &path, Flat::Webp, q, credit)?,
                "psd" => std::fs::write(&path, psd::write(&files::bake(doc))).map_err(|e| e.to_string())?,
                "ora" => ora::write(&files::bake(doc), &path)?,
                _ => {
                    let p = if kind_of(&path) == "png" { path.clone() } else { path.with_extension("png") };
                    files::export_flat(&doc, &p, Flat::Png, q, credit)?;
                    return Ok(Done::Exported(p));
                }
            }
            Ok(Done::Exported(path))
        });
    }

    pub(super) fn import_picture_dialog(&mut self, now: f64) {
        let picked = rfd::FileDialog::new()
            .set_title("Picture as a new layer")
            .add_filter("Pictures", &["png", "jpg", "jpeg", "webp", "bmp", "gif"])
            .pick_file();
        let Some(p) = picked else { return };
        match files::read_picture(&p) {
            Ok(img) => {
                self.draw.import_picture_layer(&img, &stem(&p));
                self.tab = crate::tabs::Tab::Draw;
            }
            Err(e) => self.say(e, now),
        }
    }

    /// Every frame: finished jobs, autosave, dropped files, window title, close guard.
    pub(super) fn files_tick(&mut self, ctx: &egui::Context) {
        let now = ctx.input(|i| i.time);
        // finished background work
        if self.files.job.as_ref().is_some_and(|j| j.handle.is_finished()) {
            let job = self.files.job.take().unwrap();
            match job.handle.join().unwrap_or_else(|_| Err("Something went wrong (the job crashed).".into())) {
                Ok(Done::Saved { path, generation, then }) => {
                    self.files.saved_generation = generation;
                    self.files.name = stem(&path);
                    self.settings.add_recent(&path);
                    self.say(format!("Saved {}", path.display()), now);
                    self.files.path = Some(path);
                    if let Some(p) = then {
                        self.perform(p);
                    }
                }
                Ok(Done::Exported(path)) => self.say(format!("Exported {}", path.display()), now),
                Ok(Done::Autosaved) => {}
                Ok(Done::Opened { doc, path, project }) => {
                    self.settings.add_recent(&path);
                    let name = stem(&path);
                    self.load_document(*doc, project.then(|| path.clone()), name);
                    self.files.manager_open = false;
                    self.say(format!("Opened {}", path.display()), now);
                }
                Err(e) => self.say(e, now),
            }
        }
        if self.files.job.is_some() {
            ctx.request_repaint_after_secs(0.1);
        }
        // project files dropped on the window
        for p in std::mem::take(&mut self.draw.open_requests) {
            self.request(Pending::Open(p));
        }
        // autosave: a recovery copy every few minutes while there are changes
        let minutes = self.draw.doc.meta.autosave_minutes.unwrap_or(self.settings.autosave_minutes);
        if self.persist
            && minutes > 0.0
            && self.files.job.is_none()
            && !self.draw.transforming()
            && self.history.generation != self.files.autosaved_generation
            && now - self.files.last_autosave > minutes as f64 * 60.0
            && !ctx.input(|i| i.pointer.any_down())
        {
            self.files.last_autosave = now;
            self.files.autosaved_generation = self.history.generation;
            let state = self.draw.doc.snapshot();
            let (w, h) = (self.draw.doc.w, self.draw.doc.h);
            let dir = Settings::recovery_folder();
            let keep = self.draw.doc.meta.recovery_copies.unwrap_or(self.settings.recovery_copies);
            let name = self.files.name.clone();
            let also = if self.settings.autosave_project_file { self.files.path.clone() } else { None };
            let generation = self.history.generation;
            let log = self.draw.doc.log.clone();
            self.start("Autosaving…", move || {
                std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
                let stamp = chrono::Local::now().format("%Y%m%d-%H%M%S");
                let file = dir.join(format!("{name} {stamp}.seviath"));
                project::save_full(w, h, state.paper, &state.layers, &state.presets, &state.meta, &log, &state.rig, &file)?;
                prune_recovery(&dir, keep);
                if let Some(p) = also {
                    project::save_full(w, h, state.paper, &state.layers, &state.presets, &state.meta, &log, &state.rig, &p)?;
                    return Ok(Done::Saved { path: p, generation, then: None });
                }
                Ok(Done::Autosaved)
            });
        }
        // window title: name, * when there are unsaved changes
        let title = format!("{}{} — SeviathStudio", self.files.name, if self.is_dirty() { " *" } else { "" });
        if title != self.files.title {
            ctx.send_viewport_cmd(egui::ViewportCommand::Title(title.clone()));
            self.files.title = title;
        }
        // closing the window
        if ctx.input(|i| i.viewport().close_requested()) && !self.files.close_ok {
            if self.is_dirty() || self.files.job.is_some() {
                ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
                if self.files.job.is_some() && !self.is_dirty() {
                    self.say("Still saving — closing as soon as it's done.", now);
                }
                self.files.pending = Some(Pending::Close);
            }
        }
        if self.files.close_ok && self.files.job.is_none() {
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        }
        // a pending close waits for a running save, then closes
        if matches!(self.files.pending, Some(Pending::Close)) && !self.is_dirty() && self.files.job.is_none() {
            self.files.pending = None;
            self.files.close_ok = true;
        }
    }

    /// "Save changes to …?" before replacing or closing a document with unsaved work.
    pub(super) fn unsaved_dialog(&mut self, ctx: &egui::Context) {
        if self.files.pending.is_none() || !self.is_dirty() {
            if let Some(p) = self.files.pending.take() {
                if !matches!(p, Pending::Close) || self.files.job.is_none() {
                    self.perform(p);
                } else {
                    self.files.pending = Some(p);
                }
            }
            return;
        }
        let what = match &self.files.pending {
            Some(Pending::Close) => "before closing",
            Some(Pending::New(_)) => "before starting a new project",
            _ => "before opening another file",
        };
        let mut choice = 0;
        egui::Modal::new(egui::Id::new("unsaved")).show(ctx, |ui| {
            ui.set_width(380.0);
            ui.label(RichText::new(format!("Save changes to \"{}\"?", self.files.name)).size(16.0).strong());
            ui.label(RichText::new(format!("You have unsaved changes {what}.")).color(TEXT_DIM));
            ui.add_space(8.0);
            ui.horizontal(|ui| {
                if ui.button(RichText::new("Save").strong()).clicked() {
                    choice = 1;
                }
                if ui.button("Don't save").clicked() {
                    choice = 2;
                }
                if ui.button("Cancel").clicked() {
                    choice = 3;
                }
            });
        });
        match choice {
            1 => {
                let then = self.files.pending.take();
                self.save(then);
            }
            2 => {
                let p = self.files.pending.take().unwrap();
                // forget the changes: count as saved
                self.files.saved_generation = self.history.generation;
                self.perform(p);
            }
            3 => self.files.pending = None,
            _ => {}
        }
    }

    // ---------- Project manager ----------

    fn thumb(&mut self, ctx: &egui::Context, path: &Path, modified: SystemTime, budget: &mut u32) -> Option<egui::TextureId> {
        if let Some((m, t)) = self.files.thumbs.get(path) {
            if *m == modified {
                return t.as_ref().map(|t| t.id());
            }
        }
        if *budget == 0 {
            ctx.request_repaint();
            return None;
        }
        *budget -= 1;
        let tex = project::read_thumbnail(path).map(|img| {
            let ci = egui::ColorImage::from_rgba_unmultiplied([img.width() as usize, img.height() as usize], img.as_raw());
            ctx.load_texture(format!("thumb:{}", path.display()), ci, TextureOptions::LINEAR)
        });
        let id = tex.as_ref().map(|t| t.id());
        self.files.thumbs.insert(path.to_path_buf(), (modified, tex));
        id
    }

    #[cfg(test)]
    pub fn test_manager_query(&mut self, q: &str) {
        self.files.manager_query = q.into();
    }

    /// Size, DPI, tags and notes of a project, read once and remembered until the file changes.
    fn project_info(&mut self, path: &Path, modified: SystemTime) -> Option<project::Info> {
        if let Some((m, i)) = self.files.infos.get(path) {
            if *m == modified {
                return i.clone();
            }
        }
        let info = project::read_info(path);
        self.files.infos.insert(path.to_path_buf(), (modified, info.clone()));
        info
    }

    pub(super) fn project_manager(&mut self, ctx: &egui::Context) {
        if !self.files.manager_open {
            return;
        }
        let mut open = true;
        let mut action: Option<(u8, PathBuf)> = None; // 0 open, 1 duplicate, 2 trash, 3 show, 4 rename-start
        egui::Window::new(RichText::new("Projects").strong())
            .open(&mut open)
            .default_size(vec2(820.0, 560.0))
            .pivot(Align2::CENTER_CENTER)
            .default_pos(ctx.content_rect().center())
            .collapsible(false)
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    if ui.button(RichText::new("+ New project").strong()).clicked() {
                        self.open_new_project();
                    }
                    if ui.button("Open a file…").clicked() {
                        self.open_dialog();
                    }
                    ui.add_space(12.0);
                    for (t, label) in [(ManagerTab::Projects, "My projects"), (ManagerTab::Recent, "Recent"), (ManagerTab::Recovery, "Recovery copies")] {
                        if widgets::chip(ui, label, self.files.manager_tab == t).clicked() {
                            self.files.manager_tab = t;
                        }
                    }
                });
                ui.add(
                    egui::TextEdit::singleline(&mut self.files.manager_query)
                        .hint_text("Search by name, tags or notes…")
                        .desired_width(f32::INFINITY),
                );
                let list: Vec<(PathBuf, SystemTime, u64)> = match self.files.manager_tab {
                    ManagerTab::Projects => {
                        let dir = self.settings.projects_folder();
                        ui.label(RichText::new(format!("Folder: {}", dir.display())).size(11.0).color(TEXT_DIM));
                        list_seviath(&dir)
                    }
                    ManagerTab::Recovery => {
                        ui.label(
                            RichText::new("Copies made automatically while you work (Settings → Files). Open one to get your work back.")
                                .size(11.0)
                                .color(TEXT_DIM),
                        );
                        list_seviath(&Settings::recovery_folder())
                    }
                    ManagerTab::Recent => self
                        .settings
                        .recent
                        .iter()
                        .map(PathBuf::from)
                        .filter_map(|p| {
                            let m = std::fs::metadata(&p).ok()?;
                            Some((p, m.modified().ok()?, m.len()))
                        })
                        .collect(),
                };
                ui.separator();
                let query = self.files.manager_query.trim().to_lowercase();
                let list: Vec<(PathBuf, SystemTime, u64)> = if query.is_empty() {
                    list
                } else {
                    list.into_iter()
                        .filter(|(p, m, _)| {
                            if stem(p).to_lowercase().contains(&query) {
                                return true;
                            }
                            self.project_info(p, *m).is_some_and(|i| {
                                i.meta.tags.iter().any(|t| t.to_lowercase().contains(&query)) || i.meta.notes.to_lowercase().contains(&query)
                            })
                        })
                        .collect()
                };
                if list.is_empty() {
                    ui.add_space(30.0);
                    let msg = if query.is_empty() { "Nothing here yet." } else { "No project matches your search." };
                    ui.vertical_centered(|ui| ui.label(RichText::new(msg).color(TEXT_DIM)));
                    return;
                }
                let mut budget = 2;
                egui::ScrollArea::vertical().show(ui, |ui| {
                    ui.horizontal_wrapped(|ui| {
                        ui.spacing_mut().item_spacing = vec2(10.0, 10.0);
                        for (path, modified, size) in list {
                            let (cell, resp) = ui.allocate_exact_size(vec2(180.0, 210.0), Sense::click());
                            let tex = if kind_of(&path) == "seviath" { self.thumb(ctx, &path, modified, &mut budget) } else { None };
                            let p = ui.painter();
                            p.rect_filled(cell, CornerRadius::same(10), if resp.hovered() { HOVER } else { RAISED });
                            let pic = egui::Rect::from_min_size(cell.min + vec2(10.0, 10.0), vec2(160.0, 150.0));
                            p.rect_filled(pic, CornerRadius::same(6), BG);
                            if let Some(t) = tex {
                                if let Some((_, Some(h))) = self.files.thumbs.get(&path) {
                                    let s = h.size_vec2();
                                    let k = (pic.width() / s.x).min(pic.height() / s.y);
                                    let r = egui::Rect::from_center_size(pic.center(), s * k);
                                    p.image(t, r, egui::Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)), Color32::WHITE);
                                }
                            }
                            let current = self.files.path.as_deref() == Some(path.as_path());
                            if current {
                                p.rect_stroke(cell, CornerRadius::same(10), Stroke::new(2.0, VENOM), StrokeKind::Inside);
                            }
                            let name = stem(&path);
                            let info = if kind_of(&path) == "seviath" { self.project_info(&path, modified) } else { None };
                            let p = ui.painter();
                            p.with_clip_rect(cell.shrink(6.0)).text(cell.left_bottom() + vec2(10.0, -44.0), Align2::LEFT_CENTER, &name, FontId::proportional(13.0), TEXT);
                            if let Some(i) = &info {
                                let mut line = format!("{}×{} · {} dpi", i.w, i.h, i.meta.dpi);
                                if !i.meta.tags.is_empty() {
                                    line += &format!(" · {}", i.meta.tags.join(", "));
                                }
                                p.with_clip_rect(cell.shrink(6.0)).text(cell.left_bottom() + vec2(10.0, -29.0), Align2::LEFT_CENTER, line, FontId::proportional(10.5), VENOM);
                            }
                            p.text(
                                cell.left_bottom() + vec2(10.0, -14.0),
                                Align2::LEFT_CENTER,
                                format!("{} · {}", when(modified), size_text(size)),
                                FontId::proportional(10.5),
                                TEXT_DIM,
                            );
                            let resp = resp.on_hover_text(path.display().to_string());
                            if resp.double_clicked() || resp.clicked() {
                                action = Some((0, path.clone()));
                            }
                            resp.context_menu(|ui| {
                                if ui.button("Open").clicked() {
                                    action = Some((0, path.clone()));
                                    ui.close();
                                }
                                if ui.button("Rename…").clicked() {
                                    action = Some((4, path.clone()));
                                    ui.close();
                                }
                                if ui.button("Duplicate").clicked() {
                                    action = Some((1, path.clone()));
                                    ui.close();
                                }
                                if ui.button("Show in folder").clicked() {
                                    action = Some((3, path.clone()));
                                    ui.close();
                                }
                                ui.separator();
                                if ui.button(RichText::new("Move to Recycle Bin").color(Color32::from_rgb(232, 110, 110))).clicked() {
                                    action = Some((2, path.clone()));
                                    ui.close();
                                }
                            });
                        }
                    });
                });
            });
        self.files.manager_open = open;
        let now = ctx.input(|i| i.time);
        if let Some((a, path)) = action {
            match a {
                0 => self.request(Pending::Open(path)),
                1 => {
                    let mut n = 1;
                    let target = loop {
                        let t = path.with_file_name(format!("{} copy{}.seviath", stem(&path), if n > 1 { format!(" {n}") } else { String::new() }));
                        if !t.exists() {
                            break t;
                        }
                        n += 1;
                    };
                    if let Err(e) = std::fs::copy(&path, &target) {
                        self.say(format!("Couldn't duplicate: {e}"), now);
                    }
                }
                2 => {
                    if self.files.path.as_deref() == Some(path.as_path()) {
                        self.say("That project is open — close it first.", now);
                    } else if let Err(e) = trash::delete(&path) {
                        self.say(format!("Couldn't move it to the Recycle Bin: {e}"), now);
                    } else {
                        self.say(format!("Moved {} to the Recycle Bin", stem(&path)), now);
                    }
                }
                3 => {
                    #[cfg(windows)]
                    {
                        let _ = std::process::Command::new("explorer").arg(format!("/select,{}", path.display())).spawn();
                    }
                    #[cfg(not(windows))]
                    {
                        if let Some(dir) = path.parent() {
                            let _ = std::process::Command::new("xdg-open").arg(dir).spawn();
                        }
                    }
                }
                _ => self.files.rename = Some((path.clone(), stem(&path))),
            }
        }
        self.rename_window(ctx);
    }

    fn rename_window(&mut self, ctx: &egui::Context) {
        let Some((path, mut name)) = self.files.rename.clone() else { return };
        let now = ctx.input(|i| i.time);
        let mut done = None;
        egui::Modal::new(egui::Id::new("rename_project")).show(ctx, |ui| {
            ui.label(RichText::new("Rename project").strong());
            let r = ui.add(egui::TextEdit::singleline(&mut name).desired_width(280.0).char_limit(120));
            r.request_focus();
            ui.horizontal(|ui| {
                if ui.button("Rename").clicked() || ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                    done = Some(true);
                }
                if ui.button("Cancel").clicked() {
                    done = Some(false);
                }
            });
        });
        match done {
            Some(true) => {
                self.files.rename = None;
                let clean: String = name.trim().chars().filter(|c| !"\\/:*?\"<>|".contains(*c)).collect();
                if clean.is_empty() {
                    return;
                }
                let target = path.with_file_name(format!("{clean}.seviath"));
                if target.exists() {
                    self.say("A project with that name already exists.", now);
                } else if let Err(e) = std::fs::rename(&path, &target) {
                    self.say(format!("Couldn't rename: {e}"), now);
                } else if self.files.path.as_deref() == Some(path.as_path()) {
                    self.files.path = Some(target);
                    self.files.name = clean;
                }
            }
            Some(false) => self.files.rename = None,
            None => self.files.rename = Some((path, name)),
        }
    }

    /// The File menu in the top bar.
    pub(super) fn file_menu(&mut self, ui: &mut Ui) {
        let now = ui.input(|i| i.time);
        ui.menu_button(RichText::new("File").color(TEXT), |ui| {
            ui.set_min_width(230.0);
            let item = |ui: &mut Ui, label: &str, keys: &str| {
                ui.add(egui::Button::new(label).shortcut_text(keys)).clicked()
            };
            if item(ui, "New project…", "Ctrl+N") {
                self.open_new_project();
                ui.close();
            }
            if item(ui, "Open…", "Ctrl+O") {
                ui.close();
                self.open_dialog();
            }
            if item(ui, "Projects…", "Ctrl+Shift+O") {
                self.files.manager_open = true;
                ui.close();
            }
            ui.menu_button("Open recent", |ui| {
                let recent = self.settings.recent.clone();
                if recent.is_empty() {
                    ui.label(RichText::new("Nothing yet").color(TEXT_DIM));
                }
                for r in recent {
                    let p = PathBuf::from(&r);
                    if ui.button(stem(&p)).on_hover_text(&r).clicked() {
                        ui.close();
                        self.request(Pending::Open(p));
                    }
                }
            });
            ui.separator();
            if item(ui, "Save", "Ctrl+S") {
                ui.close();
                self.save(None);
            }
            if item(ui, "Save as…", "Ctrl+Shift+S") {
                ui.close();
                self.save_as(None);
            }
            if item(ui, "Work history…", "") {
                ui.close();
                self.open_work_history();
            }
            if item(ui, "Recorder…", "") {
                ui.close();
                self.open_recorder();
            }
            if item(ui, "Font from my drawings…", "") {
                ui.close();
                self.open_font_maker();
            }
            if item(ui, "Signatures…", "") {
                ui.close();
                self.open_signatures();
            }
            if item(ui, "Project info…", "") {
                ui.close();
                self.open_project_info();
            }
            ui.separator();
            if item(ui, "Export…  (PNG, JPEG, WebP, PSD, ORA)", "Ctrl+Shift+E") {
                ui.close();
                self.export_dialog();
            }
            if item(ui, "Picture as a new layer…", "") {
                ui.close();
                self.import_picture_dialog(now);
            }
        });
    }
}
