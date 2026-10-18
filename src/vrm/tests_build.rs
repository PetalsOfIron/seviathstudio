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


#[test]
fn physics_chains_become_spring_bones_and_colliders_in_both_formats() {
    use crate::rig::physics::{Collider, Kind as PKind, Material, PhysChain, make_line};
    let mut d = character();
    {
        let rig = std::sync::Arc::make_mut(&mut d.rig);
        let layer = d.layers.iter().find(|l| l.name == "Hair back").unwrap().id;
        let mi = rig.meshes.iter().position(|m| m.layer == layer).unwrap();
        let (mut lo, mut hi) = ([f32::MAX; 2], [f32::MIN; 2]);
        for v in &rig.meshes[mi].verts {
            for k in 0..2 {
                lo[k] = lo[k].min(v[k]);
                hi[k] = hi[k].max(v[k]);
            }
        }
        let line = make_line(rig, "Hair back", [lo[0], lo[1], hi[0], hi[1]], [1000.0, -10000.0], 5, None);
        rig.meshes[mi].parent = Some(line);
        rig.physics.push(PhysChain::new("Hair back", PKind::Hair, Material::Normal, line));
        rig.colliders.push(Collider { name: "Head".into(), center: [1000.0, 550.0], radius: 260.0, changer: None });
    }
    let m = build(&d, &Options { name: "Test".into(), author: "Me".into(), ..Default::default() }).unwrap();
    assert_eq!(m.springs.len(), 1);
    let sp = &m.springs[0];
    assert_eq!(sp.joints.len(), 5);
    assert!(sp.stiffness > 0.3 && sp.stiffness < 2.5 && sp.drag > 0.0 && sp.gravity > 0.0, "{} {} {}", sp.stiffness, sp.drag, sp.gravity);
    // the joints hang down from the head: each lower than the one before, the first fixed to the head bone
    let ys: Vec<f32> = sp.joints.iter().map(|j| m.bones[*j].pos[1]).collect();
    assert!(ys.windows(2).all(|w| w[1] < w[0]), "{ys:?}");
    assert_eq!(m.bones[sp.joints[0]].parent, Some(m.bone("head").unwrap()));
    assert!(m.bones[sp.joints[0]].humanoid == false && m.bone("head").is_some());
    // the hair follows the joints of the chain, not the head
    let hair = m.parts.iter().find(|p| p.name == "Hair back").unwrap();
    let humanoid = m.bones.iter().filter(|b| b.humanoid).count() as u16;
    assert!(hair.joints.iter().zip(&hair.weights).all(|(j, w)| (0..4).filter(|k| w[*k] > 0.0).all(|k| j[k] >= humanoid)), "only chain joints");
    assert_eq!(m.colliders.len(), 1);
    assert_eq!(m.colliders[0].bone, m.bone("head").unwrap());
    // both formats say it, and are valid
    let (one, zero) = (export(&m, Format::Vrm1).unwrap(), export(&m, Format::Vrm0).unwrap());
    let (s1, s0) = (validate(&one).unwrap(), validate(&zero).unwrap());
    assert_eq!((s1.springs, s0.springs), (5, 1));
    let g1 = read_glb(&one).unwrap();
    let sb = &g1.json["extensions"]["VRMC_springBone"];
    assert_eq!(sb["springs"][0]["joints"].as_array().unwrap().len(), 5);
    assert!(sb["springs"][0]["joints"][4].get("stiffness").is_none(), "the last joint is only a place");
    assert_eq!(sb["colliders"].as_array().unwrap().len(), 1);
    assert!(g1.json["extensionsUsed"].as_array().unwrap().iter().any(|e| e == "VRMC_springBone"));
    let g0 = read_glb(&zero).unwrap();
    let groups = g0.json["extensions"]["VRM"]["secondaryAnimation"]["boneGroups"].as_array().unwrap();
    assert_eq!(groups.len(), 1);
    assert!(groups[0]["stiffiness"].as_f64().unwrap() > 0.3, "the 0.x spelling of the field");
    // the humanoid still has only the humanoid bones
    assert!(g1.json["extensions"]["VRMC_vrm"]["humanoid"]["humanBones"].as_object().unwrap().keys().all(|k| !k.starts_with("J_Sec")));
    // a collider group that points nowhere is refused
    let mut g = read_glb(&one).unwrap();
    g.json["extensions"]["VRMC_springBone"]["colliderGroups"][0]["colliders"] = serde_json::json!([5]);
    assert!(validate(&pack(&g.json, &g.bin)).is_err());
}

/// A document with one layer holding the whole picture (an uncut character).
pub(crate) fn uncut_doc(img: &image::RgbaImage) -> Document {
    let mut d = Document::new(img.width(), img.height());
    let mut l = d.new_layer("Character", LayerKind::Raster);
    l.tiles = crate::files::tiles_from_rgba(img, 0, 0, img.width(), img.height());
    l.rev += 1;
    d.layers.insert(0, l);
    d.mark_all_dirty();
    d
}

#[test]
fn an_uncut_figure_is_rigged_and_becomes_a_valid_vrm_in_both_formats() {
    use super::model::build_any;
    let img = crate::rig::uncut::tests::figure_image();
    let mut d = uncut_doc(&img);
    let ids: Vec<crate::doc::LayerId> = d.layers.iter().map(|l| l.id).collect();
    let mut rig = Rig::default();
    let fig = crate::rig::uncut::auto_rig(&mut rig, &d, &ids, 0.6).unwrap();
    assert!(fig.confidence > 0.6);
    d.rig = std::sync::Arc::new(rig);
    let sk = d.rig.skin.as_ref().unwrap();
    assert!(sk.bones.len() >= 15 && sk.verts.len() > 50);
    let m = build_any(&d, &Options { name: "Uncut".into(), author: "Me".into(), ..Default::default() }).unwrap();
    assert_eq!(m.parts.len(), 2, "the body and the inside of the mouth");
    for b in ["hips", "spine", "head", "leftUpperArm", "rightHand", "leftFoot", "rightLowerLeg"] {
        assert!(m.bone(b).is_some(), "{b}");
    }
    // 1.6 m tall, standing on the floor
    let top = m.parts[0].verts.iter().map(|v| v[1]).fold(0.0f32, f32::max);
    let bottom = m.parts[0].verts.iter().map(|v| v[1]).fold(f32::MAX, f32::min);
    assert!((top - bottom - 1.6).abs() < 0.02 && bottom.abs() < 0.02, "{bottom} {top}");
    // an arm vertex follows the arm bones; a leg vertex the leg bones; all add up to one
    let p = &m.parts[0];
    let hand = m.bone("leftHand").unwrap() as u16;
    let near_hand = (0..p.verts.len()).min_by(|a, b| (p.verts[*a][0] - 0.8).abs().total_cmp(&(p.verts[*b][0] - 0.8).abs())).unwrap();
    let hand_side = |j: u16| m.bones[j as usize].name.starts_with("left") && (m.bones[j as usize].name.ends_with("Hand") || ["Thumb", "Index", "Middle", "Ring", "Little"].iter().any(|f| m.bones[j as usize].name.contains(f)));
    assert!(p.joints[near_hand].iter().zip(&p.weights[near_hand]).any(|(j, w)| (*j == hand || hand_side(*j)) && *w > 0.4), "the fingertip follows the hand or its fingers: {:?} {:?}", p.joints[near_hand], p.weights[near_hand]);
    assert!(p.weights.iter().all(|w| (w.iter().sum::<f32>() - 1.0).abs() < 1e-3));
    // both formats are valid
    let (one, zero) = (export(&m, Format::Vrm1).unwrap(), export(&m, Format::Vrm0).unwrap());
    let (s1, s0) = (validate(&one).unwrap(), validate(&zero).unwrap());
    assert_eq!((s1.version, s0.version), (1, 0));
    assert_eq!(s1.parts, 2);
    // the preview turns round
    let front = super::preview::render(&m, 0.0, 400, 520, &[]);
    let side = super::preview::render(&m, 50.0, 400, 520, &[]);
    assert_ne!(front.as_raw(), side.as_raw());
}

