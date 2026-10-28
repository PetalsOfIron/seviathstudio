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
}
