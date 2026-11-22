// SPDX-License-Identifier: GPL-3.0-or-later
// The Seviath look: night-jungle greens, emerald accents, venom-lime highlights and gold scales.

use eframe::egui::{self, Color32, CornerRadius, Margin, Shadow, Stroke, Theme, Vec2};

pub const BG: Color32 = Color32::from_rgb(10, 14, 12); // behind the canvas
pub const PANEL: Color32 = Color32::from_rgb(17, 23, 20);
pub const CARD: Color32 = Color32::from_rgb(23, 31, 27);
pub const RAISED: Color32 = Color32::from_rgb(31, 42, 36);
pub const HOVER: Color32 = Color32::from_rgb(40, 55, 47);
pub const LINE: Color32 = Color32::from_rgb(42, 56, 48);
pub const TEXT: Color32 = Color32::from_rgb(226, 236, 229);
pub const TEXT_DIM: Color32 = Color32::from_rgb(136, 157, 145);
pub const EMERALD: Color32 = Color32::from_rgb(46, 191, 113);
pub const EMERALD_DEEP: Color32 = Color32::from_rgb(22, 104, 64);
pub const VENOM: Color32 = Color32::from_rgb(160, 228, 96);
pub const GOLD: Color32 = Color32::from_rgb(214, 168, 76);
pub const ON_ACCENT: Color32 = Color32::from_rgb(8, 22, 14);

pub const CORNER: u8 = 7;

pub fn apply(ctx: &egui::Context) {
    ctx.set_theme(Theme::Dark);

    let mut v = egui::Visuals::dark();
    v.panel_fill = PANEL;
    v.window_fill = CARD;
    v.extreme_bg_color = BG;
    v.faint_bg_color = CARD;
    v.code_bg_color = RAISED;
    v.window_stroke = Stroke::new(1.0, LINE);
    v.window_corner_radius = CornerRadius::same(12);
    v.menu_corner_radius = CornerRadius::same(10);
    v.window_shadow = Shadow { offset: [0, 8], blur: 24, spread: 0, color: Color32::from_black_alpha(140) };
    v.popup_shadow = Shadow { offset: [0, 6], blur: 16, spread: 0, color: Color32::from_black_alpha(120) };
    v.hyperlink_color = VENOM;
    v.selection.bg_fill = EMERALD_DEEP;
    v.selection.stroke = Stroke::new(1.0, VENOM);
    v.override_text_color = None;

    let corner = CornerRadius::same(CORNER);
    let w = &mut v.widgets;
    w.noninteractive.bg_fill = CARD;
    w.noninteractive.weak_bg_fill = CARD;
    w.noninteractive.bg_stroke = Stroke::new(1.0, LINE);
    w.noninteractive.fg_stroke = Stroke::new(1.0, TEXT);
    w.noninteractive.corner_radius = corner;

    w.inactive.bg_fill = RAISED;
    w.inactive.weak_bg_fill = RAISED;
    w.inactive.bg_stroke = Stroke::NONE;
    w.inactive.fg_stroke = Stroke::new(1.0, TEXT);
    w.inactive.corner_radius = corner;

    w.hovered.bg_fill = HOVER;
    w.hovered.weak_bg_fill = HOVER;
    w.hovered.bg_stroke = Stroke::new(1.0, EMERALD);
    w.hovered.fg_stroke = Stroke::new(1.5, TEXT);
    w.hovered.corner_radius = corner;
    w.hovered.expansion = 0.0;

    w.active.bg_fill = EMERALD;
    w.active.weak_bg_fill = EMERALD;
    w.active.bg_stroke = Stroke::new(1.0, VENOM);
    w.active.fg_stroke = Stroke::new(1.5, TEXT); // also egui's "strong" text color
    w.active.corner_radius = corner;
    w.active.expansion = 0.0;

    w.open.bg_fill = HOVER;
    w.open.weak_bg_fill = HOVER;
    w.open.bg_stroke = Stroke::new(1.0, EMERALD);
    w.open.fg_stroke = Stroke::new(1.0, TEXT);
    w.open.corner_radius = corner;

    ctx.set_visuals_of(Theme::Dark, v);
    ctx.all_styles_mut(|s| {
        s.spacing.item_spacing = Vec2::new(8.0, 6.0);
        s.spacing.button_padding = Vec2::new(10.0, 5.0);
        s.spacing.window_margin = Margin::same(12);
        s.spacing.menu_margin = Margin::same(8);
        s.spacing.interact_size = Vec2::new(28.0, 24.0);
        s.spacing.slider_width = 150.0;
    });
}

