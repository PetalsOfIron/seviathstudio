// SPDX-License-Identifier: GPL-3.0-or-later
// Editing the texture of a character on an AI shape. The picture on the shape is cut into parts that can each be moved: the
// whole head, the face, each eye, the nose and the mouth. A part is cut out with a soft edge, the place it leaves is filled
// with the skin around it (so the skin always covers the whole body), and the part is put down again where it was moved to.
// Moving a part inside a part moves the small one with the big one. The blend shapes follow the moved features, and the dark
// inside of the mouth has its own settings.

use image::{Rgba, RgbaImage};
use serde::{Deserialize, Serialize};

use super::face::FaceMarks;

#[derive(Clone, PartialEq, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct TexEdit {
    /// How far each part is moved, in pixels of the drawing (x to the right, y down).
    pub head: [f32; 2],
    pub face: [f32; 2],
    pub eye_l: [f32; 2],
    pub eye_r: [f32; 2],
    pub nose: [f32; 2],
    pub mouth: [f32; 2],
    /// The dark inside of the mouth: how open it gets (1 is the usual), how wide it is, its colour, and how far it is moved
    /// from the lips.
    pub mouth_open: f32,
    pub mouth_width: f32,
    pub mouth_color: [u8; 3],
    pub mouth_move: [f32; 2],
    /// How soft the edge of a moved part is, as a share of its size (0.05 sharp, 0.6 very soft): where the seam blends.
    pub feather: f32,
}

impl Default for TexEdit {
    fn default() -> Self {
        TexEdit { head: [0.0; 2], face: [0.0; 2], eye_l: [0.0; 2], eye_r: [0.0; 2], nose: [0.0; 2], mouth: [0.0; 2], mouth_open: 1.0, mouth_width: 1.0, mouth_color: [70, 22, 30], mouth_move: [0.0; 2], feather: 0.28 }
    }
}

fn add(a: [f32; 2], b: [f32; 2]) -> [f32; 2] {
    [a[0] + b[0], a[1] + b[1]]
}

impl TexEdit {
    /// Where the features are after the parts are moved: the face points the blend shapes and the mouth follow.
    pub fn effective(&self, m: &FaceMarks) -> FaceMarks {
        let base = add(self.head, self.face);
        FaceMarks { eye_l: add(add(m.eye_l, base), self.eye_l), eye_r: add(add(m.eye_r, base), self.eye_r), mouth: add(add(m.mouth, base), self.mouth), ..*m }
    }

    pub fn is_default(&self) -> bool {
        *self == TexEdit::default()
    }

    /// Does anything move the picture (not only the settings of the mouth)?
    pub fn moves_picture(&self) -> bool {
        [self.head, self.face, self.eye_l, self.eye_r, self.nose, self.mouth].iter().any(|o| o[0].abs() > 0.2 || o[1].abs() > 0.2)
    }

    /// The nose, taken as two thirds of the way down from the eyes to the mouth, in the middle.
    pub fn nose_at(m: &FaceMarks) -> [f32; 2] {
        let eye_y = (m.eye_l[1] + m.eye_r[1]) / 2.0;
        [m.eye_center()[0], eye_y + 0.64 * (m.mouth[1] - eye_y)]
    }

