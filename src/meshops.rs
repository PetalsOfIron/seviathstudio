// SPDX-License-Identifier: GPL-3.0-or-later
// More that is done to the geometry of a mesh: shapes made from numbers (a ball, a disc, a cylinder, a cone, a box, a plane, with as many points as asked for),
// splitting faces into smaller ones (more points where a mesh has to bend: at the joints of a body), and joining pairs of triangles into quads (for programs and
// files that work with quads). Nothing here knows about the screen.

use std::collections::HashMap;

use crate::assetview::AssetMesh;
use crate::rig3d::Rig3;

/// A shape made from numbers, as a mesh of its own (no texture: its texture coordinates are 0 to 1 over the whole of it, for the cell of its colours).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Shape {
    /// A ball (a UV sphere: rings of points round the up axis).
    Ball,
    Disc,
    Cylinder,
    Cone,
    Cube,
    Plane,
}

impl Shape {
    pub const ALL: [Shape; 6] = [Shape::Ball, Shape::Disc, Shape::Cylinder, Shape::Cone, Shape::Cube, Shape::Plane];

    pub fn label(self) -> &'static str {
        match self {
            Shape::Ball => "Ball",
            Shape::Disc => "Disc",
            Shape::Cylinder => "Cylinder",
            Shape::Cone => "Cone",
            Shape::Cube => "Box",
            Shape::Plane => "Plane",
        }
    }
}

/// The numbers of a shape.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ShapeParams {
    /// The size (the radius of a ball, a disc, a cylinder, a cone; half the side of a box or a plane), in metres.
    pub size: f32,
    /// The height of a cylinder or a cone, in metres.
    pub height: f32,
    /// How many points go round (a ball, a disc, a cylinder, a cone), or along a side (a box, a plane).
    pub segments: usize,
    /// How many rings from the top to the bottom (a ball), or along the height (a cylinder, a cone).
    pub rings: usize,
}

impl Default for ShapeParams {
    fn default() -> Self {
        ShapeParams { size: 0.05, height: 0.1, segments: 16, rings: 8 }
    }
}

