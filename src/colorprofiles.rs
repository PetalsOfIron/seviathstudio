// SPDX-License-Identifier: GPL-3.0-or-later
// Color profiles: how the RGB numbers of a drawing are meant to look.
//
// The canvas always holds plain RGB numbers. The RGB profile says how to read them:
//  - sRGB is the web / most-screens standard (what every app assumes without a profile).
//  - Display P3, Adobe RGB and ProPhoto RGB are wider color spaces.
//  - "Seviath Illustration" is SeviathStudio's own profile, tuned on anime / illustration
//    references: deep but clean blacks, clean whites, slightly cool shadows and warm highlights,
//    and a little more color in the mid tones.
// The app shows the canvas through the chosen profile, exports can carry it (an ICC file written
// here, no outside library) or have it baked into the pixels for apps that ignore profiles.
//
// A profile is a tone curve per channel (in linear light) and a 3×3 matrix to XYZ, which is
// exactly what an ICC "matrix/TRC" profile stores, so one set of numbers drives the preview, the
// baked pixels and the ICC file.
//
// Print: a press model (ink colors, darkest black, ink limit, black generation, dot gain) pulls
// colors that ink can't reach back to what it can print. That is an approximation of a coated
// paper press; a print shop with a real ICC profile gives the exact result.

use std::collections::HashMap;
use std::sync::{Arc, Mutex, OnceLock};

use eframe::egui::Color32;
use rayon::prelude::*;

use crate::check::CheckView;
use crate::oklab::{oklab_to_rgb, rgb_to_oklab};

type M3 = [f64; 9];

#[derive(Clone, Copy, PartialEq, Debug)]
enum Trc {
    Srgb,
    Gamma(f64),
    Seviath,
}

pub struct RgbProfile {
    pub id: &'static str,
    /// As written in the ICC file.
    pub name: &'static str,
    pub about: &'static str,
    /// Chromaticities (x, y) of red, green, blue and the white point.
    primaries: [[f64; 2]; 4],
    trc: Trc,
    /// Extra linear 3×3 applied after the curves, in the profile's own primaries.
    look: Option<M3>,
}

const D65: [f64; 2] = [0.3127, 0.329];
const D50: [f64; 2] = [0.3457, 0.3585];
const SRGB_P: [[f64; 2]; 4] = [[0.64, 0.33], [0.3, 0.6], [0.15, 0.06], D65];

pub const RGB_PROFILES: [RgbProfile; 5] = [
    RgbProfile { id: "srgb", name: "sRGB IEC61966-2.1", about: "The standard for screens and the web. Safe choice for posting online.", primaries: SRGB_P, trc: Trc::Srgb, look: None },
    RgbProfile {
        id: "seviath",
        name: "Seviath Illustration",
        about: "SeviathStudio's own profile for illustration: deep clean blacks, clean whites, cool shadows, warm highlights, natural lively color.",
        primaries: SRGB_P,
        trc: Trc::Seviath,
        look: Some(saturation(1.08)),
    },
    RgbProfile { id: "p3", name: "Display P3", about: "Wider colors of newer phones, Macs and wide-gamut monitors.", primaries: [[0.68, 0.32], [0.265, 0.69], [0.15, 0.06], D65], trc: Trc::Srgb, look: None },
    RgbProfile { id: "adobe", name: "Adobe RGB (1998)", about: "Wider greens and cyans, common for print and photography.", primaries: [[0.64, 0.33], [0.21, 0.71], [0.15, 0.06], D65], trc: Trc::Gamma(563.0 / 256.0), look: None },
    RgbProfile { id: "prophoto", name: "ProPhoto RGB", about: "Very wide space for photo editing. Not for posting online.", primaries: [[0.7347, 0.2653], [0.1596, 0.8404], [0.0366, 0.0001], D50], trc: Trc::Gamma(1.8), look: None },
];

pub fn rgb_profile(id: &str) -> &'static RgbProfile {
    RGB_PROFILES.iter().find(|p| p.id == id).unwrap_or(&RGB_PROFILES[0])
}

/// Is this a profile we know?
pub fn known_rgb(id: &str) -> bool {
    RGB_PROFILES.iter().any(|p| p.id == id)
}

fn srgb_decode(v: f64) -> f64 {
    if v <= 0.04045 { v / 12.92 } else { ((v + 0.055) / 1.055).powf(2.4) }
}

fn srgb_encode(v: f64) -> f64 {
    if v <= 0.0031308 { 12.92 * v } else { 1.055 * v.powf(1.0 / 2.4) - 0.055 }
}

