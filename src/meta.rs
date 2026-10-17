// SPDX-License-Identifier: GPL-3.0-or-later
// Per-project information saved in the .seviath file: print size (DPI), notes, tags, guide lines
// and an optional autosave override. It is part of undo, so changing it counts as a change.

use serde::{Deserialize, Serialize};

/// A guide line across the whole canvas.
#[derive(Clone, Copy, PartialEq, Debug, Serialize, Deserialize)]
pub struct Guide {
    /// true = a vertical line at x = `pos`; false = a horizontal line at y = `pos`
    pub vertical: bool,
    pub pos: f32,
}

#[derive(Clone, PartialEq, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct ProjectMeta {
    /// Dots per inch: sets the real-world size and is written into exported files.
    pub dpi: u32,
    pub notes: String,
    pub tags: Vec<String>,
    pub autosave_minutes: Option<f32>,
    pub recovery_copies: Option<usize>,
    pub guides: Vec<Guide>,
    /// How the RGB numbers are meant to look (see colorprofiles.rs).
    pub rgb_profile: String,
    /// The press used for the print preview.
    pub cmyk_profile: String,
    /// How colors that can't be printed are brought in.
    pub intent: String,
    /// Turn the profile into plain sRGB pixels on export instead of embedding it.
    pub export_bake: bool,
}

impl Default for ProjectMeta {
    fn default() -> Self {
        Self { dpi: 72, notes: String::new(), tags: Vec::new(), autosave_minutes: None, recovery_copies: None, guides: Vec::new(), rgb_profile: "srgb".into(), cmyk_profile: "japan2001".into(), intent: "perceptual".into(), export_bake: false }
    }
}

impl ProjectMeta {
    /// Keep every value in a sane range (project files come from outside).
    pub fn sanitized(mut self) -> Self {
        self.dpi = self.dpi.clamp(1, 2400);
        self.notes = self.notes.chars().take(4000).collect();
        self.tags = clean_tags(self.tags.iter().map(String::as_str));
        self.autosave_minutes = self.autosave_minutes.map(|m| if m.is_finite() { m.clamp(0.0, 120.0) } else { 3.0 });
        self.recovery_copies = self.recovery_copies.map(|c| c.clamp(1, 100));
        self.guides.retain(|g| g.pos.is_finite());
        self.guides.truncate(200);
        if !crate::colorprofiles::known_rgb(&self.rgb_profile) {
            self.rgb_profile = "srgb".into();
        }
        if !crate::colorprofiles::known_press(&self.cmyk_profile) {
            self.cmyk_profile = "japan2001".into();
        }
        self.intent = crate::colorprofiles::Intent::from_id(&self.intent).id().into();
        self
    }
}

/// Tags typed as "hair, rig, commission" → trimmed, no empties, no repeats, at most 12.
pub fn parse_tags(text: &str) -> Vec<String> {
    clean_tags(text.split(','))
}

fn clean_tags<'a>(tags: impl Iterator<Item = &'a str>) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for t in tags {
        let t: String = t.trim().chars().take(24).collect();
        if !t.is_empty() && !out.iter().any(|o| o.eq_ignore_ascii_case(&t)) {
            out.push(t);
        }
        if out.len() >= 12 {
            break;
        }
    }
    out
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Unit {
    Px,
    Mm,
    Inch,
}

