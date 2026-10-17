// SPDX-License-Identifier: GPL-3.0-or-later
// Brush panel: categories, search, brush cards with real previews (painted by the engine),
// and every brush setting in folding sections. Brush, eraser and smudge each remember their own brush.

use std::collections::HashMap;

use eframe::egui::{
    self, Align, Align2, Color32, ColorImage, CornerRadius, FontId, Layout, RichText, Sense, Stroke as UiStroke, StrokeKind,
    TextureHandle, TextureOptions, Ui, pos2, vec2,
};

use super::{DrawState, Tool};
use crate::brush::engine::{Sample, Stroke};
use crate::brush::{
    BrushSettings, CATEGORIES, Dip, Edge, Ends, Fray, MixRefer, Paint, Preset, Transition, ShapeBase, SubMode, SubTip, TexKind, TexPalette, Tip, eraser_presets,
    presets,
};
use crate::doc::{Document, TILE};
use crate::theme::*;
use crate::widgets::{self, section_title};

const PREVIEW_W: usize = 220;
const PREVIEW_H: usize = 64;
const INK: Color32 = Color32::from_rgb(29, 29, 36);

#[derive(Clone)]
pub struct ToolBrush {
    pub s: BrushSettings,
    pub name: String,
    pub category: usize,
}

pub struct BrushLib {
    pub presets: Vec<Preset>,
    pub erasers: Vec<Preset>,
    pub mem: [ToolBrush; 3], // brush, eraser, smudge
    previews: HashMap<String, TextureHandle>,
    current_preview: Option<(String, TextureHandle)>,
    query: String,
    /// The groups the randomizer rolls (the others are locked).
    pub rolled: Vec<crate::brush::random::Group>,
    roll_count: u32,
}

impl Default for BrushLib {
    fn default() -> Self {
        let presets = presets();
        let erasers = eraser_presets();
        let pick = |name: &str| presets.iter().find(|p| p.name == name).cloned().unwrap_or_else(|| presets[0].clone());
        let cat = |p: &Preset| CATEGORIES.iter().position(|c| *c == p.category).unwrap_or(0);
        let pen = pick("Pen");
        let smudge = pick("Smudge");
        let mem = [
            ToolBrush { s: pen.s.clone(), name: pen.name.into(), category: cat(&pen) },
            ToolBrush { s: erasers[0].s.clone(), name: erasers[0].name.into(), category: 0 },
            ToolBrush { s: smudge.s.clone(), name: smudge.name.into(), category: cat(&smudge) },
        ];
        Self { presets, erasers, mem, previews: HashMap::new(), current_preview: None, query: String::new(), rolled: crate::brush::random::Group::ALL.iter().map(|g| g.0).collect(), roll_count: 0 }
    }
}

/// Which remembered brush a tool uses.
pub fn slot(tool: Tool) -> usize {
    match tool {
        Tool::Eraser => 1,
        Tool::Smudge => 2,
        _ => 0,
    }
}

/// A short S-curve painted with the brush by the real engine, for the brush list.
pub fn render_preview(s: &BrushSettings) -> ColorImage {
    render_preview_sized(s, PREVIEW_W, PREVIEW_H, INK)
}

pub fn render_preview_sized(s: &BrushSettings, pw: usize, ph: usize, ink: Color32) -> ColorImage {
    let mut doc = Document::new(pw as u32, ph as u32);
    let h = ph as f32;
    let w = pw as f32;
    let mut p = s.clone();
    p.stabilize = 0.0;
    p.size = if s.size > 60.0 { h * 0.45 } else { (s.size * (h / 70.0)).max(4.0) }.min(h * 0.5);
    if s.mix >= 0.9 {
        // blenders add no color: show them faint
        p.mix = 0.0;
        p.opacity *= 0.35;
    } else if s.mix > 0.0 {
        p.mix = 0.0;
    }
    let none = |_: i32, _: i32| -> Option<Color32> { None };
    let pt = |i: usize| {
        let t = i as f32 / 60.0;
        Sample { x: w * 0.08 + t * w * 0.84, y: h / 2.0 + (t * std::f32::consts::TAU).sin() * h * 0.22, p: (t * std::f32::consts::PI).sin() * 0.9 + 0.1 }
    };
    let mut st = Stroke::begin(&mut doc, 0, &p, ink, false, 1.0, true, pt(0), &none);
    let pts: Vec<Sample> = (1..=60).map(pt).collect();
    st.add(&mut doc, &pts, &none);
    st.finish(&mut doc, &none);
    let mut px = vec![Color32::TRANSPARENT; pw * ph];
    for y in 0..ph {
        for x in 0..pw {
            if let Some(t) = doc.layers[0].tiles.get(&((x / TILE) as i32, (y / TILE) as i32)) {
                px[y * pw + x] = t[(y % TILE) * TILE + x % TILE];
            }
        }
    }
    ColorImage::new([pw, ph], px)
}

fn pct(ui: &mut Ui, label: &str, v: &mut f32, tip: &str) -> bool {
    row(ui, label, tip, |ui| ui.add(egui::Slider::new(v, 0.0..=1.0).custom_formatter(|x, _| format!("{:.0}%", x * 100.0))).changed())
}

fn num(ui: &mut Ui, label: &str, v: &mut f32, range: std::ops::RangeInclusive<f32>, tip: &str) -> bool {
    row(ui, label, tip, |ui| ui.add(egui::Slider::new(v, range).max_decimals(2)).changed())
}

