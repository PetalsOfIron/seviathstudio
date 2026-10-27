// SPDX-License-Identifier: GPL-3.0-or-later
// A textured 3D prop (a game asset made by the Generate tab) as a picture you can turn: reads the .glb the AI helper wrote (points,
// triangles, texture coordinates and the texture), and draws it with a small software renderer (depth buffer, texture, one light),
// from any side, with the option to see the triangles. Also the numbers a game maker wants: triangles, size, texture.

use image::{Rgba, RgbaImage};

use crate::vrm::gltf::read_glb;

#[derive(Clone, Debug)]
pub struct AssetMesh {
    pub verts: Vec<[f32; 3]>,
    pub uvs: Vec<[f32; 2]>,
    pub tris: Vec<[u32; 3]>,
    pub texture: Option<RgbaImage>,
    /// The tangent space normal map that was baked from the fine mesh (it keeps the shape that the reduction of the triangles took away).
    pub normal: Option<RgbaImage>,
}

/// How the model is drawn.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum View {
    /// With its colours and a light.
    #[default]
    Textured,
    /// The direction of the surface in colours (red to the right, green up, blue to the viewer): a normal map of this view.
    Normals,
    /// How far every point is: white is near, black is far.
    Depth,
}

/// What a game maker wants to know.
#[derive(Clone, Debug, PartialEq)]
pub struct AssetStats {
    pub triangles: usize,
    pub vertices: usize,
    /// Width, height and depth in the units of the file (metres).
    pub size: [f32; 3],
    pub texture: Option<(u32, u32)>,
    pub normal_map: bool,
}

impl AssetMesh {
    pub fn from_glb(bytes: &[u8]) -> Result<AssetMesh, String> {
        let g = read_glb(bytes)?;
        let (mut verts, mut uvs, mut tris): (Vec<[f32; 3]>, Vec<[f32; 2]>, Vec<[u32; 3]>) = (Vec::new(), Vec::new(), Vec::new());
        for m in g.json["meshes"].as_array().ok_or("The file has no meshes.")? {
            for p in m["primitives"].as_array().ok_or("A mesh has no primitives.")? {
                let pos = g.vec3s(p["attributes"]["POSITION"].as_u64().ok_or("A mesh has no positions.")? as usize)?;
                let uv = match p["attributes"]["TEXCOORD_0"].as_u64() {
                    Some(a) => g.vec2s(a as usize)?,
                    None => vec![[0.0, 0.0]; pos.len()],
                };
                if uv.len() != pos.len() {
                    return Err("The texture coordinates do not fit the points.".into());
                }
                let base = verts.len() as u32;
                let idx = match p["indices"].as_u64() {
                    Some(i) => g.index_list(i as usize)?,
                    None => (0..pos.len() as u32).collect(),
                };
                if idx.iter().any(|i| *i as usize >= pos.len()) {
                    return Err("A triangle points past the last point.".into());
                }
                tris.extend(idx.chunks_exact(3).map(|c| [base + c[0], base + c[1], base + c[2]]));
                verts.extend(pos);
                uvs.extend(uv);
            }
        }
        if verts.is_empty() || tris.is_empty() || verts.iter().any(|v| v.iter().any(|x| !x.is_finite())) {
            return Err("The model is empty or has points that are not numbers.".into());
        }
        let image_of = |tex: Option<u64>| -> Option<RgbaImage> {
            let source = g.json["textures"].get(tex? as usize)?["source"].as_u64()?;
            image::load_from_memory(g.image(source as usize).ok()?).ok().map(|i| i.to_rgba8())
        };
        let material = &g.json["materials"][0];
        let colour_tex = material["pbrMetallicRoughness"]["baseColorTexture"]["index"].as_u64().or(Some(0));
        let texture = g.json["images"].as_array().filter(|a| !a.is_empty()).and_then(|_| image_of(colour_tex));
        let normal = image_of(material["normalTexture"]["index"].as_u64());
        Ok(AssetMesh { verts, uvs, tris, texture, normal })
    }

