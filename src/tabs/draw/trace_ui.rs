// SPDX-License-Identifier: GPL-3.0-or-later
// Draw tab: the Auto-trace window. Pick a preset, adjust, see the result live on a small copy,
// then trace the layer into a new vector layer (the original pixel layer is kept, hidden if you like).

use crate::widgets::HelpExt;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::mpsc::{Receiver, TryRecvError, channel};
use std::sync::Arc;

use eframe::egui::{self, Align2, Color32, ColorImage, RichText, TextureHandle, TextureOptions, vec2};

use super::DrawState;
use crate::doc::{LayerId, LayerKind};
use crate::theme::*;
use crate::trace::{self, LineColor, PRESETS, TraceMode, TraceOptions, TraceResult};
use crate::vector;
use crate::widgets::{self, section_title};

#[derive(Clone, Copy, PartialEq, Eq)]
enum View {
    Result,
    Both,
    Original,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Quality {
    Full,
    Faster,
}

struct Small {
    img: Arc<image::RgbaImage>,
    scale: f32,
}

struct Final {
    rx: Receiver<TraceResult>,
    progress: Arc<AtomicU32>,
    scale: f32,
}

pub struct TraceUi {
    pub open: bool,
    layer: LayerId,
    name: String,
    /// The layer's pixels (unmultiplied), cropped to its drawing plus a small margin.
    src: Option<Arc<image::RgbaImage>>,
    origin: (i32, i32),
    opts: TraceOptions,
    preset: usize,
    view: View,
    quality: Quality,
    hide_original: bool,
    small: Option<Small>,
    last_opts: Option<TraceOptions>,
    due: f64,
    job: Option<Receiver<TraceResult>>,
    result: Option<Arc<TraceResult>>,
    preview: Option<TextureHandle>,
    stats: String,
    redraw: bool,
    fin: Option<Final>,
}

impl Default for TraceUi {
    fn default() -> Self {
        Self {
            open: false,
            layer: 0,
            name: String::new(),
            src: None,
            origin: (0, 0),
            opts: TraceOptions::default(),
            preset: 0,
            view: View::Result,
            quality: Quality::Full,
            hide_original: true,
            small: None,
            last_opts: None,
            due: 0.0,
            job: None,
            result: None,
            preview: None,
            stats: String::new(),
            redraw: false,
            fin: None,
        }
    }
}

/// The traced lines and shapes as vector layer data, mapped back to canvas coordinates.
pub fn to_vecdata(r: &TraceResult, scale: f32, ox: f32, oy: f32) -> vector::VecData {
    let k = 1.0 / scale;
    let map = |x: f32, y: f32| [x * k + ox, y * k + oy];
    let strokes = r
        .lines
        .iter()
        .map(|l| vector::VecStroke {
            pts: l.pts.iter().map(|p| {
                let q = map(p[0], p[1]);
                [(q[0] * 100.0).round() / 100.0, (q[1] * 100.0).round() / 100.0, p[2]]
            }).collect(),
            style: vector::LineStyle { color: l.color, size: l.size * k, min_size: 0.0, size_pressure: l.pressure, ends: vector::LineEnds::Round, taper: 0.3, opacity: 1.0 },
        })
        .collect();
    let fills = r
        .fills
        .iter()
        .map(|f| vector::VecFill {
            color: f.color,
            rings: f.rings.iter().map(|ring| ring.iter().map(|p| {
                let q = map(p[0], p[1]);
                [(q[0] * 100.0).round() / 100.0, (q[1] * 100.0).round() / 100.0]
            }).collect()).collect(),
        })
        .collect();
    vector::VecData { strokes, fills, width: 1.0 }
}

/// A traced result drawn as a picture (for the preview).
fn render(r: &TraceResult, w: u32, h: u32) -> image::RgbaImage {
    let mut doc = crate::doc::Document::new(w, h);
    let mut layer = doc.new_layer("trace", LayerKind::Vector);
    layer.vector = Some(Arc::new(to_vecdata(r, 1.0, 0.0, 0.0)));
    vector::render_all(&mut layer, w, h);
    crate::files::layer_rgba(&layer, 0, 0, w, h)
}

fn over_white(img: &image::RgbaImage, strength: f32, canvas: &mut [[f32; 3]]) {
    for (p, c) in img.pixels().zip(canvas.iter_mut()) {
        let a = p.0[3] as f32 / 255.0 * strength;
        for k in 0..3 {
            c[k] = c[k] * (1.0 - a) + p.0[k] as f32 * a;
        }
    }
}

fn compose(small: &image::RgbaImage, traced: Option<&image::RgbaImage>, view: View) -> ColorImage {
    let (w, h) = small.dimensions();
    let mut canvas = vec![[255.0f32; 3]; (w * h) as usize];
    if view != View::Result {
        over_white(small, if view == View::Both { 0.35 } else { 1.0 }, &mut canvas);
    }
    if view != View::Original {
        if let Some(t) = traced {
            over_white(t, 1.0, &mut canvas);
        }
    }
    let px = canvas.iter().map(|c| Color32::from_rgb(c[0] as u8, c[1] as u8, c[2] as u8)).collect();
    ColorImage::new([w as usize, h as usize], px)
}

fn scaled(src: &image::RgbaImage, scale: f32) -> image::RgbaImage {
    if scale >= 0.999 {
        return src.clone();
    }
    let (w, h) = (((src.width() as f32 * scale).round() as u32).max(1), ((src.height() as f32 * scale).round() as u32).max(1));
    image::imageops::resize(src, w, h, image::imageops::FilterType::Triangle)
}

fn spawn(img: Arc<image::RgbaImage>, opts: TraceOptions, progress: Option<Arc<AtomicU32>>) -> Receiver<TraceResult> {
    let (tx, rx) = channel();
    std::thread::spawn(move || {
        let (w, h) = (img.width() as usize, img.height() as usize);
        let report = move |p: f32| {
            if let Some(a) = &progress {
                a.store(p.to_bits(), Ordering::Relaxed);
            }
        };
        let _ = tx.send(trace::trace(img.as_raw(), w, h, &opts, &report));
    });
    rx
}

impl DrawState {
    /// Open the Auto-trace window for a pixel layer.
    pub(super) fn open_trace(&mut self, id: LayerId) {
        let Some(li) = self.doc.index_of(id) else { return };
        let layer = &self.doc.layers[li];
        if layer.kind != LayerKind::Raster {
            self.notice = Some("Auto-trace works on pixel layers.");
            return;
        }
        let (dw, dh) = (self.doc.w, self.doc.h);
        let Some(mut b) = crate::files::layer_bounds(layer, dw, dh) else {
            self.notice = Some("This layer is empty: nothing to trace.");
            return;
        };
        // a selection limits what is traced
        let sel = self.doc.selection.clone();
        if let Some(sb) = sel.as_ref().and_then(|s| s.bounds()) {
            b = (b.0.max(sb.0), b.1.max(sb.1), b.2.min(sb.2), b.3.min(sb.3));
            if b.0 >= b.2 || b.1 >= b.3 {
                self.notice = Some("Nothing of this layer is inside the selection.");
                return;
            }
        }
        let m = 4; // empty margin, so lines touching the edge keep their end
        let (x0, y0) = (b.0 - m, b.1 - m);
        let (w, h) = ((b.2 - b.0 + 2 * m) as u32, (b.3 - b.1 + 2 * m) as u32);
        if w as u64 * h as u64 > 120_000_000 {
            self.notice = Some("This is too big to trace in one go. Select a part of it first.");
            return;
        }
        let mut img = crate::files::layer_rgba(layer, x0, y0, w, h);
        if let Some(s) = &sel {
            for (x, y, p) in img.enumerate_pixels_mut() {
                if s.at(x0 + x as i32, y0 + y as i32) < 128 {
                    p.0[3] = 0;
                }
            }
        }
        let t = &mut self.trace;
        t.open = true;
        t.layer = id;
        t.name = layer.name.clone();
        t.src = Some(Arc::new(img));
        t.origin = (x0, y0);
        t.small = None;
        t.result = None;
        t.preview = None;
        t.job = None;
        t.fin = None;
        t.last_opts = None;
        t.stats.clear();
    }

