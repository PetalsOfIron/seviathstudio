// SPDX-License-Identifier: GPL-3.0-or-later
// A camera for the Animate tab: it looks at the finished picture and can pan, zoom, turn and shake, so a still character can move
// through a scene. The numbers come from camera tracks on the timeline; this file turns them into where the picture goes on the screen
// and into the pictures that are saved.

use image::{Rgba, RgbaImage};
use serde::{Deserialize, Serialize};

/// Where the camera looks.
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Camera {
    /// How far the middle of the view is from the middle of the picture, in pixels of the picture.
    pub x: f32,
    pub y: f32,
    /// How close the camera is, in percent (100 sees the whole picture, 200 sees the middle half).
    pub zoom: f32,
    /// How far the view is turned (degrees, clockwise).
    pub rot: f32,
}

impl Default for Camera {
    fn default() -> Self {
        Camera { x: 0.0, y: 0.0, zoom: 100.0, rot: 0.0 }
    }
}

impl Camera {
    pub fn is_default(&self) -> bool {
        self.x.abs() < 1e-3 && self.y.abs() < 1e-3 && (self.zoom - 100.0).abs() < 1e-3 && self.rot.abs() < 1e-3
    }

    fn scale(&self) -> f32 {
        (self.zoom / 100.0).clamp(0.01, 100.0)
    }

    /// Where a point of the picture is in the view (both in pixels of a `w` by `h` picture).
    pub fn to_view(&self, p: [f32; 2], w: f32, h: f32) -> [f32; 2] {
        let (cx, cy) = (w / 2.0 + self.x, h / 2.0 + self.y);
        let (s, c) = (-self.rot.to_radians()).sin_cos();
        let (dx, dy) = (p[0] - cx, p[1] - cy);
        let k = self.scale();
        [(dx * c - dy * s) * k + w / 2.0, (dx * s + dy * c) * k + h / 2.0]
    }

    /// Where a point of the view is in the picture.
    pub fn from_view(&self, p: [f32; 2], w: f32, h: f32) -> [f32; 2] {
        let k = self.scale();
        let (dx, dy) = ((p[0] - w / 2.0) / k, (p[1] - h / 2.0) / k);
        let (s, c) = self.rot.to_radians().sin_cos();
        [w / 2.0 + self.x + dx * c - dy * s, h / 2.0 + self.y + dx * s + dy * c]
    }
}

/// A shaking camera (an explosion, a hit, a hand held camera).
#[derive(Clone, Copy, PartialEq, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct Shake {
    pub on: bool,
    /// The biggest move, in pixels.
    pub amount: f32,
    /// How fast it shakes (about this many times a second).
    pub speed: f32,
    /// The biggest turn in degrees.
    pub spin: f32,
}

impl Default for Shake {
    fn default() -> Self {
        Shake { on: false, amount: 12.0, speed: 8.0, spin: 0.6 }
    }
}

impl Shake {
    /// The move (x, y) and the turn at a moment, from a few sines that never repeat exactly.
    pub fn at(&self, t: f32) -> (f32, f32, f32) {
        if !self.on {
            return (0.0, 0.0, 0.0);
        }
        let w = self.speed.max(0.0) * std::f32::consts::TAU;
        let n = |a: f32, b: f32, c: f32| ((t * w * a + b).sin() + (t * w * b * 1.7 + c).sin() * 0.6) / 1.6;
        (n(1.0, 0.3, 1.1) * self.amount, n(1.31, 2.1, 0.4) * self.amount, n(0.77, 1.4, 2.6) * self.spin)
    }
}

/// The picture as the camera sees it: the same size, what the camera does not see of the picture is cut, what is outside the picture
/// is see-through.
pub fn warp(img: &RgbaImage, cam: &Camera) -> RgbaImage {
    if cam.is_default() {
        return img.clone();
    }
    let (w, h) = (img.width(), img.height());
    let mut out = RgbaImage::new(w, h);
    let (wf, hf) = (w as f32, h as f32);
    for y in 0..h {
        for x in 0..w {
            let src = cam.from_view([x as f32 + 0.5, y as f32 + 0.5], wf, hf);
            out.put_pixel(x, y, sample(img, src[0] - 0.5, src[1] - 0.5));
        }
    }
    out
}

