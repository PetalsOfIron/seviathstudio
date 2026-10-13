// SPDX-License-Identifier: GPL-3.0-or-later
// Snake artwork drawn with the painter, so it stays sharp at every UI scale:
// the coiled-S logo, the snake divider under the top bar and the scale pattern behind the canvas.

use std::f32::consts::{PI, TAU};

use eframe::egui::{Color32, Painter, Pos2, Rect, Shape, Stroke, Vec2, pos2, vec2};

use crate::theme::{EMERALD, EMERALD_DEEP, GOLD, VENOM};

const TONGUE: Color32 = Color32::from_rgb(222, 64, 86);
const OUTLINE: Color32 = Color32::from_rgb(6, 12, 9);

/// One shape of the logo (so the same drawing works on screen and in the app icon).
pub enum Mark {
    Circle(Pos2, f32, Color32),
    Line(Pos2, Pos2, f32, Color32),
}

/// The SeviathStudio mark: a snake coiled into an "S", head on top with a forked tongue.
pub fn logo_marks(center: Pos2, size: f32) -> Vec<Mark> {
    let mut m = Vec::new();
    let w = size * 0.40;
    let h = size * 0.80;
    let spine = |s: f32| center + vec2(-(TAU * s).sin() * w * 0.5, (s - 0.5) * h);
    let radius = |s: f32| size * (0.035 + 0.06 * (1.0 - s));

    let n = 56;
    // Outline first, then the body from tail to head so the head ends on top.
    for i in (0..=n).rev() {
        let s = i as f32 / n as f32;
        m.push(Mark::Circle(spine(s), radius(s) + size * 0.018, OUTLINE));
    }
    for i in (0..=n).rev() {
        let s = i as f32 / n as f32;
        let band = (i / 5) % 2 == 0;
        m.push(Mark::Circle(spine(s), radius(s), if band { EMERALD } else { EMERALD_DEEP }));
        if i % 5 == 2 {
            // Gold scale glint along the back.
            m.push(Mark::Circle(spine(s) + vec2(-radius(s) * 0.35, -radius(s) * 0.35), radius(s) * 0.28, GOLD));
        }
    }

    // Head.
    let head = spine(0.0);
    let dir = (spine(0.0) - spine(0.03)).normalized();
    let side = vec2(-dir.y, dir.x);
    let hr = size * 0.105;
    let hc = head + dir * hr * 0.35;
    m.push(Mark::Circle(hc, hr + size * 0.018, OUTLINE));
    m.push(Mark::Circle(hc, hr, EMERALD));
    m.push(Mark::Circle(hc + dir * hr * 0.2 - side * hr * 0.15, hr * 0.55, VENOM.gamma_multiply(0.55)));

    // Eye with a slit pupil.
    let eye = hc + dir * hr * 0.25 + side * hr * 0.45;
    m.push(Mark::Circle(eye, hr * 0.28, GOLD));
    m.push(Mark::Line(eye - dir * hr * 0.2, eye + dir * hr * 0.2, (hr * 0.1).max(1.0), OUTLINE));

    // Forked tongue.
    let tw = (size * 0.025).max(1.0);
    let base = hc + dir * hr;
    let mid = base + dir * size * 0.10;
    m.push(Mark::Line(base, mid, tw, TONGUE));
    for a in [-0.45f32, 0.45] {
        let d = rotate(dir, a);
        m.push(Mark::Line(mid, mid + d * size * 0.06, tw, TONGUE));
    }
    m
}

pub fn logo(p: &Painter, center: Pos2, size: f32) {
    for mark in logo_marks(center, size) {
        match mark {
            Mark::Circle(c, r, col) => {
                p.circle_filled(c, r, col);
            }
            Mark::Line(a, b, w, col) => {
                p.line_segment([a, b], Stroke::new(w, col));
            }
        }
    }
}

