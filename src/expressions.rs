// SPDX-License-Identifier: GPL-3.0-or-later
// Expressions for streaming: a look (the eyes happy, a blush, a heart) that goes on and off with a key, a button or a pad, and fades in
// and out. Two kinds exist side by side: the ones made here (the sliders as they are now, saved under a name) and the ones that came
// with a Live2D model (they add to, multiply or replace sliders). Expressions sit on top of the tracked face: while one is on, the
// sliders it touches are changed from what tracking or the user set, and when it goes off they are exactly that again.

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use crate::rig::{Id, Rig, Values};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Blend {
    Add,
    Multiply,
    Overwrite,
}

#[derive(Clone, PartialEq, Debug)]
pub struct Effect {
    pub param: Id,
    pub value: f32,
    pub blend: Blend,
}

/// What the face has to do for an expression to switch on by itself (a cheek puffed, a smile, a tongue out): one of the inputs of the tracking goes over (or
/// under) a value. It goes off again when the input comes back a little past that value, so it does not flicker.
#[derive(Clone, PartialEq, Debug, Serialize, Deserialize)]
pub struct Trigger {
    /// The name of an input of the tracking (`CheekPuff`, `MouthSmile`, `TongueOut`...).
    pub input: String,
    pub value: f32,
    /// On when the input is under the value instead of over it.
    #[serde(default)]
    pub below: bool,
}

/// An expression as the engine uses it, whatever it came from.
#[derive(Clone, PartialEq, Debug)]
pub struct ExprDef {
    pub name: String,
    pub fade_in: f32,
    pub fade_out: f32,
    pub effects: Vec<Effect>,
    pub own: bool,
    /// Layers (props, accessories) that show while it is on, and layers that hide.
    pub show: Vec<u64>,
    pub hide: Vec<u64>,
    pub trigger: Option<Trigger>,
}

/// An expression made in this program: sliders at values.
#[derive(Clone, PartialEq, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct OwnExpr {
    pub name: String,
    pub values: Vec<(Id, f32)>,
    pub fade_in: f32,
    pub fade_out: f32,
    pub show: Vec<u64>,
    pub hide: Vec<u64>,
    /// The face switches it on by itself.
    pub trigger: Option<Trigger>,
}

impl Default for OwnExpr {
    fn default() -> Self {
        OwnExpr { name: String::new(), values: Vec::new(), fade_in: 0.25, fade_out: 0.25, show: Vec::new(), hide: Vec::new(), trigger: None }
    }
}

/// Every expression of the rig: the own ones first, then the ones of a Live2D model (sliders found by their Live2D id).
pub fn defs(rig: &Rig) -> Vec<ExprDef> {
    let mut out: Vec<ExprDef> = rig.own_exprs.iter().map(|e| ExprDef { name: e.name.clone(), fade_in: e.fade_in, fade_out: e.fade_out, effects: e.values.iter().map(|(p, v)| Effect { param: *p, value: *v, blend: Blend::Overwrite }).collect(), own: true, show: e.show.clone(), hide: e.hide.clone(), trigger: e.trigger.clone() }).collect();
    for e in &rig.expressions {
        let effects: Vec<Effect> = e
            .params
            .iter()
            .filter_map(|p| {
                let param = rig.params.iter().find(|x| !x.cid.is_empty() && x.cid == p.id)?;
                let blend = match p.blend.to_lowercase().as_str() {
                    "multiply" => Blend::Multiply,
                    "overwrite" => Blend::Overwrite,
                    _ => Blend::Add,
                };
                Some(Effect { param: param.id, value: p.value, blend })
            })
            .collect();
        if !effects.is_empty() && !out.iter().any(|o| o.name == e.name) {
            out.push(ExprDef { name: e.name.clone(), fade_in: e.fade_in.max(0.0), fade_out: e.fade_out.max(0.0), effects, own: false, show: Vec::new(), hide: Vec::new(), trigger: None });
        }
    }
    out
}

/// Which expressions are on, how far each has faded in, and what was changed in the sliders.
#[derive(Default)]
pub struct ExprState {
    on: HashMap<String, bool>,
    level: HashMap<String, f32>,
    /// For every slider an expression changed: what it was before and what was written.
    applied: HashMap<Id, (f32, f32)>,
    /// For every expression with a trigger: whether the face had it on at the last look (only a change switches it, so a key can still change it).
    seen: HashMap<String, bool>,
}

impl ExprState {
    pub fn is_on(&self, name: &str) -> bool {
        self.on.get(name).copied().unwrap_or(false)
    }

    pub fn level(&self, name: &str) -> f32 {
        self.level.get(name).copied().unwrap_or(0.0)
    }

