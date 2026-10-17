// SPDX-License-Identifier: GPL-3.0-or-later
// "Hold to straighten" (port of the first app's shapes.ts): draw a shape by hand, keep the pen
// still at the end, and it becomes a clean line, circle, ellipse, triangle, rectangle or polygon.

use crate::vector::VPt;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ShapeKind {
    Line,
    Circle,
    Ellipse,
    Triangle,
    Rectangle,
    Polygon,
}

type P2 = (f32, f32);

fn rdp(pts: &[VPt], eps: f32) -> Vec<VPt> {
    if pts.len() <= 2 {
        return pts.to_vec();
    }
    let mut keep = vec![false; pts.len()];
    keep[0] = true;
    keep[pts.len() - 1] = true;
    let mut stack = vec![(0usize, pts.len() - 1)];
    while let Some((a, b)) = stack.pop() {
        let (pa, pb) = (pts[a], pts[b]);
        let (dx, dy) = (pb[0] - pa[0], pb[1] - pa[1]);
        let len = dx.hypot(dy);
        let (mut worst, mut at) = (-1.0f32, 0usize);
        for i in a + 1..b {
            let q = pts[i];
            let d = if len < 1e-6 { (q[0] - pa[0]).hypot(q[1] - pa[1]) } else { ((q[0] - pa[0]) * dy - (q[1] - pa[1]) * dx).abs() / len };
            if d > worst {
                worst = d;
                at = i;
            }
        }
        if worst > eps && at > 0 {
            keep[at] = true;
            stack.push((a, at));
            stack.push((at, b));
        }
    }
    pts.iter().zip(keep).filter(|(_, k)| *k).map(|(p, _)| *p).collect()
}

/// Points every few pixels along straight edges; corners are doubled so curves through them stay sharp.
fn edges(corners: &[P2], p: f32, closed: bool) -> Vec<VPt> {
    let mut list: Vec<P2> = corners.to_vec();
    if closed {
        list.push(corners[0]);
    }
    let mut out = Vec::new();
    for w in list.windows(2) {
        let (a, b) = (w[0], w[1]);
        let n = ((b.0 - a.0).hypot(b.1 - a.1) / 4.0).ceil().max(1.0) as usize;
        out.push([a.0, a.1, p]);
        for j in 0..n {
            let t = j as f32 / n as f32;
            out.push([a.0 + (b.0 - a.0) * t, a.1 + (b.1 - a.1) * t, p]);
        }
    }
    let last = *list.last().unwrap();
    out.push([last.0, last.1, p]);
    out.push([last.0, last.1, p]);
    out
}

fn ellipse_pts(cx: f32, cy: f32, rx: f32, ry: f32, p: f32, start: f32) -> Vec<VPt> {
    let n = ((rx + ry) * 0.6).round().clamp(48.0, 240.0) as usize;
    (0..=n)
        .map(|i| {
            let a = start + i as f32 / n as f32 * std::f32::consts::TAU;
            [cx + a.cos() * rx, cy + a.sin() * ry, p]
        })
        .collect()
}

fn seg_dist(q: VPt, a: P2, b: P2) -> f32 {
    let (dx, dy) = (b.0 - a.0, b.1 - a.1);
    let l2 = (dx * dx + dy * dy).max(1e-9);
    let t = (((q[0] - a.0) * dx + (q[1] - a.1) * dy) / l2).clamp(0.0, 1.0);
    (q[0] - (a.0 + dx * t)).hypot(q[1] - (a.1 + dy * t))
}

