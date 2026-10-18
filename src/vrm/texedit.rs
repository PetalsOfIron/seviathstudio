// SPDX-License-Identifier: GPL-3.0-or-later
// Editing the texture of a character on an AI shape. The picture on the shape is cut into parts that can each be moved: the
// whole head, the face, each eye, each eyebrow, the nose and the mouth. A part is cut out with a soft edge, the place it leaves
// is filled with the skin around it (so the skin always covers the whole body), and the part is put down again where it was
// moved to. Moving a part inside a part moves the small one with the big one. A part the app found in the wrong place can be
// picked again on the drawing (a *pick*): that piece is cut out and put where the part belongs, and the old one is painted over
// with skin. What is painted over the result by hand is kept as a picture of its own. The blend shapes follow the moved features,
// and the dark inside of the mouth has its own settings.

use image::{Rgba, RgbaImage};
use serde::{Deserialize, Serialize};

use super::face::FaceMarks;

/// The parts of the picture that can be moved or picked.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub enum Part {
    Head,
    Face,
    EyeL,
    EyeR,
    BrowL,
    BrowR,
    Nose,
    Mouth,
}

impl Part {
    pub const ALL: [Part; 8] = [Part::Head, Part::Face, Part::EyeL, Part::EyeR, Part::BrowL, Part::BrowR, Part::Nose, Part::Mouth];

    pub fn label(self) -> &'static str {
        match self {
            Part::Head => "Head",
            Part::Face => "Face",
            Part::EyeL => "Left eye",
            Part::EyeR => "Right eye",
            Part::BrowL => "Left eyebrow",
            Part::BrowR => "Right eyebrow",
            Part::Nose => "Nose",
            Part::Mouth => "Mouth",
        }
    }
}

/// A piece of the drawing the user selected for a part: an ellipse around it (centre and radii, page pixels).
#[derive(Clone, PartialEq, Debug, Serialize, Deserialize)]
pub struct Pick {
    pub part: Part,
    pub centre: [f32; 2],
    pub radii: [f32; 2],
}

#[derive(Clone, PartialEq, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct TexEdit {
    /// How far each part is moved, in pixels of the drawing (x to the right, y down).
    pub head: [f32; 2],
    pub face: [f32; 2],
    pub eye_l: [f32; 2],
    pub eye_r: [f32; 2],
    pub brow_l: [f32; 2],
    pub brow_r: [f32; 2],
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
    /// Parts picked again on the drawing; the piece is put where the part belongs.
    pub picks: Vec<Pick>,
    /// Paint the place of the old part over with skin when a part is picked again.
    pub replace_old: bool,
    /// What was painted by hand over the front of the picture: a PNG as text, empty when nothing was painted.
    pub paint: String,
    /// The layer of the document the texture is painted on with the brushes of the drawing app, and the locked layer that shows the
    /// texture to paint over (0 is none).
    pub paint_layer: u64,
    pub view_layer: u64,
    /// What was painted straight on the 3D character: a PNG as text, the size of the whole picture on the model.
    pub atlas_paint: String,
}

impl Default for TexEdit {
    fn default() -> Self {
        TexEdit {
            head: [0.0; 2],
            face: [0.0; 2],
            eye_l: [0.0; 2],
            eye_r: [0.0; 2],
            brow_l: [0.0; 2],
            brow_r: [0.0; 2],
            nose: [0.0; 2],
            mouth: [0.0; 2],
            mouth_open: 1.0,
            mouth_width: 1.0,
            mouth_color: [70, 22, 30],
            mouth_move: [0.0; 2],
            feather: 0.28,
            picks: Vec::new(),
            replace_old: true,
            paint: String::new(),
            paint_layer: 0,
            view_layer: 0,
            atlas_paint: String::new(),
        }
    }
}

fn add(a: [f32; 2], b: [f32; 2]) -> [f32; 2] {
    [a[0] + b[0], a[1] + b[1]]
}

impl TexEdit {
    pub fn offset(&self, p: Part) -> [f32; 2] {
        match p {
            Part::Head => self.head,
            Part::Face => self.face,
            Part::EyeL => self.eye_l,
            Part::EyeR => self.eye_r,
            Part::BrowL => self.brow_l,
            Part::BrowR => self.brow_r,
            Part::Nose => self.nose,
            Part::Mouth => self.mouth,
        }
    }

    pub fn offset_mut(&mut self, p: Part) -> &mut [f32; 2] {
        match p {
            Part::Head => &mut self.head,
            Part::Face => &mut self.face,
            Part::EyeL => &mut self.eye_l,
            Part::EyeR => &mut self.eye_r,
            Part::BrowL => &mut self.brow_l,
            Part::BrowR => &mut self.brow_r,
            Part::Nose => &mut self.nose,
            Part::Mouth => &mut self.mouth,
        }
    }

