// SPDX-License-Identifier: GPL-3.0-or-later
// Hair that moves, for a character built on an AI shape. The hair is found on the drawing (grown from the top of the head by
// colour), made a part of its own that sits a little outside the head, and hung on chains of bones that a game or a VRM app
// swings with its spring bones. The head under it keeps the hair colour, so nothing opens up when the hair swings.

use image::RgbaImage;

use super::hairedit::{HairChain, HairDab, HairEdit};
use super::model::{Collider, Model3d, Part3d, Spring};

#[derive(Clone)]
pub struct HairMask {
    x0: i32,
    y0: i32,
    w: i32,
    h: i32,
    bits: Vec<bool>,
}

impl HairMask {
    pub fn at(&self, x: f32, y: f32) -> bool {
        let (ix, iy) = (x.round() as i32 - self.x0, y.round() as i32 - self.y0);
        ix >= 0 && iy >= 0 && ix < self.w && iy < self.h && self.bits[(iy * self.w + ix) as usize]
    }

    /// Correct the hair with the dabs of the hair brush: they add to it or take away from it, in order.
    pub fn apply_dabs(&mut self, dabs: &[HairDab]) {
        for d in dabs {
            let r = d.radius.max(1.0);
            for y in ((d.at[1] - r).floor() as i32)..=((d.at[1] + r).ceil() as i32) {
                for x in ((d.at[0] - r).floor() as i32)..=((d.at[0] + r).ceil() as i32) {
                    let (ix, iy) = (x - self.x0, y - self.y0);
                    if ix >= 0 && iy >= 0 && ix < self.w && iy < self.h && ((x as f32 - d.at[0]).powi(2) + (y as f32 - d.at[1]).powi(2)).sqrt() <= r {
                        self.bits[(iy * self.w + ix) as usize] = d.add;
                    }
                }
            }
        }
    }

    /// The hair as a picture of its own (green, a little see-through), and where its top left is on the page.
    pub fn to_image(&self) -> (RgbaImage, [i32; 2]) {
        let mut img = RgbaImage::new(self.w as u32, self.h as u32);
        for y in 0..self.h {
            for x in 0..self.w {
                if self.bits[(y * self.w + x) as usize] {
                    img.put_pixel(x as u32, y as u32, image::Rgba([40, 220, 120, 90]));
                }
            }
        }
        (img, [self.x0, self.y0])
    }

    pub fn count(&self) -> usize {
        self.bits.iter().filter(|b| **b).count()
    }
}

