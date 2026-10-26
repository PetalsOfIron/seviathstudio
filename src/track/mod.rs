// SPDX-License-Identifier: GPL-3.0-or-later
// Tracking: what the face does (from a phone, a program or a camera) moves the sliders of the rig. The readings of the sources come
// in as `Frame`s (`net.rs`); they are turned into a handful of named inputs (the same names VTube Studio uses: FaceAngleX, EyeOpenLeft,
// MouthOpen...), and a mapping says which input moves which slider and by how much, with smoothing. A model that came with VTube
// Studio settings brings its own mapping. Blinking, breathing and looking at the mouse fill in when nothing tracks the face.

pub mod net;

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use crate::rig::{Id, Rig, Values};
use net::Frame;

/// The inputs and what they usually range over: (name, label, lowest, highest).
pub const CHANNELS: &[(&str, &str, f32, f32)] = &[
    ("FaceAngleX", "Head turn left and right", -30.0, 30.0),
    ("FaceAngleY", "Head nod", -30.0, 30.0),
    ("FaceAngleZ", "Head tilt", -30.0, 30.0),
    ("FacePositionX", "Head left and right (cm)", -10.0, 10.0),
    ("FacePositionY", "Head up and down (cm)", -10.0, 10.0),
    ("FacePositionZ", "Head near and far (cm)", -10.0, 10.0),
    ("EyeOpenLeft", "Left eye open", 0.0, 1.0),
    ("EyeOpenRight", "Right eye open", 0.0, 1.0),
    ("EyeX", "Eyes look left and right", -1.0, 1.0),
    ("EyeY", "Eyes look up and down", -1.0, 1.0),
    ("EyeLeftX", "Left eye looks sideways", -1.0, 1.0),
    ("EyeLeftY", "Left eye looks up and down", -1.0, 1.0),
    ("EyeRightX", "Right eye looks sideways", -1.0, 1.0),
    ("EyeRightY", "Right eye looks up and down", -1.0, 1.0),
    ("MouthOpen", "Mouth open", 0.0, 1.0),
    ("MouthSmile", "Smile", 0.0, 1.0),
    ("MouthX", "Mouth to the side", -1.0, 1.0),
    ("Brows", "Brows up", 0.0, 1.0),
    ("BrowLeftY", "Left brow", 0.0, 1.0),
    ("BrowRightY", "Right brow", 0.0, 1.0),
    ("CheekPuff", "Cheeks puffed", 0.0, 1.0),
    ("TongueOut", "Tongue out", 0.0, 1.0),
    ("MousePositionX", "Mouse left and right", -1.0, 1.0),
    ("MousePositionY", "Mouse up and down", -1.0, 1.0),
    ("Breath", "Breathing", 0.0, 1.0),
    ("ArmLeftAngle", "Left upper arm (0 down, 90 out, 180 up)", 0.0, 180.0),
    ("ArmRightAngle", "Right upper arm (0 down, 90 out, 180 up)", 0.0, 180.0),
    ("ForearmLeftAngle", "Left forearm (0 down, 90 out, 180 up)", 0.0, 180.0),
    ("ForearmRightAngle", "Right forearm (0 down, 90 out, 180 up)", 0.0, 180.0),
    ("BodyTilt", "Shoulders tilt", -30.0, 30.0),
    ("BodyLeanX", "Body leans left and right", -1.0, 1.0),
    ("HandLeftTurn", "Left hand bent from the arm (degrees)", -90.0, 90.0),
    ("HandRightTurn", "Right hand bent from the arm (degrees)", -90.0, 90.0),
    ("HandLeftOpen", "Left hand open", 0.0, 1.0),
    ("HandRightOpen", "Right hand open", 0.0, 1.0),
    ("FingerLeftThumb", "Left thumb open", 0.0, 1.0),
    ("FingerLeftIndex", "Left index finger open", 0.0, 1.0),
    ("FingerLeftMiddle", "Left middle finger open", 0.0, 1.0),
    ("FingerLeftRing", "Left ring finger open", 0.0, 1.0),
    ("FingerLeftPinky", "Left little finger open", 0.0, 1.0),
    ("FingerRightThumb", "Right thumb open", 0.0, 1.0),
    ("FingerRightIndex", "Right index finger open", 0.0, 1.0),
    ("FingerRightMiddle", "Right middle finger open", 0.0, 1.0),
    ("FingerRightRing", "Right ring finger open", 0.0, 1.0),
    ("FingerRightPinky", "Right little finger open", 0.0, 1.0),
];

