// SPDX-License-Identifier: GPL-3.0-or-later
// The "My brush" tools in the brush panel: save the brush you are holding to My brushes with its
// credit and license, share it as a brush card or file, bring brushes in, remix, and make a stamp
// brush from a picture or from what you have selected.

use eframe::egui::{self, Color32, RichText, Ui, vec2};

use super::DrawState;
use super::brushes::{render_preview_sized, slot};
use crate::brush::tipimage::TipImage;
use crate::brush::userlib::{self, UserBrush};
use crate::brush::{BrushSettings, CATEGORIES, Preset, Tip};
use crate::theme::*;
use crate::widgets::{self, section_title};

/// A saved brush as a brush-list entry (the names live as long as the program, they are few).
pub fn preset_of(b: &UserBrush) -> Preset {
    let leak = |s: &str| -> &'static str { Box::leak(s.to_owned().into_boxed_str()) };
    let about = if b.about.is_empty() && !b.author.is_empty() { format!("By {}", b.author) } else { b.about.clone() };
    Preset { name: leak(&b.name), category: "My brushes", about: leak(&about), s: b.settings.clone() }
}

fn card_picture(b: &UserBrush) -> image::RgbaImage {
    let mut s = b.settings.clone();
    s.size = s.size.clamp(10.0, 70.0);
    let pv = render_preview_sized(&s, 420, 150, Color32::from_rgb(29, 29, 36));
    image::RgbaImage::from_fn(420, 150, |x, y| {
        let c = pv.pixels[(y * 420 + x) as usize];
        let a = c.a() as f32 / 255.0;
        let mix = |v: u8, bg: f32| (v as f32 + bg * (1.0 - a)).round().min(255.0) as u8;
        image::Rgba([mix(c.r(), 244.0), mix(c.g(), 241.0), mix(c.b(), 234.0), 255])
    })
}

impl DrawState {
    /// Read the saved brushes again and rebuild the brush list.
    pub fn reload_user_brushes(&mut self) {
        self.lib.user = userlib::load_all_in(&userlib::dir());
        let mut all = crate::brush::presets();
        all.extend(self.lib.user.iter().map(preset_of));
        self.lib.presets = all;
    }

    fn current_user_brush(&self) -> UserBrush {
        let sl = slot(self.tool);
        UserBrush { settings: self.lib.mem[sl].s.clone(), ..self.lib.meta.clone() }.sanitized()
    }

    /// Save the brush in hand to My brushes (a new name makes a new brush, the same name replaces it).
    pub fn save_current_brush(&mut self) {
        let b = self.current_user_brush();
        match userlib::save_in(&userlib::dir(), &b) {
            Ok(_) => {
                self.reload_user_brushes();
                let sl = slot(self.tool);
                self.lib.mem[sl].name = b.name.clone();
                self.lib.mem[sl].category = CATEGORIES.iter().position(|c| *c == "My brushes").unwrap_or(0);
                self.lib.status = format!("Saved \"{}\" to My brushes.", b.name);
            }
            Err(e) => self.lib.status = e,
        }
    }

    pub fn import_brushes(&mut self, paths: &[std::path::PathBuf]) {
        let (mut ok, mut last, mut problem) = (0, None, None);
        for p in paths {
            match userlib::import_file(p) {
                Ok(list) => {
                    for b in list {
                        if userlib::save_in(&userlib::dir(), &b).is_ok() {
                            ok += 1;
                            last = Some(b);
                        }
                    }
                }
                Err(e) => problem = Some(e),
            }
        }
        self.reload_user_brushes();
        if let Some(b) = last {
            let sl = slot(self.tool);
            self.lib.mem[sl].s = BrushSettings { size: self.lib.mem[sl].s.size, ..b.settings.clone() };
            self.lib.mem[sl].name = b.name.clone();
            self.lib.mem[sl].category = CATEGORIES.iter().position(|c| *c == "My brushes").unwrap_or(0);
            self.lib.meta = UserBrush { settings: BrushSettings::default(), ..b };
        }
        self.lib.status = match (ok, problem) {
            (0, Some(e)) => e,
            (n, Some(e)) => format!("Added {n} brush(es). {e}"),
            (1, None) => "Added the brush to My brushes.".into(),
            (n, None) => format!("Added {n} brushes to My brushes."),
        };
    }

