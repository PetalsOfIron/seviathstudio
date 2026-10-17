// SPDX-License-Identifier: GPL-3.0-or-later
// Where the panels of the Draw tab are: docked on the left or right, floating over the canvas, in their own
// window (for a second monitor) or hidden. A layout can be saved as a named workspace, switched with one
// click, exported to a file and imported again.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Mode {
    Docked,
    /// A window over the canvas.
    Floating,
    /// Its own window of the computer (can be moved to another screen).
    Window,
    Hidden,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Side {
    Left,
    Right,
}

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PanelId {
    Brush,
    Color,
    SubView,
    Layers,
}

impl PanelId {
    pub const ALL: [PanelId; 4] = [PanelId::Brush, PanelId::Color, PanelId::SubView, PanelId::Layers];

    pub fn title(self) -> &'static str {
        match self {
            PanelId::Brush => "Brush and tool options",
            PanelId::Color => "Color",
            PanelId::SubView => "Sub view",
            PanelId::Layers => "Layers",
        }
    }

    pub fn action_id(self) -> &'static str {
        match self {
            PanelId::Brush => "panel.brush",
            PanelId::Color => "panel.color",
            PanelId::SubView => "panel.subview",
            PanelId::Layers => "panel.layers",
        }
    }
}

#[derive(Clone, Copy, PartialEq, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct PanelState {
    pub mode: Mode,
    pub side: Side,
    /// Position in its column (small numbers first).
    pub order: i32,
    pub pos: [f32; 2],
    pub size: [f32; 2],
}

impl Default for PanelState {
    fn default() -> Self {
        Self { mode: Mode::Docked, side: Side::Right, order: 0, pos: [120.0, 120.0], size: [300.0, 420.0] }
    }
}

#[derive(Clone, PartialEq, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct Layout {
    pub panels: BTreeMap<PanelId, PanelState>,
    pub left_w: f32,
    pub right_w: f32,
}

impl Default for Layout {
    fn default() -> Self {
        let mut panels = BTreeMap::new();
        panels.insert(PanelId::Brush, PanelState { side: Side::Left, order: 0, size: [320.0, 600.0], ..PanelState::default() });
        panels.insert(PanelId::Color, PanelState { side: Side::Right, order: 0, size: [300.0, 380.0], ..PanelState::default() });
        panels.insert(PanelId::SubView, PanelState { side: Side::Right, order: 1, size: [300.0, 300.0], ..PanelState::default() });
        panels.insert(PanelId::Layers, PanelState { side: Side::Right, order: 2, size: [300.0, 520.0], ..PanelState::default() });
        Self { panels, left_w: 270.0, right_w: 290.0 }
    }
}

impl Layout {
    pub fn get(&self, id: PanelId) -> PanelState {
        self.panels.get(&id).copied().unwrap_or_default()
    }

    pub fn set_mode(&mut self, id: PanelId, mode: Mode) {
        self.panels.entry(id).or_default().mode = mode;
    }

    /// The docked panels of a side, top to bottom.
    pub fn column(&self, side: Side) -> Vec<PanelId> {
        let mut v: Vec<(i32, PanelId)> = PanelId::ALL.iter().filter(|id| self.get(**id).mode == Mode::Docked && self.get(**id).side == side).map(|id| (self.get(*id).order, *id)).collect();
        v.sort();
        v.into_iter().map(|x| x.1).collect()
    }

    /// Dock a panel at the bottom of a side.
    pub fn dock(&mut self, id: PanelId, side: Side) {
        let last = self.column(side).iter().filter(|o| **o != id).map(|o| self.get(*o).order).max().map(|m| m + 1).unwrap_or(0);
        let p = self.panels.entry(id).or_default();
        p.mode = Mode::Docked;
        p.side = side;
        p.order = last;
    }

