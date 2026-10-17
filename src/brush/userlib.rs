// SPDX-License-Identifier: GPL-3.0-or-later
// Your own brushes: saved as small JSON files (`.seviathbrush`) in your SeviathStudio folder, shared
// as "brush cards" (a PNG picture of the brush with the brush inside it), imported from those files
// and from plain pictures (which become stamp brushes). Every brush carries its author, license and
// use terms, so they travel with it.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use super::tipimage::TipImage;
use super::{BrushSettings, Tip};

pub const FORMAT: &str = "seviathbrush";
pub const EXT: &str = "seviathbrush";
const MAX_FILE: u64 = 12 << 20;
const MAX_BRUSHES: usize = 200;
/// The PNG text field that holds the brush inside a card.
pub const CARD_KEY: &str = "SeviathBrush";

#[derive(Clone, PartialEq, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct UserBrush {
    pub name: String,
    pub about: String,
    pub author: String,
    pub license: String,
    pub terms: String,
    pub settings: BrushSettings,
}

impl Default for UserBrush {
    fn default() -> Self {
        UserBrush { name: "My brush".into(), about: String::new(), author: String::new(), license: String::new(), terms: String::new(), settings: BrushSettings::default() }
    }
}

#[derive(Serialize, Deserialize)]
struct BrushFile {
    format: String,
    version: u32,
    brushes: Vec<UserBrush>,
}

impl UserBrush {
    /// Everything from a file is held to sane limits.
    pub fn sanitized(mut self) -> UserBrush {
        let cut = |s: &str, n: usize| s.chars().take(n).collect::<String>();
        self.name = cut(self.name.trim(), 60);
        if self.name.is_empty() {
            self.name = "Imported brush".into();
        }
        self.about = cut(&self.about, 400);
        self.author = cut(&self.author, 80);
        self.license = cut(&self.license, 80);
        self.terms = cut(&self.terms, 1000);
        self.settings = self.settings.sanitized();
        self
    }
}

/// A file-name safe version of a name.
pub fn slug(name: &str) -> String {
    let s: String = name.trim().chars().map(|c| if c.is_alphanumeric() { c.to_ascii_lowercase() } else { '-' }).collect();
    let s = s.split('-').filter(|p| !p.is_empty()).collect::<Vec<_>>().join("-");
    if s.is_empty() { "brush".into() } else { s.chars().take(60).collect() }
}

/// Where your brushes live.
pub fn dir() -> PathBuf {
    if cfg!(test) {
        return std::env::temp_dir().join("SeviathStudio-tests").join("brushes");
    }
    dirs::config_dir().unwrap_or_else(std::env::temp_dir).join("SeviathStudio").join("brushes")
}

pub fn to_json(brushes: &[UserBrush]) -> String {
    let f = BrushFile { format: FORMAT.into(), version: 1, brushes: brushes.to_vec() };
    serde_json::to_string_pretty(&f).unwrap_or_default()
}

/// Brushes from the text of a brush file (one brush or several).
pub fn from_json(text: &str) -> Result<Vec<UserBrush>, String> {
    let v: serde_json::Value = serde_json::from_str(text).map_err(|e| format!("This isn't a brush file ({e})."))?;
    let list: Vec<UserBrush> = if v.get("brushes").is_some() {
        let f: BrushFile = serde_json::from_value(v).map_err(|e| format!("This brush file is damaged ({e})."))?;
        f.brushes
    } else if v.get("settings").is_some() {
        vec![serde_json::from_value(v).map_err(|e| format!("This brush file is damaged ({e})."))?]
    } else {
        return Err("This file has no brushes in it.".into());
    };
    Ok(list.into_iter().take(MAX_BRUSHES).map(UserBrush::sanitized).collect())
}

pub fn save_in(dir: &Path, b: &UserBrush) -> Result<PathBuf, String> {
    std::fs::create_dir_all(dir).map_err(|e| format!("Can't make the brush folder: {e}"))?;
    let path = dir.join(format!("{}.{EXT}", slug(&b.name)));
    std::fs::write(&path, to_json(std::slice::from_ref(b))).map_err(|e| format!("Can't save the brush: {e}"))?;
    Ok(path)
}

pub fn delete_in(dir: &Path, name: &str) {
    let _ = std::fs::remove_file(dir.join(format!("{}.{EXT}", slug(name))));
}

/// All the saved brushes, in name order. Damaged files are skipped.
pub fn load_all_in(dir: &Path) -> Vec<UserBrush> {
    let mut out = Vec::new();
    let Ok(rd) = std::fs::read_dir(dir) else { return out };
    for e in rd.flatten() {
        let p = e.path();
        if p.extension().is_some_and(|x| x == EXT) && e.metadata().map(|m| m.len() <= MAX_FILE).unwrap_or(false) {
            if let Ok(text) = std::fs::read_to_string(&p) {
                if let Ok(mut v) = from_json(&text) {
                    out.append(&mut v);
                }
            }
        }
    }
    out.sort_by_key(|b| b.name.to_lowercase());
    out.dedup_by(|a, b| a.name == b.name);
    out
}

