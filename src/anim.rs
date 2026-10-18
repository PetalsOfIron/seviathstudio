// SPDX-License-Identifier: GPL-3.0-or-later
// The animation of a character: a timeline of tracks. A track moves one thing over time with keys (a frame, a value and how to get
// to the next key): a slider of the rig (head turn, mouth open...), or a property of a layer (how see-through it is, whether it is
// shown). The playhead moves along the timeline at a frame rate, and what the tracks say at that frame is put on the rig. Rig clips
// (recorded while posing, or the motions of a Live2D model) can be turned into tracks with the keys that matter.

use serde::{Deserialize, Serialize};

use crate::rig::{Clip, Id};

/// How the value goes from a key to the next one.
#[derive(Clone, Copy, PartialEq, Debug, Serialize, Deserialize)]
pub enum Ease {
    Linear,
    /// Holds the value until the next key.
    Step,
    In,
    Out,
    InOut,
    /// A curve with two handles (x1, y1, x2, y2), as on the web: x is the share of the time, y the share of the change.
    Bezier([f32; 4]),
}

impl Ease {
    pub const NAMED: [(Ease, &'static str); 5] = [(Ease::Linear, "Linear"), (Ease::Step, "Step"), (Ease::In, "Ease in"), (Ease::Out, "Ease out"), (Ease::InOut, "Ease in and out")];

    pub fn label(&self) -> &'static str {
        match self {
            Ease::Linear => "Linear",
            Ease::Step => "Step",
            Ease::In => "Ease in",
            Ease::Out => "Ease out",
            Ease::InOut => "Ease in and out",
            Ease::Bezier(_) => "Curve",
        }
    }

    /// The share of the change (0..1) at `t` (0..1) of the time between two keys.
    pub fn at(&self, t: f32) -> f32 {
        let t = t.clamp(0.0, 1.0);
        match self {
            Ease::Linear => t,
            Ease::Step => 0.0,
            Ease::In => t * t * t,
            Ease::Out => 1.0 - (1.0 - t).powi(3),
            Ease::InOut => t * t * (3.0 - 2.0 * t),
            Ease::Bezier(h) => {
                // find the curve parameter whose x is t, then read y
                let bez = |a: f32, b: f32, u: f32| 3.0 * (1.0 - u).powi(2) * u * a + 3.0 * (1.0 - u) * u * u * b + u * u * u;
                let (mut lo, mut hi) = (0.0f32, 1.0f32);
                for _ in 0..30 {
                    let m = (lo + hi) / 2.0;
                    if bez(h[0].clamp(0.0, 1.0), h[2].clamp(0.0, 1.0), m) < t { lo = m } else { hi = m }
                }
                bez(h[1], h[3], (lo + hi) / 2.0)
            }
        }
    }
}

#[derive(Clone, Copy, PartialEq, Debug, Serialize, Deserialize)]
pub struct Key {
    pub frame: f32,
    pub value: f32,
    /// How to go from this key to the next.
    pub ease: Ease,
}

/// What a track moves.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub enum Target {
    /// A slider of the rig.
    Param(Id),
    /// How see-through a layer is (0 to 100).
    Opacity(u64),
    /// Whether a layer is shown (on from 0.5).
    Visible(u64),
    /// The camera: how far it is moved across and down (pixels), how close it is (percent) and how far it is turned (degrees).
    CameraX,
    CameraY,
    CameraZoom,
    CameraRotate,
}

impl Target {
    pub fn is_camera(&self) -> bool {
        matches!(self, Target::CameraX | Target::CameraY | Target::CameraZoom | Target::CameraRotate)
    }
}

#[derive(Clone, PartialEq, Debug, Serialize, Deserialize)]
pub struct Track {
    pub id: u32,
    pub name: String,
    pub target: Target,
    pub keys: Vec<Key>,
    pub muted: bool,
}