    pub fn stats(&self) -> AssetStats {
        let (lo, hi) = self.bounds();
        AssetStats { triangles: self.tris.len(), vertices: self.verts.len(), size: [hi[0] - lo[0], hi[1] - lo[1], hi[2] - lo[2]], texture: self.texture.as_ref().map(|t| t.dimensions()), normal_map: self.normal.is_some() }
    }

    fn bounds(&self) -> ([f32; 3], [f32; 3]) {
        let (mut lo, mut hi) = ([f32::MAX; 3], [f32::MIN; 3]);
        for v in &self.verts {
            for k in 0..3 {
                lo[k] = lo[k].min(v[k]);
                hi[k] = hi[k].max(v[k]);
            }
        }
        (lo, hi)
    }

    /// The model from a side: `yaw` turns it round the up axis, `pitch` tips it forward (degrees). `wire` draws the triangle edges.
    pub fn render(&self, yaw: f32, pitch: f32, w: u32, h: u32, wire: bool) -> RgbaImage {
        self.render_view(yaw, pitch, w, h, wire, View::Textured)
    }

    /// The same, as colours, as the directions of the surface, or as depth.
    pub fn render_view(&self, yaw: f32, pitch: f32, w: u32, h: u32, wire: bool, view: View) -> RgbaImage {
        let background = match view {
            View::Textured => Rgba([40, 42, 48, 255]),
            View::Normals => Rgba([128, 128, 255, 255]),
            View::Depth => Rgba([0, 0, 0, 255]),
        };
        let mut img = RgbaImage::from_pixel(w, h, background);
        let mut depth = vec![f32::MAX; (w * h) as usize];
        let (lo, hi) = self.bounds();
        let centre = [(lo[0] + hi[0]) / 2.0, (lo[1] + hi[1]) / 2.0, (lo[2] + hi[2]) / 2.0];
        let radius = (0..3).map(|k| (hi[k] - lo[k]).powi(2)).sum::<f32>().sqrt().max(1e-6) / 2.0;
        let (sy, cy) = yaw.to_radians().sin_cos();
        let (sp, cp) = pitch.to_radians().sin_cos();
        let scale = (w.min(h) as f32) * 0.46 / radius;
        // the point on the screen and its depth (smaller is nearer)
        let project = |v: [f32; 3]| -> [f32; 3] {
            let (x, y, z) = (v[0] - centre[0], v[1] - centre[1], v[2] - centre[2]);
            let (x1, z1) = (x * cy + z * sy, -x * sy + z * cy);
            let (y2, z2) = (y * cp - z1 * sp, y * sp + z1 * cp);
            [w as f32 / 2.0 + x1 * scale, h as f32 / 2.0 - y2 * scale, -z2 * scale]
        };
        let pts: Vec<[f32; 3]> = self.verts.iter().map(|v| project(*v)).collect();
        let (zmin, zmax) = pts.iter().fold((f32::MAX, f32::MIN), |(lo, hi), p| (lo.min(p[2]), hi.max(p[2])));
        let light = {
            let l = [0.4f32, 0.6, 0.7];
            let n = (l[0] * l[0] + l[1] * l[1] + l[2] * l[2]).sqrt();
            [l[0] / n, l[1] / n, l[2] / n]
        };
        for t in &self.tris {
            let (a, b, c) = (pts[t[0] as usize], pts[t[1] as usize], pts[t[2] as usize]);
            // the face's own direction (in the turned space), for the light, and which side faces the viewer
            let (e1, e2) = ([b[0] - a[0], b[1] - a[1], b[2] - a[2]], [c[0] - a[0], c[1] - a[1], c[2] - a[2]]);
            let area = e1[0] * e2[1] - e1[1] * e2[0];
            if area.abs() < 1e-6 {
                continue;
            }
            let mut nrm = [e1[1] * e2[2] - e1[2] * e2[1], e1[2] * e2[0] - e1[0] * e2[2], area];
            // the direction of the surface as a viewer sees it: red to the right, green up, blue towards the viewer
            let view_normal = {
                let l = (nrm[0] * nrm[0] + nrm[1] * nrm[1] + nrm[2] * nrm[2]).sqrt().max(1e-9);
                let n = [nrm[0] / l, -nrm[1] / l, -nrm[2] / l];
                if n[2] < 0.0 { [-n[0], -n[1], -n[2]] } else { n }
            };
            let len = (nrm[0] * nrm[0] + nrm[1] * nrm[1] + nrm[2] * nrm[2]).sqrt().max(1e-9);
            // the screen has y downwards: turn the direction so that the one that faces the viewer is positive
            nrm = [nrm[0] / len, -nrm[1] / len, -nrm[2] / len];
            let facing = if area < 0.0 { 1.0 } else { -1.0 };
            let shade = 0.35 + 0.65 * (facing * (nrm[0] * light[0] + nrm[1] * light[1] + nrm[2] * light[2])).abs();
            let (minx, maxx) = (a[0].min(b[0]).min(c[0]).floor().max(0.0) as i32, a[0].max(b[0]).max(c[0]).ceil().min(w as f32 - 1.0) as i32);
            let (miny, maxy) = (a[1].min(b[1]).min(c[1]).floor().max(0.0) as i32, a[1].max(b[1]).max(c[1]).ceil().min(h as f32 - 1.0) as i32);
            let (ua, ub, uc) = (self.uvs[t[0] as usize], self.uvs[t[1] as usize], self.uvs[t[2] as usize]);
            for y in miny..=maxy {
                for x in minx..=maxx {
                    let (px, py) = (x as f32 + 0.5, y as f32 + 0.5);
                    let w0 = ((b[0] - px) * (c[1] - py) - (b[1] - py) * (c[0] - px)) / area;
                    let w1 = ((c[0] - px) * (a[1] - py) - (c[1] - py) * (a[0] - px)) / area;
                    let w2 = 1.0 - w0 - w1;
                    if w0 < -0.001 || w1 < -0.001 || w2 < -0.001 {
                        continue;
                    }
                    let z = w0 * a[2] + w1 * b[2] + w2 * c[2];
                    let at = (y as u32 * w + x as u32) as usize;
                    if z >= depth[at] {
                        continue;
                    }
                    depth[at] = z;
                    if view != View::Textured {
                        let c = match view {
                            View::Normals => [((view_normal[0] * 0.5 + 0.5) * 255.0) as u8, ((view_normal[1] * 0.5 + 0.5) * 255.0) as u8, ((view_normal[2] * 0.5 + 0.5) * 255.0) as u8],
                            _ => {
                                let d = 255.0 * (1.0 - (z - zmin) / (zmax - zmin).max(1e-6));
                                [d as u8; 3]
                            }
                        };
                        img.put_pixel(x as u32, y as u32, Rgba([c[0], c[1], c[2], 255]));
                        continue;
                    }
                    let base = match &self.texture {
                        Some(tex) => {
                            let (u, v) = (w0 * ua[0] + w1 * ub[0] + w2 * uc[0], w0 * ua[1] + w1 * ub[1] + w2 * uc[1]);
                            let (tx, ty) = (((u.rem_euclid(1.0)) * (tex.width() - 1) as f32) as u32, ((v.rem_euclid(1.0)) * (tex.height() - 1) as f32) as u32);
                            tex.get_pixel(tx, ty).0
                        }
                        None => [190, 190, 195, 255],
                    };
                    img.put_pixel(x as u32, y as u32, Rgba([(base[0] as f32 * shade) as u8, (base[1] as f32 * shade) as u8, (base[2] as f32 * shade) as u8, 255]));
                }
            }
        }
        if wire {
            for t in &self.tris {
                for k in 0..3 {
                    let (p, q) = (pts[t[k] as usize], pts[t[(k + 1) % 3] as usize]);
                    let steps = ((q[0] - p[0]).abs().max((q[1] - p[1]).abs()) as usize).clamp(1, 400);
                    for s in 0..=steps {
                        let f = s as f32 / steps as f32;
                        let (x, y, z) = (p[0] + (q[0] - p[0]) * f, p[1] + (q[1] - p[1]) * f, p[2] + (q[2] - p[2]) * f);
                        if x >= 0.0 && y >= 0.0 && (x as u32) < w && (y as u32) < h && z <= depth[(y as u32 * w + x as u32) as usize] + 0.02 * radius * scale {
                            img.put_pixel(x as u32, y as u32, Rgba([20, 220, 160, 255]));
                        }
                    }
                }
            }
        }
        img
    }
}