/// What the drawn points look like. The clean shape, or None when it doesn't look like any shape.
pub fn recognize(raw: &[VPt]) -> Option<(ShapeKind, Vec<VPt>)> {
    let mut pts: Vec<VPt> = Vec::with_capacity(raw.len());
    for (i, q) in raw.iter().enumerate() {
        if i == 0 || (q[0] - raw[i - 1][0]).hypot(q[1] - raw[i - 1][1]) > 0.5 {
            pts.push(*q);
        }
    }
    if pts.len() < 3 {
        return None;
    }
    let len: f32 = pts.windows(2).map(|w| (w[1][0] - w[0][0]).hypot(w[1][1] - w[0][1])).sum();
    if len < 12.0 {
        return None;
    }
    let p = pts.iter().map(|q| q[2]).sum::<f32>() / pts.len() as f32; // even pressure for clean shapes
    let (mut min_x, mut max_x, mut min_y, mut max_y) = (f32::MAX, f32::MIN, f32::MAX, f32::MIN);
    for q in &pts {
        min_x = min_x.min(q[0]);
        max_x = max_x.max(q[0]);
        min_y = min_y.min(q[1]);
        max_y = max_y.max(q[1]);
    }
    let size = (max_x - min_x).max(max_y - min_y);
    let (first, last) = (pts[0], *pts.last().unwrap());
    let gap = (last[0] - first[0]).hypot(last[1] - first[1]);

    // open: a straight line?
    if gap > (size * 0.25).max(len * 0.12) {
        let (dx, dy) = (last[0] - first[0], last[1] - first[1]);
        let chord = dx.hypot(dy);
        let worst = pts.iter().map(|q| ((q[0] - first[0]) * dy - (q[1] - first[1]) * dx).abs() / chord).fold(0.0f32, f32::max);
        return (worst <= chord * 0.06 + 2.0).then(|| (ShapeKind::Line, edges(&[(first[0], first[1]), (last[0], last[1])], p, false)));
    }

    // closed shapes
    let mut closed = pts.clone();
    closed.push(first);
    let perim: f32 = closed.windows(2).map(|w| (w[1][0] - w[0][0]).hypot(w[1][1] - w[0][1])).sum();
    let mut corners = rdp(&closed, perim * 0.045);
    // the start point is usually not a real corner: drop it if it lies on a straight edge
    if corners.len() > 3 {
        let (a, b, c) = (corners[corners.len() - 2], corners[0], corners[1]);
        let ab = (b[0] - a[0]).hypot(b[1] - a[1]);
        let cl = (c[0] - a[0]).hypot(c[1] - a[1]);
        let dev = if cl > 0.0 { ((b[0] - a[0]) * (c[1] - a[1]) - (b[1] - a[1]) * (c[0] - a[0])).abs() / cl } else { ab };
        if dev < perim * 0.03 {
            let second = corners[1];
            corners = corners[1..corners.len() - 1].to_vec();
            corners.push(second);
        }
    }
    let n = corners.len() - 1; // the last one repeats the first
    let c2: Vec<P2> = corners.iter().map(|c| (c[0], c[1])).collect();

    let poly_err = |cs: &[P2]| {
        let sum: f32 = pts.iter().map(|q| (0..cs.len()).map(|i| seg_dist(*q, cs[i], cs[(i + 1) % cs.len()])).fold(f32::MAX, f32::min)).sum();
        sum / pts.len() as f32 / size
    };
    let (ecx, ecy) = ((min_x + max_x) / 2.0, (min_y + max_y) / 2.0);
    let (rx, ry) = (((max_x - min_x) / 2.0).max(1.0), ((max_y - min_y) / 2.0).max(1.0));
    let e_sum: f32 = pts.iter().map(|q| (((q[0] - ecx) / rx).hypot((q[1] - ecy) / ry) - 1.0).abs() * rx.min(ry)).sum();
    let ell_err = e_sum / pts.len() as f32 / size;
    let p_err = if (3..=8).contains(&n) { poly_err(&c2[..n]) } else { f32::INFINITY };

    if ell_err.min(p_err) > 0.05 {
        return None;
    }
    if ell_err <= p_err {
        let start = (first[1] - ecy).atan2(first[0] - ecx);
        return Some(if (rx - ry).abs() / rx.max(ry) < 0.12 {
            let r = (rx + ry) / 2.0;
            (ShapeKind::Circle, ellipse_pts(ecx, ecy, r, r, p, start))
        } else {
            (ShapeKind::Ellipse, ellipse_pts(ecx, ecy, rx, ry, p, start))
        });
    }
    if n == 3 {
        return Some((ShapeKind::Triangle, edges(&c2[..3], p, true)));
    }
    if n == 4 {
        // nearly right angles: a true rectangle, kept at the angle it was drawn
        let c4 = &c2[..4];
        let (mx, my) = (c4.iter().map(|c| c.0).sum::<f32>() / 4.0, c4.iter().map(|c| c.1).sum::<f32>() / 4.0);
        let ang = (c4[1].1 - c4[0].1).atan2(c4[1].0 - c4[0].0);
        let step = std::f32::consts::FRAC_PI_2;
        let snapped = (ang / step).round() * step;
        let rot = if (ang - snapped).abs() < 0.12 { snapped } else { ang };
        let (ux, uy) = (rot.cos(), rot.sin());
        let proj: Vec<(f32, f32)> = c4.iter().map(|q| ((q.0 - mx) * ux + (q.1 - my) * uy, -(q.0 - mx) * uy + (q.1 - my) * ux)).collect();
        let u0 = proj.iter().map(|q| q.0).fold(f32::MAX, f32::min);
        let u1 = proj.iter().map(|q| q.0).fold(f32::MIN, f32::max);
        let v0 = proj.iter().map(|q| q.1).fold(f32::MAX, f32::min);
        let v1 = proj.iter().map(|q| q.1).fold(f32::MIN, f32::max);
        let to_xy = |u: f32, v: f32| (mx + u * ux - v * uy, my + u * uy + v * ux);
        return Some((ShapeKind::Rectangle, edges(&[to_xy(u0, v0), to_xy(u1, v0), to_xy(u1, v1), to_xy(u0, v1)], p, true)));
    }
    Some((ShapeKind::Polygon, edges(&c2[..n], p, true)))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn jitter(i: usize) -> f32 {
        ((i * 2654435761) % 1000) as f32 / 1000.0 - 0.5
    }

    fn pts(list: Vec<(f32, f32)>) -> Vec<VPt> {
        list.iter().enumerate().map(|(i, p)| [p.0 + jitter(i) * 2.0, p.1 + jitter(i + 77) * 2.0, 0.6]).collect()
    }

    fn polyline(corners: &[(f32, f32)]) -> Vec<(f32, f32)> {
        let mut out = Vec::new();
        for w in corners.windows(2) {
            for j in 0..30 {
                let t = j as f32 / 30.0;
                out.push((w[0].0 + (w[1].0 - w[0].0) * t, w[0].1 + (w[1].1 - w[0].1) * t));
            }
        }
        out.push(*corners.last().unwrap());
        out
    }

    #[test]
    fn recognizes_the_basic_shapes() {
        let circle = pts((0..=60).map(|i| (200.0 + 80.0 * (i as f32 / 60.0 * 6.2).cos(), 200.0 + 80.0 * (i as f32 / 60.0 * 6.2).sin())).collect());
        assert_eq!(recognize(&circle).unwrap().0, ShapeKind::Circle);
        let ellipse = pts((0..=60).map(|i| (200.0 + 120.0 * (i as f32 / 60.0 * 6.3).cos(), 200.0 + 50.0 * (i as f32 / 60.0 * 6.3).sin())).collect());
        assert_eq!(recognize(&ellipse).unwrap().0, ShapeKind::Ellipse);
        let line = pts(polyline(&[(20.0, 20.0), (300.0, 120.0)]));
        let (k, l) = recognize(&line).unwrap();
        assert_eq!(k, ShapeKind::Line);
        assert!((l[0][0] - 20.0).abs() < 3.0 && (l.last().unwrap()[0] - 300.0).abs() < 3.0);
        let rect = pts(polyline(&[(50.0, 50.0), (250.0, 50.0), (250.0, 150.0), (50.0, 150.0), (50.0, 50.0)]));
        let (k, r) = recognize(&rect).unwrap();
        assert_eq!(k, ShapeKind::Rectangle);
        assert!(r.iter().all(|p| (p[0] - 50.0).abs() < 4.0 || (p[0] - 250.0).abs() < 4.0 || (p[1] - 50.0).abs() < 4.0 || (p[1] - 150.0).abs() < 4.0), "clean edges");
        let tri = pts(polyline(&[(50.0, 150.0), (150.0, 30.0), (250.0, 150.0), (50.0, 150.0)]));
        assert_eq!(recognize(&tri).unwrap().0, ShapeKind::Triangle);
    }

    #[test]
    fn a_squiggle_is_left_alone() {
        let squiggle = pts((0..80).map(|i| (i as f32 * 4.0, 100.0 + (i as f32 * 0.5).sin() * 40.0)).collect());
        assert!(recognize(&squiggle).is_none());
        assert!(recognize(&pts(vec![(0.0, 0.0), (1.0, 1.0)])).is_none(), "too short");
    }
}