    pub(super) fn trace_window(&mut self, ctx: &egui::Context) {
        if !self.trace.open {
            return;
        }
        let now = ctx.input(|i| i.time);
        let mut open = true;
        let mut start_final = false;
        let mut close = false;
        egui::Window::new(RichText::new(format!("Auto-trace · {}", self.trace.name)).strong())
            .open(&mut open)
            .collapsible(false)
            .resizable(false)
            .pivot(Align2::CENTER_CENTER)
            .default_pos(ctx.content_rect().center())
            .show(ctx, |ui| {
                let busy = self.trace.fin.is_some();
                ui.horizontal_top(|ui| {
                    ui.vertical(|ui| {
                        ui.set_width(330.0);
                        let t = &mut self.trace;
                        ui.add_enabled_ui(!busy, |ui| {
                            ui.horizontal_wrapped(|ui| {
                                ui.spacing_mut().item_spacing = vec2(4.0, 4.0);
                                for (i, p) in PRESETS.iter().enumerate() {
                                    if widgets::chip(ui, p.name, t.preset == i).help(p.about).clicked() {
                                        t.preset = i;
                                        t.opts = (p.opts)(&t.opts);
                                    }
                                }
                            });
                            ui.label(RichText::new(PRESETS[t.preset].about).size(11.5).color(TEXT_DIM));
                            ui.add_space(4.0);
                            section_title(ui, "Trace");
                            let o = &mut t.opts;
                            ui.horizontal(|ui| {
                                ui.spacing_mut().item_spacing.x = 4.0;
                                for (m, label) in [(TraceMode::Lines, "Lines"), (TraceMode::Colors, "Colors"), (TraceMode::Both, "Both"), (TraceMode::Silhouette, "Silhouette")] {
                                    if widgets::chip(ui, label, o.mode == m).help("Pick how it works.").clicked() {
                                        o.mode = m;
                                    }
                                }
                            });
                            let lines = matches!(o.mode, TraceMode::Lines | TraceMode::Both);
                            let colors = matches!(o.mode, TraceMode::Colors | TraceMode::Both);
                            let pct = |ui: &mut egui::Ui, label: &str, tip: &str, v: &mut f32, lo: f32, hi: f32| {
                                ui.horizontal(|ui| {
                                    ui.label(RichText::new(label).size(12.5).color(TEXT_DIM)).help(tip);
                                    let mut p = *v * 100.0;
                                    if ui.add(egui::Slider::new(&mut p, lo * 100.0..=hi * 100.0).suffix("%").max_decimals(0)).changed() {
                                        *v = p / 100.0;
                                    }
                                });
                            };
                            if o.mode != TraceMode::Colors {
                                let label = if o.mode == TraceMode::Silhouette { "Solidness" } else { "Line darkness" };
                                pct(ui, label, "How dark (or solid) something must be to count as a line", &mut o.threshold, 0.05, 0.95);
                            }
                            if lines {
                                pct(ui, "Smoothness", "0 follows every wobble, 100 gives smooth curves", &mut o.smooth, 0.0, 1.0);
                                ui.horizontal(|ui| {
                                    ui.label(RichText::new("Ignore specks").size(12.5).color(TEXT_DIM)).help("Dots and specks smaller than this are ignored");
                                    ui.add(egui::Slider::new(&mut o.detail, 0..=300).suffix(" px"));
                                });
                                ui.horizontal(|ui| {
                                    ui.label(RichText::new("Close gaps").size(12.5).color(TEXT_DIM)).help("Closes small gaps in the lines");
                                    ui.add(egui::Slider::new(&mut o.close_gaps, 0..=6).suffix(" px"));
                                });
                                ui.horizontal(|ui| {
                                    ui.label(RichText::new("Shortest line").size(12.5).color(TEXT_DIM)).help("Shorter lines are left out");
                                    ui.add(egui::Slider::new(&mut o.min_length, 0.0..=100.0).suffix(" px").max_decimals(0));
                                });
                                ui.checkbox(&mut o.keep_width, "Keep thick and thin parts (as pen pressure)").help("The traced line is thick and thin as a pen stroke.");
                            }
                            if o.mode != TraceMode::Colors {
                                ui.horizontal(|ui| {
                                    ui.label(RichText::new(if o.mode == TraceMode::Silhouette { "Color" } else { "Line color" }).size(12.5).color(TEXT_DIM));
                                    if o.mode != TraceMode::Silhouette {
                                        if widgets::chip(ui, "As drawn", o.line_color == LineColor::Original).help("Draw it as it is, without any change.").clicked() {
                                            o.line_color = LineColor::Original;
                                        }
                                        if widgets::chip(ui, "One color", o.line_color == LineColor::Single).help("Use one color for the whole thing.").clicked() {
                                            o.line_color = LineColor::Single;
                                        }
                                    }
                                    if o.line_color == LineColor::Single || o.mode == TraceMode::Silhouette {
                                        egui::color_picker::color_edit_button_srgb(ui, &mut o.single_color);
                                    }
                                });
                            }
                            if colors {
                                ui.horizontal(|ui| {
                                    ui.label(RichText::new("Colors").size(12.5).color(TEXT_DIM)).help("How many different colors the shapes use");
                                    ui.add(egui::Slider::new(&mut o.colors, 2..=24));
                                });
                                ui.horizontal(|ui| {
                                    ui.label(RichText::new("Merge spots").size(12.5).color(TEXT_DIM)).help("Color spots smaller than this join the color around them");
                                    ui.add(egui::Slider::new(&mut o.min_area, 0..=500).suffix(" px"));
                                });
                                pct(ui, "Edge smooth", "0 = pixel-exact edges, 100 = round smooth edges", &mut o.color_smooth, 0.0, 1.0);
                                ui.checkbox(&mut o.ignore_background, "Leave out white paper / background").help("The white paper does not count as drawing.");
                            }
                        });
                    });
                    ui.vertical(|ui| {
                        let t = &mut self.trace;
                        ui.horizontal(|ui| {
                            ui.spacing_mut().item_spacing.x = 4.0;
                            for (v, label) in [(View::Result, "Result"), (View::Both, "Over original"), (View::Original, "Original")] {
                                if widgets::chip(ui, label, t.view == v).help("Pick how the preview looks.").clicked() && t.view != v {
                                    t.view = v;
                                    t.redraw = true;
                                }
                            }
                        });
                        let size = vec2(520.0, 460.0);
                        match &t.preview {
                            Some(tex) => {
                                let s = tex.size_vec2();
                                let k = (size.x / s.x).min(size.y / s.y);
                                ui.add(egui::Image::new((tex.id(), s * k)));
                            }
                            None => {
                                ui.allocate_space(size);
                            }
                        }
                        ui.label(RichText::new(format!("{} · the preview is a small copy; the real trace uses full detail", if t.stats.is_empty() { "Working…" } else { &t.stats })).size(11.0).color(TEXT_DIM));
                    });
                });
                ui.separator();
                ui.horizontal(|ui| {
                    let t = &mut self.trace;
                    widgets::segmented(ui, &mut t.quality, &[(Quality::Full, "Full detail"), (Quality::Faster, "Faster")]);
                    ui.checkbox(&mut t.hide_original, "Hide the original layer").help("Hide the layer that this was made from.");
                    ui.add_space(8.0);
                    if let Some(f) = &t.fin {
                        ui.add(egui::ProgressBar::new(f32::from_bits(f.progress.load(Ordering::Relaxed))).desired_width(160.0).animate(true));
                    }
                    if ui.add_enabled(!busy, egui::Button::new("Cancel")).clicked() {
                        close = true;
                    }
                    if ui.add_enabled(!busy, egui::Button::new(RichText::new("Trace to vector layer").strong())).clicked() {
                        start_final = true;
                    }
                });
            });
        if !open && self.trace.fin.is_none() {
            close = true;
        }

        // settings changed: trace the small copy again a moment later
        if self.trace.last_opts.as_ref() != Some(&self.trace.opts) {
            self.trace.last_opts = Some(self.trace.opts.clone());
            self.trace.due = now + 0.25;
            ctx.request_repaint_after_secs(0.3);
        }
        let t = &mut self.trace;
        if t.due > 0.0 && now >= t.due && t.job.is_none() {
            t.due = 0.0;
            if let Some(src) = t.src.clone() {
                if t.small.is_none() {
                    let scale = (520.0 / src.width().max(src.height()) as f32).min(1.0);
                    t.small = Some(Small { img: Arc::new(scaled(&src, scale)), scale });
                }
                let sm = t.small.as_ref().unwrap();
                t.job = Some(spawn(sm.img.clone(), t.opts.scaled(sm.scale), None));
            }
        } else if t.due > 0.0 {
            ctx.request_repaint_after_secs(0.1);
        }
        if let Some(rx) = &t.job {
            match rx.try_recv() {
                Ok(r) => {
                    t.stats = format!("{} lines · {} color shapes", r.lines.len(), r.fills.len());
                    t.result = Some(Arc::new(r));
                    t.job = None;
                    t.redraw = true;
                }
                Err(TryRecvError::Empty) => ctx.request_repaint_after_secs(0.1),
                Err(TryRecvError::Disconnected) => {
                    t.job = None;
                    t.stats = "The preview failed.".into();
                }
            }
        }
        if t.redraw {
            t.redraw = false;
            if let Some(sm) = &t.small {
                let traced = t.result.as_ref().map(|r| render(r, sm.img.width(), sm.img.height()));
                let img = compose(&sm.img, traced.as_ref(), t.view);
                match &mut t.preview {
                    Some(tex) => tex.set(img, TextureOptions::LINEAR),
                    None => t.preview = Some(ctx.load_texture("trace_preview", img, TextureOptions::LINEAR)),
                }
            }
        }

        if start_final {
            if let Some(src) = t.src.clone() {
                let scale = match t.quality {
                    Quality::Full => (4000.0 / src.width().max(src.height()) as f32).min(1.0),
                    Quality::Faster => (2000.0 / src.width().max(src.height()) as f32).min(0.5),
                };
                let progress = Arc::new(AtomicU32::new(0));
                let img = Arc::new(scaled(&src, scale));
                let rx = spawn(img, t.opts.scaled(scale), Some(progress.clone()));
                t.fin = Some(Final { rx, progress, scale });
            }
        }
        if let Some(f) = &t.fin {
            match f.rx.try_recv() {
                Ok(r) => {
                    let (scale, ox, oy) = (f.scale, t.origin.0 as f32, t.origin.1 as f32);
                    let (id, name, hide) = (t.layer, t.name.clone(), t.hide_original);
                    t.fin = None;
                    self.finish_trace(id, &name, hide, &r, scale, ox, oy);
                    self.trace.open = false;
                    return;
                }
                Err(TryRecvError::Empty) => ctx.request_repaint_after_secs(0.1),
                Err(TryRecvError::Disconnected) => {
                    t.fin = None;
                    t.stats = "The trace failed.".into();
                }
            }
        }
        if close {
            self.trace.open = false;
            self.trace.job = None;
        }
    }

