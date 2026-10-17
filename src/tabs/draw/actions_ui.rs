// SPDX-License-Identifier: GPL-3.0-or-later
// The Draw tab's side of the action table (see actions.rs): the same code runs from a key press and from
// the command palette.

use eframe::egui::{self, vec2};

use super::{DrawState, Tool, TOOL_GROUPS};
use crate::actions::{self, ACTIONS};
use crate::doc::LayerKind;

impl Tool {
    pub fn action_id(self) -> &'static str {
        match self {
            Tool::Brush => "tool.brush",
            Tool::Eraser => "tool.eraser",
            Tool::Smudge => "tool.smudge",
            Tool::Fill => "tool.fill",
            Tool::Gradient => "tool.gradient",
            Tool::Picker => "tool.picker",
            Tool::Select => "tool.select",
            Tool::Lasso => "tool.lasso",
            Tool::Move => "tool.move",
            Tool::Pen => "tool.pen",
            Tool::Shape => "tool.shape",
            Tool::Text => "tool.text",
            Tool::Hand => "tool.hand",
            Tool::Zoom => "tool.zoom",
        }
    }
}

/// Does this action belong to the Draw tab?
pub fn is_draw_action(id: &str) -> bool {
    ["tool.", "color.", "brush.", "view.", "sel.", "layer.", "panel.", "ws."].iter().any(|p| id.starts_with(p))
}

impl DrawState {
    /// Look for the keys of the Draw tab's actions and run what was pressed.
    pub(super) fn action_keys(&mut self, ctx: &egui::Context) {
        if ctx.egui_wants_keyboard_input() {
            return;
        }
        let keys = self.set.shortcuts.clone();
        let fired = ctx.input_mut(|i| actions::pressed(i, &keys, ACTIONS.iter().filter(|d| is_draw_action(d.id))));
        for id in fired {
            self.run_draw_action(id, ctx);
        }
    }

    /// Run one action of the Draw tab. False when it isn't one of ours.
    pub fn run_draw_action(&mut self, id: &str, ctx: &egui::Context) -> bool {
        if self.run_layout_action(id) {
            return true;
        }
        if let Some(t) = TOOL_GROUPS.iter().flat_map(|g| g.iter().copied()).find(|t| t.action_id() == id) {
            self.tool = t;
            return true;
        }
        let (step, max) = (self.set.size_step, self.set.brush_size_max);
        let pen_tool = self.tool == Tool::Pen;
        match id {
            "color.swap" => {
                std::mem::swap(&mut self.primary, &mut self.secondary);
                self.hex = super::hex_of(self.primary);
            }
            "brush.smaller" => {
                if pen_tool {
                    self.vec.style.size = (self.vec.style.size / step).max(0.5);
                } else {
                    let s = self.brush_mut();
                    s.size = (s.size / step).max(0.5);
                }
            }
            "brush.bigger" => {
                if pen_tool {
                    self.vec.style.size = (self.vec.style.size * step).min(max);
                } else {
                    let s = self.brush_mut();
                    s.size = (s.size * step).min(max);
                }
            }
            "view.fit" => self.view.fit(self.canvas_rect, vec2(self.doc.w as f32, self.doc.h as f32)),
            "view.actual" => self.view.set_zoom(1.0),
            "view.zoom_in" => {
                let c = self.view.screen_center;
                self.view.zoom_at(c, 1.25);
            }
            "view.zoom_out" => {
                let c = self.view.screen_center;
                self.view.zoom_at(c, 0.8);
            }
            "view.mirror" => self.view.flip = !self.view.flip,
            "sel.all" => self.select_all(),
            "sel.deselect" => self.deselect(),
            "sel.invert" => self.invert_selection(),
            "sel.transform" => {
                self.start_transform();
            }
            "sel.fill" => self.fill_selected(),
            "sel.quickmask" => self.toggle_quick_mask(),
            "layer.new" => self.add_layer(LayerKind::Raster),
            "layer.duplicate" => self.duplicate_active(),
            "layer.merge" => self.merge_down_active(),
            "layer.group" => self.group_active(),
            "layer.clip" => self.toggle_clip_active(),
            "layer.quickbar" => {
                self.lui.quick = Some(ctx.pointer_latest_pos().unwrap_or(egui::pos2(400.0, 300.0)) + vec2(8.0, 8.0));
                self.lui.quick_age = 0;
            }
            "layer.search" => self.lui.focus_search = true,
            _ => return false,
        }
        true
    }

    #[cfg(test)]
    pub fn tool_name(&self) -> &'static str {
        self.tool.name()
    }

    #[cfg(test)]
    pub fn test_canvas_width(&self) -> f32 {
        self.canvas_rect.width()
    }
}
