// SPDX-License-Identifier: GPL-3.0-or-later
// Draw tab: tool strip, brush panel, canvas, and the right column (Color → Sub view → Layers).
// Painting itself arrives with the brush engine; this is the layout and all panel state.

use eframe::egui::{
    self, Align, Color32, CornerRadius, CursorIcon, Event, Key, Layout, Margin, Modifiers,
    MouseWheelUnit, Panel, PointerButton, Pos2, Rect, RichText, Sense, Shape, Stroke, StrokeKind, TouchPhase, Ui, UiBuilder, Vec2,
    ecolor::Hsva, pos2, vec2,
};

use crate::canvas::{self as cv, CanvasRenderer};
use crate::colorpick;
use crate::history::{self, DocState};
use crate::refs::RefStore;
use crate::settings::{CursorStyle, Settings, WheelAction};
use crate::doc::{Document, LayerId, LayerKind};
use crate::icons::Icon;
use crate::brush::engine::{Sample, Stroke as BrushStroke, StrokeResult};
use crate::snake;
use crate::theme::*;
use crate::view::View;
use crate::widgets::{self, icon_button, section_title, segmented};

mod brushes;
pub mod effects_ui;
pub mod filters_ui;
pub mod mixchart_ui;
pub mod mybrushes_ui;
pub mod gradient_ui;
mod layers;
pub mod organizer;
mod random_ui;
mod references;
mod tools;
pub mod trace_ui;
pub mod vector_ui;

use brushes::BrushLib;
#[cfg(test)]
pub use tools::SelShape;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Tool {
    Brush,
    Eraser,
    Smudge,
    Fill,
    Gradient,
    Picker,
    Select,
    Lasso,
    Move,
    Pen,
    Shape,
    Text,
    Hand,
    Zoom,
}

/// Tools in groups; a thin line separates the groups in the strip.
const TOOL_GROUPS: &[&[Tool]] = &[
    &[Tool::Brush, Tool::Eraser, Tool::Smudge],
    &[Tool::Fill, Tool::Gradient, Tool::Picker],
    &[Tool::Select, Tool::Lasso, Tool::Move],
    &[Tool::Pen, Tool::Shape, Tool::Text],
    &[Tool::Hand, Tool::Zoom],
];