    fn finish_trace(&mut self, id: LayerId, name: &str, hide: bool, r: &TraceResult, scale: f32, ox: f32, oy: f32) {
        if r.lines.is_empty() && r.fills.is_empty() {
            self.notice = Some("Nothing was found to trace. Try a lower line darkness.");
            return;
        }
        let data = to_vecdata(r, scale, ox, oy).sanitized();
        let (w, h) = (self.doc.w, self.doc.h);
        let n = format!("{name} (traced)");
        // (inside the frame, so the history picks the change up by itself; the hint names the step)
        let mut layer = self.doc.new_layer(&n, LayerKind::Vector);
        layer.vector = Some(Arc::new(data));
        vector::render_all(&mut layer, w, h);
        let new = self.doc.insert_layer(layer, id);
        if hide {
            if let Some(i) = self.doc.index_of(id) {
                self.doc.layers[i].visible = false;
            }
        }
        self.active = new;
        self.doc.mark_all_dirty();
        self.label_hint = Some(format!("Auto-trace · {name}"));
    }
}

#[cfg(test)]
impl DrawState {
    pub fn trace_mut(&mut self) -> &mut TraceUi {
        &mut self.trace
    }

    pub fn test_open_trace(&mut self, id: LayerId) {
        self.open_trace(id);
    }

    pub fn trace_ready(&self) -> bool {
        self.trace.result.is_some() && self.trace.job.is_none() && self.trace.preview.is_some()
    }

    pub fn trace_mut_ref(&self) -> &TraceUi {
        &self.trace
    }

    pub fn test_trace_now(&mut self) {
        // the same as pressing "Trace to vector layer"
        let t = &mut self.trace;
        if let Some(src) = t.src.clone() {
            let scale = (4000.0 / src.width().max(src.height()) as f32).min(1.0);
            let progress = Arc::new(AtomicU32::new(0));
            let rx = spawn(Arc::new(scaled(&src, scale)), t.opts.scaled(scale), Some(progress.clone()));
            t.fin = Some(Final { rx, progress, scale });
        }
    }
}

impl TraceUi {
    #[cfg(test)]
    pub fn set_mode(&mut self, m: TraceMode) {
        self.opts.mode = m;
    }
}
