// SPDX-License-Identifier: GPL-3.0-or-later
// Keeping the picture sharp when a mesh bends. Two things lose quality: a warp that bends in straight-edged cells (a
// visible kink at every grid line), and a mesh with big triangles, whose inside is only moved in straight lines between
// its corners. A smooth warp is a curve through the control points. Refining adds triangles exactly where the
// bending is too much for the triangles that are there.

use std::collections::HashMap;

use super::{ArtMesh, Id, Kind, Pose, Rig};

/// Catmull-Rom between p1 and p2 (p0 and p3 are the points around).
fn cr(p0: [f32; 2], p1: [f32; 2], p2: [f32; 2], p3: [f32; 2], t: f32) -> [f32; 2] {
    let (t2, t3) = (t * t, t * t * t);
    let mut o = [0.0; 2];
    for k in 0..2 {
        o[k] = 0.5 * (2.0 * p1[k] + (-p0[k] + p2[k]) * t + (2.0 * p0[k] - 5.0 * p1[k] + 4.0 * p2[k] - p3[k]) * t2 + (-p0[k] + 3.0 * p1[k] - 3.0 * p2[k] + p3[k]) * t3);
    }
    o
}

fn lerp2(a: [f32; 2], b: [f32; 2], t: f32) -> [f32; 2] {
    [a[0] + (b[0] - a[0]) * t, a[1] + (b[1] - a[1]) * t]
}

/// How far a point at grid position (`u` across, `v` down, in cells) is shifted by a warp, as a smooth curve through
/// the control point shifts. At a control point it is exactly that point's shift; outside the grid the curve goes on
/// in a straight line, so a plain slant stays a plain slant.
pub fn smooth_shift(off: &[[f32; 2]], cols: u32, rows: u32, u: f32, v: f32) -> [f32; 2] {
    let (cols, rows) = (cols as i32, rows as i32);
    let node = |c: i32, r: i32| off[(r * (cols + 1) + c) as usize];
    // a point of the row `r` (0..=rows) at the column position u, along x
    let row_at = |r: i32, i: i32, fu: f32| -> [f32; 2] {
        let at = |c: i32| -> [f32; 2] {
            if c < 0 {
                let (a, b) = (node(0, r), node(1, r));
                [2.0 * a[0] - b[0], 2.0 * a[1] - b[1]]
            } else if c > cols {
                let (a, b) = (node(cols, r), node(cols - 1, r));
                [2.0 * a[0] - b[0], 2.0 * a[1] - b[1]]
            } else {
                node(c, r)
            }
        };
        cr(at(i - 1), at(i), at(i + 1), at(i + 2), fu)
    };
    let i = (u.floor() as i32).clamp(0, cols - 1);
    let j = (v.floor() as i32).clamp(0, rows - 1);
    let (fu, fv) = (u - i as f32, v - j as f32);
    let row = |r: i32| -> [f32; 2] {
        if r < 0 {
            let (a, b) = (row_at(0, i, fu), row_at(1, i, fu));
            [2.0 * a[0] - b[0], 2.0 * a[1] - b[1]]
        } else if r > rows {
            let (a, b) = (row_at(rows, i, fu), row_at(rows - 1, i, fu));
            [2.0 * a[0] - b[0], 2.0 * a[1] - b[1]]
        } else {
            row_at(r, i, fu)
        }
    };
    cr(row(j - 1), row(j), row(j + 1), row(j + 2), fv)
}

/// Drag the picture itself: the control points near `at` follow `delta`, the nearer the more (a soft grab, so the
/// bend stays smooth). `off` are the control point shifts being edited.
pub fn pull_warp(rect: [f32; 4], cols: u32, rows: u32, off: &mut [[f32; 2]], at: [f32; 2], delta: [f32; 2]) {
    let rest = Rig::warp_rest(rect, cols, rows);
    let reach = ((rect[2] - rect[0]) / cols as f32).max((rect[3] - rect[1]) / rows as f32).max(1.0) * 1.3;
    for (o, r) in off.iter_mut().zip(&rest) {
        let d = ((r[0] + o[0] - at[0]).hypot(r[1] + o[1] - at[1])) / reach;
        let w = (-d * d).exp();
        o[0] += delta[0] * w;
        o[1] += delta[1] * w;
    }
}

