// SPDX-License-Identifier: GPL-3.0-or-later
// Skin weights that an AI worked out for the 3D shape of a character (Make-It-Animatable, `tools/ai/rig_mia.py`): for every point of the
// shape, which bones move it and how much. The AI names its bones the Mixamo way; here they are turned into the names of the VRM
// skeleton, and a bone that the character does not have gives its share to the bone above it.

use std::collections::HashMap;

/// The weights of every point of a shape.
#[derive(Clone, Debug, PartialEq)]
pub struct AiSkin {
    /// The bones the weights talk about, by VRM name ("leftUpperArm"...).
    pub names: Vec<String>,
    /// For every point: bone number (in `names`) and share.
    pub weights: Vec<Vec<(u16, f32)>>,
}

/// The VRM name of a Mixamo bone ("LeftForeArm" is "leftLowerArm").
pub fn vrm_name(mixamo: &str) -> Option<String> {
    let m = mixamo.strip_prefix("mixamorig:").unwrap_or(mixamo);
    let (side, rest) = if let Some(r) = m.strip_prefix("Left") {
        ("left", r)
    } else if let Some(r) = m.strip_prefix("Right") {
        ("right", r)
    } else {
        ("", m)
    };
    let body = match rest {
        "Hips" => "hips",
        "Spine" => "spine",
        "Spine1" => "chest",
        "Spine2" => "upperChest",
        "Neck" => "neck",
        "Head" => "head",
        "Shoulder" => "Shoulder",
        "Arm" => "UpperArm",
        "ForeArm" => "LowerArm",
        "Hand" => "Hand",
        "UpLeg" => "UpperLeg",
        "Leg" => "LowerLeg",
        "Foot" => "Foot",
        "ToeBase" => "Toes",
        _ => "",
    };
    if !body.is_empty() {
        return Some(if side.is_empty() { body.to_string() } else { format!("{side}{body}") });
    }
    // fingers: LeftHandThumb1, RightHandPinky3...
    let f = rest.strip_prefix("Hand")?;
    let (finger, n) = f.split_at(f.len().checked_sub(1)?);
    let finger = match finger {
        "Thumb" => "Thumb",
        "Index" => "Index",
        "Middle" => "Middle",
        "Ring" => "Ring",
        "Pinky" => "Little",
        _ => return None,
    };
    let seg = match (finger, n) {
        ("Thumb", "1") => "Metacarpal",
        ("Thumb", "2") => "Proximal",
        ("Thumb", "3") => "Distal",
        (_, "1") => "Proximal",
        (_, "2") => "Intermediate",
        (_, "3") => "Distal",
        _ => return None,
    };
    (!side.is_empty()).then(|| format!("{side}{finger}{seg}"))
}

/// The bone above a VRM bone, for a character that does not have it.
pub fn fallback(name: &str) -> Option<String> {
    let side = if name.starts_with("left") {
        "left"
    } else if name.starts_with("right") {
        "right"
    } else {
        ""
    };
    let up = |s: &str| if side.is_empty() { s.to_string() } else { format!("{side}{s}") };
    Some(match name {
        "upperChest" => "chest".into(),
        "chest" => "spine".into(),
        "spine" => "hips".into(),
        "neck" => "chest".into(),
        "head" => "neck".into(),
        n if n.ends_with("Toes") => up("Foot"),
        n if n.ends_with("Foot") => up("LowerLeg"),
        n if n.ends_with("LowerLeg") => up("UpperLeg"),
        n if n.ends_with("UpperLeg") => "hips".into(),
        n if n.ends_with("Hand") => up("LowerArm"),
        n if n.ends_with("LowerArm") => up("UpperArm"),
        n if n.ends_with("UpperArm") => up("Shoulder"),
        n if n.ends_with("Shoulder") => "chest".into(),
        n if n.ends_with("Distal") => n.replace("Distal", if n.contains("Thumb") { "Proximal" } else { "Intermediate" }),
        n if n.ends_with("Intermediate") => n.replace("Intermediate", "Proximal"),
        n if n.ends_with("Proximal") && n.contains("Thumb") => n.replace("Proximal", "Metacarpal"),
        n if n.ends_with("Proximal") || n.ends_with("Metacarpal") => up("Hand"),
        _ => return None,
    })
}

impl AiSkin {
    /// Read the answer of the helper (its JSON).
    pub fn from_json(text: &str) -> Result<AiSkin, String> {
        let v: serde_json::Value = serde_json::from_str(text).map_err(|e| format!("The skin weights are not readable: {e}"))?;
        let names: Vec<String> = v["names"].as_array().ok_or("The answer has no bone names.")?.iter().map(|n| vrm_name(n.as_str().unwrap_or("")).unwrap_or_default()).collect();
        let mut weights = Vec::new();
        for p in v["weights"].as_array().ok_or("The answer has no weights.")? {
            let mut row = Vec::new();
            for pair in p.as_array().ok_or("A point has no weights.")? {
                let (b, w) = (pair[0].as_u64().ok_or("A bone number is wrong.")?, pair[1].as_f64().ok_or("A weight is wrong.")?);
                if (b as usize) < names.len() && w.is_finite() && w > 0.0 {
                    row.push((b as u16, w as f32));
                }
            }
            weights.push(row);
        }
        if weights.is_empty() {
            return Err("The AI gave no weights.".into());
        }
        Ok(AiSkin { names, weights })
    }

