// SPDX-License-Identifier: GPL-3.0-or-later
// Reference images: kept as normalized PNG copies in your app-data folder
// (Windows: %APPDATA%\SeviathStudio\references) so they come back next time.
// Each reference remembers its own zoom, position, rotation, mirror and grayscale.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use eframe::egui::{self, Color32, ColorImage, TextureHandle, TextureOptions, Vec2, pos2, vec2};
use serde::{Deserialize, Serialize};

/// Biggest side kept for a reference (bigger pictures are scaled down).
const MAX_SIDE: u32 = 2048;

pub struct Reference {
    pub id: u64,
    pub name: String,
    pub img: Arc<ColorImage>,
    tex: Option<TextureHandle>,
    tex_gray: Option<TextureHandle>,
    pub zoom: f32,
    pub pan: Vec2,
    pub rot: u8, // quarter turns clockwise
    pub flip: bool,
    pub gray: bool,
}

#[derive(Serialize, Deserialize)]
struct Entry {
    id: u64,
    name: String,
    zoom: f32,
    pan: [f32; 2],
    rot: u8,
    flip: bool,
    gray: bool,
}

#[derive(Serialize, Deserialize, Default)]
struct Index {
    entries: Vec<Entry>,
    current: Option<u64>,
    grid: bool,
    overlay: bool,
    overlay_opacity: f32,
}

pub struct RefStore {
    pub refs: Vec<Reference>,
    pub current: Option<u64>,
    pub grid: bool,
    pub overlay: bool,
    pub overlay_opacity: f32,
    dir: Option<PathBuf>,
    next_id: u64,
}

/// Decode an image safely: size and memory limits so a broken or hostile file can't eat all memory.
fn decode(reader: image::ImageReader<impl std::io::BufRead + std::io::Seek>) -> Result<image::RgbaImage, String> {
    let mut reader = reader.with_guessed_format().map_err(|e| e.to_string())?;
    let mut limits = image::Limits::default();
    limits.max_image_width = Some(20_000);
    limits.max_image_height = Some(20_000);
    limits.max_alloc = Some(768 << 20);
    reader.limits(limits);
    let img = reader.decode().map_err(|e| format!("Can't read this picture: {e}"))?.to_rgba8();
    let (w, h) = img.dimensions();
    if w.max(h) > MAX_SIDE {
        let k = MAX_SIDE as f32 / w.max(h) as f32;
        let (nw, nh) = (((w as f32 * k).round() as u32).max(1), ((h as f32 * k).round() as u32).max(1));
        return Ok(image::imageops::resize(&img, nw, nh, image::imageops::FilterType::Triangle));
    }
    Ok(img)
}

fn to_color_image(img: &image::RgbaImage) -> ColorImage {
    ColorImage::from_rgba_unmultiplied([img.width() as usize, img.height() as usize], img.as_raw())
}

impl RefStore {
    /// References kept in memory only (tests, or when there is no app-data folder).
    pub fn in_memory() -> Self {
        Self { refs: Vec::new(), current: None, grid: false, overlay: false, overlay_opacity: 0.35, dir: None, next_id: 1 }
    }

    /// The saved references of this user.
    pub fn load_saved() -> Self {
        match dirs::config_dir() {
            Some(d) => Self::load_from(d.join("SeviathStudio").join("references")),
            None => Self::in_memory(),
        }
    }

    pub fn load_from(dir: PathBuf) -> Self {
        let mut s = Self::in_memory();
        let index: Index = std::fs::read_to_string(dir.join("index.json")).ok().and_then(|t| serde_json::from_str(&t).ok()).unwrap_or_default();
        for e in index.entries {
            let file = dir.join(format!("{}.png", e.id));
            let Ok(reader) = image::ImageReader::open(&file) else { continue };
            let Ok(img) = decode(reader) else { continue };
            s.next_id = s.next_id.max(e.id + 1);
            s.refs.push(Reference {
                id: e.id,
                name: e.name,
                img: Arc::new(to_color_image(&img)),
                tex: None,
                tex_gray: None,
                zoom: e.zoom.clamp(0.05, 40.0),
                pan: vec2(e.pan[0], e.pan[1]),
                rot: e.rot % 4,
                flip: e.flip,
                gray: e.gray,
            });
        }
        s.current = index.current.filter(|c| s.refs.iter().any(|r| r.id == *c)).or(s.refs.last().map(|r| r.id));
        s.grid = index.grid;
        s.overlay = index.overlay;
        s.overlay_opacity = if index.overlay_opacity > 0.0 { index.overlay_opacity } else { 0.35 };
        s.dir = Some(dir);
        s
    }