/// A smooth look at the picture between its pixels; outside it is see-through.
fn sample(img: &RgbaImage, x: f32, y: f32) -> Rgba<u8> {
    let (w, h) = (img.width() as i32, img.height() as i32);
    let (x0, y0) = (x.floor() as i32, y.floor() as i32);
    let (fx, fy) = (x - x0 as f32, y - y0 as f32);
    let px = |xx: i32, yy: i32| -> [f32; 4] {
        if xx < 0 || yy < 0 || xx >= w || yy >= h {
            return [0.0; 4];
        }
        let p = img.get_pixel(xx as u32, yy as u32).0;
        let a = p[3] as f32;
        [p[0] as f32 * a, p[1] as f32 * a, p[2] as f32 * a, a]
    };
    let (a, b, c, d) = (px(x0, y0), px(x0 + 1, y0), px(x0, y0 + 1), px(x0 + 1, y0 + 1));
    let mut o = [0.0f32; 4];
    for k in 0..4 {
        o[k] = (a[k] * (1.0 - fx) + b[k] * fx) * (1.0 - fy) + (c[k] * (1.0 - fx) + d[k] * fx) * fy;
    }
    if o[3] <= 0.0 {
        return Rgba([0, 0, 0, 0]);
    }
    Rgba([(o[0] / o[3]).round() as u8, (o[1] / o[3]).round() as u8, (o[2] / o[3]).round() as u8, o[3].round() as u8])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_camera_maps_points_both_ways() {
        let cam = Camera { x: 20.0, y: -10.0, zoom: 200.0, rot: 30.0 };
        for p in [[0.0, 0.0], [100.0, 50.0], [37.5, 80.25]] {
            let v = cam.to_view(p, 200.0, 100.0);
            let back = cam.from_view(v, 200.0, 100.0);
            assert!((back[0] - p[0]).abs() < 1e-3 && (back[1] - p[1]).abs() < 1e-3, "{p:?} {back:?}");
        }
        // looking at a point puts it in the middle of the view
        let v = Camera { x: 30.0, y: 0.0, zoom: 100.0, rot: 0.0 }.to_view([130.0, 50.0], 200.0, 100.0);
        assert!((v[0] - 100.0).abs() < 1e-3 && (v[1] - 50.0).abs() < 1e-3);
        // twice as close: a point 10 pixels right of the middle is 20 pixels right
        let v = Camera { zoom: 200.0, ..Default::default() }.to_view([110.0, 50.0], 200.0, 100.0);
        assert!((v[0] - 120.0).abs() < 1e-3);
        assert!(Camera::default().is_default());
    }

    #[test]
    fn a_picture_is_moved_zoomed_and_turned_by_the_camera() {
        let mut img = RgbaImage::new(40, 20);
        for x in 0..40 {
            for y in 0..20 {
                img.put_pixel(x, y, Rgba([x as u8 * 6, 100, 100, 255]));
            }
        }
        assert_eq!(warp(&img, &Camera::default()), img);
        // the camera moved 10 pixels right: the picture moves 10 left
        let p = warp(&img, &Camera { x: 10.0, ..Default::default() });
        assert_eq!(p.get_pixel(5, 10).0[0], img.get_pixel(15, 10).0[0]);
        assert_eq!(p.get_pixel(35, 10).0[3], 0, "nothing of the picture is there");
        // twice as close: the middle of the picture fills the view
        let z = warp(&img, &Camera { zoom: 200.0, ..Default::default() });
        assert!((z.get_pixel(20, 10).0[0] as i32 - img.get_pixel(20, 10).0[0] as i32).abs() <= 6);
        assert!(z.get_pixel(0, 10).0[0] > 40 && z.get_pixel(0, 10).0[0] < 100, "the left edge of the view is a quarter in: {}", z.get_pixel(0, 10).0[0]);
        // a quarter turn about the middle of a picture that is as wide as it is high: the top of the view shows the right side
        let sq = RgbaImage::from_fn(20, 20, |x, y| Rgba([x as u8 * 10, y as u8 * 10, 0, 255]));
        let r = warp(&sq, &Camera { rot: 90.0, ..Default::default() });
        let (a, b) = (r.get_pixel(10, 2).0, sq.get_pixel(17, 10).0);
        assert!((a[0] as i32 - b[0] as i32).abs() <= 10 && (a[1] as i32 - b[1] as i32).abs() <= 10, "{a:?} {b:?}");
    }

    #[test]
    fn a_shake_stays_inside_its_size_and_does_not_repeat() {
        let s = Shake { on: true, amount: 10.0, speed: 6.0, spin: 1.0 };
        let mut seen = std::collections::HashSet::new();
        for i in 0..200 {
            let (x, y, r) = s.at(i as f32 / 30.0);
            assert!(x.abs() <= 10.0 && y.abs() <= 10.0 && r.abs() <= 1.0);
            seen.insert((x * 100.0) as i32);
        }
        assert!(seen.len() > 100, "it keeps moving");
        assert_eq!(Shake::default().at(1.0), (0.0, 0.0, 0.0), "off does nothing");
    }
}
