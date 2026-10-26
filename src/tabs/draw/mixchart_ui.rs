// SPDX-License-Identifier: GPL-3.0-or-later
// The mixing chart: the classic painter's test sheet, made for you. Every two colors of your
// swatches, mixed as real paint (above the diagonal) next to what plain RGB averaging gives
// (below it), so you see every combination before you commit to it.

use crate::widgets::HelpExt;
use eframe::egui::{self, Color32, CornerRadius, RichText, Sense, Stroke, StrokeKind, vec2};

use super::DrawState;
use crate::pigment;
use crate::theme::*;
use crate::widgets::{self, section_title};

#[derive(Default)]
pub struct MixChart {
    pub open: bool,
    /// The pair whose ratio strip is shown.
    pair: Option<(usize, usize)>,
}

const MAX_COLORS: usize = 8;

fn c01(c: Color32) -> [f32; 3] {
    [c.r() as f32 / 255.0, c.g() as f32 / 255.0, c.b() as f32 / 255.0]
}

fn from01(c: [f32; 3]) -> Color32 {
    let q = |v: f32| (v.clamp(0.0, 1.0) * 255.0).round() as u8;
    Color32::from_rgb(q(c[0]), q(c[1]), q(c[2]))
}

/// What plain RGB averaging gives, for comparison.
fn rgb_mix(a: Color32, b: Color32, t: f32) -> Color32 {
    let (a, b) = (c01(a), c01(b));
    from01([a[0] + (b[0] - a[0]) * t, a[1] + (b[1] - a[1]) * t, a[2] + (b[2] - a[2]) * t])
}

fn paint_mix(a: Color32, b: Color32, t: f32) -> Color32 {
    from01(pigment::mix(c01(a), c01(b), t))
}

impl DrawState {
    /// The colors the chart uses: the current color first, then the swatches.
    fn chart_colors(&self) -> Vec<Color32> {
        let mut v = vec![self.primary];
        for c in self.swatches.iter().rev() {
            if !v.contains(c) && v.len() < MAX_COLORS {
                v.push(*c);
            }
        }
        v
    }

    /// The chart as a picture: paint mixes above the diagonal, plain RGB below it.
    pub fn mix_chart_image(colors: &[Color32], cell: u32) -> image::RgbaImage {
        let n = colors.len() as u32;
        image::RgbaImage::from_fn(n * cell, n * cell, |x, y| {
            let (i, j) = ((y / cell) as usize, (x / cell) as usize);
            let c = if i == j {
                colors[i]
            } else if j > i {
                paint_mix(colors[i], colors[j], 0.5)
            } else {
                rgb_mix(colors[i], colors[j], 0.5)
            };
            image::Rgba([c.r(), c.g(), c.b(), 255])
        })
    }

