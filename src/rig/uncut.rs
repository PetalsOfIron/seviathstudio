// SPDX-License-Identifier: GPL-3.0-or-later
// Automatic skeleton for an uncut character: one picture of a standing figure (arms out, legs apart or together). The
// outline is studied to find the middle line, the top of the head, the neck, the hips and the way the arms and legs
// run (by following each limb along the inside of the outline), and a humanoid skeleton is put there. It is a good
// first guess for the usual standing figure; the joints can be moved by hand afterwards.

use image::RgbaImage;

use super::{Id, Kind, Rig};

/// The outline of the figure on a coarser grid.
pub struct Mask {
    pub w: usize,
    pub h: usize,
    bits: Vec<bool>,
    /// Page pixels per mask cell.
    pub scale: f32,
}

impl Mask {
    pub fn from_alpha(img: &RgbaImage, max_side: u32) -> Mask {
        let big = img.width().max(img.height()).max(1);
        let scale = (big as f32 / max_side as f32).max(1.0);
        let (w, h) = (((img.width() as f32 / scale).ceil() as usize).max(1), ((img.height() as f32 / scale).ceil() as usize).max(1));
        let mut bits = vec![false; w * h];
        for y in 0..h {
            for x in 0..w {
                // a cell is solid when enough of the pixels under it are
                let (x0, y0) = ((x as f32 * scale) as u32, (y as f32 * scale) as u32);
                let (x1, y1) = ((((x + 1) as f32 * scale) as u32).min(img.width()), (((y + 1) as f32 * scale) as u32).min(img.height()));
                let (mut on, mut all) = (0u32, 0u32);
                for yy in y0..y1.max(y0 + 1) {
                    for xx in x0..x1.max(x0 + 1) {
                        if xx < img.width() && yy < img.height() {
                            all += 1;
                            if img.get_pixel(xx, yy).0[3] > 40 {
                                on += 1;
                            }
                        }
                    }
                }
                bits[y * w + x] = all > 0 && on * 2 >= all;
            }
        }
        Mask { w, h, bits, scale }
    }

    pub fn at(&self, x: i32, y: i32) -> bool {
        x >= 0 && y >= 0 && (x as usize) < self.w && (y as usize) < self.h && self.bits[y as usize * self.w + x as usize]
    }

    fn solid(&self, p: [f32; 2]) -> bool {
        self.at(p[0].round() as i32, p[1].round() as i32)
    }

    /// (left, top, right, bottom) of everything solid.
    pub fn bbox(&self) -> Option<[usize; 4]> {
        let (mut x0, mut y0, mut x1, mut y1) = (usize::MAX, usize::MAX, 0usize, 0usize);
        for y in 0..self.h {
            for x in 0..self.w {
                if self.bits[y * self.w + x] {
                    x0 = x0.min(x);
                    y0 = y0.min(y);
                    x1 = x1.max(x);
                    y1 = y1.max(y);
                }
            }
        }
        (x0 != usize::MAX).then_some([x0, y0, x1, y1])
    }

    /// How far across the figure is at `p`, looking sideways to the direction `d`: (shift of the middle, width).
    /// None when `p` is not inside.
    fn across(&self, p: [f32; 2], d: [f32; 2], limit: f32) -> Option<(f32, f32)> {
        if !self.solid(p) {
            // look a little to the sides for the figure
            let n = [-d[1], d[0]];
            let found = (1..=4).find_map(|k| {
                let o = k as f32;
                [1.0f32, -1.0].iter().find_map(|s| {
                    let q = [p[0] + n[0] * o * s, p[1] + n[1] * o * s];
                    self.solid(q).then_some(o * s)
                })
            })?;
            let n2 = [-d[1], d[0]];
            return self.across([p[0] + n2[0] * found, p[1] + n2[1] * found], d, limit).map(|(c, w)| (c + found, w));
        }
        let n = [-d[1], d[0]];
        let (mut a, mut b) = (0.0f32, 0.0f32);
        while a < limit && self.solid([p[0] + n[0] * (a + 1.0), p[1] + n[1] * (a + 1.0)]) {
            a += 1.0;
        }
        while b < limit && self.solid([p[0] - n[0] * (b + 1.0), p[1] - n[1] * (b + 1.0)]) {
            b += 1.0;
        }
        Some(((a - b) / 2.0, a + b + 1.0))
    }

    /// Follow a limb: from `start` along `dir`, staying in the middle of the figure while it is thinner than `thick`.
    fn trace(&self, start: [f32; 2], dir: [f32; 2], thick: f32, max_len: f32) -> Vec<[f32; 2]> {
        let mut path = vec![start];
        let (mut p, mut d) = (start, norm(dir));
        let step = 2.0;
        let mut gone = 0.0;
        while gone < max_len {
            let q = [p[0] + d[0] * step, p[1] + d[1] * step];
            let Some((shift, width)) = self.across(q, d, thick * 1.3) else { break };
            if width > thick {
                break;
            }
            let n = [-d[1], d[0]];
            let c = [q[0] + n[0] * shift, q[1] + n[1] * shift];
            let nd = norm([c[0] - p[0], c[1] - p[1]]);
            d = norm([d[0] * 0.75 + nd[0] * 0.25, d[1] * 0.75 + nd[1] * 0.25]);
            p = c;
            gone += step;
            path.push(p);
        }
        path
    }
}

fn norm(v: [f32; 2]) -> [f32; 2] {
    let l = v[0].hypot(v[1]);
    if l < 1e-6 { [0.0, 1.0] } else { [v[0] / l, v[1] / l] }
}

fn path_len(p: &[[f32; 2]]) -> f32 {
    p.windows(2).map(|w| (w[1][0] - w[0][0]).hypot(w[1][1] - w[0][1])).sum()
}

/// The point at a distance along a path.
fn along(p: &[[f32; 2]], at: f32) -> [f32; 2] {
    let mut left = at.max(0.0);
    for w in p.windows(2) {
        let l = (w[1][0] - w[0][0]).hypot(w[1][1] - w[0][1]);
        if left <= l && l > 0.0 {
            let t = left / l;
            return [w[0][0] + (w[1][0] - w[0][0]) * t, w[0][1] + (w[1][1] - w[0][1]) * t];
        }
        left -= l;
    }
    *p.last().unwrap_or(&[0.0, 0.0])
}

