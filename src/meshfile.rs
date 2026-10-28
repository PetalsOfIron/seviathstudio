// SPDX-License-Identifier: GPL-3.0-or-later
// The file of a 3D mesh with the rig that was made by hand: a glTF binary (.glb) with the mesh, its picture (and normal map), the bones as nodes,
// the skin (which bones, where they were at rest, and the weight of every point) and, when asked, animations for the bones that have the names
// of a humanoid (the ready motions of `motion.rs`). Game engines and Blender open it. And the way back: the rig is read from such a file.

use serde_json::json;

use crate::assetview::AssetMesh;
use crate::motion::Clip;
use crate::rig3d::{Bone3, Rig3};
use crate::vrm::gltf::{Builder, read_glb};

fn png_bytes(img: &image::RgbaImage) -> Vec<u8> {
    let mut out = std::io::Cursor::new(Vec::new());
    let _ = img.write_to(&mut out, image::ImageFormat::Png);
    out.into_inner()
}

/// The smooth normal of every point (the points that are the same place share it, so a seam does not show in the light).
fn vertex_normals(verts: &[[f32; 3]], tris: &[[u32; 3]]) -> Vec<[f32; 3]> {
    let key = |p: [f32; 3]| ((p[0] * 4000.0).round() as i32, (p[1] * 4000.0).round() as i32, (p[2] * 4000.0).round() as i32);
    let mut sum: std::collections::HashMap<(i32, i32, i32), [f32; 3]> = std::collections::HashMap::new();
    for t in tris {
        let (a, b, c) = (verts[t[0] as usize], verts[t[1] as usize], verts[t[2] as usize]);
        let (u, v) = ([b[0] - a[0], b[1] - a[1], b[2] - a[2]], [c[0] - a[0], c[1] - a[1], c[2] - a[2]]);
        let n = [u[1] * v[2] - u[2] * v[1], u[2] * v[0] - u[0] * v[2], u[0] * v[1] - u[1] * v[0]];
        for i in t {
            let e = sum.entry(key(verts[*i as usize])).or_insert([0.0; 3]);
            for k in 0..3 {
                e[k] += n[k];
            }
        }
    }
    verts
        .iter()
        .map(|p| {
            let n = sum.get(&key(*p)).copied().unwrap_or([0.0, 1.0, 0.0]);
            let l = (n[0] * n[0] + n[1] * n[1] + n[2] * n[2]).sqrt().max(1e-9);
            [n[0] / l, n[1] / l, n[2] / l]
        })
        .collect()
}

/// Are the arms of the rig hanging down (a relaxed stand) and not out to the sides (a T-pose)? Decides what the ready animations are made relative to.
pub fn arms_hang_down(rig: &Rig3) -> bool {
    let arm = rig.bones.iter().find(|b| b.name.ends_with("UpperArm") || b.name.starts_with("UpperArm") || b.name.contains("upperarm"));
    arm.is_some_and(|b| {
        let (dx, dy) = ((b.tail[0] - b.head[0]).abs(), b.head[1] - b.tail[1]);
        dy > dx * 0.5
    })
}

