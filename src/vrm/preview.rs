// SPDX-License-Identifier: GPL-3.0-or-later
// A small software picture of the 3D character, for the preview in the app and for the tests: turn it round, and show an
// expression. Straight on (no perspective), with the picture of every part on its card and a little shading.

use image::{Rgba, RgbaImage};

use super::model::Model3d;

/// Draw the character turned `yaw_deg` degrees round the up axis. `active` are expression shapes by name and amount.
pub fn render(model: &Model3d, yaw_deg: f32, w: u32, h: u32, active: &[(&str, f32)]) -> RgbaImage {
    render_posed(model, yaw_deg, w, h, active, &[])
}

/// What is under the pointer on the preview: the part of the model and the place on its picture (u, v in 0..1) of the front-most
/// point there, or None when the pointer is over nothing. Same view as `render` (no pose, no expression).
pub fn pick(model: &Model3d, yaw_deg: f32, w: u32, h: u32, px: f32, py: f32) -> Option<(usize, [f32; 2])> {
    let (s, c) = yaw_deg.to_radians().sin_cos();
    let top = model.parts.iter().flat_map(|p| p.verts.iter().map(|v| v[1])).fold(0.0f32, f32::max).max(0.1);
    let scale = h as f32 * 0.9 / top;
    let mut best: Option<(f32, usize, [f32; 2])> = None;
    for (pi, part) in model.parts.iter().enumerate() {
        let screen: Vec<[f32; 3]> = part.verts.iter().map(|v| [w as f32 / 2.0 + (v[0] * c + v[2] * s) * scale, h as f32 * 0.95 - v[1] * scale, -v[0] * s + v[2] * c]).collect();
        for t in &part.tris {
            let (a, b, cc) = (screen[t[0] as usize], screen[t[1] as usize], screen[t[2] as usize]);
            if px < a[0].min(b[0]).min(cc[0]) - 0.5 || px > a[0].max(b[0]).max(cc[0]) + 0.5 || py < a[1].min(b[1]).min(cc[1]) - 0.5 || py > a[1].max(b[1]).max(cc[1]) + 0.5 {
                continue;
            }
            let det = (b[1] - cc[1]) * (a[0] - cc[0]) + (cc[0] - b[0]) * (a[1] - cc[1]);
            if det.abs() < 1e-6 {
                continue;
            }
            let l1 = ((b[1] - cc[1]) * (px - cc[0]) + (cc[0] - b[0]) * (py - cc[1])) / det;
            let l2 = ((cc[1] - a[1]) * (px - cc[0]) + (a[0] - cc[0]) * (py - cc[1])) / det;
            let l3 = 1.0 - l1 - l2;
            if l1 < -0.001 || l2 < -0.001 || l3 < -0.001 {
                continue;
            }
            let z = a[2] * l1 + b[2] * l2 + cc[2] * l3;
            if best.as_ref().is_none_or(|b| z > b.0) {
                let (ua, ub, uc) = (part.uvs[t[0] as usize], part.uvs[t[1] as usize], part.uvs[t[2] as usize]);
                best = Some((z, pi, [ua[0] * l1 + ub[0] * l2 + uc[0] * l3, ua[1] * l1 + ub[1] * l2 + uc[1] * l3]));
            }
        }
    }
    best.map(|b| (b.1, b.2))
}

