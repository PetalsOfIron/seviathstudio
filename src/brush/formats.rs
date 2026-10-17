// SPDX-License-Identifier: GPL-3.0-or-later
// Brushes from and for other programs: Photoshop .abr (also read by Clip Studio Paint), GIMP .gbr and
// .gih, Krita .kpp presets and .bundle files; and out again as .abr (version 2), .kpp and a tip PNG.
// Every program has its own engine, so the tip, size, spacing, angle, roundness, opacity and pressure
// go across; effects only SeviathStudio has (wet rim, particles, textures…) do not.

use std::collections::HashMap;
use std::io::Read;

use super::tipimage::{TipImage, base64_decode, base64_encode};
use super::userlib::UserBrush;
use super::{BrushSettings, Tip};

/// What an import found, and what it could not use.
#[derive(Default)]
pub struct Imported {
    pub brushes: Vec<UserBrush>,
    pub notes: Vec<String>,
}

const MAX_PIXELS: usize = 16_000_000;

// ---------- reading bytes safely ----------

struct Reader<'a> {
    b: &'a [u8],
    pos: usize,
}

impl<'a> Reader<'a> {
    fn new(b: &'a [u8]) -> Self {
        Reader { b, pos: 0 }
    }
    fn left(&self) -> usize {
        self.b.len().saturating_sub(self.pos)
    }
    fn take(&mut self, n: usize) -> Option<&'a [u8]> {
        let s = self.b.get(self.pos..self.pos.checked_add(n)?)?;
        self.pos += n;
        Some(s)
    }
    fn u8(&mut self) -> Option<u8> {
        self.take(1).map(|s| s[0])
    }
    fn u16(&mut self) -> Option<u16> {
        self.take(2).map(|s| u16::from_be_bytes([s[0], s[1]]))
    }
    fn i32(&mut self) -> Option<i32> {
        self.take(4).map(|s| i32::from_be_bytes([s[0], s[1], s[2], s[3]]))
    }
    fn u32(&mut self) -> Option<u32> {
        self.take(4).map(|s| u32::from_be_bytes([s[0], s[1], s[2], s[3]]))
    }
}

fn base(file: &str) -> String {
    std::path::Path::new(file).file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_else(|| "Imported brush".into())
}

/// A brush that stamps a picture given as paint values (255 = paint).
fn stamp_brush(name: String, vals: &[u8], w: usize, h: usize, size: f32, spacing: f32) -> Option<UserBrush> {
    if w == 0 || h == 0 || w * h > MAX_PIXELS || vals.len() != w * h {
        return None;
    }
    let mut img = image::RgbaImage::new(w as u32, h as u32);
    for (p, v) in img.pixels_mut().zip(vals) {
        *p = image::Rgba([0, 0, 0, *v]);
    }
    // a brush that is paint everywhere has no alpha to read: make sure it still counts
    let tip = if vals.iter().all(|v| *v >= 250) {
        let n = w.max(h).min(super::tipimage::MAX_SIDE as usize);
        TipImage::from_square(n as u32, vec![255; n * n])?
    } else {
        TipImage::from_picture(&img)?
    };
    let settings = BrushSettings { tip: Tip::Image, tip_image: Some(tip), size: size.clamp(1.0, 500.0), spacing: spacing.clamp(0.02, 2.0), hardness: 1.0, roundness: 1.0, size_pressure: true, min_size: 0.5, ..BrushSettings::default() };
    Some(UserBrush { name: name.chars().take(60).collect(), about: "Imported brush".into(), settings, ..UserBrush::default() }.sanitized())
}

// ---------- Photoshop .abr ----------

/// Photoshop's simple run-length packing: one row of `row` bytes from `count` packed bytes.
fn unpack_bits(r: &mut Reader, out: &mut [u8], count: usize) -> Option<()> {
    let end = r.pos.checked_add(count)?;
    let mut o = 0;
    while r.pos < end && o < out.len() {
        let n = r.u8()? as i8;
        if n >= 0 {
            for _ in 0..=n {
                let v = r.u8()?;
                if o < out.len() {
                    out[o] = v;
                    o += 1;
                }
            }
        } else if n != -128 {
            let v = r.u8()?;
            for _ in 0..(1 - n as i32) {
                if o < out.len() {
                    out[o] = v;
                    o += 1;
                }
            }
        }
    }
    r.pos = end;
    Some(())
}

struct Sampled {
    vals: Vec<u8>,
    w: usize,
    h: usize,
}

/// The picture of one sampled brush: rectangle, bit depth, compression, then pixels.
fn read_sampled(r: &mut Reader, limit: usize) -> Option<Sampled> {
    let (top, left, bottom, right) = (r.i32()?, r.i32()?, r.i32()?, r.i32()?);
    let depth = r.u16()?;
    let comp = r.u8()?;
    let (w, h) = (right.checked_sub(left)?, bottom.checked_sub(top)?);
    if w <= 0 || h <= 0 || w > 8000 || h > 8000 || (depth != 8 && depth != 16) || comp > 1 || (w as usize) * (h as usize) > MAX_PIXELS {
        return None;
    }
    let (w, h, bpp) = (w as usize, h as usize, depth as usize / 8);
    let mut raw = vec![0u8; w * h * bpp];
    if comp == 0 {
        if r.pos + raw.len() > limit {
            return None;
        }
        let n = raw.len();
        raw.copy_from_slice(r.take(n)?);
    } else {
        let counts: Vec<usize> = (0..h).map(|_| r.u16().map(|c| c as usize)).collect::<Option<_>>()?;
        for (y, c) in counts.iter().enumerate() {
            if r.pos + c > limit {
                return None;
            }
            unpack_bits(r, &mut raw[y * w * bpp..(y + 1) * w * bpp], *c)?;
        }
    }
    let vals = (0..w * h).map(|i| raw[i * bpp]).collect(); // 16 bit: the high byte is enough
    Some(Sampled { vals, w, h })
}

