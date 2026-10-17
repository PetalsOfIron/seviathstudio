// SPDX-License-Identifier: GPL-3.0-or-later
// Undo history. Every step keeps the document as it was before and after. Tiles are shared
// copy-on-write, so a step only costs memory for the tiles it really changed.
// Steps remember their tab, so "undo this tab" and "undo everything" can both work, and any
// single step can be undone from the middle (only what that step changed is put back).

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use eframe::egui::Color32;

use crate::doc::{Document, Layer, LayerId, LayerKind, TILE};
use crate::tabs::Tab;

#[derive(Clone)]
pub struct DocState {
    pub layers: Vec<Layer>,
    pub paper: Color32,
    pub selection: Option<crate::select::Selection>,
    pub presets: Vec<crate::layerfilter::Preset>,
    pub meta: crate::meta::ProjectMeta,
}

impl Document {
    pub fn snapshot(&self) -> DocState {
        DocState { layers: self.layers.clone(), paper: self.paper, selection: self.selection.clone(), presets: self.filter_presets.clone(), meta: self.meta.clone() }
    }

    pub fn restore(&mut self, s: &DocState) {
        self.live = None;
        self.layers = s.layers.clone();
        self.paper = s.paper;
        self.selection = s.selection.clone();
        self.filter_presets = s.presets.clone();
        self.meta = s.meta.clone();
        // fresh revisions so cached thumbnails never show an old picture
        for l in &mut self.layers {
            l.rev = l.rev.wrapping_add(1_000_003);
        }
        self.mark_all_dirty();
    }

    /// Same picture and layers as `s` (folder open/closed doesn't count).
    pub fn same_as(&self, s: &DocState) -> bool {
        sel_id(&self.selection) == sel_id(&s.selection)
            && self.filter_presets == s.presets
            && self.meta == s.meta
            && self.paper == s.paper
            && self.layers.len() == s.layers.len() && self.layers.iter().zip(&s.layers).all(|(a, b)| layer_same(a, b))
    }
}

fn sel_id(s: &Option<crate::select::Selection>) -> u64 {
    s.as_ref().map(|s| s.id).unwrap_or(0)
}

fn meta_same(a: &Layer, b: &Layer) -> bool {
    a.id == b.id
        && a.parent == b.parent
        && a.name == b.name
        && a.kind == b.kind
        && a.visible == b.visible
        && a.locked == b.locked
        && a.lock_alpha == b.lock_alpha
        && a.clip == b.clip
        && a.tag == b.tag
        && a.fav == b.fav
        && crate::vector::same(&a.vector, &b.vector)
        && a.opacity == b.opacity
        && a.blend == b.blend
}

fn tiles_same(a: &Layer, b: &Layer) -> bool {
    a.tiles.len() == b.tiles.len() && a.tiles.iter().all(|(k, t)| b.tiles.get(k).is_some_and(|u| Arc::ptr_eq(t, u)))
}

fn layer_same(a: &Layer, b: &Layer) -> bool {
    meta_same(a, b) && tiles_same(a, b)
}

/// A short name for what changed between two states.
pub fn describe(before: &DocState, after: &DocState) -> String {
    let ids_b: HashSet<LayerId> = before.layers.iter().map(|l| l.id).collect();
    let ids_a: HashSet<LayerId> = after.layers.iter().map(|l| l.id).collect();
    let added: Vec<&Layer> = after.layers.iter().filter(|l| !ids_b.contains(&l.id)).collect();
    let removed: Vec<&Layer> = before.layers.iter().filter(|l| !ids_a.contains(&l.id)).collect();
    if !added.is_empty() && removed.is_empty() {
        let l = added[0];
        return if l.name.ends_with(" copy") {
            "Duplicate layer".into()
        } else if l.kind == LayerKind::Group && added.len() == 1 && after.layers.iter().any(|c| c.parent == Some(l.id)) {
            "Put into folder".into()
        } else if l.kind == LayerKind::Group {
            "New folder".into()
        } else if l.kind == LayerKind::Vector {
            "New vector layer".into()
        } else {
            "New layer".into()
        };
    }
    if !removed.is_empty() {
        let merged = after.layers.iter().any(|a| before.layers.iter().any(|b| b.id == a.id && !tiles_same(a, b)));
        return if merged { "Merge down".into() } else { format!("Delete {}", removed[0].name) };
    }
    if before.paper != after.paper {
        return "Paper color".into();
    }
    if before.meta != after.meta && before.layers.iter().zip(&after.layers).all(|(a, b)| layer_same(a, b)) {
        return if before.meta.guides != after.meta.guides && before.meta.dpi == after.meta.dpi { "Guides".into() } else { "Project info".into() };
    }
    if before.presets != after.presets && before.layers.iter().zip(&after.layers).all(|(a, b)| layer_same(a, b)) {
        return "Layer filter preset".into();
    }
    if sel_id(&before.selection) != sel_id(&after.selection) && before.layers.iter().zip(&after.layers).all(|(a, b)| layer_same(a, b)) {
        return if after.selection.is_none() { "Deselect".into() } else { "Selection".into() };
    }
    let order_b: Vec<(LayerId, Option<LayerId>)> = before.layers.iter().map(|l| (l.id, l.parent)).collect();
    let order_a: Vec<(LayerId, Option<LayerId>)> = after.layers.iter().map(|l| (l.id, l.parent)).collect();
    if order_a != order_b {
        return "Move layer".into();
    }
    for (b, a) in before.layers.iter().zip(&after.layers) {
        let what = if !tiles_same(a, b) {
            "Paint"
        } else if a.name != b.name {
            "Rename layer"
        } else if a.visible != b.visible {
            if a.visible { "Show layer" } else { "Hide layer" }
        } else if a.opacity != b.opacity {
            "Layer opacity"
        } else if a.blend != b.blend {
            "Blend mode"
        } else if a.clip != b.clip {
            "Clipping mask"
        } else if a.locked != b.locked || a.lock_alpha != b.lock_alpha {
            "Lock"
        } else if a.tag != b.tag {
            "Color label"
        } else if a.fav != b.fav {
            "Favorite"
        } else {
            continue;
        };
        return format!("{what} · {}", a.name);
    }
    "Change".into()
}