impl Track {
    /// The value at `frame`: before the first key and after the last the first and last values, in between by the ease of the key before.
    pub fn value_at(&self, frame: f32) -> Option<f32> {
        let k = &self.keys;
        let first = k.first()?;
        if frame <= first.frame {
            return Some(first.value);
        }
        let last = k.last()?;
        if frame >= last.frame {
            return Some(last.value);
        }
        let i = k.partition_point(|x| x.frame <= frame).saturating_sub(1);
        let (a, b) = (&k[i], &k[i + 1]);
        let span = (b.frame - a.frame).max(1e-6);
        Some(a.value + (b.value - a.value) * a.ease.at((frame - a.frame) / span))
    }

    /// Put a key at `frame` (one that is there is replaced); keeps the keys in order. Returns the index of the key.
    pub fn set_key(&mut self, frame: f32, value: f32, ease: Ease) -> usize {
        if let Some(i) = self.keys.iter().position(|k| (k.frame - frame).abs() < 0.001) {
            self.keys[i].value = value;
            return i;
        }
        self.keys.push(Key { frame, value, ease });
        self.keys.sort_by(|a, b| a.frame.total_cmp(&b.frame));
        self.keys.iter().position(|k| (k.frame - frame).abs() < 0.001).unwrap_or(0)
    }

    pub fn key_at(&self, frame: f32) -> Option<usize> {
        self.keys.iter().position(|k| (k.frame - frame).abs() < 0.5)
    }

    /// Move a key to another frame (the keys stay in order). Returns its new index.
    pub fn move_key(&mut self, i: usize, frame: f32) -> usize {
        if i >= self.keys.len() {
            return i;
        }
        let mut k = self.keys.remove(i);
        k.frame = frame.max(0.0);
        // a key already there is taken over by the moved one
        self.keys.retain(|o| (o.frame - k.frame).abs() > 0.001);
        self.keys.push(k);
        self.keys.sort_by(|a, b| a.frame.total_cmp(&b.frame));
        self.keys.iter().position(|o| (o.frame - k.frame).abs() < 0.001).unwrap_or(0)
    }
}

/// From this frame on a track shows this drawing (a layer) until the next exposure; None shows nothing (a blank frame).
#[derive(Clone, Copy, PartialEq, Debug, Serialize, Deserialize)]
pub struct Exposure {
    pub frame: u32,
    pub cel: Option<u64>,
}

/// Frame by frame animation: a folder of layers, each layer one drawing (a cel), and which of them shows at every frame.
#[derive(Clone, PartialEq, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct CelTrack {
    pub id: u32,
    pub name: String,
    /// The folder layer the drawings are in.
    pub folder: u64,
    /// The drawings (layer ids), in the order they were made: their numbers are the positions here, from 1.
    pub cels: Vec<u64>,
    pub exposures: Vec<Exposure>,
    pub muted: bool,
}

impl CelTrack {
    /// The exposure in force at `frame` (the last one that started at or before it).
    pub fn exposure_at(&self, frame: u32) -> Option<usize> {
        self.exposures.partition_point(|e| e.frame <= frame).checked_sub(1)
    }

    pub fn shown_at(&self, frame: u32) -> Option<u64> {
        self.exposure_at(frame).and_then(|i| self.exposures[i].cel)
    }

    /// From `frame` on show `cel` (an exposure that starts there is replaced).
    pub fn set_exposure(&mut self, frame: u32, cel: Option<u64>) {
        match self.exposures.iter().position(|e| e.frame == frame) {
            Some(i) => self.exposures[i].cel = cel,
            None => {
                self.exposures.push(Exposure { frame, cel });
                self.exposures.sort_by_key(|e| e.frame);
            }
        }
    }

    pub fn remove_exposure(&mut self, frame: u32) -> bool {
        let n = self.exposures.len();
        self.exposures.retain(|e| e.frame != frame);
        self.exposures.len() != n
    }

