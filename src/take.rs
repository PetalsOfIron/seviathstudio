// SPDX-License-Identifier: GPL-3.0-or-later
// An animation made by hand: the poses of the bones are set at some frames (keys) and the frames between are worked out, a straight line from key to key.
// A take is played in the 3D tab and saved in the game files next to the ready animations. Nothing here knows about the screen.

use std::collections::BTreeMap;

use crate::rig3d::{Pose, Rig3};

/// An animation: what every bone and the whole figure do, at which frame.
#[derive(Clone, Debug, PartialEq)]
pub struct Take {
    pub name: String,
    pub fps: f32,
    /// How many frames it lasts (the last frame is `frames`, the first is 0).
    pub frames: usize,
    pub looping: bool,
    /// For every bone: the turn (x, y, z in degrees) at the frames that have a key.
    pub keys: Vec<BTreeMap<usize, [f32; 3]>>,
    /// The move of the whole figure (metres) at the frames that have a key.
    pub moves: BTreeMap<usize, [f32; 3]>,
}

/// A whole animation as poses, one for every frame: what a file is made from.
#[derive(Clone, Debug, PartialEq)]
pub struct Anim {
    pub name: String,
    pub fps: f32,
    pub poses: Vec<Pose>,
}

impl Take {
    pub fn new(name: &str, bones: usize) -> Take {
        Take { name: name.to_owned(), fps: 30.0, frames: 30, looping: false, keys: vec![BTreeMap::new(); bones], moves: BTreeMap::new() }
    }

    /// The bones can change in number (a bone was added or taken away): the keys of the new bones are empty.
    pub fn fit(&mut self, bones: usize) {
        self.keys.resize(bones, BTreeMap::new());
    }

    pub fn seconds(&self) -> f32 {
        self.frames as f32 / self.fps.max(1.0)
    }

    pub fn set_key(&mut self, bone: usize, frame: usize, rot: [f32; 3]) {
        if let Some(k) = self.keys.get_mut(bone) {
            k.insert(frame.min(self.frames), rot);
        }
    }

    pub fn set_move(&mut self, frame: usize, by: [f32; 3]) {
        self.moves.insert(frame.min(self.frames), by);
    }

    pub fn remove_keys_at(&mut self, frame: usize) {
        for k in &mut self.keys {
            k.remove(&frame);
        }
        self.moves.remove(&frame);
    }

    pub fn has_key(&self, bone: usize, frame: usize) -> bool {
        self.keys.get(bone).is_some_and(|k| k.contains_key(&frame))
    }

    /// Does any bone or the figure have a key at this frame.
    pub fn any_key_at(&self, frame: usize) -> bool {
        self.moves.contains_key(&frame) || self.keys.iter().any(|k| k.contains_key(&frame))
    }

    pub fn key_count(&self) -> usize {
        self.keys.iter().map(|k| k.len()).sum::<usize>() + self.moves.len()
    }

    fn at<const N: usize>(track: &BTreeMap<usize, [f32; N]>, frame: f32) -> Option<[f32; N]> {
        let before = track.range(..=frame.floor() as usize).next_back();
        let after = track.range((frame.floor() as usize + 1)..).next();
        match (before, after) {
            (None, None) => None,
            (Some((_, v)), None) | (None, Some((_, v))) => Some(*v),
            (Some((a, va)), Some((b, vb))) => {
                let t = ((frame - *a as f32) / (*b - *a) as f32).clamp(0.0, 1.0);
                let mut out = [0.0; N];
                for k in 0..N {
                    out[k] = va[k] + (vb[k] - va[k]) * t;
                }
                Some(out)
            }
        }
    }

    /// The pose at a frame (a place between two frames works too). A bone with no key stands still.
    pub fn pose_at(&self, frame: f32) -> Pose {
        let frame = frame.clamp(0.0, self.frames as f32);
        Pose { rot: self.keys.iter().map(|k| Self::at(k, frame).unwrap_or([0.0; 3])).collect(), offset: Self::at(&self.moves, frame).unwrap_or([0.0; 3]) }
    }

    /// Every frame as a pose, for a file.
    pub fn bake(&self) -> Anim {
        Anim { name: self.name.clone(), fps: self.fps, poses: (0..=self.frames).map(|f| self.pose_at(f as f32)).collect() }
    }

