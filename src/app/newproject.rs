// SPDX-License-Identifier: GPL-3.0-or-later
// The New project dialog (templates, print size and DPI, background, guides, rig-ready folders,
// your own templates) and the Project info window (DPI, notes, tags, guides, autosave).

use eframe::egui::{self, Align2, RichText, vec2};

use super::SeviathApp;
use super::files::Pending;
use crate::files::{self, template};
use crate::files::template::{BUILTIN, Background, DPI_CHOICES, NewProject, Template};
use crate::meta::{Guide, GuidePreset, ProjectMeta, Unit, parse_tags, real_size};
use crate::theme::*;
use crate::widgets::{self, section_title};

/// Editing copy for the Project info window.
pub struct Draft {
    meta: ProjectMeta,
    tags: String,
    own_autosave: bool,
    guide_vertical: bool,
    guide_pos: f32,
}

#[derive(Default)]
pub struct NewState {
    pub form: NewProject,
    templates: Vec<Template>,
    templates_loaded: bool,
    tpl_name: String,
    message: Option<String>,
    pub info_open: bool,
    draft: Option<Draft>,
}

fn unit_range(unit: Unit) -> (f32, f32, f32) {
    // (speed, min, max)
    match unit {
        Unit::Px => (10.0, 16.0, files::MAX_SIDE as f32),
        Unit::Mm => (1.0, 1.0, 5000.0),
        Unit::Inch => (0.05, 0.1, 200.0),
    }
}

fn dpi_row(ui: &mut egui::Ui, dpi: &mut u32) {
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing = vec2(4.0, 4.0);
        ui.label("DPI");
        for d in DPI_CHOICES {
            if widgets::chip(ui, &d.to_string(), *dpi == d).clicked() {
                *dpi = d;
            }
        }
        ui.add(egui::DragValue::new(dpi).range(1..=2400).speed(1));
    });
}

fn color3(ui: &mut egui::Ui, c: &mut [u8; 3]) {
    egui::color_picker::color_edit_button_srgb(ui, c);
}

impl SeviathApp {
    pub(super) fn open_new_project(&mut self) {
        self.files.np.templates_loaded = false;
        self.files.new_open = true;
    }

