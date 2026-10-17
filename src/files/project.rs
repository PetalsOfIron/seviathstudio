// SPDX-License-Identifier: GPL-3.0-or-later
// SeviathStudio project files (.seviath): a ZIP with project.json (canvas + layer tree),
// one standard PNG per painted 256×256 tile, and a thumbnail. Easy to read by other tools,
// and versionable (plan §28).

use std::collections::HashMap;
use std::io::{Read, Write};
use std::path::Path;
use std::sync::Arc;

use eframe::egui::Color32;
use rayon::prelude::*;
use serde::{Deserialize, Serialize};

use crate::doc::{Blend, Document, Layer, LayerKind, TILE, TileData, TileKey};
use crate::layerfilter::{LayerFilter, Preset};
use crate::meta::ProjectMeta;
use crate::vector::VecData;

pub const EXT: &str = "seviath";
const FORMAT: &str = "seviath";
const VERSION: u32 = 1;
const MAX_LAYERS: usize = 20_000;

#[derive(Serialize, Deserialize)]
struct ProjectJson {
    format: String,
    version: u32,
    app: String,
    width: u32,
    height: u32,
    paper: [u8; 4],
    layers: Vec<LayerJson>,
    #[serde(default)]
    filter_presets: Vec<PresetJson>,
    #[serde(default)]
    meta: ProjectMeta,
}

#[derive(Serialize, Deserialize)]
struct LayerJson {
    id: u64,
    parent: Option<u64>,
    name: String,
    kind: String,
    visible: bool,
    locked: bool,
    lock_alpha: bool,
    clip: bool,
    expanded: bool,
    tag: u8,
    #[serde(default)]
    fav: bool,
    #[serde(default)]
    vector: Option<VecData>,
    opacity: f32,
    blend: String,
    tiles: Vec<[i32; 2]>,
}

#[derive(Serialize, Deserialize)]
struct PresetJson {
    name: String,
    query: String,
    label: u8,
    chips: Vec<String>,
}

pub fn blend_name(b: Blend) -> &'static str {
    match b {
        Blend::PassThrough => "pass-through",
        Blend::Normal => "normal",
        Blend::Multiply => "multiply",
        Blend::Screen => "screen",
        Blend::Overlay => "overlay",
        Blend::Darken => "darken",
        Blend::Lighten => "lighten",
        Blend::ColorDodge => "color-dodge",
        Blend::ColorBurn => "color-burn",
        Blend::HardLight => "hard-light",
        Blend::SoftLight => "soft-light",
        Blend::Difference => "difference",
        Blend::Exclusion => "exclusion",
        Blend::Hue => "hue",
        Blend::Saturation => "saturation",
        Blend::Color => "color",
        Blend::Luminosity => "luminosity",
        Blend::Add => "add",
    }
}

pub fn blend_from_name(s: &str) -> Blend {
    std::iter::once(Blend::PassThrough).chain(Blend::ALL).find(|b| blend_name(*b) == s).unwrap_or(Blend::Normal)
}

fn kind_name(k: LayerKind) -> &'static str {
    match k {
        LayerKind::Paper => "paper",
        LayerKind::Raster => "raster",
        LayerKind::Vector => "vector",
        LayerKind::Group => "group",
    }
}

fn kind_from_name(s: &str) -> LayerKind {
    match s {
        "paper" => LayerKind::Paper,
        "vector" => LayerKind::Vector,
        "group" => LayerKind::Group,
        _ => LayerKind::Raster,
    }
}

fn tile_png(px: &[Color32]) -> Vec<u8> {
    let mut raw = Vec::with_capacity(TILE * TILE * 4);
    for c in px {
        raw.extend_from_slice(&c.to_srgba_unmultiplied());
    }
    let img = image::RgbaImage::from_raw(TILE as u32, TILE as u32, raw).expect("tile size");
    let mut out = Vec::new();
    img.write_to(&mut std::io::Cursor::new(&mut out), image::ImageFormat::Png).expect("png in memory");
    out
}

fn tile_from_png(bytes: &[u8]) -> Result<Vec<Color32>, String> {
    let mut r = image::ImageReader::new(std::io::Cursor::new(bytes)).with_guessed_format().map_err(|e| e.to_string())?;
    let mut limits = image::Limits::default();
    limits.max_image_width = Some(TILE as u32);
    limits.max_image_height = Some(TILE as u32);
    r.limits(limits);
    let img = r.decode().map_err(|e| e.to_string())?.to_rgba8();
    if img.dimensions() != (TILE as u32, TILE as u32) {
        return Err("tile has the wrong size".into());
    }
    Ok(img.pixels().map(|p| Color32::from_rgba_unmultiplied(p.0[0], p.0[1], p.0[2], p.0[3])).collect())
}

