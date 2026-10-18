// SPDX-License-Identifier: GPL-3.0-or-later
// The pre-rig improver: looks for things in the drawing that go wrong when a part is deformed (stray pixels,
// tiny holes, nearly invisible pixels, light or dark fringes on the edge) and can fix them, one place at a time.
// Nothing here changes a layer by itself: `detect` only looks, `apply` is called when you press Fix.

use std::sync::Arc;

use eframe::egui::Color32;
use rayon::prelude::*;

use crate::doc::{Layer, LayerId, TILE};
use crate::files::layer_bounds;

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum Kind {
    /// A small separate speck of paint.
    Stray,
    /// A small see-through hole inside paint.
    Hole,
    /// Pixels that are nearly see-through (you can't see them, but they are there).
    Ghost,
    /// The edge is lighter than the paint next to it.
    LightFringe,
    /// The edge is darker than the paint next to it.
    DarkFringe,
}

impl Kind {
    pub fn label(self) -> &'static str {
        match self {
            Kind::Stray => "Stray pixels",
            Kind::Hole => "Hole in the paint",
            Kind::Ghost => "Nearly invisible pixels",
            Kind::LightFringe => "Light fringe on the edge",
            Kind::DarkFringe => "Dark fringe on the edge",
        }
    }

    pub fn explain(self) -> &'static str {
        match self {
            Kind::Stray => "A tiny speck apart from the rest of the part. It would drift on its own when the part moves.",
            Kind::Hole => "A small gap inside the paint. It can show the background through the part.",
            Kind::Ghost => "Almost see-through pixels. They make the part's edge bigger than it looks and can show as a faint smudge.",
            Kind::LightFringe => "The edge pixels are lighter than the paint next to them (often from a white background). It shows as a halo when the part is on a dark background.",
            Kind::DarkFringe => "The edge pixels are darker than the paint next to them. It shows as an outline you did not draw.",
        }
    }
}

/// How hard it looks. 0 = only the obvious, 1 = also small details (freckles, specks).
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Sens {
    /// Alpha up to this counts as "nearly invisible".
    pub ghost: u8,
    /// Separate pieces up to this many pixels are stray.
    pub speck: usize,
    /// Holes up to this many pixels are holes to fill.
    pub hole: usize,
    /// How much lighter or darker (0..255 brightness) an edge pixel has to be to count as a fringe.
    pub fringe: f32,
}

impl Sens {
    pub fn from_slider(s: f32) -> Sens {
        let s = s.clamp(0.0, 1.0);
        Sens { ghost: 2 + (s * 14.0) as u8, speck: 4 + (s * 116.0) as usize, hole: 4 + (s * 56.0) as usize, fringe: 95.0 - s * 45.0 }
    }
}

#[derive(Clone, Debug)]
pub struct Problem {
    pub layer: LayerId,
    pub kind: Kind,
    /// The pixels (page coordinates).
    pub px: Vec<(i32, i32)>,
    /// A box around where to look (x0, y0, x1, y1).
    pub focus: (i32, i32, i32, i32),
}

impl Problem {
    /// Same place, same kind: used to remember what you chose to ignore.
    pub fn key(&self) -> (LayerId, Kind, i32, i32) {
        (self.layer, self.kind, self.focus.0, self.focus.1)
    }
}

pub fn px_get(l: &Layer, x: i32, y: i32) -> Color32 {
    let t = TILE as i32;
    l.tiles.get(&(x.div_euclid(t), y.div_euclid(t))).map(|tile| tile[(y.rem_euclid(t) * t + x.rem_euclid(t)) as usize]).unwrap_or(Color32::TRANSPARENT)
}

fn px_set(l: &mut Layer, x: i32, y: i32, c: Color32) {
    let t = TILE as i32;
    let key = (x.div_euclid(t), y.div_euclid(t));
    if c == Color32::TRANSPARENT && !l.tiles.contains_key(&key) {
        return;
    }
    let tile = l.tiles.entry(key).or_insert_with(|| Arc::new(vec![Color32::TRANSPARENT; TILE * TILE]));
    Arc::make_mut(tile)[(y.rem_euclid(t) * t + x.rem_euclid(t)) as usize] = c;
}

/// A layer's alpha and color over its bounding box (with a margin of 2).
struct Plane {
    x0: i32,
    y0: i32,
    w: usize,
    h: usize,
    a: Vec<u8>,
    /// not premultiplied
    rgb: Vec<[u8; 3]>,
}

