// SPDX-License-Identifier: GPL-3.0-or-later
// A picture used as the stamp of a brush (a leaf you drew, a lasso'd object, a PNG): a square
// of alpha values, kept small so it can travel inside the brush file as a tiny PNG.

use std::fmt;
use std::sync::Arc;

use serde::{Deserialize, Deserializer, Serialize, Serializer};

/// Biggest side of a stored tip (pixels).
pub const MAX_SIDE: u32 = 192;

#[derive(Clone)]
pub struct TipImage {
    pub n: u32,
    /// `n`×`n` alpha values: 255 = full paint.
    pub a: Arc<Vec<u8>>,
    /// A fingerprint, so brushes with the same picture share one tip.
    pub id: u64,
}

impl PartialEq for TipImage {
    fn eq(&self, o: &Self) -> bool {
        self.id == o.id && self.n == o.n
    }
}

impl fmt::Debug for TipImage {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "TipImage({}, {:x})", self.n, self.id)
    }
}

fn fingerprint(n: u32, a: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325 ^ n as u64;
    for b in a {
        h ^= *b as u64;
        h = h.wrapping_mul(0x0100_0000_01b3);
    }
    h
}

impl TipImage {
    pub fn from_square(n: u32, a: Vec<u8>) -> Option<TipImage> {
        if n == 0 || n > MAX_SIDE || a.len() != (n * n) as usize {
            return None;
        }
        let id = fingerprint(n, &a);
        Some(TipImage { n, a: Arc::new(a), id })
    }

    /// From any picture: paint where it is opaque, or where it is dark when it has no transparency.
    pub fn from_picture(img: &image::RgbaImage) -> Option<TipImage> {
        let (w, h) = img.dimensions();
        if w == 0 || h == 0 {
            return None;
        }
        let has_alpha = img.pixels().any(|p| p.0[3] < 250);
        let value = |p: &image::Rgba<u8>| -> f32 {
            if has_alpha {
                p.0[3] as f32 / 255.0
            } else {
                1.0 - (0.299 * p.0[0] as f32 + 0.587 * p.0[1] as f32 + 0.114 * p.0[2] as f32) / 255.0
            }
        };
        // fit inside a square, centered, scaled down if it is big
        let side = w.max(h);
        let n = side.min(MAX_SIDE);
        let k = side as f32 / n as f32;
        let mut a = vec![0u8; (n * n) as usize];
        for y in 0..n {
            for x in 0..n {
                // the picture's own pixel under this square pixel (box average when shrinking)
                let (sx, sy) = (x as f32 * k - (side - w) as f32 / 2.0, y as f32 * k - (side - h) as f32 / 2.0);
                let (mut acc, mut cnt) = (0.0f32, 0.0f32);
                let r = k.ceil().max(1.0) as i32;
                for j in 0..r {
                    for i in 0..r {
                        let (px, py) = ((sx + i as f32).floor() as i32, (sy + j as f32).floor() as i32);
                        if px >= 0 && py >= 0 && px < w as i32 && py < h as i32 {
                            acc += value(img.get_pixel(px as u32, py as u32));
                        }
                        cnt += 1.0;
                    }
                }
                a[(y * n + x) as usize] = (acc / cnt * 255.0).round().clamp(0.0, 255.0) as u8;
            }
        }
        // a picture with nothing in it is not a brush
        if a.iter().all(|v| *v < 8) {
            return None;
        }
        TipImage::from_square(n, a)
    }

    /// Bilinear sample at `x`, `y` in 0..n.
    pub fn at(&self, x: f32, y: f32) -> f32 {
        let (fx, fy) = ((x - 0.5).clamp(0.0, self.n as f32 - 1.0), (y - 0.5).clamp(0.0, self.n as f32 - 1.0));
        let (x0, y0) = (fx.floor() as usize, fy.floor() as usize);
        let (x1, y1) = ((x0 + 1).min(self.n as usize - 1), (y0 + 1).min(self.n as usize - 1));
        let (tx, ty) = (fx - x0 as f32, fy - y0 as f32);
        let g = |x: usize, y: usize| self.a[y * self.n as usize + x] as f32 / 255.0;
        let top = g(x0, y0) + (g(x1, y0) - g(x0, y0)) * tx;
        let bot = g(x0, y1) + (g(x1, y1) - g(x0, y1)) * tx;
        top + (bot - top) * ty
    }

    fn to_text(&self) -> String {
        let mut png = Vec::new();
        {
            let mut enc = png::Encoder::new(&mut png, self.n, self.n);
            enc.set_color(png::ColorType::Grayscale);
            enc.set_depth(png::BitDepth::Eight);
            if let Ok(mut w) = enc.write_header() {
                let _ = w.write_image_data(&self.a);
            }
        }
        base64_encode(&png)
    }

