// SPDX-License-Identifier: GPL-3.0-or-later
// Text layers: the letters are worked out from the font's outlines and drawn as color shapes of a
// vector layer, so text stays sharp at any size, can be edited again and again, and can bend along
// a curve. Layout: lines, alignment, letter spacing, line height, vertical text, rotation.
// Not yet: kerning pairs and ligatures (every letter just advances by its own width).

use serde::{Deserialize, Serialize};
use ttf_parser::{Face, OutlineBuilder};

use crate::fonts::{self, ProjectFont};
use crate::vector::VecFill;

#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub enum Align {
    Left,
    Center,
    Right,
}

#[derive(Clone, PartialEq, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct TextData {
    pub text: String,
    pub font: String,
    /// The height of the letter M's em box, in pixels.
    pub size: f32,
    pub color: [u8; 3],
    pub align: Align,
    /// Distance between lines as a multiple of the size.
    pub line_height: f32,
    /// Extra space after every letter, in pixels.
    pub letter_spacing: f32,
    /// Top left of the text box on the page.
    pub x: f32,
    pub y: f32,
    /// Degrees, around the top left.
    pub rotation: f32,
    /// -1..1: bends the text along a circle (positive arches up, negative sags like a smile).
    pub bend: f32,
    /// Letters stand one above the other.
    pub vertical: bool,
}

impl Default for TextData {
    fn default() -> Self {
        TextData { text: "Text".into(), font: fonts::DEFAULT.into(), size: 64.0, color: [34, 34, 34], align: Align::Left, line_height: 1.2, letter_spacing: 0.0, x: 100.0, y: 100.0, rotation: 0.0, bend: 0.0, vertical: false }
    }
}

pub const MAX_CHARS: usize = 4000;
pub const MAX_SIZE: f32 = 2000.0;

impl TextData {
    pub fn sanitized(mut self) -> TextData {
        let f = |v: f32, lo: f32, hi: f32, d: f32| if v.is_finite() { v.clamp(lo, hi) } else { d };
        self.text = self.text.chars().take(MAX_CHARS).collect();
        self.font = self.font.chars().take(80).collect();
        self.size = f(self.size, 2.0, MAX_SIZE, 64.0);
        self.line_height = f(self.line_height, 0.5, 4.0, 1.2);
        self.letter_spacing = f(self.letter_spacing, -self.size, self.size * 2.0, 0.0);
        self.x = f(self.x, -100_000.0, 100_000.0, 0.0);
        self.y = f(self.y, -100_000.0, 100_000.0, 0.0);
        self.rotation = f(self.rotation, -720.0, 720.0, 0.0);
        self.bend = f(self.bend, -1.0, 1.0, 0.0);
        self
    }
}

/// Collects the outline of one glyph as closed rings of points, in font units (y up).
struct Rings {
    rings: Vec<Vec<[f32; 2]>>,
    cur: Vec<[f32; 2]>,
}

impl Rings {
    fn last(&self) -> [f32; 2] {
        self.cur.last().copied().unwrap_or([0.0, 0.0])
    }
}

impl OutlineBuilder for Rings {
    fn move_to(&mut self, x: f32, y: f32) {
        self.close();
        self.cur.push([x, y]);
    }
    fn line_to(&mut self, x: f32, y: f32) {
        self.cur.push([x, y]);
    }
    fn quad_to(&mut self, x1: f32, y1: f32, x: f32, y: f32) {
        let p0 = self.last();
        for i in 1..=8 {
            let t = i as f32 / 8.0;
            let u = 1.0 - t;
            self.cur.push([u * u * p0[0] + 2.0 * u * t * x1 + t * t * x, u * u * p0[1] + 2.0 * u * t * y1 + t * t * y]);
        }
    }
    fn curve_to(&mut self, x1: f32, y1: f32, x2: f32, y2: f32, x: f32, y: f32) {
        let p0 = self.last();
        for i in 1..=10 {
            let t = i as f32 / 10.0;
            let u = 1.0 - t;
            let (a, b, c, d) = (u * u * u, 3.0 * u * u * t, 3.0 * u * t * t, t * t * t);
            self.cur.push([a * p0[0] + b * x1 + c * x2 + d * x, a * p0[1] + b * y1 + c * y2 + d * y]);
        }
    }
    fn close(&mut self) {
        if self.cur.len() >= 3 {
            self.rings.push(std::mem::take(&mut self.cur));
        } else {
            self.cur.clear();
        }
    }
}