/// The hair on the picture: the pixels joined to the top of the head that have the colour of the hair there. `head` is the box of
/// the head (x, y, width, height) from `head_box`.
pub fn hair_mask(page: &RgbaImage, head: [f32; 4]) -> Option<HairMask> {
    let hh = head[2] / 2.0;
    let (cx, top) = (head[0] + hh, head[1] + 0.1 * hh);
    // the whole width of the picture: long hair spreads far beyond the head (and only what is joined to the scalp by the colour of the hair counts)
    let (x0, y0) = (0, ((top - 0.2 * hh) as i32).max(0));
    let (x1, y1) = (page.width() as i32, ((top + 9.0 * hh) as i32).min(page.height() as i32));
    let (w, h) = (x1 - x0, y1 - y0);
    if w < 8 || h < 8 {
        return None;
    }
    let px = |x: i32, y: i32| page.get_pixel((x0 + x) as u32, (y0 + y) as u32).0;
    // the colour of the hair: the upper part of the head, in the middle
    let (mut sum, mut n) = ([0u64; 3], 0u64);
    for y in ((top - y0 as f32 + 0.1 * hh) as i32)..((top - y0 as f32 + 0.4 * hh) as i32) {
        for x in ((cx - x0 as f32 - 0.4 * hh) as i32)..((cx - x0 as f32 + 0.4 * hh) as i32) {
            if x >= 0 && y >= 0 && x < w && y < h && px(x, y)[3] >= 128 {
                let p = px(x, y);
                for c in 0..3 {
                    sum[c] += p[c] as u64;
                }
                n += 1;
            }
        }
    }
    if n < 50 {
        return None;
    }
    let mean = [(sum[0] / n) as f32, (sum[1] / n) as f32, (sum[2] / n) as f32];
    let lum = 0.3 * mean[0] + 0.59 * mean[1] + 0.11 * mean[2];
    // dark hair has dark shadows and bright highlights; light hair is close to the colour of skin
    let tol = (90.0 - 0.25 * lum).clamp(45.0, 85.0);
    // Coloured hair (pink, blue, red...) has light and dark strands of the same hue; grey, black and white hair is told by its colour distance.
    let (mean_hue, mean_sat) = hue_sat(mean);
    // Coloured hair (pink, blue, red...) has light and dark strands of the same hue; grey, black and white hair is told by its colour distance.
    let (mean_hue, mean_sat) = hue_sat(mean);
    let strict = |p: [u8; 4]| {
        p[3] >= 128 && {
            let d = [p[0] as f32 - mean[0], p[1] as f32 - mean[1], p[2] as f32 - mean[2]];
            (d[0] * d[0] + d[1] * d[1] + d[2] * d[2]).sqrt() < tol
        }
    };
    let by_hue = |p: [u8; 4]| {
        strict(p) || (p[3] >= 128 && {
            let (h, sat) = hue_sat([p[0] as f32, p[1] as f32, p[2] as f32]);
            let dh = (h - mean_hue).abs().min(360.0 - (h - mean_hue).abs());
            dh < 24.0 && sat > 0.05 && sat > mean_sat * 0.3
        })
    };
    // the pixels joined to the scalp that the rule takes
    let grow = |close: &dyn Fn([u8; 4]) -> bool| -> Vec<bool> {
        let mut bits = vec![false; (w * h) as usize];
        let mut stack: Vec<(i32, i32)> = Vec::new();
        for y in ((top - y0 as f32 + 0.1 * hh) as i32)..((top - y0 as f32 + 0.4 * hh) as i32) {
            for x in ((cx - x0 as f32 - 0.4 * hh) as i32)..((cx - x0 as f32 + 0.4 * hh) as i32) {
                if x >= 0 && y >= 0 && x < w && y < h && close(px(x, y)) && !bits[(y * w + x) as usize] {
                    bits[(y * w + x) as usize] = true;
                    stack.push((x, y));
                }
            }
        }
        while let Some((x, y)) = stack.pop() {
            // a thin dark outline between two strands is stepped over (two pixels at a time)
            for (dx, dy) in [(1, 0), (-1, 0), (0, 1), (0, -1), (2, 0), (-2, 0), (0, 2), (0, -2)] {
                let (nx, ny) = (x + dx, y + dy);
                if nx >= 0 && ny >= 0 && nx < w && ny < h && !bits[(ny * w + nx) as usize] && close(px(nx, ny)) {
                    bits[(ny * w + nx) as usize] = true;
                    stack.push((nx, ny));
                }
            }
        }
        bits
    };
    let mut bits = grow(&strict);
    let mut coloured = false;
    // The hue rule finds the long strands of coloured hair. It would take skin for hair when the hair is brown or reddish, so it is used only when
    // the hue of the hair is far from the hue of the skin (taken from the jaw, under the mouth).
    let skin_hue = {
        let (mut sum, mut n) = ([0f32; 3], 0f32);
        for y in ((top - y0 as f32 + 1.55 * hh) as i32)..((top - y0 as f32 + 1.8 * hh) as i32) {
            for x in ((cx - x0 as f32 - 0.2 * hh) as i32)..((cx - x0 as f32 + 0.2 * hh) as i32) {
                if x >= 0 && y >= 0 && x < w && y < h {
                    let p = px(x, y);
                    if p[3] >= 128 && 0.3 * p[0] as f32 + 0.59 * p[1] as f32 + 0.11 * p[2] as f32 > 90.0 {
                        for c in 0..3 {
                            sum[c] += p[c] as f32;
                        }
                        n += 1.0;
                    }
                }
            }
        }
        (n >= 30.0).then(|| hue_sat([sum[0] / n, sum[1] / n, sum[2] / n]).0)
    };
    let far_from_skin = skin_hue.is_some_and(|sh| (mean_hue - sh).abs().min(360.0 - (mean_hue - sh).abs()) >= 20.0);
    if mean_sat > 0.10 && far_from_skin {
        let wide = grow(&by_hue);
        let (n_wide, n_strict) = (wide.iter().filter(|b| **b).count(), bits.iter().filter(|b| **b).count());
        if n_wide < n_strict * 12 {
            bits = wide;
            coloured = true;
        }
    }
    let mask = HairMask { x0, y0, w, h, bits };
    if std::env::var("SEVIATH_DEBUG_HAIR").is_ok() {
        let mut dbg = page.clone();
        for y in 0..h {
            for x in 0..w {
                if mask.bits[(y * w + x) as usize] {
                    let p = dbg.get_pixel_mut((x0 + x) as u32, (y0 + y) as u32);
                    p.0 = [255, 0, 0, 255];
                }
            }
        }
        let _ = dbg.save(std::env::temp_dir().join("hair_mask.png"));
        let _ = page.save(std::env::temp_dir().join("hair_page.png"));
        eprintln!("hair colour {:?} tolerance {tol}, coloured {coloured}, hue {mean_hue}, sat {mean_sat}, skin hue {skin_hue:?}", mean);
    }
    // a few pixels are not hair
    (mask.count() as f32 > 0.02 * (hh * hh)).then_some(mask)
}

