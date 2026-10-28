// SPDX-License-Identifier: GPL-3.0-or-later
// A library of the animations made by hand. An animation is saved by the names of the bones, not by their numbers, so it can be used on another character that
// has bones with the same names (hips, spine, leftUpperArm...). Two places are read: the user's own folder (in the settings folder of the app), and the folder
// `animations` next to the program, which the installer fills: the animations that were made while the app was made are already there when it is installed.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::rig3d::Rig3;
use crate::take::Take;

/// The ending of the files of the library.
pub const EXTENSION: &str = "sevanim";

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct TakeFile {
    pub name: String,
    pub fps: f32,
    pub frames: usize,
    pub looping: bool,
    /// For every bone (by name) the turns at the frames that have a key.
    pub tracks: BTreeMap<String, BTreeMap<usize, [f32; 3]>>,
    /// The move of the whole figure at the frames that have a key.
    pub moves: BTreeMap<usize, [f32; 3]>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Source {
    /// The animations of the user.
    Mine,
    /// The animations that came with the app.
    BuiltIn,
}

#[derive(Clone, Debug)]
pub struct Entry {
    pub name: String,
    pub path: PathBuf,
    pub source: Source,
    pub frames: usize,
    pub keys: usize,
}

/// The folder of the user's animations.
pub fn user_folder() -> PathBuf {
    dirs::config_dir().unwrap_or_else(std::env::temp_dir).join("SeviathStudio").join("animations")
}

/// The folder next to the program that the installer fills.
pub fn built_in_folder() -> PathBuf {
    std::env::current_exe().ok().and_then(|p| p.parent().map(|d| d.join("animations"))).unwrap_or_else(|| PathBuf::from("animations"))
}

/// The folder of the source of the program, when it is on this computer (where the animations are put to go in the installer).
pub fn source_folder() -> Option<PathBuf> {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    root.join("Cargo.toml").exists().then(|| root.join("animations"))
}

impl TakeFile {
    /// A take as a file, with the names of the bones it was made for.
    pub fn from_take(t: &Take, rig: &Rig3) -> TakeFile {
        let mut tracks = BTreeMap::new();
        for (i, keys) in t.keys.iter().enumerate() {
            if let (Some(b), false) = (rig.bones.get(i), keys.is_empty()) {
                tracks.insert(b.name.clone(), keys.clone());
            }
        }
        TakeFile { name: t.name.clone(), fps: t.fps, frames: t.frames, looping: t.looping, tracks, moves: t.moves.clone() }
    }

    /// The take for a rig: a bone gets the keys of the track with its name (the case does not matter). Returns the take and how many tracks found a bone.
    pub fn to_take(&self, rig: &Rig3) -> (Take, usize) {
        let mut t = Take::new(&self.name, rig.bones.len());
        t.fps = self.fps;
        t.frames = self.frames;
        t.looping = self.looping;
        t.moves = self.moves.clone();
        let mut found = 0;
        for (name, keys) in &self.tracks {
            if let Some(i) = rig.bones.iter().position(|b| b.name.eq_ignore_ascii_case(name)) {
                t.keys[i] = keys.clone();
                found += 1;
            }
        }
        (t, found)
    }

    pub fn key_count(&self) -> usize {
        self.tracks.values().map(|k| k.len()).sum::<usize>() + self.moves.len()
    }
}

/// A name that is fine as the name of a file.
pub fn file_stem(name: &str) -> String {
    let s: String = name.trim().chars().map(|c| if c.is_alphanumeric() || c == '-' || c == '_' { c } else { '_' }).collect();
    if s.is_empty() { "animation".into() } else { s }
}

/// Save an animation in a folder (made if it is not there). Returns the file.
pub fn save(folder: &Path, f: &TakeFile) -> Result<PathBuf, String> {
    std::fs::create_dir_all(folder).map_err(|e| format!("The folder could not be made: {e}"))?;
    let path = folder.join(format!("{}.{EXTENSION}", file_stem(&f.name)));
    let text = serde_json::to_string_pretty(f).map_err(|e| e.to_string())?;
    std::fs::write(&path, text).map_err(|e| format!("The file could not be saved: {e}"))?;
    Ok(path)
}

pub fn open(path: &Path) -> Result<TakeFile, String> {
    let text = std::fs::read_to_string(path).map_err(|e| format!("The file could not be read: {e}"))?;
    let f: TakeFile = serde_json::from_str(&text).map_err(|e| format!("The animation file is not readable: {e}"))?;
    if f.frames == 0 || f.fps <= 0.0 {
        return Err("The animation has no length.".into());
    }
    Ok(f)
}

/// Every animation of a folder, by name.
pub fn list(folder: &Path, source: Source) -> Vec<Entry> {
    let mut v: Vec<Entry> = std::fs::read_dir(folder)
        .map(|rd| {
            rd.filter_map(|e| e.ok())
                .filter(|e| e.path().extension().is_some_and(|x| x.eq_ignore_ascii_case(EXTENSION)))
                .filter_map(|e| open(&e.path()).ok().map(|f| Entry { name: f.name.clone(), path: e.path(), source, frames: f.frames, keys: f.key_count() }))
                .collect()
        })
        .unwrap_or_default();
    v.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));
    v
}

