// SPDX-License-Identifier: GPL-3.0-or-later
// Photoshop .psd reading and writing (8-bit RGB): layers with names (Unicode), opacity,
// visibility, blend modes, clipping, lock and lock-alpha, folders (open/closed, pass-through),
// and the paper as its own layer. Written from the public PSD format description.

use std::collections::HashMap;

use eframe::egui::Color32;

use super::{MAX_SIDE, layer_bounds, layer_rgba, tiles_from_rgba};
use crate::doc::{Blend, Document, Layer, LayerId, LayerKind};

const PAPER_NAME: &str = "Paper";

fn blend_key(b: Blend) -> &'static [u8; 4] {
    match b {
        Blend::PassThrough => b"pass",
        Blend::Normal => b"norm",
        Blend::Multiply => b"mul ",
        Blend::Screen => b"scrn",
        Blend::Overlay => b"over",
        Blend::Darken => b"dark",
        Blend::Lighten => b"lite",
        Blend::ColorDodge => b"div ",
        Blend::ColorBurn => b"idiv",
        Blend::HardLight => b"hLit",
        Blend::SoftLight => b"sLit",
        Blend::Difference => b"diff",
        Blend::Exclusion => b"smud",
        Blend::Hue => b"hue ",
        Blend::Saturation => b"sat ",
        Blend::Color => b"colr",
        Blend::Luminosity => b"lum ",
        Blend::Add => b"lddg",
    }
}

fn blend_from_key(k: &[u8]) -> Blend {
    std::iter::once(Blend::PassThrough).chain(Blend::ALL).find(|b| blend_key(*b) == k).unwrap_or(Blend::Normal)
}

// ---------- writing ----------

/// PackBits (RLE) one row.
fn packbits(row: &[u8], out: &mut Vec<u8>) {
    let n = row.len();
    let mut i = 0;
    while i < n {
        let mut run = 1;
        while i + run < n && run < 128 && row[i + run] == row[i] {
            run += 1;
        }
        if run >= 2 {
            out.push((1i32 - run as i32) as i8 as u8);
            out.push(row[i]);
            i += run;
        } else {
            let start = i;
            let mut len = 0;
            while i < n && len < 128 {
                if i + 2 < n && row[i] == row[i + 1] && row[i] == row[i + 2] {
                    break;
                }
                i += 1;
                len += 1;
            }
            out.push((len - 1) as u8);
            out.extend_from_slice(&row[start..start + len]);
        }
    }
}

/// One channel plane (w×h) as PSD channel data: compression 1 (RLE) + row counts + rows.
fn rle_channel(plane: &[u8], w: usize, h: usize) -> Vec<u8> {
    if w == 0 || h == 0 {
        return vec![0, 0]; // raw, no data
    }
    let mut counts = Vec::with_capacity(h * 2);
    let mut data = Vec::new();
    for y in 0..h {
        let before = data.len();
        packbits(&plane[y * w..(y + 1) * w], &mut data);
        counts.extend_from_slice(&((data.len() - before) as u16).to_be_bytes());
    }
    let mut out = vec![0, 1];
    out.extend(counts);
    out.extend(data);
    out
}

fn pascal_name(name: &str) -> Vec<u8> {
    let ascii: Vec<u8> = name.chars().take(255).map(|c| if c.is_ascii() && !c.is_ascii_control() { c as u8 } else { b'?' }).collect();
    let mut out = vec![ascii.len() as u8];
    out.extend(ascii);
    while out.len() % 4 != 0 {
        out.push(0);
    }
    out
}

fn block(key: &[u8; 4], data: &[u8]) -> Vec<u8> {
    let mut out = b"8BIM".to_vec();
    out.extend_from_slice(key);
    let mut d = data.to_vec();
    if d.len() % 2 == 1 {
        d.push(0);
    }
    out.extend_from_slice(&(d.len() as u32).to_be_bytes());
    out.extend(d);
    out
}

fn unicode_name(name: &str) -> Vec<u8> {
    let units: Vec<u16> = name.encode_utf16().collect();
    let mut d = (units.len() as u32).to_be_bytes().to_vec();
    for u in units {
        d.extend_from_slice(&u.to_be_bytes());
    }
    block(b"luni", &d)
}

struct Record {
    name: String,
    rect: (i32, i32, i32, i32), // top, left, bottom, right
    channels: Vec<(i16, Vec<u8>)>,
    blend: Blend,
    opacity: u8,
    clip: bool,
    flags: u8,
    extra_blocks: Vec<u8>,
}

