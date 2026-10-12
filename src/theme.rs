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

/// A titled card used for the dockable sections in the side panels.
pub fn card() -> egui::Frame {
    egui::Frame::new()
        .fill(CARD)
        .stroke(Stroke::new(1.0, LINE))
        .corner_radius(CornerRadius::same(10))
        .inner_margin(Margin::same(10))
}
