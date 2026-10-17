// SPDX-License-Identifier: GPL-3.0-or-later
// Signatures: your own hand-drawn mark, kept as lines with pressure and timing (not as a picture),
// in four slots (full, short, initials, collab mark). A signature can be stamped as a vector layer,
// painted on again with your own timing, or added to every export in a corner of your choice.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::doc::{Document, LayerKind};
use crate::vector::{self, LineEnds, LineStyle, VecData, VecStroke};

#[derive(Clone, Copy, PartialEq, Eq, Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Slot {
    #[default]
    Full,
    Short,
    Initials,
    Collab,
}

impl Slot {
    pub const ALL: [Slot; 4] = [Slot::Full, Slot::Short, Slot::Initials, Slot::Collab];

    pub fn id(self) -> &'static str {
        match self {
            Slot::Full => "full",
            Slot::Short => "short",
            Slot::Initials => "initials",
            Slot::Collab => "collab",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Slot::Full => "Full signature",
            Slot::Short => "Short",
            Slot::Initials => "Initials",
            Slot::Collab => "Collab mark",
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Corner {
    TopLeft,
    TopRight,
    BottomLeft,
    #[default]
    BottomRight,
}

impl Corner {
    pub const ALL: [Corner; 4] = [Corner::TopLeft, Corner::TopRight, Corner::BottomLeft, Corner::BottomRight];

    pub fn label(self) -> &'static str {
        match self {
            Corner::TopLeft => "Top left",
            Corner::TopRight => "Top right",
            Corner::BottomLeft => "Bottom left",
            Corner::BottomRight => "Bottom right",
        }
    }
}

/// The per-project rule for signing exports.
#[derive(Clone, Copy, PartialEq, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct AutoSign {
    pub on: bool,
    pub slot: Slot,
    pub corner: Corner,
    /// Width of the signature as a share of the picture width, percent.
    pub size: f32,
    /// 0..100
    pub opacity: f32,
    /// Distance from the edges as a share of the picture width, percent.
    pub margin: f32,
}

impl Default for AutoSign {
    fn default() -> Self {
        Self { on: false, slot: Slot::Full, corner: Corner::BottomRight, size: 14.0, opacity: 90.0, margin: 3.0 }
    }
}

impl AutoSign {
    pub fn sanitized(mut self) -> Self {
        let f = |v: f32, lo: f32, hi: f32, d: f32| if v.is_finite() { v.clamp(lo, hi) } else { d };
        self.size = f(self.size, 2.0, 80.0, 14.0);
        self.opacity = f(self.opacity, 5.0, 100.0, 90.0);
        self.margin = f(self.margin, 0.0, 20.0, 3.0);
        self
    }
}

/// One line of a signature: x, y, pressure 0..1, seconds since the signature began.
#[derive(Clone, PartialEq, Debug, Default, Serialize, Deserialize)]
pub struct SigStroke {
    pub pts: Vec<[f32; 4]>,
}

#[derive(Clone, PartialEq, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct Signature {
    pub strokes: Vec<SigStroke>,
    pub style: LineStyle,
}

impl Default for Signature {
    fn default() -> Self {
        Self { strokes: Vec::new(), style: LineStyle { size: 5.0, min_size: 0.2, ends: LineEnds::Taper, taper: 0.5, ..LineStyle::default() } }
    }
}

/// Longest pause kept between two lines when a signature is replayed.
const MAX_GAP: f32 = 0.35;

impl Signature {
    pub fn is_empty(&self) -> bool {
        self.strokes.iter().all(|s| s.pts.len() < 2)
    }

    /// Box around the lines (with the line width): (x0, y0, x1, y1).
    pub fn bounds(&self) -> Option<(f32, f32, f32, f32)> {
        let mut b: Option<(f32, f32, f32, f32)> = None;
        for p in self.strokes.iter().flat_map(|s| &s.pts) {
            b = Some(match b {
                Some(o) => (o.0.min(p[0]), o.1.min(p[1]), o.2.max(p[0]), o.3.max(p[1])),
                None => (p[0], p[1], p[0], p[1]),
            });
        }
        let r = self.style.size / 2.0;
        b.map(|b| (b.0 - r, b.1 - r, b.2 + r, b.3 + r))
    }

