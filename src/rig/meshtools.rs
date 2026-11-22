// SPDX-License-Identifier: GPL-3.0-or-later
// Tools for the points of a 2D mesh, one point at a time: pick points (with a box), weld two points into one, weld the points that are too close, purge points
// no triangle uses, line points up, relax clumped points, turn the edge between two triangles, put a point in, take points out, and cut a cloud of points into
// triangles again. They work on the rest mesh of a part (the picture is mapped by where the points were drawn, so moving a point never smears the paint).

use super::ArtMesh;

/// Which way points are lined up.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Align {
    /// All on one horizontal line (the middle of their heights).
    Horizontal,
    /// All on one vertical line.
    Vertical,
    /// All on the straight line between the two ends (the two that are farthest apart), in the order they are in along it.
    Line,
}

fn dist(a: [f32; 2], b: [f32; 2]) -> f32 {
    (a[0] - b[0]).hypot(a[1] - b[1])
}

/// The points inside a box (any two corners).
pub fn select_in_rect(m: &ArtMesh, a: [f32; 2], b: [f32; 2]) -> Vec<usize> {
    let (x0, x1, y0, y1) = (a[0].min(b[0]), a[0].max(b[0]), a[1].min(b[1]), a[1].max(b[1]));
    (0..m.verts.len()).filter(|i| (x0..=x1).contains(&m.verts[*i][0]) && (y0..=y1).contains(&m.verts[*i][1])).collect()
}

/// The point nearest to `p` within `reach`.
pub fn nearest_vertex(m: &ArtMesh, p: [f32; 2], reach: f32) -> Option<usize> {
    (0..m.verts.len()).map(|i| (i, dist(m.verts[i], p))).filter(|(_, d)| *d <= reach).min_by(|a, b| a.1.total_cmp(&b.1)).map(|(i, _)| i)
}

/// Take the points that are not kept out of the mesh, and renumber the triangles; triangles that used a removed point go. Returns the new number of every old point.
fn compact(m: &mut ArtMesh, keep: &[bool]) -> Vec<Option<u32>> {
    let mut map = vec![None; m.verts.len()];
    let mut verts = Vec::with_capacity(m.verts.len());
    for (i, v) in m.verts.iter().enumerate() {
        if keep.get(i).copied().unwrap_or(false) {
            map[i] = Some(verts.len() as u32);
            verts.push(*v);
        }
    }
    m.tris = m.tris.iter().filter_map(|t| Some([map[t[0] as usize]?, map[t[1] as usize]?, map[t[2] as usize]?])).collect();
    m.verts = verts;
    map
}

/// Point `a` is dragged onto point `b` and they are one point; triangles that become flat go. Returns whether anything was done.
pub fn weld(m: &mut ArtMesh, a: usize, b: usize) -> bool {
    if a == b || a >= m.verts.len() || b >= m.verts.len() {
        return false;
    }
    for t in &mut m.tris {
        for k in t.iter_mut() {
            if *k as usize == a {
                *k = b as u32;
            }
        }
    }
    m.tris.retain(|t| t[0] != t[1] && t[1] != t[2] && t[0] != t[2]);
    let mut keep = vec![true; m.verts.len()];
    keep[a] = false;
    compact(m, &keep);
    true
}

/// The picked points become one point, in the middle of them. Returns how many points went.
pub fn weld_selected(m: &mut ArtMesh, sel: &[usize]) -> usize {
    let sel: Vec<usize> = { let mut s: Vec<usize> = sel.iter().copied().filter(|i| *i < m.verts.len()).collect(); s.sort_unstable(); s.dedup(); s };
    if sel.len() < 2 {
        return 0;
    }
    let n = sel.len() as f32;
    let mid = [sel.iter().map(|i| m.verts[*i][0]).sum::<f32>() / n, sel.iter().map(|i| m.verts[*i][1]).sum::<f32>() / n];
    let target = sel[0];
    m.verts[target] = mid;
    // from the last to the second, so the numbers of the ones not yet done stay what they were
    for a in sel.iter().skip(1).rev() {
        for t in &mut m.tris {
            for k in t.iter_mut() {
                if *k as usize == *a {
                    *k = target as u32;
                }
            }
        }
    }
    m.tris.retain(|t| t[0] != t[1] && t[1] != t[2] && t[0] != t[2]);
    let mut keep = vec![true; m.verts.len()];
    for a in sel.iter().skip(1) {
        keep[*a] = false;
    }
    compact(m, &keep);
    sel.len() - 1
}