/// The color shapes of the text (rings in page pixels). Empty when the font can't be found.
pub fn build(t: &TextData, project_fonts: &[ProjectFont]) -> Vec<VecFill> {
    let Some(bytes) = fonts::load(&t.font, project_fonts).or_else(|| fonts::load(fonts::DEFAULT, &[])) else { return Vec::new() };
    let Ok(face) = Face::parse(&bytes, 0) else { return Vec::new() };
    let upm = face.units_per_em().max(1) as f32;
    let k = t.size / upm;
    let ascent = face.ascender() as f32 * k;
    let adv = |c: char| -> f32 {
        let g = face.glyph_index(c).or_else(|| face.glyph_index('?'));
        g.and_then(|g| face.glyph_hor_advance(g)).map(|a| a as f32 * k).unwrap_or(t.size * 0.5) + t.letter_spacing
    };

    // lay the letters out: every letter gets (char, x along the baseline, line index)
    struct Placed {
        c: char,
        x: f32,
        line: usize,
    }
    let mut placed: Vec<Placed> = Vec::new();
    let mut line_widths: Vec<f32> = Vec::new();
    if t.vertical {
        // one column: every letter on its own line
        for (i, c) in t.text.chars().filter(|c| *c != '\n').enumerate() {
            placed.push(Placed { c, x: 0.0, line: i });
            line_widths.push(adv(c) - t.letter_spacing);
        }
    } else {
        for (li, line) in t.text.split('\n').enumerate() {
            let mut x = 0.0;
            for c in line.chars() {
                placed.push(Placed { c, x, line: li });
                x += adv(c);
            }
            line_widths.push((x - t.letter_spacing).max(0.0));
        }
    }
    let block_w = line_widths.iter().cloned().fold(0.0f32, f32::max);
    let (sin_r, cos_r) = t.rotation.to_radians().sin_cos();
    let radius = if t.bend.abs() > 0.01 && block_w > 1.0 && !t.vertical { block_w / (t.bend.abs() * std::f32::consts::PI) } else { 0.0 };
    let mut out: Vec<VecFill> = Vec::new();
    for p in &placed {
        let Some(g) = face.glyph_index(p.c).or_else(|| face.glyph_index('?')) else { continue };
        let mut rings = Rings { rings: Vec::new(), cur: Vec::new() };
        if face.outline_glyph(g, &mut rings).is_none() {
            continue; // a space or an empty glyph
        }
        rings.close();
        // the letter's place in the box (y measured down from the box top)
        let line_off = if t.vertical { 0.0 } else { 0.0 };
        let _ = line_off;
        let offset_x = match t.align {
            Align::Left => 0.0,
            Align::Center => (block_w - line_widths[p.line]) / 2.0,
            Align::Right => block_w - line_widths[p.line],
        } + if t.vertical { (block_w - line_widths[p.line]) / 2.0 } else { 0.0 };
        let baseline = ascent + p.line as f32 * t.size * t.line_height;
        let map = |pt: [f32; 2]| -> [f32; 2] {
            // font units → box pixels: x along the line, y down from the box top
            let (mut bx, mut by) = (offset_x + p.x + pt[0] * k, baseline - pt[1] * k);
            if radius > 0.0 {
                // along a circle: the baseline of the first line is the arc, other lines are concentric
                let rel_y = by - ascent; // down from the first baseline
                let arch = t.bend > 0.0;
                let ry = if arch { rel_y } else { -rel_y };
                let phi = (bx - block_w / 2.0) / radius;
                let rho = radius - ry;
                let (px, py) = (block_w / 2.0 + rho * phi.sin(), ascent + radius - rho * phi.cos());
                bx = px;
                by = if arch { py } else { 2.0 * ascent - py };
            }
            // then turn the whole box about its top left and put it on the page
            [t.x + bx * cos_r - by * sin_r, t.y + bx * sin_r + by * cos_r]
        };
        let rings: Vec<Vec<[f32; 2]>> = rings.rings.iter().map(|r| r.iter().map(|p| map(*p)).collect()).collect();
        if !rings.is_empty() {
            out.push(VecFill { rings, color: t.color });
        }
    }
    // vertical text: the letters are stacked, so move each line down by its own row (done through the line index)
    out
}