    pub fn save(&self) {
        let Some(dir) = &self.dir else { return };
        let index = Index {
            entries: self
                .refs
                .iter()
                .map(|r| Entry { id: r.id, name: r.name.clone(), zoom: r.zoom, pan: [r.pan.x, r.pan.y], rot: r.rot, flip: r.flip, gray: r.gray })
                .collect(),
            current: self.current,
            grid: self.grid,
            overlay: self.overlay,
            overlay_opacity: self.overlay_opacity,
        };
        if std::fs::create_dir_all(dir).is_ok() {
            if let Ok(json) = serde_json::to_string_pretty(&index) {
                let tmp = dir.join("index.json.tmp");
                if std::fs::write(&tmp, json).is_ok() {
                    let _ = std::fs::rename(&tmp, dir.join("index.json"));
                }
            }
        }
    }

    fn add_image(&mut self, name: String, img: image::RgbaImage) -> Result<u64, String> {
        let id = self.next_id;
        self.next_id += 1;
        if let Some(dir) = &self.dir {
            std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
            img.save(dir.join(format!("{id}.png"))).map_err(|e| format!("Couldn't keep a copy: {e}"))?;
        }
        self.refs.push(Reference {
            id,
            name,
            img: Arc::new(to_color_image(&img)),
            tex: None,
            tex_gray: None,
            zoom: 1.0,
            pan: Vec2::ZERO,
            rot: 0,
            flip: false,
            gray: false,
        });
        self.current = Some(id);
        self.save();
        Ok(id)
    }

    pub fn add_path(&mut self, path: &Path) -> Result<u64, String> {
        let reader = image::ImageReader::open(path).map_err(|e| e.to_string())?;
        let img = decode(reader)?;
        let name = path.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_else(|| "Reference".into());
        self.add_image(name, img)
    }

    #[cfg_attr(not(test), allow(dead_code))] // pasted pictures (clipboard) will use this too
    pub fn add_bytes(&mut self, name: &str, bytes: &[u8]) -> Result<u64, String> {
        let img = decode(image::ImageReader::new(std::io::Cursor::new(bytes)))?;
        self.add_image(name.to_owned(), img)
    }

    pub fn remove(&mut self, id: u64) {
        self.refs.retain(|r| r.id != id);
        if let Some(dir) = &self.dir {
            let _ = std::fs::remove_file(dir.join(format!("{id}.png")));
        }
        if self.current == Some(id) {
            self.current = self.refs.last().map(|r| r.id);
        }
        self.save();
    }

    pub fn current_mut(&mut self) -> Option<&mut Reference> {
        let id = self.current?;
        self.refs.iter_mut().find(|r| r.id == id)
    }

    pub fn current(&self) -> Option<&Reference> {
        let id = self.current?;
        self.refs.iter().find(|r| r.id == id)
    }
}

impl Reference {
    /// The texture to show (grayscale version when Gray is on), made on first use.
    pub fn texture(&mut self, ctx: &egui::Context) -> egui::TextureId {
        if self.gray {
            if self.tex_gray.is_none() {
                let mut g = (*self.img).clone();
                for p in g.pixels.iter_mut() {
                    let [r, gg, b, a] = p.to_srgba_unmultiplied();
                    let l = (0.299 * r as f32 + 0.587 * gg as f32 + 0.114 * b as f32).round() as u8;
                    *p = Color32::from_rgba_unmultiplied(l, l, l, a);
                }
                self.tex_gray = Some(ctx.load_texture(format!("ref{}g", self.id), g, TextureOptions::LINEAR));
            }
            return self.tex_gray.as_ref().unwrap().id();
        }
        if self.tex.is_none() {
            self.tex = Some(ctx.load_texture(format!("ref{}", self.id), (*self.img).clone(), TextureOptions::LINEAR));
        }
        self.tex.as_ref().unwrap().id()
    }