/// The user's uncut bases, when they are on this computer: every one is rigged, built and checked as a VRM, and a
/// contact sheet of the 3D result (front and turned) is written to target/screens.
#[test]
fn real_uncut_characters_become_checked_vrms() {
    use super::model::build_any;
    let base = std::path::PathBuf::from(std::env::var("USERPROFILE").unwrap_or_default()).join("Downloads/Bases/images/races/images");
    let _ = std::fs::create_dir_all("target/screens");
    for folder in ["female", "male"] {
        let Ok(rd) = std::fs::read_dir(base.join(folder)) else { continue };
        let mut files: Vec<std::path::PathBuf> = rd.flatten().map(|e| e.path()).filter(|p| p.extension().is_some_and(|e| e == "png")).collect();
        files.sort();
        let (tw, th) = (200u32, 260u32);
        let cols = 6u32;
        let rows = ((files.len() as u32) * 2).div_ceil(cols).max(1);
        let mut sheet = image::RgbaImage::from_pixel(cols * tw, rows * th, image::Rgba([255, 255, 255, 255]));
        for (i, f) in files.iter().enumerate() {
            let Ok(bytes) = std::fs::read(f) else { continue };
            let Ok(dynimg) = image::load_from_memory(&bytes) else { continue };
            let img = dynimg.to_rgba8();
            let mut d = uncut_doc(&img);
            let ids: Vec<crate::doc::LayerId> = d.layers.iter().map(|l| l.id).collect();
            let mut rig = Rig::default();
            let name = f.file_name().unwrap().to_string_lossy().to_string();
            let fig = crate::rig::uncut::auto_rig(&mut rig, &d, &ids, 0.5).unwrap_or_else(|e| panic!("{folder}/{name}: {e}"));
            d.rig = std::sync::Arc::new(rig);
            let t0 = std::time::Instant::now();
            let m = build_any(&d, &Options { name: name.clone(), author: "test".into(), max_texture: 1024, ..Default::default() }).unwrap_or_else(|| panic!("{folder}/{name}: no model"));
            let bytes1 = export(&m, Format::Vrm1).unwrap();
            let bytes0 = export(&m, Format::Vrm0).unwrap();
            let (s1, s0) = (validate(&bytes1).unwrap_or_else(|e| panic!("{folder}/{name} 1.0: {e}")), validate(&bytes0).unwrap_or_else(|e| panic!("{folder}/{name} 0.x: {e}")));
            eprintln!("{folder}/{name}: {:.0}% sure, {} vertices, {} bones, VRM {} KB / {} KB, {:?}", fig.confidence * 100.0, s1.vertices, s1.bones, bytes1.len() / 1024, bytes0.len() / 1024, t0.elapsed());
            assert_eq!((s1.version, s0.version), (1, 0));
            // keep the files, to open them in a VRM viewer
            let stem = f.file_stem().unwrap().to_string_lossy().to_string();
            let out = std::path::Path::new("target/vrm").join(folder);
            let _ = std::fs::create_dir_all(&out);
            std::fs::write(out.join(format!("{stem}.vrm")), &bytes1).unwrap();
            std::fs::write(out.join(format!("{stem}_vrm0.vrm")), &bytes0).unwrap();
            // front, and with both arms raised and the head tilted (the skin weights at work)
            let up = [("leftUpperArm", 55.0f32), ("rightUpperArm", -55.0), ("head", 12.0), ("leftUpperLeg", -12.0), ("rightUpperLeg", 12.0)];
            for (k, yaw) in [0.0f32, 0.0].iter().enumerate() {
                let view = if k == 0 { super::preview::render(&m, *yaw, tw, th, &[]) } else { super::preview::render_posed(&m, *yaw, tw, th, &[], &up) };
                let slot = (i as u32) * 2 + k as u32;
                image::imageops::replace(&mut sheet, &view, ((slot % cols) * tw) as i64, ((slot / cols) * th) as i64);
            }
        }
        sheet.save(format!("target/screens/vrm_uncut_{folder}.png")).unwrap();
    }
}

/// Figures with the arms at the sides and the legs together (told so): rigged, built, checked, and posed with the left
/// arm raised and the left leg lifted, to see that the weights bend the right parts.
#[test]
fn closed_pose_figures_are_rigged_with_hints_and_bend_where_they_should() {
    use super::model::build_any;
    use crate::rig::uncut::{Found, Hints, Hold, apply, find};
    let base = std::path::PathBuf::from(std::env::var("USERPROFILE").unwrap_or_default()).join("Downloads/Bases/images/races/images/male");
    let names = ["Goblinbase4.png", "InfernalBase.png", "OgreBase.png", "WolfBeastsBase.png", "Merfolkbase.png"];
    let (tw, th) = (200u32, 280u32);
    let mut sheet = image::RgbaImage::from_pixel(names.len() as u32 * tw * 2, th, image::Rgba([255, 255, 255, 255]));
    let mut any = false;
    for (i, n) in names.iter().enumerate() {
        let Ok(bytes) = std::fs::read(base.join(n)) else { continue };
        let img = image::load_from_memory(&bytes).unwrap().to_rgba8();
        let mut d = uncut_doc(&img);
        let ids: Vec<crate::doc::LayerId> = d.layers.iter().map(|l| l.id).collect();
        let f: Found = find(&d, &ids, 0.5, Hints { arms: Hold::Down, legs: Hold::Down }).unwrap();
        let mut rig = Rig::default();
        apply(&mut rig, (d.w, d.h), &ids, &f);
        d.rig = std::sync::Arc::new(rig);
        let m = build_any(&d, &Options { name: n.to_string(), max_texture: 1024, ..Default::default() }).unwrap();
        assert!(validate(&export(&m, Format::Vrm1).unwrap()).is_ok());
        let pose = [("leftUpperArm", -70.0f32), ("leftLowerArm", -20.0), ("leftUpperLeg", -25.0), ("head", 10.0)];
        for (k, p) in [&[][..], &pose[..]].iter().enumerate() {
            let view = super::preview::render_posed(&m, 0.0, tw, th, &[], p);
            image::imageops::replace(&mut sheet, &view, ((i as u32 * 2 + k as u32) * tw) as i64, 0);
        }
        any = true;
    }
    if any {
        sheet.save("target/screens/vrm_uncut_closed.png").unwrap();
    }
}

/// What a viewer does with the model: it starts from the T pose and swings the arms down to an idle pose.
#[test]
fn arms_swung_down_from_the_t_pose_keep_the_body_whole() {
    use super::model::build_any;
    let base = std::path::PathBuf::from(std::env::var("USERPROFILE").unwrap_or_default()).join("Downloads/Bases/images/races/images");
    let files = [("male", "ghoulbase.png"), ("female", "PixieBase.png"), ("male", "OgreBase.png"), ("male", "WolfBeastsBase.png")];
    let mut sheet = image::RgbaImage::from_pixel(files.len() as u32 * 300, 380, image::Rgba([255, 255, 255, 255]));
    let mut any = false;
    for (i, (folder, name)) in files.iter().enumerate() {
        let Ok(bytes) = std::fs::read(base.join(folder).join(name)) else { continue };
        let img = image::load_from_memory(&bytes).unwrap().to_rgba8();
        let mut d = uncut_doc(&img);
        let ids: Vec<crate::doc::LayerId> = d.layers.iter().map(|l| l.id).collect();
        let mut rig = Rig::default();
        crate::rig::uncut::auto_rig(&mut rig, &d, &ids, 0.5).unwrap();
        d.rig = std::sync::Arc::new(rig);
        let m = build_any(&d, &Options { max_texture: 1024, ..Default::default() }).unwrap();
        // the rest pose is a real T pose: arms level, legs plumb
        let pos = |n: &str| m.bones[m.bone(n).unwrap()].pos;
        let (ua, h) = (pos("leftUpperArm"), pos("leftHand"));
        assert!((ua[1] - h[1]).abs() < 0.01 && h[0] > ua[0], "{name}: left arm level {ua:?} {h:?}");
        let (ur, hr) = (pos("rightUpperArm"), pos("rightHand"));
        assert!((ur[1] - hr[1]).abs() < 0.01 && hr[0] < ur[0], "{name}: right arm level");
        let (hip, foot) = (pos("leftUpperLeg"), pos("leftFoot"));
        assert!((hip[0] - foot[0]).abs() < 0.01 && foot[1] < hip[1], "{name}: left leg plumb {hip:?} {foot:?}");
        let idle = [("leftUpperArm", -80.0f32), ("rightUpperArm", 80.0)];
        let rest = super::preview::render_posed(&m, 0.0, 300, 380, &[], &[]);
        let down = super::preview::render_posed(&m, 0.0, 300, 380, &[], &idle);
        let _ = rest;
        image::imageops::replace(&mut sheet, &down, (i as u32 * 300) as i64, 0);
        any = true;
    }
    if any {
        sheet.save("target/screens/vrm_idle_pose.png").unwrap();
    }
}