    pub fn pick_of(&self, p: Part) -> Option<&Pick> {
        self.picks.iter().find(|k| k.part == p)
    }

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
        !self.picks.is_empty() || Part::ALL.iter().any(|p| { let o = self.offset(*p); o[0].abs() > 0.2 || o[1].abs() > 0.2 })
    }

    /// The nose, taken as two thirds of the way down from the eyes to the mouth, in the middle.
    pub fn nose_at(m: &FaceMarks) -> [f32; 2] {
        let eye_y = (m.eye_l[1] + m.eye_r[1]) / 2.0;
        [m.eye_center()[0], eye_y + 0.64 * (m.mouth[1] - eye_y)]
    }

    /// The eyebrow above an eye on the drawing.
    pub fn brow_at(m: &FaceMarks, left: bool) -> [f32; 2] {
        let e = if left { m.eye_l } else { m.eye_r };
        [e[0], e[1] - 0.85 * m.eye_w]
    }

    /// Where a part is on the drawing as the app found it: the centre and the radii of the ellipse around it.
    pub fn auto_region(part: Part, m: &FaceMarks, head: Option<[f32; 4]>) -> Option<([f32; 2], [f32; 2])> {
        let eye_y = (m.eye_l[1] + m.eye_r[1]) / 2.0;
        let (c, dist) = (m.eye_center(), m.eye_distance());
        let d = (m.mouth[1] - eye_y).max(8.0);
        Some(match part {
            Part::Head => {
                let hb = head?;
                let hh = hb[2] / 2.0;
                ([hb[0] + hh, hb[1] + 0.1 * hh + 0.5 * hh], [0.9 * hh, 0.62 * hh])
            }
            Part::Face => ([c[0], eye_y + 0.6 * d], [dist * 0.95 + m.eye_w, 1.5 * d]),
            Part::EyeL => (m.eye_l, [m.eye_w, m.eye_w * 0.75]),
            Part::EyeR => (m.eye_r, [m.eye_w, m.eye_w * 0.75]),
            Part::BrowL => (TexEdit::brow_at(m, true), [m.eye_w * 1.15, m.eye_w * 0.42]),
            Part::BrowR => (TexEdit::brow_at(m, false), [m.eye_w * 1.15, m.eye_w * 0.42]),
            Part::Nose => (TexEdit::nose_at(m), [0.32 * dist, 0.32 * d]),
            Part::Mouth => (m.mouth, [m.mouth_w * 0.75, 0.42 * d]),
        })
    }

    /// Move the parts on `tile`, the picture of the front of the character. `m` are the face points on the drawing, `head` the box
    /// of the head, `origin` the place of the top left of the tile on the page and `k` the tile pixels per page pixel.
    pub fn apply(&self, tile: &mut RgbaImage, m: &FaceMarks, head: Option<[f32; 4]>, origin: [f32; 2], k: f32) {
        if self.moves_picture() {
            let to_tile = |p: [f32; 2]| [(p[0] - origin[0]) * k, (p[1] - origin[1]) * k];
            let radii = |r: [f32; 2]| [r[0] * k, r[1] * k];
            // the parts picked again: the piece is cut where the user chose it, the old part is painted over, and the piece is
            // put down where the part belongs
            for pk in &self.picks {
                let Some((dest, _)) = TexEdit::auto_region(pk.part, m, head) else { continue };
                let piece = cut(tile, to_tile(pk.centre), radii(pk.radii), self.feather);
                if self.replace_old {
                    if let Some((_, dr)) = TexEdit::auto_region(pk.part, m, head) {
                        let _ = cut(tile, to_tile(dest), radii(dr), self.feather);
                    }
                }
                paste(tile, &piece, to_tile(dest), [0.0, 0.0]);
            }
            // the big parts, then the small ones where the big ones have taken them
            let mut shift = [0.0f32; 2];
            for part in [Part::Head, Part::Face] {
                let Some((centre, r)) = TexEdit::auto_region(part, m, head) else { continue };
                let off = self.offset(part);
                let r = self.pick_of(part).map_or(r, |p| p.radii);
                move_region(tile, to_tile(add(centre, shift)), radii(r), [off[0] * k, off[1] * k], self.feather);
                shift = add(shift, off);
            }
            for part in [Part::EyeL, Part::EyeR, Part::BrowL, Part::BrowR, Part::Nose, Part::Mouth] {
                let Some((centre, r)) = TexEdit::auto_region(part, m, head) else { continue };
                let off = self.offset(part);
                let r = self.pick_of(part).map_or(r, |p| p.radii);
                move_region(tile, to_tile(add(centre, shift)), radii(r), [off[0] * k, off[1] * k], self.feather);
            }
        }
        self.apply_paint(tile);
    }

    /// What was painted straight on the 3D character, as a picture the size of the whole picture on the model.
    pub fn atlas_paint_image(&self) -> Option<RgbaImage> {
        if self.atlas_paint.is_empty() {
            return None;
        }
        let bytes = crate::brush::tipimage::base64_decode(&self.atlas_paint)?;
        image::load_from_memory_with_format(&bytes, image::ImageFormat::Png).ok().map(|i| i.to_rgba8())
    }

    pub fn set_atlas_paint(&mut self, img: &RgbaImage) {
        if img.pixels().all(|p| p.0[3] == 0) {
            self.atlas_paint.clear();
            return;
        }
        let mut out = std::io::Cursor::new(Vec::new());
        if img.write_to(&mut out, image::ImageFormat::Png).is_ok() {
            self.atlas_paint = crate::brush::tipimage::base64_encode(&out.into_inner());
        }
    }

    /// What was painted by hand, as a picture the size of the tile.
    pub fn paint_image(&self) -> Option<RgbaImage> {
        if self.paint.is_empty() {
            return None;
        }
        let bytes = crate::brush::tipimage::base64_decode(&self.paint)?;
        image::load_from_memory_with_format(&bytes, image::ImageFormat::Png).ok().map(|i| i.to_rgba8())
    }

    pub fn set_paint(&mut self, img: &RgbaImage) {
        if img.pixels().all(|p| p.0[3] == 0) {
            self.paint.clear();
            return;
        }
        let mut out = std::io::Cursor::new(Vec::new());
        if img.write_to(&mut out, image::ImageFormat::Png).is_ok() {
            self.paint = crate::brush::tipimage::base64_encode(&out.into_inner());
        }
    }

    /// Lay the hand painting over the picture (when it has the size of the tile).
    pub fn apply_paint(&self, tile: &mut RgbaImage) {
        let Some(p) = self.paint_image() else { return };
        if p.dimensions() != tile.dimensions() {
            return;
        }
        for (d, s) in tile.pixels_mut().zip(p.pixels()) {
            let a = s.0[3] as f32 / 255.0;
            if a > 0.0 {
                for c in 0..3 {
                    d.0[c] = (d.0[c] as f32 * (1.0 - a) + s.0[c] as f32 * a) as u8;
                }
                d.0[3] = 255;
            }
        }
    }
}