    /// Make room for `by` frames at `frame` (everything from there moves later), or take `-by` frames away there (what was inside the
    /// taken part is gone, what was after it moves earlier).
    pub fn shift(&mut self, frame: u32, by: i32) {
        if by >= 0 {
            for e in &mut self.exposures {
                if e.frame >= frame {
                    e.frame += by as u32;
                }
            }
        } else {
            let cut = by.unsigned_abs();
            // what the first exposure inside the taken part would have shown carries on at the start of the gap
            let carry = self.shown_at(frame.saturating_add(cut).saturating_sub(1));
            self.exposures.retain(|e| e.frame < frame || e.frame >= frame + cut);
            for e in &mut self.exposures {
                if e.frame >= frame + cut {
                    e.frame -= cut;
                }
            }
            if self.shown_at(frame) != carry && !self.exposures.iter().any(|e| e.frame == frame) {
                self.exposures.push(Exposure { frame, cel: carry });
                self.exposures.sort_by_key(|e| e.frame);
            }
        }
    }

    /// The number (from 1) a drawing has in this track.
    pub fn number_of(&self, cel: u64) -> Option<usize> {
        self.cels.iter().position(|c| *c == cel).map(|i| i + 1)
    }
}

/// Onion skin: the drawings before and after the frame, faintly, while drawing and scrubbing.
#[derive(Clone, Copy, PartialEq, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct Onion {
    pub on: bool,
    pub before: u8,
    pub after: u8,
    pub opacity: f32,
}

impl Default for Onion {
    fn default() -> Self {
        Onion { on: true, before: 2, after: 1, opacity: 0.4 }
    }
}

/// A named place on the timeline (the start of a walk, the moment of a hit...). Markers cut the animation into parts that can be
/// played and saved on their own.
#[derive(Clone, PartialEq, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Marker {
    pub frame: u32,
    pub name: String,
}

/// The sound of the animation: the file it is read from (kept by its path) and the frame it starts at (negative: before the start).
#[derive(Clone, PartialEq, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct AudioRef {
    pub path: String,
    pub offset: f32,
}

#[derive(Clone, PartialEq, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct Animation {
    pub fps: f32,
    /// How many frames long the animation is.
    pub length: u32,
    /// The part that plays (first and last frame), the work area; None plays everything.
    pub range: Option<[u32; 2]>,
    pub looping: bool,
    /// When looping, go back and forth instead of jumping back.
    pub ping_pong: bool,
    pub tracks: Vec<Track>,
    pub markers: Vec<Marker>,
    pub audio: Option<AudioRef>,
    pub cel_tracks: Vec<CelTrack>,
    pub onion: Onion,
    pub shake: crate::camera::Shake,
    next_id: u32,
}

impl Default for Animation {
    fn default() -> Self {
        Animation { fps: 24.0, length: 120, range: None, looping: true, ping_pong: false, tracks: Vec::new(), markers: Vec::new(), audio: None, cel_tracks: Vec::new(), onion: Onion::default(), shake: crate::camera::Shake::default(), next_id: 1 }
    }
}

impl Animation {
    pub fn is_empty(&self) -> bool {
        self.tracks.is_empty()
    }

    pub fn seconds(&self) -> f32 {
        self.length as f32 / self.fps.max(1.0)
    }

    /// The first and last frame that play.
    pub fn play_range(&self) -> (u32, u32) {
        let last = self.length.saturating_sub(1);
        match self.range {
            Some([a, b]) if a < b => (a.min(last), b.min(last)),
            _ => (0, last),
        }
    }

    /// Where the playhead is after `seconds` of play from `frame` going `dir` (1 or -1): the new frame and the new direction.
    pub fn advance(&self, frame: f32, dir: f32, seconds: f32) -> (f32, f32) {
        let (a, b) = self.play_range();
        let (a, b) = (a as f32, b as f32);
        let mut f = frame + dir * seconds * self.fps;
        let mut d = dir;
        if f > b || f < a {
            if self.ping_pong && self.looping {
                // back and forth between the ends
                let span = (b - a).max(1.0);
                let mut t = (f - a).rem_euclid(2.0 * span);
                if t > span {
                    t = 2.0 * span - t;
                    d = -1.0;
                } else {
                    d = 1.0;
                }
                f = a + t;
            } else if self.looping {
                f = a + (f - a).rem_euclid((b - a + 1.0).max(1.0));
            } else {
                f = f.clamp(a, b);
            }
        }
        (f, d)
    }

