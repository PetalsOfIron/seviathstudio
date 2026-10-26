// SPDX-License-Identifier: GPL-3.0-or-later
// App shell: top bar with the logo, workspace switch and tabs; status bar; undo history;
// settings (saved to disk); account window.

use crate::widgets::HelpExt;
use eframe::egui::{self, Align, Align2, CornerRadius, FontId, Layout, Margin, Panel, RichText, Sense, Ui, vec2};

use crate::history::{History, TabUndo};
use crate::icons::Icon;
use crate::settings::Settings;
use crate::settings_ui::{self, Page, PenMonitor};
use crate::snake;
use crate::tabs::{self, Tab, draw::DrawState};
use crate::theme::{self, *};
use crate::widgets::{self, icon_button, segmented, tab_button};

mod aihelper;
mod physgroups;
mod rigbench;
mod downloadsui;
mod animtab;
mod celtab;
mod streamtab;
mod commands;
mod files;
mod layoutmenu;
mod fontmaker;
mod newproject;
mod partstab;
mod gameui;
mod animpage;
mod generatetab;

#[cfg(test)]
pub fn generate_page_characters() -> generatetab::Page {
    generatetab::Page::Characters
}

#[cfg(test)]
pub fn generate_page_animations() -> generatetab::Page {
    generatetab::Page::Animations
}
mod physicsui;
mod prerigui;
mod recording;
mod rigtab;
mod worklog;
mod signs;

pub const VERSION: &str = env!("CARGO_PKG_VERSION");

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Workspace {
    Character,
    Design,
}

pub struct SeviathApp {
    tab: Tab,
    workspace: Workspace,
    draw: DrawState,
    settings: Settings,
    applied: Option<Settings>, // last settings put into effect
    saved: Settings,           // last settings written to disk
    persist: bool,             // false in tests: never touch the user's settings file
    history: History,
    history_open: bool,
    history_filter: Option<Tab>,
    undo_choice: Option<usize>, // a step waiting for "undo both / only this / cancel"
    settings_open: bool,
    settings_page: Page,
    account_open: bool,
    pen: PenMonitor,
    notice: Option<(String, f64)>,
    files: files::FileState,
    signs: signs::SignState,
    maker: fontmaker::FontMaker,
    rec: recording::RecUi,
    ai: aihelper::AiUi,
    downloads: downloadsui::DownloadsUi,
    work: worklog::WorkClock,
    palette: commands::Palette,
    parts: partstab::PartsState,
    prerig: prerigui::PreRig,
    rigs: rigtab::RigState,
    genr: generatetab::GenerateUi,
    layout_name: String,
}

