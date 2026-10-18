// SPDX-License-Identifier: GPL-3.0-or-later
// The VRM engine: our own glTF reader and writer, and (next to it) the VRM 1.0 and 0.x model builder for games.

pub mod gltf;
pub mod export;
pub mod face;
pub mod hair;
pub mod hairedit;
pub mod hands;
pub mod model;
pub mod paint3d;
pub mod preview;
pub mod shape;
pub mod svrm;
pub mod template;
pub mod texedit;

#[cfg(test)]
mod tests {
    use super::gltf::read_glb;

    /// The user's real models, when they are on this computer: our reader must open every one, find the humanoid
    /// and the face shapes, and read the mesh numbers.
    #[test]
    fn real_vrm_files_open_with_our_reader() {
        let base = std::path::PathBuf::from(std::env::var("USERPROFILE").unwrap_or_default()).join("Downloads/art/Finished2D3D");
        for rel in ["VrmSem/Sem.vrm", "VrmSem/Sem0.vrm", "vrmemi/Emi.vrm", "vrmemi/Emi1.0.vrm"] {
            let path = base.join(rel);
            if !path.exists() {
                continue;
            }
            let g = read_glb(&std::fs::read(&path).unwrap()).unwrap_or_else(|e| panic!("{rel}: {e}"));
            let ext = &g.json["extensions"];
            let one = ext.get("VRMC_vrm").is_some();
            assert!(one || ext.get("VRM").is_some(), "{rel} is a VRM");
            // the humanoid has the bones every game needs
            let has = |name: &str| if one { ext["VRMC_vrm"]["humanoid"]["humanBones"].get(name).is_some() } else { ext["VRM"]["humanoid"]["humanBones"].as_array().is_some_and(|a| a.iter().any(|b| b["bone"] == name)) };
            for bone in ["hips", "spine", "head", "leftUpperArm", "rightUpperLeg", "leftFoot"] {
                assert!(has(bone), "{rel}: {bone}");
            }
            // the first mesh is the face with its shapes, and its numbers read back
            let prim = &g.json["meshes"][0]["primitives"][0];
            assert!(prim["targets"].as_array().is_some_and(|t| t.len() > 40), "{rel}: face shapes");
            let pos = g.vec3s(prim["attributes"]["POSITION"].as_u64().unwrap() as usize).unwrap();
            assert!(pos.len() > 100 && pos.iter().all(|p| p.iter().all(|v| v.is_finite())), "{rel}");
            let idx = g.index_list(prim["indices"].as_u64().unwrap() as usize).unwrap();
            assert!(idx.iter().all(|i| (*i as usize) < pos.len()), "{rel}: indices in range");
            eprintln!("{rel}: {} nodes, {} vertices in the face, VRM {}", g.json["nodes"].as_array().unwrap().len(), pos.len(), if one { "1.0" } else { "0.x" });
        }
    }
}

#[cfg(test)]
pub(crate) mod tests_build;