    /// Move the parts on `tile`, the picture of the front of the character. `m` are the face points on the drawing, `head` the box
    /// of the head, `origin` the place of the top left of the tile on the page and `k` the tile pixels per page pixel.
    pub fn apply(&self, tile: &mut RgbaImage, m: &FaceMarks, head: Option<[f32; 4]>, origin: [f32; 2], k: f32) {
        if !self.moves_picture() {
            return;
        }
        let eye_y = (m.eye_l[1] + m.eye_r[1]) / 2.0;
        let (c, dist) = (m.eye_center(), m.eye_distance());
        let d = (m.mouth[1] - eye_y).max(8.0);
        let to_tile = |p: [f32; 2]| [(p[0] - origin[0]) * k, (p[1] - origin[1]) * k];
        // (centre, radii) of every part on the drawing
        let mut parts: Vec<([f32; 2], [f32; 2], [f32; 2], usize)> = Vec::new();
        let base = add(self.head, self.face);
        if let Some(hb) = head {
            let hh = hb[2] / 2.0;
            parts.push(([hb[0] + hh, hb[1] + 0.1 * hh + 0.5 * hh], [0.9 * hh, 0.62 * hh], self.head, 0));
        }
        parts.push(([c[0], eye_y + 0.6 * d], [dist * 0.95 + m.eye_w, 1.5 * d], self.face, 1));
        let _ = base;
        // the small parts sit where the big ones have taken them
        let nose = TexEdit::nose_at(m);
        let small: [([f32; 2], [f32; 2], [f32; 2]); 4] = [
            (m.eye_l, [m.eye_w * 1.0, m.eye_w * 0.75], self.eye_l),
            (m.eye_r, [m.eye_w * 1.0, m.eye_w * 0.75], self.eye_r),
            (nose, [0.32 * dist, 0.32 * d], self.nose),
            (m.mouth, [m.mouth_w * 0.75, 0.42 * d], self.mouth),
        ];
        let mut shift = [0.0f32; 2];
        for (centre, radii, off, _) in parts {
            let at = add(centre, shift);
            move_region(tile, to_tile(at), [radii[0] * k, radii[1] * k], [off[0] * k, off[1] * k], self.feather);
            shift = add(shift, off);
        }
        for (centre, radii, off) in small {
            let at = add(centre, shift);
            move_region(tile, to_tile(at), [radii[0] * k, radii[1] * k], [off[0] * k, off[1] * k], self.feather);
        }
    }
}

