// SPDX-License-Identifier: GPL-3.0-or-later
// The process log: what happened while a picture was made (strokes and time per layer, imports, references
// that were shown, AI tools later). Every entry carries the hash of the one before it, so changing or
// removing an old entry shows up. The log is kept inside the .seviath file.
//
// Honest limit: the app is open source, so this is tamper-EVIDENT, not forge-proof. Someone who rewrites
// the whole chain with their own program can make a new valid one. It shows careless edits, not a determined faker.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

/// The most entries one project keeps (the oldest are never dropped, new ones stop being added).
pub const MAX_ENTRIES: usize = 300_000;
/// Time between two events that counts as work; longer pauses are break time.
pub const MAX_GAP_SECS: f64 = 60.0;

#[derive(Clone, PartialEq, Debug, Serialize, Deserialize)]
pub struct Entry {
    /// Unix time, seconds.
    pub t: u64,
    /// stroke, import, reference, trace, text, edit, ai
    pub kind: String,
    pub layer: String,
    pub label: String,
    /// Seconds of work (or of showing a reference) this entry stands for.
    pub secs: f32,
    pub prev: String,
    pub hash: String,
}

#[derive(Clone, PartialEq, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct ProcessLog {
    pub entries: Vec<Entry>,
}

const GENESIS: &str = "genesis";

fn hash_of(prev: &str, t: u64, kind: &str, layer: &str, label: &str, secs: f32) -> String {
    let mut h = Sha256::new();
    h.update(format!("{prev}\n{t}\n{kind}\n{layer}\n{label}\n{:.2}", secs).as_bytes());
    h.finalize().iter().map(|b| format!("{b:02x}")).collect()
}

/// A history label → (kind, layer or picture name).
pub fn classify(label: &str) -> (&'static str, String) {
    let layer = label.rsplit_once(" · ").map(|(_, l)| l.to_owned()).unwrap_or_default();
    let kind = if label.starts_with("Import picture") {
        "import"
    } else if label.starts_with("Stroke ") || label.starts_with("Erase ") || label.starts_with("Smudge ") || label.starts_with("Vector line") || label.starts_with("Vector erase") {
        "stroke"
    } else if label.starts_with("Auto-trace") {
        "trace"
    } else if label.starts_with("Text") || label == "New text layer" {
        "text"
    } else if label.starts_with("Stamp signature") {
        "signature"
    } else {
        "edit"
    };
    (kind, layer)
}

#[derive(Default, Debug)]
pub struct Summary {
    pub entries: usize,
    pub work_secs: f32,
    pub strokes: usize,
    /// layer name, strokes, seconds
    pub layers: Vec<(String, usize, f32)>,
    pub imports: Vec<String>,
    /// reference name, seconds shown
    pub references: Vec<(String, f32)>,
    pub ai: Vec<String>,
    pub first: Option<u64>,
    pub last: Option<u64>,
}

impl ProcessLog {
    pub fn add(&mut self, t: u64, kind: &str, layer: &str, label: &str, secs: f32) {
        if self.entries.len() >= MAX_ENTRIES {
            return;
        }
        let clip = |s: &str, n: usize| -> String { s.chars().filter(|c| !c.is_control()).take(n).collect() };
        let (kind, layer, label) = (clip(kind, 16), clip(layer, 80), clip(label, 160));
        let secs = if secs.is_finite() { secs.clamp(0.0, 86_400.0) } else { 0.0 };
        let prev = self.entries.last().map(|e| e.hash.clone()).unwrap_or_else(|| GENESIS.to_owned());
        let hash = hash_of(&prev, t, &kind, &layer, &label, secs);
        self.entries.push(Entry { t, kind, layer, label, secs, prev, hash });
    }

    /// Ok when every entry matches its hash and points at the one before it.
    /// Err(i): the first entry that doesn't.
    pub fn verify(&self) -> Result<(), usize> {
        let mut prev = GENESIS.to_owned();
        for (i, e) in self.entries.iter().enumerate() {
            if e.prev != prev || e.hash != hash_of(&e.prev, e.t, &e.kind, &e.layer, &e.label, e.secs) {
                return Err(i);
            }
            prev = e.hash.clone();
        }
        Ok(())
    }

    /// Files come from outside: bound the size. (The chain is not touched, so changes still show.)
    pub fn sanitized(mut self) -> Self {
        self.entries.truncate(MAX_ENTRIES);
        for e in &mut self.entries {
            e.kind.truncate(16);
            e.layer = e.layer.chars().take(80).collect();
            e.label = e.label.chars().take(160).collect();
            e.hash.truncate(64);
            e.prev.truncate(64);
        }
        self
    }

