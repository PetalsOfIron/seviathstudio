// SPDX-License-Identifier: GPL-3.0-or-later
// The document: size, paper and a stack of layers. Pixels live in 256×256 tiles that are only
// created where something is drawn, so big canvases and many layers stay light.
// Pixels are premultiplied RGBA (egui's Color32), the same as the GPU textures.
//
// Layers are one flat list, top first, like the layer panel. A folder is followed directly by its
// contents (children carry the folder's id as `parent`), so a folder and everything in it is always
// one contiguous block that can be moved or deleted together.

use std::borrow::Cow;
use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use eframe::egui::Color32;

use crate::blend::{self, CLEAR, Px};
use crate::stroke::{self, LiveStroke};

pub const TILE: usize = 256;
pub type TileKey = (i32, i32);
pub type LayerId = u64;
/// Pixels of one tile. Shared and copy-on-write: history steps keep old tiles for free,
/// and a tile is only copied when it is painted on.
pub type TileData = Arc<Vec<Color32>>;

/// Color labels for organizing many layers (index = Layer::tag, 0 = none).
pub const TAG_COLORS: [(Color32, &str); 8] = [
    (Color32::TRANSPARENT, "None"),
    (Color32::from_rgb(226, 88, 88), "Red"),
    (Color32::from_rgb(236, 150, 64), "Orange"),
    (Color32::from_rgb(222, 200, 72), "Yellow"),
    (Color32::from_rgb(76, 196, 110), "Green"),
    (Color32::from_rgb(80, 150, 236), "Blue"),
    (Color32::from_rgb(170, 120, 236), "Violet"),
    (Color32::from_rgb(150, 150, 150), "Gray"),
];

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Blend {
    PassThrough, // folders only: contents blend straight into what is below
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
    /// Modes for normal layers (folders also get Pass through).
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
            Blend::PassThrough => "Pass through",
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
            Blend::PassThrough => "The folder's layers blend straight into what is below, as if there were no folder.",
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
    /// Changes the colors of everything below it (one of the color filters).
    Adjust,
    /// Live shading from the depth layers below it.
    Light,
}

#[derive(Clone, Debug)]
pub struct Layer {
    pub id: LayerId,
    pub parent: Option<LayerId>,
    pub name: String,
    pub kind: LayerKind,
    pub visible: bool,
    pub locked: bool,
    pub lock_alpha: bool,
    pub clip: bool,
    pub expanded: bool, // folders: open in the panel
    pub tag: u8,        // color label, 0 = none (see TAG_COLORS)
    pub fav: bool,      // starred: shows in the favorites row
    /// The lines of a vector layer (its tiles are drawn from them).
    pub vector: Option<Arc<crate::vector::VecData>>,
    /// Non-destructive effects (shadow, glow, outline, overlays), drawn when the picture is put together.
    pub fx: Vec<crate::effects::Effect>,
    /// Adjustment layers: which color filter and its settings.
    pub adjust: Option<crate::effects::Adjust>,
    /// Light layers: where the light is and how it looks.
    pub light: Option<crate::light::Light>,
    /// A depth layer: paint white where things are near, black where they are far. Only shown while selected.
    pub depth: bool,
    /// A material layer: the paint is shaded live as metal, glass, velvet, neon…
    pub material: Option<crate::material::Material>,
    /// A shadow, glow or ray layer: the paint is a shape the light works on.
    pub lightfx: Option<crate::lightfx::LightFx>,
    /// Blocks light: casts shadows and stops rays of the light-effect layers.
    pub solid: bool,
    /// A text layer: the words and how they look (its shapes are in `vector`).
    pub text: Option<crate::text::TextData>,
    /// What this layer is for rigging (left eye, front hair…); see parts.rs.
    pub part: Option<crate::parts::Part>,
    pub opacity: f32,   // 0..100
    pub blend: Blend,
    pub tiles: HashMap<TileKey, TileData>,
    pub rev: u64, // bumped on every pixel change (thumbnails watch it)
}

impl Layer {
    /// Pixel layer with at least one effect switched on, or a material (both need the area around a tile).
    pub fn fx_active(&self) -> bool {
        matches!(self.kind, LayerKind::Raster | LayerKind::Vector) && (crate::effects::any_on(&self.fx) || self.material.is_some() || self.lightfx.is_some())
    }

    pub fn paintable(&self) -> bool {
        self.kind == LayerKind::Raster && !self.locked
    }

    /// Tile for writing, created empty on first use.
    pub fn tile_mut(&mut self, key: TileKey) -> &mut Vec<Color32> {
        Arc::make_mut(self.tiles.entry(key).or_insert_with(|| Arc::new(vec![Color32::TRANSPARENT; TILE * TILE])))
    }
}

/// Where a dragged layer lands, relative to another layer.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Drop {
    Above(LayerId),
    Below(LayerId),
    Into(LayerId), // top of a folder
}

