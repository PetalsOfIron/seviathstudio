// SPDX-License-Identifier: GPL-3.0-or-later
// Fonts for text layers: the two that come with the app, the ones installed on the computer, and the
// font files you add to a project (they are kept inside the project, so it opens the same elsewhere).

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex, OnceLock};

use serde::{Deserialize, Serialize};

/// A font file kept inside a project.
#[derive(Clone, PartialEq, Debug, Serialize, Deserialize)]
pub struct ProjectFont {
    pub name: String,
    /// The .ttf or .otf file, as base64 text.
    pub data: String,
}

pub const SANS: &str = "Sans (comes with the app)";
pub const MONO: &str = "Mono (comes with the app)";
pub const DEFAULT: &str = SANS;
/// Biggest font file we keep in a project or read from disk.
pub const MAX_FONT_BYTES: usize = 12 << 20;
/// Most fonts one project keeps.
pub const MAX_PROJECT_FONTS: usize = 12;

fn bundled(name: &str) -> Option<Arc<Vec<u8>>> {
    static B: OnceLock<HashMap<&'static str, Arc<Vec<u8>>>> = OnceLock::new();
    let b = B.get_or_init(|| {
        let defs = eframe::egui::FontDefinitions::default();
        let take = |key: &str| defs.font_data.get(key).map(|d| Arc::new(d.font.to_vec()));
        let mut m = HashMap::new();
        if let Some(d) = take("Ubuntu-Light") {
            m.insert(SANS, d);
        }
        if let Some(d) = take("Hack") {
            m.insert(MONO, d);
        }
        m
    });
    b.get(name).cloned()
}

/// Folders where the computer keeps its fonts.
fn system_dirs() -> Vec<PathBuf> {
    let mut v = Vec::new();
    if cfg!(windows) {
        if let Some(w) = std::env::var_os("WINDIR") {
            v.push(PathBuf::from(w).join("Fonts"));
        }
        if let Some(l) = std::env::var_os("LOCALAPPDATA") {
            v.push(PathBuf::from(l).join("Microsoft").join("Windows").join("Fonts"));
        }
    } else if cfg!(target_os = "macos") {
        v.push(PathBuf::from("/Library/Fonts"));
        v.push(PathBuf::from("/System/Library/Fonts"));
        if let Some(h) = dirs::home_dir() {
            v.push(h.join("Library").join("Fonts"));
        }
    } else {
        v.push(PathBuf::from("/usr/share/fonts"));
        v.push(PathBuf::from("/usr/local/share/fonts"));
        if let Some(h) = dirs::home_dir() {
            v.push(h.join(".fonts"));
            v.push(h.join(".local").join("share").join("fonts"));
        }
    }
    v
}

fn scan(dir: &std::path::Path, depth: usize, out: &mut Vec<(String, PathBuf)>) {
    let Ok(rd) = std::fs::read_dir(dir) else { return };
    for e in rd.flatten() {
        let p = e.path();
        if p.is_dir() {
            if depth < 3 {
                scan(&p, depth + 1, out);
            }
        } else if p.extension().is_some_and(|x| x.eq_ignore_ascii_case("ttf") || x.eq_ignore_ascii_case("otf")) {
            if let Some(stem) = p.file_stem() {
                out.push((stem.to_string_lossy().into_owned(), p));
            }
        }
    }
}

/// The fonts installed on this computer: (name, file). Looked up once.
pub fn system_fonts() -> &'static Vec<(String, PathBuf)> {
    static S: OnceLock<Vec<(String, PathBuf)>> = OnceLock::new();
    S.get_or_init(|| {
        let mut v = Vec::new();
        for d in system_dirs() {
            scan(&d, 0, &mut v);
        }
        v.sort_by_key(|f| f.0.to_lowercase());
        v.dedup_by(|a, b| a.0.eq_ignore_ascii_case(&b.0));
        v
    })
}

