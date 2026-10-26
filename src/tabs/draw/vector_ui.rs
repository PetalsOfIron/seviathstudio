// SPDX-License-Identifier: GPL-3.0-or-later
// Draw tab: vector layers. The vector pen draws lines that keep their points and pen pressure;
// the eraser cuts them cleanly; Edit mode moves, adds and removes nodes, and the pressure graph
// reshapes the width along a line. Hold the pen still at the end of a shape to straighten it.

use crate::widgets::HelpExt;
use std::collections::HashMap;

use eframe::egui::{self, Color32, CornerRadius, Painter, Pos2, Rect, Response, RichText, Sense, Shape, Stroke, StrokeKind, Ui, pos2, vec2};

use super::{DrawState, Tool};
use crate::brush::engine::Sample;
use crate::doc::{LayerId, LayerKind, TileData, TileKey};
use crate::shapes;
use crate::theme::*;
use crate::vector::{self, EraseMode, LineEnds, LineStyle, VPt, VecStroke};
use crate::widgets::{self, section_title, slider_row};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum VecMode {
    Draw,
    Edit,
}

struct DrawLive {
    layer: LayerId,
    pts: Vec<VPt>,
    /// The layer's pixels before the line was started (cheap: tiles are reference counted).
    before: HashMap<TileKey, TileData>,
    style: LineStyle,
    drawn_to: usize,
    last_move: f64,
    snapped: bool,
}

struct EraseLive {
    layer: LayerId,
    last: (f32, f32),
    r: f32,
}

enum Live {
    Draw(DrawLive),
    Erase(EraseLive),
}

enum EditDrag {
    Node { k: usize, orig: VecStroke, press: Pos2 },
    Stroke { orig: VecStroke, press: Pos2 },
}

pub struct VecState {
    pub mode: VecMode,
    pub style: LineStyle,
    /// 0..1: how much a drawn line is smoothed and simplified when the pen lifts
    pub smooth: f32,
    /// Keep the pen still at the end of a shape to make it clean.
    pub straighten: bool,
    pub erase: EraseMode,
    /// How far (pixels along the line) the neighbours of a moved node follow.
    pub soft: f32,
    live: Option<Live>,
    /// The selected line and node in Edit mode.
    pub sel: Option<usize>,
    pub node: Option<usize>,
    drag: Option<EditDrag>,
    /// Pressure graph: the point being dragged.
    graph_drag: bool,
}

impl Default for VecState {
    fn default() -> Self {
        Self {
            mode: VecMode::Draw,
            style: LineStyle::default(),
            smooth: 0.35,
            straighten: true,
            erase: EraseMode::Cut,
            soft: 40.0,
            live: None,
            sel: None,
            node: None,
            drag: None,
            graph_drag: false,
        }
    }
}

impl VecState {
    /// Another document or an undo: forget what was selected or in progress.
    pub fn reset(&mut self) {
        self.live = None;
        self.sel = None;
        self.node = None;
        self.drag = None;
        self.graph_drag = false;
    }

    pub fn drawing(&self) -> bool {
        self.live.is_some() || self.drag.is_some() || self.graph_drag
    }
}

fn full(doc_w: u32, doc_h: u32) -> vector::Bounds {
    (0, 0, doc_w as i32, doc_h as i32)
}

impl DrawState {
    /// Index of the active layer when it is a vector layer.
    pub(super) fn vec_active_layer(&self) -> Option<usize> {
        let i = self.active_idx();
        (self.doc.layers[i].kind == LayerKind::Vector && self.doc.layers[i].text.is_none()).then_some(i)
    }

    pub(super) fn vec_tool_active(&self) -> bool {
        self.tool == Tool::Pen || (self.tool == Tool::Eraser && self.vec_active_layer().is_some())
    }

