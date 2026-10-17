// SPDX-License-Identifier: GPL-3.0-or-later
// Krita (.kra) import: ZIP with maindoc.xml (the layer tree) and one tiled, LZF-compressed
// pixel file per paint layer. 8-bit RGBA layers are read with their folders, blend modes,
// opacity and visibility; files in other color depths fall back to Krita's merged picture.

use std::collections::HashMap;
use std::io::Read;

use eframe::egui::Color32;
use quick_xml::events::Event;

use super::{MAX_SIDE, tiles_from_rgba};
use crate::doc::{Blend, Document, Layer, LayerId, LayerKind};

const KTILE: usize = 64;

/// liblzf decompression (what Krita uses for its tiles). None when the data is damaged.
pub fn lzf_decompress(input: &[u8], out_len: usize) -> Option<Vec<u8>> {
    let mut out = Vec::with_capacity(out_len);
    let mut ip = 0;
    while ip < input.len() {
        let ctrl = input[ip] as usize;
        ip += 1;
        if ctrl < 32 {
            // literal run of ctrl + 1 bytes
            let n = ctrl + 1;
            if ip + n > input.len() || out.len() + n > out_len {
                return None;
            }
            out.extend_from_slice(&input[ip..ip + n]);
            ip += n;
        } else {
            // back reference
            let mut len = ctrl >> 5;
            if len == 7 {
                len += *input.get(ip)? as usize;
                ip += 1;
            }
            len += 2;
            let back = ((ctrl & 0x1f) << 8) + *input.get(ip)? as usize + 1;
            ip += 1;
            if back > out.len() || out.len() + len > out_len {
                return None;
            }
            let start = out.len() - back;
            for k in 0..len {
                let b = out[start + k];
                out.push(b);
            }
        }
    }
    (out.len() == out_len).then_some(out)
}

fn blend_from_krita(op: &str) -> Blend {
    match op {
        "multiply" => Blend::Multiply,
        "screen" => Blend::Screen,
        "overlay" => Blend::Overlay,
        "darken" => Blend::Darken,
        "lighten" => Blend::Lighten,
        "dodge" => Blend::ColorDodge,
        "burn" => Blend::ColorBurn,
        "hard_light" => Blend::HardLight,
        "soft_light" | "soft_light_svg" => Blend::SoftLight,
        "diff" => Blend::Difference,
        "exclusion" => Blend::Exclusion,
        "hue" => Blend::Hue,
        "saturation" => Blend::Saturation,
        "color" => Blend::Color,
        "luminize" => Blend::Luminosity,
        "add" | "linear_dodge" => Blend::Add,
        _ => Blend::Normal,
    }
}