impl Plane {
    fn of(l: &Layer, doc_w: u32, doc_h: u32) -> Option<Plane> {
        let (bx0, by0, bx1, by1) = layer_bounds(l, doc_w, doc_h)?;
        let (x0, y0) = (bx0 - 2, by0 - 2);
        let (w, h) = ((bx1 - bx0 + 4) as usize, (by1 - by0 + 4) as usize);
        let mut a = vec![0u8; w * h];
        let mut rgb = vec![[0u8; 3]; w * h];
        for y in 0..h {
            for x in 0..w {
                let c = px_get(l, x0 + x as i32, y0 + y as i32);
                if c.a() != 0 {
                    a[y * w + x] = c.a();
                    let u = c.to_srgba_unmultiplied();
                    rgb[y * w + x] = [u[0], u[1], u[2]];
                }
            }
        }
        Some(Plane { x0, y0, w, h, a, rgb })
    }

    fn at(&self, x: usize, y: usize) -> usize {
        y * self.w + x
    }

    /// Pieces of pixels where `mask` is true (4-connected): (pixel indices).
    fn pieces(&self, mask: &[bool]) -> Vec<Vec<usize>> {
        let mut seen = vec![false; self.w * self.h];
        let mut out = Vec::new();
        for start in 0..self.w * self.h {
            if !mask[start] || seen[start] {
                continue;
            }
            let mut piece = vec![start];
            seen[start] = true;
            let mut i = 0;
            while i < piece.len() {
                let p = piece[i];
                i += 1;
                let (x, y) = (p % self.w, p / self.w);
                let mut push = |q: usize| {
                    if mask[q] && !seen[q] {
                        seen[q] = true;
                        piece.push(q);
                    }
                };
                if x > 0 {
                    push(p - 1);
                }
                if x + 1 < self.w {
                    push(p + 1);
                }
                if y > 0 {
                    push(p - self.w);
                }
                if y + 1 < self.h {
                    push(p + self.w);
                }
            }
            out.push(piece);
        }
        out
    }

    fn page(&self, p: usize) -> (i32, i32) {
        (self.x0 + (p % self.w) as i32, self.y0 + (p / self.w) as i32)
    }
}

fn focus_of(px: &[(i32, i32)]) -> (i32, i32, i32, i32) {
    let (mut x0, mut y0, mut x1, mut y1) = (i32::MAX, i32::MAX, i32::MIN, i32::MIN);
    for (x, y) in px {
        x0 = x0.min(*x);
        y0 = y0.min(*y);
        x1 = x1.max(*x + 1);
        y1 = y1.max(*y + 1);
    }
    (x0, y0, x1, y1)
}

/// Of a big crowd of pixels, the 64 x 64 square holding the most of them (where to look first).
fn densest(px: &[(i32, i32)]) -> Vec<(i32, i32)> {
    use std::collections::HashMap;
    let mut cells: HashMap<(i32, i32), Vec<(i32, i32)>> = HashMap::new();
    for p in px {
        cells.entry((p.0.div_euclid(64), p.1.div_euclid(64))).or_default().push(*p);
    }
    cells.into_values().max_by_key(|v| v.len()).unwrap_or_default()
}

