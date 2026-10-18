// SPDX-License-Identifier: GPL-3.0-or-later
// Writes a `Model3d` as a VRM file for VTubers, or as a plain glTF binary (GLB) for games: VRM 1.0 (extension VRMC_vrm, faces +Z) or VRM 0.x (extension VRM, faces -Z), the way
// games and VRM apps expect. `validate` reads a file back and checks it the way a loader would.

use serde_json::{Value, json};

use super::gltf::{Builder, read_glb};
use super::model::{ARKIT_ALIASES, Model3d};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Format {
    /// VRM 1.0.
    Vrm1,
    /// VRM 0.x (the older format many apps still use).
    Vrm0,
    /// A plain GLB for game engines: the skeleton, the skin and the blend shapes, no VRM data.
    Glb,
}

fn png_bytes(img: &image::RgbaImage) -> Vec<u8> {
    let mut out = std::io::Cursor::new(Vec::new());
    let _ = img.write_to(&mut out, image::ImageFormat::Png);
    out.into_inner()
}

/// 1.0 faces +Z; 0.x faces -Z, so everything is turned half way round the up axis.
fn conv(fmt: Format, p: [f32; 3]) -> [f32; 3] {
    match fmt {
        Format::Vrm1 | Format::Glb => p,
        Format::Vrm0 => [-p[0], p[1], -p[2]],
    }
}

/// The 1.0 name of an expression is the preset name when there is one.
const PRESETS1: [&str; 18] = ["happy", "angry", "sad", "relaxed", "surprised", "aa", "ih", "ou", "ee", "oh", "blink", "blinkLeft", "blinkRight", "lookUp", "lookDown", "lookLeft", "lookRight", "neutral"];

/// (our name, the 0.x preset name)
const PRESETS0: [(&str, &str); 15] = [("aa", "a"), ("ih", "i"), ("ou", "u"), ("ee", "e"), ("oh", "o"), ("blink", "blink"), ("blinkLeft", "blink_l"), ("blinkRight", "blink_r"), ("happy", "joy"), ("sad", "sorrow"), ("relaxed", "fun"), ("lookUp", "lookup"), ("lookDown", "lookdown"), ("lookLeft", "lookleft"), ("lookRight", "lookright")];

