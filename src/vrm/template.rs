// SPDX-License-Identifier: GPL-3.0-or-later
// Face shapes taken from a real VRM model (the user's own Emi, a VRoid-style face with its Fcl_* shapes). Only the way the
// front of the face moves is kept: for every shape, how far each point of the front surface goes. It is moved onto a
// picture by the same three points as the built-in recipes (the eyes and the mouth), so a blink closes the lid the way
// that model's lid closes and a smile lifts the corners the way hers do. The model file itself is not copied anywhere.

use std::collections::HashMap;

use super::face::FaceMarks;
use super::gltf::read_glb;

/// Our shape name and the shape of the template it is taken from. Left and right are settled by where the shape is.
const MAP: [(&str, &str); 19] = [
    ("blink", "Fcl_EYE_Close"),
    ("happy", "Fcl_ALL_Joy"),
    ("relaxed", "Fcl_ALL_Fun"),
    ("sad", "Fcl_ALL_Sorrow"),
    ("angry", "Fcl_ALL_Angry"),
    ("surprised", "Fcl_ALL_Surprised"),
    ("browInnerUp", "Fcl_BRW_Surprised"),
    ("browDown", "Fcl_BRW_Angry"),
    ("mouthSmile", "Fcl_MTH_Up"),
    ("mouthFrown", "Fcl_MTH_Down"),
    ("eyeWide", "Fcl_EYE_Spread"),
    ("eyeSquint", "Fcl_EYE_Fun"),
    ("aa", "Fcl_MTH_A"),
    ("ih", "Fcl_MTH_I"),
    ("ou", "Fcl_MTH_U"),
    ("ee", "Fcl_MTH_E"),
    ("oh", "Fcl_MTH_O"),
    ("jawOpen", "Fcl_MTH_Large"),
    ("blinkLeft", "Fcl_EYE_Close_L"),
];

#[derive(Debug)]
pub struct Template {
    pub eye_l: [f32; 2],
    pub eye_r: [f32; 2],
    pub mouth: [f32; 2],
    pub eye_w: f32,
    pub mouth_w: f32,
    /// The points of the front surface (x, y in metres).
    front: Vec<[f32; 2]>,
    cells: HashMap<(i32, i32), Vec<u32>>,
    /// Our shape names, and for each one how far every front point moves.
    shapes: Vec<(String, Vec<[f32; 2]>)>,
}

const CELL: f32 = 0.003;
const REACH: f32 = 0.0035;

fn cell_of(p: [f32; 2]) -> (i32, i32) {
    ((p[0] / CELL).floor() as i32, (p[1] / CELL).floor() as i32)
}