fn row(ui: &mut Ui, label: &str, tip: &str, add: impl FnOnce(&mut Ui) -> bool) -> bool {
    ui.horizontal(|ui| {
        let (r, resp) = ui.allocate_exact_size(vec2(84.0, 18.0), Sense::hover());
        ui.painter().text(r.left_center(), Align2::LEFT_CENTER, label, FontId::proportional(12.0), TEXT_DIM);
        if !tip.is_empty() {
            resp.on_hover_text(tip);
        }
        add(ui)
    })
    .inner
}

fn section(ui: &mut Ui, title: &str, open: bool, add: impl FnOnce(&mut Ui)) {
    egui::CollapsingHeader::new(RichText::new(title).size(13.0).strong().color(TEXT))
        .default_open(open)
        .show(ui, |ui| {
            ui.spacing_mut().slider_width = (ui.available_width() - 150.0).max(80.0);
            add(ui);
        });
}

impl DrawState {
    pub fn brush(&self) -> &BrushSettings {
        &self.lib.mem[slot(self.tool)].s
    }

    pub fn brush_mut(&mut self) -> &mut BrushSettings {
        &mut self.lib.mem[slot(self.tool)].s
    }

    pub fn brush_name(&self) -> &str {
        &self.lib.mem[slot(self.tool)].name
    }

    fn preview(&mut self, ctx: &egui::Context, p: &Preset, budget: &mut u32) -> Option<egui::TextureId> {
        if let Some(t) = self.lib.previews.get(p.name) {
            return Some(t.id());
        }
        if *budget == 0 {
            ctx.request_repaint();
            return None;
        }
        *budget -= 1;
        let img = render_preview(&p.s);
        let t = ctx.load_texture(format!("preview:{}", p.name), img, TextureOptions::LINEAR);
        let id = t.id();
        self.lib.previews.insert(p.name.to_owned(), t);
        Some(id)
    }