    /// The bones were taken away or changed: keep the keys of the bones that are still there, by their names. `old` has the names the keys were made for.
    pub fn remap(&mut self, old: &[String], new: &Rig3) {
        let mut keys = vec![BTreeMap::new(); new.bones.len()];
        for (i, b) in new.bones.iter().enumerate() {
            if let Some(j) = old.iter().position(|n| *n == b.name) {
                if let Some(k) = self.keys.get(j) {
                    keys[i] = k.clone();
                }
            }
        }
        self.keys = keys;
    }
}

/// The ready animations of a rig as poses (the bones need the names of a person to move).
pub fn anims_from_clips(rig: &Rig3, clips: &[crate::motion::Clip], fps: f32) -> Vec<Anim> {
    let relative = crate::meshfile::arms_hang_down(rig);
    clips.iter().map(|c| Anim { name: c.name.clone(), fps, poses: c.frames(fps).iter().map(|f| Pose::from_frame(rig, f, relative)).collect() }).filter(|a| !a.poses.is_empty()).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_frames_between_two_keys_are_a_straight_line() {
        let mut t = Take::new("wave", 2);
        t.set_key(1, 0, [0.0, 0.0, 0.0]);
        t.set_key(1, 20, [0.0, 0.0, 80.0]);
        assert_eq!(t.pose_at(10.0).rot[1], [0.0, 0.0, 40.0]);
        assert_eq!(t.pose_at(0.0).rot[1], [0.0; 3]);
        assert_eq!(t.pose_at(25.0).rot[1], [0.0, 0.0, 80.0], "after the last key it holds");
        assert_eq!(t.pose_at(5.0).rot[0], [0.0; 3], "a bone with no key stands still");
    }

    #[test]
    fn keys_can_be_taken_away_counted_and_the_move_works() {
        let mut t = Take::new("hop", 1);
        t.set_key(0, 3, [10.0, 0.0, 0.0]);
        t.set_move(3, [0.0, 0.2, 0.0]);
        assert_eq!(t.key_count(), 2);
        assert!(t.has_key(0, 3) && t.any_key_at(3) && !t.any_key_at(4));
        assert_eq!(t.pose_at(3.0).offset, [0.0, 0.2, 0.0]);
        t.remove_keys_at(3);
        assert_eq!(t.key_count(), 0);
        t.set_key(0, 999, [1.0, 0.0, 0.0]);
        assert!(t.has_key(0, t.frames), "a key past the end is put at the end");
    }

    #[test]
    fn a_take_becomes_one_pose_for_every_frame() {
        let mut t = Take::new("x", 1);
        t.frames = 10;
        t.set_key(0, 0, [0.0; 3]);
        t.set_key(0, 10, [0.0, 90.0, 0.0]);
        let a = t.bake();
        assert_eq!(a.poses.len(), 11);
        assert_eq!(a.poses[5].rot[0], [0.0, 45.0, 0.0]);
    }

    #[test]
    fn the_keys_follow_the_bones_by_name_when_the_bones_change() {
        let mut rig = Rig3::new(0);
        rig.add_bone("a", None, [0.0; 3], [0.0, 1.0, 0.0]);
        rig.add_bone("b", Some(0), [0.0, 1.0, 0.0], [0.0, 2.0, 0.0]);
        let mut t = Take::new("t", 2);
        t.set_key(1, 5, [9.0, 0.0, 0.0]);
        let old = vec!["a".to_string(), "b".to_string()];
        rig.delete_bone(0);
        t.remap(&old, &rig);
        assert_eq!(t.keys.len(), 1);
        assert!(t.has_key(0, 5), "the key of b is the key of bone 0 now");
    }

    #[test]
    fn the_ready_animations_become_poses_for_a_rig_with_the_names_of_a_person() {
        let rig = Rig3::humanoid(0, [0.0; 3], [1.0, 2.0, 1.0]);
        let clips = vec![crate::motion::Clip::new(crate::motion::Motion::Walk, crate::motion::Style::default())];
        let a = anims_from_clips(&rig, &clips, 30.0);
        assert_eq!(a.len(), 1);
        assert!(a[0].poses.len() > 10 && a[0].poses[0].rot.len() == rig.bones.len());
        assert!(a[0].poses.iter().any(|p| p.rot.iter().any(|r| *r != [0.0; 3])));
    }
}
