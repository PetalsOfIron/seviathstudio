// SPDX-License-Identifier: GPL-3.0-or-later
// Parameter physics the way Live2D does it: a hanging chain of weights (a pendulum) is pushed by some sliders (the inputs: how far
// the head is turned, how open an eye is...) and swings; where its weights end up drives other sliders (the outputs: the angle of a
// strand of hair, the sway of a skirt). The numbers of a model's `.physics3.json` are used as they are, so a model that comes from
// Live2D swings the way it was set up. This is next to the chains of the Physics tab (which move Line changers): these move sliders.

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

/// What the chain reads: a slider, how much of it counts, and what it does to the chain (move it across, up and down, or turn it).
#[derive(Clone, PartialEq, Debug, Serialize, Deserialize)]
pub struct PhysInput {
    pub id: String,
    pub weight: f32,
    pub kind: Kind,
    pub reflect: bool,
}

/// What the chain gives: a weight of the chain (by number), a slider, how much, and how it is read from the weight.
#[derive(Clone, PartialEq, Debug, Serialize, Deserialize)]
pub struct PhysOutput {
    pub id: String,
    pub vertex: usize,
    pub scale: f32,
    pub weight: f32,
    pub kind: Kind,
    pub reflect: bool,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub enum Kind {
    X,
    Y,
    Angle,
}

/// One weight of the chain: where it rests, how freely it moves (0 to 1), how slowly it follows (delay), how strongly gravity pulls
/// it, and how far it hangs from the one before it.
#[derive(Clone, PartialEq, Debug, Serialize, Deserialize)]
pub struct PhysVertex {
    pub pos: [f32; 2],
    pub mobility: f32,
    pub delay: f32,
    pub acceleration: f32,
    pub radius: f32,
}

/// How the sliders are scaled into the chain: the range a slider is stretched to.
#[derive(Clone, Copy, PartialEq, Debug, Serialize, Deserialize)]
pub struct Range {
    pub min: f32,
    pub default: f32,
    pub max: f32,
}

#[derive(Clone, PartialEq, Debug, Serialize, Deserialize)]
pub struct PhysSetting {
    pub name: String,
    pub inputs: Vec<PhysInput>,
    pub outputs: Vec<PhysOutput>,
    pub vertices: Vec<PhysVertex>,
    pub position: Range,
    pub angle: Range,
}

#[derive(Clone, PartialEq, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct ParamPhysics {
    pub settings: Vec<PhysSetting>,
    pub gravity: [f32; 2],
    pub wind: [f32; 2],
    pub fps: f32,
    /// How much of the swing is used (1 is as the model was set up).
    pub strength: f32,
    pub on: bool,
}

impl Default for ParamPhysics {
    fn default() -> Self {
        ParamPhysics { settings: Vec::new(), gravity: [0.0, -1.0], wind: [0.0, 0.0], fps: 60.0, strength: 1.0, on: true }
    }
}

#[derive(Clone, Copy, Debug, Default)]
struct Particle {
    pos: [f32; 2],
    last_pos: [f32; 2],
    velocity: [f32; 2],
    last_gravity: [f32; 2],
}

/// The state of the swinging (not saved): where every weight of every chain is, and the time not yet used.
#[derive(Clone, Debug, Default)]
pub struct PhysState {
    chains: Vec<Vec<Particle>>,
    remain: f32,
}

/// A way to read and set the sliders by their Live2D id.
pub trait Sliders {
    /// (value, min, max, default) of the slider, or None when there is none with this id.
    fn get(&self, id: &str) -> Option<(f32, f32, f32, f32)>;
    fn set(&mut self, id: &str, value: f32);
}

impl Sliders for HashMap<String, (f32, f32, f32, f32)> {
    fn get(&self, id: &str) -> Option<(f32, f32, f32, f32)> {
        HashMap::get(self, id).copied()
    }

