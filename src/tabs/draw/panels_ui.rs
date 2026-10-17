// SPDX-License-Identifier: GPL-3.0-or-later
// The panels of the Draw tab (brush, color, sub view, layers): docked on a side, floating over the canvas,
// in their own window (for a second screen) or hidden. Layouts are kept as workspaces (see layout.rs).

use eframe::egui::{self, Align, Layout as UiLayout, Margin, RichText, Ui, ViewportBuilder, ViewportClass, ViewportId};

use super::{DrawState, Tool};
use crate::layout::{Mode, PanelId, Side};
use crate::theme::*;

/// What a click in a panel's top row asks for.
#[derive(Clone, Copy, PartialEq, Debug)]
enum Ask {
    Float,
    Window,
    Hide,
    Dock(Side),
    Up,
    Down,
}

impl DrawState {
    fn panel_content(&mut self, ui: &mut Ui, id: PanelId) {
        match id {
            PanelId::Brush => {
                if matches!(self.tool, Tool::Select | Tool::Lasso | Tool::Fill | Tool::Move | Tool::Gradient | Tool::Pen | Tool::Text) {
                    egui::ScrollArea::vertical().id_salt("tool_options").show(ui, |ui| self.tool_options(ui));
                } else {
                    self.brush_panel(ui);
                }
            }
            PanelId::Color => {
                card().show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    self.color_section(ui);
                });
            }
            PanelId::SubView => {
                card().show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    self.subview_section(ui);
                });
            }
            PanelId::Layers => {
                card().show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    self.layers_section(ui);
                });
            }
        }
    }

    /// The small row above a panel: where it lives.
    fn panel_handle(&mut self, ui: &mut Ui, id: PanelId, docked: bool) -> Option<Ask> {
        let mut ask = None;
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 2.0;
            ui.spacing_mut().button_padding = egui::vec2(4.0, 0.0);
            let small = |s: &str| RichText::new(s).size(11.0).color(TEXT_DIM);
            ui.with_layout(UiLayout::right_to_left(Align::Center), |ui| {
                ui.add_space(if docked { 8.0 } else { 0.0 });
                if ui.add(egui::Button::new(small("Hide")).frame(false)).on_hover_text("Hide this panel (Layout menu or Ctrl+K brings it back)").clicked() {
                    ask = Some(Ask::Hide);
                }
                if ui.add(egui::Button::new(small("Window")).frame(false)).on_hover_text("Its own window: drag it to another screen").clicked() {
                    ask = Some(Ask::Window);
                }
                if docked {
                    if ui.add(egui::Button::new(small("Float")).frame(false)).on_hover_text("A window over the canvas").clicked() {
                        ask = Some(Ask::Float);
                    }
                    ui.menu_button(small("Move"), |ui| {
                        if ui.button("Up").clicked() {
                            ask = Some(Ask::Up);
                            ui.close();
                        }
                        if ui.button("Down").clicked() {
                            ask = Some(Ask::Down);
                            ui.close();
                        }
                        ui.separator();
                        if ui.button("To the left side").clicked() {
                            ask = Some(Ask::Dock(Side::Left));
                            ui.close();
                        }
                        if ui.button("To the right side").clicked() {
                            ask = Some(Ask::Dock(Side::Right));
                            ui.close();
                        }
                    });
                } else {
                    let side = self.layouts.working.get(id).side;
                    if ui.add(egui::Button::new(small("Dock")).frame(false)).on_hover_text("Back into the side column").clicked() {
                        ask = Some(Ask::Dock(side));
                    }
                }
            });
        });
        ask
    }

    fn apply_ask(&mut self, id: PanelId, ask: Ask) {
        let l = &mut self.layouts.working;
        match ask {
            Ask::Float => l.set_mode(id, Mode::Floating),
            Ask::Window => l.set_mode(id, Mode::Window),
            Ask::Hide => l.set_mode(id, Mode::Hidden),
            Ask::Dock(side) => l.dock(id, side),
            Ask::Up => l.nudge(id, -1),
            Ask::Down => l.nudge(id, 1),
        }
    }

    /// The side columns.
    fn dock_column(&mut self, ui: &mut Ui, side: Side) {
        let ids = self.layouts.working.column(side);
        let alone_brush = ids.len() == 1 && ids[0] == PanelId::Brush;
        let mut asks: Vec<(PanelId, Ask)> = Vec::new();
        let mut body = |this: &mut Self, ui: &mut Ui| {
            for (i, id) in ids.iter().enumerate() {
                if let Some(a) = this.panel_handle(ui, *id, true) {
                    asks.push((*id, a));
                }
                if *id == PanelId::Brush && !alone_brush {
                    // the brush panel scrolls inside itself: give it a fixed height next to the other panels
                    let h = (ui.ctx().content_rect().height() * 0.62).max(380.0);
                    let w = (ui.available_width() - 10.0).max(200.0);
                    ui.allocate_ui(egui::vec2(w, h), |ui| this.panel_content(ui, *id));
                } else {
                    this.panel_content(ui, *id);
                }
                if i + 1 < ids.len() {
                    ui.add_space(8.0);
                }
            }
        };
        if alone_brush {
            body(self, ui);
        } else {
            egui::ScrollArea::vertical().id_salt(("dock_col", side == Side::Left)).show(ui, |ui| body(self, ui));
        }
        for (id, a) in asks {
            self.apply_ask(id, a);
        }
    }

    /// Draw the docked columns (the tools strip and the canvas are drawn by the caller).
    pub(super) fn dock_panels(&mut self, ui: &mut Ui) {
        let lefty = self.set.left_handed;
        let mk = |side: Side, id: &str| match (side == Side::Left) != lefty {
            true => egui::Panel::left(id.to_owned()),
            false => egui::Panel::right(id.to_owned()),
        };
        if !self.layouts.working.column(Side::Left).is_empty() {
            let w = self.layouts.working.left_w;
            mk(Side::Left, "draw_brush")
                .resizable(true)
                .default_size(w)
                .size_range(230.0..=520.0)
                .frame(egui::Frame::new().fill(PANEL).inner_margin(Margin::same(10)))
                .show(ui, |ui| self.dock_column(ui, Side::Left));
        }
        if !self.layouts.working.column(Side::Right).is_empty() {
            let w = self.layouts.working.right_w;
            mk(Side::Right, "draw_right")
                .resizable(true)
                .default_size(w)
                .size_range(250.0..=560.0)
                .frame(egui::Frame::new().fill(PANEL).inner_margin(Margin::same(10)))
                .show(ui, |ui| self.dock_column(ui, Side::Right));
        }
    }

    /// Floating panels and panels in their own window.
    pub(super) fn loose_panels(&mut self, ctx: &egui::Context) {
        for id in PanelId::ALL {
            let st = self.layouts.working.get(id);
            match st.mode {
                Mode::Floating => {
                    let mut open = true;
                    let mut ask = None;
                    let r = egui::Window::new(RichText::new(id.title()).strong())
                        .id(egui::Id::new(("panel_float", id)))
                        .open(&mut open)
                        .default_pos(egui::pos2(st.pos[0], st.pos[1]))
                        .default_size(egui::vec2(st.size[0], st.size[1]))
                        .resizable(true)
                        .collapsible(true)
                        .show(ctx, |ui| {
                            ask = self.panel_handle(ui, id, false);
                            egui::ScrollArea::vertical().id_salt(("float_scroll", id)).show(ui, |ui| self.panel_content(ui, id));
                        });
                    if let Some(r) = r {
                        // remember where you put it (only once the pointer is up)
                        if !ctx.input(|i| i.pointer.any_down()) {
                            let (p, s) = (r.response.rect.min, r.response.rect.size());
                            let e = self.layouts.working.panels.entry(id).or_default();
                            if (e.pos[0] - p.x).abs() > 0.5 || (e.pos[1] - p.y).abs() > 0.5 || (e.size[0] - s.x).abs() > 0.5 || (e.size[1] - s.y).abs() > 0.5 {
                                e.pos = [p.x, p.y];
                                e.size = [s.x, s.y];
                            }
                        }
                    }
                    if !open {
                        ask = Some(Ask::Hide);
                    }
                    if let Some(a) = ask {
                        self.apply_ask(id, a);
                    }
                }
                Mode::Window => {
                    let builder = ViewportBuilder::default().with_title(format!("SeviathStudio: {}", id.title())).with_inner_size([st.size[0], st.size[1]]).with_min_inner_size([220.0, 160.0]);
                    let mut ask = None;
                    ctx.show_viewport_immediate(ViewportId::from_hash_of(("panel_window", id)), builder, |ui, class| {
                        if class == ViewportClass::EmbeddedWindow {
                            // no second window possible here: it shows as a window inside this one
                            let mut open = true;
                            egui::Window::new(RichText::new(id.title()).strong()).id(egui::Id::new(("panel_embedded", id))).open(&mut open).default_size(egui::vec2(st.size[0], st.size[1])).show(ui.ctx(), |ui| {
                                ask = self.panel_handle(ui, id, false);
                                egui::ScrollArea::vertical().id_salt(("win_scroll", id)).show(ui, |ui| self.panel_content(ui, id));
                            });
                            if !open {
                                ask = Some(Ask::Dock(st.side));
                            }
                        } else {
                            ask = self.panel_handle(ui, id, false);
                            egui::ScrollArea::vertical().id_salt(("win_scroll", id)).show(ui, |ui| self.panel_content(ui, id));
                            if ui.ctx().input(|i| i.viewport().close_requested()) {
                                ask = Some(Ask::Dock(st.side));
                            }
                        }
                    });
                    if let Some(a) = ask {
                        self.apply_ask(id, a);
                    }
                }
                _ => {}
            }
        }
    }

    /// Keep layouts in their file once a change is finished.
    pub(super) fn save_layouts(&mut self, ctx: &egui::Context) {
        if !self.persist_layouts || ctx.input(|i| i.pointer.any_down()) || self.layouts == self.layouts_saved {
            return;
        }
        crate::layout::save(&self.layouts);
        self.layouts_saved = self.layouts.clone();
    }

    /// Layout actions of the action table (panel.* and ws.*).
    pub fn run_layout_action(&mut self, id: &str) -> bool {
        if let Some(p) = PanelId::ALL.iter().find(|p| p.action_id() == id) {
            self.layouts.working.toggle_hidden(*p);
            return true;
        }
        let names = self.layouts.names();
        let cur = names.iter().position(|n| *n == self.layouts.active).unwrap_or(0);
        match id {
            "ws.next" => {
                let n = names[(cur + 1) % names.len()].clone();
                self.layouts.switch(&n);
            }
            "ws.prev" => {
                let n = names[(cur + names.len() - 1) % names.len()].clone();
                self.layouts.switch(&n);
            }
            "ws.reset" => {
                let a = self.layouts.active.clone();
                self.layouts.switch(&a);
            }
            _ => return false,
        }
        true
    }
}
