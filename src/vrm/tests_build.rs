// SPDX-License-Identifier: GPL-3.0-or-later
// Tests of the 3D character, the VRM files in both formats, and the preview.

use super::export::{Format, export, validate};
use super::gltf::{Builder, Glb, pack, read_glb};
use super::model::{Options, build};
use crate::doc::{Document, LayerKind};
use crate::parts::Part;
use crate::rig::Rig;
use crate::rig::autorig::{BodyOptions, FaceOptions, rig_body, rig_face};

/// A character on a 2000 x 3000 page: hair, a face with eyes, brows, nose and mouth, a torso, arms, hands and legs.
pub(crate) fn character() -> Document {
    let mut d = Document::new(2000, 3000);
    let mut add = |name: &str, part: Option<Part>, b: (i32, i32, i32, i32), c: [u8; 3]| {
        let img = image::RgbaImage::from_pixel((b.2 - b.0) as u32, (b.3 - b.1) as u32, image::Rgba([c[0], c[1], c[2], 255]));
        let mut l = d.new_layer(name, LayerKind::Raster);
        l.tiles = crate::files::tiles_from_rgba(&img, b.0, b.1, 2000, 3000);
        l.part = part;
        l.rev += 1;
        d.layers.insert(0, l);
    };
    add("Hair back", Some(Part::HairBack), (700, 150, 1300, 1100), [90, 60, 140]);
    add("Leg L", Some(Part::LegL), (1000, 1700, 1150, 2900), [50, 60, 120]);
    add("Leg R", Some(Part::LegR), (850, 1700, 1000, 2900), [50, 60, 120]);
    add("Torso", Some(Part::Torso), (800, 850, 1200, 1750), [70, 130, 200]);
    add("Arm L", Some(Part::ArmL), (1200, 900, 1330, 1500), [60, 110, 180]);
    add("Hand L", Some(Part::HandL), (1200, 1500, 1330, 1650), [240, 205, 180]);
    add("Arm R", Some(Part::ArmR), (670, 900, 800, 1500), [60, 110, 180]);
    add("Hand R", Some(Part::HandR), (670, 1500, 800, 1650), [240, 205, 180]);
    add("Neck", Some(Part::Neck), (950, 780, 1050, 880), [240, 205, 180]);
    add("Face", Some(Part::Face), (800, 300, 1200, 800), [240, 205, 180]);
    add("Eye L", Some(Part::EyeL), (1050, 500, 1150, 560), [40, 70, 140]);
    add("Eyeball L", Some(Part::EyeballL), (1080, 510, 1120, 550), [20, 20, 30]);
    add("Eyeball R", Some(Part::EyeballR), (880, 510, 920, 550), [20, 20, 30]);
    add("Eye R", Some(Part::EyeR), (850, 500, 950, 560), [40, 70, 140]);
    add("Brow L", Some(Part::BrowL), (1050, 440, 1150, 465), [60, 40, 40]);
    add("Brow R", Some(Part::BrowR), (850, 440, 950, 465), [60, 40, 40]);
    add("Nose", Some(Part::Nose), (975, 580, 1025, 660), [220, 170, 150]);
    add("Mouth", Some(Part::Mouth), (940, 690, 1060, 730), [200, 80, 90]);
    add("Hair front", Some(Part::HairFront), (780, 250, 1220, 430), [110, 70, 170]);
    d.mark_all_dirty();
    let mut rig = Rig::default();
    rig_face(&mut rig, &d.layers, d.w, d.h, &FaceOptions::default()).unwrap();
    rig_body(&mut rig, &d.layers, d.w, d.h, &BodyOptions::default()).unwrap();
    d.rig = std::sync::Arc::new(rig);
    d
}