/// Interface preferences from Settings: high contrast, motion, tooltips, scale.
/// The font definitions for the interface: the ones that come with egui, and when `path` is a font file, that font first, so its letters are used and the
/// built-in font fills in what it has not got. Returns what went wrong when the file cannot be used (the built-in fonts are then kept).
pub fn font_definitions(path: &str) -> (egui::FontDefinitions, Option<String>) {
    let mut defs = egui::FontDefinitions::default();
    if path.trim().is_empty() {
        return (defs, None);
    }
    let bytes = match std::fs::metadata(path) {
        Ok(m) if m.len() as usize > crate::fonts::MAX_FONT_BYTES => return (defs, Some("This font file is too big.".into())),
        Ok(_) => std::fs::read(path),
        Err(e) => Err(e),
    };
    match bytes {
        Ok(b) if is_font_file(&b) => {
            defs.font_data.insert("UserInterfaceFont".into(), std::sync::Arc::new(egui::FontData::from_owned(b)));
            for family in [egui::FontFamily::Proportional, egui::FontFamily::Monospace] {
                defs.families.entry(family).or_default().insert(0, "UserInterfaceFont".into());
            }
            (defs, None)
        }
        Ok(_) => (defs, Some("This is not a font file I can read (.ttf or .otf).".into())),
        Err(e) => (defs, Some(format!("Could not read the font: {e}"))),
    }
}

/// Is this a font file (a TrueType or OpenType header)?
fn is_font_file(b: &[u8]) -> bool {
    matches!(b.get(..4), Some([0, 1, 0, 0]) | Some(b"OTTO") | Some(b"true") | Some(b"ttcf"))
}

pub fn apply_prefs(ctx: &egui::Context, set: &crate::settings::Settings) {
    ctx.set_fonts(font_definitions(&set.interface_font).0);
    apply(ctx);
    if set.high_contrast {
        ctx.style_mut_of(Theme::Dark, |s| {
            let v = &mut s.visuals;
            v.override_text_color = Some(Color32::WHITE);
            v.window_stroke = Stroke::new(1.5, TEXT);
            v.widgets.noninteractive.bg_stroke = Stroke::new(1.0, Color32::from_gray(140));
            v.widgets.inactive.bg_stroke = Stroke::new(1.0, Color32::from_gray(150));
            v.widgets.inactive.bg_fill = Color32::from_gray(30);
            v.widgets.inactive.weak_bg_fill = Color32::from_gray(30);
            v.widgets.hovered.bg_stroke = Stroke::new(2.0, VENOM);
            v.selection.stroke = Stroke::new(2.0, Color32::WHITE);
            v.panel_fill = Color32::from_gray(8);
            v.window_fill = Color32::from_gray(14);
        });
    }
    ctx.all_styles_mut(|s| {
        s.animation_time = if set.reduced_motion { 0.0 } else { 1.0 / 12.0 };
        s.interaction.tooltip_delay = if set.show_tooltips { 0.5 } else { 1.0e9 };
    });
    ctx.set_zoom_factor(set.ui_scale);
}

/// A titled card used for the dockable sections in the side panels.
pub fn card() -> egui::Frame {
    egui::Frame::new()
        .fill(CARD)
        .stroke(Stroke::new(1.0, LINE))
        .corner_radius(CornerRadius::same(10))
        .inner_margin(Margin::same(10))
}

#[cfg(test)]
mod font_tests {
    use super::*;

    #[test]
    fn a_font_file_goes_first_and_a_bad_one_changes_nothing() {
        let (plain, err) = font_definitions("");
        assert!(err.is_none() && !plain.font_data.contains_key("UserInterfaceFont"));
        let (_, err) = font_definitions("C:/does/not/exist.ttf");
        assert!(err.unwrap().contains("Could not read"));
        let dir = std::env::temp_dir().join(format!("seviath_font_{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let junk = dir.join("junk.ttf");
        std::fs::write(&junk, b"not a font at all").unwrap();
        let (defs, err) = font_definitions(junk.to_str().unwrap());
        assert!(err.unwrap().contains("not a font file") && !defs.font_data.contains_key("UserInterfaceFont"));
        // a real font: the one that comes with egui, written out as a file
        let bundled = egui::FontDefinitions::default().font_data.get("Ubuntu-Light").map(|d| d.font.to_vec()).unwrap();
        let ok = dir.join("ok.ttf");
        std::fs::write(&ok, bundled).unwrap();
        let (defs, err) = font_definitions(ok.to_str().unwrap());
        assert!(err.is_none());
        assert_eq!(defs.families[&egui::FontFamily::Proportional][0], "UserInterfaceFont", "it is used first, the built-in ones fill in");
        assert!(defs.families[&egui::FontFamily::Proportional].len() > 1);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
