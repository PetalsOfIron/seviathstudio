// SPDX-License-Identifier: GPL-3.0-or-later
// A font made from your own drawings. You draw a shape for every key you pick (any style: gothic,
// tattoo script, symbols), the app never has to "read" the letters. Each drawing is turned into clean
// outlines and written as a real TrueType font file (.ttf), kept in the project or saved for other programs.

use std::collections::BTreeMap;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::trace::{self, TraceMode, TraceOptions};

/// The drawing cell, in pad pixels.
pub const CELL_W: f32 = 320.0;
pub const CELL_H: f32 = 400.0;
/// Where the baseline is in the cell (pixels from the top).
pub const BASE: f32 = 288.0;
/// Font units per pad pixel (the em is 1000 units tall, the cell is 400 px).
pub const K: f32 = 2.5;
const ASCENT: i16 = 720;
const DESCENT: i16 = -280;
/// Space on each side of a letter, units.
const BEARING: f32 = 40.0;
const SPACE_ADVANCE: u16 = 350;

/// One drawn line: x, y, pressure 0..1, in cell pixels.
pub type Line = Vec<[f32; 3]>;

#[derive(Clone, PartialEq, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct HandFont {
    pub name: String,
    /// Line width at full pressure, in cell pixels.
    pub width: f32,
    /// Letters you did not draw in small form use the capital (and the other way round).
    pub caps_for_small: bool,
    pub glyphs: BTreeMap<char, Vec<Line>>,
}

impl Default for HandFont {
    fn default() -> Self {
        Self { name: "My handwriting".into(), width: 14.0, caps_for_small: false, glyphs: BTreeMap::new() }
    }
}

pub const GROUPS: [(&str, &str); 6] = [
    ("A B C", "ABCDEFGHIJKLMNOPQRSTUVWXYZ"),
    ("a b c", "abcdefghijklmnopqrstuvwxyz"),
    ("Latvian", "āčēģīķļņšūžĀČĒĢĪĶĻŅŠŪŽ"),
    ("0 1 2", "0123456789"),
    ("Signs", ".,:;!?-–'\"()&@/+*#%$€"),
    ("Other", ""),
];

impl HandFont {
    /// Keep numbers sane (the file may come from outside).
    pub fn sanitized(mut self) -> Self {
        self.name = self.name.chars().filter(|c| !c.is_control()).take(40).collect();
        if self.name.trim().is_empty() {
            self.name = "My handwriting".into();
        }
        self.width = if self.width.is_finite() { self.width.clamp(2.0, 60.0) } else { 14.0 };
        let mut n = 0;
        self.glyphs.retain(|c, g| {
            n += 1;
            g.truncate(200);
            for l in g.iter_mut() {
                l.truncate(5_000);
                l.retain(|p| p.iter().all(|v| v.is_finite()) && p[0].abs() < 5_000.0 && p[1].abs() < 5_000.0);
                for p in l.iter_mut() {
                    p[2] = p[2].clamp(0.0, 1.0);
                }
            }
            g.retain(|l| l.len() >= 2);
            n <= 600 && !c.is_control() && (*c as u32) < 0xFFFF
        });
        self
    }

    /// The drawing used for a character, with the capital / small fallback.
    pub fn glyph_for(&self, c: char) -> Option<&Vec<Line>> {
        let has = |c: char| self.glyphs.get(&c).filter(|g| !g.is_empty());
        has(c).or_else(|| {
            if !self.caps_for_small {
                return None;
            }
            let alt = if c.is_lowercase() { c.to_uppercase().next() } else { c.to_lowercase().next() }?;
            if alt.to_string().chars().count() == 1 { has(alt) } else { None }
        })
    }
}

// ---------- kept on this computer (the work in progress) ----------

fn draft_path() -> PathBuf {
    if cfg!(test) {
        return std::env::temp_dir().join(format!("seviath_handfont_{}.json", std::process::id()));
    }
    dirs::config_dir().unwrap_or_else(std::env::temp_dir).join("SeviathStudio").join("fonts").join("my-handwriting.json")
}

pub fn load_draft() -> HandFont {
    let p = draft_path();
    std::fs::metadata(&p)
        .ok()
        .filter(|m| m.len() < 64 << 20)
        .and_then(|_| std::fs::read(&p).ok())
        .and_then(|b| serde_json::from_slice::<HandFont>(&b).ok())
        .map(HandFont::sanitized)
        .unwrap_or_default()
}

