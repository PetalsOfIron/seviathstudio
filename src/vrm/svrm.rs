// SPDX-License-Identifier: GPL-3.0-or-later
// SVRM: what a game needs to know about a Seviath character beyond what VRM says. A VRM (or GLB) file made by the program carries one
// more glTF extension, `SEVIATH_svrm`, written here as plain JSON with a version number. Programs that do not know it ignore it; a game
// runtime reads it to get the sliders of the rig (name, range, default), the expressions (how they move the sliders, which props they
// show), the animation clips (the tracks and keys of the timeline, cut into parts by the markers), the views (front, side, back) and the
// props (layers that expressions show and hide). Everything refers to sliders by name, so it does not depend on the numbers inside the
// project file.

use serde_json::{Value, json};

use crate::anim::{Ease, Target};
use crate::expressions;
use crate::rig::Rig;

pub const VERSION: u32 = 1;
pub const EXTENSION: &str = "SEVIATH_svrm";

fn ease_name(e: &Ease) -> Value {
    match e {
        Ease::Linear => json!("linear"),
        Ease::Step => json!("step"),
        Ease::In => json!("in"),
        Ease::Out => json!("out"),
        Ease::InOut => json!("inOut"),
        Ease::Bezier(h) => json!({ "bezier": h }),
    }
}

/// The SVRM description of a rig. `layer_name` gives the name of a layer by its number (for the props).
pub fn describe(rig: &Rig, layer_name: &dyn Fn(u64) -> Option<String>) -> Value {
    let pname = |id: crate::rig::Id| rig.param(id).map(|p| p.name.clone());
    let params: Vec<Value> = rig.params.iter().map(|p| json!({ "name": p.name, "min": p.min, "max": p.max, "default": p.default, "live2dId": p.cid })).collect();
    let expressions: Vec<Value> = expressions::defs(rig)
        .iter()
        .map(|d| {
            let effects: Vec<Value> = d
                .effects
                .iter()
                .filter_map(|e| {
                    Some(json!({ "param": pname(e.param)?, "value": e.value, "blend": match e.blend { expressions::Blend::Add => "add", expressions::Blend::Multiply => "multiply", expressions::Blend::Overwrite => "overwrite" } }))
                })
                .collect();
            let names = |ids: &[u64]| -> Vec<String> { ids.iter().filter_map(|l| layer_name(*l)).collect() };
            json!({ "name": d.name, "fadeIn": d.fade_in, "fadeOut": d.fade_out, "effects": effects, "showLayers": names(&d.show), "hideLayers": names(&d.hide) })
        })
        .collect();
    let a = &rig.anim;
    let tracks: Vec<Value> = a
        .tracks
        .iter()
        .filter_map(|t| {
            let Target::Param(id) = t.target else { return None };
            let name = pname(id)?;
            Some(json!({ "param": name, "keys": t.keys.iter().map(|k| json!({ "frame": k.frame, "value": k.value, "ease": ease_name(&k.ease) })).collect::<Vec<_>>() }))
        })
        .collect();
    let clips: Vec<Value> = if tracks.is_empty() {
        Vec::new()
    } else {
        let parts: Vec<Value> = (0..a.markers.len()).filter_map(|i| a.marker_range(i).map(|[s, e]| json!({ "name": a.markers[i].name, "start": s, "end": e }))).collect();
        vec![json!({ "name": "Timeline", "fps": a.fps, "length": a.length, "loop": a.looping, "parts": parts, "tracks": tracks })]
    };
    let views = if rig.views.names.len() >= 2 { json!({ "names": rig.views.names, "param": rig.views.param.and_then(pname), "crossfade": rig.views.crossfade }) } else { Value::Null };
    json!({ "version": VERSION, "generator": "SeviathStudio", "params": params, "expressions": expressions, "clips": clips, "views": views })
}