#[test]
fn the_skeleton_has_clavicles_and_fingers_named_for_both_vrm_formats() {
    use super::model::build_any;
    let img = crate::rig::uncut::tests::figure_image();
    let mut d = uncut_doc(&img);
    let ids: Vec<crate::doc::LayerId> = d.layers.iter().map(|l| l.id).collect();
    let mut rig = Rig::default();
    crate::rig::uncut::auto_rig(&mut rig, &d, &ids, 0.6).unwrap();
    d.rig = std::sync::Arc::new(rig);
    let m = build_any(&d, &Options { name: "Fingers".into(), author: "Me".into(), ..Default::default() }).unwrap();
    // clavicles between the chest and the arm; the arm hangs from the clavicle
    for side in ["left", "right"] {
        let sh = m.bone(&format!("{side}Shoulder")).unwrap_or_else(|| panic!("{side} shoulder"));
        assert_eq!(m.bones[sh].parent, m.bone("chest"));
        assert_eq!(m.bones[m.bone(&format!("{side}UpperArm")).unwrap()].parent, Some(sh));
    }
    // fingers: a thumb and four fingers, three bones each, hanging from the hand and from each other
    for side in ["left", "right"] {
        for (finger, segs) in [("Thumb", ["Metacarpal", "Proximal", "Distal"]), ("Index", ["Proximal", "Intermediate", "Distal"]), ("Little", ["Proximal", "Intermediate", "Distal"])] {
            let mut parent = m.bone(&format!("{side}Hand")).unwrap();
            for s in segs {
                let b = m.bone(&format!("{side}{finger}{s}")).unwrap_or_else(|| panic!("{side}{finger}{s}"));
                assert_eq!(m.bones[b].parent, Some(parent));
                parent = b;
            }
        }
    }
    assert!(m.bones.len() >= 40, "the body and the thirty finger bones: {}", m.bones.len());
    // the fingertip follows a finger bone, the palm the hand
    let p = &m.parts[0];
    let hand = m.bone("leftHand").unwrap();
    let tip_bone = m.bone("leftMiddleDistal").unwrap();
    let tip_vertex = (0..p.verts.len()).filter(|i| p.joints[*i][0] as usize != 0 || true).max_by(|a, b| p.verts[*a][0].total_cmp(&p.verts[*b][0])).unwrap();
    let uses = |v: usize, bone: usize| p.joints[v].iter().zip(&p.weights[v]).any(|(j, w)| *j as usize == bone && *w > 0.0);
    assert!(p.joints[tip_vertex].iter().zip(&p.weights[tip_vertex]).any(|(j, w)| m.bones[*j as usize].name.starts_with("left") && (m.bones[*j as usize].name.contains("Index") || m.bones[*j as usize].name.contains("Middle") || m.bones[*j as usize].name.contains("Ring") || m.bones[*j as usize].name.contains("Little") || m.bones[*j as usize].name.contains("Thumb")) && *w > 0.2), "the fingertip follows the fingers: {:?} {:?}", p.joints[tip_vertex], p.weights[tip_vertex]);
    let _ = (hand, tip_bone, uses);
    // VRM 1.0 uses the Metacarpal name; 0.x calls the thumb's bones Proximal, Intermediate and Distal
    let (one, zero) = (export(&m, Format::Vrm1).unwrap(), export(&m, Format::Vrm0).unwrap());
    assert!(validate(&one).is_ok() && validate(&zero).is_ok());
    let g1 = read_glb(&one).unwrap();
    let hb1 = g1.json["extensions"]["VRMC_vrm"]["humanoid"]["humanBones"].as_object().unwrap();
    for n in ["leftShoulder", "rightShoulder", "leftThumbMetacarpal", "leftThumbDistal", "leftIndexIntermediate", "rightLittleDistal"] {
        assert!(hb1.contains_key(n), "{n}");
    }
    let g0 = read_glb(&zero).unwrap();
    let names0: Vec<&str> = g0.json["extensions"]["VRM"]["humanoid"]["humanBones"].as_array().unwrap().iter().map(|b| b["bone"].as_str().unwrap()).collect();
    for n in ["leftShoulder", "leftThumbProximal", "leftThumbIntermediate", "leftThumbDistal", "rightIndexIntermediate"] {
        assert!(names0.contains(&n), "{n} in the 0.x list");
    }
    assert!(!names0.contains(&"leftThumbMetacarpal"));
    // every name appears once
    let mut sorted = names0.clone();
    sorted.sort();
    sorted.dedup();
    assert_eq!(sorted.len(), names0.len(), "no bone twice");
}