/// Where the hair is on a shape that has its own colours (CharacterGen paints its mesh): the points that are hair on the drawing are the start (`seeds`),
/// and the hair grows over the surface into every point of the colour of the hair that touches it, so the strands that hang beside an arm or a hand are
/// found by what they are (hair), not by where they are in the picture of the front. A point of another colour (a sleeve, a hand) stops it.
pub fn hair_from_colors(verts: &[[f32; 3]], tris: &[[u32; 3]], colors: &[[u8; 3]], seeds: &[bool]) -> Vec<bool> {
    let n = verts.len();
    if colors.len() != n || seeds.len() != n || n == 0 {
        return vec![false; n];
    }
    // the colour of the hair: the middle of the colours of the start
    let (mut sum, mut cnt) = ([0f64; 3], 0f64);
    for i in 0..n {
        if seeds[i] {
            for c in 0..3 {
                sum[c] += colors[i][c] as f64;
            }
            cnt += 1.0;
        }
    }
    if cnt < 50.0 {
        return vec![false; n];
    }
    let mean = [(sum[0] / cnt) as f32, (sum[1] / cnt) as f32, (sum[2] / cnt) as f32];
    let lum = 0.3 * mean[0] + 0.59 * mean[1] + 0.11 * mean[2];
    let tol = (80.0 - 0.25 * lum).clamp(40.0, 75.0);
    let (mean_hue, mean_sat) = hue_sat(mean);
    let by_hue = mean_sat > 0.12;
    let like = |c: [u8; 3]| {
        let f = [c[0] as f32, c[1] as f32, c[2] as f32];
        let d = ((f[0] - mean[0]).powi(2) + (f[1] - mean[1]).powi(2) + (f[2] - mean[2]).powi(2)).sqrt();
        if d < tol {
            return true;
        }
        if by_hue {
            let (h, s) = hue_sat(f);
            let dh = (h - mean_hue).abs().min(360.0 - (h - mean_hue).abs());
            return dh < 16.0 && s > mean_sat * 0.55;
        }
        false
    };
    // the points that are the same place (a seam of the picture on the shape has two) count as one when the hair grows
    let key = |p: [f32; 3]| ((p[0] * 4000.0).round() as i32, (p[1] * 4000.0).round() as i32, (p[2] * 4000.0).round() as i32);
    let mut id_of: std::collections::HashMap<(i32, i32, i32), usize> = std::collections::HashMap::new();
    let ids: Vec<usize> = verts.iter().map(|p| { let k = key(*p); let next = id_of.len(); *id_of.entry(k).or_insert(next) }).collect();
    let m = id_of.len();
    let mut next_to: Vec<Vec<usize>> = vec![Vec::new(); m];
    for t in tris {
        for k in 0..3 {
            let (a, b) = (ids[t[k] as usize], ids[t[(k + 1) % 3] as usize]);
            if a != b {
                next_to[a].push(b);
                next_to[b].push(a);
            }
        }
    }
    // a place is hair-like when any of its points has the colour of the hair
    let mut like_at = vec![false; m];
    let mut seed_at = vec![false; m];
    for i in 0..n {
        if like(colors[i]) {
            like_at[ids[i]] = true;
        }
        if seeds[i] {
            seed_at[ids[i]] = true;
        }
    }
    let mut hair = vec![false; m];
    let mut stack: Vec<usize> = (0..m).filter(|i| seed_at[*i]).collect();
    for &s in &stack {
        hair[s] = true;
    }
    while let Some(a) = stack.pop() {
        for &b in &next_to[a] {
            if !hair[b] && like_at[b] {
                hair[b] = true;
                stack.push(b);
            }
        }
    }
    (0..n).map(|i| hair[ids[i]]).collect()
}