pub struct Entry {
    pub label: String,
    pub tab: Tab,
    pub before: DocState,
    pub after: DocState,
    pub time: f64,
    pub bytes: usize,
}

pub struct History {
    pub entries: Vec<Entry>,
    /// How many entries are applied (entries[cursor..] are undone and can be redone).
    pub cursor: usize,
    pub max_steps: usize,
    pub max_bytes: usize,
    /// Changes every time the document changes through history (push/undo/redo/jump);
    /// "unsaved changes" = generation differs from the one at the last save.
    pub generation: u64,
}

/// Result of "undo this tab" when other tabs have newer steps.
pub enum TabUndo {
    Done,
    Nothing,
    /// The step is older than steps of other tabs: ask the user (undo both / only this / cancel).
    NeedsChoice(usize),
}

impl Default for History {
    fn default() -> Self {
        Self { entries: Vec::new(), cursor: 0, max_steps: 200, max_bytes: 2048 << 20, generation: 0 }
    }
}

/// Memory used by tiles that changed in this step.
fn changed_bytes(before: &DocState, after: &DocState) -> usize {
    let mut n = 0;
    let old: HashMap<LayerId, &Layer> = before.layers.iter().map(|l| (l.id, l)).collect();
    for a in &after.layers {
        for (k, t) in &a.tiles {
            let same = old.get(&a.id).and_then(|b| b.tiles.get(k)).is_some_and(|u| Arc::ptr_eq(t, u));
            if !same {
                n += TILE * TILE * 4;
            }
        }
    }
    n + 1024
}

impl History {
    pub fn push(&mut self, label: String, tab: Tab, before: DocState, after: DocState, time: f64) {
        self.entries.truncate(self.cursor); // a new change drops the redo steps
        let bytes = changed_bytes(&before, &after);
        self.entries.push(Entry { label, tab, before, after, time, bytes });
        self.cursor = self.entries.len();
        self.generation += 1;
        self.trim();
    }

    fn trim(&mut self) {
        let mut total: usize = self.entries.iter().map(|e| e.bytes).sum();
        while self.entries.len() > 1 && (self.entries.len() > self.max_steps || total > self.max_bytes) {
            total -= self.entries[0].bytes;
            self.entries.remove(0);
            self.cursor = self.cursor.saturating_sub(1);
        }
    }

    /// Forget all steps (a different document was opened). The generation keeps counting.
    pub fn clear(&mut self) {
        self.entries.clear();
        self.cursor = 0;
        self.generation += 1;
    }

    pub fn set_limits(&mut self, steps: usize, megabytes: usize) {
        self.max_steps = steps.max(1);
        self.max_bytes = megabytes.max(16) << 20;
        self.trim();
    }

    pub fn can_undo(&self) -> bool {
        self.cursor > 0
    }
    pub fn can_redo(&self) -> bool {
        self.cursor < self.entries.len()
    }

    /// Undo the newest step, whatever tab it belongs to.
    pub fn undo(&mut self, doc: &mut Document) -> Option<&str> {
        if self.cursor == 0 {
            return None;
        }
        self.cursor -= 1;
        doc.restore(&self.entries[self.cursor].before);
        self.generation += 1;
        Some(&self.entries[self.cursor].label)
    }