#[test]
fn an_uncut_figure_gets_a_face_with_blend_shapes_and_a_mouth_that_opens() {
    use super::model::build_any;
    let img = crate::rig::uncut::tests::figure_image();
    let mut d = uncut_doc(&img);
    let ids: Vec<crate::doc::LayerId> = d.layers.iter().map(|l| l.id).collect();
    let mut rig = Rig::default();
    crate::rig::uncut::auto_rig(&mut rig, &d, &ids, 0.6).unwrap();
    d.rig = std::sync::Arc::new(rig);
    let m = build_any(&d, &Options { name: "Face".into(), author: "Me".into(), ..Default::default() }).unwrap();
    // the body and the inside of the mouth
    assert_eq!(m.parts.len(), 2);
    assert_eq!(m.parts[1].name, "Mouth");
    for e in ["blink", "blinkLeft", "blinkRight", "happy", "sad", "angry", "surprised", "lookLeft", "lookRight", "lookUp", "lookDown", "aa", "ih", "ou", "ee", "oh", "jawOpen", "browInnerUp", "mouthSmile"] {
        assert!(m.expressions.iter().any(|x| x.name == e), "expression {e}");
    }
    // a blink moves points of the body, only near the eyes; the left blink and the right blink move different points
    let body = &m.parts[0];
    let blink = |name: &str| -> Vec<usize> {
        let e = m.expressions.iter().find(|x| x.name == name).unwrap();
        let (pi, mi, _) = e.binds[0];
        assert_eq!(pi, 0);
        (0..body.verts.len()).filter(|i| body.morphs[mi].deltas[*i][1].abs() > 1e-5).collect()
    };
    let (l, r) = (blink("blinkLeft"), blink("blinkRight"));
    assert!(l.len() >= 8 && r.len() >= 8, "{} {}", l.len(), r.len());
    let mean_x = |v: &[usize]| v.iter().map(|i| body.verts[*i][0]).sum::<f32>() / v.len() as f32;
    assert!(mean_x(&l) > 0.0 && mean_x(&r) < 0.0, "the character's left is +X");
    assert!(l.iter().all(|i| body.verts[*i][1] > 1.2), "only the head moves");
    // the mouth is flat at rest, and a vowel opens it
    let mouth = &m.parts[1];
    let area = |verts: &[[f32; 3]]| -> f32 { mouth.tris.iter().map(|t| { let (a, b, c) = (verts[t[0] as usize], verts[t[1] as usize], verts[t[2] as usize]); ((b[0] - a[0]) * (c[1] - a[1]) - (b[1] - a[1]) * (c[0] - a[0])) / 2.0 }).sum() };
    assert!(area(&mouth.verts).abs() < 1e-9);
    let aa = m.expressions.iter().find(|x| x.name == "aa").unwrap();
    let (pi, mi, _) = aa.binds[0];
    assert_eq!(pi, 1);
    let open: Vec<[f32; 3]> = mouth.verts.iter().zip(&mouth.morphs[mi].deltas).map(|(v, d)| [v[0] + d[0], v[1] + d[1], v[2]]).collect();
    assert!(area(&open) > 1e-5, "the mouth shows: {}", area(&open));
    // both formats carry the shapes and are valid
    let (one, zero) = (export(&m, Format::Vrm1).unwrap(), export(&m, Format::Vrm0).unwrap());
    let (s1, s0) = (validate(&one).unwrap(), validate(&zero).unwrap());
    assert!(s1.expressions >= 19 && s0.expressions >= 19, "{} {}", s1.expressions, s0.expressions);
    // for games: a plain GLB with the skeleton, the skin and the same blend shapes, and no VRM data
    let glb = export(&m, Format::Glb).unwrap();
    let sg = validate(&glb).unwrap();
    assert_eq!(sg.version, 2);
    assert!(sg.expressions >= 19 && sg.parts == 2 && sg.bones >= 20, "{sg:?}");
    let g = super::gltf::read_glb(&glb).unwrap();
    assert!(g.json["extensions"].get("VRMC_vrm").is_none() && g.json["extensions"].get("VRM").is_none());
    assert!(g.json["meshes"][0]["extras"]["targetNames"].as_array().unwrap().iter().any(|n| n == "blink"));
    // face off: no shapes
    let plain = build_any(&d, &Options { face: false, ..Default::default() }).unwrap();
    assert_eq!(plain.parts.len(), 1);
    assert!(plain.expressions.is_empty());
}

/// The user's uncut bases, when they are on this computer: the head with each expression, written to target/screens.
#[test]
fn faces_of_real_uncut_characters_move() {
    face_sheet(None, "vrm_faces");
    // the same with the shapes of the user's Emi
    let path = std::path::PathBuf::from(std::env::var("USERPROFILE").unwrap_or_default()).join("Downloads/art/Finished2D3D/vrmemi/Emi1.0.vrm");
    if let Ok(bytes) = std::fs::read(path) {
        face_sheet(Some(std::sync::Arc::new(super::template::Template::from_vrm(&bytes).unwrap())), "vrm_faces_emi");
    }
}

fn face_sheet(template: Option<std::sync::Arc<super::template::Template>>, sheet_name: &str) {
    use super::model::build_any;
    let base = std::path::PathBuf::from(std::env::var("USERPROFILE").unwrap_or_default()).join("Downloads/Bases/images/races/images");
    let files = [("female", "PixieBase.png"), ("male", "ghoulbase.png"), ("male", "OgreBase.png")];
    let looks: [(&str, &[(&str, f32)]); 7] = [("rest", &[]), ("blink", &[("blink", 1.0)]), ("happy", &[("happy", 1.0)]), ("surprised", &[("surprised", 1.0), ("oh", 1.0)]), ("aa", &[("aa", 1.0)]), ("angry", &[("angry", 1.0)]), ("look left", &[("lookLeft", 1.0)])];
    let (w, h, cw, ch) = (1200u32, 2400u32, 300u32, 420u32);
    let mut sheet = image::RgbaImage::from_pixel(looks.len() as u32 * cw, files.len() as u32 * ch, image::Rgba([255, 255, 255, 255]));
    let mut any = false;
    for (row, (folder, name)) in files.iter().enumerate() {
        let Ok(bytes) = std::fs::read(base.join(folder).join(name)) else { continue };
        let img = image::load_from_memory(&bytes).unwrap().to_rgba8();
        let mut d = uncut_doc(&img);
        let ids: Vec<crate::doc::LayerId> = d.layers.iter().map(|l| l.id).collect();
        let mut rig = Rig::default();
        crate::rig::uncut::auto_rig(&mut rig, &d, &ids, 0.5).unwrap();
        d.rig = std::sync::Arc::new(rig);
        // the ghoul's face points set by hand, as a user would (the guess is thrown off by his hair)
        if *name == "ghoulbase.png" {
            let cx = super::model::face_marks(&d.rig).map(|f| f.eye_center()[0]).unwrap_or(0.0);
            if let Some(sk) = std::sync::Arc::make_mut(&mut d.rig).skin.as_mut() {
                sk.face = Some(super::face::FaceMarks { eye_l: [cx + 28.0, 178.0], eye_r: [cx - 28.0, 178.0], mouth: [cx, 226.0], eye_w: 34.0, mouth_w: 40.0 });
            }
        }
        if let (Some(sk), Some(f)) = (d.rig.skin.as_ref(), super::model::face_marks(&d.rig)) {
            let hb = sk.bones.iter().find(|b| d.rig.changer(b.changer).is_some_and(|c| c.name == "head")).unwrap();
            let pivot = match d.rig.changer(hb.changer).unwrap().kind { crate::rig::Kind::Rotation { pivot } => pivot, _ => [0.0, 0.0] };
            let (top, bot) = sk.verts.iter().fold((f32::MAX, f32::MIN), |a, v| (a.0.min(v[1]), a.1.max(v[1])));
            eprintln!("{name}: figure {top:.0}..{bot:.0}, head pivot y {:.0}, head tail y {:.0}, guess eye y {:.0} mouth y {:.0}", pivot[1], hb.tail[1], f.eye_l[1], f.mouth[1]);
        }
        let m = build_any(&d, &Options { max_texture: 2048, template: template.clone(), ..Default::default() }).unwrap();
        assert!(m.expressions.len() >= 15, "{name}");
        let (one, zero) = (export(&m, Format::Vrm1).unwrap(), export(&m, Format::Vrm0).unwrap());
        validate(&one).unwrap_or_else(|e| panic!("{name} 1.0: {e}"));
        validate(&zero).unwrap_or_else(|e| panic!("{name} 0.x: {e}"));
        let stem = name.trim_end_matches(".png");
        let out = std::path::Path::new("target/vrm").join(folder);
        let _ = std::fs::create_dir_all(&out);
        std::fs::write(out.join(format!("{stem}.vrm")), &one).unwrap();
        std::fs::write(out.join(format!("{stem}_vrm0.vrm")), &zero).unwrap();
        // the head: find it on the picture from the head bone
        let top = m.parts[0].verts.iter().map(|v| v[1]).fold(0.0f32, f32::max);
        let scale = h as f32 * 0.9 / top;
        let hp = m.bones[m.bone("head").unwrap()].pos;
        let (cx, cy) = (w as f32 / 2.0 + hp[0] * scale, h as f32 * 0.95 - hp[1] * scale);
        for (k, (_, active)) in looks.iter().enumerate() {
            let full = super::preview::render(&m, 0.0, w, h, active);
            let x0 = ((cx - cw as f32 / 2.0).max(0.0) as u32).min(w - cw);
            let y0 = ((cy - ch as f32 * 0.82).max(0.0) as u32).min(h - ch);
            let crop = image::imageops::crop_imm(&full, x0, y0, cw, ch).to_image();
            image::imageops::replace(&mut sheet, &crop, (k as u32 * cw) as i64, (row as u32 * ch) as i64);
        }
        any = true;
    }
    if any {
        let _ = std::fs::create_dir_all("target/screens");
        sheet.save(format!("target/screens/{sheet_name}.png")).unwrap();
    }
}

