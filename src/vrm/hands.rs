// SPDX-License-Identifier: GPL-3.0-or-later
// Fingers for the skeleton of a figure: a thumb of three bones and four fingers of three, as in a normal rig (the
// reference rig of the user: palm, then three bones per finger). They are put along the hand from the wrist to the
// fingertip, and the weight a hand has is shared out between the palm and the finger bones by where a point lies on
// the hand. Positions are in the model's space (metres, Y up) but only the x and y are used.

use super::model::Bone;

/// The VRM 1.0 names of the finger bones of one side, in the order they are made: (name, parent).
pub fn finger_names(left: bool) -> Vec<(String, String)> {
    let side = if left { "left" } else { "right" };
    let mut v = Vec::new();
    for (finger, segs) in [("Thumb", ["Metacarpal", "Proximal", "Distal"]), ("Index", ["Proximal", "Intermediate", "Distal"]), ("Middle", ["Proximal", "Intermediate", "Distal"]), ("Ring", ["Proximal", "Intermediate", "Distal"]), ("Little", ["Proximal", "Intermediate", "Distal"])] {
        for (i, s) in segs.iter().enumerate() {
            let parent = if i == 0 { format!("{side}Hand") } else { format!("{side}{finger}{}", segs[i - 1]) };
            v.push((format!("{side}{finger}{s}"), parent));
        }
    }
    v
}

fn norm(v: [f32; 2]) -> [f32; 2] {
    let l = v[0].hypot(v[1]);
    if l < 1e-9 { [1.0, 0.0] } else { [v[0] / l, v[1] / l] }
}

/// The frame of a hand: the way it points and which side the thumb is on.
pub struct Frame {
    pub wrist: [f32; 3],
    pub dir: [f32; 2],
    pub len: f32,
    /// A unit vector across the hand, towards the thumb.
    pub thumb: [f32; 2],
}

pub fn frame(wrist: [f32; 3], tip: [f32; 3], left: bool) -> Frame {
    let d = norm([tip[0] - wrist[0], tip[1] - wrist[1]]);
    let len = (tip[0] - wrist[0]).hypot(tip[1] - wrist[1]).max(1e-4);
    let n = [-d[1], d[0]];
    // a hand held out has the thumb up; a hand hanging down has it towards the body
    let sgn = if left { 1.0 } else { -1.0 };
    let pref = if d[1].abs() > d[0].abs() { [-sgn, 0.0] } else { [0.0, 1.0] };
    let thumb = if n[0] * pref[0] + n[1] * pref[1] >= 0.0 { n } else { [-n[0], -n[1]] };
    Frame { wrist, dir: d, len, thumb }
}

/// Where along the hand the palm ends and the fingers begin, as a share of the length.
pub const PALM: f32 = 0.42;
const FINGERS: [(f32, f32); 4] = [(0.33, 0.95), (0.11, 1.0), (-0.11, 0.95), (-0.33, 0.8)];

impl Frame {
    fn at(&self, along: f32, across: f32) -> [f32; 3] {
        [self.wrist[0] + self.dir[0] * along * self.len + self.thumb[0] * across * self.len, self.wrist[1] + self.dir[1] * along * self.len + self.thumb[1] * across * self.len, self.wrist[2]]
    }

    /// The finger bones: each with its head position and the place where its piece ends.
    pub fn bones(&self, left: bool) -> Vec<Bone> {
        let names = finger_names(left);
        let hand = if left { "leftHand" } else { "rightHand" };
        let mut out: Vec<Bone> = Vec::new();
        let width = 0.38;
        // the thumb: from near the wrist, out to the thumb side and forward
        let tdir = norm([self.dir[0] * 0.75 + self.thumb[0] * 0.45, self.dir[1] * 0.75 + self.thumb[1] * 0.45]);
        let tbase = self.at(0.08, 0.40 * width);
        let mut p = tbase;
        for (i, share) in [0.15f32, 0.12, 0.11].iter().enumerate() {
            let q = [p[0] + tdir[0] * share * self.len, p[1] + tdir[1] * share * self.len, p[2]];
            out.push(Bone { name: names[i].0.clone(), humanoid: true, parent: None, pos: p, tip: q });
            p = q;
        }
        // the four fingers
        for (f, (lateral, scale)) in FINGERS.iter().enumerate() {
            let base = self.at(PALM, lateral * width);
            let mut p = base;
            for (i, share) in [0.40f32, 0.33, 0.27].iter().enumerate() {
                let l = (1.0 - PALM) * share * scale;
                let q = [p[0] + self.dir[0] * l * self.len, p[1] + self.dir[1] * l * self.len, p[2]];
                out.push(Bone { name: names[3 + f * 3 + i].0.clone(), humanoid: true, parent: None, pos: p, tip: q });
                p = q;
            }
        }
        // the parents are set by the caller from the names; keep the hand's name for it
        let _ = hand;
        out
    }