    /// Add a marker at `frame` (one that is there is renamed) and keep the list in order. Returns its index.
    pub fn set_marker(&mut self, frame: u32, name: &str) -> usize {
        if let Some(i) = self.markers.iter().position(|m| m.frame == frame) {
            self.markers[i].name = name.to_string();
            return i;
        }
        self.markers.push(Marker { frame, name: name.to_string() });
        self.markers.sort_by_key(|m| m.frame);
        self.markers.iter().position(|m| m.frame == frame).unwrap_or(0)
    }

    /// The first and last frame of the part a marker begins: up to the frame before the next marker, or the end.
    pub fn marker_range(&self, i: usize) -> Option<[u32; 2]> {
        let m = self.markers.get(i)?;
        let end = self.markers.get(i + 1).map(|n| n.frame.saturating_sub(1)).unwrap_or(self.length.saturating_sub(1));
        Some([m.frame, end.max(m.frame + 1).min(self.length.saturating_sub(1).max(m.frame + 1))])
    }

    /// Make (or remake) the track of a slider from a value in 0 to 1 for every frame (the mouth from the loudness of a sound): the keys
    /// that matter, scaled to the range of the slider. Returns how many keys the track has.
    pub fn set_from_curve(&mut self, param: Id, name: &str, values: &[f32], lo: f32, hi: f32) -> usize {
        let t = self.track_for(Target::Param(param), name);
        let samples: Vec<(f32, f32)> = values.iter().enumerate().map(|(f, v)| (f as f32, lo + (hi - lo) * v.clamp(0.0, 1.0))).collect();
        let keys = simplify(&samples, ((hi - lo).abs() * 0.02).max(1e-4));
        self.tracks[t].keys.clear();
        for (f, v) in keys {
            self.tracks[t].set_key(f, v, Ease::Linear);
        }
        self.tracks[t].keys.len()
    }

    /// A new frame by frame track on a folder with its first drawing, shown from frame 0. Returns its index.
    pub fn add_cel_track(&mut self, name: &str, folder: u64, first: u64) -> usize {
        let id = self.next_id;
        self.next_id += 1;
        self.cel_tracks.push(CelTrack { id, name: name.to_string(), folder, cels: vec![first], exposures: vec![Exposure { frame: 0, cel: Some(first) }], muted: false });
        self.cel_tracks.len() - 1
    }

    /// For every drawing of every track that is on: whether it shows at `frame`.
    pub fn cel_visibility(&self, frame: u32) -> Vec<(u64, bool)> {
        let mut out = Vec::new();
        for t in self.cel_tracks.iter().filter(|t| !t.muted) {
            let shown = t.shown_at(frame);
            out.extend(t.cels.iter().map(|c| (*c, shown == Some(*c))));
        }
        out
    }

    /// Where the camera looks at a frame: the camera tracks, and the shake on top.
    pub fn camera_at(&self, frame: f32) -> crate::camera::Camera {
        let mut c = crate::camera::Camera::default();
        for (t, v) in self.sample(frame) {
            match t {
                Target::CameraX => c.x = v,
                Target::CameraY => c.y = v,
                Target::CameraZoom => c.zoom = v.max(1.0),
                Target::CameraRotate => c.rot = v,
                _ => {}
            }
        }
        let (dx, dy, dr) = self.shake.at(frame / self.fps.max(1.0));
        c.x += dx;
        c.y += dy;
        c.rot += dr;
        c
    }

    pub fn track(&self, target: Target) -> Option<usize> {
        self.tracks.iter().position(|t| t.target == target)
    }

    /// A track for a target (the one there is, or a new one).
    pub fn track_for(&mut self, target: Target, name: &str) -> usize {
        if let Some(i) = self.track(target) {
            return i;
        }
        let id = self.next_id;
        self.next_id += 1;
        self.tracks.push(Track { id, name: name.to_string(), target, keys: Vec::new(), muted: false });
        self.tracks.len() - 1
    }

    pub fn remove_track(&mut self, i: usize) {
        if i < self.tracks.len() {
            self.tracks.remove(i);
        }
    }

    /// What every track that is on says at `frame`.
    pub fn sample(&self, frame: f32) -> Vec<(Target, f32)> {
        self.tracks.iter().filter(|t| !t.muted).filter_map(|t| t.value_at(frame).map(|v| (t.target, v))).collect()
    }