    /// Why a stroke can't start on the active layer (None = it can).
    pub(super) fn block_reason(&self) -> Option<&'static str> {
        if self.vec_tool_active() {
            return self.vec_can_start().err();
        }
        let l = &self.doc.layers[self.active_idx()];
        if l.paintable() {
            None
        } else if l.locked {
            Some("This layer is locked.")
        } else if l.text.is_some() {
            Some("This is a text layer: change it with the Text tool (T).")
        } else if l.kind == LayerKind::Vector {
            Some("This is a vector layer: draw with the vector pen (P), cut with the eraser.")
        } else {
            Some("Pick a pixel layer to paint on.")
        }
    }

    pub(super) fn vec_live(&self) -> bool {
        self.vec.live.is_some()
    }

    /// Can a vector stroke start on the active layer? Err gives the reason.
    pub(super) fn vec_can_start(&self) -> Result<(), &'static str> {
        let l = &self.doc.layers[self.active_idx()];
        if l.text.is_some() {
            return Err("This is a text layer: change it with the Text tool.");
        }
        match self.tool {
            Tool::Pen if l.kind != LayerKind::Vector => Ok(()), // a vector layer is made for it
            _ if l.locked => Err("This layer is locked."),
            _ => Ok(()),
        }
    }

    fn mark_bounds(&mut self, b: vector::Bounds) {
        let (w, h) = (self.doc.w as i32, self.doc.h as i32);
        self.doc.mark_dirty_rect(b.0.max(0), b.1.max(0), b.2.min(w), b.3.min(h));
    }

    pub(super) fn vec_begin(&mut self, start: Sample, now: f64) {
        if self.tool == Tool::Eraser {
            let Some(li) = self.vec_active_layer() else { return };
            let r = (self.lib.mem[1].s.size / 2.0).max(0.5);
            let id = self.doc.layers[li].id;
            self.vec.live = Some(Live::Erase(EraseLive { layer: id, last: (start.x, start.y), r }));
            self.label_hint = Some(format!("Vector erase · {}", self.doc.layers[li].name));
            self.vec_erase(id, &[(start.x, start.y, r)]);
            return;
        }
        if self.vec_active_layer().is_none() {
            self.add_layer(LayerKind::Vector);
        }
        let li = self.active_idx();
        let mut style = self.vec.style;
        style.color = [self.primary.r(), self.primary.g(), self.primary.b()];
        let layer = &self.doc.layers[li];
        self.label_hint = Some(format!("Vector line · {}", layer.name));
        self.vec.live = Some(Live::Draw(DrawLive {
            layer: layer.id,
            pts: vec![[start.x, start.y, start.p]],
            before: vector::snapshot(layer),
            style,
            drawn_to: 0,
            last_move: now,
            snapped: false,
        }));
        self.vec.sel = None;
    }

    fn vec_erase(&mut self, layer: LayerId, path: &[(f32, f32, f32)]) {
        let Some(li) = self.doc.index_of(layer) else { return };
        let layer = &mut self.doc.layers[li];
        let Some(data) = layer.vector.as_deref() else { return };
        let mut area: Option<vector::Bounds> = None;
        let mut new_strokes = None;
        let mut new_fills = None;
        if let Some((strokes, a)) = vector::erase(&data.strokes, path, self.vec.erase, data.width) {
            new_strokes = Some(strokes);
            area = Some(vector::union(area, a));
        }
        if self.vec.erase == EraseMode::Whole {
            if let Some((fills, a)) = vector::erase_fills(&data.fills, path) {
                new_fills = Some(fills);
                area = Some(vector::union(area, a));
            }
        }
        let Some(area) = area else { return };
        let data = vector::data_mut(layer);
        if let Some(s) = new_strokes {
            data.strokes = s;
        }
        if let Some(f) = new_fills {
            data.fills = f;
        }
        vector::render_region(layer, area);
        self.mark_bounds(area);
    }

    pub(super) fn vec_add(&mut self, samples: &[Sample], now: f64, ctx: &egui::Context) {
        let Some(mut live) = self.vec.live.take() else { return };
        self.vec_add_inner(&mut live, samples, now, ctx);
        self.vec.live = Some(live);
    }

    fn vec_add_inner(&mut self, live: &mut Live, samples: &[Sample], now: f64, ctx: &egui::Context) {
        match live {
            Live::Erase(e) => {
                let (mut last, r) = (e.last, e.r);
                let mut path = vec![(last.0, last.1, r)];
                for s in samples {
                    path.push((s.x, s.y, r));
                    last = (s.x, s.y);
                }
                e.last = last;
                if path.len() > 1 {
                    self.vec_erase(e.layer, &path);
                }
            }
            Live::Draw(d) => {
                if d.snapped {
                    return;
                }
                if !samples.is_empty() {
                    d.last_move = now;
                }
                for s in samples {
                    let q = [s.x, s.y, s.p];
                    if d.pts.last().is_none_or(|l| (l[0] - q[0]).hypot(l[1] - q[1]) > 0.05) {
                        d.pts.push(q);
                    }
                }
                let Some(li) = self.doc.index_of(d.layer) else { return };
                let width = self.doc.layers[li].vector.as_ref().map(|v| v.width).unwrap_or(1.0);
                if d.pts.len() >= 2 {
                    let discs = vector::outline(&d.pts, &d.style, width, true);
                    let from = d.drawn_to.saturating_sub(1);
                    vector::draw_live(&mut self.doc.layers[li], &discs, d.drawn_to, &d.style, full(self.doc.w, self.doc.h));
                    let (mut x0, mut y0, mut x1, mut y1) = (f32::MAX, f32::MAX, f32::MIN, f32::MIN);
                    for q in &discs[from..] {
                        x0 = x0.min(q.x - q.r);
                        y0 = y0.min(q.y - q.r);
                        x1 = x1.max(q.x + q.r);
                        y1 = y1.max(q.y + q.r);
                    }
                    d.drawn_to = discs.len() - 1;
                    let b = (x0.floor() as i32 - 2, y0.floor() as i32 - 2, x1.ceil() as i32 + 2, y1.ceil() as i32 + 2);
                    self.mark_bounds(b);
                }
                // hold the pen still: make the shape clean
                if self.vec.straighten && now - d.last_move > 0.6 && d.pts.len() >= 8 {
                    if let Some((_, clean)) = shapes::recognize(&d.pts) {
                        self.doc.layers[li].tiles = d.before.clone();
                        d.pts = clean;
                        d.snapped = true;
                        let discs = vector::outline(&d.pts, &d.style, width, false);
                        vector::draw_live(&mut self.doc.layers[li], &discs, 0, &d.style, full(self.doc.w, self.doc.h));
                        self.doc.mark_all_dirty();
                    }
                }
                if self.vec.straighten {
                    ctx.request_repaint_after_secs(0.12);
                }
            }
        }
    }

    pub(super) fn vec_finish(&mut self) {
        let Some(live) = self.vec.live.take() else { return };
        let Live::Draw(d) = live else { return };
        let Some(li) = self.doc.index_of(d.layer) else { return };
        let (w, h) = (self.doc.w, self.doc.h);
        let layer = &mut self.doc.layers[li];
        layer.tiles = d.before;
        let pts = if d.snapped { d.pts } else { vector::clean_line(&d.pts, self.vec.smooth) };
        let stroke = VecStroke { pts, style: d.style };
        let data = vector::data_mut(layer);
        data.strokes.push(stroke.clone());
        let width = data.width;
        vector::draw_stroke(layer, &stroke, width, full(w, h));
        self.doc.mark_all_dirty();
        if let Some(p) = stroke.pts.last() {
            self.tools.last_end = Some(pos2(p[0], p[1]));
        }
    }

    // ---------- Edit mode ----------

    fn edit_tol(&self) -> f32 {
        8.0 / self.view.zoom.max(0.01)
    }

    fn replace_stroke(&mut self, li: usize, idx: usize, new: VecStroke) {
        let layer = &mut self.doc.layers[li];
        let data = vector::data_mut(layer);
        let width = data.width;
        let Some(old) = data.strokes.get(idx) else { return };
        let b = vector::union(Some(vector::stroke_bounds(old, width)), vector::stroke_bounds(&new, width));
        data.strokes[idx] = new;
        vector::render_region(layer, b);
        self.mark_bounds(b);
    }

    fn delete_stroke(&mut self, li: usize, idx: usize) {
        let layer = &mut self.doc.layers[li];
        let data = vector::data_mut(layer);
        if idx >= data.strokes.len() {
            return;
        }
        let b = vector::stroke_bounds(&data.strokes[idx], data.width);
        data.strokes.remove(idx);
        vector::render_region(layer, b);
        self.mark_bounds(b);
        self.vec.sel = None;
        self.vec.node = None;
        self.label_hint = Some("Delete vector line".into());
    }

    /// Delete / Backspace in Edit mode: the selected node, or the whole line.
    pub(super) fn vec_delete(&mut self) {
        let (Some(li), Some(idx)) = (self.vec_active_layer(), self.vec.sel) else { return };
        if self.doc.layers[li].locked {
            return;
        }
        let Some(s) = self.doc.layers[li].vector.as_ref().and_then(|d| d.strokes.get(idx)).cloned() else { return };
        match self.vec.node {
            Some(k) if s.pts.len() > 2 => {
                self.replace_stroke(li, idx, vector::delete_node(&s, k));
                self.vec.node = None;
                self.label_hint = Some("Delete node".into());
            }
            _ => self.delete_stroke(li, idx),
        }
    }

    pub(super) fn vec_editing_selection(&self) -> bool {
        self.tool == Tool::Pen && self.vec.mode == VecMode::Edit && self.vec.sel.is_some()
    }

    pub(super) fn vec_deselect(&mut self) {
        self.vec.sel = None;
        self.vec.node = None;
    }

    /// Pointer in Edit mode. Returns true when the pen tool is in Edit mode (the pointer is ours).
    pub(super) fn vec_edit_input(&mut self, resp: &Response, ctx: &egui::Context) -> bool {
        if self.vec.mode != VecMode::Edit {
            return false;
        }
        let (pressed, down, shift, latest) = ctx.input(|i| (i.pointer.primary_pressed(), i.pointer.primary_down(), i.modifiers.shift, i.pointer.latest_pos()));
        let over = resp.hovered() || resp.is_pointer_button_down_on();
        let Some(li) = self.vec_active_layer() else {
            if pressed && over {
                self.notice = Some("Pick a vector layer to edit its lines. Drawing with the pen makes one.");
            }
            return true;
        };
        let locked = self.doc.layers[li].locked;
        let Some(sp) = latest else { return true };
        let d = self.view.screen_to_doc(sp);
        let tol = self.edit_tol();
        if pressed && over {
            let width = self.doc.layers[li].vector.as_ref().map(|v| v.width).unwrap_or(1.0);
            let strokes: Vec<VecStroke> = self.doc.layers[li].vector.as_ref().map(|v| v.strokes.clone()).unwrap_or_default();
            let sel_stroke = self.vec.sel.and_then(|i| strokes.get(i)).cloned();
            self.vec.drag = None;
            if let Some(s) = &sel_stroke {
                if let Some(k) = vector::nearest_node(s, d.x, d.y, tol * 1.4, 18.0 / self.view.zoom.max(0.01)) {
                    self.vec.node = Some(k);
                    if !locked {
                        self.vec.drag = Some(EditDrag::Node { k, orig: s.clone(), press: d });
                        self.label_hint = Some("Move node".into());
                    }
                    return true;
                }
                if vector::hit_stroke(std::slice::from_ref(s), d.x, d.y, tol, width).is_some() {
                    if !locked {
                        if shift {
                            self.vec.drag = Some(EditDrag::Stroke { orig: s.clone(), press: d });
                            self.label_hint = Some("Move vector line".into());
                        } else {
                            // pull a new node out of the line
                            let (ns, k) = vector::insert_node(s, d.x, d.y);
                            if let Some(idx) = self.vec.sel {
                                self.replace_stroke(li, idx, ns.clone());
                            }
                            self.vec.node = Some(k);
                            self.vec.drag = Some(EditDrag::Node { k, orig: ns, press: d });
                            self.label_hint = Some("Add node".into());
                        }
                    }
                    return true;
                }
            }
            match vector::hit_stroke(&strokes, d.x, d.y, tol, width) {
                Some(i) => {
                    self.vec.sel = Some(i);
                    self.vec.node = None;
                }
                None => self.vec_deselect(),
            }
            return true;
        }
        if let (Some(drag), true) = (&self.vec.drag, down) {
            let Some(idx) = self.vec.sel else { return true };
            let soft = self.vec.soft;
            let new = match drag {
                EditDrag::Node { k, orig, press } => vector::move_node(orig, *k, d.x - press.x, d.y - press.y, soft),
                EditDrag::Stroke { orig, press } => vector::move_stroke(orig, d.x - press.x, d.y - press.y),
            };
            self.replace_stroke(li, idx, new);
        }
        if !down {
            self.vec.drag = None;
        }
        true
    }

    /// The selected line and its nodes, drawn over the canvas in Edit mode.
    pub(super) fn vec_overlay(&self, p: &Painter) {
        if self.tool != Tool::Pen || self.vec.mode != VecMode::Edit {
            return;
        }
        let (Some(li), Some(idx)) = (self.vec_active_layer(), self.vec.sel) else { return };
        let Some(s) = self.doc.layers[li].vector.as_ref().and_then(|d| d.strokes.get(idx)) else { return };
        let path: Vec<Pos2> = vector::densify(&s.pts, 3.0).iter().map(|q| self.view.doc_to_screen(pos2(q[0], q[1]))).collect();
        p.add(Shape::line(path.clone(), Stroke::new(5.0, Color32::from_black_alpha(110))));
        p.add(Shape::line(path, Stroke::new(1.6, VENOM)));
        for k in vector::key_nodes(s, 18.0 / self.view.zoom.max(0.01)) {
            let c = self.view.doc_to_screen(pos2(s.pts[k][0], s.pts[k][1]));
            let on = self.vec.node == Some(k);
            let r = Rect::from_center_size(c, vec2(9.0, 9.0));
            p.rect_filled(r, CornerRadius::same(2), if on { VENOM } else { Color32::WHITE });
            p.rect_stroke(r, CornerRadius::same(2), Stroke::new(1.4, Color32::BLACK), StrokeKind::Outside);
        }
    }

    // ---------- Options ----------

    pub(super) fn vector_options(&mut self, ui: &mut Ui) {
        let hint = |ui: &mut Ui, t: &str| {
            ui.label(RichText::new(t).size(11.5).color(TEXT_DIM));
        };
        widgets::segmented(ui, &mut self.vec.mode, &[(VecMode::Draw, "Draw"), (VecMode::Edit, "Edit lines")]);
        ui.add_space(4.0);
        let li = self.vec_active_layer();
        match self.vec.mode {
            VecMode::Draw => {
                hint(ui, "Lines keep their points and pressure, so you can reshape or thicken them later. Drawing on a pixel layer starts a new vector layer.");
                ui.add_space(2.0);
                let st = &mut self.vec.style;
                slider_row(ui, "Size", &mut st.size, 0.5..=200.0, " px");
                ui.checkbox(&mut st.size_pressure, "Pressure changes the width").help("A harder press makes the line wider.");
                if st.size_pressure {
                    let mut m = st.min_size * 100.0;
                    if slider_row(ui, "Smallest", &mut m, 0.0..=100.0, "%").changed() {
                        st.min_size = m / 100.0;
                    }
                }
                widgets::segmented(ui, &mut st.ends, &[(LineEnds::Round, "Round"), (LineEnds::Taper, "Pointed"), (LineEnds::Flat, "Flat")]);
                if st.ends == LineEnds::Taper {
                    slider_row(ui, "Taper", &mut st.taper, 0.0..=2.0, "");
                }
                let mut o = st.opacity * 100.0;
                if slider_row(ui, "Opacity", &mut o, 1.0..=100.0, "%").changed() {
                    st.opacity = o / 100.0;
                }
                let mut sm = self.vec.smooth * 100.0;
                if slider_row(ui, "Smoothing", &mut sm, 0.0..=100.0, "%").changed() {
                    self.vec.smooth = sm / 100.0;
                }
                hint(ui, "0 keeps your hand exactly. Higher gives smoother curves with fewer points.");
                ui.checkbox(&mut self.vec.straighten, "Hold still to straighten").help("Keep the pen still for a moment at the end of a circle, rectangle, triangle or line");
            }
            VecMode::Edit => {
                hint(ui, "Click a line to select it. Drag its nodes. Drag along the line to pull out a new node. Shift + drag moves the whole line. Delete removes the node or the line.");
                ui.add_space(2.0);
                slider_row(ui, "Follow", &mut self.vec.soft, 0.0..=200.0, " px");
                hint(ui, "How far the nodes next to a moved node follow it along the line.");
                ui.add_space(4.0);
                self.vec_selected_options(ui);
            }
        }
        ui.add_space(6.0);
        if li.is_none() && self.doc.layers[self.active_idx()].kind == LayerKind::Raster {
            section_title(ui, "Trace");
            hint(ui, "Turn a pixel drawing into editable lines and color shapes.");
            if ui.button("Auto-trace this layer…").help("Turn the pixels of this layer into smooth vector lines.").clicked() {
                let id = self.active;
                self.open_trace(id);
            }
            ui.add_space(6.0);
        }
        if let Some(li) = li {
            section_title(ui, "This layer");
            let w = self.doc.layers[li].vector.as_ref().map(|v| v.width).unwrap_or(1.0);
            let mut pct = w * 100.0;
            let r = slider_row(ui, "Line width", &mut pct, 20.0..=400.0, "%");
            if r.changed() && !self.doc.layers[li].locked {
                let layer = &mut self.doc.layers[li];
                vector::data_mut(layer).width = pct / 100.0;
                let (dw, dh) = (self.doc.w, self.doc.h);
                vector::render_all(layer, dw, dh);
                self.doc.mark_all_dirty();
                self.label_hint = Some("Line width".into());
            }
            hint(ui, "Makes every line of this layer thicker or thinner, then and later.");
        }
    }

    fn vec_selected_options(&mut self, ui: &mut Ui) {
        let (Some(li), Some(idx)) = (self.vec_active_layer(), self.vec.sel) else {
            ui.label(RichText::new("No line selected.").size(12.0).color(TEXT_DIM));
            return;
        };
        let Some(s) = self.doc.layers[li].vector.as_ref().and_then(|d| d.strokes.get(idx)).cloned() else { return };
        let locked = self.doc.layers[li].locked;
        section_title(ui, "Selected line");
        ui.label(RichText::new(format!("{} nodes", s.pts.len())).size(12.0).color(TEXT_DIM));
        let mut ns = s.clone();
        ui.horizontal(|ui| {
            ui.label("Color");
            egui::color_picker::color_edit_button_srgb(ui, &mut ns.style.color);
        });
        slider_row(ui, "Size", &mut ns.style.size, 0.5..=200.0, " px");
        let mut o = ns.style.opacity * 100.0;
        if slider_row(ui, "Opacity", &mut o, 1.0..=100.0, "%").changed() {
            ns.style.opacity = o / 100.0;
        }
        widgets::segmented(ui, &mut ns.style.ends, &[(LineEnds::Round, "Round"), (LineEnds::Taper, "Pointed"), (LineEnds::Flat, "Flat")]);
        ui.checkbox(&mut ns.style.size_pressure, "Pressure changes the width").help("A harder press makes the line wider.");
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing = vec2(4.0, 4.0);
            if ui.button("Smoother").help("Make the line smoother.").clicked() {
                ns = vector::smooth_stroke(&s, 0.4);
                self.label_hint = Some("Smooth vector line".into());
            }
            if ui.button("Fewer nodes").help("Take away points that are not needed. The line stays almost the same.").clicked() {
                ns = vector::simplify_stroke(&s, 1.5);
                self.label_hint = Some("Simplify vector line".into());
            }
            if ui.button("More nodes").help("A node every 6 pixels, to shape the pressure finely").clicked() {
                ns = vector::densify_stroke(&s, 6.0);
                self.label_hint = Some("Add nodes".into());
            }
            if ui.button(RichText::new("Delete line").color(Color32::from_rgb(232, 110, 110))).help("Delete this line.").clicked() {
                self.vec_delete_whole(li, idx);
            }
        });
        // pressure graph
        ui.add_space(4.0);
        ui.label(RichText::new("Width along the line (left = start). Drag up or down.").size(11.5).color(TEXT_DIM));
        if let Some(p) = pressure_graph(ui, &ns, &mut self.vec.graph_drag) {
            ns = vector::set_pressures(&ns, &p);
            self.label_hint = Some("Line pressure".into());
        }
        if ns != s && !locked && self.vec.sel == Some(idx) && li < self.doc.layers.len() {
            if self.label_hint.is_none() {
                self.label_hint = Some("Edit vector line".into());
            }
            self.replace_stroke(li, idx, ns);
        }
    }

    fn vec_delete_whole(&mut self, li: usize, idx: usize) {
        if !self.doc.layers[li].locked {
            self.delete_stroke(li, idx);
        }
    }
}

