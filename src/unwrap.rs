// SPDX-License-Identifier: GPL-3.0-or-later
// A UV unwrap for a mesh that has none (the ready bases are only points and triangles): the surface is cut into pieces that are nearly flat (the
// triangles that face about the same way and touch each other), every piece is laid flat along its own average direction, and the pieces are packed into
// one square texture with the same number of texels for every metre. No AI: plain geometry, the way a 3D program's smart projection does it.

use std::collections::HashMap;

use crate::assetview::AssetMesh;

/// How the cutting goes: how far the triangles of one piece may turn from the direction of the piece (degrees), and from the first triangle of it.
#[derive(Clone, Copy, Debug)]
pub struct Options {
    pub angle: f32,
    /// A piece with fewer triangles than this is joined to the piece next to it that faces most like it.
    pub min_tris: usize,
    /// The space between pieces, in texels of the texture of `size`.
    pub margin: f32,
    pub size: u32,
    /// The share of the texture (at the bottom of the picture, v from 1 - reserve to 1) that no piece uses: the strokes of the mesh brushes put their colours there.
    pub reserve: f32,
}

impl Default for Options {
    fn default() -> Self {
        Options { angle: 62.0, min_tris: 12, margin: 4.0, size: 2048, reserve: 0.0 }
    }
}

fn sub(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}

fn cross(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]]
}

fn dot(a: [f32; 3], b: [f32; 3]) -> f32 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

fn unit(a: [f32; 3]) -> [f32; 3] {
    let l = dot(a, a).sqrt().max(1e-20);
    [a[0] / l, a[1] / l, a[2] / l]
}

/// For every triangle the triangles that share an edge with it (points that are the same place count as one).
fn next_to(mesh: &AssetMesh) -> Vec<Vec<u32>> {
    let key = |p: [f32; 3]| ((p[0] * 20000.0).round() as i32, (p[1] * 20000.0).round() as i32, (p[2] * 20000.0).round() as i32);
    let mut ids: HashMap<(i32, i32, i32), u32> = HashMap::with_capacity(mesh.verts.len());
    let place: Vec<u32> = mesh.verts.iter().map(|p| { let n = ids.len() as u32; *ids.entry(key(*p)).or_insert(n) }).collect();
    let mut edges: HashMap<(u32, u32), Vec<u32>> = HashMap::with_capacity(mesh.tris.len() * 2);
    for (ti, t) in mesh.tris.iter().enumerate() {
        for k in 0..3 {
            let (a, b) = (place[t[k] as usize], place[t[(k + 1) % 3] as usize]);
            if a != b {
                edges.entry((a.min(b), a.max(b))).or_default().push(ti as u32);
            }
        }
    }
    let mut out = vec![Vec::new(); mesh.tris.len()];
    for tris in edges.values() {
        for &a in tris {
            for &b in tris {
                if a != b {
                    out[a as usize].push(b);
                }
            }
        }
    }
    out
}

/// The pieces: which piece every triangle is in, and how many pieces there are.
fn charts(mesh: &AssetMesh, normals: &[[f32; 3]], areas: &[f32], near: &[Vec<u32>], o: &Options) -> (Vec<u32>, usize) {
    let cos_run = o.angle.to_radians().cos();
    let cos_seed = (o.angle + 14.0).min(120.0).to_radians().cos();
    let n = mesh.tris.len();
    let mut chart = vec![u32::MAX; n];
    // the big triangles start the pieces, so the pieces grow from the flat middle of a part
    let mut order: Vec<u32> = (0..n as u32).collect();
    order.sort_unstable_by(|a, b| areas[*b as usize].total_cmp(&areas[*a as usize]));
    let mut count = 0u32;
    let mut stack: Vec<u32> = Vec::new();
    for &seed in &order {
        if chart[seed as usize] != u32::MAX {
            continue;
        }
        let id = count;
        count += 1;
        let seed_n = normals[seed as usize];
        let mut sum = [seed_n[0] * areas[seed as usize], seed_n[1] * areas[seed as usize], seed_n[2] * areas[seed as usize]];
        chart[seed as usize] = id;
        stack.push(seed);
        while let Some(t) = stack.pop() {
            for &u in &near[t as usize] {
                if chart[u as usize] != u32::MAX {
                    continue;
                }
                let nu = normals[u as usize];
                if dot(nu, seed_n) < cos_seed || dot(nu, unit(sum)) < cos_run {
                    continue;
                }
                chart[u as usize] = id;
                let a = areas[u as usize];
                for k in 0..3 {
                    sum[k] += nu[k] * a;
                }
                stack.push(u);
            }
        }
    }
    // the small pieces join the piece next to them that faces most like them
    let mut size = vec![0usize; count as usize];
    for c in &chart {
        size[*c as usize] += 1;
    }
    for _ in 0..3 {
        let mut changed = false;
        for t in 0..n {
            let c = chart[t] as usize;
            if size[c] >= o.min_tris {
                continue;
            }
            let best = near[t].iter().filter(|u| chart[**u as usize] as usize != c).max_by(|a, b| dot(normals[**a as usize], normals[t]).total_cmp(&dot(normals[**b as usize], normals[t])));
            if let Some(&u) = best {
                let to = chart[u as usize];
                size[c] -= 1;
                size[to as usize] += 1;
                chart[t] = to;
                changed = true;
            }
        }
        if !changed {
            break;
        }
    }
    // the numbers of the pieces that are left, one after the other
    let mut renumber: HashMap<u32, u32> = HashMap::new();
    for c in chart.iter_mut() {
        let next = renumber.len() as u32;
        *c = *renumber.entry(*c).or_insert(next);
    }
    let count = renumber.len();
    (chart, count)
}

