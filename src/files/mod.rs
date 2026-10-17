// SPDX-License-Identifier: GPL-3.0-or-later
// Files: SeviathStudio projects (.seviath), PSD, OpenRaster (.ora) and flat pictures.

pub mod kra;
pub mod ora;
pub mod project;
pub mod psd;
pub mod template;

use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;

use eframe::egui::Color32;
use rayon::prelude::*;

use crate::blend;
use crate::doc::{Document, Layer, LayerKind, TILE, TileData, TileKey};

/// Biggest canvas side we open (bigger files are refused instead of eating all memory).
pub const MAX_SIDE: u32 = 30_000;

/// Unpremultiplied RGBA picture → tiles (empty tiles are left out). `ox, oy` = where the
/// picture's top-left lands on the canvas.
pub fn tiles_from_rgba(img: &image::RgbaImage, ox: i32, oy: i32, doc_w: u32, doc_h: u32) -> HashMap<TileKey, TileData> {
    let t = TILE as i32;
    let (w, h) = (img.width() as i32, img.height() as i32);
    let x0 = ox.max(0);
    let y0 = oy.max(0);
    let x1 = (ox + w).min(doc_w as i32);
    let y1 = (oy + h).min(doc_h as i32);
    let mut out = HashMap::new();
    if x1 <= x0 || y1 <= y0 {
        return out;
    }
    let keys: Vec<TileKey> = (y0.div_euclid(t)..=(y1 - 1).div_euclid(t))
        .flat_map(|ty| (x0.div_euclid(t)..=(x1 - 1).div_euclid(t)).map(move |tx| (tx, ty)))
        .collect();
    let made: Vec<(TileKey, Vec<Color32>)> = keys
        .into_par_iter()
        .filter_map(|(tx, ty)| {
            let mut px = vec![Color32::TRANSPARENT; TILE * TILE];
            let mut any = false;
            for j in 0..t {
                let y = ty * t + j;
                if y < y0 || y >= y1 {
                    continue;
                }
                for i in 0..t {
                    let x = tx * t + i;
                    if x < x0 || x >= x1 {
                        continue;
                    }
                    let p = img.get_pixel((x - ox) as u32, (y - oy) as u32).0;
                    if p[3] != 0 {
                        px[(j * t + i) as usize] = Color32::from_rgba_unmultiplied(p[0], p[1], p[2], p[3]);
                        any = true;
                    }
                }
            }
            any.then_some(((tx, ty), px))
        })
        .collect();
    for (k, px) in made {
        out.insert(k, Arc::new(px));
    }
    out
}

/// A layer's own pixels as one unpremultiplied picture covering `(x, y, w, h)` of the canvas.
pub fn layer_rgba(layer: &Layer, x: i32, y: i32, w: u32, h: u32) -> image::RgbaImage {
    let mut img = image::RgbaImage::new(w, h);
    let t = TILE as i32;
    for (key, tile) in &layer.tiles {
        let (bx, by) = (key.0 * t, key.1 * t);
        for j in 0..t {
            let py = by + j - y;
            if py < 0 || py >= h as i32 {
                continue;
            }
            for i in 0..t {
                let px = bx + i - x;
                if px < 0 || px >= w as i32 {
                    continue;
                }
                let c = tile[(j * t + i) as usize];
                if c.a() != 0 {
                    img.put_pixel(px as u32, py as u32, image::Rgba(c.to_srgba_unmultiplied()));
                }
            }
        }
    }
    img
}

