// SPDX-License-Identifier: GPL-3.0-or-later
// Lit view of a rig picture. The flat picture is lit per pixel by one light from extra maps drawn as layers (a normal map, an emissive map, a roughness map and an
// occlusion map, each optional), plus a rim light. A picture without maps is still lit: its edges are turned into a rounded normal.

use eframe::egui::Color32;

/// The optional maps, each the size of the picture (same order of pixels).
#[derive(Default, Clone)]
pub struct Maps {
    /// Red and green are x and y of the normal (128 = flat), blue is z. y points down, like the picture.
    pub normal: Option<Vec<Color32>>,
    /// Added on top as light the part gives off itself.
    pub emissive: Option<Vec<Color32>>,
    /// Grey: 0 shiny, 1 rough.
    pub roughness: Option<Vec<Color32>>,
    /// Grey: 1 open, 0 closed in (darkens the ambient light).
    pub occlusion: Option<Vec<Color32>>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Lighting {
    /// Where the light is: x and y in -1..1 (x right, y down), the height follows from them.
    pub dir: [f32; 2],
    pub strength: f32,
    pub ambient: f32,
    /// The rim light: a bright line on the edges that face away from the viewer.
    pub rim: f32,
    pub rim_color: [u8; 3],
    /// How strong the shine is (for the parts that are not rough).
    pub shine: f32,
}

impl Default for Lighting {
    fn default() -> Self {
        Lighting { dir: [-0.5, -0.55], strength: 0.8, ambient: 0.45, rim: 0.0, rim_color: [255, 240, 220], shine: 0.35 }
    }
}

impl Lighting {
    pub fn sanitized(mut self) -> Lighting {
        let f = |v: f32, lo: f32, hi: f32| if v.is_finite() { v.clamp(lo, hi) } else { lo };
        let (x, y) = (f(self.dir[0], -1.0, 1.0), f(self.dir[1], -1.0, 1.0));
        let len = x.hypot(y);
        self.dir = if len > 0.97 { [x / len * 0.97, y / len * 0.97] } else { [x, y] };
        self.strength = f(self.strength, 0.0, 2.0);
        self.ambient = f(self.ambient, 0.0, 1.0);
        self.rim = f(self.rim, 0.0, 2.0);
        self.shine = f(self.shine, 0.0, 1.0);
        self
    }
}

fn unit(v: [f32; 3]) -> [f32; 3] {
    let l = (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt().max(1e-6);
    [v[0] / l, v[1] / l, v[2] / l]
}

/// A normal from the edges of the picture's own coverage (alpha), for a picture with no normal map: the edges round off toward the viewer.
pub fn normals_from_alpha(albedo: &[Color32], w: usize, h: usize) -> Vec<[f32; 3]> {
    let height = |x: i32, y: i32| -> f32 {
        let (x, y) = (x.clamp(0, w as i32 - 1) as usize, y.clamp(0, h as i32 - 1) as usize);
        albedo[y * w + x].a() as f32 / 255.0
    };
    let mut out = Vec::with_capacity(w * h);
    for y in 0..h as i32 {
        for x in 0..w as i32 {
            // wider than one pixel, so the rounding has some width
            let dx = height(x + 4, y) - height(x - 4, y);
            let dy = height(x, y + 4) - height(x, y - 4);
            out.push(unit([-dx * 1.5, -dy * 1.5, 1.0]));
        }
    }
    out
}

/// A normal map from a height picture (brightness = height): the way to get one from a grey painting. Returned as colors, ready to be used as `Maps::normal`.
pub fn normal_map_from_height(height: &[Color32], w: usize, h: usize, strength: f32) -> Vec<Color32> {
    let at = |x: i32, y: i32| -> f32 {
        let (x, y) = (x.clamp(0, w as i32 - 1) as usize, y.clamp(0, h as i32 - 1) as usize);
        let c = height[y * w + x];
        (0.299 * c.r() as f32 + 0.587 * c.g() as f32 + 0.114 * c.b() as f32) / 255.0 * (c.a() as f32 / 255.0)
    };
    let mut out = Vec::with_capacity(w * h);
    for y in 0..h as i32 {
        for x in 0..w as i32 {
            let n = unit([-(at(x + 1, y) - at(x - 1, y)) * strength, -(at(x, y + 1) - at(x, y - 1)) * strength, 1.0]);
            out.push(Color32::from_rgb((n[0] * 127.5 + 127.5) as u8, (n[1] * 127.5 + 127.5) as u8, (n[2] * 127.5 + 127.5) as u8));
        }
    }
    out
}

fn grey(c: Color32) -> f32 {
    (0.299 * c.r() as f32 + 0.587 * c.g() as f32 + 0.114 * c.b() as f32) / 255.0
}

/// The picture lit. `albedo` is premultiplied, `w`×`h`. The result has the same alpha.
pub fn light(albedo: &[Color32], maps: &Maps, w: usize, h: usize, l: &Lighting) -> Vec<Color32> {
    let l = l.clone().sanitized();
    let lv = unit([l.dir[0], l.dir[1], (1.0 - l.dir[0] * l.dir[0] - l.dir[1] * l.dir[1]).max(0.0).sqrt()]);
    let half = unit([lv[0], lv[1], lv[2] + 1.0]);
    let own = if maps.normal.is_none() { Some(normals_from_alpha(albedo, w, h)) } else { None };
    let rim = [l.rim_color[0] as f32, l.rim_color[1] as f32, l.rim_color[2] as f32];
    let mut out = Vec::with_capacity(w * h);
    for i in 0..w * h {
        let a = albedo[i];
        if a.a() == 0 {
            // light given off by itself shows even where the picture is clear
            out.push(emit(maps, i, Color32::TRANSPARENT));
            continue;
        }
        let n = match (&maps.normal, &own) {
            (Some(m), _) => {
                let c = m[i];
                unit([c.r() as f32 / 127.5 - 1.0, c.g() as f32 / 127.5 - 1.0, (c.b() as f32 / 127.5 - 1.0).max(0.05)])
            }
            (None, Some(o)) => o[i],
            _ => [0.0, 0.0, 1.0],
        };
        let rough = maps.roughness.as_ref().map_or(0.6, |m| grey(m[i]));
        let open = maps.occlusion.as_ref().map_or(1.0, |m| grey(m[i]));
        let diffuse = (n[0] * lv[0] + n[1] * lv[1] + n[2] * lv[2]).max(0.0);
        // a surface facing the viewer keeps the colors of the flat picture: the lit view differs from it only by the shape
        let mut f = l.ambient * open + l.strength * diffuse;
        f /= (l.ambient + l.strength * lv[2]).max(1e-3);
        let spec = (n[0] * half[0] + n[1] * half[1] + n[2] * half[2]).max(0.0).powf(2.0 + (1.0 - rough) * 60.0) * (1.0 - rough) * l.shine * l.strength;
        let edge = (1.0 - n[2]).clamp(0.0, 1.0);
        let r = edge * edge * l.rim;
        let alpha = a.a() as f32 / 255.0;
        // premultiplied: a channel never goes above the alpha
        let chan = |v: u8, ri: f32| ((v as f32 * f + 255.0 * spec * alpha + ri * r * alpha).clamp(0.0, a.a() as f32)) as u8;
        let lit = Color32::from_rgba_premultiplied(chan(a.r(), rim[0]), chan(a.g(), rim[1]), chan(a.b(), rim[2]), a.a());
        out.push(emit(maps, i, lit));
    }
    out
}

/// Add the emissive map on top.
fn emit(maps: &Maps, i: usize, base: Color32) -> Color32 {
    let Some(e) = &maps.emissive else { return base };
    let e = e[i];
    let (r, g, b) = (base.r().saturating_add(e.r()), base.g().saturating_add(e.g()), base.b().saturating_add(e.b()));
    let a = base.a().max(e.r().max(e.g()).max(e.b()));
    Color32::from_rgba_premultiplied(r.min(a), g.min(a), b.min(a), a)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn disc(n: usize) -> Vec<Color32> {
        (0..n * n)
            .map(|i| {
                let (x, y) = ((i % n) as f32 - n as f32 / 2.0, (i / n) as f32 - n as f32 / 2.0);
                if x.hypot(y) < n as f32 / 2.0 - 3.0 { Color32::from_rgb(180, 140, 120) } else { Color32::TRANSPARENT }
            })
            .collect()
    }

    #[test]
    fn the_side_facing_the_light_is_brighter_than_the_side_facing_away() {
        let n = 40;
        let lit = light(&disc(n), &Maps::default(), n, n, &Lighting { dir: [-0.7, 0.0], ..Default::default() });
        let (toward, away) = (lit[20 * n + 5], lit[20 * n + 34]);
        assert!(toward.r() > away.r(), "{toward:?} {away:?}");
        assert_eq!(lit[0], Color32::TRANSPARENT, "clear stays clear");
    }

    #[test]
    fn the_light_moving_round_moves_the_bright_side() {
        let n = 40;
        let left = light(&disc(n), &Maps::default(), n, n, &Lighting { dir: [-0.7, 0.0], ..Default::default() });
        let right = light(&disc(n), &Maps::default(), n, n, &Lighting { dir: [0.7, 0.0], ..Default::default() });
        assert!(left[20 * n + 5].r() > left[20 * n + 34].r());
        assert!(right[20 * n + 5].r() < right[20 * n + 34].r());
    }

    #[test]
    fn a_normal_map_changes_the_shading_and_a_flat_one_does_not() {
        let n = 8;
        let flat_pic = vec![Color32::from_rgb(120, 120, 120); n * n];
        let l = Lighting { dir: [-0.7, 0.0], ..Default::default() };
        let flat_map = Maps { normal: Some(vec![Color32::from_rgb(128, 128, 255); n * n]), ..Default::default() };
        let tilted = Maps { normal: Some(vec![Color32::from_rgb(40, 128, 200); n * n]), ..Default::default() };
        let a = light(&flat_pic, &flat_map, n, n, &l)[27];
        let b = light(&flat_pic, &tilted, n, n, &l)[27];
        assert!(b.r() > a.r(), "tilted toward the light is brighter: {a:?} {b:?}");
    }

    #[test]
    fn emissive_shows_even_where_the_picture_is_clear_and_occlusion_darkens() {
        let n = 4;
        let pic = vec![Color32::from_rgb(200, 200, 200); n * n];
        let mut glow = vec![Color32::TRANSPARENT; n * n];
        glow[0] = Color32::from_rgb(255, 0, 0);
        let clear = vec![Color32::TRANSPARENT; n * n];
        let out = light(&clear, &Maps { emissive: Some(glow), ..Default::default() }, n, n, &Lighting::default());
        assert!(out[0].r() > 200 && out[1] == Color32::TRANSPARENT);
        let l = Lighting::default();
        let open = light(&pic, &Maps::default(), n, n, &l);
        let closed = light(&pic, &Maps { occlusion: Some(vec![Color32::BLACK; n * n]), ..Default::default() }, n, n, &l);
        assert!(closed[5].r() < open[5].r());
    }

    #[test]
    fn rim_light_lights_the_edge_only() {
        let n = 40;
        let l = Lighting { rim: 1.5, strength: 0.0, ambient: 0.5, ..Default::default() };
        let lit = light(&disc(n), &Maps::default(), n, n, &l);
        let plain = light(&disc(n), &Maps::default(), n, n, &Lighting { rim: 0.0, ..l.clone() });
        assert!(lit[20 * n + 5].r() > plain[20 * n + 5].r(), "the edge gets brighter");
        assert_eq!(lit[20 * n + 20], plain[20 * n + 20], "the middle does not");
    }

    #[test]
    fn a_height_picture_becomes_a_normal_map_tilted_the_right_way() {
        let (w, h) = (8, 3);
        // a ramp rising to the right
        let ramp: Vec<Color32> = (0..w * h).map(|i| { let v = ((i % w) * 30) as u8; Color32::from_rgb(v, v, v) }).collect();
        let m = normal_map_from_height(&ramp, w, h, 2.0);
        assert!(m[w + 4].r() < 128, "the surface faces left when it rises to the right: {:?}", m[w + 4]);
        assert!((m[w + 4].g() as i32 - 128).abs() <= 1);
    }
}
