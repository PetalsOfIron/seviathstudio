// SPDX-License-Identifier: GPL-3.0-or-later
// Animations for the rigged 3D character: 22 ready motions (idle, walk, run, jump, wave, bow, dance, punch, kick...) written as code, so
// they are ours and can be sold with the characters, and a generator that mixes a motion with a mood, a speed and a strength (and a
// little chance) from a few words. A motion is the turn of the humanoid bones (in degrees, in the bone's own rest space of the VRM
// T-pose) and how far the hips move, for every moment. The files that carry them are written by `vrm/vrma.rs` and `vrm/export.rs`.
//
// The turns: `x` swings forward and back (for a hanging arm or leg a negative x is forward; for the spine and the head a positive x leans
// forward), `y` twists, `z` leans sideways or raises an arm (the left arm is raised by a positive z, the right arm by a negative one).
// The order is x after z after y, so an arm that hangs can still swing forward. The hips offset is in metres (up is y, forward is z).

use std::collections::BTreeMap;
use std::f32::consts::TAU;

/// The bones the motions turn.
pub const BONES: [&str; 22] = [
    "hips", "spine", "chest", "neck", "head", "leftShoulder", "leftUpperArm", "leftLowerArm", "leftHand", "rightShoulder", "rightUpperArm", "rightLowerArm", "rightHand", "leftUpperLeg", "leftLowerLeg", "leftFoot", "rightUpperLeg", "rightLowerLeg",
    "rightFoot", "leftToes", "rightToes", "upperChest",
];

/// The turn of the bones and the move of the hips at one moment.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Frame {
    pub rot: BTreeMap<&'static str, [f32; 3]>,
    pub hips: [f32; 3],
}

impl Frame {
    /// Standing with the arms down: the TPose of the model turned to a relaxed stand.
    pub fn rest() -> Frame {
        let mut f = Frame::default();
        f.add("leftUpperArm", [0.0, 0.0, -72.0]);
        f.add("rightUpperArm", [0.0, 0.0, 72.0]);
        f.add("leftLowerArm", [0.0, -8.0, 0.0]);
        f.add("rightLowerArm", [0.0, 8.0, 0.0]);
        f
    }

    /// Add a turn to what a bone already has.
    pub fn add(&mut self, bone: &'static str, r: [f32; 3]) {
        let e = self.rot.entry(bone).or_insert([0.0; 3]);
        for k in 0..3 {
            e[k] += r[k];
        }
    }

    pub fn get(&self, bone: &str) -> [f32; 3] {
        self.rot.get(bone).copied().unwrap_or([0.0; 3])
    }

    /// The same frame, as turns from the relaxed stand (arms down): for a model that is built in that pose and not in a T-pose.
    pub fn minus_rest(&self) -> Frame {
        let rest = Frame::rest();
        let mut out = Frame::default();
        for b in BONES {
            let (a, r) = (self.get(b), rest.get(b));
            let v = [a[0] - r[0], a[1] - r[1], a[2] - r[2]];
            if v.iter().any(|x| x.abs() > 1e-6) {
                out.rot.insert(b, v);
            }
        }
        out.hips = self.hips;
        out
    }

    /// This frame mixed with another: 0 is this one, 1 the other.
    pub fn mix(&self, other: &Frame, t: f32) -> Frame {
        let mut out = Frame::default();
        for b in BONES {
            let (a, c) = (self.get(b), other.get(b));
            let v = [a[0] + (c[0] - a[0]) * t, a[1] + (c[1] - a[1]) * t, a[2] + (c[2] - a[2]) * t];
            if v != [0.0; 3] {
                out.rot.insert(b, v);
            }
        }
        out.hips = [self.hips[0] + (other.hips[0] - self.hips[0]) * t, self.hips[1] + (other.hips[1] - self.hips[1]) * t, self.hips[2] + (other.hips[2] - self.hips[2]) * t];
        out
    }

    /// The turn of every bone as a quaternion (x, y, z, w).
    pub fn quat(&self, bone: &str) -> [f32; 4] {
        euler_quat(self.get(bone))
    }
}

fn axis_quat(axis: usize, deg: f32) -> [f32; 4] {
    let h = deg.to_radians() * 0.5;
    let mut q = [0.0, 0.0, 0.0, h.cos()];
    q[axis] = h.sin();
    q
}

fn qmul(a: [f32; 4], b: [f32; 4]) -> [f32; 4] {
    [
        a[3] * b[0] + a[0] * b[3] + a[1] * b[2] - a[2] * b[1],
        a[3] * b[1] - a[0] * b[2] + a[1] * b[3] + a[2] * b[0],
        a[3] * b[2] + a[0] * b[1] - a[1] * b[0] + a[2] * b[3],
        a[3] * b[3] - a[0] * b[0] - a[1] * b[1] - a[2] * b[2],
    ]
}

/// The turn x after z after y (degrees) as a quaternion (x, y, z, w).
pub fn euler_quat(r: [f32; 3]) -> [f32; 4] {
    qmul(qmul(axis_quat(0, r[0]), axis_quat(2, r[2])), axis_quat(1, r[1]))
}

/// A point turned by the same: x after z after y, about the origin.
pub fn euler_apply(r: [f32; 3], p: [f32; 3]) -> [f32; 3] {
    let rot = |axis: usize, deg: f32, p: [f32; 3]| -> [f32; 3] {
        let (s, c) = deg.to_radians().sin_cos();
        match axis {
            0 => [p[0], p[1] * c - p[2] * s, p[1] * s + p[2] * c],
            1 => [p[0] * c + p[2] * s, p[1], -p[0] * s + p[2] * c],
            _ => [p[0] * c - p[1] * s, p[0] * s + p[1] * c, p[2]],
        }
    };
    rot(0, r[0], rot(2, r[2], rot(1, r[1], p)))
}

// ---------------------------------------------------------------------------------------------- the motions

/// One of the ready motions.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Motion {
    IdleBreathe,
    IdleShift,
    Walk,
    Run,
    Sneak,
    Jump,
    Duck,
    LookAround,
    Wave,
    NodYes,
    ShakeNo,
    Shrug,
    Bow,
    Cheer,
    Clap,
    Point,
    Punch,
    Kick,
    Slash,
    HitReaction,
    DanceSway,
    DanceBounce,
    Laugh,
}

pub struct Info {
    pub motion: Motion,
    pub name: &'static str,
    pub category: &'static str,
    /// Seconds for one go.
    pub seconds: f32,
    /// Plays again and again (the end joins the start).
    pub looping: bool,
    pub about: &'static str,
    /// Words that name it, for the text that describes a motion.
    pub words: &'static [&'static str],
}