/// One Krita paint-layer pixel file → an unpremultiplied picture of the canvas (plus offset).
fn read_layer_pixels(data: &[u8], ox: i32, oy: i32, w: u32, h: u32) -> Result<image::RgbaImage, String> {
    let bad = || "A Krita layer is damaged.".to_string();
    let mut pos = 0;
    fn next_line(data: &[u8], pos: &mut usize) -> Option<String> {
        let end = data[*pos..].iter().position(|&b| b == b'\n')? + *pos;
        let s = String::from_utf8_lossy(&data[*pos..end]).into_owned();
        *pos = end + 1;
        Some(s)
    }
    let mut header = HashMap::new();
    for _ in 0..5 {
        let l = next_line(data, &mut pos).ok_or_else(bad)?;
        let mut it = l.splitn(2, ' ');
        header.insert(it.next().unwrap_or("").to_owned(), it.next().unwrap_or("").trim().to_owned());
    }
    let num = |k: &str| header.get(k).and_then(|v| v.parse::<usize>().ok());
    if num("VERSION") != Some(2) || num("TILEWIDTH") != Some(KTILE) || num("TILEHEIGHT") != Some(KTILE) {
        return Err("This Krita file uses a tile format that isn't supported.".into());
    }
    if num("PIXELSIZE") != Some(4) {
        return Err("16-bit and 32-bit Krita layers aren't supported yet.".into());
    }
    let tiles = num("DATA").ok_or_else(bad)?;
    let raw_len = KTILE * KTILE * 4;
    let mut img = image::RgbaImage::new(w, h);
    for _ in 0..tiles.min(1 << 20) {
        let l = next_line(data, &mut pos).ok_or_else(bad)?;
        let parts: Vec<&str> = l.split(',').collect();
        if parts.len() != 4 {
            return Err(bad());
        }
        let tx: i32 = parts[0].trim().parse().map_err(|_| bad())?;
        let ty: i32 = parts[1].trim().parse().map_err(|_| bad())?;
        let size: usize = parts[3].trim().parse().map_err(|_| bad())?;
        if parts[2].trim() != "LZF" || size == 0 || size > raw_len * 2 || pos + size > data.len() {
            return Err(bad());
        }
        let chunk = &data[pos..pos + size];
        pos += size;
        // flag 1: LZF of the planar bytes (all B, then all G, R, A); flag 0: plain BGRA pixels
        let px: Vec<u8> = match chunk[0] {
            1 => {
                let planar = lzf_decompress(&chunk[1..], raw_len).ok_or_else(bad)?;
                let n = KTILE * KTILE;
                (0..raw_len).map(|i| planar[(i % 4) * n + i / 4]).collect()
            }
            _ => {
                if chunk.len() - 1 != raw_len {
                    return Err(bad());
                }
                chunk[1..].to_vec()
            }
        };
        for j in 0..KTILE {
            let y = ty + oy + j as i32;
            if y < 0 || y >= h as i32 {
                continue;
            }
            for i in 0..KTILE {
                let x = tx + ox + i as i32;
                if x < 0 || x >= w as i32 {
                    continue;
                }
                let k = (j * KTILE + i) * 4;
                if px[k + 3] != 0 {
                    img.put_pixel(x as u32, y as u32, image::Rgba([px[k + 2], px[k + 1], px[k], px[k + 3]]));
                }
            }
        }
    }
    Ok(img)
}

fn layer(id: LayerId, parent: Option<LayerId>, name: String, kind: LayerKind) -> Layer {
    Layer {
        id,
        parent,
        name,
        kind,
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
        rev: 1,
    }
}

