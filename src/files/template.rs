// SPDX-License-Identifier: GPL-3.0-or-later
// New-project setup: the built-in templates, your own templates (saved as small JSON files you
// can share), and building the empty document from the choices made in the dialog.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use eframe::egui::Color32;
use rayon::prelude::*;
use serde::{Deserialize, Serialize};

use crate::doc::{Document, Layer, LayerKind, TILE, TileKey};
use crate::meta::{GuidePreset, ProjectMeta, Unit, parse_tags};

pub struct BuiltIn {
    pub name: &'static str,
    pub w: u32,
    pub h: u32,
    pub dpi: u32,
    pub guides: GuidePreset,
    /// Start with the tagged folder structure for rigging.
    pub rig: bool,
}

const fn t(name: &'static str, w: u32, h: u32, dpi: u32, guides: GuidePreset, rig: bool) -> BuiltIn {
    BuiltIn { name, w, h, dpi, guides, rig }
}

pub const BUILTIN: &[BuiltIn] = &[
    t("VTuber — bust", 3000, 4000, 300, GuidePreset::Center, true),
    t("VTuber — half body", 4000, 6000, 300, GuidePreset::Center, true),
    t("VTuber — full body", 5000, 8000, 300, GuidePreset::Center, true),
    t("Chibi / PNGTuber", 2500, 2500, 300, GuidePreset::Center, true),
    t("Emote (draw big, export 112 px)", 1120, 1120, 72, GuidePreset::Safe, false),
    t("Twitch panel", 640, 320, 72, GuidePreset::Safe, false),
    t("Stream screen / overlay", 1920, 1080, 72, GuidePreset::Thirds, false),
    t("YouTube thumbnail", 1280, 720, 72, GuidePreset::Safe, false),
    t("Phone wallpaper", 1080, 1920, 72, GuidePreset::Center, false),
    t("A4 print, 300 DPI", 2480, 3508, 300, GuidePreset::Safe, false),
    t("Comic page B5, 600 DPI", 4157, 5906, 600, GuidePreset::Safe, false),
];

pub const DPI_CHOICES: [u32; 5] = [72, 96, 150, 300, 600];

#[derive(Clone, PartialEq, Debug, Serialize, Deserialize)]
pub enum Background {
    Paper([u8; 3]),
    Transparent,
    /// A top-to-bottom two-color start on its own layer above the paper.
    Gradient { top: [u8; 3], bottom: [u8; 3] },
}

impl Background {
    pub fn color32(c: [u8; 3]) -> Color32 {
        Color32::from_rgb(c[0], c[1], c[2])
    }
}

#[derive(Clone)]
pub struct NewProject {
    pub name: String,
    pub w: u32,
    pub h: u32,
    pub dpi: u32,
    /// The unit width and height are typed in.
    pub unit: Unit,
    pub bg: Background,
    pub guides: GuidePreset,
    pub rig_starter: bool,
    /// Color profile id (see colorprofiles.rs).
    pub rgb_profile: String,
    pub notes: String,
    /// Typed as "hair, commission".
    pub tags: String,
}

impl Default for NewProject {
    fn default() -> Self {
        let b = &BUILTIN[0];
        Self {
            name: "Untitled".into(),
            w: b.w,
            h: b.h,
            dpi: b.dpi,
            unit: Unit::Px,
            bg: Background::Paper([255, 255, 255]),
            guides: b.guides,
            rig_starter: b.rig,
            rgb_profile: "srgb".into(),
            notes: String::new(),
            tags: String::new(),
        }
    }
}

impl NewProject {
    pub fn use_builtin(&mut self, b: &BuiltIn) {
        self.w = b.w;
        self.h = b.h;
        self.dpi = b.dpi;
        self.guides = b.guides;
        self.rig_starter = b.rig;
    }

    pub fn to_template(&self, name: &str) -> Template {
        Template {
            name: name.trim().chars().take(40).collect(),
            w: self.w,
            h: self.h,
            dpi: self.dpi,
            bg: self.bg.clone(),
            guides: self.guides.key().into(),
            rig_starter: self.rig_starter,
            tags: self.tags.clone(),
        }
    }