/// The app icon as straight RGBA, `n`×`n`: the logo on a dark rounded tile with an emerald rim.
pub fn icon_rgba(n: usize) -> Vec<u8> {
    let nf = n as f32;
    let mut px = vec![[0.0f32; 4]; n * n]; // premultiplied
    let over = |px: &mut [f32; 4], c: Color32, cov: f32| {
        if cov <= 0.0 {
            return;
        }
        let a = c.a() as f32 / 255.0 * cov.min(1.0);
        let k = 1.0 - a;
        let ch = |v: u8| v as f32 / 255.0;
        // Color32 colors are premultiplied; scale them by coverage
        *px = [ch(c.r()) * cov + px[0] * k, ch(c.g()) * cov + px[1] * k, ch(c.b()) * cov + px[2] * k, a + px[3] * k];
    };
    // rounded tile
    let (rad, inset) = (nf * 0.22, nf * 0.04);
    let tile = Rect::from_min_max(pos2(inset, inset), pos2(nf - inset, nf - inset));
    for y in 0..n {
        for x in 0..n {
            let p = pos2(x as f32 + 0.5, y as f32 + 0.5);
            let q = vec2((p.x - tile.center().x).abs() - (tile.width() / 2.0 - rad), (p.y - tile.center().y).abs() - (tile.height() / 2.0 - rad));
            let outside = vec2(q.x.max(0.0), q.y.max(0.0)).length() + q.x.max(q.y).min(0.0) - rad;
            over(&mut px[y * n + x], EMERALD_DEEP, (0.5 - outside).clamp(0.0, 1.0));
            over(&mut px[y * n + x], Color32::from_rgb(14, 20, 17), (0.5 - outside - nf * 0.03).clamp(0.0, 1.0));
        }
    }
    for mark in logo_marks(pos2(nf * 0.5, nf * 0.555), nf * 0.74) {
        let (col, lo, hi) = match &mark {
            Mark::Circle(c, r, col) => (*col, *c - vec2(r + 1.0, r + 1.0), *c + vec2(r + 1.0, r + 1.0)),
            Mark::Line(a, b, w, col) => {
                let r = w / 2.0 + 1.0;
                (*col, pos2(a.x.min(b.x) - r, a.y.min(b.y) - r), pos2(a.x.max(b.x) + r, a.y.max(b.y) + r))
            }
        };
        let coverage = |p: Pos2| match &mark {
            Mark::Circle(c, r, _) => (r - (p - *c).length() + 0.5).clamp(0.0, 1.0),
            Mark::Line(a, b, w, _) => {
                let ab = *b - *a;
                let t = ((p - *a).dot(ab) / ab.length_sq().max(1e-6)).clamp(0.0, 1.0);
                (w / 2.0 - (p - (*a + ab * t)).length() + 0.5).clamp(0.0, 1.0)
            }
        };
        for y in (lo.y.floor().max(0.0) as usize)..(hi.y.ceil().min(nf) as usize) {
            for x in (lo.x.floor().max(0.0) as usize)..(hi.x.ceil().min(nf) as usize) {
                over(&mut px[y * n + x], col, coverage(pos2(x as f32 + 0.5, y as f32 + 0.5)));
            }
        }
    }
    let mut out = Vec::with_capacity(n * n * 4);
    for p in px {
        let a = p[3].clamp(0.0, 1.0);
        let un = |v: f32| if a > 0.0 { (v / a * 255.0).round().clamp(0.0, 255.0) as u8 } else { 0 };
        out.extend_from_slice(&[un(p[0]), un(p[1]), un(p[2]), (a * 255.0).round() as u8]);
    }
    out
}

/// A thin snake slithering along the bottom edge of a bar, head at the right.
pub fn divider(p: &Painter, rect: Rect) {
    let y = rect.bottom() - 2.5;
    let amp = 1.6;
    let period = 26.0;
    let x0 = rect.left();
    let x1 = rect.right() - 14.0;
    let mut pts = Vec::new();
    let mut x = x0;
    while x <= x1 {
        pts.push(pos2(x, y + (x / period * TAU).sin() * amp));
        x += 2.0;
    }
    p.add(Shape::line(pts, Stroke::new(1.4, EMERALD.gamma_multiply(0.55))));

    // Gold scales on every crest.
    let mut k = (x0 / period).ceil();
    while k * period < x1 {
        let cx = k * period + period * 0.25;
        p.circle_filled(pos2(cx, y - amp), 1.1, GOLD.gamma_multiply(0.7));
        k += 1.0;
    }

    let head = pos2(x1 + 6.0, y + (x1 / period * TAU).sin() * amp);
    p.circle_filled(head, 3.2, EMERALD);
    p.circle_filled(head + vec2(1.0, -0.8), 0.9, GOLD);
    p.line_segment([head + vec2(3.0, 0.0), head + vec2(7.0, 0.0)], Stroke::new(0.9, TONGUE));
}

/// Overlapping scales, very faint, used behind the canvas.
pub fn scales(p: &Painter, rect: Rect, color: Color32) {
    let p = p.with_clip_rect(rect);
    let sx = 28.0;
    let sy = 15.0;
    let r = sx * 0.5;
    let stroke = Stroke::new(1.0, color);
    let mut row = 0;
    let mut y = rect.top() - sy;
    while y < rect.bottom() + sy {
        let off = if row % 2 == 0 { 0.0 } else { r };
        let mut x = rect.left() - sx + off;
        while x < rect.right() + sx {
            p.add(Shape::line(arc(pos2(x, y), r, 0.0, PI, 10), stroke));
            x += sx;
        }
        y += sy;
        row += 1;
    }
}

/// Points along a circle arc (angles in radians, screen y down).
pub fn arc(c: Pos2, r: f32, a0: f32, a1: f32, n: usize) -> Vec<Pos2> {
    (0..=n)
        .map(|i| {
            let a = a0 + (a1 - a0) * i as f32 / n as f32;
            c + vec2(a.cos(), a.sin()) * r
        })
        .collect()
}

fn rotate(v: Vec2, a: f32) -> Vec2 {
    let (s, c) = a.sin_cos();
    vec2(v.x * c - v.y * s, v.x * s + v.y * c)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Writes assets/seviath.ico (16–256 px) and assets/icon.png from the logo.
    /// Run after changing the logo: `cargo test --release write_app_icon`
    #[test]
    fn write_app_icon() {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("assets");
        std::fs::create_dir_all(&dir).unwrap();
        let png = |n: usize| image::RgbaImage::from_raw(n as u32, n as u32, icon_rgba(n)).unwrap();
        png(512).save(dir.join("icon.png")).unwrap();
        let frames: Vec<image::codecs::ico::IcoFrame> = [16usize, 24, 32, 48, 64, 128, 256]
            .iter()
            .map(|&n| image::codecs::ico::IcoFrame::as_png(&icon_rgba(n), n as u32, n as u32, image::ExtendedColorType::Rgba8).unwrap())
            .collect();
        let f = std::fs::File::create(dir.join("seviath.ico")).unwrap();
        image::codecs::ico::IcoEncoder::new(f).encode_images(&frames).unwrap();
        // the tile is opaque in the middle, transparent in the corners
        let px = icon_rgba(64);
        assert_eq!(px[(32 * 64 + 32) * 4 + 3], 255);
        assert_eq!(px[3], 0);
    }
}