    pub fn size(&self) -> (f32, f32) {
        self.bounds().map(|b| ((b.2 - b.0).max(1.0), (b.3 - b.1).max(1.0))).unwrap_or((1.0, 1.0))
    }

    /// How long it takes to write, seconds.
    pub fn duration(&self) -> f32 {
        self.strokes.iter().flat_map(|s| s.pts.last()).map(|p| p[3]).fold(0.0, f32::max)
    }

    /// Move the box's top-left corner to (0, 0), start the clock at 0 and shorten long pauses.
    pub fn normalized(mut self) -> Self {
        self.strokes.retain(|s| s.pts.len() >= 2);
        if let Some(b) = self.bounds() {
            for p in self.strokes.iter_mut().flat_map(|s| s.pts.iter_mut()) {
                p[0] -= b.0;
                p[1] -= b.1;
            }
        }
        let t0 = self.strokes.first().and_then(|s| s.pts.first()).map(|p| p[3]).unwrap_or(0.0);
        let mut shift = t0;
        let mut prev_end = t0;
        for s in &mut self.strokes {
            let start = s.pts.first().map(|p| p[3]).unwrap_or(0.0);
            let gap = start - prev_end;
            if gap > MAX_GAP {
                shift += gap - MAX_GAP;
            }
            for p in &mut s.pts {
                p[3] -= shift;
            }
            prev_end = s.pts.last().map(|p| p[3] + shift).unwrap_or(prev_end);
        }
        self
    }

    /// Project and user files come from outside: keep numbers sane and sizes bounded.
    pub fn sanitized(mut self) -> Self {
        self.strokes.truncate(300);
        for s in &mut self.strokes {
            s.pts.truncate(20_000);
            s.pts.retain(|p| p.iter().all(|v| v.is_finite()) && p[0].abs() < 1e6 && p[1].abs() < 1e6);
            for p in &mut s.pts {
                p[2] = p[2].clamp(0.0, 1.0);
                p[3] = p[3].clamp(0.0, 600.0);
            }
        }
        self.strokes.retain(|s| s.pts.len() >= 2);
        let st = &mut self.style;
        st.size = if st.size.is_finite() { st.size.clamp(0.5, 200.0) } else { 5.0 };
        st.min_size = st.min_size.clamp(0.0, 1.0);
        st.taper = st.taper.clamp(0.0, 3.0);
        st.opacity = st.opacity.clamp(0.0, 1.0);
        self
    }

    /// Smooth out the wobble and even out the weight, but keep the shape (amount 0..1).
    pub fn cleaned(&self, amount: f32) -> Signature {
        let a = amount.clamp(0.0, 1.0);
        let passes = (a * 6.0).round() as usize;
        let mut out = self.clone();
        let (mut sum, mut n) = (0.0, 0.0);
        for p in out.strokes.iter().flat_map(|s| &s.pts) {
            sum += p[2];
            n += 1.0;
        }
        let mean = if n > 0.0 { sum / n } else { 1.0 };
        for s in &mut out.strokes {
            for _ in 0..passes {
                let old = s.pts.clone();
                for i in 1..old.len().saturating_sub(1) {
                    for k in [0, 1, 2] {
                        s.pts[i][k] = old[i - 1][k] * 0.25 + old[i][k] * 0.5 + old[i + 1][k] * 0.25;
                    }
                }
            }
            for p in &mut s.pts {
                p[2] = p[2] * (1.0 - a * 0.6) + mean * a * 0.6;
            }
        }
        out.normalized()
    }

    /// The lines as vector strokes, with the top-left corner at `(x, y)` and everything scaled by `k`.
    /// `until`: only what was written in the first `until` seconds (None = all of it).
    pub fn vec_strokes(&self, x: f32, y: f32, k: f32, until: Option<f32>) -> Vec<VecStroke> {
        let mut out = Vec::new();
        let mut style = self.style;
        style.size = (style.size * k).max(0.3);
        for s in &self.strokes {
            let pts: Vec<[f32; 3]> = s.pts.iter().take_while(|p| until.is_none_or(|u| p[3] <= u)).map(|p| [x + p[0] * k, y + p[1] * k, p[2]]).collect();
            if pts.len() >= 2 {
                out.push(VecStroke { pts, style });
            }
        }
        out
    }