const fn info(motion: Motion, name: &'static str, category: &'static str, seconds: f32, looping: bool, about: &'static str, words: &'static [&'static str]) -> Info {
    Info { motion, name, category, seconds, looping, about, words }
}

pub const CATEGORIES: [&str; 5] = ["Standing", "Moving", "Gestures", "Fighting", "Dance and fun"];

pub const MOTIONS: &[Info] = &[
    info(Motion::IdleBreathe, "Idle, breathing", "Standing", 4.0, true, "Standing still and breathing slowly. The first motion for any character.", &["idle", "breathe", "breathing", "stand", "standing", "wait"]),
    info(Motion::IdleShift, "Idle, shifting weight", "Standing", 6.0, true, "Standing and moving the weight from foot to foot, so the character looks alive.", &["shift", "weight", "relax", "relaxed", "bored"]),
    info(Motion::LookAround, "Look around", "Standing", 6.0, true, "The head turns to one side, then the other, as if looking for something.", &["look", "looking", "around", "search", "scan", "watch"]),
    info(Motion::Walk, "Walk", "Moving", 1.0, true, "A walk cycle, two steps. The hips move up and down and the arms swing.", &["walk", "walking", "stroll", "step"]),
    info(Motion::Run, "Run", "Moving", 0.7, true, "A run cycle, two strides, leaning forward.", &["run", "running", "sprint", "dash", "jog"]),
    info(Motion::Sneak, "Sneak", "Moving", 1.4, true, "Walking low and slow, so nobody hears.", &["sneak", "sneaking", "creep", "stealth", "tiptoe"]),
    info(Motion::Jump, "Jump", "Moving", 1.1, false, "Bends the knees, jumps with the arms up, and lands.", &["jump", "jumping", "hop", "leap"]),
    info(Motion::Duck, "Duck", "Moving", 1.4, false, "Goes down into a crouch, stays a moment, comes up.", &["duck", "crouch", "squat", "dodge", "kneel"]),
    info(Motion::Wave, "Wave hello", "Gestures", 2.4, false, "Raises the right hand and waves it.", &["wave", "waving", "hello", "hi", "greet", "bye", "goodbye"]),
    info(Motion::NodYes, "Nod yes", "Gestures", 1.6, false, "The head nods twice.", &["nod", "yes", "agree", "ok"]),
    info(Motion::ShakeNo, "Shake no", "Gestures", 1.6, false, "The head shakes from side to side.", &["shake", "no", "disagree", "refuse", "deny"]),
    info(Motion::Shrug, "Shrug", "Gestures", 1.8, false, "The shoulders go up, the hands open: I do not know.", &["shrug", "unsure", "dunno", "maybe"]),
    info(Motion::Bow, "Bow", "Gestures", 2.6, false, "A polite bow from the waist, and back up.", &["bow", "bowing", "thank", "polite", "respect"]),
    info(Motion::Point, "Point forward", "Gestures", 2.2, false, "The right arm points forward and stays there for a moment.", &["point", "pointing", "there", "direct"]),
    info(Motion::Clap, "Clap", "Gestures", 1.2, true, "Clapping the hands in front of the chest.", &["clap", "clapping", "applaud", "applause"]),
    info(Motion::Punch, "Punch", "Fighting", 0.9, false, "A straight punch with the right fist, the left hand guarding.", &["punch", "punching", "hit", "strike", "fist", "box"]),
    info(Motion::Kick, "Kick", "Fighting", 1.2, false, "A forward kick with the right leg.", &["kick", "kicking"]),
    info(Motion::Slash, "Slash", "Fighting", 1.1, false, "An overhead swing, for a sword or an axe, from high on the right to low on the left.", &["slash", "swing", "sword", "attack", "axe", "cut", "chop"]),
    info(Motion::HitReaction, "Hit reaction", "Fighting", 0.8, false, "Thrown back by a blow, then back on the feet.", &["hurt", "flinch", "damage", "ouch", "reaction", "stagger"]),
    info(Motion::Cheer, "Cheer", "Dance and fun", 1.6, true, "Both arms up, hopping with joy.", &["cheer", "cheering", "celebrate", "victory", "win", "hooray", "yay"]),
    info(Motion::DanceSway, "Dance, sway", "Dance and fun", 4.0, true, "A slow dance, the hips swaying and the arms floating.", &["dance", "dancing", "sway", "slow", "waltz"]),
    info(Motion::DanceBounce, "Dance, bounce", "Dance and fun", 1.0, true, "A quick dance, bouncing on the knees with the arms pumping.", &["bounce", "party", "disco", "groove", "jump around"]),
    info(Motion::Laugh, "Laugh", "Dance and fun", 2.0, true, "Laughing: the body shakes, the head goes back, a hand on the belly.", &["laugh", "laughing", "giggle", "haha", "funny"]),
];

pub fn info_of(m: Motion) -> &'static Info {
    MOTIONS.iter().find(|i| i.motion == m).expect("every motion has its info")
}

/// A value that follows `keys` (time 0 to 1, value) with soft steps between them.
fn keyed(t: f32, keys: &[(f32, f32)]) -> f32 {
    let t = t.clamp(0.0, 1.0);
    if t <= keys[0].0 {
        return keys[0].1;
    }
    for w in keys.windows(2) {
        if t <= w[1].0 {
            let u = ((t - w[0].0) / (w[1].0 - w[0].0).max(1e-6)).clamp(0.0, 1.0);
            let s = u * u * (3.0 - 2.0 * u);
            return w[0].1 + (w[1].1 - w[0].1) * s;
        }
    }
    keys[keys.len() - 1].1
}

/// A hump that rises over the first part and falls over the last: 0 at both ends, 1 between `a` and `b`.
fn hump(t: f32, a: f32, b: f32) -> f32 {
    keyed(t, &[(0.0, 0.0), (a, 1.0), (b, 1.0), (1.0, 0.0)])
}