/// The glb of a mesh, with a rig (and animations for it) when it has one.
pub fn write_glb(mesh: &AssetMesh, rig: Option<&Rig3>, clips: &[Clip], fps: f32) -> Vec<u8> {
    let mut b = Builder::new("SeviathStudio");
    let nb = rig.map(|r| r.bones.len()).unwrap_or(0);
    // the bones come first: node i is bone i
    if let Some(r) = rig {
        for (i, bone) in r.bones.iter().enumerate() {
            let up = bone.parent.map(|p| r.bones[p].head).unwrap_or([0.0; 3]);
            let mut n = json!({ "name": bone.name, "translation": [bone.head[0] - up[0], bone.head[1] - up[1], bone.head[2] - up[2]] });
            let kids = r.children(i);
            if !kids.is_empty() {
                n["children"] = json!(kids);
            }
            b.nodes.push(n);
        }
    }
    let pos = b.vec3s(&mesh.verts, true);
    let nor = b.vec3s(&vertex_normals(&mesh.verts, &mesh.tris), false);
    let uv = mesh.uvs.clone();
    let tc = b.vec2s(&uv);
    let idx = b.indices(&mesh.tris);
    let mut attrs = json!({ "POSITION": pos, "NORMAL": nor, "TEXCOORD_0": tc });
    if let Some(r) = rig.filter(|r| !r.bones.is_empty() && r.weights.len() == mesh.verts.len()) {
        let mut joints: Vec<[u16; 4]> = Vec::with_capacity(mesh.verts.len());
        let mut weights: Vec<[f32; 4]> = Vec::with_capacity(mesh.verts.len());
        for w in &r.weights {
            let sum: f32 = w.iter().map(|x| x.1).sum();
            let (mut j, mut x) = ([0u16; 4], [0.0f32; 4]);
            if sum > 1e-9 {
                for (k, (bone, wt)) in w.iter().take(4).enumerate() {
                    j[k] = *bone;
                    x[k] = wt / sum;
                }
            } else {
                // nothing moves it: it follows the first bone, so it is still in the file
                x[0] = 1.0;
            }
            joints.push(j);
            weights.push(x);
        }
        attrs["JOINTS_0"] = json!(b.joints(&joints));
        attrs["WEIGHTS_0"] = json!(b.vec4s(&weights));
    }
    let mut material = json!({ "name": "Surface", "pbrMetallicRoughness": { "metallicFactor": 0.0, "roughnessFactor": 0.85 }, "doubleSided": false });
    if let Some(t) = &mesh.texture {
        let tex = b.png_texture(&png_bytes(t), "colour");
        material["pbrMetallicRoughness"]["baseColorTexture"] = json!({ "index": tex });
    }
    if let Some(n) = &mesh.normal {
        let tex = b.png_texture(&png_bytes(n), "normal");
        material["normalTexture"] = json!({ "index": tex });
    }
    b.materials.push(material);
    b.meshes.push(json!({ "primitives": [{ "attributes": attrs, "indices": idx, "material": 0, "mode": 4 }] }));
    let mut mesh_node = json!({ "name": "Mesh", "mesh": 0 });
    let roots: Vec<usize> = rig.map(|r| (0..nb).filter(|i| r.bones[*i].parent.is_none()).collect()).unwrap_or_default();
    if let Some(r) = rig.filter(|r| !r.bones.is_empty()) {
        let inverse: Vec<[f32; 16]> = r.bones.iter().map(|bone| [1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, -bone.head[0], -bone.head[1], -bone.head[2], 1.0]).collect();
        let ibm = b.mat4s(&inverse);
        b.skins.push(json!({ "joints": (0..nb).collect::<Vec<_>>(), "inverseBindMatrices": ibm, "skeleton": roots.first().copied().unwrap_or(0) }));
        mesh_node["skin"] = json!(0);
    }
    b.nodes.push(mesh_node);
    let mesh_index = b.nodes.len() - 1;
    let mut scene: Vec<usize> = roots.clone();
    scene.push(mesh_index);
    b.scenes.push(json!({ "nodes": scene }));
    // animations, for the bones that have the name of a humanoid bone
    if let Some(r) = rig.filter(|r| !clips.is_empty() && !r.bones.is_empty()) {
        let relative = arms_hang_down(r);
        let targets: Vec<crate::vrm::vrma::Target> = crate::motion::BONES
            .iter()
            .filter_map(|name| {
                let i = r.bones.iter().position(|bn| bn.name.eq_ignore_ascii_case(name))?;
                let up = r.bones[i].parent.map(|p| r.bones[p].head).unwrap_or([0.0; 3]);
                let h = r.bones[i].head;
                Some(crate::vrm::vrma::Target { bone: name, node: i, rest: [h[0] - up[0], h[1] - up[1], h[2] - up[2]] })
            })
            .collect();
        for c in clips {
            let frames: Vec<crate::motion::Frame> = if relative { c.frames(fps).iter().map(|f| f.minus_rest()).collect() } else { c.frames(fps) };
            crate::vrm::vrma::add_animation(&mut b, &c.name, fps, &frames, &targets, false);
        }
    }
    b.glb()
}