/// The animations of the user, and then the ones that came with the app.
pub fn all() -> Vec<Entry> {
    let mut v = list(&user_folder(), Source::Mine);
    v.extend(list(&built_in_folder(), Source::BuiltIn));
    v
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rig() -> Rig3 {
        Rig3::humanoid(0, [0.0; 3], [1.0, 2.0, 1.0])
    }

    #[test]
    fn an_animation_is_saved_by_the_names_of_the_bones_and_fits_another_rig_with_the_same_names() {
        let a = rig();
        let arm = a.bones.iter().position(|b| b.name == "leftUpperArm").unwrap();
        let mut t = Take::new("Wave hello", a.bones.len());
        t.frames = 20;
        t.set_key(arm, 0, [0.0; 3]);
        t.set_key(arm, 20, [0.0, 0.0, 70.0]);
        t.set_move(10, [0.0, 0.1, 0.0]);
        let f = TakeFile::from_take(&t, &a);
        assert_eq!(f.tracks.len(), 1);
        // another character: the same names in another order, one extra bone
        let mut b = Rig3::new(0);
        b.add_bone("extra", None, [0.0; 3], [0.0, 1.0, 0.0]);
        b.add_bone("LeftUpperArm", Some(0), [0.0, 1.0, 0.0], [0.0, 2.0, 0.0]);
        let (t2, found) = f.to_take(&b);
        assert_eq!(found, 1, "the case of a name does not matter");
        assert!(t2.has_key(1, 20) && !t2.has_key(0, 20));
        assert_eq!(t2.pose_at(20.0).rot[1], [0.0, 0.0, 70.0]);
        assert_eq!((t2.frames, t2.fps), (20, 30.0));
        assert_eq!(t2.pose_at(10.0).offset, [0.0, 0.1, 0.0]);
        // a rig without that bone finds nothing
        assert_eq!(f.to_take(&Rig3::new(0)).1, 0);
    }

    #[test]
    fn the_files_of_a_folder_are_saved_listed_and_read_back() {
        let dir = std::env::temp_dir().join("sevi_animlib_test");
        let _ = std::fs::remove_dir_all(&dir);
        let a = rig();
        let mut t = Take::new("Wave / hello?", a.bones.len());
        t.set_key(3, 5, [10.0, 0.0, 0.0]);
        let path = save(&dir, &TakeFile::from_take(&t, &a)).unwrap();
        assert!(path.file_name().unwrap().to_string_lossy().ends_with(".sevanim") && !path.to_string_lossy().contains('?'), "{path:?}");
        std::fs::write(dir.join("note.txt"), "x").unwrap();
        std::fs::write(dir.join("broken.sevanim"), "{").unwrap();
        let l = list(&dir, Source::Mine);
        assert_eq!(l.len(), 1, "only the good animation files");
        assert_eq!((l[0].name.as_str(), l[0].keys), ("Wave / hello?", 1));
        assert_eq!(open(&l[0].path).unwrap().tracks.len(), 1);
        assert!(open(&dir.join("broken.sevanim")).is_err());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn the_source_of_the_program_is_found_where_the_installer_takes_the_animations_from() {
        assert!(source_folder().is_some_and(|p| p.ends_with("animations")));
        assert_eq!(file_stem("  "), "animation");
    }
}