/// Looks for the brush rectangle header inside a block (its place differs between versions).
fn find_sampled(buf: &[u8], from: usize, to: usize) -> Option<Sampled> {
    let try_at = |at: usize| -> Option<Sampled> {
        if at + 19 > to {
            return None;
        }
        let mut r = Reader::new(buf);
        r.pos = at;
        read_sampled(&mut r, to).filter(|s| s.w * s.h > 1)
    };
    for skip in [301, 47, 37, 264] {
        if let Some(s) = try_at(from + skip) {
            return Some(s);
        }
    }
    (from..to.saturating_sub(19).min(from + 1200)).find_map(try_at)
}

fn find(bytes: &[u8], sig: &[u8], from: usize) -> Option<usize> {
    bytes.get(from..)?.windows(sig.len()).position(|w| w == sig).map(|p| p + from)
}

/// Names, spacings and diameters from the brush descriptions.
fn desc_values(bytes: &[u8]) -> (Vec<String>, Vec<f32>, Vec<f32>) {
    let (mut names, mut spacing, mut diam) = (Vec::new(), Vec::new(), Vec::new());
    let be32 = |at: usize| bytes.get(at..at + 4).map(|s| u32::from_be_bytes([s[0], s[1], s[2], s[3]]) as usize);
    let mut i = 0;
    while let Some(p) = find(bytes, b"Nm  TEXT", i) {
        if let Some(n) = be32(p + 8) {
            let mut s = String::new();
            for k in 0..n.min(200) {
                match bytes.get(p + 12 + k * 2..p + 14 + k * 2) {
                    Some(c) => {
                        let u = u16::from_be_bytes([c[0], c[1]]);
                        if u != 0 {
                            s.push(char::from_u32(u as u32).unwrap_or('?'));
                        }
                    }
                    None => break,
                }
            }
            names.push(s.trim().to_string());
        }
        i = p + 8;
    }
    let f64_at = |at: usize| bytes.get(at..at + 8).map(|s| f64::from_be_bytes([s[0], s[1], s[2], s[3], s[4], s[5], s[6], s[7]]) as f32);
    let mut i = 0;
    while let Some(p) = find(bytes, b"SpcnUntF#Prc", i) {
        spacing.extend(f64_at(p + 12));
        i = p + 12;
    }
    let mut i = 0;
    while let Some(p) = find(bytes, b"DmtrUntF#Pxl", i) {
        diam.extend(f64_at(p + 12));
        i = p + 12;
    }
    (names, spacing, diam)
}

pub fn parse_abr(buf: &[u8], file: &str) -> Imported {
    let mut out = Imported::default();
    let mut r = Reader::new(buf);
    let Some(version) = r.u16() else {
        out.notes.push(format!("{file}: this is not a Photoshop brush file."));
        return out;
    };
    if version == 1 || version == 2 {
        let count = r.u16().unwrap_or(0) as usize;
        for i in 0..count {
            if r.left() <= 6 {
                break;
            }
            let (Some(kind), Some(size)) = (r.u16(), r.u32()) else { break };
            let next = r.pos + size as usize;
            if kind == 1 {
                // a computed (round) brush
                let _ = r.u32();
                let (spacing, diameter, roundness) = (r.u16().unwrap_or(25), r.u16().unwrap_or(20), r.u16().unwrap_or(100));
                let angle = r.u16().unwrap_or(0) as i16;
                let hardness = r.u16().unwrap_or(100);
                let settings = BrushSettings { size: diameter as f32, spacing: (spacing as f32 / 100.0).max(0.02), roundness: (roundness as f32 / 100.0).max(0.05), angle: angle as f32, hardness: hardness as f32 / 100.0, ..BrushSettings::default() };
                out.brushes.push(UserBrush { name: format!("{} {}", base(file), i + 1), settings, about: "Imported brush".into(), ..UserBrush::default() }.sanitized());
            } else if kind == 2 {
                let _ = r.u32();
                let spacing = r.u16().unwrap_or(25);
                let mut name = format!("{} {}", base(file), i + 1);
                if version == 2 {
                    let n = r.u32().unwrap_or(0) as usize;
                    let s: String = (0..n.min(200)).filter_map(|_| r.u16()).filter(|c| *c != 0).map(|c| char::from_u32(c as u32).unwrap_or('?')).collect();
                    if !s.trim().is_empty() {
                        name = s.trim().to_string();
                    }
                }
                let _ = r.u8(); // antialias
                r.pos += 8; // short bounds
                if let Some(s) = read_sampled(&mut r, next) {
                    if let Some(b) = stamp_brush(name, &s.vals, s.w, s.h, s.w.max(s.h) as f32, if spacing == 0 { 0.25 } else { spacing as f32 / 100.0 }) {
                        out.brushes.push(b);
                    }
                }
            }
            r.pos = next;
        }
    } else if version >= 6 {
        let _ = r.u16();
        let mut desc: Option<(usize, usize)> = None;
        let mut samples: Vec<Sampled> = Vec::new();
        while r.left() >= 12 {
            if r.take(4) != Some(b"8BIM") {
                // sections are padded: step back and search for the next one
                r.pos = r.pos.saturating_sub(3);
                continue;
            }
            let (Some(key), Some(size)) = (r.take(4), r.u32()) else { break };
            let (start, end) = (r.pos, buf.len().min(r.pos + size as usize));
            if key == b"samp" {
                let mut p = start;
                while p + 4 < end {
                    let len = u32::from_be_bytes([buf[p], buf[p + 1], buf[p + 2], buf[p + 3]]) as usize;
                    let block = p + 4;
                    let mut next = block + len;
                    if len % 4 != 0 {
                        next += 4 - len % 4;
                    }
                    if let Some(s) = find_sampled(buf, block, end.min(block + len)) {
                        samples.push(s);
                    }
                    if len == 0 {
                        break;
                    }
                    p = next;
                }
            } else if key == b"desc" {
                desc = Some((start, end));
            }
            r.pos = end;
        }
        let (names, spacing, diam) = desc.map(|(a, b)| desc_values(&buf[a..b])).unwrap_or_default();
        // the descriptions also list the computed brushes, so names only line up when the counts match
        let named: Vec<String> = if names.len() >= samples.len() { names[names.len() - samples.len()..].to_vec() } else { Vec::new() };
        let n = samples.len();
        for (i, s) in samples.iter().enumerate() {
            let sp = if spacing.len() == n { spacing[i] / 100.0 } else { 0.25 };
            let size = if diam.len() == n { diam[i] } else { s.w.max(s.h) as f32 };
            let name = named.get(i).filter(|n| !n.is_empty()).cloned().unwrap_or_else(|| format!("{} {}", base(file), i + 1));
            if let Some(b) = stamp_brush(name, &s.vals, s.w, s.h, size, sp) {
                out.brushes.push(b);
            }
        }
        if samples.is_empty() {
            out.notes.push(format!("{file}: no brush tip pictures found (it may only contain round brushes or patterns)."));
        }
    } else {
        out.notes.push(format!("{file}: this .abr version ({version}) is not supported."));
    }
    out
}