    /// The face switches expressions: an expression with a trigger goes on when its input passes the value and off when it goes back past it (with a gap of
    /// a tenth of the range of the input). Only a change of the face switches it, so a key can still switch it by hand in between.
    pub fn watch(&mut self, defs: &[ExprDef], inputs: &HashMap<&'static str, f32>) {
        for d in defs {
            let Some(t) = &d.trigger else { continue };
            let Some(v) = inputs.get(t.input.as_str()).copied() else { continue };
            let was = self.seen.get(&d.name).copied().unwrap_or(false);
            let gap = 0.1 * (t.value.abs().max(0.2));
            let now = match (t.below, was) {
                (false, false) => v > t.value,
                (false, true) => v > t.value - gap,
                (true, false) => v < t.value,
                (true, true) => v < t.value + gap,
            };
            if now != was {
                self.seen.insert(d.name.clone(), now);
                self.set(&d.name, now);
            }
        }
    }

    pub fn set(&mut self, name: &str, on: bool) {
        self.on.insert(name.to_string(), on);
    }

    pub fn toggle(&mut self, name: &str) {
        let now = self.is_on(name);
        self.set(name, !now);
    }

    /// This one on, every other one off.
    pub fn show_only(&mut self, name: &str) {
        for v in self.on.values_mut() {
            *v = false;
        }
        self.set(name, true);
    }

    pub fn clear(&mut self) {
        for v in self.on.values_mut() {
            *v = false;
        }
    }

    /// Did the expressions change any slider that has not been given back?
    pub fn has_applied(&self) -> bool {
        !self.applied.is_empty()
    }

    pub fn any(&self) -> bool {
        self.on.values().any(|v| *v) || self.level.values().any(|v| *v > 0.0)
    }

    /// Take away what the expressions did to the sliders (the ones that nobody else has changed since).
    pub fn undo(&mut self, values: &mut Values) {
        for (id, (base, written)) in self.applied.drain() {
            if values.0.get(&id).is_some_and(|v| (v - written).abs() < 1e-4) {
                values.set(id, base);
            }
        }
    }

    /// The layers the expressions that are on (or fading) show or hide: for each, whether it is to show and how far (0 to 1).
    pub fn layer_levels(&self, defs: &[ExprDef]) -> HashMap<u64, (bool, f32)> {
        let mut out: HashMap<u64, (bool, f32)> = HashMap::new();
        for d in defs {
            let l = self.level(&d.name);
            if l <= 0.0 {
                continue;
            }
            for (ids, show) in [(&d.show, true), (&d.hide, false)] {
                for id in ids {
                    let e = out.entry(*id).or_insert((show, 0.0));
                    // the strongest expression decides
                    if l > e.1 {
                        *e = (show, l);
                    }
                }
            }
        }
        out
    }

    /// The sliders without what the expressions are doing now (to save an expression from).
    pub fn without_effects(&self, values: &Values) -> Values {
        let mut v = values.clone();
        for (id, (base, written)) in &self.applied {
            if v.0.get(id).is_some_and(|x| (x - written).abs() < 1e-4) {
                v.set(*id, *base);
            }
        }
        v
    }

