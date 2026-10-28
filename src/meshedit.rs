// SPDX-License-Identifier: GPL-3.0-or-later
// Editing the mesh itself: finding the separate pieces of a mesh (the wings of a base are pieces of their own), taking pieces away, and putting new geometry on
// (the strokes of the mesh brushes). Points that are the same place count as one when pieces are found, so a seam of the texture does not cut a piece in two.

use std::collections::HashMap;

use crate::assetview::AssetMesh;

/// For every triangle the number of the piece it is in (pieces are numbered from the biggest), and the number of triangles of every piece.
pub fn pieces(mesh: &AssetMesh) -> (Vec<u32>, Vec<usize>) {
    let key = |p: [f32; 3]| ((p[0] * 20000.0).round() as i32, (p[1] * 20000.0).round() as i32, (p[2] * 20000.0).round() as i32);
    let mut ids: HashMap<(i32, i32, i32), u32> = HashMap::with_capacity(mesh.verts.len());
    let place: Vec<u32> = mesh.verts.iter().map(|p| { let n = ids.len() as u32; *ids.entry(key(*p)).or_insert(n) }).collect();
    let mut parent: Vec<u32> = (0..ids.len() as u32).collect();
    fn find(parent: &mut [u32], mut x: u32) -> u32 {
        while parent[x as usize] != x {
            parent[x as usize] = parent[parent[x as usize] as usize];
            x = parent[x as usize];
        }
        x
    }
    for t in &mesh.tris {
        let (a, b, c) = (place[t[0] as usize], place[t[1] as usize], place[t[2] as usize]);
        for (x, y) in [(a, b), (b, c)] {
            let (rx, ry) = (find(&mut parent, x), find(&mut parent, y));
            if rx != ry {
                parent[rx as usize] = ry;
            }
        }
    }
    let roots: Vec<u32> = mesh.tris.iter().map(|t| find(&mut parent, place[t[0] as usize])).collect();
    let mut size: HashMap<u32, usize> = HashMap::new();
    for r in &roots {
        *size.entry(*r).or_default() += 1;
    }
    let mut order: Vec<(u32, usize)> = size.into_iter().collect();
    order.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
    let number: HashMap<u32, u32> = order.iter().enumerate().map(|(i, (r, _))| (*r, i as u32)).collect();
    (roots.iter().map(|r| number[r]).collect(), order.iter().map(|o| o.1).collect())
}

/// The box (lowest and highest point) of the triangles of a piece.
pub fn piece_bounds(mesh: &AssetMesh, tri_piece: &[u32], piece: u32) -> ([f32; 3], [f32; 3]) {
    let (mut lo, mut hi) = ([f32::MAX; 3], [f32::MIN; 3]);
    for (t, p) in mesh.tris.iter().zip(tri_piece) {
        if *p != piece {
            continue;
        }
        for i in t {
            for k in 0..3 {
                lo[k] = lo[k].min(mesh.verts[*i as usize][k]);
                hi[k] = hi[k].max(mesh.verts[*i as usize][k]);
            }
        }
    }
    (lo, hi)
}

/// The mesh without the triangles that `remove` says (the points nothing uses any more go too). Gives the new number of every old point (`None` for the
/// ones that went), so what belongs to the points (weights) can follow.
pub fn remove_tris(mesh: &AssetMesh, remove: &[bool]) -> (AssetMesh, Vec<Option<u32>>) {
    let mut keep = vec![false; mesh.verts.len()];
    for (t, r) in mesh.tris.iter().zip(remove) {
        if !*r {
            for i in t {
                keep[*i as usize] = true;
            }
        }
    }
    let mut new_index: Vec<Option<u32>> = vec![None; mesh.verts.len()];
    let (mut verts, mut uvs) = (Vec::new(), Vec::new());
    for (i, k) in keep.iter().enumerate() {
        if *k {
            new_index[i] = Some(verts.len() as u32);
            verts.push(mesh.verts[i]);
            uvs.push(mesh.uvs[i]);
        }
    }
    let tris = mesh.tris.iter().zip(remove).filter(|(_, r)| !**r).map(|(t, _)| [new_index[t[0] as usize].unwrap(), new_index[t[1] as usize].unwrap(), new_index[t[2] as usize].unwrap()]).collect();
    (AssetMesh { verts, uvs, tris, texture: mesh.texture.clone(), normal: mesh.normal.clone() }, new_index)
}