// ---------- GIMP .gbr and .gih ----------

pub fn parse_gbr(buf: &[u8], file: &str, offset: usize) -> Option<(UserBrush, usize)> {
    let mut r = Reader::new(buf);
    r.pos = offset;
    let (header, version, w, h, bytes) = (r.u32()? as usize, r.u32()?, r.u32()? as usize, r.u32()? as usize, r.u32()?);
    if w == 0 || h == 0 || w > 8000 || h > 8000 || w * h > MAX_PIXELS || (bytes != 1 && bytes != 4) || header < 20 {
        return None;
    }
    let (mut spacing, mut name) = (25.0, base(file));
    if version >= 2 {
        r.pos = offset + 24;
        spacing = r.u32().unwrap_or(25) as f32;
        let nb = buf.get(offset + 28..(offset + header).min(buf.len())).unwrap_or(&[]);
        let n = String::from_utf8_lossy(nb.split(|b| *b == 0).next().unwrap_or(&[])).trim().to_string();
        if !n.is_empty() {
            name = n;
        }
    }
    let n = w * h * bytes as usize;
    let data = buf.get(offset + header..offset + header + n)?;
    let vals: Vec<u8> = (0..w * h).map(|i| if bytes == 1 { data[i] } else { data[i * 4 + 3] }).collect();
    let b = stamp_brush(name, &vals, w, h, w.max(h) as f32, spacing / 100.0)?;
    Some((b, offset + header + n))
}

fn parse_gih(buf: &[u8], file: &str) -> Option<UserBrush> {
    // two text lines (name, then parameters), then GIMP brushes; the first one is used
    let (mut lines, mut i) = (0, 0);
    while i < buf.len() && lines < 2 {
        if buf[i] == 10 {
            lines += 1;
        }
        i += 1;
    }
    parse_gbr(buf, file, i).map(|(b, _)| b)
}

// ---------- Krita .kpp and .bundle ----------

/// A tiny view of an XML text: every element with its attributes and its text (CDATA included).
struct El {
    name: String,
    attrs: HashMap<String, String>,
    text: String,
}

fn xml_elements(xml: &str) -> Vec<El> {
    use quick_xml::events::Event;
    let mut rd = quick_xml::Reader::from_str(xml);
    let mut out: Vec<El> = Vec::new();
    let mut open: Vec<usize> = Vec::new();
    loop {
        match rd.read_event() {
            Ok(Event::Start(e)) | Ok(Event::Empty(e)) => {
                let mut attrs = HashMap::new();
                for a in e.attributes().flatten() {
                    let val = quick_xml::escape::unescape(&a.value).map(|v| v.into_owned()).unwrap_or_else(|_| a.value.to_string());
                    attrs.insert(a.key.as_ref().to_owned(), val);
                }
                out.push(El { name: e.name().as_ref().to_owned(), attrs, text: String::new() });
                open.push(out.len() - 1);
            }
            Ok(Event::End(_)) => {
                open.pop();
            }
            Ok(Event::Text(t)) => {
                if let Some(i) = open.last() {
                    let raw = t.as_ref().to_owned();
                    out[*i].text.push_str(&quick_xml::escape::unescape(&raw).map(|v| v.into_owned()).unwrap_or(raw));
                }
            }
            Ok(Event::CData(c)) => {
                if let Some(i) = open.last() {
                    out[*i].text.push_str(&c.into_inner());
                }
            }
            Ok(Event::Eof) | Err(_) => break,
            _ => {}
        }
    }
    out
}

/// The text fields of a PNG (Krita keeps the preset in one).
fn png_texts(bytes: &[u8]) -> HashMap<String, String> {
    let mut out = HashMap::new();
    let Ok(reader) = png::Decoder::new(std::io::Cursor::new(bytes)).read_info() else { return out };
    let info = reader.info();
    for t in &info.uncompressed_latin1_text {
        out.insert(t.keyword.clone(), t.text.clone());
    }
    for t in &info.compressed_latin1_text {
        if let Ok(s) = t.get_text() {
            out.insert(t.keyword.clone(), s);
        }
    }
    for t in &info.utf8_text {
        if let Ok(s) = t.get_text() {
            out.insert(t.keyword.clone(), s);
        }
    }
    out
}