    /// The weights of a point for the bones of a character: `slot` gives the number of a bone by its VRM name. A bone the character does not have
    /// gives its share to the bone above it; what is left over goes to `default`.
    pub fn weights_for(&self, point: usize, slot: &HashMap<String, usize>, default: usize) -> Vec<(usize, f32)> {
        let Some(row) = self.weights.get(point) else { return Vec::new() };
        let mut acc: Vec<(usize, f32)> = Vec::new();
        for (b, w) in row {
            let mut name = self.names.get(*b as usize).cloned().unwrap_or_default();
            let mut target = None;
            for _ in 0..8 {
                if let Some(&i) = slot.get(&name) {
                    target = Some(i);
                    break;
                }
                match fallback(&name) {
                    Some(n) => name = n,
                    None => break,
                }
            }
            let i = target.unwrap_or(default);
            match acc.iter_mut().find(|a| a.0 == i) {
                Some(a) => a.1 += w,
                None => acc.push((i, *w)),
            }
        }
        let total: f32 = acc.iter().map(|a| a.1).sum();
        if total > 1e-6 {
            acc.iter_mut().for_each(|a| a.1 /= total);
        }
        acc
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mixamo_bones_get_their_vrm_names() {
        for (m, v) in [
            ("mixamorig:Hips", "hips"),
            ("mixamorig:Spine1", "chest"),
            ("mixamorig:Spine2", "upperChest"),
            ("mixamorig:LeftArm", "leftUpperArm"),
            ("mixamorig:RightForeArm", "rightLowerArm"),
            ("mixamorig:LeftUpLeg", "leftUpperLeg"),
            ("mixamorig:RightToeBase", "rightToes"),
            ("mixamorig:LeftHandThumb1", "leftThumbMetacarpal"),
            ("mixamorig:LeftHandThumb3", "leftThumbDistal"),
            ("RightHandPinky2", "rightLittleIntermediate"),
            ("mixamorig:LeftHandIndex1", "leftIndexProximal"),
            ("mixamorig:LeftShoulder", "leftShoulder"),
        ] {
            assert_eq!(vrm_name(m).as_deref(), Some(v), "{m}");
        }
        assert_eq!(vrm_name("mixamorig:Banana"), None);
    }

    #[test]
    fn a_missing_bone_gives_its_share_to_the_bone_above() {
        let slot: HashMap<String, usize> = [("hips", 0), ("spine", 1), ("chest", 2), ("leftHand", 3), ("leftLowerArm", 4)].into_iter().map(|(n, i)| (n.to_string(), i)).collect();
        let skin = AiSkin { names: vec!["upperChest".into(), "leftIndexProximal".into(), "leftToes".into(), "spine".into()], weights: vec![vec![(0, 0.5), (1, 0.3), (3, 0.2)], vec![(2, 1.0)]] };
        let w = skin.weights_for(0, &slot, 0);
        let get = |i: usize| w.iter().find(|a| a.0 == i).map(|a| a.1).unwrap_or(0.0);
        assert!((get(2) - 0.5).abs() < 1e-5, "the upper chest goes to the chest");
        assert!((get(3) - 0.3).abs() < 1e-5, "a finger goes to the hand");
        assert!((get(1) - 0.2).abs() < 1e-5);
        assert!((w.iter().map(|a| a.1).sum::<f32>() - 1.0).abs() < 1e-5);
        // the toes of a character without toes and without feet: the default bone
        let w2 = skin.weights_for(1, &slot, 0);
        assert_eq!(w2, vec![(0, 1.0)]);
        assert!(skin.weights_for(99, &slot, 0).is_empty());
    }

    #[test]
    fn the_answer_of_the_helper_is_read() {
        let json = r#"{"names": ["mixamorig:Hips", "mixamorig:LeftArm"], "points": 2, "weights": [[[0, 0.75], [1, 0.25]], [[1, 1.0]]]}"#;
        let s = AiSkin::from_json(json).unwrap();
        assert_eq!(s.names, vec!["hips", "leftUpperArm"]);
        assert_eq!(s.weights[0], vec![(0, 0.75), (1, 0.25)]);
        assert!(AiSkin::from_json("{}").is_err());
        assert!(AiSkin::from_json(r#"{"names": [], "weights": []}"#).is_err());
    }
}