    /// The painted pixels of the selection on the selected layer, as a picture (only what is inside the selection).
    fn selection_picture(&mut self, what: &str) -> Option<image::RgbaImage> {
        let Some(sel) = self.doc.selection.as_ref() else {
            self.lib.status = format!("Select something first (with the selection tools) and I will make {what} from it.");
            return None;
        };
        let Some((x0, y0, x1, y1)) = sel.bounds() else {
            self.lib.status = "The selection is empty.".into();
            return None;
        };
        let i = self.doc.index_of(self.active)?;
        let (w, h) = ((x1 - x0).max(1) as u32, (y1 - y0).max(1) as u32);
        if w * h > 16_000_000 {
            self.lib.status = format!("That selection is too big for {what}. Try a smaller one.");
            return None;
        }
        let mut img = crate::files::layer_rgba(&self.doc.layers[i], x0, y0, w, h);
        for (x, y, p) in img.enumerate_pixels_mut() {
            let k = sel.at(x0 + x as i32, y0 + y as i32) as u32;
            p.0[3] = (p.0[3] as u32 * k / 255) as u8;
        }
        Some(img)
    }

    /// Reload the materials library from disk.
    pub fn reload_materials(&mut self) {
        let dir = userlib::materials_dir();
        self.lib.textures = userlib::list_materials_in(&dir, "textures");
        self.lib.stamps = userlib::list_materials_in(&dir, "tips");
    }

    /// Make the picture the texture of the brush in hand (repeating), and keep it in the library.
    pub fn use_texture(&mut self, img: &image::RgbaImage) {
        let sl = slot(self.tool);
        let Some(mut t) = TipImage::from_picture(img) else {
            self.lib.status = "There is nothing in that picture to make a texture from.".into();
            return;
        };
        if self.lib.seamless {
            t = t.seamless();
        }
        let name = if self.lib.mat_name.trim().is_empty() { format!("Texture {}", self.lib.textures.len() + 1) } else { self.lib.mat_name.trim().to_string() };
        let saved = userlib::save_material_in(&userlib::materials_dir(), "textures", &name, &t);
        let b = &mut self.lib.mem[sl].s;
        b.tex_kind = crate::brush::TexKind::Custom;
        b.texture = b.texture.max(0.55);
        b.tex_image = Some(t);
        self.reload_materials();
        self.lib.status = match saved {
            Ok(_) => format!("\"{name}\" is the texture of this brush, and is kept in your materials."),
            Err(e) => e,
        };
    }

    pub fn texture_from_selection(&mut self) {
        if let Some(img) = self.selection_picture("a texture") {
            self.use_texture(&img);
        }
    }

    pub fn texture_from_file(&mut self, path: &std::path::Path) {
        match crate::files::read_picture(path) {
            Ok(img) => self.use_texture(&img),
            Err(e) => self.lib.status = e,
        }
    }

    /// A stamp from the pixels of the selection on the selected layer (what is painted there, or its darkness).
    pub fn tip_from_selection(&mut self) {
        let sl = slot(self.tool);
        let Some(img) = self.selection_picture("a stamp") else { return };
        match TipImage::from_picture(&img) {
            Some(t) => {
                let s = &mut self.lib.mem[sl].s;
                s.tip = Tip::Image;
                s.tip_image = Some(t);
                s.follow_direction = false;
                s.spacing = s.spacing.max(0.15);
                self.lib.status = "The selection is now the stamp of this brush.".into();
            }
            None => self.lib.status = "There is nothing painted in the selection.".into(),
        }
    }

