// SPDX-License-Identifier: GPL-3.0-or-later
// OpenRaster (.ora) — the open layered format shared by Krita, GIMP and MyPaint:
// a ZIP with stack.xml (layer tree), one PNG per layer, a merged picture and a thumbnail.

use std::collections::HashMap;
use std::io::{Read, Write};

use eframe::egui::Color32;
use quick_xml::events::Event;

use super::{MAX_SIDE, layer_bounds, layer_rgba, tiles_from_rgba};
use crate::doc::{Blend, Document, Layer, LayerId, LayerKind};

fn op_name(b: Blend) -> &'static str {
    match b {
        Blend::PassThrough | Blend::Normal => "svg:src-over",
        Blend::Multiply => "svg:multiply",
        Blend::Screen => "svg:screen",
        Blend::Overlay => "svg:overlay",
        Blend::Darken => "svg:darken",
        Blend::Lighten => "svg:lighten",
        Blend::ColorDodge => "svg:color-dodge",
        Blend::ColorBurn => "svg:color-burn",
        Blend::HardLight => "svg:hard-light",
        Blend::SoftLight => "svg:soft-light",
        Blend::Difference => "svg:difference",
        Blend::Exclusion => "svg:exclusion",
        Blend::Hue => "svg:hue",
        Blend::Saturation => "svg:saturation",
        Blend::Color => "svg:color",
        Blend::Luminosity => "svg:luminosity",
        Blend::Add => "svg:plus",
    }
}

fn op_from_name(s: &str) -> Blend {
    Blend::ALL.into_iter().find(|b| op_name(*b) == s).unwrap_or(Blend::Normal)
}

fn esc(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;")
}

fn png(img: &image::RgbaImage) -> Result<Vec<u8>, String> {
    let mut out = Vec::new();
    img.write_to(&mut std::io::Cursor::new(&mut out), image::ImageFormat::Png).map_err(|e| e.to_string())?;
    Ok(out)
}

fn stack_xml(doc: &Document, parent: Option<LayerId>, files: &mut Vec<(String, Vec<u8>)>, out: &mut String) -> Result<(), String> {
    for l in doc.layers.iter().filter(|l| l.parent == parent) {
        let vis = if l.visible { "visible" } else { "hidden" };
        let op = format!("{:.3}", l.opacity / 100.0);
        match l.kind {
            LayerKind::Group => {
                let iso = if l.blend == Blend::PassThrough { "auto" } else { "isolate" };
                out.push_str(&format!(
                    "<stack name=\"{}\" visibility=\"{vis}\" opacity=\"{op}\" composite-op=\"{}\" isolation=\"{iso}\">",
                    esc(&l.name),
                    op_name(l.blend)
                ));
                stack_xml(doc, Some(l.id), files, out)?;
                out.push_str("</stack>");
            }
            LayerKind::Paper => {
                let src = format!("data/paper_{}.png", l.id);
                let img = image::RgbaImage::from_pixel(doc.w, doc.h, image::Rgba(doc.paper.to_srgba_unmultiplied()));
                files.push((src.clone(), png(&img)?));
                out.push_str(&format!(
                    "<layer name=\"{}\" src=\"{src}\" x=\"0\" y=\"0\" visibility=\"{vis}\" opacity=\"{op}\" composite-op=\"svg:src-over\" seviath:paper=\"1\"/>",
                    esc(&l.name)
                ));
            }
            _ => {
                let (x, y, img) = match layer_bounds(l, doc.w, doc.h) {
                    Some((x0, y0, x1, y1)) => (x0, y0, layer_rgba(l, x0, y0, (x1 - x0) as u32, (y1 - y0) as u32)),
                    None => (0, 0, image::RgbaImage::new(1, 1)),
                };
                let src = format!("data/layer_{}.png", l.id);
                files.push((src.clone(), png(&img)?));
                let mut extra = String::new();
                if l.locked {
                    extra.push_str(" edit-locked=\"true\"");
                }
                if l.lock_alpha {
                    extra.push_str(" alpha-preserve=\"true\"");
                }
                if l.clip {
                    extra.push_str(" seviath:clip=\"1\"");
                }
                out.push_str(&format!(
                    "<layer name=\"{}\" src=\"{src}\" x=\"{x}\" y=\"{y}\" visibility=\"{vis}\" opacity=\"{op}\" composite-op=\"{}\"{extra}/>",
                    esc(&l.name),
                    op_name(l.blend)
                ));
            }
        }
    }
    Ok(())
}