/// The width and height of the text box in pixels (before turning and bending).
pub fn measure(t: &TextData, project_fonts: &[ProjectFont]) -> (f32, f32) {
    let Some(bytes) = fonts::load(&t.font, project_fonts).or_else(|| fonts::load(fonts::DEFAULT, &[])) else { return (0.0, 0.0) };
    let Ok(face) = Face::parse(&bytes, 0) else { return (0.0, 0.0) };
    let k = t.size / face.units_per_em().max(1) as f32;
    let adv = |c: char| face.glyph_index(c).and_then(|g| face.glyph_hor_advance(g)).map(|a| a as f32 * k).unwrap_or(t.size * 0.5) + t.letter_spacing;
    if t.vertical {
        let n = t.text.chars().filter(|c| *c != '\n').count();
        let w = t.text.chars().filter(|c| *c != '\n').map(adv).fold(0.0f32, f32::max);
        return (w, n as f32 * t.size * t.line_height);
    }
    let lines: Vec<f32> = t.text.split('\n').map(|l| l.chars().map(adv).sum::<f32>() - t.letter_spacing).collect();
    (lines.iter().cloned().fold(0.0, f32::max).max(0.0), lines.len() as f32 * t.size * t.line_height)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bounds(f: &[VecFill]) -> (f32, f32, f32, f32) {
        let (mut x0, mut y0, mut x1, mut y1) = (f32::MAX, f32::MAX, f32::MIN, f32::MIN);
        for fill in f {
            for r in &fill.rings {
                for p in r {
                    x0 = x0.min(p[0]);
                    y0 = y0.min(p[1]);
                    x1 = x1.max(p[0]);
                    y1 = y1.max(p[1]);
                }
            }
        }
        (x0, y0, x1, y1)
    }

    fn t(text: &str) -> TextData {
        TextData { text: text.into(), x: 0.0, y: 0.0, size: 100.0, ..TextData::default() }
    }

    #[test]
    fn letters_have_the_size_and_place_asked_for_and_spaces_make_no_shapes() {
        let one = build(&t("H"), &[]);
        assert_eq!(one.len(), 1);
        let (x0, y0, x1, y1) = bounds(&one);
        assert!(y1 - y0 > 55.0 && y1 - y0 < 90.0, "a capital is a bit less than the size: {}", y1 - y0);
        assert!(x0 >= 0.0 && x1 < 100.0 && y0 > 0.0, "inside the box ({x0},{y0})-({x1},{y1})");
        assert_eq!(build(&t("a b"), &[]).len(), 2, "the space draws nothing");
        assert!(build(&t(""), &[]).is_empty());
        // a bigger size, a bigger word
        let big = TextData { size: 200.0, ..t("Hello") };
        let (a, b) = (bounds(&build(&t("Hello"), &[])), bounds(&build(&big, &[])));
        assert!((b.2 - b.0) > (a.2 - a.0) * 1.8);
        // Latvian letters are letters
        assert_eq!(build(&t("āčēģīķļņšūž"), &[]).len(), 11);
    }

    #[test]
    fn lines_alignment_spacing_and_vertical() {
        let two = build(&t("HH\nH"), &[]);
        assert_eq!(two.len(), 3);
        let (_, y0, _, y1) = bounds(&two);
        let (_, a0, _, a1) = bounds(&build(&t("H"), &[]));
        assert!(y1 - y0 > (a1 - a0) + 100.0, "the second line is lower");
        // alignment: the short second line sits at the left, centered, or at the right
        let first_x = |align| {
            let d = TextData { align, ..t("HHHH\nH") };
            let f = build(&d, &[]);
            f[4].rings[0].iter().map(|p| p[0]).fold(f32::MAX, f32::min)
        };
        assert!(first_x(Align::Left) < first_x(Align::Center) && first_x(Align::Center) < first_x(Align::Right));
        // spacing
        let w = |sp| {
            let b = bounds(&build(&TextData { letter_spacing: sp, ..t("Wide") }, &[]));
            b.2 - b.0
        };
        assert!(w(20.0) > w(0.0) + 50.0 && w(-10.0) < w(0.0));
        // vertical: the letters are in a column
        let v = build(&TextData { vertical: true, ..t("ABC") }, &[]);
        let (x0, y0, x1, y1) = bounds(&v);
        assert!(y1 - y0 > 2.0 * (x1 - x0) * 0.9, "tall and narrow: {} x {}", x1 - x0, y1 - y0);
        let (mw, mh) = measure(&t("Hello\nyou"), &[]);
        assert!(mw > 150.0 && (mh - 240.0).abs() < 1.0);
    }

    #[test]
    fn rotation_and_bend_move_the_letters() {
        let flat = bounds(&build(&t("HELLO"), &[]));
        let turned = bounds(&build(&TextData { rotation: 90.0, ..t("HELLO") }, &[]));
        assert!((turned.3 - turned.1) > (flat.2 - flat.0) * 0.9 && (turned.2 - turned.0) < (flat.2 - flat.0) * 0.5, "turned a quarter: stands up");
        // an arch: the letters in the middle are higher than at the ends; a smile: lower
        let mid_vs_end = |bend: f32| {
            let f = build(&TextData { bend, ..t("HHHHH") }, &[]);
            let top = |fill: &VecFill| fill.rings[0].iter().map(|p| p[1]).fold(f32::MAX, f32::min);
            (top(&f[0]) + top(&f[4])) / 2.0 - top(&f[2])
        };
        assert!(mid_vs_end(0.0).abs() < 0.5, "straight");
        assert!(mid_vs_end(0.8) > 15.0, "an arch: the middle is higher: {}", mid_vs_end(0.8));
        assert!(mid_vs_end(-0.8) < -15.0, "a smile: the middle is lower");
        // the letters stay the same size on the curve
        let h = |bend: f32| {
            let f = build(&TextData { bend, ..t("HHH") }, &[]);
            let b = bounds(&f[1..2]);
            (b.2 - b.0, b.3 - b.1)
        };
        let (flat_w, flat_h) = h(0.0);
        let (curv_w, curv_h) = h(0.2);
        assert!((curv_w - flat_w).abs() < flat_w * 0.35 && (curv_h - flat_h).abs() < flat_h * 0.35, "{flat_w}x{flat_h} vs {curv_w}x{curv_h}");
    }

    #[test]
    fn settings_are_held_in_range_and_unknown_fonts_fall_back() {
        let wild = TextData { size: 1e9, bend: f32::NAN, line_height: -5.0, text: "x".repeat(10_000), ..TextData::default() }.sanitized();
        assert_eq!((wild.size, wild.bend, wild.line_height, wild.text.chars().count()), (MAX_SIZE, 0.0, 0.5, MAX_CHARS));
        let missing = TextData { font: "A font nobody has".into(), ..t("Hi") };
        assert_eq!(build(&missing, &[]).len(), 2, "the font that comes with the app is used instead");
        let j = serde_json::to_string(&t("Hi")).unwrap();
        assert_eq!(serde_json::from_str::<TextData>(&j).unwrap(), t("Hi"));
        assert_eq!(serde_json::from_str::<TextData>("{}").unwrap(), TextData::default(), "missing fields get defaults");
    }
}
