// SPDX-License-Identifier: GPL-3.0-or-later
// Painting straight on the 3D character, as in VRoid: the point of the model under the pointer gives a place on the picture
// of the model, and a round brush puts paint there. The paint is kept as a picture of its own (the size of the whole picture on
// the model) that is laid over the texture whenever the character is built, and is also put into the pictures of the parts at
// once, so the preview and the saved file show it without making the character again. Tools: brush, eraser, blend (smudges
// with the colours around) and an eyedropper.

use image::{Rgba, RgbaImage};

use super::model::Model3d;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Tool {
    Brush,
    Eraser,
    Blend,
    Dropper,
}

#[derive(Clone, Debug)]
pub struct Brush {
    pub tool: Tool,
    pub colour: [u8; 3],
    /// Radius in pixels of the picture on the model.
    pub size: f32,
    /// 0 soft edge all through, 1 hard edge.
    pub hardness: f32,
    pub opacity: f32,
}

impl Default for Brush {
    fn default() -> Self {
        Brush { tool: Tool::Brush, colour: [200, 60, 60], size: 14.0, hardness: 0.5, opacity: 0.9 }
    }
}

fn falloff(r: f32, hardness: f32) -> f32 {
    if r >= 1.0 {
        0.0
    } else if r <= hardness {
        1.0
    } else {
        let t = (1.0 - r) / (1.0 - hardness).max(1e-3);
        t * t * (3.0 - 2.0 * t)
    }
}

/// The part pictures of a model that are the whole picture on the model (they share one size).
fn part_images(model: &mut Model3d, dims: (u32, u32)) -> Vec<&mut RgbaImage> {
    model.parts.iter_mut().map(|p| &mut p.image).filter(|i| i.dimensions() == dims).collect()
}

/// Put one dab of the brush at `(cx, cy)` on the picture of the model. `overlay` is the paint kept for the project. Returns the colour
/// under the dab for the eyedropper, and whether the pictures of the parts could not show the change at once (an eraser needs the
/// model to be made again).
pub fn dab(model: &mut Model3d, overlay: &mut RgbaImage, brush: &Brush, cx: f32, cy: f32) -> (Option<[u8; 3]>, bool) {
    let dims = overlay.dimensions();
    let (w, h) = (dims.0 as i32, dims.1 as i32);
    let r = brush.size.max(1.0);
    let x0 = ((cx - r).floor() as i32).max(0);
    let x1 = ((cx + r).ceil() as i32).min(w - 1);
    let y0 = ((cy - r).floor() as i32).max(0);
    let y1 = ((cy + r).ceil() as i32).min(h - 1);
    if x1 < x0 || y1 < y0 {
        return (None, false);
    }
    // the colour under the middle (from the first picture of the model that has the whole size)
    let under = {
        let imgs = part_images(model, dims);
        imgs.first().map(|i| {
            let p = i.get_pixel((cx.round() as i32).clamp(0, w - 1) as u32, (cy.round() as i32).clamp(0, h - 1) as u32).0;
            [p[0], p[1], p[2]]
        })
    };
    match brush.tool {
        Tool::Dropper => return (under, false),
        Tool::Eraser => {
            for y in y0..=y1 {
                for x in x0..=x1 {
                    let d = ((x as f32 - cx).powi(2) + (y as f32 - cy).powi(2)).sqrt() / r;
                    let a = falloff(d, brush.hardness) * brush.opacity;
                    if a > 0.0 {
                        let p = overlay.get_pixel_mut(x as u32, y as u32);
                        p.0[3] = (p.0[3] as f32 * (1.0 - a)) as u8;
                    }
                }
            }
            return (None, true);
        }
        _ => {}
    }
    // blend: the colour is the average of the colours around, so edges soften
    let colour = if brush.tool == Tool::Blend {
        let imgs = part_images(model, dims);
        let Some(img) = imgs.first() else { return (None, false) };
        let (mut sum, mut n) = ([0f32; 3], 0f32);
        let step = (r / 4.0).max(1.0);
        let mut yy = y0 as f32;
        while yy <= y1 as f32 {
            let mut xx = x0 as f32;
            while xx <= x1 as f32 {
                if ((xx - cx).powi(2) + (yy - cy).powi(2)).sqrt() <= r {
                    let p = img.get_pixel(xx as u32, yy as u32).0;
                    for k in 0..3 {
                        sum[k] += p[k] as f32;
                    }
                    n += 1.0;
                }
                xx += step;
            }
            yy += step;
        }
        if n < 1.0 {
            return (None, false);
        }
        [(sum[0] / n) as u8, (sum[1] / n) as u8, (sum[2] / n) as u8]
    } else {
        brush.colour
    };
    let strength = if brush.tool == Tool::Blend { brush.opacity * 0.5 } else { brush.opacity };
    for y in y0..=y1 {
        for x in x0..=x1 {
            let d = ((x as f32 - cx).powi(2) + (y as f32 - cy).powi(2)).sqrt() / r;
            let a = falloff(d, brush.hardness) * strength;
            if a <= 0.0 {
                continue;
            }
            // the kept paint: colour over colour
            let o = overlay.get_pixel(x as u32, y as u32).0;
            let oa = o[3] as f32 / 255.0;
            let na = a + oa * (1.0 - a);
            let mix = |c: u8, oc: u8| ((c as f32 * a + oc as f32 * oa * (1.0 - a)) / na.max(1e-6)) as u8;
            overlay.put_pixel(x as u32, y as u32, Rgba([mix(colour[0], o[0]), mix(colour[1], o[1]), mix(colour[2], o[2]), (na * 255.0) as u8]));
        }
    }
    // and into the pictures of the parts, so it shows at once
    for img in part_images(model, dims) {
        for y in y0..=y1 {
            for x in x0..=x1 {
                let d = ((x as f32 - cx).powi(2) + (y as f32 - cy).powi(2)).sqrt() / r;
                let a = falloff(d, brush.hardness) * strength;
                if a > 0.0 {
                    let p = img.get_pixel(x as u32, y as u32).0;
                    let m = |b: u8, c: u8| (b as f32 * (1.0 - a) + c as f32 * a) as u8;
                    img.put_pixel(x as u32, y as u32, Rgba([m(p[0], colour[0]), m(p[1], colour[1]), m(p[2], colour[2]), 255]));
                }
            }
        }
    }
    (None, false)
}

