// SPDX-License-Identifier: GPL-3.0-or-later
// A VRM animation file (.vrma): the turns of humanoid bones over time, as VRMC_vrm_animation (glTF) that VRM apps and game
// engines play on any VRM model. The 3D bone sliders turn a bone in the picture plane, which is a turn around the Z axis.

use super::gltf::Builder;
use serde_json::json;

/// One bone with its turn in degrees at every frame (a turn in the picture plane, as the bone sliders make it).
pub struct BoneTurns {
    pub bone: String,
    pub degrees: Vec<f32>,
}

/// The quaternion (x, y, z, w) of a turn around Z.
fn turn_z(deg: f32) -> [f32; 4] {
    let h = deg.to_radians() * 0.5;
    [0.0, 0.0, h.sin(), h.cos()]
}

/// The .vrma file for `turns` at `fps` frames a second. The hips are always in it (the format needs them).
pub fn write_vrma(fps: f32, turns: &[BoneTurns]) -> Vec<u8> {
    let mut b = Builder::new("SeviathStudio");
    let mut names: Vec<&str> = vec!["hips"];
    for t in turns {
        if !names.contains(&t.bone.as_str()) {
            names.push(&t.bone);
        }
    }
    let mut bones = serde_json::Map::new();
    for (i, n) in names.iter().enumerate() {
        b.nodes.push(json!({ "name": n }));
        bones.insert((*n).to_owned(), json!({ "node": i }));
    }
    let mut samplers = Vec::new();
    let mut channels = Vec::new();
    for t in turns {
        let node = names.iter().position(|n| *n == t.bone).unwrap_or(0);
        let times: Vec<f32> = (0..t.degrees.len()).map(|f| f as f32 / fps.max(1.0)).collect();
        let quats: Vec<f32> = t.degrees.iter().flat_map(|d| turn_z(*d)).collect();
        let input = b.animation_data(&times, "SCALAR");
        let output = b.animation_data(&quats, "VEC4");
        samplers.push(json!({ "input": input, "output": output, "interpolation": "LINEAR" }));
        channels.push(json!({ "sampler": samplers.len() - 1, "target": { "node": node, "path": "rotation" } }));
    }
    b.animations.push(json!({ "name": "SeviathStudio", "samplers": samplers, "channels": channels }));
    b.scenes.push(json!({ "nodes": (0..names.len()).collect::<Vec<_>>() }));
    b.extensions_used.push("VRMC_vrm_animation".into());
    b.extensions.insert("VRMC_vrm_animation".into(), json!({ "specVersion": "1.0", "humanoid": { "humanBones": bones } }));
    b.glb()
}

/// A bone that an animation turns: its name, its node in the file, and where the node is at rest (relative to its parent).
pub struct Target {
    pub bone: &'static str,
    pub node: usize,
    pub rest: [f32; 3],
}

/// Add one animation (a motion's frames at `fps`) to a file that has the nodes of `targets`. `flip` is for VRM 0.x, which faces the other way.
pub fn add_animation(b: &mut Builder, name: &str, fps: f32, frames: &[crate::motion::Frame], targets: &[Target], flip: bool) {
    if frames.is_empty() {
        return;
    }
    let times: Vec<f32> = (0..frames.len()).map(|i| i as f32 / fps.max(1.0)).collect();
    let mut samplers = Vec::new();
    let mut channels = Vec::new();
    for t in targets {
        let moves = frames.iter().any(|f| f.get(t.bone) != [0.0; 3]);
        if moves {
            let mut prev: Option<[f32; 4]> = None;
            let mut data: Vec<f32> = Vec::with_capacity(frames.len() * 4);
            for f in frames {
                let mut q = f.quat(t.bone);
                if flip {
                    q = [-q[0], q[1], -q[2], q[3]];
                }
                // the nearer of the two quaternions of the same turn, so the file does not spin the long way
                if let Some(p) = prev {
                    if p[0] * q[0] + p[1] * q[1] + p[2] * q[2] + p[3] * q[3] < 0.0 {
                        q = [-q[0], -q[1], -q[2], -q[3]];
                    }
                }
                prev = Some(q);
                data.extend(q);
            }
            let input = b.animation_data(&times, "SCALAR");
            let output = b.animation_data(&data, "VEC4");
            samplers.push(json!({ "input": input, "output": output, "interpolation": "LINEAR" }));
            channels.push(json!({ "sampler": samplers.len() - 1, "target": { "node": t.node, "path": "rotation" } }));
        }
        if t.bone == "hips" && frames.iter().any(|f| f.hips != [0.0; 3]) {
            let data: Vec<f32> = frames
                .iter()
                .flat_map(|f| {
                    let h = if flip { [-f.hips[0], f.hips[1], -f.hips[2]] } else { f.hips };
                    [t.rest[0] + h[0], t.rest[1] + h[1], t.rest[2] + h[2]]
                })
                .collect();
            let input = b.animation_data(&times, "SCALAR");
            let output = b.animation_data(&data, "VEC3");
            samplers.push(json!({ "input": input, "output": output, "interpolation": "LINEAR" }));
            channels.push(json!({ "sampler": samplers.len() - 1, "target": { "node": t.node, "path": "translation" } }));
        }
    }
    b.animations.push(json!({ "name": name, "samplers": samplers, "channels": channels }));
}