/// Which points of a shape are hair, found again by place: the nearest point of the shape (in the model's space) says it.
pub struct HairPoints {
    cell: f32,
    grid: std::collections::HashMap<(i32, i32, i32), Vec<([f32; 3], bool)>>,
}

impl HairPoints {
    pub fn new(points: &[[f32; 3]], flags: &[bool]) -> HairPoints {
        let cell = 0.02;
        let mut grid: std::collections::HashMap<(i32, i32, i32), Vec<([f32; 3], bool)>> = std::collections::HashMap::new();
        for (p, f) in points.iter().zip(flags) {
            grid.entry(((p[0] / cell).floor() as i32, (p[1] / cell).floor() as i32, (p[2] / cell).floor() as i32)).or_default().push((*p, *f));
        }
        HairPoints { cell, grid }
    }

    /// Is the nearest point of the shape (within a few centimetres) hair.
    pub fn is_hair(&self, p: &[f32; 3]) -> bool {
        let c = ((p[0] / self.cell).floor() as i32, (p[1] / self.cell).floor() as i32, (p[2] / self.cell).floor() as i32);
        let mut best: Option<(f32, bool)> = None;
        for dx in -1..=1 {
            for dy in -1..=1 {
                for dz in -1..=1 {
                    if let Some(list) = self.grid.get(&(c.0 + dx, c.1 + dy, c.2 + dz)) {
                        for (q, f) in list {
                            let d = (q[0] - p[0]).powi(2) + (q[1] - p[1]).powi(2) + (q[2] - p[2]).powi(2);
                            if best.is_none_or(|b| d < b.0) {
                                best = Some((d, *f));
                            }
                        }
                    }
                }
            }
        }
        best.is_some_and(|b| b.1)
    }
}

/// Hue (degrees) and saturation (0 to 1) of a colour.
fn hue_sat(c: [f32; 3]) -> (f32, f32) {
    let (mx, mn) = (c[0].max(c[1]).max(c[2]), c[0].min(c[1]).min(c[2]));
    let d = mx - mn;
    if mx <= 0.0 || d <= 0.0 {
        return (0.0, 0.0);
    }
    let h = if mx == c[0] { ((c[1] - c[2]) / d).rem_euclid(6.0) } else if mx == c[1] { (c[2] - c[0]) / d + 2.0 } else { (c[0] - c[1]) / d + 4.0 };
    (h * 60.0, d / mx)
}

const COLUMNS: usize = 6;
const REACH: [f32; 4] = [0.0, 0.33, 0.66, 1.0];

