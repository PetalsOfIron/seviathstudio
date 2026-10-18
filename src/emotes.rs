// SPDX-License-Identifier: GPL-3.0-or-later
// Emotes and small pictures of the character: a square cut of the face (or the whole picture), made at the sizes that Twitch, Discord
// and others ask for, one for every expression.

use image::{RgbaImage, imageops};

/// The sizes the services ask for: (name, side in pixels).
pub const SIZES: [(&str, u32); 5] = [("Twitch 28", 28), ("Twitch 56", 56), ("Twitch 112", 112), ("Discord 128", 128), ("Big 256", 256)];

/// A square cut out of `img` around the box `[x0, y0, x1, y1]` (in pixels of `img`), `margin` times as wide as the box is long,
/// moved inside the picture when it would stick out. The box is the whole picture when it is None.
pub fn square_cut(img: &RgbaImage, around: Option<[f32; 4]>, margin: f32) -> RgbaImage {
    let (w, h) = (img.width() as f32, img.height() as f32);
    let [x0, y0, x1, y1] = around.unwrap_or([0.0, 0.0, w, h]);
    let side = ((x1 - x0).max(y1 - y0) * margin).clamp(2.0, w.max(h));
    let (cx, cy) = ((x0 + x1) / 2.0, (y0 + y1) / 2.0);
    // the square may be bigger than the picture on one side: it is then filled with see-through pixels
    let (sx, sy) = (cx - side / 2.0, cy - side / 2.0);
    let side_px = side.round().max(2.0) as u32;
    let mut out = RgbaImage::new(side_px, side_px);
    imageops::replace(&mut out, img, (-sx).round() as i64, (-sy).round() as i64);
    out
}

/// Make a picture `side` pixels wide and high (smoothly).
pub fn shrink(img: &RgbaImage, side: u32) -> RgbaImage {
    imageops::resize(img, side, side, imageops::FilterType::Lanczos3)
}

/// A name that is safe as a file name.
pub fn file_name(name: &str) -> String {
    let s: String = name.chars().map(|c| if c.is_alphanumeric() || c == '-' || c == '_' { c } else { '_' }).collect();
    if s.is_empty() { "emote".into() } else { s }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_square_is_cut_around_a_box_and_stays_square() {
        let mut img = RgbaImage::new(200, 100);
        for x in 80..120 {
            for y in 20..60 {
                img.put_pixel(x, y, image::Rgba([255, 0, 0, 255]));
            }
        }
        // the box 80..120 by 20..60 with a margin of 1.5: a 60 pixel square around (100, 40)
        let c = square_cut(&img, Some([80.0, 20.0, 120.0, 60.0]), 1.5);
        assert_eq!(c.dimensions(), (60, 60));
        assert_eq!(c.get_pixel(30, 30).0, [255, 0, 0, 255]);
        assert_eq!(c.get_pixel(2, 2).0[3], 0);
        // near the edge: the part outside the picture is see-through, the square is still square
        let c = square_cut(&img, Some([0.0, 0.0, 40.0, 40.0]), 2.0);
        assert_eq!(c.dimensions(), (80, 80));
        assert_eq!(c.get_pixel(0, 0).0[3], 0);
        // the whole picture
        assert_eq!(square_cut(&img, None, 1.0).dimensions(), (200, 200));
        let s = shrink(&c, 28);
        assert_eq!(s.dimensions(), (28, 28));
        assert_eq!(file_name("Happy face!"), "Happy_face_");
        assert_eq!(file_name(""), "emote");
    }
}