/// A strip of the texture that nothing uses is added at the bottom (for the colours of the strokes of the mesh brushes and of parts), on a mesh that has a
/// texture but no free place: the picture gets taller, the old picture keeps its place at the top, and the texture coordinates are squeezed to fit. The normal
/// map, when there is one, grows the same way. Returns where the strip starts (v).
pub fn add_strip(mesh: &mut AssetMesh, share: f32) -> Option<f32> {
    let tex = mesh.texture.as_ref()?;
    let (w, h) = tex.dimensions();
    let new_h = (h as f32 / (1.0 - share)).ceil() as u32;
    let grow = |img: &image::RgbaImage, fill: image::Rgba<u8>| {
        let mut out = image::RgbaImage::from_pixel(img.width(), new_h, fill);
        image::imageops::replace(&mut out, img, 0, 0);
        out
    };
    let keep = h as f32 / new_h as f32;
    mesh.texture = Some(grow(tex, image::Rgba([128, 128, 128, 255])));
    if let Some(n) = mesh.normal.as_ref().filter(|n| n.dimensions() == (w, h)) {
        mesh.normal = Some(grow(n, image::Rgba([128, 128, 255, 255])));
    }
    for uv in &mut mesh.uvs {
        uv[1] *= keep;
    }
    Some(keep)
}

/// The triangles that have all three corners selected (a face is selected when its points are).
pub fn selected_tris(mesh: &AssetMesh, sel: &[bool]) -> Vec<bool> {
    mesh.tris.iter().map(|t| t.iter().all(|i| sel.get(*i as usize).copied().unwrap_or(false))).collect()
}

/// The selected faces are taken away (the points nothing uses any more go with them).
pub fn delete_selected(mesh: &AssetMesh, sel: &[bool]) -> (AssetMesh, Vec<Option<u32>>) {
    remove_tris(mesh, &selected_tris(mesh, sel))
}

/// The selection one step wider: the points next to a selected point are selected too.
pub fn grow(sel: &[bool], near: &[Vec<u32>]) -> Vec<bool> {
    (0..sel.len()).map(|i| sel[i] || near.get(i).is_some_and(|n| n.iter().any(|j| sel.get(*j as usize).copied().unwrap_or(false)))).collect()
}

/// The selection one step narrower: a point stays selected when all the points next to it are.
pub fn shrink(sel: &[bool], near: &[Vec<u32>]) -> Vec<bool> {
    (0..sel.len()).map(|i| sel[i] && near.get(i).is_none_or(|n| n.iter().all(|j| sel.get(*j as usize).copied().unwrap_or(false)))).collect()
}

/// What an operation that makes new points gives back: the mesh, which old point every new point is a copy of (for what belongs to the points: the weights),
/// and the new selection (the new points).
pub struct Made {
    pub mesh: AssetMesh,
    pub copy_of: Vec<u32>,
    pub selection: Vec<bool>,
}

fn place_key(p: [f32; 3]) -> (i32, i32, i32) {
    ((p[0] * 20000.0).round() as i32, (p[1] * 20000.0).round() as i32, (p[2] * 20000.0).round() as i32)
}