/// The same picture with guide lines at the places of the features: the eyes, the nose and the mouth where the texture puts them
/// (cyan) and the nose and the chin of the shape itself (pink), each as a line across the face and a dot where it is on the model.
pub fn render_guides(model: &Model3d, yaw_deg: f32, w: u32, h: u32, active: &[(&str, f32)], show: bool) -> RgbaImage {
    let mut img = render_posed(model, yaw_deg, w, h, active, &[]);
    if !show {
        return img;
    }
    let (s, c) = yaw_deg.to_radians().sin_cos();
    let top = model.parts.iter().flat_map(|p| p.verts.iter().map(|v| v[1])).fold(0.0f32, f32::max).max(0.1);
    let scale = h as f32 * 0.9 / top;
    for (name, p) in &model.landmarks {
        let (x, y) = (w as f32 / 2.0 + (p[0] * c + p[2] * s) * scale, h as f32 * 0.95 - p[1] * scale);
        let col = if name.starts_with("mesh") { Rgba([255, 0, 200, 255]) } else { Rgba([0, 190, 220, 255]) };
        let half = 0.1 * scale;
        let mut px = |ix: i32, iy: i32, col: Rgba<u8>| {
            if ix >= 0 && iy >= 0 && (ix as u32) < w && (iy as u32) < h {
                img.put_pixel(ix as u32, iy as u32, col);
            }
        };
        for dx in (-half as i32)..=(half as i32) {
            if dx % 6 < 4 {
                px(x as i32 + dx, y as i32, col);
            }
        }
        for dy in -3..=3 {
            for dx in -3..=3 {
                px(x as i32 + dx, y as i32 + dy, col);
            }
        }
    }
    img
}

/// Where a point goes when the bones are turned: `pose` are bone names with an angle in degrees (counter-clockwise seen
/// from the front), every bone turning about its own joint together with the bones above it.
fn pose_point(model: &Model3d, bone: usize, turn: &[f32], p: [f32; 3]) -> [f32; 3] {
    let b = &model.bones[bone];
    let a = turn[bone].to_radians();
    let (s, c) = a.sin_cos();
    let (x, y) = (p[0] - b.pos[0], p[1] - b.pos[1]);
    let q = [b.pos[0] + x * c - y * s, b.pos[1] + x * s + y * c, p[2]];
    match b.parent {
        Some(par) => pose_point(model, par, turn, q),
        None => q,
    }
}

/// How the skeleton is posed: a turn in the picture plane for some bones, or a full 3D turn (x, y, z in degrees as `motion.rs` gives them)
/// for the bones and a move of the hips.
enum Posing {
    Plane(Vec<f32>),
    Full(Vec<[f32; 3]>, [f32; 3]),
}

impl Posing {
    fn active(&self) -> bool {
        match self {
            Posing::Plane(t) => t.iter().any(|v| *v != 0.0),
            Posing::Full(r, h) => *h != [0.0; 3] || r.iter().any(|v| *v != [0.0; 3]),
        }
    }

    /// Where a point of the model goes when it hangs on `bone`.
    fn point(&self, model: &Model3d, bone: usize, p: [f32; 3]) -> [f32; 3] {
        match self {
            Posing::Plane(turn) => pose_point(model, bone, turn, p),
            Posing::Full(rots, hips) => {
                let q = pose_point3(model, bone, rots, p);
                [q[0] + hips[0], q[1] + hips[1], q[2] + hips[2]]
            }
        }
    }
}

/// A full 3D turn of the bones, parent after parent (the same as `pose_point`, with turns about every axis).
fn pose_point3(model: &Model3d, bone: usize, rots: &[[f32; 3]], p: [f32; 3]) -> [f32; 3] {
    let b = &model.bones[bone];
    let d = [p[0] - b.pos[0], p[1] - b.pos[1], p[2] - b.pos[2]];
    let r = crate::motion::euler_apply(rots[bone], d);
    let q = [b.pos[0] + r[0], b.pos[1] + r[1], b.pos[2] + r[2]];
    match b.parent {
        Some(par) => pose_point3(model, par, rots, q),
        None => q,
    }
}

/// The character in a motion's frame (full 3D turns of the bones, the hips moved): `room` more than 1 leaves room above for a jump.
pub fn render_motion(model: &Model3d, yaw_deg: f32, w: u32, h: u32, frame: &crate::motion::Frame, room: f32) -> RgbaImage {
    let mut rots = vec![[0.0f32; 3]; model.bones.len()];
    for (name, r) in &frame.rot {
        if let Some(i) = model.bone(name) {
            rots[i] = *r;
        }
    }
    render_core(model, yaw_deg, w, h, &[], &Posing::Full(rots, frame.hips), room, MapKind::Colour)
}