pub struct Document {
    pub w: u32,
    pub h: u32,
    pub paper: Color32,
    pub layers: Vec<Layer>,
    dirty: HashSet<TileKey>,
    pub revision: u64,
    next_id: LayerId,
    /// The stroke being drawn right now (shown on top of its layer until the pen lifts).
    pub live: Option<LiveStroke>,
    /// What happened while this picture was made (kept in the project file).
    pub log: crate::processlog::ProcessLog,
    /// Meshes, changers and sliders for moving the drawing (see rig/). Shared between history steps.
    pub rig: std::sync::Arc<crate::rig::Rig>,
    /// The depth layer being looked at (depth layers are hidden otherwise).
    pub depth_view: Option<LayerId>,
    /// Which paint is still wet (for bleeding), and the clock it is measured on (seconds).
    pub wet: crate::wet::WetMap,
    /// Relief of material layers, made once per change of the layer (see material.rs).
    height_cache: std::sync::Mutex<HashMap<LayerId, Arc<crate::material::HeightField>>>,
    pub clock: f64,
    /// Size of the checkerboard squares shown where the picture is transparent.
    pub checker: usize,
    /// What is selected (None = nothing: tools work everywhere).
    pub selection: Option<crate::select::Selection>,
    /// Saved layer-panel filters of this project.
    pub filter_presets: Vec<crate::layerfilter::Preset>,
    /// DPI, notes, tags, guide lines, autosave override.
    pub meta: crate::meta::ProjectMeta,
}

impl Document {
    pub fn new(w: u32, h: u32) -> Self {
        let mut d = Self {
            w,
            h,
            paper: Color32::WHITE,
            layers: Vec::new(),
            dirty: HashSet::new(),
            revision: 0,
            next_id: 1,
            live: None,
            log: Default::default(),
            rig: Default::default(),
            depth_view: None,
            wet: Default::default(),
            height_cache: Default::default(),
            clock: 0.0,
            selection: None,
            filter_presets: Vec::new(),
            meta: crate::meta::ProjectMeta::default(),
            checker: 16,
        };
        let l1 = d.new_layer("Layer 1", LayerKind::Raster);
        let paper = d.new_layer("Paper", LayerKind::Paper);
        d.layers = vec![l1, paper];
        d.mark_all_dirty();
        d
    }

    /// A document from loaded parts (files). Layer ids are kept; new ids continue after them.
    pub fn from_parts(w: u32, h: u32, paper: Color32, layers: Vec<Layer>) -> Self {
        let next_id = layers.iter().map(|l| l.id).max().unwrap_or(0) + 1;
        let mut d = Self { w, h, paper, layers, dirty: HashSet::new(), revision: 0, next_id, live: None, log: Default::default(), rig: Default::default(), depth_view: None, wet: Default::default(), height_cache: Default::default(), clock: 0.0, checker: 16, selection: None, filter_presets: Vec::new(), meta: crate::meta::ProjectMeta::default() };
        d.mark_all_dirty();
        d
    }

    pub fn new_layer(&mut self, name: &str, kind: LayerKind) -> Layer {
        let id = self.next_id;
        self.next_id += 1;
        Layer {
            id,
            parent: None,
            name: name.into(),
            kind,
            visible: true,
            locked: false,
            lock_alpha: false,
            clip: false,
            expanded: true,
            tag: 0,
            opacity: 100.0,
            blend: if kind == LayerKind::Group { Blend::PassThrough } else { Blend::Normal },
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
            rev: 0,
        }
    }