fn avg(f: &Frame, names: &[&str]) -> Option<f32> {
    let v: Vec<f32> = names.iter().filter_map(|n| f.get(n)).collect();
    (!v.is_empty()).then(|| v.iter().sum::<f32>() / v.len() as f32)
}

/// A name that lives for the whole run, for the inputs that come from devices (a stick, a knob): the list of them is short.
pub fn intern(name: &str) -> &'static str {
    use std::sync::{Mutex, OnceLock};
    static NAMES: OnceLock<Mutex<std::collections::HashSet<&'static str>>> = OnceLock::new();
    let mut set = NAMES.get_or_init(|| Mutex::new(Default::default())).lock().unwrap();
    if let Some(s) = set.get(name) {
        return s;
    }
    if set.len() > 512 {
        // a program that sends endless different names does not fill the memory
        return "Overflow";
    }
    let s: &'static str = Box::leak(name.to_string().into_boxed_str());
    set.insert(s);
    s
}

/// The named inputs of one reading of the face.
pub fn channels(f: &Frame) -> HashMap<&'static str, f32> {
    let mut c: HashMap<&'static str, f32> = HashMap::new();
    if let Some(h) = f.head {
        c.insert("FaceAngleX", h[0]);
        c.insert("FaceAngleY", h[1]);
        c.insert("FaceAngleZ", h[2]);
    }
    if let Some(p) = f.pos {
        c.insert("FacePositionX", p[0]);
        c.insert("FacePositionY", p[1]);
        c.insert("FacePositionZ", p[2]);
    }
    // eyes: a model's own "blink" counts for both
    let blink = f.get("blink");
    let (bl, br) = (f.get("eyeBlinkLeft").or(f.get("blinkL")).or(blink), f.get("eyeBlinkRight").or(f.get("blinkR")).or(blink));
    if let Some(v) = bl {
        c.insert("EyeOpenLeft", 1.0 - v);
    }
    if let Some(v) = br {
        c.insert("EyeOpenRight", 1.0 - v);
    }
    if let (Some(l), Some(r)) = (bl, br) {
        c.insert("EyeOpen", 1.0 - (l + r) / 2.0);
    } else if let Some(v) = bl.or(br) {
        c.insert("EyeOpen", 1.0 - v);
    }
    // where the eyes look: from the turn of the eyes, or from the look shapes
    if let Some(e) = f.eyes {
        let (lx, ly, rx, ry) = ((e[0] / 30.0).clamp(-1.0, 1.0), (e[1] / 30.0).clamp(-1.0, 1.0), (e[2] / 30.0).clamp(-1.0, 1.0), (e[3] / 30.0).clamp(-1.0, 1.0));
        c.insert("EyeLeftX", lx);
        c.insert("EyeLeftY", ly);
        c.insert("EyeRightX", rx);
        c.insert("EyeRightY", ry);
    } else {
        // looking outward is to the person's own side
        if let (Some(out), Some(inn)) = (f.get("eyeLookOutLeft"), f.get("eyeLookInLeft")) {
            c.insert("EyeLeftX", out - inn);
        }
        if let (Some(out), Some(inn)) = (f.get("eyeLookOutRight"), f.get("eyeLookInRight")) {
            c.insert("EyeRightX", inn - out);
        }
        if let (Some(up), Some(down)) = (f.get("eyeLookUpLeft"), f.get("eyeLookDownLeft")) {
            c.insert("EyeLeftY", up - down);
        }
        if let (Some(up), Some(down)) = (f.get("eyeLookUpRight"), f.get("eyeLookDownRight")) {
            c.insert("EyeRightY", up - down);
        }
    }
    if let (Some(l), Some(r)) = (c.get("EyeLeftX").copied(), c.get("EyeRightX").copied()) {
        c.insert("EyeX", (l + r) / 2.0);
    }
    if let (Some(l), Some(r)) = (c.get("EyeLeftY").copied(), c.get("EyeRightY").copied()) {
        c.insert("EyeY", (l + r) / 2.0);
    }
    // mouth
    if let Some(v) = f.get("jawOpen").or(f.get("a")) {
        c.insert("MouthOpen", v);
    }
    if let Some(v) = avg(f, &["mouthSmileLeft", "mouthSmileRight"]).or(f.get("joy")) {
        c.insert("MouthSmile", v);
    }
    if let (Some(l), Some(r)) = (f.get("mouthLeft"), f.get("mouthRight")) {
        c.insert("MouthX", r - l);
    }
    // brows
    let up = avg(f, &["browInnerUp", "browOuterUpLeft", "browOuterUpRight"]);
    let down = avg(f, &["browDownLeft", "browDownRight"]).unwrap_or(0.0);
    if let Some(u) = up {
        c.insert("Brows", (u - down * 0.7).clamp(0.0, 1.0));
    }
    if let Some(v) = f.get("browOuterUpLeft") {
        c.insert("BrowLeftY", (v - f.get("browDownLeft").unwrap_or(0.0) * 0.7).clamp(0.0, 1.0));
    }
    if let Some(v) = f.get("browOuterUpRight") {
        c.insert("BrowRightY", (v - f.get("browDownRight").unwrap_or(0.0) * 0.7).clamp(0.0, 1.0));
    }
    if let Some(v) = f.get("cheekPuff") {
        c.insert("CheekPuff", v);
    }
    if let Some(v) = f.get("tongueOut") {
        c.insert("TongueOut", v);
    }
    // the body and the hands
    for (n, v) in &f.body {
        if let Some(ch) = CHANNELS.iter().find(|x| x.0 == n.as_str()) {
            c.insert(ch.0, *v);
        }
    }
    // how far the forearm is turned from the upper arm: the hand slider of a body rig is a turn from the arm
    for (side, arm, fore, out) in [("Left", "ArmLeftAngle", "ForearmLeftAngle", "HandLeftTurn"), ("Right", "ArmRightAngle", "ForearmRightAngle", "HandRightTurn")] {
        let _ = side;
        if let (Some(a), Some(b)) = (c.get(arm).copied(), c.get(fore).copied()) {
            c.insert(out, (b - a).clamp(-90.0, 90.0));
        }
    }
    c
}

