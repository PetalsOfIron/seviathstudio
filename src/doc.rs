// The document: size, paper and a stack of layers. Pixels live in 256×256 tiles that are only
// created where something is drawn, so big canvases and many layers stay light.
// Pixels are premultiplied RGBA (egui's Color32), the same as the GPU textures.

use std::collections::{HashMap, HashSet};

use eframe::egui::Color32;

pub const TILE: usize = 256;
pub type TileKey = (i32, i32);

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Blend {
    Normal,
    Multiply,
    Screen,
    Overlay,
    Darken,
    Lighten,
    ColorDodge,
    ColorBurn,
    HardLight,
    SoftLight,
    Difference,
    Exclusion,
    Hue,
    Saturation,
    Color,
    Luminosity,
    Add,
}

impl Blend {
    pub const ALL: [Blend; 17] = [
        Blend::Normal,
        Blend::Multiply,
        Blend::Screen,
        Blend::Overlay,
        Blend::Darken,
        Blend::Lighten,
        Blend::ColorDodge,
        Blend::ColorBurn,
        Blend::HardLight,
        Blend::SoftLight,
        Blend::Difference,
        Blend::Exclusion,
        Blend::Hue,
        Blend::Saturation,
        Blend::Color,
        Blend::Luminosity,
        Blend::Add,
    ];

