// SPDX-License-Identifier: GPL-3.0-or-later
// Runs the actions of the action table (actions.rs) from keys and from the command palette (Ctrl+K).

use eframe::egui::{self, Align2, Key, Modifiers, RichText};

use super::{SeviathApp, Workspace};
use crate::actions::{self, ACTIONS, ActionDef};
use crate::tabs::Tab;
use crate::tabs::draw::actions_ui::is_draw_action;
use crate::theme::*;

#[derive(Default)]
pub struct Palette {
    pub open: bool,
    pub query: String,
    pub sel: usize,
    focus: bool,
    /// The last actions you ran from the palette, newest first.
    recent: Vec<&'static str>,
}

impl Palette {
    /// The actions to show for what is typed.
    pub fn results(&self) -> Vec<&'static ActionDef> {
        let mut v: Vec<(i32, usize, &'static ActionDef)> = ACTIONS.iter().enumerate().filter_map(|(i, d)| actions::match_score(&self.query, d).map(|s| (s, i, d))).collect();
        if self.query.trim().is_empty() {
            // nothing typed: what you used last first
            let rank = |d: &ActionDef| self.recent.iter().position(|r| *r == d.id).map(|p| 1000 - p as i32).unwrap_or(0);
            v.sort_by_key(|(_, i, d)| (std::cmp::Reverse(rank(d)), *i));
        } else {
            v.sort_by_key(|(s, i, _)| (std::cmp::Reverse(*s), *i));
        }
        v.into_iter().map(|x| x.2).take(40).collect()
    }
}

impl SeviathApp {
    /// Every frame: the keys of the actions outside the Draw tab's own canvas.
    pub(super) fn command_keys(&mut self, ctx: &egui::Context) {
        let typing = ctx.egui_wants_keyboard_input();
        let keys = self.settings.shortcuts.clone();
        let fired = ctx.input_mut(|i| actions::pressed(i, &keys, ACTIONS.iter().filter(|d| !is_draw_action(d.id) && (d.typing_ok || !typing) && !(d.id == "rig.keypose" && self.tab != Tab::Rig))));
        let redo_alias = !typing && ctx.input_mut(|i| i.consume_key(Modifiers::COMMAND | Modifiers::SHIFT, Key::Z));
        for id in fired {
            // the pose key only means something in the Rig tab
            if id == "rig.keypose" && self.tab != Tab::Rig {
                continue;
            }
            self.run_action(id, ctx);
        }
        if redo_alias {
            self.run_action("edit.redo", ctx);
        }
    }

    /// Run an action by its id (from a key or from the palette).
    pub fn run_action(&mut self, id: &str, ctx: &egui::Context) {
        let now = ctx.input(|i| i.time);
        match id {
            "file.new" => self.open_new_project(),
            "file.open" => self.open_dialog(),
            "file.save" => self.save(None),
            "file.save_as" => self.save_as(None),
            "file.manager" => self.files.manager_open = true,
            "file.export" => self.export_dialog(),
            "file.info" => self.open_project_info(),
            "edit.undo" => self.undo_tab(now),
            "edit.undo_all" => self.undo_all(now),
            "edit.redo" => self.redo(now),
            "edit.history" => self.history_open = !self.history_open,
            "app.settings" => self.settings_open = !self.settings_open,
            "app.palette" => {
                self.palette.open = !self.palette.open;
                self.palette.query.clear();
                self.palette.sel = 0;
                self.palette.focus = true;
            }
            "win.recorder" => self.open_recorder(),
            "win.signatures" => self.open_signatures(),
            "win.fonts" => self.open_font_maker(),
            "win.work" => self.open_work_history(),
            "win.prerig" => self.open_prerig(),
            "rig.keypose" => {
                self.tab = Tab::Rig;
                self.rig_key_pose(now);
            }
            _ if id.starts_with("tab.") => {
                if let Some(t) = id[4..].parse::<usize>().ok().and_then(|n| Tab::ALL.get(n.wrapping_sub(1))) {
                    self.tab = *t;
                    self.workspace = Workspace::Character;
                }
            }
            _ if is_draw_action(id) => {
                self.tab = Tab::Draw;
                self.workspace = Workspace::Character;
                self.draw.run_draw_action(id, ctx);
            }
            _ => {}
        }
    }