pub fn save_draft(f: &HandFont) {
    let p = draft_path();
    if let Some(d) = p.parent() {
        let _ = std::fs::create_dir_all(d);
    }
    if let Ok(t) = serde_json::to_string(f) {
        let _ = std::fs::write(p, t);
    }
}

// ---------- from lines to outlines ----------

/// The drawing as a black-and-white picture, 1 pixel = 1 font unit.
fn rasterize(lines: &[Line], width: f32) -> (Vec<u8>, usize, usize) {
    let (w, h) = ((CELL_W * K) as usize, (CELL_H * K) as usize);
    let mut m = vec![0u8; w * h];
    let mut stamp = |cx: f32, cy: f32, r: f32| {
        let (x0, x1) = (((cx - r).floor() as i32).max(0), ((cx + r).ceil() as i32).min(w as i32 - 1));
        let (y0, y1) = (((cy - r).floor() as i32).max(0), ((cy + r).ceil() as i32).min(h as i32 - 1));
        for y in y0..=y1 {
            for x in x0..=x1 {
                let (dx, dy) = (x as f32 + 0.5 - cx, y as f32 + 0.5 - cy);
                if dx * dx + dy * dy <= r * r {
                    m[y as usize * w + x as usize] = 255;
                }
            }
        }
    };
    for l in lines {
        let r_of = |p: &[f32; 3]| (width * K * (0.25 + 0.75 * p[2]) / 2.0).max(1.0);
        for s in l.windows(2) {
            let (a, b) = (s[0], s[1]);
            let len = ((b[0] - a[0]) * K).hypot((b[1] - a[1]) * K);
            let n = (len / 0.7).ceil().max(1.0) as usize;
            for i in 0..=n {
                let t = i as f32 / n as f32;
                let p = [a[0] + (b[0] - a[0]) * t, a[1] + (b[1] - a[1]) * t, a[2] + (b[2] - a[2]) * t];
                stamp(p[0] * K, p[1] * K, r_of(&p));
            }
        }
    }
    (m, w, h)
}

fn signed_area(r: &[[f32; 2]]) -> f32 {
    let mut a = 0.0;
    for i in 0..r.len() {
        let (p, q) = (r[i], r[(i + 1) % r.len()]);
        a += p[0] * q[1] - q[0] * p[1];
    }
    a / 2.0
}

fn inside_ring(r: &[[f32; 2]], x: f32, y: f32) -> bool {
    let mut inside = false;
    let mut j = r.len() - 1;
    for i in 0..r.len() {
        let (a, b) = (r[i], r[j]);
        if (a[1] > y) != (b[1] > y) && x < (b[0] - a[0]) * (y - a[1]) / (b[1] - a[1]) + a[0] {
            inside = !inside;
        }
        j = i;
    }
    inside
}

fn rdp(pts: &[[f32; 2]], eps: f32, out: &mut Vec<[f32; 2]>) {
    // keeps the first point, drops the last (it is added by the next piece)
    if pts.len() < 3 {
        out.push(pts[0]);
        return;
    }
    let (a, b) = (pts[0], *pts.last().unwrap());
    let (dx, dy) = (b[0] - a[0], b[1] - a[1]);
    let len = dx.hypot(dy).max(1e-6);
    let (mut worst, mut wi) = (0.0, 0);
    for (i, p) in pts.iter().enumerate().take(pts.len() - 1).skip(1) {
        let d = ((p[0] - a[0]) * dy - (p[1] - a[1]) * dx).abs() / len;
        if d > worst {
            worst = d;
            wi = i;
        }
    }
    if worst > eps {
        rdp(&pts[..=wi], eps, out);
        rdp(&pts[wi..], eps, out);
    } else {
        out.push(a);
    }
}

fn simplify_ring(r: &[[f32; 2]], eps: f32) -> Vec<[f32; 2]> {
    if r.len() < 6 {
        return r.to_vec();
    }
    let mid = r.len() / 2;
    let mut out = Vec::new();
    rdp(&r[..=mid], eps, &mut out);
    let mut second = r[mid..].to_vec();
    second.push(r[0]);
    rdp(&second, eps, &mut out);
    out
}

