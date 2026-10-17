// SPDX-License-Identifier: GPL-3.0-or-later
// The Layout menu: switch workspaces, keep your own, share them as files, show or hide panels.

use eframe::egui::{self, RichText, Ui};

use super::SeviathApp;
use crate::layout::{self, Layouts, Mode, PanelId};
use crate::theme::*;

impl SeviathApp {
    pub(super) fn layout_menu(&mut self, ui: &mut Ui) {
        let now = ui.ctx().input(|i| i.time);
        let mut say: Option<String> = None;
        ui.menu_button(RichText::new("Layout").color(TEXT), |ui| {
            ui.set_min_width(250.0);
            ui.label(RichText::new("Workspaces").size(11.5).color(TEXT_DIM));
            let ls = &mut self.draw.layouts;
            for name in ls.names() {
                let on = name == ls.active;
                let label = if on && ls.changed() { format!("{name} (changed)") } else { name.clone() };
                if ui.selectable_label(on, label).on_hover_text(if on { "Click to put the panels back as saved" } else { "Switch to this workspace" }).clicked() {
                    ls.switch(&name);
                    ui.close();
                }
            }
            ui.separator();
            ui.label(RichText::new("Panels").size(11.5).color(TEXT_DIM));
            for id in PanelId::ALL {
                let mut shown = ls.working.get(id).mode != Mode::Hidden;
                if ui.checkbox(&mut shown, id.title()).changed() {
                    ls.working.toggle_hidden(id);
                }
            }
            ui.separator();
            ui.horizontal(|ui| {
                ui.add(egui::TextEdit::singleline(&mut self.layout_name).hint_text("Name").desired_width(120.0).char_limit(40));
                if ui.button("Save as workspace").clicked() {
                    let name = self.layout_name.clone();
                    match self.draw.layouts.save_as(&name) {
                        Ok(()) => {
                            say = Some(format!("Workspace “{}” saved.", self.draw.layouts.active));
                            self.layout_name.clear();
                        }
                        Err(e) => say = Some(e),
                    }
                }
            });
            let active = self.draw.layouts.active.clone();
            if !Layouts::is_builtin(&active) && ui.button(format!("Delete “{active}”")).clicked() {
                self.draw.layouts.delete(&active);
                ui.close();
            }
            ui.horizontal(|ui| {
                if ui.button("Export…").on_hover_text("Save this layout as a file to share").clicked() {
                    ui.close();
                    let name = self.draw.layouts.active.clone();
                    if let Some(p) = rfd::FileDialog::new().set_title("Export the workspace").set_file_name(format!("{name}.seviathworkspace")).add_filter("Workspace", &["seviathworkspace", "json"]).save_file() {
                        say = Some(match layout::export(&p, &name, &self.draw.layouts.working) {
                            Ok(()) => "Workspace saved as a file.".into(),
                            Err(e) => e,
                        });
                    }
                }
                if ui.button("Import…").clicked() {
                    ui.close();
                    if let Some(p) = rfd::FileDialog::new().set_title("Import a workspace").add_filter("Workspace", &["seviathworkspace", "json"]).pick_file() {
                        say = Some(match layout::import(&p) {
                            Ok((name, l)) => {
                                self.draw.layouts.working = l;
                                let name = if Layouts::is_builtin(&name) { format!("{name} (imported)") } else { name };
                                match self.draw.layouts.save_as(&name) {
                                    Ok(()) => format!("Workspace “{name}” added."),
                                    Err(e) => e,
                                }
                            }
                            Err(e) => e,
                        });
                    }
                }
            });
        });
        if let Some(t) = say {
            self.say(t, now);
        }
    }
}