/// The mesh of a shape, around the origin (the bottom of a cylinder or a cone is at y = 0, a ball and a box are round the origin).
pub fn make(shape: Shape, p: &ShapeParams) -> AssetMesh {
    let seg = p.segments.clamp(3, 128);
    let rings = p.rings.clamp(2, 128);
    let (mut verts, mut uvs, mut tris): (Vec<[f32; 3]>, Vec<[f32; 2]>, Vec<[u32; 3]>) = (Vec::new(), Vec::new(), Vec::new());
    let tau = std::f32::consts::TAU;
    match shape {
        Shape::Ball => {
            // a point at the top, rings of points, a point at the bottom
            verts.push([0.0, p.size, 0.0]);
            uvs.push([0.5, 0.0]);
            for r in 1..rings {
                let a = r as f32 / rings as f32 * std::f32::consts::PI;
                for k in 0..seg {
                    let b = k as f32 / seg as f32 * tau;
                    verts.push([a.sin() * b.cos() * p.size, a.cos() * p.size, a.sin() * b.sin() * p.size]);
                    uvs.push([k as f32 / seg as f32, r as f32 / rings as f32]);
                }
            }
            verts.push([0.0, -p.size, 0.0]);
            uvs.push([0.5, 1.0]);
            let bottom = (verts.len() - 1) as u32;
            for k in 0..seg as u32 {
                let k2 = (k + 1) % seg as u32;
                tris.push([0, 1 + k2, 1 + k]);
                let last = 1 + (rings as u32 - 2) * seg as u32;
                tris.push([bottom, last + k, last + k2]);
            }
            for r in 0..(rings as u32 - 2) {
                for k in 0..seg as u32 {
                    let k2 = (k + 1) % seg as u32;
                    let (a, b, c, d) = (1 + r * seg as u32 + k, 1 + r * seg as u32 + k2, 1 + (r + 1) * seg as u32 + k, 1 + (r + 1) * seg as u32 + k2);
                    tris.push([a, b, c]);
                    tris.push([b, d, c]);
                }
            }
        }
        Shape::Disc => {
            verts.push([0.0, 0.0, 0.0]);
            uvs.push([0.5, 0.5]);
            for k in 0..seg {
                let b = k as f32 / seg as f32 * tau;
                verts.push([b.cos() * p.size, 0.0, b.sin() * p.size]);
                uvs.push([0.5 + b.cos() * 0.5, 0.5 + b.sin() * 0.5]);
            }
            for k in 0..seg as u32 {
                tris.push([0, 1 + (k + 1) % seg as u32, 1 + k]);
                // seen from both sides
                tris.push([0, 1 + k, 1 + (k + 1) % seg as u32]);
            }
        }
        Shape::Cylinder | Shape::Cone => {
            let rr = rings.max(1);
            for r in 0..=rr {
                let t = r as f32 / rr as f32;
                let radius = if shape == Shape::Cone { p.size * (1.0 - t) } else { p.size };
                for k in 0..seg {
                    let b = k as f32 / seg as f32 * tau;
                    verts.push([b.cos() * radius, t * p.height, b.sin() * radius]);
                    uvs.push([k as f32 / seg as f32, t]);
                }
            }
            for r in 0..rr as u32 {
                for k in 0..seg as u32 {
                    let k2 = (k + 1) % seg as u32;
                    let (a, b, c, d) = (r * seg as u32 + k, r * seg as u32 + k2, (r + 1) * seg as u32 + k, (r + 1) * seg as u32 + k2);
                    tris.push([a, c, b]);
                    tris.push([b, c, d]);
                }
            }
            // the bottom (and the top of a cylinder) closed with a point in the middle
            let ring_at = |r: usize| (r * seg) as u32;
            for (r, up) in [(0usize, false), (rr, true)] {
                if shape == Shape::Cone && up {
                    continue;
                }
                let mid = verts.len() as u32;
                verts.push([0.0, if up { p.height } else { 0.0 }, 0.0]);
                uvs.push([0.5, if up { 1.0 } else { 0.0 }]);
                for k in 0..seg as u32 {
                    let k2 = (k + 1) % seg as u32;
                    let (a, b) = (ring_at(r) + k, ring_at(r) + k2);
                    tris.push(if up { [mid, a, b] } else { [mid, b, a] });
                }
            }
        }
        Shape::Plane => {
            let n = seg;
            for y in 0..=n {
                for x in 0..=n {
                    verts.push([(x as f32 / n as f32 * 2.0 - 1.0) * p.size, 0.0, (y as f32 / n as f32 * 2.0 - 1.0) * p.size]);
                    uvs.push([x as f32 / n as f32, y as f32 / n as f32]);
                }
            }
            for y in 0..n as u32 {
                for x in 0..n as u32 {
                    let a = y * (n as u32 + 1) + x;
                    let (b, c, d) = (a + 1, a + n as u32 + 1, a + n as u32 + 2);
                    tris.push([a, c, b]);
                    tris.push([b, c, d]);
                    tris.push([a, b, c]);
                    tris.push([b, d, c]);
                }
            }
        }
        Shape::Cube => {
            let n = (seg / 4).clamp(1, 32);
            // six planes, each a grid of n by n squares
            for face in 0..6 {
                let base = verts.len() as u32;
                for y in 0..=n {
                    for x in 0..=n {
                        let (u, v) = (x as f32 / n as f32 * 2.0 - 1.0, y as f32 / n as f32 * 2.0 - 1.0);
                        let q = match face {
                            0 => [1.0, v, -u],
                            1 => [-1.0, v, u],
                            2 => [u, 1.0, -v],
                            3 => [u, -1.0, v],
                            4 => [u, v, 1.0],
                            _ => [-u, v, -1.0],
                        };
                        verts.push([q[0] * p.size, q[1] * p.size, q[2] * p.size]);
                        uvs.push([x as f32 / n as f32, y as f32 / n as f32]);
                    }
                }
                for y in 0..n as u32 {
                    for x in 0..n as u32 {
                        let a = base + y * (n as u32 + 1) + x;
                        let (b, c, d) = (a + 1, a + n as u32 + 1, a + n as u32 + 2);
                        // the faces look out of the box
                        if face % 2 == 0 {
                            tris.push([a, b, c]);
                            tris.push([b, d, c]);
                        } else {
                            tris.push([a, c, b]);
                            tris.push([b, c, d]);
                        }
                    }
                }
            }
        }
    }
    AssetMesh { verts, uvs, tris, texture: None, normal: None }
}

