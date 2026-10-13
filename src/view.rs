// SPDX-License-Identifier: GPL-3.0-or-later
// Canvas view: zoom, pan, rotation and mirror. Maps between document pixels and screen points.
//
// screen = screen_center + R(angle) · M(flip) · (doc − center) · zoom

use std::f32::consts::PI;

use eframe::egui::{Pos2, Rect, Vec2, pos2, vec2};

pub const MIN_ZOOM: f32 = 0.01;
pub const MAX_ZOOM: f32 = 64.0;

#[derive(Clone, Copy, Debug)]
pub struct View {
    pub zoom: f32,
    pub angle: f32, // radians, clockwise on screen
    pub flip: bool, // mirror left/right
    pub center: Pos2, // document point shown at the middle of the canvas
    pub screen_center: Pos2,
}

impl Default for View {
    fn default() -> Self {
        Self { zoom: 1.0, angle: 0.0, flip: false, center: Pos2::ZERO, screen_center: Pos2::ZERO }
    }
}

fn rot(v: Vec2, a: f32) -> Vec2 {
    let (s, c) = a.sin_cos();
    vec2(v.x * c - v.y * s, v.x * s + v.y * c)
}

impl View {
    pub fn doc_to_screen(&self, p: Pos2) -> Pos2 {
        let mut d = p - self.center;
        if self.flip {
            d.x = -d.x;
        }
        self.screen_center + rot(d * self.zoom, self.angle)
    }

    pub fn screen_to_doc(&self, s: Pos2) -> Pos2 {
        let mut d = rot(s - self.screen_center, -self.angle) / self.zoom;
        if self.flip {
            d.x = -d.x;
        }
        self.center + d
    }

    /// Screen-space delta → document-space delta (for panning).
    fn screen_delta_to_doc(&self, v: Vec2) -> Vec2 {
        let mut d = rot(v, -self.angle) / self.zoom;
        if self.flip {
            d.x = -d.x;
        }
        d
    }

    pub fn pan(&mut self, screen_delta: Vec2) {
        self.center -= self.screen_delta_to_doc(screen_delta);
    }

    /// Zoom by `factor`, keeping the document point under `anchor` (screen) in place.
    pub fn zoom_at(&mut self, anchor: Pos2, factor: f32) {
        let before = self.screen_to_doc(anchor);
        self.zoom = (self.zoom * factor).clamp(MIN_ZOOM, MAX_ZOOM);
        let after = self.screen_to_doc(anchor);
        self.center += before - after;
    }

    pub fn set_zoom(&mut self, z: f32) {
        self.zoom = z.clamp(MIN_ZOOM, MAX_ZOOM);
    }

    /// Rotate around the middle of the canvas. Angle is kept in −180°..180°.
    pub fn rotate(&mut self, da: f32) {
        self.angle = wrap(self.angle + da);
    }

    pub fn set_angle(&mut self, a: f32) {
        self.angle = wrap(a);
    }

    /// Fit the whole document into `area`, keeping rotation and mirror.
    pub fn fit(&mut self, area: Rect, doc: Vec2) {
        let (s, c) = self.angle.sin_cos();
        let bw = (doc.x * c).abs() + (doc.y * s).abs();
        let bh = (doc.x * s).abs() + (doc.y * c).abs();
        let margin = 48.0;
        let z = ((area.width() - margin) / bw).min((area.height() - margin) / bh);
        self.zoom = z.clamp(MIN_ZOOM, MAX_ZOOM);
        self.center = pos2(doc.x / 2.0, doc.y / 2.0);
    }

    /// The four screen corners of the canvas area, in document coordinates (for the navigator frame).
    pub fn visible_quad(&self, area: Rect) -> [Pos2; 4] {
        [
            self.screen_to_doc(area.left_top()),
            self.screen_to_doc(area.right_top()),
            self.screen_to_doc(area.right_bottom()),
            self.screen_to_doc(area.left_bottom()),
        ]
    }

    pub fn angle_deg(&self) -> f32 {
        self.angle.to_degrees()
    }
}

fn wrap(a: f32) -> f32 {
    let mut a = a % (2.0 * PI);
    if a > PI {
        a -= 2.0 * PI;
    }
    if a < -PI {
        a += 2.0 * PI;
    }
    a
}

#[cfg(test)]
mod tests {
    use super::*;

    fn close(a: Pos2, b: Pos2) -> bool {
        (a - b).length() < 1e-3
    }

    #[test]
    fn round_trip_with_rotation_and_flip() {
        let v = View { zoom: 0.37, angle: 0.8, flip: true, center: pos2(1500.0, 2000.0), screen_center: pos2(700.0, 450.0) };
        for p in [pos2(0.0, 0.0), pos2(3000.0, 4000.0), pos2(123.4, 987.6)] {
            assert!(close(v.screen_to_doc(v.doc_to_screen(p)), p));
        }
    }

    #[test]
    fn zoom_keeps_anchor_fixed() {
        let mut v = View { zoom: 0.5, angle: -0.3, flip: false, center: pos2(800.0, 600.0), screen_center: pos2(500.0, 400.0) };
        let anchor = pos2(321.0, 222.0);
        let doc_pt = v.screen_to_doc(anchor);
        v.zoom_at(anchor, 2.7);
        assert!(close(v.doc_to_screen(doc_pt), anchor));
    }

    #[test]
    fn pan_moves_content_with_pointer() {
        let mut v = View { zoom: 2.0, angle: 1.1, flip: true, center: pos2(10.0, 10.0), screen_center: pos2(0.0, 0.0) };
        let p = pos2(40.0, 25.0);
        let before = v.doc_to_screen(p);
        v.pan(vec2(13.0, -7.0));
        assert!(close(v.doc_to_screen(p), before + vec2(13.0, -7.0)));
    }
}