    pub fn redo(&mut self, doc: &mut Document) -> Option<&str> {
        if self.cursor >= self.entries.len() {
            return None;
        }
        doc.restore(&self.entries[self.cursor].after);
        self.cursor += 1;
        self.generation += 1;
        Some(&self.entries[self.cursor - 1].label)
    }

    /// Go to the state right after `n` steps (0 = before the first kept step).
    pub fn jump_to(&mut self, doc: &mut Document, n: usize) {
        let n = n.min(self.entries.len());
        if self.entries.is_empty() {
            return;
        }
        let state = if n == 0 { &self.entries[0].before } else { &self.entries[n - 1].after };
        doc.restore(state);
        self.cursor = n;
        self.generation += 1;
    }

    /// Undo the newest step of `tab`.
    pub fn undo_tab(&mut self, doc: &mut Document, tab: Tab) -> TabUndo {
        let Some(i) = (0..self.cursor).rev().find(|&i| self.entries[i].tab == tab) else { return TabUndo::Nothing };
        if i + 1 == self.cursor {
            self.undo(doc);
            TabUndo::Done
        } else {
            TabUndo::NeedsChoice(i)
        }
    }

    /// Undo only step `i`, keeping everything done after it. Recorded as a new step.
    /// Returns false when some of it couldn't be put back because later steps changed the same thing.
    pub fn undo_only(&mut self, doc: &mut Document, i: usize, tab: Tab, time: f64) -> bool {
        if i >= self.cursor {
            return true;
        }
        let cur = doc.snapshot();
        let (out, clean) = revert_only(&cur, &self.entries[i].before, &self.entries[i].after);
        let label = format!("Undo only: {}", self.entries[i].label);
        doc.restore(&out);
        let after = doc.snapshot();
        self.push(label, tab, cur, after, time);
        clean
    }
}

