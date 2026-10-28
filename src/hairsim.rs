// SPDX-License-Identifier: GPL-3.0-or-later
// Hair that moves like hair. A strand of hair drawn with the hair brush has a chain of bones. This works out how such a chain swings when the head moves: the first
// joint is held by the head, the others hang and swing (gravity, a spring back to where they would be if the chain were stiff, damping, and the length of every
// piece kept), and the positions of the joints are turned into the turns of the bones (the swing of each bone from where it points at rest), so the skinned strand
// follows. It is run live in the preview, and it can be run over a whole animation to put the hair's motion in the keys of the file (a .glb or an .fbx cannot carry
// hair physics, but it can carry keys).

use crate::rig3d::{Pose, Rig3};
use crate::take::Anim;

/// How the hair behaves.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct HairParams {
    /// How strongly a joint is pulled back to where it would be if the chain were stiff (0 hangs free, 1 is stiff).
    pub stiffness: f32,
    /// How much of the speed is kept every step (1 never stops, 0.8 stops soon).
    pub damping: f32,
    /// How hard the hair falls (metres a second, a second).
    pub gravity: f32,
}

impl Default for HairParams {
    fn default() -> Self {
        HairParams { stiffness: 0.12, damping: 0.9, gravity: 9.8 }
    }
}

/// The bones of a strand, from the root to the tip.
#[derive(Clone, Debug, PartialEq)]
pub struct Chain {
    pub bones: Vec<usize>,
}

/// Bones whose name starts with `hair` are hair; a chain starts at a hair bone whose parent is not a hair bone and goes down (the first child) while the next one is a
/// hair bone too.
pub fn chains(rig: &Rig3) -> Vec<Chain> {
    let is_hair = |i: usize| rig.bones[i].name.to_lowercase().starts_with("hair");
    let mut out = Vec::new();
    for i in 0..rig.bones.len() {
        if !is_hair(i) || rig.bones[i].parent.is_some_and(&is_hair) {
            continue;
        }
        let mut bones = vec![i];
        let mut at = i;
        while let Some(next) = rig.children(at).into_iter().find(|c| is_hair(*c)) {
            bones.push(next);
            at = next;
            if bones.len() > 64 {
                break;
            }
        }
        out.push(Chain { bones });
    }
    out
}

type M3 = [[f32; 3]; 3];

fn mul(a: &M3, b: &M3) -> M3 {
    let mut m = [[0.0; 3]; 3];
    for i in 0..3 {
        for j in 0..3 {
            m[i][j] = (0..3).map(|k| a[i][k] * b[k][j]).sum();
        }
    }
    m
}

fn transpose(a: &M3) -> M3 {
    let mut m = [[0.0; 3]; 3];
    for i in 0..3 {
        for j in 0..3 {
            m[i][j] = a[j][i];
        }
    }
    m
}

fn apply(m: &M3, v: [f32; 3]) -> [f32; 3] {
    [m[0][0] * v[0] + m[0][1] * v[1] + m[0][2] * v[2], m[1][0] * v[0] + m[1][1] * v[1] + m[1][2] * v[2], m[2][0] * v[0] + m[2][1] * v[1] + m[2][2] * v[2]]
}

fn unit(v: [f32; 3]) -> [f32; 3] {
    let l = (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt().max(1e-9);
    [v[0] / l, v[1] / l, v[2] / l]
}

fn sub(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}

fn add(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [a[0] + b[0], a[1] + b[1], a[2] + b[2]]
}

fn scale(a: [f32; 3], k: f32) -> [f32; 3] {
    [a[0] * k, a[1] * k, a[2] * k]
}

fn len(a: [f32; 3]) -> f32 {
    (a[0] * a[0] + a[1] * a[1] + a[2] * a[2]).sqrt()
}

/// The shortest turn that takes the direction `a` to the direction `b` (both of length 1), as a matrix.
fn swing(a: [f32; 3], b: [f32; 3]) -> M3 {
    let c = a[0] * b[0] + a[1] * b[1] + a[2] * b[2];
    let v = [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]];
    if c < -0.9999 {
        // turned right round: any axis across `a`
        let axis = if a[0].abs() < 0.9 { unit([0.0, -a[2], a[1]]) } else { unit([a[2], 0.0, -a[0]]) };
        let (x, y, z) = (axis[0], axis[1], axis[2]);
        return [[2.0 * x * x - 1.0, 2.0 * x * y, 2.0 * x * z], [2.0 * x * y, 2.0 * y * y - 1.0, 2.0 * y * z], [2.0 * x * z, 2.0 * y * z, 2.0 * z * z - 1.0]];
    }
    let k = 1.0 / (1.0 + c);
    [[1.0 - k * (v[1] * v[1] + v[2] * v[2]), -v[2] + k * v[0] * v[1], v[1] + k * v[0] * v[2]], [v[2] + k * v[0] * v[1], 1.0 - k * (v[0] * v[0] + v[2] * v[2]), -v[0] + k * v[1] * v[2]], [-v[1] + k * v[0] * v[2], v[0] + k * v[1] * v[2], 1.0 - k * (v[0] * v[0] + v[1] * v[1])]]
}