impl Template {
    /// Read the shapes from a VRM file (the first mesh with many shapes, taken as the face).
    pub fn from_vrm(bytes: &[u8]) -> Result<Template, String> {
        let g = read_glb(bytes)?;
        let meshes = g.json["meshes"].as_array().ok_or("The file has no meshes.")?;
        let (mesh, prim) = meshes
            .iter()
            .find_map(|m| {
                let p = &m["primitives"][0];
                p["targets"].as_array().filter(|t| t.len() >= 20).map(|_| (m, p))
            })
            .ok_or("This model has no face with shapes (about 20 or more are needed).")?;
        let names: Vec<String> = mesh["extras"]["targetNames"].as_array().map(|a| a.iter().map(|n| n.as_str().unwrap_or("").to_string()).collect()).unwrap_or_default();
        if names.is_empty() {
            return Err("The shapes of this face have no names.".into());
        }
        let pos = g.vec3s(prim["attributes"]["POSITION"].as_u64().ok_or("The face has no points.")? as usize)?;
        let targets = prim["targets"].as_array().unwrap();
        let delta = |name: &str| -> Option<Vec<[f32; 3]>> {
            let i = names.iter().position(|n| n == name)?;
            g.vec3s(targets[i]["POSITION"].as_u64()? as usize).ok().filter(|d| d.len() == pos.len())
        };
        // the front of the face: of the points in one small cell of the picture, the one that sticks out most
        let mut best: HashMap<(i32, i32), usize> = HashMap::new();
        for (i, p) in pos.iter().enumerate() {
            let c = cell_of([p[0], p[1]]);
            match best.get(&c) {
                Some(&j) if pos[j][2] >= p[2] => {}
                _ => {
                    best.insert(c, i);
                }
            }
        }
        let mut order: Vec<usize> = best.values().copied().collect();
        order.sort_unstable();
        let front: Vec<[f32; 2]> = order.iter().map(|&i| [pos[i][0], pos[i][1]]).collect();
        let mut cells: HashMap<(i32, i32), Vec<u32>> = HashMap::new();
        for (k, p) in front.iter().enumerate() {
            cells.entry(cell_of(*p)).or_default().push(k as u32);
        }
        // where the eyes and the mouth are: where the shapes that close the eyes and the lips move things
        let weighted = |d: &[[f32; 3]], keep: f32| -> Option<([f32; 2], f32, f32)> {
            let mag: Vec<f32> = d.iter().map(|v| (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt()).collect();
            let top = mag.iter().cloned().fold(0.0f32, f32::max);
            if top <= 0.0 {
                return None;
            }
            let (mut sx, mut sy, mut sw) = (0.0f32, 0.0f32, 0.0f32);
            let (mut x0, mut x1) = (f32::MAX, f32::MIN);
            for (i, m) in mag.iter().enumerate() {
                if *m > keep * top {
                    sx += pos[i][0] * m;
                    sy += pos[i][1] * m;
                    sw += m;
                    x0 = x0.min(pos[i][0]);
                    x1 = x1.max(pos[i][0]);
                }
            }
            Some(([sx / sw, sy / sw], x1 - x0, sw))
        };
        let (c_l, c_r) = (delta("Fcl_EYE_Close_L").ok_or("No Fcl_EYE_Close_L shape.")?, delta("Fcl_EYE_Close_R").ok_or("No Fcl_EYE_Close_R shape.")?);
        let (a, b) = (weighted(&c_l, 0.25).ok_or("The left blink moves nothing.")?, weighted(&c_r, 0.25).ok_or("The right blink moves nothing.")?);
        // the character's left is the side with the larger x
        let ((eye_l, eye_w), (eye_r, _), left_is_l) = if a.0[0] >= b.0[0] { ((a.0, a.1), (b.0, b.1), true) } else { ((b.0, b.1), (a.0, a.1), false) };
        let mouth_d = delta("Fcl_MTH_Close").or_else(|| delta("Fcl_MTH_Neutral")).ok_or("No mouth shape to find the mouth by.")?;
        let m = weighted(&mouth_d, 0.2).ok_or("The mouth shape moves nothing.")?;
        let mut shapes: Vec<(String, Vec<[f32; 2]>)> = Vec::new();
        let flat = |d: &[[f32; 3]]| -> Vec<[f32; 2]> { order.iter().map(|&i| [d[i][0], d[i][1]]).collect() };
        for (ours, theirs) in MAP {
            let theirs = match (ours, left_is_l) {
                ("blinkLeft", false) => "Fcl_EYE_Close_R",
                _ => theirs,
            };
            if let Some(d) = delta(theirs) {
                shapes.push((ours.to_string(), flat(&d)));
            }
        }
        // the other eye alone
        if let Some(d) = delta(if left_is_l { "Fcl_EYE_Close_R" } else { "Fcl_EYE_Close_L" }) {
            shapes.push(("blinkRight".to_string(), flat(&d)));
        }
        if shapes.len() < 10 {
            return Err("Too few of the expected shapes (Fcl_EYE_Close, Fcl_ALL_Joy, Fcl_MTH_A...) were found.".into());
        }
        Ok(Template { eye_l, eye_r, mouth: m.0, eye_w: eye_w.max(1e-4), mouth_w: m.1.max(1e-4), front, cells, shapes })
    }

    /// The names of the shapes this template gives.
    pub fn names(&self) -> Vec<&str> {
        self.shapes.iter().map(|s| s.0.as_str()).collect()
    }

    pub fn has(&self, name: &str) -> bool {
        self.shapes.iter().any(|s| s.0 == name)
    }

    /// How far a point of the page moves for every shape (page pixels), the shapes in the order of `names`. The face of
    /// the template is laid on the picture by the eyes (across) and by the way down from the eyes to the mouth.
    pub fn field(&self, marks: &FaceMarks, p: [f32; 2]) -> Option<Vec<[f32; 2]>> {
        let (tc, pc) = ([(self.eye_l[0] + self.eye_r[0]) / 2.0, (self.eye_l[1] + self.eye_r[1]) / 2.0], marks.eye_center());
        let t_across = (self.eye_l[0] - self.eye_r[0]).hypot(self.eye_l[1] - self.eye_r[1]).max(1e-6);
        let t_down = (self.mouth[1] - tc[1]).abs().max(1e-6);
        let p_down = (marks.mouth[1] - pc[1]).abs().max(1e-3);
        let (sx, sy) = (marks.eye_distance() / t_across, p_down / t_down);
        let q = [tc[0] + (p[0] - pc[0]) / sx, tc[1] - (p[1] - pc[1]) / sy];
        let (cx, cy) = cell_of(q);
        let mut near: Vec<(u32, f32)> = Vec::new();
        for dx in -2..=2 {
            for dy in -2..=2 {
                if let Some(v) = self.cells.get(&(cx + dx, cy + dy)) {
                    for &k in v {
                        let f = self.front[k as usize];
                        let d = (f[0] - q[0]).hypot(f[1] - q[1]);
                        if d < REACH * 2.0 {
                            near.push((k, 1.0 / (d * d + 1e-7)));
                        }
                    }
                }
            }
        }
        if near.is_empty() {
            return None;
        }
        let sum: f32 = near.iter().map(|n| n.1).sum();
        Some(
            self.shapes
                .iter()
                .map(|(_, d)| {
                    let (mut dx, mut dy) = (0.0f32, 0.0f32);
                    for (k, w) in &near {
                        let v = d[*k as usize];
                        dx += v[0] * w / sum;
                        dy += v[1] * w / sum;
                    }
                    // template metres to page pixels (y up in the template, down on the page)
                    [dx * sx, -dy * sy]
                })
                .collect(),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn emi() -> Option<Template> {
        let path = std::path::PathBuf::from(std::env::var("USERPROFILE").unwrap_or_default()).join("Downloads/art/Finished2D3D/vrmemi/Emi1.0.vrm");
        let bytes = std::fs::read(path).ok()?;
        Some(Template::from_vrm(&bytes).unwrap_or_else(|e| panic!("Emi: {e}")))
    }

    #[test]
    fn a_real_face_gives_its_eyes_its_mouth_and_its_shapes() {
        let Some(t) = emi() else { return };
        eprintln!("eyes {:?} {:?} w {:.3}, mouth {:?} w {:.3}, shapes {}", t.eye_l, t.eye_r, t.eye_w, t.mouth, t.mouth_w, t.shapes.len());
        // the character's left is on the larger x, the eyes are level, the mouth is below them and in the middle
        assert!(t.eye_l[0] > t.eye_r[0] && (t.eye_l[1] - t.eye_r[1]).abs() < 0.01);
        assert!(t.mouth[1] < t.eye_l[1] - 0.02 && (t.mouth[0] - (t.eye_l[0] + t.eye_r[0]) / 2.0).abs() < 0.01);
        assert!(t.eye_w > 0.01 && t.eye_w < 0.12 && t.mouth_w > 0.01 && t.mouth_w < 0.1);
        for n in ["blink", "blinkLeft", "blinkRight", "happy", "aa", "oh", "browInnerUp"] {
            assert!(t.has(n), "{n}");
        }
        // laid on a picture: a blink moves the points above the eye down, the ones below it up or not at all
        let marks = FaceMarks { eye_l: [600.0, 400.0], eye_r: [400.0, 400.0], mouth: [500.0, 600.0], eye_w: 80.0, mouth_w: 100.0 };
        let names = t.names();
        let blink = names.iter().position(|n| *n == "blink").unwrap();
        let mut moved_down = 0;
        for y in (360..=420).step_by(6) {
            if let Some(f) = t.field(&marks, [600.0, y as f32]) {
                if f[blink][1] > 1.0 {
                    moved_down += 1;
                }
            }
        }
        assert!(moved_down >= 2, "the upper lid comes down");
        // far from the face nothing is given
        assert!(t.field(&marks, [5000.0, 5000.0]).is_none());
    }
}