/// Clean outlines of a drawing, in font units (x right from the cell's left edge, y up from the baseline).
/// Outer outlines go clockwise and holes counter-clockwise, as TrueType wants.
pub fn outlines(lines: &[Line], width: f32) -> Vec<Vec<[f32; 2]>> {
    let (mask, w, h) = rasterize(lines, width);
    if mask.iter().all(|v| *v == 0) {
        return Vec::new();
    }
    let rgba: Vec<u8> = mask.iter().flat_map(|v| [0, 0, 0, *v]).collect();
    let o = TraceOptions { mode: TraceMode::Silhouette, threshold: 0.4, color_smooth: 0.3, min_area: 30, single_color: [0, 0, 0], ..TraceOptions::default() };
    let res = trace::trace(&rgba, w, h, &o, &|_| {});
    let base = BASE * K;
    let mut rings: Vec<Vec<[f32; 2]>> = Vec::new();
    for f in res.fills {
        for r in f.rings {
            let r = simplify_ring(&r, 1.6);
            if r.len() >= 3 {
                rings.push(r.iter().map(|p| [p[0], base - p[1]]).collect());
            }
        }
    }
    let depth: Vec<usize> = (0..rings.len())
        .map(|i| {
            let p = rings[i][0];
            (0..rings.len()).filter(|&j| j != i && inside_ring(&rings[j], p[0], p[1])).count()
        })
        .collect();
    for (r, d) in rings.iter_mut().zip(depth) {
        let outer = d % 2 == 0;
        // y points up here: a clockwise ring has a negative area
        if (signed_area(r) < 0.0) != outer {
            r.reverse();
        }
    }
    rings
}


/// Clean lines from a drawing on the canvas (unmultiplied RGBA pixels): they are traced like lineart,
/// then scaled to fit the cell and set on the baseline.
pub fn lines_from_pixels(rgba: &[u8], w: usize, h: usize) -> Vec<Line> {
    let o = TraceOptions { mode: TraceMode::Lines, threshold: 0.45, smooth: 0.5, detail: 12, min_length: 6.0, keep_width: true, single_color: [0, 0, 0], ..TraceOptions::default() };
    // very big pictures are traced at a smaller size
    let (rgba, w, h, back) = if w.max(h) > 1600 {
        let k = 1600.0 / w.max(h) as f32;
        let (nw, nh) = (((w as f32 * k) as u32).max(3), ((h as f32 * k) as u32).max(3));
        match image::RgbaImage::from_raw(w as u32, h as u32, rgba.to_vec()) {
            Some(img) => (image::imageops::resize(&img, nw, nh, image::imageops::FilterType::Triangle).into_raw(), nw as usize, nh as usize, 1.0 / k),
            None => return Vec::new(),
        }
    } else {
        (rgba.to_vec(), w, h, 1.0)
    };
    let res = trace::trace(&rgba, w, h, &o, &|_| {});
    let mut lines: Vec<Line> = res.lines.iter().map(|l| l.pts.iter().map(|p| [p[0] * back, p[1] * back, p[2]]).collect::<Line>()).filter(|l| l.len() >= 2).collect();
    let (mut x0, mut y0, mut x1, mut y1) = (f32::MAX, f32::MAX, f32::MIN, f32::MIN);
    for p in lines.iter().flatten() {
        x0 = x0.min(p[0]);
        y0 = y0.min(p[1]);
        x1 = x1.max(p[0]);
        y1 = y1.max(p[1]);
    }
    if x1 <= x0 && y1 <= y0 {
        return Vec::new();
    }
    // fits in 280 x 240 pixels, standing on the baseline
    let k = (280.0 / (x1 - x0).max(1.0)).min(240.0 / (y1 - y0).max(1.0));
    for p in lines.iter_mut().flatten() {
        p[0] = 20.0 + (p[0] - x0) * k;
        p[1] = BASE - (y1 - p[1]) * k;
    }
    lines
}

// ---------- writing the font file ----------

struct Glyph {
    contours: Vec<Vec<[i16; 2]>>,
    advance: u16,
    lsb: i16,
    bbox: [i16; 4],
}

