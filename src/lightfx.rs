// SPDX-License-Identifier: GPL-3.0-or-later
// Light, shadow and ray layers: paint a shape, and the light does the rest, live.
//  - Shadow: what you paint (or every layer marked "blocks light") throws a soft shadow away from
//    the scene's light. Lower light, longer shadow. Deeper places (depth layers) get darker shadow.
//  - Glow: the edges of what you paint that face the light shine and bloom: rim light and highlights
//    that sit on the lit side and move with the light.
//  - Rays: what you paint is a source of light (a window, a gap in the leaves); beams stream from it
//    away from the light, blocked by layers marked "blocks light", with streaks and dust motes.
// Move the light layer and all of them follow; nothing is baked into the paint.

use serde::{Deserialize, Serialize};

use crate::blend::{CLEAR, Px};
use crate::filters::gauss_channel;
use crate::light::Light;

#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub enum LightKind {
    Shadow,
    Glow,
    Rays,
}

impl LightKind {
    pub fn name(self) -> &'static str {
        match self {
            LightKind::Shadow => "Shadow",
            LightKind::Glow => "Glow",
            LightKind::Rays => "Rays",
        }
    }

    pub fn about(self) -> &'static str {
        match self {
            LightKind::Shadow => "What you paint here throws a soft shadow away from the light.",
            LightKind::Glow => "The edges of what you paint that face the light shine and bloom.",
            LightKind::Rays => "What you paint is a source of light: beams stream from it, blocked by layers marked \"blocks light\".",
        }
    }
}

#[derive(Clone, PartialEq, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct LightFx {
    pub kind: LightKind,
    pub strength: f32,
    /// Pixels: how far the shadow or the beams reach, or how thick the lit edge is.
    pub length: f32,
    /// Pixels: how soft it is.
    pub soft: f32,
    /// Shadow: deeper places (depth layers) get darker.
    pub depth: f32,
    /// Shadow: cast shadows of every layer marked "blocks light" too.
    pub from_solid: bool,
    /// Rays: how much the beams break up into streaks.
    pub beams: f32,
    /// Rays: how much solid layers block the beams.
    pub occlusion: f32,
    /// Rays: tiny floating specks in the beams.
    pub dust: f32,
    /// Also show the shape you painted.
    pub show_shape: bool,
}

impl Default for LightFx {
    fn default() -> Self {
        LightFx { kind: LightKind::Shadow, strength: 0.6, length: 70.0, soft: 10.0, depth: 0.5, from_solid: false, beams: 0.5, occlusion: 0.9, dust: 0.0, show_shape: false }
    }
}

pub const MAX_LENGTH: f32 = 160.0;
pub const MAX_SOFT: f32 = 40.0;
/// Farthest a shadow or beam travels (pixels): less than a tile, so a tile only needs its neighbours.
const REACH_CAP: f32 = 150.0;

impl LightFx {
    pub fn new(kind: LightKind) -> LightFx {
        let mut f = LightFx { kind, ..LightFx::default() };
        match kind {
            LightKind::Shadow => {}
            LightKind::Glow => {
                f.strength = 0.8;
                f.length = 8.0;
                f.soft = 14.0;
            }
            LightKind::Rays => {
                f.strength = 0.8;
                f.length = 150.0;
                f.soft = 6.0;
            }
        }
        f
    }

    pub fn sanitized(mut self) -> LightFx {
        let f = |v: f32, lo: f32, hi: f32| if v.is_finite() { v.clamp(lo, hi) } else { lo };
        self.strength = f(self.strength, 0.0, 1.0);
        self.length = f(self.length, 1.0, MAX_LENGTH);
        self.soft = f(self.soft, 0.0, MAX_SOFT);
        self.depth = f(self.depth, 0.0, 1.0);
        self.beams = f(self.beams, 0.0, 1.0);
        self.occlusion = f(self.occlusion, 0.0, 1.0);
        self.dust = f(self.dust, 0.0, 1.0);
        self
    }