/// The distance from a point to a chain of joints (3D), and how far along the chain the nearest place is: a number from 0 (the scalp)
/// to the number of joints minus one (the tip).
fn along_chain(p: [f32; 3], joints: &[[f32; 3]]) -> (f32, f32) {
    let (mut best, mut at) = (f32::MAX, 0.0f32);
    for i in 0..joints.len().saturating_sub(1) {
        let (a, b) = (joints[i], joints[i + 1]);
        let ab = [b[0] - a[0], b[1] - a[1], b[2] - a[2]];
        let ap = [p[0] - a[0], p[1] - a[1], p[2] - a[2]];
        let len2 = (ab[0] * ab[0] + ab[1] * ab[1] + ab[2] * ab[2]).max(1e-12);
        let t = ((ap[0] * ab[0] + ap[1] * ab[1] + ap[2] * ab[2]) / len2).clamp(0.0, 1.0);
        let q = [a[0] + ab[0] * t - p[0], a[1] + ab[1] * t - p[1], a[2] + ab[2] * t - p[2]];
        let d = (q[0] * q[0] + q[1] * q[1] + q[2] * q[2]).sqrt();
        if d < best {
            best = d;
            at = i as f32 + t;
        }
    }
    (best, at)
}

/// The strands the app makes by itself for the hair found on the drawing: a few columns across it, in front and behind. The joints
/// are in page pixels (`frame` brings the model to the page: the middle of the figure, its bottom, metres per pixel).
pub fn auto_chains(used: &[[f32; 3]], frame: (f32, f32, f32)) -> Vec<HairChain> {
    let (xc, bottom, scale) = frame;
    let (mut lo, mut hi) = ([f32::MAX; 3], [f32::MIN; 3]);
    for v in used {
        for k in 0..3 {
            lo[k] = lo[k].min(v[k]);
            hi[k] = hi[k].max(v[k]);
        }
    }
    let colw = (hi[0] - lo[0]) / COLUMNS as f32;
    let top = hi[1];
    let mut chains = Vec::new();
    if colw < 1e-4 {
        return chains;
    }
    for layer in [1.0f32, -1.0] {
        if !used.iter().any(|v| v[2] * layer > 0.0) {
            continue;
        }
        for c in 0..COLUMNS {
            let x = lo[0] + (c as f32 + 0.5) * colw;
            let low = used.iter().filter(|v| (v[0] - x).abs() <= colw * 0.5 && v[2] * layer > 0.0).map(|v| v[1]).fold(f32::MAX, f32::min);
            if low == f32::MAX || top - low < 0.05 {
                continue;
            }
            let len = top - low;
            let joints: Vec<[f32; 2]> = REACH.iter().map(|f| [x / scale + xc, bottom - (top - len * f) / scale]).collect();
            chains.push(HairChain { joints, layer, ..Default::default() });
        }
    }
    chains
}