pub fn read(path: &std::path::Path) -> Result<Document, String> {
    let f = std::fs::File::open(path).map_err(|e| e.to_string())?;
    let mut zip = zip::ZipArchive::new(std::io::BufReader::new(f)).map_err(|_| "This isn't a Krita file.".to_string())?;
    let names: Vec<String> = zip.file_names().filter_map(|n| n.ok().map(|n| n.into_owned())).collect();
    let mut read_entry = |name: &str, max: u64| -> Result<Vec<u8>, String> {
        let mut e = zip.by_name(name).map_err(|_| format!("The file is missing {name}"))?;
        if e.size() > max {
            return Err(format!("{name} is too big"));
        }
        let mut v = Vec::new();
        e.by_ref().take(max).read_to_end(&mut v).map_err(|e| e.to_string())?;
        Ok(v)
    };
    let xml = String::from_utf8(read_entry("maindoc.xml", 16 << 20)?).map_err(|_| "maindoc.xml is not text".to_string())?;
    let mut reader = quick_xml::Reader::from_str(&xml);
    let (mut w, mut h) = (0u32, 0u32);
    let mut depth_ok = true; // false when the canvas is not 8-bit RGBA
    let mut layers: Vec<Layer> = Vec::new();
    let mut open: Vec<Option<LayerId>> = Vec::new(); // <layer> elements we are inside (Some = folder)
    let mut next: LayerId = 1;
    let mut pixels: Vec<(LayerId, String, i32, i32)> = Vec::new();
    loop {
        let ev = reader.read_event().map_err(|e| format!("maindoc.xml is damaged: {e}"))?;
        let (e, is_empty) = match &ev {
            Event::Start(e) => (e, false),
            Event::Empty(e) => (e, true),
            Event::End(e) => {
                if e.name().as_ref() == "layer" {
                    open.pop();
                }
                continue;
            }
            Event::Eof => break,
            _ => continue,
        };
        let mut a: HashMap<String, String> = HashMap::new();
        for at in e.attributes().flatten() {
            let val = quick_xml::escape::unescape(&at.value).map(|v| v.into_owned()).unwrap_or_else(|_| at.value.to_string());
            a.insert(at.key.as_ref().to_owned(), val);
        }
        let get = |k: &str| a.get(k).cloned().unwrap_or_default();
        match e.name().as_ref() {
            "IMAGE" if w == 0 => {
                w = get("width").parse().unwrap_or(0);
                h = get("height").parse().unwrap_or(0);
                if w == 0 || h == 0 || w > MAX_SIDE || h > MAX_SIDE {
                    return Err("The canvas size in this file is not valid or too big.".into());
                }
                depth_ok = get("colorspacename") == "RGBA";
            }
            "layer" => {
                let parent = open.iter().rev().find_map(|x| *x);
                let kind = match get("nodetype").as_str() {
                    "grouplayer" => LayerKind::Group,
                    "paintlayer" => LayerKind::Raster,
                    _ => {
                        // vector, file, fill and filter layers: not read (the merged picture still shows them)
                        if !is_empty {
                            open.push(None);
                        }
                        continue;
                    }
                };
                let id = next;
                next += 1;
                let mut l = layer(id, parent, get("name"), kind);
                l.visible = get("visible") != "0";
                l.locked = get("locked") == "1";
                l.opacity = get("opacity").parse::<f32>().map(|o| o / 255.0 * 100.0).unwrap_or(100.0).clamp(0.0, 100.0);
                l.clip = get("inheritalpha") == "1";
                l.expanded = get("collapsed") != "1";
                l.blend = if kind == LayerKind::Group && get("passthrough") == "1" { Blend::PassThrough } else { blend_from_krita(&get("compositeop")) };
                if kind == LayerKind::Raster {
                    if !get("colorspacename").is_empty() && get("colorspacename") != "RGBA" {
                        depth_ok = false;
                    }
                    pixels.push((id, get("filename"), get("x").parse().unwrap_or(0), get("y").parse().unwrap_or(0)));
                }
                layers.push(l);
                if !is_empty {
                    open.push((kind == LayerKind::Group).then_some(id));
                }
            }
            _ => {}
        }
    }
    if w == 0 {
        return Err("This Krita file has no canvas size.".into());
    }
    let mut use_merged = !depth_ok || pixels.is_empty();
    let mut why = String::from("This Krita file can't be read (no merged picture inside).");
    if !use_merged {
        for (id, file, x, y) in &pixels {
            let suffix = format!("/layers/{file}");
            let Some(entry) = names.iter().find(|n| n.ends_with(&suffix)) else { continue };
            let data = read_entry(entry, 1 << 30)?;
            match read_layer_pixels(&data, *x, *y, w, h) {
                Ok(img) => {
                    if let Some(l) = layers.iter_mut().find(|l| l.id == *id) {
                        l.tiles = tiles_from_rgba(&img, 0, 0, w, h);
                    }
                }
                Err(e) => {
                    why = e;
                    use_merged = true;
                    break;
                }
            }
        }
    }
    if use_merged {
        // Krita keeps a flattened copy of the picture: use it as one layer
        let bytes = read_entry("mergedimage.png", 1 << 30).map_err(|_| why)?;
        let img = super::decode_limited(bytes)?;
        let mut l = layer(1, None, "Krita picture".into(), LayerKind::Raster);
        l.tiles = tiles_from_rgba(&img, 0, 0, w, h);
        layers = vec![l];
        next = 2;
    }
    let mut paper = layer(next, None, "Paper".into(), LayerKind::Paper);
    paper.visible = false; // Krita has no paper: its background is a normal layer
    layers.push(paper);
    let mut doc = Document::from_parts(w, h, Color32::WHITE, layers);
    super::project::ensure_usable(&mut doc);
    Ok(doc)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    /// Literal-only LZF (valid input for the decompressor).
    fn lzf_literal(data: &[u8]) -> Vec<u8> {
        let mut out = Vec::new();
        for c in data.chunks(32) {
            out.push((c.len() - 1) as u8);
            out.extend_from_slice(c);
        }
        out
    }

    #[test]
    fn lzf_back_references() {
        // "abc" then copy 6 bytes from 3 back = "abcabcabc"
        let stream = [2, b'a', b'b', b'c', (4 << 5) as u8, 2];
        assert_eq!(lzf_decompress(&stream, 9).unwrap(), b"abcabcabc");
        assert!(lzf_decompress(&[0x20, 5], 4).is_none()); // reference before the start
    }

    fn tile_file(tiles: &[(i32, i32, [u8; 4], bool)]) -> Vec<u8> {
        let mut v = format!("VERSION 2\nTILEWIDTH 64\nTILEHEIGHT 64\nPIXELSIZE 4\nDATA {}\n", tiles.len()).into_bytes();
        for (x, y, rgba, compressed) in tiles {
            let bgra = [rgba[2], rgba[1], rgba[0], rgba[3]];
            let mut body = if *compressed {
                let n = KTILE * KTILE;
                let planar: Vec<u8> = (0..n * 4).map(|i| bgra[i / n]).collect();
                let mut b = vec![1];
                b.extend(lzf_literal(&planar));
                b
            } else {
                let mut b = vec![0];
                for _ in 0..KTILE * KTILE {
                    b.extend_from_slice(&bgra);
                }
                b
            };
            v.extend(format!("{x},{y},LZF,{}\n", body.len()).into_bytes());
            v.append(&mut body);
        }
        v
    }

    #[test]
    fn reads_layers_and_folders() {
        let dir = std::env::temp_dir().join(format!("seviath_kra_test_{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let p = dir.join("t.kra");
        let maindoc = r#"<?xml version="1.0"?><DOC><IMAGE width="200" height="100" colorspacename="RGBA" name="Pic"><layers>
            <layer name="Shade" nodetype="paintlayer" filename="layer3" compositeop="multiply" opacity="128" visible="1" x="10" y="0" inheritalpha="1"/>
            <layer name="Hair &amp; face" nodetype="grouplayer" filename="layer2" passthrough="1" visible="1" opacity="255" collapsed="1"><layers>
              <layer name="Lines" nodetype="paintlayer" filename="layer4" opacity="255" visible="0" locked="1" x="0" y="0"/>
            </layers></layer>
            <layer name="Vector" nodetype="shapelayer" filename="layer5"/>
            </layers></IMAGE></DOC>"#;
        let mut z = zip::ZipWriter::new(std::fs::File::create(&p).unwrap());
        let o = zip::write::SimpleFileOptions::default();
        z.start_file("mimetype", o).unwrap();
        z.write_all(b"application/x-krita").unwrap();
        z.start_file("maindoc.xml", o).unwrap();
        z.write_all(maindoc.as_bytes()).unwrap();
        z.start_file("Pic/layers/layer3", o).unwrap();
        z.write_all(&tile_file(&[(0, 0, [200, 40, 30, 255], true), (64, 64, [0, 0, 255, 128], false)])).unwrap();
        z.start_file("Pic/layers/layer4", o).unwrap();
        z.write_all(&tile_file(&[(128, 0, [10, 20, 30, 255], false)])).unwrap();
        z.finish().unwrap();

        let d = read(&p).unwrap();
        let names: Vec<&str> = d.layers.iter().map(|l| l.name.as_str()).collect();
        assert_eq!(names, ["Shade", "Hair & face", "Lines", "Paper"]);
        let shade = &d.layers[0];
        assert_eq!((shade.blend, shade.clip, shade.opacity.round() as i32), (Blend::Multiply, true, 50));
        assert_eq!((d.layers[1].kind, d.layers[1].blend, d.layers[1].expanded), (LayerKind::Group, Blend::PassThrough, false));
        assert_eq!(d.layers[2].parent, Some(d.layers[1].id));
        assert!(!d.layers[2].visible && d.layers[2].locked);
        // pixels: offset x=10 moves the first tile; the second tile is clipped by the canvas height
        let img = crate::files::layer_rgba(shade, 0, 0, 200, 100);
        assert_eq!(img.get_pixel(5, 5).0[3], 0);
        assert_eq!(img.get_pixel(10, 5).0, [200, 40, 30, 255]);
        assert_eq!(img.get_pixel(80, 70).0[3], 128);
        let lines = crate::files::layer_rgba(&d.layers[2], 0, 0, 200, 100);
        assert_eq!(lines.get_pixel(150, 10).0, [10, 20, 30, 255]);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
