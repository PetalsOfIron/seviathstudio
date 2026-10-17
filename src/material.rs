// SPDX-License-Identifier: GPL-3.0-or-later
// Material layers: paint a shape, say what it is made of. The layer's paint is the base color and
// the shape; metal, gloss, velvet, neon, glass or a cut gem is worked out live from the shape's
// own roundness (or the depth layers below) and the scene's light: move the light and every
// highlight, rim and glow follows. Nothing is baked into the paint.
//
// Our own simple 2D model with the familiar parameter names: metallic, roughness, anisotropy,
// fresnel, clear coat, sheen, emission with bloom, transmission, faceting.

use serde::{Deserialize, Serialize};

use crate::blend::{CLEAR, Px};
use crate::filters::gauss_channel;
use crate::light::Light;

#[derive(Clone, PartialEq, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct Material {
    pub name: String,
    /// Use this color instead of the paint's own (gold stays gold whatever color you paint).
    pub base: Option<[u8; 3]>,
    pub metallic: f32,
    pub roughness: f32,
    pub aniso: f32,
    /// Degrees: the direction of the brushing.
    pub aniso_angle: f32,
    pub fresnel: f32,
    pub coat: f32,
    pub sheen: f32,
    pub emission: f32,
    /// Glow color (None = the paint's color).
    pub emission_color: Option<[u8; 3]>,
    /// Pixels: how far the glow spreads.
    pub bloom: f32,
    pub transmission: f32,
    pub tint: [u8; 3],
    /// Cut facets (0 = smooth, 3..12 = faceted like a gem).
    pub facets: u32,
    /// Pixels: the radius of the rounded edge made from the shape's outline (0 = automatic: the shape
    /// is as round as its thickest part allows, so a big shape is one big sphere).
    pub round: f32,
    /// How strong the surface relief is (dome and depth layers).
    pub bump: f32,
    /// How much of the sky and ground a shiny surface mirrors.
    pub reflect: f32,
}

impl Default for Material {
    fn default() -> Self {
        Material {
            name: "Plastic".into(),
            base: None,
            metallic: 0.0,
            roughness: 0.35,
            aniso: 0.0,
            aniso_angle: 0.0,
            fresnel: 0.5,
            coat: 0.0,
            sheen: 0.0,
            emission: 0.0,
            emission_color: None,
            bloom: 16.0,
            transmission: 0.0,
            tint: [190, 220, 255],
            facets: 0,
            round: 0.0,
            bump: 60.0,
            reflect: 0.5,
        }
    }
}

pub const MAX_ROUND: f32 = 600.0;
pub const MAX_BLOOM: f32 = 60.0;
pub const MAX_BUMP: f32 = 200.0;

fn m(name: &str, f: impl FnOnce(&mut Material)) -> Material {
    let mut x = Material { name: name.into(), ..Material::default() };
    f(&mut x);
    x
}

/// The ready materials.
pub fn presets() -> Vec<Material> {
    vec![
        m("Plastic", |_| {}),
        m("Chrome", |x| {
            x.base = Some([235, 238, 245]);
            x.metallic = 1.0;
            x.roughness = 0.04;
            x.fresnel = 1.0;
            x.reflect = 1.0;
        }),
        m("Gold", |x| {
            x.base = Some([255, 196, 80]);
            x.metallic = 1.0;
            x.roughness = 0.22;
            x.fresnel = 0.7;
            x.reflect = 0.8;
        }),
        m("Copper", |x| {
            x.base = Some([228, 126, 92]);
            x.metallic = 1.0;
            x.roughness = 0.28;
            x.fresnel = 0.6;
            x.reflect = 0.75;
        }),
        m("Steel", |x| {
            x.base = Some([188, 194, 204]);
            x.metallic = 1.0;
            x.roughness = 0.35;
            x.fresnel = 0.5;
            x.reflect = 0.6;
        }),
        m("Brushed metal", |x| {
            x.base = Some([190, 196, 206]);
            x.metallic = 1.0;
            x.roughness = 0.4;
            x.aniso = 0.9;
            x.fresnel = 0.4;
            x.reflect = 0.35;
        }),
        m("Car paint", |x| {
            x.metallic = 0.55;
            x.roughness = 0.3;
            x.coat = 1.0;
            x.fresnel = 0.8;
            x.reflect = 0.7;
        }),
        m("Velvet", |x| {
            x.roughness = 0.85;
            x.sheen = 1.0;
            x.fresnel = 0.2;
            x.reflect = 0.0;
        }),
        m("Neon", |x| {
            x.emission = 1.4;
            x.bloom = 26.0;
            x.roughness = 0.6;
            x.reflect = 0.0;
        }),
        m("Glass", |x| {
            x.base = Some([200, 225, 245]);
            x.transmission = 0.92;
            x.roughness = 0.05;
            x.fresnel = 1.0;
            x.reflect = 0.9;
        }),
        m("Gem", |x| {
            x.transmission = 0.55;
            x.facets = 7;
            x.roughness = 0.08;
            x.fresnel = 1.0;
            x.coat = 0.5;
            x.reflect = 0.9;
            x.bump = 110.0;
        }),
    ]
}