/// `cur` with just the changes of one step (before → after) taken back.
pub fn revert_only(cur: &DocState, before: &DocState, after: &DocState) -> (DocState, bool) {
    let mut out = cur.clone();
    let mut clean = true;
    if sel_id(&before.selection) != sel_id(&after.selection) {
        if sel_id(&cur.selection) == sel_id(&after.selection) {
            out.selection = before.selection.clone();
        } else {
            clean = false;
        }
    }
    let b_ids: HashSet<LayerId> = before.layers.iter().map(|l| l.id).collect();
    let a_ids: HashSet<LayerId> = after.layers.iter().map(|l| l.id).collect();
    // layers the step added: remove them again
    out.layers.retain(|l| !(a_ids.contains(&l.id) && !b_ids.contains(&l.id)));
    // layers the step removed: put them back where they were
    for (pos, l) in before.layers.iter().enumerate() {
        if !a_ids.contains(&l.id) && !out.layers.iter().any(|o| o.id == l.id) {
            let at = pos.min(out.layers.len().saturating_sub(1));
            out.layers.insert(at, l.clone());
        }
    }
    // changed settings and pixels of layers that exist in both
    for o in &mut out.layers {
        let (Some(b), Some(a)) = (before.layers.iter().find(|l| l.id == o.id), after.layers.iter().find(|l| l.id == o.id)) else {
            continue;
        };
        macro_rules! field {
            ($f:ident) => {
                if b.$f != a.$f {
                    if o.$f == a.$f {
                        o.$f = b.$f.clone();
                    } else {
                        clean = false;
                    }
                }
            };
        }
        field!(name);
        field!(visible);
        field!(locked);
        field!(lock_alpha);
        field!(clip);
        field!(tag);
        field!(opacity);
        field!(blend);
        if !crate::vector::same(&b.vector, &a.vector) {
            if crate::vector::same(&o.vector, &a.vector) {
                o.vector = b.vector.clone();
            } else {
                clean = false;
            }
        }
        let keys: HashSet<_> = b.tiles.keys().chain(a.tiles.keys()).copied().collect();
        for k in keys {
            let (tb, ta, tc) = (b.tiles.get(&k), a.tiles.get(&k), o.tiles.get(&k));
            let changed = match (tb, ta) {
                (Some(x), Some(y)) => !Arc::ptr_eq(x, y),
                (None, None) => false,
                _ => true,
            };
            if !changed {
                continue;
            }
            let untouched_since = match (ta, tc) {
                (Some(x), Some(y)) => Arc::ptr_eq(x, y),
                (None, None) => true,
                _ => false,
            };
            if untouched_since {
                match tb {
                    Some(t) => {
                        o.tiles.insert(k, t.clone());
                    }
                    None => {
                        o.tiles.remove(&k);
                    }
                }
                o.rev = o.rev.wrapping_add(1);
            } else {
                clean = false;
            }
        }
    }
    if before.paper != after.paper {
        if cur.paper == after.paper {
            out.paper = before.paper;
        } else {
            clean = false;
        }
    }
    (out, clean)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::brush::BrushSettings;
    use crate::brush::engine::{Sample, Stroke};

    fn none(_: i32, _: i32) -> Option<Color32> {
        None
    }

    fn stroke(doc: &mut Document, li: usize, y: f32) {
        let s = BrushSettings { size: 20.0, stabilize: 0.0, ..BrushSettings::default() };
        let mut st = Stroke::begin(doc, li, &s, Color32::RED, false, 1.0, false, Sample { x: 20.0, y, p: 1.0 }, &none);
        st.add(doc, &[Sample { x: 200.0, y, p: 1.0 }], &none);
        st.finish(doc, &none);
    }

    fn record(h: &mut History, doc: &mut Document, f: impl FnOnce(&mut Document)) {
        let before = doc.snapshot();
        f(doc);
        let after = doc.snapshot();
        let label = describe(&before, &after);
        h.push(label, Tab::Draw, before, after, 0.0);
    }

    #[test]
    fn undo_redo_stroke_and_layer() {
        let mut doc = Document::new(300, 300);
        let mut h = History::default();
        let empty = doc.snapshot();
        record(&mut h, &mut doc, |d| stroke(d, 0, 50.0));
        assert_eq!(h.entries[0].label, "Paint · Layer 1");
        let painted = doc.snapshot();
        record(&mut h, &mut doc, |d| {
            let l = d.new_layer("Layer 2", LayerKind::Raster);
            let top = d.layers[0].id;
            d.insert_layer(l, top);
        });
        assert_eq!(h.entries[1].label, "New layer");
        assert_eq!(doc.layers.len(), 3);
        h.undo(&mut doc);
        assert!(doc.same_as(&painted));
        h.undo(&mut doc);
        assert!(doc.same_as(&empty));
        assert!(!h.can_undo());
        h.redo(&mut doc);
        h.redo(&mut doc);
        assert_eq!(doc.layers.len(), 3);
        assert!(!h.can_redo());
    }

    #[test]
    fn painting_does_not_touch_older_steps() {
        let mut doc = Document::new(300, 300);
        let mut h = History::default();
        record(&mut h, &mut doc, |d| stroke(d, 0, 50.0));
        let after_first = h.entries[0].after.clone();
        record(&mut h, &mut doc, |d| stroke(d, 0, 52.0)); // same tile, painted again
        // copy-on-write: the first step's picture is untouched
        let l = &after_first.layers[0];
        let px = l.tiles[&(0, 0)][52 * TILE + 100];
        assert_eq!(px, Color32::RED);
        h.undo(&mut doc);
        assert!(doc.same_as(&after_first));
    }

    #[test]
    fn undo_only_a_middle_step() {
        let mut doc = Document::new(600, 300);
        let mut h = History::default();
        record(&mut h, &mut doc, |d| stroke(d, 0, 30.0)); // tile row 0
        record(&mut h, &mut doc, |d| d.layers[0].opacity = 50.0);
        assert!(h.undo_only(&mut doc, 0, Tab::Draw, 0.0));
        // the stroke is gone, the opacity change stays
        assert!(doc.layers[0].tiles.is_empty());
        assert_eq!(doc.layers[0].opacity, 50.0);
        assert_eq!(h.entries.len(), 3);
        // and that is undoable too
        h.undo(&mut doc);
        assert!(!doc.layers[0].tiles.is_empty());
    }

    #[test]
    fn tab_undo_asks_when_other_tabs_are_newer() {
        let mut doc = Document::new(300, 300);
        let mut h = History::default();
        record(&mut h, &mut doc, |d| stroke(d, 0, 50.0));
        let b = doc.snapshot();
        h.push("Rig thing".into(), Tab::Rig, b.clone(), b, 0.0);
        assert!(matches!(h.undo_tab(&mut doc, Tab::Draw), TabUndo::NeedsChoice(0)));
        assert!(matches!(h.undo_tab(&mut doc, Tab::Physics), TabUndo::Nothing));
        assert!(matches!(h.undo_tab(&mut doc, Tab::Rig), TabUndo::Done));
    }

    #[test]
    fn limits_drop_oldest() {
        let mut doc = Document::new(300, 300);
        let mut h = History::default();
        h.set_limits(3, 1024);
        for i in 0..6 {
            record(&mut h, &mut doc, |d| d.layers[0].opacity = 10.0 + i as f32);
        }
        assert_eq!(h.entries.len(), 3);
        assert_eq!(h.cursor, 3);
        h.jump_to(&mut doc, 0);
        assert_eq!(doc.layers[0].opacity, 12.0);
    }
}