fn tip_from_bytes(bytes: &[u8], name: &str) -> Option<(Vec<u8>, usize, usize)> {
    let lower = name.to_lowercase();
    if lower.ends_with(".gbr") || lower.ends_with(".gih") {
        let b = if lower.ends_with(".gih") { parse_gih(bytes, name)? } else { parse_gbr(bytes, name, 0)?.0 };
        let t = b.settings.tip_image?;
        return Some((t.a.to_vec(), t.n as usize, t.n as usize));
    }
    let img = crate::files::decode_limited(bytes.to_vec()).ok()?;
    let t = TipImage::from_picture(&img)?;
    Some((t.a.to_vec(), t.n as usize, t.n as usize))
}

fn kpp_to_brush(bytes: &[u8], file: &str, files: &HashMap<String, Vec<u8>>) -> Option<UserBrush> {
    let texts = png_texts(bytes);
    let xml = texts.get("preset")?;
    let els = xml_elements(xml);
    let root = els.iter().find(|e| e.name == "Preset")?;
    let name = root.attrs.get("name").cloned().unwrap_or_else(|| base(file));
    let paintop = root.attrs.get("paintopid").cloned().unwrap_or_else(|| "paintbrush".into());
    let param = |n: &str| els.iter().find(|e| e.name == "param" && e.attrs.get("name").map(|v| v == n).unwrap_or(false)).map(|e| e.text.trim().to_string());
    let mut s = BrushSettings::default();
    if let Some(v) = param("OpacityValue").and_then(|v| v.parse::<f32>().ok()) {
        s.opacity = v;
    }
    if let Some(v) = param("FlowValue").and_then(|v| v.parse::<f32>().ok()) {
        s.flow = v;
    }
    s.size_pressure = param("PressureSize").map(|v| v == "true").unwrap_or(true);
    s.opacity_pressure = param("PressureOpacity").map(|v| v == "true").unwrap_or(false);
    match paintop.as_str() {
        "colorsmudge" => s.mix = 0.5,
        "hairy" => s.tip = Tip::Bristle,
        "spray" => {
            s.tip = Tip::Splatter;
            s.scatter = 0.5;
        }
        _ => {}
    }
    if let Some(def) = param("brush_definition") {
        let bd = xml_elements(&def);
        if let Some(b) = bd.iter().find(|e| e.name == "Brush") {
            let f = |k: &str, d: f32| b.attrs.get(k).and_then(|v| v.parse::<f32>().ok()).unwrap_or(d);
            s.spacing = f("spacing", 0.1);
            let mut angle = f("angle", 0.0);
            if angle.abs() <= 6.3 {
                angle = angle.to_degrees();
            }
            s.angle = angle.round();
            if b.attrs.get("type").map(|t| t == "auto_brush").unwrap_or(false) {
                if let Some(m) = bd.iter().find(|e| e.name == "MaskGenerator") {
                    let g = |k: &str, d: f32| m.attrs.get(k).and_then(|v| v.parse::<f32>().ok()).unwrap_or(d);
                    s.size = g("diameter", 20.0);
                    s.roundness = g("ratio", 1.0);
                    s.hardness = g("hfade", 1.0);
                }
            } else {
                let fname = b.attrs.get("filename").map(|f| f.rsplit(['/', '\\']).next().unwrap_or(f).to_string()).unwrap_or_default();
                let scale = f("scale", 1.0).max(0.01);
                // the tip is in the bundle, or embedded in the preset as base64
                let embedded = els.iter().filter(|e| e.name == "resource" || e.attrs.contains_key("md5sum")).filter(|e| e.name != "Brush").find_map(|e| {
                    let fl = e.attrs.get("filename").or_else(|| e.attrs.get("name")).map(|f| f.rsplit(['/', '\\']).next().unwrap_or(f).to_string()).unwrap_or_default();
                    (e.text.trim().len() > 64 && (fname.is_empty() || fl == fname)).then(|| base64_decode(&e.text)).flatten()
                });
                let data = files.get(&fname).cloned().or(embedded);
                match data.as_deref().and_then(|d| tip_from_bytes(d, &fname)) {
                    Some((vals, w, h)) => {
                        let spacing = s.spacing;
                        let (angle, opacity, flow, sp, op) = (s.angle, s.opacity, s.flow, s.size_pressure, s.opacity_pressure);
                        if let Some(mut b) = stamp_brush(name.clone(), &vals, w, h, w.max(h) as f32 * scale, spacing) {
                            b.settings.angle = angle;
                            b.settings.opacity = opacity;
                            b.settings.flow = flow;
                            b.settings.size_pressure = sp;
                            b.settings.opacity_pressure = op;
                            return Some(b.sanitized());
                        }
                    }
                    None => s.size = 30.0 * scale,
                }
            }
        }
    }
    Some(UserBrush { name, about: "Imported from Krita".into(), settings: s, ..UserBrush::default() }.sanitized())
}