/// The motion at the moment `t` of 0 to 1 (a loop joins at 1 and 0).
pub fn eval(m: Motion, t: f32) -> Frame {
    let w = TAU * t;
    let mut f = Frame::rest();
    let s = |x: f32| x.sin();
    match m {
        Motion::IdleBreathe => {
            f.add("chest", [1.6 * s(w), 0.0, 0.0]);
            f.add("spine", [0.8 * s(w), 0.0, 0.0]);
            f.add("head", [-0.8 * s(w), 0.0, 0.0]);
            f.add("leftShoulder", [0.0, 0.0, 1.2 * s(w)]);
            f.add("rightShoulder", [0.0, 0.0, -1.2 * s(w)]);
            f.add("leftUpperArm", [0.0, 0.0, 0.8 * s(w)]);
            f.add("rightUpperArm", [0.0, 0.0, -0.8 * s(w)]);
            f.hips[1] = 0.004 * s(w);
        }
        Motion::IdleShift => {
            f.add("hips", [0.0, 2.0 * s(w), 3.0 * s(w)]);
            f.add("spine", [0.0, -1.5 * s(w), -2.0 * s(w)]);
            f.add("head", [0.0, 2.0 * s(w), 1.5 * s(w)]);
            f.add("leftUpperLeg", [-3.0 * (0.5 - 0.5 * s(w)), 0.0, 0.0]);
            f.add("leftLowerLeg", [5.0 * (0.5 - 0.5 * s(w)), 0.0, 0.0]);
            f.add("rightUpperLeg", [-3.0 * (0.5 + 0.5 * s(w)), 0.0, 0.0]);
            f.add("rightLowerLeg", [5.0 * (0.5 + 0.5 * s(w)), 0.0, 0.0]);
            f.add("chest", [1.0 * (0.5 + 0.5 * (2.0 * w).sin()), 0.0, 0.0]);
            f.hips[0] = 0.015 * s(w);
        }
        Motion::LookAround => {
            let look = keyed(t, &[(0.0, 0.0), (0.12, 38.0), (0.3, 38.0), (0.5, 0.0), (0.62, -42.0), (0.82, -42.0), (1.0, 0.0)]);
            f.add("head", [-2.0 + 2.0 * (look.abs() / 40.0), look * 0.65, 0.0]);
            f.add("neck", [0.0, look * 0.3, 0.0]);
            f.add("chest", [0.0, look * 0.15, 0.0]);
            f.add("spine", [0.0, look * 0.08, 0.0]);
        }
        Motion::Walk => {
            let (l, r) = (s(w), -s(w));
            walk_legs(&mut f, w, 28.0, 38.0);
            f.add("leftUpperArm", [-24.0 * l, 0.0, 0.0]);
            f.add("rightUpperArm", [-24.0 * r, 0.0, 0.0]);
            f.add("leftLowerArm", [0.0, -14.0 - 10.0 * l.max(0.0), 0.0]);
            f.add("rightLowerArm", [0.0, 14.0 + 10.0 * r.max(0.0), 0.0]);
            f.add("hips", [0.0, 5.0 * s(w), 2.0 * (w).cos()]);
            f.add("spine", [2.0, -4.0 * s(w), 0.0]);
            f.add("head", [0.0, 2.0 * s(w), 0.0]);
            f.hips[1] = 0.018 * (2.0 * w).cos() - 0.018;
        }
        Motion::Run => {
            let (l, r) = (s(w), -s(w));
            walk_legs(&mut f, w, 48.0, 85.0);
            f.add("leftUpperArm", [0.0, 0.0, 22.0]);
            f.add("rightUpperArm", [0.0, 0.0, -22.0]);
            f.add("leftUpperArm", [-44.0 * l, 0.0, 0.0]);
            f.add("rightUpperArm", [-44.0 * r, 0.0, 0.0]);
            f.add("leftLowerArm", [0.0, -75.0, 0.0]);
            f.add("rightLowerArm", [0.0, 75.0, 0.0]);
            f.add("hips", [10.0, 8.0 * s(w), 3.0 * (w).cos()]);
            f.add("spine", [4.0, -8.0 * s(w), 0.0]);
            f.add("head", [-8.0, 3.0 * s(w), 0.0]);
            f.hips[1] = 0.05 * (2.0 * w).cos() - 0.07;
            f.hips[2] = 0.0;
        }
        Motion::Sneak => {
            walk_legs(&mut f, w, 16.0, 30.0);
            f.add("leftUpperLeg", [-34.0, 0.0, 0.0]);
            f.add("rightUpperLeg", [-34.0, 0.0, 0.0]);
            f.add("leftLowerLeg", [60.0, 0.0, 0.0]);
            f.add("rightLowerLeg", [60.0, 0.0, 0.0]);
            f.add("hips", [18.0, 4.0 * s(w), 0.0]);
            f.add("spine", [14.0, -3.0 * s(w), 0.0]);
            f.add("head", [-18.0, 0.0, 0.0]);
            f.add("leftUpperArm", [-10.0 + 10.0 * s(w), 0.0, 8.0]);
            f.add("rightUpperArm", [-10.0 - 10.0 * s(w), 0.0, -8.0]);
            f.add("leftLowerArm", [0.0, -40.0, 0.0]);
            f.add("rightLowerArm", [0.0, 40.0, 0.0]);
            f.hips[1] = -0.2 + 0.01 * (2.0 * w).cos();
            f.hips[2] = 0.05;
        }
        Motion::Jump => {
            let crouch = keyed(t, &[(0.0, 0.0), (0.22, 1.0), (0.3, 0.9), (0.38, 0.0), (0.72, 0.0), (0.84, 0.9), (1.0, 0.0)]);
            let air = hump(keyed(t, &[(0.0, 0.0), (0.3, 0.0), (0.38, 0.0), (0.6, 0.5), (0.72, 0.0), (1.0, 0.0)]), 0.5, 0.5);
            let height = if (0.34..0.76).contains(&t) { 0.55 * (std::f32::consts::PI * (t - 0.34) / 0.42).sin() } else { 0.0 };
            let arms = keyed(t, &[(0.0, 0.0), (0.2, -0.6), (0.3, 0.2), (0.42, 1.0), (0.66, 0.8), (0.8, 0.0), (1.0, 0.0)]);
            let _ = air;
            f.add("leftUpperLeg", [-50.0 * crouch - 12.0 * (height / 0.55), 0.0, 0.0]);
            f.add("rightUpperLeg", [-50.0 * crouch - 12.0 * (height / 0.55), 0.0, 0.0]);
            f.add("leftLowerLeg", [95.0 * crouch + 25.0 * (height / 0.55), 0.0, 0.0]);
            f.add("rightLowerLeg", [95.0 * crouch + 25.0 * (height / 0.55), 0.0, 0.0]);
            f.add("hips", [15.0 * crouch, 0.0, 0.0]);
            f.add("spine", [10.0 * crouch, 0.0, 0.0]);
            f.add("head", [-12.0 * crouch, 0.0, 0.0]);
            f.add("leftUpperArm", [if arms < 0.0 { arms * 40.0 } else { 0.0 }, 0.0, 140.0 * arms.max(0.0)]);
            f.add("rightUpperArm", [if arms < 0.0 { arms * 40.0 } else { 0.0 }, 0.0, -140.0 * arms.max(0.0)]);
            f.hips[1] = -0.28 * crouch + height;
        }
        Motion::Duck => {
            let d = keyed(t, &[(0.0, 0.0), (0.3, 1.0), (0.7, 1.0), (1.0, 0.0)]);
            f.add("leftUpperLeg", [-75.0 * d, 0.0, -4.0 * d]);
            f.add("rightUpperLeg", [-75.0 * d, 0.0, 4.0 * d]);
            f.add("leftLowerLeg", [120.0 * d, 0.0, 0.0]);
            f.add("rightLowerLeg", [120.0 * d, 0.0, 0.0]);
            f.add("hips", [24.0 * d, 0.0, 0.0]);
            f.add("spine", [16.0 * d, 0.0, 0.0]);
            f.add("head", [-28.0 * d, 0.0, 0.0]);
            f.add("leftUpperArm", [-30.0 * d, 0.0, 6.0 * d]);
            f.add("rightUpperArm", [-30.0 * d, 0.0, -6.0 * d]);
            f.add("leftLowerArm", [0.0, -45.0 * d, 0.0]);
            f.add("rightLowerArm", [0.0, 45.0 * d, 0.0]);
            f.hips[1] = -0.38 * d;
            f.hips[2] = 0.1 * d;
        }
        Motion::Wave => {
            let up = hump(t, 0.18, 0.82);
            f.add("rightUpperArm", [0.0, 0.0, -150.0 * up]);
            f.add("rightUpperArm", [-12.0 * up, 0.0, 0.0]);
            f.add("rightLowerArm", [0.0, 0.0, 0.0]);
            f.add("rightLowerArm", [0.0, -55.0 * up, 28.0 * up * (5.0 * w).sin()]);
            f.add("head", [0.0, 0.0, -5.0 * up]);
            f.add("chest", [0.0, 0.0, 2.0 * up]);
            f.add("spine", [0.0, -3.0 * up, 0.0]);
        }
        Motion::NodYes => {
            let k = hump(t, 0.1, 0.9);
            f.add("head", [14.0 * k * (0.5 - 0.5 * (2.0 * w).cos()) - 2.0 * k, 0.0, 0.0]);
            f.add("neck", [4.0 * k * (0.5 - 0.5 * (2.0 * w).cos()), 0.0, 0.0]);
        }
        Motion::ShakeNo => {
            let k = hump(t, 0.1, 0.9);
            f.add("head", [0.0, 28.0 * k * (2.0 * w).sin(), 0.0]);
            f.add("neck", [0.0, 8.0 * k * (2.0 * w).sin(), 0.0]);
            f.add("chest", [0.0, 3.0 * k * (2.0 * w).sin(), 0.0]);
        }
        Motion::Shrug => {
            let k = hump(t, 0.25, 0.7);
            f.add("leftShoulder", [0.0, 0.0, 14.0 * k]);
            f.add("rightShoulder", [0.0, 0.0, -14.0 * k]);
            f.add("leftUpperArm", [-14.0 * k, 0.0, 12.0 * k]);
            f.add("rightUpperArm", [-14.0 * k, 0.0, -12.0 * k]);
            f.add("leftLowerArm", [0.0, -62.0 * k, 0.0]);
            f.add("rightLowerArm", [0.0, 62.0 * k, 0.0]);
            f.add("leftHand", [0.0, 0.0, 25.0 * k]);
            f.add("rightHand", [0.0, 0.0, -25.0 * k]);
            f.add("head", [-3.0 * k, 0.0, 9.0 * k]);
            f.add("chest", [-2.0 * k, 0.0, 0.0]);
        }
        Motion::Bow => {
            let k = keyed(t, &[(0.0, 0.0), (0.35, 1.0), (0.6, 1.0), (1.0, 0.0)]);
            f.add("hips", [10.0 * k, 0.0, 0.0]);
            f.add("spine", [24.0 * k, 0.0, 0.0]);
            f.add("chest", [14.0 * k, 0.0, 0.0]);
            f.add("head", [12.0 * k, 0.0, 0.0]);
            f.add("leftUpperArm", [-8.0 * k, 0.0, 4.0 * k]);
            f.add("rightUpperArm", [-8.0 * k, 0.0, -4.0 * k]);
            f.add("leftLowerArm", [0.0, -25.0 * k, 0.0]);
            f.add("rightLowerArm", [0.0, 25.0 * k, 0.0]);
            f.hips[2] = -0.06 * k;
            f.hips[1] = -0.02 * k;
        }
        Motion::Point => {
            let k = hump(t, 0.22, 0.8);
            f.add("rightUpperArm", [-92.0 * k, 0.0, -66.0 * k]);
            f.add("rightLowerArm", [0.0, 8.0 * -k, 0.0]);
            f.add("rightHand", [0.0, 0.0, 0.0]);
            f.add("spine", [0.0, -6.0 * k, 0.0]);
            f.add("chest", [0.0, -6.0 * k, 0.0]);
            f.add("head", [0.0, -4.0 * k, 0.0]);
        }
        Motion::Clap => {
            let c = 0.5 + 0.5 * (4.0 * w).sin();
            f.add("leftUpperArm", [-58.0, 0.0, 52.0]);
            f.add("rightUpperArm", [-58.0, 0.0, -52.0]);
            f.add("leftLowerArm", [0.0, -70.0, 0.0]);
            f.add("rightLowerArm", [0.0, 70.0, 0.0]);
            f.add("leftUpperArm", [0.0, 14.0 * c, 0.0]);
            f.add("rightUpperArm", [0.0, -14.0 * c, 0.0]);
            f.add("head", [3.0 * c, 0.0, 0.0]);
        }
        Motion::Punch => {
            let wind = keyed(t, &[(0.0, 0.0), (0.3, 1.0), (0.38, 1.0), (0.5, 0.0), (1.0, 0.0)]);
            let hit = keyed(t, &[(0.0, 0.0), (0.3, 0.0), (0.45, 1.0), (0.58, 1.0), (0.85, 0.0), (1.0, 0.0)]);
            f.add("leftUpperArm", [-70.0, 0.0, 30.0 * (1.0 - 0.0)]);
            f.add("leftLowerArm", [0.0, -100.0, 0.0]);
            f.add("rightUpperArm", [40.0 * wind - 95.0 * hit, 0.0, -45.0 * (wind + hit).min(1.0)]);
            f.add("rightLowerArm", [0.0, 110.0 * wind * (1.0 - hit) + 6.0 * hit, 0.0]);
            f.add("spine", [4.0 * hit, -22.0 * wind + 38.0 * hit, 0.0]);
            f.add("chest", [0.0, -10.0 * wind + 18.0 * hit, 0.0]);
            f.add("hips", [0.0, -8.0 * wind + 14.0 * hit, 0.0]);
            f.add("leftUpperLeg", [-14.0 - 6.0 * hit, 0.0, -4.0]);
            f.add("rightUpperLeg", [8.0 + 4.0 * hit, 0.0, 4.0]);
            f.add("leftLowerLeg", [18.0, 0.0, 0.0]);
            f.add("rightLowerLeg", [14.0, 0.0, 0.0]);
            f.hips[2] = 0.08 * hit - 0.04 * wind;
            f.hips[1] = -0.04;
        }
        Motion::Kick => {
            let lift = keyed(t, &[(0.0, 0.0), (0.25, 1.0), (0.4, 1.0), (0.55, 0.0), (1.0, 0.0)]);
            let ext = keyed(t, &[(0.0, 0.0), (0.25, 0.0), (0.42, 1.0), (0.58, 1.0), (0.72, 0.0), (1.0, 0.0)]);
            let up = hump(t, 0.22, 0.7);
            f.add("rightUpperLeg", [-110.0 * ext - 70.0 * lift * (1.0 - ext), 0.0, -8.0 * up]);
            f.add("rightLowerLeg", [95.0 * lift * (1.0 - ext) + 6.0 * ext, 0.0, 0.0]);
            f.add("leftUpperLeg", [-10.0 * up, 0.0, 0.0]);
            f.add("leftLowerLeg", [14.0 * up, 0.0, 0.0]);
            f.add("spine", [-12.0 * ext, 0.0, 0.0]);
            f.add("hips", [-6.0 * ext, 0.0, 0.0]);
            f.add("leftUpperArm", [0.0, 0.0, 28.0 * up]);
            f.add("rightUpperArm", [0.0, 0.0, -28.0 * up]);
            f.add("leftUpperArm", [-30.0 * up, 0.0, 0.0]);
            f.add("leftLowerArm", [0.0, -55.0 * up, 0.0]);
            f.add("rightLowerArm", [0.0, 55.0 * up, 0.0]);
            f.hips[1] = -0.04 * up;
        }
        Motion::Slash => {
            let raise = keyed(t, &[(0.0, 0.0), (0.3, 1.0), (0.4, 1.0), (0.55, 0.0), (1.0, 0.0)]);
            let cut = keyed(t, &[(0.0, 0.0), (0.38, 0.0), (0.55, 1.0), (0.72, 1.0), (1.0, 0.0)]);
            f.add("rightUpperArm", [-20.0 * raise - 70.0 * cut, 0.0, -165.0 * raise + 100.0 * cut]);
            f.add("rightLowerArm", [0.0, 40.0 * raise * (1.0 - cut) + 14.0 * cut, 0.0]);
            f.add("leftUpperArm", [-40.0 * raise - 30.0 * cut, 0.0, 14.0 * raise]);
            f.add("leftLowerArm", [0.0, -80.0 * (raise + cut).min(1.0), 0.0]);
            f.add("spine", [-8.0 * raise + 22.0 * cut, 22.0 * raise - 40.0 * cut, 0.0]);
            f.add("chest", [0.0, 10.0 * raise - 20.0 * cut, 0.0]);
            f.add("hips", [0.0, 12.0 * raise - 20.0 * cut, 0.0]);
            f.add("leftUpperLeg", [-16.0 * cut, 0.0, -4.0]);
            f.add("leftLowerLeg", [24.0 * cut, 0.0, 0.0]);
            f.add("rightUpperLeg", [8.0 * cut, 0.0, 4.0]);
            f.hips[2] = 0.06 * cut;
            f.hips[1] = -0.08 * cut;
        }
        Motion::HitReaction => {
            let hit = keyed(t, &[(0.0, 0.0), (0.12, 1.0), (0.3, 0.85), (1.0, 0.0)]);
            f.add("spine", [-20.0 * hit, 6.0 * hit, 0.0]);
            f.add("chest", [-10.0 * hit, 0.0, 4.0 * hit]);
            f.add("head", [-22.0 * hit, 12.0 * hit, 0.0]);
            f.add("leftUpperArm", [10.0 * hit, 0.0, 26.0 * hit]);
            f.add("rightUpperArm", [10.0 * hit, 0.0, -26.0 * hit]);
            f.add("leftLowerArm", [0.0, -20.0 * hit, 0.0]);
            f.add("rightLowerArm", [0.0, 20.0 * hit, 0.0]);
            f.add("hips", [-8.0 * hit, 0.0, 0.0]);
            f.add("leftUpperLeg", [-14.0 * hit, 0.0, 0.0]);
            f.add("rightUpperLeg", [8.0 * hit, 0.0, 0.0]);
            f.add("leftLowerLeg", [20.0 * hit, 0.0, 0.0]);
            f.hips[2] = -0.12 * hit;
            f.hips[1] = -0.03 * hit;
        }
        Motion::Cheer => {
            let hop = (w).sin().abs();
            f.add("leftUpperArm", [0.0, 0.0, 155.0 + 10.0 * (2.0 * w).sin()]);
            f.add("rightUpperArm", [0.0, 0.0, -155.0 - 10.0 * (2.0 * w).sin()]);
            f.add("leftLowerArm", [0.0, 0.0, 18.0 * (2.0 * w).sin()]);
            f.add("rightLowerArm", [0.0, 0.0, -18.0 * (2.0 * w).sin()]);
            f.add("chest", [-6.0, 0.0, 0.0]);
            f.add("head", [-10.0, 0.0, 0.0]);
            f.add("leftUpperLeg", [-20.0 * (1.0 - hop), 0.0, 0.0]);
            f.add("rightUpperLeg", [-20.0 * (1.0 - hop), 0.0, 0.0]);
            f.add("leftLowerLeg", [34.0 * (1.0 - hop), 0.0, 0.0]);
            f.add("rightLowerLeg", [34.0 * (1.0 - hop), 0.0, 0.0]);
            f.hips[1] = 0.12 * hop - 0.06;
        }
        Motion::DanceSway => {
            f.add("hips", [0.0, 6.0 * s(w), 9.0 * s(w)]);
            f.add("spine", [0.0, -4.0 * s(w), -6.0 * s(w)]);
            f.add("chest", [0.0, -3.0 * s(w), -3.0 * s(w)]);
            f.add("head", [0.0, 0.0, 6.0 * s(w)]);
            f.add("leftUpperArm", [-10.0, 0.0, 50.0 + 30.0 * s(w)]);
            f.add("rightUpperArm", [-10.0, 0.0, -50.0 + 30.0 * s(w)]);
            f.add("leftLowerArm", [0.0, -30.0, 14.0 * s(w)]);
            f.add("rightLowerArm", [0.0, 30.0, 14.0 * s(w)]);
            f.add("leftUpperLeg", [-8.0 * (0.5 + 0.5 * s(w)), 0.0, 0.0]);
            f.add("rightUpperLeg", [-8.0 * (0.5 - 0.5 * s(w)), 0.0, 0.0]);
            f.add("leftLowerLeg", [12.0 * (0.5 + 0.5 * s(w)), 0.0, 0.0]);
            f.add("rightLowerLeg", [12.0 * (0.5 - 0.5 * s(w)), 0.0, 0.0]);
            f.hips[0] = 0.04 * s(w);
            f.hips[1] = 0.01 * (2.0 * w).cos() - 0.01;
        }
        Motion::DanceBounce => {
            let b = 0.5 + 0.5 * (2.0 * w).cos();
            f.add("leftUpperLeg", [-24.0 * b, 0.0, -3.0]);
            f.add("rightUpperLeg", [-24.0 * b, 0.0, 3.0]);
            f.add("leftLowerLeg", [44.0 * b, 0.0, 0.0]);
            f.add("rightLowerLeg", [44.0 * b, 0.0, 0.0]);
            f.add("hips", [6.0 * b, 0.0, 5.0 * s(w)]);
            f.add("spine", [0.0, 0.0, -4.0 * s(w)]);
            f.add("head", [8.0 * b - 4.0, 0.0, 6.0 * s(w)]);
            f.add("leftUpperArm", [-30.0, 0.0, 30.0 + 45.0 * s(w)]);
            f.add("rightUpperArm", [-30.0, 0.0, -30.0 + 45.0 * s(w)]);
            f.add("leftLowerArm", [0.0, -70.0, 0.0]);
            f.add("rightLowerArm", [0.0, 70.0, 0.0]);
            f.hips[1] = -0.07 * b;
            f.hips[0] = 0.02 * s(w);
        }
        Motion::Laugh => {
            let shake = (8.0 * w).sin();
            let k = hump(t, 0.1, 0.9);
            f.add("spine", [-6.0 * k + 5.0 * k * shake, 0.0, 0.0]);
            f.add("chest", [-8.0 * k + 4.0 * k * shake, 0.0, 0.0]);
            f.add("head", [-14.0 * k + 3.0 * k * shake, 0.0, 0.0]);
            f.add("leftShoulder", [0.0, 0.0, 6.0 * k * shake]);
            f.add("rightShoulder", [0.0, 0.0, -6.0 * k * shake]);
            f.add("leftUpperArm", [-30.0 * k, 0.0, 20.0 * k]);
            f.add("leftLowerArm", [0.0, -95.0 * k, 0.0]);
            f.add("rightUpperArm", [-10.0 * k, 0.0, -10.0 * k]);
            f.add("rightLowerArm", [0.0, 20.0 * k, 0.0]);
            f.hips[1] = 0.01 * k * shake;
        }
    }
    f
}