    pub fn name(self) -> &'static str {
        match self {
            Blend::Normal => "Normal",
            Blend::Multiply => "Multiply",
            Blend::Screen => "Screen",
            Blend::Overlay => "Overlay",
            Blend::Darken => "Darken",
            Blend::Lighten => "Lighten",
            Blend::ColorDodge => "Color dodge",
            Blend::ColorBurn => "Color burn",
            Blend::HardLight => "Hard light",
            Blend::SoftLight => "Soft light",
            Blend::Difference => "Difference",
            Blend::Exclusion => "Exclusion",
            Blend::Hue => "Hue",
            Blend::Saturation => "Saturation",
            Blend::Color => "Color",
            Blend::Luminosity => "Luminosity",
            Blend::Add => "Add (glow)",
        }
    }

    pub fn about(self) -> &'static str {
        match self {
            Blend::Normal => "Covers what is below.",
            Blend::Multiply => "Darkens: good for shadows.",
            Blend::Screen => "Lightens: good for light and glow.",
            Blend::Overlay => "Adds contrast: darks darker, lights lighter.",
            Blend::Darken => "Keeps whichever is darker.",
            Blend::Lighten => "Keeps whichever is lighter.",
            Blend::ColorDodge => "Strong bright light, burns out highlights.",
            Blend::ColorBurn => "Strong deep darkening with more saturation.",
            Blend::HardLight => "Like Overlay, but driven by this layer.",
            Blend::SoftLight => "Gentle Overlay: soft light and shade.",
            Blend::Difference => "Subtracts colors; for checking alignment.",
            Blend::Exclusion => "Softer Difference.",
            Blend::Hue => "Takes only the hue of this layer.",
            Blend::Saturation => "Takes only the saturation of this layer.",
            Blend::Color => "Hue and saturation: recolors, keeps shading.",
            Blend::Luminosity => "Takes only the brightness of this layer.",
            Blend::Add => "Adds light: the brightest glow.",
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum LayerKind {
    Paper,
    Raster,
    Vector,
    Group,
}

#[derive(Clone, Debug)]
pub struct Layer {
    pub name: String,
    pub kind: LayerKind,
    pub visible: bool,
    pub locked: bool,
    pub lock_alpha: bool,
    pub clip: bool,
    pub opacity: f32, // 0..100
    pub blend: Blend,
    pub tiles: HashMap<TileKey, Vec<Color32>>,
}

impl Layer {
    pub fn new(name: &str, kind: LayerKind) -> Self {
        Self {
            name: name.into(),
            kind,
            visible: true,
            locked: false,
            lock_alpha: false,
            clip: false,
            opacity: 100.0,
            blend: Blend::Normal,
            tiles: HashMap::new(),
        }
    }

    pub fn paintable(&self) -> bool {
        self.kind == LayerKind::Raster && !self.locked
    }

    /// Tile for writing, created empty on first use.
    pub fn tile_mut(&mut self, key: TileKey) -> &mut Vec<Color32> {
        self.tiles.entry(key).or_insert_with(|| vec![Color32::TRANSPARENT; TILE * TILE])
    }
}

pub struct Document {
    pub w: u32,
    pub h: u32,
    pub paper: Color32,
    pub layers: Vec<Layer>, // top first, like the layer panel
    dirty: HashSet<TileKey>,
    pub revision: u64,
}

impl Document {
    pub fn new(w: u32, h: u32) -> Self {
        let mut d = Self {
            w,
            h,
            paper: Color32::WHITE,
            layers: vec![Layer::new("Layer 1", LayerKind::Raster), Layer::new("Paper", LayerKind::Paper)],
            dirty: HashSet::new(),
            revision: 0,
        };
        d.mark_all_dirty();
        d
    }

    pub fn tiles_x(&self) -> i32 {
        (self.w as usize).div_ceil(TILE) as i32
    }
    pub fn tiles_y(&self) -> i32 {
        (self.h as usize).div_ceil(TILE) as i32
    }

    pub fn mark_all_dirty(&mut self) {
        for ty in 0..self.tiles_y() {
            for tx in 0..self.tiles_x() {
                self.dirty.insert((tx, ty));
            }
        }
        self.revision += 1;
    }

    /// Marks the tiles touched by a rectangle in document pixels (inclusive-exclusive).
    pub fn mark_dirty_rect(&mut self, x0: i32, y0: i32, x1: i32, y1: i32) {
        let t = TILE as i32;
        let (x0, y0) = (x0.max(0), y0.max(0));
        let (x1, y1) = (x1.min(self.w as i32), y1.min(self.h as i32));
        if x1 <= x0 || y1 <= y0 {
            return;
        }
        for ty in y0.div_euclid(t)..=(y1 - 1).div_euclid(t) {
            for tx in x0.div_euclid(t)..=(x1 - 1).div_euclid(t) {
                self.dirty.insert((tx, ty));
            }
        }
        self.revision += 1;
    }

    pub fn take_dirty(&mut self) -> Vec<TileKey> {
        self.dirty.drain().collect()
    }

    pub fn paper_visible(&self) -> bool {
        self.layers.iter().any(|l| l.kind == LayerKind::Paper && l.visible)
    }

    /// Composites one tile for display: paper (or a checkerboard when the paper is hidden),
    /// then the visible layers from bottom to top. Blend modes and clipping arrive with the layer step;
    /// until then every layer blends as Normal.
    pub fn composite_tile(&self, key: TileKey, out: &mut [Color32]) {
        let (ox, oy) = (key.0 as usize * TILE, key.1 as usize * TILE);
        let paper = self.paper_visible();
        for y in 0..TILE {
            for x in 0..TILE {
                let i = y * TILE + x;
                let (dx, dy) = (ox + x, oy + y);
                out[i] = if dx >= self.w as usize || dy >= self.h as usize {
                    Color32::TRANSPARENT
                } else if paper {
                    self.paper
                } else if ((dx / 16) + (dy / 16)) % 2 == 0 {
                    Color32::from_gray(204)
                } else {
                    Color32::from_gray(240)
                };
            }
        }
        for layer in self.layers.iter().rev() {
            if !layer.visible || layer.kind != LayerKind::Raster || layer.opacity <= 0.0 {
                continue;
            }
            let Some(src) = layer.tiles.get(&key) else { continue };
            let op = layer.opacity / 100.0;
            for (d, s) in out.iter_mut().zip(src.iter()) {
                if s.a() == 0 {
                    continue;
                }
                *d = over(*d, *s, op);
            }
        }
    }
}

/// Source-over for premultiplied colors with an extra opacity on the source.
#[inline]
pub fn over(dst: Color32, src: Color32, opacity: f32) -> Color32 {
    let sa = src.a() as f32 / 255.0 * opacity;
    let k = 1.0 - sa;
    let ch = |s: u8, d: u8| (s as f32 * opacity + d as f32 * k).round().clamp(0.0, 255.0) as u8;
    Color32::from_rgba_premultiplied(
        ch(src.r(), dst.r()),
        ch(src.g(), dst.g()),
        ch(src.b(), dst.b()),
        ch(src.a(), dst.a()),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dirty_rect_maps_to_tiles() {
        let mut d = Document::new(1000, 600);
        d.take_dirty();
        d.mark_dirty_rect(250, 250, 260, 520);
        let mut t = d.take_dirty();
        t.sort();
        assert_eq!(t, vec![(0, 0), (0, 1), (0, 2), (1, 0), (1, 1), (1, 2)]);
    }

    #[test]
    fn composite_paper_and_layer() {
        let mut d = Document::new(300, 300);
        d.layers[0].tile_mut((0, 0))[0] = Color32::from_rgba_premultiplied(0, 0, 128, 128); // half blue
        let mut out = vec![Color32::TRANSPARENT; TILE * TILE];
        d.composite_tile((0, 0), &mut out);
        let p = out[0];
        assert_eq!(p.a(), 255);
        assert!(p.b() > 250 && p.r() > 120 && p.r() < 135, "{p:?}");
        // Outside the document stays transparent.
        d.composite_tile((1, 1), &mut out);
        assert_eq!(out[TILE * TILE - 1], Color32::TRANSPARENT);
    }
}