/// A .vrma file for one animation of the character: the skeleton in its T-pose (so a player knows the rest), and the turns of the bones.
pub fn write_vrma_clip(model: &crate::vrm::model::Model3d, clip: &crate::motion::Clip, fps: f32) -> Vec<u8> {
    let mut b = Builder::new("SeviathStudio");
    // the humanoid bones that this model has, with the nearest one above each as its parent
    let keep: Vec<usize> = (0..model.bones.len()).filter(|i| model.bones[*i].humanoid && crate::motion::BONES.contains(&model.bones[*i].name.as_str())).collect();
    let node_of = |bone: usize| keep.iter().position(|k| *k == bone);
    let parent_of = |bone: usize| -> Option<usize> {
        let mut p = model.bones[bone].parent;
        while let Some(i) = p {
            if node_of(i).is_some() {
                return Some(i);
            }
            p = model.bones[i].parent;
        }
        None
    };
    let mut targets = Vec::new();
    let mut humanoid = serde_json::Map::new();
    let mut roots = Vec::new();
    for (n, &bi) in keep.iter().enumerate() {
        let bone = &model.bones[bi];
        let par = parent_of(bi);
        let rest = match par {
            Some(p) => [bone.pos[0] - model.bones[p].pos[0], bone.pos[1] - model.bones[p].pos[1], bone.pos[2] - model.bones[p].pos[2]],
            None => bone.pos,
        };
        let children: Vec<usize> = keep.iter().enumerate().filter(|(_, c)| parent_of(**c) == Some(bi)).map(|(k, _)| k).collect();
        let mut node = json!({ "name": bone.name, "translation": rest });
        if !children.is_empty() {
            node["children"] = json!(children);
        }
        b.nodes.push(node);
        if par.is_none() {
            roots.push(n);
        }
        humanoid.insert(bone.name.clone(), json!({ "node": n }));
        if let Some(name) = crate::motion::BONES.iter().find(|x| **x == bone.name) {
            targets.push(Target { bone: name, node: n, rest });
        }
    }
    add_animation(&mut b, &clip.name, fps, &clip.frames(fps), &targets, false);
    b.scenes.push(json!({ "nodes": roots }));
    b.extensions_used.push("VRMC_vrm_animation".into());
    b.extensions.insert("VRMC_vrm_animation".into(), json!({ "specVersion": "1.0", "humanoid": { "humanBones": humanoid } }));
    b.glb()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::vrm::gltf::read_glb;

    #[test]
    fn a_turning_bone_makes_a_vrma_that_reads_back() {
        let file = write_vrma(24.0, &[BoneTurns { bone: "leftUpperArm".into(), degrees: vec![0.0, 45.0, 90.0] }]);
        let g = read_glb(&file).expect("a glTF binary");
        let ext = &g.json["extensions"]["VRMC_vrm_animation"];
        assert_eq!(ext["specVersion"], "1.0");
        assert!(ext["humanoid"]["humanBones"]["hips"].is_object());
        assert_eq!(ext["humanoid"]["humanBones"]["leftUpperArm"]["node"], 1);
        let a = &g.json["animations"][0];
        assert_eq!(a["channels"][0]["target"]["path"], "rotation");
        let out = a["samplers"][0]["output"].as_u64().unwrap() as usize;
        let acc = &g.json["accessors"][out];
        assert_eq!(acc["count"], 3);
        assert_eq!(acc["type"], "VEC4");
        // the last key is 90 degrees around Z: z = w = sqrt(0.5)
        let view = &g.json["bufferViews"][acc["bufferView"].as_u64().unwrap() as usize];
        let at = view["byteOffset"].as_u64().unwrap() as usize + 2 * 16;
        let f = |o: usize| f32::from_le_bytes(g.bin[at + o..at + o + 4].try_into().unwrap());
        assert!((f(8) - 0.7071).abs() < 1e-3 && (f(12) - 0.7071).abs() < 1e-3);
    }
}