/// Things a game maker should know before using the file; empty when all is well.
pub fn report(svrm: &Value) -> Vec<String> {
    let mut out = Vec::new();
    let names: Vec<&str> = svrm["params"].as_array().map(|a| a.iter().filter_map(|p| p["name"].as_str()).collect()).unwrap_or_default();
    if names.is_empty() {
        out.push("The rig has no sliders, so a game cannot move anything by them.".into());
    }
    let mut seen = std::collections::HashSet::new();
    for n in &names {
        if !seen.insert(*n) {
            out.push(format!("Two sliders are named {n}: a game cannot tell them apart."));
        }
    }
    for e in svrm["expressions"].as_array().unwrap_or(&Vec::new()) {
        let en = e["name"].as_str().unwrap_or("?");
        let empty = e["effects"].as_array().is_none_or(|a| a.is_empty()) && e["showLayers"].as_array().is_none_or(|a| a.is_empty()) && e["hideLayers"].as_array().is_none_or(|a| a.is_empty());
        if empty {
            out.push(format!("The expression {en} changes nothing."));
        }
        for fx in e["effects"].as_array().unwrap_or(&Vec::new()) {
            if !fx["param"].as_str().is_some_and(|p| names.contains(&p)) {
                out.push(format!("The expression {en} moves a slider that is not in the rig."));
            }
        }
    }
    for c in svrm["clips"].as_array().unwrap_or(&Vec::new()) {
        for t in c["tracks"].as_array().unwrap_or(&Vec::new()) {
            if !t["param"].as_str().is_some_and(|p| names.contains(&p)) {
                out.push("A clip has a track for a slider that is not in the rig.".into());
            }
            if t["keys"].as_array().is_none_or(|k| k.is_empty()) {
                out.push(format!("A clip has an empty track for {}.", t["param"].as_str().unwrap_or("?")));
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::anim::Ease;
    use crate::expressions::OwnExpr;

    fn rig() -> Rig {
        let mut r = Rig::default();
        let a = r.add_param("Mouth open", 0.0, 1.0, 0.0);
        let b = r.add_param("Angle X", -30.0, 30.0, 0.0);
        r.own_exprs.push(OwnExpr { name: "Happy".into(), values: vec![(a, 1.0)], show: vec![7], hide: vec![8, 99], ..Default::default() });
        let t = r.anim.track_for(Target::Param(b), "Angle X");
        r.anim.tracks[t].set_key(0.0, -10.0, Ease::Linear);
        r.anim.tracks[t].set_key(12.0, 10.0, Ease::InOut);
        r.anim.set_marker(0, "Look");
        r
    }

    #[test]
    fn a_rig_is_described_by_names() {
        let r = rig();
        let v = describe(&r, &|l| match l {
            7 => Some("Glasses".into()),
            8 => Some("Hat".into()),
            _ => None,
        });
        assert_eq!(v["version"], 1);
        assert_eq!(v["params"][1]["name"], "Angle X");
        assert_eq!(v["params"][1]["min"], -30.0);
        let e = &v["expressions"][0];
        assert_eq!((e["name"].as_str(), e["effects"][0]["param"].as_str(), e["effects"][0]["blend"].as_str()), (Some("Happy"), Some("Mouth open"), Some("overwrite")));
        assert_eq!(e["showLayers"], json!(["Glasses"]));
        assert_eq!(e["hideLayers"], json!(["Hat"]), "a layer that is gone is left out");
        let c = &v["clips"][0];
        assert_eq!((c["fps"].as_f64(), c["length"].as_u64()), (Some(24.0), Some(120)));
        assert_eq!(c["tracks"][0]["param"], "Angle X");
        assert_eq!(c["tracks"][0]["keys"][1]["ease"], "inOut");
        assert_eq!(c["parts"][0]["name"], "Look");
        assert!(v["views"].is_null());
        assert!(report(&v).is_empty(), "{:?}", report(&v));
    }

    #[test]
    fn problems_a_game_maker_should_know_are_listed() {
        let mut r = rig();
        r.params.push(crate::rig::Param { id: 500, name: "Angle X".into(), min: 0.0, max: 1.0, default: 0.0, cid: String::new() });
        r.own_exprs.push(OwnExpr { name: "Nothing".into(), ..Default::default() });
        let v = describe(&r, &|_| None);
        let rep = report(&v);
        assert!(rep.iter().any(|l| l.contains("Two sliders are named Angle X")), "{rep:?}");
        assert!(rep.iter().any(|l| l.contains("Nothing changes nothing") || l.contains("The expression Nothing changes nothing")), "{rep:?}");
        let empty = describe(&Rig::default(), &|_| None);
        assert!(report(&empty).iter().any(|l| l.contains("no sliders")));
    }
}