pub fn export(model: &Model3d, fmt: Format) -> Result<Vec<u8>, String> {
    if model.parts.is_empty() || model.bones.is_empty() {
        return Err("There is nothing to export: the character has no parts.".into());
    }
    let mut b = Builder::new("SeviathStudio");
    let nb = model.bones.len();
    let bone_node = |i: usize| 1 + i;
    let mesh_node = |i: usize| 1 + nb + i;
    // nodes: the root, the bones, then one node for every part
    b.nodes.push(json!({ "name": "Root", "children": [bone_node(0)] }));
    for (i, bone) in model.bones.iter().enumerate() {
        let world = conv(fmt, bone.pos);
        let parent = bone.parent.map(|p| conv(fmt, model.bones[p].pos)).unwrap_or([0.0; 3]);
        let children: Vec<usize> = model.bones.iter().enumerate().filter(|(_, c)| c.parent == Some(i)).map(|(k, _)| bone_node(k)).collect();
        let mut n = json!({ "name": bone.name, "translation": [world[0] - parent[0], world[1] - parent[1], world[2] - parent[2]] });
        if !children.is_empty() {
            n["children"] = json!(children);
        }
        b.nodes.push(n);
    }
    // the skin: the bones and where they were at rest
    let inverse: Vec<[f32; 16]> = model.bones.iter().map(|bone| {
        let p = conv(fmt, bone.pos);
        [1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, -p[0], -p[1], -p[2], 1.0]
    }).collect();
    let ibm = b.mat4s(&inverse);
    b.skins.push(json!({ "inverseBindMatrices": ibm, "joints": (0..nb).map(bone_node).collect::<Vec<_>>(), "skeleton": bone_node(0), "name": "Armature" }));
    // the parts
    let mut shared_tex: Vec<(u64, usize)> = Vec::new();
    for (pi, part) in model.parts.iter().enumerate() {
        // parts with the same picture (the body, the face and the hair of a character on an AI shape) write it once
        let hash = {
            use std::hash::{Hash, Hasher};
            let mut h = std::collections::hash_map::DefaultHasher::new();
            (part.image.width(), part.image.height()).hash(&mut h);
            part.image.as_raw().hash(&mut h);
            h.finish()
        };
        let tex = match shared_tex.iter().find(|(k, _)| *k == hash) {
            Some((_, t)) => *t,
            None => {
                let t = b.png_texture(&png_bytes(&part.image), &part.name);
                shared_tex.push((hash, t));
                t
            }
        };
        let mut material = json!({
            "name": part.name,
            "pbrMetallicRoughness": { "baseColorTexture": { "index": tex }, "baseColorFactor": [1, 1, 1, 1], "metallicFactor": 0.0, "roughnessFactor": 0.9 },
            "alphaMode": "MASK",
            "alphaCutoff": 0.5,
            "doubleSided": true,
        });
        if fmt == Format::Glb {
            // a game lights the picture itself
        } else if fmt == Format::Vrm1 {
            material["extensions"] = json!({
                "KHR_materials_unlit": {},
                "VRMC_materials_mtoon": {
                    "specVersion": "1.0",
                    "transparentWithZWrite": false,
                    "renderQueueOffsetNumber": 0,
                    "shadeColorFactor": [0.9, 0.82, 0.85],
                    "shadeMultiplyTexture": { "index": tex },
                    "shadingShiftFactor": 0.0,
                    "shadingToonyFactor": 0.9,
                    "giEqualizationFactor": 0.9,
                    "outlineWidthMode": "none",
                },
            });
        } else {
            material["extensions"] = json!({ "KHR_materials_unlit": {} });
        }
        b.materials.push(material);
        let verts: Vec<[f32; 3]> = part.verts.iter().map(|v| conv(fmt, *v)).collect();
        let normals: Vec<[f32; 3]> = part.normals.iter().map(|v| conv(fmt, *v)).collect();
        let pos = b.vec3s(&verts, true);
        let nor = b.vec3s(&normals, false);
        let uv = b.vec2s(&part.uvs);
        let jn = b.joints(&part.joints);
        let wt = b.vec4s(&part.weights);
        let idx = b.indices(&part.tris);
        let mut prim = json!({ "attributes": { "POSITION": pos, "NORMAL": nor, "TEXCOORD_0": uv, "JOINTS_0": jn, "WEIGHTS_0": wt }, "indices": idx, "material": pi, "mode": 4 });
        let mut mesh = json!({ "name": part.name });
        if !part.morphs.is_empty() {
            let mut targets = Vec::new();
            for m in &part.morphs {
                let d: Vec<[f32; 3]> = m.deltas.iter().map(|v| [conv(fmt, *v)[0], v[1], conv(fmt, *v)[2]]).collect();
                targets.push(json!({ "POSITION": b.vec3s(&d, true) }));
            }
            prim["targets"] = json!(targets);
            mesh["extras"] = json!({ "targetNames": part.morphs.iter().map(|m| m.name.clone()).collect::<Vec<_>>() });
            mesh["weights"] = json!(vec![0.0; part.morphs.len()]);
        }
        mesh["primitives"] = json!([prim]);
        b.meshes.push(mesh);
        b.nodes.push(json!({ "name": part.name, "mesh": pi, "skin": 0 }));
    }
    let mut scene_nodes = vec![0usize];
    scene_nodes.extend((0..model.parts.len()).map(mesh_node));
    b.scenes.push(json!({ "nodes": scene_nodes }));
    // the VRM part
    let hb = |name: &str| model.bone(name).map(bone_node);
    match fmt {
        Format::Glb => {}
        Format::Vrm1 => {
            b.extensions_used = vec!["KHR_materials_unlit".into(), "VRMC_vrm".into(), "VRMC_materials_mtoon".into()];
            let mut human = serde_json::Map::new();
            for bone in model.bones.iter().filter(|b| b.humanoid) {
                human.insert(bone.name.clone(), json!({ "node": hb(&bone.name).unwrap() }));
            }
            let binds = |e: &super::model::Expression| -> Vec<Value> { e.binds.iter().map(|(p, m, w)| json!({ "node": mesh_node(*p), "index": m, "weight": w })).collect() };
            let expr_json = |e: &super::model::Expression| json!({ "morphTargetBinds": binds(e), "isBinary": false, "overrideBlink": "none", "overrideLookAt": "none", "overrideMouth": "none" });
            let (mut preset, mut custom) = (serde_json::Map::new(), serde_json::Map::new());
            preset.insert("neutral".into(), json!({ "isBinary": false, "overrideBlink": "none", "overrideLookAt": "none", "overrideMouth": "none" }));
            for e in &model.expressions {
                if PRESETS1.contains(&e.name.as_str()) {
                    preset.insert(e.name.clone(), expr_json(e));
                } else {
                    custom.insert(e.name.clone(), expr_json(e));
                }
            }
            for (arkit, ours) in ARKIT_ALIASES {
                if let Some(e) = model.expressions.iter().find(|e| e.name == *ours) {
                    if !custom.contains_key(*arkit) && !preset.contains_key(*arkit) {
                        custom.insert((*arkit).into(), expr_json(e));
                    }
                }
            }
            let mut vrm = json!({
                "specVersion": "1.0",
                "meta": {
                    "name": model.name, "version": "1.0", "authors": [model.author],
                    "licenseUrl": "https://vrm.dev/licenses/1.0/", "avatarPermission": "onlyAuthor",
                    "allowExcessivelyViolentUsage": false, "allowExcessivelySexualUsage": false, "commercialUsage": "personalNonProfit",
                    "allowPoliticalOrReligiousUsage": false, "allowAntisocialOrHateUsage": false, "creditNotation": "required",
                    "allowRedistribution": false, "modification": "prohibited",
                },
                "humanoid": { "humanBones": human },
                "expressions": { "preset": preset, "custom": custom },
            });
            if model.bone("leftEye").is_some() {
                vrm["lookAt"] = json!({ "offsetFromHeadBone": [0.0, 0.06, 0.0], "type": "bone",
                    "rangeMapHorizontalInner": { "inputMaxValue": 90, "outputScale": 8 }, "rangeMapHorizontalOuter": { "inputMaxValue": 90, "outputScale": 12 },
                    "rangeMapVerticalDown": { "inputMaxValue": 90, "outputScale": 10 }, "rangeMapVerticalUp": { "inputMaxValue": 90, "outputScale": 10 } });
            }
            b.extensions.insert("VRMC_vrm".into(), vrm);
            if !model.springs.is_empty() || !model.colliders.is_empty() {
                b.extensions_used.push("VRMC_springBone".into());
                let colliders: Vec<Value> = model.colliders.iter().map(|c| {
                    let o = conv(fmt, c.offset);
                    json!({ "node": bone_node(c.bone), "shape": { "sphere": { "offset": o, "radius": c.radius } } })
                }).collect();
                let groups: Vec<Value> = if model.colliders.is_empty() { Vec::new() } else { vec![json!({ "name": "body", "colliders": (0..model.colliders.len()).collect::<Vec<_>>() })] };
                let springs: Vec<Value> = model.springs.iter().map(|s| {
                    let mut joints: Vec<Value> = s.joints.iter().take(s.joints.len().saturating_sub(1)).map(|j| json!({ "node": bone_node(*j), "hitRadius": s.hit_radius, "stiffness": s.stiffness, "gravityPower": s.gravity, "gravityDir": [0, -1, 0], "dragForce": s.drag })).collect();
                    joints.push(json!({ "node": bone_node(*s.joints.last().unwrap()) }));
                    let mut sp = json!({ "name": s.name, "joints": joints });
                    if !groups.is_empty() {
                        sp["colliderGroups"] = json!([0]);
                    }
                    sp
                }).collect();
                b.extensions.insert("VRMC_springBone".into(), json!({ "specVersion": "1.0", "colliders": colliders, "colliderGroups": groups, "springs": springs }));
            }
        }
        Format::Vrm0 => {
            b.extensions_used = vec!["KHR_materials_unlit".into(), "VRM".into()];
            // 0.x calls the three bones of a thumb Proximal, Intermediate and Distal
            let old = |n: &str| -> String { n.replace("ThumbProximal", "ThumbIntermediate").replace("ThumbMetacarpal", "ThumbProximal") };
            let bones: Vec<Value> = model.bones.iter().filter(|b| b.humanoid).map(|bone| json!({ "bone": old(&bone.name), "node": hb(&bone.name).unwrap(), "useDefaultValues": true })).collect();
            let mut groups = vec![json!({ "name": "Neutral", "presetName": "neutral", "binds": [], "isBinary": false, "materialValues": [] })];
            for e in &model.expressions {
                let preset = PRESETS0.iter().find(|p| p.0 == e.name).map(|p| p.1).unwrap_or("unknown");
                let name: String = {
                    let mut c = e.name.chars();
                    c.next().map(|f| f.to_uppercase().collect::<String>() + c.as_str()).unwrap_or_default()
                };
                let binds: Vec<Value> = e.binds.iter().map(|(p, m, w)| json!({ "mesh": p, "index": m, "weight": w * 100.0 })).collect();
                groups.push(json!({ "name": name, "presetName": preset, "binds": binds, "isBinary": false, "materialValues": [] }));
            }
            let mat_props: Vec<Value> = model.parts.iter().enumerate().map(|(i, part)| json!({
                "name": part.name, "shader": "VRM/MToon", "renderQueue": 2450,
                "floatProperties": { "_Cutoff": 0.5, "_CullMode": 0, "_BlendMode": 1, "_SrcBlend": 1, "_DstBlend": 0, "_ZWrite": 1, "_ShadeToony": 0.9, "_ShadeShift": 0.0,
                    "_OutlineWidthMode": 0, "_OutlineColorMode": 0, "_MToonVersion": 34, "_DebugMode": 0, "_LightColorAttenuation": 0, "_IndirectLightIntensity": 0.1,
                    "_ReceiveShadowRate": 1, "_ShadingGradeRate": 1, "_RimLightingMix": 0, "_RimFresnelPower": 100, "_RimLift": 0.0 },
                "vectorProperties": { "_Color": [1, 1, 1, 1], "_ShadeColor": [0.9, 0.82, 0.85, 1], "_MainTex": [0, 0, 1, 1], "_ShadeTexture": [0, 0, 1, 1] },
                "textureProperties": { "_MainTex": i, "_ShadeTexture": i },
                "keywordMap": { "_ALPHATEST_ON": true },
                "tagMap": { "RenderType": "TransparentCutout" },
            })).collect();
            let curve = json!([0, 0, 0, 1, 1, 1, 1, 0]);
            b.extensions.insert("VRM".into(), json!({
                "exporterVersion": "SeviathStudio", "specVersion": "0.0",
                "meta": { "title": model.name, "version": "1.0", "author": model.author, "contactInformation": "", "reference": "",
                    "allowedUserName": "OnlyAuthor", "violentUssageName": "Disallow", "sexualUssageName": "Disallow", "commercialUssageName": "Disallow",
                    "otherPermissionUrl": "", "licenseName": "Redistribution_Prohibited", "otherLicenseUrl": "" },
                "humanoid": { "humanBones": bones, "armStretch": 0.05, "legStretch": 0.05, "upperArmTwist": 0.5, "lowerArmTwist": 0.5, "upperLegTwist": 0.5, "lowerLegTwist": 0.5, "feetSpacing": 0, "hasTranslationDoF": false },
                "firstPerson": { "firstPersonBone": hb("head").unwrap(), "firstPersonBoneOffset": { "x": 0, "y": 0.06, "z": 0 }, "meshAnnotations": [], "lookAtTypeName": "Bone",
                    "lookAtHorizontalInner": { "curve": curve, "xRange": 90, "yRange": 8 }, "lookAtHorizontalOuter": { "curve": curve, "xRange": 90, "yRange": 12 },
                    "lookAtVerticalDown": { "curve": curve, "xRange": 90, "yRange": 10 }, "lookAtVerticalUp": { "curve": curve, "xRange": 90, "yRange": 10 } },
                "blendShapeMaster": { "blendShapeGroups": groups },
                "secondaryAnimation": { "boneGroups": model.springs.iter().map(|s| json!({
                        "comment": s.name, "stiffiness": s.stiffness, "gravityPower": s.gravity, "gravityDir": { "x": 0, "y": -1, "z": 0 }, "dragForce": s.drag,
                        "center": -1, "hitRadius": s.hit_radius, "bones": [bone_node(s.joints[0])], "colliderGroups": (0..model.colliders.len().min(1)).collect::<Vec<_>>() })).collect::<Vec<_>>(),
                    "colliderGroups": model.colliders.iter().map(|c| { let o = conv(fmt, c.offset); json!({ "node": bone_node(c.bone), "colliders": [{ "offset": { "x": o[0], "y": o[1], "z": o[2] }, "radius": c.radius }] }) }).collect::<Vec<_>>() },
                "materialProperties": mat_props,
            }));
        }
    }
    Ok(b.glb())
}