/// The turns (x, y, z in degrees, in the order the animations use: y, then z, then x) that make a matrix.
fn euler_of(m: &M3) -> [f32; 3] {
    let c = (-m[0][1]).clamp(-1.0, 1.0).asin();
    let (a, b) = if c.cos().abs() > 1e-4 { (m[2][1].atan2(m[1][1]), m[0][2].atan2(m[0][0])) } else { (0.0, (-m[2][0]).atan2(m[2][2])) };
    [a.to_degrees(), b.to_degrees(), c.to_degrees()]
}

/// The state of the hair: where the joints of every chain are, and where they were.
#[derive(Clone, Debug, Default)]
pub struct HairSim {
    chains: Vec<Chain>,
    /// For every chain the joints: the head of every bone and the tail of the last one.
    now: Vec<Vec<[f32; 3]>>,
    before: Vec<Vec<[f32; 3]>>,
    started: bool,
}

impl HairSim {
    pub fn new(rig: &Rig3) -> HairSim {
        HairSim { chains: chains(rig), ..Default::default() }
    }

    pub fn is_empty(&self) -> bool {
        self.chains.is_empty()
    }

    /// The hair is put back where it hangs when the head is still (the next step starts from there).
    pub fn reset(&mut self) {
        self.started = false;
    }

    /// One step of `dt` seconds. `pose` is the pose of the animation without any hair motion; the turns of the hair bones are put in it.
    pub fn step(&mut self, rig: &Rig3, pose: &mut Pose, params: &HairParams, dt: f32) {
        if self.chains.is_empty() {
            return;
        }
        let n = rig.bones.len();
        if pose.rot.len() < n {
            pose.rot.resize(n, [0.0; 3]);
        }
        for c in &self.chains {
            for b in &c.bones {
                pose.rot[*b] = [0.0; 3];
            }
        }
        // where the joints would be if the hair were stiff (it turns only with what it hangs from)
        let stiff = rig.posed_bones(pose);
        let rigid: Vec<Vec<[f32; 3]>> = self.chains.iter().map(|c| c.bones.iter().map(|b| stiff[*b].0).chain(c.bones.last().map(|b| stiff[*b].1)).collect()).collect();
        if !self.started || self.now.len() != self.chains.len() || self.now.iter().zip(&rigid).any(|(a, b)| a.len() != b.len()) {
            self.now = rigid.clone();
            self.before = rigid.clone();
            self.started = true;
        }
        let dt = dt.clamp(1.0 / 240.0, 1.0 / 20.0);
        for (ci, joints) in rigid.iter().enumerate() {
            let rest: Vec<f32> = rig_lengths(rig, &self.chains[ci]);
            let (now, before) = (&mut self.now[ci], &mut self.before[ci]);
            let old = now.clone();
            // the root is held by the head
            now[0] = joints[0];
            for i in 1..now.len() {
                let velocity = scale(sub(old[i], before[i]), params.damping);
                let mut p = add(add(old[i], velocity), [0.0, -params.gravity * dt * dt, 0.0]);
                // a spring back to where it would be if the hair were stiff
                p = add(p, scale(sub(joints[i], p), params.stiffness.clamp(0.0, 1.0)));
                now[i] = p;
            }
            // every piece keeps its length
            for _ in 0..4 {
                for i in 1..now.len() {
                    let d = sub(now[i], now[i - 1]);
                    let l = len(d).max(1e-9);
                    now[i] = add(now[i - 1], scale(d, rest[i - 1] / l));
                }
            }
            *before = old;
        }
        // the turns of the bones: each swings from where it points at rest to where the next joint is now
        for (ci, chain) in self.chains.iter().enumerate() {
            let mut parent_rot: M3 = match rig.bones[chain.bones[0]].parent {
                Some(p) => rig.world_rotation(pose, p),
                None => [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]],
            };
            for (k, b) in chain.bones.iter().enumerate() {
                let bone = &rig.bones[*b];
                let rest_dir = unit(sub(bone.tail, bone.head));
                let want = unit(sub(self.now[ci][k + 1], self.now[ci][k]));
                // the way it has to point in the frame of what it hangs from
                let local = apply(&transpose(&parent_rot), want);
                let r = swing(rest_dir, unit(local));
                pose.rot[*b] = euler_of(&r);
                parent_rot = mul(&parent_rot, &r);
            }
        }
    }
}