/// A brush card: a picture of the brush, with the brush itself stored inside the PNG.
pub fn card_png(b: &UserBrush, preview: &image::RgbaImage) -> Result<Vec<u8>, String> {
    let mut out = Vec::new();
    {
        let mut enc = png::Encoder::new(&mut out, preview.width(), preview.height());
        enc.set_color(png::ColorType::Rgba);
        enc.set_depth(png::BitDepth::Eight);
        enc.add_itxt_chunk(CARD_KEY.into(), to_json(std::slice::from_ref(b))).map_err(|e| e.to_string())?;
        enc.add_itxt_chunk("Title".into(), b.name.clone()).map_err(|e| e.to_string())?;
        if !b.author.is_empty() {
            enc.add_itxt_chunk("Author".into(), b.author.clone()).map_err(|e| e.to_string())?;
        }
        let mut w = enc.write_header().map_err(|e| e.to_string())?;
        w.write_image_data(preview.as_raw()).map_err(|e| e.to_string())?;
    }
    Ok(out)
}

/// The brushes inside a card (None when the PNG is just a picture).
pub fn from_card(bytes: &[u8]) -> Option<Vec<UserBrush>> {
    let dec = png::Decoder::new(std::io::Cursor::new(bytes));
    let reader = dec.read_info().ok()?;
    let info = reader.info();
    let text = info.utf8_text.iter().find(|t| t.keyword == CARD_KEY).and_then(|t| t.get_text().ok()).or_else(|| info.uncompressed_latin1_text.iter().find(|t| t.keyword == CARD_KEY).map(|t| t.text.clone()))?;
    from_json(&text).ok()
}

/// A plain picture as a stamp brush.
pub fn from_picture(name: &str, img: &image::RgbaImage) -> Option<UserBrush> {
    let tip = TipImage::from_picture(img)?;
    let settings = BrushSettings { tip: Tip::Image, tip_image: Some(tip), size: 60.0, spacing: 0.25, hardness: 1.0, follow_direction: false, ..BrushSettings::default() };
    Some(UserBrush { name: name.chars().take(60).collect(), about: "A brush made from a picture.".into(), settings, ..UserBrush::default() }.sanitized())
}