pub fn write(doc: &Document, path: &std::path::Path) -> Result<(), String> {
    let mut files = Vec::new();
    let mut body = String::new();
    stack_xml(doc, None, &mut files, &mut body)?;
    let xml = format!(
        "<?xml version='1.0' encoding='UTF-8'?>\n<image version=\"0.0.5\" w=\"{}\" h=\"{}\" xres=\"{}\" yres=\"{}\" xmlns:seviath=\"https://github.com/santamadaraneilande-ctrl/seviathstudio\"><stack>{body}</stack></image>\n",
        doc.w, doc.h, doc.meta.dpi, doc.meta.dpi
    );
    let merged = super::flatten(doc);
    let thumb = super::thumbnail(doc, 256);
    let tmp = path.with_extension("ora.saving");
    let result = (|| -> Result<(), String> {
        let f = std::fs::File::create(&tmp).map_err(|e| e.to_string())?;
        let mut zip = zip::ZipWriter::new(std::io::BufWriter::new(f));
        let stored = zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored);
        let deflated = zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);
        // the spec wants "mimetype" first and uncompressed
        zip.start_file("mimetype", stored).map_err(|e| e.to_string())?;
        zip.write_all(b"image/openraster").map_err(|e| e.to_string())?;
        zip.start_file("stack.xml", deflated).map_err(|e| e.to_string())?;
        zip.write_all(xml.as_bytes()).map_err(|e| e.to_string())?;
        for (name, bytes) in files.iter().chain([("mergedimage.png".to_string(), png(&merged)?), ("Thumbnails/thumbnail.png".to_string(), png(&thumb)?)].iter()) {
            zip.start_file(name.as_str(), stored).map_err(|e| e.to_string())?;
            zip.write_all(bytes).map_err(|e| e.to_string())?;
        }
        zip.finish().map_err(|e| e.to_string())?;
        Ok(())
    })();
    if let Err(e) = result {
        let _ = std::fs::remove_file(&tmp);
        return Err(e);
    }
    std::fs::rename(&tmp, path).map_err(|e| e.to_string())
}