    /// Move every expression toward on or off by the time that passed, and put them on the sliders. Call `undo` first.
    pub fn apply(&mut self, defs: &[ExprDef], rig: &Rig, values: &mut Values, dt: f32) {
        for d in defs {
            let target = if self.is_on(&d.name) { 1.0 } else { 0.0 };
            let l = self.level.entry(d.name.clone()).or_insert(0.0);
            let fade = if target > *l { d.fade_in } else { d.fade_out };
            *l = if fade <= 1e-3 { target } else if target > *l { (*l + dt / fade).min(target) } else { (*l - dt / fade).max(target) };
        }
        self.level.retain(|_, l| *l > 0.0);
        for d in defs {
            let l = self.level(&d.name);
            if l <= 0.0 {
                continue;
            }
            for e in &d.effects {
                let Some(p) = rig.param(e.param) else { continue };
                let cur = values.0.get(&e.param).copied().unwrap_or(p.default);
                let new = match e.blend {
                    Blend::Add => cur + e.value * l,
                    Blend::Multiply => cur * (1.0 + (e.value - 1.0) * l),
                    Blend::Overwrite => cur + (e.value - cur) * l,
                }
                .clamp(p.min.min(p.max), p.max.max(p.min));
                let base = self.applied.get(&e.param).map(|(b, _)| *b).unwrap_or(cur);
                self.applied.insert(e.param, (base, new));
                values.set(e.param, new);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::import::runtime::{ExpParam, Expression};

    fn rig() -> Rig {
        let mut r = Rig::default();
        let a = r.add_param("Mouth form", -1.0, 1.0, 0.0);
        let b = r.add_param("Eyes open", 0.0, 1.0, 1.0);
        r.params.iter_mut().find(|p| p.id == a).unwrap().cid = "ParamMouthForm".into();
        r.params.iter_mut().find(|p| p.id == b).unwrap().cid = "ParamEyeOpen".into();
        r
    }

    #[test]
    fn own_and_live2d_expressions_become_one_list() {
        let mut r = rig();
        let (mouth, eyes) = (r.params[0].id, r.params[1].id);
        r.own_exprs.push(OwnExpr { name: "Happy".into(), values: vec![(mouth, 1.0)], ..Default::default() });
        r.expressions.push(Expression { name: "Sad".into(), fade_in: 0.5, fade_out: 0.2, params: vec![ExpParam { id: "ParamEyeOpen".into(), value: 0.5, blend: "Multiply".into() }, ExpParam { id: "ParamNothing".into(), value: 1.0, blend: "Add".into() }] });
        // one with the name of an own one is not listed twice
        r.expressions.push(Expression { name: "Happy".into(), params: vec![ExpParam { id: "ParamMouthForm".into(), value: 1.0, blend: "Add".into() }], ..Default::default() });
        let d = defs(&r);
        assert_eq!(d.iter().map(|e| (e.name.as_str(), e.own)).collect::<Vec<_>>(), [("Happy", true), ("Sad", false)]);
        assert_eq!(d[1].effects, [Effect { param: eyes, value: 0.5, blend: Blend::Multiply }], "the slider it does not know is left out");
        assert_eq!((d[1].fade_in, d[1].fade_out), (0.5, 0.2));
    }

    #[test]
    fn an_expression_fades_in_changes_the_sliders_and_gives_them_back() {
        let mut r = rig();
        let (mouth, eyes) = (r.params[0].id, r.params[1].id);
        r.own_exprs.push(OwnExpr { name: "Happy".into(), values: vec![(mouth, 1.0)], fade_in: 1.0, fade_out: 1.0, ..Default::default() });
        r.expressions.push(Expression { name: "Squint".into(), fade_in: 0.0, fade_out: 0.0, params: vec![ExpParam { id: "ParamEyeOpen".into(), value: 0.5, blend: "Multiply".into() }] });
        let d = defs(&r);
        let mut st = ExprState::default();
        let mut v = Values::default();
        v.set(mouth, -0.5); // what tracking or the user set
        st.toggle("Happy");
        // half a second: half way from -0.5 to 1
        st.undo(&mut v);
        st.apply(&d, &r, &mut v, 0.5);
        assert!((v.0[&mouth] - 0.25).abs() < 1e-4, "{}", v.0[&mouth]);
        // the next frame starts from what was set, not from the changed value (no build up)
        st.undo(&mut v);
        assert_eq!(v.0[&mouth], -0.5);
        st.apply(&d, &r, &mut v, 0.5);
        assert!((v.0[&mouth] - 1.0).abs() < 1e-4, "full after a second: {}", v.0[&mouth]);
        // saving an expression from the sliders takes the expression off first
        assert_eq!(st.without_effects(&v).0[&mouth], -0.5);
        // a second expression multiplies the eyes at once (no fade)
        st.set("Squint", true);
        st.undo(&mut v);
        st.apply(&d, &r, &mut v, 0.016);
        assert!((v.0[&eyes] - 0.5).abs() < 1e-4, "default 1 times 0.5: {:?}", v.0.get(&eyes));
        // off: the values come back as they were
        st.clear();
        for _ in 0..3 {
            st.undo(&mut v);
            st.apply(&d, &r, &mut v, 0.5);
        }
        assert!(!st.any());
        assert_eq!(v.0[&mouth], -0.5);
        assert_eq!(v.0[&eyes], 1.0);
    }

    #[test]
    fn expressions_show_and_hide_layers_by_how_far_they_have_faded() {
        let mut r = rig();
        r.own_exprs.push(OwnExpr { name: "Glasses".into(), fade_in: 1.0, fade_out: 1.0, show: vec![7], hide: vec![8], ..Default::default() });
        r.own_exprs.push(OwnExpr { name: "Hat".into(), fade_in: 0.0, fade_out: 0.0, show: vec![9], ..Default::default() });
        let d = defs(&r);
        let mut st = ExprState::default();
        let mut v = Values::default();
        assert!(st.layer_levels(&d).is_empty());
        st.set("Glasses", true);
        st.apply(&d, &r, &mut v, 0.5);
        let l = st.layer_levels(&d);
        assert_eq!(l[&7], (true, 0.5));
        assert_eq!(l[&8], (false, 0.5));
        assert!(!l.contains_key(&9));
        st.set("Hat", true);
        st.apply(&d, &r, &mut v, 0.1);
        assert_eq!(st.layer_levels(&d)[&9], (true, 1.0));
        st.clear();
        st.apply(&d, &r, &mut v, 2.0);
        assert!(st.layer_levels(&d).is_empty(), "back to how the layers are");
    }

    #[test]
    fn a_slider_changed_by_someone_else_is_not_put_back() {
        let mut r = rig();
        let mouth = r.params[0].id;
        r.own_exprs.push(OwnExpr { name: "Happy".into(), values: vec![(mouth, 1.0)], fade_in: 0.0, fade_out: 0.0, ..Default::default() });
        let d = defs(&r);
        let mut st = ExprState::default();
        let mut v = Values::default();
        st.set("Happy", true);
        st.undo(&mut v);
        st.apply(&d, &r, &mut v, 0.1);
        assert_eq!(v.0[&mouth], 1.0);
        v.set(mouth, 0.3); // the user moved it
        st.undo(&mut v);
        assert_eq!(v.0[&mouth], 0.3, "kept as the user left it");
        // show_only turns the others off
        st.set("Other", true);
        st.show_only("Happy");
        assert!(st.is_on("Happy") && !st.is_on("Other"));
        // a rig without the slider is ignored
        r.own_exprs[0].values.push((9999, 1.0));
        let d = defs(&r);
        st.apply(&d, &r, &mut v, 0.1);
    }
}

/// The sliders with one expression fully on (to draw it, for example as an emote).
pub fn pose_with(def: &ExprDef, rig: &Rig, base: &Values) -> Values {
    let mut st = ExprState::default();
    st.set(&def.name, true);
    let mut d = def.clone();
    d.fade_in = 0.0;
    let mut v = base.clone();
    st.apply(&[d], rig, &mut v, 1.0);
    v
}

#[cfg(test)]
mod pose_tests {
    use super::*;

    #[test]
    fn an_expression_can_be_looked_at_fully_on_without_fading() {
        let mut r = Rig::default();
        let a = r.add_param("Smile", 0.0, 1.0, 0.0);
        r.own_exprs.push(OwnExpr { name: "Happy".into(), values: vec![(a, 1.0)], fade_in: 5.0, fade_out: 5.0, ..Default::default() });
        let d = defs(&r);
        let base = Values::default();
        assert_eq!(pose_with(&d[0], &r, &base).0[&a], 1.0);
        assert!(base.0.is_empty(), "the sliders given are not changed");
    }

    #[test]
    fn the_face_switches_an_expression_on_and_off_and_a_key_can_still_change_it_between() {
        let defs = vec![ExprDef { name: "Puffy".into(), fade_in: 0.0, fade_out: 0.0, effects: Vec::new(), own: true, show: Vec::new(), hide: Vec::new(), trigger: Some(Trigger { input: "CheekPuff".into(), value: 0.5, below: false }) }];
        let mut st = ExprState::default();
        let mut ch: HashMap<&'static str, f32> = HashMap::new();
        ch.insert("CheekPuff", 0.2);
        st.watch(&defs, &ch);
        assert!(!st.is_on("Puffy"));
        ch.insert("CheekPuff", 0.7);
        st.watch(&defs, &ch);
        assert!(st.is_on("Puffy"), "over the value: on");
        // a key switches it off while the cheeks are still puffed: the face does not switch it on again until it changed
        st.set("Puffy", false);
        st.watch(&defs, &ch);
        assert!(!st.is_on("Puffy"));
        // a little under the value, but not past the gap: still counted as puffed
        ch.insert("CheekPuff", 0.47);
        st.watch(&defs, &ch);
        assert!(!st.is_on("Puffy"));
        ch.insert("CheekPuff", 0.1);
        st.watch(&defs, &ch);
        ch.insert("CheekPuff", 0.9);
        st.watch(&defs, &ch);
        assert!(st.is_on("Puffy"), "puffed again");
    }

    #[test]
    fn an_expression_that_is_on_when_the_input_is_under_a_value_works_the_other_way() {
        let defs = vec![ExprDef { name: "Squint".into(), fade_in: 0.0, fade_out: 0.0, effects: Vec::new(), own: true, show: Vec::new(), hide: Vec::new(), trigger: Some(Trigger { input: "EyeOpenLeft".into(), value: 0.3, below: true }) }];
        let mut st = ExprState::default();
        let mut ch: HashMap<&'static str, f32> = HashMap::new();
        ch.insert("EyeOpenLeft", 0.9);
        st.watch(&defs, &ch);
        assert!(!st.is_on("Squint"));
        ch.insert("EyeOpenLeft", 0.1);
        st.watch(&defs, &ch);
        assert!(st.is_on("Squint"));
    }
}