    /// Where it goes on a picture and how much it is scaled: (x, y, scale).
    pub fn place(&self, doc_w: u32, doc_h: u32, rule: &AutoSign) -> (f32, f32, f32) {
        let (w, h) = self.size();
        let (dw, dh) = (doc_w as f32, doc_h as f32);
        let mut k = dw * rule.size / 100.0 / w;
        // a tall mark must not cover the picture
        k = k.min(dh * 0.4 / h);
        let (sw, sh) = (w * k, h * k);
        let m = dw * rule.margin / 100.0;
        let x = match rule.corner {
            Corner::TopLeft | Corner::BottomLeft => m,
            _ => dw - m - sw,
        };
        let y = match rule.corner {
            Corner::TopLeft | Corner::TopRight => m,
            _ => dh - m - sh,
        };
        (x.max(0.0), y.max(0.0), k)
    }

    /// A new top layer holding the signature, placed by `rule` (a vector layer, so it can still be moved and scaled).
    pub fn add_layer(&self, doc: &mut Document, rule: &AutoSign, name: &str) {
        let (x, y, k) = self.place(doc.w, doc.h, rule);
        let mut l = doc.new_layer(name, LayerKind::Vector);
        l.vector = Some(std::sync::Arc::new(VecData { strokes: self.vec_strokes(x, y, k, None), width: 1.0, fills: Vec::new() }));
        l.opacity = rule.opacity.clamp(0.0, 100.0);
        let (w, h) = (doc.w, doc.h);
        vector::render_all(&mut l, w, h);
        doc.layers.insert(0, l);
        doc.mark_all_dirty();
    }
}

// ---------- kept on this computer, one file per slot ----------

pub fn dir() -> PathBuf {
    if cfg!(test) {
        return std::env::temp_dir().join(format!("seviath_signatures_{}", std::process::id()));
    }
    dirs::config_dir().unwrap_or_else(std::env::temp_dir).join("SeviathStudio").join("signatures")
}

pub fn save_in(dir: &Path, slot: Slot, sig: &Signature) -> Result<(), String> {
    std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    let text = serde_json::to_string(sig).map_err(|e| e.to_string())?;
    std::fs::write(dir.join(format!("{}.json", slot.id())), text).map_err(|e| e.to_string())
}

pub fn load_in(dir: &Path, slot: Slot) -> Option<Signature> {
    let p = dir.join(format!("{}.json", slot.id()));
    if std::fs::metadata(&p).ok()?.len() > 32 << 20 {
        return None;
    }
    let s: Signature = serde_json::from_slice(&std::fs::read(p).ok()?).ok()?;
    let s = s.sanitized();
    (!s.is_empty()).then_some(s)
}