/// The rig that is in a glb (its skin: the bones, and the weight of every point), or None when it has none. The tail of a bone is where its first child is
/// (a bone with no child points on, a tenth of a metre, the way its parent points).
pub fn rig_from_glb(bytes: &[u8]) -> Option<Rig3> {
    let g = read_glb(bytes).ok()?;
    let skin = g.json["skins"].get(0)?;
    let joints: Vec<usize> = skin["joints"].as_array()?.iter().filter_map(|j| j.as_u64().map(|x| x as usize)).collect();
    let nodes = g.json["nodes"].as_array()?;
    if joints.is_empty() {
        return None;
    }
    // the place of every node: the translations added up from the top
    let mut parent_of = vec![None; nodes.len()];
    for (i, n) in nodes.iter().enumerate() {
        for c in n["children"].as_array().into_iter().flatten().filter_map(|c| c.as_u64()) {
            if (c as usize) < nodes.len() {
                parent_of[c as usize] = Some(i);
            }
        }
    }
    let local = |i: usize| -> [f32; 3] {
        let t = &nodes[i]["translation"];
        [t[0].as_f64().unwrap_or(0.0) as f32, t[1].as_f64().unwrap_or(0.0) as f32, t[2].as_f64().unwrap_or(0.0) as f32]
    };
    let world = |mut i: usize| -> [f32; 3] {
        let mut p = local(i);
        let mut steps = 0;
        while let Some(up) = parent_of[i] {
            let l = local(up);
            p = [p[0] + l[0], p[1] + l[1], p[2] + l[2]];
            i = up;
            steps += 1;
            if steps > nodes.len() {
                break;
            }
        }
        p
    };
    let index_of: std::collections::HashMap<usize, usize> = joints.iter().enumerate().map(|(k, j)| (*j, k)).collect();
    let mut bones: Vec<Bone3> = joints
        .iter()
        .map(|&j| Bone3 { name: nodes[j]["name"].as_str().unwrap_or("bone").to_owned(), parent: parent_of[j].and_then(|p| index_of.get(&p).copied()), head: world(j), tail: world(j) })
        .collect();
    for k in 0..bones.len() {
        let kids: Vec<usize> = (0..bones.len()).filter(|c| bones[*c].parent == Some(k)).collect();
        bones[k].tail = match kids.first() {
            Some(c) => bones[*c].head,
            None => {
                let up = bones[k].parent.map(|p| bones[p].head).unwrap_or([bones[k].head[0], bones[k].head[1] - 0.1, bones[k].head[2]]);
                let d = [bones[k].head[0] - up[0], bones[k].head[1] - up[1], bones[k].head[2] - up[2]];
                let l = (d[0] * d[0] + d[1] * d[1] + d[2] * d[2]).sqrt().max(1e-6);
                [bones[k].head[0] + d[0] / l * 0.1, bones[k].head[1] + d[1] / l * 0.1, bones[k].head[2] + d[2] / l * 0.1]
            }
        };
    }
    // the weights of the first mesh that has them
    let prim = g.json["meshes"].as_array()?.iter().flat_map(|m| m["primitives"].as_array().cloned().unwrap_or_default()).find(|p| p["attributes"]["JOINTS_0"].is_u64())?;
    let j = g.index_list(prim["attributes"]["JOINTS_0"].as_u64()? as usize).ok()?;
    let w = g.floats(prim["attributes"]["WEIGHTS_0"].as_u64()? as usize).ok()?;
    let count = j.len() / 4;
    let weights: Vec<Vec<(u16, f32)>> = (0..count).map(|i| (0..4).filter(|k| w[i * 4 + k] > 1e-5 && (j[i * 4 + k] as usize) < bones.len()).map(|k| (j[i * 4 + k] as u16, w[i * 4 + k])).collect()).collect();
    Some(Rig3 { bones, weights })
}