#[test]
fn a_character_becomes_a_3d_model_with_a_skeleton_skin_and_expressions() {
    let d = character();
    let m = build(&d, &Options { name: "Test".into(), author: "Me".into(), ..Default::default() }).unwrap();
    assert_eq!(m.parts.len(), d.rig.meshes.len(), "every meshed part");
    // the size: 1.6 m tall, standing on the floor, centred
    let (mut lo, mut hi) = ([f32::MAX; 3], [f32::MIN; 3]);
    for p in &m.parts {
        for v in &p.verts {
            for k in 0..3 {
                lo[k] = lo[k].min(v[k]);
                hi[k] = hi[k].max(v[k]);
            }
        }
    }
    assert!((hi[1] - lo[1] - 1.6).abs() < 0.01 && lo[1].abs() < 0.01, "{lo:?} {hi:?}");
    assert!(lo[0] > -0.6 && hi[0] < 0.6, "centred");
    // depth: the front hair is in front of the face, which is in front of the back hair
    let z = |n: &str| {
        let p = m.parts.iter().find(|p| p.name == n).unwrap();
        p.verts.iter().map(|v| v[2]).sum::<f32>() / p.verts.len() as f32
    };
    assert!(z("Hair front") > z("Hair back") && z("Face") > z("Hair back"), "{} {} {}", z("Hair front"), z("Face"), z("Hair back"));
    // a part sits on the surface of what is under it: the eye is on the bulge of the face, not inside it
    assert!(z("Eye L") > z("Face") - 0.003, "{} {}", z("Eye L"), z("Face"));
    let eye = m.parts.iter().find(|p| p.name == "Eye L").unwrap();
    assert!(eye.verts.iter().all(|v| v[2] > 0.0), "in front of the middle of the face");
    // the parts are round: the middle of a part is in front of its edge
    let face = m.parts.iter().find(|p| p.name == "Face").unwrap();
    let (zmin, zmax) = face.verts.iter().fold((f32::MAX, f32::MIN), |a, v| (a.0.min(v[2]), a.1.max(v[2])));
    assert!(zmax - zmin > 0.005, "{}", zmax - zmin);
    // the skeleton: the bones that games need, in the right places
    for b in ["hips", "spine", "head", "leftUpperArm", "rightHand", "leftFoot", "rightLowerLeg"] {
        assert!(m.bone(b).is_some(), "{b}");
    }
    let y = |n: &str| m.bones[m.bone(n).unwrap()].pos[1];
    assert!(y("head") > y("chest") && y("chest") > y("spine") && y("spine") > y("hips") && y("hips") > y("leftLowerLeg") && y("leftLowerLeg") > y("leftFoot"), "bones stand above each other");
    assert!(m.bones[m.bone("leftUpperArm").unwrap()].pos[0] > 0.0 && m.bones[m.bone("rightUpperArm").unwrap()].pos[0] < 0.0, "left is +X, the right of the picture");
    // skin: head parts follow the head only, a left arm follows the left arm bones, the weights add up to one
    let head = m.bone("head").unwrap() as u16;
    let hair = m.parts.iter().find(|p| p.name == "Hair front").unwrap();
    assert!(hair.joints.iter().all(|j| j[0] == head) && hair.weights.iter().all(|w| w[0] == 1.0));
    let arm = m.parts.iter().find(|p| p.name == "Arm L").unwrap();
    let arm_bones: Vec<u16> = ["leftUpperArm", "leftLowerArm", "leftHand", "chest"].iter().map(|n| m.bone(n).unwrap() as u16).collect();
    assert!(arm.joints.iter().zip(&arm.weights).all(|(j, w)| (0..4).filter(|k| w[*k] > 0.0).all(|k| arm_bones.contains(&j[k]))), "only the arm's own bones");
    for p in &m.parts {
        assert!(p.weights.iter().all(|w| (w.iter().sum::<f32>() - 1.0).abs() < 1e-4));
        assert!(p.normals.iter().all(|n| (n[0] * n[0] + n[1] * n[1] + n[2] * n[2] - 1.0).abs() < 1e-3));
        assert!(p.tris.iter().all(|t| t.iter().all(|i| (*i as usize) < p.verts.len())));
    }
    // the expressions the face sliders make
    let names: Vec<&str> = m.expressions.iter().map(|e| e.name.as_str()).collect();
    for want in ["blink", "aa", "ih", "ou", "ee", "oh", "happy", "sad", "surprised", "jawOpen"] {
        assert!(names.contains(&want), "{want} in {names:?}");
    }
    assert!(names.iter().any(|n| n.starts_with("look")), "the eyes can look somewhere: {names:?}");
    // blinking moves the eyes; the mouth shape moves the mouth
    let blink = m.expressions.iter().find(|e| e.name == "blink").unwrap();
    assert!(blink.binds.iter().any(|(p, _, _)| m.parts[*p].name.starts_with("Eye")));
    let aa = m.expressions.iter().find(|e| e.name == "aa").unwrap();
    assert!(aa.binds.iter().any(|(p, _, _)| m.parts[*p].name == "Mouth"));
}

