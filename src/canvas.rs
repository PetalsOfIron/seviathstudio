// SPDX-License-Identifier: GPL-3.0-or-later
// Canvas display: composites dirty tiles, keeps one GPU texture per tile, draws them through the
// view transform (so zoom/rotate/mirror are free), and keeps a small thumbnail for the navigator.

use std::collections::HashMap;

use rayon::prelude::*;

use eframe::egui::{
    self, Color32, ColorImage, Mesh, Painter, Pos2, Rect, Shape, TextureHandle, TextureOptions, pos2,
};

use crate::doc::{Document, Layer, LayerId, LayerKind, TILE, TileKey};
use crate::view::View;

const THUMB_MAX: usize = 320;
const LAYER_THUMB: usize = 64;

pub struct CanvasRenderer {
    textures: HashMap<TileKey, TextureHandle>,
    composite: HashMap<TileKey, Vec<Color32>>, // CPU copy, used for the thumbnail and color picking
    thumb: Option<TextureHandle>,
    thumb_revision: u64,
    thumb_time: f64,
    layer_thumbs: HashMap<LayerId, (u64, f64, TextureHandle)>, // (layer rev, time, texture)
    smooth: bool,       // smooth (linear) pixels when zoomed in, instead of crisp ones
    pub nav_fps: f32,   // navigator thumbnail refreshes per second while drawing
}

impl Default for CanvasRenderer {
    fn default() -> Self {
        Self {
            textures: HashMap::new(),
            composite: HashMap::new(),
            thumb: None,
            thumb_revision: u64::MAX,
            thumb_time: 0.0,
            layer_thumbs: HashMap::new(),
            smooth: false,
            nav_fps: 6.0,
        }
    }
}

fn tex_options(smooth: bool) -> TextureOptions {
    TextureOptions {
        magnification: if smooth { egui::TextureFilter::Linear } else { egui::TextureFilter::Nearest },
        minification: egui::TextureFilter::Linear,
        wrap_mode: egui::TextureWrapMode::ClampToEdge,
        mipmap_mode: Some(egui::TextureFilter::Linear), // smooth when zoomed out
    }
}

impl CanvasRenderer {
    /// Drop everything (new document / different size).
    #[allow(dead_code)] // used by the New project dialog (file step)
    pub fn reset(&mut self) {
        self.textures.clear();
        self.composite.clear();
        self.thumb = None;
        self.thumb_revision = u64::MAX;
        self.layer_thumbs.clear();
    }

    /// Smooth or crisp pixels when zoomed in. Returns true when everything must be redrawn.
    pub fn set_smooth(&mut self, smooth: bool) -> bool {
        if self.smooth == smooth {
            return false;
        }
        self.smooth = smooth;
        self.textures.clear();
        true
    }

    /// Re-composite dirty tiles and upload them.
    pub fn sync(&mut self, ctx: &egui::Context, doc: &mut Document) {
        // Composite all dirty tiles in parallel, then upload.
        let dirty = doc.take_dirty();
        let doc_ref: &Document = doc;
        let done: Vec<(TileKey, Vec<Color32>)> = dirty
            .into_par_iter()
            .map(|key| {
                let mut buf = vec![Color32::TRANSPARENT; TILE * TILE];
                doc_ref.composite_tile(key, &mut buf);
                (key, buf)
            })
            .collect();
        for (key, buf) in done {
            let img = ColorImage::new([TILE, TILE], buf.clone());
            match self.textures.get_mut(&key) {
                Some(t) => t.set(img, tex_options(self.smooth)),
                None => {
                    let t = ctx.load_texture(format!("tile{}_{}", key.0, key.1), img, tex_options(self.smooth));
                    self.textures.insert(key, t);
                }
            }
            self.composite.insert(key, buf);
        }

        // Navigator thumbnail: at most ~6 times a second while drawing.
        let now = ctx.input(|i| i.time);
        let gap = 1.0 / self.nav_fps.max(1.0) as f64;
        if doc.revision != self.thumb_revision && now - self.thumb_time <= gap && self.thumb.is_some() {
            ctx.request_repaint_after_secs(gap as f32 + 0.01); // come back for the last change
        }
        if doc.revision != self.thumb_revision && (now - self.thumb_time > gap || self.thumb.is_none()) {
            self.thumb_revision = doc.revision;
            self.thumb_time = now;
            let img = self.thumbnail(doc);
            match &mut self.thumb {
                Some(t) => t.set(img, TextureOptions::LINEAR),
                None => self.thumb = Some(ctx.load_texture("navigator", img, TextureOptions::LINEAR)),
            }
            ctx.request_repaint_after_secs(0.2);
        }
    }