    pub(super) fn mix_chart_window(&mut self, ctx: &egui::Context) {
        if !self.mixchart.open {
            return;
        }
        let colors = self.chart_colors();
        let mut open = true;
        let mut pick: Option<Color32> = None;
        let mut as_layer = false;
        egui::Window::new(RichText::new("Mixing chart").strong())
            .open(&mut open)
            .collapsible(false)
            .resizable(false)
            .default_pos(ctx.content_rect().center() - vec2(200.0, 260.0))
            .show(ctx, |ui| {
                ui.set_width(400.0);
                if colors.len() < 2 {
                    ui.label("Save a few colors to your swatches first (Save under the color picker), then open the chart again.");
                    return;
                }
                ui.label(RichText::new("Above the diagonal: mixed like paint. Below it: plain RGB, for comparison. Click a square to use that color.").size(11.5).color(TEXT_DIM));
                let n = colors.len();
                let cell = (380.0 / n as f32).min(52.0);
                let (rect, resp) = ui.allocate_exact_size(vec2(cell * n as f32, cell * n as f32), Sense::click());
                let p = ui.painter_at(rect);
                for i in 0..n {
                    for j in 0..n {
                        let r = egui::Rect::from_min_size(rect.min + vec2(j as f32 * cell, i as f32 * cell), vec2(cell, cell));
                        let c = if i == j {
                            colors[i]
                        } else if j > i {
                            paint_mix(colors[i], colors[j], 0.5)
                        } else {
                            rgb_mix(colors[i], colors[j], 0.5)
                        };
                        p.rect_filled(r, CornerRadius::ZERO, c);
                        if i == j {
                            p.rect_stroke(r.shrink(1.5), CornerRadius::ZERO, Stroke::new(1.5, Color32::WHITE.gamma_multiply(0.7)), StrokeKind::Inside);
                        }
                    }
                }
                if let (Some(pos), true) = (resp.interact_pointer_pos(), resp.clicked()) {
                    let (j, i) = (((pos.x - rect.left()) / cell) as usize, ((pos.y - rect.top()) / cell) as usize);
                    if i < n && j < n {
                        pick = Some(if i == j {
                            colors[i]
                        } else if j > i {
                            paint_mix(colors[i], colors[j], 0.5)
                        } else {
                            rgb_mix(colors[i], colors[j], 0.5)
                        });
                        self.mixchart.pair = Some((i.min(j), i.max(j)));
                    }
                }
                // the ratios of one pair
                ui.add_space(6.0);
                let (a, b) = self.mixchart.pair.filter(|(a, b)| *a < n && *b < n && a != b).unwrap_or((0, 1));
                section_title(ui, "One pair, step by step");
                ui.label(RichText::new("Click a square in the chart to choose the pair.").size(11.0).color(TEXT_DIM));
                let steps = 9usize;
                let sw = 380.0 / steps as f32;
                for (label, real) in [("Paint", true), ("RGB", false)] {
                    ui.horizontal(|ui| {
                        ui.label(RichText::new(label).size(11.5).color(TEXT_DIM));
                    });
                    let (r, resp) = ui.allocate_exact_size(vec2(sw * steps as f32, 26.0), Sense::click());
                    for k in 0..steps {
                        let t = k as f32 / (steps - 1) as f32;
                        let c = if real { paint_mix(colors[a], colors[b], t) } else { rgb_mix(colors[a], colors[b], t) };
                        ui.painter().rect_filled(egui::Rect::from_min_size(r.min + vec2(k as f32 * sw, 0.0), vec2(sw + 0.5, 26.0)), CornerRadius::ZERO, c);
                    }
                    if let (Some(pos), true) = (resp.interact_pointer_pos(), resp.clicked()) {
                        let k = (((pos.x - r.left()) / sw) as usize).min(steps - 1);
                        let t = k as f32 / (steps - 1) as f32;
                        pick = Some(if real { paint_mix(colors[a], colors[b], t) } else { rgb_mix(colors[a], colors[b], t) });
                    }
                }
                ui.add_space(6.0);
                ui.horizontal(|ui| {
                    if widgets::chip(ui, "Make it a layer", false).help("Puts the chart into your drawing as a picture, to keep or print").clicked() {
                        as_layer = true;
                    }
                });
            });
        self.mixchart.open = open;
        if let Some(c) = pick {
            self.primary = c;
        }
        if as_layer {
            let img = Self::mix_chart_image(&colors, 120);
            self.apply_external("Mixing chart", |d| d.import_picture_layer(&img, "Mixing chart"));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_chart_shows_paint_above_and_rgb_below_the_diagonal() {
        let (blue, yellow) = (Color32::from_rgb(25, 50, 217), Color32::from_rgb(250, 224, 25));
        let img = DrawState::mix_chart_image(&[blue, yellow], 10);
        assert_eq!(img.dimensions(), (20, 20));
        assert_eq!(img.get_pixel(5, 5).0[..3], [25, 50, 217], "the diagonal is the pure color");
        let (paint, rgb) = (img.get_pixel(15, 5).0, img.get_pixel(5, 15).0);
        assert!(paint[1] > paint[0] && paint[1] > paint[2], "paint: green {paint:?}");
        let chroma = |c: [u8; 4]| {
            let l = crate::oklab::rgb_to_oklab([c[0] as f32 / 255.0, c[1] as f32 / 255.0, c[2] as f32 / 255.0]);
            l[1].hypot(l[2])
        };
        assert!(chroma(paint) > chroma(rgb) * 1.4, "paint is livelier than the RGB average: {paint:?} {rgb:?}");
    }
}