fn parse_krita(bytes: &[u8], file: &str) -> Imported {
    let mut out = Imported::default();
    if file.to_lowercase().ends_with(".kpp") {
        match kpp_to_brush(bytes, file, &HashMap::new()) {
            Some(b) => out.brushes.push(b),
            None => out.notes.push(format!("{file}: no Krita brush settings found inside.")),
        }
        return out;
    }
    // a .bundle is a zip of presets and tips
    let Ok(mut zip) = zip::ZipArchive::new(std::io::Cursor::new(bytes)) else {
        out.notes.push(format!("{file}: not a Krita bundle."));
        return out;
    };
    let mut files: HashMap<String, Vec<u8>> = HashMap::new();
    let mut presets: Vec<(String, Vec<u8>)> = Vec::new();
    for i in 0..zip.len().min(2000) {
        let Ok(f) = zip.by_index(i) else { continue };
        if f.is_dir() || f.size() > 12 << 20 {
            continue;
        }
        let n = f.name().map(|c| c.into_owned()).unwrap_or_default();
        let mut data = Vec::new();
        if f.take(12 << 20).read_to_end(&mut data).is_err() {
            continue;
        }
        let short = n.rsplit('/').next().unwrap_or(&n).to_string();
        if n.to_lowercase().ends_with(".kpp") {
            presets.push((n, data));
        } else if n.to_lowercase().starts_with("brushes/") || [".gbr", ".gih", ".png"].iter().any(|e| n.to_lowercase().ends_with(e)) {
            files.insert(short, data);
        }
    }
    for (n, data) in &presets {
        if let Some(b) = kpp_to_brush(data, n, &files) {
            out.brushes.push(b);
        }
    }
    if out.brushes.is_empty() {
        // a bundle with only tips: each tip becomes a brush
        for (n, data) in &files {
            if let Some((vals, w, h)) = tip_from_bytes(data, n) {
                if let Some(b) = stamp_brush(base(n), &vals, w, h, w.max(h) as f32, 0.15) {
                    out.brushes.push(b);
                }
            }
        }
    }
    if out.brushes.is_empty() {
        out.notes.push(format!("{file}: no brushes found in this bundle."));
    }
    out
}

/// Brushes from a Photoshop, GIMP or Krita file.
pub fn import(bytes: &[u8], file: &str) -> Option<Imported> {
    let ext = std::path::Path::new(file).extension().map(|e| e.to_string_lossy().to_lowercase()).unwrap_or_default();
    match ext.as_str() {
        "abr" => Some(parse_abr(bytes, file)),
        "gbr" => Some(match parse_gbr(bytes, file, 0) {
            Some((b, _)) => Imported { brushes: vec![b], notes: vec![] },
            None => Imported { brushes: vec![], notes: vec![format!("{file}: not a GIMP brush.")] },
        }),
        "gih" => Some(match parse_gih(bytes, file) {
            Some(b) => Imported { brushes: vec![b], notes: vec![] },
            None => Imported { brushes: vec![], notes: vec![format!("{file}: not a GIMP brush pipe.")] },
        }),
        "kpp" | "bundle" => Some(parse_krita(bytes, file)),
        _ => None,
    }
}

// ---------- the tip as pixels, for export ----------

/// The brush tip as paint values (255 = paint) in an `n`×`n` square, turned and squashed like a dab.
pub fn tip_pixels(s: &BrushSettings, n: usize) -> Vec<u8> {
    let n = n.clamp(1, 2500);
    let mask = super::tips::tip_mask(s, 0);
    let m = mask.level(n as f32);
    let ro = s.roundness.clamp(0.05, 1.0);
    let (sn, cs) = s.angle.to_radians().sin_cos();
    let k = m.n as f32 / n as f32;
    let mut out = vec![0u8; n * n];
    for y in 0..n {
        for x in 0..n {
            let (dx, dy) = (x as f32 + 0.5 - n as f32 / 2.0, y as f32 + 0.5 - n as f32 / 2.0);
            let (lx, ly) = (dx * cs + dy * sn, (-dx * sn + dy * cs) / ro);
            let (u, v) = (lx * k + m.n as f32 / 2.0, ly * k + m.n as f32 / 2.0);
            out[y * n + x] = (super::tips::sample(m, u, v).clamp(0.0, 1.0) * 255.0).round() as u8;
        }
    }
    out
}

fn gray_png(vals: &[u8], n: usize, invert: bool) -> Vec<u8> {
    let mut png_bytes = Vec::new();
    {
        let mut enc = png::Encoder::new(&mut png_bytes, n as u32, n as u32);
        enc.set_color(png::ColorType::Grayscale);
        enc.set_depth(png::BitDepth::Eight);
        if let Ok(mut w) = enc.write_header() {
            let data: Vec<u8> = vals.iter().map(|v| if invert { 255 - v } else { *v }).collect();
            let _ = w.write_image_data(&data);
        }
    }
    png_bytes
}

/// The tip as a PNG picture: black paint on white (what most programs expect for a brush tip).
pub fn tip_png(s: &BrushSettings, size: usize) -> Vec<u8> {
    gray_png(&tip_pixels(s, size), size.clamp(1, 2500), true)
}

// ---------- Photoshop .abr (version 2) ----------

pub fn to_abr(list: &[UserBrush]) -> Vec<u8> {
    let mut out = Vec::new();
    out.extend_from_slice(&2u16.to_be_bytes());
    out.extend_from_slice(&(list.len().min(65535) as u16).to_be_bytes());
    for b in list.iter().take(65535) {
        let s = &b.settings;
        let n = (s.size.round() as usize).clamp(1, 2500);
        let vals = tip_pixels(s, n);
        let name: Vec<u16> = b.name.chars().take(60).map(|c| c as u16).collect();
        let body = 4 + 2 + 4 + (name.len() + 1) * 2 + 1 + 8 + 16 + 2 + 1 + vals.len();
        out.extend_from_slice(&2u16.to_be_bytes()); // sampled brush
        out.extend_from_slice(&(body as u32).to_be_bytes());
        out.extend_from_slice(&0u32.to_be_bytes()); // misc
        out.extend_from_slice(&((s.spacing * 100.0).round().clamp(1.0, 999.0) as u16).to_be_bytes());
        out.extend_from_slice(&((name.len() + 1) as u32).to_be_bytes());
        for c in &name {
            out.extend_from_slice(&c.to_be_bytes());
        }
        out.extend_from_slice(&0u16.to_be_bytes());
        out.push(if s.antialias { 1 } else { 0 });
        for v in [0i16, 0, n.min(32767) as i16, n.min(32767) as i16] {
            out.extend_from_slice(&v.to_be_bytes());
        }
        for v in [0i32, 0, n as i32, n as i32] {
            out.extend_from_slice(&v.to_be_bytes());
        }
        out.extend_from_slice(&8u16.to_be_bytes()); // depth
        out.push(0); // no compression
        out.extend_from_slice(&vals);
    }
    out
}