impl Material {
    pub fn sanitized(mut self) -> Material {
        let f = |v: f32, lo: f32, hi: f32| if v.is_finite() { v.clamp(lo, hi) } else { lo };
        self.name = self.name.chars().take(40).collect();
        self.metallic = f(self.metallic, 0.0, 1.0);
        self.roughness = f(self.roughness, 0.0, 1.0);
        self.aniso = f(self.aniso, 0.0, 1.0);
        self.aniso_angle = f(self.aniso_angle, -360.0, 360.0);
        self.fresnel = f(self.fresnel, 0.0, 1.0);
        self.coat = f(self.coat, 0.0, 1.0);
        self.sheen = f(self.sheen, 0.0, 1.0);
        self.emission = f(self.emission, 0.0, 3.0);
        self.bloom = f(self.bloom, 0.0, MAX_BLOOM);
        self.transmission = f(self.transmission, 0.0, 1.0);
        self.facets = self.facets.min(16);
        self.round = f(self.round, 0.0, MAX_ROUND);
        self.bump = f(self.bump, 0.0, MAX_BUMP);
        self.reflect = f(self.reflect, 0.0, 1.0);
        self
    }

    /// Pixels the shading needs around a tile.
    pub fn reach(&self) -> usize {
        (self.bloom * 2.2).ceil() as usize + 6
    }
}