    pub fn use_template(&mut self, t: &Template) {
        self.w = t.w.clamp(16, super::MAX_SIDE);
        self.h = t.h.clamp(16, super::MAX_SIDE);
        self.dpi = t.dpi.clamp(1, 2400);
        self.bg = t.bg.clone();
        self.guides = GuidePreset::from_key(&t.guides);
        self.rig_starter = t.rig_starter;
        self.tags = t.tags.clone();
    }

    /// Width / height in the chosen unit, and back.
    pub fn size_in_unit(&self) -> (f32, f32) {
        (self.unit.from_px(self.w as f32, self.dpi), self.unit.from_px(self.h as f32, self.dpi))
    }
}

/// A saved set of choices (not the name of the project).
#[derive(Clone, PartialEq, Debug, Serialize, Deserialize)]
pub struct Template {
    pub name: String,
    pub w: u32,
    pub h: u32,
    pub dpi: u32,
    pub bg: Background,
    pub guides: String,
    pub rig_starter: bool,
    pub tags: String,
}

pub fn templates_dir() -> PathBuf {
    dirs::config_dir().unwrap_or_else(std::env::temp_dir).join("SeviathStudio").join("templates")
}

fn file_name(name: &str) -> String {
    let clean: String = name.chars().map(|c| if c.is_alphanumeric() || " -_.,".contains(c) { c } else { '_' }).collect();
    format!("{}.json", clean.trim())
}

pub fn list_in(dir: &Path) -> Vec<Template> {
    let Ok(rd) = std::fs::read_dir(dir) else { return Vec::new() };
    let mut v: Vec<Template> = rd
        .flatten()
        .filter(|e| e.path().extension().is_some_and(|x| x == "json"))
        .filter_map(|e| std::fs::metadata(e.path()).ok().filter(|m| m.len() < 64 * 1024).and_then(|_| std::fs::read_to_string(e.path()).ok()))
        .filter_map(|s| serde_json::from_str::<Template>(&s).ok())
        .filter(|t| !t.name.trim().is_empty())
        .collect();
    v.sort_by_key(|t| t.name.to_lowercase());
    v
}

pub fn save_in(dir: &Path, t: &Template) -> Result<(), String> {
    if t.name.trim().is_empty() {
        return Err("Give the template a name first.".into());
    }
    std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    let json = serde_json::to_string_pretty(t).map_err(|e| e.to_string())?;
    std::fs::write(dir.join(file_name(&t.name)), json).map_err(|e| e.to_string())
}

pub fn delete_in(dir: &Path, name: &str) {
    let _ = std::fs::remove_file(dir.join(file_name(name)));
}

pub fn read_file(path: &Path) -> Result<Template, String> {
    let len = std::fs::metadata(path).map_err(|e| e.to_string())?.len();
    if len > 64 * 1024 {
        return Err("That file is too big to be a template.".into());
    }
    let text = std::fs::read_to_string(path).map_err(|e| e.to_string())?;
    let mut t: Template = serde_json::from_str(&text).map_err(|_| "That isn't a SeviathStudio template.".to_string())?;
    t.name = t.name.trim().chars().take(40).collect();
    t.w = t.w.clamp(16, super::MAX_SIDE);
    t.h = t.h.clamp(16, super::MAX_SIDE);
    t.dpi = t.dpi.clamp(1, 2400);
    if t.name.is_empty() {
        return Err("That template has no name.".into());
    }
    Ok(t)
}

/// "A full layer takes 46 MB" and a warning for very large canvases.
pub fn memory_text(w: u32, h: u32) -> (String, bool) {
    let px = w as f64 * h as f64;
    let mb = px * 4.0 / (1u64 << 20) as f64;
    let mut s = format!("A fully painted layer takes about {mb:.0} MB; empty areas take nothing. Ten full layers: {:.1} GB.", mb * 10.0 / 1024.0);
    let big = px > 100_000_000.0;
    if big {
        s += " This is very large: expect slower exports and filters.";
    }
    (s, big)
}

