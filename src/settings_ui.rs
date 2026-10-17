// SPDX-License-Identifier: GPL-3.0-or-later
// The Settings window pages. Every setting here is saved and takes effect right away.

use eframe::egui::{self, Align2, Color32, CornerRadius, Event, FontId, Pos2, Rect, RichText, Sense, Shape, Stroke, StrokeKind, Ui, pos2, vec2};

use crate::settings::{CursorStyle, Settings, WheelAction};
use crate::snake;
use crate::theme::*;
use crate::widgets::{section_title, segmented};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Page {
    Interface,
    Canvas,
    Cursor,
    Pen,
    Brushes,
    Color,
    Undo,
    Files,
    Performance,
    Shortcuts,
    Info,
}

impl Page {
    pub const ALL: [(Page, &'static str); 11] = [
        (Page::Interface, "Interface"),
        (Page::Canvas, "Canvas & view"),
        (Page::Cursor, "Cursor"),
        (Page::Pen, "Pen & tablet"),
        (Page::Brushes, "Brushes"),
        (Page::Color, "Color"),
        (Page::Undo, "Undo & history"),
        (Page::Files, "Files & autosave"),
        (Page::Performance, "Performance"),
        (Page::Shortcuts, "Shortcuts"),
        (Page::Info, "Info"),
    ];
}

/// Watches pen input (Windows Ink pressure) for the Pen & tablet page.
#[derive(Default)]
pub struct PenMonitor {
    pub raw: f32,
    pub last_time: f64,
    pub events: u64,
    scribble: Vec<Vec<(Pos2, f32)>>,
}

impl PenMonitor {
    pub fn update(&mut self, ctx: &egui::Context) {
        ctx.input(|i| {
            for e in &i.events {
                if let Event::Touch { force: Some(f), .. } = e {
                    self.raw = *f;
                    self.last_time = i.time;
                    self.events += 1;
                }
            }
        });
    }