    pub(super) fn brush_panel(&mut self, ui: &mut Ui) {
        let sl = slot(self.tool);
        let title = match sl {
            1 => "Eraser",
            2 => "Smudge & blur",
            _ => "Brush",
        };
        ui.horizontal(|ui| {
            section_title(ui, title);
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                ui.label(RichText::new(self.lib.mem[sl].name.clone()).size(12.0).color(VENOM));
            });
        });
        ui.add_space(2.0);

        if sl == 1 && self.vec_active_layer().is_some() {
            ui.label(RichText::new("This is a vector layer: the eraser works on its lines.").size(11.5).color(TEXT_DIM));
            widgets::segmented(ui, &mut self.vec.erase, &[(crate::vector::EraseMode::Cut, "Cut the touched part"), (crate::vector::EraseMode::Whole, "Remove whole lines")]);
            ui.add_space(4.0);
        }
        if sl != 1 {
            ui.add(egui::TextEdit::singleline(&mut self.lib.query).hint_text("Search brushes…").desired_width(f32::INFINITY));
            ui.add_space(2.0);
            if self.lib.query.trim().is_empty() {
                ui.horizontal_wrapped(|ui| {
                    ui.spacing_mut().item_spacing = vec2(4.0, 4.0);
                    for (i, name) in CATEGORIES.iter().enumerate() {
                        if widgets::chip(ui, name, self.lib.mem[sl].category == i).clicked() {
                            self.lib.mem[sl].category = i;
                        }
                    }
                });
            }
            ui.add_space(6.0);
        }

        // The brush list
        let list: Vec<Preset> = if sl == 1 {
            self.lib.erasers.clone()
        } else {
            let q = self.lib.query.trim().to_lowercase();
            if q.is_empty() {
                let cat = CATEGORIES[self.lib.mem[sl].category];
                self.lib.presets.iter().filter(|p| p.category == cat).cloned().collect()
            } else {
                self.lib.presets.iter().filter(|p| p.name.to_lowercase().contains(&q) || p.about.to_lowercase().contains(&q)).cloned().collect()
            }
        };
        let ctx = ui.ctx().clone();
        let mut budget = self.set.brush_preview_per_frame; // previews made per frame, so opening a category never stutters
        egui::ScrollArea::vertical().id_salt("brush_list").max_height(ui.available_height() * 0.42).show(ui, |ui| {
            if list.is_empty() {
                ui.add_space(10.0);
                ui.label(RichText::new("Your saved and imported brushes will appear here.").color(TEXT_DIM).italics());
                return;
            }
            let w = ((ui.available_width() - 6.0) / 2.0).floor();
            egui::Grid::new("brush_grid").spacing(vec2(6.0, 6.0)).show(ui, |ui| {
                for (i, p) in list.iter().enumerate() {
                    let selected = self.lib.mem[sl].name == p.name;
                    let tex = self.preview(&ctx, p, &mut budget);
                    if brush_card(ui, p, tex, w, selected).clicked() {
                        // switching brushes keeps your size (like the web version), unless turned off
                        let size = if self.set.keep_size_on_switch { self.lib.mem[sl].s.size } else { p.s.size };
                        self.lib.mem[sl].s = BrushSettings { size, ..p.s.clone() };
                        self.lib.mem[sl].name = p.name.to_owned();
                    }
                    if i % 2 == 1 {
                        ui.end_row();
                    }
                }
            });
        });

        ui.add_space(6.0);
        ui.separator();
        egui::ScrollArea::vertical().id_salt("brush_settings").show(ui, |ui| self.brush_settings(ui));
    }

    fn brush_settings(&mut self, ui: &mut Ui) {
        let sl = slot(self.tool);
        let ctx = ui.ctx().clone();
        // live preview of the brush as it is set now
        let key = format!("{:?}", self.lib.mem[sl].s);
        let stale = self.lib.current_preview.as_ref().map(|(k, _)| *k != key).unwrap_or(true);
        if stale {
            let img = render_preview(&self.lib.mem[sl].s);
            match &mut self.lib.current_preview {
                Some((k, t)) => {
                    t.set(img, TextureOptions::LINEAR);
                    *k = key;
                }
                None => self.lib.current_preview = Some((key, ctx.load_texture("preview:current", img, TextureOptions::LINEAR))),
            }
        }
        if let Some((_, t)) = &self.lib.current_preview {
            let (r, _) = ui.allocate_exact_size(vec2(ui.available_width(), 48.0), Sense::hover());
            ui.painter().rect_filled(r, CornerRadius::same(6), Color32::from_rgb(236, 240, 236));
            ui.painter().image(t.id(), r.shrink2(vec2(4.0, 2.0)), egui::Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)), Color32::WHITE);
        }

        let size_max = self.set.brush_size_max;
        let name = self.lib.mem[sl].name.clone();
        let preset = self.lib.presets.iter().chain(self.lib.erasers.iter()).find(|p| p.name == name).cloned();
        if let Some(p) = &preset {
            ui.label(RichText::new(p.about).size(11.5).color(TEXT_DIM));
        }
        let s = &mut self.lib.mem[sl].s;
        let rolled = &mut self.lib.rolled;

        ui.add_space(4.0);
        row(ui, "Size", "Brush size in pixels ([ and ] change it)", |ui| {
            ui.add(egui::Slider::new(&mut s.size, 0.5..=size_max).logarithmic(true).suffix(" px").max_decimals(1)).changed()
        });
        ui.horizontal(|ui| {
            ui.add_space(92.0);
            ui.spacing_mut().item_spacing.x = 3.0;
            for v in [2.0f32, 4.0, 8.0, 16.0, 32.0, 64.0, 128.0] {
                if widgets::chip(ui, &format!("{v}"), (s.size - v).abs() < 0.01).clicked() {
                    s.size = v;
                }
            }
        });
        pct(ui, "Opacity", &mut s.opacity, "The most paint one stroke can lay down, however often it overlaps itself");
        pct(ui, "Flow", &mut s.flow, "How much paint each dab adds (low = builds up like an airbrush)");
        pct(ui, "Hardness", &mut s.hardness, "Sharp or soft edge");
        num(ui, "Spacing", &mut s.spacing, 0.01..=2.0, "Distance between dabs, × brush size");
        pct(ui, "Stabilizer", &mut s.stabilize, "Smooths shaky lines: the brush follows the pen on a rope");

        section(ui, "Pen pressure", true, |ui| {
            ui.checkbox(&mut s.size_pressure, "Pressure changes the size");
            if s.size_pressure {
                pct(ui, "Smallest", &mut s.min_size, "Size at the lightest pressure");
            }
            ui.checkbox(&mut s.opacity_pressure, "Pressure changes the paint amount");
            ui.label(RichText::new("Uses Windows Ink. Mouse strokes count as full pressure.").size(11.0).color(TEXT_DIM));
        });

        section(ui, "Stroke ends", false, |ui| {
            widgets::segmented(ui, &mut s.ends, &[(Ends::Round, "Round"), (Ends::Taper, "Pointed"), (Ends::Flat, "Flat")]);
            if s.ends == Ends::Taper {
                pct(ui, "Taper", &mut s.taper, "How long the pointed tips are");
            }
        });

        section(ui, "Gradient color", false, |ui| DrawState::gradient_brush_section(&mut self.grad, ui));

        section(ui, "Tip & shape", false, |ui| {
            row(ui, "Tip", "", |ui| {
                let mut changed = false;
                egui::ComboBox::from_id_salt("tip").selected_text(s.tip.label()).show_ui(ui, |ui| {
                    for t in Tip::ALL {
                        changed |= ui.selectable_value(&mut s.tip, t, t.label()).changed();
                    }
                });
                changed
            });
            if s.tip == Tip::Shape {
                ui.horizontal_wrapped(|ui| {
                    ui.spacing_mut().item_spacing = vec2(4.0, 4.0);
                    for b in ShapeBase::ALL {
                        if widgets::chip(ui, b.label(), s.shape_base == b).clicked() {
                            s.shape_base = b;
                        }
                    }
                });
                if matches!(s.shape_base, ShapeBase::Polygon | ShapeBase::Star) {
                    row(ui, "Points", "", |ui| ui.add(egui::Slider::new(&mut s.sides, 3..=12)).changed());
                }
                if s.shape_base == ShapeBase::Star {
                    num(ui, "Star depth", &mut s.star_inner, 0.1..=0.9, "How deep the star's points are cut");
                }
            }
            num(ui, "Roundness", &mut s.roundness, 0.05..=1.0, "1 = round, small = flat like a chisel or calligraphy nib");
            num(ui, "Angle", &mut s.angle, -180.0..=180.0, "Turns the tip");
            ui.checkbox(&mut s.follow_direction, "Tip turns with the stroke");
            ui.checkbox(&mut s.antialias, "Smooth edges (off = crisp pixels)");
        });

        section(ui, "Edges & wet look", false, |ui| {
            row(ui, "Edge", "", |ui| {
                let mut changed = false;
                egui::ComboBox::from_id_salt("edge").selected_text(s.edge.label()).show_ui(ui, |ui| {
                    for e in Edge::ALL {
                        changed |= ui.selectable_value(&mut s.edge, e, e.label()).on_hover_text(e.about()).changed();
                    }
                });
                changed
            });
            if s.edge != Edge::Smooth {
                pct(ui, "Amount", &mut s.edge_amount, "How strong the edge effect is");
                pct(ui, "Detail", &mut s.edge_detail, "Big wobbles … fine fringe");
            }
            pct(ui, "Wet rim", &mut s.wet_rim, "Paint collects at the edge like watercolor");
            pct(ui, "Grain", &mut s.grain, "Grainy holes inside (dry media)");
            pct(ui, "Bristles", &mut s.streaks, "Bristle marks along the stroke");
            pct(ui, "Warp", &mut s.deform, "Bends and warps the shape");
            pct(ui, "Edge holes", &mut s.edge_holes, "The outline breaks up into holes");
            pct(ui, "Outline only", &mut s.hollow, "The middle empties out: double lines, outlined strokes");
            if s.hollow > 0.0 {
                num(ui, "Outline width", &mut s.outline_width, 0.05..=0.6, "× brush size");
            }
            row(ui, "Versions", "Each dab uses a slightly different version of the tip", |ui| {
                ui.add(egui::Slider::new(&mut s.variants, 1..=8)).changed()
            });
        });

        section(ui, "Texture", s.texture > 0.0 || s.tex_color > 0.0, |ui| {
            pct(ui, "Strength", &mut s.texture, "Paper grain or surface texture, fixed to the canvas like CSP");
            row(ui, "Texture", "", |ui| {
                let mut changed = false;
                egui::ComboBox::from_id_salt("texkind").selected_text(s.tex_kind.label()).height(400.0).show_ui(ui, |ui| {
                    let mut last = "";
                    for k in TexKind::ALL {
                        if k.section() != last {
                            last = k.section();
                            ui.label(RichText::new(last).size(11.0).color(GOLD));
                        }
                        changed |= ui.selectable_value(&mut s.tex_kind, k, k.label()).changed();
                    }
                });
                changed
            });
            num(ui, "Scale", &mut s.tex_scale, 0.25..=4.0, "Size of the texture");
            let mut keep = s.tex_color > 0.0;
            if ui.checkbox(&mut keep, "Keep texture colors").on_hover_text("Paint with the texture's own colors instead of only your color").changed() {
                s.tex_color = if keep { 1.0 } else { 0.0 };
            }
            if keep {
                pct(ui, "Color amount", &mut s.tex_color, "0 = your color … 100% = the texture's colors");
                widgets::segmented(ui, &mut s.tex_palette, &[(TexPalette::Natural, "Natural colors"), (TexPalette::FromBrush, "From my color")]);
            }
        });

        section(ui, "Scatter & random", false, |ui| {
            num(ui, "Scatter", &mut s.scatter, 0.0..=3.0, "Dabs spread sideways (× size)");
            pct(ui, "Size", &mut s.size_jitter, "Random size changes");
            pct(ui, "Angle", &mut s.angle_jitter, "Random turning (100% = any angle)");
            pct(ui, "Density", &mut s.opacity_jitter, "Thicker and thinner patches of paint");
            pct(ui, "Color", &mut s.color_jitter, "Shades drifting along the stroke (leaves, grass)");
        });

        section(ui, "Particles", s.particles > 0, |ui| {
            row(ui, "Particles", "0 = off; else how many separate small dabs make up each dab", |ui| {
                ui.add(egui::Slider::new(&mut s.particles, 0..=60)).changed()
            });
            if s.particles > 0 {
                num(ui, "Particle size", &mut s.particle_size, 0.02..=0.8, "× brush size");
                pct(ui, "Keep apart", &mut s.particle_gap, "Particles keep a gap between each other instead of overlapping");
            }
        });

        section(ui, "Dual / triple tip", !s.extra_tips.is_empty(), |ui| {
            ui.label(RichText::new("Extra tips combine with the main tip, like CSP's 2-brush tip.").size(11.0).color(TEXT_DIM));
            let mut remove = None;
            for (i, sub) in s.extra_tips.iter_mut().enumerate() {
                ui.push_id(i, |ui| {
                    ui.horizontal(|ui| {
                        ui.label(RichText::new(format!("Tip {}", i + 2)).strong());
                        egui::ComboBox::from_id_salt("subtip").selected_text(sub.tip.label()).show_ui(ui, |ui| {
                            for t in Tip::ALL {
                                ui.selectable_value(&mut sub.tip, t, t.label());
                            }
                        });
                        if ui.small_button("Remove").clicked() {
                            remove = Some(i);
                        }
                    });
                    egui::ComboBox::from_id_salt("submode").selected_text(sub.mode.label()).show_ui(ui, |ui| {
                        for m in [SubMode::Mask, SubMode::Add, SubMode::Cut] {
                            ui.selectable_value(&mut sub.mode, m, m.label());
                        }
                    });
                    num(ui, "Size", &mut sub.scale, 0.1..=2.0, "× main tip");
                    pct(ui, "Scatter", &mut sub.scatter, "Moves around inside the main tip");
                    pct(ui, "Turning", &mut sub.angle_jitter, "Random angle");
                    pct(ui, "Strength", &mut sub.strength, "");
                    ui.separator();
                });
            }
            if let Some(i) = remove {
                s.extra_tips.remove(i);
            }
            if s.extra_tips.len() < 2 && ui.button("+ Add a tip").clicked() {
                s.extra_tips.push(SubTip::default());
                s.variants = s.variants.max(4);
            }
        });

        section(ui, "Color mixing", s.mix > 0.0, |ui| {
            pct(ui, "Mix", &mut s.mix, "0 = clean paint … 100% = only blends the colors already there");
            if s.mix > 0.0 {
                widgets::segmented(ui, &mut s.mix_refer, &[(MixRefer::All, "All layers"), (MixRefer::Layer, "This layer")]);
            }
        });

        section(ui, "Hair strands", s.strands > 0 || s.comb > 0.0, |ui| {
            row(ui, "Strands", "How many fine hairs the stroke is made of (0 = off)", |ui| ui.add(egui::Slider::new(&mut s.strands, 0..=40)).changed());
            if s.strands > 0 {
                num(ui, "Thickness", &mut s.strand_width, 0.005..=0.15, "Strand width (× brush size)");
                pct(ui, "Clumps", &mut s.clump, "Strands gather in clumps instead of spreading evenly");
                if s.clump > 0.0 {
                    row(ui, "Clump count", "How many clumps", |ui| ui.add(egui::Slider::new(&mut s.clump_count, 1..=8)).changed());
                }
                pct(ui, "Curl", &mut s.curl, "Strands wind from side to side");
                if s.curl > 0.0 {
                    num(ui, "Curl length", &mut s.curl_len, 20.0..=500.0, "Pixels of stroke for one curl");
                }
                pct(ui, "Wobble", &mut s.strand_wobble, "Each strand wanders a little");
                pct(ui, "Flyaways", &mut s.flyaways, "Stray hairs lift off the edge");
                pct(ui, "Shine", &mut s.sheen, "A band of shine glides across the strands");
                pct(ui, "Underpaint", &mut s.strand_fill, "A soft solid fill between the strands, for volume");
            }
            pct(ui, "Comb", &mut s.comb, "A blending brush pulls colors along the strands under it, not across them (set Mix up)");
            pct(ui, "Gloss", &mut s.gloss, "A blending brush leaves a little shine");
        });

        section(ui, "Skin", s.terminator > 0.0 || s.tint != 0.0 || s.keep_detail > 0.0 || s.sss > 0.0 || s.temp_blend || s.dot_cluster > 0.0, |ui| {
            pct(ui, "Shade edge", &mut s.terminator, "A clearer edge between light and shade (100% = cel style)");
            num(ui, "Warm / cool", &mut s.tint, -1.0..=1.0, "Nudges the color toward warm (right) or cool (left)");
            pct(ui, "Dot patches", &mut s.dot_cluster, "Dots (freckles, blush) land in natural patches");
            pct(ui, "Keep texture", &mut s.keep_detail, "A blending brush changes the soft color and keeps the fine texture (pores)");
            pct(ui, "Skin glow", &mut s.sss, "Mid tones glow a little red, like light through skin");
            ui.checkbox(&mut s.temp_blend, "Even out warm and cool").on_hover_text("A blending brush moves warm and cool patches together without flattening lights and darks");
        });

        section(ui, "Width behavior", s.pinch > 0.0 || s.pressure_curve != 0.0, |ui| {
            pct(ui, "Pinch", &mut s.pinch, "The stroke narrows at regular points, like a bamboo stem or a bead chain");
            if s.pinch > 0.0 {
                num(ui, "Pinch every", &mut s.pinch_every, 20.0..=800.0, "Pixels from one pinch to the next");
            }
            num(ui, "Pressure curve", &mut s.pressure_curve, -1.0..=1.0, "Negative: a light touch is enough. Positive: you have to press harder");
            if s.ends == Ends::Taper {
                pct(ui, "Start width", &mut s.tip_start, "How thin the stroke begins");
                pct(ui, "End width", &mut s.tip_end, "How thin the stroke ends");
            } else {
                ui.label(RichText::new("Set Stroke ends to Taper to choose the start and end widths.").size(11.0).color(TEXT_DIM));
            }
        });

        section(ui, "Color along the stroke", s.transition != Transition::Off || s.drift != 0.0, |ui| {
            let opts: Vec<(Transition, &str)> = Transition::ALL.iter().map(|(t, l, _)| (*t, *l)).collect();
            widgets::segmented(ui, &mut s.transition, &opts);
            if let Some((_, _, about)) = Transition::ALL.iter().find(|(t, _, _)| *t == s.transition) {
                ui.label(RichText::new(*about).size(11.5).color(TEXT_DIM));
            }
            if s.transition != Transition::Off {
                row(ui, "Second color", "The color the stroke moves toward (also used by dots)", |ui| egui::color_picker::color_edit_button_rgb(ui, &mut s.trans_color).changed());
                if matches!(s.transition, Transition::Length | Transition::Cycle) {
                    num(ui, "Over", &mut s.trans_len, 30.0..=3000.0, "Pixels of stroke for the whole change");
                }
                ui.checkbox(&mut s.trans_back, "Backwards").on_hover_text("Start with the second color");
            }
            num(ui, "Paint runs out", &mut s.drift, -1.0..=1.0, "The color changes as the brush runs dry: positive darkens and warms, negative lightens");
            if s.drift != 0.0 {
                num(ui, "Runs out after", &mut s.drift_len, 100.0..=3000.0, "Pixels of stroke");
            }
        });

        section(ui, "Dots", s.dots > 0.0, |ui| {
            pct(ui, "Dots", &mut s.dots, "100% = the stroke is made of separate dots; less keeps part of the normal stroke");
            if s.dots > 0.0 {
                num(ui, "Smallest", &mut s.dot_min, 0.01..=0.8, "× brush size");
                num(ui, "Biggest", &mut s.dot_max, 0.01..=0.9, "× brush size");
                row(ui, "Dots per dab", "How many dots each dab drops", |ui| ui.add(egui::Slider::new(&mut s.dot_count, 1..=16)).changed());
                num(ui, "Spread", &mut s.dot_spread, 0.0..=2.5, "How far from the line the dots land (× brush size)");
                pct(ui, "Many colors", &mut s.dot_colors, "Each dot takes a color between the brush color and the second color (set under Color along the stroke)");
                ui.label(RichText::new("Raise Spacing so the dots don't pile up.").size(11.0).color(TEXT_DIM));
            }
        });

        section(ui, "Lines inside the stroke", s.lines > 0, |ui| {
            row(ui, "Threads", "How many lines run inside the stroke (0 = off)", |ui| ui.add(egui::Slider::new(&mut s.lines, 0..=12)).changed());
            if s.lines > 0 {
                num(ui, "Wave", &mut s.line_wave, 0.0..=1.0, "How far each thread swings sideways");
                num(ui, "Wave length", &mut s.line_len, 10.0..=600.0, "Pixels of stroke for one wave");
                num(ui, "Thickness", &mut s.line_width, 0.01..=0.4, "Thread width (× half the stroke width)");
                pct(ui, "Strength", &mut s.line_strength, "How visible the threads are");
                num(ui, "Shade", &mut s.line_shade, -1.0..=1.0, "Darker (left) or lighter (right) than the stroke");
            }
        });

        section(ui, "Edge fray", s.fray != Fray::Off, |ui| {
            let opts: Vec<(Fray, &str)> = Fray::ALL.iter().map(|(t, l, _)| (*t, *l)).collect();
            widgets::segmented(ui, &mut s.fray, &opts);
            if let Some((_, _, about)) = Fray::ALL.iter().find(|(t, _, _)| *t == s.fray) {
                ui.label(RichText::new(*about).size(11.5).color(TEXT_DIM));
            }
            if s.fray != Fray::Off {
                pct(ui, "Amount", &mut s.fray_amount, "How much of the edge breaks up");
                num(ui, "Size", &mut s.fray_len, 1.0..=60.0, "Pixels: the size of the teeth, splits or lumps");
                ui.label(RichText::new("Works for strokes drawn in one go (not scattered dabs).").size(11.0).color(TEXT_DIM));
            }
        });

        section(ui, "Dashes", s.gap_len > 0.0, |ui| {
            num(ui, "Gap", &mut s.gap_len, 0.0..=400.0, "Pixels left empty between dashes (0 = a solid line)");
            if s.gap_len > 0.0 {
                num(ui, "Dash", &mut s.dash_len, 2.0..=800.0, "Pixels painted before each gap");
            }
        });

        section(ui, "Wet paint", s.water > 0.0 || s.bleed > 0.0 || s.pool > 0.0 || s.granulation > 0.0, |ui| {
            pct(ui, "Water", &mut s.water, "Thins the paint and leaves it wet for twenty seconds");
            pct(ui, "Bleed", &mut s.bleed, "Wet paint spreads into the paint around it when the pen lifts");
            pct(ui, "Edge pooling", &mut s.pool, "Pigment collects at the edge of a wash and makes it darker");
            pct(ui, "Granulation", &mut s.granulation, "Pigment settles into the paper tooth (see Texture for the paper)");
        });
        let mut roll: Option<Vec<crate::brush::random::Group>> = None;
        section(ui, "Randomizer", false, |ui| {
            ui.label(RichText::new("Roll the dice on this brush. Highlighted groups are rolled; click one to lock it so it stays as it is.").size(11.5).color(TEXT_DIM));
            ui.horizontal_wrapped(|ui| {
                ui.spacing_mut().item_spacing = vec2(4.0, 4.0);
                for (g, label, about) in crate::brush::random::Group::ALL {
                    let on = rolled.contains(&g);
                    if widgets::chip(ui, label, on).on_hover_text(about).clicked() {
                        if on {
                            rolled.retain(|x| *x != g);
                        } else {
                            rolled.push(g);
                        }
                    }
                }
            });
            ui.horizontal(|ui| {
                if ui.button("Roll the dice").on_hover_text("Randomizes the highlighted groups (never the size or opacity)").clicked() {
                    roll = Some(rolled.clone());
                }
                if ui.small_button("All").clicked() {
                    *rolled = crate::brush::random::Group::ALL.iter().map(|g| g.0).collect();
                }
                if ui.small_button("None").clicked() {
                    rolled.clear();
                }
            });
        });
        let mut open_chart = false;
        section(ui, "Real paint", s.paint != Paint::Normal, |ui| {
            let opts: Vec<(Paint, &str)> = Paint::ALL.iter().map(|(p, l, _)| (*p, *l)).collect();
            widgets::segmented(ui, &mut s.paint, &opts);
            if let Some((_, _, about)) = Paint::ALL.iter().find(|(p, _, _)| *p == s.paint) {
                ui.label(RichText::new(*about).size(11.5).color(TEXT_DIM));
            }
            if s.paint != Paint::Normal && s.mix > 0.0 {
                ui.label(RichText::new("How the brush is loaded").size(11.5).color(TEXT_DIM));
                let opts: Vec<(Dip, &str)> = Dip::ALL.iter().map(|(p, l, _)| (*p, *l)).collect();
                widgets::segmented(ui, &mut s.dip, &opts);
                if let Some((_, _, about)) = Dip::ALL.iter().find(|(p, _, _)| *p == s.dip) {
                    ui.label(RichText::new(*about).size(11.5).color(TEXT_DIM));
                }
                if s.dip == Dip::Every {
                    num(ui, "Dip every", &mut s.dip_every, 20.0..=2000.0, "Pixels of stroke between dips in fresh paint");
                }
            } else if s.paint != Paint::Normal {
                ui.label(RichText::new("Turn Mix up in Color mixing above to make the brush carry paint along.").size(11.0).color(TEXT_DIM));
            }
            if widgets::chip(ui, "Mixing chart", false).on_hover_text("See how your swatches mix, as paint and as plain RGB").clicked() {
                open_chart = true;
            }
        });
        if open_chart {
            self.mixchart.open = true;
        }

        section(ui, "Pattern seed", false, |ui| {
            ui.horizontal(|ui| {
                ui.add(egui::DragValue::new(&mut s.seed).range(1..=99_999));
                if ui.button("New pattern").on_hover_text("Changes the random pattern of edges and grain").clicked() {
                    s.seed = (s.seed * 7919 + 13) % 99_991 + 1;
                }
            });
        });

        if let Some(groups) = roll {
            self.lib.roll_count = self.lib.roll_count.wrapping_add(1);
            let seed = self.lib.roll_count.wrapping_mul(7919).wrapping_add(self.lib.mem[sl].s.seed);
            crate::brush::random::randomize(&mut self.lib.mem[sl].s, &groups, seed);
        }
        ui.add_space(6.0);
        if let Some(p) = preset {
            if ui.button("Reset brush").on_hover_text("Back to the original settings (keeps your size)").clicked() {
                let size = self.lib.mem[sl].s.size;
                self.lib.mem[sl].s = BrushSettings { size, ..p.s.clone() };
            }
        }
        ui.add_space(12.0);
    }
}