/// Look at one layer. Specks and holes are found one by one; ghost pixels and fringes once per layer.
pub fn detect_layer(l: &Layer, doc_w: u32, doc_h: u32, s: Sens) -> Vec<Problem> {
    let Some(pl) = Plane::of(l, doc_w, doc_h) else { return Vec::new() };
    let mut out = Vec::new();
    let n = pl.w * pl.h;
    let fg: Vec<bool> = pl.a.iter().map(|a| *a > s.ghost).collect();

    // stray pieces: small ones that are not the biggest piece
    let pieces = pl.pieces(&fg);
    if pieces.len() > 1 {
        let biggest = pieces.iter().enumerate().max_by_key(|(_, p)| p.len()).map(|(i, _)| i).unwrap_or(0);
        for (i, p) in pieces.iter().enumerate() {
            if i != biggest && p.len() <= s.speck {
                let px: Vec<(i32, i32)> = p.iter().map(|q| pl.page(*q)).collect();
                out.push(Problem { layer: l.id, kind: Kind::Stray, focus: focus_of(&px), px });
            }
        }
    }

    // holes: see-through pieces that don't reach the outside
    let bg: Vec<bool> = fg.iter().map(|f| !*f).collect();
    for p in pl.pieces(&bg) {
        let touches_edge = p.iter().any(|q| {
            let (x, y) = (q % pl.w, q / pl.w);
            x == 0 || y == 0 || x + 1 == pl.w || y + 1 == pl.h
        });
        if !touches_edge && p.len() <= s.hole {
            // only a hole when it is really inside paint: not a gap between two separate pieces
            let px: Vec<(i32, i32)> = p.iter().map(|q| pl.page(*q)).collect();
            out.push(Problem { layer: l.id, kind: Kind::Hole, focus: focus_of(&px), px });
        }
    }

    // nearly invisible pixels
    let ghosts: Vec<(i32, i32)> = (0..n).filter(|p| pl.a[*p] > 0 && pl.a[*p] <= s.ghost).map(|p| pl.page(p)).collect();
    if ghosts.len() >= 12 {
        let focus = focus_of(&densest(&ghosts));
        out.push(Problem { layer: l.id, kind: Kind::Ghost, px: ghosts, focus });
    }

    // fringes: partly see-through edge pixels whose color is far from the solid paint next to them
    let (mut light, mut dark) = (Vec::new(), Vec::new());
    for y in 1..pl.h - 1 {
        for x in 1..pl.w - 1 {
            let i = pl.at(x, y);
            let a = pl.a[i];
            if a <= s.ghost || a >= 250 {
                continue;
            }
            let (mut sum, mut cnt) = ([0f32; 3], 0f32);
            for (dx, dy) in [(-1i32, -1i32), (0, -1), (1, -1), (-1, 0), (1, 0), (-1, 1), (0, 1), (1, 1)] {
                let j = pl.at((x as i32 + dx) as usize, (y as i32 + dy) as usize);
                if pl.a[j] >= 250 {
                    for k in 0..3 {
                        sum[k] += pl.rgb[j][k] as f32;
                    }
                    cnt += 1.0;
                }
            }
            if cnt == 0.0 {
                continue;
            }
            let lum = |c: [f32; 3]| 0.299 * c[0] + 0.587 * c[1] + 0.114 * c[2];
            let mine = lum([pl.rgb[i][0] as f32, pl.rgb[i][1] as f32, pl.rgb[i][2] as f32]);
            let theirs = lum([sum[0] / cnt, sum[1] / cnt, sum[2] / cnt]);
            if mine - theirs > s.fringe {
                light.push(pl.page(i));
            } else if theirs - mine > s.fringe {
                dark.push(pl.page(i));
            }
        }
    }
    for (px, kind) in [(light, Kind::LightFringe), (dark, Kind::DarkFringe)] {
        if px.len() >= 8 {
            let focus = focus_of(&densest(&px));
            out.push(Problem { layer: l.id, kind, px, focus });
        }
    }
    out
}

/// Look at every given layer (in parallel).
pub fn detect(layers: &[Layer], ids: &[LayerId], doc_w: u32, doc_h: u32, s: Sens) -> Vec<Problem> {
    let mut v: Vec<Problem> = layers.par_iter().filter(|l| ids.contains(&l.id)).flat_map(|l| detect_layer(l, doc_w, doc_h, s)).collect();
    // in the order of the layer list, then the kind
    v.sort_by_key(|p| (layers.iter().position(|l| l.id == p.layer).unwrap_or(usize::MAX), p.kind as u8, p.focus.1, p.focus.0));
    v
}

