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

/// A real VTuber PSD from the user's folder, when it is on this computer: it must open, and the layers are listed.
#[test]
fn a_real_vtuber_psd_opens() {
    let path = std::path::PathBuf::from(r"C:\Users\SaphiresSilver\Downloads\art\Finished2D3D\Sellablebeija.psd");
    if !path.exists() {
        return;
    }
    let t0 = std::time::Instant::now();
    let bytes = std::fs::read(&path).unwrap();
    let doc = read(&bytes).unwrap_or_else(|e| panic!("{e}"));
    eprintln!("opened in {:?}: {}x{}, {} layers", t0.elapsed(), doc.w, doc.h, doc.layers.len());
    let mut depth = |i: usize| {
        let mut d = 0;
        let mut p = doc.layers[i].parent;
        while let Some(pid) = p {
            d += 1;
            p = doc.layers.iter().find(|l| l.id == pid).and_then(|l| l.parent);
        }
        d
    };
    for i in 0..doc.layers.len().min(400) {
        let l = &doc.layers[i];
        eprintln!("{}{} [{:?}] {}", "  ".repeat(depth(i)), l.name, l.kind, if l.visible { "" } else { "(hidden)" });
    }
    assert!(doc.layers.len() > 5);
}

/// The user's real VTuber PSDs, through the whole pipeline: open, tag the parts from the names, rig the face and the body, build the 3D
/// character, save it as VRM and check the file. A report is printed; run it with `cargo test --release -- --ignored --nocapture real_psds`.
#[test]
#[ignore]
fn real_psds_run_the_pipeline() {
    use crate::rig::Rig;
    use crate::rig::autorig::{BodyOptions, FaceOptions, rig_body, rig_face};
    let dir = std::path::PathBuf::from(r"C:\Users\SaphiresSilver\Downloads\Vroids and psd 2dstarted");
    let only = std::env::var("PSD").ok();
    for name in ["Beija", "Evielina", "Semi", "vtuberscale", "dreija", "elfie"] {
        if only.as_deref().is_some_and(|o| o != name) {
            continue;
        }
        let path = dir.join(format!("{name}.psd"));
        if !path.exists() {
            continue;
        }
        let t0 = std::time::Instant::now();
        let bytes_for_compare = std::fs::read(&path).unwrap();
        let mut doc = match read(&bytes_for_compare) {
            Ok(d) => d,
            Err(e) => {
                eprintln!("== {name}: DOES NOT OPEN: {e}");
                continue;
            }
        };
        eprintln!("== {name}: {}x{}, {} layers, opened in {:?}", doc.w, doc.h, doc.layers.len(), t0.elapsed());
        let t_tidy = std::time::Instant::now();
        let rep = crate::prepare::tidy(&mut doc, &[], false);
        let (sug, tagged): (Vec<()>, usize) = (Vec::new(), rep.parts);
        if std::env::var("TAGS").is_ok() {
            for l in doc.layers.iter().filter(|l| crate::parts::is_part_layer(l) && !l.tiles.is_empty()) {
                let mut chain = String::new();
                let mut p = l.parent;
                while let Some(pid) = p {
                    let Some(f) = doc.layers.iter().find(|x| x.id == pid) else { break };
                    chain += &format!(" < {}", f.name);
                    p = f.parent;
                }
                let at = crate::files::layer_bounds(l, doc.w, doc.h).map(|b| format!("x {:.2}-{:.2} y {:.2}", b.0 as f32 / doc.w as f32, b.2 as f32 / doc.w as f32, (b.1 + b.3) as f32 / 2.0 / doc.h as f32)).unwrap_or_default();
                eprintln!("   tag {:?} <- {} (blend {:?}, clip {}, {at}){chain}", l.part, l.name, l.blend, l.clip);
            }
        }
        eprintln!("   tidy in {:?}: {} layers tagged, {} kinds, {} clips baked, left alone: {} ({})", t_tidy.elapsed(), rep.tagged, rep.parts, rep.clips_baked, rep.left_alone.len(), rep.left_alone.iter().take(8).cloned().collect::<Vec<_>>().join(", "));
        let checks = crate::parts::checklist(&doc.layers);
        eprintln!("   parts: {} suggestions, {tagged} tagged; checklist: {}", sug.len(), checks.iter().map(|c| format!("{:?} {}", c.level, c.text)).collect::<Vec<_>>().join(" | "));
        let mut rig = Rig::default();
        rig.clip_to = doc.rig.clip_to.clone();
        let t1 = std::time::Instant::now();
        match rig_face(&mut rig, &doc.layers, doc.w, doc.h, &FaceOptions::default()) {
            Ok(r) => eprintln!("   face rig ok in {:?}: {:?}", t1.elapsed(), r.notes.iter().take(4).collect::<Vec<_>>()),
            Err(e) => eprintln!("   FACE RIG FAILED: {e}"),
        }
        match rig_body(&mut rig, &doc.layers, doc.w, doc.h, &BodyOptions::default()) {
            Ok(r) => eprintln!("   body rig ok: {:?}", r.notes.iter().take(4).collect::<Vec<_>>()),
            Err(e) => eprintln!("   BODY RIG FAILED: {e}"),
        }
        eprintln!("   rig: {} meshes, {} changers, {} sliders", rig.meshes.len(), rig.changers.len(), rig.params.len());
        doc.rig = std::sync::Arc::new(rig);
        // a picture of the rig at rest, and with the head turned, to look at (target/screens/psd_<name>_*.png)
        {
            use crate::rig::raster::{self, Item, LayerTex};
            use crate::rig::Values;
            let _ = std::fs::create_dir_all("target/screens");
            let rig = &*doc.rig;
            let (w, h) = (doc.w, doc.h);
            let scale = (900.0 / w.max(h) as f32).min(1.0);
            let (pw, ph) = (((w as f32 * scale) as usize).max(8), ((h as f32 * scale) as usize).max(8));
            // the head turned (eyes open), and the eyes closed
            let mut turned = Values::default();
            let mut closed = Values::default();
            for p in rig.params.iter() {
                if p.name.starts_with("Angle X") {
                    turned.set(p.id, p.max * 0.8);
                }
                if p.name.starts_with("Eyes open") {
                    closed.set(p.id, p.min);
                }
            }
            // every layer that is shown, as the app draws them: a layer with a mesh by its mesh, any other as a flat picture
            struct Piece {
                id: crate::doc::LayerId,
                tex: LayerTex,
                rest: Vec<[f32; 2]>,
                now: Vec<[f32; 2]>,
                tris: Vec<[u32; 3]>,
                op: f32,
                blend: crate::doc::Blend,
            }
            fn pieces_for(doc: &crate::doc::Document, rig: &crate::rig::Rig, all: &[Vec<[f32; 2]>]) -> Vec<Piece> {
                let mut pieces: Vec<Piece> = Vec::new();
                for l in doc.layers.iter().rev().filter(|l| l.kind == LayerKind::Raster && l.visible && std::env::var("HIDE").map_or(true, |h| !l.name.contains(&h))) {
                    let (mut shown, mut op) = (true, l.opacity / 100.0);
                    let mut p = l.parent;
                    while let Some(pid) = p {
                        let Some(f) = doc.layers.iter().find(|x| x.id == pid) else { break };
                        shown &= f.visible;
                        op *= f.opacity / 100.0;
                        p = f.parent;
                    }
                    let Some(tex) = LayerTex::of_in(doc, l) else { continue };
                    if !shown {
                        continue;
                    }
                    let (rest, now, tris) = match rig.meshes.iter().position(|m| m.layer == l.id) {
                        Some(mi) => (rig.meshes[mi].verts.clone(), all[mi].clone(), rig.meshes[mi].tris.clone()),
                        None => {
                            let (x0, y0, x1, y1) = (tex.x0 as f32, tex.y0 as f32, (tex.x0 + tex.w as i32) as f32, (tex.y0 + tex.h as i32) as f32);
                            let v = vec![[x0, y0], [x1, y0], [x1, y1], [x0, y1]];
                            (v.clone(), v, vec![[0, 1, 2], [0, 2, 3]])
                        }
                    };
                    pieces.push(Piece { id: l.id, tex, rest, now, tris, op, blend: l.blend });
                }
                pieces
            }
            // how close the rig at rest is to the drawing as the file shows it (both on white, 600 pixels): the stacking must be the same
            if let Some(theirs) = merged_preview(&bytes_for_compare) {
                let k = 600.0 / w.max(h) as f32;
                let (sw, sh) = (((w as f32 * k) as usize).max(8), ((h as f32 * k) as usize).max(8));
                let pose = rig.pose(&Values::default());
                let all = rig.deformed_all(&pose);
                let pieces = pieces_for(&doc, rig, &all);
                let items: Vec<Item> = pieces.iter().map(|p| Item { tex: &p.tex, rest: &p.rest, now: p.now.clone(), tris: &p.tris, opacity: p.op, blend: p.blend }).collect();
                let px = raster::render(&items, sw, sh, k, Some(eframe::egui::Color32::WHITE));
                let small = image::imageops::resize(&theirs, sw as u32, sh as u32, image::imageops::FilterType::Triangle);
                let mut sum = 0.0f64;
                let mut diff_img = image::RgbaImage::new(sw as u32, sh as u32);
                for (i, p) in px.iter().enumerate() {
                    let q = small.get_pixel((i % sw) as u32, (i / sw) as u32).0;
                    let a = q[3] as f64 / 255.0;
                    let mut d = 0.0;
                    for (ch, mine) in [p.r(), p.g(), p.b()].into_iter().enumerate() {
                        let theirs_on_white = q[ch] as f64 * a + 255.0 * (1.0 - a);
                        d += (mine as f64 - theirs_on_white).abs() / 3.0;
                    }
                    sum += d;
                    // where the rig differs from the drawing: dark is the same, bright is different (x4)
                    let v = (d * 4.0).min(255.0) as u8;
                    diff_img.put_pixel((i % sw) as u32, (i / sw) as u32, image::Rgba([v, v, v, 255]));
                }
                let _ = diff_img.save(format!("target/screens/psd_{name}_rigdiff.png"));
                eprintln!("   rig at rest against the file's own picture: mean difference {:.2} of 255", sum / (sw * sh) as f64);
            }
            for (tag, vals) in [("rest", Values::default()), ("turned", turned), ("closed", closed)] {
                let pose = rig.pose(&vals);
                let all = rig.deformed_all(&pose);
                let pieces = pieces_for(&doc, rig, &all);
                let items: Vec<Item> = pieces.iter().map(|p| Item { tex: &p.tex, rest: &p.rest, now: p.now.clone(), tris: &p.tris, opacity: p.op, blend: p.blend }).collect();
                // what is cut to the face (teeth, a skin patch) is cut live to the outline of the face as it is moved
                let ids: Vec<crate::doc::LayerId> = pieces.iter().map(|p| p.id).collect();
                let omask = raster::outline_mask(&doc, crate::parts::Part::Face);
                let line = doc.layers.iter().find(|l| l.part == Some(crate::parts::Part::Face) && l.visible && !l.clip && l.name.to_lowercase().contains("line")).and_then(|l| rig.mesh_of(l.id));
                let masks: Vec<Item> = match (&omask, line) {
                    (Some(t), Some(m)) => vec![Item { tex: t, rest: &m.verts, now: all[rig.meshes.iter().position(|x| x.id == m.id).unwrap()].clone(), tris: &m.tris, opacity: 1.0, blend: crate::doc::Blend::Normal }],
                    _ => Vec::new(),
                };
                let clips: Vec<(usize, usize)> = if masks.is_empty() { Vec::new() } else { doc.rig.clip_to.iter().filter(|c| c.1 == crate::parts::Part::Face).filter_map(|c| ids.iter().position(|i| *i == c.0)).map(|i| (i, 0)).collect() };
                let px = raster::render_with(&items, &masks, &clips, pw, ph, scale, Some(eframe::egui::Color32::from_rgb(244, 242, 236)));
                let mut img = image::RgbaImage::new(pw as u32, ph as u32);
                for (i, p) in px.iter().enumerate() {
                    img.put_pixel((i % pw) as u32, (i / pw) as u32, image::Rgba([p.r(), p.g(), p.b(), 255]));
                }
                let _ = img.save(format!("target/screens/psd_{name}_{tag}.png"));
                // with FACE=1 a bigger picture of the face, to look at the details
                if std::env::var("FACE").is_ok() {
                    let (mut lo, mut hi) = ([f32::MAX; 2], [f32::MIN; 2]);
                    for m in &rig.meshes {
                        if doc.layers.iter().find(|l| l.id == m.layer).and_then(|l| l.part) == Some(crate::parts::Part::Face) {
                            for v in &m.verts {
                                for a in 0..2 {
                                    lo[a] = lo[a].min(v[a]);
                                    hi[a] = hi[a].max(v[a]);
                                }
                            }
                        }
                    }
                    if lo[0] < hi[0] {
                        let s = 700.0 / (hi[0] - lo[0]).max(hi[1] - lo[1]);
                        let (fw, fh) = (((w as f32 * s) as usize).max(8), ((h as f32 * s) as usize).max(8));
                        let px = raster::render_with(&items, &masks, &clips, fw, fh, s, Some(eframe::egui::Color32::from_rgb(244, 242, 236)));
                        let (x0, y0) = (((lo[0] - 40.0).max(0.0) * s) as usize, ((lo[1] - 40.0).max(0.0) * s) as usize);
                        let (cw, ch) = ((((hi[0] - lo[0]) + 80.0) * s) as usize, (((hi[1] - lo[1]) + 80.0) * s) as usize);
                        let mut crop = image::RgbaImage::new(cw.min(fw - x0) as u32, ch.min(fh - y0) as u32);
                        for y in 0..crop.height() {
                            for x in 0..crop.width() {
                                let p = px[(y0 + y as usize) * fw + x0 + x as usize];
                                crop.put_pixel(x, y, image::Rgba([p.r(), p.g(), p.b(), 255]));
                            }
                        }
                        let _ = crop.save(format!("target/screens/psd_{name}_face_{tag}.png"));
                        // CROP=x0,y0,x1,y1 (pixels of the face picture): the same place three times bigger
                        if let Ok(c) = std::env::var("CROP") {
                            let n: Vec<u32> = c.split(',').filter_map(|v| v.trim().parse().ok()).collect();
                            if n.len() == 4 && n[2] > n[0] && n[3] > n[1] && n[2] <= crop.width() && n[3] <= crop.height() {
                                let sub = image::imageops::crop_imm(&crop, n[0], n[1], n[2] - n[0], n[3] - n[1]).to_image();
                                let big = image::imageops::resize(&sub, sub.width() * 3, sub.height() * 3, image::imageops::FilterType::Nearest);
                                let _ = big.save(format!("target/screens/psd_{name}_crop_{tag}.png"));
                            }
                        }
                    }
                }
            }
        }
        let t2 = std::time::Instant::now();
        match crate::vrm::model::build(&doc, &crate::vrm::model::Options { name: name.into(), author: "test".into(), ..Default::default() }) {
            Some(m) => {
                eprintln!("   3D built in {:?}: {} parts, {} bones, {} expressions", t2.elapsed(), m.parts.len(), m.bones.len(), m.expressions.len());
                // the 3D character from the front, and with the guide lines of the face (blue the eyes, red the mouth...)
                if std::env::var("FACE").is_ok() {
                    let img = crate::vrm::preview::render_guides(&m, 0.0, 700, 1000, &[], true);
                    let _ = img.save(format!("target/screens/psd_{name}_3d_guides.png"));
                    let img = crate::vrm::preview::render(&m, 0.0, 700, 1000, &[]);
                    let _ = img.save(format!("target/screens/psd_{name}_3d.png"));
                    for (n, v) in [("blink", 1.0f32), ("happy", 1.0)] {
                        let img = crate::vrm::preview::render(&m, 0.0, 700, 1000, &[(n, v)]);
                        let _ = img.save(format!("target/screens/psd_{name}_3d_{n}.png"));
                    }
                }
                match crate::vrm::export::export(&m, crate::vrm::export::Format::Vrm1).and_then(|b| crate::vrm::export::validate(&b).map(|s| (b.len(), s))) {
                    Ok((n, s)) => eprintln!("   VRM 1.0 ok: {} KB, {:?}", n / 1024, s),
                    Err(e) => eprintln!("   VRM FAILED: {e}"),
                }
            }
            None => eprintln!("   3D BUILD FAILED: nothing could be built (no meshed parts?)"),
        }
    }
}