#[cfg(test)]
pub fn tests_figure() -> (AssetMesh, Rig3) {
    tests::figure()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::motion::{Motion, Style};

    /// A tall box with a texture, and a rig of three bones along it.
    pub(crate) fn figure() -> (AssetMesh, Rig3) {
        let mut verts = Vec::new();
        for i in 0..=10 {
            let y = i as f32 * 0.2;
            for (x, z) in [(-0.1, -0.1), (0.1, -0.1), (0.1, 0.1), (-0.1, 0.1)] {
                verts.push([x, y, z]);
            }
        }
        let mut tris = Vec::new();
        for i in 0..10u32 {
            let (a, b) = (i * 4, (i + 1) * 4);
            for k in 0..4 {
                let (k1, k2) = (k, (k + 1) % 4);
                tris.push([a + k1, a + k2, b + k1]);
                tris.push([a + k2, b + k2, b + k1]);
            }
        }
        let uvs = verts.iter().map(|v| [v[0] + 0.5, v[1] / 2.0]).collect();
        let mesh = AssetMesh { verts, uvs, tris, texture: Some(image::RgbaImage::from_pixel(4, 4, image::Rgba([200, 100, 50, 255]))), normal: Some(image::RgbaImage::from_pixel(4, 4, image::Rgba([128, 128, 255, 255]))) };
        let mut rig = Rig3::new(mesh.verts.len());
        let hips = rig.add_bone("hips", None, [0.0, 1.0, 0.0], [0.0, 1.2, 0.0]);
        let up = rig.add_bone("leftUpperLeg", Some(hips), [0.0, 1.0, 0.0], [0.0, 0.5, 0.0]);
        rig.add_bone("leftLowerLeg", Some(up), [0.0, 0.5, 0.0], [0.0, 0.0, 0.0]);
        rig.assign_by_distance(&mesh.verts);
        rig.normalize(4);
        (mesh, rig)
    }

    #[test]
    fn a_rigged_mesh_is_written_with_its_bones_skin_weights_and_pictures() {
        let (mesh, rig) = figure();
        let glb = write_glb(&mesh, Some(&rig), &[], 30.0);
        let g = read_glb(&glb).unwrap();
        assert_eq!(g.json["skins"][0]["joints"].as_array().unwrap().len(), 3);
        assert_eq!(g.json["materials"][0]["normalTexture"]["index"], 1);
        // every point has four joints and four weights that add up to one
        let prim = &g.json["meshes"][0]["primitives"][0];
        let w = g.floats(prim["attributes"]["WEIGHTS_0"].as_u64().unwrap() as usize).unwrap();
        assert_eq!(w.len(), mesh.verts.len() * 4);
        for i in 0..mesh.verts.len() {
            assert!((w[i * 4..i * 4 + 4].iter().sum::<f32>() - 1.0).abs() < 1e-4, "point {i}");
        }
        // our own viewer reads it back too: the mesh and both pictures
        let back = AssetMesh::from_glb(&glb).unwrap();
        assert_eq!(back.verts.len(), mesh.verts.len());
        assert!(back.texture.is_some() && back.normal.is_some());
    }

    #[test]
    fn the_rig_comes_back_from_the_file_with_its_places_parents_and_weights() {
        let (mesh, rig) = figure();
        let back = rig_from_glb(&write_glb(&mesh, Some(&rig), &[], 30.0)).expect("a rig");
        assert_eq!(back.bones.len(), 3);
        for (a, b) in back.bones.iter().zip(&rig.bones) {
            assert_eq!(a.name, b.name);
            assert_eq!(a.parent, b.parent);
            for k in 0..3 {
                assert!((a.head[k] - b.head[k]).abs() < 1e-4, "{} {:?} {:?}", a.name, a.head, b.head);
            }
        }
        assert_eq!(back.bones[1].tail, back.bones[2].head, "the tail is where the child starts");
        assert_eq!(back.weights.len(), mesh.verts.len());
        assert!((back.weight_of(0, 2) - rig.weight_of(0, 2)).abs() < 1e-3);
        assert!(rig_from_glb(&write_glb(&mesh, None, &[], 30.0)).is_none(), "a mesh with no rig has none");
    }

    #[test]
    fn a_rig_with_humanoid_names_gets_the_ready_animations() {
        let (mesh, rig) = figure();
        let clips = vec![Clip::new(Motion::Walk, Style::default())];
        let glb = write_glb(&mesh, Some(&rig), &clips, 30.0);
        let g = read_glb(&glb).unwrap();
        let anims = g.json["animations"].as_array().expect("an animation");
        assert_eq!(anims[0]["name"], "Walk");
        let channels = anims[0]["channels"].as_array().unwrap();
        assert!(channels.len() >= 2, "{} channels", channels.len());
        // only the bones that have a humanoid name are moved (the three here are hips and the left leg)
        assert!(channels.iter().all(|c| (c["target"]["node"].as_u64().unwrap() as usize) < 3));
        // the legs hang down here, so the animation is relative to the relaxed stand: no arm bones, but the flag is read from the arms that are not there
        assert!(!arms_hang_down(&rig));
    }

    #[test]
    fn the_normals_are_smooth_and_point_out_of_a_box() {
        let (mesh, _) = figure();
        let n = vertex_normals(&mesh.verts, &mesh.tris);
        assert_eq!(n.len(), mesh.verts.len());
        // a corner of the box points out at a slant
        let c = n[8];
        assert!(c[0].abs() > 0.3 && c[2].abs() > 0.3 && c[1].abs() < 0.8, "{c:?}");
    }
}