    /// The last frame with a key (the animation can be cut to this).
    pub fn last_key(&self) -> Option<u32> {
        self.tracks.iter().flat_map(|t| t.keys.iter()).map(|k| k.frame.ceil() as u32).max()
    }

    /// Make tracks of the sliders of a clip, from `start`: for every slider the keys that matter (a straight line between the keys
    /// is within `tolerance` of the value at every frame of the clip), at the frame rate of the clip.
    pub fn add_clip(&mut self, clip: &Clip, start: f32, names: &dyn Fn(Id) -> String, ranges: &dyn Fn(Id) -> f32) -> usize {
        let mut ids: Vec<Id> = clip.frames.iter().flat_map(|f| f.iter().map(|(i, _)| *i)).collect();
        ids.sort_unstable();
        ids.dedup();
        let scale = self.fps / clip.fps.max(1.0);
        let mut added = 0;
        for id in ids {
            let samples: Vec<(f32, f32)> = clip.frames.iter().enumerate().filter_map(|(i, f)| f.iter().find(|(p, _)| *p == id).map(|(_, v)| (i as f32 * scale + start, *v))).collect();
            if samples.is_empty() {
                continue;
            }
            let tol = (ranges(id) * 0.004).max(1e-4);
            let keys = simplify(&samples, tol);
            let t = self.track_for(Target::Param(id), &names(id));
            for (f, v) in keys {
                self.tracks[t].set_key(f, v, Ease::Linear);
            }
            added += 1;
        }
        let end = (clip.frames.len() as f32 * scale + start).ceil() as u32;
        self.length = self.length.max(end + 1);
        added
    }
}

/// The samples that matter: the ends, and the ones that a straight line between the kept samples would miss by more than `tol`.
pub fn simplify(samples: &[(f32, f32)], tol: f32) -> Vec<(f32, f32)> {
    if samples.len() <= 2 {
        return samples.to_vec();
    }
    let mut keep = vec![false; samples.len()];
    keep[0] = true;
    keep[samples.len() - 1] = true;
    let mut stack = vec![(0usize, samples.len() - 1)];
    while let Some((a, b)) = stack.pop() {
        let (pa, pb) = (samples[a], samples[b]);
        let mut worst = (0.0f32, a);
        for i in a + 1..b {
            let t = (samples[i].0 - pa.0) / (pb.0 - pa.0).max(1e-6);
            let d = (samples[i].1 - (pa.1 + (pb.1 - pa.1) * t)).abs();
            if d > worst.0 {
                worst = (d, i);
            }
        }
        if worst.0 > tol {
            keep[worst.1] = true;
            stack.push((a, worst.1));
            stack.push((worst.1, b));
        }
    }
    samples.iter().zip(&keep).filter(|(_, k)| **k).map(|(s, _)| *s).collect()
}

#[cfg(test)]
mod tests {

    #[test]
    fn camera_tracks_give_the_camera_at_a_frame() {
        let mut a = Animation::default();
        assert!(a.camera_at(10.0).is_default());
        let z = a.track_for(Target::CameraZoom, "Zoom");
        a.tracks[z].set_key(0.0, 100.0, Ease::Linear);
        a.tracks[z].set_key(10.0, 200.0, Ease::Linear);
        let x = a.track_for(Target::CameraX, "Pan");
        a.tracks[x].set_key(0.0, 0.0, Ease::Linear);
        a.tracks[x].set_key(10.0, -50.0, Ease::Linear);
        let c = a.camera_at(5.0);
        assert_eq!((c.zoom, c.x, c.y, c.rot), (150.0, -25.0, 0.0, 0.0));
        assert!(Target::CameraRotate.is_camera() && !Target::Param(1).is_camera());
        // a shake moves it a little without a track
        a.shake.on = true;
        assert_ne!(a.camera_at(3.0).x, -15.0 + 0.0, "the shake is added");
        assert!((a.camera_at(3.0).x - (-15.0)).abs() <= a.shake.amount + 1e-3);
    }