    /// Share a hand's weight between the palm and the finger bones. `p` is the point; the answer is (bone name, share)
    /// pairs adding up to 1, the palm being the hand bone itself.
    pub fn split(&self, p: [f32; 2], left: bool) -> Vec<(String, f32)> {
        let hand = if left { "leftHand".to_string() } else { "rightHand".to_string() };
        let (rx, ry) = (p[0] - self.wrist[0], p[1] - self.wrist[1]);
        let s = (rx * self.dir[0] + ry * self.dir[1]) / self.len;
        let u = (rx * self.thumb[0] + ry * self.thumb[1]) / self.len;
        let smooth = |t: f32| {
            let t = t.clamp(0.0, 1.0);
            t * t * (3.0 - 2.0 * t)
        };
        let names = finger_names(left);
        let width = 0.38;
        // fingers: from the end of the palm out
        let fingers_w = smooth((s - (PALM - 0.05)) / 0.10);
        // the thumb: out to the thumb side, from the wrist
        let thumb_w = smooth((u - 0.30 * width) / 0.07) * smooth((s - 0.02) / 0.08) * (1.0 - fingers_w * 0.6);
        let mut out: Vec<(String, f32)> = Vec::new();
        let mut total = 0.0;
        if thumb_w > 0.0 {
            let seg = ((s - 0.08) / 0.38).clamp(0.0, 0.999);
            let k = (seg * 3.0) as usize;
            out.push((names[k].0.clone(), thumb_w));
            total += thumb_w;
        }
        let rest = (1.0 - thumb_w).max(0.0);
        if fingers_w > 0.0 && rest > 0.0 {
            // which finger: the nearest across the hand, softly
            let mut ws: Vec<f32> = FINGERS.iter().map(|(lat, _)| (-((u - lat * width) / 0.075).powi(2)).exp()).collect();
            let sum: f32 = ws.iter().sum::<f32>().max(1e-6);
            for w in &mut ws {
                *w /= sum;
            }
            let seg = ((s - PALM) / (1.0 - PALM)).clamp(0.0, 0.999);
            let (k, frac) = if seg < 0.40 { (0, seg / 0.40) } else if seg < 0.73 { (1, (seg - 0.40) / 0.33) } else { (2, (seg - 0.73) / 0.27) };
            let _ = frac;
            for (f, w) in ws.iter().enumerate() {
                let share = rest * fingers_w * w;
                if share > 0.002 {
                    out.push((names[3 + f * 3 + k].0.clone(), share));
                    total += share;
                }
            }
        }
        out.push((hand, (1.0 - total).max(0.0)));
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_hand_gets_five_fingers_of_three_bones_in_a_row() {
        let names = finger_names(true);
        assert_eq!(names.len(), 15);
        assert_eq!(names[0], ("leftThumbMetacarpal".to_string(), "leftHand".to_string()));
        assert_eq!(names[1].1, "leftThumbMetacarpal");
        assert_eq!(names[3], ("leftIndexProximal".to_string(), "leftHand".to_string()));
        assert_eq!(names[14].0, "leftLittleDistal");
        // a hand pointing out to the right (a left arm in a T pose): the fingers run out along it, the thumb is up
        let f = frame([0.6, 1.5, 0.0], [0.7, 1.5, 0.0], true);
        assert!(f.dir[0] > 0.99 && f.thumb[1] > 0.99, "{:?} {:?}", f.dir, f.thumb);
        let bones = f.bones(true);
        assert_eq!(bones.len(), 15);
        // the fingers end near the fingertip and none goes past it
        let farthest = bones.iter().map(|b| b.tip[0]).fold(0.0f32, f32::max);
        assert!(farthest > 0.69 && farthest < 0.705, "{farthest}");
        // index above middle above ring above little, on the thumb side first
        let y = |n: &str| bones.iter().find(|b| b.name == n).unwrap().pos[1];
        assert!(y("leftIndexProximal") > y("leftMiddleProximal") && y("leftMiddleProximal") > y("leftRingProximal") && y("leftRingProximal") > y("leftLittleProximal"));
        // a hand hanging down has the thumb towards the body: for the left arm that is -x
        let g = frame([0.8, 1.0, 0.0], [0.8, 0.9, 0.0], true);
        assert!(g.thumb[0] < -0.99, "{:?}", g.thumb);
        let h = frame([-0.8, 1.0, 0.0], [-0.8, 0.9, 0.0], false);
        assert!(h.thumb[0] > 0.99, "{:?}", h.thumb);
    }

    #[test]
    fn the_weight_of_a_hand_is_shared_between_the_palm_and_the_fingers() {
        let f = frame([0.0, 0.0, 0.0], [0.1, 0.0, 0.0], true);
        let sum = |w: &[(String, f32)]| w.iter().map(|x| x.1).sum::<f32>();
        // the palm: only the hand bone
        let palm = f.split([0.02, 0.0], true);
        assert_eq!(palm.len(), 1);
        assert_eq!(palm[0].0, "leftHand");
        // the end of the middle of the hand: mostly the fingers, the middle finger above the ring finger
        let tip = f.split([0.095, 0.0], true);
        assert!((sum(&tip) - 1.0).abs() < 1e-4);
        assert!(tip.iter().any(|w| w.0.ends_with("Distal") && w.1 > 0.1), "{tip:?}");
        let up = f.split([0.08, 0.0046], true);
        let strongest = up.iter().max_by(|a, b| a.1.total_cmp(&b.1)).unwrap();
        assert!(strongest.0.starts_with("leftMiddle"), "{strongest:?}");
        let low = f.split([0.08, -0.0046], true);
        assert!(low.iter().max_by(|a, b| a.1.total_cmp(&b.1)).unwrap().0.starts_with("leftRing"));
        // by the thumb side near the wrist: the thumb
        let thumb = f.split([0.04, 0.1 * 0.38 * 0.45], true);
        assert!(thumb.iter().any(|w| w.0.starts_with("leftThumb") && w.1 > 0.3), "{thumb:?}");
        for p in [[0.0, 0.0], [0.05, 0.03], [0.1, -0.03], [0.07, 0.0]] {
            assert!((sum(&f.split(p, true)) - 1.0).abs() < 1e-4);
        }
    }
}