/// The legs of a walk or a run: they swing opposite to each other, and a leg bends its knee while it comes forward.
fn walk_legs(f: &mut Frame, w: f32, swing: f32, knee: f32) {
    for (side, phase) in [("left", 0.0f32), ("right", std::f32::consts::PI)] {
        let a = w + phase;
        let forward = -a.sin(); // negative x is forward: the leg is forward when sin is negative
        let (up, low, foot) = match side {
            "left" => ("leftUpperLeg", "leftLowerLeg", "leftFoot"),
            _ => ("rightUpperLeg", "rightLowerLeg", "rightFoot"),
        };
        f.add(up, [swing * -forward, 0.0, 0.0]);
        // the knee bends most while the leg swings through (when it goes from back to front)
        f.add(low, [knee * (-a.cos()).max(0.0).powf(1.2) * 0.9 + 4.0, 0.0, 0.0]);
        f.add(foot, [-6.0 * forward, 0.0, 0.0]);
    }
}

// ---------------------------------------------------------------------------------------------- generating

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Mood {
    Neutral,
    Happy,
    Sad,
    Angry,
    Sleepy,
    Proud,
    Scared,
    Old,
}

pub const MOODS: [(Mood, &str, &[&str]); 8] = [
    (Mood::Neutral, "Neutral", &["neutral", "normal", "plain"]),
    (Mood::Happy, "Happy", &["happy", "joy", "joyful", "cheerful", "excited", "glad"]),
    (Mood::Sad, "Sad", &["sad", "unhappy", "depressed", "down", "sorrow", "gloomy"]),
    (Mood::Angry, "Angry", &["angry", "mad", "furious", "rage", "annoyed"]),
    (Mood::Sleepy, "Sleepy", &["sleepy", "tired", "exhausted", "lazy", "drowsy"]),
    (Mood::Proud, "Proud", &["proud", "confident", "brave", "heroic", "bold"]),
    (Mood::Scared, "Scared", &["scared", "afraid", "nervous", "fearful", "terrified", "shy"]),
    (Mood::Old, "Old", &["old", "elderly", "weak", "wounded", "injured", "limp"]),
];