/// The vertex welder: points closer than `reach` to each other are made one (in the middle of the group), which closes cracks. Returns how many points went.
pub fn weld_near(m: &mut ArtMesh, reach: f32) -> usize {
    let n = m.verts.len();
    if n < 2 || n > 6000 {
        return 0;
    }
    let mut group: Vec<usize> = (0..n).collect();
    fn root(g: &mut [usize], i: usize) -> usize {
        let mut r = i;
        while g[r] != r {
            r = g[r];
        }
        let mut c = i;
        while g[c] != r {
            let next = g[c];
            g[c] = r;
            c = next;
        }
        r
    }
    for i in 0..n {
        for j in i + 1..n {
            if dist(m.verts[i], m.verts[j]) < reach {
                let (a, b) = (root(&mut group, i), root(&mut group, j));
                if a != b {
                    group[a.max(b)] = a.min(b);
                }
            }
        }
    }
    let mut members: std::collections::BTreeMap<usize, Vec<usize>> = std::collections::BTreeMap::new();
    for i in 0..n {
        let r = root(&mut group, i);
        members.entry(r).or_default().push(i);
    }
    let mut gone = 0;
    let mut keep = vec![true; n];
    let mut to = (0..n as u32).collect::<Vec<u32>>();
    for (r, ms) in &members {
        if ms.len() < 2 {
            continue;
        }
        let c = ms.len() as f32;
        m.verts[*r] = [ms.iter().map(|i| m.verts[*i][0]).sum::<f32>() / c, ms.iter().map(|i| m.verts[*i][1]).sum::<f32>() / c];
        for i in ms.iter().filter(|i| **i != *r) {
            keep[*i] = false;
            to[*i] = *r as u32;
            gone += 1;
        }
    }
    for t in &mut m.tris {
        for k in t.iter_mut() {
            *k = to[*k as usize];
        }
    }
    m.tris.retain(|t| t[0] != t[1] && t[1] != t[2] && t[0] != t[2]);
    compact(m, &keep);
    gone
}

/// The vertex leak finder: points that no triangle uses are taken out. Returns how many.
pub fn remove_orphans(m: &mut ArtMesh) -> usize {
    let mut used = vec![false; m.verts.len()];
    for t in &m.tris {
        for k in t {
            used[*k as usize] = true;
        }
    }
    let gone = used.iter().filter(|u| !**u).count();
    if gone > 0 {
        compact(m, &used);
    }
    gone
}

/// Take the picked points out, with the triangles that used them (and the points that are left alone because of it).
pub fn delete_points(m: &mut ArtMesh, sel: &[usize]) -> usize {
    let mut keep = vec![true; m.verts.len()];
    for i in sel {
        if let Some(k) = keep.get_mut(*i) {
            *k = false;
        }
    }
    let before = m.verts.len();
    compact(m, &keep);
    remove_orphans(m);
    before - m.verts.len()
}

/// Line the picked points up.
pub fn align(m: &mut ArtMesh, sel: &[usize], how: Align) {
    let sel: Vec<usize> = sel.iter().copied().filter(|i| *i < m.verts.len()).collect();
    if sel.len() < 2 {
        return;
    }
    let n = sel.len() as f32;
    match how {
        Align::Horizontal => {
            let y = sel.iter().map(|i| m.verts[*i][1]).sum::<f32>() / n;
            sel.iter().for_each(|i| m.verts[*i][1] = y);
        }
        Align::Vertical => {
            let x = sel.iter().map(|i| m.verts[*i][0]).sum::<f32>() / n;
            sel.iter().for_each(|i| m.verts[*i][0] = x);
        }
        Align::Line => {
            // the two farthest apart are the ends; every other point goes to the nearest place on the line between them
            let (mut a, mut b, mut best) = (sel[0], sel[1], -1.0f32);
            for (k, i) in sel.iter().enumerate() {
                for j in &sel[k + 1..] {
                    let d = dist(m.verts[*i], m.verts[*j]);
                    if d > best {
                        (a, b, best) = (*i, *j, d);
                    }
                }
            }
            let (p, q) = (m.verts[a], m.verts[b]);
            let (dx, dy) = (q[0] - p[0], q[1] - p[1]);
            let len2 = (dx * dx + dy * dy).max(1e-9);
            for i in &sel {
                let v = m.verts[*i];
                let t = ((v[0] - p[0]) * dx + (v[1] - p[1]) * dy) / len2;
                m.verts[*i] = [p[0] + dx * t, p[1] + dy * t];
            }
        }
    }
}