    /// Image size as shown (turned a quarter: width and height swap).
    pub fn shown_size(&self) -> Vec2 {
        let [w, h] = self.img.size;
        if self.rot % 2 == 1 { vec2(h as f32, w as f32) } else { vec2(w as f32, h as f32) }
    }

    /// Texture coordinates for the shown corners: top-left, top-right, bottom-right, bottom-left.
    pub fn corner_uvs(&self) -> [eframe::egui::Pos2; 4] {
        let base = [pos2(0.0, 0.0), pos2(1.0, 0.0), pos2(1.0, 1.0), pos2(0.0, 1.0)];
        let k = (self.rot % 4) as usize;
        let mut uv = [0, 1, 2, 3].map(|i| base[(i + 4 - k) % 4]);
        if self.flip {
            uv.swap(0, 1);
            uv.swap(2, 3);
        }
        uv
    }

    /// Color at a point given as 0..1 across the shown picture.
    pub fn color_at(&self, sx: f32, sy: f32) -> Option<Color32> {
        if !(0.0..=1.0).contains(&sx) || !(0.0..=1.0).contains(&sy) {
            return None;
        }
        let [tl, tr, _, bl] = self.corner_uvs();
        let u = tl.x + (tr.x - tl.x) * sx + (bl.x - tl.x) * sy;
        let v = tl.y + (tr.y - tl.y) * sx + (bl.y - tl.y) * sy;
        let [w, h] = self.img.size;
        let (x, y) = (((u * w as f32) as usize).min(w - 1), ((v * h as f32) as usize).min(h - 1));
        let c = self.img.pixels[y * w + x];
        if c.a() == 0 {
            return None;
        }
        let [r, g, b, _] = c.to_srgba_unmultiplied();
        Some(if self.gray {
            let l = (0.299 * r as f32 + 0.587 * g as f32 + 0.114 * b as f32).round() as u8;
            Color32::from_rgb(l, l, l)
        } else {
            Color32::from_rgb(r, g, b)
        })
    }