/// The smallest rectangle holding all of a layer's paint, or None when it is empty.
pub fn layer_bounds(layer: &Layer, doc_w: u32, doc_h: u32) -> Option<(i32, i32, i32, i32)> {
    let t = TILE as i32;
    let (mut x0, mut y0, mut x1, mut y1) = (i32::MAX, i32::MAX, i32::MIN, i32::MIN);
    for (key, tile) in &layer.tiles {
        for j in 0..t {
            for i in 0..t {
                if tile[(j * t + i) as usize].a() != 0 {
                    let (x, y) = (key.0 * t + i, key.1 * t + j);
                    x0 = x0.min(x);
                    y0 = y0.min(y);
                    x1 = x1.max(x + 1);
                    y1 = y1.max(y + 1);
                }
            }
        }
    }
    let (x1, y1) = (x1.min(doc_w as i32), y1.min(doc_h as i32));
    (x0 < x1 && y0 < y1).then_some((x0, y0, x1, y1))
}

/// The whole picture flattened (paper included when it is shown), unpremultiplied.
pub fn flatten(doc: &Document) -> image::RgbaImage {
    let keys: Vec<TileKey> = (0..doc.tiles_y()).flat_map(|ty| (0..doc.tiles_x()).map(move |tx| (tx, ty))).collect();
    let tiles: Vec<(TileKey, Vec<blend::Px>)> = keys.into_par_iter().map(|k| (k, doc.flatten_tile(k))).collect();
    let mut img = image::RgbaImage::new(doc.w, doc.h);
    for ((tx, ty), px) in tiles {
        for j in 0..TILE {
            let y = ty as usize * TILE + j;
            if y >= doc.h as usize {
                break;
            }
            for i in 0..TILE {
                let x = tx as usize * TILE + i;
                if x >= doc.w as usize {
                    break;
                }
                let c = blend::from_px(px[j * TILE + i]);
                img.put_pixel(x as u32, y as u32, image::Rgba(c.to_srgba_unmultiplied()));
            }
        }
    }
    img
}

/// A copy for formats that know nothing of our effects and adjustment layers (PSD, OpenRaster):
/// effects are drawn into the pixels; each adjustment is applied to the layers below it in its folder
/// and then dropped. (The flat picture always looks exactly like the canvas; layered files are close.)
pub fn bake(mut doc: Document) -> Document {
    use std::collections::HashSet;
    let (tx, ty) = (doc.tiles_x(), doc.tiles_y());
    let with_fx: Vec<usize> = (0..doc.layers.len()).filter(|&i| doc.layers[i].fx_active()).collect();
    for i in with_fx {
        let mut keys: HashSet<TileKey> = HashSet::new();
        for k in doc.layers[i].tiles.keys() {
            for dy in -1..=1 {
                for dx in -1..=1 {
                    let q = (k.0 + dx, k.1 + dy);
                    if q.0 >= 0 && q.1 >= 0 && q.0 < tx && q.1 < ty {
                        keys.insert(q);
                    }
                }
            }
        }
        let baked: Vec<(TileKey, Vec<Color32>)> = keys
            .into_par_iter()
            .filter_map(|k| doc.fx_pixels(&doc.layers[i], k).map(|px| (k, px.into_iter().map(blend::from_px).collect())))
            .collect();
        let l = &mut doc.layers[i];
        l.tiles.clear();
        for (k, px) in baked {
            if px.iter().any(|c| c.a() != 0) {
                l.tiles.insert(k, Arc::new(px));
            }
        }
        l.fx.clear();
        l.rev += 1;
    }
    let len = doc.layers.len();
    let adjusts: Vec<usize> = (0..len).rev().filter(|&i| doc.layers[i].kind == LayerKind::Adjust).collect();
    for ia in adjusts {
        let (visible, op, parent, adj) = {
            let l = &doc.layers[ia];
            (l.visible, l.opacity / 100.0, l.parent, l.adjust.clone())
        };
        let Some(adj) = adj.filter(|_| visible && op > 0.0) else { continue };
        for j in ia + 1..len {
            if let Some(p) = parent {
                if !doc.is_inside(doc.layers[j].id, p) {
                    continue;
                }
            }
            match doc.layers[j].kind {
                LayerKind::Raster | LayerKind::Vector => {
                    for t in doc.layers[j].tiles.values_mut() {
                        let mut px: Vec<blend::Px> = t.iter().map(|c| blend::to_px(*c)).collect();
                        adj.apply(&mut px, TILE, op);
                        *t = Arc::new(px.into_iter().map(blend::from_px).collect());
                    }
                }
                LayerKind::Paper if parent.is_none() => {
                    let mut px = [blend::to_px(doc.paper)];
                    adj.apply(&mut px, 1, op);
                    doc.paper = blend::from_px(px[0]);
                }
                _ => {}
            }
        }
    }
    doc.layers.retain(|l| l.kind != LayerKind::Adjust);
    doc
}