/// The points on the edge of the mesh (an edge of one triangle only).
pub fn border_points(m: &ArtMesh) -> Vec<bool> {
    let mut count: std::collections::HashMap<(u32, u32), u32> = std::collections::HashMap::new();
    for t in &m.tris {
        for (a, b) in [(t[0], t[1]), (t[1], t[2]), (t[2], t[0])] {
            *count.entry((a.min(b), a.max(b))).or_insert(0) += 1;
        }
    }
    let mut border = vec![false; m.verts.len()];
    for ((a, b), c) in count {
        if c == 1 {
            border[a as usize] = true;
            border[b as usize] = true;
        }
    }
    border
}

/// Relax: the points go toward the middle of the points they are joined to, so clumps spread out and the spacing evens out. The points on the edge of the mesh
/// stay where they are, so the outline of the art does not move. `only`: just these points (None: all inside). `strength` is 0 to 1 a round.
pub fn relax(m: &mut ArtMesh, only: Option<&[usize]>, rounds: usize, strength: f32) {
    let border = border_points(m);
    let mut next: Vec<std::collections::BTreeSet<usize>> = vec![Default::default(); m.verts.len()];
    for t in &m.tris {
        for (a, b) in [(t[0], t[1]), (t[1], t[2]), (t[2], t[0])] {
            next[a as usize].insert(b as usize);
            next[b as usize].insert(a as usize);
        }
    }
    let strength = strength.clamp(0.0, 1.0);
    for _ in 0..rounds {
        let old = m.verts.clone();
        for i in 0..old.len() {
            if border[i] || next[i].is_empty() || only.is_some_and(|o| !o.contains(&i)) {
                continue;
            }
            let c = next[i].len() as f32;
            let mid = [next[i].iter().map(|j| old[*j][0]).sum::<f32>() / c, next[i].iter().map(|j| old[*j][1]).sum::<f32>() / c];
            m.verts[i] = [old[i][0] + (mid[0] - old[i][0]) * strength, old[i][1] + (mid[1] - old[i][1]) * strength];
        }
    }
}

fn area(a: [f32; 2], b: [f32; 2], c: [f32; 2]) -> f32 {
    (b[0] - a[0]) * (c[1] - a[1]) - (b[1] - a[1]) * (c[0] - a[0])
}

/// Turn the edge between points `a` and `b`: the two triangles on it are joined the other way across their four corners. Refused when the four corners are
/// not a convex shape (the new edge would leave it) or the edge is on the border.
pub fn flip_edge(m: &mut ArtMesh, a: usize, b: usize) -> bool {
    let (a32, b32) = (a as u32, b as u32);
    let sharing: Vec<usize> = (0..m.tris.len()).filter(|i| m.tris[*i].contains(&a32) && m.tris[*i].contains(&b32)).collect();
    if sharing.len() != 2 {
        return false;
    }
    let other = |t: [u32; 3]| t.into_iter().find(|k| *k != a32 && *k != b32).unwrap_or(a32);
    let (c, d) = (other(m.tris[sharing[0]]) as usize, other(m.tris[sharing[1]]) as usize);
    if c == d {
        return false;
    }
    let (pa, pb, pc, pd) = (m.verts[a], m.verts[b], m.verts[c], m.verts[d]);
    // convex: c and d on different sides of ab, and a and b on different sides of cd
    if area(pa, pb, pc) * area(pa, pb, pd) >= 0.0 || area(pc, pd, pa) * area(pc, pd, pb) >= 0.0 {
        return false;
    }
    let t0 = m.tris[sharing[0]];
    let sign = area(m.verts[t0[0] as usize], m.verts[t0[1] as usize], m.verts[t0[2] as usize]).signum();
    let mut t1 = [a as u32, c as u32, d as u32];
    let mut t2 = [b as u32, d as u32, c as u32];
    for t in [&mut t1, &mut t2] {
        if area(m.verts[t[0] as usize], m.verts[t[1] as usize], m.verts[t[2] as usize]).signum() != sign {
            t.swap(1, 2);
        }
    }
    m.tris[sharing[0]] = t1;
    m.tris[sharing[1]] = t2;
    true
}

