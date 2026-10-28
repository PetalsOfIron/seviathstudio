// SPDX-License-Identifier: GPL-3.0-or-later
// The FBX file of a mesh with the rig that was made by hand (binary FBX 7.4, the kind Unreal, Unity, Maya and Blender read): the mesh with its normals and
// texture coordinates, the picture of the texture inside the file, the bones as a skeleton, the skin (the weights of every bone), the bind pose, and the
// animations (one take for each) for the bones that have the names of a person. Lengths are written in centimetres (the unit of FBX), the up axis is y.

use std::collections::HashMap;

use crate::assetview::AssetMesh;
use crate::motion::Clip;
use crate::rig3d::Rig3;
use crate::take::Anim;

/// One value of a node.
enum P {
    I(i32),
    L(i64),
    D(f64),
    S(String),
    Raw(Vec<u8>),
    Dd(Vec<f64>),
    Ff(Vec<f32>),
    Ii(Vec<i32>),
    Ll(Vec<i64>),
}

struct Node {
    name: &'static str,
    props: Vec<P>,
    kids: Vec<Node>,
}

fn n(name: &'static str, props: Vec<P>, kids: Vec<Node>) -> Node {
    Node { name, props, kids }
}

fn s(x: &str) -> P {
    P::S(x.to_owned())
}

/// A property row of a `Properties70`: name, type, then either the flag `A` (it can be animated) or a label, and the values.
fn prop(name: &str, ty: &str, flag_or_label: &str, values: Vec<P>) -> Node {
    let (label, flags) = if flag_or_label == "A" { ("", "A") } else { (flag_or_label, "") };
    let mut props = vec![s(name), s(ty), s(label), s(flags)];
    props.extend(values);
    n("P", props, vec![])
}

fn props70(rows: Vec<Node>) -> Node {
    n("Properties70", vec![], rows)
}

fn name_of(name: &str, class: &str) -> P {
    P::S(format!("{name}\u{0}\u{1}{class}"))
}

fn write_prop(out: &mut Vec<u8>, p: &P) {
    fn array(out: &mut Vec<u8>, tag: u8, count: usize, bytes: Vec<u8>) {
        out.push(tag);
        out.extend((count as u32).to_le_bytes());
        out.extend(0u32.to_le_bytes());
        out.extend((bytes.len() as u32).to_le_bytes());
        out.extend(bytes);
    }
    match p {
        P::I(v) => {
            out.push(b'I');
            out.extend(v.to_le_bytes());
        }
        P::L(v) => {
            out.push(b'L');
            out.extend(v.to_le_bytes());
        }
        P::D(v) => {
            out.push(b'D');
            out.extend(v.to_le_bytes());
        }
        P::S(v) => {
            out.push(b'S');
            out.extend((v.len() as u32).to_le_bytes());
            out.extend(v.as_bytes());
        }
        P::Raw(v) => {
            out.push(b'R');
            out.extend((v.len() as u32).to_le_bytes());
            out.extend(v);
        }
        P::Dd(v) => array(out, b'd', v.len(), v.iter().flat_map(|x| x.to_le_bytes()).collect()),
        P::Ff(v) => array(out, b'f', v.len(), v.iter().flat_map(|x| x.to_le_bytes()).collect()),
        P::Ii(v) => array(out, b'i', v.len(), v.iter().flat_map(|x| x.to_le_bytes()).collect()),
        P::Ll(v) => array(out, b'l', v.len(), v.iter().flat_map(|x| x.to_le_bytes()).collect()),
    }
}

fn write_node(out: &mut Vec<u8>, node: &Node) {
    let start = out.len();
    out.extend([0u8; 12]);
    out.push(node.name.len() as u8);
    out.extend(node.name.as_bytes());
    let props_start = out.len();
    for p in &node.props {
        write_prop(out, p);
    }
    let props_len = (out.len() - props_start) as u32;
    for k in &node.kids {
        write_node(out, k);
    }
    if !node.kids.is_empty() {
        out.extend([0u8; 13]);
    }
    let end = out.len() as u32;
    out[start..start + 4].copy_from_slice(&end.to_le_bytes());
    out[start + 4..start + 8].copy_from_slice(&(node.props.len() as u32).to_le_bytes());
    out[start + 8..start + 12].copy_from_slice(&props_len.to_le_bytes());
}