/// What a checked file holds.
#[derive(Debug, PartialEq)]
pub struct Summary {
    pub version: u8,
    pub bones: usize,
    pub parts: usize,
    pub vertices: usize,
    pub expressions: usize,
    /// Joints of swinging chains (hair, cloth).
    pub springs: usize,
}

/// Read a VRM back and check it the way a loader would: the humanoid has what games need, every index points at
/// something, every attribute has the same count, the expression shapes exist.
pub fn validate(bytes: &[u8]) -> Result<Summary, String> {
    let g = read_glb(bytes)?;
    let j = &g.json;
    let nodes = j["nodes"].as_array().ok_or("The file has no nodes.")?;
    let node_ok = |v: &Value| v.as_u64().is_some_and(|n| (n as usize) < nodes.len());
    let ext = &j["extensions"];
    let (version, bones, expressions): (u8, Vec<(String, Value)>, usize) = if ext.get("VRMC_vrm").is_some() {
        let v = &ext["VRMC_vrm"];
        let hb = v["humanoid"]["humanBones"].as_object().ok_or("The humanoid is missing.")?;
        let ex = v["expressions"]["preset"].as_object().map(|o| o.len()).unwrap_or(0) + v["expressions"]["custom"].as_object().map(|o| o.len()).unwrap_or(0);
        (1, hb.iter().map(|(k, x)| (k.clone(), x["node"].clone())).collect(), ex)
    } else if ext.get("VRM").is_some() {
        let v = &ext["VRM"];
        let hb = v["humanoid"]["humanBones"].as_array().ok_or("The humanoid is missing.")?;
        (0, hb.iter().map(|x| (x["bone"].as_str().unwrap_or("").to_owned(), x["node"].clone())).collect(), v["blendShapeMaster"]["blendShapeGroups"].as_array().map(|a| a.len()).unwrap_or(0))
    } else if ext.get("VRMC_vrm").is_none() && ext.get("VRM").is_none() && j["skins"].as_array().is_some_and(|s| !s.is_empty()) {
        // a plain GLB for games: the bones are the joints of the skin, named as the humanoid, the expressions are the shapes
        let joints: Vec<(String, Value)> = j["skins"][0]["joints"].as_array().map(|a| a.iter().map(|n| (nodes.get(n.as_u64().unwrap_or(u64::MAX) as usize).and_then(|x| x["name"].as_str()).unwrap_or("").to_owned(), n.clone())).collect()).unwrap_or_default();
        let mut names: std::collections::BTreeSet<String> = std::collections::BTreeSet::new();
        for m in j["meshes"].as_array().unwrap_or(&Vec::new()) {
            for t in m["extras"]["targetNames"].as_array().unwrap_or(&Vec::new()) {
                names.insert(t.as_str().unwrap_or("").to_owned());
            }
        }
        (2, joints, names.len())
    } else {
        return Err("This is a glTF file but not a VRM.".into());
    };
    for need in ["hips", "spine", "head", "leftUpperArm", "leftLowerArm", "leftHand", "rightUpperArm", "rightLowerArm", "rightHand", "leftUpperLeg", "leftLowerLeg", "leftFoot", "rightUpperLeg", "rightLowerLeg", "rightFoot"] {
        match bones.iter().find(|b| b.0 == need) {
            Some((_, n)) if node_ok(n) => {}
            Some(_) => return Err(format!("The bone {need} points at a node that is not there.")),
            None => return Err(format!("The humanoid has no {need} bone.")),
        }
    }
    // skins, meshes
    for s in j["skins"].as_array().unwrap_or(&Vec::new()) {
        if !s["joints"].as_array().is_some_and(|a| a.iter().all(node_ok)) {
            return Err("A skin has a joint that is not a node.".into());
        }
    }
    let meshes = j["meshes"].as_array().ok_or("The file has no meshes.")?;
    let mut vertices = 0;
    for (mi, m) in meshes.iter().enumerate() {
        for p in m["primitives"].as_array().ok_or("A mesh has no primitives.")? {
            let a = &p["attributes"];
            let pos = g.vec3s(a["POSITION"].as_u64().ok_or("A mesh has no positions.")? as usize)?;
            let n = pos.len();
            if pos.iter().any(|v| v.iter().any(|x| !x.is_finite())) {
                return Err(format!("Mesh {mi} has a position that is not a number."));
            }
            if g.vec3s(a["NORMAL"].as_u64().ok_or("A mesh has no normals.")? as usize)?.len() != n || g.vec2s(a["TEXCOORD_0"].as_u64().ok_or("A mesh has no UV.")? as usize)?.len() != n {
                return Err(format!("Mesh {mi}: positions, normals and UV are not the same count."));
            }
            if g.floats(a["WEIGHTS_0"].as_u64().ok_or("A mesh has no skin weights.")? as usize)?.len() != n * 4 {
                return Err(format!("Mesh {mi}: skin weights are not the same count."));
            }
            let idx = g.index_list(p["indices"].as_u64().ok_or("A mesh has no indices.")? as usize)?;
            if idx.len() % 3 != 0 || idx.iter().any(|i| *i as usize >= n) {
                return Err(format!("Mesh {mi}: an index points past the last vertex."));
            }
            for t in p["targets"].as_array().unwrap_or(&Vec::new()) {
                if g.vec3s(t["POSITION"].as_u64().ok_or("A shape has no positions.")? as usize)?.len() != n {
                    return Err(format!("Mesh {mi}: a shape has a different number of vertices."));
                }
            }
            vertices += n;
        }
    }
    // expression binds
    let targets_of = |mesh: usize| meshes.get(mesh).and_then(|m| m["primitives"][0]["targets"].as_array()).map(|t| t.len()).unwrap_or(0);
    if version == 1 {
        let ex = &ext["VRMC_vrm"]["expressions"];
        for group in [&ex["preset"], &ex["custom"]] {
            for (name, e) in group.as_object().into_iter().flatten() {
                for bd in e["morphTargetBinds"].as_array().unwrap_or(&Vec::new()) {
                    let mesh = bd["node"].as_u64().and_then(|n| nodes.get(n as usize)).and_then(|n| n["mesh"].as_u64()).ok_or_else(|| format!("Expression {name} points at a node with no mesh."))? as usize;
                    if bd["index"].as_u64().is_none_or(|i| i as usize >= targets_of(mesh)) {
                        return Err(format!("Expression {name} points at a shape that is not there."));
                    }
                }
            }
        }
    } else {
        for gr in ext["VRM"]["blendShapeMaster"]["blendShapeGroups"].as_array().unwrap_or(&Vec::new()) {
            for bd in gr["binds"].as_array().unwrap_or(&Vec::new()) {
                let mesh = bd["mesh"].as_u64().unwrap_or(u64::MAX) as usize;
                if bd["index"].as_u64().is_none_or(|i| i as usize >= targets_of(mesh)) {
                    return Err("An expression points at a shape that is not there.".into());
                }
            }
        }
    }
    // swinging chains and the balls they avoid
    let spring_nodes: Vec<&Value> = if version == 1 {
        let sb = &ext["VRMC_springBone"];
        let n_col = sb["colliders"].as_array().map(|a| a.len()).unwrap_or(0);
        for c in sb["colliders"].as_array().unwrap_or(&Vec::new()) {
            if !node_ok(&c["node"]) || c["shape"]["sphere"]["radius"].as_f64().is_none_or(|r| !(r > 0.0)) {
                return Err("A collider is not on a node or has no size.".into());
            }
        }
        for gr in sb["colliderGroups"].as_array().unwrap_or(&Vec::new()) {
            if gr["colliders"].as_array().is_some_and(|a| a.iter().any(|i| i.as_u64().is_none_or(|i| i as usize >= n_col))) {
                return Err("A collider group points at a collider that is not there.".into());
            }
        }
        sb["springs"].as_array().into_iter().flatten().flat_map(|s| s["joints"].as_array().into_iter().flatten().map(|j| &j["node"])).collect()
    } else {
        ext["VRM"]["secondaryAnimation"]["boneGroups"].as_array().into_iter().flatten().flat_map(|g| g["bones"].as_array().into_iter().flatten()).collect()
    };
    if spring_nodes.iter().any(|n| !node_ok(n)) {
        return Err("A swinging chain has a joint that is not a node.".into());
    }
    Ok(Summary { version, bones: bones.len(), parts: meshes.len(), vertices, expressions, springs: spring_nodes.len() })
}
