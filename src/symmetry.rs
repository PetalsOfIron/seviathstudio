// SPDX-License-Identifier: GPL-3.0-or-later
// Live symmetry: every brush dab is also drawn at its mirror images. The axes go through a
// center you can drag anywhere and can be turned. Modes as in the first app: left/right,
// top/bottom, both (4), radial (N slices) and kaleidoscope (radial + mirrored).

use eframe::egui::{Pos2, pos2};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SymMode {
    Off,
    Vertical,
    Horizontal,
    Quad,
    Radial,
    Kaleido,
}

impl SymMode {
    pub const ALL: [(SymMode, &'static str, &'static str); 6] = [
        (SymMode::Off, "Off", "Draw normally."),
        (SymMode::Vertical, "Left / right", "What you draw on one side appears mirrored on the other. Good for faces, bodies, armor."),
        (SymMode::Horizontal, "Top / bottom", "Top and bottom mirror each other."),
        (SymMode::Quad, "Both (4)", "Mirrors across both axes: 4 copies."),
        (SymMode::Radial, "Radial", "Copies turn around the center, like a flower or a mandala."),
        (SymMode::Kaleido, "Kaleidoscope", "Radial copies that are also mirrored, like lace or a kaleidoscope."),
    ];

    pub fn label(self) -> &'static str {
        Self::ALL.iter().find(|m| m.0 == self).map(|m| m.1).unwrap_or("Off")
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Symmetry {
    pub mode: SymMode,
    pub cx: f32,
    pub cy: f32,
    /// Radial / kaleidoscope: number of slices (2..32).
    pub count: u32,
    /// Turns the axes, degrees (0 = straight up).
    pub angle: f32,
}

impl Default for Symmetry {
    fn default() -> Self {
        Self { mode: SymMode::Off, cx: 0.0, cy: 0.0, count: 6, angle: 0.0 }
    }
}

/// One mirror image: a rotation or a reflection around the center.
#[derive(Clone, Copy, Debug)]
pub struct Mirror {
    cx: f32,
    cy: f32,
    reflect: bool,
    /// rotation: the turn; reflection: the axis direction (radians, 0 = pointing right)
    a: f32,
}

impl Mirror {
    pub fn point(&self, x: f32, y: f32) -> (f32, f32) {
        let (dx, dy) = (x - self.cx, y - self.cy);
        if self.reflect {
            let (s, c) = (2.0 * self.a).sin_cos();
            (self.cx + dx * c + dy * s, self.cy + dx * s - dy * c)
        } else {
            let (s, c) = self.a.sin_cos();
            (self.cx + dx * c - dy * s, self.cy + dx * s + dy * c)
        }
    }

    /// Where a dab's tip angle points in the copy.
    pub fn angle(&self, ang: f32) -> f32 {
        if self.reflect { 2.0 * self.a - ang } else { ang + self.a }
    }
}

impl Symmetry {
    pub fn on(&self) -> bool {
        self.mode != SymMode::Off
    }

    fn slices(&self) -> u32 {
        self.count.clamp(2, 32)
    }

    /// Every copy, without the original.
    pub fn mirrors(&self) -> Vec<Mirror> {
        use std::f32::consts::{PI, TAU};
        let (cx, cy) = (self.cx, self.cy);
        let a = self.angle.to_radians();
        let v_axis = PI / 2.0 + a;
        let h_axis = a;
        let refl = |ax: f32| Mirror { cx, cy, reflect: true, a: ax };
        let rot = |t: f32| Mirror { cx, cy, reflect: false, a: t };
        match self.mode {
            SymMode::Off => Vec::new(),
            SymMode::Vertical => vec![refl(v_axis)],
            SymMode::Horizontal => vec![refl(h_axis)],
            SymMode::Quad => vec![refl(v_axis), refl(h_axis), rot(PI)],
            SymMode::Radial | SymMode::Kaleido => {
                let n = self.slices();
                let step = TAU / n as f32;
                let mut out: Vec<Mirror> = (1..n).map(|i| rot(step * i as f32)).collect();
                if self.mode == SymMode::Kaleido {
                    out.extend((0..n).map(|i| refl(v_axis + step * i as f32 / 2.0)));
                }
                out
            }
        }
    }

    /// The axes to draw on the canvas (each as two end points), `reach` = canvas diagonal.
    pub fn axis_lines(&self, reach: f32) -> Vec<[Pos2; 2]> {
        use std::f32::consts::{PI, TAU};
        let c = pos2(self.cx, self.cy);
        let a = self.angle.to_radians();
        let dir = |ang: f32| pos2(ang.cos(), ang.sin()).to_vec2() * reach;
        let line = |ang: f32| [c - dir(ang), c + dir(ang)];
        match self.mode {
            SymMode::Off => Vec::new(),
            SymMode::Vertical => vec![line(PI / 2.0 + a)],
            SymMode::Horizontal => vec![line(a)],
            SymMode::Quad => vec![line(PI / 2.0 + a), line(a)],
            SymMode::Radial | SymMode::Kaleido => {
                let n = self.slices();
                (0..n).map(|i| [c, c + dir(-PI / 2.0 + a + TAU * i as f32 / n as f32)]).collect()
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mirrors_land_where_expected() {
        let s = Symmetry { mode: SymMode::Vertical, cx: 100.0, cy: 50.0, ..Default::default() };
        let m = s.mirrors();
        let (x, y) = m[0].point(80.0, 10.0);
        assert!((x - 120.0).abs() < 1e-3 && (y - 10.0).abs() < 1e-3);
        let q = Symmetry { mode: SymMode::Quad, cx: 0.0, cy: 0.0, ..Default::default() };
        let pts: Vec<(i32, i32)> = q.mirrors().iter().map(|m| m.point(3.0, 4.0)).map(|(x, y)| (x.round() as i32, y.round() as i32)).collect();
        assert!(pts.contains(&(-3, 4)) && pts.contains(&(3, -4)) && pts.contains(&(-3, -4)));
        let r = Symmetry { mode: SymMode::Radial, count: 6, ..Default::default() };
        assert_eq!(r.mirrors().len(), 5);
        let k = Symmetry { mode: SymMode::Kaleido, count: 6, ..Default::default() };
        assert_eq!(k.mirrors().len(), 11);
    }
}