/// The head of every one of the user's uncut bases with the guessed eyes (red) and mouth (blue), to see where the guess fails.
#[test]
fn face_guess_sheet() {
    let base = std::path::PathBuf::from(std::env::var("USERPROFILE").unwrap_or_default()).join("Downloads/Bases/images/races/images");
    let mut tiles: Vec<image::RgbaImage> = Vec::new();
    for folder in ["female", "male"] {
        let Ok(rd) = std::fs::read_dir(base.join(folder)) else { continue };
        let mut files: Vec<std::path::PathBuf> = rd.flatten().map(|e| e.path()).filter(|p| p.extension().is_some_and(|e| e == "png")).collect();
        files.sort();
        for f in files {
            let Ok(bytes) = std::fs::read(&f) else { continue };
            let img = image::load_from_memory(&bytes).unwrap().to_rgba8();
            let d = uncut_doc(&img);
            let ids: Vec<crate::doc::LayerId> = d.layers.iter().map(|l| l.id).collect();
            let mut rig = Rig::default();
            if crate::rig::uncut::auto_rig(&mut rig, &d, &ids, 0.5).is_err() {
                continue;
            }
            let mut d = d;
            d.rig = std::sync::Arc::new(rig);
            let Some(m) = super::model::face_marks(&d.rig) else { continue };
            let sk = d.rig.skin.as_ref().unwrap();
            let hb = sk.bones.iter().find(|b| d.rig.changer(b.changer).is_some_and(|c| c.name == "head")).unwrap();
            let pivot = match d.rig.changer(hb.changer).unwrap().kind { crate::rig::Kind::Rotation { pivot } => pivot, _ => [0.0, 0.0] };
            let hh = (pivot[1] - hb.tail[1]).abs().max(30.0);
            let (x0, y0, w, h) = ((pivot[0] - hh * 0.9).max(0.0) as u32, (hb.tail[1] - 20.0).max(0.0) as u32, (hh * 1.8) as u32, (hh + 90.0) as u32);
            let (w, h) = (w.min(img.width() - x0), h.min(img.height() - y0));
            let mut crop = image::imageops::crop_imm(&img, x0, y0, w, h).to_image();
            let mut dot = |p: [f32; 2], c: [u8; 4]| {
                for dy in -4i32..=4 {
                    for dx in -4i32..=4 {
                        let (x, y) = ((p[0] - x0 as f32) as i32 + dx, (p[1] - y0 as f32) as i32 + dy);
                        if x >= 0 && y >= 0 && (x as u32) < w && (y as u32) < h {
                            crop.put_pixel(x as u32, y as u32, image::Rgba(c));
                        }
                    }
                }
            };
            // the AI helper's points (yellow eyes, magenta mouth), when SEVIATH_AI_PYTHON names a Python that has the packages
            if let (Ok(py), Some(crop)) = (std::env::var("SEVIATH_AI_PYTHON"), super::model::head_box(&d.rig)) {
                match crate::ai::find_face(&py, &img, crop, "auto") {
                    Ok(f) => {
                        dot(f.eye_l, [255, 220, 0, 255]);
                        dot(f.eye_r, [255, 220, 0, 255]);
                        dot(f.mouth, [255, 0, 255, 255]);
                    }
                    Err(e) => eprintln!("{}: {e}", f.display()),
                }
            }
            dot(m.eye_l, [255, 0, 0, 255]);
            dot(m.eye_r, [255, 0, 0, 255]);
            dot(m.mouth, [0, 0, 255, 255]);
            // a green tick at the head pivot (the neck)
            dot(pivot, [0, 200, 0, 255]);
            let k = 260.0 / h as f32;
            let flat = image::DynamicImage::ImageRgba8(crop);
            let mut bg = image::RgbaImage::from_pixel(240, 260, image::Rgba([225, 225, 225, 255]));
            let small = flat.resize(240, 260, image::imageops::FilterType::Triangle).to_rgba8();
            let _ = k;
            image::imageops::overlay(&mut bg, &small, 0, 0);
            tiles.push(bg);
        }
    }
    let cols = 8u32;
    let rows = (tiles.len() as u32).div_ceil(cols).max(1);
    let mut sheet = image::RgbaImage::from_pixel(cols * 240, rows * 260, image::Rgba([255, 255, 255, 255]));
    for (i, t) in tiles.iter().enumerate() {
        image::imageops::replace(&mut sheet, t, ((i as u32 % cols) * 240) as i64, ((i as u32 / cols) * 260) as i64);
    }
    let _ = std::fs::create_dir_all("target/screens");
    sheet.save(if std::env::var("SEVIATH_AI_PYTHON").is_ok() { "target/screens/face_guess_ai.png" } else { "target/screens/face_guess.png" }).unwrap();
}

/// A shape that an AI made, stood in for by the mesh of the picture with a depth.
#[test]
fn a_character_is_built_on_an_ai_shape_with_texture_weights_and_face_shapes() {
    use super::model::build_any;
    use super::shape::Shape;
    let img = crate::rig::uncut::tests::figure_image();
    let mut d = uncut_doc(&img);
    let ids: Vec<crate::doc::LayerId> = d.layers.iter().map(|l| l.id).collect();
    let mut rig = Rig::default();
    crate::rig::uncut::auto_rig(&mut rig, &d, &ids, 0.6).unwrap();
    d.rig = std::sync::Arc::new(rig);
    // the stand-in: the mesh of the picture itself, given a depth, in other coordinates and size as an AI would give them
    let sk = d.rig.skin.as_ref().unwrap();
    let (sv, st) = crate::rig::refine::subdivide(&sk.verts, &sk.tris, 12.0, 60_000);
    let mut verts: Vec<[f32; 3]> = Vec::new();
    let mut tris: Vec<[u32; 3]> = Vec::new();
    for (side, z) in [(0u32, 0.05f32), (1, -0.05)] {
        let base = verts.len() as u32;
        verts.extend(sv.iter().map(|v| [v[0] * 0.002 + 0.3, -v[1] * 0.002 + 1.1, z]));
        tris.extend(st.iter().map(|t| if side == 0 { [base + t[0], base + t[2], base + t[1]] } else { [base + t[0], base + t[1], base + t[2]] }));
    }
    let shape = Shape { verts, tris };
    let m = build_any(&d, &Options { name: "OnShape".into(), author: "Me".into(), shape: Some(std::sync::Arc::new(shape)), ..Default::default() }).unwrap();
    assert!(m.parts.iter().any(|p| p.name == "Mouth") && m.parts[0].name == "Body", "the body and the inside of the mouth");
    let body = &m.parts[0];
    // the same size as the character, standing on the floor
    let top = body.verts.iter().map(|v| v[1]).fold(0.0f32, f32::max);
    let bottom = body.verts.iter().map(|v| v[1]).fold(f32::MAX, f32::min);
    assert!((top - bottom - 1.6).abs() < 0.03 && bottom.abs() < 0.03, "{bottom} {top}");
    // the texture holds the front and the mirrored back; every UV is inside it
    assert_eq!(body.image.width() % 2, 0);
    assert!(body.uvs.iter().all(|u| u[0] >= -0.01 && u[0] <= 1.01 && u[1] >= -0.01 && u[1] <= 1.01));
    assert!(body.uvs.iter().any(|u| u[0] < 0.5) && body.uvs.iter().any(|u| u[0] > 0.5), "front and back");
    assert!(body.image.pixels().all(|p| p.0[3] == 255), "no holes in the texture");
    // weights add up to one and the arm follows the arm
    assert!(body.weights.iter().all(|w| (w.iter().sum::<f32>() - 1.0).abs() < 1e-3));
    let hand = m.bones.iter().position(|b| b.name == "leftHand").unwrap();
    let hp = m.bones[hand].pos;
    let nearest = (0..body.verts.len()).min_by(|a, b| {
        let da = (body.verts[*a][0] - hp[0]).hypot(body.verts[*a][1] - hp[1]);
        let db = (body.verts[*b][0] - hp[0]).hypot(body.verts[*b][1] - hp[1]);
        da.total_cmp(&db)
    }).unwrap();
    let on_arm = |j: u16| m.bones[j as usize].name.starts_with("left") && ["Arm", "Hand", "Thumb", "Index", "Middle", "Ring", "Little", "Shoulder"].iter().any(|k| m.bones[j as usize].name.contains(k));
    assert!(body.joints[nearest].iter().zip(&body.weights[nearest]).any(|(j, w)| on_arm(*j) && *w > 0.4), "{:?} {:?}", body.joints[nearest], body.weights[nearest]);
    // the face shapes exist and only move the front
    assert!(m.expressions.iter().any(|e| e.name == "blink") && m.expressions.iter().any(|e| e.name == "aa"));
    let blink = m.expressions.iter().find(|e| e.name == "blink").unwrap();
    let (pi, mi, _) = blink.binds[0];
    // the face is a part of its own that carries the shapes; the body has none
    assert_eq!(m.parts[pi].name, "Face");
    assert!(body.morphs.is_empty());
    let face = &m.parts[pi];
    let moved = (0..face.verts.len()).filter(|i| face.morphs[mi].deltas[*i][1].abs() > 1e-5).count();
    assert!(moved > 0);
    assert!((0..face.verts.len()).filter(|i| face.morphs[mi].deltas[*i][1].abs() > 1e-5).all(|i| face.normals[i][2] > 0.2), "the back of the head stays");
    // saved in all three ways
    for f in [Format::Vrm1, Format::Vrm0, Format::Glb] {
        let bytes = export(&m, f).unwrap();
        validate(&bytes).unwrap_or_else(|e| panic!("{f:?}: {e}"));
    }
}