    pub fn summary(&self) -> Summary {
        let mut s = Summary { entries: self.entries.len(), first: self.entries.first().map(|e| e.t), last: self.entries.last().map(|e| e.t), ..Summary::default() };
        let mut layers: BTreeMap<String, (usize, f32)> = BTreeMap::new();
        let mut refs: BTreeMap<String, f32> = BTreeMap::new();
        for e in &self.entries {
            match e.kind.as_str() {
                "reference" => *refs.entry(e.layer.clone()).or_default() += e.secs,
                "import" => {
                    s.imports.push(e.layer.clone());
                    s.work_secs += e.secs;
                }
                "ai" => {
                    if !s.ai.contains(&e.label) {
                        s.ai.push(e.label.clone());
                    }
                }
                k => {
                    s.work_secs += e.secs;
                    let l = layers.entry(if e.layer.is_empty() { "(no layer)".into() } else { e.layer.clone() }).or_default();
                    l.1 += e.secs;
                    if k == "stroke" {
                        s.strokes += 1;
                        l.0 += 1;
                    }
                }
            }
        }
        s.layers = layers.into_iter().map(|(n, (c, t))| (n, c, t)).collect();
        s.layers.sort_by(|a, b| b.2.total_cmp(&a.2));
        s.references = refs.into_iter().collect();
        s.references.sort_by(|a, b| b.1.total_cmp(&a.1));
        s
    }
}

/// "1 h 05 min", "12 min", "40 s"
pub fn duration_text(secs: f32) -> String {
    let s = secs.max(0.0).round() as u64;
    if s >= 3600 {
        format!("{} h {:02} min", s / 3600, (s % 3600) / 60)
    } else if s >= 60 {
        format!("{} min", s / 60)
    } else {
        format!("{s} s")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> ProcessLog {
        let mut l = ProcessLog::default();
        l.add(1000, "stroke", "Lines", "Stroke · Pen · Lines", 12.0);
        l.add(1020, "stroke", "Lines", "Stroke · Pen · Lines", 8.0);
        l.add(1100, "import", "photo.png", "Import picture · photo.png", 3.0);
        l.add(1200, "reference", "pose.jpg", "Reference shown", 95.0);
        l.add(1300, "stroke", "Color", "Stroke · Brush · Color", 30.0);
        l.add(1400, "edit", "Color", "Layer opacity · Color", 2.0);
        l
    }

    #[test]
    fn a_fresh_chain_checks_out_and_every_change_shows() {
        let l = sample();
        assert_eq!(l.verify(), Ok(()));
        assert_eq!(ProcessLog::default().verify(), Ok(()));
        // change a number
        let mut a = l.clone();
        a.entries[1].secs = 1.0;
        assert_eq!(a.verify(), Err(1));
        // change a label
        let mut b = l.clone();
        b.entries[3].label = "Something else".into();
        assert_eq!(b.verify(), Err(3));
        // remove an entry in the middle
        let mut c = l.clone();
        c.entries.remove(2);
        assert_eq!(c.verify(), Err(2));
        // swap two
        let mut d = l.clone();
        d.entries.swap(0, 1);
        assert_eq!(d.verify(), Err(0));
        // new entries after a removal at the end stay valid (the chain only protects what is before)
        let mut e = l.clone();
        e.entries.pop();
        assert_eq!(e.verify(), Ok(()));
    }

    #[test]
    fn the_summary_adds_things_up_per_layer() {
        let s = sample().summary();
        assert_eq!(s.entries, 6);
        assert_eq!(s.strokes, 3);
        assert!((s.work_secs - (12.0 + 8.0 + 3.0 + 30.0 + 2.0)).abs() < 0.01, "references are not work: {}", s.work_secs);
        assert_eq!(s.layers[0].0, "Color");
        assert_eq!(s.layers.iter().find(|l| l.0 == "Lines").map(|l| (l.1, l.2)), Some((2, 20.0)));
        assert_eq!(s.imports, ["photo.png"]);
        assert_eq!(s.references, [("pose.jpg".to_string(), 95.0)]);
        assert_eq!((s.first, s.last), (Some(1000), Some(1400)));
        assert!(s.ai.is_empty());
    }

    #[test]
    fn history_labels_are_sorted_into_kinds() {
        assert_eq!(classify("Stroke · Pen · Lines"), ("stroke", "Lines".into()));
        assert_eq!(classify("Erase · Soft · Sketch").0, "stroke");
        assert_eq!(classify("Vector line · Ink"), ("stroke", "Ink".into()));
        assert_eq!(classify("Import picture · photo").0, "import");
        assert_eq!(classify("Auto-trace · Sketch").0, "trace");
        assert_eq!(classify("New text layer").0, "text");
        assert_eq!(classify("Layer opacity · Color"), ("edit", "Color".into()));
        assert_eq!(classify("Select all"), ("edit", String::new()));
    }

    #[test]
    fn odd_values_are_cleaned_when_adding() {
        let mut l = ProcessLog::default();
        l.add(1, "stroke", "A\nB", &"x".repeat(500), f32::NAN);
        let e = &l.entries[0];
        assert_eq!(e.layer, "AB");
        assert_eq!(e.label.chars().count(), 160);
        assert_eq!(e.secs, 0.0);
        assert_eq!(l.verify(), Ok(()));
        assert_eq!(duration_text(4000.0), "1 h 06 min");
        assert_eq!(duration_text(125.0), "2 min");
        assert_eq!(duration_text(9.0), "9 s");
    }
}