    /// Pixels the effect needs around a tile.
    pub fn reach(&self) -> usize {
        (REACH_CAP + self.soft * 2.5).min(200.0).ceil() as usize + 4
    }
}

/// `plane` moved by (dx, dy) pixels (whole pixels, empty where it came from outside).
fn shifted(plane: &[f32], n: usize, dx: i32, dy: i32) -> Vec<f32> {
    let mut out = vec![0f32; n * n];
    for y in 0..n as i32 {
        let sy = y - dy;
        if sy < 0 || sy >= n as i32 {
            continue;
        }
        for x in 0..n as i32 {
            let sx = x - dx;
            if sx >= 0 && sx < n as i32 {
                out[(y * n as i32 + x) as usize] = plane[(sy * n as i32 + sx) as usize];
            }
        }
    }
    out
}

/// The union of `plane` moved along (vx, vy) all the way: a streak, made in a few doubling steps.
fn smeared(plane: &[f32], n: usize, vx: f32, vy: f32) -> Vec<f32> {
    let len = vx.hypot(vy);
    if len < 1.0 {
        return plane.to_vec();
    }
    let step = 2.0f32;
    let count = (len / step).ceil().max(1.0);
    let (ux, uy) = (vx / len * step, vy / len * step);
    let mut out = plane.to_vec();
    let mut have = 1.0f32; // copies of the shape in the union so far (0, step, 2 step …)
    while have < count {
        let (dx, dy) = ((ux * have).round() as i32, (uy * have).round() as i32);
        let s = shifted(&out, n, dx, dy);
        for (o, v) in out.iter_mut().zip(s) {
            *o = o.max(v);
        }
        have *= 2.0;
    }
    out
}

fn tint(c: [u8; 3]) -> [f32; 3] {
    [c[0] as f32 / 255.0, c[1] as f32 / 255.0, c[2] as f32 / 255.0]
}

