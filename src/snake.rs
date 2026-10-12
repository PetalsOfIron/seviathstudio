// Snake artwork drawn with the painter, so it stays sharp at every UI scale:
// the coiled-S logo, the snake divider under the top bar and the scale pattern behind the canvas.

use std::f32::consts::{PI, TAU};

use eframe::egui::{Color32, Painter, Pos2, Rect, Shape, Stroke, Vec2, pos2, vec2};

use crate::theme::{EMERALD, EMERALD_DEEP, GOLD, VENOM};

const TONGUE: Color32 = Color32::from_rgb(222, 64, 86);
const OUTLINE: Color32 = Color32::from_rgb(6, 12, 9);

/// The SeviathStudio mark: a snake coiled into an "S", head on top with a forked tongue.
pub fn logo(p: &Painter, center: Pos2, size: f32) {
    let w = size * 0.40;
    let h = size * 0.80;
    let spine = |s: f32| center + vec2(-(TAU * s).sin() * w * 0.5, (s - 0.5) * h);
    let radius = |s: f32| size * (0.035 + 0.06 * (1.0 - s));

    let n = 56;
    // Outline first, then the body from tail to head so the head ends on top.
    for i in (0..=n).rev() {
        let s = i as f32 / n as f32;
        p.circle_filled(spine(s), radius(s) + size * 0.018, OUTLINE);
    }
    for i in (0..=n).rev() {
        let s = i as f32 / n as f32;
        let band = (i / 5) % 2 == 0;
        p.circle_filled(spine(s), radius(s), if band { EMERALD } else { EMERALD_DEEP });
        if i % 5 == 2 {
            // Gold scale glint along the back.
            p.circle_filled(spine(s) + vec2(-radius(s) * 0.35, -radius(s) * 0.35), radius(s) * 0.28, GOLD);
        }
    }

    // Head.
    let head = spine(0.0);
    let dir = (spine(0.0) - spine(0.03)).normalized();
    let side = vec2(-dir.y, dir.x);
    let hr = size * 0.105;
    let hc = head + dir * hr * 0.35;
    p.circle_filled(hc, hr + size * 0.018, OUTLINE);
    p.circle_filled(hc, hr, EMERALD);
    p.circle_filled(hc + dir * hr * 0.2 - side * hr * 0.15, hr * 0.55, VENOM.gamma_multiply(0.55));

    // Eye with a slit pupil.
    let eye = hc + dir * hr * 0.25 + side * hr * 0.45;
    p.circle_filled(eye, hr * 0.28, GOLD);
    p.line_segment([eye - dir * hr * 0.2, eye + dir * hr * 0.2], Stroke::new((hr * 0.1).max(1.0), OUTLINE));

    // Forked tongue.
    let tw = (size * 0.025).max(1.0);
    let base = hc + dir * hr;
    let mid = base + dir * size * 0.10;
    p.line_segment([base, mid], Stroke::new(tw, TONGUE));
    for a in [-0.45f32, 0.45] {
        let d = rotate(dir, a);
        p.line_segment([mid, mid + d * size * 0.06], Stroke::new(tw, TONGUE));
    }
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
