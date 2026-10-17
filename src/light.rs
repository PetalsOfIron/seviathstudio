// SPDX-License-Identifier: GPL-3.0-or-later
// Light and depth. A depth layer is a picture you paint once with a brush: white = near, black =
// far (nose close, eye sockets deep). A light layer reads the depth layers below it and draws the
// shading live: surfaces that face the light get a highlight, surfaces that face away get a shadow.
// Move the light (or change the depth) and the shading follows; no repainting. The shading is a
// layer of its own, so it can be hidden, faded or given a blend mode like any other.
//
// This is form shading from the depth (like a bump map). It doesn't cast shadows onto other layers.

use serde::{Deserialize, Serialize};

use crate::blend::{CLEAR, Px};
use crate::filters::gauss_channel;

#[derive(Clone, PartialEq, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct Light {
    /// Where the light is, as a point in a disc seen from above: the middle is straight above the
    /// drawing, the edge is low and sideways. x to the right, y downward (screen directions).
    pub dir: [f32; 2],
    /// 0..1
    pub strength: f32,
    /// How tall full white is, in pixels (bigger = steeper surfaces = stronger shading).
    pub depth: f32,
    /// Smooths the depth before shading, in pixels, so brush strokes don't show.
    pub soften: f32,
    /// How quickly the shading reaches full strength.
    pub contrast: f32,
    pub shadow: [u8; 3],
    pub highlight: [u8; 3],
}

impl Default for Light {
    fn default() -> Self {
        Light { dir: [-0.5, -0.55], strength: 0.7, depth: 60.0, soften: 10.0, contrast: 2.0, shadow: [48, 36, 96], highlight: [255, 238, 205] }
    }
}

pub const MAX_DEPTH_PX: f32 = 400.0;
pub const MAX_SOFTEN: f32 = 40.0;

impl Light {
    pub fn sanitized(mut self) -> Light {
        let f = |v: f32, lo: f32, hi: f32| if v.is_finite() { v.clamp(lo, hi) } else { lo };
        let (x, y) = (f(self.dir[0], -1.0, 1.0), f(self.dir[1], -1.0, 1.0));
        self.dir = Light::clamp_dir([x, y]);
        self.strength = f(self.strength, 0.0, 1.0);
        self.depth = f(self.depth, 1.0, MAX_DEPTH_PX);
        self.soften = f(self.soften, 0.0, MAX_SOFTEN);
        self.contrast = f(self.contrast, 0.2, 8.0);
        self
    }

    /// Keep the light inside the disc (just short of the rim, so it always has some height).
    pub fn clamp_dir(d: [f32; 2]) -> [f32; 2] {
        let len = d[0].hypot(d[1]);
        if len > 0.97 { [d[0] / len * 0.97, d[1] / len * 0.97] } else { d }
    }

    /// Unit vector from a surface toward the light.
    pub fn vector(&self) -> [f32; 3] {
        let d = Light::clamp_dir(self.dir);
        [d[0], d[1], (1.0 - d[0] * d[0] - d[1] * d[1]).max(0.0).sqrt()]
    }

    /// Pixels the shading needs around a tile (for the smoothing).
    pub fn reach(&self) -> usize {
        (self.soften * 1.5).ceil() as usize + 3
    }
}

/// The shading for an `n`×`n` square of depth (0..1, white = near, 0 outside any depth paint).
/// Returns premultiplied pixels the same size; flat areas stay clear.
pub fn shade(l: &Light, height: &[f32], n: usize) -> Vec<Px> {
    let h = if l.soften > 0.3 { gauss_channel(height, n, n, l.soften / 2.0) } else { height.to_vec() };
    let lv = l.vector();
    let flat = lv[2]; // what a surface facing straight up receives
    let (sc, hc) = (color(l.shadow), color(l.highlight));
    let mut out = vec![CLEAR; n * n];
    for y in 1..n - 1 {
        for x in 1..n - 1 {
            let (gx, gy) = ((h[y * n + x + 1] - h[y * n + x - 1]) * 0.5 * l.depth, (h[(y + 1) * n + x] - h[(y - 1) * n + x]) * 0.5 * l.depth);
            if gx == 0.0 && gy == 0.0 {
                continue;
            }
            // the surface normal, tilted by the slope
            let inv = 1.0 / (gx * gx + gy * gy + 1.0).sqrt();
            let ndl = (-gx * lv[0] - gy * lv[1] + lv[2]) * inv;
            let d = ndl - flat;
            // a surface tilted a little already changes the light a lot: scale the difference up
            let t = (d.abs() * l.contrast * 4.0).min(1.0) * l.strength;
            if t <= 0.0 {
                continue;
            }
            let c = if d < 0.0 { sc } else { hc };
            out[y * n + x] = [c[0] * t, c[1] * t, c[2] * t, t];
        }
    }
    out
}