/// Fix one problem on its layer. Returns how many pixels were changed.
pub fn apply(l: &mut Layer, p: &Problem) -> usize {
    let mut n = 0;
    match p.kind {
        Kind::Stray | Kind::Ghost => {
            for (x, y) in &p.px {
                if px_get(l, *x, *y).a() != 0 {
                    px_set(l, *x, *y, Color32::TRANSPARENT);
                    n += 1;
                }
            }
        }
        Kind::Hole => {
            // the average color of the paint around the hole, fully solid
            let inside: std::collections::HashSet<(i32, i32)> = p.px.iter().copied().collect();
            let (mut sum, mut cnt) = ([0f32; 3], 0f32);
            for (x, y) in &p.px {
                for (dx, dy) in [(-1, 0), (1, 0), (0, -1), (0, 1)] {
                    let q = (x + dx, y + dy);
                    if !inside.contains(&q) {
                        let c = px_get(l, q.0, q.1);
                        if c.a() > 200 {
                            let u = c.to_srgba_unmultiplied();
                            for k in 0..3 {
                                sum[k] += u[k] as f32;
                            }
                            cnt += 1.0;
                        }
                    }
                }
            }
            if cnt > 0.0 {
                let c = Color32::from_rgb((sum[0] / cnt) as u8, (sum[1] / cnt) as u8, (sum[2] / cnt) as u8);
                for (x, y) in &p.px {
                    px_set(l, *x, *y, c);
                    n += 1;
                }
            }
        }
        Kind::LightFringe | Kind::DarkFringe => {
            for (x, y) in &p.px {
                let me = px_get(l, *x, *y);
                if me.a() == 0 {
                    continue;
                }
                let (mut sum, mut cnt) = ([0f32; 3], 0f32);
                for dy in -1..=1 {
                    for dx in -1..=1 {
                        let c = px_get(l, x + dx, y + dy);
                        if c.a() >= 250 {
                            let u = c.to_srgba_unmultiplied();
                            for k in 0..3 {
                                sum[k] += u[k] as f32;
                            }
                            cnt += 1.0;
                        }
                    }
                }
                if cnt > 0.0 {
                    let a = me.a();
                    px_set(l, *x, *y, Color32::from_rgba_unmultiplied((sum[0] / cnt) as u8, (sum[1] / cnt) as u8, (sum[2] / cnt) as u8, a));
                    n += 1;
                }
            }
        }
    }
    if n > 0 {
        l.rev += 1;
    }
    n
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::doc::{Document, LayerKind};

    fn put(l: &mut Layer, x: i32, y: i32, c: Color32) {
        px_set(l, x, y, c);
    }

    /// A red disc (radius 40) at (100,100) with: a stray 2x2 speck, a bigger 10x10 blob, a 2x2 hole,
    /// ghost pixels, and a white fringe on the edge.
    fn sample() -> (Document, LayerId) {
        let mut d = Document::new(400, 300);
        let mut l = d.new_layer("Eye", LayerKind::Raster);
        let red = Color32::from_rgb(200, 40, 40);
        for y in 50..150 {
            for x in 50..150 {
                let (dx, dy) = ((x - 100) as f32, (y - 100) as f32);
                let r = (dx * dx + dy * dy).sqrt();
                if r <= 40.0 {
                    put(&mut l, x, y, red);
                } else if r <= 41.5 {
                    // a white fringe: half see-through white pixels
                    put(&mut l, x, y, Color32::from_rgba_unmultiplied(255, 255, 255, 120));
                }
            }
        }
        // a 2x2 hole in the middle
        for (x, y) in [(100, 100), (101, 100), (100, 101), (101, 101)] {
            put(&mut l, x, y, Color32::TRANSPARENT);
        }
        // a stray speck and a bigger blob apart from the disc
        for (x, y) in [(300, 50), (301, 50), (300, 51), (301, 51)] {
            put(&mut l, x, y, red);
        }
        for y in 200..210 {
            for x in 300..310 {
                put(&mut l, x, y, red);
            }
        }
        // ghost pixels: alpha 3, 20 of them
        for i in 0..20 {
            put(&mut l, 200 + i, 250, Color32::from_rgba_unmultiplied(10, 10, 10, 3));
        }
        let id = l.id;
        d.layers.insert(0, l);
        (d, id)
    }

    fn kinds(p: &[Problem]) -> Vec<Kind> {
        p.iter().map(|p| p.kind).collect()
    }

    #[test]
    fn finds_the_things_that_were_planted() {
        let (d, id) = sample();
        let found = detect(&d.layers, &[id], d.w, d.h, Sens::from_slider(0.4));
        let k = kinds(&found);
        assert_eq!(k.iter().filter(|x| **x == Kind::Hole).count(), 1, "{k:?}");
        assert!(k.contains(&Kind::Ghost));
        assert!(k.contains(&Kind::LightFringe), "{k:?}");
        assert!(!k.contains(&Kind::DarkFringe));
        // the 2x2 speck is stray; the 10x10 blob (100 px) is too at this sensitivity
        let strays: Vec<&Problem> = found.iter().filter(|p| p.kind == Kind::Stray).collect();
        assert!(strays.iter().any(|p| p.px.len() == 4), "{:?}", strays.iter().map(|p| p.px.len()).collect::<Vec<_>>());
        let hole = found.iter().find(|p| p.kind == Kind::Hole).unwrap();
        assert_eq!(hole.px.len(), 4);
        assert!(hole.focus.0 == 100 && hole.focus.2 == 102);
    }

    #[test]
    fn sensitivity_decides_how_small_a_detail_has_to_be() {
        let (d, id) = sample();
        let low = detect(&d.layers, &[id], d.w, d.h, Sens::from_slider(0.0));
        let high = detect(&d.layers, &[id], d.w, d.h, Sens::from_slider(1.0));
        let strays = |v: &[Problem]| v.iter().filter(|p| p.kind == Kind::Stray).count();
        assert_eq!(strays(&low), 1, "only the 2x2 speck at the lowest setting");
        assert_eq!(strays(&high), 2, "the 10x10 blob too at the highest");
        // freckles are safe at low sensitivity: a 6x6 freckle is not touched
        let mut l = d.layers[0].clone();
        for y in 20..26 {
            for x in 20..26 {
                put(&mut l, x, y, Color32::from_rgb(90, 50, 40));
            }
        }
        let freckle = detect_layer(&l, d.w, d.h, Sens::from_slider(0.0));
        assert!(freckle.iter().filter(|p| p.kind == Kind::Stray).all(|p| p.px.len() != 36));
    }

    #[test]
    fn each_fix_does_its_one_job_and_the_rest_stays() {
        let (d, id) = sample();
        let mut l = d.layers[0].clone();
        let found = detect_layer(&l, d.w, d.h, Sens::from_slider(0.4));
        let red = Color32::from_rgb(200, 40, 40);
        let find = |k: Kind| found.iter().find(|p| p.kind == k).unwrap().clone();
        // hole: filled with the red around it, solid
        assert_eq!(apply(&mut l, &find(Kind::Hole)), 4);
        let c = px_get(&l, 100, 100);
        assert_eq!(c.a(), 255);
        assert!((c.r() as i32 - red.r() as i32).abs() < 3 && (c.g() as i32 - red.g() as i32).abs() < 3);
        // ghost: gone
        assert!(px_get(&l, 205, 250).a() != 0);
        apply(&mut l, &find(Kind::Ghost));
        assert_eq!(px_get(&l, 205, 250).a(), 0);
        // stray 2x2: removed, the blob remains
        let speck = found.iter().find(|p| p.kind == Kind::Stray && p.px.len() == 4).unwrap();
        apply(&mut l, speck);
        assert_eq!(px_get(&l, 300, 50).a(), 0);
        assert_eq!(px_get(&l, 305, 205).a(), 255, "a bigger piece is not touched unless it was found");
        // fringe: same see-through amount, but the color is the red of the paint now
        let edge = (100 + 41, 100);
        let before = px_get(&l, edge.0, edge.1);
        assert!(before.a() > 0 && before.a() < 255);
        apply(&mut l, &find(Kind::LightFringe));
        let after = px_get(&l, edge.0, edge.1);
        assert_eq!(after.a(), before.a());
        let u = after.to_srgba_unmultiplied();
        assert!(u[0] > 150 && u[1] < 90, "red now: {u:?}");
        // the paint itself did not move
        assert_eq!(px_get(&l, 70, 100), red);
        // looking again finds none of the fixed things
        let again = detect_layer(&l, d.w, d.h, Sens::from_slider(0.4));
        assert!(!kinds(&again).contains(&Kind::Hole) && !kinds(&again).contains(&Kind::Ghost) && !kinds(&again).contains(&Kind::LightFringe), "{:?}", kinds(&again));
    }

    #[test]
    fn a_clean_layer_has_nothing_to_report_and_looking_changes_nothing() {
        let mut d = Document::new(200, 200);
        let mut l = d.new_layer("Clean", LayerKind::Raster);
        for y in 20..120 {
            for x in 20..120 {
                put(&mut l, x, y, Color32::from_rgb(30, 120, 200));
            }
        }
        let snapshot: Vec<_> = l.tiles.iter().map(|(k, t)| (*k, t.clone())).collect();
        let id = l.id;
        d.layers.insert(0, l);
        assert!(detect(&d.layers, &[id], d.w, d.h, Sens::from_slider(1.0)).is_empty());
        for (k, t) in snapshot {
            assert!(Arc::ptr_eq(&t, &d.layers[0].tiles[&k]), "looking never touches the pixels");
        }
        // an empty layer is fine too
        let e = d.new_layer("Empty", LayerKind::Raster);
        assert!(detect_layer(&e, d.w, d.h, Sens::from_slider(0.5)).is_empty());
    }

    #[test]
    fn a_gap_between_two_pieces_is_not_a_hole() {
        let mut d = Document::new(200, 100);
        let mut l = d.new_layer("Two", LayerKind::Raster);
        for y in 10..60 {
            for x in 10..40 {
                put(&mut l, x, y, Color32::from_rgb(200, 40, 40));
            }
            for x in 44..80 {
                put(&mut l, x, y, Color32::from_rgb(200, 40, 40));
            }
        }
        // the gap (x 40..44) is open at the top and bottom
        let found = detect_layer(&l, d.w, d.h, Sens::from_slider(1.0));
        assert!(!kinds(&found).contains(&Kind::Hole), "{:?}", kinds(&found));
    }
}