fn smooth(a: f64, b: f64, x: f64) -> f64 {
    let t = ((x - a) / (b - a)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

/// The Seviath curve: a gentle S (deeper darks, cleaner light end), cool shadows, warm highlights.
fn seviath_curve(ch: usize, v: f64) -> f64 {
    let shadow = [-0.012, -0.004, 0.018][ch];
    let light = [0.012, 0.005, -0.012][ch];
    let pi = std::f64::consts::PI;
    let mut x = v + 0.06 * (pi * (v - 0.5)).sin() * (pi * v).sin() * 0.9;
    x -= 0.02 * (1.0 - smooth(0.0, 0.08, v)) * (1.0 - v);
    x += 0.015 * smooth(0.9, 1.0, v);
    x += shadow * smooth(0.0, 0.25, v) * (1.0 - smooth(0.25, 0.5, v));
    x += light * smooth(0.55, 0.85, v) * (1.0 - smooth(0.92, 1.0, v));
    srgb_decode(x.clamp(0.0, 1.0))
}

const fn saturation(s: f64) -> M3 {
    let (lr, lg, lb) = (0.2126, 0.7152, 0.0722);
    [lr + (1.0 - lr) * s, lg - lg * s, lb - lb * s, lr - lr * s, lg + (1.0 - lg) * s, lb - lb * s, lr - lr * s, lg - lg * s, lb + (1.0 - lb) * s]
}

impl Trc {
    /// Stored value (0..1) → linear light.
    fn decode(self, ch: usize, v: f64) -> f64 {
        match self {
            Trc::Srgb => srgb_decode(v),
            Trc::Gamma(g) => v.max(0.0).powf(g),
            Trc::Seviath => seviath_curve(ch, v),
        }
    }
}

// ---------- matrix math ----------

fn mul(a: &M3, b: &M3) -> M3 {
    let mut o = [0.0; 9];
    for i in 0..3 {
        for j in 0..3 {
            for k in 0..3 {
                o[i * 3 + j] += a[i * 3 + k] * b[k * 3 + j];
            }
        }
    }
    o
}

fn inv(m: &M3) -> M3 {
    let [a, b, c, d, e, f, g, h, i] = *m;
    let (aa, bb, cc) = (e * i - f * h, -(d * i - f * g), d * h - e * g);
    let det = a * aa + b * bb + c * cc;
    [aa / det, -(b * i - c * h) / det, (b * f - c * e) / det, bb / det, (a * i - c * g) / det, -(a * f - c * d) / det, cc / det, -(a * h - b * g) / det, (a * e - b * d) / det]
}

fn xyz(p: [f64; 2]) -> [f64; 3] {
    [p[0] / p[1], 1.0, (1.0 - p[0] - p[1]) / p[1]]
}

/// RGB (linear) → XYZ with the profile's own white.
fn rgb_to_xyz(p: &[[f64; 2]; 4]) -> M3 {
    let (r, g, b) = (xyz(p[0]), xyz(p[1]), xyz(p[2]));
    let pm = [r[0], g[0], b[0], r[1], g[1], b[1], r[2], g[2], b[2]];
    let w = xyz(p[3]);
    let pi = inv(&pm);
    let s = [pi[0] * w[0] + pi[1] * w[1] + pi[2] * w[2], pi[3] * w[0] + pi[4] * w[1] + pi[5] * w[2], pi[6] * w[0] + pi[7] * w[1] + pi[8] * w[2]];
    [pm[0] * s[0], pm[1] * s[1], pm[2] * s[2], pm[3] * s[0], pm[4] * s[1], pm[5] * s[2], pm[6] * s[0], pm[7] * s[1], pm[8] * s[2]]
}

/// Bradford white point adaptation.
fn adapt(from: [f64; 2], to: [f64; 2]) -> M3 {
    const BRAD: M3 = [0.8951, 0.2664, -0.1614, -0.7502, 1.7135, 0.0367, 0.0389, -0.0685, 1.0296];
    if from == to {
        return [1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0];
    }
    let (s, d) = (xyz(from), xyz(to));
    let cone = |v: [f64; 3]| [0, 1, 2].map(|i| BRAD[i * 3] * v[0] + BRAD[i * 3 + 1] * v[1] + BRAD[i * 3 + 2] * v[2]);
    let (cs, cd) = (cone(s), cone(d));
    mul(&inv(&BRAD), &mul(&[cd[0] / cs[0], 0.0, 0.0, 0.0, cd[1] / cs[1], 0.0, 0.0, 0.0, cd[2] / cs[2]], &BRAD))
}

/// The profile's linear RGB → XYZ under D50 (what ICC files store), look included.
fn to_d50(p: &RgbProfile) -> M3 {
    let m = mul(&adapt(p.primaries[3], D50), &rgb_to_xyz(&p.primaries));
    match &p.look {
        Some(l) => mul(&m, l),
        None => m,
    }
}

/// The profile's linear RGB → linear sRGB (to show it on a normal screen).
fn to_srgb_matrix(p: &RgbProfile) -> M3 {
    let to_s = inv(&mul(&adapt(D65, D50), &rgb_to_xyz(&SRGB_P)));
    mul(&to_s, &to_d50(p))
}

// ---------- showing and baking ----------

/// Reads stored colors in a profile and gives sRGB for the screen (or for baking).
#[derive(Clone)]
pub struct ProfileXform {
    decode: [[f32; 256]; 3],
    m: [f32; 9],
    encode: Arc<Vec<u8>>,
}

const ENC: usize = 4096;

fn encode_table() -> Arc<Vec<u8>> {
    static T: OnceLock<Arc<Vec<u8>>> = OnceLock::new();
    T.get_or_init(|| Arc::new((0..=ENC).map(|i| (srgb_encode(i as f64 / ENC as f64) * 255.0).round().clamp(0.0, 255.0) as u8).collect())).clone()
}

impl ProfileXform {
    /// None for sRGB (nothing to do).
    pub fn new(id: &str) -> Option<ProfileXform> {
        if id == "srgb" || !known_rgb(id) {
            return None;
        }
        let p = rgb_profile(id);
        let mut decode = [[0f32; 256]; 3];
        for (ch, row) in decode.iter_mut().enumerate() {
            for (i, v) in row.iter_mut().enumerate() {
                *v = p.trc.decode(ch, i as f64 / 255.0) as f32;
            }
        }
        let m = to_srgb_matrix(p).map(|v| v as f32);
        Some(ProfileXform { decode, m, encode: encode_table() })
    }

    pub fn apply(&self, rgb: [u8; 3]) -> [u8; 3] {
        let (r, g, b) = (self.decode[0][rgb[0] as usize], self.decode[1][rgb[1] as usize], self.decode[2][rgb[2] as usize]);
        let m = &self.m;
        let q = |v: f32| self.encode[(v.clamp(0.0, 1.0) * ENC as f32).round() as usize];
        [q(m[0] * r + m[1] * g + m[2] * b), q(m[3] * r + m[4] * g + m[5] * b), q(m[6] * r + m[7] * g + m[8] * b)]
    }
}

/// For apps and websites that ignore profiles: turn the pixels into sRGB that looks the same.
pub fn bake_to_srgb(img: &mut image::RgbaImage, id: &str) {
    let Some(x) = ProfileXform::new(id) else { return };
    img.par_chunks_mut(4).for_each(|p| {
        if p[3] != 0 {
            let o = x.apply([p[0], p[1], p[2]]);
            p[..3].copy_from_slice(&o);
        }
    });
}

// ---------- the ICC profile file (version 2, matrix/TRC display profile) ----------

fn put32(v: &mut Vec<u8>, x: u32) {
    v.extend_from_slice(&x.to_be_bytes());
}

fn s15(v: f64) -> u32 {
    ((v * 65536.0).round() as i32) as u32
}

fn xyz_tag(x: f64, y: f64, z: f64) -> Vec<u8> {
    let mut b = b"XYZ \0\0\0\0".to_vec();
    for v in [x, y, z] {
        put32(&mut b, s15(v));
    }
    b
}

fn desc_tag(text: &str) -> Vec<u8> {
    let mut b = b"desc\0\0\0\0".to_vec();
    put32(&mut b, text.len() as u32 + 1);
    b.extend_from_slice(text.as_bytes());
    b.push(0);
    b.extend_from_slice(&[0u8; 4 + 4 + 2 + 1 + 67]); // no unicode, no Macintosh text
    b
}

fn text_tag(text: &str) -> Vec<u8> {
    let mut b = b"text\0\0\0\0".to_vec();
    b.extend_from_slice(text.as_bytes());
    b.push(0);
    b
}

fn curve_tag(p: &RgbProfile, ch: usize) -> Vec<u8> {
    let n = 1024usize;
    let mut b = b"curv\0\0\0\0".to_vec();
    put32(&mut b, n as u32);
    for i in 0..n {
        let v = p.trc.decode(ch, i as f64 / (n - 1) as f64).clamp(0.0, 1.0);
        b.extend_from_slice(&((v * 65535.0).round() as u16).to_be_bytes());
    }
    b
}

/// The profile as an ICC file others can read.
pub fn icc_profile(id: &str) -> Vec<u8> {
    let p = rgb_profile(id);
    let m = to_d50(p);
    let tags: Vec<(&[u8; 4], Vec<u8>)> = vec![
        (b"desc", desc_tag(p.name)),
        (b"cprt", text_tag(if p.id == "seviath" { "SeviathStudio, free to use (CC0)" } else { "No copyright, use freely" })),
        (b"wtpt", xyz_tag(0.9642, 1.0, 0.8249)),
        (b"rXYZ", xyz_tag(m[0], m[3], m[6])),
        (b"gXYZ", xyz_tag(m[1], m[4], m[7])),
        (b"bXYZ", xyz_tag(m[2], m[5], m[8])),
        (b"rTRC", curve_tag(p, 0)),
        (b"gTRC", curve_tag(p, 1)),
        (b"bTRC", curve_tag(p, 2)),
    ];
    let pad = |n: usize| (n + 3) & !3;
    let mut offset = 128 + 4 + tags.len() * 12;
    let mut places = Vec::new();
    for (_, d) in &tags {
        places.push(offset);
        offset = pad(offset + d.len());
    }
    let mut out = vec![0u8; offset];
    out[0..4].copy_from_slice(&(offset as u32).to_be_bytes());
    out[4..8].copy_from_slice(b"SVTH");
    out[8..12].copy_from_slice(&0x0210_0000u32.to_be_bytes());
    out[12..24].copy_from_slice(b"mntrRGB XYZ ");
    // creation date: fixed, so the same profile is always the same bytes
    for (i, v) in [2026u16, 1, 1, 0, 0, 0].iter().enumerate() {
        out[24 + i * 2..26 + i * 2].copy_from_slice(&v.to_be_bytes());
    }
    out[36..40].copy_from_slice(b"acsp");
    out[40..44].copy_from_slice(b"APPL");
    out[68..72].copy_from_slice(&s15(0.9642).to_be_bytes());
    out[72..76].copy_from_slice(&s15(1.0).to_be_bytes());
    out[76..80].copy_from_slice(&s15(0.8249).to_be_bytes());
    out[80..84].copy_from_slice(b"SVTH");
    out[128..132].copy_from_slice(&(tags.len() as u32).to_be_bytes());
    for (i, (sig, d)) in tags.iter().enumerate() {
        let at = 132 + i * 12;
        out[at..at + 4].copy_from_slice(*sig);
        out[at + 4..at + 8].copy_from_slice(&(places[i] as u32).to_be_bytes());
        out[at + 8..at + 12].copy_from_slice(&(d.len() as u32).to_be_bytes());
        out[places[i]..places[i] + d.len()].copy_from_slice(d);
    }
    out
}

// ---------- print: the press model ----------

#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
pub enum Intent {
    Perceptual,
    Relative,
    Saturation,
    Absolute,
}

impl Intent {
    pub const ALL: [(Intent, &'static str, &'static str, &'static str); 4] = [
        (Intent::Perceptual, "perceptual", "Perceptual", "Squeezes all colors a little so smooth gradients stay smooth. Best for illustration."),
        (Intent::Relative, "relative", "Relative colorimetric", "Colors that print stay exact; only the others are moved in."),
        (Intent::Saturation, "saturation", "Saturation", "Keeps colors as strong as possible, even if lightness shifts. For bold graphics."),
        (Intent::Absolute, "absolute", "Absolute colorimetric", "Also simulates the paper color. For proofs."),
    ];

    pub fn id(self) -> &'static str {
        Self::ALL.iter().find(|i| i.0 == self).map(|i| i.1).unwrap_or("perceptual")
    }

    pub fn from_id(id: &str) -> Intent {
        Self::ALL.iter().find(|i| i.1 == id).map(|i| i.0).unwrap_or(Intent::Perceptual)
    }
}

pub struct Press {
    pub id: &'static str,
    pub name: &'static str,
    pub about: &'static str,
    /// C, C+M, M, M+Y, Y, C+Y as printed.
    inks: [&'static str; 6],
    /// Darkest printable lightness (OKLab).
    black_l: f32,
    /// Total ink limit, percent.
    tac: f32,
    /// How much gray is printed with black ink instead of C+M+Y.
    gcr: f32,
    /// Mid-tone darkening on press.
    dot_gain: f32,
}

pub const PRESSES: [Press; 6] = [
    Press { id: "japan2001", name: "Japan Color 2001 Coated", about: "Standard for Japanese coated paper (the usual default in Japanese art software).", inks: ["#00a0e9", "#1d2088", "#e4007f", "#e60012", "#fff100", "#009944"], black_l: 0.18, tac: 350.0, gcr: 0.5, dot_gain: 0.14 },
    Press { id: "fogra39", name: "Coated FOGRA39", about: "European standard for coated paper.", inks: ["#00aeef", "#2e3192", "#ec008c", "#ed1c24", "#fff200", "#00a651"], black_l: 0.2, tac: 330.0, gcr: 0.5, dot_gain: 0.13 },
    Press { id: "gracol", name: "Coated GRACoL 2006", about: "US standard for high-quality sheet-fed print.", inks: ["#00aeef", "#2b3990", "#ec008c", "#ee2a24", "#fff200", "#00a651"], black_l: 0.19, tac: 320.0, gcr: 0.5, dot_gain: 0.12 },
    Press { id: "swop", name: "U.S. Web Coated (SWOP) v2", about: "US magazines and web presses.", inks: ["#00a6e2", "#2e3192", "#e6007e", "#e31e24", "#ffed00", "#009846"], black_l: 0.22, tac: 300.0, gcr: 0.55, dot_gain: 0.18 },
    Press { id: "fogra29", name: "Uncoated FOGRA29", about: "Uncoated paper (books, office paper): softer, duller colors.", inks: ["#3aa6dd", "#4b4a8f", "#d9468f", "#d9433f", "#fbe74e", "#3a9d5a"], black_l: 0.3, tac: 300.0, gcr: 0.6, dot_gain: 0.22 },
    Press { id: "seviath", name: "Seviath Illustration Print", about: "SeviathStudio's print profile for illustration: keeps pinks, reds and skin tones lively, rich deep blacks.", inks: ["#00a8e8", "#2a2f8f", "#ea0a8c", "#ec1c2e", "#fff300", "#00a24e"], black_l: 0.16, tac: 340.0, gcr: 0.4, dot_gain: 0.12 },
];

pub fn press(id: &str) -> &'static Press {
    PRESSES.iter().find(|p| p.id == id).unwrap_or(&PRESSES[0])
}

pub fn known_press(id: &str) -> bool {
    PRESSES.iter().any(|p| p.id == id)
}

fn hex_rgb(hex: &str) -> [f32; 3] {
    let n = u32::from_str_radix(hex.trim_start_matches('#'), 16).unwrap_or(0);
    [((n >> 16) & 255) as f32 / 255.0, ((n >> 8) & 255) as f32 / 255.0, (n & 255) as f32 / 255.0]
}

#[derive(Clone, Copy)]
struct Cusp {
    h: f32,
    l: f32,
    c: f32,
}

/// What a press can print, ready to pull colors in.
pub struct Printer {
    press: &'static Press,
    intent: Intent,
    cusps: Vec<Cusp>,
}

impl Printer {
    pub fn new(press_id: &str, intent: Intent) -> Printer {
        let press = press(press_id);
        let mut cusps: Vec<Cusp> = press
            .inks
            .iter()
            .map(|h| {
                let [l, a, b] = rgb_to_oklab(hex_rgb(h));
                Cusp { h: b.atan2(a), l, c: a.hypot(b) }
            })
            .collect();
        cusps.sort_by(|a, b| a.h.total_cmp(&b.h));
        Printer { press, intent, cusps }
    }

    /// The most colorful printable color for any hue, blended between the two nearest inks.
    fn cusp_at(&self, h: f32) -> (f32, f32) {
        let cs = &self.cusps;
        let n = cs.len();
        let tau = std::f32::consts::TAU;
        for i in 0..n {
            let (a, b) = (cs[i], cs[(i + 1) % n]);
            let mut span = b.h - a.h;
            if span <= 0.0 {
                span += tau;
            }
            let mut d = h - a.h;
            if d < 0.0 {
                d += tau;
            }
            if d <= span {
                let t = d / span;
                return (a.l + (b.l - a.l) * t, a.c + (b.c - a.c) * t);
            }
        }
        (cs[0].l, cs[0].c)
    }

    /// Pull a color into the printable range. Colors that already print stay (nearly) the same.
    pub fn to_printable(&self, rgb: [f32; 3]) -> [u8; 3] {
        let [l0, mut a, mut b] = rgb_to_oklab([rgb[0] / 255.0, rgb[1] / 255.0, rgb[2] / 255.0]);
        let lb = self.press.black_l * 0.9;
        let mut l = l0.clamp(lb, 1.0);
        let c = a.hypot(b);
        if c > 1e-4 {
            let (cl, cc) = self.cusp_at(b.atan2(a));
            // the most colorful printable color at lightness `x` (the edges bulge outward a little)
            let max_at = |x: f32| {
                if x >= cl {
                    cc * (1.0 - ((x - cl) / (1.0 - cl).max(1e-3)).min(1.0).powf(1.6))
                } else {
                    cc * (1.0 - ((cl - x) / (cl - lb).max(1e-3)).min(1.0).powf(1.6))
                }
            };
            if self.intent == Intent::Perceptual {
                let m = max_at(l);
                let knee = m * 0.75;
                if c > knee && m > 1e-4 {
                    let t = (c - knee) / (m * 0.6);
                    let nc = knee + m * 0.25 * t.tanh();
                    a *= nc / c;
                    b *= nc / c;
                }
            }
            let c2 = a.hypot(b);
            if c2 > max_at(l) {
                let start = if self.intent == Intent::Saturation {
                    cl
                } else if l > cl {
                    (cl + 0.4) / 2.0
                } else {
                    cl
                };
                let (mut lo, mut hi) = (0f32, 1f32);
                for _ in 0..18 {
                    let t = (lo + hi) / 2.0;
                    if t * c2 <= max_at(start + t * (l - start)) {
                        lo = t;
                    } else {
                        hi = t;
                    }
                }
                l = start + lo * (l - start);
                a *= lo;
                b *= lo;
            }
        }
        // dot gain: ink spreads on paper, mid tones print a little darker
        l -= self.press.dot_gain * 0.25 * 4.0 * l * (1.0 - l) * 0.5;
        if self.intent == Intent::Absolute {
            l *= 0.965;
            b += 0.008 * l;
        }
        let o = oklab_to_rgb([l, a, b]);
        [(o[0] * 255.0).round() as u8, (o[1] * 255.0).round() as u8, (o[2] * 255.0).round() as u8]
    }

    /// A table of 64 steps per channel (index = r>>2, g>>2, b>>2), for whole pictures.
    pub fn table(press_id: &str, intent: Intent) -> Arc<Vec<u8>> {
        static CACHE: OnceLock<Mutex<HashMap<(String, Intent), Arc<Vec<u8>>>>> = OnceLock::new();
        let cache = CACHE.get_or_init(|| Mutex::new(HashMap::new()));
        let key = (press(press_id).id.to_string(), intent);
        if let Some(t) = cache.lock().ok().and_then(|c| c.get(&key).cloned()) {
            return t;
        }
        let pr = Printer::new(press_id, intent);
        let t: Vec<u8> = (0..64usize)
            .into_par_iter()
            .flat_map_iter(|r| {
                let pr = &pr;
                (0..64usize).flat_map(move |g| (0..64usize).flat_map(move |b| pr.to_printable([(r * 4 + 2) as f32, (g * 4 + 2) as f32, (b * 4 + 2) as f32])))
            })
            .collect();
        let t = Arc::new(t);
        if let Ok(mut c) = cache.lock() {
            c.insert(key, t.clone());
        }
        t
    }

    /// C, M, Y, K in percent for a screen color as it would print (black generation and ink limit included).
    pub fn cmyk(&self, rgb: [u8; 3]) -> [f32; 4] {
        let t = Printer::table(self.press.id, self.intent);
        let j = (((rgb[0] >> 2) as usize) << 12 | ((rgb[1] >> 2) as usize) << 6 | (rgb[2] >> 2) as usize) * 3;
        let (r, g, b) = (t[j] as f32 / 255.0, t[j + 1] as f32 / 255.0, t[j + 2] as f32 / 255.0);
        let (c0, m0, y0) = (1.0 - r, 1.0 - g, 1.0 - b);
        let gray = c0.min(m0).min(y0);
        let gcr = self.press.gcr;
        let k = if gray >= 0.999 { 1.0 } else { gray * (gcr + (1.0 - gcr) * gray * gray) };
        let inv = if k >= 0.999 { 0.0 } else { 1.0 / (1.0 - k) };
        let (mut c, mut m, mut y) = (((c0 - k) * inv).max(0.0), ((m0 - k) * inv).max(0.0), ((y0 - k) * inv).max(0.0));
        let tac = self.press.tac / 100.0;
        if c + m + y + k > tac && c + m + y > 0.0 {
            let f = ((tac - k) / (c + m + y)).max(0.0);
            c *= f;
            m *= f;
            y *= f;
        }
        [c * 100.0, m * 100.0, y * 100.0, k * 100.0]
    }
}

/// How far a color moves when printed (sum of the three channel differences).
fn print_shift(table: &[u8], rgb: [u8; 3]) -> i32 {
    let j = (((rgb[0] >> 2) as usize) << 12 | ((rgb[1] >> 2) as usize) << 6 | (rgb[2] >> 2) as usize) * 3;
    (0..3).map(|k| (table[j + k] as i32 - rgb[k] as i32).abs()).sum()
}

/// Colors that move more than this don't print as they look.
const UNPRINTABLE: i32 = 18;

/// Share (0..1) of the visible pixels that ink can't reproduce.
pub fn unprintable_share(img: &image::RgbaImage, press_id: &str, intent: Intent) -> f32 {
    let t = Printer::table(press_id, intent);
    let (bad, all) = img
        .as_raw()
        .par_chunks(4)
        .map(|p| if p[3] < 8 { (0u64, 0u64) } else { (u64::from(print_shift(&t, [p[0], p[1], p[2]]) >= UNPRINTABLE), 1) })
        .reduce(|| (0, 0), |a, b| (a.0 + b.0, a.1 + b.1));
    if all == 0 { 0.0 } else { bad as f32 / all as f32 }
}

// ---------- how the canvas is shown ----------

#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum Proof {
    #[default]
    Off,
    /// The drawing as it would print.
    Print,
    /// Colors that can't be printed show as flat gray.
    Warn,
}

impl Proof {
    pub const ALL: [(Proof, &'static str, &'static str); 3] = [
        (Proof::Off, "Off", "The drawing as it is."),
        (Proof::Print, "Print preview", "How the colors look once printed: bright neon colors get duller."),
        (Proof::Warn, "Out of range", "Colors ink can't print show as flat gray."),
    ];
}

/// Everything that changes how the canvas is *shown* (never the drawing itself).
#[derive(Clone, PartialEq, Debug)]
pub struct Look {
    pub check: CheckView,
    pub profile: String,
    pub proof: Proof,
    pub press: String,
    pub intent: Intent,
}

impl Default for Look {
    fn default() -> Self {
        Look { check: CheckView::Normal, profile: "srgb".into(), proof: Proof::Off, press: "japan2001".into(), intent: Intent::Perceptual }
    }
}

pub struct Display {
    pub look: Look,
    xf: Option<ProfileXform>,
    table: Option<Arc<Vec<u8>>>,
}

impl Display {
    pub fn new(look: Look) -> Display {
        let xf = ProfileXform::new(&look.profile);
        let table = (look.proof != Proof::Off).then(|| Printer::table(&look.press, look.intent));
        Display { look, xf, table }
    }

    /// True when the display is the plain numbers.
    pub fn is_plain(&self) -> bool {
        self.xf.is_none() && self.table.is_none() && self.look.check == CheckView::Normal
    }

    /// The color as shown (premultiplied in, premultiplied out).
    pub fn apply(&self, c: Color32) -> Color32 {
        if c.a() == 0 || self.is_plain() {
            return c;
        }
        let [r, g, b, a] = c.to_srgba_unmultiplied();
        let mut rgb = [r, g, b];
        if let Some(x) = &self.xf {
            rgb = x.apply(rgb);
        }
        if let Some(t) = &self.table {
            let j = (((rgb[0] >> 2) as usize) << 12 | ((rgb[1] >> 2) as usize) << 6 | (rgb[2] >> 2) as usize) * 3;
            rgb = if self.look.proof == Proof::Warn {
                if print_shift(t, rgb) >= UNPRINTABLE { [128, 128, 128] } else { rgb }
            } else {
                [t[j], t[j + 1], t[j + 2]]
            };
        }
        self.look.check.apply(Color32::from_rgba_unmultiplied(rgb[0], rgb[1], rgb[2], a))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn srgb_is_untouched_and_wide_profiles_move_colors() {
        assert!(ProfileXform::new("srgb").is_none());
        assert!(ProfileXform::new("nonsense-id").is_none(), "an unknown profile reads as sRGB");
        let p3 = ProfileXform::new("p3").unwrap();
        // gray stays gray
        let g = p3.apply([128, 128, 128]);
        assert!(g.iter().all(|v| (*v as i32 - 128).abs() <= 2), "{g:?}");
        // a pure P3 red is more colorful than sRGB can show: it lands on the sRGB edge
        let r = p3.apply([255, 0, 0]);
        assert!(r[0] >= 254 && r[1] == 0 && r[2] == 0, "{r:?}");
        // a mild P3 color is shown less saturated in sRGB terms (the same numbers mean a stronger color)
        let mild = p3.apply([200, 120, 120]);
        assert!(mild[0] > 200 && mild[1] < 120, "more colorful on screen: {mild:?}");
        let adobe = ProfileXform::new("adobe").unwrap();
        assert_eq!(adobe.apply([0, 0, 0]), [0, 0, 0]);
        assert_eq!(adobe.apply([255, 255, 255]), [255, 255, 255]);
    }

    #[test]
    fn seviath_profile_keeps_black_and_white_and_adds_a_little_color() {
        let s = ProfileXform::new("seviath").unwrap();
        let w = s.apply([255, 255, 255]);
        assert!(w.iter().all(|v| *v >= 250), "clean whites: {w:?}");
        assert!(s.apply([0, 0, 0]).iter().all(|v| *v <= 6), "deep blacks");
        let c = s.apply([200, 90, 100]);
        let (before, after) = ((200 - 90) as i32, c[0] as i32 - c[1] as i32);
        assert!(after >= before - 4, "no less color than before: {c:?}");
    }

    #[test]
    fn baking_changes_pixels_but_not_alpha_or_clear_ones() {
        let mut img = image::RgbaImage::from_pixel(2, 1, image::Rgba([220, 40, 40, 200]));
        img.put_pixel(1, 0, image::Rgba([220, 40, 40, 0]));
        let before = img.clone();
        bake_to_srgb(&mut img, "srgb");
        assert_eq!(img, before);
        bake_to_srgb(&mut img, "p3");
        assert_ne!(img.get_pixel(0, 0), before.get_pixel(0, 0));
        assert_eq!(img.get_pixel(0, 0).0[3], 200);
        assert_eq!(img.get_pixel(1, 0), before.get_pixel(1, 0), "clear pixels are left alone");
    }

    #[test]
    fn icc_files_have_a_valid_header_and_tags() {
        for p in &RGB_PROFILES {
            let icc = icc_profile(p.id);
            assert_eq!(u32::from_be_bytes(icc[0..4].try_into().unwrap()) as usize, icc.len(), "{}", p.id);
            assert_eq!(&icc[36..40], b"acsp");
            assert_eq!(&icc[12..16], b"mntr");
            assert_eq!(&icc[16..20], b"RGB ");
            assert_eq!(icc.len() % 4, 0);
            let n = u32::from_be_bytes(icc[128..132].try_into().unwrap()) as usize;
            assert_eq!(n, 9);
            let mut sigs = Vec::new();
            for i in 0..n {
                let at = 132 + i * 12;
                let (off, len) = (u32::from_be_bytes(icc[at + 4..at + 8].try_into().unwrap()) as usize, u32::from_be_bytes(icc[at + 8..at + 12].try_into().unwrap()) as usize);
                assert!(off + len <= icc.len() && off % 4 == 0);
                sigs.push(String::from_utf8_lossy(&icc[at..at + 4]).into_owned());
            }
            for want in ["desc", "wtpt", "rXYZ", "gXYZ", "bXYZ", "rTRC", "gTRC", "bTRC"] {
                assert!(sigs.iter().any(|s| s == want), "{want}");
            }
        }
        // the white point of the matrix columns adds up to D50 (so white is white)
        let m = to_d50(rgb_profile("p3"));
        let (x, y, z) = (m[0] + m[1] + m[2], m[3] + m[4] + m[5], m[6] + m[7] + m[8]);
        assert!((x - 0.9642).abs() < 0.003 && (y - 1.0).abs() < 0.003 && (z - 0.8249).abs() < 0.003, "{x} {y} {z}");
    }

    #[test]
    fn printing_dulls_neon_colors_but_leaves_printable_ones() {
        let pr = Printer::new("japan2001", Intent::Relative);
        let skin = pr.to_printable([225.0, 170.0, 150.0]);
        let shift: i32 = (0..3).map(|k| (skin[k] as i32 - [225, 170, 150][k]).abs()).sum();
        assert!(shift < 30, "a skin tone prints about as it looks: {skin:?}");
        let neon = pr.to_printable([0.0, 255.0, 60.0]);
        assert!(neon[1] < 245 || neon[0] > 20, "neon green is pulled in: {neon:?}");
        let (l_in, l_out) = (rgb_to_oklab([0.0, 1.0, 60.0 / 255.0])[0], rgb_to_oklab([neon[0] as f32 / 255.0, neon[1] as f32 / 255.0, neon[2] as f32 / 255.0])[0]);
        assert!(l_out < l_in + 0.05);
        let black = pr.to_printable([0.0, 0.0, 0.0]);
        assert!(black[0] > 5, "printed black is not pure black: {black:?}");
        // an uncoated press dulls more than the coated one
        let un = Printer::new("fogra29", Intent::Relative).to_printable([230.0, 40.0, 140.0]);
        let co = Printer::new("fogra39", Intent::Relative).to_printable([230.0, 40.0, 140.0]);
        let chroma = |c: [u8; 3]| rgb_to_oklab([c[0] as f32 / 255.0, c[1] as f32 / 255.0, c[2] as f32 / 255.0]);
        let (cu, cc) = (chroma(un), chroma(co));
        assert!(cu[1].hypot(cu[2]) < cc[1].hypot(cc[2]) + 0.001, "uncoated is duller");
    }

    #[test]
    fn cmyk_numbers_respect_the_ink_limit() {
        let pr = Printer::new("swop", Intent::Relative);
        let [c, m, y, k] = pr.cmyk([10, 10, 10]);
        assert!(c + m + y + k <= 300.5, "{c} {m} {y} {k}");
        assert!(k > 50.0, "dark gray is mostly black ink: {k}");
        let [c, m, _, _] = pr.cmyk([30, 140, 220]);
        assert!(c > m, "blue is cyan heavy: {c} {m}");
        let [c, m, y, k] = pr.cmyk([255, 255, 255]);
        assert!(c + m + y + k < 10.0, "paper white takes no ink");
    }

    #[test]
    fn unprintable_share_counts_only_visible_pixels() {
        let mut img = image::RgbaImage::from_pixel(10, 10, image::Rgba([0, 255, 60, 255]));
        let s = unprintable_share(&img, "japan2001", Intent::Relative);
        assert!(s > 0.9, "neon green everywhere: {s}");
        for p in img.pixels_mut() {
            p.0 = [0, 255, 60, 0];
        }
        assert_eq!(unprintable_share(&img, "japan2001", Intent::Relative), 0.0);
        let skin = image::RgbaImage::from_pixel(4, 4, image::Rgba([225, 170, 150, 255]));
        assert!(unprintable_share(&skin, "japan2001", Intent::Relative) < 0.1);
    }

    #[test]
    fn display_is_plain_for_srgb_and_changes_with_each_option() {
        let red = Color32::from_rgb(220, 30, 30);
        let plain = Display::new(Look::default());
        assert!(plain.is_plain());
        assert_eq!(plain.apply(red), red);
        let mut look = Look::default();
        look.profile = "p3".into();
        assert_ne!(Display::new(look.clone()).apply(red), red);
        look.profile = "srgb".into();
        look.proof = Proof::Print;
        let neon = Color32::from_rgb(0, 255, 60);
        assert_ne!(Display::new(look.clone()).apply(neon), neon);
        look.proof = Proof::Warn;
        let g = Display::new(look.clone()).apply(neon);
        assert_eq!((g.r(), g.g(), g.b()), (128, 128, 128), "flagged colors go flat gray");
        assert_eq!(Display::new(look.clone()).apply(Color32::from_rgb(235, 235, 235)), Color32::from_rgb(235, 235, 235), "printable colors are left");
        // the check view still applies on top, and clear pixels stay clear
        look.proof = Proof::Off;
        look.check = CheckView::Gray;
        let d = Display::new(look);
        let c = d.apply(red);
        assert!(c.r() == c.g() && c.g() == c.b());
        assert_eq!(d.apply(Color32::TRANSPARENT), Color32::TRANSPARENT);
    }

    // ---------- files ----------

    fn red_doc(profile: &str, bake: bool) -> crate::doc::Document {
        let mut d = crate::doc::Document::new(40, 30);
        for p in d.layers[0].tile_mut((0, 0)).iter_mut().take(40 * 30) {
            *p = Color32::from_rgb(200, 90, 90);
        }
        d.meta.rgb_profile = profile.into();
        d.meta.export_bake = bake;
        d
    }

    fn tmp(name: &str) -> std::path::PathBuf {
        std::env::temp_dir().join(format!("seviath_cp_{}_{name}", std::process::id()))
    }

    #[test]
    fn png_carries_the_profile_or_has_it_baked_in() {
        use crate::files::{Flat, export_flat};
        let read = |path: &std::path::Path| {
            let dec = png::Decoder::new(std::io::BufReader::new(std::fs::File::open(path).unwrap()));
            let mut r = dec.read_info().unwrap();
            let icc = r.info().icc_profile.as_ref().map(|c| c.to_vec());
            let mut buf = vec![0; r.output_buffer_size().unwrap()];
            r.next_frame(&mut buf).unwrap();
            (icc, [buf[0], buf[1], buf[2]])
        };
        let (a, b, c) = (tmp("a.png"), tmp("b.png"), tmp("c.png"));
        export_flat(&red_doc("p3", false), &a, Flat::Png, 90, false).unwrap();
        let (icc, px) = read(&a);
        assert_eq!(icc.as_deref(), Some(&icc_profile("p3")[..]), "the P3 profile is inside the file");
        assert_eq!(px, [200, 90, 90], "the pixels are the numbers you painted");
        export_flat(&red_doc("p3", true), &b, Flat::Png, 90, false).unwrap();
        let (icc, px) = read(&b);
        assert!(icc.is_none(), "baked: no profile needed");
        assert_ne!(px, [200, 90, 90], "baked: the pixels changed to look the same in sRGB");
        export_flat(&red_doc("srgb", false), &c, Flat::Png, 90, false).unwrap();
        let (icc, px) = read(&c);
        assert!(icc.is_none() && px == [200, 90, 90], "plain sRGB stays plain");
        let j = tmp("d.jpg");
        export_flat(&red_doc("adobe", false), &j, Flat::Jpeg, 90, false).unwrap();
        let bytes = std::fs::read(&j).unwrap();
        assert!(bytes.windows(12).any(|w| w == b"ICC_PROFILE "), "the JPEG carries the profile");
        let img = image::open(&j).unwrap();
        assert_eq!((img.width(), img.height()), (40, 30), "and still opens");
        for f in [a, b, c, j] {
            std::fs::remove_file(f).ok();
        }
    }

    #[test]
    fn the_profile_and_press_are_saved_with_the_project() {
        let mut d = red_doc("seviath", true);
        d.meta.cmyk_profile = "fogra29".into();
        d.meta.intent = "saturation".into();
        let path = tmp("p.seviath");
        crate::files::project::save(d.w, d.h, d.paper, &d.layers, &[], &d.meta, &path).unwrap();
        let back = crate::files::project::load(&path).unwrap();
        std::fs::remove_file(&path).ok();
        assert_eq!((back.meta.rgb_profile.as_str(), back.meta.cmyk_profile.as_str(), back.meta.intent.as_str(), back.meta.export_bake), ("seviath", "fogra29", "saturation", true));
        // junk from a file falls back to the defaults
        let m = crate::meta::ProjectMeta { rgb_profile: "x".into(), cmyk_profile: "y".into(), intent: "z".into(), ..Default::default() }.sanitized();
        assert_eq!((m.rgb_profile.as_str(), m.cmyk_profile.as_str(), m.intent.as_str()), ("srgb", "japan2001", "perceptual"));
    }
}