    #[test]
    fn a_cel_track_shows_the_drawing_of_the_exposure_in_force() {
        let mut a = Animation::default();
        let t = a.add_cel_track("Walk", 100, 11);
        a.cel_tracks[t].cels.extend([12, 13]);
        a.cel_tracks[t].set_exposure(4, Some(12));
        a.cel_tracks[t].set_exposure(8, None);
        a.cel_tracks[t].set_exposure(10, Some(13));
        let tr = &a.cel_tracks[0];
        assert_eq!((tr.shown_at(0), tr.shown_at(3), tr.shown_at(4), tr.shown_at(7), tr.shown_at(8), tr.shown_at(9), tr.shown_at(10), tr.shown_at(500)), (Some(11), Some(11), Some(12), Some(12), None, None, Some(13), Some(13)));
        assert_eq!(tr.number_of(13), Some(3));
        assert_eq!(tr.number_of(99), None);
        // which drawings are on at a frame
        assert_eq!(a.cel_visibility(5), [(11, false), (12, true), (13, false)]);
        assert_eq!(a.cel_visibility(8), [(11, false), (12, false), (13, false)], "a blank frame shows none");
        a.cel_tracks[0].muted = true;
        assert!(a.cel_visibility(5).is_empty(), "a muted track leaves the layers alone");
        a.cel_tracks[0].muted = false;
        // a second drawing at the same frame replaces the first, and one can be taken away
        a.cel_tracks[0].set_exposure(4, Some(13));
        assert_eq!(a.cel_tracks[0].shown_at(5), Some(13));
        assert!(a.cel_tracks[0].remove_exposure(4));
        assert!(!a.cel_tracks[0].remove_exposure(4));
        assert_eq!(a.cel_tracks[0].shown_at(5), Some(11));
        // before the first exposure nothing shows
        let mut late = CelTrack::default();
        late.set_exposure(5, Some(1));
        assert_eq!((late.shown_at(2), late.shown_at(5)), (None, Some(1)));
    }

    #[test]
    fn frames_can_be_made_room_for_and_taken_away() {
        let mut t = CelTrack::default();
        t.cels = vec![1, 2, 3];
        for (f, c) in [(0, 1), (3, 2), (6, 3)] {
            t.set_exposure(f, Some(c));
        }
        // two frames of room at frame 3: the second drawing starts at 5
        t.shift(3, 2);
        assert_eq!(t.exposures.iter().map(|e| e.frame).collect::<Vec<_>>(), [0, 5, 8]);
        assert_eq!((t.shown_at(4), t.shown_at(5)), (Some(1), Some(2)), "the first drawing is held longer");
        // taking two frames away at frame 3 is the other way round
        t.shift(3, -2);
        assert_eq!(t.exposures.iter().map(|e| e.frame).collect::<Vec<_>>(), [0, 3, 6]);
        // taking away a part that holds a drawing start: what was shown at the end of the part goes on
        t.shift(2, -2);
        assert_eq!((t.shown_at(1), t.shown_at(2), t.shown_at(4)), (Some(1), Some(2), Some(3)), "{:?}", t.exposures);
    }

    #[test]
    fn onion_skin_and_cel_tracks_are_saved_and_old_files_still_load() {
        let mut a = Animation::default();
        a.add_cel_track("A", 5, 6);
        a.onion.before = 3;
        let back: Animation = serde_json::from_str(&serde_json::to_string(&a).unwrap()).unwrap();
        assert_eq!(back.cel_tracks, a.cel_tracks);
        assert_eq!(back.onion.before, 3);
        let old: Animation = serde_json::from_str("{\"fps\":12.0}").unwrap();
        assert!(old.cel_tracks.is_empty() && old.onion.on);
    }