// ---------- Building the document ----------

/// (name, tag color, layers inside)
const RIG_STARTER: &[(&str, u8, &[&str])] = &[
    ("Hair front", 3, &["Fringe", "Side hair"]),
    ("Brows", 1, &["Brow L", "Brow R"]),
    ("Eyes", 6, &["Eye L", "Eye R"]),
    ("Nose and mouth", 4, &["Nose", "Mouth"]),
    ("Face", 2, &["Face base", "Ears"]),
    ("Body", 5, &["Body", "Clothes"]),
    ("Hair back", 3, &["Hair back"]),
];

fn gradient_tiles(w: u32, h: u32, top: [u8; 3], bottom: [u8; 3]) -> HashMap<TileKey, Arc<Vec<Color32>>> {
    let (tw, th) = (w.div_ceil(TILE as u32) as i32, h.div_ceil(TILE as u32) as i32);
    let keys: Vec<TileKey> = (0..th).flat_map(|ty| (0..tw).map(move |tx| (tx, ty))).collect();
    keys.into_par_iter()
        .map(|(tx, ty)| {
            let mut px = vec![Color32::TRANSPARENT; TILE * TILE];
            for j in 0..TILE {
                let y = ty as usize * TILE + j;
                if y >= h as usize {
                    break;
                }
                let k = y as f32 / (h.max(2) - 1) as f32;
                let mix = |a: u8, b: u8| (a as f32 + (b as f32 - a as f32) * k).round() as u8;
                let c = Color32::from_rgb(mix(top[0], bottom[0]), mix(top[1], bottom[1]), mix(top[2], bottom[2]));
                for i in 0..TILE {
                    if (tx as usize * TILE + i) < w as usize {
                        px[j * TILE + i] = c;
                    }
                }
            }
            ((tx, ty), Arc::new(px))
        })
        .collect()
}