/// The mesh with its faces cut into smaller ones: every triangle of `faces` is cut into four (a new point in the middle of every edge), and the triangles next to
/// them are cut as much as is needed so nothing is left open. Returns the mesh and, for every new point, the two old points it is in the middle of (what belongs to
/// the points, the weights, is the mean of those two).
pub fn subdivide(mesh: &AssetMesh, faces: &[bool]) -> Option<(AssetMesh, Vec<(u32, u32)>)> {
    if !faces.iter().any(|f| *f) {
        return None;
    }
    let mut out = mesh.clone();
    let mut mid: HashMap<(u32, u32), u32> = HashMap::new();
    let mut parents: Vec<(u32, u32)> = Vec::new();
    // an edge is cut when a face on one of its sides is cut (the points that are the same place count as one, so a seam does not open)
    let key = |p: [f32; 3]| ((p[0] * 20000.0).round() as i32, (p[1] * 20000.0).round() as i32, (p[2] * 20000.0).round() as i32);
    let mut cut: std::collections::HashSet<((i32, i32, i32), (i32, i32, i32))> = std::collections::HashSet::new();
    let edge_key = |a: u32, b: u32| {
        let (ka, kb) = (key(mesh.verts[a as usize]), key(mesh.verts[b as usize]));
        if ka <= kb { (ka, kb) } else { (kb, ka) }
    };
    for (t, f) in mesh.tris.iter().zip(faces) {
        if *f {
            for k in 0..3 {
                cut.insert(edge_key(t[k], t[(k + 1) % 3]));
            }
        }
    }
    // the middle of an edge: one point for the edge (and the same point for the two points that are the same place on a seam: they get one each)
    let mut midpoint = |a: u32, b: u32, out: &mut AssetMesh| -> u32 {
        let k = (a.min(b), a.max(b));
        *mid.entry(k).or_insert_with(|| {
            let (p, q) = (mesh.verts[a as usize], mesh.verts[b as usize]);
            out.verts.push([(p[0] + q[0]) / 2.0, (p[1] + q[1]) / 2.0, (p[2] + q[2]) / 2.0]);
            let (u, v) = (mesh.uvs[a as usize], mesh.uvs[b as usize]);
            out.uvs.push([(u[0] + v[0]) / 2.0, (u[1] + v[1]) / 2.0]);
            parents.push((a, b));
            (out.verts.len() - 1) as u32
        })
    };
    let mut tris: Vec<[u32; 3]> = Vec::with_capacity(mesh.tris.len() * 2);
    for t in &mesh.tris {
        let split: [bool; 3] = [0, 1, 2].map(|k| cut.contains(&edge_key(t[k], t[(k + 1) % 3])));
        let m: [Option<u32>; 3] = [0, 1, 2].map(|k| split[k].then(|| midpoint(t[k], t[(k + 1) % 3], &mut out)));
        match (split[0], split[1], split[2]) {
            (false, false, false) => tris.push(*t),
            (true, true, true) => {
                let (a, b, c) = (m[0].unwrap(), m[1].unwrap(), m[2].unwrap());
                tris.extend([[t[0], a, c], [a, t[1], b], [c, b, t[2]], [a, b, c]]);
            }
            _ => {
                // one or two edges are cut: the corners are cut off, and what is left is cut into triangles
                let n = split.iter().filter(|s| **s).count();
                if n == 1 {
                    let k = split.iter().position(|s| *s).unwrap();
                    let (a, b, c, mm) = (t[k], t[(k + 1) % 3], t[(k + 2) % 3], m[k].unwrap());
                    tris.extend([[a, mm, c], [mm, b, c]]);
                } else {
                    // the corner between the two cut edges is cut off, the rest is a quad made of two triangles
                    let k = split.iter().position(|s| !*s).unwrap();
                    let (a, b, c) = (t[k], t[(k + 1) % 3], t[(k + 2) % 3]);
                    let (mb, mc) = (m[(k + 1) % 3].unwrap(), m[(k + 2) % 3].unwrap());
                    tris.extend([[a, b, mb], [a, mb, mc], [mc, mb, c]]);
                }
            }
        }
    }
    out.tris = tris;
    Some((out, parents))
}