/// The flattened picture that Photoshop saved inside the file (what other programs show as the preview), for comparing with ours.
#[cfg(test)]
fn merged_preview(bytes: &[u8]) -> Option<image::RgbaImage> {
    let mut c = Cur { b: bytes, p: 0 };
    c.take(4).ok()?;
    c.u16().ok()?;
    c.skip(6).ok()?;
    let nchan = c.u16().ok()? as usize;
    let (h, w) = (c.u32().ok()? as usize, c.u32().ok()? as usize);
    c.u16().ok()?;
    c.u16().ok()?;
    // the colour mode data, the image resources and the layers
    for _ in 0..3 {
        let n = c.u32().ok()? as usize;
        c.skip(n).ok()?;
    }
    let comp = c.u16().ok()?;
    let rest = &bytes[c.p..];
    if comp != 1 || nchan < 3 {
        return None;
    }
    let counts_len = nchan * h * 2;
    let mut at = counts_len;
    let mut planes: Vec<Vec<u8>> = Vec::new();
    for k in 0..nchan.min(4) {
        let mut plane = Vec::with_capacity(w * h);
        for y in 0..h {
            let i = (k * h + y) * 2;
            let len = u16::from_be_bytes([*rest.get(i)?, *rest.get(i + 1)?]) as usize;
            let end = (at + len).min(rest.len());
            plane.extend(unpackbits(&rest[at.min(end)..end], w).ok()?);
            at = end;
        }
        planes.push(plane);
    }
    Some(image::RgbaImage::from_fn(w as u32, h as u32, |x, y| {
        let i = y as usize * w + x as usize;
        image::Rgba([planes[0][i], planes[1][i], planes[2][i], planes.get(3).map(|p| p[i]).unwrap_or(255)])
    }))
}