    #[test]
    fn markers_cut_the_animation_into_parts() {
        let mut a = Animation { length: 100, ..Default::default() };
        a.set_marker(40, "Walk");
        a.set_marker(0, "Idle");
        a.set_marker(70, "Jump");
        assert_eq!(a.markers.iter().map(|m| (m.frame, m.name.as_str())).collect::<Vec<_>>(), [(0, "Idle"), (40, "Walk"), (70, "Jump")], "kept in order");
        assert_eq!(a.marker_range(0), Some([0, 39]));
        assert_eq!(a.marker_range(1), Some([40, 69]));
        assert_eq!(a.marker_range(2), Some([70, 99]), "the last part goes to the end");
        assert_eq!(a.marker_range(3), None);
        // a marker where one is only gives it the new name
        assert_eq!(a.set_marker(40, "Run"), 1);
        assert_eq!(a.markers.len(), 3);
        assert_eq!(a.markers[1].name, "Run");
        // saved and read back (old files have no names)
        let back: Animation = serde_json::from_str(&serde_json::to_string(&a).unwrap()).unwrap();
        assert_eq!(back.markers, a.markers);
        let old: Animation = serde_json::from_str("{\"markers\":[{\"frame\":5}]}").unwrap();
        assert_eq!(old.markers, [Marker { frame: 5, name: String::new() }]);
    }
    use super::*;

    fn track(keys: &[(f32, f32, Ease)]) -> Track {
        Track { id: 1, name: "t".into(), target: Target::Param(1), keys: keys.iter().map(|k| Key { frame: k.0, value: k.1, ease: k.2 }).collect(), muted: false }
    }

    #[test]
    fn a_track_holds_before_and_after_its_keys_and_goes_between_them_by_the_ease() {
        let t = track(&[(10.0, 0.0, Ease::Linear), (20.0, 10.0, Ease::Step), (30.0, 0.0, Ease::InOut), (40.0, 5.0, Ease::Linear)]);
        assert_eq!(t.value_at(0.0), Some(0.0));
        assert_eq!(t.value_at(50.0), Some(5.0));
        assert!((t.value_at(15.0).unwrap() - 5.0).abs() < 1e-5, "linear");
        assert_eq!(t.value_at(25.0), Some(10.0), "step holds");
        assert!((t.value_at(35.0).unwrap() - 2.5).abs() < 1e-4 || t.value_at(35.0).is_some());
        // ease in and out is slower at the ends than a straight line and the same in the middle
        let io = track(&[(0.0, 0.0, Ease::InOut), (10.0, 10.0, Ease::Linear)]);
        assert!(io.value_at(1.0).unwrap() < 1.0 && io.value_at(9.0).unwrap() > 9.0);
        assert!((io.value_at(5.0).unwrap() - 5.0).abs() < 1e-4);
        // a curve with the handles of a straight line is a straight line
        let b = Ease::Bezier([1.0 / 3.0, 1.0 / 3.0, 2.0 / 3.0, 2.0 / 3.0]);
        for t in [0.1, 0.5, 0.9] {
            assert!((b.at(t) - t).abs() < 0.01, "{t} {}", b.at(t));
        }
        assert!(Track { keys: Vec::new(), ..t.clone() }.value_at(5.0).is_none());
    }

    #[test]
    fn keys_are_set_moved_and_kept_in_order() {
        let mut t = track(&[]);
        t.set_key(20.0, 2.0, Ease::Linear);
        t.set_key(10.0, 1.0, Ease::Linear);
        t.set_key(30.0, 3.0, Ease::Linear);
        assert_eq!(t.keys.iter().map(|k| k.frame).collect::<Vec<_>>(), vec![10.0, 20.0, 30.0]);
        // a key at the same frame is replaced, not doubled
        t.set_key(20.0, 9.0, Ease::Linear);
        assert_eq!(t.keys.len(), 3);
        assert_eq!(t.value_at(20.0), Some(9.0));
        // moving a key past another reorders them; landing on a key takes its place
        let i = t.move_key(0, 25.0);
        assert_eq!(t.keys[i].frame, 25.0);
        assert_eq!(t.keys.iter().map(|k| k.frame).collect::<Vec<_>>(), vec![20.0, 25.0, 30.0]);
        t.move_key(1, 30.0);
        assert_eq!(t.keys.len(), 2);
        assert_eq!(t.key_at(30.2), Some(1));
        assert_eq!(t.key_at(5.0), None);
    }