/// How a motion is changed: how fast, how strong, in which mood, and a little chance.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Style {
    pub speed: f32,
    pub strength: f32,
    pub mood: Mood,
    /// 0 for none; any other number gives a different small change of the motion.
    pub seed: u32,
}

impl Default for Style {
    fn default() -> Self {
        Style { speed: 1.0, strength: 1.0, mood: Mood::Neutral, seed: 0 }
    }
}

/// How the mood changes the pose all the time (added to every frame, in the same turns as the motions) and how it changes the speed.
fn mood_frame(m: Mood) -> (Frame, f32) {
    let mut f = Frame::default();
    let speed = match m {
        Mood::Neutral => 1.0,
        Mood::Happy => {
            f.add("chest", [-4.0, 0.0, 0.0]);
            f.add("head", [-6.0, 0.0, 0.0]);
            f.add("leftUpperArm", [0.0, 0.0, 6.0]);
            f.add("rightUpperArm", [0.0, 0.0, -6.0]);
            f.hips[1] = 0.01;
            1.1
        }
        Mood::Sad => {
            f.add("spine", [12.0, 0.0, 0.0]);
            f.add("chest", [6.0, 0.0, 0.0]);
            f.add("head", [22.0, 0.0, 0.0]);
            f.add("leftShoulder", [0.0, 0.0, -6.0]);
            f.add("rightShoulder", [0.0, 0.0, 6.0]);
            f.add("leftUpperArm", [4.0, 0.0, -4.0]);
            f.add("rightUpperArm", [4.0, 0.0, 4.0]);
            0.8
        }
        Mood::Angry => {
            f.add("chest", [6.0, 0.0, 0.0]);
            f.add("head", [10.0, 0.0, 0.0]);
            f.add("leftShoulder", [0.0, 0.0, 8.0]);
            f.add("rightShoulder", [0.0, 0.0, -8.0]);
            f.add("leftUpperArm", [-8.0, 0.0, 8.0]);
            f.add("rightUpperArm", [-8.0, 0.0, -8.0]);
            f.add("leftLowerArm", [0.0, -26.0, 0.0]);
            f.add("rightLowerArm", [0.0, 26.0, 0.0]);
            1.15
        }
        Mood::Sleepy => {
            f.add("spine", [9.0, 0.0, 0.0]);
            f.add("head", [26.0, 0.0, 6.0]);
            f.add("leftShoulder", [0.0, 0.0, -5.0]);
            f.add("rightShoulder", [0.0, 0.0, 5.0]);
            f.hips[1] = -0.015;
            0.7
        }
        Mood::Proud => {
            f.add("spine", [-3.0, 0.0, 0.0]);
            f.add("chest", [-7.0, 0.0, 0.0]);
            f.add("head", [-6.0, 0.0, 0.0]);
            f.add("leftUpperArm", [0.0, 0.0, 8.0]);
            f.add("rightUpperArm", [0.0, 0.0, -8.0]);
            1.0
        }
        Mood::Scared => {
            f.add("spine", [8.0, 0.0, 0.0]);
            f.add("chest", [4.0, 0.0, 0.0]);
            f.add("head", [8.0, 0.0, 0.0]);
            f.add("leftShoulder", [0.0, 0.0, 10.0]);
            f.add("rightShoulder", [0.0, 0.0, -10.0]);
            f.add("leftUpperArm", [-22.0, 0.0, 12.0]);
            f.add("rightUpperArm", [-22.0, 0.0, -12.0]);
            f.add("leftLowerArm", [0.0, -50.0, 0.0]);
            f.add("rightLowerArm", [0.0, 50.0, 0.0]);
            f.add("leftUpperLeg", [-8.0, 0.0, 0.0]);
            f.add("rightUpperLeg", [-8.0, 0.0, 0.0]);
            f.add("leftLowerLeg", [14.0, 0.0, 0.0]);
            f.add("rightLowerLeg", [14.0, 0.0, 0.0]);
            f.hips[1] = -0.03;
            1.15
        }
        Mood::Old => {
            f.add("spine", [16.0, 0.0, 0.0]);
            f.add("chest", [8.0, 0.0, 0.0]);
            f.add("head", [14.0, 0.0, 0.0]);
            f.add("leftUpperLeg", [-8.0, 0.0, 0.0]);
            f.add("rightUpperLeg", [-8.0, 0.0, 0.0]);
            f.add("leftLowerLeg", [16.0, 0.0, 0.0]);
            f.add("rightLowerLeg", [16.0, 0.0, 0.0]);
            f.hips[1] = -0.04;
            0.65
        }
    };
    (f, speed)
}