/// Cut the triangles at the new points on their edges (`cut`: edge to the new point), so neighbours stay joined.
pub fn split_tris(tris: &[[u32; 3]], cut: &HashMap<(u32, u32), u32>) -> Vec<[u32; 3]> {
        let mut out: Vec<[u32; 3]> = Vec::with_capacity(tris.len() * 2);
        for t in tris {
            let m = |a: u32, b: u32| cut.get(&(a.min(b), a.max(b))).copied();
            let mids = [m(t[0], t[1]), m(t[1], t[2]), m(t[2], t[0])];
            let count = mids.iter().flatten().count();
            // turn the triangle so the pattern is the standard one
            let rot = match count {
                1 => mids.iter().position(|x| x.is_some()).unwrap(),
                2 => mids.iter().position(|x| x.is_none()).map(|u| (u + 1) % 3).unwrap(),
                _ => 0,
            };
            let (v0, v1, v2) = (t[rot], t[(rot + 1) % 3], t[(rot + 2) % 3]);
            let (m0, m1, m2) = (mids[rot], mids[(rot + 1) % 3], mids[(rot + 2) % 3]);
            match count {
                0 => out.push(*t),
                1 => {
                    let m0 = m0.unwrap();
                    out.push([v0, m0, v2]);
                    out.push([m0, v1, v2]);
                }
                2 => {
                    let (m0, m1) = (m0.unwrap(), m1.unwrap());
                    out.push([m0, v1, m1]);
                    out.push([v0, m0, m1]);
                    out.push([v0, m1, v2]);
                }
                _ => {
                    let (m0, m1, m2) = (m0.unwrap(), m1.unwrap(), m2.unwrap());
                    out.push([v0, m0, m2]);
                    out.push([m0, v1, m1]);
                    out.push([m2, m1, v2]);
                    out.push([m0, m1, m2]);
                }
            }
        }
    out
}

/// More triangles everywhere: every edge longer than `max_edge` is cut in two, again and again.
pub fn subdivide(verts: &[[f32; 2]], tris: &[[u32; 3]], max_edge: f32, max_verts: usize) -> (Vec<[f32; 2]>, Vec<[u32; 3]>) {
    subdivide_by(verts, tris, &|_| max_edge, max_verts)
}

/// The same, but the longest edge allowed depends on where the edge is (its middle): finer where it matters, such as a face.
pub fn subdivide_by(verts: &[[f32; 2]], tris: &[[u32; 3]], max_edge: &dyn Fn([f32; 2]) -> f32, max_verts: usize) -> (Vec<[f32; 2]>, Vec<[u32; 3]>) {
    let (mut v, mut t) = (verts.to_vec(), tris.to_vec());
    for _ in 0..10 {
        let mut cut: HashMap<(u32, u32), u32> = HashMap::new();
        let mut order: Vec<(u32, u32)> = Vec::new();
        for tri in &t {
            for k in 0..3 {
                let (a, b) = (tri[k], tri[(k + 1) % 3]);
                let key = (a.min(b), a.max(b));
                if cut.contains_key(&key) {
                    continue;
                }
                let (pa, pb) = (v[a as usize], v[b as usize]);
                if (pa[0] - pb[0]).hypot(pa[1] - pb[1]) > max_edge([(pa[0] + pb[0]) / 2.0, (pa[1] + pb[1]) / 2.0]) && v.len() + order.len() < max_verts {
                    cut.insert(key, (v.len() + order.len()) as u32);
                    order.push(key);
                }
            }
        }
        if order.is_empty() {
            break;
        }
        for (a, b) in &order {
            let (pa, pb) = (v[*a as usize], v[*b as usize]);
            v.push([(pa[0] + pb[0]) / 2.0, (pa[1] + pb[1]) / 2.0]);
        }
        t = split_tris(&t, &cut);
    }
    (v, t)
}

pub struct Refined {
    pub rest: Vec<[f32; 2]>,
    pub now: Vec<[f32; 2]>,
    pub tris: Vec<[u32; 3]>,
}

/// Does anything that moves this mesh bend it (a warp, a line or a soft push)? A turn keeps straight lines straight.
pub fn bends(rig: &Rig, mesh: &ArtMesh) -> bool {
    let mut cur = mesh.parent;
    let mut depth = 0;
    while let Some(id) = cur {
        let Some(c) = rig.changer(id) else { return false };
        if !matches!(c.kind, Kind::Rotation { .. }) {
            return true;
        }
        cur = c.parent;
        depth += 1;
        if depth > 16 {
            return false;
        }
    }
    false
}

const MAX_PASSES: usize = 6;
const MAX_VERTS: usize = 150_000;