fn norm(v: [f32; 3]) -> [f32; 3] {
    let l = (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt().max(1e-6);
    [v[0] / l, v[1] / l, v[2] / l]
}

fn dot(a: [f32; 3], b: [f32; 3]) -> f32 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

/// Sky above, ground below, a bright line at the horizon: what a shiny surface mirrors.
fn environment(up: f32) -> [f32; 3] {
    let s = ((up + 0.35) / 1.0).clamp(0.0, 1.0);
    let s = s * s * (3.0 - 2.0 * s);
    let ground = [0.3, 0.26, 0.22];
    let sky = [0.72, 0.84, 1.0];
    let horizon = (-(up * up) / 0.01).exp() * 0.5;
    [0, 1, 2].map(|i| ground[i] + (sky[i] - ground[i]) * s + horizon)
}

fn luma(c: [f32; 3]) -> f32 {
    0.2126 * c[0] + 0.7152 * c[1] + 0.0722 * c[2]
}

/// Shade the paint in an `n`×`n` square (premultiplied pixels). `depth`: extra height from depth
/// layers (0..1, white = near), same size.
#[cfg(test)]
pub fn shade(mat: &Material, light: &Light, paint: &[Px], depth: Option<&[f32]>, n: usize) -> Vec<Px> {
    let alpha: Vec<f32> = paint.iter().map(|p| p[3]).collect();
    let height = dome(mat, &alpha, n);
    shade_with_height(mat, light, paint, height, depth, n)
}

/// The shape's own roundness: its outline is inflated into a rounded bevel (a full sphere or tube
/// when the shape is no wider than twice the bevel radius), like a cushion of the same shape.
/// This works on one square of pixels; whole layers use `HeightField`.
#[cfg(test)]
pub fn dome(mat: &Material, alpha: &[f32], n: usize) -> Vec<f32> {
    let outside: Vec<bool> = alpha.iter().map(|a| *a < 0.5).collect();
    let dist = crate::effects::distance_to(&outside, n);
    let thickest = dist.iter().zip(alpha).filter(|(_, a)| **a >= 0.5).map(|(d, _)| *d).fold(0.0f32, f32::max);
    let radius = if mat.round <= 0.0 { thickest.max(2.0) } else { (mat.round * 1.5).max(2.0) };
    let height: Vec<f32> = dist.iter().zip(alpha).map(|(d, a)| bevel(radius, mat.bump / 60.0, *d, *a)).collect();
    gauss_channel(&height, n, n, 3.0)
}

/// Height of the surface at distance `d` (pixels) from the edge of the shape.
fn bevel(radius: f32, relief: f32, d: f32, alpha: f32) -> f32 {
    if alpha < 0.002 {
        0.0
    } else if d >= radius {
        radius * relief
    } else {
        (radius * radius - (radius - d.max(0.5)).powi(2)).max(0.0).sqrt() * relief
    }
}

/// The whole layer's relief at a coarse resolution, so a big shape can be one smooth sphere however
/// large it is (the tile by tile shading only sees a window around each tile).
pub struct HeightField {
    k: usize,
    w: usize,
    h: usize,
    data: Vec<f32>,
    /// What it was made from: the layer's revision and the settings that shape it.
    pub key: (u64, u32, u32, u32, u32),
}

impl HeightField {
    pub fn key_of(rev: u64, mat: &Material, w: u32, h: u32) -> (u64, u32, u32, u32, u32) {
        (rev, mat.round.to_bits(), mat.bump.to_bits(), w, h)
    }

    /// `alpha(x, y)`: coverage 0..1 of the layer at a document pixel.
    pub fn build(alpha: impl Fn(i32, i32) -> f32, doc_w: u32, doc_h: u32, mat: &Material, key: (u64, u32, u32, u32, u32)) -> HeightField {
        let k = (doc_w.max(doc_h) as usize).div_ceil(1024).max(1);
        let (w, h) = ((doc_w as usize).div_ceil(k), (doc_h as usize).div_ceil(k));
        let n = w.max(h);
        let mut a = vec![0f32; n * n];
        for y in 0..h {
            for x in 0..w {
                let (bx, by) = ((x * k) as i32, (y * k) as i32);
                let q = k as i32;
                // a few samples inside the block
                let mut s = alpha(bx + q / 2, by + q / 2);
                if k > 1 {
                    s = (s * 2.0 + alpha(bx, by) + alpha(bx + q - 1, by) + alpha(bx, by + q - 1) + alpha(bx + q - 1, by + q - 1)) / 6.0;
                }
                a[y * n + x] = s;
            }
        }
        // outside the page nothing ends: those pixels are not "outside the shape"
        let outside: Vec<bool> = (0..n * n).map(|i| (i % n) < w && (i / n) < h && a[i] < 0.5).collect();
        let dist = crate::effects::distance_to(&outside, n);
        let thickest = (0..n * n).filter(|&i| a[i] >= 0.5 && outside.get(i) == Some(&false)).map(|i| dist[i]).fold(0.0f32, f32::max) * k as f32;
        let radius = if mat.round <= 0.0 { thickest.max(2.0) } else { (mat.round * 1.5).max(2.0) };
        let relief = mat.bump / 60.0;
        let mut data = vec![0f32; w * h];
        for y in 0..h {
            for x in 0..w {
                let i = y * n + x;
                data[y * w + x] = bevel(radius, relief, dist[i] * k as f32, a[i]);
            }
        }
        let sigma = (6.0 / k as f32).max(1.2);
        let data = gauss_channel(&data, w, h, sigma);
        HeightField { k, w, h, data, key }
    }

    /// Height at a document position (bilinear).
    pub fn at(&self, x: f32, y: f32) -> f32 {
        let (fx, fy) = ((x / self.k as f32 - 0.5).clamp(0.0, self.w as f32 - 1.0), (y / self.k as f32 - 0.5).clamp(0.0, self.h as f32 - 1.0));
        let (x0, y0) = (fx.floor() as usize, fy.floor() as usize);
        let (x1, y1) = ((x0 + 1).min(self.w - 1), (y0 + 1).min(self.h - 1));
        let (tx, ty) = (fx - x0 as f32, fy - y0 as f32);
        let d = |xx: usize, yy: usize| self.data[yy * self.w + xx];
        let top = d(x0, y0) + (d(x1, y0) - d(x0, y0)) * tx;
        let bot = d(x0, y1) + (d(x1, y1) - d(x0, y1)) * tx;
        top + (bot - top) * ty
    }
}

/// Shade with the relief already worked out (`height`: surface height in pixels, `n`×`n`).
pub fn shade_with_height(mat: &Material, light: &Light, paint: &[Px], mut height: Vec<f32>, depth: Option<&[f32]>, n: usize) -> Vec<Px> {
    let alpha: Vec<f32> = paint.iter().map(|p| p[3]).collect();
    if let Some(d) = depth {
        let soft = gauss_channel(d, n, n, 3.0);
        for (h, s) in height.iter_mut().zip(soft) {
            *h += s * mat.bump * 1.5;
        }
    }
    let lv = light.vector();
    let t = mat.aniso_angle.to_radians();
    let tangent = [t.cos(), t.sin(), 0.0];
    let alpha_r = (mat.roughness * mat.roughness).max(0.004);
    let (ax, ay) = (alpha_r * (1.0 - 0.7 * mat.aniso), alpha_r * (1.0 + 5.0 * mat.aniso));
    let view = [0.0, 0.0, 1.0];
    let half = norm([lv[0] + view[0], lv[1] + view[1], lv[2] + view[2]]);
    let mut out = vec![CLEAR; n * n];
    let mut glow = vec![0f32; n * n * 3];
    let sectors = mat.facets as f32;
    for y in 1..n - 1 {
        for x in 1..n - 1 {
            let k = y * n + x;
            let a = alpha[k];
            if a <= 0.002 {
                continue;
            }
            let p = paint[k];
            let own = [(p[0] / a).clamp(0.0, 1.0), (p[1] / a).clamp(0.0, 1.0), (p[2] / a).clamp(0.0, 1.0)];
            let albedo = match mat.base {
                Some(b) => [b[0] as f32 / 255.0, b[1] as f32 / 255.0, b[2] as f32 / 255.0],
                None => own,
            };
            // the surface direction from the relief
            let (mut gx, mut gy) = ((height[k + 1] - height[k - 1]) * 0.5, (height[k + n] - height[k - n]) * 0.5);
            if sectors >= 3.0 {
                // cut facets: the direction snaps to a few flat faces, and the tilt to a few steps
                let ang = gy.atan2(gx);
                let snapped = (ang / std::f32::consts::TAU * sectors).round() / sectors * std::f32::consts::TAU;
                let mag = gx.hypot(gy);
                let step = ((mag * 3.0).ceil() / 3.0).min(1.2);
                gx = snapped.cos() * step * (mag > 0.01) as u8 as f32;
                gy = snapped.sin() * step * (mag > 0.01) as u8 as f32;
            }
            let nrm = norm([-gx, -gy, 1.0]);
            let nl = dot(nrm, lv).max(0.0);
            let nv = nrm[2].max(0.02);
            let nh = dot(nrm, half).max(0.02);
            // a stretched, elongated highlight for brushed surfaces
            // the brushing direction lies in the surface, so tilt it with the surface
            let tn = dot(tangent, nrm);
            let tl = norm([tangent[0] - nrm[0] * tn, tangent[1] - nrm[1] * tn, tangent[2] - nrm[2] * tn]);
            let bl = [nrm[1] * tl[2] - nrm[2] * tl[1], nrm[2] * tl[0] - nrm[0] * tl[2], nrm[0] * tl[1] - nrm[1] * tl[0]];
            let ht = dot(half, tl);
            let hb = dot(half, bl);
            let exponent = -((ht / ax).powi(2) + (hb / ay).powi(2)) / (nh * nh);
            let lobe = exponent.exp();
            let energy = 0.35 + 0.65 * (1.0 - mat.roughness);
            let spec = lobe * nl.sqrt() * energy;
            let f0 = [0, 1, 2].map(|i| 0.04 + (albedo[i] - 0.04) * mat.metallic);
            let edge = (1.0 - nv).powi(3) * mat.fresnel;
            let diffuse = [0, 1, 2].map(|i| albedo[i] * (0.25 + 0.85 * nl) * (1.0 - mat.metallic));
            // a shiny surface mirrors the sky and the ground
            let r = norm([2.0 * nv * nrm[0], 2.0 * nv * nrm[1], 2.0 * nv * nrm[2] - 1.0]);
            // rough surfaces mirror a blurred sky: the horizon softens into the sky and ground
            let env = environment(-r[1] * (1.0 - 0.75 * mat.roughness));
            let mirror = [0, 1, 2].map(|i| env[i] * f0[i] * mat.reflect * 1.5 * (1.0 - 0.6 * mat.roughness) * (0.55 + 0.45 * edge));
            // the glint: white on plastic (artistically brighter than the physical 4%), the paint's color on metal
            let glint = [0, 1, 2].map(|i| 0.4 * (1.0 - mat.metallic) + albedo[i] * mat.metallic);
            let mut c = [0, 1, 2].map(|i| diffuse[i] + spec * (glint[i] + (1.0 - glint[i]) * edge) * 1.2 + mirror[i] + edge * 0.25 * mat.reflect * f0[i]);
            let mut flash = (spec * 1.2 * (luma(glint) + edge)).min(1.0);
            if mat.coat > 0.0 {
                // a clear film: a small sharp white highlight over everything
                let coat = ((-(1.0 - nh).powi(2) / 0.0012).exp()) * nl.sqrt() * mat.coat;
                flash = (flash + coat).min(1.0);
                for v in c.iter_mut() {
                    *v += coat * 0.9;
                }
            }
            if mat.sheen > 0.0 {
                // velvet: the rim catches light from behind
                let rim = (1.0 - nv).powi(2) * mat.sheen * (0.4 + 0.6 * nl);
                for i in 0..3 {
                    c[i] += rim * (0.35 + 0.65 * albedo[i]) * 0.8;
                }
            }
            let mut alpha_out = a;
            if mat.transmission > 0.0 {
                // glass: clear in the middle, solid at the rim and where the light flashes
                let tint = [mat.tint[0] as f32 / 255.0, mat.tint[1] as f32 / 255.0, mat.tint[2] as f32 / 255.0];
                let t = mat.transmission;
                let rim = (edge * 2.0).min(1.0);
                for i in 0..3 {
                    c[i] = c[i] * (1.0 - 0.6 * t) + tint[i] * 0.55 * t * (0.4 + 0.6 * rim);
                }
                alpha_out = a * (1.0 - t * (1.0 - rim));
            }
            if mat.emission > 0.0 {
                let ec = mat.emission_color.map(|e| [e[0] as f32 / 255.0, e[1] as f32 / 255.0, e[2] as f32 / 255.0]).unwrap_or(albedo);
                for i in 0..3 {
                    c[i] = c[i] * 0.35 + ec[i] * mat.emission;
                    glow[k * 3 + i] = ec[i] * mat.emission * a;
                }
            }
            let c = c.map(|v| v.clamp(0.0, 1.0));
            // bright parts of a see-through surface (the flash on glass) stay visible
            let alpha_out = alpha_out.max(flash * a * mat.transmission.min(1.0)).clamp(0.0, 1.0);
            out[k] = [c[0] * alpha_out, c[1] * alpha_out, c[2] * alpha_out, alpha_out];
        }
    }
    // bloom: the glow spreads past the edge of the shape
    if mat.emission > 0.0 && mat.bloom > 0.5 {
        let mut planes: Vec<Vec<f32>> = (0..3).map(|c| (0..n * n).map(|k| glow[k * 3 + c]).collect()).collect();
        for pl in planes.iter_mut() {
            *pl = gauss_channel(pl, n, n, mat.bloom / 2.0);
        }
        for k in 0..n * n {
            let g = [planes[0][k], planes[1][k], planes[2][k]];
            let s = (g[0] + g[1] + g[2]) / 3.0;
            if s <= 0.003 {
                continue;
            }
            let ga = (s * 1.6).min(1.0);
            let col = [(g[0] / s.max(1e-4)).min(1.0), (g[1] / s.max(1e-4)).min(1.0), (g[2] / s.max(1e-4)).min(1.0)];
            // over the shape's own pixels the glow adds light; outside it is a soft halo
            let o = out[k];
            let under = 1.0 - ga * 0.6;
            out[k] = [
                (o[0] * under + col[0] * ga * 0.8).min(1.0),
                (o[1] * under + col[1] * ga * 0.8).min(1.0),
                (o[2] * under + col[2] * ga * 0.8).min(1.0),
                (o[3] + ga * 0.8 * (1.0 - o[3])).min(1.0),
            ];
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    const N: usize = 121;

    /// A round blob of one color in the middle.
    fn blob(c: [f32; 3], r: f32) -> Vec<Px> {
        (0..N * N)
            .map(|i| {
                let (x, y) = ((i % N) as f32 - 60.0, (i / N) as f32 - 60.0);
                let d = x.hypot(y);
                let a = (r - d).clamp(0.0, 1.0);
                [c[0] * a, c[1] * a, c[2] * a, a]
            })
            .collect()
    }

    fn lit_from(dir: [f32; 2]) -> Light {
        Light { dir, ..Light::default() }
    }

    fn brightest(img: &[Px]) -> (usize, usize, f32) {
        let mut best = (0, 0, 0.0);
        for (i, p) in img.iter().enumerate() {
            let l = luma([p[0], p[1], p[2]]);
            let (dx, dy) = ((i % N) as f32 - 60.0, (i / N) as f32 - 60.0);
            if p[3] > 0.9 && dx.hypot(dy) < 30.0 && l > best.2 {
                best = (i % N, i / N, l);
            }
        }
        best
    }

    fn shaded(name: &str, light: &Light, paint: &[Px]) -> Vec<Px> {
        let mat = presets().into_iter().find(|p| p.name == name).unwrap();
        shade(&mat, light, paint, None, N)
    }

    #[test]
    fn the_highlight_sits_on_the_side_the_light_comes_from() {
        let paint = blob([0.8, 0.2, 0.2], 40.0);
        let left = brightest(&shaded("Plastic", &lit_from([-0.6, -0.4]), &paint));
        let right = brightest(&shaded("Plastic", &lit_from([0.6, 0.4]), &paint));
        assert!(left.0 < 60 && left.1 < 60, "light from the upper left: {left:?}");
        assert!(right.0 > 60 && right.1 > 60, "light from the lower right: {right:?}");
    }

    #[test]
    fn metal_is_tinted_by_its_own_color_and_chrome_mirrors_the_sky() {
        let paint = blob([0.5, 0.5, 0.5], 40.0);
        let gold = shaded("Gold", &Light::default(), &paint);
        let mid = gold[60 * N + 60];
        let un = |p: Px| [p[0] / p[3], p[1] / p[3], p[2] / p[3]];
        let g = un(mid);
        assert!(g[0] > g[2] + 0.1, "gold is yellow whatever the paint color: {g:?}");
        // plastic keeps the paint color, chrome has a bright top (sky) and a darker bottom (ground)
        let chrome = shaded("Chrome", &Light::default(), &paint);
        let (top, bottom) = (un(chrome[40 * N + 60]), un(chrome[82 * N + 60]));
        assert!(luma(top) > luma(bottom) + 0.15, "sky above, ground below: {top:?} {bottom:?}");
    }

    #[test]
    fn roughness_widens_the_highlight_and_brushing_stretches_it() {
        let paint = blob([0.04, 0.04, 0.04], 45.0);
        let width = |img: &[Px], axis_x: bool| {
            let (bx, by, peak) = brightest(img);
            let thr = peak * 0.5;
            (31..90).filter(|&t| {
                let (x, y) = if axis_x { (t, by) } else { (bx, t) };
                let p = img[y * N + x];
                p[3] > 0.9 && luma([p[0], p[1], p[2]]) > thr
            }).count()
        };
        let sharp = shade(&Material { roughness: 0.25, ..presets()[0].clone() }, &Light::default(), &paint, None, N);
        let rough = shade(&Material { roughness: 0.7, ..presets()[0].clone() }, &Light::default(), &paint, None, N);
        assert!(width(&rough, true) > width(&sharp, true), "a rough surface spreads the highlight: {} vs {}", width(&rough, true), width(&sharp, true));
        // brushed along x: the streak runs across it (taller than wide) and the other way round
        let brushed = |angle: f32| shade(&Material { aniso: 0.9, aniso_angle: angle, roughness: 0.4, metallic: 0.0, ..presets()[0].clone() }, &Light::default(), &paint, None, N);
        let a = brushed(0.0);
        let b = brushed(90.0);
        let (ax, ay) = (width(&a, true), width(&a, false));
        let (bx, by) = (width(&b, true), width(&b, false));
        assert!((ax as i32 - ay as i32).abs() > 3, "stretched: {ax} x {ay}");
        assert!((ax > ay) != (bx > by), "turning the brushing turns the streak: {ax}/{ay} vs {bx}/{by}");
    }

    #[test]
    fn neon_glows_past_its_edge_and_glass_is_clear_in_the_middle() {
        let paint = blob([1.0, 0.2, 0.6], 25.0);
        let neon = shaded("Neon", &Light::default(), &paint);
        assert!(neon[60 * N + 95][3] > 0.05, "a halo outside the shape (35 px from the middle): {:?}", neon[60 * N + 95]);
        assert!(neon[60 * N + 60][3] > 0.95, "and a strong core");
        let still = shade(&Material { emission: 0.0, ..presets()[0].clone() }, &Light::default(), &paint, None, N);
        assert_eq!(still[60 * N + 95][3], 0.0, "no emission, no halo");
        let glass = shaded("Glass", &Light::default(), &blob([0.8, 0.9, 1.0], 45.0));
        let (middle, rim) = (glass[60 * N + 60][3], glass[60 * N + 60 - 43][3]);
        assert!(middle < 0.5 && rim > middle + 0.1, "see-through middle, solid rim: {middle} {rim}");
    }

    #[test]
    fn a_gem_has_flat_facets_and_velvet_glows_at_the_rim() {
        let paint = blob([0.1, 0.7, 0.4], 45.0);
        let gem = shaded("Gem", &Light::default(), &paint);
        // facets: many pixels share (nearly) exactly the same color
        let mut counts = std::collections::HashMap::new();
        for p in gem.iter().filter(|p| p[3] > 0.5) {
            *counts.entry(((p[0] * 40.0) as i32, (p[1] * 40.0) as i32, (p[2] * 40.0) as i32)).or_insert(0usize) += 1;
        }
        let top: usize = { let mut v: Vec<_> = counts.values().cloned().collect(); v.sort_unstable_by(|a, b| b.cmp(a)); v.into_iter().take(14).sum() };
        let total = gem.iter().filter(|p| p[3] > 0.5).count();
        assert!(top * 10 > total * 6, "flat faces: {top} of {total} pixels in the biggest color patches");
        let smooth = shade(&Material { facets: 0, ..presets()[10].clone() }, &Light::default(), &paint, None, N);
        let mut c2 = std::collections::HashMap::new();
        for p in smooth.iter().filter(|p| p[3] > 0.5) {
            *c2.entry(((p[0] * 40.0) as i32, (p[1] * 40.0) as i32, (p[2] * 40.0) as i32)).or_insert(0usize) += 1;
        }
        let top2: usize = { let mut v: Vec<_> = c2.values().cloned().collect(); v.sort_unstable_by(|a, b| b.cmp(a)); v.into_iter().take(14).sum() };
        assert!(top > top2, "faceting concentrates the colors: {top} vs {top2}");
        let velvet = shaded("Velvet", &Light::default(), &blob([0.5, 0.1, 0.2], 45.0));
        let (middle, rim) = (luma([velvet[60 * N + 60][0], velvet[60 * N + 60][1], velvet[60 * N + 60][2]]), luma([velvet[60 * N + 17][0], velvet[60 * N + 17][1], velvet[60 * N + 17][2]]));
        assert!(rim > middle * 0.9, "the rim catches light on velvet: {rim} vs {middle}");
    }

    #[test]
    fn depth_changes_the_shape_and_settings_stay_in_range() {
        let paint = blob([0.8, 0.8, 0.8], 50.0);
        let flat = shade(&presets()[0], &Light::default(), &paint, None, N);
        let depth: Vec<f32> = (0..N * N).map(|i| (i % N) as f32 / N as f32).collect();
        let sloped = shade(&presets()[0], &Light::default(), &paint, Some(&depth), N);
        assert_ne!(flat[60 * N + 60], sloped[60 * N + 60], "the depth layers tilt the surface");
        let wild = Material { metallic: 9.0, roughness: f32::NAN, bloom: 1e9, round: -3.0, facets: 99, ..Material::default() }.sanitized();
        assert!(wild.metallic <= 1.0 && wild.roughness == 0.0 && wild.bloom == MAX_BLOOM && wild.round == 0.0 && wild.facets == 16);
        let j = serde_json::to_string(&presets()[5]).unwrap();
        assert_eq!(serde_json::from_str::<Material>(&j).unwrap(), presets()[5]);
        assert_eq!(presets().len(), 11);
    }

    // ---------- in a whole document ----------

    use crate::doc::{Document, LayerKind, TILE};
    use eframe::egui::Color32;

    /// A page with a disc of paint (across the tile border at x = 256) on a material layer.
    fn material_doc(mat: Material, color: Color32) -> Document {
        let mut d = Document::new(600, 300);
        let paper = d.layers.iter().position(|l| l.kind == LayerKind::Paper).unwrap();
        d.layers[paper].visible = false;
        for y in 0..300usize {
            for x in 150..360usize {
                let r = ((x as f32 - 255.0).hypot(y as f32 - 150.0)) / 100.0;
                if r < 1.0 {
                    d.layers[0].tile_mut(((x / TILE) as i32, (y / TILE) as i32))[(y % TILE) * TILE + x % TILE] = color;
                }
            }
        }
        d.layers[0].material = Some(mat);
        d.take_dirty();
        d
    }

    fn px(d: &Document, x: usize, y: usize) -> Px {
        d.flatten_tile(((x / TILE) as i32, (y / TILE) as i32))[(y % TILE) * TILE + x % TILE]
    }

    fn mat(name: &str) -> Material {
        presets().into_iter().find(|p| p.name == name).unwrap()
    }

    #[test]
    fn a_material_layer_turns_the_paint_into_a_shiny_shape_without_seams() {
        let d = material_doc(mat("Gold"), Color32::from_rgb(40, 60, 200));
        let c = px(&d, 235, 120);
        let un = [c[0] / c[3], c[1] / c[3], c[2] / c[3]];
        assert!(un[0] > un[2] + 0.1, "gold, although the paint is blue: {un:?}");
        assert_eq!(d.layers[0].tiles.len(), 2, "the paint itself is untouched: still just the two tiles");
        // across the border between tiles the shading carries on
        let (a, b, e) = (px(&d, 255, 90), px(&d, 256, 90), px(&d, 254, 90));
        let l = |p: Px| 0.3 * p[0] + 0.59 * p[1] + 0.11 * p[2];
        assert!((l(a) - l(b)).abs() <= (l(e) - l(a)).abs() * 2.0 + 0.03, "no seam at the tile border: {e:?} {a:?} {b:?}");
        assert_eq!(px(&d, 30, 30)[3], 0.0, "and nothing outside the shape");
    }

    #[test]
    fn moving_the_light_layer_moves_the_highlight_and_depth_reshapes_it() {
        let mut d = material_doc(mat("Plastic"), Color32::from_rgb(25, 25, 25));
        let mut lightl = d.new_layer("Light", LayerKind::Light);
        lightl.light = Some(crate::light::Light { dir: [-0.6, -0.5], ..Default::default() });
        let top = d.layers[0].id;
        d.insert_layer(lightl, top);
        let li = d.layers.iter().position(|l| l.kind == LayerKind::Light).unwrap();
        let best = |d: &Document| {
            let img = crate::files::flatten(d);
            let mut best = (0u32, 0u32, 0.0f32);
            for y in (60..240u32).step_by(2) {
                for x in (170..340u32).step_by(2) {
                    let p = img.get_pixel(x, y).0;
                    let v = 0.3 * p[0] as f32 + 0.59 * p[1] as f32 + 0.11 * p[2] as f32;
                    if p[3] > 240 && v > best.2 && ((x as f32 - 255.0).hypot(y as f32 - 150.0)) < 75.0 {
                        best = (x, y, v);
                    }
                }
            }
            best
        };
        let (x1, y1, _) = best(&d);
        d.layers[li].light.as_mut().unwrap().dir = [0.6, 0.5];
        let (x2, y2, _) = best(&d);
        assert!(x1 < 255 && y1 < 150 && x2 > 255 && y2 > 150, "the glint follows the light: ({x1},{y1}) then ({x2},{y2})");
    }

    #[test]
    fn materials_are_saved_with_the_project_and_baked_for_layered_exports() {
        let d = material_doc(mat("Gem"), Color32::from_rgb(20, 160, 90));
        let path = std::env::temp_dir().join(format!("seviath_mat_{}.seviath", std::process::id()));
        crate::files::project::save(d.w, d.h, d.paper, &d.layers, &[], &d.meta, &path).unwrap();
        let back = crate::files::project::load(&path).unwrap();
        std::fs::remove_file(&path).ok();
        assert_eq!(back.layers[0].material, d.layers[0].material);
        assert!(px(&back, 235, 120)[3] > 0.1, "it still shades after opening");
        let want = [px(&d, 235, 120), px(&d, 300, 160), px(&d, 30, 30)];
        let baked = crate::files::bake(d);
        assert!(baked.layers.iter().all(|l| l.material.is_none()), "no material left for PSD / OpenRaster");
        for (w, g) in want.iter().zip([px(&baked, 235, 120), px(&baked, 300, 160), px(&baked, 30, 30)]) {
            assert!(w.iter().zip(&g).all(|(a, b)| (a - b).abs() < 0.03), "same look after baking: {w:?} vs {g:?}");
        }
    }

    /// Every material on a sphere, side by side: target/screens/material_sheet.png
    #[test]
    fn material_sheet() {
        let all = presets();
        let (cell, cols) = (230usize, 4usize);
        let rows = all.len().div_ceil(cols);
        let mut sheet = image::RgbaImage::from_pixel((cell * cols) as u32, (cell * rows) as u32, image::Rgba([46, 48, 56, 255]));
        for (i, m) in all.iter().enumerate() {
            let mut d = Document::new(cell as u32, cell as u32);
            d.layers[1].visible = false;
            for y in 0..cell {
                for x in 0..cell {
                    if ((x as f32 - 115.0).hypot(y as f32 - 115.0)) < 85.0 {
                        let color = if m.name == "Velvet" { Color32::from_rgb(130, 20, 60) } else if m.name == "Neon" { Color32::from_rgb(255, 60, 160) } else if m.name == "Gem" { Color32::from_rgb(30, 170, 100) } else if m.name == "Car paint" { Color32::from_rgb(190, 20, 30) } else { Color32::from_rgb(190, 190, 190) };
                        d.layers[0].tile_mut(((x / TILE) as i32, (y / TILE) as i32))[(y % TILE) * TILE + x % TILE] = color;
                    }
                }
            }
            d.layers[0].material = Some(m.clone());
            let img = crate::files::flatten(&d);
            let (ox, oy) = ((i % cols) * cell, (i / cols) * cell);
            for y in 0..cell {
                for x in 0..cell {
                    let c = img.get_pixel(x as u32, y as u32).0;
                    let a = c[3] as f32 / 255.0;
                    let bg = sheet.get_pixel((ox + x) as u32, (oy + y) as u32).0;
                    let mix = |s: u8, b: u8| (s as f32 * a + b as f32 * (1.0 - a)).round() as u8;
                    sheet.put_pixel((ox + x) as u32, (oy + y) as u32, image::Rgba([mix(c[0], bg[0]), mix(c[1], bg[1]), mix(c[2], bg[2]), 255]));
                }
            }
        }
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("target").join("screens");
        std::fs::create_dir_all(&dir).unwrap();
        sheet.save(dir.join("material_sheet.png")).unwrap();
        std::fs::write(dir.join("material_sheet.txt"), all.iter().map(|m| m.name.as_str()).collect::<Vec<_>>().chunks(cols).map(|r| r.join(" | ")).collect::<Vec<_>>().join("\n")).unwrap();
    }
}