/// Save a project. Writes a temporary file first and swaps it in, so a crash never leaves a
/// half-written project behind.
pub fn save(w: u32, h: u32, paper: Color32, layers: &[Layer], presets: &[Preset], meta: &ProjectMeta, path: &Path) -> Result<(), String> {
    let json = ProjectJson {
        format: FORMAT.into(),
        version: VERSION,
        app: format!("SeviathStudio {}", crate::app::VERSION),
        width: w,
        height: h,
        paper: paper.to_srgba_unmultiplied(),
        layers: layers
            .iter()
            .map(|l| {
                let mut tiles: Vec<[i32; 2]> = l.tiles.keys().map(|k| [k.0, k.1]).collect();
                tiles.sort();
                LayerJson {
                    id: l.id,
                    parent: l.parent,
                    name: l.name.clone(),
                    kind: kind_name(l.kind).into(),
                    visible: l.visible,
                    locked: l.locked,
                    lock_alpha: l.lock_alpha,
                    clip: l.clip,
                    expanded: l.expanded,
                    tag: l.tag,
                    fav: l.fav,
                    vector: l.vector.as_deref().cloned(),
                    opacity: l.opacity,
                    blend: blend_name(l.blend).into(),
                    tiles,
                }
            })
            .collect(),
        filter_presets: presets
            .iter()
            .map(|p| PresetJson { name: p.name.clone(), query: p.filter.query.clone(), label: p.filter.label, chips: p.filter.chip_keys() })
            .collect(),
        meta: meta.clone(),
    };
    // encode every tile in parallel
    let jobs: Vec<(String, &TileData)> =
        layers.iter().flat_map(|l| l.tiles.iter().map(move |(k, t)| (format!("tiles/{}/{}_{}.png", l.id, k.0, k.1), t))).collect();
    let pngs: Vec<(String, Vec<u8>)> = jobs.into_par_iter().map(|(name, t)| (name, tile_png(t))).collect();
    let thumb = {
        let doc = Document::from_parts(w, h, paper, layers.to_vec());
        let img = super::thumbnail(&doc, 512);
        let mut out = Vec::new();
        img.write_to(&mut std::io::Cursor::new(&mut out), image::ImageFormat::Png).map_err(|e| e.to_string())?;
        out
    };

    let tmp = path.with_extension("seviath.saving");
    let result = (|| -> Result<(), String> {
        let file = std::fs::File::create(&tmp).map_err(|e| format!("Can't write {}: {e}", tmp.display()))?;
        let mut zip = zip::ZipWriter::new(std::io::BufWriter::new(file));
        let stored = zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored);
        let deflated = zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);
        let mut put = |name: &str, bytes: &[u8], opts| -> Result<(), String> {
            zip.start_file(name, opts).map_err(|e| e.to_string())?;
            zip.write_all(bytes).map_err(|e| e.to_string())
        };
        put("mimetype", b"application/x-seviath", stored)?;
        put("project.json", serde_json::to_string_pretty(&json).map_err(|e| e.to_string())?.as_bytes(), deflated)?;
        put("thumbnail.png", &thumb, stored)?;
        for (name, bytes) in &pngs {
            put(name, bytes, stored)?; // PNGs are already compressed
        }
        let w = zip.finish().map_err(|e| e.to_string())?;
        w.into_inner().map_err(|e| e.to_string())?.sync_all().map_err(|e| e.to_string())
    })();
    if let Err(e) = result {
        let _ = std::fs::remove_file(&tmp);
        return Err(e);
    }
    std::fs::rename(&tmp, path).map_err(|e| format!("Couldn't finish saving: {e}"))
}

fn read_entry(zip: &mut zip::ZipArchive<std::io::BufReader<std::fs::File>>, name: &str, max: u64) -> Result<Vec<u8>, String> {
    let mut f = zip.by_name(name).map_err(|_| format!("The project is missing {name}"))?;
    if f.size() > max {
        return Err(format!("{name} is too big"));
    }
    let mut out = Vec::with_capacity(f.size() as usize);
    f.by_ref().take(max).read_to_end(&mut out).map_err(|e| e.to_string())?;
    Ok(out)
}