/// The real thing, when the AI files are on this computer: the Midlander drawing and the shape Hunyuan3D made of it.
#[test]
fn the_ai_shape_of_the_midlander_becomes_a_textured_rigged_character() {
    use super::model::build_any;
    use super::shape::Shape;
    let dir = std::path::PathBuf::from(std::env::var("USERPROFILE").unwrap_or_default()).join("SeviathAI/jobs/midlander");
    let (Ok(png), Ok(glb)) = (std::fs::read(dir.join("original.png")), std::fs::read(dir.join("shape.glb"))) else { return };
    let img = image::load_from_memory(&png).unwrap().to_rgba8();
    let shape = Shape::from_glb(&glb).unwrap();
    let mut d = uncut_doc(&img);
    let ids: Vec<crate::doc::LayerId> = d.layers.iter().map(|l| l.id).collect();
    let mut rig = Rig::default();
    crate::rig::uncut::auto_rig(&mut rig, &d, &ids, 0.5).unwrap();
    if let Some(sk) = std::sync::Arc::make_mut(&mut d.rig).skin.as_mut() {
        let _ = sk;
    }
    d.rig = std::sync::Arc::new(rig);
    let emi = std::path::PathBuf::from(std::env::var("USERPROFILE").unwrap_or_default()).join("Downloads/art/Finished2D3D/vrmemi/Emi1.0.vrm");
    let template = std::fs::read(emi).ok().and_then(|b| super::template::Template::from_vrm(&b).ok()).map(std::sync::Arc::new);
    let t0 = std::time::Instant::now();
    let m = build_any(&d, &Options { name: "Midlander".into(), author: "test".into(), max_texture: 2048, shape: Some(std::sync::Arc::new(shape)), template, ..Default::default() }).unwrap();
    assert!(m.parts.iter().any(|p| p.name == "Hair"), "the hair is a part of its own");
    assert!(m.springs.len() >= 6 && m.bones.iter().any(|b| b.name.starts_with("hair") && !b.humanoid), "bones and springs for the hair");
    eprintln!("hair part {} triangles, {} springs, {} colliders", m.parts.iter().find(|p| p.name == "Hair").map(|p| p.tris.len()).unwrap_or(0), m.springs.len(), m.colliders.len());
    eprintln!("built in {:?}: {} vertices, {} triangles, {} expressions", t0.elapsed(), m.parts[0].verts.len(), m.parts[0].tris.len(), m.expressions.len());
    let out = std::path::Path::new("target/vrm/ai");
    let _ = std::fs::create_dir_all(out);
    for (f, name) in [(Format::Vrm1, "Midlander_ai.vrm"), (Format::Vrm0, "Midlander_ai_vrm0.vrm"), (Format::Glb, "Midlander_ai.glb")] {
        let bytes = export(&m, f).unwrap();
        validate(&bytes).unwrap_or_else(|e| panic!("{name}: {e}"));
        std::fs::write(out.join(name), &bytes).unwrap();
    }
    {
        // how clean are the weights out on the arm? points far from the body should follow one arm bone almost alone
        let body = &m.parts[0];
        let ua = m.bones[m.bone("rightUpperArm").unwrap()].pos;
        let far: Vec<usize> = (0..body.verts.len()).filter(|i| body.verts[*i][0] < ua[0] - 0.25 && (body.verts[*i][1] - ua[1]).abs() < 0.12).collect();
        let mixed = far.iter().filter(|i| body.weights[**i][0] < 0.9).count();
        let names: std::collections::BTreeMap<String, usize> = far.iter().fold(Default::default(), |mut a, i| { *a.entry(m.bones[body.joints[*i][0] as usize].name.clone()).or_default() += 1; a });
        eprintln!("far arm points {} mixed {} main bones {:?}", far.len(), mixed, names);
        let arm_side = |j: u16| { let n = &m.bones[j as usize].name; n.starts_with("right") && (n.contains("Arm") || n.contains("Hand") || ["Thumb", "Index", "Middle", "Ring", "Little"].iter().any(|k| n.contains(k))) };
        let bad: Vec<usize> = far.iter().copied().filter(|i| { let off: f32 = body.joints[*i].iter().zip(&body.weights[*i]).filter(|(j, _)| !arm_side(**j)).map(|(_, w)| *w).sum(); off > 0.1 }).collect();
        eprintln!("far arm points that follow something other than the arm by more than 10%: {} of {}", bad.len(), far.len());
        if let Some(i) = bad.first() { eprintln!("example {:?} {:?} -> {:?}", body.verts[*i], body.joints[*i].iter().map(|j| m.bones[*j as usize].name.clone()).collect::<Vec<_>>(), body.weights[*i]); }
    }
    let (w, h) = (500u32, 1000u32);
    let mut sheet = image::RgbaImage::new(w * 4, h);
    for (k, (yaw, pose)) in [(0.0f32, vec![]), (40.0, vec![]), (180.0, vec![]), (0.0, vec![("leftUpperArm", -70.0f32), ("rightUpperArm", 70.0), ("leftUpperLeg", -15.0), ("head", 10.0)])].iter().enumerate() {
        let view = super::preview::render_posed(&m, *yaw, w, h, &[], pose);
        image::imageops::replace(&mut sheet, &view, (k as u32 * w) as i64, 0);
    }
    sheet.save("target/screens/ai_midlander.png").unwrap();
    // the head close up, turned
    {
        let (hw, hh, bw, bh) = (400u32, 480u32, 1400u32, 2800u32);
        let top = m.parts[0].verts.iter().map(|v| v[1]).fold(0.0f32, f32::max);
        let scale = bh as f32 * 0.9 / top;
        let hp = m.bones[m.bone("head").unwrap()].pos;
        let mut heads = image::RgbaImage::new(hw * 8, hh);
        for (k, yaw) in [0.0f32, 35.0, 65.0, 90.0].iter().enumerate() {
            let full = super::preview::render(&m, *yaw, bw, bh, &[]);
            let (c, s2) = ((yaw.to_radians()).cos(), (yaw.to_radians()).sin());
            let cx = bw as f32 / 2.0 + (hp[0] * c + hp[2] * s2) * scale;
            let cy = bh as f32 * 0.95 - hp[1] * scale;
            let x0 = ((cx - hw as f32 / 2.0).max(0.0) as u32).min(bw - hw);
            let y0 = ((cy - hh as f32 * 0.78).max(0.0) as u32).min(bh - hh);
            let crop = image::imageops::crop_imm(&full, x0, y0, hw, hh).to_image();
            image::imageops::replace(&mut heads, &crop, (k as u32 * hw) as i64, 0);
        }
        // the hair swung out (every hair bone turned), seen from the front and from the side
        let swing: Vec<(String, f32)> = m.bones.iter().filter(|b| b.name.starts_with("hair") && b.name.ends_with("_1")).map(|b| (b.name.clone(), 28.0f32)).collect();
        let swing: Vec<(&str, f32)> = swing.iter().map(|(n, a)| (n.as_str(), *a)).collect();
        for (k, yaw) in [0.0f32, 50.0].iter().enumerate() {
            let full = super::preview::render_posed(&m, *yaw, bw, bh, &[], &swing);
            let (c, s2) = ((yaw.to_radians()).cos(), (yaw.to_radians()).sin());
            let cx = bw as f32 / 2.0 + (hp[0] * c + hp[2] * s2) * scale;
            let cy = bh as f32 * 0.95 - hp[1] * scale;
            let x0 = ((cx - hw as f32 / 2.0).max(0.0) as u32).min(bw - hw);
            let y0 = ((cy - hh as f32 * 0.78).max(0.0) as u32).min(bh - hh);
            let crop = image::imageops::crop_imm(&full, x0, y0, hw, hh).to_image();
            image::imageops::replace(&mut heads, &crop, ((4 + k as u32) * hw) as i64, 0);
        }
        // with the guide lines: the texture features (blue) and the nose and chin of the shape (pink)
        for (k, yaw) in [65.0f32, 90.0].iter().enumerate() {
            let full = super::preview::render_guides(&m, *yaw, bw, bh, &[], true);
            let (c, s2) = ((yaw.to_radians()).cos(), (yaw.to_radians()).sin());
            let cx = bw as f32 / 2.0 + (hp[0] * c + hp[2] * s2) * scale;
            let cy = bh as f32 * 0.95 - hp[1] * scale;
            let x0 = ((cx - hw as f32 / 2.0).max(0.0) as u32).min(bw - hw);
            let y0 = ((cy - hh as f32 * 0.78).max(0.0) as u32).min(bh - hh);
            let crop = image::imageops::crop_imm(&full, x0, y0, hw, hh).to_image();
            image::imageops::replace(&mut heads, &crop, ((6 + k as u32) * hw) as i64, 0);
        }
        heads.save("target/screens/ai_midlander_head.png").unwrap();
    }
}