impl Record {
    fn bytes(&self) -> Vec<u8> {
        let mut out = Vec::new();
        for v in [self.rect.0, self.rect.1, self.rect.2, self.rect.3] {
            out.extend_from_slice(&v.to_be_bytes());
        }
        out.extend_from_slice(&(self.channels.len() as u16).to_be_bytes());
        for (id, data) in &self.channels {
            out.extend_from_slice(&id.to_be_bytes());
            out.extend_from_slice(&(data.len() as u32).to_be_bytes());
        }
        out.extend_from_slice(b"8BIM");
        out.extend_from_slice(blend_key(self.blend));
        out.push(self.opacity);
        out.push(self.clip as u8);
        out.push(self.flags);
        out.push(0);
        let mut extra = Vec::new();
        extra.extend_from_slice(&0u32.to_be_bytes()); // no layer mask
        extra.extend_from_slice(&0u32.to_be_bytes()); // no blending ranges
        extra.extend(pascal_name(&self.name));
        extra.extend(unicode_name(&self.name));
        extra.extend_from_slice(&self.extra_blocks);
        out.extend_from_slice(&(extra.len() as u32).to_be_bytes());
        out.extend(extra);
        out
    }
}

fn pixel_record(l: &Layer, doc: &Document) -> Record {
    let (rect, channels) = match layer_bounds(l, doc.w, doc.h) {
        Some((x0, y0, x1, y1)) => {
            let (w, h) = ((x1 - x0) as u32, (y1 - y0) as u32);
            let img = layer_rgba(l, x0, y0, w, h);
            let plane = |c: usize| img.pixels().map(|p| p.0[c]).collect::<Vec<u8>>();
            let ch = |c| rle_channel(&plane(c), w as usize, h as usize);
            ((y0, x0, y1, x1), vec![(-1, ch(3)), (0, ch(0)), (1, ch(1)), (2, ch(2))])
        }
        None => ((0, 0, 0, 0), vec![(-1, vec![0, 0]), (0, vec![0, 0]), (1, vec![0, 0]), (2, vec![0, 0])]),
    };
    record_for(l, rect, channels)
}

fn record_for(l: &Layer, rect: (i32, i32, i32, i32), channels: Vec<(i16, Vec<u8>)>) -> Record {
    let mut flags = 0u8;
    if l.lock_alpha {
        flags |= 1;
    }
    if !l.visible {
        flags |= 2;
    }
    let mut extra_blocks = Vec::new();
    if l.locked {
        extra_blocks.extend(block(b"lspf", &0x8000_0000u32.to_be_bytes()));
    }
    Record {
        name: l.name.clone(),
        rect,
        channels,
        blend: if l.kind == LayerKind::Group { l.blend } else if l.blend == Blend::PassThrough { Blend::Normal } else { l.blend },
        opacity: (l.opacity / 100.0 * 255.0).round().clamp(0.0, 255.0) as u8,
        clip: l.clip,
        flags,
        extra_blocks,
    }
}

/// PSD records bottom-first: for a folder, its end marker, then its contents, then the folder.
fn records(doc: &Document, parent: Option<LayerId>, out: &mut Vec<Record>) {
    let kids: Vec<&Layer> = doc.layers.iter().filter(|l| l.parent == parent).collect();
    for l in kids.into_iter().rev() {
        match l.kind {
            LayerKind::Group => {
                let empty = vec![(-1, vec![0, 0]), (0, vec![0, 0]), (1, vec![0, 0]), (2, vec![0, 0])];
                let mut end = Record {
                    name: "</Layer group>".into(),
                    rect: (0, 0, 0, 0),
                    channels: empty.clone(),
                    blend: Blend::Normal,
                    opacity: 255,
                    clip: false,
                    flags: 0x18,
                    extra_blocks: Vec::new(),
                };
                end.extra_blocks.extend(block(b"lsct", &3u32.to_be_bytes()));
                out.push(end);
                records(doc, Some(l.id), out);
                let mut folder = record_for(l, (0, 0, 0, 0), empty);
                folder.flags |= 0x18;
                let mut lsct = (if l.expanded { 1u32 } else { 2u32 }).to_be_bytes().to_vec();
                lsct.extend_from_slice(b"8BIM");
                lsct.extend_from_slice(blend_key(l.blend));
                folder.extra_blocks.extend(block(b"lsct", &lsct));
                out.push(folder);
            }
            LayerKind::Paper => {
                // the paper becomes a full-canvas layer filled with the paper color
                let (w, h) = (doc.w as usize, doc.h as usize);
                let [r, g, b, a] = doc.paper.to_srgba_unmultiplied();
                let ch = |v: u8| rle_channel(&vec![v; w * h], w, h);
                let mut rec = record_for(l, (0, 0, doc.h as i32, doc.w as i32), vec![(-1, ch(a)), (0, ch(r)), (1, ch(g)), (2, ch(b))]);
                rec.name = PAPER_NAME.into();
                out.push(rec);
            }
            _ => out.push(pixel_record(l, doc)),
        }
    }
}