    #[cfg(test)]
    pub fn test_set_shortcut(&mut self, id: &str, text: &str) {
        actions::assign(&mut self.settings.shortcuts, id, actions::Shortcut::parse(text));
    }

    #[cfg(test)]
    pub fn test_shortcut(&self, id: &str) -> String {
        actions::effective_text(&self.settings.shortcuts, id)
    }

    #[cfg(test)]
    pub fn test_open_shortcuts_page(&mut self) {
        self.settings_open = true;
        self.settings_page = crate::settings_ui::Page::Shortcuts;
    }

    #[cfg(test)]
    pub fn test_run_action_now(&mut self, id: &str) {
        self.run_action(id, &eframe::egui::Context::default());
    }

    #[cfg(test)]
    pub fn test_run_action(&mut self, id: &str, ctx: &egui::Context) {
        self.run_action(id, ctx);
    }

    pub(super) fn palette_window(&mut self, ctx: &egui::Context) {
        if !self.palette.open {
            return;
        }
        let results = self.palette.results();
        let n = results.len();
        let (mut run, mut close) = (None::<&'static str>, false);
        ctx.input_mut(|i| {
            if i.consume_key(Modifiers::NONE, Key::Escape) {
                close = true;
            }
            if n > 0 {
                if i.consume_key(Modifiers::NONE, Key::ArrowDown) {
                    self.palette.sel = (self.palette.sel + 1) % n;
                }
                if i.consume_key(Modifiers::NONE, Key::ArrowUp) {
                    self.palette.sel = (self.palette.sel + n - 1) % n;
                }
                if i.consume_key(Modifiers::NONE, Key::Enter) {
                    run = results.get(self.palette.sel.min(n - 1)).map(|d| d.id);
                }
            }
        });
        egui::Window::new("Command palette")
            .title_bar(false)
            .resizable(false)
            .collapsible(false)
            .anchor(Align2::CENTER_TOP, [0.0, 70.0])
            .fixed_size([480.0, 0.0])
            .show(ctx, |ui| {
                let edit = ui.add(egui::TextEdit::singleline(&mut self.palette.query).hint_text("Type what you want to do…").desired_width(f32::INFINITY));
                if self.palette.focus {
                    edit.request_focus();
                    self.palette.focus = false;
                }
                if edit.changed() {
                    self.palette.sel = 0;
                }
                ui.add_space(4.0);
                egui::ScrollArea::vertical().max_height(360.0).show(ui, |ui| {
                    for (i, d) in results.iter().enumerate() {
                        let on = i == self.palette.sel.min(n.saturating_sub(1));
                        let key = actions::effective_text(&self.settings.shortcuts, d.id);
                        let r = ui.horizontal(|ui| {
                            ui.set_min_width(ui.available_width());
                            let t = RichText::new(d.name).color(if on { TEXT } else { TEXT_DIM });
                            let resp = ui.selectable_label(on, t);
                            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                ui.label(RichText::new(key).size(11.5).color(TEXT_DIM));
                                ui.label(RichText::new(d.group).size(11.5).color(TEXT_DIM));
                            });
                            resp
                        });
                        if on {
                            r.inner.scroll_to_me(None);
                        }
                        if r.inner.clicked() {
                            run = Some(d.id);
                        }
                    }
                    if results.is_empty() {
                        ui.label(RichText::new("Nothing like that.").color(TEXT_DIM));
                    }
                });
            });
        if close {
            self.palette.open = false;
        }
        if let Some(id) = run {
            self.palette.open = false;
            self.palette.recent.retain(|r| *r != id);
            self.palette.recent.insert(0, id);
            self.palette.recent.truncate(8);
            self.run_action(id, ctx);
        }
    }
}