/// The pressure of each node as a graph. Returns the new pressures while the user drags.
fn pressure_graph(ui: &mut Ui, s: &VecStroke, dragging: &mut bool) -> Option<Vec<f32>> {
    let w = ui.available_width();
    let (rect, resp) = ui.allocate_exact_size(vec2(w, 72.0), Sense::click_and_drag());
    let frac = vector::node_fractions(s);
    let mut pr: Vec<f32> = s.pts.iter().map(|p| p[2]).collect();
    let mut changed = false;
    if resp.dragged() || resp.is_pointer_button_down_on() {
        if let Some(pp) = resp.interact_pointer_pos() {
            *dragging = true;
            let x = ((pp.x - rect.left()) / rect.width()).clamp(0.0, 1.0);
            let y = (1.0 - (pp.y - rect.top() - 3.0) / (rect.height() - 6.0)).clamp(0.0, 1.0);
            // nodes close to the pointer take the value, fading out over a few percent of the line
            for (i, f) in frac.iter().enumerate() {
                let near = 1.0 - ((f - x).abs() / 0.06).min(1.0);
                if near > 0.0 {
                    pr[i] = pr[i] + (y - pr[i]) * near.powf(0.7);
                    changed = true;
                }
            }
        }
    } else {
        *dragging = false;
    }
    let p = ui.painter_at(rect);
    p.rect_filled(rect, CornerRadius::same(6), BG);
    for g in [0.25, 0.5, 0.75] {
        let y = rect.bottom() - g * rect.height();
        p.line_segment([pos2(rect.left(), y), pos2(rect.right(), y)], Stroke::new(1.0, LINE));
    }
    let at = |i: usize| pos2(rect.left() + frac[i] * rect.width(), rect.bottom() - 3.0 - pr[i] * (rect.height() - 6.0));
    p.add(Shape::line((0..pr.len()).map(at).collect(), Stroke::new(2.0, VENOM)));
    p.rect_stroke(rect, CornerRadius::same(6), Stroke::new(1.0, LINE), StrokeKind::Outside);
    changed.then_some(pr)
}

#[cfg(test)]
impl DrawState {
    pub fn vec_mut(&mut self) -> &mut VecState {
        &mut self.vec
    }

    pub fn vector_strokes(&self, li: usize) -> Vec<VecStroke> {
        self.doc.layers[li].vector.as_ref().map(|v| v.strokes.clone()).unwrap_or_default()
    }

    /// Select a line in Edit mode (no pointer needed).
    pub fn test_vec_select(&mut self, idx: usize) {
        self.vec.sel = Some(idx);
        self.vec.node = None;
    }

    pub fn test_doc_to_screen(&self, p: Pos2) -> Pos2 {
        self.view.doc_to_screen(p)
    }
}