fn open(path: &Path) -> Result<zip::ZipArchive<std::io::BufReader<std::fs::File>>, String> {
    let file = std::fs::File::open(path).map_err(|e| format!("Can't open {}: {e}", path.display()))?;
    zip::ZipArchive::new(std::io::BufReader::new(file)).map_err(|_| "This isn't a SeviathStudio project (not a ZIP file).".to_string())
}

pub fn load(path: &Path) -> Result<Document, String> {
    let mut zip = open(path)?;
    let json: ProjectJson =
        serde_json::from_slice(&read_entry(&mut zip, "project.json", 64 << 20)?).map_err(|e| format!("project.json is damaged: {e}"))?;
    if json.format != FORMAT {
        return Err("This isn't a SeviathStudio project.".into());
    }
    if json.version > VERSION {
        return Err("This project was made by a newer SeviathStudio — please update the app.".into());
    }
    if json.width == 0 || json.height == 0 || json.width > super::MAX_SIDE || json.height > super::MAX_SIDE {
        return Err("The canvas size in this project is not valid.".into());
    }
    if json.layers.len() > MAX_LAYERS {
        return Err("This project has too many layers.".into());
    }
    let tiles_x = (json.width as usize).div_ceil(TILE) as i32;
    let tiles_y = (json.height as usize).div_ceil(TILE) as i32;
    // read tile bytes (zip is sequential), decode in parallel
    let mut raw: Vec<(u64, TileKey, Vec<u8>)> = Vec::new();
    for l in &json.layers {
        for [tx, ty] in &l.tiles {
            if *tx < 0 || *ty < 0 || *tx >= tiles_x || *ty >= tiles_y {
                continue; // outside the canvas: ignore
            }
            let bytes = read_entry(&mut zip, &format!("tiles/{}/{}_{}.png", l.id, tx, ty), 4 << 20)?;
            raw.push((l.id, (*tx, *ty), bytes));
        }
    }
    let decoded: Vec<(u64, TileKey, Result<Vec<Color32>, String>)> = raw.into_par_iter().map(|(id, k, b)| (id, k, tile_from_png(&b))).collect();
    let mut tiles: HashMap<u64, HashMap<TileKey, TileData>> = HashMap::new();
    for (id, k, px) in decoded {
        let px = px.map_err(|e| format!("A tile of layer {id} is damaged: {e}"))?;
        if px.iter().any(|c| c.a() != 0) {
            tiles.entry(id).or_default().insert(k, Arc::new(px));
        }
    }
    let ids: std::collections::HashSet<u64> = json.layers.iter().map(|l| l.id).collect();
    let layers: Vec<Layer> = json
        .layers
        .into_iter()
        .map(|l| Layer {
            id: l.id,
            parent: l.parent.filter(|p| ids.contains(p) && *p != l.id),
            name: l.name.chars().take(200).collect(),
            kind: kind_from_name(&l.kind),
            visible: l.visible,
            locked: l.locked,
            lock_alpha: l.lock_alpha,
            clip: l.clip,
            expanded: l.expanded,
            tag: l.tag.min(7),
            fav: l.fav,
            vector: l.vector.map(|v| Arc::new(v.sanitized())),
            opacity: l.opacity.clamp(0.0, 100.0),
            blend: blend_from_name(&l.blend),
            tiles: tiles.remove(&l.id).unwrap_or_default(),
            rev: 1,
        })
        .collect();
    let meta = json.meta.clone().sanitized();
    let presets: Vec<Preset> = json
        .filter_presets
        .into_iter()
        .take(50)
        .map(|p| Preset { name: p.name.chars().take(40).collect(), filter: LayerFilter::from_parts(p.query, p.label, &p.chips) })
        .collect();
    let [r, g, b, a] = json.paper;
    let mut doc = Document::from_parts(json.width, json.height, Color32::from_rgba_unmultiplied(r, g, b, a), layers);
    doc.filter_presets = presets;
    doc.meta = meta;
    ensure_usable(&mut doc);
    Ok(doc)
}

/// What the project manager shows without opening the whole project.
#[derive(Clone)]
pub struct Info {
    pub w: u32,
    pub h: u32,
    pub meta: ProjectMeta,
}

#[derive(Deserialize)]
struct InfoJson {
    width: u32,
    height: u32,
    #[serde(default)]
    meta: ProjectMeta,
}

/// Size, DPI, notes and tags of a project file (reads only `project.json`).
pub fn read_info(path: &Path) -> Option<Info> {
    let f = std::fs::File::open(path).ok()?;
    let mut zip = zip::ZipArchive::new(std::io::BufReader::new(f)).ok()?;
    let mut e = zip.by_name("project.json").ok()?;
    if e.size() > 64 << 20 {
        return None;
    }
    let mut buf = Vec::new();
    e.by_ref().take(64 << 20).read_to_end(&mut buf).ok()?;
    let j: InfoJson = serde_json::from_slice(&buf).ok()?;
    Some(Info { w: j.width, h: j.height, meta: j.meta.sanitized() })
}

