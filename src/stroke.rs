// SPDX-License-Identifier: GPL-3.0-or-later
// The stroke being drawn: its paint lives in its own buffer until the pen lifts, so a stroke never
// gets darker than its opacity where it overlaps itself, and paper grain / textures apply to the
// whole stroke. The canvas shows layer + live stroke; on pen-up it is merged into the layer.

use std::collections::HashMap;

use eframe::egui::Color32;

use crate::blend::{self, Px};
use crate::brush::textures;
use crate::brush::{TexKind, TexPalette};
use crate::doc::{LayerId, TILE, TileKey};

/// Accumulated paint of the live stroke for one tile.
#[derive(Clone)]
pub struct StrokeTile {
    pub px: Vec<Px>,  // premultiplied paint, built up dab by dab with flow
    pub pr: Vec<f32>, // highest pen pressure that reached each pixel
}

impl StrokeTile {
    pub fn new() -> Self {
        Self { px: vec![[0.0; 4]; TILE * TILE], pr: vec![0.0; TILE * TILE] }
    }
}

/// How the stroke buffer turns into paint on the layer.
#[derive(Clone, Debug)]
pub struct StrokeLook {
    pub opacity: f32,
    pub texture: f32,
    pub tex_kind: TexKind,
    pub tex_scale: f32,
    pub tex_color: f32,
    pub tex_palette: TexPalette,
    pub canvas_texture: bool, // false for hair/fur textures, which live inside the tip
    pub pen: bool,            // real pen pressure: light pressure shows more grain
    pub color: [f32; 3],
    pub erase: bool,
    pub lock_alpha: bool,
}

pub struct LiveStroke {
    pub layer: LayerId,
    pub tiles: HashMap<TileKey, StrokeTile>,
    pub look: StrokeLook,
}

impl LiveStroke {
    /// The layer tile with the live stroke applied (None when neither has anything here).
    pub fn merged(&self, layer_tile: Option<&Vec<Color32>>, key: TileKey) -> Option<Vec<Color32>> {
        let Some(st) = self.tiles.get(&key) else { return layer_tile.cloned() };
        let mut out = layer_tile.cloned().unwrap_or_else(|| vec![Color32::TRANSPARENT; TILE * TILE]);
        apply(&mut out, st, &self.look, key);
        Some(out)
    }
}

/// Put the stroke's paint onto layer pixels.
pub fn apply(dst: &mut [Color32], st: &StrokeTile, look: &StrokeLook, key: TileKey) {
    let tex = (look.texture > 0.0 || look.tex_color > 0.0).then(|| textures::get(look.tex_kind));
    let (ox, oy) = (key.0 as f32 * TILE as f32, key.1 as f32 * TILE as f32);
    for i in 0..TILE * TILE {
        let s = st.px[i];
        if s[3] <= 0.0005 {
            continue;
        }
        let (x, y) = (ox + (i % TILE) as f32 + 0.5, oy + (i / TILE) as f32 + 0.5);
        let mut a = s[3] * look.opacity;
        let mut c = [s[0] / s[3], s[1] / s[3], s[2] / s[3]];
        if let Some(t) = tex {
            if look.texture > 0.0 && look.canvas_texture {
                // Light pen pressure shows more grain, heavy pressure fills it in.
                let fill = if look.pen { 1.0 - 0.45 * st.pr[i] * st.pr[i] } else { 1.0 };
                a *= 1.0 - (look.texture * fill).min(1.0) * textures::sample_a(t, x, y, look.tex_scale);
            }
            if look.tex_color > 0.0 && !look.erase {
                let tc = match look.tex_palette {
                    TexPalette::Natural => textures::sample_col(t, x, y, look.tex_scale),
                    TexPalette::FromBrush => textures::brush_shade(look.color, textures::sample_a(t, x, y, look.tex_scale)),
                };
                let k = look.tex_color.min(1.0);
                c = [0, 1, 2].map(|j| c[j] + (tc[j] - c[j]) * k);
            }
        }
        if a <= 0.0 {
            continue;
        }
        let d = blend::to_px(dst[i]);
        let out = if look.erase {
            let k = 1.0 - a.min(1.0);
            [d[0] * k, d[1] * k, d[2] * k, d[3] * k]
        } else if look.lock_alpha {
            [d[0] + (c[0] * d[3] - d[0]) * a, d[1] + (c[1] * d[3] - d[1]) * a, d[2] + (c[2] * d[3] - d[2]) * a, d[3]]
        } else {
            let k = 1.0 - a;
            [c[0] * a + d[0] * k, c[1] * a + d[1] * k, c[2] * a + d[2] * k, a + d[3] * k]
        };
        dst[i] = blend::from_px(out);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn look() -> StrokeLook {
        StrokeLook {
            opacity: 0.5,
            texture: 0.0,
            tex_kind: TexKind::Paper,
            tex_scale: 1.0,
            tex_color: 0.0,
            tex_palette: TexPalette::Natural,
            canvas_texture: true,
            pen: false,
            color: [1.0, 0.0, 0.0],
            erase: false,
            lock_alpha: false,
        }
    }

    #[test]
    fn opacity_caps_overlapping_paint() {
        let mut st = StrokeTile::new();
        st.px[0] = [1.0, 0.0, 0.0, 1.0]; // fully built-up red
        let mut dst = vec![Color32::TRANSPARENT; TILE * TILE];
        apply(&mut dst, &st, &look(), (0, 0));
        assert!((dst[0].a() as i32 - 128).abs() <= 1, "{:?}", dst[0]);
    }

    #[test]
    fn texture_takes_paint_away_somewhere() {
        let mut st = StrokeTile::new();
        for p in st.px.iter_mut() {
            *p = [1.0, 0.0, 0.0, 1.0];
        }
        let mut l = look();
        l.opacity = 1.0;
        l.texture = 1.0;
        l.tex_kind = TexKind::Dots;
        let mut dst = vec![Color32::TRANSPARENT; TILE * TILE];
        apply(&mut dst, &st, &l, (0, 0));
        let min = dst.iter().map(|c| c.a()).min().unwrap();
        let max = dst.iter().map(|c| c.a()).max().unwrap();
        assert!(min < 60 && max > 200, "{min}..{max}");
    }
}