/// A point is put in at `p`: the triangle it is in is cut into three. Returns its number, or None when it is outside every triangle.
pub fn add_point(m: &mut ArtMesh, p: [f32; 2]) -> Option<usize> {
    let hit = m.tris.iter().position(|t| {
        let (a, b, c) = (m.verts[t[0] as usize], m.verts[t[1] as usize], m.verts[t[2] as usize]);
        let (d1, d2, d3) = (area(a, b, p), area(b, c, p), area(c, a, p));
        (d1 >= 0.0 && d2 >= 0.0 && d3 >= 0.0) || (d1 <= 0.0 && d2 <= 0.0 && d3 <= 0.0)
    })?;
    let t = m.tris[hit];
    let i = m.verts.len() as u32;
    m.verts.push(p);
    m.tris[hit] = [t[0], t[1], i];
    m.tris.push([t[1], t[2], i]);
    m.tris.push([t[2], t[0], i]);
    Some(i as usize)
}

/// The edge loop generator: every edge between two picked points gets a new point in its middle, and the triangles on it are cut so everything stays joined.
/// Pick a chain of points (round an eyelid, along a lip) and a loop of new points comes in along it, to be dragged into the shape. Returns the numbers of the
/// new points.
pub fn split_edges(m: &mut ArtMesh, sel: &[usize]) -> Vec<usize> {
    let mut cut: std::collections::HashMap<(u32, u32), u32> = std::collections::HashMap::new();
    let mut made = Vec::new();
    for t in m.tris.clone() {
        for (a, b) in [(t[0], t[1]), (t[1], t[2]), (t[2], t[0])] {
            let key = (a.min(b), a.max(b));
            if cut.contains_key(&key) || !sel.contains(&(a as usize)) || !sel.contains(&(b as usize)) {
                continue;
            }
            let (pa, pb) = (m.verts[a as usize], m.verts[b as usize]);
            m.verts.push([(pa[0] + pb[0]) / 2.0, (pa[1] + pb[1]) / 2.0]);
            let i = m.verts.len() - 1;
            cut.insert(key, i as u32);
            made.push(i);
        }
    }
    if !made.is_empty() {
        m.tris = super::refine::split_tris(&m.tris, &cut);
    }
    made
}

/// The point on the other side of the line x = `axis_x` that is the mirror image of point `i`, if there is one within `reach`.
pub fn mirror_partner(m: &ArtMesh, i: usize, axis_x: f32, reach: f32) -> Option<usize> {
    let v = *m.verts.get(i)?;
    let want = [2.0 * axis_x - v[0], v[1]];
    (0..m.verts.len()).filter(|j| *j != i).map(|j| (j, dist(m.verts[j], want))).filter(|(_, d)| *d <= reach).min_by(|a, b| a.1.total_cmp(&b.1)).map(|(j, _)| j)
}