pub fn delete_in(dir: &Path, slot: Slot) {
    let _ = std::fs::remove_file(dir.join(format!("{}.json", slot.id())));
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> Signature {
        let line = |dx: f32, t0: f32| SigStroke { pts: (0..20).map(|i| [10.0 + dx + i as f32 * 5.0, 40.0 + (i as f32 * 0.7).sin() * 12.0, 0.3 + i as f32 / 40.0, t0 + i as f32 * 0.05]).collect() };
        Signature { strokes: vec![line(0.0, 3.0), line(30.0, 9.0)], ..Signature::default() }
    }

    #[test]
    fn normalizing_moves_it_to_the_corner_and_shortens_pauses() {
        let s = sample().normalized();
        let b = s.bounds().unwrap();
        assert!(b.0.abs() < 0.01 && b.1.abs() < 0.01, "{b:?}");
        assert!(s.strokes[0].pts[0][3].abs() < 1e-4);
        // the 5 second pause is now short
        assert!(s.strokes[1].pts[0][3] - s.strokes[0].pts.last().unwrap()[3] <= MAX_GAP + 1e-3);
        assert!(s.duration() < 3.0);
    }

    #[test]
    fn replay_shows_only_what_was_written_so_far() {
        let s = sample().normalized();
        assert!(s.vec_strokes(0.0, 0.0, 1.0, Some(-1.0)).is_empty());
        let part = s.vec_strokes(0.0, 0.0, 1.0, Some(0.5));
        assert_eq!(part.len(), 1);
        assert!(part[0].pts.len() > 2 && part[0].pts.len() < 20);
        assert_eq!(s.vec_strokes(0.0, 0.0, 1.0, None).len(), 2);
        assert_eq!(s.vec_strokes(0.0, 0.0, 1.0, Some(100.0)).len(), 2);
        let big = s.vec_strokes(100.0, 50.0, 2.0, None);
        assert!((big[0].pts[0][0] - (100.0 + s.strokes[0].pts[0][0] * 2.0)).abs() < 1e-3);
        assert!((big[0].style.size - s.style.size * 2.0).abs() < 1e-3);
    }

    #[test]
    fn placing_by_corner_size_and_margin() {
        let s = sample().normalized();
        let rule = AutoSign { size: 20.0, margin: 5.0, ..AutoSign::default() };
        let (x, y, k) = s.place(2000, 1000, &rule);
        let (w, _) = s.size();
        assert!((w * k - 400.0).abs() < 1.0, "width {}", w * k);
        assert!(x > 1000.0 && y > 500.0, "bottom right: {x},{y}");
        assert!((x + w * k - 1900.0).abs() < 1.0, "5% from the right edge");
        let (x, y, _) = s.place(2000, 1000, &AutoSign { corner: Corner::TopLeft, ..rule });
        assert!((x - 100.0).abs() < 0.01 && (y - 100.0).abs() < 0.01);
    }

    #[test]
    fn cleaning_smooths_the_wobble_and_keeps_the_shape() {
        let mut s = sample();
        for (i, p) in s.strokes[0].pts.iter_mut().enumerate() {
            p[1] += if i % 2 == 0 { 3.0 } else { -3.0 };
        }
        let s = s.normalized();
        let rough = |s: &Signature| s.strokes[0].pts.windows(3).map(|w| (w[0][1] - 2.0 * w[1][1] + w[2][1]).abs()).sum::<f32>();
        let c = s.cleaned(0.8);
        assert!(rough(&c) < rough(&s) * 0.5);
        assert_eq!(c.strokes.len(), 2);
        assert!((c.size().0 - s.size().0).abs() < 8.0);
        let same = s.cleaned(0.0);
        assert!((rough(&same) - rough(&s)).abs() < 0.01);
    }

    #[test]
    fn a_layer_is_added_on_top_with_the_opacity() {
        let mut d = crate::files::testdoc::sample();
        let n = d.layers.len();
        let rule = AutoSign { opacity: 60.0, ..AutoSign::default() };
        sample().normalized().add_layer(&mut d, &rule, "Signature");
        assert_eq!(d.layers.len(), n + 1);
        let l = &d.layers[0];
        assert_eq!(l.name, "Signature");
        assert_eq!(l.opacity, 60.0);
        assert!(!l.tiles.is_empty(), "drawn");
    }

    #[test]
    fn saved_signatures_come_back_and_bad_files_are_refused() {
        let dir = std::env::temp_dir().join(format!("seviath_sig_t_{}", std::process::id()));
        let s = sample().normalized();
        save_in(&dir, Slot::Initials, &s).unwrap();
        assert_eq!(load_in(&dir, Slot::Initials).unwrap().strokes.len(), 2);
        assert!(load_in(&dir, Slot::Full).is_none());
        std::fs::write(dir.join("short.json"), b"{ not json").unwrap();
        assert!(load_in(&dir, Slot::Short).is_none());
        std::fs::write(dir.join("collab.json"), br#"{"strokes":[{"pts":[[1e30,0,5,0],[2,2,2,-4]]}]}"#).unwrap();
        assert!(load_in(&dir, Slot::Collab).is_none(), "nothing usable left");
        delete_in(&dir, Slot::Initials);
        assert!(load_in(&dir, Slot::Initials).is_none());
        std::fs::remove_dir_all(&dir).ok();
    }
}
