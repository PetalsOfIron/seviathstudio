// SPDX-License-Identifier: GPL-3.0-or-later
// Check views: look at your drawing in black and white (are the values readable?), inverted, or
// the way people with color blindness see it. Only the display changes; the drawing is untouched.

use eframe::egui::Color32;

use crate::oklab::{to_linear, to_srgb};

#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum CheckView {
    #[default]
    Normal,
    Gray,
    Invert,
    /// no red cones
    Protanopia,
    /// no green cones
    Deuteranopia,
    /// no blue cones
    Tritanopia,
}

impl CheckView {
    pub const ALL: [(CheckView, &'static str, &'static str); 6] = [
        (CheckView::Normal, "Normal", "The drawing as it is."),
        (CheckView::Gray, "Black & white", "Are the lights and darks readable without color?"),
        (CheckView::Invert, "Inverted", "A negative view: mistakes in shapes and values show up."),
        (CheckView::Protanopia, "No red", "Protanopia: red looks dark and brownish."),
        (CheckView::Deuteranopia, "No green", "Deuteranopia: the most common color blindness."),
        (CheckView::Tritanopia, "No blue", "Tritanopia: blue and yellow get confused."),
    ];

    /// Rows of a 3×3 matrix on linear colors (Machado et al. 2009, full severity).
    fn matrix(self) -> Option<[[f32; 3]; 3]> {
        Some(match self {
            CheckView::Protanopia => [[0.152_286, 1.052_583, -0.204_868], [0.114_503, 0.786_281, 0.099_216], [-0.003_882, -0.048_116, 1.051_998]],
            CheckView::Deuteranopia => [[0.367_322, 0.860_646, -0.227_968], [0.280_085, 0.672_501, 0.047_413], [-0.011_820, 0.042_940, 0.968_881]],
            CheckView::Tritanopia => [[1.255_528, -0.076_749, -0.178_779], [-0.078_411, 0.930_809, 0.147_602], [0.004_733, 0.691_367, 0.303_900]],
            _ => return None,
        })
    }

    /// The color as this view shows it (premultiplied in, premultiplied out).
    pub fn apply(self, c: Color32) -> Color32 {
        if self == CheckView::Normal || c.a() == 0 {
            return c;
        }
        let a = c.a() as f32 / 255.0;
        let un = [c.r() as f32 / 255.0 / a, c.g() as f32 / 255.0 / a, c.b() as f32 / 255.0 / a];
        let out = match self {
            CheckView::Normal => un,
            CheckView::Invert => [1.0 - un[0], 1.0 - un[1], 1.0 - un[2]],
            CheckView::Gray => {
                let lin = [to_linear(un[0].min(1.0)), to_linear(un[1].min(1.0)), to_linear(un[2].min(1.0))];
                let y = to_srgb(0.2126 * lin[0] + 0.7152 * lin[1] + 0.0722 * lin[2]);
                [y, y, y]
            }
            _ => {
                let m = self.matrix().unwrap();
                let lin = [to_linear(un[0].min(1.0)), to_linear(un[1].min(1.0)), to_linear(un[2].min(1.0))];
                let f = |r: &[f32; 3]| to_srgb((r[0] * lin[0] + r[1] * lin[1] + r[2] * lin[2]).clamp(0.0, 1.0));
                [f(&m[0]), f(&m[1]), f(&m[2])]
            }
        };
        let q = |v: f32| (v.clamp(0.0, 1.0) * a * 255.0).round() as u8;
        Color32::from_rgba_premultiplied(q(out[0]), q(out[1]), q(out[2]), c.a())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn views_change_the_display_not_the_alpha() {
        let red = Color32::from_rgb(220, 30, 30);
        assert_eq!(CheckView::Normal.apply(red), red);
        let g = CheckView::Gray.apply(red);
        assert!(g.r() == g.g() && g.g() == g.b() && g.a() == 255, "{g:?}");
        assert_eq!(CheckView::Invert.apply(red), Color32::from_rgb(35, 225, 225));
        // red turns dark brown/olive without red cones, and still opaque
        let p = CheckView::Protanopia.apply(red);
        assert!(p.r() < red.r() && p.a() == 255, "{p:?}");
        // white and black stay (nearly) themselves in the simulations
        for v in [CheckView::Protanopia, CheckView::Deuteranopia, CheckView::Tritanopia] {
            let w = v.apply(Color32::WHITE);
            assert!(w.r() > 245 && w.g() > 245 && w.b() > 245, "{v:?} {w:?}");
            assert_eq!(v.apply(Color32::BLACK), Color32::BLACK);
        }
        assert_eq!(CheckView::Gray.apply(Color32::TRANSPARENT), Color32::TRANSPARENT);
        let half = Color32::from_rgba_premultiplied(100, 15, 15, 128);
        assert_eq!(CheckView::Gray.apply(half).a(), 128);
    }

    #[test]
    fn green_and_red_get_confused_by_deuteranopia() {
        let (r, g) = (Color32::from_rgb(200, 60, 40), Color32::from_rgb(90, 140, 40));
        let dist = |a: Color32, b: Color32| ((a.r() as i32 - b.r() as i32).pow(2) + (a.g() as i32 - b.g() as i32).pow(2) + (a.b() as i32 - b.b() as i32).pow(2)) as f32;
        assert!(dist(CheckView::Deuteranopia.apply(r), CheckView::Deuteranopia.apply(g)) < dist(r, g), "they look more alike");
    }
}