fn u16b(v: &mut Vec<u8>, x: u16) {
    v.extend_from_slice(&x.to_be_bytes());
}
fn i16b(v: &mut Vec<u8>, x: i16) {
    v.extend_from_slice(&x.to_be_bytes());
}
fn u32b(v: &mut Vec<u8>, x: u32) {
    v.extend_from_slice(&x.to_be_bytes());
}

fn checksum(b: &[u8]) -> u32 {
    let mut s = 0u32;
    for c in b.chunks(4) {
        let mut w = [0u8; 4];
        w[..c.len()].copy_from_slice(c);
        s = s.wrapping_add(u32::from_be_bytes(w));
    }
    s
}

fn make_glyph(rings: Vec<Vec<[f32; 2]>>) -> Glyph {
    let (mut x0, mut x1) = (f32::MAX, f32::MIN);
    for p in rings.iter().flatten() {
        x0 = x0.min(p[0]);
        x1 = x1.max(p[0]);
    }
    let shift = BEARING - x0;
    let mut contours = Vec::new();
    let mut bbox = [i16::MAX, i16::MAX, i16::MIN, i16::MIN];
    for r in &rings {
        let c: Vec<[i16; 2]> = r.iter().map(|p| [(p[0] + shift).round().clamp(-30000.0, 30000.0) as i16, p[1].round().clamp(-30000.0, 30000.0) as i16]).collect();
        for p in &c {
            bbox = [bbox[0].min(p[0]), bbox[1].min(p[1]), bbox[2].max(p[0]), bbox[3].max(p[1])];
        }
        contours.push(c);
    }
    let advance = ((x1 - x0) + BEARING * 2.0).round().clamp(100.0, 4000.0) as u16;
    Glyph { contours, advance, lsb: bbox[0], bbox }
}

fn glyf_bytes(g: &Glyph) -> Vec<u8> {
    let mut v = Vec::new();
    if g.contours.is_empty() {
        return v;
    }
    i16b(&mut v, g.contours.len() as i16);
    for b in g.bbox {
        i16b(&mut v, b);
    }
    let mut end = -1i32;
    for c in &g.contours {
        end += c.len() as i32;
        u16b(&mut v, end as u16);
    }
    u16b(&mut v, 0); // no instructions
    let n: usize = g.contours.iter().map(Vec::len).sum();
    v.extend(std::iter::repeat_n(0x01u8, n)); // on the curve, x and y as 16-bit changes
    let mut prev = 0i32;
    for p in g.contours.iter().flatten() {
        i16b(&mut v, (p[0] as i32 - prev) as i16);
        prev = p[0] as i32;
    }
    prev = 0;
    for p in g.contours.iter().flatten() {
        i16b(&mut v, (p[1] as i32 - prev) as i16);
        prev = p[1] as i32;
    }
    while v.len() % 4 != 0 {
        v.push(0);
    }
    v
}

fn utf16(s: &str) -> Vec<u8> {
    s.encode_utf16().flat_map(|u| u.to_be_bytes()).collect()
}