    /// Move a docked panel one place up (-1) or down (+1) in its column.
    pub fn nudge(&mut self, id: PanelId, d: i32) {
        let side = self.get(id).side;
        let mut col = self.column(side);
        let Some(i) = col.iter().position(|c| *c == id) else { return };
        let j = i as i32 + d;
        if j < 0 || j as usize >= col.len() {
            return;
        }
        col.swap(i, j as usize);
        for (n, c) in col.iter().enumerate() {
            self.panels.entry(*c).or_default().order = n as i32;
        }
    }

    pub fn toggle_hidden(&mut self, id: PanelId) {
        if self.get(id).mode == Mode::Hidden {
            let s = self.get(id).side;
            self.dock(id, s);
        } else {
            self.set_mode(id, Mode::Hidden);
        }
    }

    /// Files come from outside: keep every number sane.
    pub fn sanitized(mut self) -> Self {
        let fix = |v: f32, lo: f32, hi: f32, d: f32| if v.is_finite() { v.clamp(lo, hi) } else { d };
        self.left_w = fix(self.left_w, 200.0, 700.0, 270.0);
        self.right_w = fix(self.right_w, 200.0, 700.0, 290.0);
        for id in PanelId::ALL {
            let p = self.panels.entry(id).or_default();
            p.order = p.order.clamp(-100, 100);
            for v in &mut p.pos {
                *v = fix(*v, -4000.0, 8000.0, 100.0);
            }
            p.size[0] = fix(p.size[0], 180.0, 3000.0, 300.0);
            p.size[1] = fix(p.size[1], 120.0, 3000.0, 400.0);
        }
        self
    }
}

/// The ready-made workspaces.
pub fn builtin() -> Vec<(&'static str, Layout)> {
    let drawing = Layout::default();
    let mut focus = Layout::default();
    for id in PanelId::ALL {
        focus.set_mode(id, Mode::Hidden);
    }
    let mut compact = Layout::default();
    compact.set_mode(PanelId::SubView, Mode::Hidden);
    compact.right_w = 250.0;
    compact.left_w = 240.0;
    let mut paint = Layout::default();
    // brush on the left, a tall sub view floating for references
    paint.panels.insert(PanelId::SubView, PanelState { mode: Mode::Floating, pos: [140.0, 90.0], size: [380.0, 380.0], ..PanelState::default() });
    vec![("Drawing", drawing), ("Compact", compact), ("Reference floating", paint), ("Focus", focus)]
}

/// Workspaces you saved, the working layout and which workspace it came from.
#[derive(Clone, PartialEq, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct Layouts {
    pub active: String,
    pub working: Layout,
    pub saved: Vec<(String, Layout)>,
}

impl Default for Layouts {
    fn default() -> Self {
        Self { active: "Drawing".into(), working: Layout::default(), saved: Vec::new() }
    }
}

pub const MAX_SAVED: usize = 24;

impl Layouts {
    pub fn names(&self) -> Vec<String> {
        builtin().into_iter().map(|b| b.0.to_owned()).chain(self.saved.iter().map(|s| s.0.clone())).collect()
    }

    pub fn find(&self, name: &str) -> Option<Layout> {
        self.saved.iter().find(|s| s.0 == name).map(|s| s.1.clone()).or_else(|| builtin().into_iter().find(|b| b.0 == name).map(|b| b.1))
    }

    pub fn is_builtin(name: &str) -> bool {
        builtin().iter().any(|b| b.0 == name)
    }

    /// Does the working layout still match the workspace it was started from?
    pub fn changed(&self) -> bool {
        self.find(&self.active).is_none_or(|l| l != self.working)
    }

    pub fn switch(&mut self, name: &str) -> bool {
        match self.find(name) {
            Some(l) => {
                self.working = l;
                self.active = name.to_owned();
                true
            }
            None => false,
        }
    }