/// The holes of a mesh closed: an edge that has a face on one side only is a border; borders that go round and meet are a hole, and a hole of no more than
/// `max_edges` edges is capped with triangles to a new point in its middle. Returns the new mesh, for every new point one old point it is a copy of (for what
/// belongs to the points, the weights), and how many holes were closed. None when there was no hole to close.
pub fn fill_holes(mesh: &AssetMesh, max_edges: usize) -> Option<(AssetMesh, Vec<u32>, usize)> {
    let key = |p: [f32; 3]| ((p[0] * 20000.0).round() as i32, (p[1] * 20000.0).round() as i32, (p[2] * 20000.0).round() as i32);
    let place: Vec<(i32, i32, i32)> = mesh.verts.iter().map(|p| key(*p)).collect();
    // every edge with its direction, by the places of its ends
    let mut directed: HashMap<((i32, i32, i32), (i32, i32, i32)), Vec<(u32, u32)>> = HashMap::new();
    for t in &mesh.tris {
        for k in 0..3 {
            let (a, b) = (t[k], t[(k + 1) % 3]);
            directed.entry((place[a as usize], place[b as usize])).or_default().push((a, b));
        }
    }
    // the border: an edge a to b whose way back, b to a, is not there
    let mut border: HashMap<(i32, i32, i32), Vec<(u32, u32)>> = HashMap::new();
    for ((pa, pb), edges) in &directed {
        if !directed.contains_key(&(*pb, *pa)) {
            for e in edges {
                border.entry(*pa).or_default().push(*e);
            }
        }
    }
    let mut out = mesh.clone();
    let mut copy_of = Vec::new();
    let mut closed = 0;
    let mut used: std::collections::HashSet<(u32, u32)> = std::collections::HashSet::new();
    let starts: Vec<(u32, u32)> = border.values().flatten().copied().collect();
    for start in starts {
        if used.contains(&start) {
            continue;
        }
        // follow the border: the next edge starts where this one ends
        let mut loop_edges = vec![start];
        let mut at = start;
        let mut ok = false;
        while loop_edges.len() <= max_edges + 1 {
            let next = border.get(&place[at.1 as usize]).and_then(|v| v.iter().find(|e| !used.contains(e) && !loop_edges.contains(e)).copied());
            match next {
                Some(e) => {
                    if place[e.1 as usize] == place[start.0 as usize] {
                        loop_edges.push(e);
                        ok = true;
                        break;
                    }
                    loop_edges.push(e);
                    at = e;
                }
                None => break,
            }
        }
        for e in &loop_edges {
            used.insert(*e);
        }
        if !ok || loop_edges.len() > max_edges || loop_edges.len() < 3 {
            continue;
        }
        // a new point in the middle, and a triangle for every edge of the border, turned the way that closes the hole
        let n = loop_edges.len() as f32;
        let (mut c, mut uv) = ([0.0f32; 3], [0.0f32; 2]);
        for (a, _) in &loop_edges {
            for k in 0..3 {
                c[k] += mesh.verts[*a as usize][k] / n;
            }
            uv[0] += mesh.uvs[*a as usize][0] / n;
            uv[1] += mesh.uvs[*a as usize][1] / n;
        }
        out.verts.push(c);
        out.uvs.push(uv);
        copy_of.push(loop_edges[0].0);
        let centre = (out.verts.len() - 1) as u32;
        for (a, b) in &loop_edges {
            out.tris.push([*b, *a, centre]);
        }
        closed += 1;
    }
    (closed > 0).then_some((out, copy_of, closed))
}

/// The faces that have a corner within `radius` of the head of a bone (the joints of a body), except the bones that hang from nothing.
pub fn faces_near_joints(mesh: &AssetMesh, rig: &Rig3, radius: f32) -> Vec<bool> {
    let joints: Vec<[f32; 3]> = rig.bones.iter().filter(|b| b.parent.is_some()).map(|b| b.head).collect();
    let near = |p: [f32; 3]| joints.iter().any(|j| (p[0] - j[0]).powi(2) + (p[1] - j[1]).powi(2) + (p[2] - j[2]).powi(2) < radius * radius);
    mesh.tris.iter().map(|t| t.iter().any(|i| near(mesh.verts[*i as usize]))).collect()
}