/// How one input moves one slider.
#[derive(Clone, PartialEq, Debug, Serialize, Deserialize)]
pub struct Map {
    pub param: Id,
    pub input: String,
    pub in_lo: f32,
    pub in_hi: f32,
    pub out_lo: f32,
    pub out_hi: f32,
    /// Hold the slider inside its output range.
    pub clamp: bool,
    /// 0 follows at once; bigger is smoother and slower.
    pub smoothing: f32,
    pub invert: bool,
}

#[derive(Clone, PartialEq, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct TrackMapping {
    pub maps: Vec<Map>,
    /// Blink by itself when the eyes are not tracked.
    pub auto_blink: bool,
    pub breathing: bool,
    /// Eyes and head follow the mouse pointer when the face is not tracked.
    pub look_at_mouse: bool,
    /// What the face did when calibrated, taken off every reading (so the neutral face is the middle of the sliders).
    pub neutral: HashMap<String, f32>,
}

impl Default for TrackMapping {
    fn default() -> Self {
        TrackMapping { maps: Vec::new(), auto_blink: true, breathing: true, look_at_mouse: false, neutral: HashMap::new() }
    }
}

fn norm(s: &str) -> String {
    s.to_lowercase().chars().filter(|c| c.is_alphanumeric()).collect()
}

fn range_of(input: &str) -> (f32, f32) {
    CHANNELS.iter().find(|c| c.0 == input).map(|c| (c.2, c.3)).unwrap_or((0.0, 1.0))
}