    /// The main colors of the picture (k-means), darkest first.
    pub fn palette(&self, k: usize) -> Vec<Color32> {
        let px = &self.img.pixels;
        let step = (px.len() / 4096).max(1);
        let pts: Vec<[f32; 3]> = px
            .iter()
            .step_by(step)
            .filter(|c| c.a() > 128)
            .map(|c| {
                let [r, g, b, _] = c.to_srgba_unmultiplied();
                [r as f32, g as f32, b as f32]
            })
            .collect();
        if pts.is_empty() {
            return Vec::new();
        }
        let k = k.min(pts.len()).max(1);
        // start from evenly spread samples sorted by brightness
        let mut sorted = pts.clone();
        sorted.sort_by(|a, b| (a[0] + a[1] + a[2]).total_cmp(&(b[0] + b[1] + b[2])));
        let mut centers: Vec<[f32; 3]> = (0..k).map(|i| sorted[(i * (sorted.len() - 1)) / (k.max(2) - 1).max(1)]).collect();
        for _ in 0..10 {
            let mut sum = vec![[0.0f32; 4]; k];
            for p in &pts {
                let (best, _) = centers
                    .iter()
                    .enumerate()
                    .map(|(i, c)| (i, (p[0] - c[0]).powi(2) + (p[1] - c[1]).powi(2) + (p[2] - c[2]).powi(2)))
                    .min_by(|a, b| a.1.total_cmp(&b.1))
                    .unwrap();
                sum[best] = [sum[best][0] + p[0], sum[best][1] + p[1], sum[best][2] + p[2], sum[best][3] + 1.0];
            }
            for (c, s) in centers.iter_mut().zip(&sum) {
                if s[3] > 0.0 {
                    *c = [s[0] / s[3], s[1] / s[3], s[2] / s[3]];
                }
            }
        }
        centers.sort_by(|a, b| (a[0] + a[1] + a[2]).total_cmp(&(b[0] + b[1] + b[2])));
        let mut out: Vec<Color32> = Vec::new();
        for c in centers {
            let col = Color32::from_rgb(c[0].round() as u8, c[1].round() as u8, c[2].round() as u8);
            // skip near-duplicates
            if !out.iter().any(|o| (o.r() as i32 - col.r() as i32).abs() + (o.g() as i32 - col.g() as i32).abs() + (o.b() as i32 - col.b() as i32).abs() < 24) {
                out.push(col);
            }
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A 4×2 test picture: left half red, right half blue, top-left pixel green.
    fn picture() -> image::RgbaImage {
        let mut img = image::RgbaImage::from_fn(4, 2, |x, _| if x < 2 { image::Rgba([255, 0, 0, 255]) } else { image::Rgba([0, 0, 255, 255]) });
        img.put_pixel(0, 0, image::Rgba([0, 255, 0, 255]));
        img
    }

    fn png_bytes(img: &image::RgbaImage) -> Vec<u8> {
        let mut out = Vec::new();
        img.write_to(&mut std::io::Cursor::new(&mut out), image::ImageFormat::Png).unwrap();
        out
    }

    #[test]
    fn colors_follow_rotation_and_mirror() {
        let mut s = RefStore::in_memory();
        let id = s.add_bytes("test", &png_bytes(&picture())).unwrap();
        let r = s.refs.iter_mut().find(|r| r.id == id).unwrap();
        assert_eq!(r.color_at(0.05, 0.1), Some(Color32::from_rgb(0, 255, 0)));
        assert_eq!(r.color_at(0.9, 0.5), Some(Color32::from_rgb(0, 0, 255)));
        r.flip = true; // the green corner moves to the top right
        assert_eq!(r.color_at(0.95, 0.1), Some(Color32::from_rgb(0, 255, 0)));
        r.flip = false;
        r.rot = 1; // turned clockwise: the image's top-left ends up top-right
        assert_eq!(r.shown_size(), vec2(2.0, 4.0));
        assert_eq!(r.color_at(0.9, 0.05), Some(Color32::from_rgb(0, 255, 0)));
        r.gray = true;
        let g = r.color_at(0.1, 0.9).unwrap();
        assert!(g.r() == g.g() && g.g() == g.b());
    }

    #[test]
    fn palette_finds_the_main_colors() {
        let mut s = RefStore::in_memory();
        let big = image::RgbaImage::from_fn(64, 64, |x, _| if x < 32 { image::Rgba([200, 30, 30, 255]) } else { image::Rgba([30, 30, 200, 255]) });
        let id = s.add_bytes("two colors", &png_bytes(&big)).unwrap();
        let p = s.refs.iter().find(|r| r.id == id).unwrap().palette(6);
        assert_eq!(p.len(), 2, "{p:?}");
    }

    #[test]
    fn saved_and_loaded_again() {
        let dir = std::env::temp_dir().join(format!("seviath_refs_test_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        {
            let mut s = RefStore::load_from(dir.clone());
            let id = s.add_bytes("kept", &png_bytes(&picture())).unwrap();
            s.current_mut().unwrap().zoom = 2.5;
            s.grid = true;
            s.save();
            assert!(dir.join(format!("{id}.png")).exists());
        }
        let s = RefStore::load_from(dir.clone());
        assert_eq!(s.refs.len(), 1);
        assert_eq!(s.refs[0].name, "kept");
        assert_eq!(s.refs[0].zoom, 2.5);
        assert!(s.grid);
        let mut s = s;
        let id = s.refs[0].id;
        s.remove(id);
        assert!(!dir.join(format!("{id}.png")).exists());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn rejects_garbage() {
        let mut s = RefStore::in_memory();
        assert!(s.add_bytes("junk", b"definitely not a picture").is_err());
        assert!(s.refs.is_empty());
    }
}
