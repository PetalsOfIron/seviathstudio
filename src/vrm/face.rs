// SPDX-License-Identifier: GPL-3.0-or-later
// A face for an uncut picture: three points (the two eyes and the mouth) say where the face is, and the expressions a
// tracking app asks for (blink, the vowels, smile, brows, where the eyes look) are made as small movements of the
// picture around those points. The mouth opens by a dark shape that grows out of the lips. The shapes can also be
// taken from a real model (`template.rs`), moved onto this face by the same three points.

use serde::{Deserialize, Serialize};

/// Where the face is on the page (page pixels). "Left" is the character's left, the right of the picture.
#[derive(Clone, Copy, PartialEq, Debug, Serialize, Deserialize)]
pub struct FaceMarks {
    pub eye_l: [f32; 2],
    pub eye_r: [f32; 2],
    pub mouth: [f32; 2],
    /// Width of one eye and of the mouth.
    pub eye_w: f32,
    pub mouth_w: f32,
}

impl FaceMarks {
    /// Cleaned: finite and sensible.
    pub fn sanitized(mut self) -> Option<FaceMarks> {
        let ok = |v: f32| v.is_finite() && v.abs() < 1e6;
        if ![self.eye_l[0], self.eye_l[1], self.eye_r[0], self.eye_r[1], self.mouth[0], self.mouth[1], self.eye_w, self.mouth_w].iter().all(|v| ok(*v)) {
            return None;
        }
        self.eye_w = self.eye_w.clamp(2.0, 2000.0);
        self.mouth_w = self.mouth_w.clamp(2.0, 2000.0);
        Some(self)
    }

    pub fn eye_center(&self) -> [f32; 2] {
        [(self.eye_l[0] + self.eye_r[0]) / 2.0, (self.eye_l[1] + self.eye_r[1]) / 2.0]
    }

    /// The distance between the eyes: the unit the face is measured in.
    pub fn eye_distance(&self) -> f32 {
        (self.eye_l[0] - self.eye_r[0]).hypot(self.eye_l[1] - self.eye_r[1]).max(1.0)
    }
}

/// A first guess from the head: its top and its bottom (the neck), and the middle line.
pub fn guess(head_top: [f32; 2], neck: [f32; 2]) -> FaceMarks {
    let h = (neck[1] - head_top[1]).abs().max(20.0);
    let xc = head_top[0];
    let head_w = 0.72 * h;
    FaceMarks { eye_l: [xc + 0.2 * head_w, head_top[1] + 0.54 * h], eye_r: [xc - 0.2 * head_w, head_top[1] + 0.54 * h], mouth: [xc, head_top[1] + 0.79 * h], eye_w: 0.19 * head_w, mouth_w: 0.24 * head_w }
}

