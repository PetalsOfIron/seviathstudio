// SPDX-License-Identifier: GPL-3.0-or-later
// The Text tool: click the canvas to start a text layer, click a text to pick it up (drag to move it),
// and shape it in the options: the words, the font, size, color, alignment, spacing, bend, turn.

use std::sync::Arc;

use eframe::egui::{self, Color32, Pos2, Response, RichText, Ui, vec2};

use super::DrawState;
use crate::doc::LayerKind;
use crate::fonts;
use crate::text::{self, Align, TextData};
use crate::theme::*;
use crate::vector::{self, VecData};
use crate::widgets;

/// What dragging with the Text tool is doing.
#[derive(Default)]
pub struct TextUi {
    /// The text being dragged and the grab point's distance from its corner.
    drag: Option<(usize, egui::Vec2)>,
    /// True while the words box has the keyboard (typing is one undo step, not one per letter).
    pub typing: bool,
    /// Ask the words box for the keyboard on the next frame.
    focus_next: bool,
    font_filter: String,
}

impl DrawState {
    /// Index of the active layer when it is a text layer.
    pub(super) fn text_layer(&self) -> Option<usize> {
        let i = self.active_idx();
        self.doc.layers[i].text.is_some().then_some(i)
    }

    /// Make the layer's shapes and pixels from its text again.
    pub fn rebuild_text(&mut self, i: usize) {
        let Some(t) = self.doc.layers[i].text.clone() else { return };
        let fills = text::build(&t, &self.doc.meta.fonts);
        let (w, h) = (self.doc.w, self.doc.h);
        let l = &mut self.doc.layers[i];
        l.vector = Some(Arc::new(VecData { strokes: Vec::new(), width: 1.0, fills }));
        vector::render_all(l, w, h);
        l.rev += 1;
        self.doc.mark_all_dirty();
    }

    /// A new text layer above the selected one, with its corner at `at`.
    pub fn add_text_layer(&mut self, at: Pos2) {
        let t = TextData { x: at.x, y: at.y, color: [self.primary.r(), self.primary.g(), self.primary.b()], size: (self.doc.h as f32 / 14.0).clamp(24.0, 160.0), ..TextData::default() };
        let mut l = self.doc.new_layer("Text", LayerKind::Vector);
        l.text = Some(t);
        self.active = self.doc.insert_layer(l, self.active);
        let i = self.active_idx();
        self.rebuild_text(i);
        self.text_ui.focus_next = true;
        self.label_hint = Some("New text layer".into());
    }

    /// The box of a text layer on the page (x0, y0, x1, y1).
    fn text_box(&self, i: usize) -> Option<(f32, f32, f32, f32)> {
        let data = self.doc.layers[i].vector.as_ref()?;
        let mut b: Option<(f32, f32, f32, f32)> = None;
        for f in &data.fills {
            let (x0, y0, x1, y1) = vector::fill_bounds(f);
            b = Some(match b {
                Some(o) => (o.0.min(x0 as f32), o.1.min(y0 as f32), o.2.max(x1 as f32), o.3.max(y1 as f32)),
                None => (x0 as f32, y0 as f32, x1 as f32, y1 as f32),
            });
        }
        b
    }

    pub(super) fn text_input(&mut self, resp: &Response, ctx: &egui::Context) -> bool {
        let (pressed, down, latest) = ctx.input(|i| (i.pointer.primary_pressed(), i.pointer.primary_down(), i.pointer.latest_pos()));
        let over = resp.hovered() || resp.is_pointer_button_down_on();
        let view = self.view;
        if pressed && over {
            if let Some(sp) = latest {
                let d = view.screen_to_doc(sp);
                // the topmost text layer under the pointer
                let hit = (0..self.doc.layers.len()).find(|&i| {
                    let l = &self.doc.layers[i];
                    l.visible && !l.locked && l.text.is_some() && self.text_box(i).is_some_and(|b| d.x >= b.0 - 6.0 && d.x <= b.2 + 6.0 && d.y >= b.1 - 6.0 && d.y <= b.3 + 6.0)
                });
                match hit {
                    Some(i) => {
                        self.active = self.doc.layers[i].id;
                        let t = self.doc.layers[i].text.as_ref().map(|t| vec2(t.x, t.y)).unwrap_or_default();
                        self.text_ui.drag = Some((i, t - d.to_vec2()));
                        self.text_ui.focus_next = true;
                    }
                    None => {
                        let l = &self.doc.layers[self.active_idx()];
                        if l.locked {
                            self.notice = Some("This layer is locked.");
                        } else {
                            self.add_text_layer(d);
                        }
                    }
                }
            }
        }
        if let (Some((i, grab)), Some(sp)) = (self.text_ui.drag, latest) {
            if down {
                let d = view.screen_to_doc(sp).to_vec2() + grab;
                if let Some(t) = self.doc.layers[i].text.as_mut() {
                    if (t.x - d.x).abs() > 0.4 || (t.y - d.y).abs() > 0.4 {
                        t.x = d.x;
                        t.y = d.y;
                        self.rebuild_text(i);
                    }
                }
            } else {
                self.text_ui.drag = None;
            }
        }
        true
    }

    /// The outline of the picked text on the canvas.
    pub(super) fn text_overlay(&self, p: &egui::Painter) {
        if self.tool != crate::tabs::draw::Tool::Text {
            return;
        }
        let Some(i) = self.text_layer() else { return };
        if let Some((x0, y0, x1, y1)) = self.text_box(i) {
            let a = self.view.doc_to_screen(Pos2::new(x0, y0));
            let b = self.view.doc_to_screen(Pos2::new(x1, y1));
            let r = egui::Rect::from_two_pos(a, b).expand(4.0);
            p.rect_stroke(r, egui::CornerRadius::same(2), egui::Stroke::new(1.5, VENOM), egui::StrokeKind::Outside);
        }
    }