/// Paint along a line from the last place to this one (pixels of the picture of the model), a dab every few pixels.
pub fn stroke(model: &mut Model3d, overlay: &mut RgbaImage, brush: &Brush, from: [f32; 2], to: [f32; 2]) -> (Option<[u8; 3]>, bool) {
    let len = (to[0] - from[0]).hypot(to[1] - from[1]);
    let step = (brush.size * 0.3).max(1.0);
    let n = (len / step).ceil().max(1.0) as usize;
    let (mut picked, mut rebuild) = (None, false);
    for i in 1..=n {
        let t = i as f32 / n as f32;
        let (c, r) = dab(model, overlay, brush, from[0] + (to[0] - from[0]) * t, from[1] + (to[1] - from[1]) * t);
        picked = c.or(picked);
        rebuild |= r;
    }
    (picked, rebuild)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::vrm::model::{Model3d, Part3d};

    fn model(w: u32, h: u32) -> Model3d {
        let part = Part3d {
            name: "Body".into(),
            layer: 0,
            verts: vec![[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]],
            normals: vec![[0.0, 0.0, 1.0]; 3],
            uvs: vec![[0.0, 0.0], [1.0, 0.0], [0.0, 1.0]],
            tris: vec![[0, 1, 2]],
            joints: vec![[0; 4]; 3],
            weights: vec![[1.0, 0.0, 0.0, 0.0]; 3],
            image: RgbaImage::from_pixel(w, h, Rgba([200, 190, 180, 255])),
            morphs: Vec::new(),
        };
        Model3d { name: "t".into(), author: "t".into(), parts: vec![part], bones: Vec::new(), expressions: Vec::new(), springs: Vec::new(), colliders: Vec::new(), scale: 1.0, landmarks: Vec::new() }
    }

    #[test]
    fn a_brush_paints_the_overlay_and_the_part_picture_and_an_eraser_takes_it_off() {
        let mut m = model(100, 100);
        let mut overlay = RgbaImage::new(100, 100);
        let mut brush = Brush { colour: [255, 0, 0], size: 10.0, hardness: 1.0, opacity: 1.0, ..Brush::default() };
        dab(&mut m, &mut overlay, &brush, 50.0, 50.0);
        assert_eq!(overlay.get_pixel(50, 50).0, [255, 0, 0, 255]);
        assert_eq!(m.parts[0].image.get_pixel(50, 50).0, [255, 0, 0, 255]);
        assert_eq!(m.parts[0].image.get_pixel(5, 5).0, [200, 190, 180, 255], "far away nothing changed");
        // the eyedropper reads the colour under it and paints nothing
        brush.tool = Tool::Dropper;
        let (got, _) = dab(&mut m, &mut overlay, &brush, 50.0, 50.0);
        assert_eq!(got, Some([255, 0, 0]));
        // the eraser empties the kept paint and asks for the model to be made again
        brush.tool = Tool::Eraser;
        let (_, rebuild) = dab(&mut m, &mut overlay, &brush, 50.0, 50.0);
        assert!(rebuild);
        assert_eq!(overlay.get_pixel(50, 50).0[3], 0);
    }

    #[test]
    fn a_stroke_paints_a_line_and_blend_softens_an_edge() {
        let mut m = model(120, 60);
        let mut overlay = RgbaImage::new(120, 60);
        let brush = Brush { colour: [0, 0, 255], size: 4.0, hardness: 1.0, opacity: 1.0, ..Brush::default() };
        stroke(&mut m, &mut overlay, &brush, [10.0, 30.0], [110.0, 30.0]);
        for x in [15, 60, 105] {
            assert_eq!(overlay.get_pixel(x, 30).0[2], 255, "x {x}");
        }
        // a hard edge between two colours; blending across it makes in-between colours
        let img = &mut m.parts[0].image;
        for y in 0..60 {
            for x in 0..120 {
                img.put_pixel(x, y, if x < 60 { Rgba([0, 0, 0, 255]) } else { Rgba([255, 255, 255, 255]) });
            }
        }
        let mut overlay = RgbaImage::new(120, 60);
        let blend = Brush { tool: Tool::Blend, size: 12.0, hardness: 1.0, opacity: 1.0, ..Brush::default() };
        dab(&mut m, &mut overlay, &blend, 60.0, 30.0);
        let (l, r) = (m.parts[0].image.get_pixel(56, 30).0[0], m.parts[0].image.get_pixel(64, 30).0[0]);
        assert!(l > 20 && r < 235, "the edge softened: {l} {r}");
    }
}