impl TrackMapping {
    /// A mapping made from the rig: the settings VTube Studio saved with a Live2D model when it has them, else by the names of the sliders.
    pub fn auto(rig: &Rig) -> TrackMapping {
        let mut m = TrackMapping::default();
        if let Some(vt) = rig.vtube.as_ref().filter(|v| !v.params.is_empty()) {
            for p in &vt.params {
                let Some(param) = rig.params.iter().find(|x| !x.cid.is_empty() && x.cid == p.output) else { continue };
                // VTube Studio's own inputs have the same names; a few have another spelling
                let input = match p.input.as_str() {
                    "MouthSmile" | "MouthOpen" | "FaceAngleX" | "FaceAngleY" | "FaceAngleZ" | "FacePositionX" | "FacePositionY" | "FacePositionZ" | "EyeOpenLeft" | "EyeOpenRight" | "EyeLeftX" | "EyeLeftY" | "EyeRightX" | "EyeRightY" | "Brows" | "BrowLeftY" | "BrowRightY" | "CheekPuff" | "TongueOut" | "MouthX" | "MousePositionX" | "MousePositionY" => p.input.clone(),
                    _ => continue,
                };
                m.maps.push(Map { param: param.id, input, in_lo: p.input_lower, in_hi: p.input_upper, out_lo: p.output_lower, out_hi: p.output_upper, clamp: p.clamp_output, smoothing: p.smoothing, invert: false });
            }
            if !m.maps.is_empty() {
                return m;
            }
        }
        for p in &rig.params {
            let n = norm(&p.name);
            let pick: Option<(&str, f32, f32)> = match n.as_str() {
                "anglex" => Some(("FaceAngleX", p.min, p.max)),
                "angley" => Some(("FaceAngleY", p.min, p.max)),
                "anglez" => Some(("FaceAngleZ", p.min, p.max)),
                "eyesopen" | "eyeopen" => Some(("EyeOpen", p.min, p.max)),
                "eyelopen" | "eyeopenleft" | "eyesopenleft" => Some(("EyeOpenLeft", p.min, p.max)),
                "eyeropen" | "eyeopenright" | "eyesopenright" => Some(("EyeOpenRight", p.min, p.max)),
                "eyeballx" => Some(("EyeX", p.min, p.max)),
                "eyebally" => Some(("EyeY", p.min, p.max)),
                "brows" | "brow" | "browsy" => Some(("Brows", p.min, p.max)),
                "mouthopen" => Some(("MouthOpen", p.min, p.max)),
                "mouthform" => Some(("MouthSmile", p.default.max(p.min), p.max)),
                "eyessmile" => Some(("MouthSmile", p.min, p.max)),
                "bodytilt" | "bodyanglez" => Some(("FaceAngleZ", p.min * 0.4, p.max * 0.4)),
                "bodyanglex" => Some(("FaceAngleX", p.min * 0.5, p.max * 0.5)),
                "bodyangley" => Some(("FaceAngleY", p.min * 0.5, p.max * 0.5)),
                "breath" | "breathing" => Some(("Breath", p.min, p.max)),
                _ => None,
            };
            // the arms of the body rig follow the arms from the camera: hanging is 0, straight out is 90 (the slider is a swing from hanging)
            let arm = match n.as_str() {
                "arml" | "armleft" => Some("ArmLeftAngle"),
                "armr" | "armright" => Some("ArmRightAngle"),
                _ => None,
            };
            // the hand of the body rig turns from the arm
            let hand = match n.as_str() {
                "handl" | "handleft" => Some("HandLeftTurn"),
                "handr" | "handright" => Some("HandRightTurn"),
                _ => None,
            };
            if let Some(input) = hand {
                m.maps.push(Map { param: p.id, input: input.to_string(), in_lo: -60.0, in_hi: 60.0, out_lo: p.min, out_hi: p.max, clamp: true, smoothing: 8.0, invert: false });
                continue;
            }
            if let Some(input) = arm {
                m.maps.push(Map { param: p.id, input: input.to_string(), in_lo: 0.0, in_hi: 90.0, out_lo: 0.0, out_hi: p.max, clamp: true, smoothing: 8.0, invert: false });
                continue;
            }
            if let Some((input, lo, hi)) = pick {
                let (il, ih) = range_of(input);
                m.maps.push(Map { param: p.id, input: input.to_string(), in_lo: il, in_hi: ih, out_lo: lo, out_hi: hi, clamp: true, smoothing: if input == "Breath" { 0.0 } else { 8.0 }, invert: false });
            }
        }
        m
    }