pub fn write(doc: &Document) -> Vec<u8> {
    let (w, h) = (doc.w as usize, doc.h as usize);
    let mut out = Vec::new();
    out.extend_from_slice(b"8BPS");
    out.extend_from_slice(&1u16.to_be_bytes());
    out.extend_from_slice(&[0u8; 6]);
    out.extend_from_slice(&3u16.to_be_bytes()); // RGB composite
    out.extend_from_slice(&(doc.h).to_be_bytes());
    out.extend_from_slice(&(doc.w).to_be_bytes());
    out.extend_from_slice(&8u16.to_be_bytes());
    out.extend_from_slice(&3u16.to_be_bytes()); // RGB mode
    out.extend_from_slice(&0u32.to_be_bytes()); // color mode data
    // image resources: only the resolution (0x03ED), so the print size survives
    let dpi = (doc.meta.dpi.clamp(1, 65535) as u32) << 16; // 16.16 fixed point
    let mut res = Vec::new();
    res.extend_from_slice(b"8BIM");
    res.extend_from_slice(&0x03EDu16.to_be_bytes());
    res.extend_from_slice(&[0, 0]); // empty pascal name, padded
    res.extend_from_slice(&16u32.to_be_bytes());
    for _ in 0..2 {
        res.extend_from_slice(&dpi.to_be_bytes());
        res.extend_from_slice(&1u16.to_be_bytes()); // pixels per inch
        res.extend_from_slice(&1u16.to_be_bytes()); // shown in inches
    }
    out.extend_from_slice(&(res.len() as u32).to_be_bytes());
    out.extend_from_slice(&res);

    // layer and mask information
    let mut recs = Vec::new();
    records(doc, None, &mut recs);
    let mut info = Vec::new();
    info.extend_from_slice(&(recs.len() as i16).to_be_bytes());
    for r in &recs {
        info.extend(r.bytes());
    }
    for r in &recs {
        for (_, data) in &r.channels {
            info.extend_from_slice(data);
        }
    }
    if info.len() % 2 == 1 {
        info.push(0);
    }
    let mut lm = Vec::new();
    lm.extend_from_slice(&(info.len() as u32).to_be_bytes());
    lm.extend(info);
    lm.extend_from_slice(&0u32.to_be_bytes()); // global layer mask
    out.extend_from_slice(&(lm.len() as u32).to_be_bytes());
    out.extend(lm);

    // merged picture (what programs show as the preview / flattened image), on the paper or white
    let flat = super::flatten(doc);
    let bg = if doc.paper_visible() { doc.paper } else { Color32::WHITE };
    let mut counts = Vec::new();
    let mut data = Vec::new();
    let mut row = vec![0u8; w];
    for c in 0..3 {
        let bgc = [bg.r(), bg.g(), bg.b()][c];
        for y in 0..h {
            for (x, v) in row.iter_mut().enumerate() {
                let p = flat.get_pixel(x as u32, y as u32).0;
                let a = p[3] as f32 / 255.0;
                *v = (p[c] as f32 * a + bgc as f32 * (1.0 - a)).round() as u8;
            }
            let before = data.len();
            packbits(&row, &mut data);
            counts.extend_from_slice(&((data.len() - before) as u16).to_be_bytes());
        }
    }
    out.extend_from_slice(&1u16.to_be_bytes());
    out.extend(counts);
    out.extend(data);
    out
}

// ---------- reading ----------

struct Cur<'a> {
    b: &'a [u8],
    p: usize,
}