/// Make the hair of a model a part of its own with bone chains. `page_of` gives the place on the picture of a point of the body (page
/// pixels). `edit` has the strands the user made (when it has none the app makes them, and they are returned in `model.hair_chains`
/// for the user to start from). `frame` is (the middle of the figure on the page, its bottom, metres per pixel).
pub fn add_hair(model: &mut Model3d, mask: &HairMask, page_of: impl Fn(&[f32; 3]) -> [f32; 2], edit: &HairEdit, frame: (f32, f32, f32), by_colour: Option<&HairPoints>) -> bool {
    let Some(head) = model.bone("head") else { return false };
    // the points and triangles of the body and the face together (the face carries the blend shapes, the hair covers both)
    let (mut b_verts, mut b_normals, mut b_uvs, mut b_tris): (Vec<[f32; 3]>, Vec<[f32; 3]>, Vec<[f32; 2]>, Vec<[u32; 3]>) = (Vec::new(), Vec::new(), Vec::new(), Vec::new());
    let (b_layer, b_image) = (model.parts[0].layer, model.parts[0].image.clone());
    for p in model.parts.iter().filter(|p| p.name == "Body" || p.name == "Face") {
        let base = b_verts.len() as u32;
        b_verts.extend(p.verts.iter().copied());
        b_normals.extend(p.normals.iter().copied());
        b_uvs.extend(p.uvs.iter().copied());
        b_tris.extend(p.tris.iter().map(|t| [t[0] + base, t[1] + base, t[2] + base]));
    }
    // hair on the drawing, and hair by the colours of the shape itself (strands that hang beside an arm or a hand)
    let is_hair: Vec<bool> = b_verts.iter().map(|v| { let p = page_of(v); mask.at(p[0], p[1]) || by_colour.is_some_and(|h| h.is_hair(v)) }).collect();
    if std::env::var("SEVIATH_DEBUG_HAIR").is_ok() {
        eprintln!("hair: {} of {} points are hair (mask {} pixels in {} x {})", is_hair.iter().filter(|h| **h).count(), is_hair.len(), mask.count(), mask.w, mask.h);
    }
    let tris: Vec<[u32; 3]> = b_tris.iter().copied().filter(|t| t.iter().all(|i| is_hair[*i as usize])).collect();
    if tris.len() < 50 {
        return false;
    }
    let mut used: Vec<u32> = tris.iter().flatten().copied().collect();
    used.sort_unstable();
    used.dedup();
    let used_verts: Vec<[f32; 3]> = used.iter().map(|i| b_verts[*i as usize]).collect();
    let zext = used_verts.iter().map(|v| v[2].abs()).fold(0.0f32, f32::max);
    let top = used_verts.iter().map(|v| v[1]).fold(f32::MIN, f32::max);
    // the strands: the user's, or the app's own
    let chains: Vec<HairChain> = if edit.chains.is_empty() { auto_chains(&used_verts, frame) } else { edit.chains.clone() };
    let chains: Vec<HairChain> = chains.into_iter().filter(|c| c.joints.len() >= 2).collect();
    if chains.is_empty() {
        return false;
    }
    model.hair_chains = chains.clone();
    let (xc, bottom, scale) = frame;
    // the joints of every strand in the model (the depth by how far to the front or back it hangs)
    let to_model = |p: [f32; 2], layer: f32| [(p[0] - xc) * scale, (bottom - p[1]) * scale, layer.clamp(-1.0, 1.0) * zext * 0.6];
    let joints3: Vec<Vec<[f32; 3]>> = chains.iter().map(|c| c.joints.iter().map(|j| to_model(*j, c.layer)).collect()).collect();
    // the bones, and the springs that swing them: one bone for every joint after the first (the first is on the scalp)
    let mut bone_of: Vec<Vec<usize>> = Vec::new();
    for (ci, (c, js)) in chains.iter().zip(&joints3).enumerate() {
        let mut parent = head;
        let mut idx = Vec::new();
        for k in 1..js.len() {
            let tip = if k + 1 < js.len() { js[k + 1] } else { [js[k][0] + (js[k][0] - js[k - 1][0]) * 0.5, js[k][1] + (js[k][1] - js[k - 1][1]) * 0.5, js[k][2] + (js[k][2] - js[k - 1][2]) * 0.5] };
            model.bones.push(super::model::Bone { name: format!("hair{ci}_{k}"), humanoid: false, parent: Some(parent), pos: js[k], tip });
            parent = model.bones.len() - 1;
            idx.push(parent);
        }
        model.springs.push(Spring { name: format!("hair{ci}"), joints: idx.clone(), hit_radius: 0.012, stiffness: c.stiffness, gravity: c.gravity, drag: c.drag });
        bone_of.push(idx);
    }
    // a ball for the head, which the hair does not go through
    let hp = model.bones[head].pos;
    model.colliders.push(Collider { bone: head, offset: [0.0, (top - hp[1]) * 0.5, 0.0], radius: ((top - hp[1]) * 0.55).max(0.07) });
    // the part: the hair triangles, a little outside the head
    let mut remap = std::collections::HashMap::new();
    let (mut verts, mut normals, mut uvs, mut joints, mut weights) = (Vec::new(), Vec::new(), Vec::new(), Vec::new(), Vec::new());
    let mut out_tris: Vec<[u32; 3]> = Vec::with_capacity(tris.len());
    for t in &tris {
        let mut o = [0u32; 3];
        for k in 0..3 {
            let i = t[k];
            o[k] = *remap.entry(i).or_insert_with(|| {
                let (v, n) = (b_verts[i as usize], b_normals[i as usize]);
                let p = [v[0] + n[0] * 0.003, v[1] + n[1] * 0.003, v[2] + n[2] * 0.003];
                // the two nearest strands share the point; along a strand the two bones next to the place share it
                let mut near: Vec<(usize, f32, f32)> = joints3.iter().enumerate().map(|(ci, js)| { let (d, at) = along_chain(p, js); (ci, d, at) }).collect();
                near.sort_by(|a, b| a.1.total_cmp(&b.1));
                near.truncate(2);
                let total: f32 = near.iter().map(|n| 1.0 / (n.1 * n.1 + 1e-4)).sum();
                let mut acc: Vec<(usize, f32)> = Vec::new();
                for (ci, d, at) in near {
                    let share = 1.0 / (d * d + 1e-4) / total;
                    let last = bone_of[ci].len();
                    let seg = (at.floor() as usize).min(last.saturating_sub(1));
                    let t = (at - seg as f32).clamp(0.0, 1.0);
                    // joint `seg` is the bone before (the head for the first), joint `seg + 1` the bone after
                    let upper = if seg == 0 { head } else { bone_of[ci][seg - 1] };
                    let lower = bone_of[ci][seg.min(last - 1)];
                    for (b, w) in [(upper, 1.0 - t), (lower, t)] {
                        match acc.iter_mut().find(|a| a.0 == b) {
                            Some(a) => a.1 += w * share,
                            None => acc.push((b, w * share)),
                        }
                    }
                }
                acc.sort_by(|a, b| b.1.total_cmp(&a.1));
                acc.truncate(4);
                let sum: f32 = acc.iter().map(|a| a.1).sum::<f32>().max(1e-9);
                let (mut j, mut w) = ([0u16; 4], [0.0f32; 4]);
                for (k, (b, x)) in acc.iter().enumerate() {
                    j[k] = *b as u16;
                    w[k] = x / sum;
                }
                verts.push(p);
                normals.push(n);
                uvs.push(b_uvs[i as usize]);
                joints.push(j);
                weights.push(w);
                (verts.len() - 1) as u32
            });
        }
        out_tris.push(o);
    }
    let part = Part3d { name: "Hair".to_string(), layer: b_layer, verts, normals, uvs, tris: out_tris, joints, weights, image: b_image, morphs: Vec::new() };
    // The AI makes one surface of everything: where the hair touches a hand or a leg, the surface is joined there. Two pieces that move on their
    // own cannot share triangles (they would stretch into long lines), so the points on the seam are copied: each piece gets its own, in the same
    // place, and nothing shows at rest.
    let mut at_cut = 0usize;
    let mut cut = 0usize;
    let mut orig_lens: Vec<usize> = Vec::new();
    for p in model.parts.iter_mut().filter(|p| p.name == "Body" || p.name == "Face") {
        orig_lens.push(p.verts.len());
        // the hair is piece 1, everything else 0 (the body joins both); the seam between hair and not hair needs two pieces, so the rest is 2
        let group: Vec<usize> = (0..p.verts.len()).map(|i| if is_hair[at_cut + i] { 1 } else { 2 }).collect();
        cut += super::model::separate_groups(p, &group);
        at_cut += group.len();
    }
    if std::env::var("SEVIATH_DEBUG_HAIR").is_ok() {
        eprintln!("hair: {cut} triangles between the hair and the rest were separated");
    }
    // The hair is a part of its own now, but the body still has the points of the hair under it. Those must not follow an arm or a leg
    // that the hair happens to hang beside (the strand would stretch into a sheet when the arm moves): they follow the head.
    let mut at = 0usize;
    for (p, n) in model.parts.iter_mut().filter(|p| p.name == "Body" || p.name == "Face").zip(orig_lens) {
        for i in 0..n {
            if is_hair[at + i] {
                p.joints[i] = [head as u16, 0, 0, 0];
                p.weights[i] = [1.0, 0.0, 0.0, 0.0];
            }
        }
        at += n;
    }
    model.parts.push(part);
    true
}
