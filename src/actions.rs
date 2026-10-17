// SPDX-License-Identifier: GPL-3.0-or-later
// The action table: every command that has a name, a place in the command palette (Ctrl+K) and a keyboard
// shortcut you can change. The keys, the palette and the Shortcuts page all read this one table.

use std::collections::BTreeMap;

use eframe::egui::{self, Event, Key, Modifiers};

pub struct ActionDef {
    pub id: &'static str,
    pub name: &'static str,
    pub group: &'static str,
    /// The key when you haven't changed it ("" = none), like "Ctrl+Shift+N".
    pub default: &'static str,
    /// Works while a text box has the keyboard (Ctrl+, and so on).
    pub typing_ok: bool,
}

const fn a(id: &'static str, name: &'static str, group: &'static str, default: &'static str) -> ActionDef {
    ActionDef { id, name, group, default, typing_ok: false }
}

const fn t(id: &'static str, name: &'static str, group: &'static str, default: &'static str) -> ActionDef {
    ActionDef { id, name, group, default, typing_ok: true }
}

pub const ACTIONS: &[ActionDef] = &[
    // tools
    a("tool.brush", "Brush", "Tools", "B"),
    a("tool.eraser", "Eraser", "Tools", "E"),
    a("tool.smudge", "Smudge", "Tools", "S"),
    a("tool.fill", "Fill", "Tools", "G"),
    a("tool.gradient", "Gradient", "Tools", "Shift+G"),
    a("tool.picker", "Color picker", "Tools", "I"),
    a("tool.select", "Selection", "Tools", "M"),
    a("tool.lasso", "Lasso", "Tools", "L"),
    a("tool.move", "Move and transform", "Tools", "V"),
    a("tool.pen", "Vector pen", "Tools", "P"),
    a("tool.shape", "Shapes", "Tools", "U"),
    a("tool.text", "Text", "Tools", "T"),
    a("tool.hand", "Hand", "Tools", "H"),
    a("tool.zoom", "Zoom", "Tools", "Z"),
    // painting
    a("color.swap", "Swap main and second color", "Painting", "X"),
    a("brush.smaller", "Smaller brush", "Painting", "["),
    a("brush.bigger", "Bigger brush", "Painting", "]"),
    // view
    a("view.fit", "Fit the page", "View", "Ctrl+0"),
    a("view.actual", "View at 100%", "View", "Ctrl+Alt+0"),
    a("view.zoom_in", "Zoom in", "View", "Ctrl+="),
    a("view.zoom_out", "Zoom out", "View", "Ctrl+-"),
    a("view.mirror", "Mirror the view", "View", "F"),
    // selection
    a("sel.all", "Select all", "Selection", "Ctrl+A"),
    a("sel.deselect", "Deselect", "Selection", "Ctrl+D"),
    a("sel.invert", "Invert selection", "Selection", "Ctrl+Shift+I"),
    a("sel.transform", "Transform the selection", "Selection", "Ctrl+T"),
    a("sel.fill", "Fill the selection", "Selection", "Alt+Backspace"),
    a("sel.quickmask", "Quick mask", "Selection", "Q"),
    // layers
    a("layer.new", "New layer", "Layers", "Ctrl+Shift+N"),
    a("layer.duplicate", "Duplicate layer", "Layers", "Ctrl+J"),
    a("layer.merge", "Merge down", "Layers", "Ctrl+E"),
    a("layer.group", "Put layer into a folder", "Layers", "Ctrl+G"),
    a("layer.clip", "Clipping mask on or off", "Layers", "Ctrl+Alt+G"),
    a("layer.quickbar", "Quick layer bar", "Layers", "Ctrl+L"),
    a("layer.search", "Search layers", "Layers", "Alt+L"),
    // files
    a("file.new", "New project", "File", "Ctrl+N"),
    a("file.open", "Open", "File", "Ctrl+O"),
    a("file.save", "Save", "File", "Ctrl+S"),
    a("file.save_as", "Save as", "File", "Ctrl+Shift+S"),
    a("file.manager", "Project manager", "File", "Ctrl+Shift+O"),
    a("file.export", "Export", "File", "Ctrl+Shift+E"),
    a("file.info", "Project info", "File", ""),
    // edit
    a("edit.undo", "Undo in this tab", "Edit", "Ctrl+Z"),
    a("edit.undo_all", "Undo everything (newest first)", "Edit", "Ctrl+Alt+Z"),
    a("edit.redo", "Redo", "Edit", "Ctrl+Y"),
    t("edit.history", "History", "Edit", "Ctrl+H"),
    // windows
    t("app.settings", "Settings", "Windows", "Ctrl+,"),
    t("app.palette", "Command palette", "Windows", "Ctrl+K"),
    a("win.recorder", "Recorder", "Windows", ""),
    a("win.signatures", "Signatures", "Windows", ""),
    a("win.fonts", "Font from my drawings", "Windows", ""),
    a("win.work", "Work history", "Windows", ""),
    // panels and workspaces
    a("panel.brush", "Show or hide the brush panel", "Panels", ""),
    a("panel.color", "Show or hide the color panel", "Panels", ""),
    a("panel.subview", "Show or hide the sub view", "Panels", ""),
    a("panel.layers", "Show or hide the layers panel", "Panels", ""),
    a("ws.next", "Next workspace", "Panels", ""),
    a("ws.prev", "Previous workspace", "Panels", ""),
    a("ws.reset", "Put the panels back as in this workspace", "Panels", ""),
    // tabs
    t("tab.1", "Go to the first tab", "Tabs", "Ctrl+1"),
    t("tab.2", "Go to the second tab", "Tabs", "Ctrl+2"),
    t("tab.3", "Go to the third tab", "Tabs", "Ctrl+3"),
    t("tab.4", "Go to the fourth tab", "Tabs", "Ctrl+4"),
    t("tab.5", "Go to the fifth tab", "Tabs", "Ctrl+5"),
    t("tab.6", "Go to the sixth tab", "Tabs", "Ctrl+6"),
];