impl<'a> Cur<'a> {
    fn take(&mut self, n: usize) -> Result<&'a [u8], String> {
        if self.p + n > self.b.len() {
            return Err("The PSD file ends too early (damaged?).".into());
        }
        let s = &self.b[self.p..self.p + n];
        self.p += n;
        Ok(s)
    }
    fn u8(&mut self) -> Result<u8, String> {
        Ok(self.take(1)?[0])
    }
    fn u16(&mut self) -> Result<u16, String> {
        let s = self.take(2)?;
        Ok(u16::from_be_bytes([s[0], s[1]]))
    }
    fn i16(&mut self) -> Result<i16, String> {
        Ok(self.u16()? as i16)
    }
    fn u32(&mut self) -> Result<u32, String> {
        let s = self.take(4)?;
        Ok(u32::from_be_bytes([s[0], s[1], s[2], s[3]]))
    }
    fn i32(&mut self) -> Result<i32, String> {
        Ok(self.u32()? as i32)
    }
    fn skip(&mut self, n: usize) -> Result<(), String> {
        self.take(n).map(|_| ())
    }
}

/// The DPI from a PSD's image-resource block (resource 0x03ED), if there is one.
fn read_dpi(res: &[u8]) -> Option<u32> {
    let mut i = 0;
    while i + 12 <= res.len() {
        if &res[i..i + 4] != b"8BIM" {
            return None;
        }
        let id = u16::from_be_bytes([res[i + 4], res[i + 5]]);
        let name_len = res[i + 6] as usize;
        let name_total = (1 + name_len + 1) & !1; // pascal string padded to an even size
        let len_at = i + 6 + name_total;
        if len_at + 4 > res.len() {
            return None;
        }
        let len = u32::from_be_bytes([res[len_at], res[len_at + 1], res[len_at + 2], res[len_at + 3]]) as usize;
        let data = len_at + 4;
        if id == 0x03ED && len >= 4 && data + 4 <= res.len() {
            let fixed = u32::from_be_bytes([res[data], res[data + 1], res[data + 2], res[data + 3]]);
            let dpi = (fixed as f64 / 65536.0).round() as u32;
            return (1..=2400).contains(&dpi).then_some(dpi);
        }
        i = data + len + (len & 1);
    }
    None
}

fn unpackbits(src: &[u8], want: usize) -> Result<Vec<u8>, String> {
    let mut out = Vec::with_capacity(want);
    let mut i = 0;
    while i < src.len() && out.len() < want {
        let n = src[i] as i8;
        i += 1;
        if n >= 0 {
            let len = n as usize + 1;
            let end = (i + len).min(src.len());
            out.extend_from_slice(&src[i..end]);
            i = end;
        } else if n != -128 {
            let len = (1 - n as i32) as usize;
            if i < src.len() {
                out.extend(std::iter::repeat_n(src[i], len));
                i += 1;
            }
        }
    }
    out.resize(want, 0);
    Ok(out)
}

/// One channel's pixels from its data block.
fn read_channel(data: &[u8], w: usize, h: usize) -> Result<Option<Vec<u8>>, String> {
    if w == 0 || h == 0 {
        return Ok(Some(Vec::new()));
    }
    if data.len() < 2 {
        return Ok(None);
    }
    let comp = u16::from_be_bytes([data[0], data[1]]);
    let body = &data[2..];
    match comp {
        0 => Ok(Some(body.get(..w * h).map(|s| s.to_vec()).unwrap_or_else(|| {
            let mut v = body.to_vec();
            v.resize(w * h, 0);
            v
        }))),
        1 => {
            if body.len() < h * 2 {
                return Err("A layer's pixel data is damaged.".into());
            }
            let mut out = Vec::with_capacity(w * h);
            let mut at = h * 2;
            for y in 0..h {
                let len = u16::from_be_bytes([body[y * 2], body[y * 2 + 1]]) as usize;
                let end = (at + len).min(body.len());
                out.extend(unpackbits(&body[at.min(end)..end], w)?);
                at = end;
            }
            Ok(Some(out))
        }
        _ => Ok(None), // ZIP-compressed channels (rare in 8-bit files) are not read yet
    }
}

struct InRecord {
    name: String,
    rect: (i32, i32, i32, i32),
    channels: Vec<(i16, usize)>,
    blend: Blend,
    opacity: u8,
    clip: bool,
    flags: u8,
    section: Option<(u32, Option<Blend>)>,
    locked: bool,
}