    fn set(&mut self, id: &str, value: f32) {
        if let Some(e) = self.get_mut(id) {
            e.0 = value;
        }
    }
}

fn normalize(value: f32, pmin: f32, pmax: f32, pdef: f32, n: Range, inverted: bool) -> f32 {
    let (hi, lo) = (pmax.max(pmin), pmax.min(pmin));
    let v = value.clamp(lo, hi);
    let (n_lo, n_hi) = (n.min.min(n.max), n.min.max(n.max));
    let delta = v - pdef;
    let mut r = 0.0;
    if delta > 0.0 {
        let (nl, pl) = (n_hi - n.default, hi - pdef);
        if pl != 0.0 {
            r = delta * (nl / pl);
        }
        r += n.default;
    } else if delta < 0.0 {
        let (nl, pl) = (n_lo - n.default, lo - pdef);
        if pl != 0.0 {
            r = delta * (nl / pl);
        }
        r += n.default;
    } else {
        r = n.default;
    }
    if inverted { r } else { -r }
}

fn to_radian(from: [f32; 2], to: [f32; 2]) -> f32 {
    let mut r = to[1].atan2(to[0]) - from[1].atan2(from[0]);
    while r < -std::f32::consts::PI {
        r += std::f32::consts::TAU;
    }
    while r > std::f32::consts::PI {
        r -= std::f32::consts::TAU;
    }
    r
}

fn unit(v: [f32; 2]) -> [f32; 2] {
    let l = v[0].hypot(v[1]);
    if l < 1e-9 { [0.0, 0.0] } else { [v[0] / l, v[1] / l] }
}

impl ParamPhysics {
    pub fn is_empty(&self) -> bool {
        self.settings.is_empty()
    }

    /// A state with every chain at rest.
    pub fn rest(&self) -> PhysState {
        PhysState {
            chains: self
                .settings
                .iter()
                .map(|s| s.vertices.iter().map(|v| Particle { pos: v.pos, last_pos: v.pos, velocity: [0.0, 0.0], last_gravity: [0.0, 1.0] }).collect())
                .collect(),
            remain: 0.0,
        }
    }

    /// Let `dt` seconds go by: the chains are pushed by the sliders as they are now and swing, and the outputs are set on the sliders.
    pub fn step(&self, state: &mut PhysState, sliders: &mut dyn Sliders, dt: f32) {
        if !self.on || self.settings.is_empty() || dt <= 0.0 {
            return;
        }
        if state.chains.len() != self.settings.len() || state.chains.iter().zip(&self.settings).any(|(c, s)| c.len() != s.vertices.len()) {
            *state = self.rest();
        }
        // fixed small steps, so the swing does not depend on how fast the frames come
        let h = 1.0 / self.fps.max(1.0);
        state.remain = (state.remain + dt).min(0.25);
        let mut stepped = false;
        while state.remain >= h {
            state.remain -= h;
            for (i, s) in self.settings.iter().enumerate() {
                self.step_setting(s, &mut state.chains[i], sliders, h);
            }
            stepped = true;
        }
        if !stepped {
            return;
        }
        // the outputs, from where the chains are now
        for (i, s) in self.settings.iter().enumerate() {
            let parts = &state.chains[i];
            for o in &s.outputs {
                if o.vertex < 1 || o.vertex >= parts.len() {
                    continue;
                }
                let t = [parts[o.vertex].pos[0] - parts[o.vertex - 1].pos[0], parts[o.vertex].pos[1] - parts[o.vertex - 1].pos[1]];
                let mut v = match o.kind {
                    Kind::X => t[0],
                    Kind::Y => t[1],
                    Kind::Angle => {
                        let parent = if o.vertex >= 2 { [parts[o.vertex - 1].pos[0] - parts[o.vertex - 2].pos[0], parts[o.vertex - 1].pos[1] - parts[o.vertex - 2].pos[1]] } else { [-self.gravity[0], -self.gravity[1]] };
                        to_radian(parent, t)
                    }
                };
                if o.reflect {
                    v = -v;
                }
                if let Some((cur, lo, hi, _)) = sliders.get(&o.id) {
                    let scaled = (v * o.scale).clamp(lo.min(hi), hi.max(lo));
                    let w = (o.weight / 100.0 * self.strength).clamp(0.0, 1.0);
                    sliders.set(&o.id, cur * (1.0 - w) + scaled * w);
                }
            }
        }
    }