fn file(roots: &[Node]) -> Vec<u8> {
    let mut out = b"Kaydara FBX Binary  \0\x1a\0".to_vec();
    out.extend(7400u32.to_le_bytes());
    for r in roots {
        write_node(&mut out, r);
    }
    out.extend([0u8; 13]);
    out.extend([0xfa, 0xbc, 0xab, 0x09, 0xd0, 0xc8, 0xd4, 0x66, 0xb1, 0x76, 0xfb, 0x83, 0x1c, 0xf7, 0x26, 0x7e]);
    let pad = match (16 - out.len() % 16) % 16 {
        0 => 16,
        p => p,
    };
    out.extend(vec![0u8; pad]);
    out.extend(7400u32.to_le_bytes());
    out.extend([0u8; 120]);
    out.extend([0xf8, 0x5a, 0x8c, 0x6a, 0xde, 0xf5, 0xd9, 0x7e, 0xec, 0xe9, 0x0c, 0xe3, 0x75, 0x8f, 0x29, 0x0b]);
    out
}

const TICKS: f64 = 46_186_158_000.0;

fn matrix_translate(t: [f32; 3]) -> Vec<f64> {
    vec![1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, t[0] as f64, t[1] as f64, t[2] as f64, 1.0]
}

fn identity() -> Vec<f64> {
    matrix_translate([0.0; 3])
}

fn png(img: &image::RgbaImage) -> Vec<u8> {
    let mut out = std::io::Cursor::new(Vec::new());
    let _ = img.write_to(&mut out, image::ImageFormat::Png);
    out.into_inner()
}

/// What goes with an FBX file: the texture as a file next to it (its name is in the FBX file, and the picture is also inside it).
pub struct Export {
    pub fbx: Vec<u8>,
    pub texture_name: Option<String>,
}

/// The FBX of a mesh with its rig and animations. `name` is the name of the object (and of the texture file: `<name>_color.png`).
pub fn write_fbx(mesh: &AssetMesh, rig: Option<&Rig3>, clips: &[Clip], fps: f32, name: &str) -> Export {
    let anims = rig.map(|r| crate::take::anims_from_clips(r, clips, fps)).unwrap_or_default();
    write_fbx_anims(mesh, rig, &anims, fps, name)
}

