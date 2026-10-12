// Round-dab painting into layer tiles: anti-aliased edge, hardness falloff, even dab spacing along
// the stroke, eraser and lock-alpha. The full brush engine (pressure, textures, tips) builds on this.

use eframe::egui::{Color32, Pos2};

use crate::doc::{Document, TILE};

#[derive(Clone, Copy, Debug)]
pub struct DabParams {
    pub radius: f32,   // document pixels
    pub hardness: f32, // 0..1
    pub alpha: f32,    // 0..1 per dab
    pub color: Color32, // opaque color (alpha ignored)
    pub erase: bool,
    pub spacing: f32, // fraction of the diameter between dabs
}

#[derive(Default)]
pub struct StrokeState {
    last: Option<Pos2>,
    residue: f32, // distance already travelled toward the next dab
}

impl StrokeState {
    pub fn end(&mut self) {
        self.last = None;
        self.residue = 0.0;
    }
    pub fn active(&self) -> bool {
        self.last.is_some()
    }
}

/// Continue the stroke on layer `li` to document point `p`.
pub fn stroke_to(doc: &mut Document, li: usize, st: &mut StrokeState, p: Pos2, prm: &DabParams) {
    let Some(last) = st.last else {
        dab(doc, li, p, prm);
        st.last = Some(p);
        st.residue = 0.0;
        return;
    };
    let step = (prm.radius * 2.0 * prm.spacing).max(0.5);
    let dir = p - last;
    let len = dir.length();
    if len <= 0.0 {
        return;
    }
    let unit = dir / len;
    let mut t = step - st.residue;
    while t <= len {
        dab(doc, li, last + unit * t, prm);
        t += step;
    }
    st.residue = len - (t - step);
    st.last = Some(p);
}

/// One dab centered at `c` (document pixels).
pub fn dab(doc: &mut Document, li: usize, c: Pos2, prm: &DabParams) {
    let (w, h) = (doc.w as i32, doc.h as i32);
    let r = prm.radius.max(0.5);
    let x0 = ((c.x - r - 1.0).floor() as i32).max(0);
    let y0 = ((c.y - r - 1.0).floor() as i32).max(0);
    let x1 = ((c.x + r + 1.0).ceil() as i32).min(w);
    let y1 = ((c.y + r + 1.0).ceil() as i32).min(h);
    if x1 <= x0 || y1 <= y0 {
        return;
    }
    let lock_alpha = doc.layers[li].lock_alpha;
    let hard = prm.hardness.clamp(0.0, 0.999);
    let (cr, cg, cb) = (prm.color.r() as f32, prm.color.g() as f32, prm.color.b() as f32);
    let t = TILE as i32;

    let layer = &mut doc.layers[li];
    for ty in y0.div_euclid(t)..=(y1 - 1).div_euclid(t) {
        for tx in x0.div_euclid(t)..=(x1 - 1).div_euclid(t) {
            // Erasing or lock-alpha never needs to create an empty tile.
            if (prm.erase || lock_alpha) && !layer.tiles.contains_key(&(tx, ty)) {
                continue;
            }
            let tile = layer.tile_mut((tx, ty));
            let (bx, by) = (tx * t, ty * t);
            for y in y0.max(by)..y1.min(by + t) {
                let fy = y as f32 + 0.5 - c.y;
                for x in x0.max(bx)..x1.min(bx + t) {
                    let fx = x as f32 + 0.5 - c.x;
                    let dist = (fx * fx + fy * fy).sqrt();
                    if dist >= r + 0.5 {
                        continue;
                    }
                    let edge = (r - dist + 0.5).clamp(0.0, 1.0); // anti-aliased rim
                    let d = dist / r;
                    let soft = if d <= hard {
                        1.0
                    } else {
                        let k = ((d - hard) / (1.0 - hard)).clamp(0.0, 1.0);
                        1.0 - k * k * (3.0 - 2.0 * k)
                    };
                    let a = edge.min(soft) * prm.alpha;
                    if a <= 0.0 {
                        continue;
                    }
                    let i = ((y - by) * t + (x - bx)) as usize;
                    let px = tile[i];
                    let (pr, pg, pb, pa) = (px.r() as f32, px.g() as f32, px.b() as f32, px.a() as f32);
                    let q = |v: f32| v.round().clamp(0.0, 255.0) as u8;
                    tile[i] = if prm.erase {
                        let k = 1.0 - a;
                        Color32::from_rgba_premultiplied(q(pr * k), q(pg * k), q(pb * k), q(pa * k))
                    } else if lock_alpha {
                        // Recolor what is there, keep its transparency.
                        let da = pa / 255.0;
                        Color32::from_rgba_premultiplied(
                            q(pr + (cr * da - pr) * a),
                            q(pg + (cg * da - pg) * a),
                            q(pb + (cb * da - pb) * a),
                            px.a(),
                        )
                    } else {
                        let k = 1.0 - a;
                        Color32::from_rgba_premultiplied(
                            q(cr * a + pr * k),
                            q(cg * a + pg * k),
                            q(cb * a + pb * k),
                            q(255.0 * a + pa * k),
                        )
                    };
                }
            }
        }
    }
    doc.mark_dirty_rect(x0, y0, x1, y1);
}

#[cfg(test)]
mod tests {
    use super::*;
    use eframe::egui::pos2;

    fn prm() -> DabParams {
        DabParams { radius: 10.0, hardness: 1.0, alpha: 1.0, color: Color32::RED, erase: false, spacing: 0.1 }
    }

    #[test]
    fn dab_paints_center_and_leaves_outside() {
        let mut d = Document::new(512, 512);
        dab(&mut d, 0, pos2(100.0, 100.0), &prm());
        let t = &d.layers[0].tiles[&(0, 0)];
        assert_eq!(t[100 * TILE + 100], Color32::RED);
        assert_eq!(t[100 * TILE + 130], Color32::TRANSPARENT);
    }

    #[test]
    fn stroke_across_tile_border_and_erase() {
        let mut d = Document::new(600, 300);
        let mut st = StrokeState::default();
        stroke_to(&mut d, 0, &mut st, pos2(200.0, 50.0), &prm());
        stroke_to(&mut d, 0, &mut st, pos2(320.0, 50.0), &prm());
        assert_eq!(d.layers[0].tiles[&(1, 0)][50 * TILE + (300 - 256)], Color32::RED);
        let mut e = prm();
        e.erase = true;
        dab(&mut d, 0, pos2(300.0, 50.0), &e);
        assert_eq!(d.layers[0].tiles[&(1, 0)][50 * TILE + (300 - 256)].a(), 0);
    }
}