pub fn def(id: &str) -> Option<&'static ActionDef> {
    ACTIONS.iter().find(|d| d.id == id)
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Shortcut {
    pub mods: Modifiers,
    pub key: Key,
}

fn key_text(k: Key) -> String {
    match k {
        Key::Num0 => "0".into(),
        Key::Num1 => "1".into(),
        Key::Num2 => "2".into(),
        Key::Num3 => "3".into(),
        Key::Num4 => "4".into(),
        Key::Num5 => "5".into(),
        Key::Num6 => "6".into(),
        Key::Num7 => "7".into(),
        Key::Num8 => "8".into(),
        Key::Num9 => "9".into(),
        Key::OpenBracket => "[".into(),
        Key::CloseBracket => "]".into(),
        Key::Comma => ",".into(),
        Key::Period => ".".into(),
        Key::Equals => "=".into(),
        Key::Minus => "-".into(),
        Key::Plus => "+".into(),
        Key::Slash => "/".into(),
        Key::Backslash => "\\".into(),
        Key::Semicolon => ";".into(),
        Key::Colon => ":".into(),
        Key::Quote => "'".into(),
        Key::Backtick => "`".into(),
        k => k.name().to_owned(),
    }
}

fn key_from_text(s: &str) -> Option<Key> {
    Some(match s {
        "0" => Key::Num0,
        "1" => Key::Num1,
        "2" => Key::Num2,
        "3" => Key::Num3,
        "4" => Key::Num4,
        "5" => Key::Num5,
        "6" => Key::Num6,
        "7" => Key::Num7,
        "8" => Key::Num8,
        "9" => Key::Num9,
        "[" => Key::OpenBracket,
        "]" => Key::CloseBracket,
        "," => Key::Comma,
        "." => Key::Period,
        "=" => Key::Equals,
        "-" => Key::Minus,
        "/" => Key::Slash,
        "\\" => Key::Backslash,
        ";" => Key::Semicolon,
        "'" => Key::Quote,
        "`" => Key::Backtick,
        other => Key::from_name(other)?,
    })
}

impl Shortcut {
    /// "Ctrl+Shift+N" → a shortcut. The key is the last part, so "Ctrl++" (plus) and "Ctrl+-" work.
    pub fn parse(text: &str) -> Option<Shortcut> {
        let text = text.trim();
        if text.is_empty() {
            return None;
        }
        let (mods_part, key_part) = match text.rfind('+') {
            Some(i) if i + 1 == text.len() => (&text[..i.saturating_sub(1)], "+"),
            Some(i) => (&text[..i], &text[i + 1..]),
            None => ("", text),
        };
        let mut mods = Modifiers::NONE;
        for m in mods_part.split('+').filter(|p| !p.is_empty()) {
            match m.trim().to_ascii_lowercase().as_str() {
                "ctrl" | "cmd" | "command" => mods |= Modifiers::COMMAND,
                "shift" => mods |= Modifiers::SHIFT,
                "alt" => mods |= Modifiers::ALT,
                _ => return None,
            }
        }
        let key = if key_part == "+" { Key::Plus } else { key_from_text(key_part)? };
        Some(Shortcut { mods, key })
    }

    pub fn text(&self) -> String {
        let mut s = String::new();
        if self.mods.command || self.mods.ctrl {
            s += "Ctrl+";
        }
        if self.mods.alt {
            s += "Alt+";
        }
        if self.mods.shift {
            s += "Shift+";
        }
        s + &key_text(self.key)
    }
}