    pub(super) fn text_options(&mut self, ui: &mut Ui) {
        let Some(i) = self.text_layer() else {
            ui.label(RichText::new("Click the canvas to start a text. Click a text to pick it up; drag it to move it.").size(11.5).color(TEXT_DIM));
            return;
        };
        let before = self.doc.layers[i].text.clone().unwrap_or_default();
        let mut t = before.clone();
        ui.label(RichText::new("The words").size(12.0).color(TEXT_DIM));
        let words = ui.add(egui::TextEdit::multiline(&mut t.text).desired_rows(3).desired_width(f32::INFINITY).char_limit(text::MAX_CHARS).id(egui::Id::new("text_words")));
        if self.text_ui.focus_next {
            words.request_focus();
            self.text_ui.focus_next = false;
        }
        self.text_ui.typing = words.has_focus();
        // font
        let mut fonts_list: Vec<String> = vec![fonts::SANS.into(), fonts::MONO.into()];
        fonts_list.extend(self.doc.meta.fonts.iter().map(|f| f.name.clone()));
        ui.horizontal(|ui| {
            ui.label(RichText::new("Font").size(12.0).color(TEXT_DIM));
            egui::ComboBox::from_id_salt("text_font").selected_text(t.font.clone()).width(190.0).height(360.0).show_ui(ui, |ui| {
                ui.add(egui::TextEdit::singleline(&mut self.text_ui.font_filter).hint_text("Search fonts…").desired_width(f32::INFINITY));
                let q = self.text_ui.font_filter.to_lowercase();
                for n in &fonts_list {
                    if n.to_lowercase().contains(&q) {
                        ui.selectable_value(&mut t.font, n.clone(), n);
                    }
                }
                ui.separator();
                for (n, _) in fonts::system_fonts().iter().filter(|f| f.0.to_lowercase().contains(&q)).take(400) {
                    ui.selectable_value(&mut t.font, n.clone(), n);
                }
            });
        });
        ui.horizontal(|ui| {
            if widgets::chip(ui, "Add a font file…", false).on_hover_text("A .ttf or .otf file: it is kept inside this project").clicked() {
                if let Some(p) = rfd::FileDialog::new().set_title("Add a font").add_filter("Fonts", &["ttf", "otf"]).pick_file() {
                    match fonts::project_font_from_file(&p) {
                        Ok(f) if self.doc.meta.fonts.len() < fonts::MAX_PROJECT_FONTS => {
                            t.font = f.name.clone();
                            self.doc.meta.fonts.retain(|x| x.name != f.name);
                            self.doc.meta.fonts.push(f);
                        }
                        Ok(_) => self.notice = Some("This project already keeps as many fonts as it can."),
                        Err(_) => self.notice = Some("That doesn't look like a .ttf or .otf font."),
                    }
                }
            }
        });
        let slider = |ui: &mut Ui, label: &str, v: &mut f32, range: std::ops::RangeInclusive<f32>, suffix: &str| {
            ui.horizontal(|ui| {
                ui.label(RichText::new(label).size(12.0).color(TEXT_DIM));
                ui.add(egui::Slider::new(v, range).suffix(suffix).max_decimals(2));
            });
        };
        slider(ui, "Size", &mut t.size, 4.0..=600.0, " px");
        ui.horizontal(|ui| {
            ui.label(RichText::new("Color").size(12.0).color(TEXT_DIM));
            egui::color_picker::color_edit_button_srgb(ui, &mut t.color);
            if ui.small_button("Use my color").clicked() {
                t.color = [self.primary.r(), self.primary.g(), self.primary.b()];
            }
        });
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 4.0;
            for (a, name) in [(Align::Left, "Left"), (Align::Center, "Center"), (Align::Right, "Right")] {
                if widgets::chip(ui, name, t.align == a).clicked() {
                    t.align = a;
                }
            }
            ui.checkbox(&mut t.vertical, "Up and down");
        });
        slider(ui, "Letter gap", &mut t.letter_spacing, -20.0..=100.0, " px");
        slider(ui, "Line height", &mut t.line_height, 0.6..=3.0, "");
        slider(ui, "Bend", &mut t.bend, -1.0..=1.0, "");
        slider(ui, "Turn", &mut t.rotation, -180.0..=180.0, " deg");
        ui.horizontal(|ui| {
            ui.label(RichText::new("Place").size(12.0).color(TEXT_DIM));
            ui.add(egui::DragValue::new(&mut t.x).speed(1.0).suffix(" x"));
            ui.add(egui::DragValue::new(&mut t.y).speed(1.0).suffix(" y"));
            if ui.small_button("Center").on_hover_text("Put the text in the middle of the page").clicked() {
                let (w, h) = text::measure(&t, &self.doc.meta.fonts);
                t.x = (self.doc.w as f32 - w) / 2.0;
                t.y = (self.doc.h as f32 - h) / 2.0;
            }
        });
        ui.label(RichText::new("Shadow, outline and glow: Effects on the layer. Missing letters show as ?.").size(11.0).color(TEXT_DIM));
        let _ = Color32::WHITE;
        if t != before {
            self.doc.layers[i].text = Some(t.sanitized());
            self.rebuild_text(i);
            self.label_hint = Some(format!("Text · {}", self.doc.layers[i].name));
        }
    }
}