/// New points for the points of the selected faces, moved by `shift` (or along the way the selected faces face, by `dist`).
fn copy_selected(mesh: &AssetMesh, sel: &[bool], dist: f32, shift: [f32; 3], walls: bool, keep_old: bool) -> Option<Made> {
    let faces = selected_tris(mesh, sel);
    if !faces.iter().any(|f| *f) {
        return None;
    }
    let old = mesh.verts.len();
    // the way the selected faces face at every point of them
    let mut normal = vec![[0.0f32; 3]; old];
    for (t, f) in mesh.tris.iter().zip(&faces) {
        if !*f {
            continue;
        }
        let (a, b, c) = (mesh.verts[t[0] as usize], mesh.verts[t[1] as usize], mesh.verts[t[2] as usize]);
        let (u, v) = ([b[0] - a[0], b[1] - a[1], b[2] - a[2]], [c[0] - a[0], c[1] - a[1], c[2] - a[2]]);
        let n = [u[1] * v[2] - u[2] * v[1], u[2] * v[0] - u[0] * v[2], u[0] * v[1] - u[1] * v[0]];
        for i in t {
            for k in 0..3 {
                normal[*i as usize][k] += n[k];
            }
        }
    }
    let mut out = mesh.clone();
    let mut new_of: HashMap<u32, u32> = HashMap::new();
    let mut copy_of = Vec::new();
    for (t, f) in mesh.tris.iter().zip(&faces) {
        if *f {
            for i in t {
                new_of.entry(*i).or_insert_with(|| {
                    let n = normal[*i as usize];
                    let l = (n[0] * n[0] + n[1] * n[1] + n[2] * n[2]).sqrt().max(1e-12);
                    let p = mesh.verts[*i as usize];
                    out.verts.push([p[0] + shift[0] + n[0] / l * dist, p[1] + shift[1] + n[1] / l * dist, p[2] + shift[2] + n[2] / l * dist]);
                    out.uvs.push(mesh.uvs[*i as usize]);
                    copy_of.push(*i);
                    (out.verts.len() - 1) as u32
                });
            }
        }
    }
    // the edges of the selected faces that have no selected face on the other side: the border
    let mut seen: HashMap<((i32, i32, i32), (i32, i32, i32)), u32> = HashMap::new();
    let key = |a: u32, b: u32| {
        let (ka, kb) = (place_key(mesh.verts[a as usize]), place_key(mesh.verts[b as usize]));
        if ka <= kb { (ka, kb) } else { (kb, ka) }
    };
    for (t, f) in mesh.tris.iter().zip(&faces) {
        if *f {
            for k in 0..3 {
                *seen.entry(key(t[k], t[(k + 1) % 3])).or_default() += 1;
            }
        }
    }
    let mut tris = Vec::new();
    for (t, f) in mesh.tris.iter().zip(&faces) {
        if !*f {
            tris.push(*t);
            continue;
        }
        let moved = [new_of[&t[0]], new_of[&t[1]], new_of[&t[2]]];
        if keep_old {
            tris.push(*t);
        }
        tris.push(moved);
        if walls {
            for k in 0..3 {
                let (a, b) = (t[k], t[(k + 1) % 3]);
                if seen.get(&key(a, b)).copied() == Some(1) {
                    let (a2, b2) = (new_of[&a], new_of[&b]);
                    tris.push([a, b, b2]);
                    tris.push([a, b2, a2]);
                }
            }
        }
    }
    out.tris = tris;
    let mut selection = vec![false; out.verts.len()];
    for n in new_of.values() {
        selection[*n as usize] = true;
    }
    Some(Made { mesh: out, copy_of, selection })
}

/// The selected faces are pushed out along the way they face (in when the distance is below 0), with walls round them, like the extrude of Blender.
pub fn extrude(mesh: &AssetMesh, sel: &[bool], dist: f32) -> Option<Made> {
    copy_selected(mesh, sel, dist, [0.0; 3], true, false)
}

/// A copy of the selected faces, moved by `offset` (as new geometry of its own).
pub fn duplicate(mesh: &AssetMesh, sel: &[bool], offset: [f32; 3]) -> Option<Made> {
    copy_selected(mesh, sel, 0.0, offset, false, true)
}