/// The stand-in for an AI shape used by the tests: the mesh of the picture given a depth, in other coordinates and size.
fn standin_shape(d: &Document) -> super::shape::Shape {
    let sk = d.rig.skin.as_ref().unwrap();
    let (sv, st) = crate::rig::refine::subdivide(&sk.verts, &sk.tris, 12.0, 60_000);
    let (mut verts, mut tris): (Vec<[f32; 3]>, Vec<[u32; 3]>) = (Vec::new(), Vec::new());
    for (side, z) in [(0u32, 0.05f32), (1, -0.05)] {
        let base = verts.len() as u32;
        verts.extend(sv.iter().map(|v| [v[0] * 0.002 + 0.3, -v[1] * 0.002 + 1.1, z]));
        tris.extend(st.iter().map(|t| if side == 0 { [base + t[0], base + t[2], base + t[1]] } else { [base + t[0], base + t[1], base + t[2]] }));
    }
    super::shape::Shape { verts, tris }
}

#[test]
fn moved_texture_parts_move_the_picture_the_blend_shapes_and_the_mouth_settings() {
    use super::model::build_any;
    let img = crate::rig::uncut::tests::figure_image();
    let mut d = uncut_doc(&img);
    let ids: Vec<crate::doc::LayerId> = d.layers.iter().map(|l| l.id).collect();
    let mut rig = Rig::default();
    crate::rig::uncut::auto_rig(&mut rig, &d, &ids, 0.6).unwrap();
    d.rig = std::sync::Arc::new(rig);
    let shape = std::sync::Arc::new(standin_shape(&d));
    let opt = Options { shape: Some(shape), ..Default::default() };
    let plain = build_any(&d, &opt).unwrap();
    let landmark = |m: &super::model::Model3d, n: &str| m.landmarks.iter().find(|l| l.0 == n).map(|l| l.1).unwrap_or_else(|| panic!("landmark {n}"));
    for n in ["eye_l", "eye_r", "nose", "mouth", "mesh_nose"] {
        landmark(&plain, n);
    }
    // move the mouth down, the face to the right, and change the inside of the mouth
    {
        let sk = std::sync::Arc::make_mut(&mut d.rig).skin.as_mut().unwrap();
        sk.tex.mouth = [0.0, 12.0];
        sk.tex.face = [6.0, 0.0];
        sk.tex.mouth_open = 2.0;
        sk.tex.mouth_color = [200, 10, 10];
    }
    let moved = build_any(&d, &opt).unwrap();
    // the texture is not the same, and the places of the features follow
    assert_ne!(plain.parts[0].image.as_raw(), moved.parts[0].image.as_raw());
    let (a, b) = (landmark(&plain, "mouth"), landmark(&moved, "mouth"));
    assert!(b[1] < a[1] - 0.005, "the mouth is lower: {a:?} {b:?}");
    let (ea, eb) = (landmark(&plain, "eye_l"), landmark(&moved, "eye_l"));
    assert!(eb[0] > ea[0], "the eyes went right with the face");
    // the shape's own nose does not move
    assert_eq!(landmark(&plain, "mesh_nose"), landmark(&moved, "mesh_nose"));
    // the inside of the mouth: its colour and how far it opens
    let (mp, mq) = (plain.parts.iter().find(|p| p.name == "Mouth").unwrap(), moved.parts.iter().find(|p| p.name == "Mouth").unwrap());
    assert_eq!(mq.image.get_pixel(0, 0).0, [200, 10, 10, 255]);
    let reach = |p: &super::model::Part3d| { let aa = p.morphs.iter().find(|m| m.name == "aa").unwrap(); aa.deltas.iter().map(|v| v[1].abs()).fold(0.0f32, f32::max) };
    assert!(reach(mq) > reach(mp) * 1.5, "open 2: {} {}", reach(mq), reach(mp));
    // a project saves and loads the edit
    let text = serde_json::to_string(&d.rig.skin.as_ref().unwrap().tex).unwrap();
    assert!(text.contains("mouth_open"));
    // still valid files
    for f in [Format::Vrm1, Format::Glb] {
        validate(&export(&moved, f).unwrap()).unwrap_or_else(|e| panic!("{f:?}: {e}"));
    }
    // the guide lines draw on the preview and do not when they are off
    let off = super::preview::render_guides(&moved, 35.0, 300, 480, &[], false);
    let on = super::preview::render_guides(&moved, 35.0, 300, 480, &[], true);
    assert_ne!(off.as_raw(), on.as_raw());
    assert_eq!(off.as_raw(), super::preview::render(&moved, 35.0, 300, 480, &[]).as_raw());
}