    pub(super) fn new_project_window(&mut self, ctx: &egui::Context) {
        if !self.files.new_open {
            return;
        }
        if !self.files.np.templates_loaded {
            self.files.np.templates = template::list_in(&template::templates_dir());
            self.files.np.templates_loaded = true;
        }
        let mut open = true;
        let mut create = false;
        let mut import = false;
        let mut save_tpl = false;
        let mut delete_tpl: Option<String> = None;
        let mut export_tpl: Option<Template> = None;
        let mut use_tpl: Option<Template> = None;
        egui::Window::new(RichText::new("New project").strong())
            .open(&mut open)
            .collapsible(false)
            .resizable(false)
            .pivot(Align2::CENTER_CENTER)
            .default_pos(ctx.content_rect().center())
            .show(ctx, |ui| {
                ui.set_width(560.0);
                let np = &mut self.files.np;
                ui.horizontal(|ui| {
                    ui.label("Name");
                    ui.add(egui::TextEdit::singleline(&mut np.form.name).desired_width(280.0).char_limit(80));
                    if ui.button(RichText::new("Create").strong()).clicked() {
                        create = true;
                    }
                });
                egui::ScrollArea::vertical().max_height((ctx.content_rect().height() - 330.0).max(280.0)).show(ui, |ui| {
                    let f = &mut np.form;
                    section_title(ui, "Start from");
                    ui.horizontal_wrapped(|ui| {
                        ui.spacing_mut().item_spacing = vec2(4.0, 4.0);
                        for b in BUILTIN {
                            let on = f.w == b.w && f.h == b.h && f.dpi == b.dpi;
                            let tip = format!("{} × {} px · {} DPI · {}", b.w, b.h, b.dpi, real_size(b.w, b.h, b.dpi));
                            if widgets::chip(ui, b.name, on).on_hover_text(tip).clicked() {
                                f.use_builtin(b);
                            }
                        }
                    });
                    if !np.templates.is_empty() {
                        ui.label(RichText::new("My templates").size(12.0).color(TEXT_DIM));
                        ui.horizontal_wrapped(|ui| {
                            ui.spacing_mut().item_spacing = vec2(4.0, 4.0);
                            for t in &np.templates {
                                let r = widgets::chip(ui, &t.name, false).on_hover_text("Click to use · right-click for more");
                                if r.clicked() {
                                    use_tpl = Some(t.clone());
                                }
                                r.context_menu(|ui| {
                                    if ui.button("Export to a file…").clicked() {
                                        export_tpl = Some(t.clone());
                                        ui.close();
                                    }
                                    if ui.button("Delete this template").clicked() {
                                        delete_tpl = Some(t.name.clone());
                                        ui.close();
                                    }
                                });
                            }
                        });
                    }
                    ui.add_space(4.0);

                    section_title(ui, "Size and print resolution");
                    ui.horizontal(|ui| {
                        ui.spacing_mut().item_spacing.x = 4.0;
                        ui.label("Type sizes in");
                        for (u, label) in Unit::ALL {
                            if widgets::chip(ui, label, f.unit == u).clicked() {
                                f.unit = u;
                            }
                        }
                    });
                    let (mut vw, mut vh) = f.size_in_unit();
                    let (speed, lo, hi) = unit_range(f.unit);
                    let dec = if f.unit == Unit::Px { 0 } else { 2 };
                    ui.horizontal(|ui| {
                        ui.label("Width");
                        let a = ui.add(egui::DragValue::new(&mut vw).speed(speed).range(lo..=hi).max_decimals(dec));
                        ui.label("Height");
                        let b = ui.add(egui::DragValue::new(&mut vh).speed(speed).range(lo..=hi).max_decimals(dec));
                        if a.changed() {
                            f.w = f.unit.to_px(vw, f.dpi).round().clamp(16.0, files::MAX_SIDE as f32) as u32;
                        }
                        if b.changed() {
                            f.h = f.unit.to_px(vh, f.dpi).round().clamp(16.0, files::MAX_SIDE as f32) as u32;
                        }
                        if ui.small_button("Swap").on_hover_text("Switch portrait and landscape").clicked() {
                            std::mem::swap(&mut f.w, &mut f.h);
                        }
                    });
                    dpi_row(ui, &mut f.dpi);
                    ui.label(RichText::new(format!("{} × {} px = {} at {} DPI", f.w, f.h, real_size(f.w, f.h, f.dpi), f.dpi)).size(12.0).color(VENOM));
                    let (mem, warn) = template::memory_text(f.w, f.h);
                    ui.label(RichText::new(mem).size(11.5).color(if warn { GOLD } else { TEXT_DIM }));
                    ui.label(RichText::new("Changing DPI keeps the pixels and changes the print size. The DPI is written into PNG, JPEG, PSD and OpenRaster exports.").size(11.5).color(TEXT_DIM));
                    ui.add_space(4.0);

                    section_title(ui, "Background");
                    ui.horizontal(|ui| {
                        ui.spacing_mut().item_spacing.x = 4.0;
                        let is_paper = matches!(f.bg, Background::Paper(_));
                        let is_clear = f.bg == Background::Transparent;
                        let is_grad = matches!(f.bg, Background::Gradient { .. });
                        if widgets::chip(ui, "Paper", is_paper).clicked() && !is_paper {
                            f.bg = Background::Paper([255, 255, 255]);
                        }
                        if widgets::chip(ui, "Transparent", is_clear).clicked() {
                            f.bg = Background::Transparent;
                        }
                        if widgets::chip(ui, "Gradient", is_grad).on_hover_text("A top-to-bottom two-color start on its own layer").clicked() && !is_grad {
                            f.bg = Background::Gradient { top: [255, 255, 255], bottom: [170, 200, 235] };
                        }
                        match &mut f.bg {
                            Background::Paper(c) => color3(ui, c),
                            Background::Gradient { top, bottom } => {
                                color3(ui, top);
                                ui.label("to");
                                color3(ui, bottom);
                            }
                            Background::Transparent => {}
                        }
                    });
                    ui.add_space(4.0);

                    section_title(ui, "Guides and folders");
                    ui.horizontal_wrapped(|ui| {
                        ui.spacing_mut().item_spacing = vec2(4.0, 4.0);
                        for (p, label, about) in GuidePreset::ALL {
                            if widgets::chip(ui, label, f.guides == p).on_hover_text(about).clicked() {
                                f.guides = p;
                            }
                        }
                    });
                    ui.checkbox(&mut f.rig_starter, "Rig-ready folders (hair, brows, eyes, nose and mouth, face, body)")
                        .on_hover_text("Pre-made, color-labeled folders so every part of a character has its place from the first stroke");
                    ui.add_space(4.0);

                    section_title(ui, "Notes and tags");
                    ui.add(egui::TextEdit::multiline(&mut f.notes).hint_text("Notes about this project").desired_rows(2).desired_width(f32::INFINITY).char_limit(4000));
                    ui.horizontal(|ui| {
                        ui.label("Tags");
                        ui.add(egui::TextEdit::singleline(&mut f.tags).hint_text("hair, commission, wip").desired_width(280.0).char_limit(200));
                    });
                });
                ui.add_space(6.0);
                ui.separator();
                ui.horizontal(|ui| {
                    ui.add(egui::TextEdit::singleline(&mut np.tpl_name).hint_text("Template name").desired_width(150.0).char_limit(40));
                    if ui.add_enabled(!np.tpl_name.trim().is_empty(), egui::Button::new("Save as my template")).on_hover_text("Size, DPI, background, guides, folders and tags").clicked() {
                        save_tpl = true;
                    }
                    if ui.button("Import…").on_hover_text("A template file someone shared").clicked() {
                        import = true;
                    }
                });
                if let Some(m) = &np.message {
                    ui.label(RichText::new(m).size(12.0).color(GOLD));
                }
            });

        let dir = template::templates_dir();
        if let Some(t) = use_tpl {
            self.files.np.form.use_template(&t);
        }
        if let Some(name) = delete_tpl {
            template::delete_in(&dir, &name);
            self.files.np.templates_loaded = false;
        }
        if let Some(t) = export_tpl {
            if let Some(path) = rfd::FileDialog::new().set_title("Export template").set_file_name(format!("{}.json", t.name)).add_filter("Template", &["json"]).save_file() {
                let res = serde_json::to_string_pretty(&t).map_err(|e| e.to_string()).and_then(|s| std::fs::write(path, s).map_err(|e| e.to_string()));
                self.files.np.message = Some(match res {
                    Ok(()) => "Template exported.".into(),
                    Err(e) => format!("Couldn't export: {e}"),
                });
            }
        }
        if import {
            if let Some(path) = rfd::FileDialog::new().set_title("Import template").add_filter("Template", &["json"]).pick_file() {
                self.files.np.message = Some(match template::read_file(&path).and_then(|t| template::save_in(&dir, &t).map(|_| t)) {
                    Ok(t) => {
                        self.files.np.form.use_template(&t);
                        self.files.np.templates_loaded = false;
                        format!("Imported \"{}\".", t.name)
                    }
                    Err(e) => e,
                });
            }
        }
        if save_tpl {
            let t = self.files.np.form.to_template(&self.files.np.tpl_name);
            self.files.np.message = Some(match template::save_in(&dir, &t) {
                Ok(()) => format!("Saved \"{}\" in My templates.", t.name),
                Err(e) => e,
            });
            self.files.np.tpl_name.clear();
            self.files.np.templates_loaded = false;
        }
        if create {
            open = false;
            let np = self.files.np.form.clone();
            self.request(Pending::New(np));
        }
        self.files.new_open = open;
    }