// ---------- MD5 (Krita lists embedded tips by it) ----------

fn md5(data: &[u8]) -> String {
    let k: Vec<u32> = (0..64).map(|i| ((i as f64 + 1.0).sin().abs() * 4_294_967_296.0) as u32).collect();
    let sh = [7u32, 12, 17, 22, 5, 9, 14, 20, 4, 11, 16, 23, 6, 10, 15, 21];
    let mut msg = data.to_vec();
    msg.push(0x80);
    while msg.len() % 64 != 56 {
        msg.push(0);
    }
    msg.extend_from_slice(&((data.len() as u64) * 8).to_le_bytes());
    let (mut a0, mut b0, mut c0, mut d0) = (0x6745_2301u32, 0xefcd_ab89u32, 0x98ba_dcfeu32, 0x1032_5476u32);
    for chunk in msg.chunks(64) {
        let m: Vec<u32> = chunk.chunks(4).map(|c| u32::from_le_bytes([c[0], c[1], c[2], c[3]])).collect();
        let (mut a, mut b, mut c, mut d) = (a0, b0, c0, d0);
        for i in 0..64 {
            let (f, g) = match i / 16 {
                0 => ((b & c) | (!b & d), i),
                1 => ((d & b) | (!d & c), (5 * i + 1) % 16),
                2 => (b ^ c ^ d, (3 * i + 5) % 16),
                _ => (c ^ (b | !d), (7 * i) % 16),
            };
            let tmp = d;
            d = c;
            c = b;
            let x = a.wrapping_add(f).wrapping_add(k[i]).wrapping_add(m[g]);
            b = b.wrapping_add(x.rotate_left(sh[(i / 16) * 4 + i % 4]));
            a = tmp;
        }
        a0 = a0.wrapping_add(a);
        b0 = b0.wrapping_add(b);
        c0 = c0.wrapping_add(c);
        d0 = d0.wrapping_add(d);
    }
    [a0, b0, c0, d0].iter().flat_map(|v| v.to_le_bytes()).map(|b| format!("{b:02x}")).collect()
}

fn xml_esc(t: &str) -> String {
    t.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;")
}

// ---------- Krita .kpp ----------