fn rig_lengths(rig: &Rig3, c: &Chain) -> Vec<f32> {
    c.bones.iter().map(|b| len(sub(rig.bones[*b].tail, rig.bones[*b].head)).max(1e-4)).collect()
}

/// The hair of a whole animation: the motion of the hair is worked out frame by frame and put in the poses (the animation is run twice so the hair is already
/// moving when it starts, which matters for an animation that loops).
pub fn bake(rig: &Rig3, anims: &mut [Anim], params: &HairParams) {
    for a in anims.iter_mut() {
        if a.poses.is_empty() {
            continue;
        }
        let mut sim = HairSim::new(rig);
        if sim.is_empty() {
            return;
        }
        let dt = 1.0 / a.fps.max(1.0);
        for pass in 0..2 {
            for p in a.poses.iter_mut() {
                // two steps for every frame: the hair is steadier
                let mut q = p.clone();
                sim.step(rig, &mut q, params, dt / 2.0);
                sim.step(rig, &mut q, params, dt / 2.0);
                if pass == 1 {
                    *p = q;
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A head bone and a strand of four hair bones that hang down from it.
    fn rig() -> Rig3 {
        let mut r = Rig3::new(0);
        let head = r.add_bone("head", None, [0.0, 1.7, 0.0], [0.0, 1.9, 0.0]);
        let mut parent = head;
        for i in 0..4 {
            let y = 1.9 - i as f32 * 0.1;
            parent = r.add_bone(&format!("hair_0_{i}"), Some(parent), [0.0, y, 0.0], [0.0, y - 0.1, 0.0]);
        }
        r
    }

    #[test]
    fn the_hair_bones_are_found_as_chains() {
        let mut r = rig();
        let c = chains(&r);
        assert_eq!(c.len(), 1);
        assert_eq!(c[0].bones, vec![1, 2, 3, 4]);
        // a second strand on another bone, and a bone that is not hair
        let h2 = r.add_bone("hair_1_0", Some(0), [0.1, 1.9, 0.0], [0.1, 1.8, 0.0]);
        r.add_bone("hips", None, [0.0, 1.0, 0.0], [0.0, 1.1, 0.0]);
        let c = chains(&r);
        assert_eq!(c.len(), 2);
        assert_eq!(c[1].bones, vec![h2]);
    }

    #[test]
    fn the_turns_put_the_bones_where_the_joints_are() {
        // the strand hangs from a head that has turned: the pose of the bones has to put every joint where the simulation says
        let r = rig();
        let mut sim = HairSim::new(&r);
        let mut pose = Pose { rot: vec![[0.0; 3]; r.bones.len()], offset: [0.0; 3] };
        pose.rot[0] = [0.0, 0.0, 25.0];
        let p = HairParams::default();
        for _ in 0..30 {
            let mut q = pose.clone();
            sim.step(&r, &mut q, &p, 1.0 / 60.0);
            let posed = r.posed_bones(&q);
            for (k, b) in sim.chains[0].bones.iter().enumerate() {
                let d = len(sub(posed[*b].0, sim.now[0][k]));
                assert!(d < 0.005, "joint {k} is {d} from where the simulation put it");
            }
            let last = *sim.chains[0].bones.last().unwrap();
            assert!(len(sub(posed[last].1, *sim.now[0].last().unwrap())) < 0.005, "the tip");
        }
    }

    #[test]
    fn the_hair_lags_behind_a_moving_head_and_comes_back_to_hang() {
        let r = rig();
        let mut sim = HairSim::new(&r);
        let params = HairParams::default();
        let tip = |sim: &HairSim| *sim.now[0].last().unwrap();
        let still = Pose { rot: vec![[0.0; 3]; r.bones.len()], offset: [0.0; 3] };
        for _ in 0..20 {
            sim.step(&r, &mut still.clone(), &params, 1.0 / 60.0);
        }
        let rest = tip(&sim);
        // the whole figure moves to the right, fast, then stops
        let mut x = 0.0;
        let mut lag: f32 = 0.0;
        for i in 0..40 {
            if i < 10 {
                x += 0.03;
            }
            let mut p = still.clone();
            p.offset = [x, 0.0, 0.0];
            sim.step(&r, &mut p, &params, 1.0 / 60.0);
            lag = lag.max(x - (tip(&sim)[0] - rest[0]));
        }
        assert!(lag > 0.03, "the tip was left behind by {lag}");
        // it settles under the head again
        for _ in 0..400 {
            let mut p = still.clone();
            p.offset = [x, 0.0, 0.0];
            sim.step(&r, &mut p, &params, 1.0 / 60.0);
        }
        assert!((tip(&sim)[0] - rest[0] - x).abs() < 0.01, "it hangs again: {} {}", tip(&sim)[0] - rest[0], x);
        // the pieces keep their length all the time
        let l = len(sub(sim.now[0][1], sim.now[0][0]));
        assert!((l - 0.1).abs() < 0.005, "{l}");
    }

    #[test]
    fn without_a_spring_the_hair_hangs_and_with_one_it_holds_its_shape() {
        // the strand sticks out sideways: free it falls, stiff it stays
        let mut r = Rig3::new(0);
        r.add_bone("head", None, [0.0, 1.7, 0.0], [0.0, 1.9, 0.0]);
        let a = r.add_bone("hair_0_0", Some(0), [0.0, 1.9, 0.0], [0.1, 1.9, 0.0]);
        r.add_bone("hair_0_1", Some(a), [0.1, 1.9, 0.0], [0.2, 1.9, 0.0]);
        let run = |stiffness: f32| {
            let mut sim = HairSim::new(&r);
            let p = HairParams { stiffness, ..Default::default() };
            for _ in 0..300 {
                sim.step(&r, &mut Pose { rot: vec![[0.0; 3]; 3], offset: [0.0; 3] }, &p, 1.0 / 60.0);
            }
            sim.now[0].last().unwrap()[1]
        };
        assert!(run(0.0) < 1.8, "free: it hangs");
        assert!(run(1.0) > 1.89, "stiff: it stays out");
    }

    #[test]
    fn a_whole_animation_gets_the_motion_of_the_hair_in_its_poses() {
        let r = rig();
        let poses: Vec<Pose> = (0..40).map(|i| Pose { rot: vec![[0.0; 3]; r.bones.len()], offset: [(i as f32 * 0.4).sin() * 0.1, 0.0, 0.0] }).collect();
        let mut anims = vec![Anim { name: "shake".into(), fps: 30.0, poses }];
        bake(&r, &mut anims, &HairParams::default());
        let moved = anims[0].poses.iter().filter(|p| (1..5).any(|b| p.rot[b] != [0.0; 3])).count();
        assert!(moved > 20, "{moved} frames have hair motion");
        assert!(anims[0].poses.iter().all(|p| p.rot[0] == [0.0; 3]), "the head is left as it was");
        // a rig without hair is left alone
        let plain = Rig3::humanoid(0, [0.0; 3], [1.0, 2.0, 1.0]);
        let mut a2 = vec![Anim { name: "x".into(), fps: 30.0, poses: vec![Pose { rot: vec![[0.0; 3]; plain.bones.len()], offset: [0.0; 3] }; 3] }];
        bake(&plain, &mut a2, &HairParams::default());
        assert!(a2[0].poses.iter().all(|p| p.rot.iter().all(|r| *r == [0.0; 3])));
    }
}
