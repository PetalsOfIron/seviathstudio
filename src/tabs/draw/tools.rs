// SPDX-License-Identifier: GPL-3.0-or-later
// Draw tab: selection (rectangle, ellipse, magic wand, lasso, polygon lasso), quick mask,
// paint bucket and lasso fill, move & transform, live symmetry and their option panels.
// The canvas calls `tool_input` before painting and `tool_overlay` after the picture is drawn.

use eframe::egui::{
    self, Align, Align2, Color32, CornerRadius, CursorIcon, Key, Layout, Margin, Mesh, Modifiers, Pos2, Rect, Response, RichText,
    Shape, Stroke, StrokeKind, TextureHandle, TextureOptions, Ui, UiBuilder, Vec2, pos2, vec2,
};

use super::{DrawState, Tool};
use crate::doc::{LayerId, LayerKind};
use crate::fill::{self, FillOpts};
use crate::icons::Icon;
use crate::select::{Combine, Dense, Selection};
use crate::symmetry::{SymMode, Symmetry};
use crate::theme::*;
use crate::transform::{Floating, Homography};
use crate::widgets::{self, icon_button, section_title, segmented};

/// Name of the temporary layer that holds the quick mask.
pub const QUICK_MASK: &str = "Quick mask";
const QM_COLOR: [u8; 3] = [230, 40, 70];

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SelShape {
    Rect,
    Ellipse,
    Wand,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Handle {
    Move,
    Corner(usize),
    Edge(usize),
    Rotate,
}

struct XfDrag {
    handle: Handle,
    start: Pos2,
    corners: [Pos2; 4],
    ctrl: bool,
    alt: bool,
    /// Move tool used straight on the canvas: put the picture down when the pointer lifts.
    quick: bool,
}

struct Gesture {
    start: Pos2,
    pts: Vec<Pos2>,
    combine: Combine,
    erase: bool,
}

pub struct ToolState {
    pub sel_shape: SelShape,
    pub lasso_poly: bool,
    pub combine: Combine,
    pub feather: f32,
    pub smooth: bool,
    pub wand_tol: u8,
    pub wand_contiguous: bool,
    pub wand_all: bool,
    /// Grow / shrink / feather buttons work by this many pixels.
    pub edit_px: f32,
    pub fill: FillOpts,
    pub fill_lasso: bool,
    pub sym: Symmetry,
    sym_drag: bool,
    pub sym_open: bool,
    gesture: Option<Gesture>,
    poly: Vec<Pos2>,
    pub floating: Option<Floating>,
    xf: Option<XfDrag>,
    tex: Option<TextureHandle>,
    ants: (u64, Vec<Vec<Pos2>>),
    qm_return: Option<LayerId>,
    /// Where the last brush stroke ended (Shift + click draws a straight line from there).
    pub last_end: Option<Pos2>,
    pub notice: Option<String>,
}

impl Default for ToolState {
    fn default() -> Self {
        Self {
            sel_shape: SelShape::Rect,
            lasso_poly: false,
            combine: Combine::New,
            feather: 0.0,
            smooth: true,
            wand_tol: 32,
            wand_contiguous: true,
            wand_all: true,
            edit_px: 8.0,
            fill: FillOpts::default(),
            fill_lasso: false,
            sym: Symmetry::default(),
            sym_drag: false,
            sym_open: false,
            gesture: None,
            poly: Vec::new(),
            floating: None,
            xf: None,
            tex: None,
            ants: (0, Vec::new()),
            qm_return: None,
            last_end: None,
            notice: None,
        }
    }
}

impl ToolState {
    /// Another document: keep the tool options, drop everything in progress.
    pub fn new_document(&mut self, w: u32, h: u32) {
        self.sym.cx = (w / 2) as f32;
        self.sym.cy = (h / 2) as f32;
        self.gesture = None;
        self.poly.clear();
        self.floating = None;
        self.xf = None;
        self.tex = None;
        self.ants = (0, Vec::new());
        self.qm_return = None;
        self.last_end = None;
        self.notice = None;
    }
}

fn combine_from(m: Modifiers, default: Combine) -> Combine {
    match (m.shift, m.alt) {
        (true, true) => Combine::Intersect,
        (true, false) => Combine::Add,
        (false, true) => Combine::Subtract,
        _ => default,
    }
}

fn ellipse(a: Pos2, b: Pos2) -> Vec<Pos2> {
    let c = pos2((a.x + b.x) / 2.0, (a.y + b.y) / 2.0);
    let (rx, ry) = (((b.x - a.x) / 2.0).abs(), ((b.y - a.y) / 2.0).abs());
    let n = ((rx + ry) * 0.6).clamp(24.0, 720.0) as usize;
    (0..n)
        .map(|i| {
            let t = i as f32 / n as f32 * std::f32::consts::TAU;
            pos2(c.x + rx * t.cos(), c.y + ry * t.sin())
        })
        .collect()
}

fn rect_pts(a: Pos2, b: Pos2) -> Vec<Pos2> {
    let (x0, x1) = (a.x.min(b.x).round(), a.x.max(b.x).round());
    let (y0, y1) = (a.y.min(b.y).round(), a.y.max(b.y).round());
    vec![pos2(x0, y0), pos2(x1, y0), pos2(x1, y1), pos2(x0, y1)]
}

impl DrawState {
    pub(super) fn quick_mask_idx(&self) -> Option<usize> {
        self.doc.layers.iter().position(|l| l.name == QUICK_MASK && l.kind == LayerKind::Raster)
    }

    fn say(&mut self, s: impl Into<String>) {
        self.tools.notice = Some(s.into());
    }

    /// The picture the wand and the bucket look at.
    fn reference(&self, all: bool) -> image::RgbaImage {
        if all {
            crate::files::flatten(&self.doc)
        } else {
            let li = self.active_idx();
            crate::files::layer_rgba(&self.doc.layers[li], 0, 0, self.doc.w, self.doc.h)
        }
    }

    // ---------- Selection actions ----------

    fn set_selection(&mut self, new: Option<Selection>, how: Combine, label: &str) {
        let (w, h) = (self.doc.w, self.doc.h);
        let new = match new {
            Some(s) if self.tools.feather >= 0.5 => s.feathered(self.tools.feather, w, h),
            s => s,
        };
        self.doc.selection = Selection::combine(self.doc.selection.as_ref(), new, how);
        self.label_hint = Some(label.to_owned());
    }

    pub fn select_all(&mut self) {
        self.doc.selection = Some(Selection::all(self.doc.w, self.doc.h));
        self.label_hint = Some("Select all".into());
    }

    pub fn deselect(&mut self) {
        if self.doc.selection.is_some() {
            self.doc.selection = None;
            self.label_hint = Some("Deselect".into());
        }
    }

    pub fn invert_selection(&mut self) {
        let (w, h) = (self.doc.w, self.doc.h);
        self.doc.selection = match &self.doc.selection {
            Some(s) => Selection::inverted(Some(s), w, h),
            None => Some(Selection::all(w, h)),
        };
        self.label_hint = Some("Invert selection".into());
    }

    fn grow_selection(&mut self, px: f32) {
        let (w, h) = (self.doc.w, self.doc.h);
        if let Some(s) = &self.doc.selection {
            self.doc.selection = s.grown(px, w, h);
            self.label_hint = Some(if px >= 0.0 { "Grow selection".into() } else { "Shrink selection".into() });
        }
    }

    fn feather_selection(&mut self, px: f32) {
        let (w, h) = (self.doc.w, self.doc.h);
        if let Some(s) = &self.doc.selection {
            self.doc.selection = s.feathered(px, w, h);
            self.label_hint = Some("Feather selection".into());
        }
    }

    fn paintable_layer(&mut self) -> Option<usize> {
        let li = self.active_idx();
        let l = &self.doc.layers[li];
        if l.paintable() {
            Some(li)
        } else {
            self.say(if l.locked { "This layer is locked." } else { "Pick a pixel layer first." });
            None
        }
    }

    /// Delete key: empty the selected area of the layer (the whole layer when nothing is selected).
    pub fn clear_selected(&mut self) {
        let Some(li) = self.paintable_layer() else { return };
        let name = self.doc.layers[li].name.clone();
        match self.doc.selection.clone() {
            Some(sel) => {
                let Some((x0, y0, x1, y1)) = sel.bounds() else { return };
                let d = sel.dense(x0, y0, (x1 - x0) as usize, (y1 - y0) as usize);
                let keys = fill::paint(&mut self.doc.layers[li], &d, Color32::BLACK, 1.0, None, true);
                for k in keys {
                    self.doc.mark_tile_dirty(k);
                }
            }
            None => {
                let l = &mut self.doc.layers[li];
                l.tiles.clear();
                l.rev += 1;
                self.doc.mark_all_dirty();
            }
        }
        self.label_hint = Some(format!("Clear · {name}"));
    }

    /// Fill the selected area (or the whole layer) with the main color.
    pub fn fill_selected(&mut self) {
        let Some(li) = self.paintable_layer() else { return };
        let (w, h) = (self.doc.w, self.doc.h);
        let d = match &self.doc.selection {
            Some(sel) => {
                let Some((x0, y0, x1, y1)) = sel.bounds() else { return };
                sel.dense(x0, y0, (x1 - x0) as usize, (y1 - y0) as usize)
            }
            None => Dense { x0: 0, y0: 0, w: w as usize, h: h as usize, px: vec![255; w as usize * h as usize] },
        };
        let name = self.doc.layers[li].name.clone();
        let keys = fill::paint(&mut self.doc.layers[li], &d, self.primary, self.tools.fill.opacity, None, false);
        for k in keys {
            self.doc.mark_tile_dirty(k);
        }
        self.label_hint = Some(format!("Fill · {name}"));
    }

    // ---------- Quick mask ----------

    /// Q: show the selection as a red layer you paint on (brush adds, eraser removes); Q again turns it back.
    pub fn toggle_quick_mask(&mut self) {
        let (w, h) = (self.doc.w, self.doc.h);
        if let Some(qi) = self.quick_mask_idx() {
            let layer = self.doc.layers.remove(qi);
            let img = crate::files::layer_rgba(&layer, 0, 0, w, h);
            let px: Vec<u8> = img.pixels().map(|p| p.0[3]).collect();
            self.doc.selection = Selection::from_dense(&Dense { x0: 0, y0: 0, w: w as usize, h: h as usize, px }, w, h);
            if let Some(r) = self.tools.qm_return.take().filter(|r| self.doc.index_of(*r).is_some()) {
                self.active = r;
            }
            self.fix_active();
            self.doc.mark_all_dirty();
            self.label_hint = Some("Quick mask off".into());
            return;
        }
        let mut layer = self.doc.new_layer(QUICK_MASK, LayerKind::Raster);
        layer.opacity = 50.0;
        if let Some(sel) = &self.doc.selection {
            let d = sel.dense(0, 0, w as usize, h as usize);
            let img = image::RgbaImage::from_fn(w, h, |x, y| {
                let a = d.px[(y * w + x) as usize];
                image::Rgba([QM_COLOR[0], QM_COLOR[1], QM_COLOR[2], a])
            });
            layer.tiles = crate::files::tiles_from_rgba(&img, 0, 0, w, h);
        }
        layer.parent = None;
        self.tools.qm_return = Some(self.active);
        self.active = layer.id;
        self.doc.layers.insert(0, layer);
        self.doc.selection = None;
        self.doc.mark_all_dirty();
        if !matches!(self.tool, Tool::Brush | Tool::Eraser) {
            self.tool = Tool::Brush;
        }
        self.label_hint = Some("Quick mask on".into());
    }

    // ---------- Move & transform ----------

    /// Lift the selection (or the layer) so it can be moved, scaled, turned and distorted.
    pub fn start_transform(&mut self) -> bool {
        if self.tools.floating.is_some() {
            return true;
        }
        let Some(li) = self.paintable_layer() else { return false };
        match Floating::lift(&mut self.doc, li) {
            Some(f) => {
                self.tools.floating = Some(f);
                self.tools.tex = None;
                self.tool = Tool::Move;
                true
            }
            None => {
                self.say("Nothing to move here: the layer (or the selected part of it) is empty.");
                false
            }
        }
    }

    pub fn apply_transform(&mut self) {
        if let Some(f) = self.tools.floating.take() {
            self.tools.xf = None;
            self.tools.tex = None;
            let name = self.doc.index_of(f.layer).map(|i| self.doc.layers[i].name.clone()).unwrap_or_default();
            f.apply(&mut self.doc);
            self.label_hint = Some(format!("Transform · {name}"));
        }
    }

    pub fn cancel_transform(&mut self) {
        if let Some(f) = self.tools.floating.take() {
            self.tools.xf = None;
            self.tools.tex = None;
            f.cancel(&mut self.doc);
        }
    }

    pub fn transforming(&self) -> bool {
        self.tools.floating.is_some()
    }

    /// Screen positions of the 4 corners and 4 edge middles.
    fn handles(&self) -> Option<([Pos2; 4], [Pos2; 4])> {
        let f = self.tools.floating.as_ref()?;
        let c = f.corners.map(|p| self.view.doc_to_screen(p));
        let e = [0, 1, 2, 3].map(|i| {
            let (a, b) = (f.corners[i], f.corners[(i + 1) % 4]);
            self.view.doc_to_screen(pos2((a.x + b.x) / 2.0, (a.y + b.y) / 2.0))
        });
        Some((c, e))
    }

    fn handle_at(&self, s: Pos2) -> Option<Handle> {
        let (c, e) = self.handles()?;
        let r = 10.0;
        if let Some(i) = c.iter().position(|p| p.distance(s) < r) {
            return Some(Handle::Corner(i));
        }
        if let Some(i) = e.iter().position(|p| p.distance(s) < r) {
            return Some(Handle::Edge(i));
        }
        let f = self.tools.floating.as_ref()?;
        let inside = Homography::square_to_quad(&f.corners)
            .inverse()
            .map(|inv| {
                let d = self.view.screen_to_doc(s);
                let p = inv.map(d.x, d.y);
                (0.0..=1.0).contains(&p.x) && (0.0..=1.0).contains(&p.y)
            })
            .unwrap_or(false);
        Some(if inside { Handle::Move } else { Handle::Rotate })
    }

    fn drag_transform(&mut self, d: Pos2, shift: bool) {
        let Some(x) = &self.tools.xf else { return };
        let Some(f) = self.tools.floating.as_mut() else { return };
        let s = x.corners;
        let delta = d - x.start;
        match x.handle {
            Handle::Move => f.corners = s.map(|c| c + delta),
            Handle::Rotate => {
                let ctr = (s.iter().fold(Vec2::ZERO, |a, c| a + c.to_vec2()) / 4.0).to_pos2();
                let mut a = (d - ctr).angle() - (x.start - ctr).angle();
                if shift {
                    let step = 15f32.to_radians();
                    a = (a / step).round() * step;
                }
                let (sn, cs) = a.sin_cos();
                f.corners = s.map(|c| {
                    let v = c - ctr;
                    ctr + vec2(v.x * cs - v.y * sn, v.x * sn + v.y * cs)
                });
            }
            Handle::Corner(i) if x.ctrl => {
                // free distort / perspective: only this corner moves
                f.corners = s;
                f.corners[i] = s[i] + delta;
            }
            Handle::Edge(i) if x.ctrl => {
                // skew: the edge slides along itself
                let (a, b) = (i, (i + 1) % 4);
                let dir = (s[b] - s[a]).normalized();
                let along = dir * delta.dot(dir);
                f.corners = s;
                f.corners[a] = s[a] + along;
                f.corners[b] = s[b] + along;
            }
            Handle::Corner(_) | Handle::Edge(_) => {
                // scale in the picture's own directions, from the opposite side (or the middle with Alt)
                let u = (s[1] - s[0]).normalized();
                let v = (s[3] - s[0]).normalized();
                let ctr = (s.iter().fold(Vec2::ZERO, |a, c| a + c.to_vec2()) / 4.0).to_pos2();
                let (anchor, grip) = match x.handle {
                    Handle::Corner(i) => (if x.alt { ctr } else { s[(i + 2) % 4] }, s[i]),
                    Handle::Edge(i) => {
                        let mid = |k: usize| pos2((s[k].x + s[(k + 1) % 4].x) / 2.0, (s[k].y + s[(k + 1) % 4].y) / 2.0);
                        (if x.alt { ctr } else { mid((i + 2) % 4) }, mid(i))
                    }
                    _ => unreachable!(),
                };
                let local = |p: Pos2| vec2((p - anchor).dot(u), (p - anchor).dot(v));
                let g0 = local(grip);
                let g1 = local(d - (x.start - grip));
                let mut fx = if g0.x.abs() > 0.5 { g1.x / g0.x } else { 1.0 };
                let mut fy = if g0.y.abs() > 0.5 { g1.y / g0.y } else { 1.0 };
                match x.handle {
                    Handle::Edge(i) if i % 2 == 0 => fx = 1.0, // top / bottom edge: height only
                    Handle::Edge(_) => fy = 1.0,              // left / right edge: width only
                    _ if shift => {
                        // keep the proportions
                        let k = if fx.abs() > fy.abs() { fx } else { fy };
                        fx = k.abs() * fx.signum();
                        fy = k.abs() * fy.signum();
                    }
                    _ => {}
                }
                f.corners = s.map(|c| {
                    let l = local(c);
                    anchor + u * (l.x * fx) + v * (l.y * fy)
                });
            }
        }
    }

    // ---------- Canvas input ----------

    /// Handles the pointer for selection, fill, move and the symmetry center.
    /// Returns true when it used the pointer (so the brush must not paint).
    pub(super) fn tool_input(&mut self, resp: &Response, ctx: &egui::Context) -> bool {
        let (pressed, released, down, mods, latest, origin, double) = ctx.input(|i| {
            (
                i.pointer.primary_pressed(),
                i.pointer.primary_released(),
                i.pointer.primary_down(),
                i.modifiers,
                i.pointer.latest_pos(),
                i.pointer.press_origin(),
                i.pointer.button_double_clicked(egui::PointerButton::Primary),
            )
        });
        let over = resp.hovered() || resp.is_pointer_button_down_on();
        let view = self.view;
        let to_doc = move |p: Pos2| view.screen_to_doc(p);
        let painting = matches!(self.tool, Tool::Brush | Tool::Eraser | Tool::Smudge);

        // the symmetry center can be dragged while a brush is picked
        if self.tools.sym.on() && painting {
            let c = self.view.doc_to_screen(pos2(self.tools.sym.cx, self.tools.sym.cy));
            if pressed && over && latest.is_some_and(|p| p.distance(c) < 11.0) && self.stroke.is_none() {
                self.tools.sym_drag = true;
            }
            if self.tools.sym_drag {
                if let Some(p) = latest {
                    let d = to_doc(p);
                    self.tools.sym.cx = d.x.clamp(0.0, self.doc.w as f32).round();
                    self.tools.sym.cy = d.y.clamp(0.0, self.doc.h as f32).round();
                }
                if !down {
                    self.tools.sym_drag = false;
                }
                return true;
            }
        }

        match self.tool {
            Tool::Select if self.tools.sel_shape == SelShape::Wand => {
                if pressed && over {
                    if let Some(p) = latest {
                        let d = to_doc(p);
                        let img = self.reference(self.tools.wand_all);
                        let (x, y) = (d.x.floor() as i32, d.y.floor() as i32);
                        if x >= 0 && y >= 0 && (x as u32) < self.doc.w && (y as u32) < self.doc.h {
                            let s = crate::select::wand(&img, x as u32, y as u32, self.tools.wand_tol, self.tools.wand_contiguous, self.tools.smooth);
                            let how = combine_from(mods, self.tools.combine);
                            self.set_selection(s, how, "Magic wand");
                        }
                    }
                }
                true
            }
            Tool::Select => {
                if pressed && over {
                    if let Some(o) = origin.or(latest) {
                        let start = to_doc(o);
                        self.tools.gesture = Some(Gesture { start, pts: vec![start], combine: combine_from(mods, self.tools.combine), erase: false });
                    }
                }
                if let (Some(g), Some(p)) = (self.tools.gesture.as_mut(), latest) {
                    let d = to_doc(p);
                    g.pts = vec![g.start, d];
                }
                if released || (!down && self.tools.gesture.is_some()) {
                    if let Some(g) = self.tools.gesture.take() {
                        let end = *g.pts.last().unwrap();
                        let moved = (end - g.start).length() * self.view.zoom > 3.0;
                        if moved {
                            let pts = if self.tools.sel_shape == SelShape::Ellipse { ellipse(g.start, end) } else { rect_pts(g.start, end) };
                            let s = Selection::polygon(&pts, self.doc.w, self.doc.h, self.tools.smooth && self.tools.sel_shape == SelShape::Ellipse);
                            let label = if self.tools.sel_shape == SelShape::Ellipse { "Ellipse selection" } else { "Rectangle selection" };
                            self.set_selection(s, g.combine, label);
                        } else if g.combine == Combine::New {
                            self.deselect();
                        }
                    }
                }
                true
            }
            Tool::Lasso if self.tools.lasso_poly => {
                if pressed && over {
                    if let Some(p) = latest {
                        let d = to_doc(p);
                        let close = self.tools.poly.len() >= 3
                            && (double || self.view.doc_to_screen(self.tools.poly[0]).distance(p) < 9.0);
                        if self.tools.poly.is_empty() {
                            self.tools.gesture = Some(Gesture { start: d, pts: Vec::new(), combine: combine_from(mods, self.tools.combine), erase: false });
                        }
                        if close {
                            self.finish_polygon();
                        } else {
                            self.tools.poly.push(d);
                        }
                    }
                }
                true
            }
            Tool::Lasso => {
                self.freehand(pressed && over, down, latest, mods, false);
                true
            }
            Tool::Fill if self.tools.fill_lasso => {
                self.freehand(pressed && over, down, latest, mods, true);
                true
            }
            Tool::Fill => {
                if pressed && over {
                    if let Some(p) = latest {
                        self.bucket_at(to_doc(p));
                    }
                }
                true
            }
            Tool::Move => {
                if pressed && over {
                    if let Some(p) = latest {
                        let quick = self.tools.floating.is_none();
                        if quick && !self.start_transform() {
                            return true;
                        }
                        let handle = if quick { Handle::Move } else { self.handle_at(p).unwrap_or(Handle::Move) };
                        let corners = self.tools.floating.as_ref().map(|f| f.corners).unwrap_or_default();
                        self.tools.xf = Some(XfDrag { handle, start: to_doc(p), corners, ctrl: mods.command, alt: mods.alt, quick });
                    }
                }
                if self.tools.xf.is_some() {
                    if let Some(p) = latest {
                        let d = to_doc(p);
                        self.drag_transform(d, mods.shift);
                    }
                    if !down {
                        let x = self.tools.xf.take().unwrap();
                        if x.quick {
                            let unchanged = self.tools.floating.as_ref().is_some_and(|f| f.corners == x.corners);
                            if unchanged { self.cancel_transform() } else { self.apply_transform() }
                        }
                    }
                }
                true
            }
            _ => false,
        }
    }

    /// Lasso selection / lasso fill drawn by dragging.
    fn freehand(&mut self, start: bool, down: bool, latest: Option<Pos2>, mods: Modifiers, fill: bool) {
        if start {
            if let Some(p) = latest {
                let d = self.view.screen_to_doc(p);
                let combine = if fill { Combine::New } else { combine_from(mods, self.tools.combine) };
                self.tools.gesture = Some(Gesture { start: d, pts: vec![d], combine, erase: fill && mods.alt });
            }
        }
        if let (Some(g), Some(p)) = (self.tools.gesture.as_mut(), latest) {
            let d = self.view.screen_to_doc(p);
            if g.pts.last().is_none_or(|l| (*l - d).length() * self.view.zoom >= 1.5) {
                g.pts.push(d);
            }
        }
        if !down {
            if let Some(g) = self.tools.gesture.take() {
                if g.pts.len() < 3 {
                    if !fill && g.combine == Combine::New {
                        self.deselect();
                    }
                    return;
                }
                if fill {
                    self.lasso_fill(&g.pts, g.erase);
                } else {
                    let s = Selection::polygon(&g.pts, self.doc.w, self.doc.h, self.tools.smooth);
                    self.set_selection(s, g.combine, "Lasso selection");
                }
            }
        }
    }

    fn finish_polygon(&mut self) {
        let pts = std::mem::take(&mut self.tools.poly);
        let how = self.tools.gesture.take().map(|g| g.combine).unwrap_or(self.tools.combine);
        if pts.len() >= 3 {
            let s = Selection::polygon(&pts, self.doc.w, self.doc.h, self.tools.smooth);
            self.set_selection(s, how, "Polygon selection");
        }
    }

    fn lasso_fill(&mut self, pts: &[Pos2], erase: bool) {
        let Some(li) = self.paintable_layer() else { return };
        let Some(mut d) = crate::select::raster_polygon(pts, self.doc.w, self.doc.h, true) else { return };
        if self.tools.feather >= 0.5 {
            let (w, h) = (self.doc.w, self.doc.h);
            if let Some(s) = Selection::from_dense(&d, w, h).and_then(|s| s.feathered(self.tools.feather, w, h)) {
                if let Some((x0, y0, x1, y1)) = s.bounds() {
                    d = s.dense(x0, y0, (x1 - x0) as usize, (y1 - y0) as usize);
                }
            }
        }
        let sel = self.doc.selection.clone();
        let name = self.doc.layers[li].name.clone();
        let keys = fill::paint(&mut self.doc.layers[li], &d, self.primary, self.tools.fill.opacity, sel.as_ref(), erase);
        for k in keys {
            self.doc.mark_tile_dirty(k);
        }
        self.label_hint = Some(format!("{} · {name}", if erase { "Lasso erase" } else { "Lasso fill" }));
    }

    fn bucket_at(&mut self, d: Pos2) {
        let Some(li) = self.paintable_layer() else { return };
        let img = self.reference(self.tools.fill.all_layers);
        let Some(area) = fill::bucket(&img, d.x.floor() as i32, d.y.floor() as i32, &self.tools.fill) else { return };
        let sel = self.doc.selection.clone();
        let name = self.doc.layers[li].name.clone();
        let keys = fill::paint(&mut self.doc.layers[li], &area, self.primary, self.tools.fill.opacity, sel.as_ref(), false);
        for k in keys {
            self.doc.mark_tile_dirty(k);
        }
        self.label_hint = Some(format!("Fill · {name}"));
    }

    // ---------- Keys ----------

    pub(super) fn tool_shortcuts(&mut self, ctx: &egui::Context) {
        if ctx.egui_wants_keyboard_input() {
            return;
        }
        let mut act: Vec<&'static str> = Vec::new();
        let floating = self.tools.floating.is_some();
        let polygon = !self.tools.poly.is_empty();
        ctx.input_mut(|i| {
            let mut k = |m: Modifiers, key: Key, name: &'static str| {
                if i.consume_key(m, key) {
                    act.push(name);
                }
            };
            k(Modifiers::COMMAND | Modifiers::SHIFT, Key::I, "invert");
            k(Modifiers::COMMAND, Key::A, "all");
            k(Modifiers::COMMAND, Key::D, "deselect");
            k(Modifiers::COMMAND, Key::T, "transform");
            k(Modifiers::ALT, Key::Backspace, "fill");
            k(Modifiers::NONE, Key::Q, "quickmask");
            if floating || polygon {
                k(Modifiers::NONE, Key::Enter, "enter");
                k(Modifiers::NONE, Key::Escape, "escape");
            }
            if polygon {
                k(Modifiers::NONE, Key::Backspace, "back");
            }
            if floating {
                for (key, name) in [(Key::ArrowLeft, "left"), (Key::ArrowRight, "right"), (Key::ArrowUp, "up"), (Key::ArrowDown, "down")] {
                    k(Modifiers::SHIFT, key, if name == "left" { "left10" } else if name == "right" { "right10" } else if name == "up" { "up10" } else { "down10" });
                    k(Modifiers::NONE, key, name);
                }
            } else {
                k(Modifiers::NONE, Key::Delete, "clear");
                k(Modifiers::NONE, Key::Backspace, "clear");
            }
        });
        for a in act {
            match a {
                "invert" => self.invert_selection(),
                "all" => self.select_all(),
                "deselect" => self.deselect(),
                "transform" => {
                    self.start_transform();
                }
                "fill" => self.fill_selected(),
                "quickmask" => self.toggle_quick_mask(),
                "clear" => self.clear_selected(),
                "enter" if floating => self.apply_transform(),
                "escape" if floating => self.cancel_transform(),
                "enter" => self.finish_polygon(),
                "escape" => {
                    self.tools.poly.clear();
                    self.tools.gesture = None;
                }
                "back" => {
                    self.tools.poly.pop();
                }
                nudge => {
                    let (dx, dy) = match nudge {
                        "left" => (-1.0, 0.0),
                        "right" => (1.0, 0.0),
                        "up" => (0.0, -1.0),
                        "down" => (0.0, 1.0),
                        "left10" => (-10.0, 0.0),
                        "right10" => (10.0, 0.0),
                        "up10" => (0.0, -10.0),
                        _ => (0.0, 10.0),
                    };
                    if let Some(f) = self.tools.floating.as_mut() {
                        for c in &mut f.corners {
                            *c += vec2(dx, dy);
                        }
                    }
                }
            }
        }
    }

    // ---------- Drawing on the canvas ----------

    pub(super) fn tool_overlay(&mut self, ui: &Ui, p: &egui::Painter, rect: Rect) {
        let ctx = ui.ctx().clone();
        let now = ctx.input(|i| i.time);
        let to_s = |d: Pos2| self.view.doc_to_screen(d);
        let dark = Stroke::new(1.0, Color32::from_black_alpha(200));
        let light = Stroke::new(1.0, Color32::WHITE);
        let march = if self.set.reduced_motion { 0.0 } else { (now * 12.0 % 8.0) as f32 };

        // the project's guide lines
        if self.set.show_guides {
            let (dw, dh) = (self.doc.w as f32, self.doc.h as f32);
            for g in &self.doc.meta.guides {
                let (a, b) = if g.vertical { (pos2(g.pos, 0.0), pos2(g.pos, dh)) } else { (pos2(0.0, g.pos), pos2(dw, g.pos)) };
                let line = [to_s(a), to_s(b)];
                p.line_segment(line, Stroke::new(2.5, Color32::from_black_alpha(70)));
                p.line_segment(line, Stroke::new(1.5, Color32::from_rgb(70, 200, 235)));
            }
        }

        // symmetry axes and center
        if self.tools.sym.on() {
            let reach = (self.doc.w as f32).hypot(self.doc.h as f32) * 1.5;
            for [a, b] in self.tools.sym.axis_lines(reach) {
                let line = [to_s(a), to_s(b)];
                p.line_segment(line, Stroke::new(3.0, Color32::from_black_alpha(60)));
                p.line_segment(line, Stroke::new(1.2, VENOM.gamma_multiply(0.85)));
            }
            let c = to_s(pos2(self.tools.sym.cx, self.tools.sym.cy));
            p.circle_filled(c, 6.0, Color32::from_black_alpha(140));
            p.circle_stroke(c, 6.0, Stroke::new(1.5, VENOM));
            p.circle_filled(c, 2.0, VENOM);
        }

        // marching ants
        let qm = self.quick_mask_idx().is_some();
        if let (Some(sel), false, false) = (&self.doc.selection, self.tools.floating.is_some(), qm) {
            if self.tools.ants.0 != sel.id {
                self.tools.ants = (sel.id, sel.outline());
            }
            let mut shapes = Vec::new();
            for line in &self.tools.ants.1 {
                let pts: Vec<Pos2> = line.iter().map(|d| to_s(*d)).collect();
                shapes.push(Shape::line(pts.clone(), dark));
                Shape::dashed_line_many_with_offset(&pts, light, &[4.0], &[4.0], march, &mut shapes);
            }
            p.extend(shapes);
            if !self.set.reduced_motion {
                ctx.request_repaint_after_secs(0.08);
            }
        }

        // the shape being dragged out
        if let Some(g) = &self.tools.gesture {
            let pts: Vec<Pos2> = match self.tool {
                Tool::Select if g.pts.len() >= 2 => {
                    let (a, b) = (g.start, *g.pts.last().unwrap());
                    if self.tools.sel_shape == SelShape::Ellipse { ellipse(a, b) } else { rect_pts(a, b) }
                }
                Tool::Lasso | Tool::Fill if !self.tools.lasso_poly || self.tool == Tool::Fill => g.pts.clone(),
                _ => Vec::new(),
            };
            if pts.len() >= 2 {
                let s: Vec<Pos2> = pts.iter().map(|d| to_s(*d)).collect();
                if self.tool == Tool::Fill {
                    let col = if g.erase { Color32::from_white_alpha(70) } else { self.primary.gamma_multiply(0.45) };
                    p.add(Shape::line(s.iter().copied().chain(std::iter::once(s[0])).collect(), Stroke::new(1.5, col)));
                }
                let mut shapes = vec![Shape::closed_line(s.clone(), dark)];
                let mut closed = s.clone();
                closed.push(s[0]);
                Shape::dashed_line_many_with_offset(&closed, light, &[4.0], &[4.0], march, &mut shapes);
                p.extend(shapes);
            }
        }
        if !self.tools.poly.is_empty() {
            let mut s: Vec<Pos2> = self.tools.poly.iter().map(|d| to_s(*d)).collect();
            if let Some(h) = ctx.input(|i| i.pointer.hover_pos()) {
                s.push(h);
            }
            p.add(Shape::line(s.clone(), Stroke::new(2.5, Color32::from_black_alpha(150))));
            p.add(Shape::line(s.clone(), Stroke::new(1.0, Color32::WHITE)));
            for (i, q) in self.tools.poly.iter().enumerate() {
                let c = to_s(*q);
                p.circle_filled(c, if i == 0 { 5.0 } else { 3.0 }, if i == 0 { VENOM } else { Color32::WHITE });
            }
        }

        // the floating picture and its handles
        if let Some(f) = &self.tools.floating {
            if self.tools.tex.is_none() {
                self.tools.tex = Some(ctx.load_texture("floating", f.preview(2048), TextureOptions::LINEAR));
            }
            let tex = self.tools.tex.as_ref().unwrap();
            let hm = Homography::square_to_quad(&f.corners);
            let n = 16;
            let mut mesh = Mesh::with_texture(tex.id());
            for j in 0..=n {
                for i in 0..=n {
                    let (u, v) = (i as f32 / n as f32, j as f32 / n as f32);
                    mesh.vertices.push(egui::epaint::Vertex { pos: to_s(hm.map(u, v)), uv: pos2(u, v), color: Color32::WHITE });
                }
            }
            for j in 0..n {
                for i in 0..n {
                    let a = (j * (n + 1) + i) as u32;
                    let b = a + 1;
                    let c = a + (n + 1) as u32;
                    mesh.add_triangle(a, b, c);
                    mesh.add_triangle(b, c + 1, c);
                }
            }
            p.add(Shape::mesh(mesh));
            let q: Vec<Pos2> = f.corners.iter().map(|c| to_s(*c)).collect();
            p.add(Shape::closed_line(q.clone(), Stroke::new(2.5, Color32::from_black_alpha(140))));
            p.add(Shape::closed_line(q, Stroke::new(1.2, VENOM)));
            if let Some((c, e)) = self.handles() {
                for h in c.iter().chain(e.iter()) {
                    let r = Rect::from_center_size(*h, Vec2::splat(9.0));
                    p.rect_filled(r, CornerRadius::same(2), Color32::WHITE);
                    p.rect_stroke(r, CornerRadius::same(2), Stroke::new(1.2, EMERALD_DEEP), StrokeKind::Outside);
                }
            }
            // cursor that says what a drag here will do
            if let Some(hp) = ctx.input(|i| i.pointer.hover_pos()).filter(|h| rect.contains(*h)) {
                if self.tool == Tool::Move {
                    let icon = match self.tools.xf.as_ref().map(|x| x.handle).or_else(|| self.handle_at(hp)) {
                        Some(Handle::Corner(i)) => if i % 2 == 0 { CursorIcon::ResizeNwSe } else { CursorIcon::ResizeNeSw },
                        Some(Handle::Edge(i)) => if i % 2 == 0 { CursorIcon::ResizeVertical } else { CursorIcon::ResizeHorizontal },
                        Some(Handle::Rotate) => CursorIcon::Alias,
                        _ => CursorIcon::Move,
                    };
                    ctx.set_cursor_icon(icon);
                }
            }
        }
    }

    // ---------- Panels ----------

    /// Left panel for the selection, fill and move tools.
    pub(super) fn tool_options(&mut self, ui: &mut Ui) {
        section_title(ui, self.tool.name());
        ui.add_space(4.0);
        let hint = |ui: &mut Ui, t: &str| {
            ui.label(RichText::new(t).size(11.5).color(TEXT_DIM));
        };
        let has_sel = self.doc.selection.is_some();
        match self.tool {
            Tool::Select | Tool::Lasso => {
                if self.tool == Tool::Select {
                    segmented(ui, &mut self.tools.sel_shape, &[(SelShape::Rect, "Rectangle"), (SelShape::Ellipse, "Ellipse"), (SelShape::Wand, "Magic wand")]);
                } else {
                    let mut poly = self.tools.lasso_poly;
                    segmented(ui, &mut poly, &[(false, "Freehand"), (true, "Polygon")]);
                    if poly != self.tools.lasso_poly {
                        self.tools.lasso_poly = poly;
                        self.tools.poly.clear();
                    }
                }
                ui.add_space(4.0);
                hint(
                    ui,
                    match (self.tool, self.tools.sel_shape, self.tools.lasso_poly) {
                        (Tool::Select, SelShape::Wand, _) => "Click an area: everything around it with a similar color gets selected.",
                        (Tool::Select, _, _) => "Drag to select. A click without dragging deselects.",
                        (_, _, true) => "Click the corners. Click the first point, double-click or press Enter to close. Backspace removes the last point, Esc cancels.",
                        _ => "Draw around what you want to select.",
                    },
                );
                ui.add_space(6.0);
                ui.label(RichText::new("When selecting").color(TEXT).size(12.5));
                segmented(ui, &mut self.tools.combine, &Combine::ALL);
                hint(ui, "Shift: add · Alt: subtract · Shift+Alt: intersect");
                ui.add_space(4.0);
                ui.horizontal(|ui| {
                    ui.label("Soft edge");
                    ui.add(egui::Slider::new(&mut self.tools.feather, 0.0..=100.0).suffix(" px").max_decimals(0));
                });
                ui.checkbox(&mut self.tools.smooth, "Smooth edges (anti-aliasing)");
                if self.tool == Tool::Select && self.tools.sel_shape == SelShape::Wand {
                    ui.add_space(4.0);
                    ui.horizontal(|ui| {
                        ui.label("Tolerance");
                        ui.add(egui::Slider::new(&mut self.tools.wand_tol, 0..=255));
                    });
                    ui.checkbox(&mut self.tools.wand_contiguous, "Only the connected area");
                    let mut all = self.tools.wand_all;
                    segmented(ui, &mut all, &[(true, "Look at all layers"), (false, "This layer only")]);
                    self.tools.wand_all = all;
                }
            }
            Tool::Fill => {
                let mut lasso = self.tools.fill_lasso;
                segmented(ui, &mut lasso, &[(false, "Bucket"), (true, "Lasso fill")]);
                self.tools.fill_lasso = lasso;
                ui.add_space(4.0);
                if self.tools.fill_lasso {
                    hint(ui, "Draw a shape: it fills with the main color. Hold Alt to erase the shape instead.");
                    ui.horizontal(|ui| {
                        ui.label("Soft edge");
                        ui.add(egui::Slider::new(&mut self.tools.feather, 0.0..=100.0).suffix(" px").max_decimals(0));
                    });
                } else {
                    hint(ui, "Click an area to fill it with the main color. Gaps in the lines are closed so the color stays inside.");
                    let f = &mut self.tools.fill;
                    let mut all = f.all_layers;
                    segmented(ui, &mut all, &[(true, "Look at all layers"), (false, "This layer only")]);
                    f.all_layers = all;
                    ui.horizontal(|ui| {
                        ui.label("Tolerance");
                        ui.add(egui::Slider::new(&mut f.tolerance, 0..=255));
                    });
                    ui.horizontal(|ui| {
                        ui.label("Close gaps");
                        ui.add(egui::Slider::new(&mut f.gap, 0.0..=20.0).suffix(" px").max_decimals(0));
                    });
                    ui.horizontal(|ui| {
                        ui.label("Under the lines");
                        ui.add(egui::Slider::new(&mut f.expand, 0.0..=10.0).suffix(" px").max_decimals(0));
                    });
                }
                ui.horizontal(|ui| {
                    ui.label("Opacity");
                    let mut pct = self.tools.fill.opacity * 100.0;
                    ui.add(egui::Slider::new(&mut pct, 1.0..=100.0).suffix("%").max_decimals(0));
                    self.tools.fill.opacity = pct / 100.0;
                });
                if has_sel {
                    hint(ui, "Only the selected area gets color.");
                }
            }
            Tool::Move => {
                if self.tools.floating.is_none() {
                    hint(ui, "Drag on the canvas to move the layer, or just the selected part of it. Arrow keys nudge after Ctrl+T.");
                    ui.add_space(6.0);
                    if ui.button("Transform  (Ctrl+T)").clicked() {
                        self.start_transform();
                    }
                } else {
                    hint(
                        ui,
                        "Drag inside to move, outside to turn (Shift snaps). Corners scale (Shift keeps the shape, Alt from the middle). \
                         Ctrl + corner: perspective. Ctrl + side: skew. Arrow keys nudge.",
                    );
                    ui.add_space(6.0);
                    self.transform_buttons(ui, false);
                }
            }
            _ => {}
        }
        ui.add_space(12.0);
        section_title(ui, "Selection");
        self.selection_buttons(ui, false);
    }

    fn transform_buttons(&mut self, ui: &mut Ui, compact: bool) {
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing = vec2(4.0, 4.0);
            if ui.button(RichText::new("Apply").strong()).on_hover_text("Enter").clicked() {
                self.apply_transform();
            }
            if ui.button("Cancel").on_hover_text("Esc").clicked() {
                self.cancel_transform();
            }
            if let Some(f) = self.tools.floating.as_mut() {
                if ui.button("Mirror sideways").clicked() {
                    f.flip_h();
                }
                if ui.button("Mirror up / down").clicked() {
                    f.flip_v();
                }
                if compact {
                    return;
                }
                if ui.button("Turn left").clicked() {
                    f.rotate(-std::f32::consts::FRAC_PI_2);
                }
                if ui.button("Turn right").clicked() {
                    f.rotate(std::f32::consts::FRAC_PI_2);
                }
                if ui.button("Reset").on_hover_text("Back to where it started").clicked() {
                    let (x0, y0, x1, y1) = (f.x as f32, f.y as f32, (f.x + f.w as i32) as f32, (f.y + f.h as i32) as f32);
                    f.corners = [pos2(x0, y0), pos2(x1, y0), pos2(x1, y1), pos2(x0, y1)];
                }
            }
        });
    }

    fn selection_buttons(&mut self, ui: &mut Ui, compact: bool) {
        let has = self.doc.selection.is_some();
        let qm = self.quick_mask_idx().is_some();
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing = vec2(4.0, 4.0);
            if !compact && ui.button("All").on_hover_text("Select everything (Ctrl+A)").clicked() {
                self.select_all();
            }
            if has && ui.button("Deselect").on_hover_text("Ctrl+D").clicked() {
                self.deselect();
            }
            if (has || !compact) && ui.button("Invert").on_hover_text("Ctrl+Shift+I").clicked() {
                self.invert_selection();
            }
            if has {
                let px = self.tools.edit_px;
                if ui.button("Grow").on_hover_text(format!("Bigger by {px:.0} px")).clicked() {
                    self.grow_selection(px);
                }
                if ui.button("Shrink").on_hover_text(format!("Smaller by {px:.0} px")).clicked() {
                    self.grow_selection(-px);
                }
                if ui.button("Feather").on_hover_text(format!("Soft edge of {px:.0} px")).clicked() {
                    self.feather_selection(px);
                }
            }
            if ui.button("Fill").on_hover_text("Fill with the main color (Alt+Backspace)").clicked() {
                self.fill_selected();
            }
            if ui.button("Clear").on_hover_text("Erase what is selected (Delete)").clicked() {
                self.clear_selected();
            }
            if ui.button("Transform").on_hover_text("Move, scale, turn, flip, distort (Ctrl+T)").clicked() {
                self.start_transform();
            }
            let qm_label = if qm { "Quick mask off" } else { "Quick mask" };
            if ui
                .add(egui::Button::new(qm_label).selected(qm))
                .on_hover_text("Q: paint the selection with any brush (red = selected), Q again to finish")
                .clicked()
            {
                self.toggle_quick_mask();
            }
        });
        if !compact && has {
            ui.horizontal(|ui| {
                ui.label(RichText::new("Grow / shrink / feather by").size(12.0));
                ui.add(egui::DragValue::new(&mut self.tools.edit_px).range(1.0..=200.0).suffix(" px").max_decimals(0));
            });
        }
    }

    /// Floating bar above the view bar while something is selected or being transformed.
    pub(super) fn selection_bar(&mut self, ui: &mut Ui, rect: Rect) {
        let show = self.doc.selection.is_some() || self.tools.floating.is_some() || self.quick_mask_idx().is_some();
        if !show || self.stroke.is_some() {
            return;
        }
        let bar = Rect::from_center_size(pos2(rect.center().x, rect.bottom() - 74.0), vec2(rect.width().min(640.0) - 16.0, 34.0));
        ui.scope_builder(UiBuilder::new().max_rect(bar).layout(Layout::left_to_right(Align::Center)), |ui| {
            egui::Frame::new()
                .fill(PANEL.gamma_multiply(0.96))
                .stroke(Stroke::new(1.0, LINE))
                .corner_radius(CornerRadius::same(17))
                .inner_margin(Margin::symmetric(10, 3))
                .show(ui, |ui| {
                    ui.horizontal_centered(|ui| {
                        if self.tools.floating.is_some() {
                            ui.label(RichText::new("Transform").color(VENOM).size(12.5));
                            self.transform_buttons(ui, true);
                        } else {
                            self.selection_buttons(ui, true);
                        }
                    });
                });
        });
    }

    /// The symmetry button in the view bar and its little window.
    pub(super) fn symmetry_button(&mut self, ui: &mut Ui) {
        let on = self.tools.sym.on();
        let tip = format!("Symmetry: {}", self.tools.sym.mode.label());
        let r = icon_button(ui, Icon::Symmetry, on || self.tools.sym_open, 28.0, &tip);
        if r.clicked() {
            self.tools.sym_open = !self.tools.sym_open;
        }
        if !self.tools.sym_open {
            return;
        }
        let (w, h) = (self.doc.w as f32, self.doc.h as f32);
        let mut open = true;
        egui::Window::new(RichText::new("Symmetry").strong())
            .open(&mut open)
            .collapsible(false)
            .resizable(false)
            .pivot(Align2::CENTER_BOTTOM)
            .default_pos(r.rect.center_top() - vec2(0.0, 12.0))
            .show(ui.ctx(), |ui| {
                ui.set_width(300.0);
                let s = &mut self.tools.sym;
                if s.cx == 0.0 && s.cy == 0.0 {
                    s.cx = (w / 2.0).round();
                    s.cy = (h / 2.0).round();
                }
                ui.horizontal_wrapped(|ui| {
                    ui.spacing_mut().item_spacing = vec2(4.0, 4.0);
                    for (m, label, about) in SymMode::ALL {
                        if widgets::chip(ui, label, s.mode == m).on_hover_text(about).clicked() {
                            s.mode = m;
                        }
                    }
                });
                if let Some((_, _, about)) = SymMode::ALL.iter().find(|m| m.0 == s.mode) {
                    ui.label(RichText::new(*about).size(11.5).color(TEXT_DIM));
                }
                if matches!(s.mode, SymMode::Radial | SymMode::Kaleido) {
                    ui.horizontal(|ui| {
                        ui.label("Slices");
                        ui.add(egui::Slider::new(&mut s.count, 2..=32));
                    });
                }
                if s.on() {
                    ui.horizontal(|ui| {
                        ui.label("Turn axes");
                        ui.add(egui::Slider::new(&mut s.angle, -180.0..=180.0).suffix("°").max_decimals(0));
                    });
                    ui.horizontal(|ui| {
                        if ui.button("Center of the canvas").clicked() {
                            s.cx = (w / 2.0).round();
                            s.cy = (h / 2.0).round();
                            s.angle = 0.0;
                        }
                        ui.label(RichText::new(format!("{:.0}, {:.0}", s.cx, s.cy)).size(11.0).color(TEXT_DIM));
                    });
                    ui.label(RichText::new("Drag the green dot on the canvas to move the center.").size(11.5).color(TEXT_DIM));
                }
            });
        self.tools.sym_open = open;
    }

    /// Small help text over the canvas while drawing a polygon or when something can't be done.
    pub(super) fn tool_notice(&self) -> Option<&str> {
        self.tools.notice.as_deref()
    }
}