#[test]
fn painting_on_the_3d_character_is_kept_with_the_project_and_survives_making_it_again() {
    use super::model::build_any;
    use super::paint3d::{Brush, Tool, dab};
    let img = crate::rig::uncut::tests::figure_image();
    let mut d = uncut_doc(&img);
    let ids: Vec<crate::doc::LayerId> = d.layers.iter().map(|l| l.id).collect();
    let mut rig = Rig::default();
    crate::rig::uncut::auto_rig(&mut rig, &d, &ids, 0.6).unwrap();
    d.rig = std::sync::Arc::new(rig);
    let opt = Options { shape: Some(std::sync::Arc::new(standin_shape(&d))), ..Default::default() };
    let mut m = build_any(&d, &opt).unwrap();
    // something is under the pointer in the middle of the body
    let (w, h) = (300u32, 480u32);
    let hit = (0..h).step_by(8).flat_map(|y| (0..w).step_by(8).map(move |x| (x, y))).find_map(|(x, y)| super::preview::pick(&m, 0.0, w, h, x as f32, y as f32).filter(|(p, _)| m.parts[*p].name == "Body").map(|r| (x, y, r)));
    let (_, _, (part, uv)) = hit.expect("the body is under the pointer somewhere");
    let dims = m.parts[0].image.dimensions();
    assert_eq!(m.parts[part].image.dimensions(), dims);
    let at = [uv[0] * dims.0 as f32, uv[1] * dims.1 as f32];
    // paint a red dab there: the picture of the part shows it at once
    let mut overlay = image::RgbaImage::new(dims.0, dims.1);
    let brush = Brush { tool: Tool::Brush, colour: [250, 0, 0], size: 6.0, hardness: 1.0, opacity: 1.0 };
    dab(&mut m, &mut overlay, &brush, at[0], at[1]);
    let p = m.parts[part].image.get_pixel(at[0] as u32, at[1] as u32).0;
    assert_eq!(&p[..3], &[250, 0, 0]);
    // kept with the project, then the character is made again: the paint is still there
    std::sync::Arc::make_mut(&mut d.rig).skin.as_mut().unwrap().tex.set_atlas_paint(&overlay);
    assert!(!d.rig.skin.as_ref().unwrap().tex.atlas_paint.is_empty());
    let again = build_any(&d, &opt).unwrap();
    let q = again.parts[0].image.get_pixel(at[0] as u32, at[1] as u32).0;
    assert_eq!(&q[..3], &[250, 0, 0], "the paint came back");
    // and it goes into the saved files
    validate(&export(&again, Format::Vrm1).unwrap()).unwrap();
    // the plain mesh to paint by hand: one colour, no drawing
    let plain = build_any(&d, &Options { mesh_only: true, ..opt.clone() }).unwrap();
    let distinct: std::collections::HashSet<[u8; 4]> = plain.parts[0].image.pixels().map(|p| p.0).collect();
    assert!(distinct.len() < 40, "a plain texture has hardly any colours: {}", distinct.len());
}

/// The hair editing on the real Midlander (when the AI files are on this computer): strands moved, added and deleted, and the hair
/// corrected with the hair brush.
#[test]
fn hair_strands_and_the_hair_brush_edit_the_hair_of_the_midlander() {
    use super::hairedit::{HairChain, HairDab};
    use super::model::build_any;
    use super::shape::Shape;
    let dir = std::path::PathBuf::from(std::env::var("USERPROFILE").unwrap_or_default()).join("SeviathAI/jobs/midlander");
    let (Ok(png), Ok(glb)) = (std::fs::read(dir.join("original.png")), std::fs::read(dir.join("shape.glb"))) else { return };
    let img = image::load_from_memory(&png).unwrap().to_rgba8();
    let mut d = uncut_doc(&img);
    let ids: Vec<crate::doc::LayerId> = d.layers.iter().map(|l| l.id).collect();
    let mut rig = Rig::default();
    crate::rig::uncut::auto_rig(&mut rig, &d, &ids, 0.5).unwrap();
    d.rig = std::sync::Arc::new(rig);
    let opt = Options { max_texture: 1024, shape: Some(std::sync::Arc::new(Shape::from_glb(&glb).unwrap())), ..Default::default() };
    let first = build_any(&d, &opt).unwrap();
    let hair_tris = |m: &super::model::Model3d| m.parts.iter().find(|p| p.name == "Hair").map(|p| p.tris.len()).unwrap_or(0);
    assert!(first.hair_chains.len() >= 6, "the app made strands: {}", first.hair_chains.len());
    let n0 = first.springs.len();
    assert_eq!(n0, first.hair_chains.len());
    // start from the app's strands, move one joint of the first strand, add a strand and delete another
    let mut chains = first.hair_chains.clone();
    let moved_from = chains[0].joints[2];
    chains[0].joints[2][0] += 40.0;
    let removed = chains.remove(1);
    chains.push(HairChain { joints: vec![[removed.joints[0][0], removed.joints[0][1]], [removed.joints[0][0], removed.joints[0][1] + 60.0], [removed.joints[0][0], removed.joints[0][1] + 140.0]], layer: 1.0, stiffness: 0.9, ..Default::default() });
    std::sync::Arc::make_mut(&mut d.rig).skin.as_mut().unwrap().hair.chains = chains.clone();
    let edited = build_any(&d, &opt).unwrap();
    assert_eq!(edited.springs.len(), chains.len(), "one spring for every strand");
    assert!(edited.springs.iter().any(|s| (s.stiffness - 0.9).abs() < 1e-6), "the stiffness of the new strand");
    // the bone of the moved joint is where it was put
    let b = edited.bones.iter().find(|b| b.name == "hair0_2").unwrap();
    let plain = first.bones.iter().find(|b| b.name == "hair0_2").unwrap();
    assert!(b.pos[0] > plain.pos[0] + 0.01, "the joint moved to the right: {:?} {:?} (was at page {:?})", b.pos, plain.pos, moved_from);
    validate(&export(&edited, Format::Vrm1).unwrap()).unwrap();
    // the hair brush: take away hair over a patch on the top of the head, and the hair part has fewer triangles
    std::sync::Arc::make_mut(&mut d.rig).skin.as_mut().unwrap().hair.chains.clear();
    let top = first.hair_chains.iter().flat_map(|c| c.joints.iter()).map(|j| j[1]).fold(f32::MAX, f32::min);
    let mid = first.hair_chains.iter().flat_map(|c| c.joints.iter()).map(|j| j[0]).sum::<f32>() / first.hair_chains.iter().map(|c| c.joints.len()).sum::<usize>() as f32;
    std::sync::Arc::make_mut(&mut d.rig).skin.as_mut().unwrap().hair.dabs.push(HairDab { at: [mid, top + 60.0], radius: 90.0, add: false });
    let trimmed = build_any(&d, &opt).unwrap();
    assert!(hair_tris(&trimmed) < hair_tris(&first), "{} {}", hair_tris(&trimmed), hair_tris(&first));
}

#[test]
fn the_svrm_description_rides_along_in_every_format_and_is_read_back() {
    use super::export::export_with;
    let d = character();
    let m = build(&d, &Options { name: "Test".into(), author: "Me".into(), ..Default::default() }).unwrap();
    let mut rig = Rig::default();
    let p = rig.add_param("Mouth open", 0.0, 1.0, 0.0);
    rig.own_exprs.push(crate::expressions::OwnExpr { name: "Happy".into(), values: vec![(p, 1.0)], ..Default::default() });
    let svrm = super::svrm::describe(&rig, &|_| None);
    for fmt in [Format::Vrm1, Format::Vrm0, Format::Glb] {
        let with = export_with(&m, fmt, Some(&svrm)).unwrap();
        let s = validate(&with).unwrap();
        assert_eq!(s.svrm, Some(1), "{fmt:?}");
        let g = read_glb(&with).unwrap();
        assert!(g.json["extensionsUsed"].as_array().unwrap().iter().any(|e| e == "SEVIATH_svrm"));
        assert_eq!(g.json["extensions"]["SEVIATH_svrm"]["expressions"][0]["name"], "Happy");
        // without it nothing is added
        assert_eq!(validate(&export(&m, fmt).unwrap()).unwrap().svrm, None);
    }
}