/// Lay out the pieces: the places of the corners of the triangles in the metres of the piece, and its box. Returns the layout of every piece.
struct Piece {
    tris: Vec<u32>,
    /// The metres of every corner: (corner of the mesh, x, y), relative to the lower left of the box.
    uv: HashMap<u32, [f32; 2]>,
    w: f32,
    h: f32,
}

fn flatten(mesh: &AssetMesh, normals: &[[f32; 3]], areas: &[f32], chart: &[u32], count: usize) -> Vec<Piece> {
    let mut members: Vec<Vec<u32>> = vec![Vec::new(); count];
    for (t, c) in chart.iter().enumerate() {
        members[*c as usize].push(t as u32);
    }
    members
        .into_iter()
        .map(|tris| {
            let mut sum = [0.0f32; 3];
            for &t in &tris {
                for k in 0..3 {
                    sum[k] += normals[t as usize][k] * areas[t as usize];
                }
            }
            let n = unit(sum);
            // the sides of the piece: the direction that is most "up" in the piece, so a standing body is laid standing
            let up = if n[1].abs() < 0.9 { [0.0, 1.0, 0.0] } else { [0.0, 0.0, 1.0] };
            let tx = unit(cross(up, n));
            let ty = cross(n, tx);
            let mut uv: HashMap<u32, [f32; 2]> = HashMap::new();
            let (mut lo, mut hi) = ([f32::MAX; 2], [f32::MIN; 2]);
            for &t in &tris {
                for &v in &mesh.tris[t as usize] {
                    let p = mesh.verts[v as usize];
                    let q = [dot(p, tx), dot(p, ty)];
                    uv.insert(v, q);
                    for k in 0..2 {
                        lo[k] = lo[k].min(q[k]);
                        hi[k] = hi[k].max(q[k]);
                    }
                }
            }
            for q in uv.values_mut() {
                q[0] -= lo[0];
                q[1] -= lo[1];
            }
            Piece { tris, uv, w: (hi[0] - lo[0]).max(1e-5), h: (hi[1] - lo[1]).max(1e-5) }
        })
        .collect()
}

/// Shelves, tallest first: where every piece goes (lower left corner, in units) when the pieces are `scale` times as large and `pad` apart, and whether it
/// all fits in the square.
fn pack(pieces: &[Piece], order: &[usize], scale: f32, pad: f32, height: f32) -> Option<Vec<[f32; 2]>> {
    let mut at = vec![[0.0f32; 2]; pieces.len()];
    let (mut x, mut y, mut row) = (pad, pad, 0.0f32);
    for &i in order {
        let (w, h) = (pieces[i].w * scale, pieces[i].h * scale);
        if x + w + pad > 1.0 {
            x = pad;
            y += row + pad;
            row = 0.0;
        }
        if y + h + pad > height || w + 2.0 * pad > 1.0 {
            return None;
        }
        at[i] = [x, y];
        x += w + pad;
        row = row.max(h);
    }
    Some(at)
}