/// The shortcut an action has now: your choice, or the default. None when it has none.
pub fn effective(overrides: &BTreeMap<String, String>, id: &str) -> Option<Shortcut> {
    match overrides.get(id) {
        Some(s) => Shortcut::parse(s),
        None => def(id).and_then(|d| Shortcut::parse(d.default)),
    }
}

pub fn effective_text(overrides: &BTreeMap<String, String>, id: &str) -> String {
    effective(overrides, id).map(|s| s.text()).unwrap_or_default()
}

/// Which other actions already use this shortcut.
pub fn conflicts(overrides: &BTreeMap<String, String>, id: &str, sc: Shortcut) -> Vec<&'static ActionDef> {
    ACTIONS.iter().filter(|d| d.id != id && effective(overrides, d.id) == Some(sc)).collect()
}

/// Give an action a new shortcut (None = no key). Other actions that had it lose it.
pub fn assign(overrides: &mut BTreeMap<String, String>, id: &str, sc: Option<Shortcut>) {
    if let Some(sc) = sc {
        for d in conflicts(overrides, id, sc) {
            overrides.insert(d.id.to_owned(), String::new());
        }
    }
    let default = def(id).map(|d| d.default).unwrap_or("");
    let text = sc.map(|s| s.text()).unwrap_or_default();
    if text == default {
        overrides.remove(id);
    } else {
        overrides.insert(id.to_owned(), text);
    }
}

/// Is this key pressed right now (the modifiers must be exactly the ones of the shortcut)? Uses the key up.
pub fn hit(i: &mut egui::InputState, overrides: &BTreeMap<String, String>, id: &str) -> bool {
    let Some(sc) = effective(overrides, id) else { return false };
    let pos = i.events.iter().position(|e| matches!(e, Event::Key { key, pressed: true, modifiers, .. } if *key == sc.key && modifiers.matches_exact(sc.mods)));
    match pos {
        Some(p) => {
            i.events.remove(p);
            true
        }
        None => false,
    }
}