    fn step_setting(&self, s: &PhysSetting, parts: &mut [Particle], sliders: &dyn Sliders, dt: f32) {
        if parts.is_empty() {
            return;
        }
        // what the sliders push: a move across and up, and a turn
        let (mut angle, mut tx, mut ty) = (0.0f32, 0.0f32, 0.0f32);
        for inp in &s.inputs {
            let Some((v, lo, hi, def)) = sliders.get(&inp.id) else { continue };
            let w = inp.weight / 100.0;
            match inp.kind {
                Kind::X => tx += normalize(v, lo, hi, def, s.position, inp.reflect) * w,
                Kind::Y => ty += normalize(v, lo, hi, def, s.position, inp.reflect) * w,
                Kind::Angle => angle += normalize(v, lo, hi, def, s.angle, inp.reflect) * w,
            }
        }
        let rad = (-angle).to_radians();
        let nx = tx * rad.cos() - ty * rad.sin();
        let ny = nx * rad.sin() + ty * rad.cos();
        parts[0].pos = [nx, ny];
        let gravity_dir = unit([angle.to_radians().sin(), angle.to_radians().cos()]);
        let gravity = if gravity_dir == [0.0, 0.0] { [0.0, 1.0] } else { gravity_dir };
        for i in 1..parts.len() {
            let v = &s.vertices[i];
            let force = [gravity[0] * v.acceleration + self.wind[0], gravity[1] * v.acceleration + self.wind[1]];
            let last = parts[i].pos;
            let delay = v.delay * dt * 30.0;
            let prev = parts[i - 1].pos;
            let mut dir = [parts[i].pos[0] - prev[0], parts[i].pos[1] - prev[1]];
            let radian = to_radian(parts[i].last_gravity, gravity) / 5.0f32.max(1.0);
            let (c, sn) = (radian.cos(), radian.sin());
            dir = [c * dir[0] - sn * dir[1], sn * dir[0] + c * dir[1]];
            parts[i].pos = [prev[0] + dir[0], prev[1] + dir[1]];
            let vel = [parts[i].velocity[0] * delay, parts[i].velocity[1] * delay];
            let f = [force[0] * delay * delay, force[1] * delay * delay];
            parts[i].pos = [parts[i].pos[0] + vel[0] + f[0], parts[i].pos[1] + vel[1] + f[1]];
            let nd = unit([parts[i].pos[0] - prev[0], parts[i].pos[1] - prev[1]]);
            parts[i].pos = [prev[0] + nd[0] * v.radius, prev[1] + nd[1] * v.radius];
            if parts[i].pos[0].abs() < 0.001 {
                parts[i].pos[0] = 0.0;
            }
            if delay != 0.0 {
                parts[i].velocity = [(parts[i].pos[0] - last[0]) / delay * v.mobility, (parts[i].pos[1] - last[1]) / delay * v.mobility];
            }
            parts[i].last_pos = last;
            parts[i].last_gravity = gravity;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hair() -> ParamPhysics {
        let v = |y: f32, mob: f32| PhysVertex { pos: [0.0, y], mobility: mob, delay: 0.9, acceleration: 1.0, radius: if y == 0.0 { 0.0 } else { 10.0 } };
        ParamPhysics {
            settings: vec![PhysSetting {
                name: "Hair".into(),
                inputs: vec![PhysInput { id: "ParamAngleX".into(), weight: 100.0, kind: Kind::X, reflect: false }],
                outputs: vec![PhysOutput { id: "ParamHair".into(), vertex: 2, scale: 20.0, weight: 100.0, kind: Kind::Angle, reflect: false }],
                vertices: vec![v(0.0, 1.0), v(10.0, 0.95), v(20.0, 0.95), v(30.0, 0.95)],
                position: Range { min: -10.0, default: 0.0, max: 10.0 },
                angle: Range { min: -10.0, default: 0.0, max: 10.0 },
            }],
            ..Default::default()
        }
    }

    fn sliders(angle_x: f32) -> HashMap<String, (f32, f32, f32, f32)> {
        let mut m = HashMap::new();
        m.insert("ParamAngleX".to_string(), (angle_x, -30.0, 30.0, 0.0));
        m.insert("ParamHair".to_string(), (0.0, -1.0, 1.0, 0.0));
        m
    }

    #[test]
    fn a_chain_at_rest_gives_nothing_and_a_push_makes_it_swing_and_settle() {
        let p = hair();
        let mut st = p.rest();
        let mut s = sliders(0.0);
        for _ in 0..120 {
            p.step(&mut st, &mut s, 1.0 / 60.0);
        }
        assert!(s["ParamHair"].0.abs() < 0.02, "nothing pushes: {}", s["ParamHair"].0);
        // the head turns quickly: the hair swings away
        let mut peak = 0.0f32;
        s.get_mut("ParamAngleX").unwrap().0 = 30.0;
        for _ in 0..30 {
            p.step(&mut st, &mut s, 1.0 / 60.0);
            peak = peak.max(s["ParamHair"].0.abs());
        }
        assert!(peak > 0.05, "the hair swung: {peak}");
        // the head comes back: the swing dies down
        s.get_mut("ParamAngleX").unwrap().0 = 0.0;
        for _ in 0..600 {
            p.step(&mut st, &mut s, 1.0 / 60.0);
        }
        assert!(s["ParamHair"].0.abs() < peak, "it settled: {} {}", s["ParamHair"].0, peak);
    }

    #[test]
    fn off_or_zero_strength_changes_nothing_and_the_output_stays_in_the_range_of_its_slider() {
        let mut p = hair();
        let mut st = p.rest();
        let mut s = sliders(30.0);
        p.on = false;
        for _ in 0..30 {
            p.step(&mut st, &mut s, 1.0 / 60.0);
        }
        assert_eq!(s["ParamHair"].0, 0.0);
        p.on = true;
        p.settings[0].outputs[0].scale = 5000.0;
        for _ in 0..60 {
            p.step(&mut st, &mut s, 1.0 / 60.0);
        }
        assert!(s["ParamHair"].0.abs() <= 1.0 + 1e-4, "clamped to the slider: {}", s["ParamHair"].0);
        p.strength = 0.0;
        s.get_mut("ParamHair").unwrap().0 = 0.5;
        for _ in 0..10 {
            p.step(&mut st, &mut s, 1.0 / 60.0);
        }
        assert!((s["ParamHair"].0 - 0.5).abs() < 1e-5, "no strength: the slider stays");
    }

    #[test]
    fn the_slider_values_are_stretched_into_the_range_of_the_chain() {
        let n = Range { min: -10.0, default: 0.0, max: 10.0 };
        // half way to the top of a -30..30 slider is 5 in the chain (and turned round: the chain is pushed the other way)
        assert!((normalize(15.0, -30.0, 30.0, 0.0, n, true) - 5.0).abs() < 1e-5);
        assert!((normalize(15.0, -30.0, 30.0, 0.0, n, false) + 5.0).abs() < 1e-5);
        assert!((normalize(-30.0, -30.0, 30.0, 0.0, n, true) + 10.0).abs() < 1e-5);
        assert_eq!(normalize(0.0, -30.0, 30.0, 0.0, n, true), 0.0);
        // beyond the slider's ends it is held at the end
        assert!((normalize(90.0, -30.0, 30.0, 0.0, n, true) - 10.0).abs() < 1e-5);
    }
}