    fn thumbnail(&self, doc: &Document) -> ColorImage {
        let s = (THUMB_MAX as f32 / doc.w.max(doc.h) as f32).min(1.0);
        let tw = ((doc.w as f32 * s).round() as usize).max(1);
        let th = ((doc.h as f32 * s).round() as usize).max(1);
        let mut px = vec![Color32::TRANSPARENT; tw * th];
        for y in 0..th {
            for x in 0..tw {
                let dx = ((x as f32 + 0.5) / s) as usize;
                let dy = ((y as f32 + 0.5) / s) as usize;
                if let Some(c) = self.pixel(dx, dy) {
                    px[y * tw + x] = c;
                }
            }
        }
        ColorImage::new([tw, th], px)
    }

    /// Displayed (composited) color at a document pixel.
    pub fn pixel(&self, x: usize, y: usize) -> Option<Color32> {
        let key = ((x / TILE) as i32, (y / TILE) as i32);
        self.composite.get(&key).map(|t| t[(y % TILE) * TILE + x % TILE])
    }

    pub fn thumb(&self) -> Option<&TextureHandle> {
        self.thumb.as_ref()
    }

    /// Small picture of one layer's own pixels for the layer panel, refreshed at most 4× a second.
    pub fn layer_thumb(&mut self, ctx: &egui::Context, layer: &Layer, doc_w: u32, doc_h: u32) -> Option<egui::TextureId> {
        if !matches!(layer.kind, LayerKind::Raster | LayerKind::Vector) {
            return None;
        }
        let now = ctx.input(|i| i.time);
        if let Some((rev, time, tex)) = self.layer_thumbs.get(&layer.id) {
            if *rev == layer.rev {
                return Some(tex.id());
            }
            if now - *time < 0.25 {
                ctx.request_repaint_after_secs(0.26);
                return Some(tex.id());
            }
        }
        let s = LAYER_THUMB as f32 / doc_w.max(doc_h) as f32;
        let tw = ((doc_w as f32 * s).round() as usize).max(1);
        let th = ((doc_h as f32 * s).round() as usize).max(1);
        let mut px = vec![Color32::TRANSPARENT; tw * th];
        for y in 0..th {
            for x in 0..tw {
                let dx = ((x as f32 + 0.5) / s) as usize;
                let dy = ((y as f32 + 0.5) / s) as usize;
                let key = ((dx / TILE) as i32, (dy / TILE) as i32);
                if let Some(t) = layer.tiles.get(&key) {
                    px[y * tw + x] = t[(dy % TILE) * TILE + dx % TILE];
                }
            }
        }
        let img = ColorImage::new([tw, th], px);
        match self.layer_thumbs.get_mut(&layer.id) {
            Some(entry) => {
                entry.2.set(img, TextureOptions::LINEAR);
                entry.0 = layer.rev;
                entry.1 = now;
            }
            None => {
                let tex = ctx.load_texture(format!("layer{}", layer.id), img, TextureOptions::LINEAR);
                self.layer_thumbs.insert(layer.id, (layer.rev, now, tex));
            }
        }
        self.layer_thumbs.get(&layer.id).map(|e| e.2.id())
    }

    /// Draw every tile that's on screen.
    pub fn paint(&self, p: &Painter, view: &View, doc: &Document) {
        let clip = p.clip_rect();
        let t = TILE as f32;
        for ty in 0..doc.tiles_y() {
            for tx in 0..doc.tiles_x() {
                let Some(tex) = self.textures.get(&(tx, ty)) else { continue };
                let x0 = tx as f32 * t;
                let y0 = ty as f32 * t;
                let x1 = (x0 + t).min(doc.w as f32);
                let y1 = (y0 + t).min(doc.h as f32);
                let corners = [pos2(x0, y0), pos2(x1, y0), pos2(x1, y1), pos2(x0, y1)].map(|c| view.doc_to_screen(c));
                let bb = Rect::from_points(&corners);
                if !bb.intersects(clip) {
                    continue;
                }
                let (u1, v1) = ((x1 - x0) / t, (y1 - y0) / t);
                let uvs = [pos2(0.0, 0.0), pos2(u1, 0.0), pos2(u1, v1), pos2(0.0, v1)];
                let mut m = Mesh::with_texture(tex.id());
                for (pos, uv) in corners.iter().zip(uvs) {
                    m.vertices.push(egui::epaint::Vertex { pos: *pos, uv, color: Color32::WHITE });
                }
                m.add_triangle(0, 1, 2);
                m.add_triangle(0, 2, 3);
                p.add(Shape::mesh(m));
            }
        }
    }
}

/// Document corners on screen (for the shadow and border around the page).
pub fn page_quad(view: &View, doc: &Document) -> Vec<Pos2> {
    let (w, h) = (doc.w as f32, doc.h as f32);
    [pos2(0.0, 0.0), pos2(w, 0.0), pos2(w, h), pos2(0.0, h)].iter().map(|c| view.doc_to_screen(*c)).collect()
}