    /// The face as it is now becomes the middle of the sliders.
    pub fn calibrate(&mut self, ch: &HashMap<&'static str, f32>) {
        self.neutral.clear();
        for name in ["FaceAngleX", "FaceAngleY", "FaceAngleZ", "FacePositionX", "FacePositionY", "FacePositionZ", "EyeX", "EyeY", "EyeLeftX", "EyeLeftY", "EyeRightX", "EyeRightY", "MouthX"] {
            if let Some(v) = ch.get(name) {
                self.neutral.insert(name.to_string(), *v);
            }
        }
    }
}

/// The One Euro filter: smooths a jittery reading without making fast moves lag.
#[derive(Clone, Debug, Default)]
pub struct OneEuro {
    x: Option<f32>,
    dx: f32,
}

impl OneEuro {
    fn alpha(cutoff: f32, dt: f32) -> f32 {
        let tau = 1.0 / (std::f32::consts::TAU * cutoff.max(1e-3));
        1.0 / (1.0 + tau / dt.max(1e-4))
    }

    pub fn filter(&mut self, x: f32, dt: f32, min_cutoff: f32, beta: f32) -> f32 {
        let Some(prev) = self.x else {
            self.x = Some(x);
            return x;
        };
        let d = (x - prev) / dt.max(1e-4);
        self.dx += Self::alpha(1.0, dt) * (d - self.dx);
        let cutoff = min_cutoff + beta * self.dx.abs();
        let a = Self::alpha(cutoff, dt);
        let out = prev + a * (x - prev);
        self.x = Some(out);
        out
    }
}

/// What changes while tracking runs.
#[derive(Default)]
pub struct TrackState {
    filters: HashMap<usize, OneEuro>,
    next_blink: f64,
    blink_started: Option<f64>,
    seed: u64,
}

impl TrackState {
    fn rand(&mut self) -> f32 {
        self.seed = self.seed.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        ((self.seed >> 33) as f32) / (1u64 << 31) as f32
    }

    /// The inputs with what fills in where the face is not tracked: a blink now and then, breathing, and the mouse.
    pub fn fill_in(&mut self, ch: &mut HashMap<&'static str, f32>, map: &TrackMapping, t: f64, mouse: Option<[f32; 2]>) {
        if map.auto_blink && !ch.contains_key("EyeOpen") && !ch.contains_key("EyeOpenLeft") {
            if self.next_blink == 0.0 {
                self.next_blink = t + 2.0 + self.rand() as f64 * 3.0;
            }
            if self.blink_started.is_none() && t >= self.next_blink {
                self.blink_started = Some(t);
            }
            let open = match self.blink_started {
                Some(s) => {
                    let k = ((t - s) / 0.18) as f32;
                    if k >= 1.0 {
                        self.blink_started = None;
                        self.next_blink = t + 2.0 + self.rand() as f64 * 4.0;
                        1.0
                    } else {
                        // closes quickly, opens a little slower
                        if k < 0.4 { 1.0 - k / 0.4 } else { (k - 0.4) / 0.6 }
                    }
                }
                None => 1.0,
            };
            for n in ["EyeOpen", "EyeOpenLeft", "EyeOpenRight"] {
                ch.insert(n, open);
            }
        }
        if map.breathing {
            ch.insert("Breath", 0.5 + 0.5 * (t as f32 * std::f32::consts::TAU / 4.5).sin());
        }
        if map.look_at_mouse {
            if let Some(m) = mouse {
                ch.entry("MousePositionX").or_insert(m[0]);
                ch.entry("MousePositionY").or_insert(m[1]);
                if !ch.contains_key("EyeX") {
                    ch.insert("EyeX", m[0]);
                    ch.insert("EyeY", m[1]);
                }
                if !ch.contains_key("FaceAngleX") {
                    ch.insert("FaceAngleX", m[0] * 12.0);
                    ch.insert("FaceAngleY", m[1] * 8.0);
                }
            }
        }
    }