/// The same picture with the skeleton posed (the skin weights move the parts).
pub fn render_posed(model: &Model3d, yaw_deg: f32, w: u32, h: u32, active: &[(&str, f32)], pose: &[(&str, f32)]) -> RgbaImage {
    let mut turn = vec![0.0f32; model.bones.len()];
    for (n, a) in pose {
        if let Some(i) = model.bone(n) {
            turn[i] = *a;
        }
    }
    render_core(model, yaw_deg, w, h, active, &Posing::Plane(turn), 1.0, MapKind::Colour)
}

/// What a picture of the model shows.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum MapKind {
    /// The model with its picture and a light.
    Colour,
    /// How far every point is: white is near, black is far (and nothing is black).
    Depth,
    /// The direction of the surface: red to the right, green up, blue to the viewer (a flat normal map colour where there is nothing).
    Normals,
}

/// The depth or the normals of the model seen from a side (the angle turns it round the up axis), as a picture the size of `render_posed`'s:
/// for pictures AIs that follow depth or normal maps, and for projecting pictures back on the model. Only what the texture shows counts.
pub fn render_map(model: &Model3d, yaw_deg: f32, w: u32, h: u32, kind: MapKind) -> RgbaImage {
    render_core(model, yaw_deg, w, h, &[], &Posing::Plane(vec![0.0; model.bones.len()]), 1.0, kind)
}