/// A small preview (longest side `max`), for project thumbnails.
pub fn thumbnail(doc: &Document, max: u32) -> image::RgbaImage {
    let full = flatten(doc);
    let k = max as f32 / doc.w.max(doc.h) as f32;
    if k >= 1.0 {
        return full;
    }
    let (w, h) = (((doc.w as f32 * k).round() as u32).max(1), ((doc.h as f32 * k).round() as u32).max(1));
    image::imageops::thumbnail(&full, w, h)
}

/// Picture formats for "Export".
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Flat {
    Png,
    Jpeg,
    Webp,
}

/// The app credit written into exported PNGs (as the *software* used — the art stays yours).
pub const CREDIT: &str = "Made with SeviathStudio — created by Seviath, https://x.com/SnakeSeviath";

/// `credit`: put "Software: SeviathStudio" and the app credit into the PNG's text fields.
pub fn export_flat(doc: &Document, path: &Path, kind: Flat, jpeg_quality: u8, credit: bool) -> Result<(), String> {
    let img = flatten(doc);
    let file = std::fs::File::create(path).map_err(|e| e.to_string())?;
    let mut w = std::io::BufWriter::new(file);
    match kind {
        Flat::Png => {
            let mut enc = png::Encoder::new(&mut w, img.width(), img.height());
            enc.set_color(png::ColorType::Rgba);
            enc.set_depth(png::BitDepth::Eight);
            enc.set_compression(png::Compression::Balanced);
            // pixels per metre, so other programs know the print size
            let ppm = (doc.meta.dpi as f64 / 0.0254).round() as u32;
            enc.set_pixel_dims(Some(png::PixelDimensions { xppu: ppm, yppu: ppm, unit: png::Unit::Meter }));
            if credit {
                let soft = format!("SeviathStudio {}", env!("CARGO_PKG_VERSION"));
                enc.add_text_chunk("Software".into(), soft).map_err(|e| e.to_string())?;
                enc.add_itxt_chunk("Comment".into(), CREDIT.into()).map_err(|e| e.to_string())?;
            }
            let mut wr = enc.write_header().map_err(|e| e.to_string())?;
            wr.write_image_data(img.as_raw()).map_err(|e| e.to_string())
        }
        Flat::Webp => img.write_to(&mut w, image::ImageFormat::WebP).map_err(|e| e.to_string()),
        Flat::Jpeg => {
            // JPEG has no transparency: put it on the paper color (white when the paper is hidden)
            let bg = if doc.paper_visible() { doc.paper } else { Color32::WHITE };
            let rgb = image::RgbImage::from_fn(img.width(), img.height(), |x, y| {
                let p = img.get_pixel(x, y).0;
                let a = p[3] as f32 / 255.0;
                let mix = |c: u8, b: u8| (c as f32 * a + b as f32 * (1.0 - a)).round() as u8;
                image::Rgb([mix(p[0], bg.r()), mix(p[1], bg.g()), mix(p[2], bg.b())])
            });
            let mut enc = image::codecs::jpeg::JpegEncoder::new_with_quality(&mut w, jpeg_quality.clamp(1, 100));
            enc.set_pixel_density(image::codecs::jpeg::PixelDensity::dpi(doc.meta.dpi.clamp(1, 65535) as u16));
            rgb.write_with_encoder(enc).map_err(|e| e.to_string())
        }
    }
}