/// Is the order of the layers as the file has it? Our flattened picture of the layers is compared with the preview inside the file.
/// A big difference means layers are in the wrong place (or a blend mode is not matched). The pictures are saved side by side.
#[test]
#[ignore]
fn real_psds_have_their_layer_order_as_in_the_file() {
    let dir = std::path::PathBuf::from(r"C:\Users\SaphiresSilver\Downloads\Vroids and psd 2dstarted");
    let only = std::env::var("PSD").ok();
    let _ = std::fs::create_dir_all("target/screens");
    for name in ["Beija", "Evielina", "Semi", "vtuberscale", "dreija", "elfie"] {
        if only.as_deref().is_some_and(|o| o != name) {
            continue;
        }
        let path = dir.join(format!("{name}.psd"));
        let Ok(bytes) = std::fs::read(&path) else { continue };
        let doc = read(&bytes).unwrap();
        let Some(theirs) = merged_preview(&bytes) else {
            eprintln!("== {name}: the file has no preview I can read");
            continue;
        };
        let ours = crate::files::flatten(&doc);
        let small = |img: &image::RgbaImage| {
            let k = 600.0 / img.width().max(img.height()) as f32;
            let s = image::imageops::resize(img, (img.width() as f32 * k) as u32, (img.height() as f32 * k) as u32, image::imageops::FilterType::Triangle);
            // on white, so see-through places compare as the same
            let mut out = s.clone();
            for p in out.pixels_mut() {
                let a = p.0[3] as u32;
                for ch in 0..3 {
                    p.0[ch] = ((p.0[ch] as u32 * a + 255 * (255 - a)) / 255) as u8;
                }
                p.0[3] = 255;
            }
            out
        };
        let (a, b) = (small(&ours), small(&theirs));
        let diff: f64 = a.pixels().zip(b.pixels()).map(|(p, q)| (0..3).map(|k| (p.0[k] as f64 - q.0[k] as f64).abs()).sum::<f64>() / 3.0).sum::<f64>() / (a.width() * a.height()) as f64;
        eprintln!("== {name}: mean difference from the file's own preview: {diff:.2} of 255 ({} layers)", doc.layers.len());
        let mut both = image::RgbaImage::new(a.width() * 2, a.height());
        image::imageops::replace(&mut both, &a, 0, 0);
        image::imageops::replace(&mut both, &b, a.width() as i64, 0);
        let _ = both.save(format!("target/screens/psd_{name}_order.png"));
    }
}