#[test]
fn exported_vrms_are_valid_in_both_formats_and_read_back() {
    let d = character();
    let m = build(&d, &Options { name: "Test".into(), author: "Me".into(), ..Default::default() }).unwrap();
    let (one, zero) = (export(&m, Format::Vrm1).unwrap(), export(&m, Format::Vrm0).unwrap());
    let s1 = validate(&one).unwrap();
    let s0 = validate(&zero).unwrap();
    assert_eq!((s1.version, s0.version), (1, 0));
    assert_eq!(s1.parts, m.parts.len());
    assert_eq!((s1.bones, s1.vertices), (s0.bones, s0.vertices));
    assert!(s1.expressions >= 8 && s0.expressions >= 8);
    // 1.0 has the humanoid as a map and the ARKit names for face tracking; 0.x has a list and presets
    let g1 = read_glb(&one).unwrap();
    let v1 = &g1.json["extensions"]["VRMC_vrm"];
    assert_eq!(v1["specVersion"], "1.0");
    assert!(v1["expressions"]["preset"].get("blink").is_some() && v1["expressions"]["preset"].get("aa").is_some());
    assert!(v1["expressions"]["custom"].get("jawOpen").is_some(), "ARKit name for face tracking");
    assert_eq!(v1["meta"]["authors"][0], "Me");
    assert!(g1.json["extensionsUsed"].as_array().unwrap().iter().any(|e| e == "VRMC_vrm"));
    let g0 = read_glb(&zero).unwrap();
    let groups = g0.json["extensions"]["VRM"]["blendShapeMaster"]["blendShapeGroups"].as_array().unwrap();
    assert!(groups.iter().any(|g| g["presetName"] == "blink") && groups.iter().any(|g| g["presetName"] == "a") && groups.iter().any(|g| g["presetName"] == "joy"));
    // 0.x faces -Z: the model is turned half way round, so the left arm is on the other side and the face looks the other way
    let x_of = |g: &Glb, name: &str| g.json["nodes"].as_array().unwrap().iter().find(|n| n["name"] == name).unwrap()["translation"][0].as_f64().unwrap();
    assert!(x_of(&g1, "leftUpperArm") * x_of(&g0, "leftUpperArm") < 0.0);
    // a file with a bone taken out, a shape that does not exist, or a bad index is refused
    let tamper = |f: &dyn Fn(&mut serde_json::Value)| {
        let mut g = read_glb(&one).unwrap();
        f(&mut g.json);
        pack(&g.json, &g.bin)
    };
    let no_hips = tamper(&|j| {
        j["extensions"]["VRMC_vrm"]["humanoid"]["humanBones"].as_object_mut().unwrap().remove("hips");
    });
    assert!(validate(&no_hips).unwrap_err().contains("hips"));
    let bad_shape = tamper(&|j| {
        j["extensions"]["VRMC_vrm"]["expressions"]["preset"]["blink"]["morphTargetBinds"][0]["index"] = serde_json::json!(999);
    });
    assert!(validate(&bad_shape).is_err());
    let bad_index = tamper(&|j| {
        j["extensions"]["VRMC_vrm"]["humanoid"]["humanBones"]["head"]["node"] = serde_json::json!(99999);
    });
    assert!(validate(&bad_index).is_err());
    assert!(validate(b"not a model").is_err());
    let mut plain = Builder::new("t");
    plain.nodes.push(serde_json::json!({}));
    plain.scenes.push(serde_json::json!({ "nodes": [0] }));
    assert!(validate(&plain.glb()).is_err(), "a glTF that is not a VRM");
}

#[test]
fn the_preview_turns_round_and_shows_expressions() {
    let d = character();
    let m = build(&d, &Options::default()).unwrap();
    let _ = std::fs::create_dir_all("target/screens");
    let front = super::preview::render(&m, 0.0, 480, 800, &[]);
    let turned = super::preview::render(&m, 40.0, 480, 800, &[]);
    let blink = super::preview::render(&m, 0.0, 480, 800, &[("blink", 1.0)]);
    let mouth = super::preview::render(&m, 0.0, 480, 800, &[("aa", 1.0)]);
    let ink = |img: &image::RgbaImage| img.pixels().filter(|p| p.0 != [244, 242, 236, 255]).count();
    assert!(ink(&front) > 20_000, "the character is drawn");
    assert_ne!(front.as_raw(), turned.as_raw(), "turning round changes the picture");
    assert_ne!(front.as_raw(), blink.as_raw(), "a blink changes the picture");
    assert_ne!(front.as_raw(), mouth.as_raw(), "the mouth shape changes the picture");
    // a sheet of the views for looking at
    let mut sheet = image::RgbaImage::new(480 * 4, 800);
    for (i, img) in [&front, &turned, &blink, &mouth].iter().enumerate() {
        image::imageops::replace(&mut sheet, *img, (i as i64) * 480, 0);
    }
    sheet.save("target/screens/vrm_preview.png").unwrap();
}