/// The selected points move along the way the surface faces there (the surface swells, or sinks when the distance is below 0).
pub fn push_along_normals(mesh: &mut AssetMesh, sel: &[bool], dist: f32) {
    let n = crate::meshfile::vertex_normals(&mesh.verts, &mesh.tris);
    for (i, v) in mesh.verts.iter_mut().enumerate() {
        if sel.get(i).copied().unwrap_or(false) {
            for k in 0..3 {
                v[k] += n[i][k] * dist;
            }
        }
    }
}

/// The selected points are made smoother: each moves towards the middle of the points next to it.
pub fn smooth_selected(mesh: &mut AssetMesh, sel: &[bool], near: &[Vec<u32>], passes: usize) {
    for _ in 0..passes {
        let old = mesh.verts.clone();
        for i in 0..old.len() {
            if !sel.get(i).copied().unwrap_or(false) || near[i].is_empty() {
                continue;
            }
            let mut avg = [0.0f32; 3];
            for j in &near[i] {
                for k in 0..3 {
                    avg[k] += old[*j as usize][k] / near[i].len() as f32;
                }
            }
            for k in 0..3 {
                mesh.verts[i][k] = old[i][k] + (avg[k] - old[i][k]) * 0.5;
            }
        }
    }
}

/// The selected points are scaled about their middle.
pub fn scale_selected(mesh: &mut AssetMesh, sel: &[bool], k: f32) {
    let picked: Vec<usize> = (0..mesh.verts.len()).filter(|i| sel.get(*i).copied().unwrap_or(false)).collect();
    if picked.is_empty() {
        return;
    }
    let mut c = [0.0f32; 3];
    for i in &picked {
        for a in 0..3 {
            c[a] += mesh.verts[*i][a] / picked.len() as f32;
        }
    }
    for i in picked {
        for a in 0..3 {
            mesh.verts[i][a] = c[a] + (mesh.verts[i][a] - c[a]) * k;
        }
    }
}

/// Which side of a plane is cut away.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Cut {
    /// 0 is x, 1 is y, 2 is z.
    pub axis: usize,
    pub limit: f32,
    /// Away what is more than the limit (true) or less than it (false).
    pub above: bool,
}

/// The mesh without what is on one side of a plane: every triangle that has a corner on that side goes (so no sliver of a wing is left on the body).
pub fn cut_plane(mesh: &AssetMesh, c: Cut) -> (AssetMesh, Vec<Option<u32>>) {
    let beyond = |p: &[f32; 3]| if c.above { p[c.axis] > c.limit } else { p[c.axis] < c.limit };
    let remove: Vec<bool> = mesh.tris.iter().map(|t| t.iter().any(|i| beyond(&mesh.verts[*i as usize]))).collect();
    remove_tris(mesh, &remove)
}