/// The same with animations that are already poses (the ready ones and the ones made by hand).
pub fn write_fbx_anims(mesh: &AssetMesh, rig: Option<&Rig3>, anims: &[Anim], fps: f32, name: &str) -> Export {
    let cm = 100.0f32;
    let mut next_id = 1_000_000i64;
    let mut id = || {
        next_id += 1;
        next_id
    };
    let rig = rig.filter(|r| !r.bones.is_empty() && r.weights.len() == mesh.verts.len());
    let (geo_id, mesh_id, mat_id, tex_id, vid_id, skin_id, pose_id) = (id(), id(), id(), id(), id(), id(), id());
    let bone_ids: Vec<i64> = rig.map(|r| r.bones.iter().map(|_| id()).collect()).unwrap_or_default();
    let attr_ids: Vec<i64> = bone_ids.iter().map(|_| id()).collect();
    let mut objects: Vec<Node> = Vec::new();
    let mut links: Vec<Node> = Vec::new();
    let link = |kind: &'static str, a: i64, b: i64| n("C", vec![s(kind), P::L(a), P::L(b)], vec![]);
    let link_p = |a: i64, b: i64, p: &str| n("C", vec![s("OP"), P::L(a), P::L(b), s(p)], vec![]);

    // the geometry: triangles, with the last index of each one turned over (FBX's way of ending a polygon)
    let verts: Vec<f64> = mesh.verts.iter().flat_map(|v| [(v[0] * cm) as f64, (v[1] * cm) as f64, (v[2] * cm) as f64]).collect();
    let poly: Vec<i32> = mesh.tris.iter().flat_map(|t| [t[0] as i32, t[1] as i32, -(t[2] as i32) - 1]).collect();
    let normals = crate::meshfile::vertex_normals(&mesh.verts, &mesh.tris);
    let corner_normals: Vec<f64> = mesh.tris.iter().flat_map(|t| t.iter().flat_map(|i| normals[*i as usize].map(|x| x as f64)).collect::<Vec<_>>()).collect();
    let uvs: Vec<f64> = mesh.uvs.iter().flat_map(|t| [t[0] as f64, 1.0 - t[1] as f64]).collect();
    let uv_index: Vec<i32> = mesh.tris.iter().flat_map(|t| t.map(|i| i as i32)).collect();
    objects.push(n(
        "Geometry",
        vec![P::L(geo_id), name_of(name, "Geometry"), s("Mesh")],
        vec![
            n("Vertices", vec![P::Dd(verts)], vec![]),
            n("PolygonVertexIndex", vec![P::Ii(poly)], vec![]),
            n("GeometryVersion", vec![P::I(124)], vec![]),
            n("LayerElementNormal", vec![P::I(0)], vec![n("Version", vec![P::I(101)], vec![]), n("Name", vec![s("")], vec![]), n("MappingInformationType", vec![s("ByPolygonVertex")], vec![]), n("ReferenceInformationType", vec![s("Direct")], vec![]), n("Normals", vec![P::Dd(corner_normals)], vec![])]),
            n("LayerElementUV", vec![P::I(0)], vec![n("Version", vec![P::I(101)], vec![]), n("Name", vec![s("UVMap")], vec![]), n("MappingInformationType", vec![s("ByPolygonVertex")], vec![]), n("ReferenceInformationType", vec![s("IndexToDirect")], vec![]), n("UV", vec![P::Dd(uvs)], vec![]), n("UVIndex", vec![P::Ii(uv_index)], vec![])]),
            n("LayerElementMaterial", vec![P::I(0)], vec![n("Version", vec![P::I(101)], vec![]), n("Name", vec![s("")], vec![]), n("MappingInformationType", vec![s("AllSame")], vec![]), n("ReferenceInformationType", vec![s("IndexToDirect")], vec![]), n("Materials", vec![P::Ii(vec![0])], vec![])]),
            n(
                "Layer",
                vec![P::I(0)],
                vec![
                    n("Version", vec![P::I(100)], vec![]),
                    n("LayerElement", vec![], vec![n("Type", vec![s("LayerElementNormal")], vec![]), n("TypedIndex", vec![P::I(0)], vec![])]),
                    n("LayerElement", vec![], vec![n("Type", vec![s("LayerElementMaterial")], vec![]), n("TypedIndex", vec![P::I(0)], vec![])]),
                    n("LayerElement", vec![], vec![n("Type", vec![s("LayerElementUV")], vec![]), n("TypedIndex", vec![P::I(0)], vec![])]),
                ],
            ),
        ],
    ));
    objects.push(n("Model", vec![P::L(mesh_id), name_of(name, "Model"), s("Mesh")], vec![n("Version", vec![P::I(232)], vec![]), props70(vec![prop("Lcl Translation", "Lcl Translation", "A", vec![P::D(0.0), P::D(0.0), P::D(0.0)])]), n("Shading", vec![P::I(1)], vec![]), n("Culling", vec![s("CullingOff")], vec![])]));

    // the material and the texture
    let mut material = vec![n("Version", vec![P::I(102)], vec![]), n("ShadingModel", vec![s("phong")], vec![]), n("MultiLayer", vec![P::I(0)], vec![])];
    material.push(props70(vec![prop("DiffuseColor", "Color", "A", vec![P::D(1.0), P::D(1.0), P::D(1.0)]), prop("SpecularFactor", "Number", "A", vec![P::D(0.0)]), prop("Shininess", "Number", "A", vec![P::D(10.0)])]));
    objects.push(n("Material", vec![P::L(mat_id), name_of("Surface", "Material"), s("")], material));
    let texture_name = mesh.texture.as_ref().map(|_| format!("{name}_color.png"));
    if let (Some(tex), Some(file_name)) = (&mesh.texture, &texture_name) {
        objects.push(n("Video", vec![P::L(vid_id), name_of("Colour", "Video"), s("Clip")], vec![n("Type", vec![s("Clip")], vec![]), props70(vec![prop("Path", "KString", "XRefUrl", vec![s(file_name)])]), n("UseMipMap", vec![P::I(0)], vec![]), n("Filename", vec![s(file_name)], vec![]), n("RelativeFilename", vec![s(file_name)], vec![]), n("Content", vec![P::Raw(png(tex))], vec![])]));
        objects.push(n(
            "Texture",
            vec![P::L(tex_id), name_of("Colour", "Texture"), s("")],
            vec![n("Type", vec![s("TextureVideoClip")], vec![]), n("Version", vec![P::I(202)], vec![]), n("TextureName", vec![name_of("Colour", "Texture")], vec![]), n("Media", vec![name_of("Colour", "Video")], vec![]), n("FileName", vec![s(file_name)], vec![]), n("RelativeFilename", vec![s(file_name)], vec![]), n("ModelUVTranslation", vec![P::D(0.0), P::D(0.0)], vec![]), n("ModelUVScaling", vec![P::D(1.0), P::D(1.0)], vec![]), n("Texture_Alpha_Source", vec![s("None")], vec![]), n("Cropping", vec![P::I(0), P::I(0), P::I(0), P::I(0)], vec![])],
        ));
        links.push(link("OO", vid_id, tex_id));
        links.push(link_p(tex_id, mat_id, "DiffuseColor"));
    }
    links.push(link("OO", mesh_id, 0));
    links.push(link("OO", geo_id, mesh_id));
    links.push(link("OO", mat_id, mesh_id));

    // the skeleton, the skin and the bind pose
    let mut pose_nodes = vec![n("PoseNode", vec![], vec![n("Node", vec![P::L(mesh_id)], vec![]), n("Matrix", vec![P::Dd(identity())], vec![])])];
    if let Some(r) = rig {
        for (i, b) in r.bones.iter().enumerate() {
            let up = b.parent.map(|p| r.bones[p].head).unwrap_or([0.0; 3]);
            let local = [(b.head[0] - up[0]) * cm, (b.head[1] - up[1]) * cm, (b.head[2] - up[2]) * cm];
            objects.push(n("NodeAttribute", vec![P::L(attr_ids[i]), name_of(&b.name, "NodeAttribute"), s("LimbNode")], vec![n("TypeFlags", vec![s("Skeleton")], vec![])]));
            objects.push(n(
                "Model",
                vec![P::L(bone_ids[i]), name_of(&b.name, "Model"), s("LimbNode")],
                vec![n("Version", vec![P::I(232)], vec![]), props70(vec![prop("RotationOrder", "enum", "", vec![P::I(2)]), prop("Lcl Translation", "Lcl Translation", "A", vec![P::D(local[0] as f64), P::D(local[1] as f64), P::D(local[2] as f64)]), prop("Lcl Rotation", "Lcl Rotation", "A", vec![P::D(0.0), P::D(0.0), P::D(0.0)])]), n("Shading", vec![s("Y")], vec![]), n("Culling", vec![s("CullingOff")], vec![])],
            ));
            links.push(link("OO", attr_ids[i], bone_ids[i]));
            links.push(link("OO", bone_ids[i], b.parent.map(|p| bone_ids[p]).unwrap_or(0)));
            let global = [b.head[0] * cm, b.head[1] * cm, b.head[2] * cm];
            pose_nodes.push(n("PoseNode", vec![], vec![n("Node", vec![P::L(bone_ids[i])], vec![]), n("Matrix", vec![P::Dd(matrix_translate(global))], vec![])]));
        }
        objects.push(n("Deformer", vec![P::L(skin_id), name_of("Skin", "Deformer"), s("Skin")], vec![n("Version", vec![P::I(101)], vec![]), n("Link_DeformAcuracy", vec![P::D(50.0)], vec![]), n("SkinningType", vec![s("Linear")], vec![])]));
        links.push(link("OO", skin_id, geo_id));
        let mut per_bone: Vec<(Vec<i32>, Vec<f64>)> = vec![(Vec::new(), Vec::new()); r.bones.len()];
        for (v, w) in r.weights.iter().enumerate() {
            let sum: f32 = w.iter().map(|x| x.1).sum::<f32>();
            if sum <= 1e-9 {
                continue;
            }
            for (b, x) in w {
                per_bone[*b as usize].0.push(v as i32);
                per_bone[*b as usize].1.push((x / sum) as f64);
            }
        }
        for (i, b) in r.bones.iter().enumerate() {
            let cid = id();
            let global = [b.head[0] * cm, b.head[1] * cm, b.head[2] * cm];
            let (idx, wts) = std::mem::take(&mut per_bone[i]);
            objects.push(n(
                "Deformer",
                vec![P::L(cid), name_of(&b.name, "SubDeformer"), s("Cluster")],
                vec![n("Version", vec![P::I(100)], vec![]), n("UserData", vec![s(""), s("")], vec![]), n("Indexes", vec![P::Ii(idx)], vec![]), n("Weights", vec![P::Dd(wts)], vec![]), n("Transform", vec![P::Dd(identity())], vec![]), n("TransformLink", vec![P::Dd(matrix_translate(global))], vec![])],
            ));
            links.push(link("OO", cid, skin_id));
            links.push(link("OO", bone_ids[i], cid));
        }
        objects.push(n("Pose", vec![P::L(pose_id), name_of("BindPose", "Pose"), s("BindPose")], vec![n("Type", vec![s("BindPose")], vec![]), n("Version", vec![P::I(100)], vec![]), n("NbPoseNodes", vec![P::I(pose_nodes.len() as i32)], vec![])].into_iter().chain(pose_nodes).collect()));
    }

    // the animations: one take for each, the turns of the bones that move, and the move of the figure on the bones that have no parent
    if let Some(r) = rig.filter(|_| !anims.is_empty()) {
        for an in anims.iter().filter(|a| !a.poses.is_empty()) {
            let afps = an.fps.max(1.0) as f64;
            let (stack, layer) = (id(), id());
            let end = (an.poses.len().saturating_sub(1) as f64 / afps * TICKS) as i64;
            objects.push(n("AnimationStack", vec![P::L(stack), name_of(&an.name, "AnimStack"), s("")], vec![props70(vec![prop("LocalStop", "KTime", "Time", vec![P::L(end)]), prop("ReferenceStop", "KTime", "Time", vec![P::L(end)])])]));
            objects.push(n("AnimationLayer", vec![P::L(layer), name_of("BaseLayer", "AnimLayer"), s("")], vec![]));
            links.push(link("OO", layer, stack));
            let times: Vec<i64> = (0..an.poses.len()).map(|i| (i as f64 / afps * TICKS) as i64).collect();
            let mut curve_set = |objects: &mut Vec<Node>, links: &mut Vec<Node>, bone: i64, label: &str, p: &str, series: [Vec<f32>; 3], def: [f64; 3]| {
                let cn = id();
                objects.push(n("AnimationCurveNode", vec![P::L(cn), name_of(label, "AnimCurveNode"), s("")], vec![props70(vec![prop("d|X", "Number", "A", vec![P::D(def[0])]), prop("d|Y", "Number", "A", vec![P::D(def[1])]), prop("d|Z", "Number", "A", vec![P::D(def[2])])])]));
                links.push(link("OO", cn, layer));
                links.push(link_p(cn, bone, p));
                for (k, axis) in ["d|X", "d|Y", "d|Z"].into_iter().enumerate() {
                    let cu = id();
                    objects.push(n(
                        "AnimationCurve",
                        vec![P::L(cu), name_of("", "AnimCurve"), s("")],
                        vec![n("Default", vec![P::D(def[k])], vec![]), n("KeyVer", vec![P::I(4008)], vec![]), n("KeyTime", vec![P::Ll(times.clone())], vec![]), n("KeyValueFloat", vec![P::Ff(series[k].clone())], vec![]), n("KeyAttrFlags", vec![P::Ii(vec![4])], vec![]), n("KeyAttrDataFloat", vec![P::Ff(vec![0.0, 0.0, 0.0, 0.0])], vec![]), n("KeyAttrRefCount", vec![P::Ii(vec![times.len() as i32])], vec![])],
                    ));
                    links.push(link_p(cu, cn, axis));
                }
            };
            for (bi, bone) in r.bones.iter().enumerate() {
                if an.poses.iter().any(|p| p.rot.get(bi).is_some_and(|x| *x != [0.0; 3])) {
                    let series = [0, 1, 2].map(|k| an.poses.iter().map(|p| p.rot.get(bi).map(|x| x[k]).unwrap_or(0.0)).collect::<Vec<f32>>());
                    curve_set(&mut objects, &mut links, bone_ids[bi], "R", "Lcl Rotation", series, [0.0; 3]);
                }
                if bone.parent.is_none() && an.poses.iter().any(|p| p.offset != [0.0; 3]) {
                    let rest = [bone.head[0] * cm, bone.head[1] * cm, bone.head[2] * cm];
                    let series = [0, 1, 2].map(|k| an.poses.iter().map(|p| rest[k] + p.offset[k] * cm).collect::<Vec<f32>>());
                    curve_set(&mut objects, &mut links, bone_ids[bi], "T", "Lcl Translation", series, [rest[0] as f64, rest[1] as f64, rest[2] as f64]);
                }
            }
        }
    }

    let counts: HashMap<&str, i32> = objects.iter().fold(HashMap::new(), |mut m, o| {
        *m.entry(o.name).or_insert(0) += 1;
        m
    });
    let mut definitions = vec![n("Version", vec![P::I(100)], vec![]), n("Count", vec![P::I(objects.len() as i32)], vec![])];
    let mut names: Vec<(&&str, &i32)> = counts.iter().collect();
    names.sort();
    for (nm, c) in names {
        definitions.push(n("ObjectType", vec![s(nm)], vec![n("Count", vec![P::I(*c)], vec![])]));
    }
    let time_mode = match fps.round() as i32 {
        30 => 6,
        24 => 11,
        60 => 3,
        _ => 14,
    };
    let roots = vec![
        n("FBXHeaderExtension", vec![], vec![n("FBXHeaderVersion", vec![P::I(1003)], vec![]), n("FBXVersion", vec![P::I(7400)], vec![]), n("Creator", vec![s("SeviathStudio")], vec![])]),
        n(
            "GlobalSettings",
            vec![],
            vec![
                n("Version", vec![P::I(1000)], vec![]),
                props70(vec![
                    prop("UpAxis", "int", "Integer", vec![P::I(1)]),
                    prop("UpAxisSign", "int", "Integer", vec![P::I(1)]),
                    prop("FrontAxis", "int", "Integer", vec![P::I(2)]),
                    prop("FrontAxisSign", "int", "Integer", vec![P::I(1)]),
                    prop("CoordAxis", "int", "Integer", vec![P::I(0)]),
                    prop("CoordAxisSign", "int", "Integer", vec![P::I(1)]),
                    prop("UnitScaleFactor", "double", "Number", vec![P::D(1.0)]),
                    prop("OriginalUnitScaleFactor", "double", "Number", vec![P::D(1.0)]),
                    prop("TimeMode", "enum", "", vec![P::I(time_mode)]),
                    prop("CustomFrameRate", "double", "Number", vec![P::D(fps as f64)]),
                ]),
            ],
        ),
        n("Documents", vec![], vec![n("Count", vec![P::I(1)], vec![]), n("Document", vec![P::L(1), s("Scene"), s("Scene")], vec![n("RootNode", vec![P::L(0)], vec![])])]),
        n("References", vec![], vec![]),
        n("Definitions", vec![], definitions),
        n("Objects", vec![], objects),
        n("Connections", vec![], links),
        n("Takes", vec![], vec![n("Current", vec![s("")], vec![])]),
    ];
    Export { fbx: file(&roots), texture_name }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::motion::{Motion, Style};

    /// The names of the nodes in the file, to see what is in it (only the ones whose kids are read).
    fn read_names(bytes: &[u8], at: usize, end: usize, out: &mut Vec<String>) {
        let mut pos = at;
        while pos + 13 <= end {
            let node_end = u32::from_le_bytes(bytes[pos..pos + 4].try_into().unwrap()) as usize;
            if node_end == 0 {
                break;
            }
            let (props, props_len, name_len) = (u32::from_le_bytes(bytes[pos + 4..pos + 8].try_into().unwrap()), u32::from_le_bytes(bytes[pos + 8..pos + 12].try_into().unwrap()) as usize, bytes[pos + 12] as usize);
            let _ = props;
            let name = String::from_utf8_lossy(&bytes[pos + 13..pos + 13 + name_len]).to_string();
            out.push(name);
            let kids = pos + 13 + name_len + props_len;
            if kids < node_end {
                read_names(bytes, kids, node_end, out);
            }
            pos = node_end;
        }
    }

    #[test]
    fn the_file_has_the_mesh_the_skeleton_the_skin_and_the_takes() {
        let (mesh, rig) = crate::meshfile::tests_figure();
        let clips = vec![Clip::new(Motion::Walk, Style::default())];
        let ex = write_fbx(&mesh, Some(&rig), &clips, 30.0, "figure");
        assert!(ex.fbx.starts_with(b"Kaydara FBX Binary  \0\x1a\0"));
        assert_eq!(ex.texture_name.as_deref(), Some("figure_color.png"));
        let mut names = Vec::new();
        read_names(&ex.fbx, 27, ex.fbx.len(), &mut names);
        let count = |x: &str| names.iter().filter(|n| *n == x).count();
        assert_eq!(count("Geometry"), 1);
        assert_eq!(count("Model"), 1 + rig.bones.len(), "the mesh and every bone");
        assert_eq!(count("Pose"), 1);
        assert!(count("Deformer") > rig.bones.len(), "the skin and one cluster for each bone");
        assert_eq!(count("AnimationStack"), 1);
        assert!(count("AnimationCurve") >= 3, "{}", count("AnimationCurve"));
        assert_eq!(count("Video"), 1);
        assert!(ex.fbx.ends_with(&[0xf8, 0x5a, 0x8c, 0x6a, 0xde, 0xf5, 0xd9, 0x7e, 0xec, 0xe9, 0x0c, 0xe3, 0x75, 0x8f, 0x29, 0x0b]));
    }

    #[test]
    fn a_mesh_without_a_rig_is_a_plain_mesh() {
        let (mesh, _) = crate::meshfile::tests_figure();
        let ex = write_fbx(&mesh, None, &[], 30.0, "plain");
        let mut names = Vec::new();
        read_names(&ex.fbx, 27, ex.fbx.len(), &mut names);
        assert_eq!(names.iter().filter(|n| *n == "Model").count(), 1);
        assert!(!names.iter().any(|n| n == "Pose" || n == "AnimationStack"));
    }

    /// A file to open in Blender or Unreal by hand: a person-shaped mesh with the bones of a person, every weight, the texture and some animations.
    #[test]
    #[ignore]
    fn write_a_sample_fbx() {
        let (mut mesh, _) = crate::meshfile::tests_figure();
        mesh.stand_on_floor(1.8);
        let (lo, hi) = mesh.bounds();
        let mut rig = Rig3::humanoid(mesh.verts.len(), lo, hi);
        rig.assign_by_distance(&mesh.verts);
        let clips = vec![Clip::new(Motion::Walk, Style::default()), Clip::new(Motion::Wave, Style::default())];
        let ex = write_fbx(&mesh, Some(&rig), &clips, 30.0, "sample");
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("target").join("helpwork");
        std::fs::write(dir.join("sample.fbx"), &ex.fbx).unwrap();
        mesh.texture.as_ref().unwrap().save(dir.join("sample_color.png")).unwrap();
        std::fs::write(dir.join("sample.glb"), crate::meshfile::write_glb(&mesh, Some(&rig), &clips, 30.0)).unwrap();
    }
}