/// The mesh with extra triangles where it bends: an edge is cut in two when the true position of its middle is more
/// than `tol` page pixels away from the middle of its moved ends, and is still longer than `min_edge`.
/// `now0` are the moved corners as they are (seams pulled together); new points are moved exactly.
pub fn refine(rig: &Rig, mesh: &ArtMesh, now0: &[[f32; 2]], pose: &Pose, tol: f32, min_edge: f32) -> Refined {
    let mut rest = mesh.verts.clone();
    let mut now = now0.to_vec();
    let mut tris = mesh.tris.clone();
    if !bends(rig, mesh) || rest.len() != now.len() {
        return Refined { rest, now, tris };
    }
    let dist = |a: [f32; 2], b: [f32; 2]| (a[0] - b[0]).hypot(a[1] - b[1]);
    for _ in 0..MAX_PASSES {
        let mut cut: HashMap<(u32, u32), u32> = HashMap::new();
        let mut seen: HashMap<(u32, u32), bool> = HashMap::new();
        for t in &tris {
            for k in 0..3 {
                let (a, b) = (t[k], t[(k + 1) % 3]);
                let key = (a.min(b), a.max(b));
                if seen.contains_key(&key) {
                    continue;
                }
                let (ra, rb) = (rest[a as usize], rest[b as usize]);
                let mut go = false;
                if dist(ra, rb) > min_edge {
                    let mid = [(ra[0] + rb[0]) / 2.0, (ra[1] + rb[1]) / 2.0];
                    let truth = rig.deform_point(pose, mesh.parent, mid);
                    let lin = [(now[a as usize][0] + now[b as usize][0]) / 2.0, (now[a as usize][1] + now[b as usize][1]) / 2.0];
                    go = dist(truth, lin) > tol;
                }
                seen.insert(key, go);
            }
        }
        for (key, go) in &seen {
            if !*go || rest.len() + cut.len() >= MAX_VERTS {
                continue;
            }
            cut.insert(*key, (rest.len() + cut.len()) as u32);
        }
        if cut.is_empty() {
            break;
        }
        // the new points, in the order of their numbers
        let mut news: Vec<((u32, u32), u32)> = cut.iter().map(|(k, v)| (*k, *v)).collect();
        news.sort_by_key(|n| n.1);
        for (key, _) in &news {
            let (ra, rb) = (rest[key.0 as usize], rest[key.1 as usize]);
            let mid = [(ra[0] + rb[0]) / 2.0, (ra[1] + rb[1]) / 2.0];
            rest.push(mid);
            now.push(rig.deform_point(pose, mesh.parent, mid));
        }
        tris = split_tris(&tris, &cut);
    }
    Refined { rest, now, tris }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rig::{Form, Kind as K, Values};

    /// Two big triangles over 0..200, moved by a 4x4 warp that is pushed hard in the middle.
    fn bent() -> (Rig, ArtMesh, Pose) {
        let mut r = Rig::default();
        let w = r.add_changer("Bend", K::Warp { rect: [0.0, 0.0, 200.0, 200.0], cols: 4, rows: 4 }, None);
        let p = r.add_param("Bend", 0.0, 1.0, 0.0);
        let mut off = vec![[0.0, 0.0]; 25];
        off[12] = [0.0, 70.0];
        off[11] = [0.0, 20.0];
        off[13] = [0.0, 20.0];
        r.set_keyform(w, p, 1.0, Form::Warp { offsets: off });
        let id = r.fresh_id();
        let m = ArtMesh { id, layer: 1, verts: vec![[0.0, 0.0], [200.0, 0.0], [200.0, 200.0], [0.0, 200.0]], tris: vec![[0, 1, 2], [0, 2, 3]], parent: Some(w) };
        let mut v = Values::default();
        v.set(p, 1.0);
        let pose = r.pose(&v);
        r.meshes.push(m.clone());
        (r, m, pose)
    }

    /// The worst distance between the picture of the mesh and the true bend, over a grid of points.
    fn worst(rig: &Rig, mesh: &ArtMesh, rf: &Refined, pose: &Pose) -> f32 {
        let mut worst = 0.0f32;
        for yi in 0..=40 {
            for xi in 0..=40 {
                let p = [xi as f32 * 5.0, yi as f32 * 5.0];
                let truth = rig.deform_point(pose, mesh.parent, p);
                for t in &rf.tris {
                    let (a, b, c) = (rf.rest[t[0] as usize], rf.rest[t[1] as usize], rf.rest[t[2] as usize]);
                    let det = (b[1] - c[1]) * (a[0] - c[0]) + (c[0] - b[0]) * (a[1] - c[1]);
                    if det.abs() < 1e-9 {
                        continue;
                    }
                    let l1 = ((b[1] - c[1]) * (p[0] - c[0]) + (c[0] - b[0]) * (p[1] - c[1])) / det;
                    let l2 = ((c[1] - a[1]) * (p[0] - c[0]) + (a[0] - c[0]) * (p[1] - c[1])) / det;
                    let l3 = 1.0 - l1 - l2;
                    if l1 < -1e-4 || l2 < -1e-4 || l3 < -1e-4 {
                        continue;
                    }
                    let (na, nb, nc) = (rf.now[t[0] as usize], rf.now[t[1] as usize], rf.now[t[2] as usize]);
                    let got = [na[0] * l1 + nb[0] * l2 + nc[0] * l3, na[1] * l1 + nb[1] * l2 + nc[1] * l3];
                    worst = worst.max((got[0] - truth[0]).hypot(got[1] - truth[1]));
                    break;
                }
            }
        }
        worst
    }

    fn area(rest: &[[f32; 2]], tris: &[[u32; 3]]) -> f32 {
        tris.iter().map(|t| {
            let (a, b, c) = (rest[t[0] as usize], rest[t[1] as usize], rest[t[2] as usize]);
            ((b[0] - a[0]) * (c[1] - a[1]) - (c[0] - a[0]) * (b[1] - a[1])).abs() / 2.0
        }).sum()
    }

    #[test]
    fn big_triangles_lose_the_bend_and_refining_gets_it_back() {
        let (r, m, pose) = bent();
        let now = r.deformed(&m, &pose);
        let plain = Refined { rest: m.verts.clone(), now: now.clone(), tris: m.tris.clone() };
        let before = worst(&r, &m, &plain, &pose);
        assert!(before > 15.0, "two triangles cannot show the bend: {before}");
        let rf = refine(&r, &m, &now, &pose, 0.4, 4.0);
        let after = worst(&r, &m, &rf, &pose);
        assert!(after < 3.5, "refined: {after}");
        assert!(rf.tris.len() > 20 && rf.tris.len() < 20_000, "{} triangles", rf.tris.len());
        // the cut keeps the whole picture covered, once: no holes and no overlaps
        assert!((area(&rf.rest, &rf.tris) - 40_000.0).abs() < 1.0, "{}", area(&rf.rest, &rf.tris));
        // a smooth warp has no kinks, so the same mesh follows it almost exactly
        let mut sm = r.clone();
        sm.smooth.push(m.parent.unwrap());
        let (now_s, pose_s) = (sm.deformed(&m, &pose), sm.pose(&{
            let mut v = Values::default();
            v.set(sm.params[0].id, 1.0);
            v
        }));
        let rs = refine(&sm, &m, &now_s, &pose_s, 0.4, 4.0);
        assert!(worst(&sm, &m, &rs, &pose_s) < 1.5, "smooth refined: {}", worst(&sm, &m, &rs, &pose_s));
        // a stricter tolerance stays close too, with more triangles
        let fine = refine(&r, &m, &now, &pose, 0.1, 2.0);
        assert!(worst(&r, &m, &fine, &pose) < 3.5);
        assert!(fine.tris.len() >= rf.tris.len());
    }

    #[test]
    fn pulling_the_picture_moves_what_is_under_the_pointer_most() {
        let rect = [0.0, 0.0, 400.0, 400.0];
        let mut off = vec![[0.0, 0.0]; 25];
        pull_warp(rect, 4, 4, &mut off, [200.0, 200.0], [0.0, 40.0]);
        assert!((off[12][1] - 40.0).abs() < 0.01, "the point under the pointer follows it fully");
        assert!(off[11][1] > 5.0 && off[11][1] < 40.0, "its neighbours follow less: {}", off[11][1]);
        assert!(off[0][1].abs() < 0.5 && off[24][1].abs() < 0.5, "the corners stay");
        assert!(off.iter().all(|o| o[0] == 0.0), "only the way it was pulled");
    }

    #[test]
    fn subdividing_keeps_the_shape_and_makes_small_triangles() {
        let verts = [[0.0, 0.0], [200.0, 0.0], [200.0, 100.0], [0.0, 100.0]];
        let tris = [[0, 1, 2], [0, 2, 3]];
        let (v, t) = subdivide(&verts, &tris, 30.0, 10_000);
        assert!(v.len() > 40 && t.len() > 60, "{} {}", v.len(), t.len());
        assert!((area(&v, &t) - 20_000.0).abs() < 1.0, "the same area, covered once: {}", area(&v, &t));
        let longest = t.iter().flat_map(|t| (0..3).map(move |k| (t[k], t[(k + 1) % 3]))).map(|(a, b)| (v[a as usize][0] - v[b as usize][0]).hypot(v[a as usize][1] - v[b as usize][1])).fold(0.0f32, f32::max);
        assert!(longest <= 30.0 * 1.001 + 1.0, "{longest}");
        // a limit on the number of points is kept
        let (v2, _) = subdivide(&verts, &tris, 1.0, 500);
        assert!(v2.len() <= 500 + 4);
    }

    #[test]
    fn a_turn_needs_no_extra_triangles_and_a_still_mesh_is_left_alone() {
        let mut r = Rig::default();
        let rot = r.add_changer("Turn", K::Rotation { pivot: [100.0, 100.0] }, None);
        let p = r.add_param("Turn", -30.0, 30.0, 0.0);
        r.bind_rotation(rot, p, 30.0);
        let id = r.fresh_id();
        let m = ArtMesh { id, layer: 1, verts: vec![[0.0, 0.0], [200.0, 0.0], [200.0, 200.0]], tris: vec![[0, 1, 2]], parent: Some(rot) };
        let mut v = Values::default();
        v.set(p, 30.0);
        let pose = r.pose(&v);
        let now = r.deformed(&m, &pose);
        assert!(!bends(&r, &m));
        let rf = refine(&r, &m, &now, &pose, 0.1, 1.0);
        assert_eq!((rf.rest.len(), rf.tris.len()), (3, 1));
        // at rest, even a warped mesh has nothing to refine
        let (r2, m2, _) = bent();
        let rest_pose = r2.pose(&Values::default());
        let now2 = r2.deformed(&m2, &rest_pose);
        let rf2 = refine(&r2, &m2, &now2, &rest_pose, 0.4, 4.0);
        assert_eq!(rf2.tris.len(), 2);
    }

    #[test]
    fn a_smooth_warp_goes_through_its_points_and_has_no_kinks() {
        // one point pushed up in a 4x4 grid
        let mut off = vec![[0.0, 0.0]; 25];
        off[12] = [0.0, 60.0];
        for (i, o) in off.iter().enumerate() {
            let (c, r) = ((i % 5) as f32, (i / 5) as f32);
            assert_eq!(smooth_shift(&off, 4, 4, c, r), *o, "node {i}");
        }
        // along the middle row: the slope is the same on both sides of the node (no kink); in straight cells it is not
        let h = 0.01;
        let y = |u: f32| smooth_shift(&off, 4, 4, u, 2.0)[1];
        let (left, right) = ((y(2.0) - y(2.0 - h)) / h, (y(2.0 + h) - y(2.0)) / h);
        assert!((left - right).abs() < 6.0, "smooth: {left} vs {right}");
        let lin = |u: f32| {
            let i = u.floor().min(3.0) as usize;
            let f = u - i as f32;
            off[2 * 5 + i][1] * (1.0 - f) + off[2 * 5 + i + 1][1] * f
        };
        let (l2, r2) = ((lin(2.0) - lin(2.0 - h)) / h, (lin(2.0 + h) - lin(2.0)) / h);
        assert!((l2 - r2).abs() > 30.0, "straight cells kink: {l2} vs {r2}");
        // a plain slant stays a plain slant, also at the edge of the grid
        let slant: Vec<[f32; 2]> = (0..25).map(|i| [(i % 5) as f32 * 10.0, 0.0]).collect();
        for u in [0.0, 0.3, 1.7, 3.9, 4.0] {
            assert!((smooth_shift(&slant, 4, 4, u, 1.3)[0] - u * 10.0).abs() < 1e-3, "u {u}");
        }
    }

    #[test]
    fn smooth_warps_are_used_by_the_engine_saved_and_cleaned() {
        let (mut r, m, _) = bent();
        let w = m.parent.unwrap();
        let p = r.params[0].id;
        let mut v = Values::default();
        v.set(p, 1.0);
        let straight = r.deform_point(&r.pose(&v), Some(w), [75.0, 100.0]);
        r.smooth.push(w);
        let curved = r.deform_point(&r.pose(&v), Some(w), [75.0, 100.0]);
        assert!((straight[1] - curved[1]).abs() > 0.5, "{straight:?} {curved:?}");
        // at a control point both agree
        let node = r.deform_point(&r.pose(&v), Some(w), [100.0, 100.0]);
        assert!((node[1] - 170.0).abs() < 0.01);
        let back: Rig = serde_json::from_str(&serde_json::to_string(&r).unwrap()).unwrap();
        assert_eq!(back.smooth, vec![w]);
        let mut bad = r.clone();
        bad.smooth.extend([w, 9999]);
        assert_eq!(bad.sanitized().smooth, vec![w]);
    }
}