/// The actions whose key is pressed, in the table's order (each key press is used once).
pub fn pressed(i: &mut egui::InputState, overrides: &BTreeMap<String, String>, ids: impl Iterator<Item = &'static ActionDef>) -> Vec<&'static str> {
    ids.filter(|d| hit(i, overrides, d.id)).map(|d| d.id).collect()
}

/// Words typed in the palette against an action: higher is better, None = no match.
pub fn match_score(query: &str, d: &ActionDef) -> Option<i32> {
    let q = query.trim().to_lowercase();
    if q.is_empty() {
        return Some(0);
    }
    let name = d.name.to_lowercase();
    let hay = format!("{name} {}", d.group.to_lowercase());
    if name.starts_with(&q) {
        return Some(100 - name.len() as i32);
    }
    if let Some(p) = hay.find(&q) {
        return Some(60 - p as i32);
    }
    // every typed word somewhere
    if q.split_whitespace().all(|w| hay.contains(w)) {
        return Some(30);
    }
    // letters in order inside the name ("mrg" finds "Merge down"); the tighter, the better
    let chars: Vec<char> = q.chars().filter(|c| !c.is_whitespace()).collect();
    let nc: Vec<char> = name.chars().collect();
    let mut best: Option<i32> = None;
    for start in 0..nc.len() {
        if nc[start] != chars[0] {
            continue;
        }
        let (mut k, mut last) = (1, start);
        for (j, c) in nc.iter().enumerate().skip(start + 1) {
            if k < chars.len() && *c == chars[k] {
                k += 1;
                last = j;
            }
        }
        if k == chars.len() {
            let span = (last - start + 1) as i32;
            best = Some(best.map_or(25 - span, |b| b.max(25 - span)));
        }
    }
    best.filter(|s| *s > 0).or_else(|| {
        // letters in order anywhere in name and group
        let mut it = hay.chars();
        chars.iter().all(|c| it.any(|h| h == *c)).then_some(1)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ctx_with(events: Vec<Event>) -> egui::Context {
        let ctx = egui::Context::default();
        let raw = egui::RawInput { events, ..Default::default() };
        let _ = ctx.run_ui(raw, |_| {});
        ctx
    }

    fn key(k: Key, m: Modifiers) -> Event {
        Event::Key { key: k, physical_key: Some(k), pressed: true, repeat: false, modifiers: m }
    }

    #[test]
    fn shortcuts_are_written_and_read_the_same_way() {
        for d in ACTIONS {
            if d.default.is_empty() {
                continue;
            }
            let s = Shortcut::parse(d.default).unwrap_or_else(|| panic!("{} has a default that can be read: {}", d.id, d.default));
            assert_eq!(s.text(), d.default, "{}", d.id);
        }
        assert_eq!(Shortcut::parse("Ctrl++"), Some(Shortcut { mods: Modifiers::COMMAND, key: Key::Plus }));
        assert_eq!(Shortcut::parse("ctrl+shift+n").unwrap().text(), "Ctrl+Shift+N");
        assert!(Shortcut::parse("Ctrl+Banana").is_none());
        assert!(Shortcut::parse("Hyper+N").is_none());
        assert!(Shortcut::parse("").is_none());
    }

    #[test]
    fn the_default_keys_never_collide_and_ids_are_unique() {
        let mut seen: Vec<(Shortcut, &str)> = Vec::new();
        for d in ACTIONS {
            if let Some(s) = Shortcut::parse(d.default) {
                if let Some(o) = seen.iter().find(|o| o.0 == s) {
                    panic!("{} and {} both use {}", d.id, o.1, d.default);
                }
                seen.push((s, d.id));
            }
        }
        let mut ids: Vec<&str> = ACTIONS.iter().map(|d| d.id).collect();
        ids.sort();
        ids.dedup();
        assert_eq!(ids.len(), ACTIONS.len());
    }

    #[test]
    fn changing_a_key_takes_it_from_the_other_action_and_default_means_no_entry() {
        let mut o = BTreeMap::new();
        let ctrl_j = Shortcut::parse("Ctrl+J").unwrap();
        assert_eq!(conflicts(&o, "layer.new", ctrl_j).iter().map(|d| d.id).collect::<Vec<_>>(), ["layer.duplicate"]);
        assign(&mut o, "layer.new", Some(ctrl_j));
        assert_eq!(effective_text(&o, "layer.new"), "Ctrl+J");
        assert_eq!(effective_text(&o, "layer.duplicate"), "", "it lost its key");
        assign(&mut o, "layer.duplicate", Shortcut::parse("Ctrl+Shift+N"));
        assign(&mut o, "layer.new", Shortcut::parse("Ctrl+Shift+N")); // swap back by hand
        assign(&mut o, "layer.duplicate", Shortcut::parse("Ctrl+J"));
        assert!(o.is_empty(), "back to the defaults: {o:?}");
        // taking a key away
        assign(&mut o, "tool.zoom", None);
        assert_eq!(effective(&o, "tool.zoom"), None);
        assign(&mut o, "tool.zoom", Shortcut::parse("Z"));
        assert!(o.is_empty());
    }

    #[test]
    fn a_key_only_counts_with_exactly_its_modifiers() {
        let o = BTreeMap::new();
        let ctx = ctx_with(vec![key(Key::N, Modifiers::COMMAND | Modifiers::SHIFT)]);
        // Ctrl+Shift+N is the new layer, not the new project
        assert!(!ctx.input_mut(|i| hit(i, &o, "file.new")));
        assert!(ctx.input_mut(|i| hit(i, &o, "layer.new")));
        assert!(!ctx.input_mut(|i| hit(i, &o, "layer.new")), "a key press is used once");
        // Alt+L is not the Lasso tool
        let ctx = ctx_with(vec![key(Key::L, Modifiers::ALT)]);
        assert!(!ctx.input_mut(|i| hit(i, &o, "tool.lasso")));
        assert!(ctx.input_mut(|i| hit(i, &o, "layer.search")));
        // a changed key works and the old one stops
        let mut o2 = BTreeMap::new();
        assign(&mut o2, "tool.brush", Shortcut::parse("Shift+B"));
        let ctx = ctx_with(vec![key(Key::B, Modifiers::SHIFT)]);
        assert!(ctx.input_mut(|i| hit(i, &o2, "tool.brush")));
        let ctx = ctx_with(vec![key(Key::B, Modifiers::NONE)]);
        assert!(!ctx.input_mut(|i| hit(i, &o2, "tool.brush")));
    }

    #[test]
    fn the_palette_finds_things_by_name_group_or_letters() {
        let best = |q: &str| ACTIONS.iter().filter_map(|d| match_score(q, d).map(|s| (s, d.id))).max_by_key(|x| x.0).map(|x| x.1);
        assert_eq!(best("merge"), Some("layer.merge"));
        assert_eq!(best("mrg"), Some("layer.merge"));
        assert_eq!(best("undo every"), Some("edit.undo_all"));
        assert!(match_score("zzzzqq", def("tool.brush").unwrap()).is_none());
        assert_eq!(match_score("", def("tool.brush").unwrap()), Some(0));
        assert_eq!(best("recorder"), Some("win.recorder"));
    }
}