/// Only the biggest piece stays (what a cut left floating goes).
pub fn keep_biggest(mesh: &AssetMesh) -> (AssetMesh, Vec<Option<u32>>) {
    let (piece, _) = pieces(mesh);
    let remove: Vec<bool> = piece.iter().map(|p| *p != 0).collect();
    remove_tris(mesh, &remove)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Two separate triangles and one that shares a corner place with the first (the corner is two points that are the same place).
    fn mesh() -> AssetMesh {
        let verts = vec![[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 0.0], [0.0, 0.0, 1.0], [0.0, 1.0, 1.0], [5.0, 5.0, 5.0], [6.0, 5.0, 5.0], [5.0, 6.0, 5.0]];
        AssetMesh { uvs: vec![[0.0; 2]; verts.len()], verts, tris: vec![[0, 1, 2], [3, 4, 5], [6, 7, 8]], texture: None, normal: None }
    }

    #[test]
    fn pieces_are_found_by_place_and_numbered_from_the_biggest() {
        let (piece, sizes) = pieces(&mesh());
        assert_eq!(sizes, vec![2, 1]);
        assert_eq!(piece[0], piece[1], "the points that are the same place join the two triangles");
        assert_ne!(piece[0], piece[2]);
        let (lo, hi) = piece_bounds(&mesh(), &piece, piece[2]);
        assert_eq!((lo, hi), ([5.0, 5.0, 5.0], [6.0, 6.0, 5.0]));
    }

    #[test]
    fn taking_a_piece_away_keeps_the_rest_and_says_where_the_points_went() {
        let m = mesh();
        let (piece, _) = pieces(&m);
        let remove: Vec<bool> = piece.iter().map(|p| *p == piece[2]).collect();
        let (out, map) = remove_tris(&m, &remove);
        assert_eq!((out.tris.len(), out.verts.len()), (2, 6));
        assert_eq!(map[6], None);
        assert_eq!(map[4], Some(4));
        assert!(out.tris.iter().flatten().all(|i| (*i as usize) < out.verts.len()));
    }

    #[test]
    fn a_plane_cuts_away_one_side_and_the_pieces_left_over_can_be_dropped() {
        let m = mesh();
        let (out, _) = cut_plane(&m, Cut { axis: 2, limit: 0.5, above: true });
        // the second triangle (z up to 1) and the third (z = 5) are cut
        assert_eq!(out.tris.len(), 1);
        let (out, _) = cut_plane(&m, Cut { axis: 2, limit: 0.5, above: false });
        assert_eq!(out.tris.len(), 1, "only the triangle at z = 5 is left");
        let (big, _) = keep_biggest(&m);
        assert_eq!(big.tris.len(), 2);
    }

    /// Makes the human base from Mirrorfolk (the wings cut away, the outfit kept) and writes it next to the other bases. Run by hand:
    /// `cargo test --release make_the_human_base -- --ignored --nocapture`.
    #[test]
    #[ignore]
    fn make_the_human_base() {
        let dir = "C:/Users/SaphiresSilver/Downloads/Bases/Readybases/Races";
        let m = AssetMesh::from_glb(&std::fs::read(format!("{dir}/Mirrorfolkbasefemale.glb")).unwrap()).unwrap();
        let (cut, _) = cut_plane(&m, Cut { axis: 2, limit: 0.43, above: false });
        let (human, _) = keep_biggest(&cut);
        println!("{} -> {} triangles", m.tris.len(), human.tris.len());
        let out = format!("{dir}/Humanbasefemale.glb");
        assert!(!std::path::Path::new(&out).exists(), "the file is already there");
        std::fs::write(&out, crate::meshfile::write_glb(&human, None, &[], 30.0)).unwrap();
        let work = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("target").join("helpwork");
        for (name, yaw) in [("front", 0.0), ("back", 180.0), ("side", 90.0)] {
            human.render_view(yaw, 0.0, 700, 900, false, crate::assetview::View::Normals).save(work.join(format!("human_{name}.png"))).unwrap();
        }
    }

    /// The real Mirrorfolk base: its pieces. Run by hand: `cargo test --release mirrorfolk_pieces -- --ignored --nocapture`.
    #[test]
    #[ignore]
    fn mirrorfolk_pieces() {
        let path = "C:/Users/SaphiresSilver/Downloads/Bases/Readybases/Races/Mirrorfolkbasefemale.glb";
        let m = AssetMesh::from_glb(&std::fs::read(path).unwrap()).unwrap();
        let (piece, sizes) = pieces(&m);
        println!("{} pieces", sizes.len());
        for (i, n) in sizes.iter().enumerate().take(14) {
            let (lo, hi) = piece_bounds(&m, &piece, i as u32);
            println!("piece {i}: {n} triangles, x {:.2}..{:.2}, y {:.2}..{:.2}, z {:.2}..{:.2}", lo[0], hi[0], lo[1], hi[1], lo[2], hi[2]);
        }
    }

    /// Pictures of the real Mirrorfolk base from four sides, to see where the wings are. Run by hand.
    #[test]
    #[ignore]
    fn mirrorfolk_views() {
        let path = "C:/Users/SaphiresSilver/Downloads/Bases/Readybases/Races/Mirrorfolkbasefemale.glb";
        let m = AssetMesh::from_glb(&std::fs::read(path).unwrap()).unwrap();
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("target").join("helpwork");
        for (name, yaw) in [("front", 0.0), ("side", 90.0), ("back", 180.0)] {
            m.render_view(yaw, 0.0, 700, 900, false, crate::assetview::View::Normals).save(dir.join(format!("mirror_{name}.png"))).unwrap();
        }
        // how deep the body is at some heights, in the middle (|x| < 0.12) and the whole depth
        for band in 0..10 {
            let (y0, y1) = (band as f32 * 0.2, band as f32 * 0.2 + 0.2);
            let (mut zl, mut zh, mut al, mut ah) = (f32::MAX, f32::MIN, f32::MAX, f32::MIN);
            for v in &m.verts {
                if v[1] >= y0 && v[1] < y1 {
                    al = al.min(v[2]);
                    ah = ah.max(v[2]);
                    if v[0].abs() < 0.12 {
                        zl = zl.min(v[2]);
                        zh = zh.max(v[2]);
                    }
                }
            }
            println!("y {y0:.1}-{y1:.1}: middle z {zl:.2}..{zh:.2}   all z {al:.2}..{ah:.2}");
        }
    }

    #[test]
    fn a_strip_is_added_under_the_texture_and_the_coordinates_still_find_their_colours() {
        let mut m = mesh();
        m.uvs = vec![[0.0, 1.0], [1.0, 1.0], [0.0, 0.5], [0.0, 0.0], [1.0, 0.0], [0.0, 0.25], [0.5, 0.5], [0.5, 0.5], [0.5, 0.5]];
        let mut tex = image::RgbaImage::new(64, 64);
        for (_, y, p) in tex.enumerate_pixels_mut() {
            *p = image::Rgba([y as u8 * 4, 0, 0, 255]);
        }
        let colour_at = |m: &AssetMesh, i: usize| {
            let t = m.texture.as_ref().unwrap();
            let (u, v) = (m.uvs[i][0], m.uvs[i][1]);
            t.get_pixel((u * (t.width() - 1) as f32) as u32, (v * (t.height() - 1) as f32) as u32).0[0] as i32
        };
        m.texture = Some(tex);
        let before: Vec<i32> = (0..m.verts.len()).map(|i| colour_at(&m, i)).collect();
        let v0 = add_strip(&mut m, 0.125).unwrap();
        assert!((v0 - 0.875).abs() < 0.03, "{v0}");
        assert!(m.texture.as_ref().unwrap().height() > 64);
        assert!(m.uvs.iter().all(|q| q[1] <= v0 + 1e-4), "the picture keeps to the top, the strip is free");
        for (i, b) in before.iter().enumerate() {
            assert!((colour_at(&m, i) - b).abs() <= 8, "point {i}: {} {}", colour_at(&m, i), b);
        }
        let mut none = mesh();
        assert!(add_strip(&mut none, 0.125).is_none(), "no texture: nothing to add to");
    }

    /// A flat strip of three squares in a row (x from 0 to 3), facing +z.
    fn strip3() -> AssetMesh {
        let mut verts = Vec::new();
        for x in 0..=3 {
            verts.push([x as f32, 0.0, 0.0]);
            verts.push([x as f32, 1.0, 0.0]);
        }
        let mut tris = Vec::new();
        for x in 0..3u32 {
            let (a, b, c, d) = (x * 2, x * 2 + 1, x * 2 + 2, x * 2 + 3);
            tris.push([a, c, b]);
            tris.push([c, d, b]);
        }
        AssetMesh { uvs: verts.iter().map(|v| [v[0] / 3.0, v[1]]).collect(), verts, tris, texture: None, normal: None }
    }

    #[test]
    fn the_selection_grows_shrinks_and_picks_the_faces_whose_points_are_all_selected() {
        let m = strip3();
        let near = crate::rig3d::neighbours(&m.verts, &m.tris);
        let mut sel = vec![false; m.verts.len()];
        sel[0] = true;
        sel[1] = true;
        assert_eq!(selected_tris(&m, &sel).iter().filter(|f| **f).count(), 0, "two points are no face");
        let g = grow(&sel, &near);
        assert!(g[2] && g[3] && !g[6], "one step wider");
        assert_eq!(selected_tris(&m, &g).iter().filter(|f| **f).count(), 2, "the first square");
        let s = shrink(&g, &near);
        assert!(s.iter().filter(|x| **x).count() < g.iter().filter(|x| **x).count());
    }

    #[test]
    fn selected_faces_are_deleted_and_the_others_stay() {
        let m = strip3();
        let sel: Vec<bool> = (0..m.verts.len()).map(|i| i < 4).collect();
        let (out, map) = delete_selected(&m, &sel);
        assert_eq!(out.tris.len(), 4, "the first square (two triangles) is gone");
        assert!(map[0].is_none() && map[7].is_some());
    }

    #[test]
    fn an_extrude_pushes_the_faces_out_and_puts_walls_round_them() {
        let m = strip3();
        // the middle square
        let sel: Vec<bool> = (0..m.verts.len()).map(|i| (2..6).contains(&i)).collect();
        let e = extrude(&m, &sel, 0.5).unwrap();
        assert_eq!(e.copy_of.len(), 4, "four new points");
        let new_start = m.verts.len();
        assert!(e.mesh.verts[new_start..].iter().all(|v| (v[2] - 0.5).abs() < 1e-5), "pushed along the way the faces face");
        // two triangles on top, the other four of the strip, and a wall of two triangles on each of the four sides of the square... of which two sides are shared
        // with the squares next to it (they are not selected), so all four sides are borders
        assert_eq!(e.mesh.tris.len(), 6 + 8, "{}", e.mesh.tris.len());
        assert_eq!(e.selection.iter().filter(|x| **x).count(), 4);
        assert!(e.selection[new_start]);
        assert!(e.mesh.tris.iter().flatten().all(|i| (*i as usize) < e.mesh.verts.len()));
        assert!(extrude(&m, &vec![false; m.verts.len()], 0.5).is_none(), "nothing selected, nothing made");
    }

    #[test]
    fn a_duplicate_is_a_copy_apart_and_the_other_tools_move_the_selected_points() {
        let m = strip3();
        let sel: Vec<bool> = (0..m.verts.len()).map(|i| i < 4).collect();
        let d = duplicate(&m, &sel, [0.0, 0.0, 2.0]).unwrap();
        assert_eq!(d.mesh.tris.len(), m.tris.len() + 2);
        assert!(d.mesh.verts[m.verts.len()..].iter().all(|v| (v[2] - 2.0).abs() < 1e-5));
        let mut p = m.clone();
        push_along_normals(&mut p, &sel, 0.25);
        assert!((p.verts[0][2].abs() - 0.25).abs() < 1e-4 && p.verts[7][2] == 0.0);
        let mut s = m.clone();
        scale_selected(&mut s, &sel, 2.0);
        assert!(s.verts[3][1] > m.verts[3][1] && s.verts[7] == m.verts[7]);
        let mut sm = m.clone();
        sm.verts[2][2] = 1.0;
        let near = crate::rig3d::neighbours(&sm.verts, &sm.tris);
        let only2: Vec<bool> = (0..sm.verts.len()).map(|i| i == 2).collect();
        smooth_selected(&mut sm, &only2, &near, 1);
        assert!(sm.verts[2][2] < 1.0);
    }
}