    /// Keep the working layout under a name (it can't be the name of a ready-made one).
    pub fn save_as(&mut self, name: &str) -> Result<(), String> {
        let name: String = name.chars().filter(|c| !c.is_control()).collect::<String>().trim().chars().take(40).collect();
        if name.is_empty() {
            return Err("Give the workspace a name.".into());
        }
        if Self::is_builtin(&name) {
            return Err("That name is used by a ready-made workspace. Pick another one.".into());
        }
        if let Some(i) = self.saved.iter().position(|s| s.0 == name) {
            self.saved[i].1 = self.working.clone();
        } else if self.saved.len() >= MAX_SAVED {
            return Err("That is as many workspaces as can be kept.".into());
        } else {
            self.saved.push((name.clone(), self.working.clone()));
        }
        self.active = name;
        Ok(())
    }

    pub fn delete(&mut self, name: &str) {
        self.saved.retain(|s| s.0 != name);
        if self.active == name {
            self.active = "Drawing".into();
            self.working = Layout::default();
        }
    }

    pub fn sanitized(mut self) -> Self {
        self.working = self.working.sanitized();
        self.saved.truncate(MAX_SAVED);
        self.saved.retain(|s| !s.0.trim().is_empty() && !Self::is_builtin(&s.0));
        for s in &mut self.saved {
            s.0 = s.0.chars().filter(|c| !c.is_control()).take(40).collect();
            s.1 = s.1.clone().sanitized();
        }
        if self.find(&self.active).is_none() {
            self.active = "Drawing".into();
        }
        self
    }
}

// ---------- files ----------

fn store_path() -> PathBuf {
    if cfg!(test) {
        return std::env::temp_dir().join(format!("seviath_layouts_{}.json", std::process::id()));
    }
    dirs::config_dir().unwrap_or_else(std::env::temp_dir).join("SeviathStudio").join("layouts.json")
}

pub fn load() -> Layouts {
    let p = store_path();
    std::fs::read(&p).ok().filter(|b| b.len() < 4 << 20).and_then(|b| serde_json::from_slice::<Layouts>(&b).ok()).map(Layouts::sanitized).unwrap_or_default()
}

pub fn save(l: &Layouts) {
    let p = store_path();
    if let Some(d) = p.parent() {
        let _ = std::fs::create_dir_all(d);
    }
    if let Ok(t) = serde_json::to_string_pretty(l) {
        let _ = std::fs::write(p, t);
    }
}

/// One workspace as a file you can share.
pub fn export(path: &Path, name: &str, layout: &Layout) -> Result<(), String> {
    let file = serde_json::json!({ "format": "seviath-workspace", "name": name, "layout": layout });
    std::fs::write(path, serde_json::to_string_pretty(&file).map_err(|e| e.to_string())?).map_err(|e| format!("Couldn't save: {e}"))
}