    /// Put the inputs on the sliders of the rig through the mapping.
    pub fn apply(&mut self, map: &TrackMapping, ch: &HashMap<&'static str, f32>, rig: &Rig, values: &mut Values, dt: f32) {
        for (i, m) in map.maps.iter().enumerate() {
            let Some(p) = rig.params.iter().find(|p| p.id == m.param) else { continue };
            let Some(raw) = ch.get(m.input.as_str()) else { continue };
            let raw = raw - map.neutral.get(&m.input).copied().unwrap_or(0.0);
            let denom = if (m.in_hi - m.in_lo).abs() < 1e-6 { 1e-6 } else { m.in_hi - m.in_lo };
            let mut t = (raw - m.in_lo) / denom;
            if m.invert {
                t = 1.0 - t;
            }
            let mut out = m.out_lo + t * (m.out_hi - m.out_lo);
            if m.clamp {
                out = out.clamp(m.out_lo.min(m.out_hi), m.out_lo.max(m.out_hi));
            }
            if m.smoothing > 0.0 {
                let cutoff = (14.0 / (1.0 + m.smoothing)).max(0.4);
                out = self.filters.entry(i).or_default().filter(out, dt, cutoff, 0.04 * (out - p.default).abs().max(1.0).min(3.0));
            }
            values.set(p.id, out.clamp(p.min, p.max));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use net::tests::{osf_packet, osc_bundle, osc_message};

    fn frame(pairs: &[(&str, f32)]) -> Frame {
        Frame { blend: pairs.iter().map(|(k, v)| (k.to_string(), *v)).collect(), ..Default::default() }
    }

    #[test]
    fn readings_become_the_named_inputs_with_both_eyes_and_the_mouth_worked_out() {
        let mut f = frame(&[("eyeBlinkLeft", 1.0), ("eyeBlinkRight", 0.0), ("jawOpen", 0.3), ("mouthSmileLeft", 0.4), ("mouthSmileRight", 0.6), ("browInnerUp", 0.9), ("eyeLookOutLeft", 0.5), ("eyeLookInLeft", 0.1), ("mouthLeft", 0.1), ("mouthRight", 0.5)]);
        f.head = Some([12.0, -4.0, 2.0]);
        let c = channels(&f);
        assert_eq!(c["FaceAngleX"], 12.0);
        assert_eq!(c["EyeOpenLeft"], 0.0);
        assert_eq!(c["EyeOpenRight"], 1.0);
        assert_eq!(c["EyeOpen"], 0.5, "one eye closed: half");
        assert_eq!(c["MouthOpen"], 0.3);
        assert!((c["MouthSmile"] - 0.5).abs() < 1e-6);
        assert!((c["MouthX"] - 0.4).abs() < 1e-6);
        assert!((c["EyeLeftX"] - 0.4).abs() < 1e-6);
        assert!(c["Brows"] > 0.8);
        // the blink of a model that only has "blink" counts for both eyes
        assert_eq!(channels(&frame(&[("blink", 1.0)]))["EyeOpen"], 0.0);
        assert!(channels(&Frame::default()).is_empty());
    }

    #[test]
    fn a_mapping_is_made_from_the_names_of_the_sliders_and_moves_them_with_smoothing() {
        let mut rig = Rig::default();
        let x = rig.add_param("Angle X", -30.0, 30.0, 0.0);
        let open = rig.add_param("Eyes open", 0.0, 1.0, 1.0);
        let mouth = rig.add_param("Mouth open", 0.0, 1.0, 0.0);
        rig.add_param("Wing flap", -1.0, 1.0, 0.0);
        let map = TrackMapping::auto(&rig);
        assert_eq!(map.maps.len(), 3, "only the sliders a face moves");
        let mut st = TrackState::default();
        let mut v = Values::default();
        let c = channels(&{ let mut f = frame(&[("jawOpen", 0.8), ("eyeBlinkLeft", 0.0), ("eyeBlinkRight", 0.0)]); f.head = Some([15.0, 0.0, 0.0]); f });
        // many frames: the smoothed sliders settle on the values
        for _ in 0..200 {
            st.apply(&map, &c, &rig, &mut v, 1.0 / 60.0);
        }
        assert!((v.0[&x] - 15.0).abs() < 0.5, "{}", v.0[&x]);
        assert!((v.0[&open] - 1.0).abs() < 0.05);
        assert!((v.0[&mouth] - 0.8).abs() < 0.05);
        // the first reading is not delayed, and a sudden jump is followed, not jumped to at once
        let mut st2 = TrackState::default();
        let mut v2 = Values::default();
        st2.apply(&map, &c, &rig, &mut v2, 1.0 / 60.0);
        assert!((v2.0[&x] - 15.0).abs() < 1e-3);
        let jump = channels(&{ let mut f = frame(&[]); f.head = Some([-25.0, 0.0, 0.0]); f });
        st2.apply(&map, &jump, &rig, &mut v2, 1.0 / 60.0);
        assert!(v2.0[&x] > -25.0 && v2.0[&x] < 15.0, "smoothing: {}", v2.0[&x]);
    }

    #[test]
    fn calibration_makes_the_face_as_it_is_the_middle() {
        let mut rig = Rig::default();
        let x = rig.add_param("Angle X", -30.0, 30.0, 0.0);
        let mut map = TrackMapping::auto(&rig);
        map.maps[0].smoothing = 0.0;
        let c = channels(&{ let mut f = frame(&[]); f.head = Some([10.0, 0.0, 0.0]); f });
        let mut st = TrackState::default();
        let mut v = Values::default();
        st.apply(&map, &c, &rig, &mut v, 0.016);
        assert!((v.0[&x] - 10.0).abs() < 1e-4);
        map.calibrate(&c);
        st.apply(&map, &c, &rig, &mut v, 0.016);
        assert!(v.0[&x].abs() < 1e-4, "now 10 degrees is the middle: {}", v.0[&x]);
        // an inverted map turns the other way
        map.neutral.clear();
        map.maps[0].invert = true;
        st.apply(&map, &c, &rig, &mut v, 0.016);
        assert!((v.0[&x] + 10.0).abs() < 1e-4, "{}", v.0[&x]);
    }

    #[test]
    fn a_model_with_vtube_studio_settings_brings_its_own_mapping() {
        use crate::import::runtime::{Vtube, VtubeParam};
        let mut rig = Rig::default();
        let eye = rig.add_param("Eye R", 0.0, 1.9, 1.0);
        rig.params.iter_mut().find(|p| p.id == eye).unwrap().cid = "ParamEyeROpen".into();
        rig.vtube = Some(Vtube { name: "A".into(), params: vec![VtubeParam { input: "EyeOpenRight".into(), input_lower: 0.0, input_upper: 1.0, output_lower: 0.0, output_upper: 1.9, output: "ParamEyeROpen".into(), smoothing: 10.0, ..Default::default() }, VtubeParam { input: "SomethingUnknown".into(), output: "ParamEyeROpen".into(), ..Default::default() }], ..Default::default() });
        let m = TrackMapping::auto(&rig);
        assert_eq!(m.maps.len(), 1, "the unknown input is left out");
        assert_eq!((m.maps[0].input.as_str(), m.maps[0].out_hi, m.maps[0].smoothing), ("EyeOpenRight", 1.9, 10.0));
    }

    #[test]
    fn the_one_euro_filter_calms_jitter_and_follows_fast_moves() {
        let mut f = OneEuro::default();
        let mut out = Vec::new();
        for i in 0..60 {
            let jitter = if i % 2 == 0 { 0.1 } else { -0.1 };
            out.push(f.filter(1.0 + jitter, 1.0 / 60.0, 1.0, 0.0));
        }
        let spread = out[30..].iter().cloned().fold(f32::MIN, f32::max) - out[30..].iter().cloned().fold(f32::MAX, f32::min);
        assert!(spread < 0.1, "calmer than the jitter of 0.2: {spread}");
        // a big quick move with a high beta is followed fast
        let mut g = OneEuro::default();
        g.filter(0.0, 1.0 / 60.0, 0.5, 5.0);
        let mut last = 0.0;
        for _ in 0..10 {
            last = g.filter(10.0, 1.0 / 60.0, 0.5, 5.0);
        }
        assert!(last > 5.0, "{last}");
    }

    #[test]
    fn the_face_blinks_by_itself_when_not_tracked_and_breathes_and_follows_the_mouse() {
        let mut st = TrackState::default();
        let map = TrackMapping { look_at_mouse: true, ..Default::default() };
        let (mut open, mut closed) = (0, 0);
        for i in 0..4000 {
            let mut ch = HashMap::new();
            st.fill_in(&mut ch, &map, i as f64 * 0.01, Some([0.5, -0.25]));
            if ch["EyeOpen"] < 0.2 { closed += 1 } else { open += 1 }
            assert!(ch["Breath"] >= 0.0 && ch["Breath"] <= 1.0);
            assert_eq!(ch["EyeX"], 0.5);
            assert!((ch["FaceAngleX"] - 6.0).abs() < 1e-5);
        }
        // 40 seconds: some blinks, mostly open
        assert!(closed > 5 && closed < 600 && open > 3000, "{closed} {open}");
        // a tracked eye is left alone
        let mut ch = HashMap::new();
        ch.insert("EyeOpen", 0.7);
        st.fill_in(&mut ch, &map, 99.0, None);
        assert_eq!(ch["EyeOpen"], 0.7);
        // switched off
        let mut off = HashMap::new();
        st.fill_in(&mut off, &TrackMapping { auto_blink: false, breathing: false, ..Default::default() }, 1.0, None);
        assert!(off.is_empty());
    }

    #[test]
    fn packets_of_every_kind_end_up_as_the_same_inputs() {
        // a phone's VMC packet and an OpenSeeFace packet of the same turn give the same head angle input
        let q = [0.0f32, (20.0f32.to_radians() / 2.0).sin(), 0.0, (20.0f32.to_radians() / 2.0).cos()];
        let vmc = net::parse_vmc(&osc_bundle(&[osc_message("/VMC/Ext/Bone/Pos", &[net::Arg::Str("Head".into()), net::Arg::Float(0.0), net::Arg::Float(0.0), net::Arg::Float(0.0), net::Arg::Float(q[0]), net::Arg::Float(q[1]), net::Arg::Float(q[2]), net::Arg::Float(q[3])])])).unwrap();
        let osf = net::parse_osf(&osf_packet([0.0, 20.0, 0.0], 1.0, 1.0, 0.0, 0.0, true)).unwrap();
        assert!((channels(&vmc)["FaceAngleX"] - channels(&osf)["FaceAngleX"]).abs() < 0.1);
    }

    #[test]
    fn the_arm_sliders_of_the_body_rig_follow_the_arms_of_the_camera() {
        let mut rig = Rig::default();
        let l = rig.add_param("Arm L", -45.0, 45.0, 0.0);
        let _r = rig.add_param("Arm R", -45.0, 45.0, 0.0);
        let m = TrackMapping::auto(&rig);
        let ml = m.maps.iter().find(|x| x.param == l).expect("a map for Arm L");
        assert_eq!((ml.input.as_str(), ml.in_lo, ml.in_hi, ml.out_lo, ml.out_hi), ("ArmLeftAngle", 0.0, 90.0, 0.0, 45.0));
        assert!(m.maps.iter().any(|x| x.input == "ArmRightAngle"));
        // the hand turns from the arm: a forearm 30 degrees above the upper arm is a hand turn of 30
        let ch = channels(&Frame { body: [("ArmLeftAngle".to_string(), 60.0), ("ForearmLeftAngle".to_string(), 90.0)].into_iter().collect(), ..Default::default() });
        assert_eq!(ch.get("HandLeftTurn"), Some(&30.0));
        let mut rig2 = Rig::default();
        let hl = rig2.add_param("Hand L", -40.0, 40.0, 0.0);
        let m2 = TrackMapping::auto(&rig2);
        assert!(m2.maps.iter().any(|x| x.param == hl && x.input == "HandLeftTurn"));
    }
}