/// Draw the effect for an `n`×`n` square. `paint`: the layer's own pixels (premultiplied). `solid`: how much
/// the layers marked "blocks light" cover each pixel (0..1). `depth`: height (0..1, white = near) if there are
/// depth layers. `origin`: where the square starts on the page (so beams and dust stay put).
pub fn render(fx: &LightFx, light: &Light, paint: &[Px], solid: &[f32], depth: Option<&[f32]>, n: usize, origin: (i32, i32)) -> Vec<Px> {
    let alpha: Vec<f32> = paint.iter().map(|p| p[3]).collect();
    let lv = light.vector();
    let flat = (lv[0] * lv[0] + lv[1] * lv[1]).sqrt();
    // the direction on the page that points toward the light (unit), if the light is not straight above
    let toward = if flat > 0.02 { (lv[0] / flat, lv[1] / flat) } else { (0.0, -1.0) };
    let mut out = vec![CLEAR; n * n];
    match fx.kind {
        LightKind::Shadow => {
            let mut caster = alpha.clone();
            if fx.from_solid {
                for (c, s) in caster.iter_mut().zip(solid) {
                    *c = c.max(*s);
                }
            }
            // away from the light; the lower the light, the longer the shadow
            let reach = (fx.length * (flat / lv[2].max(0.25)).clamp(0.25, 2.5)).min(REACH_CAP);
            let mut shadow = smeared(&caster, n, -toward.0 * reach, -toward.1 * reach);
            if fx.soft > 0.5 {
                shadow = gauss_channel(&shadow, n, n, fx.soft / 2.0);
            }
            let c = tint(light.shadow);
            for i in 0..n * n {
                let mut a = shadow[i] * fx.strength;
                if let Some(d) = depth {
                    // deeper places (darker in the depth layers) get a darker shadow
                    a *= 1.0 + fx.depth * (1.0 - d[i].clamp(0.0, 1.0)) * 0.8;
                }
                let a = a.clamp(0.0, 1.0);
                out[i] = [c[0] * a, c[1] * a, c[2] * a, a];
            }
        }
        LightKind::Glow => {
            // the parts of the shape whose neighbour toward the light is empty face the light
            let r = fx.length.max(1.0);
            let nb = shifted(&alpha, n, (-toward.0 * r).round() as i32, (-toward.1 * r).round() as i32);
            let edge: Vec<f32> = alpha.iter().zip(&nb).map(|(a, b)| a * (1.0 - b)).collect();
            let soft = if fx.soft > 0.5 { gauss_channel(&edge, n, n, fx.soft / 2.0) } else { edge.clone() };
            let c = tint(light.highlight);
            for i in 0..n * n {
                // a bright core on the edge itself, a soft bloom around it
                let a = (soft[i] * 1.6 + edge[i] * 0.5).clamp(0.0, 1.0) * fx.strength;
                out[i] = [c[0] * a, c[1] * a, c[2] * a, a];
            }
        }
        LightKind::Rays => {
            let len = (fx.length / lv[2].max(0.3).sqrt()).min(REACH_CAP);
            let (dx, dy) = (-toward.0, -toward.1);
            let source: Vec<f32> = if fx.soft > 0.5 { gauss_channel(&alpha, n, n, fx.soft / 2.0) } else { alpha.clone() };
            // the light travels away from the source a couple of pixels at a time: at every landing it
            // is weakened a little, and stopped where it meets something solid
            let step = 2.0f32;
            let (sx, sy) = ((dx * step).round() as i32, (dy * step).round() as i32);
            let passes = ((len / step) as usize).clamp(1, 120);
            let decay = 1.0 - 1.0 / (passes as f32 * 1.5);
            let mut beam = source.clone();
            let mut cur = source.clone();
            for _ in 0..passes {
                cur = shifted(&cur, n, sx, sy);
                let mut any = 0.0f32;
                for i in 0..n * n {
                    cur[i] *= decay * (1.0 - (solid[i] * fx.occlusion).clamp(0.0, 1.0));
                    beam[i] += cur[i];
                    any = any.max(cur[i]);
                }
                if any < 0.004 {
                    break;
                }
            }
            let norm = 7.0 / passes as f32;
            let c = tint(light.highlight);
            let (px, py) = (-dy, dx); // across the beams
            for i in 0..n * n {
                let (x, y) = (origin.0 + (i % n) as i32, origin.1 + (i / n) as i32);
                let mut v = beam[i] * norm;
                if fx.beams > 0.0 {
                    // streaks: the beams break into bright and dark lanes, fixed on the page
                    let across = x as f32 * px + y as f32 * py;
                    let lane = crate::brush::noise::noise(across / (6.0 + fx.soft), 0.5, 77) * 0.7 + crate::brush::noise::noise(across / (2.5 + fx.soft * 0.4), 3.5, 78) * 0.3;
                    v *= 1.0 - fx.beams * (1.0 - lane * 1.6).clamp(0.0, 1.0);
                }
                // light does not shine through the solid things it hits
                v *= 1.0 - (solid[i] * fx.occlusion).clamp(0.0, 1.0);
                let mut a = (v * fx.strength * 1.4).clamp(0.0, 1.0);
                if fx.dust > 0.0 && v > 0.05 {
                    let h = crate::brush::noise::hash(x.div_euclid(3), y.div_euclid(3), 91);
                    if h > 1.0 - 0.02 * fx.dust {
                        a = (a + 0.6 * v.min(1.0)).min(1.0);
                    }
                }
                out[i] = [c[0] * a, c[1] * a, c[2] * a, a];
            }
        }
    }
    if fx.show_shape {
        if fx.kind == LightKind::Shadow {
            // the object stands on top of its shadow
            for (o, p) in out.iter_mut().zip(paint) {
                if p[3] > 0.0 {
                    let k = 1.0 - p[3];
                    *o = [p[0] + o[0] * k, p[1] + o[1] * k, p[2] + o[2] * k, p[3] + o[3] * k];
                }
            }
        } else {
            // the light shines on top of the object
            for (o, p) in out.iter_mut().zip(paint) {
                let k = 1.0 - o[3];
                *o = [o[0] + p[0] * k, o[1] + p[1] * k, o[2] + p[2] * k, o[3] + p[3] * k];
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    const N: usize = 161;

    /// A solid square in the middle.
    fn square(half: usize) -> Vec<Px> {
        (0..N * N)
            .map(|i| {
                let (x, y) = (i % N, i / N);
                if x.abs_diff(80) <= half && y.abs_diff(80) <= half { [0.2, 0.2, 0.2, 1.0] } else { CLEAR }
            })
            .collect()
    }

    fn lit(dir: [f32; 2]) -> Light {
        Light { dir, ..Light::default() }
    }

    fn centre_of_mass(img: &[Px]) -> (f32, f32, f32) {
        let (mut sx, mut sy, mut s) = (0.0, 0.0, 0.0);
        for (i, p) in img.iter().enumerate() {
            sx += (i % N) as f32 * p[3];
            sy += (i / N) as f32 * p[3];
            s += p[3];
        }
        if s > 0.0 { (sx / s, sy / s, s) } else { (0.0, 0.0, 0.0) }
    }

    fn render_with(kind: LightKind, f: impl FnOnce(&mut LightFx), dir: [f32; 2], paint: &[Px], solid: &[f32]) -> Vec<Px> {
        let mut fx = LightFx::new(kind);
        f(&mut fx);
        render(&fx, &lit(dir), paint, solid, None, N, (0, 0))
    }

    #[test]
    fn a_shadow_falls_away_from_the_light_and_grows_as_the_light_gets_low() {
        let none = vec![0.0; N * N];
        let from_left = render_with(LightKind::Shadow, |_| {}, [-0.6, 0.0], &square(15), &none);
        let from_right = render_with(LightKind::Shadow, |_| {}, [0.6, 0.0], &square(15), &none);
        let (lx, _, _) = centre_of_mass(&from_left);
        let (rx, _, _) = centre_of_mass(&from_right);
        assert!(lx > 85.0 && rx < 75.0, "the shadow is thrown away from the light: {lx} {rx}");
        // a low light throws a longer shadow than a high one
        let high = render_with(LightKind::Shadow, |_| {}, [-0.15, 0.0], &square(10), &none);
        let low = render_with(LightKind::Shadow, |_| {}, [-0.9, 0.0], &square(10), &none);
        assert!(centre_of_mass(&low).2 > centre_of_mass(&high).2 * 1.2, "{} vs {}", centre_of_mass(&low).2, centre_of_mass(&high).2);
        // nothing painted, nothing thrown
        assert_eq!(centre_of_mass(&render_with(LightKind::Shadow, |_| {}, [-0.6, 0.0], &vec![CLEAR; N * N], &none)).2, 0.0);
    }

    #[test]
    fn solid_layers_cast_shadows_and_deep_places_get_darker_shadow() {
        let mut solid = vec![0.0; N * N];
        for i in 0..N * N {
            if (i % N).abs_diff(80) <= 12 && (i / N).abs_diff(80) <= 12 {
                solid[i] = 1.0;
            }
        }
        let empty = vec![CLEAR; N * N];
        let own = render_with(LightKind::Shadow, |f| f.from_solid = false, [-0.6, 0.0], &empty, &solid);
        let cast = render_with(LightKind::Shadow, |f| f.from_solid = true, [-0.6, 0.0], &empty, &solid);
        assert_eq!(centre_of_mass(&own).2, 0.0, "off: only what is painted on this layer casts");
        assert!(centre_of_mass(&cast).2 > 10.0, "on: the solid layer casts its shadow");
        let mut fx = LightFx::new(LightKind::Shadow);
        fx.depth = 1.0;
        let paint = square(15);
        let near = vec![1.0; N * N];
        let far = vec![0.0; N * N];
        let a = render(&fx, &lit([-0.6, 0.0]), &paint, &vec![0.0; N * N], Some(&near), N, (0, 0));
        let b = render(&fx, &lit([-0.6, 0.0]), &paint, &vec![0.0; N * N], Some(&far), N, (0, 0));
        assert!(centre_of_mass(&b).2 > centre_of_mass(&a).2 * 1.3, "deeper = darker shadow: {} vs {}", centre_of_mass(&b).2, centre_of_mass(&a).2);
    }

    #[test]
    fn a_glow_sits_on_the_lit_edge_and_moves_with_the_light() {
        let none = vec![0.0; N * N];
        let left = render_with(LightKind::Glow, |_| {}, [-0.7, 0.0], &square(25), &none);
        let right = render_with(LightKind::Glow, |_| {}, [0.7, 0.0], &square(25), &none);
        assert!(centre_of_mass(&left).0 < 78.0 && centre_of_mass(&right).0 > 82.0, "{} {}", centre_of_mass(&left).0, centre_of_mass(&right).0);
        // it glows past the edge (bloom) and keeps the middle of the shape dark
        assert!(left[80 * N + 53][3] > 0.02, "a halo just outside the lit edge");
        assert!(left[80 * N + 80][3] < 0.2, "the middle is not lit");
    }

    #[test]
    fn rays_stream_away_from_the_light_and_solid_layers_block_them() {
        let none = vec![0.0; N * N];
        // a small bright window
        let mut window = vec![CLEAR; N * N];
        for i in 0..N * N {
            if (i % N).abs_diff(60) <= 6 && (i / N).abs_diff(80) <= 10 {
                window[i] = [1.0, 1.0, 1.0, 1.0];
            }
        }
        let rays = render_with(LightKind::Rays, |f| f.beams = 0.0, [-0.7, 0.0], &window, &none);
        let (cx, _, total) = centre_of_mass(&rays);
        assert!(cx > 70.0 && total > 30.0, "the beams run to the right, away from the light on the left: {cx} {total}");
        let lit_far = rays[80 * N + 120][3];
        assert!(lit_far > 0.02, "and reach well past the window: {lit_far}");
        // a wall in the way
        let mut wall = vec![0.0; N * N];
        for i in 0..N * N {
            if (i % N).abs_diff(90) <= 3 {
                wall[i] = 1.0;
            }
        }
        let blocked = render_with(LightKind::Rays, |f| f.beams = 0.0, [-0.7, 0.0], &window, &wall);
        assert!(blocked[80 * N + 120][3] < lit_far * 0.5, "the wall blocks the beams: {} vs {lit_far}", blocked[80 * N + 120][3]);
        // dust and streaks change the picture without breaking it
        let dusty = render_with(LightKind::Rays, |f| f.dust = 1.0, [-0.7, 0.0], &window, &none);
        let streaky = render_with(LightKind::Rays, |f| f.beams = 1.0, [-0.7, 0.0], &window, &none);
        assert!(dusty != rays && streaky != rays);
        assert!(streaky.iter().all(|p| p[3] >= 0.0 && p[3] <= 1.0));
    }

    #[test]
    fn showing_the_shape_and_settings_in_range() {
        let none = vec![0.0; N * N];
        let shown = render_with(LightKind::Glow, |f| f.show_shape = true, [-0.7, 0.0], &square(25), &none);
        assert!((shown[80 * N + 80][3] - 1.0).abs() < 1e-5, "the shape is drawn too");
        let wild = LightFx { strength: 9.0, length: f32::NAN, soft: 1e9, ..LightFx::default() }.sanitized();
        assert_eq!((wild.strength, wild.length, wild.soft), (1.0, 1.0, MAX_SOFT));
        assert!(LightFx::new(LightKind::Rays).reach() <= 204);
        let j = serde_json::to_string(&LightFx::new(LightKind::Rays)).unwrap();
        assert_eq!(serde_json::from_str::<LightFx>(&j).unwrap(), LightFx::new(LightKind::Rays));
    }

    // ---------- in a whole document ----------

    use crate::doc::{Document, LayerKind, TILE};
    use eframe::egui::Color32;

    fn rect(d: &mut Document, i: usize, x0: usize, y0: usize, x1: usize, y1: usize, c: Color32) {
        for y in y0..y1 {
            for x in x0..x1 {
                d.layers[i].tile_mut(((x / TILE) as i32, (y / TILE) as i32))[(y % TILE) * TILE + x % TILE] = c;
            }
        }
    }

    fn px(d: &Document, x: usize, y: usize) -> Px {
        d.flatten_tile(((x / TILE) as i32, (y / TILE) as i32))[(y % TILE) * TILE + x % TILE]
    }

    fn scene(kind: LightKind) -> Document {
        let mut d = Document::new(700, 400);
        // a light from the left, and the effect layer holding a box that straddles a tile border
        let mut l = d.new_layer("Light", LayerKind::Light);
        l.light = Some(Light { dir: [-0.6, 0.0], ..Default::default() });
        d.insert_layer(l, 1);
        let mut e = d.new_layer("Effect", LayerKind::Raster);
        e.lightfx = Some(LightFx::new(kind));
        let top = d.layers[0].id;
        d.insert_layer(e, top);
        let ei = d.layers.iter().position(|l| l.lightfx.is_some()).unwrap();
        rect(&mut d, ei, 230, 150, 290, 260, Color32::from_rgb(40, 40, 40));
        d.take_dirty();
        d
    }

    #[test]
    fn a_shadow_layer_throws_its_shadow_across_the_tile_border_and_the_paint_stays_untouched() {
        let d = scene(LightKind::Shadow);
        let (inside, shadow, far) = (px(&d, 260, 200), px(&d, 330, 220), px(&d, 600, 220));
        let _ = inside;
        // the paper is white: the shadow darkens it
        assert!(shadow[0] < far[0] - 0.1, "darker than the paper: {shadow:?} vs {far:?}");
        let ei = d.layers.iter().position(|l| l.lightfx.is_some()).unwrap();
        assert_eq!(d.layers[ei].tiles.len(), 4, "the painted layer is the box only (four tiles): nothing was added");
        // across the tile border (x = 255 | 256) the shadow has no seam
        let (a, b) = (px(&d, 255, 270), px(&d, 256, 270));
        assert!((a[0] - b[0]).abs() < 0.03, "no seam: {a:?} {b:?}");
    }

    #[test]
    fn marking_a_layer_solid_makes_it_cast_and_moving_the_light_moves_the_shadow() {
        let mut d = Document::new(700, 400);
        let mut l = d.new_layer("Light", LayerKind::Light);
        l.light = Some(Light { dir: [-0.6, 0.0], ..Default::default() });
        d.insert_layer(l, 1);
        let mut e = d.new_layer("Shadow", LayerKind::Raster);
        e.lightfx = Some(LightFx { from_solid: true, ..LightFx::new(LightKind::Shadow) });
        let top = d.layers[0].id;
        d.insert_layer(e, top);
        // a tree layer that blocks light, below the effect
        let tree = d.new_layer("Tree", LayerKind::Raster);
        let ti = d.layers.len() - 1;
        d.insert_layer(tree, d.layers[ti].id);
        let ti = d.layers.iter().position(|l| l.name == "Tree").unwrap();
        rect(&mut d, ti, 300, 100, 340, 250, Color32::from_rgb(20, 80, 30));
        assert!(px(&d, 400, 180)[3] < 0.05 || px(&d, 400, 180)[0] > 0.9, "not solid yet: no shadow on the paper");
        d.layers[ti].solid = true;
        let from_left = px(&d, 380, 180);
        assert!(from_left[0] < 0.92, "now it casts a shadow to its right: {from_left:?}");
        let li = d.layers.iter().position(|l| l.kind == LayerKind::Light).unwrap();
        d.layers[li].light.as_mut().unwrap().dir = [0.6, 0.0];
        let (right_side, left_side) = (px(&d, 380, 180), px(&d, 260, 180));
        assert!(right_side[0] > from_left[0] + 0.05 && left_side[0] < 0.92, "light from the right: the shadow moves to the left of the tree: {right_side:?} {left_side:?}");
    }

    #[test]
    fn light_effects_are_saved_and_baked() {
        let d = scene(LightKind::Glow);
        let path = std::env::temp_dir().join(format!("seviath_lfx_{}.seviath", std::process::id()));
        crate::files::project::save(d.w, d.h, d.paper, &d.layers, &[], &d.meta, &path).unwrap();
        let back = crate::files::project::load(&path).unwrap();
        std::fs::remove_file(&path).ok();
        let (a, b) = (d.layers.iter().find(|l| l.lightfx.is_some()).unwrap(), back.layers.iter().find(|l| l.lightfx.is_some()).unwrap());
        assert_eq!(a.lightfx, b.lightfx);
        let mut marked = d;
        let ei = marked.layers.iter().position(|l| l.lightfx.is_some()).unwrap();
        marked.layers[ei].solid = true;
        let want = [px(&marked, 232, 200), px(&marked, 226, 200), px(&marked, 500, 300)];
        let baked = crate::files::bake(marked);
        assert!(baked.layers.iter().all(|l| l.lightfx.is_none()), "no light effect left for PSD / OpenRaster");
        for (w, g) in want.iter().zip([px(&baked, 232, 200), px(&baked, 226, 200), px(&baked, 500, 300)]) {
            assert!(w.iter().zip(&g).all(|(a, b)| (a - b).abs() < 0.03), "same look after baking: {w:?} vs {g:?}");
        }
    }

    /// Shadow, glow and rays on small scenes: target/screens/lightfx_sheet.png
    #[test]
    fn lightfx_sheet() {
        let (w, h) = (520usize, 330usize);
        let mut sheet = image::RgbaImage::from_pixel((w * 3) as u32, h as u32, image::Rgba([255, 255, 255, 255]));
        for (k, kind) in [LightKind::Shadow, LightKind::Glow, LightKind::Rays].into_iter().enumerate() {
            let mut d = Document::new(w as u32, h as u32);
            d.paper = Color32::from_rgb(236, 226, 205);
            let mut l = d.new_layer("Light", LayerKind::Light);
            l.light = Some(Light { dir: [-0.65, -0.3], ..Default::default() });
            d.insert_layer(l, 1);
            // a wall with a window gap (blocks light), and the effect layer
            let wall = d.new_layer("Wall", LayerKind::Raster);
            let top = d.layers[0].id;
            d.insert_layer(wall, top);
            let mut e = d.new_layer("Effect", LayerKind::Raster);
            e.lightfx = Some(LightFx { show_shape: kind != LightKind::Rays, ..LightFx::new(kind) });
            let top = d.layers[0].id;
            d.insert_layer(e, top);
            let ei = d.layers.iter().position(|l| l.lightfx.is_some()).unwrap();
            let wi = d.layers.iter().position(|l| l.name == "Wall").unwrap();
            match kind {
                LightKind::Shadow => rect(&mut d, ei, 120, 120, 190, 250, Color32::from_rgb(70, 60, 80)),
                LightKind::Glow => {
                    for y in 0..h {
                        for x in 0..w {
                            if ((x as f32 - 260.0).hypot(y as f32 - 165.0)) < 95.0 {
                                d.layers[ei].tile_mut(((x / TILE) as i32, (y / TILE) as i32))[(y % TILE) * TILE + x % TILE] = Color32::from_rgb(50, 40, 70);
                            }
                        }
                    }
                }
                LightKind::Rays => {
                    rect(&mut d, ei, 130, 60, 160, 150, Color32::from_rgb(255, 245, 210));
                    rect(&mut d, wi, 300, 0, 330, 320, Color32::from_rgb(40, 50, 40));
                    d.layers[wi].solid = true;
                }
            }
            let img = crate::files::flatten(&d);
            for y in 0..h {
                for x in 0..w {
                    sheet.put_pixel((k * w + x) as u32, y as u32, *img.get_pixel(x as u32, y as u32));
                }
            }
        }
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("target").join("screens");
        std::fs::create_dir_all(&dir).unwrap();
        sheet.save(dir.join("lightfx_sheet.png")).unwrap();
    }
}