/// The triangles joined in pairs where two of them make a quad that is nearly flat and not bent in: the quads, and the triangles that were not joined.
/// `flat_degrees` is how far two triangles may turn from each other to be joined (a few degrees keeps the surface).
pub fn pair_quads(mesh: &AssetMesh, flat_degrees: f32) -> (Vec<[u32; 4]>, Vec<[u32; 3]>) {
    let cos_flat = flat_degrees.to_radians().cos();
    let normal = |t: &[u32; 3]| {
        let (a, b, c) = (mesh.verts[t[0] as usize], mesh.verts[t[1] as usize], mesh.verts[t[2] as usize]);
        let (u, v) = ([b[0] - a[0], b[1] - a[1], b[2] - a[2]], [c[0] - a[0], c[1] - a[1], c[2] - a[2]]);
        let n = [u[1] * v[2] - u[2] * v[1], u[2] * v[0] - u[0] * v[2], u[0] * v[1] - u[1] * v[0]];
        let l = (n[0] * n[0] + n[1] * n[1] + n[2] * n[2]).sqrt().max(1e-12);
        [n[0] / l, n[1] / l, n[2] / l]
    };
    // the triangles that share an edge (the same two points), by the edge
    let mut by_edge: HashMap<(u32, u32), Vec<usize>> = HashMap::new();
    for (i, t) in mesh.tris.iter().enumerate() {
        for k in 0..3 {
            let (a, b) = (t[k], t[(k + 1) % 3]);
            by_edge.entry((a.min(b), a.max(b))).or_default().push(i);
        }
    }
    // every pair, with how good the quad is: the corners near right angles, the two triangles flat together
    let mut pairs: Vec<(f32, usize, usize, [u32; 4])> = Vec::new();
    for (edge, tris) in &by_edge {
        if tris.len() != 2 {
            continue;
        }
        let (i, j) = (tris[0], tris[1]);
        let (ti, tj) = (&mesh.tris[i], &mesh.tris[j]);
        let (ni, nj) = (normal(ti), normal(tj));
        if ni[0] * nj[0] + ni[1] * nj[1] + ni[2] * nj[2] < cos_flat {
            continue;
        }
        // the corner of each triangle that is not on the shared edge
        let (Some(&ci), Some(&cj)) = (ti.iter().find(|v| **v != edge.0 && **v != edge.1), tj.iter().find(|v| **v != edge.0 && **v != edge.1)) else { continue };
        // the quad goes ci, the edge (in the order of the first triangle), cj
        let k = ti.iter().position(|v| *v == ci).unwrap();
        let (e0, e1) = (ti[(k + 1) % 3], ti[(k + 2) % 3]);
        let quad = [ci, e0, cj, e1];
        let p: Vec<[f32; 3]> = quad.iter().map(|v| mesh.verts[*v as usize]).collect();
        let mut score = 0.0f32;
        let mut convex = true;
        for c in 0..4 {
            let (a, b, d) = (p[(c + 3) % 4], p[c], p[(c + 1) % 4]);
            let (u, v) = ([a[0] - b[0], a[1] - b[1], a[2] - b[2]], [d[0] - b[0], d[1] - b[1], d[2] - b[2]]);
            let (lu, lv) = ((u[0] * u[0] + u[1] * u[1] + u[2] * u[2]).sqrt().max(1e-12), (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt().max(1e-12));
            let cosine = (u[0] * v[0] + u[1] * v[1] + u[2] * v[2]) / (lu * lv);
            score += cosine.abs();
            // a corner turned the wrong way (the quad is bent in) makes the pair no good
            let cr = [u[1] * v[2] - u[2] * v[1], u[2] * v[0] - u[0] * v[2], u[0] * v[1] - u[1] * v[0]];
            if cr[0] * ni[0] + cr[1] * ni[1] + cr[2] * ni[2] > 0.0 {
                convex = false;
            }
        }
        if convex {
            pairs.push((score, i, j, quad));
        }
    }
    pairs.sort_by(|a, b| a.0.total_cmp(&b.0).then(a.1.cmp(&b.1)));
    let mut used = vec![false; mesh.tris.len()];
    let mut quads = Vec::new();
    for (_, i, j, q) in pairs {
        if !used[i] && !used[j] {
            used[i] = true;
            used[j] = true;
            quads.push(q);
        }
    }
    let left = mesh.tris.iter().enumerate().filter(|(i, _)| !used[*i]).map(|(_, t)| *t).collect();
    (quads, left)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn area_sum(m: &AssetMesh) -> f32 {
        m.tris
            .iter()
            .map(|t| {
                let (a, b, c) = (m.verts[t[0] as usize], m.verts[t[1] as usize], m.verts[t[2] as usize]);
                let (u, v) = ([b[0] - a[0], b[1] - a[1], b[2] - a[2]], [c[0] - a[0], c[1] - a[1], c[2] - a[2]]);
                let n = [u[1] * v[2] - u[2] * v[1], u[2] * v[0] - u[0] * v[2], u[0] * v[1] - u[1] * v[0]];
                0.5 * (n[0] * n[0] + n[1] * n[1] + n[2] * n[2]).sqrt()
            })
            .sum()
    }

    #[test]
    fn a_ball_has_the_number_of_points_asked_for_and_is_round_and_every_shape_is_sound() {
        let p = ShapeParams { size: 0.1, segments: 12, rings: 6, ..Default::default() };
        let b = make(Shape::Ball, &p);
        assert_eq!(b.verts.len(), 2 + 12 * 5, "a point at each pole and rings of points between");
        assert!(b.verts.iter().all(|v| ((v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt() - 0.1).abs() < 1e-5));
        // the area of a ball made of triangles is nearly the area of the ball
        assert!((area_sum(&b) / (4.0 * std::f32::consts::PI * 0.01) - 1.0).abs() < 0.08, "{}", area_sum(&b));
        let finer = make(Shape::Ball, &ShapeParams { segments: 32, rings: 16, ..p });
        assert!(finer.verts.len() > b.verts.len() * 4, "more points when asked for");
        for s in Shape::ALL {
            let m = make(s, &ShapeParams::default());
            assert!(!m.tris.is_empty(), "{s:?}");
            assert_eq!(m.uvs.len(), m.verts.len());
            assert!(m.tris.iter().flatten().all(|i| (*i as usize) < m.verts.len()), "{s:?}");
            assert!(m.uvs.iter().all(|q| (0.0..=1.0).contains(&q[0]) && (0.0..=1.0).contains(&q[1])), "{s:?}");
            assert!(m.verts.iter().flatten().all(|x| x.is_finite()));
        }
        let cone = make(Shape::Cone, &ShapeParams { height: 0.2, ..Default::default() });
        assert!((cone.bounds().1[1] - 0.2).abs() < 1e-5);
    }

    #[test]
    fn subdividing_makes_more_faces_and_points_and_leaves_no_crack() {
        let plane = make(Shape::Plane, &ShapeParams { segments: 4, ..Default::default() });
        // cut only the faces of one corner: the faces next to them are cut as much as is needed
        let faces: Vec<bool> = plane.tris.iter().enumerate().map(|(i, _)| i < 4).collect();
        let (m, parents) = subdivide(&plane, &faces).unwrap();
        assert!(m.tris.len() > plane.tris.len() && m.verts.len() > plane.verts.len());
        assert_eq!(parents.len(), m.verts.len() - plane.verts.len());
        assert!((area_sum(&m) - area_sum(&plane)).abs() < 1e-4, "the surface is the same");
        // no crack: every edge of a triangle has a triangle on the other side (the plane is double sided: two on each)
        let mut count: HashMap<(u32, u32), usize> = HashMap::new();
        for t in &m.tris {
            for k in 0..3 {
                let (a, b) = (t[k], t[(k + 1) % 3]);
                *count.entry((a.min(b), a.max(b))).or_default() += 1;
            }
        }
        // the border of the plane has two faces (the two sides), the inside four
        assert!(count.values().all(|c| *c == 2 || *c == 4), "{:?}", count.values().collect::<std::collections::BTreeSet<_>>());
        assert!(subdivide(&plane, &vec![false; plane.tris.len()]).is_none());
        // all of it cut: four times as many triangles
        let (all, _) = subdivide(&plane, &vec![true; plane.tris.len()]).unwrap();
        assert_eq!(all.tris.len(), plane.tris.len() * 4);
    }

    #[test]
    fn the_faces_near_the_joints_of_a_rig_are_found() {
        let mut rig = Rig3::new(0);
        let a = rig.add_bone("a", None, [0.0, 0.0, 0.0], [0.0, 1.0, 0.0]);
        rig.add_bone("b", Some(a), [0.0, 1.0, 0.0], [0.0, 2.0, 0.0]);
        let m = make(Shape::Cylinder, &ShapeParams { size: 0.1, height: 2.0, segments: 8, rings: 20 });
        let near = faces_near_joints(&m, &rig, 0.25);
        let n = near.iter().filter(|f| **f).count();
        assert!(n > 0 && n < m.tris.len() / 2, "{n} of {}", m.tris.len());
        // only a joint with a parent counts: a bone with no parent has none
        assert!(faces_near_joints(&m, &Rig3::new(0), 0.25).iter().all(|f| !*f));
    }

    #[test]
    fn a_flat_grid_of_triangles_pairs_up_into_quads_and_a_bent_one_does_not() {
        let plane = make(Shape::Plane, &ShapeParams { segments: 4, ..Default::default() });
        // one side only
        let mut one = plane.clone();
        one.tris = plane.tris.iter().copied().step_by(1).enumerate().filter(|(i, _)| i % 4 < 2).map(|(_, t)| t).collect();
        let (quads, left) = pair_quads(&one, 5.0);
        assert_eq!((quads.len(), left.len()), (16, 0), "sixteen squares are sixteen quads");
        for q in &quads {
            let p: Vec<[f32; 3]> = q.iter().map(|v| one.verts[*v as usize]).collect();
            // a square: the sides are as long as each other
            let d = |a: [f32; 3], b: [f32; 3]| ((a[0] - b[0]).powi(2) + (a[1] - b[1]).powi(2) + (a[2] - b[2]).powi(2)).sqrt();
            assert!((d(p[0], p[1]) - d(p[1], p[2])).abs() < 1e-4 && (d(p[2], p[3]) - d(p[3], p[0])).abs() < 1e-4);
        }
        // a box: the triangles on a face pair up, the ones across an edge (a right angle) do not
        let cube = make(Shape::Cube, &ShapeParams { segments: 4, ..Default::default() });
        let (q, l) = pair_quads(&cube, 5.0);
        assert_eq!((q.len(), l.len()), (6, 0), "six faces, six quads");
        // turned right off: nothing pairs when no angle is allowed and the triangles are not flat
        let ball = make(Shape::Ball, &ShapeParams { segments: 8, rings: 4, ..Default::default() });
        let (q2, l2) = pair_quads(&ball, 0.5);
        assert!(q2.len() * 2 + l2.len() == ball.tris.len(), "every triangle is in a quad or left");
    }

    #[test]
    fn the_holes_of_a_mesh_are_closed_up_to_the_size_given_and_a_closed_mesh_has_none() {
        // a tube with both ends open
        let mut tube = make(Shape::Cylinder, &ShapeParams { size: 0.1, height: 0.5, segments: 12, rings: 4 });
        // take the caps off: the triangles that touch the middle points of the ends are the last ones made
        let keep = tube.tris.len() - 24;
        tube.tris.truncate(keep);
        let (closed, copy_of, n) = fill_holes(&tube, 40).expect("two holes");
        assert_eq!(n, 2);
        assert_eq!(copy_of.len(), 2);
        assert_eq!(closed.tris.len(), tube.tris.len() + 24, "twelve triangles for each hole");
        // the caps look out of the tube: the top one up and the bottom one down
        let normal_y = |t: &[u32; 3]| {
            let (a, b, c) = (closed.verts[t[0] as usize], closed.verts[t[1] as usize], closed.verts[t[2] as usize]);
            (b[2] - a[2]) * (c[0] - a[0]) - (b[0] - a[0]) * (c[2] - a[2])
        };
        let new: Vec<&[u32; 3]> = closed.tris[tube.tris.len()..].iter().collect();
        let (up, down): (Vec<_>, Vec<_>) = new.into_iter().partition(|t| closed.verts[t[2] as usize][1] > 0.25);
        assert!(up.iter().all(|t| normal_y(t) > 0.0), "the top looks up");
        assert!(down.iter().all(|t| normal_y(t) < 0.0), "the bottom looks down");
        // a hole that is too big is left, and a mesh with no holes has nothing to close
        assert!(fill_holes(&tube, 8).is_none(), "twelve edges are more than eight");
        assert!(fill_holes(&closed, 40).is_none());
        assert!(fill_holes(&make(Shape::Ball, &ShapeParams::default()), 40).is_none());
    }
}