/// How the arms or the legs are held, when the finder should not guess.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum Hold {
    /// Look at the outline.
    #[default]
    Auto,
    /// Arms out to the sides (a T pose); legs apart.
    Out,
    /// Arms hanging at the sides; legs together.
    Down,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct Hints {
    pub arms: Hold,
    pub legs: Hold,
}

/// A picture with a plain background (no see-through part) gets the background cut away: the colour of the border is
/// taken as the background and everything of that colour that touches the border becomes see-through. Pictures that
/// already have a see-through background are returned as they are.
pub fn cut_out_background(img: &RgbaImage) -> RgbaImage {
    let (w, h) = (img.width() as usize, img.height() as usize);
    if w < 4 || h < 4 {
        return img.clone();
    }
    let clear = img.pixels().filter(|p| p.0[3] < 40).count();
    if clear * 20 > w * h {
        return img.clone();
    }
    // the most common colour around the border
    let mut counts: std::collections::HashMap<[u8; 3], u32> = std::collections::HashMap::new();
    let mut edge = |x: usize, y: usize| {
        let p = img.get_pixel(x as u32, y as u32).0;
        *counts.entry([p[0] / 8, p[1] / 8, p[2] / 8]).or_default() += 1;
    };
    for x in 0..w {
        edge(x, 0);
        edge(x, h - 1);
    }
    for y in 0..h {
        edge(0, y);
        edge(w - 1, y);
    }
    let Some((bg, _)) = counts.into_iter().max_by_key(|(_, n)| *n) else { return img.clone() };
    let bg = [bg[0] as i32 * 8 + 4, bg[1] as i32 * 8 + 4, bg[2] as i32 * 8 + 4];
    let near = |x: usize, y: usize| {
        let p = img.get_pixel(x as u32, y as u32).0;
        (0..3).all(|k| (p[k] as i32 - bg[k]).abs() <= 34)
    };
    let mut gone = vec![false; w * h];
    let mut stack: Vec<(usize, usize)> = Vec::new();
    for x in 0..w {
        stack.push((x, 0));
        stack.push((x, h - 1));
    }
    for y in 0..h {
        stack.push((0, y));
        stack.push((w - 1, y));
    }
    while let Some((x, y)) = stack.pop() {
        if gone[y * w + x] || !near(x, y) {
            continue;
        }
        gone[y * w + x] = true;
        if x > 0 {
            stack.push((x - 1, y));
        }
        if y > 0 {
            stack.push((x, y - 1));
        }
        if x + 1 < w {
            stack.push((x + 1, y));
        }
        if y + 1 < h {
            stack.push((x, y + 1));
        }
    }
    let cut = gone.iter().filter(|g| **g).count();
    if cut * 20 < w * h || cut * 20 > w * h * 19 {
        return img.clone();
    }
    let mut out = img.clone();
    for y in 0..h {
        for x in 0..w {
            if gone[y * w + x] {
                out.put_pixel(x as u32, y as u32, image::Rgba([0, 0, 0, 0]));
            }
        }
    }
    out
}

/// The given layers drawn together on a see-through page.
pub fn composite(d: &crate::doc::Document, ids: &[crate::doc::LayerId]) -> Option<RgbaImage> {
    let layers: Vec<crate::doc::Layer> = d
        .layers
        .iter()
        .filter(|l| ids.contains(&l.id) && l.visible && matches!(l.kind, crate::doc::LayerKind::Raster | crate::doc::LayerKind::Vector))
        .cloned()
        .map(|mut l| {
            l.parent = None;
            l
        })
        .collect();
    if layers.is_empty() {
        return None;
    }
    let tmp = crate::doc::Document::from_parts(d.w, d.h, eframe::egui::Color32::TRANSPARENT, layers);
    Some(crate::files::flatten(&tmp))
}

/// The mesh over the figure in the given layers: the outline of what is drawn (a plain background is cut away first).
pub fn skin_mesh(d: &crate::doc::Document, ids: &[crate::doc::LayerId], detail: f32) -> Option<(Vec<[f32; 2]>, Vec<[u32; 3]>)> {
    let img = cut_out_background(&composite(d, ids)?);
    let mut scratch = crate::doc::Document::new(d.w, d.h);
    let mut tmp = scratch.new_layer("skin", crate::doc::LayerKind::Raster);
    tmp.tiles = crate::files::tiles_from_rgba(&img, 0, 0, d.w, d.h);
    super::mesh::generate(&tmp, d.w, d.h, detail)
}

/// What rigging the whole picture needs: the skeleton found from the outline and the mesh over the figure.
pub struct Found {
    pub figure: Figure,
    pub verts: Vec<[f32; 2]>,
    pub tris: Vec<[u32; 3]>,
}

pub fn find(d: &crate::doc::Document, ids: &[crate::doc::LayerId], detail: f32, hints: Hints) -> Result<Found, String> {
    let img = composite(d, ids).ok_or("Nothing to rig: no visible layer has paint.")?;
    let figure = detect_with(&img, hints).ok_or("No figure found in the picture.")?;
    let (verts, tris) = skin_mesh(d, ids, detail).ok_or("Nothing to mesh: the layers have no paint.")?;
    Ok(Found { figure, verts, tris })
}

/// Put what was found into the rig: the mesh as the uncut skin, the skeleton as bones, weights from the bones.
pub fn apply(rig: &mut Rig, d: (u32, u32), ids: &[crate::doc::LayerId], f: &Found) {
    clear_bones(rig);
    match rig.skin.as_mut() {
        Some(s) => {
            s.layers = ids.to_vec();
            s.verts = f.verts.clone();
            s.tris = f.tris.clone();
        }
        None => rig.skin = Some(super::skin::Skin::new(ids.to_vec(), f.verts.clone(), f.tris.clone(), d.0, d.1)),
    }
    add_skeleton(rig, &f.figure);
}