impl SeviathApp {
    pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
        theme::apply(&cc.egui_ctx);
        // Ctrl+0 / Ctrl+plus / Ctrl+minus zoom the canvas, not the whole interface.
        cc.egui_ctx.options_mut(|o| o.zoom_with_keyboard = false);
        if let Some(rs) = cc.wgpu_render_state.as_ref() {
            let i = rs.adapter.get_info();
            let _ = settings_ui::GPU_INFO.set(format!("{} ({:?})", i.name, i.backend));
        }
        let persist = !cfg!(test);
        let settings = if persist { Settings::load() } else { Settings::default() };
        let start_projects = settings.start_with_projects;
        let mut draw = DrawState::default();
        if persist {
            draw.layouts = crate::layout::load();
            draw.persist_layouts = true;
            draw.refs = crate::refs::RefStore::load_saved();
        }
        Self {
            tab: Tab::Draw,
            workspace: Workspace::Character,
            draw,
            saved: settings.clone(),
            settings,
            applied: None,
            persist,
            history: History::default(),
            history_open: false,
            history_filter: None,
            undo_choice: None,
            settings_open: false,
            settings_page: Page::Interface,
            account_open: false,
            pen: PenMonitor::default(),
            notice: None,
            files: files::FileState::starting(persist && start_projects),
            signs: Default::default(),
            maker: Default::default(),
            rec: Default::default(),
            ai: Default::default(),
            downloads: Default::default(),
            work: Default::default(),
            palette: Default::default(),
            parts: Default::default(),
            prerig: Default::default(),
            rigs: Default::default(),
            genr: Default::default(),
            layout_name: String::new(),
        }
    }

    #[cfg(test)]
    pub fn draw_mut(&mut self) -> &mut DrawState {
        &mut self.draw
    }

    #[cfg(test)]
    pub fn draw_mut_ref(&self) -> &DrawState {
        &self.draw
    }

    #[cfg(test)]
    pub fn files_mut(&mut self) -> &mut files::FileState {
        &mut self.files
    }

    #[cfg(test)]
    pub fn settings_mut(&mut self) -> &mut Settings {
        &mut self.settings
    }

    #[cfg(test)]
    pub fn file_name(&self) -> &str {
        &self.files.name
    }

    #[cfg(test)]
    pub fn test_dirty(&self) -> bool {
        self.is_dirty()
    }

    #[cfg(test)]
    pub fn test_busy(&self) -> bool {
        self.busy().is_some()
    }

    #[cfg(test)]
    pub fn test_open(&mut self, path: std::path::PathBuf) {
        self.request(files::Pending::Open(path));
    }

    #[cfg(test)]
    pub fn history(&self) -> &History {
        &self.history
    }

    #[cfg(test)]
    pub fn open_window(&mut self, history: bool, settings: Option<Page>) {
        self.history_open = history;
        if let Some(p) = settings {
            self.settings_open = true;
            self.settings_page = p;
        }
    }

    fn say(&mut self, text: impl Into<String>, now: f64) {
        self.notice = Some((text.into(), now));
    }

    // ---------- Settings ----------

    fn apply_settings(&mut self, ctx: &egui::Context) {
        if self.applied.as_ref() == Some(&self.settings) {
            return;
        }
        self.settings = self.settings.clone().sanitized();
        theme::apply_prefs(ctx, &self.settings);
        self.history.set_limits(self.settings.undo_steps, self.settings.undo_memory_mb);
        self.draw.set = self.settings.clone();
        self.applied = Some(self.settings.clone());
        if self.persist && self.settings != self.saved {
            match self.settings.save() {
                Ok(()) => self.saved = self.settings.clone(),
                Err(e) => {
                    let now = ctx.input(|i| i.time);
                    self.say(format!("Couldn't save settings: {e}"), now);
                    self.saved = self.settings.clone(); // don't retry every frame
                }
            }
        }
    }

    // ---------- Undo ----------

    fn undo_tab(&mut self, now: f64) {
        match self.history.undo_tab(&mut self.draw.doc, self.tab) {
            TabUndo::Done => {
                self.draw.after_restore();
                let l = self.history.entries[self.history.cursor].label.clone();
                self.say(format!("Undid: {l}"), now);
            }
            TabUndo::Nothing => self.say(format!("Nothing to undo in the {} tab", self.tab.label()), now),
            TabUndo::NeedsChoice(i) => {
                if self.settings.warn_other_tab_undo {
                    self.undo_choice = Some(i);
                } else {
                    self.undo_only(i, now);
                }
            }
        }
    }

    fn undo_all(&mut self, now: f64) {
        let label = self.history.undo(&mut self.draw.doc).map(str::to_owned);
        match label {
            Some(l) => {
                self.draw.after_restore();
                self.say(format!("Undid: {l}"), now);
            }
            None => self.say("Nothing to undo", now),
        }
    }

    fn redo(&mut self, now: f64) {
        let label = self.history.redo(&mut self.draw.doc).map(str::to_owned);
        match label {
            Some(l) => {
                self.draw.after_restore();
                self.say(format!("Redid: {l}"), now);
            }
            None => self.say("Nothing to redo", now),
        }
    }

    fn undo_only(&mut self, i: usize, now: f64) {
        let label = self.history.entries[i].label.clone();
        let tab = self.history.entries[i].tab;
        let clean = self.history.undo_only(&mut self.draw.doc, i, tab, now);
        self.draw.after_restore();
        if clean {
            self.say(format!("Undid only: {label}"), now);
        } else {
            self.say(format!("Undid only: {label} — parts changed again later were kept"), now);
        }
    }

    fn jump(&mut self, n: usize, now: f64) {
        self.history.jump_to(&mut self.draw.doc, n);
        self.draw.after_restore();
        self.say("Moved through history", now);
    }

    // ---------- Bars ----------

    fn top_bar(&mut self, ui: &mut Ui) {
        let bar = ui.max_rect();
        let now = ui.input(|i| i.time);
        ui.horizontal_centered(|ui| {
            ui.add_space(6.0);
            let (r, _) = ui.allocate_exact_size(vec2(34.0, 34.0), Sense::hover());
            snake::logo(ui.painter(), r.center(), 34.0);
            ui.add_space(2.0);
            ui.label(RichText::new("Seviath").size(17.0).strong().color(TEXT));
            ui.add_space(-8.0);
            ui.label(RichText::new("Studio").size(17.0).color(EMERALD));
            ui.add_space(10.0);
            self.file_menu(ui);
            self.filter_menu(ui);
            self.layout_menu(ui);
            ui.add_space(10.0);

            segmented(ui, &mut self.workspace, &[(Workspace::Character, "Character"), (Workspace::Design, "Design")]);
            ui.add_space(14.0);

            if self.workspace == Workspace::Character {
                ui.spacing_mut().item_spacing.x = 2.0;
                for t in Tab::ALL {
                    if tab_button(ui, t.icon(), t.label(), t.accent(), self.tab == t, t.tip()).clicked() {
                        self.tab = t;
                    }
                }
            }

            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                ui.add_space(8.0);
                ui.spacing_mut().item_spacing.x = 4.0;
                self.rec_light(ui);
                if ui.add(egui::Button::new(RichText::new("Sign in").color(TEXT)).corner_radius(14)).help("Account").clicked() {
                    self.account_open = true;
                }
                if icon_button(ui, Icon::Settings, self.settings_open, 32.0, "Settings (Ctrl+,)").clicked() {
                    self.settings_open = !self.settings_open;
                }
                if icon_button(ui, Icon::History, self.history_open, 32.0, "History (Ctrl+H)").clicked() {
                    self.history_open = !self.history_open;
                }
                let can_redo = self.history.can_redo();
                let r = ui.add_enabled_ui(can_redo, |ui| icon_button(ui, Icon::Redo, false, 32.0, "Redo (Ctrl+Y)")).inner;
                if r.clicked() {
                    self.redo(now);
                }
                let can_undo = self.history.can_undo();
                let u = ui
                    .add_enabled_ui(can_undo, |ui| icon_button(ui, Icon::Undo, false, 32.0, "Undo in this tab (Ctrl+Z) · everything: Ctrl+Alt+Z"))
                    .inner;
                if u.clicked() {
                    self.undo_tab(now);
                }
            });
        });
        snake::divider(ui.painter(), bar);
    }

    /// The Filter menu: every filter and color adjustment, opened in its window.
    fn filter_menu(&mut self, ui: &mut Ui) {
        ui.menu_button(RichText::new("Filter").color(TEXT), |ui| {
            ui.set_min_width(220.0);
            for (group, label) in crate::filters::Group::ALL {
                ui.menu_button(label, |ui| {
                    for def in crate::filters::FILTERS.iter().filter(|d| d.group == group) {
                        if ui.button(def.label).help(def.about).clicked() {
                            self.tab = Tab::Draw;
                            self.workspace = Workspace::Character;
                            self.draw.open_filter(def.id);
                            ui.close();
                        }
                    }
                });
            }
        });
    }

    fn status_bar(&mut self, ui: &mut Ui) {
        let now = ui.input(|i| i.time);
        if let Some((_, t)) = &self.notice {
            if now - t > 4.0 {
                self.notice = None;
            } else {
                ui.ctx().request_repaint_after_secs(4.1 - (now - t) as f32);
            }
        }
        ui.horizontal_centered(|ui| {
            ui.add_space(6.0);
            let accent = if self.workspace == Workspace::Design { GOLD } else { self.tab.accent() };
            let (r, _) = ui.allocate_exact_size(vec2(8.0, 8.0), Sense::hover());
            ui.painter().circle_filled(r.center(), 3.5, accent);
            let text = match (self.workspace, self.tab) {
                (Workspace::Design, _) => "Design workspace".to_owned(),
                (_, Tab::Draw) => self.draw.status(),
                (_, t) => format!("{} tab", t.label()),
            };
            ui.label(RichText::new(text).size(12.0).color(TEXT_DIM));
            if let Some(b) = self.busy() {
                ui.add_space(12.0);
                ui.spinner();
                ui.label(RichText::new(b).size(12.0).color(EMERALD));
            }
            let draw_notice = (self.tab == Tab::Draw).then(|| self.draw.notice()).flatten();
            if let Some(n) = draw_notice.or(self.notice.as_ref().map(|(s, _)| s.as_str())) {
                ui.add_space(12.0);
                ui.label(RichText::new(n).size(12.0).color(GOLD));
            }
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                ui.add_space(8.0);
                ui.label(RichText::new(format!("SeviathStudio {VERSION} · Rust")).size(11.5).color(TEXT_DIM));
            });
        });
    }

    // ---------- Windows ----------

    fn history_window(&mut self, ctx: &egui::Context) {
        let now = ctx.input(|i| i.time);
        let mut open = self.history_open;
        let mut action: Option<(bool, usize)> = None; // (only this step?, index)
        egui::Window::new(RichText::new("History").strong())
            .open(&mut open)
            .default_size(vec2(320.0, 460.0))
            .default_pos(ctx.content_rect().right_top() + vec2(-660.0, 70.0))
            .collapsible(false)
            .show(ctx, |ui| {
                ui.horizontal_wrapped(|ui| {
                    ui.spacing_mut().item_spacing = vec2(4.0, 4.0);
                    if widgets::chip(ui, "All tabs", self.history_filter.is_none()).clicked() {
                        self.history_filter = None;
                    }
                    for t in Tab::ALL {
                        if widgets::chip(ui, t.label(), self.history_filter == Some(t)).clicked() {
                            self.history_filter = Some(t);
                        }
                    }
                });
                let mb: usize = self.history.entries.iter().map(|e| e.bytes).sum::<usize>() >> 20;
                ui.label(
                    RichText::new(format!("{} steps · {} MB · click a step to go back to it · right-click for more", self.history.entries.len(), mb))
                        .size(11.0)
                        .color(TEXT_DIM),
                );
                ui.separator();
                egui::ScrollArea::vertical().show(ui, |ui| {
                    let w = ui.available_width();
                    for i in (0..self.history.entries.len()).rev() {
                        let e = &self.history.entries[i];
                        if self.history_filter.is_some_and(|f| f != e.tab) {
                            continue;
                        }
                        let applied = i < self.history.cursor;
                        let current = i + 1 == self.history.cursor;
                        let (row, resp) = ui.allocate_exact_size(vec2(w, 30.0), Sense::click());
                        let p = ui.painter();
                        if current {
                            p.rect_filled(row, CornerRadius::same(6), EMERALD_DEEP.gamma_multiply(0.6));
                        } else if resp.hovered() {
                            p.rect_filled(row, CornerRadius::same(6), HOVER);
                        }
                        let accent = e.tab.accent();
                        p.circle_filled(row.left_center() + vec2(12.0, 0.0), 4.5, if applied { accent } else { accent.gamma_multiply(0.35) });
                        let col = if applied { TEXT } else { TEXT_DIM };
                        let label = e.label.clone();
                        p.with_clip_rect(row.shrink2(vec2(24.0, 0.0)).translate(vec2(0.0, 0.0))).text(
                            row.left_center() + vec2(24.0, 0.0),
                            Align2::LEFT_CENTER,
                            &label,
                            FontId::proportional(12.5),
                            col,
                        );
                        let ago = ago(now - e.time);
                        p.text(row.right_center() - vec2(6.0, 0.0), Align2::RIGHT_CENTER, ago, FontId::proportional(10.5), TEXT_DIM);
                        let resp = resp.help(format!("{} tab{}", e.tab.label(), if applied { "" } else { " · undone (click to redo up to here)" }));
                        if resp.clicked() {
                            action = Some((false, i + 1));
                        }
                        resp.context_menu(|ui| {
                            if ui.button("Go back to right after this step").clicked() {
                                action = Some((false, i + 1));
                                ui.close();
                            }
                            if applied && ui.button("Undo only this step (keep later ones)").clicked() {
                                action = Some((true, i));
                                ui.close();
                            }
                        });
                    }
                    // the starting point
                    let (row, resp) = ui.allocate_exact_size(vec2(w, 26.0), Sense::click());
                    let p = ui.painter();
                    if self.history.cursor == 0 {
                        p.rect_filled(row, CornerRadius::same(6), EMERALD_DEEP.gamma_multiply(0.6));
                    }
                    p.text(row.left_center() + vec2(24.0, 0.0), Align2::LEFT_CENTER, "Start", FontId::proportional(12.5), TEXT_DIM);
                    if resp.help("Before the oldest step kept").clicked() {
                        action = Some((false, 0));
                    }
                });
            });
        self.history_open = open;
        match action {
            Some((true, i)) => self.undo_only(i, now),
            Some((false, n)) => self.jump(n, now),
            None => {}
        }
    }

    fn undo_choice_window(&mut self, ctx: &egui::Context) {
        let Some(i) = self.undo_choice else { return };
        let now = ctx.input(|i| i.time);
        let Some(e) = self.history.entries.get(i) else {
            self.undo_choice = None;
            return;
        };
        let (label, tab) = (e.label.clone(), e.tab);
        let newer: Vec<String> =
            self.history.entries[i + 1..self.history.cursor].iter().filter(|x| x.tab != tab).map(|x| format!("{} ({})", x.label, x.tab.label())).collect();
        let mut choice = None;
        egui::Modal::new(egui::Id::new("undo_choice")).show(ctx, |ui| {
            ui.set_width(420.0);
            ui.label(RichText::new("Undo across tabs?").size(16.0).strong());
            ui.add_space(4.0);
            ui.label(format!("\"{label}\" in the {} tab is older than changes you made in other tabs:", tab.label()));
            for n in newer.iter().take(5) {
                ui.label(RichText::new(format!("  • {n}")).color(TEXT_DIM));
            }
            ui.add_space(8.0);
            ui.horizontal(|ui| {
                if ui.button("Undo both").help("Go back in time to before this step").clicked() {
                    choice = Some(1);
                }
                if ui.button("Undo only this one").help("Keep the other tabs' changes").clicked() {
                    choice = Some(2);
                }
                if ui.button("Cancel").clicked() {
                    choice = Some(0);
                }
            });
        });
        match choice {
            Some(1) => {
                self.undo_choice = None;
                self.jump(i, now);
            }
            Some(2) => {
                self.undo_choice = None;
                self.undo_only(i, now);
            }
            Some(_) => self.undo_choice = None,
            None => {}
        }
    }

    fn settings_window(&mut self, ctx: &egui::Context) {
        let mut open = self.settings_open;
        let r = settings_ui::window_rect(ctx);
        egui::Window::new(RichText::new("Settings").strong())
            .open(&mut open)
            .default_rect(r)
            .collapsible(false)
            .show(ctx, |ui| {
                ui.horizontal_top(|ui| {
                    ui.vertical(|ui| {
                        ui.set_width(140.0);
                        for (page, label) in Page::ALL {
                            if ui.selectable_label(self.settings_page == page, label).clicked() {
                                self.settings_page = page;
                            }
                        }
                        ui.add_space(16.0);
                        if ui.small_button("Reset everything").help("All settings back to default").clicked() {
                            self.settings = Settings::default();
                        }
                        if self.persist {
                            ui.label(RichText::new("Saved automatically").size(10.5).color(TEXT_DIM));
                        }
                    });
                    ui.separator();
                    ui.vertical(|ui| {
                        egui::ScrollArea::vertical().id_salt("settings_page").show(ui, |ui| {
                            ui.set_min_width(520.0);
                            settings_ui::page(ui, self.settings_page, &mut self.settings, &mut self.pen);
                        });
                    });
                });
            });
        self.settings_open = open;
    }

    fn account_window(&mut self, ctx: &egui::Context) {
        let mut open = self.account_open;
        egui::Window::new(RichText::new("Account").strong())
            .open(&mut open)
            .collapsible(false)
            .resizable(false)
            .anchor(Align2::RIGHT_TOP, vec2(-12.0, 60.0))
            .show(ctx, |ui| {
                ui.set_width(300.0);
                ui.label(RichText::new("Accounts are not in the Rust version yet.").color(TEXT));
                ui.add_space(4.0);
                ui.label(
                    RichText::new(
                        "They come with the library and shop: sign-in with two-factor security, points, \
                         brush sharing and selling. Drawing works fully without an account.",
                    )
                    .color(TEXT_DIM)
                    .size(12.5),
                );
            });
        self.account_open = open;
    }

    fn design_workspace(&mut self, ui: &mut Ui) {
        egui::CentralPanel::default().frame(egui::Frame::new().fill(BG)).show(ui, |ui| {
            let full = ui.max_rect();
            snake::scales(ui.painter(), full, GOLD.gamma_multiply(0.05));
            ui.painter().text(full.center() - vec2(0.0, 12.0), Align2::CENTER_CENTER, "Design workspace", FontId::proportional(24.0), TEXT);
            ui.painter().text(
                full.center() + vec2(0.0, 18.0),
                Align2::CENTER_CENTER,
                "Artboards, vector design, overlays and social sizes — after the Character tabs.",
                FontId::proportional(13.5),
                TEXT_DIM,
            );
        });
    }
}

