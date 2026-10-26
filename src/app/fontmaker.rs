// SPDX-License-Identifier: GPL-3.0-or-later
// Make a font from your drawings. The window shows which letter comes next (with a faint guide), so
// you never have to pick keys: draw it, press Next. Any style works, because the app does not read the
// letters, it keeps the shape you draw for the letter it asked for.

use crate::widgets::HelpExt;
use eframe::egui::{self, Align2, Color32, Event, FontId, Pos2, Rect, RichText, Sense, Stroke, Vec2, vec2};

use super::SeviathApp;
use crate::fonts::{self, ProjectFont};
use crate::handfont::{self, BASE, CELL_H, CELL_W, GROUPS, HandFont, Line};
use crate::theme::*;
use crate::widgets::{self, section_title};

pub struct FontMaker {
    pub open: bool,
    pub font: HandFont,
    /// Which groups of letters to go through.
    groups: [bool; 5],
    extra: String,
    /// The letters to draw, in order, and where we are.
    pub order: Vec<char>,
    pub at: usize,
    loaded: bool,
    drawing: bool,
    preview: String,
    message: Option<String>,
    pub pad_rect: Rect,
}

impl Default for FontMaker {
    fn default() -> Self {
        Self {
            open: false,
            font: HandFont::default(),
            groups: [true, false, true, true, false],
            extra: String::new(),
            order: Vec::new(),
            at: 0,
            loaded: false,
            drawing: false,
            preview: "Sveiki, Latvija!".into(),
            message: None,
            pad_rect: Rect::NOTHING,
        }
    }
}

impl FontMaker {
    /// The list of letters to draw: the chosen groups, then the extras you added.
    fn rebuild_order(&mut self) {
        let cur = self.order.get(self.at).copied();
        let mut o: Vec<char> = Vec::new();
        for (i, (_, chars)) in GROUPS.iter().take(5).enumerate() {
            if self.groups[i] {
                o.extend(chars.chars());
            }
        }
        for c in self.font.glyphs.keys().chain(self.extra.chars().collect::<Vec<_>>().iter()) {
            if !o.contains(c) && *c != ' ' {
                o.push(*c);
            }
        }
        self.order = o;
        self.at = cur.and_then(|c| self.order.iter().position(|x| *x == c)).unwrap_or(0).min(self.order.len().saturating_sub(1));
    }

    #[cfg(test)]
    pub fn current_char(&self) -> Option<char> {
        self.current()
    }

    fn current(&self) -> Option<char> {
        self.order.get(self.at).copied()
    }

    fn step(&mut self, d: isize) {
        if !self.order.is_empty() {
            self.at = (self.at as isize + d).rem_euclid(self.order.len() as isize) as usize;
        }
    }

    /// Go to the next letter that has no drawing yet (or stay when all are done).
    fn next_empty(&mut self) {
        let n = self.order.len();
        for k in 1..=n {
            let i = (self.at + k) % n.max(1);
            if self.font.glyphs.get(&self.order[i]).is_none_or(|g| g.is_empty()) {
                self.at = i;
                return;
            }
        }
    }
}

impl SeviathApp {
    pub(super) fn open_font_maker(&mut self) {
        self.maker.loaded = false;
        self.maker.open = true;
    }

    #[cfg(test)]
    pub fn test_open_font_maker(&mut self) {
        self.open_font_maker();
    }

    #[cfg(test)]
    pub fn test_font_maker(&self) -> &FontMaker {
        &self.maker
    }

    /// Trace the active layer into lines for the current letter.
    fn letter_from_canvas(&mut self) {
        let d = &self.draw.doc;
        let Some(i) = d.index_of(self.draw.active_id()) else { return };
        let Some((x0, y0, x1, y1)) = crate::files::layer_bounds(&d.layers[i], d.w, d.h) else {
            self.maker.message = Some("The selected layer is empty.".into());
            return;
        };
        let img = crate::files::layer_rgba(&d.layers[i], x0, y0, (x1 - x0) as u32, (y1 - y0) as u32);
        let lines = handfont::lines_from_pixels(img.as_raw(), img.width() as usize, img.height() as usize);
        let Some(c) = self.maker.current() else { return };
        self.maker.message = Some(if lines.is_empty() {
            "No lines found on that layer.".into()
        } else {
            self.maker.font.glyphs.insert(c, lines);
            handfont::save_draft(&self.maker.font);
            format!("Took the lines from the layer for {c}.")
        });
    }

