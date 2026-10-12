// App shell: top bar with the logo, workspace switch and tabs; status bar; settings window.

use eframe::egui::{self, Align, Align2, FontId, Key, Layout, Margin, Modifiers, Panel, RichText, Sense, Ui, vec2};

use crate::icons::Icon;
use crate::snake;
use crate::tabs::{self, Tab, draw::DrawState};
use crate::theme::{self, *};
use crate::widgets::{icon_button, section_title, segmented, tab_button};

pub const VERSION: &str = env!("CARGO_PKG_VERSION");

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Workspace {
    Character,
    Design,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum SettingsPage {
    General,
    Shortcuts,
    Info,
}

pub struct SeviathApp {
    tab: Tab,
    workspace: Workspace,
    draw: DrawState,
    settings_open: bool,
    settings_page: SettingsPage,
    account_open: bool,
    ui_scale: f32,
    notice: Option<String>,
}

impl SeviathApp {
    pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
        theme::apply(&cc.egui_ctx);
        // Ctrl+0 / Ctrl+plus / Ctrl+minus zoom the canvas, not the whole interface.
        cc.egui_ctx.options_mut(|o| o.zoom_with_keyboard = false);
        Self {
            tab: Tab::Draw,
            workspace: Workspace::Character,
            draw: DrawState::default(),
            settings_open: false,
            settings_page: SettingsPage::General,
            account_open: false,
            ui_scale: 1.0,
            notice: None,
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

    fn shortcuts(&mut self, ctx: &egui::Context) {
        let keys = [Key::Num1, Key::Num2, Key::Num3, Key::Num4, Key::Num5, Key::Num6];
        ctx.input_mut(|i| {
            for (k, tab) in keys.iter().zip(Tab::ALL) {
                if i.consume_key(Modifiers::COMMAND, *k) {
                    self.tab = tab;
                    self.workspace = Workspace::Character;
                }
            }
            if i.consume_key(Modifiers::COMMAND, Key::Comma) {
                self.settings_open = !self.settings_open;
            }
        });
    }

    fn top_bar(&mut self, ui: &mut Ui) {
        let bar = ui.max_rect();
        ui.horizontal_centered(|ui| {
            ui.add_space(6.0);
            let (r, _) = ui.allocate_exact_size(vec2(34.0, 34.0), Sense::hover());
            snake::logo(ui.painter(), r.center(), 34.0);
            ui.add_space(2.0);
            ui.label(RichText::new("Seviath").size(17.0).strong().color(TEXT));
            ui.add_space(-8.0);
            ui.label(RichText::new("Studio").size(17.0).color(EMERALD));
            ui.add_space(14.0);

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
                if ui
                    .add(egui::Button::new(RichText::new("Sign in").color(TEXT)).corner_radius(14))
                    .on_hover_text("Account")
                    .clicked()
                {
                    self.account_open = true;
                }
                if icon_button(ui, Icon::Settings, self.settings_open, 32.0, "Settings (Ctrl+,)").clicked() {
                    self.settings_open = !self.settings_open;
                }
                if icon_button(ui, Icon::Redo, false, 32.0, "Redo (Ctrl+Y)").clicked() {
                    self.notice = Some("Undo and redo arrive with the canvas engine.".into());
                }
                if icon_button(ui, Icon::Undo, false, 32.0, "Undo this tab (Ctrl+Z) · everything: Ctrl+Alt+Z").clicked() {
                    self.notice = Some("Undo and redo arrive with the canvas engine.".into());
                }
            });
        });
        snake::divider(ui.painter(), bar);
    }

    fn status_bar(&mut self, ui: &mut Ui) {
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
            let draw_notice = (self.tab == Tab::Draw).then(|| self.draw.notice()).flatten();
            if let Some(n) = draw_notice.or(self.notice.as_deref()) {
                ui.add_space(12.0);
                ui.label(RichText::new(n).size(12.0).color(GOLD));
            }
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                ui.add_space(8.0);
                ui.label(RichText::new(format!("SeviathStudio {VERSION} · Rust")).size(11.5).color(TEXT_DIM));
            });
        });
    }

    fn settings_window(&mut self, ctx: &egui::Context) {
        let mut open = self.settings_open;
        egui::Window::new(RichText::new("Settings").strong())
            .open(&mut open)
            .default_size(vec2(560.0, 380.0))
            .pivot(Align2::CENTER_CENTER)
            .default_pos(ctx.content_rect().center())
            .collapsible(false)
            .show(ctx, |ui| {
                ui.horizontal_top(|ui| {
                    ui.vertical(|ui| {
                        ui.set_width(120.0);
                        for (page, label) in [
                            (SettingsPage::General, "General"),
                            (SettingsPage::Shortcuts, "Shortcuts"),
                            (SettingsPage::Info, "Info"),
                        ] {
                            if ui.selectable_label(self.settings_page == page, label).clicked() {
                                self.settings_page = page;
                            }
                        }
                    });
                    ui.separator();
                    ui.vertical(|ui| match self.settings_page {
                        SettingsPage::General => {
                            section_title(ui, "Interface");
                            ui.horizontal(|ui| {
                                ui.label("UI scale");
                                if ui.add(egui::Slider::new(&mut self.ui_scale, 0.75..=2.0).step_by(0.05).max_decimals(2)).changed() {
                                    ctx.set_zoom_factor(self.ui_scale);
                                }
                            });
                            ui.label(RichText::new("Makes every panel and button bigger or smaller.").color(TEXT_DIM).size(12.0));
                        }
                        SettingsPage::Shortcuts => {
                            section_title(ui, "Keyboard");
                            egui::Grid::new("keys").striped(true).spacing(vec2(24.0, 4.0)).show(ui, |ui| {
                                for (k, what) in [
                                    ("Ctrl+1 … Ctrl+6", "Switch tab"),
                                    ("B / E / S", "Brush / Eraser / Smudge"),
                                    ("G / Shift+G", "Fill / Gradient"),
                                    ("I", "Color picker"),
                                    ("M / L / V", "Selection / Lasso / Move"),
                                    ("P / U / T", "Vector pen / Shapes / Text"),
                                    ("H / Z", "Hand / Zoom"),
                                    ("[ / ]", "Smaller / bigger brush"),
                                    ("X", "Swap main and second color"),
                                    ("Ctrl+,", "Settings"),
                                ] {
                                    ui.label(RichText::new(k).monospace().color(VENOM));
                                    ui.label(what);
                                    ui.end_row();
                                }
                            });
                        }
                        SettingsPage::Info => {
                            let (r, _) = ui.allocate_exact_size(vec2(64.0, 64.0), Sense::hover());
                            snake::logo(ui.painter(), r.center(), 64.0);
                            ui.label(RichText::new("SeviathStudio").size(20.0).strong());
                            ui.label(RichText::new(format!("Version {VERSION} — native Rust build")).color(TEXT_DIM));
                            ui.add_space(8.0);
                            ui.label("Draw, rig, animate and stream a 2D character from one file, with reliable PSD export.");
                            ui.add_space(8.0);
                            ui.horizontal(|ui| {
                                ui.label("Made by");
                                ui.hyperlink_to("@SnakeSeviath on X", "https://x.com/SnakeSeviath");
                            });
                            ui.add_space(4.0);
                            ui.label(RichText::new("Open source · MIT or MPL-2.0 (to be decided)").color(TEXT_DIM).size(12.0));
                        }
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
            ui.painter().text(
                full.center() - vec2(0.0, 12.0),
                Align2::CENTER_CENTER,
                "Design workspace",
                FontId::proportional(24.0),
                TEXT,
            );
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

impl eframe::App for SeviathApp {
    fn ui(&mut self, ui: &mut Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        self.shortcuts(&ctx);

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

        match (self.workspace, self.tab) {
            (Workspace::Design, _) => self.design_workspace(ui),
            (_, Tab::Draw) => self.draw.ui(ui),
            (_, t) => tabs::placeholder::ui(ui, t),
        }

        self.settings_window(&ctx);
        self.account_window(&ctx);
    }

    fn clear_color(&self, _visuals: &egui::Visuals) -> [f32; 4] {
        BG.to_normalized_gamma_f32()
    }
}