fn smooth(t: f32) -> f32 {
    let t = t.clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

/// Cut the ellipse out of the picture (the edge soft), fill the place with the colours around it, and put the piece down again
/// `by` pixels away.
pub fn move_region(img: &mut RgbaImage, centre: [f32; 2], radii: [f32; 2], by: [f32; 2], feather: f32) {
    let feather = feather.clamp(0.05, 0.7);
    if by[0].abs() < 0.2 && by[1].abs() < 0.2 {
        return;
    }
    let (w, h) = (img.width() as i32, img.height() as i32);
    let x0 = ((centre[0] - radii[0]).floor() as i32 - 1).max(0);
    let x1 = ((centre[0] + radii[0]).ceil() as i32 + 1).min(w - 1);
    let y0 = ((centre[1] - radii[1]).floor() as i32 - 1).max(0);
    let y1 = ((centre[1] + radii[1]).ceil() as i32 + 1).min(h - 1);
    if x1 <= x0 || y1 <= y0 {
        return;
    }
    let (bw, bh) = ((x1 - x0 + 1) as usize, (y1 - y0 + 1) as usize);
    // the piece: its colours and how much of each pixel belongs to it
    let mut mask = vec![0.0f32; bw * bh];
    let mut piece: Vec<[u8; 3]> = vec![[0; 3]; bw * bh];
    for y in y0..=y1 {
        for x in x0..=x1 {
            let r = (((x as f32 - centre[0]) / radii[0].max(1.0)).powi(2) + ((y as f32 - centre[1]) / radii[1].max(1.0)).powi(2)).sqrt();
            let m = 1.0 - smooth((r - (1.0 - feather)) / feather);
            let i = (y - y0) as usize * bw + (x - x0) as usize;
            mask[i] = m;
            let p = img.get_pixel(x as u32, y as u32).0;
            piece[i] = [p[0], p[1], p[2]];
        }
    }
    // the place it leaves: filled from the colours around it
    let pad = 8;
    let (cx0, cy0) = ((x0 - pad).max(0), (y0 - pad).max(0));
    let (cx1, cy1) = ((x1 + pad).min(w - 1), (y1 + pad).min(h - 1));
    let mut crop = image::imageops::crop_imm(img, cx0 as u32, cy0 as u32, (cx1 - cx0 + 1) as u32, (cy1 - cy0 + 1) as u32).to_image();
    let (cw, ch) = (crop.width() as i32, crop.height() as i32);
    let mut open = vec![false; (cw * ch) as usize];
    for y in y0..=y1 {
        for x in x0..=x1 {
            if mask[(y - y0) as usize * bw + (x - x0) as usize] > 0.5 {
                open[((y - cy0) * cw + (x - cx0)) as usize] = true;
            }
        }
    }
    super::shape::fill_open(&mut crop, open);
    for y in y0..=y1 {
        for x in x0..=x1 {
            let m = mask[(y - y0) as usize * bw + (x - x0) as usize];
            if m > 0.0 {
                let f = crop.get_pixel((x - cx0) as u32, (y - cy0) as u32).0;
                let old = img.get_pixel(x as u32, y as u32).0;
                // inside, the filled colour; on the soft edge, a mix, so the place the piece leaves has no ring
                let t = smooth((m - 0.2) / 0.6);
                let mix = |a: u8, b: u8| (a as f32 * (1.0 - t) + b as f32 * t) as u8;
                img.put_pixel(x as u32, y as u32, Rgba([mix(old[0], f[0]), mix(old[1], f[1]), mix(old[2], f[2]), 255]));
            }
        }
    }
    // the piece put down again
    let (dx, dy) = (by[0].round() as i32, by[1].round() as i32);
    for y in y0..=y1 {
        for x in x0..=x1 {
            let m = mask[(y - y0) as usize * bw + (x - x0) as usize];
            let (tx, ty) = (x + dx, y + dy);
            if m > 0.0 && tx >= 0 && ty >= 0 && tx < w && ty < h {
                let c = piece[(y - y0) as usize * bw + (x - x0) as usize];
                let old = img.get_pixel(tx as u32, ty as u32).0;
                let mix = |a: u8, b: u8| (a as f32 * (1.0 - m) + b as f32 * m) as u8;
                img.put_pixel(tx as u32, ty as u32, Rgba([mix(old[0], c[0]), mix(old[1], c[1]), mix(old[2], c[2]), 255]));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn picture() -> RgbaImage {
        // skin with a dark spot (an eye) at (100, 100)
        let mut img = RgbaImage::from_pixel(200, 200, Rgba([240, 200, 170, 255]));
        for y in 92..108 {
            for x in 92..108 {
                if ((x - 100) as f32).hypot((y - 100) as f32) < 7.0 {
                    img.put_pixel(x as u32, y as u32, Rgba([20, 40, 120, 255]));
                }
            }
        }
        img
    }

    #[test]
    fn a_part_moves_and_the_place_it_leaves_is_skin() {
        let mut img = picture();
        move_region(&mut img, [100.0, 100.0], [14.0, 14.0], [0.0, 20.0], 0.28);
        // the dark spot is now 20 pixels lower, and it is where it was no more
        assert!(img.get_pixel(100, 120).0[2] > 100 && img.get_pixel(100, 120).0[0] < 60, "{:?}", img.get_pixel(100, 120));
        let old = img.get_pixel(100, 100).0;
        assert!(old[0] > 200 && old[1] > 170, "the old place is skin again: {old:?}");
        // far away nothing changed
        assert_eq!(img.get_pixel(10, 10).0, [240, 200, 170, 255]);
        // a move of nothing changes nothing
        let before = img.clone();
        move_region(&mut img, [100.0, 100.0], [14.0, 14.0], [0.0, 0.0], 0.28);
        assert_eq!(before.as_raw(), img.as_raw());
    }

    #[test]
    fn parts_inside_parts_move_together_and_the_face_points_follow() {
        let m = FaceMarks { eye_l: [130.0, 90.0], eye_r: [70.0, 90.0], mouth: [100.0, 140.0], eye_w: 24.0, mouth_w: 40.0 };
        let mut e = TexEdit::default();
        assert!(e.is_default() && !e.moves_picture());
        e.face = [0.0, 10.0];
        e.eye_l = [3.0, 0.0];
        e.mouth = [0.0, -4.0];
        let f = e.effective(&m);
        assert_eq!(f.eye_l, [133.0, 100.0]);
        assert_eq!(f.eye_r, [70.0, 100.0]);
        assert_eq!(f.mouth, [100.0, 146.0]);
        assert_eq!(f.eye_w, 24.0);
        // applied to a picture: it changes, and the edit is saved with the project and read back
        let mut img = RgbaImage::from_pixel(200, 200, Rgba([240, 200, 170, 255]));
        for y in 80..100 {
            for x in 120..140 {
                img.put_pixel(x, y, Rgba([20, 40, 120, 255]));
            }
        }
        let before = img.clone();
        e.apply(&mut img, &m, None, [0.0, 0.0], 1.0);
        assert_ne!(before.as_raw(), img.as_raw());
        let text = serde_json::to_string(&e).unwrap();
        assert_eq!(serde_json::from_str::<TexEdit>(&text).unwrap(), e);
        // an old project without the field reads as the default
        assert!(serde_json::from_str::<TexEdit>("{}").unwrap().is_default());
    }
}