fn render_core(model: &Model3d, yaw_deg: f32, w: u32, h: u32, active: &[(&str, f32)], posing: &Posing, room: f32, kind: MapKind) -> RgbaImage {
    let background = match kind {
        MapKind::Colour => Rgba([244, 242, 236, 255]),
        MapKind::Depth => Rgba([0, 0, 0, 255]),
        MapKind::Normals => Rgba([128, 128, 255, 255]),
    };
    let mut img = RgbaImage::from_pixel(w, h, background);
    let mut depth = vec![f32::MIN; (w * h) as usize];
    let (s, c) = yaw_deg.to_radians().sin_cos();
    let top = model.parts.iter().flat_map(|p| p.verts.iter().map(|v| v[1])).fold(0.0f32, f32::max).max(0.1);
    let scale = h as f32 * 0.9 / (top * room.max(1.0));
    for part in &model.parts {
        // the shapes of the expressions that are on
        let mut verts = part.verts.clone();
        for m in &part.morphs {
            if let Some((_, wt)) = active.iter().find(|(n, _)| *n == m.name) {
                for (v, d) in verts.iter_mut().zip(&m.deltas) {
                    for k in 0..3 {
                        v[k] += d[k] * wt;
                    }
                }
            }
        }
        // the skeleton's pose moves the vertices by their skin weights
        if posing.active() {
            let rest = verts.clone();
            for (i, v) in verts.iter_mut().enumerate() {
                let mut acc = [0.0f32; 3];
                for k in 0..4 {
                    let wt = part.weights[i][k];
                    if wt > 0.0 {
                        let q = posing.point(model, part.joints[i][k] as usize, rest[i]);
                        for a in 0..3 {
                            acc[a] += q[a] * wt;
                        }
                    }
                }
                *v = acc;
            }
        }
        // turned, then onto the picture: x across, y up, z towards the viewer
        let screen: Vec<[f32; 3]> = verts.iter().map(|v| [w as f32 / 2.0 + (v[0] * c + v[2] * s) * scale, h as f32 * 0.95 - v[1] * scale, -v[0] * s + v[2] * c]).collect();
        let (iw, ih) = (part.image.width() as f32, part.image.height() as f32);
        for t in &part.tris {
            let (a, b, cc) = (screen[t[0] as usize], screen[t[1] as usize], screen[t[2] as usize]);
            let det = (b[1] - cc[1]) * (a[0] - cc[0]) + (cc[0] - b[0]) * (a[1] - cc[1]);
            if det.abs() < 1e-6 {
                continue;
            }
            let n = part.normals[t[0] as usize];
            let shade = 0.72 + 0.28 * (-n[0] * s + n[2] * c).abs();
            let (x0, x1) = ((a[0].min(b[0]).min(cc[0]).floor().max(0.0)) as i32, (a[0].max(b[0]).max(cc[0]).ceil().min(w as f32 - 1.0)) as i32);
            let (y0, y1) = ((a[1].min(b[1]).min(cc[1]).floor().max(0.0)) as i32, (a[1].max(b[1]).max(cc[1]).ceil().min(h as f32 - 1.0)) as i32);
            for y in y0..=y1 {
                for x in x0..=x1 {
                    let (px, py) = (x as f32 + 0.5, y as f32 + 0.5);
                    let l1 = ((b[1] - cc[1]) * (px - cc[0]) + (cc[0] - b[0]) * (py - cc[1])) / det;
                    let l2 = ((cc[1] - a[1]) * (px - cc[0]) + (a[0] - cc[0]) * (py - cc[1])) / det;
                    let l3 = 1.0 - l1 - l2;
                    if l1 < -0.001 || l2 < -0.001 || l3 < -0.001 {
                        continue;
                    }
                    let z = a[2] * l1 + b[2] * l2 + cc[2] * l3;
                    let di = (y as u32 * w + x as u32) as usize;
                    if z <= depth[di] {
                        continue;
                    }
                    let (ua, ub, uc) = (part.uvs[t[0] as usize], part.uvs[t[1] as usize], part.uvs[t[2] as usize]);
                    let (u, v) = (ua[0] * l1 + ub[0] * l2 + uc[0] * l3, ua[1] * l1 + ub[1] * l2 + uc[1] * l3);
                    let (tx, ty) = (((u * iw) as i32).clamp(0, part.image.width() as i32 - 1), ((v * ih) as i32).clamp(0, part.image.height() as i32 - 1));
                    let p = part.image.get_pixel(tx as u32, ty as u32).0;
                    if p[3] < 128 {
                        continue;
                    }
                    depth[di] = z;
                    match kind {
                        MapKind::Colour => img.put_pixel(x as u32, y as u32, Rgba([(p[0] as f32 * shade) as u8, (p[1] as f32 * shade) as u8, (p[2] as f32 * shade) as u8, 255])),
                        MapKind::Depth => {}
                        MapKind::Normals => {
                            // the smooth normal of the point, turned with the model
                            let (na, nb, nc) = (part.normals[t[0] as usize], part.normals[t[1] as usize], part.normals[t[2] as usize]);
                            let m = [na[0] * l1 + nb[0] * l2 + nc[0] * l3, na[1] * l1 + nb[1] * l2 + nc[1] * l3, na[2] * l1 + nb[2] * l2 + nc[2] * l3];
                            let v = [m[0] * c + m[2] * s, m[1], -m[0] * s + m[2] * c];
                            let len = (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt().max(1e-9);
                            let q = |x: f32| ((x / len * 0.5 + 0.5).clamp(0.0, 1.0) * 255.0) as u8;
                            img.put_pixel(x as u32, y as u32, Rgba([q(v[0]), q(v[1]), q(v[2]), 255]));
                        }
                    }
                }
            }
        }
    }
    if kind == MapKind::Depth {
        let seen: Vec<f32> = depth.iter().copied().filter(|d| *d > f32::MIN).collect();
        if let (Some(lo), Some(hi)) = (seen.iter().copied().reduce(f32::min), seen.iter().copied().reduce(f32::max)) {
            for (i, d) in depth.iter().enumerate() {
                if *d > f32::MIN {
                    // near (a big z) is white; nothing that is there is quite black, so the picture says where the model is
                    let v = 20.0 + 235.0 * ((*d - lo) / (hi - lo).max(1e-6));
                    img.put_pixel((i as u32) % w, (i as u32) / w, Rgba([v as u8, v as u8, v as u8, 255]));
                }
            }
        }
    }
    img
}