/// A small steady number from a seed and a place (the same each time).
fn chance(seed: u32, k: u32) -> f32 {
    let mut x = seed.wrapping_mul(0x9E37_79B1).wrapping_add(k.wrapping_mul(0x85EB_CA6B)).wrapping_add(0x27D4_EB2F);
    x ^= x >> 15;
    x = x.wrapping_mul(0x2C1B_3C6D);
    x ^= x >> 12;
    x = x.wrapping_mul(0x297A_2D39);
    x ^= x >> 15;
    (x as f32 / u32::MAX as f32) * 2.0 - 1.0
}

/// A motion in a style, at the moment `t` (0 to 1, a loop joins).
pub fn eval_styled(m: Motion, style: &Style, t: f32) -> Frame {
    let base = Frame::rest();
    let raw = eval(m, t);
    let strength = style.strength.clamp(0.0, 2.0);
    // the motion's own turns (more or less of them), then the mood
    let mut f = Frame::default();
    for b in BONES {
        let (a, r) = (base.get(b), raw.get(b));
        let mut v = [a[0] + (r[0] - a[0]) * strength, a[1] + (r[1] - a[1]) * strength, a[2] + (r[2] - a[2]) * strength];
        if style.seed != 0 {
            // each bone is a little more or a little less lively, and a little turned
            let k = BONES.iter().position(|x| *x == b).unwrap_or(0) as u32;
            let amp = 1.0 + 0.18 * chance(style.seed, k);
            for c in 0..3 {
                v[c] = a[c] + (v[c] - a[c]) * amp + 1.4 * chance(style.seed, 100 + k * 3 + c as u32) * strength.min(1.0) * 0.5;
            }
        }
        if v != [0.0; 3] {
            f.rot.insert(b, v);
        }
    }
    f.hips = [raw.hips[0] * strength, raw.hips[1] * strength, raw.hips[2] * strength];
    let (mood, _) = mood_frame(style.mood);
    for (b, r) in &mood.rot {
        f.add(b, *r);
    }
    for k in 0..3 {
        f.hips[k] += mood.hips[k];
    }
    f
}