    pub(super) fn font_maker_window(&mut self, ctx: &egui::Context) {
        if !self.maker.open {
            return;
        }
        if !self.maker.loaded {
            self.maker.font = handfont::load_draft();
            self.maker.rebuild_order();
            self.maker.loaded = true;
        }
        let mut open = true;
        let (mut from_canvas, mut to_project, mut to_file) = (false, false, false);
        let mouse_p = self.settings.mouse_pressure;
        egui::Window::new(RichText::new("Font from my drawings").strong())
            .open(&mut open)
            .collapsible(false)
            .resizable(false)
            .pivot(Align2::CENTER_CENTER)
            .default_pos(ctx.content_rect().center())
            .show(ctx, |ui| {
                ui.set_width(560.0);
                let m = &mut self.maker;
                ui.label(RichText::new("Draw each letter in your own style: gothic, tattoo script, anything. The window tells you which letter is next, so nothing gets mixed up.").size(11.5).color(TEXT_DIM));
                ui.add_space(4.0);
                ui.horizontal_wrapped(|ui| {
                    ui.spacing_mut().item_spacing = vec2(4.0, 4.0);
                    ui.label(RichText::new("Letters").size(12.0).color(TEXT_DIM));
                    let mut changed = false;
                    for (i, (name, _)) in GROUPS.iter().take(5).enumerate() {
                        if widgets::chip(ui, name, m.groups[i]).clicked() {
                            m.groups[i] = !m.groups[i];
                            changed = true;
                        }
                    }
                    ui.add(egui::TextEdit::singleline(&mut m.extra).hint_text("add letters").desired_width(90.0).char_limit(40));
                    if ui.small_button("Add").clicked() {
                        changed = true;
                    }
                    if changed {
                        m.rebuild_order();
                    }
                });
                let done = m.order.iter().filter(|c| m.font.glyphs.get(c).is_some_and(|g| !g.is_empty())).count();
                ui.horizontal(|ui| {
                    match m.current() {
                        Some(c) => ui.label(RichText::new(format!("Draw: {c}")).size(20.0).strong()),
                        None => ui.label("Pick some letters above."),
                    };
                    ui.label(RichText::new(format!("{done} of {} drawn", m.order.len())).size(12.0).color(TEXT_DIM));
                });
                pad(ui, m, ctx, mouse_p);
                ui.horizontal_wrapped(|ui| {
                    if ui.button("Back").clicked() {
                        m.step(-1);
                    }
                    if ui.button("Skip").clicked() {
                        m.step(1);
                    }
                    if ui.button(RichText::new("Next letter").strong()).help("Goes to the next letter you have not drawn yet").clicked() {
                        m.next_empty();
                    }
                    ui.separator();
                    if ui.button("Undo line").clicked() {
                        if let Some(c) = m.current() {
                            if let Some(g) = m.font.glyphs.get_mut(&c) {
                                g.pop();
                            }
                            handfont::save_draft(&m.font);
                        }
                    }
                    if ui.button("Clear letter").clicked() {
                        if let Some(c) = m.current() {
                            m.font.glyphs.remove(&c);
                            handfont::save_draft(&m.font);
                        }
                    }
                    if ui.button("Take from the layer").help("Draw the letter on the canvas, select that layer, and the app traces it into clean lines here").clicked() {
                        from_canvas = true;
                    }
                });
                // jump to any letter
                ui.horizontal_wrapped(|ui| {
                    ui.spacing_mut().item_spacing = vec2(2.0, 2.0);
                    for (i, c) in m.order.clone().iter().enumerate() {
                        let drawn = m.font.glyphs.get(c).is_some_and(|g| !g.is_empty());
                        let (r, resp) = ui.allocate_exact_size(vec2(24.0, 24.0), Sense::click());
                        let fill = if drawn { Color32::from_rgb(40, 110, 70) } else { Color32::from_rgb(34, 38, 36) };
                        ui.painter().rect_filled(r, 3.0, fill);
                        if i == m.at {
                            ui.painter().rect_stroke(r, 3.0, Stroke::new(1.5, VENOM), egui::StrokeKind::Inside);
                        }
                        ui.painter().text(r.center(), Align2::CENTER_CENTER, c.to_string(), FontId::proportional(14.0), TEXT);
                        if resp.clicked() {
                            m.at = i;
                        }
                    }
                });

                section_title(ui, "Look");
                ui.horizontal_wrapped(|ui| {
                    ui.label(RichText::new("Name").size(12.0).color(TEXT_DIM));
                    ui.add(egui::TextEdit::singleline(&mut m.font.name).desired_width(170.0).char_limit(40));
                    ui.label(RichText::new("Line").size(12.0).color(TEXT_DIM));
                    ui.add(egui::Slider::new(&mut m.font.width, 3.0..=40.0).suffix(" px"));
                });
                ui.checkbox(&mut m.font.caps_for_small, "Use my capitals for small letters you did not draw").help("For gothic and tattoo styles that only have one kind of letter");
                ui.horizontal(|ui| {
                    ui.label(RichText::new("Try").size(12.0).color(TEXT_DIM));
                    ui.add(egui::TextEdit::singleline(&mut m.preview).desired_width(300.0).char_limit(60));
                });
                preview(ui, &m.font, &m.preview);
                ui.horizontal(|ui| {
                    if ui.button(RichText::new("Use in this project").strong()).help("The font shows up in the Text tool, and travels inside the project").clicked() {
                        to_project = true;
                    }
                    if ui.button("Save as a font file…").help("A .ttf you can install and use in other programs").clicked() {
                        to_file = true;
                    }
                    if let Some(msg) = &m.message {
                        ui.label(RichText::new(msg).size(11.5).color(TEXT_DIM));
                    }
                });
            });
        if from_canvas {
            self.letter_from_canvas();
        }
        if to_project || to_file {
            handfont::save_draft(&self.maker.font);
            match handfont::build_ttf(&self.maker.font) {
                Err(e) => self.maker.message = Some(e),
                Ok(bytes) => {
                    let name = self.maker.font.name.clone();
                    if to_file {
                        if let Some(p) = rfd::FileDialog::new().set_title("Save the font").set_file_name(format!("{name}.ttf")).add_filter("Font", &["ttf"]).save_file() {
                            self.maker.message = Some(match std::fs::write(&p, &bytes) {
                                Ok(()) => "Font file saved.".into(),
                                Err(e) => format!("Couldn't save: {e}"),
                            });
                        }
                    } else if bytes.len() > fonts::MAX_FONT_BYTES {
                        self.maker.message = Some("That font is too big.".into());
                    } else {
                        let pf = ProjectFont { name: name.clone(), data: crate::brush::tipimage::base64_encode(&bytes) };
                        let full = self.draw.doc.meta.fonts.len() >= fonts::MAX_PROJECT_FONTS && !self.draw.doc.meta.fonts.iter().any(|f| f.name == name);
                        if full {
                            self.maker.message = Some("This project already keeps as many fonts as it can.".into());
                        } else {
                            self.draw.apply_external("Add font", |d| {
                                d.doc.meta.fonts.retain(|f| f.name != pf.name);
                                d.doc.meta.fonts.push(pf);
                            });
                            self.maker.message = Some(format!("Added. Pick “{name}” in the Text tool (T)."));
                        }
                    }
                }
            }
        }
        self.maker.open = open;
    }
}