#[cfg(test)]
pub fn tests_box_glb() -> Vec<u8> {
    tests::box_glb()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::vrm::gltf::Builder;
    use serde_json::json;

    /// A box with a two-colour texture, as the helper writes it.
    pub(crate) fn box_glb() -> Vec<u8> {
        let mut b = Builder::new("test");
        let v: Vec<[f32; 3]> = vec![[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [1.0, 2.0, 0.0], [0.0, 2.0, 0.0], [0.0, 0.0, 1.0], [1.0, 0.0, 1.0], [1.0, 2.0, 1.0], [0.0, 2.0, 1.0]];
        let uv: Vec<[f32; 2]> = v.iter().map(|p| [p[0] * 0.5 + p[2] * 0.5, p[1] / 2.0]).collect();
        let t: Vec<[u32; 3]> = vec![[0, 2, 1], [0, 3, 2], [4, 5, 6], [4, 6, 7], [0, 4, 7], [0, 7, 3], [1, 2, 6], [1, 6, 5], [3, 7, 6], [3, 6, 2], [0, 1, 5], [0, 5, 4]];
        let pos = b.vec3s(&v, true);
        let tc = b.vec2s(&uv);
        let ix = b.indices(&t);
        let mut tex = RgbaImage::new(8, 8);
        for (x, _, p) in tex.enumerate_pixels_mut() {
            *p = if x < 4 { Rgba([220, 40, 40, 255]) } else { Rgba([40, 40, 220, 255]) };
        }
        let mut png = Vec::new();
        tex.write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png).unwrap();
        b.png_texture(&png, "tex");
        // a flat normal map (pointing straight out) as the second picture
        let flat = RgbaImage::from_pixel(4, 4, Rgba([128, 128, 255, 255]));
        let mut npng = Vec::new();
        flat.write_to(&mut std::io::Cursor::new(&mut npng), image::ImageFormat::Png).unwrap();
        b.png_texture(&npng, "normal");
        b.materials.push(json!({ "pbrMetallicRoughness": { "baseColorTexture": { "index": 0 } }, "normalTexture": { "index": 1 } }));
        b.meshes.push(json!({ "primitives": [{ "attributes": { "POSITION": pos, "TEXCOORD_0": tc }, "indices": ix, "material": 0 }] }));
        b.nodes.push(json!({ "mesh": 0 }));
        b.scenes.push(json!({ "nodes": [0] }));
        b.glb()
    }

    #[test]
    fn a_textured_box_reads_and_has_the_right_numbers() {
        let m = AssetMesh::from_glb(&box_glb()).unwrap();
        let s = m.stats();
        assert_eq!((s.triangles, s.vertices), (12, 8));
        assert_eq!(s.size, [1.0, 2.0, 1.0]);
        assert_eq!(s.texture, Some((8, 8)));
        assert!(s.normal_map, "the normal map is found by the material, not by its place");
        assert_eq!(m.normal.as_ref().unwrap().dimensions(), (4, 4));
    }

    #[test]
    fn the_normals_and_the_depth_of_a_view_are_pictures_of_the_shape() {
        let m = AssetMesh::from_glb(&box_glb()).unwrap();
        // from the front the box shows one face: its normal points at the viewer (blue), and the model is all at one depth
        let n = m.render_view(0.0, 0.0, 120, 120, false, View::Normals);
        let mid = n.get_pixel(60, 60).0;
        assert!(mid[2] > 240 && (mid[0] as i32 - 128).abs() < 20 && (mid[1] as i32 - 128).abs() < 20, "{mid:?}");
        assert_eq!(n.get_pixel(0, 0).0, [128, 128, 255, 255], "the background is a flat normal");
        // tipped over, a face that looks up or down shows (green far from the middle), and turned, one that looks to the side (red)
        let tilted = m.render_view(40.0, 30.0, 160, 160, false, View::Normals);
        assert!(tilted.pixels().any(|p| (p.0[1] as i32 - 128).abs() > 70), "a face that looks up or down");
        assert!(tilted.pixels().any(|p| (p.0[0] as i32 - 128).abs() > 70), "a face that looks to the side");
        // depth: nearer is brighter; turned, one corner is nearer than the other
        let d = m.render_view(40.0, 0.0, 160, 160, false, View::Depth);
        let lit: Vec<u8> = d.pixels().filter(|p| p.0[0] > 0).map(|p| p.0[0]).collect();
        assert!(lit.len() > 1000 && lit.iter().max().unwrap() - lit.iter().min().unwrap() > 40, "depth varies across a turned box");
        assert_eq!(d.get_pixel(0, 0).0, [0, 0, 0, 255]);
    }

    #[test]
    fn the_box_is_drawn_with_its_texture_from_every_side_and_not_beyond_its_outline() {
        let m = AssetMesh::from_glb(&box_glb()).unwrap();
        for yaw in [0.0, 90.0, 180.0, 270.0] {
            let img = m.render(yaw, 15.0, 160, 160, false);
            let bg = Rgba([40, 42, 48, 255]);
            let drawn = img.pixels().filter(|p| **p != bg).count();
            assert!(drawn > 1500, "yaw {yaw}: {drawn} pixels");
            assert!(img.get_pixel(0, 0) == &bg && img.get_pixel(159, 159) == &bg, "the corners stay empty");
            // the texture is red on one half and blue on the other: both show somewhere (the box is turned so that its faces differ)
        }
        let img = m.render(30.0, 10.0, 160, 160, false);
        let red = img.pixels().filter(|p| p.0[0] > 100 && p.0[2] < 90).count();
        let blue = img.pixels().filter(|p| p.0[2] > 100 && p.0[0] < 90).count();
        assert!(red > 100 && blue > 100, "red {red}, blue {blue}");
        let wired = m.render(30.0, 10.0, 160, 160, true);
        assert!(wired.pixels().any(|p| p.0 == [20, 220, 160, 255]), "the triangle edges show");
    }

    #[test]
    fn broken_files_are_refused() {
        assert!(AssetMesh::from_glb(b"nothing").is_err());
    }
}