    fn from_text(t: &str) -> Option<TipImage> {
        let bytes = base64_decode(t)?;
        let mut limits = image::Limits::default();
        limits.max_image_width = Some(MAX_SIDE);
        limits.max_image_height = Some(MAX_SIDE);
        let mut r = image::ImageReader::new(std::io::Cursor::new(bytes)).with_guessed_format().ok()?;
        r.limits(limits);
        let img = r.decode().ok()?.to_luma8();
        if img.width() != img.height() {
            return None;
        }
        TipImage::from_square(img.width(), img.into_raw())
    }
}

impl Serialize for TipImage {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(&self.to_text())
    }
}

impl<'de> Deserialize<'de> for TipImage {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let t = String::deserialize(d)?;
        TipImage::from_text(&t).ok_or_else(|| serde::de::Error::custom("not a brush picture"))
    }
}

const B64: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

pub fn base64_encode(data: &[u8]) -> String {
    let mut out = String::with_capacity(data.len().div_ceil(3) * 4);
    for c in data.chunks(3) {
        let n = (c[0] as u32) << 16 | (*c.get(1).unwrap_or(&0) as u32) << 8 | *c.get(2).unwrap_or(&0) as u32;
        out.push(B64[(n >> 18) as usize & 63] as char);
        out.push(B64[(n >> 12) as usize & 63] as char);
        out.push(if c.len() > 1 { B64[(n >> 6) as usize & 63] as char } else { '=' });
        out.push(if c.len() > 2 { B64[n as usize & 63] as char } else { '=' });
    }
    out
}

pub fn base64_decode(text: &str) -> Option<Vec<u8>> {
    let mut out = Vec::with_capacity(text.len() / 4 * 3);
    let (mut acc, mut bits) = (0u32, 0);
    for ch in text.bytes().filter(|c| !c.is_ascii_whitespace()) {
        if ch == b'=' {
            break;
        }
        let v = B64.iter().position(|b| *b == ch)? as u32;
        acc = acc << 6 | v;
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push((acc >> bits) as u8);
            acc &= (1 << bits) - 1;
        }
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn base64_round_trips_any_bytes() {
        for len in 0..40usize {
            let data: Vec<u8> = (0..len).map(|i| (i * 37 + 11) as u8).collect();
            assert_eq!(base64_decode(&base64_encode(&data)).unwrap(), data, "{len} bytes");
        }
        assert_eq!(base64_encode(b"Man"), "TWFu");
        assert!(base64_decode("not base64 !!").is_none());
    }

    #[test]
    fn a_picture_becomes_a_square_tip_and_travels_as_text() {
        // a wide picture: a dark bar on white
        let mut img = image::RgbaImage::from_pixel(120, 40, image::Rgba([255, 255, 255, 255]));
        for y in 10..30 {
            for x in 20..100 {
                img.put_pixel(x, y, image::Rgba([10, 10, 10, 255]));
            }
        }
        let t = TipImage::from_picture(&img).expect("a tip");
        assert_eq!(t.n, 120);
        assert!(t.at(60.0, 60.0) > 0.9 && t.at(5.0, 5.0) < 0.1, "dark = paint, centered: {} {}", t.at(60.0, 60.0), t.at(5.0, 5.0));
        assert!(t.at(60.0, 20.0) < 0.1, "the padding above the bar is empty");
        let j = serde_json::to_string(&t).unwrap();
        let back: TipImage = serde_json::from_str(&j).unwrap();
        assert_eq!(back, t);
        assert_eq!(*back.a, *t.a);
        // transparency wins over darkness
        let mut cut = image::RgbaImage::from_pixel(50, 50, image::Rgba([255, 255, 255, 0]));
        for y in 10..40 {
            for x in 10..40 {
                cut.put_pixel(x, y, image::Rgba([255, 255, 255, 255]));
            }
        }
        let t2 = TipImage::from_picture(&cut).unwrap();
        assert!(t2.at(25.0, 25.0) > 0.9 && t2.at(2.0, 2.0) < 0.1);
        // empty and oversized input
        assert!(TipImage::from_picture(&image::RgbaImage::from_pixel(10, 10, image::Rgba([255, 255, 255, 255]))).is_none(), "white on white is nothing");
        let big = image::RgbaImage::from_pixel(1000, 600, image::Rgba([0, 0, 0, 255]));
        assert!(TipImage::from_picture(&big).unwrap().n <= MAX_SIDE);
        assert!(serde_json::from_str::<TipImage>("\"@@@\"").is_err());
    }
}