/// The font as a .ttf file. Only the keys you drew are in it (plus the space).
pub fn build_ttf(f: &HandFont) -> Result<Vec<u8>, String> {
    // every key that has a drawing (or a capital / small stand-in)
    let mut keys: Vec<char> = f.glyphs.keys().copied().collect();
    if f.caps_for_small {
        for c in f.glyphs.keys().copied().collect::<Vec<_>>() {
            for alt in c.to_uppercase().chain(c.to_lowercase()) {
                if (alt as u32) < 0xFFFF && !keys.contains(&alt) {
                    keys.push(alt);
                }
            }
        }
    }
    keys.retain(|c| *c != ' ' && (*c as u32) < 0xFFFF && f.glyph_for(*c).is_some());
    keys.sort();
    keys.dedup();
    if keys.is_empty() {
        return Err("Draw at least one letter first.".into());
    }
    let mut glyphs: Vec<Glyph> = Vec::new();
    // 0: the box shown for a missing letter
    let nd = |a: [i16; 2], b: [i16; 2]| vec![a, [a[0], b[1]], b, [b[0], a[1]]];
    let outer = nd([60, 0], [440, 700]);
    let mut inner = nd([110, 50], [390, 650]);
    inner.reverse();
    glyphs.push(Glyph { contours: vec![outer, inner], advance: 500, lsb: 60, bbox: [60, 0, 440, 700] });
    // 1: the space
    glyphs.push(Glyph { contours: Vec::new(), advance: SPACE_ADVANCE, lsb: 0, bbox: [0; 4] });
    let mut map: Vec<(u16, u16)> = vec![(0x20, 1)];
    for c in &keys {
        let lines = f.glyph_for(*c).unwrap();
        let rings = outlines(lines, f.width);
        if rings.is_empty() {
            continue;
        }
        map.push((*c as u16, glyphs.len() as u16));
        glyphs.push(make_glyph(rings));
    }
    if glyphs.len() < 3 {
        return Err("Nothing could be made from those drawings.".into());
    }
    map.sort();
    let n = glyphs.len();

    let mut glyf = Vec::new();
    let mut loca = Vec::new();
    for g in &glyphs {
        u32b(&mut loca, glyf.len() as u32);
        glyf.extend(glyf_bytes(g));
    }
    u32b(&mut loca, glyf.len() as u32);

    let mut hmtx = Vec::new();
    for g in &glyphs {
        u16b(&mut hmtx, g.advance);
        i16b(&mut hmtx, g.lsb);
    }
    let all = |i: usize| glyphs.iter().filter(|g| !g.contours.is_empty()).map(move |g| g.bbox[i]);
    let (xmin, ymin) = (all(0).min().unwrap_or(0), all(1).min().unwrap_or(0));
    let (xmax, ymax) = (all(2).max().unwrap_or(0), all(3).max().unwrap_or(0));
    let adv_max = glyphs.iter().map(|g| g.advance).max().unwrap_or(0);
    let max_pts = glyphs.iter().map(|g| g.contours.iter().map(Vec::len).sum::<usize>()).max().unwrap_or(0);
    let max_ctr = glyphs.iter().map(|g| g.contours.len()).max().unwrap_or(0);

    let mut head = Vec::new();
    u32b(&mut head, 0x0001_0000);
    u32b(&mut head, 0x0001_0000);
    u32b(&mut head, 0); // checkSumAdjustment, set below
    u32b(&mut head, 0x5F0F_3CF5);
    u16b(&mut head, 0x0003);
    u16b(&mut head, 1000);
    head.extend([0u8; 16]); // created, modified
    for v in [xmin, ymin, xmax, ymax] {
        i16b(&mut head, v);
    }
    u16b(&mut head, 0);
    u16b(&mut head, 8);
    i16b(&mut head, 2);
    i16b(&mut head, 1); // long loca
    i16b(&mut head, 0);

    let mut hhea = Vec::new();
    u32b(&mut hhea, 0x0001_0000);
    i16b(&mut hhea, ASCENT);
    i16b(&mut hhea, DESCENT);
    i16b(&mut hhea, 0);
    u16b(&mut hhea, adv_max);
    i16b(&mut hhea, xmin);
    i16b(&mut hhea, 0);
    i16b(&mut hhea, xmax);
    i16b(&mut hhea, 1);
    i16b(&mut hhea, 0);
    i16b(&mut hhea, 0);
    hhea.extend([0u8; 8]);
    i16b(&mut hhea, 0);
    u16b(&mut hhea, n as u16);

    let mut maxp = Vec::new();
    u32b(&mut maxp, 0x0001_0000);
    u16b(&mut maxp, n as u16);
    u16b(&mut maxp, max_pts as u16);
    u16b(&mut maxp, max_ctr as u16);
    for v in [0u16, 0, 2, 0, 0, 0, 0, 0, 0, 0, 0] {
        u16b(&mut maxp, v);
    }

    let mut os2 = Vec::new();
    u16b(&mut os2, 4);
    i16b(&mut os2, (hmtx.len() as i32 / n as i32 / 2).min(2000) as i16);
    u16b(&mut os2, 400);
    u16b(&mut os2, 5);
    u16b(&mut os2, 0);
    for v in [650i16, 600, 0, 75, 650, 600, 0, 350, 50, 300] {
        i16b(&mut os2, v);
    }
    i16b(&mut os2, 0);
    os2.extend([0u8; 10]);
    for v in [0u32, 0, 0, 0] {
        u32b(&mut os2, v);
    }
    os2.extend(b"SEVI");
    u16b(&mut os2, 0x0040);
    u16b(&mut os2, map.first().map(|m| m.0).unwrap_or(0x20));
    u16b(&mut os2, map.last().map(|m| m.0).unwrap_or(0x20));
    i16b(&mut os2, ASCENT);
    i16b(&mut os2, DESCENT);
    i16b(&mut os2, 0);
    u16b(&mut os2, ASCENT as u16);
    u16b(&mut os2, DESCENT.unsigned_abs());
    u32b(&mut os2, 1);
    u32b(&mut os2, 0);
    i16b(&mut os2, 500);
    i16b(&mut os2, 700);
    u16b(&mut os2, 0);
    u16b(&mut os2, 0x20);
    u16b(&mut os2, 1);

    // cmap, format 4: runs of keys that follow each other with glyphs that follow each other
    let mut segs: Vec<(u16, u16, u16)> = Vec::new(); // start, end, first glyph
    for (c, g) in &map {
        match segs.last_mut() {
            Some(s) if s.1 + 1 == *c && s.2 + (s.1 - s.0) + 1 == *g => s.1 = *c,
            _ => segs.push((*c, *c, *g)),
        }
    }
    segs.push((0xFFFF, 0xFFFF, 0));
    let sc = segs.len() as u16;
    let mut sub = Vec::new();
    u16b(&mut sub, 4);
    u16b(&mut sub, 16 + sc * 8);
    u16b(&mut sub, 0);
    u16b(&mut sub, sc * 2);
    let mut pow = 1u16;
    let mut sel = 0u16;
    while pow * 2 <= sc {
        pow *= 2;
        sel += 1;
    }
    u16b(&mut sub, pow * 2);
    u16b(&mut sub, sel);
    u16b(&mut sub, sc * 2 - pow * 2);
    for s in &segs {
        u16b(&mut sub, s.1);
    }
    u16b(&mut sub, 0);
    for s in &segs {
        u16b(&mut sub, s.0);
    }
    for s in &segs {
        let delta = if s.0 == 0xFFFF { 1 } else { s.2.wrapping_sub(s.0) };
        u16b(&mut sub, delta);
    }
    for _ in &segs {
        u16b(&mut sub, 0);
    }
    let mut cmap = Vec::new();
    u16b(&mut cmap, 0);
    u16b(&mut cmap, 1);
    u16b(&mut cmap, 3);
    u16b(&mut cmap, 1);
    u32b(&mut cmap, 12);
    cmap.extend(sub);

    let family: String = f.name.chars().filter(|c| !c.is_control()).collect();
    let ps: String = family.chars().filter(|c| c.is_ascii_alphanumeric()).collect::<String>();
    let ps = if ps.is_empty() { "MyHandwriting".to_owned() } else { ps };
    let strings = [(1u16, family.clone()), (2, "Regular".into()), (3, format!("{ps}-SeviathStudio")), (4, family.clone()), (6, format!("{ps}-Regular"))];
    let mut name = Vec::new();
    u16b(&mut name, 0);
    u16b(&mut name, strings.len() as u16);
    u16b(&mut name, 6 + strings.len() as u16 * 12);
    let mut pool = Vec::new();
    for (id, s) in &strings {
        let b = utf16(s);
        u16b(&mut name, 3);
        u16b(&mut name, 1);
        u16b(&mut name, 0x409);
        u16b(&mut name, *id);
        u16b(&mut name, b.len() as u16);
        u16b(&mut name, pool.len() as u16);
        pool.extend(b);
    }
    name.extend(pool);

    let mut post = Vec::new();
    u32b(&mut post, 0x0003_0000);
    u32b(&mut post, 0);
    i16b(&mut post, -100);
    i16b(&mut post, 50);
    for _ in 0..5 {
        u32b(&mut post, 0);
    }

    let mut tables: Vec<([u8; 4], Vec<u8>)> = vec![
        (*b"OS/2", os2),
        (*b"cmap", cmap),
        (*b"glyf", glyf),
        (*b"head", head),
        (*b"hhea", hhea),
        (*b"hmtx", hmtx),
        (*b"loca", loca),
        (*b"maxp", maxp),
        (*b"name", name),
        (*b"post", post),
    ];
    tables.sort_by_key(|t| t.0);
    let nt = tables.len() as u16;
    let mut out = Vec::new();
    u32b(&mut out, 0x0001_0000);
    u16b(&mut out, nt);
    let mut p2 = 1u16;
    let mut e = 0u16;
    while p2 * 2 <= nt {
        p2 *= 2;
        e += 1;
    }
    u16b(&mut out, p2 * 16);
    u16b(&mut out, e);
    u16b(&mut out, nt * 16 - p2 * 16);
    let mut offset = 12 + tables.len() * 16;
    let mut head_at = 0;
    for (tag, data) in &tables {
        out.extend_from_slice(tag);
        u32b(&mut out, checksum(data));
        u32b(&mut out, offset as u32);
        u32b(&mut out, data.len() as u32);
        if tag == b"head" {
            head_at = offset;
        }
        offset += data.len().div_ceil(4) * 4;
    }
    for (_, data) in &tables {
        out.extend_from_slice(data);
        while out.len() % 4 != 0 {
            out.push(0);
        }
    }
    let adj = 0xB1B0_AFBAu32.wrapping_sub(checksum(&out));
    out[head_at + 8..head_at + 12].copy_from_slice(&adj.to_be_bytes());
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A ring (an O), a plain stroke (an I) and a gothic-looking zigzag.
    fn sample() -> HandFont {
        let ring: Line = (0..=40).map(|i| {
            let a = i as f32 / 40.0 * std::f32::consts::TAU;
            [160.0 + a.cos() * 90.0, 190.0 + a.sin() * 100.0, 1.0]
        }).collect();
        let bar: Line = vec![[160.0, 90.0, 1.0], [160.0, 288.0, 1.0]];
        let zig: Line = vec![[80.0, 100.0, 1.0], [150.0, 288.0, 1.0], [200.0, 100.0, 1.0], [260.0, 288.0, 1.0]];
        let mut f = HandFont { name: "Test hand".into(), ..HandFont::default() };
        f.glyphs.insert('O', vec![ring]);
        f.glyphs.insert('I', vec![bar]);
        f.glyphs.insert('ā', vec![zig.clone()]);
        f.glyphs.insert('☠', vec![zig.clone()]);
        f.glyphs.insert('𝔄', vec![zig]);
        f
    }

    #[test]
    fn lines_become_outlines_with_holes() {
        let f = sample();
        let o = outlines(&f.glyphs[&'O'], f.width);
        assert_eq!(o.len(), 2, "a ring has an outside and a hole");
        let areas: Vec<f32> = o.iter().map(|r| signed_area(r)).collect();
        assert!(areas.iter().any(|a| *a < 0.0) && areas.iter().any(|a| *a > 0.0), "opposite directions: {areas:?}");
        assert_eq!(outlines(&f.glyphs[&'I'], f.width).len(), 1);
        assert!(outlines(&[], 14.0).is_empty());
        // the bar is as tall as it was drawn: from 90 to 288 px = 495 units above the baseline
        let r = &outlines(&f.glyphs[&'I'], f.width)[0];
        let top = r.iter().map(|p| p[1]).fold(f32::MIN, f32::max);
        assert!(top > 480.0 && top < 560.0, "top {top}");
    }

    #[test]
    fn the_font_file_opens_and_has_the_drawn_letters() {
        let f = sample();
        let bytes = build_ttf(&f).unwrap();
        let face = ttf_parser::Face::parse(&bytes, 0).expect("a valid font");
        assert_eq!(face.units_per_em(), 1000);
        for c in ['O', 'I', 'ā', ' '] {
            assert!(face.glyph_index(c).is_some(), "has {c}");
        }
        assert!(face.glyph_index('A').is_none(), "nothing for a key you did not draw");
        assert!(face.glyph_index('☠').is_some(), "symbols work too");
        assert!(face.glyph_index('𝔄').is_none(), "keys beyond the basic range are skipped");
        let o = face.glyph_index('O').unwrap();
        struct Count(usize);
        impl ttf_parser::OutlineBuilder for Count {
            fn move_to(&mut self, _: f32, _: f32) { self.0 += 1; }
            fn line_to(&mut self, _: f32, _: f32) {}
            fn quad_to(&mut self, _: f32, _: f32, _: f32, _: f32) {}
            fn curve_to(&mut self, _: f32, _: f32, _: f32, _: f32, _: f32, _: f32) {}
            fn close(&mut self) {}
        }
        let mut c = Count(0);
        let bb = face.outline_glyph(o, &mut c).expect("has an outline");
        assert_eq!(c.0, 2, "outside and hole");
        assert!(bb.width() > 150 && bb.height() > 200, "{bb:?}");
        assert!(face.glyph_hor_advance(o).unwrap() as i32 > bb.width() as i32);
        assert_eq!(face.glyph_hor_advance(face.glyph_index(' ').unwrap()), Some(SPACE_ADVANCE));
        assert!(crate::fonts::usable(&bytes));
        assert!(build_ttf(&HandFont::default()).is_err());
    }

    #[test]
    fn capitals_can_stand_in_for_small_letters() {
        let mut f = sample();
        assert!(f.glyph_for('o').is_none());
        f.caps_for_small = true;
        assert!(f.glyph_for('o').is_some() && f.glyph_for('i').is_some());
        assert!(f.glyph_for('Ā').is_some(), "also for the Latvian letters");
        let face_bytes = build_ttf(&f).unwrap();
        let face = ttf_parser::Face::parse(&face_bytes, 0).unwrap();
        assert!(face.glyph_index('o').is_some() && face.glyph_index('O').is_some());
    }

    #[test]
    fn the_font_works_as_text_on_a_layer_and_the_hole_stays_a_hole() {
        let f = sample();
        let bytes = build_ttf(&f).unwrap();
        let pf = crate::fonts::ProjectFont { name: "Test hand".into(), data: crate::brush::tipimage::base64_encode(&bytes) };
        let t = crate::text::TextData { text: "O".into(), font: "Test hand".into(), size: 200.0, ..crate::text::TextData::default() };
        let fills = crate::text::build(&t, std::slice::from_ref(&pf));
        assert!(!fills.is_empty(), "the text has shapes");
        let b = crate::vector::fill_bounds(&fills[0]);
        let (cx, cy) = ((b.0 + b.2) as f32 / 2.0, (b.1 + b.3) as f32 / 2.0);
        assert!(!crate::vector::inside_fill(&fills[0], cx, cy), "the middle of the O is empty");
        assert!(fills.iter().any(|f| crate::vector::inside_fill(f, b.0 as f32 + 8.0 + 0.0, cy)) || fills.iter().any(|f| crate::vector::inside_fill(f, b.2 as f32 - 8.0, cy)), "the ring itself is filled");
    }

    #[test]
    fn drafts_and_odd_files_are_handled() {
        let f = sample();
        save_draft(&f);
        let back = load_draft();
        assert_eq!(back.glyphs.len(), 4, "the key beyond the basic range is dropped");
        assert_eq!(back.glyphs[&'ā'], f.glyphs[&'ā']);
        let mut bad = f.clone();
        bad.width = f32::NAN;
        bad.glyphs.insert('x', vec![vec![[f32::NAN, 0.0, 0.0], [1.0, 1.0, 9.0]]]);
        let s = bad.sanitized();
        assert_eq!(s.width, 14.0);
        assert!(!s.glyphs.contains_key(&'x') || s.glyphs[&'x'].is_empty());
        std::fs::remove_file(draft_path()).ok();
    }

    #[test]
    fn a_drawing_on_the_canvas_becomes_clean_lines_in_the_cell() {
        // a fat slanted bar in a big picture
        let (w, h) = (500usize, 400usize);
        let px: Vec<u8> = (0..w * h).flat_map(|i| {
            let (x, y) = ((i % w) as f32, (i / w) as f32);
            let on = (x - (100.0 + y * 0.5)).abs() < 8.0 && (60.0..340.0).contains(&y);
            if on { [20u8, 20, 20, 255] } else { [0u8; 4] }
        }).collect();
        let lines = lines_from_pixels(&px, w, h);
        assert_eq!(lines.len(), 1, "one clean line");
        let (lo, hi) = lines[0].iter().fold((f32::MAX, f32::MIN), |a, p| (a.0.min(p[1]), a.1.max(p[1])));
        assert!((hi - BASE).abs() < 4.0, "stands on the baseline: {hi}");
        assert!(hi - lo > 150.0 && hi - lo <= 242.0, "fits the cell: {}", hi - lo);
        assert!(lines[0].iter().all(|p| p[0] >= 0.0 && p[0] <= CELL_W));
        assert!(lines_from_pixels(&vec![0u8; w * h * 4], w, h).is_empty(), "an empty layer gives nothing");
    }
}