/// Auto-triangulate: all the points are joined into a clean net of triangles that do not overlap (Delaunay: no point inside the circle of a triangle), and
/// the triangles whose middle is outside what the mesh covered before are dropped, so the outline of the art stays. Returns false when there are too few or
/// too many points.
pub fn triangulate(m: &mut ArtMesh) -> bool {
    let n = m.verts.len();
    if !(3..=1500).contains(&n) {
        return false;
    }
    let old_verts = m.verts.clone();
    let old_tris = m.tris.clone();
    // a triangle round everything to start from, then every point is put in
    let (mut lo, mut hi) = ([f32::MAX; 2], [f32::MIN; 2]);
    for v in &old_verts {
        for k in 0..2 {
            lo[k] = lo[k].min(v[k]);
            hi[k] = hi[k].max(v[k]);
        }
    }
    let span = (hi[0] - lo[0]).max(hi[1] - lo[1]).max(1.0) * 20.0;
    let mid = [(lo[0] + hi[0]) / 2.0, (lo[1] + hi[1]) / 2.0];
    let mut pts: Vec<[f32; 2]> = old_verts.clone();
    pts.push([mid[0] - span, mid[1] - span]);
    pts.push([mid[0] + span, mid[1] - span]);
    pts.push([mid[0], mid[1] + span]);
    let mut tris: Vec<[usize; 3]> = vec![[n, n + 1, n + 2]];
    let circum = |t: &[usize; 3], p: [f32; 2], pts: &[[f32; 2]]| -> bool {
        let (a, b, c) = (pts[t[0]], pts[t[1]], pts[t[2]]);
        let (ax, ay, bx, by, cx, cy) = (a[0] as f64 - p[0] as f64, a[1] as f64 - p[1] as f64, b[0] as f64 - p[0] as f64, b[1] as f64 - p[1] as f64, c[0] as f64 - p[0] as f64, c[1] as f64 - p[1] as f64);
        let det = (ax * ax + ay * ay) * (bx * cy - cx * by) - (bx * bx + by * by) * (ax * cy - cx * ay) + (cx * cx + cy * cy) * (ax * by - bx * ay);
        let orient = (b[0] as f64 - a[0] as f64) * (c[1] as f64 - a[1] as f64) - (b[1] as f64 - a[1] as f64) * (c[0] as f64 - a[0] as f64);
        if orient > 0.0 { det > 0.0 } else { det < 0.0 }
    };
    for (i, p) in old_verts.iter().enumerate() {
        let bad: Vec<usize> = (0..tris.len()).filter(|k| circum(&tris[*k], *p, &pts)).collect();
        let mut edges: Vec<(usize, usize)> = Vec::new();
        for k in &bad {
            let t = tris[*k];
            for (a, b) in [(t[0], t[1]), (t[1], t[2]), (t[2], t[0])] {
                if let Some(pos) = edges.iter().position(|e| (e.0 == b && e.1 == a) || (e.0 == a && e.1 == b)) {
                    edges.remove(pos);
                } else {
                    edges.push((a, b));
                }
            }
        }
        let mut k = 0;
        tris.retain(|_| {
            let keep = !bad.contains(&k);
            k += 1;
            keep
        });
        for (a, b) in edges {
            tris.push([a, b, i]);
        }
    }
    let inside_old = |c: [f32; 2]| old_tris.is_empty() || old_tris.iter().any(|t| {
        let (a, b, d) = (old_verts[t[0] as usize], old_verts[t[1] as usize], old_verts[t[2] as usize]);
        let (d1, d2, d3) = (area(a, b, c), area(b, d, c), area(d, a, c));
        (d1 >= 0.0 && d2 >= 0.0 && d3 >= 0.0) || (d1 <= 0.0 && d2 <= 0.0 && d3 <= 0.0)
    });
    m.tris = tris
        .into_iter()
        .filter(|t| t.iter().all(|k| *k < n))
        .filter(|t| area(pts[t[0]], pts[t[1]], pts[t[2]]).abs() > 1e-6)
        .filter(|t| inside_old([(pts[t[0]][0] + pts[t[1]][0] + pts[t[2]][0]) / 3.0, (pts[t[0]][1] + pts[t[1]][1] + pts[t[2]][1]) / 3.0]))
        .map(|t| [t[0] as u32, t[1] as u32, t[2] as u32])
        .collect();
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A 3 by 3 grid of points (0 to 20) cut into triangles.
    fn grid() -> ArtMesh {
        let mut verts = Vec::new();
        for y in 0..3 {
            for x in 0..3 {
                verts.push([x as f32 * 10.0, y as f32 * 10.0]);
            }
        }
        let mut tris = Vec::new();
        for y in 0..2u32 {
            for x in 0..2u32 {
                let i = y * 3 + x;
                tris.push([i, i + 1, i + 4]);
                tris.push([i, i + 4, i + 3]);
            }
        }
        ArtMesh { id: 1, layer: 1, verts, tris, parent: None }
    }

    fn valid(m: &ArtMesh) -> bool {
        m.tris.iter().all(|t| t.iter().all(|k| (*k as usize) < m.verts.len()) && t[0] != t[1] && t[1] != t[2] && t[0] != t[2])
    }

    #[test]
    fn a_box_picks_points_and_the_nearest_point_is_found() {
        let m = grid();
        assert_eq!(select_in_rect(&m, [-1.0, -1.0], [11.0, 11.0]), vec![0, 1, 3, 4]);
        assert_eq!(select_in_rect(&m, [11.0, 11.0], [-1.0, -1.0]), vec![0, 1, 3, 4], "any two corners");
        assert_eq!(nearest_vertex(&m, [9.0, 11.0], 5.0), Some(4));
        assert_eq!(nearest_vertex(&m, [100.0, 100.0], 5.0), None);
    }

    #[test]
    fn welding_joins_points_and_drops_flat_triangles() {
        let mut m = grid();
        let before = m.tris.len();
        assert!(weld(&mut m, 1, 4));
        assert_eq!(m.verts.len(), 8);
        assert!(m.tris.len() < before && valid(&m));
        // the middle of picked points
        let mut m = grid();
        assert_eq!(weld_selected(&mut m, &[0, 1, 3, 4]), 3);
        assert_eq!(m.verts.len(), 6);
        assert!(m.verts.iter().any(|v| *v == [5.0, 5.0]) && valid(&m));
        // the welder closes a crack: two points 0.5 apart are one
        let mut m = grid();
        m.verts.push([0.4, 0.0]);
        m.tris.push([9, 1, 4]);
        assert_eq!(weld_near(&mut m, 1.0), 1);
        assert_eq!(m.verts.len(), 9);
        assert!(valid(&m));
    }

    #[test]
    fn points_nothing_uses_are_found_and_taken_out() {
        let mut m = grid();
        m.verts.push([99.0, 99.0]);
        assert_eq!(remove_orphans(&mut m), 1);
        assert_eq!(m.verts.len(), 9);
        let mut m = grid();
        assert_eq!(delete_points(&mut m, &[4]), 3, "the middle point goes with its triangles, and the two corners they left alone");
        assert!(valid(&m) && m.tris.len() < 8);
    }

    #[test]
    fn points_are_lined_up_three_ways() {
        let mut m = grid();
        m.verts[1] = [10.0, 3.0];
        align(&mut m, &[0, 1, 2], Align::Horizontal);
        assert!(m.verts[0][1] == m.verts[1][1] && m.verts[1][1] == m.verts[2][1]);
        let mut m = grid();
        align(&mut m, &[0, 3, 6], Align::Vertical);
        assert!(m.verts[0][0] == m.verts[3][0] && m.verts[3][0] == m.verts[6][0]);
        let mut m = grid();
        m.verts[1] = [10.0, 4.0];
        m.verts[2] = [20.0, 10.0];
        align(&mut m, &[0, 1, 2], Align::Line);
        let (a, b, c) = (m.verts[0], m.verts[1], m.verts[2]);
        assert!(area(a, b, c).abs() < 1e-3, "on one line: {a:?} {b:?} {c:?}");
    }

    #[test]
    fn relaxing_evens_out_the_inside_and_keeps_the_outline() {
        let mut m = grid();
        m.verts[4] = [17.0, 15.0];
        let outline: Vec<[f32; 2]> = (0..9).filter(|i| *i != 4).map(|i| m.verts[i]).collect();
        relax(&mut m, None, 10, 0.5);
        assert!((m.verts[4][0] - 10.0).abs() < 0.5 && (m.verts[4][1] - 10.0).abs() < 0.5, "{:?}", m.verts[4]);
        let after: Vec<[f32; 2]> = (0..9).filter(|i| *i != 4).map(|i| m.verts[i]).collect();
        assert_eq!(outline, after, "the points on the edge do not move");
        // only the picked ones
        let mut m = grid();
        m.verts[4] = [17.0, 15.0];
        relax(&mut m, Some(&[0]), 5, 1.0);
        assert_eq!(m.verts[4], [17.0, 15.0]);
    }

    #[test]
    fn an_edge_can_be_turned_a_point_put_in_and_a_mirror_partner_found() {
        let mut m = grid();
        // the edge 0-4 is the diagonal of the first square: turning it joins 1 and 3
        assert!(flip_edge(&mut m, 0, 4));
        assert!(m.tris.iter().any(|t| t.contains(&1) && t.contains(&3)));
        assert!(valid(&m));
        let areas: Vec<f32> = m.tris.iter().map(|t| area(m.verts[t[0] as usize], m.verts[t[1] as usize], m.verts[t[2] as usize])).collect();
        assert!(areas.iter().all(|a| *a > 0.0) || areas.iter().all(|a| *a < 0.0), "the triangles all face the same way: {areas:?}");
        // the border cannot be turned
        assert!(!flip_edge(&mut m, 0, 1));
        let n = m.tris.len();
        let i = add_point(&mut m, [15.0, 5.0]).expect("inside");
        assert_eq!(m.tris.len(), n + 2);
        assert_eq!(i, m.verts.len() - 1);
        assert!(add_point(&mut m, [50.0, 50.0]).is_none());
        // mirror partner across x = 10
        assert_eq!(mirror_partner(&m, 0, 10.0, 1.0), Some(2));
        assert_eq!(mirror_partner(&m, 4, 10.0, 1.0), None, "the middle has no partner but itself");
    }

    #[test]
    fn a_chain_of_points_gets_a_loop_of_new_points_between_them() {
        let mut m = grid();
        let before_area: f32 = m.tris.iter().map(|t| area(m.verts[t[0] as usize], m.verts[t[1] as usize], m.verts[t[2] as usize]).abs() / 2.0).sum();
        // the bottom row 0-1-2: the two edges between them get a point each
        let new = split_edges(&mut m, &[0, 1, 2]);
        assert_eq!(new.len(), 2);
        assert!(m.verts[new[0]] == [5.0, 0.0] && m.verts[new[1]] == [15.0, 0.0]);
        assert!(valid(&m));
        let after_area: f32 = m.tris.iter().map(|t| area(m.verts[t[0] as usize], m.verts[t[1] as usize], m.verts[t[2] as usize]).abs() / 2.0).sum();
        assert!((before_area - after_area).abs() < 1e-3, "the same surface is covered: {before_area} {after_area}");
        assert!(border_points(&m)[new[0]], "the new points are on the edge of the mesh, joined to it");
        // one picked point has no edge to split
        assert!(split_edges(&mut grid(), &[4]).is_empty());
    }

    #[test]
    fn a_cloud_of_points_is_cut_into_clean_triangles_inside_the_old_outline() {
        let mut m = grid();
        m.tris.clear();
        assert!(triangulate(&mut m));
        assert_eq!(m.tris.len(), 8, "a 3 by 3 grid is 8 triangles");
        assert!(valid(&m));
        let total: f32 = m.tris.iter().map(|t| area(m.verts[t[0] as usize], m.verts[t[1] as usize], m.verts[t[2] as usize]).abs() / 2.0).sum();
        assert!((total - 400.0).abs() < 1.0, "they cover the square: {total}");
        // an L shape stays an L: the triangles across the notch are dropped
        let mut l = grid();
        l.tris.retain(|t| !(t.contains(&5) && t.contains(&8)) && !t.contains(&8));
        let covered_before: f32 = l.tris.iter().map(|t| area(l.verts[t[0] as usize], l.verts[t[1] as usize], l.verts[t[2] as usize]).abs() / 2.0).sum();
        assert!(triangulate(&mut l));
        let covered_after: f32 = l.tris.iter().map(|t| area(l.verts[t[0] as usize], l.verts[t[1] as usize], l.verts[t[2] as usize]).abs() / 2.0).sum();
        assert!(covered_after <= covered_before + 1.0, "{covered_before} {covered_after}");
        assert!(!triangulate(&mut ArtMesh { id: 1, layer: 1, verts: vec![[0.0, 0.0]], tris: vec![], parent: None }));
    }
}