    fn seen_recently(&self, now: f64) -> bool {
        self.events > 0 && now - self.last_time < 1.0
    }
}

fn hint(ui: &mut Ui, text: &str) {
    ui.label(RichText::new(text).size(11.5).color(TEXT_DIM));
}

fn row(ui: &mut Ui, label: &str, add: impl FnOnce(&mut Ui)) {
    ui.horizontal(|ui| {
        let (r, _) = ui.allocate_exact_size(vec2(170.0, 20.0), Sense::hover());
        ui.painter().text(r.left_center(), Align2::LEFT_CENTER, label, FontId::proportional(13.0), TEXT);
        add(ui);
    });
}

fn reset_button(ui: &mut Ui) -> bool {
    ui.add_space(10.0);
    ui.button("Reset this page").on_hover_text("Back to the default values for this page").clicked()
}

pub fn page(ui: &mut Ui, page: Page, s: &mut Settings, pen: &mut PenMonitor) {
    let d = Settings::default();
    match page {
        Page::Interface => {
            section_title(ui, "Interface");
            row(ui, "UI scale", |ui| {
                ui.add(egui::Slider::new(&mut s.ui_scale, 0.6..=2.5).step_by(0.05).max_decimals(2));
            });
            hint(ui, "Makes every panel and button bigger or smaller.");
            ui.checkbox(&mut s.high_contrast, "High contrast (brighter text and outlines)");
            ui.checkbox(&mut s.reduced_motion, "Reduce motion (no sliding or fading)");
            ui.checkbox(&mut s.show_tooltips, "Show help when hovering buttons");
            ui.checkbox(&mut s.left_handed, "Left-handed layout (tools on the right, layers on the left)");
            if reset_button(ui) {
                s.ui_scale = d.ui_scale;
                s.high_contrast = d.high_contrast;
                s.reduced_motion = d.reduced_motion;
                s.show_tooltips = d.show_tooltips;
                s.left_handed = d.left_handed;
            }
        }
        Page::Canvas => {
            section_title(ui, "Mouse wheel");
            segmented(ui, &mut s.wheel, &[(WheelAction::Zoom, "Wheel zooms (CSP)"), (WheelAction::Scroll, "Wheel scrolls (Krita / Photoshop)")]);
            hint(ui, match s.wheel {
                WheelAction::Zoom => "Wheel zooms at the pointer. Alt + wheel rotates.",
                WheelAction::Scroll => "Wheel scrolls, Shift + wheel scrolls sideways, Ctrl + wheel zooms, Alt + wheel rotates.",
            });
            row(ui, "Zoom speed", |ui| {
                ui.add(egui::Slider::new(&mut s.zoom_speed, 0.2..=4.0).max_decimals(2));
            });
            ui.checkbox(&mut s.invert_wheel, "Invert wheel zoom direction");
            row(ui, "Rotation step", |ui| {
                ui.add(egui::Slider::new(&mut s.rotate_snap, 1.0..=90.0).suffix("°").max_decimals(0));
            });
            hint(ui, "Used by the rotate buttons and by Shift while rotating with R + drag.");
            ui.add_space(6.0);
            section_title(ui, "Looks");
            ui.checkbox(&mut s.show_guides, "Show the project's guide lines");
            ui.checkbox(&mut s.pixel_grid, "Pixel grid when zoomed in far");
            if s.pixel_grid {
                row(ui, "Show grid from", |ui| {
                    ui.add(egui::Slider::new(&mut s.pixel_grid_from, 4.0..=64.0).custom_formatter(|v, _| format!("{:.0}%", v * 100.0)));
                });
            }
            ui.checkbox(&mut s.smooth_zoomed_in, "Smooth pixels when zoomed in (off = crisp, see every pixel)");
            ui.checkbox(&mut s.page_shadow, "Shadow under the page");
            ui.checkbox(&mut s.show_scales, "Snake-scale pattern behind the page");
            row(ui, "Background", |ui| {
                ui.color_edit_button_srgb(&mut s.canvas_bg);
                if ui.small_button("Default").clicked() {
                    s.canvas_bg = d.canvas_bg;
                }
            });
            row(ui, "Checkerboard squares", |ui| {
                ui.add(egui::Slider::new(&mut s.checker_size, 4..=64).suffix(" px"));
            });
            hint(ui, "The checkerboard shows where the picture is transparent (hide the paper to see it).");
            if reset_button(ui) {
                // only this page's values go back to default
                let keep = s.clone();
                *s = pick_canvas(&keep, &d);
            }
        }
        Page::Cursor => {
            section_title(ui, "Brush cursor");
            segmented(
                ui,
                &mut s.cursor,
                &[(CursorStyle::Outline, "Circle"), (CursorStyle::OutlineCross, "Circle + cross"), (CursorStyle::Cross, "Cross"), (CursorStyle::Dot, "Dot")],
            );
            cursor_preview(ui, s.cursor);
            row(ui, "Smallest circle", |ui| {
                ui.add(egui::Slider::new(&mut s.cursor_min_size, 0.0..=30.0).suffix(" px"));
            });
            hint(ui, "Keeps tiny brushes visible when zoomed out.");
            ui.checkbox(&mut s.hide_cursor_while_drawing, "Hide the cursor while drawing");
            if reset_button(ui) {
                s.cursor = d.cursor;
                s.cursor_min_size = d.cursor_min_size;
                s.hide_cursor_while_drawing = d.hide_cursor_while_drawing;
            }
        }
        Page::Pen => pen_page(ui, s, pen, &d),
        Page::Brushes => {
            section_title(ui, "Brushes");
            ui.checkbox(&mut s.keep_size_on_switch, "Keep my brush size when I pick another brush");
            row(ui, "Biggest brush size", |ui| {
                ui.add(egui::Slider::new(&mut s.brush_size_max, 50.0..=5000.0).logarithmic(true).suffix(" px"));
            });
            row(ui, "[ and ] step", |ui| {
                ui.add(egui::Slider::new(&mut s.size_step, 1.02..=2.0).custom_formatter(|v, _| format!("{:.0}%", (v - 1.0) * 100.0)));
            });
            hint(ui, "How much [ and ] change the brush size each press.");
            if reset_button(ui) {
                s.keep_size_on_switch = d.keep_size_on_switch;
                s.brush_size_max = d.brush_size_max;
                s.size_step = d.size_step;
            }
        }
        Page::Color => {
            section_title(ui, "Color picker");
            row(ui, "Pick from", |ui| {
                segmented(ui, &mut s.picker_samples, &[(1, "1 pixel"), (3, "3 × 3 average"), (5, "5 × 5 average")]);
            });
            hint(ui, "Averaging ignores single noisy pixels in textured paint.");
            row(ui, "Most saved swatches", |ui| {
                ui.add(egui::Slider::new(&mut s.swatches_max, 8..=200));
            });
            if reset_button(ui) {
                s.picker_samples = d.picker_samples;
                s.swatches_max = d.swatches_max;
            }
        }
        Page::Undo => {
            section_title(ui, "Undo & history");
            row(ui, "Undo steps kept", |ui| {
                ui.add(egui::Slider::new(&mut s.undo_steps, 5..=2000).logarithmic(true));
            });
            row(ui, "Memory for undo", |ui| {
                ui.add(egui::Slider::new(&mut s.undo_memory_mb, 64..=32768).logarithmic(true).suffix(" MB"));
            });
            hint(ui, "Steps only cost memory for the parts of the picture they changed. The oldest steps go first.");
            ui.checkbox(&mut s.warn_other_tab_undo, "Ask before undoing a step that is older than changes in other tabs");
            hint(ui, "Off = Ctrl+Z quietly undoes only that step and keeps the other tabs' changes.");
            if reset_button(ui) {
                s.undo_steps = d.undo_steps;
                s.undo_memory_mb = d.undo_memory_mb;
                s.warn_other_tab_undo = d.warn_other_tab_undo;
            }
        }
        Page::Files => {
            section_title(ui, "Projects folder");
            let folder = s.projects_folder();
            ui.label(RichText::new(folder.display().to_string()).color(TEXT).monospace());
            ui.horizontal(|ui| {
                if ui.button("Change…").clicked() {
                    let _ = std::fs::create_dir_all(&folder);
                    if let Some(p) = rfd::FileDialog::new().set_title("Projects folder").set_directory(&folder).pick_folder() {
                        s.projects_dir = p.display().to_string();
                    }
                }
                if ui.button("Open in Explorer").clicked() {
                    let _ = std::fs::create_dir_all(&folder);
                    open_folder(&folder);
                }
                if !s.projects_dir.is_empty() && ui.button("Use the default").clicked() {
                    s.projects_dir.clear();
                }
            });
            hint(ui, "Where Save suggests to put projects, and what the project manager lists under My projects.");
            ui.add_space(6.0);
            section_title(ui, "Autosave");
            row(ui, "Recovery copy every", |ui| {
                ui.add(egui::Slider::new(&mut s.autosave_minutes, 0.0..=30.0).step_by(0.5).suffix(" min").max_decimals(1));
            });
            hint(ui, if s.autosave_minutes <= 0.0 { "Off: no recovery copies are made." } else { "Only while there are new changes, and never in the middle of a stroke." });
            row(ui, "Keep the newest", |ui| {
                ui.add(egui::Slider::new(&mut s.recovery_copies, 1..=50).suffix(" copies"));
            });
            ui.checkbox(&mut s.autosave_project_file, "Also save the project file itself on autosave");
            hint(ui, "Off = your project file only changes when you press Save; the copies are kept separately.");
            if ui.button("Open the recovery folder").clicked() {
                let f = Settings::recovery_folder();
                let _ = std::fs::create_dir_all(&f);
                open_folder(&f);
            }
            ui.add_space(6.0);
            section_title(ui, "Export & start");
            row(ui, "JPEG quality", |ui| {
                ui.add(egui::Slider::new(&mut s.jpeg_quality, 40..=100));
            });
            hint(ui, "Higher = sharper and bigger files. PNG and WebP are saved without any loss.");
            ui.checkbox(&mut s.export_credit, "Note \"made with SeviathStudio\" in exported PNG files");
            hint(ui, "Written as the software used, never as the author — the art is yours.");
            ui.checkbox(&mut s.start_with_projects, "Show the project manager when the app starts");
            ui.horizontal(|ui| {
                ui.label(RichText::new(format!("{} recent files remembered", s.recent.len())).color(TEXT_DIM));
                if !s.recent.is_empty() && ui.small_button("Clear list").clicked() {
                    s.recent.clear();
                }
            });
            if reset_button(ui) {
                s.projects_dir = d.projects_dir.clone();
                s.autosave_minutes = d.autosave_minutes;
                s.recovery_copies = d.recovery_copies;
                s.autosave_project_file = d.autosave_project_file;
                s.jpeg_quality = d.jpeg_quality;
                s.start_with_projects = d.start_with_projects;
                s.export_credit = d.export_credit;
            }
        }
        Page::Performance => {
            section_title(ui, "Performance");
            row(ui, "Brush previews per frame", |ui| {
                ui.add(egui::Slider::new(&mut s.brush_preview_per_frame, 1..=20));
            });
            hint(ui, "Lower = smoother while opening big brush categories on slow computers.");
            row(ui, "Navigator updates", |ui| {
                ui.add(egui::Slider::new(&mut s.navigator_fps, 1.0..=30.0).suffix(" / s").max_decimals(0));
            });
            hint(ui, "How often the Sub view thumbnail follows your drawing.");
            if reset_button(ui) {
                s.brush_preview_per_frame = d.brush_preview_per_frame;
                s.navigator_fps = d.navigator_fps;
            }
        }
        Page::Shortcuts => shortcuts(ui, s),
        Page::Info => info(ui),
    }
}

/// The canvas page's values from `d`, everything else from `keep`.
fn pick_canvas(keep: &Settings, d: &Settings) -> Settings {
    Settings {
        wheel: d.wheel,
        zoom_speed: d.zoom_speed,
        invert_wheel: d.invert_wheel,
        rotate_snap: d.rotate_snap,
        pixel_grid: d.pixel_grid,
        show_guides: d.show_guides,
        pixel_grid_from: d.pixel_grid_from,
        smooth_zoomed_in: d.smooth_zoomed_in,
        canvas_bg: d.canvas_bg,
        show_scales: d.show_scales,
        checker_size: d.checker_size,
        page_shadow: d.page_shadow,
        ..keep.clone()
    }
}

fn cursor_preview(ui: &mut Ui, style: CursorStyle) {
    let (r, _) = ui.allocate_exact_size(vec2(220.0, 54.0), Sense::hover());
    let p = ui.painter();
    p.rect_filled(r, CornerRadius::same(6), Color32::from_rgb(236, 240, 236));
    let c = r.center();
    let dark = Stroke::new(2.2, Color32::from_black_alpha(150));
    let light = Stroke::new(1.0, Color32::from_white_alpha(230));
    if matches!(style, CursorStyle::Outline | CursorStyle::OutlineCross) {
        p.circle_stroke(c, 18.0, dark);
        p.circle_stroke(c, 18.0, light);
    }
    if matches!(style, CursorStyle::OutlineCross | CursorStyle::Cross) {
        for (a, b) in [(vec2(-5.0, 0.0), vec2(5.0, 0.0)), (vec2(0.0, -5.0), vec2(0.0, 5.0))] {
            p.line_segment([c + a, c + b], dark);
            p.line_segment([c + a, c + b], light);
        }
    }
    if style == CursorStyle::Dot {
        p.circle_filled(c, 2.0, Color32::from_black_alpha(160));
        p.circle_filled(c, 1.2, Color32::WHITE);
    }
}

fn pen_page(ui: &mut Ui, s: &mut Settings, pen: &mut PenMonitor, d: &Settings) {
    let now = ui.input(|i| i.time);
    section_title(ui, "Pen status");
    let live = pen.seen_recently(now);
    let (dot, text) = if live {
        (EMERALD, format!("Pen connected — pressure {:.0}% from the pen, {:.0}% after your curve", pen.raw * 100.0, s.pressure(pen.raw) * 100.0))
    } else if pen.events > 0 {
        (GOLD, "Pen worked before — touch the test area to check again.".to_owned())
    } else {
        (Color32::from_rgb(226, 120, 80), "No pen pressure yet — draw in the test area below with your pen.".to_owned())
    };
    ui.horizontal(|ui| {
        let (r, _) = ui.allocate_exact_size(vec2(12.0, 12.0), Sense::hover());
        ui.painter().circle_filled(r.center(), 5.0, dot);
        ui.label(RichText::new(text).color(TEXT));
    });
    // live pressure bars
    for (label, v) in [("Raw", pen.raw), ("Used", s.pressure(pen.raw))] {
        ui.horizontal(|ui| {
            ui.label(RichText::new(label).size(12.0).color(TEXT_DIM));
            let (r, _) = ui.allocate_exact_size(vec2(260.0, 10.0), Sense::hover());
            ui.painter().rect_filled(r, CornerRadius::same(5), RAISED);
            let mut f = r;
            f.set_width(r.width() * if live { v } else { 0.0 });
            ui.painter().rect_filled(f, CornerRadius::same(5), EMERALD);
        });
    }
    hint(ui, "SeviathStudio reads pressure through Windows Ink. If the bars don't move, turn on \"Windows Ink\" in your tablet driver (Huion, Wacom, XP-Pen…).");
    if live {
        ui.ctx().request_repaint();
    }

    ui.add_space(6.0);
    section_title(ui, "Test area");
    let (area, resp) = ui.allocate_exact_size(vec2(ui.available_width().min(460.0), 120.0), Sense::drag());
    let p = ui.painter_at(area);
    p.rect_filled(area, CornerRadius::same(8), Color32::from_rgb(244, 241, 234));
    if resp.drag_started() {
        pen.scribble.push(Vec::new());
        if pen.scribble.len() > 12 {
            pen.scribble.remove(0);
        }
    }
    if resp.dragged() {
        let pr = if pen.seen_recently(now) { s.pressure(pen.raw) } else { s.mouse_pressure };
        if let (Some(pos), Some(line)) = (resp.interact_pointer_pos(), pen.scribble.last_mut()) {
            line.push((pos - area.min.to_vec2(), pr));
        }
    }
    for line in &pen.scribble {
        for w in line.windows(2) {
            let (a, b) = (w[0], w[1]);
            let width = 1.0 + (a.1 + b.1) * 0.5 * 14.0;
            p.line_segment([area.min + a.0.to_vec2(), area.min + b.0.to_vec2()], Stroke::new(width, Color32::from_rgb(29, 29, 36)));
            p.circle_filled(area.min + b.0.to_vec2(), width / 2.0, Color32::from_rgb(29, 29, 36));
        }
    }
    if pen.scribble.is_empty() {
        p.text(area.center(), Align2::CENTER_CENTER, "Draw here, from light to heavy pressure", FontId::proportional(13.0), Color32::from_gray(140));
    }
    if ui.small_button("Clear test area").clicked() {
        pen.scribble.clear();
    }

    ui.add_space(6.0);
    section_title(ui, "Pressure");
    ui.horizontal(|ui| {
        ui.label("Feel:");
        for (name, g) in [("Very soft", 0.45), ("Soft", 0.7), ("Normal", 1.0), ("Firm", 1.5), ("Very firm", 2.2)] {
            if ui.selectable_label((s.pressure_curve - g).abs() < 0.01, name).clicked() {
                s.pressure_curve = g;
            }
        }
    });
    row(ui, "Curve", |ui| {
        ui.add(egui::Slider::new(&mut s.pressure_curve, 0.2..=5.0).logarithmic(true).max_decimals(2));
    });
    curve_graph(ui, s);
    hint(ui, "Soft = light pressure already gives thick, dark lines. Firm = you need to press harder.");
    row(ui, "Ignore below", |ui| {
        ui.add(egui::Slider::new(&mut s.pressure_min, 0.0..=0.9).custom_formatter(|v, _| format!("{:.0}%", v * 100.0)));
    });
    row(ui, "Full pressure at", |ui| {
        ui.add(egui::Slider::new(&mut s.pressure_max, 0.1..=1.0).custom_formatter(|v, _| format!("{:.0}%", v * 100.0)));
    });
    if s.pressure_max <= s.pressure_min + 0.05 {
        s.pressure_max = (s.pressure_min + 0.05).min(1.0);
    }
    row(ui, "Pressure smoothing", |ui| {
        ui.add(egui::Slider::new(&mut s.pressure_smoothing, 0.0..=0.95).custom_formatter(|v, _| format!("{:.0}%", v * 100.0)));
    });
    hint(ui, "Calms jumpy pressure from older or noisy tablets.");
    row(ui, "Mouse counts as", |ui| {
        ui.add(egui::Slider::new(&mut s.mouse_pressure, 0.05..=1.0).custom_formatter(|v, _| format!("{:.0}% pressure", v * 100.0)));
    });

    ui.add_space(6.0);
    section_title(ui, "Steady hands");
    row(ui, "Tremor help", |ui| {
        ui.add(egui::Slider::new(&mut s.tremor_help, 0.0..=1.0).custom_formatter(|v, _| format!("{:.0}%", v * 100.0)));
    });
    hint(ui, "Extra steadying on top of every brush's own stabilizer (accessibility).");
    row(ui, "Start stroke after moving", |ui| {
        ui.add(egui::Slider::new(&mut s.min_stroke_move, 0.0..=20.0).suffix(" px"));
    });
    hint(ui, "Stops accidental dots when the pen just touches the tablet.");
    if reset_button(ui) {
        s.pressure_curve = d.pressure_curve;
        s.pressure_min = d.pressure_min;
        s.pressure_max = d.pressure_max;
        s.mouse_pressure = d.mouse_pressure;
        s.pressure_smoothing = d.pressure_smoothing;
        s.tremor_help = d.tremor_help;
        s.min_stroke_move = d.min_stroke_move;
    }
}

fn curve_graph(ui: &mut Ui, s: &Settings) {
    let (r, _) = ui.allocate_exact_size(vec2(160.0, 110.0), Sense::hover());
    let p = ui.painter();
    p.rect_filled(r, CornerRadius::same(6), BG);
    p.rect_stroke(r, CornerRadius::same(6), Stroke::new(1.0, LINE), StrokeKind::Inside);
    let inner = r.shrink(8.0);
    p.line_segment([inner.left_bottom(), inner.right_top()], Stroke::new(1.0, LINE));
    let pts: Vec<Pos2> = (0..=40)
        .map(|i| {
            let t = i as f32 / 40.0;
            pos2(inner.left() + t * inner.width(), inner.bottom() - s.pressure(t) * inner.height())
        })
        .collect();
    p.add(Shape::line(pts, Stroke::new(2.0, VENOM)));
    p.text(inner.left_bottom() + vec2(0.0, 0.0), Align2::LEFT_BOTTOM, "pen", FontId::proportional(9.5), TEXT_DIM);
    p.text(inner.left_top(), Align2::LEFT_TOP, "brush", FontId::proportional(9.5), TEXT_DIM);
}

pub fn open_folder(dir: &std::path::Path) {
    #[cfg(windows)]
    let cmd = "explorer";
    #[cfg(not(windows))]
    let cmd = "xdg-open";
    let _ = std::process::Command::new(cmd).arg(dir).spawn();
}

/// The keyboard page: every action of the action table with its key. Click a key, press the new one.
fn shortcuts(ui: &mut Ui, s: &mut Settings) {
    use crate::actions::{self, ACTIONS, Shortcut};
    type Capture = (String, Option<Shortcut>);
    let cap_id = egui::Id::new("shortcut_capture");
    let mut cap: Option<Capture> = ui.data(|d| d.get_temp::<Option<Capture>>(cap_id)).flatten();
    let mut note = String::new();
    // the key that was pressed while listening
    if let Some((id, pending)) = cap.clone() {
        let pressed = ui.input(|i| {
            i.events.iter().find_map(|e| match e {
                egui::Event::Key { key, pressed: true, modifiers, .. } => Some((*key, *modifiers)),
                _ => None,
            })
        });
        if let Some((key, mods)) = pressed {
            ui.input_mut(|i| i.events.retain(|e| !matches!(e, egui::Event::Key { .. })));
            if key == egui::Key::Escape {
                cap = None;
            } else if (key == egui::Key::Backspace || key == egui::Key::Delete) && mods.is_none() {
                actions::assign(&mut s.shortcuts, &id, None);
                cap = None;
            } else {
                let sc = Shortcut { mods, key };
                let others = actions::conflicts(&s.shortcuts, &id, sc);
                if !others.is_empty() && pending != Some(sc) {
                    note = format!("{} is used by {}. Press it again to move it here, Esc to cancel.", sc.text(), others.iter().map(|d| d.name).collect::<Vec<_>>().join(", "));
                    cap = Some((id, Some(sc)));
                } else {
                    actions::assign(&mut s.shortcuts, &id, Some(sc));
                    cap = None;
                }
            }
        }
    }
    section_title(ui, "Keyboard");
    ui.horizontal(|ui| {
        hint(ui, "Click a key, then press the new one. Backspace takes the key away. Ctrl+K opens the command palette.");
    });
    if !note.is_empty() {
        ui.label(RichText::new(&note).color(Color32::from_rgb(235, 170, 80)).size(12.0));
    } else if let Some((_, Some(sc))) = &cap {
        ui.label(RichText::new(format!("{} is taken. Press it again to move it here, Esc to cancel.", sc.text())).color(Color32::from_rgb(235, 170, 80)).size(12.0));
    }
    egui::ScrollArea::vertical().max_height(400.0).show(ui, |ui| {
        let mut group = "";
        for d in ACTIONS {
            if d.group != group {
                group = d.group;
                ui.add_space(4.0);
                ui.label(RichText::new(group).strong().color(TEXT_DIM));
            }
            ui.horizontal(|ui| {
                let (r, _) = ui.allocate_exact_size(vec2(230.0, 20.0), Sense::hover());
                ui.painter().text(r.left_center(), Align2::LEFT_CENTER, d.name, FontId::proportional(13.0), TEXT);
                let listening = cap.as_ref().is_some_and(|c| c.0 == d.id);
                let key = actions::effective_text(&s.shortcuts, d.id);
                let label = if listening { "Press keys…".to_owned() } else if key.is_empty() { "none".to_owned() } else { key };
                if ui.add(egui::Button::new(RichText::new(label).monospace().color(if listening { Color32::from_rgb(235, 170, 80) } else { VENOM })).min_size(vec2(130.0, 20.0))).clicked() {
                    cap = if listening { None } else { Some((d.id.to_owned(), None)) };
                }
                if s.shortcuts.contains_key(d.id) && ui.small_button("Reset").clicked() {
                    s.shortcuts.remove(d.id);
                }
            });
        }
    });
    ui.add_space(6.0);
    ui.horizontal(|ui| {
        if ui.add_enabled(!s.shortcuts.is_empty(), egui::Button::new("Reset all keys")).clicked() {
            s.shortcuts.clear();
            cap = None;
        }
    });
    ui.add_space(4.0);
    section_title(ui, "Mouse and pen");
    egui::Grid::new("keys_mouse").striped(true).spacing(vec2(24.0, 4.0)).show(ui, |ui| {
        for (k, what) in [
            ("Ctrl + click  ·  Shift + click on layers", "Pick several layers  ·  pick a range"),
            ("Shift / Alt while selecting", "Add to / take away from the selection"),
            ("Shift + click", "Straight line from the last stroke"),
            ("Shift+G, drag  ·  Enter  ·  Esc", "Gradient: lay out  ·  apply  ·  cancel"),
            ("P, hold the pen still", "Vector pen: straighten a drawn shape"),
            ("Delete  ·  Esc (Edit lines)", "Remove the node or line  ·  deselect"),
            ("Enter  ·  Esc  ·  arrows", "Apply  ·  cancel  ·  nudge a floating picture"),
            ("Space + drag  ·  middle drag", "Move the view"),
            ("R + drag (Shift snaps)", "Rotate the view"),
            ("Wheel  ·  Alt + wheel", "Zoom  ·  rotate"),
        ] {
            ui.label(RichText::new(k).monospace().color(VENOM));
            ui.label(what);
            ui.end_row();
        }
    });
    ui.data_mut(|d| d.insert_temp(cap_id, cap));
}

/// Graphics adapter name and backend, set once at startup (shown on the Info page for support).
pub static GPU_INFO: std::sync::OnceLock<String> = std::sync::OnceLock::new();

const REPO: &str = "https://github.com/santamadaraneilande-ctrl/seviathstudio";

fn link_row(ui: &mut Ui, label: &str, text: &str, url: &str) {
    ui.horizontal(|ui| {
        ui.label(RichText::new(label).color(TEXT_DIM));
        ui.hyperlink_to(text, url);
    });
}

fn info(ui: &mut Ui) {
    // App
    ui.horizontal(|ui| {
        let (r, _) = ui.allocate_exact_size(vec2(64.0, 64.0), Sense::hover());
        snake::logo(ui.painter(), r.center(), 64.0);
        ui.vertical(|ui| {
            ui.label(RichText::new("SeviathStudio").size(22.0).strong());
            ui.label(RichText::new(format!("Version {} · built {} · native Rust", crate::app::VERSION, env!("SEVIATH_BUILD_DATE"))).color(TEXT_DIM));
            ui.label("Draw, rig, animate and stream a 2D character from one file, with reliable PSD export.");
        });
    });
    ui.add_space(6.0);

    // Creator
    section_title(ui, "Creator");
    ui.label(RichText::new("Created by Seviath").size(16.0).strong().color(VENOM));
    ui.horizontal(|ui| {
        if ui.button("Seviath on X").on_hover_text("https://x.com/SnakeSeviath").clicked() {
            ui.ctx().open_url(egui::OpenUrl::new_tab("https://x.com/SnakeSeviath"));
        }
        ui.hyperlink_to("https://x.com/SnakeSeviath", "https://x.com/SnakeSeviath");
    });
    ui.add_space(6.0);

    // License
    section_title(ui, "License");
    ui.label("Free software under the GNU GPL v3 or later: anyone may run, study, change and share it,");
    ui.label("and shared changes must stay open under the same license. Brushes and art keep their own licenses.");
    link_row(ui, "Full text:", "gnu.org/licenses/gpl-3.0", "https://www.gnu.org/licenses/gpl-3.0.html");
    ui.add_space(6.0);

    // Links
    section_title(ui, "Links");
    link_row(ui, "Source code:", "GitHub", REPO);
    link_row(ui, "Report a problem:", "Issues", &format!("{REPO}/issues"));
    link_row(ui, "What changed:", "Commits", &format!("{REPO}/commits/main"));
    ui.add_space(6.0);

    // Privacy
    section_title(ui, "Privacy");
    ui.label("Your projects, brushes and settings stay on this computer. SeviathStudio sends nothing anywhere:");
    ui.label("no tracking, no account needed. Accounts, the library and the shop come later and will be optional.");
    ui.add_space(6.0);

    // Credits
    section_title(ui, "Built with");
    for (name, lic) in [
        ("egui / eframe — user interface", "MIT or Apache-2.0"),
        ("wgpu — graphics", "MIT or Apache-2.0"),
        ("winit — windows and pen input", "Apache-2.0"),
        ("rayon — using all CPU cores", "MIT or Apache-2.0"),
        ("image, png — pictures", "MIT or Apache-2.0"),
        ("zip, quick-xml — project, OpenRaster and Krita files", "MIT / Apache-2.0"),
        ("rfd — open and save dialogs", "MIT"),
        ("trash — Recycle Bin", "MIT"),
        ("chrono — dates", "MIT or Apache-2.0"),
        ("serde, serde_json, dirs — settings", "MIT or Apache-2.0"),
        ("Ubuntu font", "Ubuntu Font Licence 1.0"),
        ("Noto Emoji font", "SIL Open Font License 1.1"),
    ] {
        ui.horizontal(|ui| {
            ui.label(RichText::new("•").color(GOLD));
            ui.label(name);
            ui.label(RichText::new(lic).color(TEXT_DIM).size(12.0));
        });
    }
    ui.add_space(6.0);

    // Debug
    section_title(ui, "For support");
    let gpu = GPU_INFO.get().map(String::as_str).unwrap_or("unknown");
    let report = format!(
        "SeviathStudio {} (built {}) · {} {} · graphics: {gpu}",
        crate::app::VERSION,
        env!("SEVIATH_BUILD_DATE"),
        std::env::consts::OS,
        std::env::consts::ARCH
    );
    ui.label(RichText::new(&report).monospace().size(12.0).color(TEXT_DIM));
    if ui.button("Copy to clipboard").on_hover_text("Paste this when you report a problem").clicked() {
        ui.ctx().copy_text(report);
    }
}

/// Settings window frame size helper (used by the app).
pub fn window_rect(ctx: &egui::Context) -> Rect {
    Rect::from_center_size(ctx.content_rect().center(), vec2(760.0, 560.0))
}