/// The seconds of one go in a style.
pub fn seconds_of(m: Motion, style: &Style) -> f32 {
    let (_, mood_speed) = mood_frame(style.mood);
    info_of(m).seconds / (style.speed.clamp(0.25, 3.0) * mood_speed)
}

/// An animation that is made: a motion in a style, with a name.
#[derive(Clone, Debug, PartialEq)]
pub struct Clip {
    pub name: String,
    pub motion: Motion,
    pub style: Style,
}

impl Clip {
    pub fn new(motion: Motion, style: Style) -> Clip {
        let mood = MOODS.iter().find(|m| m.0 == style.mood).map(|m| m.1).unwrap_or("");
        let name = if style.mood == Mood::Neutral { info_of(motion).name.to_string() } else { format!("{} ({})", info_of(motion).name, mood.to_lowercase()) };
        Clip { name, motion, style }
    }

    pub fn looping(&self) -> bool {
        info_of(self.motion).looping
    }

    pub fn seconds(&self) -> f32 {
        seconds_of(self.motion, &self.style)
    }

    /// The frame at a time in seconds (a loop goes on again, a single go stays at its last frame).
    pub fn at(&self, secs: f32) -> Frame {
        let d = self.seconds().max(0.05);
        let t = if self.looping() { (secs / d).rem_euclid(1.0) } else { (secs / d).clamp(0.0, 1.0) };
        eval_styled(self.motion, &self.style, t)
    }

    /// Every frame for a file: `fps` a second. A loop ends one frame before it would start again, so it joins without a stop.
    pub fn frames(&self, fps: f32) -> Vec<Frame> {
        let d = self.seconds();
        let n = (d * fps).round().max(2.0) as usize;
        let count = if self.looping() { n } else { n + 1 };
        (0..count).map(|i| self.at(i as f32 / fps)).collect()
    }
}