pub fn import(path: &Path) -> Result<(String, Layout), String> {
    let len = std::fs::metadata(path).map_err(|e| e.to_string())?.len();
    if len > 1 << 20 {
        return Err("That file is too big for a workspace.".into());
    }
    let v: serde_json::Value = serde_json::from_slice(&std::fs::read(path).map_err(|e| e.to_string())?).map_err(|_| "That isn't a workspace file.".to_string())?;
    if v.get("format").and_then(|f| f.as_str()) != Some("seviath-workspace") {
        return Err("That isn't a workspace file.".into());
    }
    let name = v.get("name").and_then(|n| n.as_str()).unwrap_or("Imported").chars().filter(|c| !c.is_control()).take(40).collect::<String>();
    let layout: Layout = serde_json::from_value(v.get("layout").cloned().unwrap_or_default()).map_err(|_| "The layout in that file is damaged.".to_string())?;
    Ok((name, layout.sanitized()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_default_layout_matches_todays_screen() {
        let l = Layout::default();
        assert_eq!(l.column(Side::Left), [PanelId::Brush]);
        assert_eq!(l.column(Side::Right), [PanelId::Color, PanelId::SubView, PanelId::Layers]);
    }

    #[test]
    fn panels_move_between_sides_float_hide_and_come_back() {
        let mut l = Layout::default();
        l.dock(PanelId::Layers, Side::Left);
        assert_eq!(l.column(Side::Left), [PanelId::Brush, PanelId::Layers]);
        assert_eq!(l.column(Side::Right), [PanelId::Color, PanelId::SubView]);
        l.nudge(PanelId::Layers, -1);
        assert_eq!(l.column(Side::Left), [PanelId::Layers, PanelId::Brush]);
        l.nudge(PanelId::Layers, -1); // already first: nothing
        assert_eq!(l.column(Side::Left), [PanelId::Layers, PanelId::Brush]);
        l.set_mode(PanelId::Color, Mode::Floating);
        l.set_mode(PanelId::SubView, Mode::Window);
        assert!(l.column(Side::Right).is_empty());
        l.toggle_hidden(PanelId::Layers);
        assert_eq!(l.get(PanelId::Layers).mode, Mode::Hidden);
        l.toggle_hidden(PanelId::Layers);
        assert_eq!(l.get(PanelId::Layers).mode, Mode::Docked, "it comes back where it was");
        assert!(l.column(Side::Left).contains(&PanelId::Layers));
    }

    #[test]
    fn workspaces_switch_save_and_notice_changes() {
        let mut ls = Layouts::default();
        assert!(!ls.changed());
        ls.working.set_mode(PanelId::SubView, Mode::Hidden);
        assert!(ls.changed());
        assert!(ls.save_as("Drawing").is_err(), "a ready-made name");
        assert!(ls.save_as("   ").is_err());
        ls.save_as("Mine").unwrap();
        assert_eq!(ls.active, "Mine");
        assert!(!ls.changed());
        assert!(ls.switch("Focus"));
        assert!(ls.working.panels.values().all(|p| p.mode == Mode::Hidden));
        assert!(ls.switch("Mine"));
        assert_eq!(ls.working.get(PanelId::SubView).mode, Mode::Hidden);
        assert!(!ls.switch("Nothing"));
        assert_eq!(ls.names().len(), builtin().len() + 1);
        ls.delete("Mine");
        assert_eq!(ls.active, "Drawing");
        assert!(ls.saved.is_empty());
        for _ in 0..30 {
            let _ = ls.save_as(&format!("w{}", ls.saved.len()));
        }
        assert_eq!(ls.saved.len(), MAX_SAVED);
    }

    #[test]
    fn layouts_are_kept_in_a_file_and_odd_files_are_cleaned() {
        let mut ls = Layouts::default();
        ls.working.set_mode(PanelId::Color, Mode::Window);
        ls.save_as("Two screens").unwrap();
        save(&ls);
        let back = load();
        assert_eq!(back.active, "Two screens");
        assert_eq!(back.working.get(PanelId::Color).mode, Mode::Window);
        let mut bad = Layout::default();
        bad.left_w = f32::NAN;
        bad.panels.get_mut(&PanelId::Brush).unwrap().size = [f32::INFINITY, -5.0];
        let c = bad.sanitized();
        assert_eq!(c.left_w, 270.0);
        assert!(c.get(PanelId::Brush).size[0] <= 3000.0 && c.get(PanelId::Brush).size[1] >= 120.0);
        std::fs::remove_file(store_path()).ok();
    }

    #[test]
    fn a_workspace_can_be_shared_as_a_file() {
        let dir = std::env::temp_dir().join(format!("seviath_ws_{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let mut l = Layout::default();
        l.dock(PanelId::Layers, Side::Left);
        l.set_mode(PanelId::SubView, Mode::Floating);
        export(&dir.join("a.seviathworkspace"), "My layout", &l).unwrap();
        let (name, back) = import(&dir.join("a.seviathworkspace")).unwrap();
        assert_eq!(name, "My layout");
        assert_eq!(back, l);
        std::fs::write(dir.join("b.json"), b"{\"hello\":1}").unwrap();
        assert!(import(&dir.join("b.json")).is_err());
        std::fs::write(dir.join("c.json"), b"not json").unwrap();
        assert!(import(&dir.join("c.json")).is_err());
        assert!(import(&dir.join("missing.json")).is_err());
        std::fs::remove_dir_all(&dir).ok();
    }
}