pub fn read(path: &std::path::Path) -> Result<Document, String> {
    let f = std::fs::File::open(path).map_err(|e| e.to_string())?;
    let mut zip = zip::ZipArchive::new(std::io::BufReader::new(f)).map_err(|_| "This isn't an OpenRaster file.".to_string())?;
    let mut read_entry = |name: &str, max: u64| -> Result<Vec<u8>, String> {
        let mut e = zip.by_name(name).map_err(|_| format!("The file is missing {name}"))?;
        if e.size() > max {
            return Err(format!("{name} is too big"));
        }
        let mut v = Vec::new();
        e.by_ref().take(max).read_to_end(&mut v).map_err(|e| e.to_string())?;
        Ok(v)
    };
    let xml = String::from_utf8(read_entry("stack.xml", 16 << 20)?).map_err(|_| "stack.xml is not text".to_string())?;
    let mut reader = quick_xml::Reader::from_str(&xml);
    let (mut w, mut h) = (0u32, 0u32);
    let mut dpi: Option<u32> = None;
    let mut layers: Vec<Layer> = Vec::new();
    let mut stack: Vec<Option<LayerId>> = Vec::new(); // the folders we are inside
    let mut next: LayerId = 1;
    let mut paper: Option<(Color32, bool)> = None;
    let mut pending_png: Vec<(LayerId, String, i32, i32)> = Vec::new();
    loop {
        let ev = reader.read_event().map_err(|e| format!("stack.xml is damaged: {e}"))?;
        let (e, is_empty) = match &ev {
            Event::Start(e) => (e, false),
            Event::Empty(e) => (e, true),
            Event::End(e) => {
                if e.name().as_ref() == "stack" {
                    stack.pop();
                }
                continue;
            }
            Event::Eof => break,
            _ => continue,
        };
        let mut attrs: HashMap<String, String> = HashMap::new();
        for a in e.attributes().flatten() {
            let key = a.key.as_ref().to_owned();
            let val = quick_xml::escape::unescape(&a.value).map(|v| v.into_owned()).unwrap_or_else(|_| a.value.to_string());
            attrs.insert(key, val);
        }
        let get = |k: &str| attrs.get(k).cloned().unwrap_or_default();
        let opacity = attrs.get("opacity").and_then(|v| v.parse::<f32>().ok()).unwrap_or(1.0);
        let visible = attrs.get("visibility").map(|v| v != "hidden").unwrap_or(true);
        match e.name().as_ref() {
            "image" => {
                w = get("w").parse().unwrap_or(0);
                h = get("h").parse().unwrap_or(0);
                if w == 0 || h == 0 || w > MAX_SIDE || h > MAX_SIDE {
                    return Err("The canvas size in this file is not valid or too big.".into());
                }
                dpi = get("xres").parse::<f32>().ok().filter(|d| (1.0..=2400.0).contains(d)).map(|d| d.round() as u32);
            }
            "stack" => {
                if stack.is_empty() {
                    stack.push(None); // the root stack
                } else {
                    let id = next;
                    next += 1;
                    layers.push(Layer {
                        id,
                        parent: stack.last().copied().flatten(),
                        name: get("name"),
                        kind: LayerKind::Group,
                        visible,
                        locked: false,
                        lock_alpha: false,
                        clip: false,
                        expanded: true,
                        tag: 0,
                        opacity: (opacity * 100.0).clamp(0.0, 100.0),
                        blend: if attrs.get("isolation").map(|v| v == "isolate").unwrap_or(false) { op_from_name(&get("composite-op")) } else { Blend::PassThrough },
                        tiles: HashMap::new(),
                        fav: false,
                        vector: None,
                        rev: 1,
                    });
                    if is_empty {
                        continue;
                    }
                    stack.push(Some(id));
                }
            }
            "layer" => {
                let src = get("src");
                if attrs.get("seviath:paper").is_some() {
                    paper = Some((Color32::WHITE, visible));
                    pending_png.push((0, src, 0, 0));
                    continue;
                }
                let id = next;
                next += 1;
                layers.push(Layer {
                    id,
                    parent: stack.last().copied().flatten(),
                    name: get("name"),
                    kind: LayerKind::Raster,
                    visible,
                    locked: attrs.get("edit-locked").map(|v| v == "true").unwrap_or(false),
                    lock_alpha: attrs.get("alpha-preserve").map(|v| v == "true").unwrap_or(false),
                    clip: attrs.get("seviath:clip").is_some(),
                    expanded: true,
                    tag: 0,
                    opacity: (opacity * 100.0).clamp(0.0, 100.0),
                    blend: op_from_name(&get("composite-op")),
                    tiles: HashMap::new(),
                    fav: false,
                    vector: None,
                    rev: 1,
                });
                pending_png.push((id, src, get("x").parse().unwrap_or(0), get("y").parse().unwrap_or(0)));
            }
            _ => {}
        }
    }
    if w == 0 {
        return Err("This OpenRaster file has no canvas size.".into());
    }
    for (id, src, x, y) in pending_png {
        if src.is_empty() {
            continue;
        }
        let bytes = read_entry(&src, 512 << 20)?;
        let mut r = image::ImageReader::new(std::io::Cursor::new(bytes)).with_guessed_format().map_err(|e| e.to_string())?;
        let mut limits = image::Limits::default();
        limits.max_image_width = Some(MAX_SIDE);
        limits.max_image_height = Some(MAX_SIDE);
        r.limits(limits);
        let img = r.decode().map_err(|e| format!("{src} is damaged: {e}"))?.to_rgba8();
        if id == 0 {
            let p = img.get_pixel(0, 0).0;
            if let Some((c, _)) = &mut paper {
                *c = Color32::from_rgb(p[0], p[1], p[2]);
            }
            continue;
        }
        if let Some(l) = layers.iter_mut().find(|l| l.id == id) {
            l.tiles = tiles_from_rgba(&img, x, y, w, h);
        }
    }
    let (pc, pv) = paper.unwrap_or((Color32::WHITE, false));
    layers.push(Layer {
        id: next,
        parent: None,
        name: "Paper".into(),
        kind: LayerKind::Paper,
        visible: pv,
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
    });
    let mut doc = Document::from_parts(w, h, pc, layers);
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
    fn ora_round_trip() {
        let d = testdoc::sample();
        let dir = std::env::temp_dir().join(format!("seviath_ora_test_{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let p = dir.join("t.ora");
        write(&d, &p).unwrap();
        let back = read(&p).unwrap();
        assert_eq!(testdoc::shape(&back), testdoc::shape(&d));
        assert_eq!(back.paper, d.paper);
        let a = crate::files::flatten(&d);
        let b = crate::files::flatten(&back);
        let max_diff = a.pixels().zip(b.pixels()).flat_map(|(p, q)| (0..4).map(move |c| (p.0[c] as i32 - q.0[c] as i32).abs())).max().unwrap();
        assert!(max_diff <= 2, "{max_diff}");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