/// The drawing pad: a faint guide letter, a baseline, and what you have drawn so far.
fn pad(ui: &mut egui::Ui, m: &mut FontMaker, ctx: &egui::Context, mouse_p: f32) {
    let (rect, resp) = ui.allocate_exact_size(vec2(CELL_W, CELL_H), Sense::click_and_drag());
    m.pad_rect = rect;
    let origin = rect.min;
    let p = ui.painter_at(rect);
    p.rect_filled(rect, 4.0, Color32::from_rgb(246, 244, 238));
    let line = |y: f32, c: Color32| p.line_segment([Pos2::new(rect.min.x + 6.0, rect.min.y + y), Pos2::new(rect.max.x - 6.0, rect.min.y + y)], Stroke::new(1.0, c));
    line(BASE, Color32::from_rgb(190, 186, 176));
    line(BASE - 200.0, Color32::from_rgb(222, 218, 208));
    p.rect_filled(Rect::from_min_max(Pos2::new(rect.min.x, rect.min.y + BASE + 1.0), rect.max), 0.0, Color32::from_rgba_unmultiplied(0, 0, 0, 8));
    let Some(c) = m.current() else { return };
    // the guide: the letter itself, very faint, standing on the baseline
    p.text(Pos2::new(rect.center().x, rect.min.y + BASE), Align2::CENTER_BOTTOM, c.to_string(), FontId::proportional(250.0), Color32::from_rgba_unmultiplied(120, 120, 120, 34));
    if resp.drag_started() {
        m.drawing = true;
        m.font.glyphs.entry(c).or_default().push(Line::new());
        if let Some(o) = ctx.input(|i| i.pointer.press_origin()) {
            add_point(m, c, o - origin, mouse_p);
        }
    }
    if m.drawing {
        let events = ctx.input(|i| i.events.clone());
        for e in &events {
            if let Event::PointerMoved(pos) = e {
                add_point(m, c, *pos - origin, mouse_p);
            }
        }
        if resp.drag_stopped() || !ctx.input(|i| i.pointer.primary_down()) {
            m.drawing = false;
            if let Some(g) = m.font.glyphs.get_mut(&c) {
                if g.last().is_some_and(|l| l.len() < 2) {
                    g.pop();
                }
            }
            handfont::save_draft(&m.font);
        }
    }
    let w = m.font.width;
    for l in m.font.glyphs.get(&c).into_iter().flatten() {
        for s in l.windows(2) {
            let pr = (s[0][2] + s[1][2]) / 2.0;
            p.line_segment([origin + vec2(s[0][0], s[0][1]), origin + vec2(s[1][0], s[1][1])], Stroke::new(w * (0.25 + 0.75 * pr), Color32::from_rgb(30, 30, 34)));
        }
    }
}