    // ---------- Project info ----------

    pub(super) fn open_project_info(&mut self) {
        let m = self.draw.doc.meta.clone();
        self.files.np.draft = Some(Draft {
            tags: m.tags.join(", "),
            own_autosave: m.autosave_minutes.is_some() || m.recovery_copies.is_some(),
            guide_vertical: true,
            guide_pos: (self.draw.doc.w / 2) as f32,
            meta: m,
        });
        self.files.np.info_open = true;
    }

    pub(super) fn project_info_window(&mut self, ctx: &egui::Context) {
        if !self.files.np.info_open {
            return;
        }
        let (w, h) = (self.draw.doc.w, self.draw.doc.h);
        let (global_minutes, global_copies) = (self.settings.autosave_minutes, self.settings.recovery_copies);
        let mut open = true;
        let mut apply = false;
        let mut cancel = false;
        egui::Window::new(RichText::new("Project info").strong())
            .open(&mut open)
            .collapsible(false)
            .resizable(false)
            .pivot(Align2::CENTER_CENTER)
            .default_pos(ctx.content_rect().center())
            .show(ctx, |ui| {
                ui.set_width(500.0);
                let Some(d) = self.files.np.draft.as_mut() else { return };
                ui.label(RichText::new(format!("{w} × {h} px")).color(TEXT_DIM));
                section_title(ui, "Print size");
                dpi_row(ui, &mut d.meta.dpi);
                ui.label(RichText::new(real_size(w, h, d.meta.dpi)).color(VENOM));
                ui.add_space(4.0);

                section_title(ui, "Notes and tags");
                ui.add(egui::TextEdit::multiline(&mut d.meta.notes).hint_text("Notes about this project").desired_rows(3).desired_width(f32::INFINITY).char_limit(4000));
                ui.horizontal(|ui| {
                    ui.label("Tags");
                    ui.add(egui::TextEdit::singleline(&mut d.tags).hint_text("hair, commission, wip").desired_width(300.0).char_limit(200));
                });
                ui.label(RichText::new("Tags and notes can be searched in the project manager.").size(11.5).color(TEXT_DIM));
                ui.add_space(4.0);

                section_title(ui, "Guide lines");
                ui.horizontal_wrapped(|ui| {
                    ui.spacing_mut().item_spacing = vec2(4.0, 4.0);
                    for (p, label, about) in GuidePreset::ALL {
                        if widgets::chip(ui, label, false).on_hover_text(about).clicked() {
                            d.meta.guides = p.build(w, h);
                        }
                    }
                });
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 4.0;
                    if widgets::chip(ui, "Vertical", d.guide_vertical).clicked() {
                        d.guide_vertical = true;
                    }
                    if widgets::chip(ui, "Horizontal", !d.guide_vertical).clicked() {
                        d.guide_vertical = false;
                    }
                    let max = if d.guide_vertical { w } else { h } as f32;
                    ui.add(egui::DragValue::new(&mut d.guide_pos).range(0.0..=max).speed(5.0).suffix(" px").max_decimals(0));
                    if ui.button("Add").clicked() && d.meta.guides.len() < 200 {
                        d.meta.guides.push(Guide { vertical: d.guide_vertical, pos: d.guide_pos.round() });
                    }
                });
                if d.meta.guides.is_empty() {
                    ui.label(RichText::new("No guide lines.").size(11.5).color(TEXT_DIM));
                } else {
                    let mut remove = None;
                    ui.horizontal_wrapped(|ui| {
                        ui.spacing_mut().item_spacing = vec2(4.0, 4.0);
                        for (i, g) in d.meta.guides.iter().enumerate() {
                            let label = format!("{} {:.0}", if g.vertical { "x" } else { "y" }, g.pos);
                            if widgets::chip(ui, &label, false).on_hover_text("Click to remove").clicked() {
                                remove = Some(i);
                            }
                        }
                    });
                    if let Some(i) = remove {
                        d.meta.guides.remove(i);
                    }
                    if ui.small_button("Remove all").clicked() {
                        d.meta.guides.clear();
                    }
                }
                ui.label(RichText::new("Show or hide guides in Settings, Canvas and view.").size(11.5).color(TEXT_DIM));
                ui.add_space(4.0);

                section_title(ui, "Autosave for this project");
                ui.checkbox(&mut d.own_autosave, "Use its own autosave settings instead of the global ones");
                if d.own_autosave {
                    let mut minutes = d.meta.autosave_minutes.unwrap_or(global_minutes);
                    let mut copies = d.meta.recovery_copies.unwrap_or(global_copies);
                    ui.horizontal(|ui| {
                        ui.label("Recovery copy every");
                        ui.add(egui::Slider::new(&mut minutes, 0.0..=30.0).step_by(0.5).suffix(" min").max_decimals(1));
                    });
                    ui.horizontal(|ui| {
                        ui.label("Keep the newest");
                        ui.add(egui::Slider::new(&mut copies, 1..=50).suffix(" copies"));
                    });
                    d.meta.autosave_minutes = Some(minutes);
                    d.meta.recovery_copies = Some(copies);
                } else {
                    d.meta.autosave_minutes = None;
                    d.meta.recovery_copies = None;
                }
                ui.add_space(8.0);
                ui.horizontal(|ui| {
                    if ui.button(RichText::new("Apply").strong()).clicked() {
                        apply = true;
                    }
                    if ui.button("Cancel").clicked() {
                        cancel = true;
                    }
                });
            });
        if apply {
            if let Some(mut d) = self.files.np.draft.take() {
                d.meta.tags = parse_tags(&d.tags);
                let meta = d.meta.sanitized();
                self.draw.apply_external("Project info", |s| s.doc.meta = meta);
            }
            open = false;
        }
        if cancel {
            open = false;
        }
        if !open {
            self.files.np.draft = None;
        }
        self.files.np.info_open = open;
    }

    #[cfg(test)]
    pub fn test_create(&mut self, np: NewProject) {
        self.request(Pending::New(np));
    }

    #[cfg(test)]
    pub fn test_set_info(&mut self, meta: ProjectMeta) {
        self.draw.apply_external("Project info", |s| s.doc.meta = meta);
    }

    #[cfg(test)]
    pub fn test_open_info(&mut self) {
        self.open_project_info();
    }
}