/// Decode a picture with size limits (also used to start a project from a picture).
pub fn read_picture(path: &Path) -> Result<image::RgbaImage, String> {
    let mut reader = image::ImageReader::open(path).map_err(|e| e.to_string())?.with_guessed_format().map_err(|e| e.to_string())?;
    let mut limits = image::Limits::default();
    limits.max_image_width = Some(MAX_SIDE);
    limits.max_image_height = Some(MAX_SIDE);
    limits.max_alloc = Some(2048 << 20);
    reader.limits(limits);
    Ok(reader.decode().map_err(|e| format!("Can't read this picture: {e}"))?.to_rgba8())
}

/// Decode a picture held in memory, with the same size limits.
pub fn decode_limited(bytes: Vec<u8>) -> Result<image::RgbaImage, String> {
    let mut r = image::ImageReader::new(std::io::Cursor::new(bytes)).with_guessed_format().map_err(|e| e.to_string())?;
    let mut limits = image::Limits::default();
    limits.max_image_width = Some(MAX_SIDE);
    limits.max_image_height = Some(MAX_SIDE);
    limits.max_alloc = Some(2048 << 20);
    r.limits(limits);
    Ok(r.decode().map_err(|e| format!("Can't read this picture: {e}"))?.to_rgba8())
}

/// A new project made from a picture: the picture becomes Layer 1 on a hidden paper.
pub fn document_from_picture(img: &image::RgbaImage, name: &str) -> Document {
    let mut doc = Document::new(img.width(), img.height());
    doc.layers[0].name = name.to_owned();
    doc.layers[0].tiles = tiles_from_rgba(img, 0, 0, img.width(), img.height());
    doc.layers[0].rev += 1;
    if img.pixels().any(|p| p.0[3] < 255) {
        // keep transparent pictures transparent
        if let Some(p) = doc.layers.iter_mut().find(|l| l.kind == LayerKind::Paper) {
            p.visible = false;
        }
    }
    doc.mark_all_dirty();
    doc
}

#[cfg(test)]
pub(crate) mod testdoc {
    use super::*;
    use crate::doc::{Blend, LayerKind};

    /// A small document with a folder, a clipped Multiply layer, a locked layer and a hidden one.
    pub fn sample() -> Document {
        let mut d = Document::new(300, 200);
        d.paper = Color32::from_rgb(250, 245, 230);
        let red = image::RgbaImage::from_fn(120, 80, |x, _| image::Rgba([220, 40, 40, if x < 100 { 255 } else { 128 }]));
        d.layers[0].name = "Lines".into();
        d.layers[0].tiles = tiles_from_rgba(&red, 30, 40, 300, 200);
        let mut shade = d.new_layer("Shadow", LayerKind::Raster);
        shade.blend = Blend::Multiply;
        shade.clip = true;
        shade.opacity = 60.0;
        let blue = image::RgbaImage::from_pixel(60, 60, image::Rgba([30, 60, 200, 255]));
        shade.tiles = tiles_from_rgba(&blue, 200, 100, 300, 200);
        let top = d.layers[0].id;
        let sid = d.insert_layer(shade, top);
        let f = d.group(sid, "Hair").unwrap();
        let fi = d.index_of(f).unwrap();
        d.layers[fi].blend = Blend::Normal;
        d.layers[fi].opacity = 80.0;
        let mut hidden = d.new_layer("Hidden sketch", LayerKind::Raster);
        hidden.visible = false;
        hidden.locked = true;
        hidden.tiles = tiles_from_rgba(&image::RgbaImage::from_pixel(10, 10, image::Rgba([0, 0, 0, 255])), 0, 0, 300, 200);
        let top = d.layers[0].id;
        d.insert_layer(hidden, top);
        d
    }