impl Tool {
    pub fn name(self) -> &'static str {
        match self {
            Tool::Brush => "Brush",
            Tool::Eraser => "Eraser",
            Tool::Smudge => "Smudge & blur",
            Tool::Fill => "Fill",
            Tool::Gradient => "Gradient",
            Tool::Picker => "Color picker",
            Tool::Select => "Selection",
            Tool::Lasso => "Lasso",
            Tool::Move => "Move & transform",
            Tool::Pen => "Vector pen",
            Tool::Shape => "Shapes",
            Tool::Text => "Text",
            Tool::Hand => "Hand (pan)",
            Tool::Zoom => "Zoom",
        }
    }

    fn key(self) -> (Modifiers, Key, &'static str) {
        match self {
            Tool::Brush => (Modifiers::NONE, Key::B, "B"),
            Tool::Eraser => (Modifiers::NONE, Key::E, "E"),
            Tool::Smudge => (Modifiers::NONE, Key::S, "S"),
            Tool::Fill => (Modifiers::NONE, Key::G, "G"),
            Tool::Gradient => (Modifiers::SHIFT, Key::G, "Shift+G"),
            Tool::Picker => (Modifiers::NONE, Key::I, "I"),
            Tool::Select => (Modifiers::NONE, Key::M, "M"),
            Tool::Lasso => (Modifiers::NONE, Key::L, "L"),
            Tool::Move => (Modifiers::NONE, Key::V, "V"),
            Tool::Pen => (Modifiers::NONE, Key::P, "P"),
            Tool::Shape => (Modifiers::NONE, Key::U, "U"),
            Tool::Text => (Modifiers::NONE, Key::T, "T"),
            Tool::Hand => (Modifiers::NONE, Key::H, "H"),
            Tool::Zoom => (Modifiers::NONE, Key::Z, "Z"),
        }
    }

    fn icon(self) -> Icon {
        match self {
            Tool::Brush => Icon::Brush,
            Tool::Eraser => Icon::Eraser,
            Tool::Smudge => Icon::Smudge,
            Tool::Fill => Icon::Fill,
            Tool::Gradient => Icon::Gradient,
            Tool::Picker => Icon::Picker,
            Tool::Select => Icon::Select,
            Tool::Lasso => Icon::Lasso,
            Tool::Move => Icon::Move,
            Tool::Pen => Icon::Pen,
            Tool::Shape => Icon::Shape,
            Tool::Text => Icon::Text,
            Tool::Hand => Icon::Hand,
            Tool::Zoom => Icon::Zoom,
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum SubTab {
    Navigator,
    References,
}

pub struct DrawState {
    pub tool: Tool,
    lib: BrushLib,
    pub primary: Color32,
    secondary: Color32,
    hsv: Hsva,
    color_old: Color32,
    color_committed: Color32,
    hex: String,
    swatches: Vec<Color32>,
    subview_open: bool,
    subview_tab: SubTab,
    subview_h: f32,
    pub doc: Document,
    active: LayerId,
    layer_counter: usize,
    renaming: Option<(LayerId, String)>,
    drag_layer: Option<LayerId>,
    lui: organizer::LayerUi,
    grad: gradient_ui::GradState,
    rand: random_ui::RandState,
    vec: vector_ui::VecState,
    trace: trace_ui::TraceUi,
    filter: filters_ui::FilterUi,
    pub check: crate::check::CheckView,
    /// Print proof on the canvas (display only).
    pub proof: crate::colorprofiles::Proof,
    color_open: bool,
    layer_rows: Vec<(LayerId, Rect)>, // where each layer row was drawn last frame
    view: View,
    view_ready: bool,
    renderer: CanvasRenderer,
    stroke: Option<BrushStroke>,
    pen_stroke: bool, // the stroke in progress comes from a pen (has pressure)
    /// What the last finished stroke changed (for undo).
    pub last_stroke: Option<StrokeResult>,
    rotate_raw: f32,
    canvas_rect: Rect,
    cursor_doc: Option<Pos2>,
    notice: Option<&'static str>,
    /// App settings (copied in by the app when they change).
    pub set: Settings,
    /// Reference pictures (Sub view → References).
    pub refs: RefStore,
    ref_popout: bool,
    mixchart: mixchart_ui::MixChart,
    /// Where the reference pop-out window was last drawn.
    ref_rect: Rect,
    ref_view_rect: Option<Rect>, // where the docked reference picture was drawn last frame
    /// Project files dropped onto the window (the app opens them).
    pub open_requests: Vec<std::path::PathBuf>,
    /// Document as it was when the current change started (recorded when the pointer lifts).
    pending: Option<DocState>,
    label_hint: Option<String>,
    /// Finished changes for the app's history: (label, before, after).
    pub changes: Vec<(String, DocState, DocState)>,
    p_smooth: f32,
    /// Selection, fill, move & transform, symmetry.
    tools: tools::ToolState,
}

impl Default for DrawState {
    fn default() -> Self {
        let doc = Document::new(3000, 4000);
        Self {
            tool: Tool::Brush,
            lib: BrushLib::default(),
            primary: Color32::from_rgb(34, 34, 34),
            secondary: Color32::WHITE,
            hsv: Hsva::from(Color32::from_rgb(34, 34, 34)),
            color_old: Color32::from_rgb(34, 34, 34),
            color_committed: Color32::from_rgb(34, 34, 34),
            hex: "#222222".into(),
            swatches: vec![
                Color32::from_rgb(34, 34, 34),
                Color32::WHITE,
                Color32::from_rgb(46, 191, 113),
                Color32::from_rgb(160, 228, 96),
                Color32::from_rgb(214, 168, 76),
                Color32::from_rgb(222, 64, 86),
                Color32::from_rgb(92, 150, 240),
                Color32::from_rgb(172, 122, 240),
                Color32::from_rgb(245, 205, 180),
                Color32::from_rgb(120, 80, 60),
            ],
            subview_open: true,
            subview_tab: SubTab::Navigator,
            subview_h: 170.0,
            active: doc.layers[0].id,
            doc,
            layer_counter: 1,
            renaming: None,
            drag_layer: None,
            lui: organizer::LayerUi::default(),
            grad: gradient_ui::GradState::default(),
            rand: random_ui::RandState::default(),
            vec: vector_ui::VecState::default(),
            trace: trace_ui::TraceUi::default(),
            filter: filters_ui::FilterUi::default(),
            check: crate::check::CheckView::Normal,
            proof: crate::colorprofiles::Proof::Off,
            color_open: true,
            layer_rows: Vec::new(),
            view: View::default(),
            view_ready: false,
            renderer: CanvasRenderer::default(),
            stroke: None,
            pen_stroke: false,
            last_stroke: None,
            rotate_raw: 0.0,
            canvas_rect: Rect::NOTHING,
            cursor_doc: None,
            notice: None,
            set: Settings::default(),
            refs: RefStore::in_memory(),
            ref_popout: false,
            mixchart: Default::default(),
            ref_rect: Rect::NOTHING,
            ref_view_rect: None,
            open_requests: Vec::new(),
            pending: None,
            label_hint: None,
            changes: Vec::new(),
            p_smooth: 1.0,
            tools: tools::ToolState::default(),
        }
    }
}

impl DrawState {
    pub fn status(&self) -> String {
        let mut s = self.tool.name().to_owned();
        if matches!(self.tool, Tool::Brush | Tool::Eraser | Tool::Smudge) {
            s += &format!(" · {}", self.brush_name());
        }
        s += &format!(" · {}×{} px · {:.0}%", self.doc.w, self.doc.h, self.view.zoom * 100.0);
        if let Some(sel) = self.doc.selection.as_ref().and_then(|s| s.bounds()) {
            s += &format!(" · selection {}×{}", sel.2 - sel.0, sel.3 - sel.1);
        }
        if self.view.angle.abs() > 0.001 {
            s += &format!(" · {:.0}°", self.view.angle_deg());
        }
        if self.view.flip {
            s += " · mirrored";
        }
        if let Some(p) = self.cursor_doc {
            if p.x >= 0.0 && p.y >= 0.0 && p.x < self.doc.w as f32 && p.y < self.doc.h as f32 {
                s += &format!(" · x {:.0}  y {:.0}", p.x, p.y);
            }
        }
        s
    }

    /// Short message for the status bar (e.g. why a stroke didn't paint).
    pub fn notice(&self) -> Option<&str> {
        self.tool_notice().or(self.notice)
    }

    pub fn ui(&mut self, ui: &mut Ui) {
        // settings that live in the document and the renderer
        if self.doc.checker != self.set.checker_size as usize {
            self.doc.checker = self.set.checker_size as usize;
            self.doc.mark_all_dirty();
        }
        if self.renderer.set_smooth(self.set.smooth_zoomed_in) {
            self.doc.mark_all_dirty();
        }
        self.renderer.nav_fps = self.set.navigator_fps;

        let frame_before = self.doc.snapshot();
        self.handle_dropped_files(ui.ctx());
        // switching to another tool puts a gradient that is being laid out down
        if self.tool != Tool::Gradient && self.gradient_editing() {
            self.apply_gradient();
        }
        // keys with Alt / Ctrl first: a plain tool key (like L) would also swallow Alt+L
        self.layer_shortcuts(ui.ctx());
        self.tool_shortcuts(ui.ctx());
        self.shortcuts(ui.ctx());
        // switching away from Move puts a floating picture down; a half-drawn polygon is dropped
        if self.tool != Tool::Move && self.transforming() {
            self.apply_transform();
        }

        // left-handed: tools and brushes on the right, color and layers on the left
        let lefty = self.set.left_handed;
        let near = |id: &str| if lefty { Panel::right(id.to_owned()) } else { Panel::left(id.to_owned()) };
        let far = |id: &str| if lefty { Panel::left(id.to_owned()) } else { Panel::right(id.to_owned()) };

        near("draw_tools")
            .exact_size(56.0)
            .resizable(false)
            .frame(egui::Frame::new().fill(PANEL).inner_margin(Margin::symmetric(8, 10)))
            .show(ui, |ui| self.tool_strip(ui));

        near("draw_brush")
            .resizable(true)
            .default_size(270.0)
            .size_range(230.0..=440.0)
            .frame(egui::Frame::new().fill(PANEL).inner_margin(Margin::same(10)))
            .show(ui, |ui| {
                if matches!(self.tool, Tool::Select | Tool::Lasso | Tool::Fill | Tool::Move | Tool::Gradient | Tool::Pen) {
                    egui::ScrollArea::vertical().id_salt("tool_options").show(ui, |ui| self.tool_options(ui));
                } else {
                    self.brush_panel(ui);
                }
            });

        far("draw_right")
            .resizable(true)
            .default_size(290.0)
            .size_range(250.0..=480.0)
            .frame(egui::Frame::new().fill(PANEL).inner_margin(Margin::same(10)))
            .show(ui, |ui| self.right_panel(ui));

        let [r, g, b] = self.set.canvas_bg;
        egui::CentralPanel::default()
            .frame(egui::Frame::new().fill(Color32::from_rgb(r, g, b)))
            .show(ui, |ui| self.canvas(ui));

        self.reference_window(ui.ctx());
        self.mix_chart_window(ui.ctx());
        self.layer_quick_bar(ui.ctx());
        self.randomizer_window(ui.ctx());
        self.trace_window(ui.ctx());
        self.filter_window(ui.ctx());
        self.track_history(ui.ctx(), frame_before);
    }

    /// Turns everything that changed the document into history steps. A change counts as
    /// finished when the pointer is up (so a whole stroke or slider drag is one step).
    fn track_history(&mut self, ctx: &egui::Context, before: DocState) {
        if self.pending.is_none() && !self.doc.same_as(&before) {
            self.pending = Some(before);
        }
        let busy = ctx.input(|i| i.pointer.any_down())
            || self.stroke.is_some()
            || self.drag_layer.is_some()
            || self.renaming.is_some()
            || self.transforming()
            || self.vec.drawing();
        if busy {
            return;
        }
        if let Some(b) = self.pending.take() {
            if !self.doc.same_as(&b) {
                let after = self.doc.snapshot();
                let label = self.label_hint.take().unwrap_or_else(|| history::describe(&b, &after));
                self.changes.push((label, b, after));
            }
        }
        self.label_hint = None;
    }

    /// A different document (opened file or new project): fresh view, selection and caches.
    pub fn replace_document(&mut self, doc: Document) {
        self.doc = doc;
        self.tools.new_document(self.doc.w, self.doc.h);
        self.vec.reset();
        self.trace.open = false;
        self.lui = organizer::LayerUi::default();
        self.grad.edit = None;
        self.renderer.reset();
        self.view_ready = false;
        self.layer_counter = self.doc.layers.len();
        // start on the top layer you can paint on (visible, not locked), else any raster layer
        let paintable = self.doc.layers.iter().find(|l| l.kind == LayerKind::Raster && l.visible && !l.locked);
        let raster = self.doc.layers.iter().find(|l| l.kind == LayerKind::Raster);
        self.active = paintable.or(raster).or(self.doc.layers.first()).map(|l| l.id).unwrap_or(0);
        self.after_restore();
        self.changes.clear();
        self.doc.mark_all_dirty();
    }

    /// A change made from outside the Draw tab's own panels (menus, imports): recorded as one history step.
    pub fn apply_external(&mut self, label: &str, f: impl FnOnce(&mut Self)) {
        let before = self.doc.snapshot();
        f(self);
        if !self.doc.same_as(&before) {
            let after = self.doc.snapshot();
            self.changes.push((label.to_owned(), before, after));
        }
    }

    /// A picture as a new layer above the selected one, centered and fit inside the canvas.
    pub fn import_picture_layer(&mut self, img: &image::RgbaImage, name: &str) {
        let (w, h) = (self.doc.w, self.doc.h);
        let k = (w as f32 / img.width() as f32).min(h as f32 / img.height() as f32).min(1.0);
        let pic = if k < 1.0 {
            let (nw, nh) = (((img.width() as f32 * k).round() as u32).max(1), ((img.height() as f32 * k).round() as u32).max(1));
            image::imageops::resize(img, nw, nh, image::imageops::FilterType::Lanczos3)
        } else {
            img.clone()
        };
        let (ox, oy) = ((w as i32 - pic.width() as i32) / 2, (h as i32 - pic.height() as i32) / 2);
        self.apply_external(&format!("Import picture · {name}"), |d| {
            let mut layer = d.doc.new_layer(name, LayerKind::Raster);
            layer.tiles = crate::files::tiles_from_rgba(&pic, ox, oy, w, h);
            layer.rev += 1;
            d.active = d.doc.insert_layer(layer, d.active);
        });
    }

    /// The document was replaced by undo/redo: drop anything in progress.
    pub fn after_restore(&mut self) {
        // undo while transforming: the document is already back to before the lift
        self.tools.floating = None;
        self.grad.edit = None;
        self.vec.reset();
        self.trace.open = false;
        self.filter.open = false;
        self.stroke = None;
        self.pen_stroke = false;
        self.pending = None;
        self.label_hint = None;
        self.renaming = None;
        self.drag_layer = None;
        self.fix_active();
    }

    #[cfg(test)]
    pub fn reference_view_rect(&self) -> Option<Rect> {
        self.ref_view_rect
    }

    #[cfg(test)]
    #[cfg(test)]
    pub fn open_mix_chart(&mut self, colors: &[Color32]) {
        self.swatches = colors.to_vec();
        self.mixchart.open = true;
    }

    #[cfg(test)]
    pub fn active_id(&self) -> LayerId {
        self.active
    }

    #[cfg(test)]
    pub fn set_active(&mut self, id: LayerId) {
        self.active = id;
    }

    #[cfg(test)]
    pub fn set_popout(&mut self, on: bool) {
        self.ref_popout = on;
    }

    #[cfg(test)]
    pub fn show_references(&mut self) {
        self.subview_open = true;
        self.subview_tab = SubTab::References;
    }

    #[cfg(test)]
    pub fn select(&mut self, idx: usize) {
        self.active = self.doc.layers[idx].id;
    }

    #[cfg(test)]
    pub fn layer_row(&self, idx: usize) -> Option<Rect> {
        let id = self.doc.layers.get(idx)?.id;
        self.layer_rows.iter().find(|(i, _)| *i == id).map(|(_, r)| *r)
    }

    #[cfg(test)]
    pub fn tools_mut(&mut self) -> &mut tools::ToolState {
        &mut self.tools
    }

    #[cfg(test)]
    pub fn screen_to_doc(&self, p: Pos2) -> Pos2 {
        self.view.screen_to_doc(p)
    }

    #[cfg(test)]
    pub fn set_color_open(&mut self, open: bool) {
        self.color_open = open;
    }

    /// Rotation back to 0°.
    pub fn straighten(&mut self) {
        self.view.set_angle(0.0);
    }

    fn doc_size(&self) -> Vec2 {
        vec2(self.doc.w as f32, self.doc.h as f32)
    }

    fn shortcuts(&mut self, ctx: &egui::Context) {
        if ctx.egui_wants_keyboard_input() {
            return;
        }
        let (step, max) = (self.set.size_step, self.set.brush_size_max);
        let pen_tool = self.tool == Tool::Pen;
        ctx.input_mut(|i| {
            // Shift+G before G, so the plain G doesn't steal it.
            let mut tools: Vec<Tool> = TOOL_GROUPS.iter().flat_map(|g| g.iter().copied()).collect();
            tools.sort_by_key(|t| if t.key().0 == Modifiers::NONE { 1 } else { 0 });
            for t in tools {
                let (m, k, _) = t.key();
                if i.consume_key(m, k) {
                    self.tool = t;
                }
            }
            if i.consume_key(Modifiers::NONE, Key::X) {
                std::mem::swap(&mut self.primary, &mut self.secondary);
                self.hex = hex_of(self.primary);
            }
            if i.consume_key(Modifiers::NONE, Key::OpenBracket) {
                if pen_tool {
                    self.vec.style.size = (self.vec.style.size / step).max(0.5);
                } else {
                    let s = self.brush_mut();
                    s.size = (s.size / step).max(0.5);
                }
            }
            if i.consume_key(Modifiers::NONE, Key::CloseBracket) {
                if pen_tool {
                    self.vec.style.size = (self.vec.style.size * step).min(max);
                } else {
                    let s = self.brush_mut();
                    s.size = (s.size * step).min(max);
                }
            }
            // View: Ctrl+0 fit, Ctrl+Alt+0 100%, Ctrl+= / Ctrl+- zoom, F mirror.
            let c = self.view.screen_center;
            if i.consume_key(Modifiers::COMMAND | Modifiers::ALT, Key::Num0) {
                self.view.set_zoom(1.0);
            }
            if i.consume_key(Modifiers::COMMAND, Key::Num0) {
                self.view.fit(self.canvas_rect, vec2(self.doc.w as f32, self.doc.h as f32));
            }
            if i.consume_key(Modifiers::COMMAND, Key::Equals) || i.consume_key(Modifiers::COMMAND, Key::Plus) {
                self.view.zoom_at(c, 1.25);
            }
            if i.consume_key(Modifiers::COMMAND, Key::Minus) {
                self.view.zoom_at(c, 0.8);
            }
            if i.consume_key(Modifiers::NONE, Key::F) {
                self.view.flip = !self.view.flip;
            }
        });
    }

    fn tool_strip(&mut self, ui: &mut Ui) {
        ui.spacing_mut().item_spacing.y = 4.0;
        ui.vertical_centered(|ui| {
            for (gi, group) in TOOL_GROUPS.iter().enumerate() {
                if gi > 0 {
                    let (r, _) = ui.allocate_exact_size(vec2(30.0, 7.0), Sense::hover());
                    ui.painter().line_segment([r.left_center(), r.right_center()], Stroke::new(1.0, LINE));
                }
                for &tool in group.iter() {
                    let tip = format!("{} ({})", tool.name(), tool.key().2);
                    if icon_button(ui, tool.icon(), self.tool == tool, 36.0, &tip).clicked() {
                        self.tool = tool;
                    }
                }
            }
        });

        ui.with_layout(Layout::bottom_up(Align::Center), |ui| {
            let (rect, resp) = ui.allocate_exact_size(Vec2::splat(38.0), Sense::click());
            let p = ui.painter();
            let back = Rect::from_min_size(rect.min + vec2(14.0, 14.0), Vec2::splat(22.0));
            let front = Rect::from_min_size(rect.min, Vec2::splat(22.0));
            p.rect_filled(back, CornerRadius::same(5), self.secondary);
            p.rect_stroke(back, CornerRadius::same(5), Stroke::new(1.5, LINE), StrokeKind::Outside);
            p.rect_filled(front, CornerRadius::same(5), self.primary);
            p.rect_stroke(front, CornerRadius::same(5), Stroke::new(1.5, GOLD), StrokeKind::Outside);
            if resp.on_hover_text("Main and second color — click or press X to swap").clicked() {
                std::mem::swap(&mut self.primary, &mut self.secondary);
                self.hex = hex_of(self.primary);
            }
        });
    }

    fn right_panel(&mut self, ui: &mut Ui) {
        egui::ScrollArea::vertical().id_salt("right_col").show(ui, |ui| {
            card().show(ui, |ui| {
                ui.set_width(ui.available_width());
                self.color_section(ui);
            });
            ui.add_space(8.0);
            card().show(ui, |ui| {
                ui.set_width(ui.available_width());
                self.subview_section(ui);
            });
            ui.add_space(8.0);
            card().show(ui, |ui| {
                ui.set_width(ui.available_width());
                self.layers_section(ui);
            });
        });
    }

    fn color_section(&mut self, ui: &mut Ui) {
        ui.horizontal(|ui| {
            section_title(ui, "Color");
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                if widgets::chip(ui, if self.color_open { "Hide" } else { "Show" }, false).clicked() {
                    self.color_open = !self.color_open;
                }
                if widgets::chip(ui, "Randomize", self.rand.open).on_hover_text("Believable random colors for skin, foliage, sky and more").clicked() {
                    self.rand.open = !self.rand.open;
                }
                let (r, _) = ui.allocate_exact_size(vec2(22.0, 16.0), Sense::hover());
                ui.painter().rect_filled(r, CornerRadius::same(4), self.primary);
            });
        });
        if !self.color_open {
            return;
        }
        let before = self.primary;
        colorpick::picker(ui, &mut self.primary, &mut self.hsv);
        ui.add_space(2.0);
        ui.horizontal(|ui| {
            if colorpick::compare(ui, self.color_old, self.primary).clicked() {
                self.primary = self.color_old;
            }
            ui.label(RichText::new("HEX").color(TEXT_DIM).size(12.0));
            let r = ui.add(egui::TextEdit::singleline(&mut self.hex).desired_width(76.0).char_limit(7));
            if r.lost_focus() {
                match parse_hex(&self.hex) {
                    Some(c) => self.primary = c,
                    None => self.hex = hex_of(self.primary),
                }
            } else if !r.has_focus() && parse_hex(&self.hex) != Some(self.primary) {
                // Color changed elsewhere (picker tool, swatch, swap): follow it.
                self.hex = hex_of(self.primary);
            }
            if ui.button("Save").on_hover_text("Add this color to the swatches").clicked() && !self.swatches.contains(&self.primary) {
                if self.swatches.len() >= self.set.swatches_max {
                    self.swatches.remove(0);
                }
                self.swatches.push(self.primary);
            }
        });
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing = vec2(4.0, 4.0);
            let mut remove = None;
            for (i, c) in self.swatches.iter().enumerate() {
                let (r, resp) = ui.allocate_exact_size(Vec2::splat(20.0), Sense::click());
                ui.painter().rect_filled(r, CornerRadius::same(4), *c);
                let edge = if *c == self.primary { VENOM } else { LINE };
                ui.painter().rect_stroke(r, CornerRadius::same(4), Stroke::new(1.0, edge), StrokeKind::Inside);
                let resp = resp.on_hover_text("Click: use · Right-click: remove");
                if resp.clicked() {
                    self.primary = *c;
                }
                if resp.secondary_clicked() {
                    remove = Some(i);
                }
            }
            if let Some(i) = remove {
                self.swatches.remove(i);
            }
        });
        if self.primary != before {
            self.hex = hex_of(self.primary);
        }
        // "Before" swatch: the color you had before the last finished change.
        if !ui.input(|i| i.pointer.any_down()) && self.primary != self.color_committed {
            self.color_old = self.color_committed;
            self.color_committed = self.primary;
        }
    }

    fn subview_section(&mut self, ui: &mut Ui) {
        ui.horizontal(|ui| {
            section_title(ui, "Sub view");
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                let label = if self.subview_open { "Hide" } else { "Show" };
                if widgets::chip(ui, label, false).clicked() {
                    self.subview_open = !self.subview_open;
                }
            });
        });
        if !self.subview_open {
            return;
        }
        segmented(ui, &mut self.subview_tab, &[(SubTab::Navigator, "Navigator"), (SubTab::References, "References")]);
        ui.add_space(4.0);

        let w = ui.available_width();
        if self.subview_tab == SubTab::References {
            let h = self.subview_h;
            self.references_panel(ui, w, h, false);
        } else {
            let (rect, resp) = ui.allocate_exact_size(vec2(w, self.subview_h), Sense::click_and_drag());
            let p = ui.painter_at(rect);
            p.rect_filled(rect, CornerRadius::same(6), BG);
            {
                let size = self.doc_size();
                let z = ((rect.width() - 16.0) / size.x).min((rect.height() - 16.0) / size.y);
                let page = Rect::from_center_size(rect.center(), size * z);
                match self.renderer.thumb() {
                    Some(t) => {
                        p.image(t.id(), page, Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)), Color32::WHITE);
                    }
                    None => {
                        p.rect_filled(page, 0.0, self.doc.paper);
                    }
                }
                // Where the canvas is looking right now (rotated with the view).
                if self.canvas_rect.is_positive() {
                    let quad: Vec<Pos2> =
                        self.view.visible_quad(self.canvas_rect).iter().map(|d| page.min + d.to_vec2() * z).collect();
                    p.add(Shape::closed_line(quad, Stroke::new(1.5, VENOM)));
                }
                if let Some(pt) = resp.interact_pointer_pos() {
                    self.view.center = ((pt - page.min) / z).to_pos2();
                }
                resp.on_hover_text("Whole drawing. Click or drag to move the view there.")
                    .on_hover_cursor(CursorIcon::Grab);
            }
        }

        // Drag the grip under the preview to resize the docked sub view.
        let (grip, gresp) = ui.allocate_exact_size(vec2(w, 10.0), Sense::drag());
        let gresp = gresp.on_hover_cursor(CursorIcon::ResizeVertical);
        if gresp.dragged() {
            self.subview_h = (self.subview_h + gresp.drag_delta().y).clamp(90.0, 1000.0);
        }
        let gc = if gresp.hovered() || gresp.dragged() { EMERALD } else { LINE };
        ui.painter().line_segment(
            [pos2(grip.center().x - 18.0, grip.center().y), pos2(grip.center().x + 18.0, grip.center().y)],
            Stroke::new(3.0, gc),
        );
    }

    fn canvas(&mut self, ui: &mut Ui) {
        let rect = ui.max_rect();
        self.canvas_rect = rect;
        let resp = ui.allocate_rect(rect, Sense::click_and_drag());
        self.view.screen_center = rect.center();
        if !self.view_ready {
            self.view.fit(rect, self.doc_size());
            self.view_ready = true;
        }
        let ctx = ui.ctx().clone();
        let (space, rkey, alt, shift, primary_down, events) = ctx.input(|i| {
            (
                i.key_down(Key::Space),
                i.key_down(Key::R),
                i.modifiers.alt,
                i.modifiers.shift,
                i.pointer.primary_down(),
                i.events.clone(),
            )
        });
        let hover = resp.hover_pos();

        // ---- Zoom (wheel / pinch) and rotate (Alt+wheel / trackpad twist), around the pointer.
        if let Some(hp) = hover {
            let mut factor = 1.0f32;
            let mut turn = 0.0f32;
            let mut scroll = Vec2::ZERO;
            let speed = self.set.zoom_speed * if self.set.invert_wheel { -1.0 } else { 1.0 };
            for e in &events {
                match e {
                    Event::MouseWheel { unit, delta, modifiers, .. } => {
                        let k = match unit {
                            MouseWheelUnit::Point => 1.0,
                            MouseWheelUnit::Line => 40.0,
                            MouseWheelUnit::Page => 400.0,
                        };
                        let (dx, dy) = (delta.x * k, delta.y * k);
                        if modifiers.alt {
                            turn += dy * 0.0033;
                        } else if self.set.wheel == WheelAction::Zoom || modifiers.command || modifiers.ctrl {
                            factor *= (dy * 0.0045 * speed).exp();
                        } else if modifiers.shift {
                            scroll += vec2(dy, dx);
                        } else {
                            scroll += vec2(dx, dy);
                        }
                    }
                    Event::Zoom(f) => factor *= *f,
                    Event::Rotate(a) => turn += *a,
                    _ => {}
                }
            }
            if factor != 1.0 {
                self.view.zoom_at(hp, factor);
            }
            if turn != 0.0 {
                self.view.rotate(turn);
            }
            if scroll != Vec2::ZERO {
                self.view.pan(scroll);
            }
        }

        // ---- Pan: middle drag, Space + drag, or the Hand tool.
        let primary_drag = resp.dragged_by(PointerButton::Primary);
        let nav_key = space || rkey;
        if resp.dragged_by(PointerButton::Middle)
            || (primary_drag && ((space && !rkey) || (self.tool == Tool::Hand && !rkey)))
        {
            self.view.pan(resp.drag_delta());
        }

        // ---- Rotate: R + drag around the middle of the canvas, Shift snaps to 15°.
        if resp.drag_started() {
            self.rotate_raw = self.view.angle;
        }
        if primary_drag && rkey {
            if let Some(p) = resp.interact_pointer_pos() {
                let c = rect.center();
                let prev = p - resp.drag_delta();
                let mut da = (p - c).angle() - (prev - c).angle();
                if da > std::f32::consts::PI {
                    da -= std::f32::consts::TAU;
                } else if da < -std::f32::consts::PI {
                    da += std::f32::consts::TAU;
                }
                self.rotate_raw += da;
                let step = self.set.rotate_snap.to_radians();
                let a = if shift { (self.rotate_raw / step).round() * step } else { self.rotate_raw };
                self.view.set_angle(a);
            }
        }

        // ---- Zoom tool: click to zoom in (Alt: out), drag sideways to scrub.
        if self.tool == Tool::Zoom && !nav_key {
            if resp.clicked() {
                if let Some(p) = resp.interact_pointer_pos() {
                    self.view.zoom_at(p, if alt { 1.0 / 1.5 } else { 1.5 });
                }
            } else if primary_drag {
                if let Some(o) = ctx.input(|i| i.pointer.press_origin()) {
                    self.view.zoom_at(o, (resp.drag_delta().x * 0.01).exp());
                }
            }
        }

        // ---- Selections, fill, move & transform, symmetry center.
        if ctx.input(|i| i.pointer.primary_pressed()) && resp.hovered() {
            self.tools.notice = None;
        }
        let tool_used = !nav_key && self.tool_input(&resp, &ctx);

        // ---- Painting and color picking.
        let painting_tool = matches!(self.tool, Tool::Brush | Tool::Eraser | Tool::Smudge | Tool::Pen) && !tool_used;
        let pressed_here = resp.is_pointer_button_down_on() && primary_down;
        let picking = self.tool == Tool::Picker || (painting_tool && alt && self.stroke.is_none());
        if picking && pressed_here && !nav_key {
            let sample_all = sampler(&self.renderer);
            if let Some(p) = resp.interact_pointer_pos() {
                let d = self.view.screen_to_doc(p);
                let half = (self.set.picker_samples as i32 - 1) / 2;
                let (mut acc, mut n) = ([0u32; 3], 0u32);
                for oy in -half..=half {
                    for ox in -half..=half {
                        if let Some(c) = sample_all(d.x as i32 + ox, d.y as i32 + oy) {
                            let [r, g, b, _] = c.to_srgba_unmultiplied();
                            acc = [acc[0] + r as u32, acc[1] + g as u32, acc[2] + b as u32];
                            n += 1;
                        }
                    }
                }
                if n > 0 {
                    self.primary = Color32::from_rgb((acc[0] / n) as u8, (acc[1] / n) as u8, (acc[2] / n) as u8);
                    self.hex = hex_of(self.primary);
                }
            }
        } else if painting_tool && !nav_key && (pressed_here || self.stroke.is_some() || self.vec_live()) && primary_down {
            // Pen samples carry pressure (Windows Ink arrives as touch events with force);
            // otherwise it's a mouse: every pointer sample of this frame at full pressure.
            let view = self.view;
            let to_doc = |p: Pos2, pressure: f32| {
                let d = view.screen_to_doc(p);
                Sample { x: d.x, y: d.y, p: pressure.clamp(0.0, 1.0) }
            };
            // pen pressure: your range and curve from Settings, then optional smoothing
            let smooth = self.set.pressure_smoothing;
            if self.stroke.is_none() && !self.vec_live() {
                self.p_smooth = -1.0;
            }
            let mut pen: Vec<Sample> = Vec::new();
            for e in &events {
                if let Event::Touch { pos, force: Some(f), phase, .. } = e {
                    if *phase != TouchPhase::Cancel {
                        let p = self.set.pressure(*f);
                        self.p_smooth = if self.p_smooth < 0.0 { p } else { self.p_smooth + (p - self.p_smooth) * (1.0 - smooth) };
                        pen.push(to_doc(*pos, self.p_smooth));
                    }
                }
            }
            let mouse_p = self.set.mouse_pressure;
            let is_pen = !pen.is_empty() || self.pen_stroke;
            let mut samples = if is_pen {
                pen
            } else {
                events.iter().filter_map(|e| if let Event::PointerMoved(p) = e { Some(to_doc(*p, mouse_p)) } else { None }).collect()
            };
            if self.stroke.is_none() && !self.vec_live() {
                let ai = self.active_idx();
                if let Some(msg) = self.block_reason() {
                    self.notice = Some(msg);
                } else if self.set.min_stroke_move > 0.0
                    && ctx.input(|i| i.pointer.press_origin().zip(i.pointer.latest_pos()).map(|(o, l)| (l - o).length()).unwrap_or(0.0))
                        < self.set.min_stroke_move
                {
                    // wait until the pen really moves (no accidental dots)
                } else {
                    self.notice = None;
                    let mut start = if is_pen && !samples.is_empty() {
                        Some(samples.remove(0))
                    } else {
                        ctx.input(|i| i.pointer.press_origin()).map(|o| to_doc(o, mouse_p))
                    };
                    // Shift + click: a straight line from where the last stroke ended
                    if let (true, Some(from), Some(click)) = (shift, self.tools.last_end, start) {
                        samples.insert(0, click);
                        start = Some(Sample { x: from.x, y: from.y, p: click.p });
                    }
                    if let (Some(start), true) = (start, self.vec_tool_active()) {
                        let now = ctx.input(|i| i.time);
                        self.pen_stroke = is_pen;
                        self.vec_begin(start, now);
                    } else if let Some(start) = start {
                        let mut settings = self.lib.mem[brushes::slot(self.tool)].s.clone();
                        // tremor help adds steadiness on top of every brush's own stabilizer
                        let t = self.set.tremor_help;
                        settings.stabilize = 1.0 - (1.0 - settings.stabilize) * (1.0 - t);
                        let erase = self.tool == Tool::Eraser;
                        let verb = match self.tool {
                            Tool::Eraser => "Erase",
                            Tool::Smudge => "Smudge",
                            _ => "Stroke",
                        };
                        let layer_name = self.doc.layers[ai].name.clone();
                        self.label_hint = Some(format!("{verb} · {} · {layer_name}", self.brush_name()));
                        self.pen_stroke = is_pen;
                        let mirrors = if self.tools.sym.on() { self.tools.sym.mirrors() } else { Vec::new() };
                        // painting the quick mask: always in its red, only the coverage counts
                        let color = if self.doc.layers[ai].name == tools::QUICK_MASK { Color32::from_rgb(230, 40, 70) } else { self.primary };
                        let sample_all = sampler(&self.renderer);
                        let mut st = BrushStroke::begin_mirrored(&mut self.doc, ai, &settings, color, erase, view.zoom, is_pen, start, &sample_all, mirrors);
                        if self.grad.brush_on && !erase && self.tool == Tool::Brush {
                            st.set_gradient(Some(self.grad.g.along_stroke(self.grad.brush_len)));
                        }
                        self.stroke = Some(st);
                    }
                }
            }
            if self.vec_live() {
                let now = ctx.input(|i| i.time);
                self.vec_add(&samples, now, &ctx);
            }
            if let Some(st) = self.stroke.as_mut() {
                let sample_all = sampler(&self.renderer);
                st.add(&mut self.doc, &samples, &sample_all);
                if let Some(l) = samples.last() {
                    self.tools.last_end = Some(pos2(l.x, l.y));
                }
            }
        }
        if !primary_down {
            if self.vec_live() {
                self.vec_finish();
            }
            if let Some(st) = self.stroke.take() {
                let sample_all = sampler(&self.renderer);
                self.last_stroke = Some(st.finish(&mut self.doc, &sample_all));
            }
            self.pen_stroke = false;
        }

        // ---- Draw.
        self.doc.clock = ctx.input(|i| i.time);
        if !self.doc.wet.is_empty() {
            self.doc.wet.prune(self.doc.clock as f32);
        }
        let viewing = self.doc.index_of(self.active).filter(|&i| self.doc.layers[i].depth).map(|i| self.doc.layers[i].id);
        if self.doc.depth_view != viewing {
            self.doc.depth_view = viewing;
            self.doc.mark_all_dirty();
        }
        let look = crate::colorprofiles::Look {
            check: self.check,
            profile: self.doc.meta.rgb_profile.clone(),
            proof: self.proof,
            press: self.doc.meta.cmyk_profile.clone(),
            intent: crate::colorprofiles::Intent::from_id(&self.doc.meta.intent),
        };
        if self.renderer.set_look(&look) {
            self.doc.mark_all_dirty();
        }
        self.renderer.sync(&ctx, &mut self.doc);
        let p = ui.painter_at(rect);
        if self.set.show_scales {
            snake::scales(&p, rect, EMERALD.gamma_multiply(0.045));
        }
        let quad = cv::page_quad(&self.view, &self.doc);
        if self.set.page_shadow {
            let shadow: Vec<Pos2> = quad.iter().map(|q| *q + vec2(0.0, 8.0)).collect();
            p.add(Shape::convex_polygon(shadow, Color32::from_black_alpha(90), Stroke::NONE));
        }
        self.renderer.paint(&p, &self.view, &self.doc);
        self.reference_overlay(&p);
        if self.set.pixel_grid && self.view.zoom >= self.set.pixel_grid_from {
            self.pixel_grid(&p, rect);
        }
        p.add(Shape::closed_line(quad, Stroke::new(1.0, Color32::from_black_alpha(80))));
        self.tool_overlay(ui, &p, rect);

        // ---- Cursor.
        self.cursor_doc = hover.map(|h| self.view.screen_to_doc(h));
        if let Some(hp) = hover {
            let dragging = resp.dragged();
            let icon = if space || resp.dragged_by(PointerButton::Middle) || self.tool == Tool::Hand {
                if dragging { CursorIcon::Grabbing } else { CursorIcon::Grab }
            } else if rkey {
                CursorIcon::AllScroll
            } else {
                match self.tool {
                    Tool::Zoom if alt => CursorIcon::ZoomOut,
                    Tool::Zoom => CursorIcon::ZoomIn,
                    Tool::Brush | Tool::Eraser | Tool::Smudge if !alt => CursorIcon::None,
                    Tool::Move => CursorIcon::Move,
                    Tool::Text => CursorIcon::Text,
                    _ => CursorIcon::Crosshair,
                }
            };
            ctx.set_cursor_icon(icon);
            let hide = self.set.hide_cursor_while_drawing && self.stroke.is_some();
            if icon == CursorIcon::None && !hide {
                let r = (self.brush().size / 2.0 * self.view.zoom).max(self.set.cursor_min_size.max(1.5));
                let dark = Stroke::new(2.2, Color32::from_black_alpha(150));
                let light = Stroke::new(1.0, Color32::from_white_alpha(230));
                let circle = matches!(self.set.cursor, CursorStyle::Outline | CursorStyle::OutlineCross);
                let cross = matches!(self.set.cursor, CursorStyle::OutlineCross | CursorStyle::Cross);
                if circle {
                    p.circle_stroke(hp, r, dark);
                    p.circle_stroke(hp, r, light);
                }
                if cross {
                    for (a, b) in [(vec2(-5.0, 0.0), vec2(5.0, 0.0)), (vec2(0.0, -5.0), vec2(0.0, 5.0))] {
                        p.line_segment([hp + a, hp + b], dark);
                        p.line_segment([hp + a, hp + b], light);
                    }
                }
                if self.set.cursor == CursorStyle::Dot || (circle && !cross && r > 6.0) {
                    p.circle_filled(hp, 2.0, Color32::from_black_alpha(160));
                    p.circle_filled(hp, 1.2, Color32::from_white_alpha(230));
                }
            }
        }

        self.selection_bar(ui, rect);
        self.view_bar(ui, rect);
    }

    /// Thin lines between pixels when zoomed in far (for pixel art and clean edges).
    fn pixel_grid(&self, p: &egui::Painter, rect: Rect) {
        let q = self.view.visible_quad(rect);
        let (mut x0, mut y0, mut x1, mut y1) = (f32::MAX, f32::MAX, f32::MIN, f32::MIN);
        for c in q {
            x0 = x0.min(c.x);
            y0 = y0.min(c.y);
            x1 = x1.max(c.x);
            y1 = y1.max(c.y);
        }
        let (x0, y0) = (x0.floor().max(0.0), y0.floor().max(0.0));
        let (x1, y1) = (x1.ceil().min(self.doc.w as f32), y1.ceil().min(self.doc.h as f32));
        if x1 - x0 > 600.0 || y1 - y0 > 600.0 {
            return;
        }
        let st = Stroke::new(1.0, Color32::from_black_alpha(45));
        let mut x = x0;
        while x <= x1 {
            p.line_segment([self.view.doc_to_screen(pos2(x, y0)), self.view.doc_to_screen(pos2(x, y1))], st);
            x += 1.0;
        }
        let mut y = y0;
        while y <= y1 {
            p.line_segment([self.view.doc_to_screen(pos2(x0, y)), self.view.doc_to_screen(pos2(x1, y))], st);
            y += 1.0;
        }
    }

    /// Floating bar at the bottom of the canvas: zoom, fit, rotation and mirror.
    fn view_bar(&mut self, ui: &mut Ui, rect: Rect) {
        let bar = Rect::from_center_size(pos2(rect.center().x, rect.bottom() - 30.0), vec2(436.0, 38.0));
        let c = rect.center();
        ui.scope_builder(UiBuilder::new().max_rect(bar), |ui| {
            egui::Frame::new()
                .fill(PANEL.gamma_multiply(0.96))
                .stroke(Stroke::new(1.0, LINE))
                .corner_radius(CornerRadius::same(19))
                .inner_margin(Margin::symmetric(10, 3))
                .show(ui, |ui| {
                    ui.horizontal_centered(|ui| {
                        ui.spacing_mut().item_spacing.x = 2.0;
                        if icon_button(ui, Icon::ZoomOut, false, 28.0, "Zoom out (Ctrl+-)").clicked() {
                            self.view.zoom_at(c, 0.8);
                        }
                        let z = format!("{:.0}%", self.view.zoom * 100.0);
                        if ui
                            .add(egui::Button::new(RichText::new(z).size(12.5)).frame(false).min_size(vec2(50.0, 24.0)))
                            .on_hover_text("Click: actual pixels, 100% (Ctrl+Alt+0)")
                            .clicked()
                        {
                            self.view.set_zoom(1.0);
                        }
                        if icon_button(ui, Icon::ZoomIn, false, 28.0, "Zoom in (Ctrl+=) · wheel zooms at the pointer").clicked() {
                            self.view.zoom_at(c, 1.25);
                        }
                        if icon_button(ui, Icon::Fit, false, 28.0, "Fit the page (Ctrl+0)").clicked() {
                            self.view.fit(rect, self.doc_size());
                        }
                        let (r, _) = ui.allocate_exact_size(vec2(10.0, 22.0), Sense::hover());
                        ui.painter().line_segment([r.center_top(), r.center_bottom()], Stroke::new(1.0, LINE));
                        let step = self.set.rotate_snap.to_radians();
                        if icon_button(ui, Icon::RotateLeft, false, 28.0, "Rotate left · R + drag rotates freely, Shift snaps").clicked() {
                            self.view.rotate(-step);
                        }
                        let a = format!("{:.0}°", self.view.angle_deg());
                        if ui
                            .add(egui::Button::new(RichText::new(a).size(12.5)).frame(false).min_size(vec2(42.0, 24.0)))
                            .on_hover_text("Click: straighten (0°)")
                            .clicked()
                        {
                            self.straighten();
                        }
                        if icon_button(ui, Icon::RotateRight, false, 28.0, "Rotate right").clicked() {
                            self.view.rotate(step);
                        }
                        if icon_button(ui, Icon::Flip, self.view.flip, 28.0, "Mirror the view (F) — only the view, not the drawing").clicked() {
                            self.view.flip = !self.view.flip;
                        }
                        self.symmetry_button(ui);
                        self.check_button(ui);
                    });
                });
        });
    }
}