/// A font as bytes: a project font first, then the ones that come with the app, then the installed ones.
/// None when it can't be found or is not a usable font.
pub fn load(name: &str, project: &[ProjectFont]) -> Option<Arc<Vec<u8>>> {
    static CACHE: OnceLock<Mutex<HashMap<String, Option<Arc<Vec<u8>>>>>> = OnceLock::new();
    let cache = CACHE.get_or_init(|| Mutex::new(HashMap::new()));
    let found = if let Some(p) = project.iter().find(|f| f.name == name) {
        let key = format!("project:{}:{}", p.name, p.data.len());
        if let Some(c) = cache.lock().ok()?.get(&key) {
            return c.clone();
        }
        let bytes = crate::brush::tipimage::base64_decode(&p.data).filter(|b| usable(b)).map(Arc::new);
        cache.lock().ok()?.insert(key, bytes.clone());
        return bytes;
    } else if let Some(b) = bundled(name) {
        Some(b)
    } else {
        let key = format!("system:{name}");
        if let Some(c) = cache.lock().ok()?.get(&key) {
            return c.clone();
        }
        let bytes = system_fonts().iter().find(|f| f.0 == name).and_then(|f| std::fs::metadata(&f.1).ok().filter(|m| m.len() as usize <= MAX_FONT_BYTES).and_then(|_| std::fs::read(&f.1).ok())).filter(|b| usable(b)).map(Arc::new);
        cache.lock().ok()?.insert(key, bytes.clone());
        bytes
    };
    found
}

/// Is this the bytes of a font we can read?
pub fn usable(bytes: &[u8]) -> bool {
    bytes.len() <= MAX_FONT_BYTES && ttf_parser::Face::parse(bytes, 0).is_ok()
}

/// A font file picked by the user, ready to be kept in a project.
pub fn project_font_from_file(path: &std::path::Path) -> Result<ProjectFont, String> {
    let len = std::fs::metadata(path).map_err(|e| format!("Can't open the font: {e}"))?.len() as usize;
    if len > MAX_FONT_BYTES {
        return Err("That font file is too big.".into());
    }
    let bytes = std::fs::read(path).map_err(|e| format!("Can't read the font: {e}"))?;
    if !usable(&bytes) {
        return Err("That doesn't look like a .ttf or .otf font.".into());
    }
    let name = ttf_parser::Face::parse(&bytes, 0)
        .ok()
        .and_then(|f| f.names().into_iter().find(|n| n.name_id == ttf_parser::name_id::FULL_NAME && n.is_unicode()).and_then(|n| n.to_string()))
        .or_else(|| path.file_stem().map(|s| s.to_string_lossy().into_owned()))
        .unwrap_or_else(|| "My font".into());
    Ok(ProjectFont { name: name.chars().take(60).collect(), data: crate::brush::tipimage::base64_encode(&bytes) })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_fonts_that_come_with_the_app_load_and_know_latvian_letters() {
        for name in [SANS, MONO] {
            let b = load(name, &[]).unwrap_or_else(|| panic!("{name} loads"));
            let face = ttf_parser::Face::parse(&b, 0).unwrap();
            for c in "Aa0ābcčēģīķļņšūžĀČĒĢĪĶĻŅŠŪŽ".chars() {
                assert!(face.glyph_index(c).is_some(), "{name} has {c}");
            }
        }
        assert!(load("No such font", &[]).is_none());
    }

    #[test]
    fn project_fonts_win_and_are_checked() {
        let bytes = load(SANS, &[]).unwrap();
        let p = ProjectFont { name: "Mine".into(), data: crate::brush::tipimage::base64_encode(&bytes) };
        assert_eq!(load("Mine", std::slice::from_ref(&p)).unwrap().len(), bytes.len());
        assert!(load("Mine", &[]).is_none(), "only inside the project that has it");
        let bad = ProjectFont { name: "Broken".into(), data: crate::brush::tipimage::base64_encode(b"not a font") };
        assert!(load("Broken", &[bad]).is_none());
        // a picked file
        let dir = std::env::temp_dir().join(format!("seviath_font_{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("good.ttf"), &*bytes).unwrap();
        std::fs::write(dir.join("junk.ttf"), b"hello").unwrap();
        assert!(project_font_from_file(&dir.join("good.ttf")).is_ok());
        assert!(project_font_from_file(&dir.join("junk.ttf")).unwrap_err().contains("font"));
        assert!(project_font_from_file(&dir.join("nothing.ttf")).is_err());
        std::fs::remove_dir_all(&dir).ok();
    }
}