    /// Layer structure as plain values, for comparing round trips.
    pub fn shape(d: &Document) -> Vec<(String, LayerKind, bool, bool, bool, u8, Blend, usize)> {
        d.layers
            .iter()
            .map(|l| (l.name.clone(), l.kind, l.visible, l.clip, l.locked, l.opacity.round() as u8, l.blend, d.depth(d.index_of(l.id).unwrap())))
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn picture_to_tiles_and_back() {
        let img = image::RgbaImage::from_fn(300, 270, |x, y| image::Rgba([(x % 256) as u8, (y % 256) as u8, 7, if (x + y) % 3 == 0 { 0 } else { 200 }]));
        let mut d = Document::new(400, 400);
        d.layers[0].tiles = tiles_from_rgba(&img, 50, 60, 400, 400);
        assert_eq!(layer_bounds(&d.layers[0], 400, 400).map(|b| (b.0, b.1)), Some((50, 60)));
        let back = layer_rgba(&d.layers[0], 50, 60, 300, 270);
        for (a, b) in img.pixels().zip(back.pixels()) {
            if a.0[3] == 0 {
                assert_eq!(b.0[3], 0);
            } else {
                for c in 0..4 {
                    assert!((a.0[c] as i32 - b.0[c] as i32).abs() <= 1, "{a:?} vs {b:?}");
                }
            }
        }
    }

    #[test]
    fn png_export_carries_the_credit() {
        let d = testdoc::sample();
        let p = std::env::temp_dir().join(format!("seviath_credit_{}.png", std::process::id()));
        export_flat(&d, &p, Flat::Png, 90, true).unwrap();
        let bytes = std::fs::read(&p).unwrap();
        let text = String::from_utf8_lossy(&bytes);
        assert!(text.contains("SeviathStudio") && text.contains("x.com/SnakeSeviath"));
        assert_eq!(image::open(&p).unwrap().to_rgba8(), flatten(&d), "pixels unchanged");
        export_flat(&d, &p, Flat::Png, 90, false).unwrap();
        assert!(!String::from_utf8_lossy(&std::fs::read(&p).unwrap()).contains("SnakeSeviath"));
        let _ = std::fs::remove_file(&p);
    }

    #[test]
    fn exports_carry_the_print_size() {
        let mut d = testdoc::sample();
        d.meta.dpi = 300;
        let dir = std::env::temp_dir().join(format!("seviath_dpi_{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let png_path = dir.join("a.png");
        export_flat(&d, &png_path, Flat::Png, 90, false).unwrap();
        let dec = png::Decoder::new(std::io::BufReader::new(std::fs::File::open(&png_path).unwrap()));
        let reader = dec.read_info().unwrap();
        let dims = reader.info().pixel_dims.expect("pHYs chunk");
        assert_eq!((dims.xppu, dims.unit), (11811, png::Unit::Meter)); // 300 dpi
        let jpg_path = dir.join("a.jpg");
        export_flat(&d, &jpg_path, Flat::Jpeg, 90, false).unwrap();
        let bytes = std::fs::read(&jpg_path).unwrap();
        // JFIF header: units = 1 (dots per inch), then x and y density 300
        let at = bytes.windows(5).position(|w| w == b"JFIF\0").expect("JFIF");
        assert_eq!(&bytes[at + 7..at + 12], &[1, 0x01, 0x2c, 0x01, 0x2c]);
        let psd_bytes = super::psd::write(&d);
        assert_eq!(super::psd::read(&psd_bytes).unwrap().meta.dpi, 300);
        let ora_path = dir.join("a.ora");
        super::ora::write(&d, &ora_path).unwrap();
        assert_eq!(super::ora::read(&ora_path).unwrap().meta.dpi, 300);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn flatten_includes_paper() {
        let d = testdoc::sample();
        let img = flatten(&d);
        assert_eq!(img.get_pixel(5, 190).0, [250, 245, 230, 255]); // paper only
        assert_eq!(img.dimensions(), (300, 200));
    }
}