/// Which motion and style a few words describe ("a tired old man walks slowly", "angry fast punch", "happy dance"). The words that do not
/// mean anything are left out. Nothing found: None.
pub fn from_words(text: &str, seed: u32) -> Option<Clip> {
    let lower = text.to_lowercase();
    let mut words: Vec<&str> = Vec::new();
    for w in lower.split(|c: char| !c.is_alphanumeric()).filter(|w| !w.is_empty()) {
        // "walks", "walking" and "walked" mean "walk"
        words.push(w);
        for end in ["s", "es", "ing", "ed", "ly"] {
            if let Some(stem) = w.strip_suffix(end).filter(|s| s.len() >= 3) {
                words.push(stem);
            }
        }
    }
    let mut best: Option<(&Info, usize)> = None;
    for i in MOTIONS {
        let hits = i.words.iter().filter(|k| if k.contains(' ') { lower.contains(**k) } else { words.contains(k) }).count();
        // the first words of a longer description count more (the verb comes early)
        if hits > 0 && best.is_none_or(|(_, h)| hits > h) {
            best = Some((i, hits));
        }
    }
    let (info, _) = best?;
    let mut style = Style { seed, ..Default::default() };
    for (m, _, keys) in MOODS {
        if keys.iter().any(|k| words.contains(k)) {
            style.mood = m;
        }
    }
    let has = |ws: &[&str]| ws.iter().any(|w| words.contains(w));
    if has(&["fast", "quick", "quickly", "rapid", "hurry", "hurried", "brisk"]) {
        style.speed = 1.4;
    }
    if has(&["slow", "slowly", "calm", "gentle", "gently"]) {
        style.speed = 0.7;
    }
    if has(&["big", "huge", "exaggerated", "strong", "wild", "heavy", "dramatic", "large"]) {
        style.strength = 1.4;
    }
    if has(&["small", "subtle", "tiny", "little", "soft", "light", "slight"]) {
        style.strength = 0.6;
    }
    Some(Clip::new(info.motion, style))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn there_are_at_least_twenty_motions_and_each_has_what_it_needs() {
        assert!(MOTIONS.len() >= 20, "{} motions", MOTIONS.len());
        for i in MOTIONS {
            assert!(CATEGORIES.contains(&i.category), "{}", i.name);
            assert!(i.seconds > 0.3 && i.seconds < 10.0 && !i.about.is_empty() && !i.words.is_empty(), "{}", i.name);
        }
        let mut names: Vec<&str> = MOTIONS.iter().map(|i| i.name).collect();
        names.sort();
        names.dedup();
        assert_eq!(names.len(), MOTIONS.len(), "every motion has its own name");
    }

    #[test]
    fn every_loop_joins_and_every_single_motion_ends_where_it_began() {
        for i in MOTIONS {
            let (a, b) = (eval(i.motion, 0.0), eval(i.motion, 1.0));
            for bone in BONES {
                let (x, y) = (a.get(bone), b.get(bone));
                for k in 0..3 {
                    assert!((x[k] - y[k]).abs() < 0.6, "{}: {bone} {k}: {} against {}", i.name, x[k], y[k]);
                }
            }
            for k in 0..3 {
                assert!((a.hips[k] - b.hips[k]).abs() < 0.01, "{}: hips {k}", i.name);
            }
        }
    }

    #[test]
    fn nothing_turns_a_bone_too_far_or_moves_the_hips_too_far() {
        for i in MOTIONS {
            for step in 0..=60 {
                let f = eval(i.motion, step as f32 / 60.0);
                for (b, r) in &f.rot {
                    for k in 0..3 {
                        assert!(r[k].is_finite() && r[k].abs() <= 175.0, "{}: {b} {k} = {}", i.name, r[k]);
                    }
                }
                assert!(f.hips.iter().all(|v| v.is_finite() && v.abs() < 0.8), "{}: hips {:?}", i.name, f.hips);
            }
        }
    }

    #[test]
    fn the_arms_come_down_from_the_t_pose_in_every_motion_that_does_not_raise_them() {
        let f = eval(Motion::IdleBreathe, 0.0);
        assert!(f.get("leftUpperArm")[2] < -60.0 && f.get("rightUpperArm")[2] > 60.0);
        // a wave raises the right arm above the shoulder (a negative z)
        let w = eval(Motion::Wave, 0.5);
        assert!(w.get("rightUpperArm")[2] < -60.0, "{:?}", w.get("rightUpperArm"));
    }

    #[test]
    fn a_walk_swings_the_legs_against_each_other_and_the_arms_against_the_legs() {
        let f = eval(Motion::Walk, 0.25);
        let (l, r) = (f.get("leftUpperLeg")[0], f.get("rightUpperLeg")[0]);
        assert!(l * r < 0.0, "the legs go opposite ways ({l}, {r})");
        let (la, ra) = (f.get("leftUpperArm")[0], f.get("rightUpperArm")[0]);
        assert!(la * ra < 0.0, "the arms go opposite ways");
        assert!(la * r > 0.0 || la * l < 0.0, "the left arm goes with the right leg ({la}, {r})");
    }

    #[test]
    fn a_jump_leaves_the_ground_and_a_duck_goes_down() {
        let top = (0..=100).map(|i| eval(Motion::Jump, i as f32 / 100.0).hips[1]).fold(f32::MIN, f32::max);
        let low = (0..=100).map(|i| eval(Motion::Jump, i as f32 / 100.0).hips[1]).fold(f32::MAX, f32::min);
        assert!(top > 0.3 && low < -0.15, "up {top}, down {low}");
        assert!(eval(Motion::Duck, 0.5).hips[1] < -0.3);
    }

    #[test]
    fn the_quaternion_of_a_turn_turns_a_point_the_same_way_as_the_degrees() {
        // an arm along +x raised by 90 degrees about z points up
        let p = euler_apply([0.0, 0.0, 90.0], [1.0, 0.0, 0.0]);
        assert!((p[1] - 1.0).abs() < 1e-5 && p[0].abs() < 1e-5);
        // a hanging leg swung by a positive x goes backward
        let q = euler_apply([30.0, 0.0, 0.0], [0.0, -1.0, 0.0]);
        assert!(q[2] < -0.4, "{q:?}");
        // a head that leans forward by a positive x
        let h = euler_apply([30.0, 0.0, 0.0], [0.0, 1.0, 0.0]);
        assert!(h[2] > 0.4);
        // the quaternion has length one and w = cos of half the angle for a single turn
        let qq = euler_quat([0.0, 0.0, 60.0]);
        assert!((qq.iter().map(|v| v * v).sum::<f32>() - 1.0).abs() < 1e-5 && (qq[3] - 30f32.to_radians().cos()).abs() < 1e-5);
        // x after z after y: a hanging arm (z = -90 from +x points down) swung forward by x -90 points forward
        let arm = euler_apply([-90.0, 0.0, -90.0], [1.0, 0.0, 0.0]);
        assert!(arm[2] > 0.9, "{arm:?}");
    }

    #[test]
    fn a_mood_and_a_strength_and_a_speed_change_the_motion() {
        let calm = Clip::new(Motion::Walk, Style::default());
        let sad = Clip::new(Motion::Walk, Style { mood: Mood::Sad, ..Default::default() });
        assert!(sad.seconds() > calm.seconds(), "a sad walk is slower");
        assert!(sad.at(0.1).get("head")[0] > calm.at(0.1).get("head")[0] + 15.0, "the head hangs");
        let big = Clip::new(Motion::Punch, Style { strength: 1.5, ..Default::default() });
        let small = Clip::new(Motion::Punch, Style { strength: 0.5, ..Default::default() });
        assert!(big.at(0.5).get("rightUpperArm")[0].abs() > small.at(0.5).get("rightUpperArm")[0].abs());
        let fast = Clip::new(Motion::Run, Style { speed: 2.0, ..Default::default() });
        assert!((fast.seconds() - 0.35).abs() < 0.01);
        // a seed makes another small change, the same one each time
        let a = Clip::new(Motion::Walk, Style { seed: 5, ..Default::default() });
        let b = Clip::new(Motion::Walk, Style { seed: 5, ..Default::default() });
        let c = Clip::new(Motion::Walk, Style { seed: 6, ..Default::default() });
        assert_eq!(a.at(0.3), b.at(0.3));
        assert_ne!(a.at(0.3), c.at(0.3));
    }

    #[test]
    fn words_find_the_motion_and_the_style() {
        let c = from_words("a tired old man walks slowly", 0).unwrap();
        assert_eq!(c.motion, Motion::Walk);
        assert!(c.style.speed < 1.0);
        assert!(matches!(c.style.mood, Mood::Sleepy | Mood::Old), "{:?}", c.style.mood);
        let p = from_words("Angry, fast, strong PUNCH!", 3).unwrap();
        assert_eq!((p.motion, p.style.mood, p.style.seed), (Motion::Punch, Mood::Angry, 3));
        assert!(p.style.speed > 1.0 && p.style.strength > 1.0);
        assert_eq!(from_words("happy dance", 0).unwrap().motion, Motion::DanceSway);
        assert!(from_words("qwerty zxcv", 0).is_none());
        assert_eq!(from_words("wave hello to everybody", 0).unwrap().motion, Motion::Wave);
    }

    #[test]
    fn the_frames_of_a_file_cover_the_clip() {
        let walk = Clip::new(Motion::Walk, Style::default());
        assert_eq!(walk.frames(30.0).len(), 30, "a loop leaves out the frame that is the first again");
        let jump = Clip::new(Motion::Jump, Style::default());
        let n = (1.1f32 * 30.0).round() as usize + 1;
        assert_eq!(jump.frames(30.0).len(), n);
        let fr = jump.frames(30.0);
        assert_eq!(fr.first().unwrap().get("hips"), fr.last().unwrap().get("hips"));
    }
}