fn add_point(m: &mut FontMaker, c: char, at: Vec2, pressure: f32) {
    if let Some(l) = m.font.glyphs.entry(c).or_default().last_mut() {
        if l.last().is_some_and(|q| (q[0] - at.x).abs() < 0.4 && (q[1] - at.y).abs() < 0.4) {
            return;
        }
        l.push([at.x.clamp(0.0, CELL_W), at.y.clamp(0.0, CELL_H), pressure.clamp(0.0, 1.0)]);
    }
}

/// A line of text drawn straight from your lines (the same spacing the font will have).
fn preview(ui: &mut egui::Ui, f: &HandFont, text: &str) {
    let k = 0.28;
    let (rect, _) = ui.allocate_exact_size(vec2(540.0, 120.0), Sense::hover());
    let p = ui.painter_at(rect);
    p.rect_filled(rect, 4.0, Color32::from_rgb(246, 244, 238));
    let mut x = 12.0;
    let base = rect.min.y + 92.0;
    for ch in text.chars() {
        let Some(g) = f.glyph_for(ch) else {
            x += if ch == ' ' { 140.0 * k } else { 60.0 * k };
            continue;
        };
        let (mut lo, mut hi) = (f32::MAX, f32::MIN);
        for q in g.iter().flatten() {
            lo = lo.min(q[0]);
            hi = hi.max(q[0]);
        }
        let shift = 16.0 - lo;
        for l in g {
            for s in l.windows(2) {
                let pr = (s[0][2] + s[1][2]) / 2.0;
                let pt = |q: &[f32; 3]| Pos2::new(rect.min.x + x + (q[0] + shift) * k, base - (BASE - q[1]) * k);
                p.line_segment([pt(&s[0]), pt(&s[1])], Stroke::new((f.width * (0.25 + 0.75 * pr) * k).max(0.8), Color32::from_rgb(30, 30, 34)));
            }
        }
        x += (hi - lo + 32.0) * k;
        if x > rect.width() {
            break;
        }
    }
}