impl Unit {
    pub const ALL: [(Unit, &'static str); 3] = [(Unit::Px, "px"), (Unit::Mm, "mm"), (Unit::Inch, "inch")];

    /// How many of this unit make `px` pixels at `dpi`.
    pub fn from_px(self, px: f32, dpi: u32) -> f32 {
        let d = dpi.max(1) as f32;
        match self {
            Unit::Px => px,
            Unit::Mm => px / d * 25.4,
            Unit::Inch => px / d,
        }
    }

    pub fn to_px(self, v: f32, dpi: u32) -> f32 {
        let d = dpi.max(1) as f32;
        match self {
            Unit::Px => v,
            Unit::Mm => v / 25.4 * d,
            Unit::Inch => v * d,
        }
    }
}

/// "21.0 × 29.7 cm (8.27 × 11.69 in)" for a canvas at some DPI.
pub fn real_size(w: u32, h: u32, dpi: u32) -> String {
    let (iw, ih) = (Unit::Inch.from_px(w as f32, dpi), Unit::Inch.from_px(h as f32, dpi));
    let (cw, ch) = (iw * 2.54, ih * 2.54);
    format!("{cw:.1} × {ch:.1} cm  ({iw:.2} × {ih:.2} in)")
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum GuidePreset {
    Off,
    Thirds,
    Golden,
    Center,
    Safe,
}

impl GuidePreset {
    pub const ALL: [(GuidePreset, &'static str, &'static str); 5] = [
        (GuidePreset::Off, "None", "No guide lines."),
        (GuidePreset::Thirds, "Thirds", "Two lines each way: a classic way to place the focus."),
        (GuidePreset::Golden, "Golden ratio", "Lines at 38% and 62%."),
        (GuidePreset::Center, "Center", "A cross through the middle: handy for symmetric characters."),
        (GuidePreset::Safe, "Safe margin", "A frame 5% in from the edges: keep important things inside (comics, print, thumbnails)."),
    ];

    pub fn key(self) -> &'static str {
        match self {
            GuidePreset::Off => "off",
            GuidePreset::Thirds => "thirds",
            GuidePreset::Golden => "golden",
            GuidePreset::Center => "center",
            GuidePreset::Safe => "safe",
        }
    }

    pub fn from_key(k: &str) -> GuidePreset {
        Self::ALL.iter().map(|a| a.0).find(|p| p.key() == k).unwrap_or(GuidePreset::Off)
    }

    pub fn build(self, w: u32, h: u32) -> Vec<Guide> {
        let (w, h) = (w as f32, h as f32);
        let both = |f: f32| [Guide { vertical: true, pos: (w * f).round() }, Guide { vertical: false, pos: (h * f).round() }];
        match self {
            GuidePreset::Off => Vec::new(),
            GuidePreset::Thirds => [1.0 / 3.0, 2.0 / 3.0].iter().flat_map(|f| both(*f)).collect(),
            GuidePreset::Golden => [0.382, 0.618].iter().flat_map(|f| both(*f)).collect(),
            GuidePreset::Center => both(0.5).to_vec(),
            GuidePreset::Safe => [0.05, 0.95].iter().flat_map(|f| both(*f)).collect(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn units_convert_both_ways() {
        // A4 at 300 dpi is 2480 x 3508 px
        assert_eq!(Unit::Mm.to_px(210.0, 300).round(), 2480.0);
        assert_eq!(Unit::Mm.to_px(297.0, 300).round(), 3508.0);
        assert!((Unit::Inch.from_px(3000.0, 300) - 10.0).abs() < 1e-4);
        assert_eq!(real_size(2480, 3508, 300), "21.0 × 29.7 cm  (8.27 × 11.69 in)");
    }

    #[test]
    fn tags_are_cleaned() {
        assert_eq!(parse_tags(" Hair, rig ,hair,, commission "), ["Hair", "rig", "commission"]);
        assert_eq!(parse_tags(&"a,".repeat(40)).len(), 1, "repeats collapse");
        let many: String = (0..30).map(|i| format!("t{i},")).collect();
        assert_eq!(parse_tags(&many).len(), 12);
    }

    #[test]
    fn guide_presets() {
        assert_eq!(GuidePreset::Thirds.build(300, 600).len(), 4);
        assert!(GuidePreset::Center.build(300, 600).contains(&Guide { vertical: true, pos: 150.0 }));
        assert_eq!(GuidePreset::from_key(GuidePreset::Safe.key()), GuidePreset::Safe);
        let bad = ProjectMeta { dpi: 0, guides: vec![Guide { vertical: true, pos: f32::NAN }], ..Default::default() }.sanitized();
        assert_eq!((bad.dpi, bad.guides.len()), (1, 0));
    }
}