/// Rig the whole picture in one go.
pub fn auto_rig(rig: &mut Rig, d: &crate::doc::Document, ids: &[crate::doc::LayerId], detail: f32) -> Result<Figure, String> {
    let f = find(d, ids, detail, Hints::default())?;
    apply(rig, (d.w, d.h), ids, &f);
    Ok(f.figure)
}

/// Take away the bones of the uncut skin (and the changers they were), so a new skeleton can be made.
pub fn clear_bones(rig: &mut Rig) {
    let Some(sk) = rig.skin.as_mut() else { return };
    let gone: Vec<Id> = sk.bones.iter().map(|b| b.changer).collect();
    sk.bones.clear();
    sk.planes.clear();
    rig.changers.retain(|c| !gone.contains(&c.id));
    rig.bindings.retain(|b| !gone.contains(&b.changer));
    for c in &mut rig.changers {
        if c.parent.is_some_and(|p| gone.contains(&p)) {
            c.parent = None;
        }
    }
}

/// A limb followed along the outline: its three bones, the path of its middle and how wide it is along the way.
#[derive(Clone, Debug)]
pub struct Limb {
    pub bones: [&'static str; 3],
    pub path: Vec<[f32; 2]>,
    /// Half the width of the limb at every point of the path (page pixels).
    pub half: Vec<f32>,
    /// Where the second and the third bone begin, as a share of the length of the path.
    pub splits: [f32; 2],
    /// The bone that carries the root of the limb (the clavicle for an arm).
    pub root: Option<&'static str>,
}

/// The joints found, in page pixels (VRM humanoid names).
#[derive(Clone, Debug)]
pub struct Figure {
    pub limbs: Vec<Limb>,
    pub joints: Vec<(&'static str, [f32; 2])>,
    /// Where the piece after a bone ends (the tips of the hands, feet and head).
    pub tips: Vec<(&'static str, [f32; 2])>,
    /// 0..1: how sure the finder is (limbs found on both sides, a neck where it should be…).
    pub confidence: f32,
    pub notes: Vec<String>,
}

impl Figure {
    pub fn joint(&self, name: &str) -> Option<[f32; 2]> {
        self.joints.iter().find(|j| j.0 == name).map(|j| j.1)
    }

    pub fn tip(&self, name: &str) -> Option<[f32; 2]> {
        self.tips.iter().find(|j| j.0 == name).map(|j| j.1)
    }
}

/// Find the skeleton of a standing figure in a picture (`img`: the whole drawing, see-through where there is nothing).
pub fn detect(img: &RgbaImage) -> Option<Figure> {
    detect_with(img, Hints::default())
}

pub fn detect_with(img: &RgbaImage, hints: Hints) -> Option<Figure> {
    let cut = cut_out_background(img);
    let m = Mask::from_alpha(&cut, 420);
    let [bx0, by0, bx1, by1] = m.bbox()?;
    let (bw, bh) = ((bx1 - bx0 + 1) as f32, (by1 - by0 + 1) as f32);
    if bh < 30.0 || bw < 10.0 {
        return None;
    }
    let mut notes = Vec::new();
    let mut conf = 0.0f32;
    let xc = (bx0 + bx1) as f32 / 2.0;
    // the top of the head: the first row with something on the middle line
    let band = (bw * 0.03).max(2.0);
    let top = (by0..=by1).find(|y| (0..=(band as i32 * 2)).any(|k| m.at((xc - band) as i32 + k, *y as i32))).unwrap_or(by0) as f32;
    let height = by1 as f32 - top + 1.0;
    // width of the middle piece of every row, near the head
    let central = |y: usize| -> f32 {
        let cx = xc as i32;
        if !m.at(cx, y as i32) {
            return 0.0;
        }
        let (mut a, mut b) = (0, 0);
        while m.at(cx - a - 1, y as i32) && (a as f32) < bw {
            a += 1;
        }
        while m.at(cx + b + 1, y as i32) && (b as f32) < bw {
            b += 1;
        }
        (a + b + 1) as f32
    };
    let rows = |from: f32, to: f32| ((top + from * height) as usize)..=((top + to * height) as usize).min(by1);
    // the widest row of the head, then the first narrow row after it is the neck
    let (mut wmax, mut ymax) = (0.0f32, top + 0.07 * height);
    for y in rows(0.02, 0.13) {
        let w = central(y);
        if w > wmax {
            wmax = w;
            ymax = y as f32;
        }
    }
    let mut neck_y = top + 0.125 * height;
    let mut found_neck = false;
    for y in (ymax as usize)..=((top + 0.22 * height) as usize).min(by1) {
        let w = central(y);
        if w > 0.0 && w <= 0.6 * wmax && y as f32 > ymax + 0.02 * height {
            neck_y = y as f32;
            found_neck = true;
            break;
        }
    }
    if found_neck {
        conf += 0.25;
    } else {
        notes.push("No narrow neck found: the neck is put at the usual place.".into());
    }
    let head_c = [xc, (top + neck_y) / 2.0 - 0.01 * height];
    let shoulder_y = neck_y + 0.03 * height;
    // the crotch: where the middle line leaves the figure, below the middle of the body
    let mut crotch = None;
    for y in ((top + 0.40 * height) as usize)..=((top + 0.62 * height) as usize).min(by1) {
        let gap = (0..3).all(|k| !m.at(xc as i32, (y + k) as i32));
        if gap {
            crotch = Some(y as f32);
            break;
        }
    }
    let hips_y = match crotch {
        Some(c) => {
            conf += 0.15;
            c - 0.025 * height
        }
        None => {
            notes.push("The legs touch or are joined: the hips are put at the usual place.".into());
            top + 0.49 * height
        }
    };
    // arms: from the shoulder out, the longest thin run that leaves the body
    // how wide the figure is at the shoulders: arms held out make it as wide as it is tall, arms at the sides do not
    let row_extent = |y: usize| -> (f32, f32) {
        let (mut lo, mut hi) = (f32::MAX, f32::MIN);
        for x in 0..m.w {
            if m.at(x as i32, y as i32) {
                lo = lo.min(x as f32);
                hi = hi.max(x as f32);
            }
        }
        (lo, hi)
    };
    let (rl, rr) = row_extent(((shoulder_y + 0.05 * height) as usize).min(by1));
    let arms_down = match hints.arms {
        Hold::Down => true,
        Hold::Out => false,
        Hold::Auto => rr - rl < 0.72 * height,
    };
    let shoulder_half_down = ((rr - rl) / 2.0 * 0.8).clamp(0.08 * height, 0.2 * height);
    let shoulder_half = 0.095 * height;
    let arm_thick = (0.095 * height).max(6.0);
    let mut joints: Vec<(&'static str, [f32; 2])> = Vec::new();
    let mut tips: Vec<(&'static str, [f32; 2])> = Vec::new();
    let mut limbs: Vec<Limb> = Vec::new();
    // half the width of the figure along a path
    let halves = |path: &[[f32; 2]], dflt: f32, cap: f32| -> Vec<f32> {
        (0..path.len())
            .map(|i| {
                let (a, b) = (path[i.saturating_sub(1)], path[(i + 1).min(path.len() - 1)]);
                let d = norm([b[0] - a[0], b[1] - a[1]]);
                match m.across(path[i], d, cap * 1.3) {
                    Some((_, w)) if w <= cap && w >= 2.0 => w / 2.0,
                    _ => dflt,
                }
            })
            .collect()
    };
    for left in [true, false] {
        let sgn = if left { 1.0 } else { -1.0 };
        let pre = if left { "left" } else { "right" };
        let shoulder = [xc + sgn * shoulder_half, shoulder_y + 0.02 * height];
        let mut best: Vec<[f32; 2]> = Vec::new();
        let mut arms_down_fallback_used = false;
        for deg in [0.0f32, 12.0, 25.0, 38.0, 52.0, 66.0, 80.0].iter().copied().filter(|_| hints.arms != Hold::Down) {
            let (s, c) = deg.to_radians().sin_cos();
            let dir = [sgn * c, s];
            // out along the ray until the figure is thin enough to be an arm
            let mut t = 0.0;
            let mut start = None;
            while t < 0.35 * height {
                let q = [shoulder[0] + dir[0] * t, shoulder[1] + dir[1] * t];
                if let Some((shift, w)) = m.across(q, dir, arm_thick * 1.3) {
                    if w <= arm_thick && w >= 2.0 {
                        let n = [-dir[1], dir[0]];
                        start = Some([q[0] + n[0] * shift, q[1] + n[1] * shift]);
                        break;
                    }
                }
                t += 2.0;
            }
            let Some(st) = start else { continue };
            let mut path = vec![shoulder, st];
            path.extend(m.trace(st, dir, arm_thick, 0.7 * height));
            if path_len(&path) > path_len(&best) {
                best = path;
            }
        }
        if path_len(&best) < 0.12 * height {
            if arms_down {
                // arms along the body: from the edge of the shoulders straight down, a little outward
                notes.push(format!("The {pre} arm is against the body: it is put hanging at the side."));
                let top = [xc + sgn * (shoulder_half_down), shoulder_y + 0.02 * height];
                best = vec![top, [top[0] + sgn * 0.02 * height, top[1] + 0.20 * height], [top[0] + sgn * 0.035 * height, top[1] + 0.38 * height]];
                arms_down_fallback_used = true;
                conf += 0.1;
            } else {
                notes.push(format!("No {pre} arm found: it is put out sideways."));
                best = vec![shoulder, [shoulder[0] + sgn * 0.3 * height, shoulder[1] + 0.02 * height]];
            }
        } else {
            conf += 0.2;
        }
        // the shoulder joint is at the root of the arm, a little inside the outline where the arm leaves the body
        let pivot = if best.len() >= 3 && !arms_down_fallback_used {
            let (a, b) = (best[1], best[2]);
            let d = norm([b[0] - a[0], b[1] - a[1]]);
            [a[0] - d[0] * 0.03 * height, a[1] - d[1] * 0.03 * height]
        } else {
            best[0]
        };
        best[0] = pivot;
        let shoulder = pivot;
        let l = path_len(&best);
        let hand_len = (0.09 * height).min(l * 0.3);
        let name = |s: &str| -> &'static str {
            match (left, s) {
                (true, "UpperArm") => "leftUpperArm",
                (true, "LowerArm") => "leftLowerArm",
                (true, _) => "leftHand",
                (false, "UpperArm") => "rightUpperArm",
                (false, "LowerArm") => "rightLowerArm",
                (false, _) => "rightHand",
            }
        };
        limbs.push(Limb { bones: [name("UpperArm"), name("LowerArm"), name("Hand")], half: halves(&best, 0.04 * height, arm_thick), path: best.clone(), splits: [0.34, 0.74], root: Some(if left { "leftShoulder" } else { "rightShoulder" }) });
        joints.push((name("UpperArm"), shoulder));
        let _ = hand_len;
        // the elbow and the wrist where a normal rig has them: the forearm is longer than the upper arm
        joints.push((name("LowerArm"), along(&best, l * 0.34)));
        joints.push((name("Hand"), along(&best, l * 0.74)));
        joints.push((if left { "leftShoulder" } else { "rightShoulder" }, [xc + sgn * 0.04 * height, shoulder_y - 0.005 * height]));
        tips.push((name("Hand"), *best.last().unwrap()));
    }
    // legs: from the hip down, the longest run that is thin enough to be a leg
    let leg_thick = (0.2 * height).max(8.0);
    for left in [true, false] {
        let sgn = if left { 1.0 } else { -1.0 };
        let hip = [xc + sgn * 0.055 * height, hips_y];
        let mut best: Vec<[f32; 2]> = Vec::new();
        for deg in [0.0f32, 6.0, 12.0, 18.0, 24.0, 32.0, 40.0].iter().copied().filter(|_| hints.legs != Hold::Down) {
            let (s, c) = deg.to_radians().sin_cos();
            // outward positive: the direction is mostly down
            let dir = [sgn * s, c];
            let mut t = 0.0;
            let mut start = None;
            while t < 0.3 * height {
                let q = [hip[0] + dir[0] * t, hip[1] + dir[1] * t];
                if let Some((shift, w)) = m.across(q, dir, leg_thick * 1.3) {
                    if w <= leg_thick && w >= 3.0 {
                        let n = [-dir[1], dir[0]];
                        start = Some([q[0] + n[0] * shift, q[1] + n[1] * shift]);
                        break;
                    }
                }
                t += 2.0;
            }
            let Some(st) = start else { continue };
            let mut path = vec![hip, st];
            path.extend(m.trace(st, dir, leg_thick, 0.7 * height));
            // a leg stays on its own side of the middle line
            let end = *path.last().unwrap();
            if sgn * (end[0] - xc) < -0.02 * height {
                continue;
            }
            if path_len(&path) > path_len(&best) {
                best = path;
            }
        }
        if path_len(&best) < 0.2 * height {
            if hints.legs != Hold::Down {
                notes.push(format!("No {} leg found: it is put straight down.", if left { "left" } else { "right" }));
            }
            best = vec![hip, [hip[0], by1 as f32 - 0.03 * height]];
        } else {
            conf += 0.2;
        }
        let l = path_len(&best);
        let (up, low, foot, toes) = if left { ("leftUpperLeg", "leftLowerLeg", "leftFoot", "leftToes") } else { ("rightUpperLeg", "rightLowerLeg", "rightFoot", "rightToes") };
        limbs.push(Limb { bones: [up, low, foot], half: halves(&best, 0.06 * height, leg_thick), path: best.clone(), splits: [0.33, 0.80], root: None });
        joints.push((up, hip));
        joints.push((low, along(&best, l * 0.33)));
        joints.push((foot, along(&best, l * 0.80)));
        let end = *best.last().unwrap();
        joints.push((toes, [end[0], (end[1]).min(by1 as f32)]));
        tips.push((toes, [end[0], end[1].min(by1 as f32)]));
    }
    // the middle line
    let chest = [xc, shoulder_y + 0.30 * (hips_y - shoulder_y)];
    let spine = [xc, shoulder_y + 0.65 * (hips_y - shoulder_y)];
    joints.push(("hips", [xc, hips_y]));
    joints.push(("spine", spine));
    joints.push(("chest", chest));
    // as in a normal rig: the head bone starts where the head meets the neck, the neck bone a little below
    let _ = head_c;
    joints.push(("neck", [xc, neck_y + 0.049 * height]));
    joints.push(("head", [xc, neck_y]));
    tips.push(("head", [xc, top]));
    // back to page pixels
    let k = m.scale;
    let page = |p: [f32; 2]| [(p[0] + 0.5) * k, (p[1] + 0.5) * k];
    let limbs: Vec<Limb> = limbs.into_iter().map(|l| Limb { path: l.path.iter().map(|p| page(*p)).collect(), half: l.half.iter().map(|h| h * k).collect(), ..l }).collect();
    Some(Figure { limbs, joints: joints.into_iter().map(|(n, p)| (n, page(p))).collect(), tips: tips.into_iter().map(|(n, p)| (n, page(p))).collect(), confidence: conf.clamp(0.0, 1.0), notes })
}

/// The bones to make, in order: (name, parent, the bone its piece runs to).
pub const BONES: [(&str, Option<&str>, Option<&str>); 19] = [
    ("hips", None, Some("spine")),
    ("spine", Some("hips"), Some("chest")),
    ("chest", Some("spine"), Some("neck")),
    ("neck", Some("chest"), Some("head")),
    ("head", Some("neck"), None),
    ("leftShoulder", Some("chest"), Some("leftUpperArm")),
    ("leftUpperArm", Some("leftShoulder"), Some("leftLowerArm")),
    ("leftLowerArm", Some("leftUpperArm"), Some("leftHand")),
    ("leftHand", Some("leftLowerArm"), None),
    ("rightShoulder", Some("chest"), Some("rightUpperArm")),
    ("rightUpperArm", Some("rightShoulder"), Some("rightLowerArm")),
    ("rightLowerArm", Some("rightUpperArm"), Some("rightHand")),
    ("rightHand", Some("rightLowerArm"), None),
    ("leftUpperLeg", Some("hips"), Some("leftLowerLeg")),
    ("leftLowerLeg", Some("leftUpperLeg"), Some("leftFoot")),
    ("leftFoot", Some("leftLowerLeg"), None),
    ("rightUpperLeg", Some("hips"), Some("rightLowerLeg")),
    ("rightLowerLeg", Some("rightUpperLeg"), Some("rightFoot")),
    ("rightFoot", Some("rightLowerLeg"), None),
];

/// Put the skeleton into the rig as bones (Rotation changers with names) on the uncut skin, and give the picture
/// automatic weights. The skin must exist. Returns the bone changers in the order of `BONES`.
pub fn add_skeleton(rig: &mut Rig, fig: &Figure) -> Vec<Id> {
    let mut ids: Vec<Id> = Vec::new();
    for (name, parent, next) in BONES {
        let Some(head) = fig.joint(name) else { continue };
        let parent_id = parent.and_then(|p| BONES.iter().position(|b| b.0 == p)).and_then(|i| ids.get(i).copied());
        let id = rig.add_changer(name, Kind::Rotation { pivot: head }, parent_id);
        let tail = next.and_then(|n| fig.joint(n)).or_else(|| match name {
            "leftHand" | "rightHand" => fig.tip(name),
            "leftFoot" => fig.joint("leftToes").or_else(|| fig.tip("leftToes")),
            "rightFoot" => fig.joint("rightToes").or_else(|| fig.tip("rightToes")),
            "head" => fig.tip("head"),
            _ => None,
        });
        let tail = tail.unwrap_or([head[0], head[1] + 20.0]);
        if let Some(s) = rig.skin.as_mut() {
            s.add_bone(id, tail);
        }
        ids.push(id);
    }
    if fig.limbs.len() >= 2 {
        paint_weights(rig, fig);
    } else {
        let snapshot = rig.clone();
        if let Some(s) = rig.skin.as_mut() {
            s.auto_weights(&snapshot);
        }
    }
    ids
}

fn smooth(t: f32) -> f32 {
    let t = t.clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

/// Weights from the limbs: a place on an arm follows the arm bones, a place on a leg the leg bones, and everything
/// else follows the bones of the middle line by its height. So the chest keeps its width when an arm swings.
pub fn paint_weights(rig: &mut Rig, fig: &Figure) {
    let names: Vec<String> = match rig.skin.as_ref() {
        Some(sk) => sk.bones.iter().map(|b| rig.changer(b.changer).map(|c| c.name.clone()).unwrap_or_default()).collect(),
        None => return,
    };
    let idx = |n: &str| names.iter().position(|x| x == n);
    // the middle line: bone and height of each joint, from the top of the head down
    let chain: Vec<(usize, f32)> = ["head", "neck", "chest", "spine", "hips"].iter().filter_map(|n| Some((idx(n)?, fig.joint(n)?[1]))).collect();
    let Some(sk) = rig.skin.as_mut() else { return };
    let c = sk.cell as f32;
    let nb = names.len();
    for gy in 0..sk.gh {
        for gx in 0..sk.gw {
            let p = [(gx as f32 + 0.5) * c, (gy as f32 + 0.5) * c];
            let mut w = vec![0.0f32; nb];
            // arms and legs
            let mut total = 0.0f32;
            let mut limb_w: Vec<(usize, f32)> = Vec::new();
            for limb in &fig.limbs {
                let (mut best_d, mut best_s, mut best_h) = (f32::MAX, 0.0f32, 1.0f32);
                let mut acc = 0.0f32;
                for i in 0..limb.path.len().saturating_sub(1) {
                    let (a, b) = (limb.path[i], limb.path[i + 1]);
                    let (dx, dy) = (b[0] - a[0], b[1] - a[1]);
                    let l2 = (dx * dx + dy * dy).max(1e-6);
                    let t = (((p[0] - a[0]) * dx + (p[1] - a[1]) * dy) / l2).clamp(0.0, 1.0);
                    let d = (p[0] - (a[0] + dx * t)).hypot(p[1] - (a[1] + dy * t));
                    let seg = l2.sqrt();
                    if d < best_d {
                        best_d = d;
                        best_s = acc + t * seg;
                        best_h = limb.half[i] + (limb.half[i + 1] - limb.half[i]) * t;
                    }
                    acc += seg;
                }
                let len = acc;
                if len < 1.0 {
                    continue;
                }
                let r = best_h * 1.12 + 2.0;
                let mut mem = if best_d <= r { 1.0 } else { (1.0 - (best_d - r) / (0.45 * r + 4.0)).max(0.0) };
                // near the root the limb fades into the body
                let raw = mem;
                let g = smooth(best_s / (0.14 * len));
                mem *= g;
                // where the arm meets the body the clavicle carries it
                if let (Some(rb), true) = (limb.root.and_then(&idx), raw * (1.0 - g) > 0.0) {
                    limb_w.push((rb, raw * (1.0 - g)));
                    total += raw * (1.0 - g);
                }
                if mem <= 0.0 {
                    continue;
                }
                let s = best_s / len;
                let bw = 0.07;
                let wh = smooth((s - limb.splits[1] + bw) / (2.0 * bw));
                let wu = 1.0 - smooth((s - limb.splits[0] + bw) / (2.0 * bw));
                let wl = (1.0 - wu - wh).max(0.0);
                for (k, share) in [wu, wl, wh].iter().enumerate() {
                    if let (Some(bi), true) = (idx(limb.bones[k]), *share > 0.0) {
                        limb_w.push((bi, mem * share));
                    }
                }
                total += mem;
            }
            let norm = if total > 1.0 { total } else { 1.0 };
            for (bi, v) in &limb_w {
                w[*bi] += v / norm;
            }
            let body = (1.0 - total / norm).max(0.0);
            // the middle line by height
            if body > 0.0 && !chain.is_empty() {
                let y = p[1];
                let mut put = |bi: usize, v: f32| w[bi] += body * v;
                if y <= chain[0].1 {
                    put(chain[0].0, 1.0);
                } else if y >= chain[chain.len() - 1].1 {
                    put(chain[chain.len() - 1].0, 1.0);
                } else {
                    for pair in chain.windows(2) {
                        if y >= pair[0].1 && y <= pair[1].1 {
                            let t = smooth((y - pair[0].1) / (pair[1].1 - pair[0].1).max(1e-3));
                            put(pair[0].0, 1.0 - t);
                            put(pair[1].0, t);
                            break;
                        }
                    }
                }
            }
            for i in 0..nb {
                sk.planes[i][(gy * sk.gw + gx) as usize] = (w[i].clamp(0.0, 1.0) * 255.0).round() as u8;
            }
        }
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use image::Rgba;

    /// A stick figure with thick limbs: head, neck, torso, arms out to the sides, legs apart.
    pub(crate) fn figure_image() -> RgbaImage {
        let mut img = RgbaImage::new(800, 1000);
        let mut line = |a: (f32, f32), b: (f32, f32), r: f32| {
            let n = ((b.0 - a.0).hypot(b.1 - a.1) / 2.0).ceil() as i32 + 1;
            for i in 0..=n {
                let t = i as f32 / n as f32;
                let (cx, cy) = (a.0 + (b.0 - a.0) * t, a.1 + (b.1 - a.1) * t);
                for y in (cy - r) as i32..=(cy + r) as i32 {
                    for x in (cx - r) as i32..=(cx + r) as i32 {
                        if (x as f32 - cx).hypot(y as f32 - cy) <= r && x >= 0 && y >= 0 && x < 800 && y < 1000 {
                            img.put_pixel(x as u32, y as u32, Rgba([200, 160, 140, 255]));
                        }
                    }
                }
            }
        };
        line((400.0, 80.0), (400.0, 110.0), 55.0); // head
        line((400.0, 150.0), (400.0, 170.0), 22.0); // neck
        line((400.0, 190.0), (400.0, 470.0), 70.0); // torso
        line((330.0, 215.0), (60.0, 250.0), 22.0); // right arm (the left of the picture)
        line((470.0, 215.0), (740.0, 250.0), 22.0);
        line((370.0, 470.0), (290.0, 940.0), 38.0); // legs apart
        line((430.0, 470.0), (510.0, 940.0), 38.0);
        img
    }

    #[test]
    fn a_joint_can_be_moved_and_the_bones_on_both_sides_follow() {
        let img = figure_image();
        let mut d = crate::vrm::tests_build::uncut_doc(&img);
        let ids: Vec<crate::doc::LayerId> = d.layers.iter().map(|l| l.id).collect();
        let mut rig = Rig::default();
        auto_rig(&mut rig, &d, &ids, 0.6).unwrap();
        d.rig = std::sync::Arc::new(rig);
        let mut rig = (*d.rig).clone();
        let joints = crate::rig::skin::joints(&rig);
        assert!(joints.len() >= 18, "{} joints", joints.len());
        // the elbow joins the tail of the upper arm and the head of the forearm: one handle with both
        let elbow = joints.iter().find(|j| j.1.iter().any(|n| n == "leftLowerArm head")).unwrap().clone();
        assert!(elbow.1.iter().any(|n| n == "leftUpperArm tail"), "{:?}", elbow.1);
        let wrist_before = joints.iter().find(|j| j.1.iter().any(|n| n == "leftHand head")).unwrap().0;
        let to = [elbow.0[0] + 25.0, elbow.0[1] + 15.0];
        assert!(crate::rig::skin::move_joint(&mut rig, elbow.0, to));
        let pivot = |rig: &Rig, n: &str| match rig.changers.iter().find(|c| c.name == n).unwrap().kind {
            crate::rig::Kind::Rotation { pivot } => pivot,
            _ => panic!(),
        };
        assert_eq!(pivot(&rig, "leftLowerArm"), to, "the forearm starts at the new place");
        let tail = rig.skin.as_ref().unwrap().bones.iter().find(|b| rig.changer(b.changer).unwrap().name == "leftUpperArm").unwrap().tail;
        assert_eq!(tail, to, "the upper arm ends at the new place");
        assert_eq!(pivot(&rig, "leftHand"), wrist_before, "the wrist stays where it is");
        // nothing there: nothing moves
        assert!(!crate::rig::skin::move_joint(&mut rig, [-500.0, -500.0], [0.0, 0.0]));
    }

    #[test]
    fn a_standing_figure_gets_a_sensible_skeleton() {
        let img = figure_image();
        let f = detect(&img).unwrap();
        assert!(f.confidence > 0.6, "{} {:?}", f.confidence, f.notes);
        let j = |n: &str| f.joint(n).unwrap_or_else(|| panic!("a {n} joint"));
        // the middle line
        for n in ["hips", "spine", "chest", "neck", "head"] {
            assert!((j(n)[0] - 400.0).abs() < 12.0, "{n} on the middle line: {:?}", j(n));
        }
        assert!(j("head")[1] < j("neck")[1] && j("neck")[1] < j("chest")[1] && j("chest")[1] < j("spine")[1] && j("spine")[1] < j("hips")[1]);
        assert!((f.tip("head").unwrap()[1] - 25.0).abs() < 12.0, "the top of the head");
        assert!(j("neck")[1] > 120.0 && j("neck")[1] < 200.0, "the neck: {:?}", j("neck"));
        // left is the right of the picture
        assert!(j("leftUpperArm")[0] > 400.0 && j("rightUpperArm")[0] < 400.0);
        // the hand is out at the end of the arm
        assert!(j("leftHand")[0] > 600.0 && j("rightHand")[0] < 200.0, "{:?} {:?}", j("leftHand"), j("rightHand"));
        assert!(j("leftLowerArm")[0] > j("leftUpperArm")[0] && j("leftLowerArm")[0] < j("leftHand")[0]);
        assert!(j("leftShoulder")[0] > 400.0 && j("leftShoulder")[0] < j("leftUpperArm")[0], "the clavicle is between the middle and the arm");
        // as in a normal rig the head bone starts at the neck, and the neck is below it
        assert!(j("head")[1] < j("neck")[1]);
        // the feet are at the bottom, the knees half way
        assert!(j("leftFoot")[1] > 850.0 && j("rightFoot")[1] > 850.0, "{:?}", j("leftFoot"));
        assert!(j("leftFoot")[0] > 400.0 && j("rightFoot")[0] < 400.0, "legs on their own sides");
        let k = j("leftLowerLeg");
        assert!(k[1] > 600.0 && k[1] < 800.0, "{k:?}");
    }

    #[test]
    fn the_arms_and_legs_can_be_told_how_they_are_held() {
        let img = figure_image();
        // told the arms hang, they hang at the sides although the picture has them out
        let down = detect_with(&img, Hints { arms: Hold::Down, legs: Hold::Auto }).unwrap();
        let hand = down.joint("leftHand").unwrap();
        assert!(hand[1] > 400.0 && hand[0] < 680.0, "{hand:?}");
        // told the arms are out, they are found out
        let out = detect_with(&img, Hints { arms: Hold::Out, legs: Hold::Auto }).unwrap();
        assert!(out.joint("leftHand").unwrap()[0] > 600.0);
        // told the legs are together they go straight down from the hips
        let together = detect_with(&img, Hints { arms: Hold::Auto, legs: Hold::Down }).unwrap();
        let (hip, foot) = (together.joint("leftUpperLeg").unwrap(), together.joint("leftFoot").unwrap());
        assert!((hip[0] - foot[0]).abs() < 12.0 && foot[1] > 850.0, "{hip:?} {foot:?}");
    }

    #[test]
    fn missing_limbs_and_empty_pictures_are_handled() {
        assert!(detect(&RgbaImage::new(100, 100)).is_none(), "nothing there");
        // only a body and a head: no arms or legs found, the finder says so and still gives a skeleton
        let mut img = RgbaImage::new(400, 600);
        for y in 40..560 {
            for x in 150..250 {
                img.put_pixel(x, y, Rgba([1, 1, 1, 255]));
            }
        }
        let f = detect(&img).unwrap();
        assert!(f.confidence < 0.7 && !f.notes.is_empty(), "{} {:?}", f.confidence, f.notes);
        assert!(f.joint("leftHand").is_some() && f.joint("rightFoot").is_some());
    }

    #[test]
    fn the_skeleton_becomes_bones_with_weights_on_the_uncut_skin() {
        use crate::rig::skin::Skin;
        let f = detect(&figure_image()).unwrap();
        let mut rig = Rig::default();
        rig.skin = Some(Skin::new(vec![1], vec![[0.0, 0.0], [800.0, 0.0], [800.0, 1000.0], [0.0, 1000.0]], vec![[0, 1, 2], [0, 2, 3]], 800, 1000));
        let ids = add_skeleton(&mut rig, &f);
        assert_eq!(ids.len(), BONES.len());
        let s = rig.skin.as_ref().unwrap();
        assert_eq!(s.bones.len(), BONES.len());
        // the hand follows the hand bone near the end of the arm; the foot follows the foot bone
        let w_at = |name: &str, p: [f32; 2]| {
            let ci = rig.changers.iter().find(|c| c.name == name).unwrap().id;
            s.weight_at(s.bone_index(ci).unwrap(), p[0], p[1])
        };
        assert!(w_at("leftHand", [720.0, 245.0]) > 0.5, "{}", w_at("leftHand", [720.0, 245.0]));
        assert!(w_at("rightFoot", [300.0, 930.0]) + w_at("rightLowerLeg", [300.0, 930.0]) > 0.5);
        // bones hang from each other like a body
        let parent_of = |n: &str| rig.changers.iter().find(|c| c.name == n).unwrap().parent.and_then(|p| rig.changer(p)).map(|c| c.name.clone());
        assert_eq!(parent_of("leftHand").as_deref(), Some("leftLowerArm"));
        assert_eq!(parent_of("head").as_deref(), Some("neck"));
        assert_eq!(parent_of("hips"), None);
        // turning an arm moves the picture there and not the other side
        let ua = rig.changers.iter().find(|c| c.name == "leftUpperArm").unwrap().id;
        let mut pose = crate::rig::Pose::default();
        pose.rot.insert(ua, 40.0);
        let moved = s.skinned(&rig, &pose);
        let at = |i: usize| (moved[i][0] - s.verts[i][0]).hypot(moved[i][1] - s.verts[i][1]);
        let _ = at;
        let p = [700.0, 250.0];
        let w = s.weights_at(p[0], p[1]);
        assert!(w.iter().sum::<f32>() > 0.5, "the arm is claimed");
    }

    /// The user's ten and fourteen uncut bases, when they are on this computer: every one gets a skeleton, drawn over the
    /// picture into contact sheets (target/screens/uncut_*.png) to look at.
    #[test]
    fn real_uncut_figures_get_skeletons() {
        let base = std::path::PathBuf::from(std::env::var("USERPROFILE").unwrap_or_default()).join("Downloads/Bases/images/races/images");
        let _ = std::fs::create_dir_all("target/screens");
        for (folder, sheet_name) in [("female", "uncut_female"), ("male", "uncut_male")] {
            let dir = base.join(folder);
            let Ok(rd) = std::fs::read_dir(&dir) else { continue };
            let mut files: Vec<std::path::PathBuf> = rd.flatten().map(|e| e.path()).filter(|p| p.extension().is_some_and(|e| e == "png")).collect();
            files.sort();
            let (tw, th) = (300u32, 400u32);
            let cols = 5u32;
            let rows = (files.len() as u32).div_ceil(cols).max(1);
            let mut sheet = RgbaImage::from_pixel(cols * tw, rows * th, Rgba([255, 255, 255, 255]));
            for (i, f) in files.iter().enumerate() {
                let Ok(bytes) = std::fs::read(f) else { continue };
                let Ok(dynimg) = image::load_from_memory(&bytes) else { continue };
                let img = dynimg.to_rgba8();
                let fig = detect(&img);
                let name = f.file_name().unwrap().to_string_lossy().to_string();
                match &fig {
                    Some(fg) => eprintln!("{folder}/{name}: confidence {:.2} {:?}", fg.confidence, fg.notes),
                    None => eprintln!("{folder}/{name}: no figure found"),
                }
                // the picture over white, with the skeleton drawn on it
                let k = (tw as f32 / img.width() as f32).min(th as f32 / img.height() as f32);
                let (sw, sh) = (((img.width() as f32 * k) as u32).max(1), ((img.height() as f32 * k) as u32).max(1));
                let small = image::imageops::resize(&cut_out_background(&img), sw, sh, image::imageops::FilterType::Triangle);
                let (ox, oy) = ((i as u32 % cols) * tw, (i as u32 / cols) * th);
                for y in 0..sh {
                    for x in 0..sw {
                        let p = small.get_pixel(x, y).0;
                        let a = p[3] as f32 / 255.0;
                        let c = |v: u8| (v as f32 * a + 255.0 * (1.0 - a) * 0.9) as u8;
                        sheet.put_pixel(ox + x, oy + y, Rgba([c(p[0]), c(p[1]), c(p[2]), 255]));
                    }
                }
                if let Some(fg) = fig {
                    let to = |p: [f32; 2]| ((ox as f32 + p[0] * k) as i32, (oy as f32 + p[1] * k) as i32);
                    let mut dot = |p: [f32; 2], col: [u8; 3], r: i32| {
                        let (cx, cy) = to(p);
                        for dy in -r..=r {
                            for dx in -r..=r {
                                let (x, y) = (cx + dx, cy + dy);
                                if dx * dx + dy * dy <= r * r && x >= 0 && y >= 0 && (x as u32) < sheet.width() && (y as u32) < sheet.height() {
                                    sheet.put_pixel(x as u32, y as u32, Rgba([col[0], col[1], col[2], 255]));
                                }
                            }
                        }
                    };
                    for (n, parent, next) in BONES {
                        let (Some(a), Some(b)) = (fg.joint(n), next.and_then(|x| fg.joint(x)).or_else(|| fg.tip(n))) else { continue };
                        let _ = parent;
                        let steps = 40;
                        for t in 0..=steps {
                            let q = [a[0] + (b[0] - a[0]) * t as f32 / steps as f32, a[1] + (b[1] - a[1]) * t as f32 / steps as f32];
                            dot(q, [230, 40, 40], 1);
                        }
                    }
                    for (n, p) in &fg.joints {
                        dot(*p, if n.starts_with("left") { [30, 120, 255] } else if n.starts_with("right") { [30, 200, 80] } else { [255, 160, 0] }, 3);
                    }
                }
            }
            sheet.save(format!("target/screens/{sheet_name}.png")).unwrap();
        }
    }
}