/// A Krita preset: a PNG picture with the settings inside, the tip embedded. `preview`: the picture shown in Krita.
pub fn to_kpp(b: &UserBrush, preview: &image::RgbaImage) -> Result<Vec<u8>, String> {
    let s = &b.settings;
    let simple_round = s.tip == Tip::Round && s.edge == super::Edge::Smooth && s.grain == 0.0 && s.deform == 0.0 && s.streaks == 0.0 && s.wet_rim == 0.0;
    let angle = s.angle.to_radians();
    let (brush_def, resources) = if simple_round {
        (format!("<Brush useAutoSpacing=\"0\" angle=\"{angle}\" spacing=\"{}\" randomness=\"{}\" BrushVersion=\"2\" density=\"1\" type=\"auto_brush\" autoSpacingCoeff=\"1\"><MaskGenerator id=\"default\" ratio=\"{}\" diameter=\"{}\" type=\"circle\" spikes=\"2\" hfade=\"{}\" vfade=\"{}\" antialiasEdges=\"{}\"/></Brush>", s.spacing, s.angle_jitter, s.roundness, s.size, s.hardness, s.hardness, if s.antialias { 1 } else { 0 }), String::new())
    } else {
        let png = tip_png(&BrushSettings { angle: 0.0, roundness: 1.0, ..s.clone() }, 256);
        let file = format!("{}_tip.png", super::userlib::slug(&b.name).replace('-', "_"));
        let sum = md5(&png);
        (
            format!("<Brush useAutoSpacing=\"0\" angle=\"{angle}\" spacing=\"{}\" BrushVersion=\"2\" scale=\"{}\" type=\"png_brush\" filename=\"{}\" md5sum=\"{sum}\" ColorAsMask=\"1\" preserveLightness=\"0\" brushApplication=\"0\" AdjustmentMidPoint=\"127\" BrightnessAdjustment=\"0\" ContrastAdjustment=\"0\" AutoAdjustMidPoint=\"0\"/>", s.spacing, s.size / 256.0, xml_esc(&file)),
            format!("<resources><resource type=\"brushes\" md5sum=\"{sum}\" name=\"{}\" filename=\"{}\">{}</resource></resources>", xml_esc(&file), xml_esc(&file), base64_encode(&png)),
        )
    };
    let op = if s.mix > 0.05 { "colorsmudge" } else { "paintbrush" };
    let p = |k: &str, v: &str| format!("<param type=\"string\" name=\"{k}\"><![CDATA[{v}]]></param>");
    let curve = "<!DOCTYPE params><params id=\"pressure\" curve=\"0,0;1,1;\"/>";
    let xml = format!(
        "<Preset paintopid=\"{op}\" name=\"{}\">{}{}{}{}{}{}{}{}{}{}{}{}{resources}</Preset>",
        xml_esc(&b.name),
        p("brush_definition", &brush_def),
        p("OpacityValue", &s.opacity.to_string()),
        p("OpacityUseCurve", "false"),
        p("FlowValue", &s.flow.to_string()),
        p("PressureSize", &s.size_pressure.to_string()),
        p("SizeUseCurve", "true"),
        p("SizeSensor", curve),
        p("PressureOpacity", &s.opacity_pressure.to_string()),
        p("OpacitySensor", curve),
        p("Texture/Pattern/Enabled", "false"),
        p("paintop", op),
        p("requiredBrushFile", ""),
    );
    let mut out = Vec::new();
    {
        let mut enc = png::Encoder::new(&mut out, preview.width(), preview.height());
        enc.set_color(png::ColorType::Rgba);
        enc.set_depth(png::BitDepth::Eight);
        enc.add_itxt_chunk("version".into(), "2.2".into()).map_err(|e| e.to_string())?;
        enc.add_itxt_chunk("preset".into(), xml).map_err(|e| e.to_string())?;
        let mut w = enc.write_header().map_err(|e| e.to_string())?;
        w.write_image_data(preview.as_raw()).map_err(|e| e.to_string())?;
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn leaf() -> UserBrush {
        let mut img = image::RgbaImage::from_pixel(80, 80, image::Rgba([255, 255, 255, 255]));
        for y in 10..70 {
            for x in 30..50 {
                img.put_pixel(x, y, image::Rgba([0, 0, 0, 255]));
            }
        }
        let tip = TipImage::from_picture(&img).unwrap();
        UserBrush { name: "Test leaf".into(), settings: BrushSettings { tip: Tip::Image, tip_image: Some(tip), size: 64.0, spacing: 0.3, opacity: 0.8, flow: 0.6, ..BrushSettings::default() }, ..UserBrush::default() }
    }

    fn preview() -> image::RgbaImage {
        image::RgbaImage::from_pixel(60, 30, image::Rgba([240, 240, 240, 255]))
    }

    #[test]
    fn md5_is_the_real_md5() {
        assert_eq!(md5(b""), "d41d8cd98f00b204e9800998ecf8427e");
        assert_eq!(md5(b"abc"), "900150983cd24fb0d6963f7d28e17f72");
        assert_eq!(md5(b"The quick brown fox jumps over the lazy dog"), "9e107d9d372bb6826bd81d3542a419d6");
    }

    #[test]
    fn abr_export_comes_back_through_the_import() {
        let b = leaf();
        let bytes = to_abr(std::slice::from_ref(&b));
        let back = import(&bytes, "x.abr").unwrap();
        assert_eq!(back.brushes.len(), 1, "{:?}", back.notes);
        let got = &back.brushes[0];
        assert_eq!(got.name, "Test leaf");
        assert!((got.settings.spacing - 0.3).abs() < 0.02 && got.settings.size >= 60.0 && got.settings.size <= 68.0, "{:?}", got.settings.size);
        let tip = got.settings.tip_image.as_ref().unwrap();
        // the bar stands up in the middle, as in the original
        assert!(tip.at(tip.n as f32 / 2.0, tip.n as f32 / 2.0) > 0.9 && tip.at(2.0, tip.n as f32 / 2.0) < 0.1);
    }

    #[test]
    fn abr_reads_packed_rows_computed_round_brushes_and_the_new_layout() {
        // version 1 with one computed brush and one sampled brush with run-length rows
        let mut v1 = Vec::new();
        v1.extend_from_slice(&1u16.to_be_bytes());
        v1.extend_from_slice(&2u16.to_be_bytes());
        let mut computed = Vec::new();
        computed.extend_from_slice(&0u32.to_be_bytes());
        for v in [50u16, 30, 80, (-20i16) as u16, 60] {
            computed.extend_from_slice(&v.to_be_bytes());
        }
        v1.extend_from_slice(&1u16.to_be_bytes());
        v1.extend_from_slice(&(computed.len() as u32).to_be_bytes());
        v1.extend_from_slice(&computed);
        // sampled: 4x2, rows packed as "repeat 0 x4" and "literal 4 bytes"
        let mut samp = Vec::new();
        samp.extend_from_slice(&0u32.to_be_bytes());
        samp.extend_from_slice(&25u16.to_be_bytes());
        samp.push(1);
        samp.extend_from_slice(&[0u8; 8]);
        for v in [0i32, 0, 2, 4] {
            samp.extend_from_slice(&v.to_be_bytes());
        }
        samp.extend_from_slice(&8u16.to_be_bytes());
        samp.push(1);
        samp.extend_from_slice(&2u16.to_be_bytes()); // row 1: 2 bytes
        samp.extend_from_slice(&5u16.to_be_bytes()); // row 2: 5 bytes
        samp.extend_from_slice(&[(1 - 4i32) as u8, 0]); // -3: repeat the next byte 4 times
        samp.extend_from_slice(&[3, 255, 128, 64, 255]); // literal: 4 bytes
        v1.extend_from_slice(&2u16.to_be_bytes());
        v1.extend_from_slice(&(samp.len() as u32).to_be_bytes());
        v1.extend_from_slice(&samp);
        let r = parse_abr(&v1, "old.abr");
        assert_eq!(r.brushes.len(), 2, "{:?}", r.notes);
        let c = &r.brushes[0].settings;
        assert_eq!((c.size, c.angle, c.hardness), (30.0, -20.0, 0.6));
        assert!((c.roundness - 0.8).abs() < 1e-5 && (c.spacing - 0.5).abs() < 1e-5);
        assert_eq!(r.brushes[1].settings.tip, Tip::Image);
        // the new layout: sections of "8BIM", with the sampled picture somewhere inside a block
        let mut v6 = Vec::new();
        v6.extend_from_slice(&6u16.to_be_bytes());
        v6.extend_from_slice(&1u16.to_be_bytes());
        let mut block = vec![0u8; 47];
        for v in [0i32, 0, 3, 3] {
            block.extend_from_slice(&v.to_be_bytes());
        }
        block.extend_from_slice(&8u16.to_be_bytes());
        block.push(0);
        block.extend_from_slice(&[0, 255, 0, 255, 255, 255, 0, 255, 0]);
        let mut section = Vec::new();
        section.extend_from_slice(&(block.len() as u32).to_be_bytes());
        section.extend_from_slice(&block);
        v6.extend_from_slice(b"8BIM");
        v6.extend_from_slice(b"samp");
        v6.extend_from_slice(&(section.len() as u32).to_be_bytes());
        v6.extend_from_slice(&section);
        let r6 = parse_abr(&v6, "new.abr");
        assert_eq!(r6.brushes.len(), 1, "{:?}", r6.notes);
        // bad input never panics
        assert!(parse_abr(&[0, 6], "x.abr").brushes.is_empty());
        assert!(parse_abr(&v6[..v6.len() - 7], "cut.abr").brushes.len() <= 1);
        assert!(parse_abr(b"", "empty.abr").brushes.is_empty());
        assert!(!parse_abr(&[0, 9, 0, 0], "v9.abr").notes.is_empty());
    }

    #[test]
    fn gimp_brushes_both_kinds() {
        let (w, h) = (6u32, 4u32);
        let make = |bytes: u32| {
            let name = b"Gimp dab\0";
            let header = 28 + name.len() as u32;
            let mut v = Vec::new();
            for x in [header, 2, w, h, bytes, 0x4749_4d50, 40] {
                v.extend_from_slice(&x.to_be_bytes());
            }
            v.extend_from_slice(name);
            let n = (w * h * bytes) as usize;
            v.extend((0..n).map(|i| if bytes == 1 { (i * 10) as u8 } else if i % 4 == 3 { 200 } else { 0 }));
            v
        };
        let g1 = import(&make(1), "dab.gbr").unwrap();
        assert_eq!(g1.brushes.len(), 1);
        assert_eq!(g1.brushes[0].name, "Gimp dab");
        assert!((g1.brushes[0].settings.spacing - 0.4).abs() < 1e-5);
        assert_eq!(import(&make(4), "rgba.gbr").unwrap().brushes.len(), 1);
        // a pipe: two text lines, then a brush
        let mut gih = b"Pipe\n1 ncells:1\n".to_vec();
        gih.extend(make(1));
        assert_eq!(import(&gih, "p.gih").unwrap().brushes.len(), 1);
        assert!(import(&make(1)[..30], "cut.gbr").unwrap().brushes.is_empty());
        assert!(import(b"junk", "j.txt").is_none(), "other files are not ours to read");
    }

    #[test]
    fn krita_presets_round_trip_with_the_tip_inside_and_as_a_bundle() {
        let b = leaf();
        let kpp = to_kpp(&b, &preview()).unwrap();
        assert!(image::load_from_memory(&kpp).is_ok(), "a normal PNG picture");
        let back = import(&kpp, "leaf.kpp").unwrap();
        assert_eq!(back.brushes.len(), 1, "{:?}", back.notes);
        let g = &back.brushes[0];
        assert_eq!(g.name, "Test leaf");
        assert!((g.settings.opacity - 0.8).abs() < 1e-4 && (g.settings.flow - 0.6).abs() < 1e-4 && (g.settings.spacing - 0.3).abs() < 1e-4);
        assert_eq!(g.settings.tip, Tip::Image, "the tip came along");
        // a simple round brush has no picture, only the numbers
        let round = UserBrush { name: "Round one".into(), settings: BrushSettings { size: 31.0, hardness: 0.7, roundness: 0.8, ..BrushSettings::default() }, ..UserBrush::default() };
        let r = import(&to_kpp(&round, &preview()).unwrap(), "r.kpp").unwrap().brushes.remove(0);
        assert_eq!((r.settings.size, r.settings.tip), (31.0, Tip::Round));
        assert!((r.settings.hardness - 0.7).abs() < 1e-4 && (r.settings.roundness - 0.8).abs() < 1e-4);
        // a bundle (zip) holding that preset
        let mut zip_bytes = Vec::new();
        {
            let mut z = zip::ZipWriter::new(std::io::Cursor::new(&mut zip_bytes));
            let o = zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored);
            z.start_file("paintoppresets/leaf.kpp", o).unwrap();
            std::io::Write::write_all(&mut z, &kpp).unwrap();
            z.finish().unwrap();
        }
        assert_eq!(import(&zip_bytes, "pack.bundle").unwrap().brushes.len(), 1);
        assert!(import(b"nope", "x.bundle").unwrap().brushes.is_empty());
        assert!(!import(&preview_png(), "plain.kpp").unwrap().notes.is_empty(), "a plain PNG has no settings inside");
    }

    fn preview_png() -> Vec<u8> {
        let mut v = Vec::new();
        preview().write_to(&mut std::io::Cursor::new(&mut v), image::ImageFormat::Png).unwrap();
        v
    }

    #[test]
    fn the_tip_png_is_black_paint_on_white() {
        let png = tip_png(&leaf().settings, 64);
        let img = image::load_from_memory(&png).unwrap().to_luma8();
        assert_eq!(img.dimensions(), (64, 64));
        assert!(img.get_pixel(32, 32).0[0] < 40 && img.get_pixel(3, 32).0[0] > 215, "dark where the bar is, white around");
    }
}