fn smooth(t: f32) -> f32 {
    let t = t.clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

/// A piece cut out of a picture: its colours and how much of each pixel belongs to it.
pub struct Piece {
    x0: i32,
    y0: i32,
    bw: usize,
    bh: usize,
    mask: Vec<f32>,
    colours: Vec<[u8; 3]>,
    centre: [f32; 2],
}

/// Cut the ellipse out of the picture (the edge soft) and fill the place it leaves with the colours around it.
pub fn cut(img: &mut RgbaImage, centre: [f32; 2], radii: [f32; 2], feather: f32) -> Piece {
    let feather = feather.clamp(0.05, 0.7);
    let (w, h) = (img.width() as i32, img.height() as i32);
    let x0 = ((centre[0] - radii[0]).floor() as i32 - 1).max(0);
    let x1 = ((centre[0] + radii[0]).ceil() as i32 + 1).min(w - 1);
    let y0 = ((centre[1] - radii[1]).floor() as i32 - 1).max(0);
    let y1 = ((centre[1] + radii[1]).ceil() as i32 + 1).min(h - 1);
    if x1 <= x0 || y1 <= y0 {
        return Piece { x0: 0, y0: 0, bw: 0, bh: 0, mask: Vec::new(), colours: Vec::new(), centre };
    }
    let (bw, bh) = ((x1 - x0 + 1) as usize, (y1 - y0 + 1) as usize);
    let mut mask = vec![0.0f32; bw * bh];
    let mut colours: Vec<[u8; 3]> = vec![[0; 3]; bw * bh];
    for y in y0..=y1 {
        for x in x0..=x1 {
            let r = (((x as f32 - centre[0]) / radii[0].max(1.0)).powi(2) + ((y as f32 - centre[1]) / radii[1].max(1.0)).powi(2)).sqrt();
            let i = (y - y0) as usize * bw + (x - x0) as usize;
            mask[i] = 1.0 - smooth((r - (1.0 - feather)) / feather);
            let p = img.get_pixel(x as u32, y as u32).0;
            colours[i] = [p[0], p[1], p[2]];
        }
    }
    // the place it leaves: filled from the colours around it
    let pad = 8;
    let (cx0, cy0) = ((x0 - pad).max(0), (y0 - pad).max(0));
    let (cx1, cy1) = ((x1 + pad).min(w - 1), (y1 + pad).min(h - 1));
    let mut crop = image::imageops::crop_imm(img, cx0 as u32, cy0 as u32, (cx1 - cx0 + 1) as u32, (cy1 - cy0 + 1) as u32).to_image();
    let cw = crop.width() as i32;
    let mut open = vec![false; (cw * crop.height() as i32) as usize];
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
    Piece { x0, y0, bw, bh, mask, colours, centre }
}

/// Put a piece down with its centre at `at`, moved by `by`.
pub fn paste(img: &mut RgbaImage, piece: &Piece, at: [f32; 2], by: [f32; 2]) {
    let (w, h) = (img.width() as i32, img.height() as i32);
    let dx = (at[0] - piece.centre[0] + by[0]).round() as i32;
    let dy = (at[1] - piece.centre[1] + by[1]).round() as i32;
    for iy in 0..piece.bh {
        for ix in 0..piece.bw {
            let m = piece.mask[iy * piece.bw + ix];
            let (tx, ty) = (piece.x0 + ix as i32 + dx, piece.y0 + iy as i32 + dy);
            if m > 0.0 && tx >= 0 && ty >= 0 && tx < w && ty < h {
                let c = piece.colours[iy * piece.bw + ix];
                let old = img.get_pixel(tx as u32, ty as u32).0;
                let mix = |a: u8, b: u8| (a as f32 * (1.0 - m) + b as f32 * m) as u8;
                img.put_pixel(tx as u32, ty as u32, Rgba([mix(old[0], c[0]), mix(old[1], c[1]), mix(old[2], c[2]), 255]));
            }
        }
    }
}

/// Cut the ellipse out of the picture, fill the place with the colours around it, and put the piece down again `by` pixels away.
pub fn move_region(img: &mut RgbaImage, centre: [f32; 2], radii: [f32; 2], by: [f32; 2], feather: f32) {
    if by[0].abs() < 0.2 && by[1].abs() < 0.2 {
        return;
    }
    let piece = cut(img, centre, radii, feather);
    paste(img, &piece, centre, by);
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

    fn marks() -> FaceMarks {
        FaceMarks { eye_l: [130.0, 90.0], eye_r: [70.0, 90.0], mouth: [100.0, 140.0], eye_w: 24.0, mouth_w: 40.0 }
    }

    #[test]
    fn a_part_moves_and_the_place_it_leaves_is_skin() {
        let mut img = picture();
        move_region(&mut img, [100.0, 100.0], [14.0, 14.0], [0.0, 20.0], 0.28);
        assert!(img.get_pixel(100, 120).0[2] > 100 && img.get_pixel(100, 120).0[0] < 60, "{:?}", img.get_pixel(100, 120));
        let old = img.get_pixel(100, 100).0;
        assert!(old[0] > 200 && old[1] > 170, "the old place is skin again: {old:?}");
        assert_eq!(img.get_pixel(10, 10).0, [240, 200, 170, 255]);
        let before = img.clone();
        move_region(&mut img, [100.0, 100.0], [14.0, 14.0], [0.0, 0.0], 0.28);
        assert_eq!(before.as_raw(), img.as_raw());
    }

    #[test]
    fn parts_inside_parts_move_together_and_the_face_points_follow() {
        let m = marks();
        let mut e = TexEdit::default();
        assert!(e.is_default() && !e.moves_picture());
        e.face = [0.0, 10.0];
        e.eye_l = [3.0, 0.0];
        e.mouth = [0.0, -4.0];
        let f = e.effective(&m);
        assert_eq!(f.eye_l, [133.0, 100.0]);
        assert_eq!(f.eye_r, [70.0, 100.0]);
        assert_eq!(f.mouth, [100.0, 146.0]);
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
        assert!(serde_json::from_str::<TexEdit>("{}").unwrap().is_default());
    }

    #[test]
    fn a_part_picked_again_is_put_where_the_part_belongs_and_the_old_one_is_painted_over() {
        let m = marks();
        // the app thinks the mouth is at (100, 140); the real mouth (a red patch) is at (60, 170); a wrong dark patch sits where
        // the app thinks it is
        let mut img = RgbaImage::from_pixel(200, 200, Rgba([240, 200, 170, 255]));
        for y in 160..180 {
            for x in 50..70 {
                img.put_pixel(x, y, Rgba([200, 30, 40, 255]));
            }
        }
        for y in 132..148 {
            for x in 92..108 {
                img.put_pixel(x, y, Rgba([30, 30, 30, 255]));
            }
        }
        let mut e = TexEdit::default();
        e.picks.push(Pick { part: Part::Mouth, centre: [60.0, 170.0], radii: [16.0, 16.0] });
        e.apply(&mut img, &m, None, [0.0, 0.0], 1.0);
        // the red patch is now at the mouth, the dark one is gone, and where the red one was is skin
        let at = img.get_pixel(100, 140).0;
        assert!(at[0] > 150 && at[1] < 100, "the picked mouth is at the mouth: {at:?}");
        assert!(img.get_pixel(60, 170).0[1] > 150, "the old place of the picked piece is skin");
        let dark = (0..200).flat_map(|y| (0..200).map(move |x| (x, y))).filter(|(x, y)| img.get_pixel(*x, *y).0[0] < 60).count();
        assert_eq!(dark, 0, "no dark patch left");
        // with the old one kept, the dark patch stays
        let mut img2 = RgbaImage::from_pixel(200, 200, Rgba([240, 200, 170, 255]));
        for y in 120..160 {
            for x in 80..120 {
                img2.put_pixel(x, y, Rgba([30, 30, 30, 255]));
            }
        }
        for y in 160..180 {
            for x in 50..70 {
                img2.put_pixel(x, y, Rgba([200, 30, 40, 255]));
            }
        }
        e.replace_old = false;
        e.apply(&mut img2, &m, None, [0.0, 0.0], 1.0);
        assert!((0..200).flat_map(|y| (0..200).map(move |x| (x, y))).any(|(x, y)| img2.get_pixel(x, y).0[0] < 60));
        // a picked part can then be moved too
        let mut e3 = TexEdit::default();
        e3.picks.push(Pick { part: Part::Mouth, centre: [60.0, 170.0], radii: [16.0, 16.0] });
        e3.mouth = [0.0, 12.0];
        let mut img3 = RgbaImage::from_pixel(200, 200, Rgba([240, 200, 170, 255]));
        for y in 160..180 {
            for x in 50..70 {
                img3.put_pixel(x, y, Rgba([200, 30, 40, 255]));
            }
        }
        e3.apply(&mut img3, &m, None, [0.0, 0.0], 1.0);
        assert!(img3.get_pixel(100, 152).0[1] < 100, "moved 12 down from the mouth");
    }

    #[test]
    fn eyebrows_are_parts_and_hand_painting_is_kept_and_laid_over_the_picture() {
        let m = marks();
        let b = TexEdit::brow_at(&m, true);
        assert!(b[1] < m.eye_l[1] && b[0] == m.eye_l[0]);
        assert!(TexEdit::auto_region(Part::BrowR, &m, None).is_some() && TexEdit::auto_region(Part::Head, &m, None).is_none());
        let mut e = TexEdit::default();
        e.brow_l = [0.0, 5.0];
        assert!(e.moves_picture());
        // painting: a patch of red with half opacity
        let mut paint = RgbaImage::new(40, 40);
        for y in 10..20 {
            for x in 10..20 {
                paint.put_pixel(x, y, Rgba([255, 0, 0, 128]));
            }
        }
        e.set_paint(&paint);
        assert!(!e.paint.is_empty());
        let mut tile = RgbaImage::from_pixel(40, 40, Rgba([0, 0, 255, 255]));
        e.apply_paint(&mut tile);
        let p = tile.get_pixel(15, 15).0;
        assert!(p[0] > 100 && p[2] > 100 && p[0] < 160, "mixed: {p:?}");
        assert_eq!(tile.get_pixel(30, 30).0, [0, 0, 255, 255]);
        // a picture of another size is not used
        let mut other = RgbaImage::from_pixel(50, 50, Rgba([0, 0, 255, 255]));
        e.apply_paint(&mut other);
        assert_eq!(other.get_pixel(15, 15).0, [0, 0, 255, 255]);
        // nothing painted: nothing saved
        e.set_paint(&RgbaImage::new(40, 40));
        assert!(e.paint.is_empty());
        // saved and read back
        e.set_paint(&paint);
        let text = serde_json::to_string(&e).unwrap();
        assert_eq!(serde_json::from_str::<TexEdit>(&text).unwrap().paint_image().unwrap().get_pixel(12, 12).0, [255, 0, 0, 128]);
    }
}