pub fn build(np: &NewProject) -> Document {
    let (w, h) = (np.w.clamp(16, super::MAX_SIDE), np.h.clamp(16, super::MAX_SIDE));
    let mut doc = Document::new(w, h);
    let mut paper = doc.layers.pop().expect("a new document has its paper last");
    debug_assert_eq!(paper.kind, LayerKind::Paper);
    let mut layers: Vec<Layer> = Vec::new();
    if np.rig_starter {
        doc.layers.clear();
        for (folder, tag, kids) in RIG_STARTER {
            let mut g = doc.new_layer(folder, LayerKind::Group);
            g.tag = *tag;
            g.blend = crate::doc::Blend::PassThrough;
            let gid = g.id;
            layers.push(g);
            for k in *kids {
                let mut l = doc.new_layer(k, LayerKind::Raster);
                l.parent = Some(gid);
                l.tag = *tag;
                layers.push(l);
            }
        }
    } else {
        layers.append(&mut doc.layers);
    }
    match &np.bg {
        Background::Paper(c) => doc.paper = Background::color32(*c),
        Background::Transparent => paper.visible = false,
        Background::Gradient { top, bottom } => {
            // the paper stays under it, white; the gradient is a normal layer you can edit or delete
            let mut g = doc.new_layer("Background gradient", LayerKind::Raster);
            g.tiles = gradient_tiles(w, h, *top, *bottom);
            g.rev += 1;
            layers.push(g);
        }
    }
    layers.push(paper);
    doc.layers = layers;
    doc.meta = ProjectMeta { dpi: np.dpi, notes: np.notes.clone(), tags: parse_tags(&np.tags), guides: np.guides.build(w, h), rgb_profile: np.rgb_profile.clone(), ..Default::default() }.sanitized();
    doc.mark_all_dirty();
    doc
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rig_starter_makes_tagged_folders() {
        let np = NewProject { rig_starter: true, tags: "vtuber, WIP".into(), ..Default::default() };
        let d = build(&np);
        let names: Vec<&str> = d.layers.iter().map(|l| l.name.as_str()).collect();
        assert_eq!(names.first(), Some(&"Hair front"));
        assert_eq!(names.last(), Some(&"Paper"));
        let eyes = d.layers.iter().find(|l| l.name == "Eyes").unwrap();
        assert_eq!((eyes.kind, eyes.tag), (LayerKind::Group, 6));
        let eye_l = d.layers.iter().find(|l| l.name == "Eye L").unwrap();
        assert_eq!(eye_l.parent, Some(eyes.id));
        assert_eq!(d.meta.tags, ["vtuber", "WIP"]);
        assert_eq!(d.meta.dpi, 300);
        assert!(!d.meta.guides.is_empty());
        let ids: std::collections::HashSet<_> = d.layers.iter().map(|l| l.id).collect();
        assert_eq!(ids.len(), d.layers.len(), "every layer id is unique");
    }

    #[test]
    fn plain_project_keeps_the_default_layer() {
        let np = NewProject { rig_starter: false, bg: Background::Transparent, guides: GuidePreset::Off, ..Default::default() };
        let d = build(&np);
        assert_eq!(d.layers.iter().map(|l| l.name.as_str()).collect::<Vec<_>>(), ["Layer 1", "Paper"]);
        assert!(!d.layers[1].visible, "transparent background hides the paper");
        assert!(d.meta.guides.is_empty());
    }

    #[test]
    fn paper_color_and_gradient() {
        let d = build(&NewProject { bg: Background::Paper([10, 20, 30]), rig_starter: false, ..Default::default() });
        assert_eq!(d.paper, Color32::from_rgb(10, 20, 30));
        assert!(d.layers.last().unwrap().visible);
        let g = build(&NewProject { w: 300, h: 600, bg: Background::Gradient { top: [255, 0, 0], bottom: [0, 0, 255] }, rig_starter: false, ..Default::default() });
        let layer = g.layers.iter().find(|l| l.name == "Background gradient").unwrap();
        let img = crate::files::layer_rgba(layer, 0, 0, 300, 600);
        assert_eq!(img.get_pixel(5, 0).0, [255, 0, 0, 255]);
        assert_eq!(img.get_pixel(299, 599).0, [0, 0, 255, 255]);
        let mid = img.get_pixel(100, 300).0;
        assert!(mid[0] > 100 && mid[2] > 100, "{mid:?}");
    }

    #[test]
    fn personal_templates_save_list_delete() {
        let dir = std::env::temp_dir().join(format!("seviath_tpl_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let np = NewProject { w: 1234, h: 4321, dpi: 150, bg: Background::Gradient { top: [1, 2, 3], bottom: [4, 5, 6] }, guides: GuidePreset::Golden, tags: "a,b".into(), ..Default::default() };
        let t = np.to_template("My / wide: banner");
        save_in(&dir, &t).unwrap();
        let listed = list_in(&dir);
        assert_eq!(listed, [t.clone()]);
        let mut again = NewProject::default();
        again.use_template(&listed[0]);
        assert_eq!((again.w, again.h, again.dpi, again.guides, again.bg), (1234, 4321, 150, GuidePreset::Golden, np.bg.clone()));
        // a template file shared from somewhere else is read safely
        let shared = dir.join("shared.json");
        std::fs::write(&shared, r#"{"name":"  Big  ","w":9999999,"h":5,"dpi":0,"bg":"Transparent","guides":"nonsense","rig_starter":true,"tags":""}"#).unwrap();
        let r = read_file(&shared).unwrap();
        assert_eq!((r.name.as_str(), r.w, r.h, r.dpi), ("Big", super::super::MAX_SIDE, 16, 1));
        std::fs::write(&shared, "not json").unwrap();
        assert!(read_file(&shared).is_err());
        assert!(save_in(&dir, &NewProject::default().to_template("  ")).is_err());
        delete_in(&dir, &t.name);
        assert!(!list_in(&dir).iter().any(|x| x.name == t.name));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn memory_hint() {
        let (s, big) = memory_text(3000, 4000);
        assert!(s.contains("46 MB") && !big);
        assert!(memory_text(15000, 15000).1);
    }
}