/// Brushes from a file: brush files, brush cards, or pictures.
pub fn import_file(path: &Path) -> Result<Vec<UserBrush>, String> {
    let len = std::fs::metadata(path).map_err(|e| format!("Can't open {}: {e}", path.display()))?.len();
    if len > MAX_FILE {
        return Err("That file is too big to be a brush.".into());
    }
    let ext = path.extension().map(|e| e.to_string_lossy().to_lowercase()).unwrap_or_default();
    let stem = path.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_else(|| "Imported brush".into());
    if ext == EXT || ext == "json" {
        let text = std::fs::read_to_string(path).map_err(|e| e.to_string())?;
        return from_json(&text);
    }
    let bytes = std::fs::read(path).map_err(|e| e.to_string())?;
    if ext == "png" {
        if let Some(v) = from_card(&bytes) {
            return Ok(v);
        }
    }
    if ["png", "jpg", "jpeg", "webp", "bmp", "gif"].contains(&ext.as_str()) {
        let img = crate::files::decode_limited(bytes).map_err(|e| e.to_string())?;
        return from_picture(&stem, &img).map(|b| vec![b]).ok_or_else(|| "That picture is empty, so there is no brush in it.".to_string());
    }
    Err("I can open brush files (.seviathbrush), brush cards (PNG) and pictures.".into())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("seviath_ub_{}_{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        d
    }

    fn sample() -> UserBrush {
        UserBrush {
            name: "Bark & Moss".into(),
            about: "Rough bark".into(),
            author: "Seviath".into(),
            license: "CC BY 4.0".into(),
            terms: "Free for any use, keep the credit.".into(),
            settings: BrushSettings { size: 33.0, lines: 4, fray: super::super::Fray::Knotted, ..BrushSettings::default() },
        }
    }

    #[test]
    fn brushes_are_saved_listed_and_deleted_as_files() {
        let d = tmp("lib");
        let b = sample();
        let p = save_in(&d, &b).unwrap();
        assert!(p.ends_with("bark-moss.seviathbrush"), "{p:?}");
        save_in(&d, &UserBrush { name: "Another".into(), ..UserBrush::default() }).unwrap();
        std::fs::write(d.join("broken.seviathbrush"), "{ nope").unwrap();
        let all = load_all_in(&d);
        assert_eq!(all.iter().map(|b| b.name.as_str()).collect::<Vec<_>>(), vec!["Another", "Bark & Moss"], "damaged files are skipped, the rest sorted");
        assert_eq!(all[1], b, "everything comes back: settings, author, license, terms");
        delete_in(&d, "Another");
        assert_eq!(load_all_in(&d).len(), 1);
        std::fs::remove_dir_all(&d).ok();
        assert!(load_all_in(&d).is_empty(), "no folder, no brushes, no crash");
    }

    #[test]
    fn a_brush_card_is_a_picture_that_carries_the_brush() {
        let mut b = sample();
        let tip = image::RgbaImage::from_fn(60, 60, |x, y| if (x as f32 - 30.0).hypot(y as f32 - 30.0) < 25.0 { image::Rgba([0, 0, 0, 255]) } else { image::Rgba([255, 255, 255, 255]) });
        b.settings.tip = Tip::Image;
        b.settings.tip_image = TipImage::from_picture(&tip);
        let preview = image::RgbaImage::from_pixel(80, 30, image::Rgba([240, 230, 220, 255]));
        let png = card_png(&b, &preview).unwrap();
        assert_eq!(image::load_from_memory(&png).unwrap().width(), 80, "it is a normal PNG picture");
        let back = from_card(&png).expect("the brush is inside");
        assert_eq!(back, vec![b], "the whole brush, picture tip included");
        assert!(from_card(&{
            let mut plain = Vec::new();
            preview.write_to(&mut std::io::Cursor::new(&mut plain), image::ImageFormat::Png).unwrap();
            plain
        })
        .is_none(), "a plain PNG is not a card");
    }

    #[test]
    fn files_of_any_kind_import_or_say_why_not() {
        let d = tmp("imp");
        std::fs::create_dir_all(&d).unwrap();
        // a brush file and a card
        let b = sample();
        let p1 = save_in(&d, &b).unwrap();
        assert_eq!(import_file(&p1).unwrap(), vec![b.clone()]);
        let card = d.join("card.png");
        std::fs::write(&card, card_png(&b, &image::RgbaImage::from_pixel(40, 20, image::Rgba([1, 2, 3, 255]))).unwrap()).unwrap();
        assert_eq!(import_file(&card).unwrap(), vec![b]);
        // a picture becomes a stamp brush named after the file
        let pic = d.join("Maple leaf.png");
        let img = image::RgbaImage::from_fn(50, 50, |x, y| if x > 10 && x < 40 && y > 5 && y < 45 { image::Rgba([20, 90, 30, 255]) } else { image::Rgba([0, 0, 0, 0]) });
        img.save(&pic).unwrap();
        let v = import_file(&pic).unwrap();
        assert_eq!((v[0].name.as_str(), v[0].settings.tip), ("Maple leaf", Tip::Image));
        assert!(v[0].settings.tip_image.is_some());
        // things that are not brushes
        std::fs::write(d.join("x.txt"), "hello").unwrap();
        assert!(import_file(&d.join("x.txt")).unwrap_err().contains("brush files"));
        let empty = d.join("empty.png");
        image::RgbaImage::from_pixel(10, 10, image::Rgba([255, 255, 255, 255])).save(&empty).unwrap();
        assert!(import_file(&empty).unwrap_err().contains("empty"));
        std::fs::write(d.join("bad.json"), "{\"nothing\": 1}").unwrap();
        assert!(import_file(&d.join("bad.json")).is_err());
        assert!(import_file(&d.join("missing.png")).is_err());
        std::fs::remove_dir_all(&d).ok();
    }

    #[test]
    fn imported_values_are_held_in_range_and_names_become_file_names() {
        let wild = UserBrush { name: "  ".into(), settings: BrushSettings { size: 1e9, opacity: -4.0, strands: 9999, lines: 500, dot_count: 99999, spacing: f32::NAN, ..BrushSettings::default() }, ..UserBrush::default() }.sanitized();
        assert_eq!(wild.name, "Imported brush");
        let s = &wild.settings;
        assert!(s.size <= 2000.0 && (0.0..=1.0).contains(&s.opacity) && s.strands <= 40 && s.lines <= 12 && s.dot_count <= 16 && s.spacing.is_finite(), "{s:?}");
        assert_eq!(slug("Bark & Moss!"), "bark-moss");
        assert_eq!(slug("???"), "brush");
        assert!(slug(&"a".repeat(200)).len() <= 60);
        // a file with several brushes, and with a single bare brush
        let two = to_json(&[sample(), UserBrush::default()]);
        assert_eq!(from_json(&two).unwrap().len(), 2);
        let bare = serde_json::json!({"name": "Bare", "settings": {"size": 12.0}}).to_string();
        assert_eq!(from_json(&bare).unwrap()[0].settings.size, 12.0);
        assert!(from_json("[1,2]").is_err());
    }
}