    #[test]
    fn the_playhead_loops_ping_pongs_or_stops() {
        let mut a = Animation { fps: 10.0, length: 21, ..Default::default() };
        // 0 to 20, 10 frames a second: a second from frame 15 is frame 25: loops to 4
        let (f, d) = a.advance(15.0, 1.0, 1.0);
        assert!((f - 4.0).abs() < 1e-4 && d == 1.0, "{f}");
        a.looping = false;
        assert_eq!(a.advance(15.0, 1.0, 1.0).0, 20.0, "stops at the end");
        a.looping = true;
        a.ping_pong = true;
        let (f, d) = a.advance(15.0, 1.0, 1.0);
        assert!((f - 15.0).abs() < 1e-3 && d == -1.0, "back from the end: {f} {d}");
        // the work area
        a.ping_pong = false;
        a.range = Some([5, 10]);
        assert_eq!(a.play_range(), (5, 10));
        let (f, _) = a.advance(9.0, 1.0, 0.3);
        assert!((5.0..=10.0).contains(&f), "{f}");
        assert!((a.seconds() - 2.1).abs() < 1e-5);
    }

    #[test]
    fn tracks_are_found_or_made_and_sampled_and_muted_ones_say_nothing() {
        let mut a = Animation::default();
        let t = a.track_for(Target::Param(7), "Head X");
        assert_eq!(a.track_for(Target::Param(7), "again"), t, "one track for a target");
        a.tracks[t].set_key(0.0, 0.0, Ease::Linear);
        a.tracks[t].set_key(24.0, 30.0, Ease::Linear);
        let o = a.track_for(Target::Opacity(3), "Veil");
        a.tracks[o].set_key(0.0, 100.0, Ease::Linear);
        let s = a.sample(12.0);
        assert_eq!(s.len(), 2);
        assert_eq!(s[0], (Target::Param(7), 15.0));
        a.tracks[t].muted = true;
        assert_eq!(a.sample(12.0), vec![(Target::Opacity(3), 100.0)]);
        assert_eq!(a.last_key(), Some(24));
        a.remove_track(t);
        assert_eq!(a.tracks.len(), 1);
        // saved and read back; an old project without it is an empty animation
        let text = serde_json::to_string(&a).unwrap();
        assert_eq!(serde_json::from_str::<Animation>(&text).unwrap(), a);
        assert!(serde_json::from_str::<Animation>("{}").unwrap().is_empty());
    }

    #[test]
    fn a_clip_becomes_tracks_with_only_the_keys_that_matter() {
        // a slider that goes 0 to 10 straight over 30 frames, and one that swings
        let frames: Vec<Vec<(Id, f32)>> = (0..30).map(|i| vec![(1, i as f32 / 3.0), (2, (i as f32 * 0.4).sin() * 5.0)]).collect();
        let clip = Clip { name: "m".into(), fps: 30.0, frames };
        let mut a = Animation { fps: 30.0, length: 10, ..Default::default() };
        let n = a.add_clip(&clip, 5.0, &|id| format!("Slider {id}"), &|_| 10.0);
        assert_eq!(n, 2);
        let straight = &a.tracks[a.track(Target::Param(1)).unwrap()];
        assert_eq!(straight.keys.len(), 2, "a straight line needs two keys");
        assert_eq!(straight.keys[0].frame, 5.0);
        let swing = &a.tracks[a.track(Target::Param(2)).unwrap()];
        assert!(swing.keys.len() > 5 && swing.keys.len() < 30, "{}", swing.keys.len());
        // the tracks give the clip back within the tolerance
        for f in [0usize, 7, 15, 29] {
            let want = (f as f32 * 0.4).sin() * 5.0;
            assert!((swing.value_at(f as f32 + 5.0).unwrap() - want).abs() < 0.2, "frame {f}");
        }
        assert!(a.length >= 35, "the animation grew to hold the clip: {}", a.length);
        // a clip at another frame rate is stretched to this one
        let mut b = Animation { fps: 60.0, length: 10, ..Default::default() };
        b.add_clip(&clip, 0.0, &|_| String::new(), &|_| 10.0);
        let s = &b.tracks[0];
        assert!((s.keys.last().unwrap().frame - 58.0).abs() < 1.1, "{:?}", s.keys.last());
    }
}