fn ago(secs: f64) -> String {
    if secs < 5.0 {
        "now".into()
    } else if secs < 60.0 {
        format!("{secs:.0} s")
    } else if secs < 3600.0 {
        format!("{:.0} min", secs / 60.0)
    } else {
        format!("{:.1} h", secs / 3600.0)
    }
}

impl eframe::App for SeviathApp {
    fn ui(&mut self, ui: &mut Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        self.apply_settings(&ctx);
        self.pen.update(&ctx);
        self.command_keys(&ctx);
        self.gen_poll(&ctx, ctx.input(|i| i.time));

        Panel::top("top_bar")
            .exact_size(54.0)
            .resizable(false)
            .show_separator_line(false)
            .frame(egui::Frame::new().fill(PANEL).inner_margin(Margin::symmetric(6, 0)))
            .show(ui, |ui| self.top_bar(ui));

        Panel::bottom("status_bar")
            .exact_size(26.0)
            .resizable(false)
            .frame(egui::Frame::new().fill(PANEL).inner_margin(Margin::symmetric(6, 0)))
            .show(ui, |ui| self.status_bar(ui));

        if self.tab != Tab::Stream {
            self.stream_release();
        }
        match (self.workspace, self.tab) {
            (Workspace::Design, _) => self.design_workspace(ui),
            (_, Tab::Draw) => {
                self.cel_onion_for_draw(ui.ctx());
                self.draw.ui(ui);
                self.cel_frame_bar(ui.ctx());
            }
            (_, Tab::Parts) => self.parts_ui(ui),
            (_, Tab::Rig) => self.rig_ui(ui),
            (_, Tab::Physics) => self.physics_ui(ui),
            (_, Tab::Animate) => self.anim_ui(ui),
            (_, Tab::Stream) => self.stream_ui(ui),
            (_, Tab::Generate) => self.generate_ui(ui),
            (_, t) => tabs::placeholder::ui(ui, t),
        }

        // finished Draw-tab changes become history steps
        let now = ctx.input(|i| i.time);
        for (label, before, after) in std::mem::take(&mut self.draw.changes) {
            self.recorder_step(&after);
            self.log_step(&label, now);
            self.history.push(label, Tab::Draw, before, after, now);
        }

        self.history_window(&ctx);
        self.undo_choice_window(&ctx);
        self.settings_window(&ctx);
        self.account_window(&ctx);
        self.new_project_window(&ctx);
        self.project_info_window(&ctx);
        self.signatures_window(&ctx);
        self.font_maker_window(&ctx);
        self.recorder_window(&ctx);
        self.ai_window(&ctx);
        self.downloads_window(&ctx);
        self.prerig_window(&ctx);
        self.palette_window(&ctx);
        self.recorder_tick(&ctx);
        self.work_history_window(&ctx);
        self.reference_clock(&ctx);
        self.project_manager(&ctx);
        self.files_tick(&ctx);
        self.unsaved_dialog(&ctx);
    }

    fn clear_color(&self, _visuals: &egui::Visuals) -> [f32; 4] {
        BG.to_normalized_gamma_f32()
    }
}