/// The picture as it is shown (all layers), read by pixel: for mixing brushes and the color picker.
fn sampler(r: &CanvasRenderer) -> impl Fn(i32, i32) -> Option<Color32> + '_ {
    move |x, y| if x < 0 || y < 0 { None } else { r.pixel(x as usize, y as usize) }
}

fn checker(p: &egui::Painter, r: Rect) {
    let a = Color32::from_rgb(200, 200, 200);
    let b = Color32::from_rgb(240, 240, 240);
    p.rect_filled(r, 0.0, b);
    let s = (r.width().min(r.height()) / 4.0).max(4.0);
    let p = p.with_clip_rect(r.intersect(p.clip_rect()));
    let mut y = r.top();
    let mut row = 0;
    while y < r.bottom() {
        let mut x = r.left() + if row % 2 == 0 { 0.0 } else { s };
        while x < r.right() {
            p.rect_filled(Rect::from_min_size(pos2(x, y), Vec2::splat(s)), 0.0, a);
            x += 2.0 * s;
        }
        y += s;
        row += 1;
    }
}

fn hex_of(c: Color32) -> String {
    format!("#{:02X}{:02X}{:02X}", c.r(), c.g(), c.b())
}

fn parse_hex(s: &str) -> Option<Color32> {
    let s = s.trim().trim_start_matches('#');
    let full = match s.len() {
        3 => s.chars().flat_map(|c| [c, c]).collect::<String>(),
        6 => s.to_owned(),
        _ => return None,
    };
    let v = u32::from_str_radix(&full, 16).ok()?;
    Some(Color32::from_rgb((v >> 16) as u8, (v >> 8) as u8, v as u8))
}