/// The mesh with texture coordinates: the points on the border of two pieces are two points (one for each piece), so the triangles are the same ones.
pub fn unwrap(mesh: &AssetMesh, o: &Options) -> AssetMesh {
    let normals: Vec<[f32; 3]> = mesh.tris.iter().map(|t| unit(cross(sub(mesh.verts[t[1] as usize], mesh.verts[t[0] as usize]), sub(mesh.verts[t[2] as usize], mesh.verts[t[0] as usize])))).collect();
    let areas: Vec<f32> = mesh.tris.iter().map(|t| 0.5 * dot(cross(sub(mesh.verts[t[1] as usize], mesh.verts[t[0] as usize]), sub(mesh.verts[t[2] as usize], mesh.verts[t[0] as usize])), cross(sub(mesh.verts[t[1] as usize], mesh.verts[t[0] as usize]), sub(mesh.verts[t[2] as usize], mesh.verts[t[0] as usize]))).sqrt()).collect();
    let near = next_to(mesh);
    let (chart, count) = charts(mesh, &normals, &areas, &near, o);
    let pieces = flatten(mesh, &normals, &areas, &chart, count);
    let mut order: Vec<usize> = (0..pieces.len()).collect();
    order.sort_unstable_by(|a, b| pieces[*b].h.total_cmp(&pieces[*a].h));
    let pad = o.margin / o.size as f32;
    // the largest size of the pieces (units per metre) that still fits
    let (mut lo, mut hi) = (0.0f32, 4.0f32);
    for _ in 0..28 {
        let mid = (lo + hi) / 2.0;
        if pack(&pieces, &order, mid, pad, 1.0 - o.reserve).is_some() { lo = mid } else { hi = mid }
    }
    let scale = lo.max(1e-4);
    let at = pack(&pieces, &order, scale, pad, 1.0 - o.reserve).unwrap_or_else(|| vec![[0.0; 2]; pieces.len()]);
    let (mut verts, mut uvs, mut tris) = (Vec::with_capacity(mesh.verts.len() + mesh.verts.len() / 4), Vec::new(), Vec::with_capacity(mesh.tris.len()));
    for (pi, piece) in pieces.iter().enumerate() {
        let mut new_index: HashMap<u32, u32> = HashMap::with_capacity(piece.uv.len());
        for (&v, q) in &piece.uv {
            new_index.insert(v, verts.len() as u32);
            verts.push(mesh.verts[v as usize]);
            uvs.push([at[pi][0] + q[0] * scale, at[pi][1] + q[1] * scale]);
        }
        for &t in &piece.tris {
            let c = mesh.tris[t as usize];
            tris.push([new_index[&c[0]], new_index[&c[1]], new_index[&c[2]]]);
        }
    }
    AssetMesh { verts, uvs, tris, texture: None, normal: None }
}

