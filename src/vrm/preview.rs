// SPDX-License-Identifier: GPL-3.0-or-later
// A small software picture of the 3D character, for the preview in the app and for the tests: turn it round, and show an
// expression. Straight on (no perspective), with the picture of every part on its card and a little shading.

use image::{Rgba, RgbaImage};

use super::model::Model3d;

/// Draw the character turned `yaw_deg` degrees round the up axis. `active` are expression shapes by name and amount.
pub fn render(model: &Model3d, yaw_deg: f32, w: u32, h: u32, active: &[(&str, f32)]) -> RgbaImage {
    let mut img = RgbaImage::from_pixel(w, h, Rgba([244, 242, 236, 255]));
    let mut depth = vec![f32::MIN; (w * h) as usize];
    let (s, c) = yaw_deg.to_radians().sin_cos();
    let top = model.parts.iter().flat_map(|p| p.verts.iter().map(|v| v[1])).fold(0.0f32, f32::max).max(0.1);
    let scale = h as f32 * 0.9 / top;
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
                    img.put_pixel(x as u32, y as u32, Rgba([(p[0] as f32 * shade) as u8, (p[1] as f32 * shade) as u8, (p[2] as f32 * shade) as u8, 255]));
                }
            }
        }
    }
    img
}
