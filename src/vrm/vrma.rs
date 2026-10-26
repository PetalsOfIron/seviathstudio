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