/// How much of the square the pieces cover (0 to 1): the higher, the more of the texture is used.
pub fn coverage(mesh: &AssetMesh) -> f32 {
    mesh.tris
        .iter()
        .map(|t| {
            let (a, b, c) = (mesh.uvs[t[0] as usize], mesh.uvs[t[1] as usize], mesh.uvs[t[2] as usize]);
            0.5 * ((b[0] - a[0]) * (c[1] - a[1]) - (b[1] - a[1]) * (c[0] - a[0])).abs()
        })
        .sum()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A cylinder along y with a lid, as points and triangles only.
    fn cylinder() -> AssetMesh {
        let (n, rows) = (24u32, 12u32);
        let mut verts = Vec::new();
        for r in 0..=rows {
            for k in 0..n {
                let a = k as f32 / n as f32 * std::f32::consts::TAU;
                verts.push([a.cos() * 0.2, r as f32 * 0.15, a.sin() * 0.2]);
            }
        }
        let mut tris = Vec::new();
        for r in 0..rows {
            for k in 0..n {
                let (a, b) = (r * n + k, r * n + (k + 1) % n);
                tris.push([a, b, a + n]);
                tris.push([b, b + n, a + n]);
            }
        }
        AssetMesh { verts: verts.clone(), uvs: vec![[0.0; 2]; verts.len()], tris, texture: None, normal: None }
    }

    #[test]
    fn every_triangle_keeps_its_shape_and_lies_inside_the_square() {
        let m = cylinder();
        let u = unwrap(&m, &Options::default());
        assert_eq!(u.tris.len(), m.tris.len());
        assert!(u.verts.len() >= m.verts.len(), "the points on the cuts are doubled");
        // the same triangles (they are in the order of the pieces now)
        let canon = |mesh: &AssetMesh, t: &[u32; 3]| {
            let mut c: Vec<[i32; 3]> = t.iter().map(|i| { let p = mesh.verts[*i as usize]; [(p[0] * 1e4).round() as i32, (p[1] * 1e4).round() as i32, (p[2] * 1e4).round() as i32] }).collect();
            c.sort();
            c
        };
        let mut before: Vec<_> = m.tris.iter().map(|t| canon(&m, t)).collect();
        let mut after: Vec<_> = u.tris.iter().map(|t| canon(&u, t)).collect();
        before.sort();
        after.sort();
        assert_eq!(before, after);
        for t in &u.tris {
            for k in 0..3 {
                let q = u.uvs[t[k] as usize];
                assert!((0.0..=1.0).contains(&q[0]) && (0.0..=1.0).contains(&q[1]), "{q:?}");
            }
        }
    }

    #[test]
    fn the_round_body_is_cut_into_several_pieces_and_the_pieces_do_not_overlap() {
        let m = cylinder();
        let u = unwrap(&m, &Options::default());
        assert!(u.verts.len() > m.verts.len(), "a cylinder cannot be laid flat in one piece");
        // the triangles cover the pieces without lying on each other: the area they cover adds up to no more than the square
        let c = coverage(&u);
        assert!(c > 0.05 && c < 1.0, "{c}");
        // sample the square: no point of it is in two triangles
        let n = 160;
        let mut hit = vec![0u8; n * n];
        for t in &u.tris {
            let (a, b, cc) = (u.uvs[t[0] as usize], u.uvs[t[1] as usize], u.uvs[t[2] as usize]);
            let (x0, x1) = (a[0].min(b[0]).min(cc[0]), a[0].max(b[0]).max(cc[0]));
            let (y0, y1) = (a[1].min(b[1]).min(cc[1]), a[1].max(b[1]).max(cc[1]));
            for iy in ((y0 * n as f32) as usize)..=((y1 * n as f32) as usize).min(n - 1) {
                for ix in ((x0 * n as f32) as usize)..=((x1 * n as f32) as usize).min(n - 1) {
                    let (px, py) = ((ix as f32 + 0.5) / n as f32, (iy as f32 + 0.5) / n as f32);
                    let area = (b[0] - a[0]) * (cc[1] - a[1]) - (b[1] - a[1]) * (cc[0] - a[0]);
                    let w0 = ((b[0] - px) * (cc[1] - py) - (b[1] - py) * (cc[0] - px)) / area;
                    let w1 = ((cc[0] - px) * (a[1] - py) - (cc[1] - py) * (a[0] - px)) / area;
                    if w0 > 0.02 && w1 > 0.02 && w0 + w1 < 0.98 {
                        hit[iy * n + ix] += 1;
                    }
                }
            }
        }
        assert!(hit.iter().all(|h| *h <= 1), "two triangles are on the same place of the texture");
    }

    #[test]
    fn a_flat_square_is_one_piece() {
        let verts = vec![[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [1.0, 1.0, 0.0], [0.0, 1.0, 0.0]];
        let m = AssetMesh { verts, uvs: vec![[0.0; 2]; 4], tris: vec![[0, 1, 2], [0, 2, 3]], texture: None, normal: None };
        let u = unwrap(&m, &Options::default());
        assert_eq!(u.verts.len(), 4, "no cut");
    }

    /// A real base: how long it takes and what the pieces look like with a checker pattern. Run by hand: `cargo test --release unwrap_a_real_base -- --ignored --nocapture`.
    #[test]
    #[ignore]
    fn unwrap_a_real_base() {
        let name = std::env::var("BASE").unwrap_or("Midlanderbasefemale".into());
        let path = format!("C:/Users/SaphiresSilver/Downloads/Bases/Readybases/Races/{name}.glb");
        let mesh = AssetMesh::from_glb(&std::fs::read(path).unwrap()).unwrap();
        let start = std::time::Instant::now();
        let mut u = unwrap(&mesh, &Options::default());
        println!("{} tris, {} -> {} points, coverage {:.2}, {:?}", u.tris.len(), mesh.verts.len(), u.verts.len(), coverage(&u), start.elapsed());
        let mut tex = image::RgbaImage::new(512, 512);
        for (x, y, p) in tex.enumerate_pixels_mut() {
            *p = if (x / 16 + y / 16) % 2 == 0 { image::Rgba([230, 230, 230, 255]) } else { image::Rgba([60, 90, 200, 255]) };
        }
        u.texture = Some(tex);
        let t0 = std::time::Instant::now();
        let img = u.render_view(0.0, 0.0, 700, 900, false, crate::assetview::View::Textured);
        println!("render {:?}", t0.elapsed());
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("target").join("helpwork");
        img.save(dir.join(format!("unwrap_{name}.png"))).unwrap();
        let mut sheet = image::RgbaImage::from_pixel(512, 512, image::Rgba([20, 20, 20, 255]));
        for t in &u.tris {
            for k in 0..3 {
                let (a, b) = (u.uvs[t[k] as usize], u.uvs[t[(k + 1) % 3] as usize]);
                for s in 0..8 {
                    let f = s as f32 / 8.0;
                    let (x, y) = (((a[0] + (b[0] - a[0]) * f) * 511.0) as u32, ((1.0 - (a[1] + (b[1] - a[1]) * f)) * 511.0) as u32);
                    sheet.put_pixel(x.min(511), y.min(511), image::Rgba([90, 220, 160, 255]));
                }
            }
        }
        sheet.save(dir.join(format!("unwrap_{name}_uv.png"))).unwrap();
    }

    #[test]
    fn a_strip_of_the_square_can_be_kept_free() {
        let m = cylinder();
        let u = unwrap(&m, &Options { reserve: 0.125, ..Options::default() });
        let top = u.uvs.iter().map(|q| q[1]).fold(0.0f32, f32::max);
        assert!(top <= 0.875 + 1e-4, "{top}");
    }
}