/// Every document needs at least one pixel layer to paint on.
pub fn ensure_usable(doc: &mut Document) {
    if !doc.layers.iter().any(|l| l.kind == LayerKind::Raster) {
        let l = doc.new_layer("Layer 1", LayerKind::Raster);
        doc.layers.insert(0, l);
    }
}

/// The project's thumbnail (for the project manager), without loading the whole project.
pub fn read_thumbnail(path: &Path) -> Option<image::RgbaImage> {
    let mut zip = open(path).ok()?;
    let bytes = read_entry(&mut zip, "thumbnail.png", 8 << 20).ok()?;
    let mut r = image::ImageReader::new(std::io::Cursor::new(bytes)).with_guessed_format().ok()?;
    let mut limits = image::Limits::default();
    limits.max_image_width = Some(1024);
    limits.max_image_height = Some(1024);
    r.limits(limits);
    Some(r.decode().ok()?.to_rgba8())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::files::testdoc;

    #[test]
    fn save_and_load_round_trip() {
        let mut d = testdoc::sample();
        d.meta = ProjectMeta { dpi: 300, notes: "for the 2 PM stream".into(), tags: vec!["hair".into(), "wip".into()], guides: crate::meta::GuidePreset::Thirds.build(d.w, d.h), autosave_minutes: Some(1.5), recovery_copies: None };
        d.layers[1].fav = true;
        d.layers[0].vector = Some(std::sync::Arc::new(VecData {
            strokes: vec![crate::vector::VecStroke { pts: vec![[10.0, 10.0, 0.5], [50.0, 40.0, 1.0], [90.0, 12.5, 0.25]], style: Default::default() }],
            width: 1.5,
            fills: vec![crate::vector::VecFill { rings: vec![vec![[1.0, 1.0], [30.5, 2.0], [20.0, 25.0]]], color: [9, 80, 200] }],
        }));
        let mut f = LayerFilter { query: "hair".into(), label: 2, chips: Vec::new() };
        f.cycle(crate::layerfilter::ChipKind::Locked);
        f.cycle(crate::layerfilter::ChipKind::Locked);
        let presets = vec![Preset { name: "Hair, unlocked".into(), filter: f }];
        let dir = std::env::temp_dir().join(format!("seviath_proj_test_{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("test.seviath");
        save(d.w, d.h, d.paper, &d.layers, &presets, &d.meta, &path).unwrap();
        let back = load(&path).unwrap();
        assert_eq!(back.filter_presets, presets, "filter presets are part of the project");
        assert_eq!(back.meta, d.meta, "print size, notes, tags, guides and autosave override are saved");
        assert!(back.layers[1].fav && !back.layers[0].fav, "favorites are saved");
        assert_eq!(back.layers[0].vector, d.layers[0].vector, "vector lines are saved, so they stay editable");
        assert!(back.layers[1].vector.is_none());
        assert_eq!((back.w, back.h, back.paper), (d.w, d.h, d.paper));
        assert_eq!(testdoc::shape(&back), testdoc::shape(&d));
        // pixels identical after a save/load cycle (premultiplied → PNG → premultiplied is exact)
        for (a, b) in d.layers.iter().zip(&back.layers) {
            assert_eq!(a.tiles.len(), b.tiles.len(), "{}", a.name);
            for (k, t) in &a.tiles {
                assert!(t.iter().zip(b.tiles[k].iter()).all(|(x, y)| x == y), "{} tile {k:?}", a.name);
            }
        }
        assert!(read_thumbnail(&path).is_some());
        let info = read_info(&path).unwrap();
        assert_eq!((info.w, info.h, info.meta.dpi, info.meta.tags.clone()), (d.w, d.h, 300, vec!["hair".to_string(), "wip".to_string()]));
        // saving again over it works and leaves no temporary file
        save(back.w, back.h, back.paper, &back.layers, &back.filter_presets, &back.meta, &path).unwrap();
        assert!(!path.with_extension("seviath.saving").exists());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn refuses_other_files() {
        let dir = std::env::temp_dir().join(format!("seviath_proj_bad_{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let p = dir.join("not.seviath");
        std::fs::write(&p, b"hello").unwrap();
        assert!(load(&p).is_err());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