fn color(c: [u8; 3]) -> [f32; 3] {
    [c[0] as f32 / 255.0, c[1] as f32 / 255.0, c[2] as f32 / 255.0]
}

#[cfg(test)]
mod tests {
    use super::*;

    const N: usize = 81;

    /// A round bump (a dome) in the middle.
    fn dome() -> Vec<f32> {
        (0..N * N)
            .map(|i| {
                let (x, y) = ((i % N) as f32 - 40.0, (i / N) as f32 - 40.0);
                let r = x.hypot(y) / 30.0;
                if r < 1.0 { (1.0 - r * r).sqrt() } else { 0.0 }
            })
            .collect()
    }

    #[test]
    fn a_dome_is_lit_on_the_side_that_faces_the_light() {
        let l = Light { dir: [-0.6, 0.0], soften: 0.0, ..Light::default() };
        let out = shade(&l, &dome(), N);
        let (left, right) = (out[40 * N + 18], out[40 * N + 62]);
        // the light is on the left: the left side is lit (warm), the right side is in shade (cool)
        assert!(left[3] > 0.3 && left[0] > left[2], "left side: {left:?}");
        assert!(right[3] > 0.3 && right[2] > right[0], "right side: {right:?}");
        assert!(out[40 * N + 40][3] < 0.1, "the top of the dome faces up: no shading");
        assert_eq!(out[2 * N + 2], CLEAR, "flat ground stays clear");
    }

    #[test]
    fn moving_the_light_moves_the_highlight() {
        let mut l = Light { dir: [0.0, -0.6], soften: 0.0, ..Light::default() };
        let top = shade(&l, &dome(), N);
        assert!(top[22 * N + 40][0] > top[22 * N + 40][2] && top[22 * N + 40][3] > 0.3, "lit from above: the upper edge is lit");
        l.dir = [0.0, 0.6];
        let bottom = shade(&l, &dome(), N);
        assert!(bottom[22 * N + 40][2] > bottom[22 * N + 40][0], "lit from below: the upper edge is in shade");
    }

    #[test]
    fn strength_depth_and_softening_do_what_they_say() {
        let base = Light { dir: [-0.6, 0.0], soften: 0.0, ..Light::default() };
        let a = |l: &Light| shade(l, &dome(), N)[40 * N + 18][3];
        assert!(a(&Light { strength: 0.3, ..base.clone() }) < a(&base));
        assert_eq!(a(&Light { strength: 0.0, ..base.clone() }), 0.0);
        assert!(a(&Light { depth: 5.0, contrast: 1.0, ..base.clone() }) < a(&Light { depth: 40.0, contrast: 1.0, ..base.clone() }));
        // softening spreads the shading beyond the edge of the dome
        let sharp = shade(&base, &dome(), N);
        let soft = shade(&Light { soften: 8.0, ..base }, &dome(), N);
        let outside = 40 * N + 6;
        assert!(soft[outside][3] > sharp[outside][3], "soft {:?} sharp {:?}", soft[outside], sharp[outside]);
    }

    #[test]
    fn settings_from_a_file_are_held_in_range() {
        let l = Light { dir: [5.0, f32::NAN], strength: 9.0, depth: -3.0, soften: 1e9, contrast: f32::INFINITY, ..Light::default() }.sanitized();
        assert!(l.dir[0].hypot(l.dir[1]) <= 0.98);
        assert_eq!((l.strength, l.depth, l.soften), (1.0, 1.0, MAX_SOFTEN));
        assert!(l.contrast.is_finite() && l.vector()[2] > 0.0, "the light always has some height");
        let j = serde_json::to_string(&Light::default()).unwrap();
        assert_eq!(serde_json::from_str::<Light>(&j).unwrap(), Light::default());
        assert_eq!(serde_json::from_str::<Light>("{}").unwrap(), Light::default(), "missing fields get defaults");
    }

    // ---------- in a whole document ----------

    use crate::doc::{Document, LayerKind, TILE};
    use eframe::egui::Color32;