fn smooth(t: f32) -> f32 {
    let t = t.clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

/// 1 inside the ellipse (`inner` of its size), fading to 0 at `outer` of its size.
fn bell(p: [f32; 2], c: [f32; 2], a: f32, b: f32, inner: f32, outer: f32) -> f32 {
    let d = (((p[0] - c[0]) / a).powi(2) + ((p[1] - c[1]) / b).powi(2)).sqrt();
    1.0 - smooth((d - inner) / (outer - inner).max(1e-3))
}

/// What every expression is made of: how much it closes each eye, opens or lifts the mouth corners, raises the brows,
/// and where it looks.
#[derive(Clone, Copy, Default)]
pub struct Recipe {
    pub blink_l: f32,
    pub blink_r: f32,
    /// Mouth corners: + up (smile), - down.
    pub corners: f32,
    /// Brows: + up, - down.
    pub brows: f32,
    /// Eyes bigger (+) or smaller.
    pub wide: f32,
    pub look_x: f32,
    pub look_y: f32,
}

/// The recipes of the expressions (name, recipe). The mouth shapes (vowels) are the mouth interior, not here.
pub fn recipes() -> Vec<(&'static str, Recipe)> {
    let r = Recipe::default();
    vec![
        ("blink", Recipe { blink_l: 1.0, blink_r: 1.0, ..r }),
        ("blinkLeft", Recipe { blink_l: 1.0, ..r }),
        ("blinkRight", Recipe { blink_r: 1.0, ..r }),
        ("happy", Recipe { blink_l: 0.55, blink_r: 0.55, corners: 1.0, brows: 0.2, ..r }),
        ("relaxed", Recipe { blink_l: 0.35, blink_r: 0.35, corners: 0.4, ..r }),
        ("sad", Recipe { corners: -1.0, brows: 0.6, blink_l: 0.15, blink_r: 0.15, ..r }),
        ("angry", Recipe { brows: -1.0, corners: -0.3, wide: -0.1, ..r }),
        ("surprised", Recipe { brows: 1.0, wide: 0.25, ..r }),
        ("browInnerUp", Recipe { brows: 1.0, ..r }),
        ("lookLeft", Recipe { look_x: 1.0, ..r }),
        ("lookRight", Recipe { look_x: -1.0, ..r }),
        ("lookUp", Recipe { look_y: -1.0, ..r }),
        ("lookDown", Recipe { look_y: 1.0, ..r }),
    ]
}

/// How far a point of the picture moves for a recipe (page pixels).
pub fn shift(m: &FaceMarks, rc: &Recipe, p: [f32; 2]) -> [f32; 2] {
    let (a, b) = (m.eye_w * 0.72, m.eye_w * 0.55);
    let mut d = [0.0f32; 2];
    for (eye, amount) in [(m.eye_l, rc.blink_l), (m.eye_r, rc.blink_r)] {
        let w = bell(p, eye, a, b, 0.7, 1.45);
        if amount != 0.0 {
            // the eye closes toward its middle line
            d[1] += (eye[1] - p[1]) * 0.9 * amount * w;
        }
        if rc.wide != 0.0 {
            d[0] += (p[0] - eye[0]) * rc.wide * w;
            d[1] += (p[1] - eye[1]) * rc.wide * w;
        }
        if rc.look_x != 0.0 || rc.look_y != 0.0 {
            // what is in the middle of the eye (the iris) moves
            let w2 = bell(p, eye, a * 0.7, b * 0.9, 0.35, 1.0);
            d[0] += rc.look_x * m.eye_w * 0.22 * w2;
            d[1] += rc.look_y * m.eye_w * 0.16 * w2;
        }
        // the brows are above the eye
        if rc.brows != 0.0 {
            let c = [eye[0], eye[1] - m.eye_w * 0.85];
            let wb = bell(p, c, a * 1.5, b * 0.9, 0.5, 1.3);
            d[1] -= rc.brows * m.eye_w * 0.3 * wb;
        }
    }
    if rc.corners != 0.0 {
        for sgn in [-1.0f32, 1.0] {
            let c = [m.mouth[0] + sgn * m.mouth_w * 0.5, m.mouth[1]];
            let w = bell(p, c, m.mouth_w * 0.35, m.mouth_w * 0.3, 0.3, 1.2);
            d[1] -= rc.corners * m.mouth_w * 0.16 * w;
            d[0] += sgn * rc.corners.max(0.0) * m.mouth_w * 0.05 * w;
        }
    }
    d
}

/// The shape of the opening of the mouth for a vowel: (half width, half height) as shares of the mouth width.
pub fn vowel(name: &str) -> Option<(f32, f32)> {
    Some(match name {
        "aa" | "jawOpen" => (0.30, 0.26),
        "ih" => (0.30, 0.07),
        "ou" => (0.12, 0.12),
        "ee" => (0.32, 0.10),
        "oh" => (0.17, 0.21),
        _ => return None,
    })
}

/// The points of the dark shape inside the mouth: a middle point and a ring. At rest all of them are at the middle (so
/// nothing is seen); `open` gives the ring of an open mouth (half width and half height in pixels).
pub fn mouth_shape(m: &FaceMarks, half: Option<(f32, f32)>) -> Vec<[f32; 2]> {
    let n = 20;
    let mut v = vec![m.mouth];
    for i in 0..n {
        let a = std::f32::consts::TAU * i as f32 / n as f32;
        v.push(match half {
            Some((rx, ry)) => [m.mouth[0] + a.cos() * rx, m.mouth[1] + a.sin() * ry],
            None => m.mouth,
        });
    }
    v
}

/// The triangles of the shape: a fan.
pub fn mouth_tris() -> Vec<[u32; 3]> {
    let n = 20u32;
    (0..n).map(|i| [0, 1 + i, 1 + (i + 1) % n]).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn marks() -> FaceMarks {
        FaceMarks { eye_l: [600.0, 400.0], eye_r: [400.0, 400.0], mouth: [500.0, 600.0], eye_w: 80.0, mouth_w: 100.0 }
    }

    #[test]
    fn the_guess_puts_the_eyes_above_the_mouth_in_the_middle_of_the_head() {
        let g = guess([500.0, 100.0], [500.0, 420.0]);
        assert!(g.eye_l[0] > 500.0 && g.eye_r[0] < 500.0, "the character's left is the right of the picture");
        assert!(g.eye_l[1] > 100.0 && g.eye_l[1] < g.mouth[1] && g.mouth[1] < 420.0);
        assert!(g.eye_w > 10.0 && g.mouth_w > 10.0);
        assert!(FaceMarks { eye_w: f32::NAN, ..g }.sanitized().is_none());
        assert!(g.sanitized().is_some());
    }

    #[test]
    fn a_blink_closes_one_eye_toward_its_middle_and_leaves_the_rest() {
        let m = marks();
        let r = Recipe { blink_l: 1.0, ..Default::default() };
        // above the left eye's middle goes down, below goes up, the middle stays
        let up = shift(&m, &r, [600.0, 380.0]);
        let down = shift(&m, &r, [600.0, 420.0]);
        assert!(up[1] > 10.0 && down[1] < -10.0, "{up:?} {down:?}");
        assert!(shift(&m, &r, [600.0, 400.0])[1].abs() < 0.01);
        // the other eye and the mouth do not move
        assert_eq!(shift(&m, &r, [400.0, 380.0]), [0.0, 0.0]);
        assert_eq!(shift(&m, &r, [500.0, 600.0]), [0.0, 0.0]);
        // far away nothing moves
        assert_eq!(shift(&m, &r, [900.0, 900.0]), [0.0, 0.0]);
    }

    #[test]
    fn a_smile_lifts_the_corners_and_the_brows_move_up() {
        let m = marks();
        let smile = Recipe { corners: 1.0, ..Default::default() };
        let corner = shift(&m, &smile, [550.0, 600.0]);
        assert!(corner[1] < -4.0, "the corner goes up: {corner:?}");
        assert!(shift(&m, &smile, [500.0, 600.0])[1].abs() < corner[1].abs(), "the middle of the lips moves less");
        let sad = shift(&m, &Recipe { corners: -1.0, ..Default::default() }, [550.0, 600.0]);
        assert!(sad[1] > 4.0, "down");
        let brows = shift(&m, &Recipe { brows: 1.0, ..Default::default() }, [600.0, 400.0 - 80.0 * 0.85]);
        assert!(brows[1] < -5.0, "{brows:?}");
        // looking right moves the middle of the eye to the right of the picture
        let look = shift(&m, &Recipe { look_x: 1.0, ..Default::default() }, [600.0, 400.0]);
        assert!(look[0] > 5.0);
    }

    #[test]
    fn the_mouth_shape_is_nothing_at_rest_and_the_vowels_open_it() {
        let m = marks();
        let rest = mouth_shape(&m, None);
        assert!(rest.iter().all(|p| *p == m.mouth), "all at one point: not seen");
        let (rx, ry) = vowel("aa").unwrap();
        let open = mouth_shape(&m, Some((rx * m.mouth_w, ry * m.mouth_w)));
        assert_eq!(open.len(), rest.len());
        let wide = open.iter().map(|p| (p[0] - m.mouth[0]).abs()).fold(0.0f32, f32::max);
        let tall = open.iter().map(|p| (p[1] - m.mouth[1]).abs()).fold(0.0f32, f32::max);
        assert!((wide - 30.0).abs() < 0.5 && (tall - 26.0).abs() < 0.5, "{wide} {tall}");
        assert_eq!(mouth_tris().len(), 20);
        assert!(mouth_tris().iter().all(|t| t.iter().all(|i| (*i as usize) < rest.len())));
        // the oh is rounder than the ee; every vowel exists
        assert!(vowel("ee").unwrap().0 > vowel("oh").unwrap().0);
        for v in ["aa", "ih", "ou", "ee", "oh"] {
            assert!(vowel(v).is_some());
        }
        assert!(vowel("blink").is_none());
    }
}