/// A brush card: real stroke preview on a light strip, name underneath.
fn brush_card(ui: &mut Ui, p: &Preset, tex: Option<egui::TextureId>, w: f32, selected: bool) -> egui::Response {
    let (rect, resp) = ui.allocate_exact_size(vec2(w, 62.0), Sense::click());
    if !ui.is_rect_visible(rect) {
        return resp;
    }
    let painter = ui.painter();
    painter.rect_filled(rect, CornerRadius::same(8), if resp.hovered() { HOVER } else { RAISED });
    painter.rect_stroke(
        rect,
        CornerRadius::same(8),
        UiStroke::new(if selected { 1.5 } else { 1.0 }, if selected { EMERALD } else { LINE }),
        StrokeKind::Inside,
    );
    let strip = egui::Rect::from_min_max(rect.min + vec2(5.0, 5.0), pos2(rect.right() - 5.0, rect.top() + 38.0));
    painter.rect_filled(strip, CornerRadius::same(5), Color32::from_rgb(236, 240, 236));
    if let Some(t) = tex {
        painter.image(t, strip, egui::Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)), Color32::WHITE);
    }
    painter.text(
        pos2(rect.center().x, rect.bottom() - 11.0),
        Align2::CENTER_CENTER,
        p.name,
        FontId::proportional(11.5),
        if selected { VENOM } else { TEXT },
    );
    resp.on_hover_text(p.about)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every built-in brush painted on one sheet: target/screens/brush_sheet.png
    #[test]
    fn brush_sheet() {
        let all: Vec<Preset> = presets().into_iter().chain(eraser_presets()).collect();
        let (cw, ch, cols) = (300usize, 96usize, 6usize);
        let rows = all.len().div_ceil(cols);
        let (w, h) = (cw * cols, ch * rows);
        let mut img = image::RgbaImage::from_pixel(w as u32, h as u32, image::Rgba([244, 241, 234, 255]));
        let inks = [Color32::from_rgb(29, 29, 36), Color32::from_rgb(46, 150, 90), Color32::from_rgb(200, 70, 60), Color32::from_rgb(60, 90, 190)];
        for (i, p) in all.iter().enumerate() {
            let mut s = p.s.clone();
            s.size = p.s.size.clamp(6.0, 90.0);
            let pv = render_preview_sized(&s, cw, ch, inks[i % inks.len()]);
            let (ox, oy) = ((i % cols) * cw, (i / cols) * ch);
            for y in 0..ch {
                for x in 0..cw {
                    let c = pv.pixels[y * cw + x];
                    let a = c.a() as f32 / 255.0;
                    let bg = img.get_pixel((ox + x) as u32, (oy + y) as u32).0;
                    let mixc = |s: u8, b: u8| (s as f32 + b as f32 * (1.0 - a)).round().min(255.0) as u8;
                    img.put_pixel((ox + x) as u32, (oy + y) as u32, image::Rgba([mixc(c.r(), bg[0]), mixc(c.g(), bg[1]), mixc(c.b(), bg[2]), 255]));
                }
            }
        }
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("target").join("screens");
        std::fs::create_dir_all(&dir).unwrap();
        img.save(dir.join("brush_sheet.png")).unwrap();
        // names, in the same order, for reading the sheet
        let names: Vec<&str> = all.iter().map(|p| p.name).collect();
        std::fs::write(dir.join("brush_sheet.txt"), names.chunks(cols).map(|r| r.join(" | ")).collect::<Vec<_>>().join("
")).unwrap();
    }

    /// The brush designer v2 brushes and some rolled ones: target/screens/designer_sheet.png
    #[test]
    fn designer_sheet() {
        use crate::brush::random::{Group, randomize};
        let names = ["Bark", "Leopard dots", "Skin pores", "Running dry", "Color ribbon", "Speed ink", "Bamboo", "Dashed pen", "Serrated pen", "Split brush", "Worn chalk", "Real oil"];
        let mut all: Vec<(String, BrushSettings)> = names.iter().map(|n| (n.to_string(), presets().into_iter().find(|p| p.name == *n).unwrap().s)).collect();
        for seed in 0..12u32 {
            let mut s = BrushSettings { size: 40.0, ..BrushSettings::default() };
            randomize(&mut s, &Group::ALL.iter().map(|g| g.0).collect::<Vec<_>>(), seed * 31 + 7);
            all.push((format!("rolled {seed}"), s));
        }
        let (cw, ch, cols) = (420usize, 120usize, 4usize);
        let rows = all.len().div_ceil(cols);
        let mut img = image::RgbaImage::from_pixel((cw * cols) as u32, (ch * rows) as u32, image::Rgba([244, 241, 234, 255]));
        let inks = [Color32::from_rgb(29, 29, 36), Color32::from_rgb(46, 110, 70), Color32::from_rgb(170, 60, 50), Color32::from_rgb(60, 90, 190)];
        for (i, (_, s)) in all.iter().enumerate() {
            let mut s = s.clone();
            s.size = s.size.clamp(8.0, 70.0);
            let pv = render_preview_sized(&s, cw, ch, inks[i % inks.len()]);
            let (ox, oy) = ((i % cols) * cw, (i / cols) * ch);
            for y in 0..ch {
                for x in 0..cw {
                    let c = pv.pixels[y * cw + x];
                    let a = c.a() as f32 / 255.0;
                    let bg = img.get_pixel((ox + x) as u32, (oy + y) as u32).0;
                    let mixc = |s: u8, b: u8| (s as f32 + b as f32 * (1.0 - a)).round().min(255.0) as u8;
                    img.put_pixel((ox + x) as u32, (oy + y) as u32, image::Rgba([mixc(c.r(), bg[0]), mixc(c.g(), bg[1]), mixc(c.b(), bg[2]), 255]));
                }
            }
        }
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("target").join("screens");
        std::fs::create_dir_all(&dir).unwrap();
        img.save(dir.join("designer_sheet.png")).unwrap();
        std::fs::write(dir.join("designer_sheet.txt"), all.iter().map(|(n, _)| n.as_str()).collect::<Vec<_>>().chunks(cols).map(|r| r.join(" | ")).collect::<Vec<_>>().join("
")).unwrap();
    }

    /// Every hair and skin brush on a small canvas: target/screens/hair_skin_sheet.png
    #[test]
    fn hair_skin_sheet() {
        use crate::doc::Document;
        let all: Vec<Preset> = presets().into_iter().filter(|p| p.category == "Hair" || p.category == "Skin").collect();
        let (cw, ch, cols) = (420usize, 150usize, 4usize);
        let rows = all.len().div_ceil(cols);
        let mut sheet = image::RgbaImage::from_pixel((cw * cols) as u32, (ch * rows) as u32, image::Rgba([244, 241, 234, 255]));
        for (i, p) in all.iter().enumerate() {
            let mut d = Document::new(cw as u32, ch as u32);
            d.layers[1].visible = false; // transparent paper: the sheet color shows
            let blender = p.s.mix > 0.5 || p.s.keep_detail > 0.0;
            let hair = p.category == "Hair";
            if blender {
                // something to blend: bands of two tones with fine grain
                for y in 0..ch {
                    for x in 0..cw {
                        let n = crate::brush::noise::hash(x as i32, y as i32, 9);
                        let (band, base) = if hair { (y / 5 % 2 == 0, [[150.0, 100.0, 70.0], [60.0, 38.0, 28.0]]) } else { (x < cw / 2, [[232.0, 176.0, 150.0], [200.0, 150.0, 160.0]]) };
                        let c = if band { base[0] } else { base[1] };
                        let v = 1.0 + (n - 0.5) * 0.18;
                        let col = eframe::egui::Color32::from_rgb((c[0] * v).min(255.0) as u8, (c[1] * v).min(255.0) as u8, (c[2] * v).min(255.0) as u8);
                        d.layers[0].tile_mut(((x / TILE) as i32, (y / TILE) as i32))[(y % TILE) * TILE + x % TILE] = col;
                    }
                }
            }
            let color = if hair { Color32::from_rgb(78, 48, 34) } else { Color32::from_rgb(214, 150, 128) };
            let mut s = p.s.clone();
            s.size = s.size.clamp(10.0, 80.0);
            let none = |_: i32, _: i32| None;
            let mut st = Stroke::begin(&mut d, 0, &s, color, false, 1.0, true, Sample { x: 30.0, y: ch as f32 / 2.0, p: 0.4 }, &none);
            let pts: Vec<Sample> = (1..=70).map(|k| Sample { x: 30.0 + k as f32 * 5.2, y: ch as f32 / 2.0 + (k as f32 * 0.11).sin() * 28.0, p: 0.4 + 0.6 * (k as f32 / 70.0).min(1.0) }).collect();
            for c in pts.chunks(5) {
                st.add(&mut d, c, &none);
            }
            st.finish(&mut d, &none);
            let img = crate::files::flatten(&d);
            let (ox, oy) = ((i % cols) * cw, (i / cols) * ch);
            for y in 0..ch {
                for x in 0..cw {
                    let c = img.get_pixel(x as u32, y as u32).0;
                    let a = c[3] as f32 / 255.0;
                    let bg = sheet.get_pixel((ox + x) as u32, (oy + y) as u32).0;
                    let mixc = |s: u8, b: u8| (s as f32 * a + b as f32 * (1.0 - a)).round() as u8;
                    sheet.put_pixel((ox + x) as u32, (oy + y) as u32, image::Rgba([mixc(c[0], bg[0]), mixc(c[1], bg[1]), mixc(c[2], bg[2]), 255]));
                }
            }
        }
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("target").join("screens");
        std::fs::create_dir_all(&dir).unwrap();
        sheet.save(dir.join("hair_skin_sheet.png")).unwrap();
        std::fs::write(dir.join("hair_skin_sheet.txt"), all.iter().map(|p| p.name).collect::<Vec<_>>().chunks(cols).map(|r| r.join(" | ")).collect::<Vec<_>>().join("\n")).unwrap();
    }
}