pub fn read(bytes: &[u8]) -> Result<Document, String> {
    let mut c = Cur { b: bytes, p: 0 };
    if c.take(4)? != b"8BPS" {
        return Err("This isn't a Photoshop file.".into());
    }
    let version = c.u16()?;
    if version != 1 {
        return Err("Large-document PSB files aren't supported yet — save as PSD.".into());
    }
    c.skip(6)?;
    let nchan = c.u16()? as usize;
    let h = c.u32()?;
    let w = c.u32()?;
    let depth = c.u16()?;
    let mode = c.u16()?;
    if w == 0 || h == 0 || w > MAX_SIDE || h > MAX_SIDE {
        return Err("The canvas size in this PSD is not valid or too big.".into());
    }
    if depth != 8 {
        return Err(format!("{depth}-bit PSD files aren't supported yet — save as 8 bits per channel."));
    }
    if mode != 3 && mode != 1 {
        return Err("Only RGB and grayscale PSD files can be opened for now.".into());
    }
    let gray = mode == 1;
    let n = c.u32()? as usize;
    c.skip(n)?; // color mode data
    let n = c.u32()? as usize;
    let dpi = read_dpi(c.take(n)?); // image resources

    let lm_len = c.u32()? as usize;
    let lm_end = c.p + lm_len;
    let mut records: Vec<InRecord> = Vec::new();
    let mut pixels: Vec<HashMap<i16, Vec<u8>>> = Vec::new();
    if lm_len > 0 {
        let info_len = c.u32()? as usize;
        if info_len > 0 {
            let count = c.i16()?.unsigned_abs() as usize;
            if count > 20_000 {
                return Err("This PSD has too many layers.".into());
            }
            for _ in 0..count {
                let rect = (c.i32()?, c.i32()?, c.i32()?, c.i32()?);
                let nch = c.u16()? as usize;
                if nch > 64 {
                    return Err("A layer in this PSD is damaged.".into());
                }
                let mut channels = Vec::new();
                for _ in 0..nch {
                    channels.push((c.i16()?, c.u32()? as usize));
                }
                c.skip(4)?; // 8BIM
                let blend = blend_from_key(c.take(4)?);
                let opacity = c.u8()?;
                let clip = c.u8()? != 0;
                let flags = c.u8()?;
                c.skip(1)?;
                let extra_len = c.u32()? as usize;
                let extra_end = c.p + extra_len;
                let mask_len = c.u32()? as usize;
                c.skip(mask_len)?;
                let ranges_len = c.u32()? as usize;
                c.skip(ranges_len)?;
                let nlen = c.u8()? as usize;
                let mut name: String = c.take(nlen)?.iter().map(|&b| b as char).collect();
                let padded = (nlen + 1).div_ceil(4) * 4;
                c.skip(padded - (nlen + 1))?;
                let mut section = None;
                let mut locked = false;
                while c.p + 12 <= extra_end {
                    let sig = c.take(4)?;
                    if sig != b"8BIM" && sig != b"8B64" {
                        break;
                    }
                    let key: [u8; 4] = c.take(4)?.try_into().unwrap();
                    let len = c.u32()? as usize;
                    let data = c.take(len.min(extra_end.saturating_sub(c.p)))?;
                    match &key {
                        b"luni" if data.len() >= 4 => {
                            let count = u32::from_be_bytes([data[0], data[1], data[2], data[3]]) as usize;
                            let units: Vec<u16> =
                                data[4..].chunks_exact(2).take(count).map(|u| u16::from_be_bytes([u[0], u[1]])).collect();
                            name = String::from_utf16_lossy(&units).trim_end_matches('\0').to_owned();
                        }
                        b"lsct" | b"lsdk" if data.len() >= 4 => {
                            let t = u32::from_be_bytes([data[0], data[1], data[2], data[3]]);
                            let b = (data.len() >= 12).then(|| blend_from_key(&data[8..12]));
                            section = Some((t, b));
                        }
                        b"lspf" if data.len() >= 4 => {
                            let f = u32::from_be_bytes([data[0], data[1], data[2], data[3]]);
                            locked = f & 0x8000_0000 != 0 || f & 0b110 != 0;
                        }
                        _ => {}
                    }
                    if len % 2 == 1 && c.p < extra_end {
                        c.skip(1)?;
                    }
                }
                c.p = extra_end;
                records.push(InRecord { name, rect, channels, blend, opacity, clip, flags, section, locked });
            }
            for r in &records {
                let (w, h) = ((r.rect.3 - r.rect.1).max(0) as usize, (r.rect.2 - r.rect.0).max(0) as usize);
                if w as u32 > MAX_SIDE || h as u32 > MAX_SIDE {
                    return Err("A layer in this PSD is too big.".into());
                }
                let mut planes = HashMap::new();
                for (id, len) in &r.channels {
                    let data = c.take(*len)?;
                    if let Some(p) = read_channel(data, w, h)? {
                        planes.insert(*id, p);
                    }
                }
                pixels.push(planes);
            }
        }
    }
    let have_layers = !records.is_empty();
    if lm_len > 0 {
        c.p = lm_end;
    }

    // build the layer tree, top first
    let mut doc_layers: Vec<Layer> = Vec::new();
    let mut paper: Option<Color32> = None;
    let mut paper_visible = true;
    let mut next_id: LayerId = 1;
    let mut stack: Vec<LayerId> = Vec::new();
    let bottom_index = 0;
    for (i, r) in records.iter().enumerate().rev() {
        let parent = stack.last().copied();
        let mut l = Layer {
            id: next_id,
            parent,
            name: r.name.clone(),
            kind: LayerKind::Raster,
            visible: r.flags & 2 == 0,
            locked: r.locked,
            lock_alpha: r.flags & 1 != 0,
            clip: r.clip,
            expanded: true,
            tag: 0,
            opacity: (r.opacity as f32 / 255.0 * 100.0).round(),
            blend: r.blend,
            tiles: HashMap::new(),
            fav: false,
            vector: None,
            fx: Vec::new(),
            adjust: None,
            light: None,
            depth: false,
            material: None,
            lightfx: None,
            solid: false,
            text: None,
            part: None,
            rev: 1,
        };
        next_id += 1;
        match r.section {
            Some((3, _)) => {
                stack.pop();
                continue;
            }
            Some((t @ (1 | 2), b)) => {
                l.kind = LayerKind::Group;
                l.expanded = t == 1;
                l.blend = b.unwrap_or(r.blend);
                l.clip = false;
                stack.push(l.id);
                doc_layers.push(l);
                continue;
            }
            _ => {}
        }
        let planes = &pixels[i];
        let (pw, ph) = ((r.rect.3 - r.rect.1).max(0) as u32, (r.rect.2 - r.rect.0).max(0) as u32);
        if pw > 0 && ph > 0 {
            let get = |id: i16| planes.get(&id);
            let (rr, gg, bb) = if gray { (get(0), get(0), get(0)) } else { (get(0), get(1), get(2)) };
            let alpha = get(-1);
            let img = image::RgbaImage::from_fn(pw, ph, |x, y| {
                let k = (y * pw + x) as usize;
                let v = |p: Option<&Vec<u8>>, d: u8| p.and_then(|p| p.get(k).copied()).unwrap_or(d);
                image::Rgba([v(rr, 0), v(gg, 0), v(bb, 0), v(alpha, 255)])
            });
            // the bottom-most top-level layer named "Paper", one solid color over the whole canvas → our paper
            let solid = img.pixels().all(|p| *p == *img.get_pixel(0, 0)) && img.get_pixel(0, 0).0[3] == 255;
            if i == bottom_index && parent.is_none() && r.name == PAPER_NAME && solid && pw == w && ph == h {
                let p = img.get_pixel(0, 0).0;
                paper = Some(Color32::from_rgb(p[0], p[1], p[2]));
                paper_visible = l.visible;
                continue;
            }
            l.tiles = tiles_from_rgba(&img, r.rect.1, r.rect.0, w, h);
        }
        doc_layers.push(l);
    }

    if !have_layers {
        // a flat PSD: the merged picture becomes one layer
        let comp = c.u16()?;
        let rest = &bytes[c.p..];
        let (wu, hu) = (w as usize, h as usize);
        let planes: Vec<Vec<u8>> = match comp {
            0 => (0..nchan.min(4)).map(|k| rest.get(k * wu * hu..(k + 1) * wu * hu).map(|s| s.to_vec()).unwrap_or_default()).collect(),
            1 => {
                let counts_len = nchan * hu * 2;
                if rest.len() < counts_len {
                    return Err("The PSD picture data is damaged.".into());
                }
                let mut at = counts_len;
                let mut planes = Vec::new();
                for k in 0..nchan.min(4) {
                    let mut plane = Vec::with_capacity(wu * hu);
                    for y in 0..hu {
                        let i = (k * hu + y) * 2;
                        let len = u16::from_be_bytes([rest[i], rest[i + 1]]) as usize;
                        let end = (at + len).min(rest.len());
                        plane.extend(unpackbits(&rest[at.min(end)..end], wu)?);
                        at = end;
                    }
                    planes.push(plane);
                }
                planes
            }
            _ => return Err("This PSD's picture uses a compression that isn't supported yet.".into()),
        };
        let px = |k: usize, i: usize| planes.get(k).and_then(|p| p.get(i)).copied().unwrap_or(0);
        let img = image::RgbaImage::from_fn(w, h, |x, y| {
            let i = (y * w + x) as usize;
            if gray {
                let g = px(0, i);
                image::Rgba([g, g, g, 255])
            } else {
                image::Rgba([px(0, i), px(1, i), px(2, i), if nchan >= 4 { px(3, i) } else { 255 }])
            }
        });
        let mut l = Layer {
            id: next_id,
            parent: None,
            name: "Background".into(),
            kind: LayerKind::Raster,
            visible: true,
            locked: false,
            lock_alpha: false,
            clip: false,
            expanded: true,
            tag: 0,
            opacity: 100.0,
            blend: Blend::Normal,
            tiles: HashMap::new(),
            fav: false,
            vector: None,
            fx: Vec::new(),
            adjust: None,
            light: None,
            depth: false,
            material: None,
            lightfx: None,
            solid: false,
            text: None,
            part: None,
            rev: 1,
        };
        next_id += 1;
        l.tiles = tiles_from_rgba(&img, 0, 0, w, h);
        doc_layers.push(l);
    }

    // a paper layer at the bottom (hidden when the PSD had none, so transparency stays)
    let mut paper_layer = Layer {
        id: next_id,
        parent: None,
        name: PAPER_NAME.into(),
        kind: LayerKind::Paper,
        visible: paper.is_some() && paper_visible,
        locked: false,
        lock_alpha: false,
        clip: false,
        expanded: true,
        tag: 0,
        opacity: 100.0,
        blend: Blend::Normal,
        tiles: HashMap::new(),
        fav: false,
        vector: None,
        fx: Vec::new(),
        adjust: None,
        light: None,
        depth: false,
        material: None,
        lightfx: None,
        solid: false,
        text: None,
        part: None,
        rev: 1,
    };
    if paper.is_none() {
        paper_layer.visible = false;
    }
    doc_layers.push(paper_layer);
    let mut doc = Document::from_parts(w, h, paper.unwrap_or(Color32::WHITE), doc_layers);
    if let Some(d) = dpi {
        doc.meta.dpi = d;
    }
    super::project::ensure_usable(&mut doc);
    Ok(doc)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::files::testdoc;

    #[test]
    fn packbits_round_trip() {
        let rows: [&[u8]; 5] = [&[1, 1, 1, 1, 2, 3, 4, 4, 4], &[7; 300], &[1, 2, 3, 4, 5], &[], &[9, 9]];
        for row in rows {
            let mut enc = Vec::new();
            packbits(row, &mut enc);
            assert_eq!(unpackbits(&enc, row.len()).unwrap(), row);
        }
    }

    #[test]
    fn psd_round_trip_keeps_layers() {
        let d = testdoc::sample();
        let bytes = write(&d);
        let back = read(&bytes).unwrap();
        assert_eq!((back.w, back.h), (d.w, d.h));
        assert_eq!(back.paper, d.paper);
        assert_eq!(testdoc::shape(&back), testdoc::shape(&d));
        // pixels survive (unpremultiplied 8-bit round trip)
        let a = crate::files::flatten(&d);
        let b = crate::files::flatten(&back);
        let max_diff = a.pixels().zip(b.pixels()).flat_map(|(p, q)| (0..4).map(move |c| (p.0[c] as i32 - q.0[c] as i32).abs())).max().unwrap();
        assert!(max_diff <= 2, "flattened pictures differ by {max_diff}");
    }

    #[test]
    fn rejects_garbage() {
        assert!(read(b"not a psd at all").is_err());
        let mut bad = write(&testdoc::sample());
        bad.truncate(200);
        assert!(read(&bad).is_err());
    }
}