    /// A page with a dome of depth across the tile border at x = 256, and a light layer above it.
    fn lit_doc(dir: [f32; 2]) -> Document {
        let mut d = Document::new(600, 300);
        let paper = d.layers.iter().position(|l| l.kind == LayerKind::Paper).unwrap();
        d.layers[paper].visible = false;
        let mut depth = d.new_layer("Depth", LayerKind::Raster);
        depth.depth = true;
        let top = d.layers[0].id;
        let depth_id = d.insert_layer(depth, top);
        let di = d.index_of(depth_id).unwrap();
        for y in 0..300usize {
            for x in 150..360usize {
                let r = ((x as f32 - 255.0).hypot(y as f32 - 150.0)) / 100.0;
                if r < 1.0 {
                    let v = ((1.0 - r * r).sqrt() * 255.0) as u8;
                    d.layers[di].tile_mut(((x / TILE) as i32, (y / TILE) as i32))[(y % TILE) * TILE + x % TILE] = Color32::from_gray(v);
                }
            }
        }
        let mut l = d.new_layer("Light", LayerKind::Light);
        l.light = Some(Light { dir, soften: 3.0, ..Light::default() });
        d.insert_layer(l, depth_id);
        d.take_dirty();
        d
    }

    fn px(d: &Document, x: usize, y: usize) -> Px {
        d.flatten_tile(((x / TILE) as i32, (y / TILE) as i32))[(y % TILE) * TILE + x % TILE]
    }

    #[test]
    fn a_light_layer_shades_the_depth_across_tile_borders_and_the_depth_stays_hidden() {
        let d = lit_doc([-0.6, 0.0]);
        assert_eq!(px(&d, 255, 150)[3] < 0.1, true, "the top of the dome faces up: hardly any shading, and the gray depth paint is not shown");
        let (left, right) = (px(&d, 175, 150), px(&d, 335, 150));
        assert!(left[3] > 0.3 && left[0] > left[2], "lit side: {left:?}");
        assert!(right[3] > 0.3 && right[2] > right[0], "shaded side: {right:?}");
        // the border between two tiles (x = 255 | 256) has no seam: both sides are shaded alike
        let (a, b, c) = (px(&d, 255, 90), px(&d, 256, 90), px(&d, 254, 90));
        assert!(a[3] > 0.05 && (a[3] - b[3]).abs() < (c[3] - a[3]).abs() * 2.0 + 0.01, "no seam at the tile border: {c:?} {a:?} {b:?}");
        assert_eq!(px(&d, 20, 20), CLEAR, "far from any depth: nothing");
    }

    #[test]
    fn moving_the_light_changes_the_picture_and_selecting_the_depth_layer_shows_it() {
        let mut d = lit_doc([-0.6, 0.0]);
        let before = px(&d, 175, 150);
        let li = d.layers.iter().position(|l| l.kind == LayerKind::Light).unwrap();
        d.layers[li].light.as_mut().unwrap().dir = [0.6, 0.0];
        let after = px(&d, 175, 150);
        assert!(before[0] > before[2] && after[2] > after[0], "the lit and the shaded side swap: {before:?} {after:?}");
        d.layers[li].visible = false;
        assert_eq!(px(&d, 255, 100), CLEAR, "light hidden, depth hidden: nothing on the page");
        let di = d.layers.iter().position(|l| l.depth).unwrap();
        d.depth_view = Some(d.layers[di].id);
        let g = px(&d, 255, 150);
        assert!(g[3] > 0.99 && g[0] > 0.9, "while selected the depth shows as gray, white = near: {g:?}");
    }

    #[test]
    fn light_and_depth_are_saved_and_baked_for_layered_exports() {
        let d = lit_doc([-0.6, 0.0]);
        let path = std::env::temp_dir().join(format!("seviath_light_{}.seviath", std::process::id()));
        crate::files::project::save(d.w, d.h, d.paper, &d.layers, &[], &d.meta, &path).unwrap();
        let back = crate::files::project::load(&path).unwrap();
        std::fs::remove_file(&path).ok();
        assert_eq!(back.layers.iter().find(|l| l.kind == LayerKind::Light).unwrap().light, d.layers.iter().find(|l| l.kind == LayerKind::Light).unwrap().light);
        assert!(back.layers.iter().any(|l| l.depth), "the depth flag is kept");
        assert!(px(&back, 175, 150)[3] > 0.3, "and it still shades after opening");
        let want = [px(&d, 175, 150), px(&d, 335, 150), px(&d, 20, 20)];
        let baked = crate::files::bake(d);
        assert!(baked.layers.iter().all(|l| l.kind != LayerKind::Light), "no light layer left for PSD / OpenRaster");
        assert!(baked.layers.iter().filter(|l| l.depth).all(|l| !l.visible), "the depth stays as a hidden layer");
        for (w, g) in want.iter().zip([px(&baked, 175, 150), px(&baked, 335, 150), px(&baked, 20, 20)]) {
            assert!(w.iter().zip(&g).all(|(a, b)| (a - b).abs() < 0.02), "same look after baking: {w:?} {g:?}");
        }
    }
}