/// Looks at the two AI shapes of the example girl, side by side, when the files are on this computer:
/// `cargo test --release compare_the_two_shapes -- --ignored`, picture in target/screens/shapes_compare.png.
#[test]
#[ignore]
fn compare_the_two_shapes() {
    let files = [("TripoSR", r"C:\Users\SaphiresSilver\AppData\Roaming\SeviathStudio\ai\shape.glb"), ("CharacterGen", r"C:\Users\SaphiresSilver\Projects\seviathstudio-rs\target\gen\cg_shape.glb")];
    let mut sheet = image::RgbaImage::new(300 * 4, 440 * 4);
    for (row, (name, path)) in files.iter().enumerate() {
        let Ok(bytes) = std::fs::read(path) else { continue };
        let m = AssetMesh::from_glb(&bytes).unwrap();
        let st = m.stats();
        println!("{name}: {} triangles, size {:?}", st.triangles, st.size);
        for (k, yaw) in [0.0f32, 45.0, 90.0, 180.0].into_iter().enumerate() {
            for (r2, view) in [View::Normals, View::Depth].into_iter().enumerate() {
                let img = m.render_view(yaw, 0.0, 300, 440, false, view);
                image::imageops::replace(&mut sheet, &img, (k as u32 * 300) as i64, ((row * 2 + r2) as u32 * 440) as i64);
            }
        }
    }
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("target").join("screens");
    std::fs::create_dir_all(&dir).unwrap();
    sheet.save(dir.join("shapes_compare.png")).unwrap();
}