    // ---------- Tiles and redraw ----------

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
        // effects reach into the neighbouring tiles
        if self.layers.iter().any(|l| l.visible && (l.fx_active() || l.kind == LayerKind::Light)) {
            let (tx, ty) = (self.tiles_x(), self.tiles_y());
            let near: Vec<TileKey> = self.dirty.iter().flat_map(|&(x, y)| (-1..=1).flat_map(move |dy| (-1..=1).map(move |dx| (x + dx, y + dy)))).collect();
            self.dirty.extend(near.into_iter().filter(|&(x, y)| x >= 0 && y >= 0 && x < tx && y < ty));
        }
        self.dirty.drain().collect()
    }

    pub fn paper_visible(&self) -> bool {
        self.layers.iter().any(|l| l.kind == LayerKind::Paper && l.visible)
    }

    // ---------- Tree helpers ----------

    pub fn index_of(&self, id: LayerId) -> Option<usize> {
        self.layers.iter().position(|l| l.id == id)
    }

    pub fn depth(&self, idx: usize) -> usize {
        let mut d = 0;
        let mut p = self.layers[idx].parent;
        while let Some(pid) = p {
            d += 1;
            p = self.index_of(pid).and_then(|i| self.layers[i].parent);
        }
        d
    }

    /// Number of entries in the block starting at `idx` (1 + everything inside a folder).
    pub fn block_len(&self, idx: usize) -> usize {
        let id = self.layers[idx].id;
        let mut n = 1;
        while idx + n < self.layers.len() && self.is_inside(self.layers[idx + n].id, id) {
            n += 1;
        }
        n
    }

    /// True when `id` is somewhere inside folder `folder`.
    pub fn is_inside(&self, id: LayerId, folder: LayerId) -> bool {
        let mut p = self.index_of(id).and_then(|i| self.layers[i].parent);
        while let Some(pid) = p {
            if pid == folder {
                return true;
            }
            p = self.index_of(pid).and_then(|i| self.layers[i].parent);
        }
        false
    }

    /// Hidden in the panel because some folder above it is closed.
    pub fn collapsed_away(&self, idx: usize) -> bool {
        let mut p = self.layers[idx].parent;
        while let Some(pid) = p {
            let Some(i) = self.index_of(pid) else { break };
            if !self.layers[i].expanded {
                return true;
            }
            p = self.layers[i].parent;
        }
        false
    }

    /// The next layer below `idx` in the same folder (skipping folder contents).
    pub fn sibling_below(&self, idx: usize) -> Option<usize> {
        let parent = self.layers[idx].parent;
        let j = idx + self.block_len(idx);
        (j < self.layers.len() && self.layers[j].parent == parent).then_some(j)
    }

    // ---------- Layer operations ----------

    /// Insert a new layer above `target` (in the same folder), or at the top inside it
    /// when the target is an open folder. Returns its id.
    pub fn insert_layer(&mut self, mut layer: Layer, target: LayerId) -> LayerId {
        let id = layer.id;
        let Some(t) = self.index_of(target) else {
            self.layers.insert(0, layer);
            return id;
        };
        let tl = &self.layers[t];
        let (at, parent) = if tl.kind == LayerKind::Group && tl.expanded { (t + 1, Some(tl.id)) } else { (t, tl.parent) };
        layer.parent = parent;
        self.layers.insert(at, layer);
        self.mark_all_dirty();
        id
    }

    /// Insert a layer just below another one (same folder). The paper has nothing below it.
    pub fn insert_below(&mut self, mut layer: Layer, target: LayerId) -> Option<LayerId> {
        let i = self.index_of(target)?;
        if self.layers[i].kind == LayerKind::Paper {
            return None;
        }
        let at = i + self.block_len(i);
        layer.parent = self.layers[i].parent;
        let id = layer.id;
        self.layers.insert(at, layer);
        self.mark_all_dirty();
        Some(id)
    }

    /// Delete a layer (a folder goes with everything in it). The paper can't be deleted.
    pub fn delete(&mut self, id: LayerId) -> bool {
        let Some(i) = self.index_of(id) else { return false };
        if self.layers[i].kind == LayerKind::Paper {
            return false;
        }
        let n = self.block_len(i);
        self.layers.drain(i..i + n);
        self.mark_all_dirty();
        true
    }

    /// Move a layer (with its contents) to a drop position. Refuses moves into itself and
    /// anything below the paper.
    pub fn move_layer(&mut self, id: LayerId, to: Drop) -> bool {
        let Some(from) = self.index_of(id) else { return false };
        if self.layers[from].kind == LayerKind::Paper {
            return false;
        }
        let target = match to {
            Drop::Above(t) | Drop::Below(t) | Drop::Into(t) => t,
        };
        if target == id || self.is_inside(target, id) {
            return false;
        }
        let Some(ti) = self.index_of(target) else { return false };
        let tl = &self.layers[ti];
        if matches!(to, Drop::Below(_)) && tl.kind == LayerKind::Paper {
            return false;
        }
        if matches!(to, Drop::Into(_)) && tl.kind != LayerKind::Group {
            return false;
        }
        let new_parent = match to {
            Drop::Into(t) => Some(t),
            _ => tl.parent,
        };

        let n = self.block_len(from);
        let mut block: Vec<Layer> = self.layers.drain(from..from + n).collect();
        block[0].parent = new_parent;
        let ti = self.index_of(target).expect("target still there");
        let at = match to {
            Drop::Above(_) => ti,
            Drop::Below(_) => ti + self.block_len(ti),
            Drop::Into(_) => ti + 1,
        };
        self.layers.splice(at..at, block);
        self.mark_all_dirty();
        true
    }

    pub fn duplicate(&mut self, id: LayerId) -> Option<LayerId> {
        let i = self.index_of(id)?;
        if self.layers[i].kind == LayerKind::Paper {
            return None;
        }
        let n = self.block_len(i);
        // Copy the whole block with fresh ids, keeping the folder structure inside it.
        let mut map = HashMap::new();
        let mut copies = Vec::with_capacity(n);
        for k in 0..n {
            let mut c = self.layers[i + k].clone();
            let new_id = self.next_id;
            self.next_id += 1;
            map.insert(c.id, new_id);
            c.id = new_id;
            c.fav = false;
            if k == 0 {
                c.name = format!("{} copy", c.name);
            } else {
                c.parent = c.parent.map(|p| *map.get(&p).unwrap_or(&p));
            }
            copies.push(c);
        }
        let first = copies[0].id;
        self.layers.splice(i..i, copies);
        self.mark_all_dirty();
        Some(first)
    }

    /// Put a layer into a new folder in its place. Returns the folder id.
    pub fn group(&mut self, id: LayerId, name: &str) -> Option<LayerId> {
        let i = self.index_of(id)?;
        if self.layers[i].kind == LayerKind::Paper {
            return None;
        }
        let mut folder = self.new_layer(name, LayerKind::Group);
        folder.parent = self.layers[i].parent;
        let fid = folder.id;
        self.layers.insert(i, folder);
        self.layers[i + 1].parent = Some(fid);
        self.mark_all_dirty();
        Some(fid)
    }

    /// Bake a pixel layer into the pixel layer below it (same folder). Returns the merged layer's id.
    pub fn merge_down(&mut self, id: LayerId) -> Result<LayerId, &'static str> {
        let i = self.index_of(id).ok_or("No layer.")?;
        let below = self.sibling_below(i).ok_or("Nothing below to merge into.")?;
        let (top, bot) = (&self.layers[i], &self.layers[below]);
        let pixels = |k: LayerKind| matches!(k, LayerKind::Raster | LayerKind::Vector);
        if !pixels(top.kind) || !pixels(bot.kind) {
            return Err("Merge down works between two pixel or vector layers.");
        }
        if bot.locked {
            return Err("The layer below is locked.");
        }
        let src = self.layers.remove(i);
        let below = below - 1;
        let dst = &mut self.layers[below];
        let op = if src.visible { src.opacity / 100.0 } else { 0.0 };
        for (key, s) in &src.tiles {
            let d = dst.tile_mut(*key);
            for (dp, sp) in d.iter_mut().zip(s.iter()) {
                if sp.a() == 0 {
                    continue;
                }
                let r = if src.clip {
                    blend::atop(blend::to_px(*dp), blend::to_px(*sp), op, src.blend)
                } else {
                    blend::over(blend::to_px(*dp), blend::to_px(*sp), op, src.blend)
                };
                *dp = blend::from_px(r);
            }
        }
        dst.rev += 1;
        let mid = dst.id;
        self.mark_all_dirty();
        Ok(mid)
    }

    /// Bake several pixel layers into one new layer that takes the place of the lowest of them (so it stays under everything that was
    /// above the lowest one). The layers are put together the way they look now (blend modes, opacity, clipping); hidden layers and
    /// layers that are not pixel layers are left alone. Returns the new layer's id, or why nothing was done.
    pub fn merge_layers(&mut self, ids: &[LayerId], name: &str) -> Result<LayerId, &'static str> {
        let mut idx: Vec<usize> = ids.iter().filter_map(|id| self.index_of(*id)).filter(|i| self.layers[*i].kind == LayerKind::Raster && self.layers[*i].visible && self.layers[*i].text.is_none()).collect();
        idx.sort_unstable();
        idx.dedup();
        if idx.is_empty() {
            return Err("There are no pixel layers to put together.");
        }
        // the layers put together in a small document of their own (top first, as here)
        let mut tmp = Document::new(self.w, self.h);
        tmp.layers.clear();
        let mut keys: HashSet<TileKey> = HashSet::new();
        for &i in &idx {
            let mut l = self.layers[i].clone();
            l.parent = None;
            keys.extend(l.tiles.keys().copied());
            tmp.layers.push(l);
        }
        // what the lowest one was clipped to is not here
        if let Some(last) = tmp.layers.last_mut() {
            last.clip = false;
        }
        let mut tiles: HashMap<TileKey, TileData> = HashMap::new();
        for key in keys {
            let px = tmp.flatten_tile(key);
            if px.iter().all(|p| p[3] <= 0.0) {
                continue;
            }
            tiles.insert(key, Arc::new(px.iter().map(|p| blend::from_px(*p)).collect()));
        }
        let bottom = *idx.last().unwrap();
        let parent = self.layers[bottom].parent;
        let mut merged = self.new_layer(name, LayerKind::Raster);
        merged.tiles = tiles;
        merged.parent = parent;
        merged.rev += 1;
        let mid = merged.id;
        for &i in idx.iter().rev() {
            self.layers.remove(i);
        }
        self.layers.insert(bottom + 1 - idx.len(), merged);
        self.mark_all_dirty();
        Ok(mid)
    }

    // ---------- Compositing ----------

    /// Composites one tile for display. Transparent areas show a checkerboard when the paper is hidden.
    pub fn composite_tile(&self, key: TileKey, out: &mut [Color32]) {
        let buf = self.flatten_tile(key);
        let (ox, oy) = (key.0 as usize * TILE, key.1 as usize * TILE);
        let paper = self.paper_visible();
        for y in 0..TILE {
            for x in 0..TILE {
                let i = y * TILE + x;
                let (dx, dy) = (ox + x, oy + y);
                out[i] = if dx >= self.w as usize || dy >= self.h as usize {
                    Color32::TRANSPARENT
                } else if paper {
                    blend::from_px(buf[i])
                } else {
                    let c = self.checker.max(1);
                    let g = if ((dx / c) + (dy / c)) % 2 == 0 { 0.8 } else { 0.94 };
                    let p = buf[i];
                    let k = 1.0 - p[3];
                    blend::from_px([p[0] + g * k, p[1] + g * k, p[2] + g * k, 1.0])
                };
            }
        }
    }

    /// The flattened picture of one tile without any checkerboard (for export and color picking).
    pub fn flatten_tile(&self, key: TileKey) -> Vec<Px> {
        let mut buf = vec![CLEAR; TILE * TILE];
        self.composite_into(None, key, &mut buf);
        buf
    }

    /// Composite the layers of folder `parent` (None = top level) onto `backdrop`, bottom to top.
    fn composite_into(&self, parent: Option<LayerId>, key: TileKey, backdrop: &mut [Px]) {
        let kids: Vec<&Layer> = self.layers.iter().filter(|l| l.parent == parent).collect();
        // `kids` is top first; walk from the bottom.
        let mut i = kids.len();
        while i > 0 {
            i -= 1;
            let base = kids[i];
            // Clipped layers directly above this one (smaller indices) belong to its clip group.
            let mut top = i;
            while top > 0 && kids[top - 1].clip {
                top -= 1;
            }
            let clips = &kids[top..i];
            let next = top;
            if !base.visible || base.opacity <= 0.0 {
                i = next;
                continue;
            }
            let op = base.opacity / 100.0;

            // depth layers are only drawn while you look at them
            if base.depth && self.depth_view != Some(base.id) {
                i = next;
                continue;
            }

            // Adjustment layer: changes what is below it (layers clipped to it paint on their own)
            if base.kind == LayerKind::Adjust {
                if let Some(a) = &base.adjust {
                    a.apply(backdrop, TILE, op);
                }
                continue;
            }

            // Pass-through folder without clipped layers: contents go straight onto the backdrop.
            if base.kind == LayerKind::Group && base.blend == Blend::PassThrough && clips.is_empty() {
                if op >= 1.0 {
                    self.composite_into(Some(base.id), key, backdrop);
                } else {
                    let mut tmp = backdrop.to_vec();
                    self.composite_into(Some(base.id), key, &mut tmp);
                    for (b, t) in backdrop.iter_mut().zip(tmp) {
                        *b = blend::lerp(*b, t, op);
                    }
                }
                i = next;
                continue;
            }

            let mode = if base.blend == Blend::PassThrough { Blend::Normal } else { base.blend };
            let any_clip = clips.iter().any(|c| c.visible && c.opacity > 0.0);

            // Fast paths (no clip group): blend straight from the stored pixels, no temporary copy.
            if !any_clip && !base.fx_active() {
                match base.kind {
                    LayerKind::Raster | LayerKind::Vector => {
                        if let Some(t) = self.raster_tile(base, key) {
                            for (b, c) in backdrop.iter_mut().zip(t.iter()) {
                                if c.a() != 0 {
                                    *b = blend::over(*b, blend::to_px(*c), op, mode);
                                }
                            }
                        }
                        i = next;
                        continue;
                    }
                    LayerKind::Paper => {
                        let pp = blend::to_px(self.paper);
                        for b in backdrop.iter_mut() {
                            *b = blend::over(*b, pp, op, mode);
                        }
                        i = next;
                        continue;
                    }
                    _ => {}
                }
            }

            let Some(mut src) = self.layer_pixels(base, key) else {
                i = next;
                continue;
            };
            // Clipped layers paint only where the base has paint.
            for c in clips.iter().rev() {
                if !c.visible || c.opacity <= 0.0 {
                    continue;
                }
                let cop = c.opacity / 100.0;
                let cmode = if c.blend == Blend::PassThrough { Blend::Normal } else { c.blend };
                if matches!(c.kind, LayerKind::Raster | LayerKind::Vector) && !c.fx_active() {
                    if let Some(t) = self.raster_tile(c, key) {
                        for (s, p) in src.iter_mut().zip(t.iter()) {
                            if p.a() != 0 {
                                *s = blend::atop(*s, blend::to_px(*p), cop, cmode);
                            }
                        }
                    }
                    continue;
                }
                let Some(cp) = self.layer_pixels(c, key) else { continue };
                for (s, p) in src.iter_mut().zip(cp.iter()) {
                    if p[3] > 0.0 {
                        *s = blend::atop(*s, *p, cop, cmode);
                    }
                }
            }
            for (b, s) in backdrop.iter_mut().zip(src.iter()) {
                if s[3] > 0.0 {
                    *b = blend::over(*b, *s, op, mode);
                }
            }
            i = next;
        }
    }

    /// A pixel layer's tile, with the live stroke on top when it is being painted on.
    fn raster_tile<'a>(&'a self, l: &'a Layer, key: TileKey) -> Option<Cow<'a, Vec<Color32>>> {
        match &self.live {
            Some(live) if live.layer == l.id => live.merged(l.tiles.get(&key).map(|t| &**t), key).map(Cow::Owned),
            _ => l.tiles.get(&key).map(|t| Cow::Borrowed(&**t)),
        }
    }

    /// Merge the live stroke into its layer. Returns the touched tiles (with the layer id).
    pub fn commit_live(&mut self) -> Option<(LayerId, Vec<TileKey>)> {
        let live = self.live.take()?;
        let i = self.index_of(live.layer)?;
        let keys: Vec<TileKey> = live.tiles.keys().copied().collect();
        let layer = &mut self.layers[i];
        for (key, st) in &live.tiles {
            let mask = match &live.sel {
                Some(sel) => match sel.tile(*key) {
                    Some(m) => Some(m),
                    None => continue, // nothing selected in this tile
                },
                None => None,
            };
            let t = layer.tile_mut(*key);
            stroke::apply(t, st, &live.look, *key, mask);
        }
        layer.rev += 1;
        for k in &keys {
            self.mark_tile_dirty(*k);
        }
        Some((live.layer, keys))
    }

    pub fn mark_tile_dirty(&mut self, key: TileKey) {
        self.dirty.insert(key);
        self.revision += 1;
    }

    /// One tile of a pixel layer with its effects drawn (shadows and glows spill in from the neighbours).
    /// None when there is no paint within reach.
    pub(crate) fn fx_pixels(&self, l: &Layer, key: TileKey) -> Option<Vec<Px>> {
        let fx_on = crate::effects::any_on(&l.fx);
        let m = if fx_on { crate::effects::reach(&l.fx) } else { 0 }.max(l.material.as_ref().map(|x| x.reach()).unwrap_or(0)).max(l.lightfx.as_ref().map(|x| x.reach()).unwrap_or(0));
        let n = TILE + 2 * m;
        let mut near: Vec<Option<Cow<Vec<Color32>>>> = Vec::with_capacity(9);
        for dy in -1..=1 {
            for dx in -1..=1 {
                near.push(self.raster_tile(l, (key.0 + dx, key.1 + dy)));
            }
        }
        let casts_solid = l.lightfx.as_ref().is_some_and(|f| f.from_solid);
        let near_empty = near.iter().all(|t| t.is_none());
        if near_empty && !casts_solid {
            return None;
        }
        let (ox, oy) = (key.0 * TILE as i32 - m as i32, key.1 * TILE as i32 - m as i32);
        let (wmax, hmax) = (self.w as i32 - 1, self.h as i32 - 1);
        let mut buf = vec![CLEAR; n * n];
        for y in 0..n {
            // outside the page the edge pixel carries on, so nothing looks cut off at the border
            let dy = (oy + y as i32).clamp(0, hmax);
            for x in 0..n {
                let dx = (ox + x as i32).clamp(0, wmax);
                let (tx, ty) = (dx.div_euclid(TILE as i32), dy.div_euclid(TILE as i32));
                let (ix, iy) = ((tx - key.0 + 1) as usize, (ty - key.1 + 1) as usize);
                if ix > 2 || iy > 2 {
                    continue;
                }
                if let Some(t) = &near[iy * 3 + ix] {
                    let c = t[dy.rem_euclid(TILE as i32) as usize * TILE + dx.rem_euclid(TILE as i32) as usize];
                    if c.a() != 0 {
                        buf[y * n + x] = blend::to_px(c);
                    }
                }
            }
        }
        let buf = match &l.material {
            Some(mat) => {
                let light = self.scene_light();
                let depth = self.depth_height(self.index_of(l.id).unwrap_or(0), l.parent, key, m, n);
                let field = self.height_field(l, mat);
                let height: Vec<f32> = (0..n * n).map(|i| field.at((ox + (i % n) as i32) as f32 + 0.5, (oy + (i / n) as i32) as f32 + 0.5)).collect();
                crate::material::shade_with_height(mat, &light, &buf, height, depth.as_deref(), n)
            }
            None => match &l.lightfx {
                Some(lfx) => {
                    let at = self.index_of(l.id).unwrap_or(0);
                    let solid = self.solid_alpha(l.id, key, m, n);
                    if casts_solid && near_empty && solid.iter().all(|v| *v <= 0.0) {
                        return None;
                    }
                    let depth = self.depth_height(at, l.parent, key, m, n);
                    crate::lightfx::render(lfx, &self.scene_light(), &buf, &solid, depth.as_deref(), n, (ox, oy))
                }
                None => buf,
            },
        };
        let out = if fx_on { crate::effects::render(&l.fx, &buf, n, (ox, oy), (self.w, self.h)) } else { buf };
        let mut tile = vec![CLEAR; TILE * TILE];
        for y in 0..TILE {
            tile[y * TILE..(y + 1) * TILE].copy_from_slice(&out[(y + m) * n + m..(y + m) * n + m + TILE]);
        }
        Some(tile)
    }

    /// The shading of a light layer for one tile, from the depth layers below it.
    /// None when there is no depth paint within reach.
    pub(crate) fn light_pixels(&self, l: &Layer, key: TileKey) -> Option<Vec<Px>> {
        let light = l.light.as_ref()?;
        let at = self.index_of(l.id)?;
        let m = light.reach();
        let n = TILE + 2 * m;
        let height = self.depth_height(at, l.parent, key, m, n)?;
        let out = crate::light::shade(light, &height, n);
        let mut tile = vec![CLEAR; TILE * TILE];
        for y in 0..TILE {
            tile[y * TILE..(y + 1) * TILE].copy_from_slice(&out[(y + m) * n + m..(y + m) * n + m + TILE]);
        }
        Some(tile)
    }

    /// The relief of a material layer: made again only when the layer or the roundness changes.
    fn height_field(&self, l: &Layer, mat: &crate::material::Material) -> Arc<crate::material::HeightField> {
        let key = crate::material::HeightField::key_of(l.rev, mat, self.w, self.h);
        if let Ok(cache) = self.height_cache.lock() {
            if let Some(f) = cache.get(&l.id).filter(|f| f.key == key) {
                return f.clone();
            }
        }
        let t = TILE as i32;
        let alpha = |x: i32, y: i32| {
            if x < 0 || y < 0 || x >= self.w as i32 || y >= self.h as i32 {
                return 0.0;
            }
            self.raster_tile(l, (x.div_euclid(t), y.div_euclid(t))).map(|tile| tile[(y.rem_euclid(t) * t + x.rem_euclid(t)) as usize].a() as f32 / 255.0).unwrap_or(0.0)
        };
        let f = Arc::new(crate::material::HeightField::build(alpha, self.w, self.h, mat, key));
        if let Ok(mut cache) = self.height_cache.lock() {
            cache.insert(l.id, f.clone());
        }
        f
    }

    /// How much the layers marked "blocks light" cover each pixel of the square of `n` pixels around a tile.
    pub(crate) fn solid_alpha(&self, except: LayerId, key: TileKey, m: usize, n: usize) -> Vec<f32> {
        let mut out = vec![0f32; n * n];
        let (ox, oy) = (key.0 * TILE as i32 - m as i32, key.1 * TILE as i32 - m as i32);
        let (wmax, hmax) = (self.w as i32 - 1, self.h as i32 - 1);
        for s in self.layers.iter().filter(|s| s.solid && s.visible && s.id != except && matches!(s.kind, LayerKind::Raster | LayerKind::Vector)) {
            let near: Vec<Option<Cow<Vec<Color32>>>> = (-1..=1).flat_map(|dy| (-1..=1).map(move |dx| (dx, dy))).map(|(dx, dy)| self.raster_tile(s, (key.0 + dx, key.1 + dy))).collect();
            if near.iter().all(|t| t.is_none()) {
                continue;
            }
            let op = (s.opacity / 100.0).clamp(0.0, 1.0);
            for y in 0..n {
                let py = (oy + y as i32).clamp(0, hmax);
                for x in 0..n {
                    let px = (ox + x as i32).clamp(0, wmax);
                    let (ix, iy) = ((px.div_euclid(TILE as i32) - key.0 + 1) as usize, (py.div_euclid(TILE as i32) - key.1 + 1) as usize);
                    if ix > 2 || iy > 2 {
                        continue;
                    }
                    if let Some(t) = &near[iy * 3 + ix] {
                        let a = t[py.rem_euclid(TILE as i32) as usize * TILE + px.rem_euclid(TILE as i32) as usize].a() as f32 / 255.0 * op;
                        let o = &mut out[y * n + x];
                        *o = o.max(a);
                    }
                }
            }
        }
        out
    }

    /// The light of the scene: the first visible light layer, or a default from the upper left.
    pub fn scene_light(&self) -> crate::light::Light {
        self.layers.iter().find(|l| l.visible && l.kind == LayerKind::Light).and_then(|l| l.light.clone()).unwrap_or_default()
    }

    /// The height (0..1, white = near) of the visible depth layers below layer index `at` (in the same
    /// folder), for the square of `n` pixels around a tile with `m` pixels to spare. None if there is no depth paint.
    pub(crate) fn depth_height(&self, at: usize, parent: Option<LayerId>, key: TileKey, m: usize, n: usize) -> Option<Vec<f32>> {
        let depth: Vec<&Layer> = self.layers[at + 1..].iter().rev().filter(|d| d.depth && d.visible && parent.is_none_or(|p| self.is_inside(d.id, p))).collect();
        if depth.is_empty() {
            return None;
        }
        let (ox, oy) = (key.0 * TILE as i32 - m as i32, key.1 * TILE as i32 - m as i32);
        let (wmax, hmax) = (self.w as i32 - 1, self.h as i32 - 1);
        let mut height = vec![0f32; n * n];
        let mut any = false;
        for d in depth {
            let mut near: Vec<Option<Cow<Vec<Color32>>>> = Vec::with_capacity(9);
            for dy in -1..=1 {
                for dx in -1..=1 {
                    near.push(self.raster_tile(d, (key.0 + dx, key.1 + dy)));
                }
            }
            if near.iter().all(|t| t.is_none()) {
                continue;
            }
            any = true;
            let op = if d.opacity > 0.0 { d.opacity / 100.0 } else { continue };
            for y in 0..n {
                let py = (oy + y as i32).clamp(0, hmax);
                for x in 0..n {
                    let px = (ox + x as i32).clamp(0, wmax);
                    let (ix, iy) = ((px.div_euclid(TILE as i32) - key.0 + 1) as usize, (py.div_euclid(TILE as i32) - key.1 + 1) as usize);
                    if ix > 2 || iy > 2 {
                        continue;
                    }
                    if let Some(t) = &near[iy * 3 + ix] {
                        let c = t[py.rem_euclid(TILE as i32) as usize * TILE + px.rem_euclid(TILE as i32) as usize];
                        if c.a() != 0 {
                            // brightness of the paint (premultiplied, so this already includes its alpha)
                            let v = (0.2126 * c.r() as f32 + 0.7152 * c.g() as f32 + 0.0722 * c.b() as f32) / 255.0;
                            let a = c.a() as f32 / 255.0 * op;
                            let h = &mut height[y * n + x];
                            *h = v * op + *h * (1.0 - a);
                        }
                    }
                }
            }
        }
        any.then_some(height)
    }

    /// A layer's own pixels for one tile (a folder: its contents flattened on their own).
    fn layer_pixels(&self, l: &Layer, key: TileKey) -> Option<Vec<Px>> {
        match l.kind {
            LayerKind::Raster | LayerKind::Vector if l.fx_active() => self.fx_pixels(l, key),
            LayerKind::Raster | LayerKind::Vector => self.raster_tile(l, key).map(|t| t.iter().map(|c| blend::to_px(*c)).collect()),
            LayerKind::Adjust => None,
            LayerKind::Light => self.light_pixels(l, key),
            LayerKind::Paper => Some(vec![blend::to_px(self.paper); TILE * TILE]),
            LayerKind::Group => {
                let mut buf = vec![CLEAR; TILE * TILE];
                self.composite_into(Some(l.id), key, &mut buf);
                Some(buf)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn raster(d: &mut Document, name: &str) -> LayerId {
        let l = d.new_layer(name, LayerKind::Raster);
        let top = d.layers[0].id;
        d.insert_layer(l, top)
    }

    fn fill(d: &mut Document, id: LayerId, c: Color32) {
        let i = d.index_of(id).unwrap();
        for p in d.layers[i].tile_mut((0, 0)).iter_mut().take(10) {
            *p = c;
        }
    }

    fn names(d: &Document) -> Vec<&str> {
        d.layers.iter().map(|l| l.name.as_str()).collect()
    }

    /// Full redraw of a 3000×4000 page with every tile painted on 4 layers (one clipped, one in a folder).
    /// `cargo test --release bench_full_redraw -- --ignored --nocapture`
    #[test]
    #[ignore]
    fn bench_full_redraw() {
        use rayon::prelude::*;
        let mut d = Document::new(3000, 4000);
        for name in ["B", "C", "D"] {
            raster(&mut d, name);
        }
        let ids: Vec<LayerId> = d.layers.iter().filter(|l| l.kind == LayerKind::Raster).map(|l| l.id).collect();
        for (n, id) in ids.iter().enumerate() {
            let i = d.index_of(*id).unwrap();
            for ty in 0..d.tiles_y() {
                for tx in 0..d.tiles_x() {
                    d.layers[i].tile_mut((tx, ty)).fill(Color32::from_rgba_premultiplied(60 * n as u8, 40, 90, 200));
                }
            }
        }
        d.layers[0].clip = true;
        d.layers[1].blend = Blend::Multiply;
        let g = d.layers[2].id;
        d.group(g, "F");
        d.take_dirty();
        d.mark_all_dirty();
        let t = std::time::Instant::now();
        let keys = d.take_dirty();
        let n = keys.len();
        let _: Vec<Vec<Color32>> = keys
            .into_par_iter()
            .map(|k| {
                let mut out = vec![Color32::TRANSPARENT; TILE * TILE];
                d.composite_tile(k, &mut out);
                out
            })
            .collect();
        println!("full redraw: {n} tiles in {:?}", t.elapsed());
    }

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
        d.composite_tile((1, 1), &mut out);
        assert_eq!(out[TILE * TILE - 1], Color32::TRANSPARENT);
    }

    #[test]
    fn multiply_layer_over_paper() {
        let mut d = Document::new(64, 64);
        let id = d.layers[0].id;
        fill(&mut d, id, Color32::from_rgb(128, 128, 128));
        let shade = raster(&mut d, "shade");
        fill(&mut d, shade, Color32::from_rgb(128, 128, 128));
        d.layers[0].blend = Blend::Multiply;
        let px = d.flatten_tile((0, 0))[0];
        assert!((px[0] - 0.25).abs() < 0.01, "{px:?}");
    }

    #[test]
    fn clipping_only_shows_on_base() {
        let mut d = Document::new(64, 64);
        let base = d.layers[0].id;
        // Base has paint on the first 10 pixels only.
        fill(&mut d, base, Color32::from_rgb(255, 0, 0));
        let clip = raster(&mut d, "clip");
        let i = d.index_of(clip).unwrap();
        for p in d.layers[i].tile_mut((0, 0)).iter_mut() {
            *p = Color32::from_rgb(0, 0, 255);
        }
        d.layers[i].clip = true;
        let out = d.flatten_tile((0, 0));
        assert!(out[0][2] > 0.99 && out[0][0] < 0.01, "clip shows on base: {:?}", out[0]);
        assert!(out[20][0] > 0.99 && out[20][1] > 0.99, "outside base stays paper: {:?}", out[20]);
    }

    #[test]
    fn folder_opacity_and_hiding() {
        let mut d = Document::new(64, 64);
        let l = d.layers[0].id;
        fill(&mut d, l, Color32::BLACK);
        let f = d.group(l, "Folder").unwrap();
        assert_eq!(names(&d), vec!["Folder", "Layer 1", "Paper"]);
        let fi = d.index_of(f).unwrap();
        d.layers[fi].blend = Blend::Normal;
        d.layers[fi].opacity = 50.0;
        assert!((d.flatten_tile((0, 0))[0][0] - 0.5).abs() < 0.01);
        d.layers[fi].visible = false;
        assert!((d.flatten_tile((0, 0))[0][0] - 1.0).abs() < 0.01);
    }

    #[test]
    fn move_into_folder_and_out() {
        let mut d = Document::new(64, 64);
        let a = d.layers[0].id; // "Layer 1"
        let b = raster(&mut d, "B");
        let f = d.group(b, "F").unwrap();
        assert_eq!(names(&d), vec!["F", "B", "Layer 1", "Paper"]);
        assert!(d.move_layer(a, Drop::Into(f)));
        assert_eq!(names(&d), vec!["F", "Layer 1", "B", "Paper"]);
        assert!(d.is_inside(a, f));
        // A folder can't go into itself; nothing goes below the paper.
        assert!(!d.move_layer(f, Drop::Into(f)));
        let paper = d.layers.last().unwrap().id;
        assert!(!d.move_layer(a, Drop::Below(paper)));
        // Moving the folder carries its contents.
        assert!(d.move_layer(f, Drop::Above(paper)));
        assert_eq!(names(&d), vec!["F", "Layer 1", "B", "Paper"]);
        assert!(d.move_layer(a, Drop::Below(f)));
        assert_eq!(names(&d), vec!["F", "B", "Layer 1", "Paper"]);
        assert!(!d.is_inside(a, f));
    }

    #[test]
    fn duplicate_and_merge_down() {
        let mut d = Document::new(64, 64);
        let a = d.layers[0].id;
        fill(&mut d, a, Color32::from_rgb(255, 0, 0));
        let c = d.duplicate(a).unwrap();
        assert_eq!(names(&d), vec!["Layer 1 copy", "Layer 1", "Paper"]);
        let ci = d.index_of(c).unwrap();
        d.layers[ci].opacity = 50.0;
        for p in d.layers[ci].tile_mut((0, 0)).iter_mut().take(10) {
            *p = Color32::from_rgb(0, 0, 255);
        }
        let m = d.merge_down(c).unwrap();
        assert_eq!(m, a);
        assert_eq!(names(&d), vec!["Layer 1", "Paper"]);
        let px = d.layers[0].tiles[&(0, 0)][0];
        assert!(px.r() > 120 && px.r() < 135 && px.b() > 120 && px.b() < 135, "{px:?}");
    }

    #[test]
    fn delete_folder_takes_contents() {
        let mut d = Document::new(64, 64);
        let a = d.layers[0].id;
        let f = d.group(a, "F").unwrap();
        assert!(d.delete(f));
        assert_eq!(names(&d), vec!["Paper"]);
        let paper = d.layers[0].id;
        assert!(!d.delete(paper));
    }
}