    pub fn tip_from_file(&mut self, path: &std::path::Path) {
        let sl = slot(self.tool);
        match crate::files::read_picture(path).ok().and_then(|img| TipImage::from_picture(&img)) {
            Some(t) => {
                let s = &mut self.lib.mem[sl].s;
                s.tip = Tip::Image;
                s.tip_image = Some(t);
                s.spacing = s.spacing.max(0.15);
                self.lib.status = "That picture is now the stamp of this brush.".into();
            }
            None => self.lib.status = "I could not make a stamp from that picture.".into(),
        }
    }

    #[cfg(test)]
    pub fn lib_meta_mut(&mut self) -> &mut UserBrush {
        &mut self.lib.meta
    }

    #[cfg(test)]
    pub fn brush_list_has(&self, name: &str) -> bool {
        self.lib.presets.iter().any(|p| p.name == name && p.category == "My brushes")
    }

    pub(super) fn my_brush_section(&mut self, ui: &mut Ui) {
        let sl = slot(self.tool);
        if sl == 1 {
            return; // erasers are not saved here
        }
        egui::CollapsingHeader::new(RichText::new("My brush").size(13.0).strong().color(TEXT)).default_open(false).show(ui, |ui| {
            ui.spacing_mut().item_spacing = vec2(4.0, 4.0);
            let is_mine = self.lib.user.iter().any(|b| b.name == self.lib.mem[sl].name);
            if self.lib.meta.name == "My brush" && !is_mine {
                self.lib.meta.name = format!("{} copy", self.lib.mem[sl].name);
            }
            ui.horizontal(|ui| {
                ui.label(RichText::new("Name").size(12.0).color(TEXT_DIM));
                ui.add(egui::TextEdit::singleline(&mut self.lib.meta.name).desired_width(f32::INFINITY).char_limit(60));
            });
            ui.add(egui::TextEdit::singleline(&mut self.lib.meta.about).hint_text("What it is for").desired_width(f32::INFINITY).char_limit(400));
            ui.horizontal(|ui| {
                ui.add(egui::TextEdit::singleline(&mut self.lib.meta.author).hint_text("Made by").desired_width(110.0).char_limit(80));
                ui.add(egui::TextEdit::singleline(&mut self.lib.meta.license).hint_text("License (CC0, CC BY…)").desired_width(f32::INFINITY).char_limit(80));
            });
            ui.add(egui::TextEdit::multiline(&mut self.lib.meta.terms).hint_text("Terms of use (optional)").desired_rows(2).desired_width(f32::INFINITY).char_limit(1000));
            ui.horizontal_wrapped(|ui| {
                if widgets::chip(ui, "Save to My brushes", false).on_hover_text("A new name makes a new brush, the same name replaces it").clicked() {
                    self.save_current_brush();
                }
                if is_mine && widgets::chip(ui, "Delete", false).on_hover_text("Remove this brush from My brushes").clicked() {
                    let name = self.lib.mem[sl].name.clone();
                    userlib::delete_in(&userlib::dir(), &name);
                    self.reload_user_brushes();
                    self.lib.status = format!("Removed \"{name}\".");
                }
                if widgets::chip(ui, "Remix", false).on_hover_text("A close cousin of this brush: small changes to many settings").clicked() {
                    self.lib.roll_count = self.lib.roll_count.wrapping_add(1);
                    let seed = self.lib.roll_count.wrapping_mul(2_654_435_761).wrapping_add(self.lib.mem[sl].s.seed);
                    crate::brush::random::remix(&mut self.lib.mem[sl].s, seed, 0.25);
                }
            });
            ui.horizontal_wrapped(|ui| {
                if widgets::chip(ui, "Brush card…", false).on_hover_text("A picture of the brush with the brush inside it: share it anywhere").clicked() {
                    let b = self.current_user_brush();
                    if let Some(p) = rfd::FileDialog::new().set_title("Save a brush card").set_file_name(format!("{}.png", userlib::slug(&b.name))).add_filter("Brush card", &["png"]).save_file() {
                        self.lib.status = match userlib::card_png(&b, &card_picture(&b)).and_then(|bytes| std::fs::write(&p, bytes).map_err(|e| e.to_string())) {
                            Ok(()) => "Saved the brush card.".into(),
                            Err(e) => e,
                        };
                    }
                }
                if widgets::chip(ui, "Brush file…", false).on_hover_text("The brush as a small .seviathbrush file").clicked() {
                    let b = self.current_user_brush();
                    if let Some(p) = rfd::FileDialog::new().set_title("Save a brush file").set_file_name(format!("{}.{}", userlib::slug(&b.name), userlib::EXT)).add_filter("SeviathStudio brush", &[userlib::EXT]).save_file() {
                        self.lib.status = match std::fs::write(&p, userlib::to_json(&[b])) {
                            Ok(()) => "Saved the brush file.".into(),
                            Err(e) => format!("Can't save: {e}"),
                        };
                    }
                }
                if widgets::chip(ui, "For Photoshop (.abr)…", false).on_hover_text("A Photoshop brush file; Clip Studio Paint opens these too. Tip, size, spacing and pressure go across.").clicked() {
                    let b = self.current_user_brush();
                    if let Some(p) = rfd::FileDialog::new().set_title("Save a Photoshop brush").set_file_name(format!("{}.abr", userlib::slug(&b.name))).add_filter("Photoshop brushes", &["abr"]).save_file() {
                        self.lib.status = match std::fs::write(&p, crate::brush::formats::to_abr(&[b])) {
                            Ok(()) => "Saved the .abr file.".into(),
                            Err(e) => format!("Can't save: {e}"),
                        };
                    }
                }
                if widgets::chip(ui, "For Krita (.kpp)…", false).on_hover_text("A Krita brush preset with the tip inside").clicked() {
                    let b = self.current_user_brush();
                    if let Some(p) = rfd::FileDialog::new().set_title("Save a Krita brush").set_file_name(format!("{}.kpp", userlib::slug(&b.name))).add_filter("Krita presets", &["kpp"]).save_file() {
                        self.lib.status = match crate::brush::formats::to_kpp(&b, &card_picture(&b)).and_then(|bytes| std::fs::write(&p, bytes).map_err(|e| e.to_string())) {
                            Ok(()) => "Saved the .kpp preset.".into(),
                            Err(e) => e,
                        };
                    }
                }
                if widgets::chip(ui, "Tip picture…", false).on_hover_text("Just the tip as a PNG: black on white").clicked() {
                    let b = self.current_user_brush();
                    if let Some(p) = rfd::FileDialog::new().set_title("Save the tip as a picture").set_file_name(format!("{}-tip.png", userlib::slug(&b.name))).add_filter("PNG picture", &["png"]).save_file() {
                        self.lib.status = match std::fs::write(&p, crate::brush::formats::tip_png(&b.settings, 512)) {
                            Ok(()) => "Saved the tip picture.".into(),
                            Err(e) => format!("Can't save: {e}"),
                        };
                    }
                }
                if widgets::chip(ui, "Open brushes…", false).on_hover_text("Brush files, brush cards and pictures (a picture becomes a stamp brush)").clicked() {
                    if let Some(paths) = rfd::FileDialog::new().set_title("Open brushes").add_filter("Brushes and pictures", &[userlib::EXT, "json", "png", "jpg", "jpeg", "webp", "bmp", "gif", "abr", "gbr", "gih", "kpp", "bundle"]).pick_files() {
                        self.import_brushes(&paths);
                    }
                }
            });
            ui.horizontal_wrapped(|ui| {
                if widgets::chip(ui, "Stamp from selection", false).on_hover_text("The painted pixels inside your selection become the brush's stamp").clicked() {
                    self.tip_from_selection();
                }
                if widgets::chip(ui, "Stamp from picture…", false).on_hover_text("A picture (dark or opaque parts paint) becomes the brush's stamp").clicked() {
                    if let Some(p) = rfd::FileDialog::new().set_title("Picture for the stamp").add_filter("Pictures", &["png", "jpg", "jpeg", "webp", "bmp", "gif"]).pick_file() {
                        self.tip_from_file(&p);
                    }
                }
            });
            ui.add_space(4.0);
            ui.label(RichText::new("Materials: textures and stamps you keep").size(12.0).color(TEXT));
            ui.horizontal(|ui| {
                ui.label(RichText::new("Name").size(11.5).color(TEXT_DIM));
                ui.add(egui::TextEdit::singleline(&mut self.lib.mat_name).hint_text("for the next material").desired_width(f32::INFINITY).char_limit(60));
            });
            ui.horizontal_wrapped(|ui| {
                if widgets::chip(ui, "Texture from selection", false).on_hover_text("The painted pixels in your selection become a repeating texture for this brush (dark paints, light holds paint back)").clicked() {
                    self.texture_from_selection();
                }
                if widgets::chip(ui, "Texture from picture…", false).clicked() {
                    if let Some(p) = rfd::FileDialog::new().set_title("Picture for the texture").add_filter("Pictures", &["png", "jpg", "jpeg", "webp", "bmp", "gif"]).pick_file() {
                        self.texture_from_file(&p);
                    }
                }
                ui.checkbox(&mut self.lib.seamless, "Make it seamless").on_hover_text("Blends the edges so the texture repeats without a visible seam");
            });
            if self.lib.mem[sl].s.tip_image.is_some() && widgets::chip(ui, "Keep this stamp as a material", false).clicked() {
                let name = if self.lib.mat_name.trim().is_empty() { format!("Stamp {}", self.lib.stamps.len() + 1) } else { self.lib.mat_name.trim().to_string() };
                let t = self.lib.mem[sl].s.tip_image.clone();
                if let Some(t) = t {
                    self.lib.status = match userlib::save_material_in(&userlib::materials_dir(), "tips", &name, &t) {
                        Ok(_) => format!("Kept \"{name}\" in your materials."),
                        Err(e) => e,
                    };
                    self.reload_materials();
                }
            }
            let mut use_texture = None;
            let mut use_stamp = None;
            let mut drop_material: Option<(&str, String)> = None;
            if !self.lib.textures.is_empty() {
                ui.label(RichText::new("Your textures (click to use, right-click to remove)").size(11.0).color(TEXT_DIM));
                ui.horizontal_wrapped(|ui| {
                    for (n, t) in &self.lib.textures {
                        let r = widgets::chip(ui, n, false);
                        if r.clicked() {
                            use_texture = Some(t.clone());
                        }
                        if r.secondary_clicked() {
                            drop_material = Some(("textures", n.clone()));
                        }
                    }
                });
            }
            if !self.lib.stamps.is_empty() {
                ui.label(RichText::new("Your stamps").size(11.0).color(TEXT_DIM));
                ui.horizontal_wrapped(|ui| {
                    for (n, t) in &self.lib.stamps {
                        let r = widgets::chip(ui, n, false);
                        if r.clicked() {
                            use_stamp = Some(t.clone());
                        }
                        if r.secondary_clicked() {
                            drop_material = Some(("tips", n.clone()));
                        }
                    }
                });
            }
            if let Some(t) = use_texture {
                let b = &mut self.lib.mem[sl].s;
                b.tex_kind = crate::brush::TexKind::Custom;
                b.texture = b.texture.max(0.55);
                b.tex_image = Some(t);
            }
            if let Some(t) = use_stamp {
                let b = &mut self.lib.mem[sl].s;
                b.tip = Tip::Image;
                b.tip_image = Some(t);
                b.spacing = b.spacing.max(0.15);
            }
            if let Some((kind, name)) = drop_material {
                userlib::delete_material_in(&userlib::materials_dir(), kind, &name);
                self.reload_materials();
            }
            if !self.lib.status.is_empty() {
                ui.label(RichText::new(self.lib.status.clone()).size(11.5).color(VENOM));
            }
            ui.label(RichText::new(format!("Brushes: {}\nMaterials: {}", userlib::dir().display(), userlib::materials_dir().display())).size(10.5).color(TEXT_DIM));
        });
        let _ = section_title;
    }
}
