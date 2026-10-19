// SPDX-License-Identifier: GPL-3.0-or-later
// Parts: what each layer of the drawing is (left eye, front hair, sleeve…), so rigging knows what to move.
// Tags can be suggested from the layer name, its shape and its place on the page, left and right halves are
// paired up, a checklist says what is missing, and a set of tags can be kept as a template.
//
// "L" always means the character's own left, which is on the RIGHT side of the picture.

use std::path::{Path, PathBuf};

use eframe::egui::Color32;
use serde::{Deserialize, Serialize};

use crate::doc::{Layer, LayerId, LayerKind};
use crate::files::layer_bounds;

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Part {
    Face,
    EyeL,
    EyeR,
    EyeballL,
    EyeballR,
    BrowL,
    BrowR,
    Nose,
    Mouth,
    MouthInside,
    EarL,
    EarR,
    Blush,
    HairFront,
    HairSideL,
    HairSideR,
    HairBack,
    Neck,
    Torso,
    ArmL,
    ArmR,
    HandL,
    HandR,
    LegL,
    LegR,
    Clothing,
    Accessory,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Group {
    Face,
    Hair,
    Body,
    Clothing,
    Accessory,
}

impl Group {
    pub const ALL: [Group; 5] = [Group::Face, Group::Hair, Group::Body, Group::Clothing, Group::Accessory];

    pub fn label(self) -> &'static str {
        match self {
            Group::Face => "Face",
            Group::Hair => "Hair",
            Group::Body => "Body",
            Group::Clothing => "Clothing",
            Group::Accessory => "Accessories",
        }
    }

    fn rgb(self) -> [u8; 3] {
        match self {
            Group::Face => [240, 120, 150],
            Group::Hair => [120, 110, 240],
            Group::Body => [70, 190, 150],
            Group::Clothing => [235, 170, 70],
            Group::Accessory => [90, 180, 235],
        }
    }
}

impl Part {
    pub const ALL: [Part; 27] = [
        Part::Face,
        Part::EyeL,
        Part::EyeR,
        Part::EyeballL,
        Part::EyeballR,
        Part::BrowL,
        Part::BrowR,
        Part::Nose,
        Part::Mouth,
        Part::MouthInside,
        Part::EarL,
        Part::EarR,
        Part::Blush,
        Part::HairFront,
        Part::HairSideL,
        Part::HairSideR,
        Part::HairBack,
        Part::Neck,
        Part::Torso,
        Part::ArmL,
        Part::ArmR,
        Part::HandL,
        Part::HandR,
        Part::LegL,
        Part::LegR,
        Part::Clothing,
        Part::Accessory,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Part::Face => "Face base",
            Part::EyeL => "Eye L",
            Part::EyeballL => "Eyeball L",
            Part::EyeballR => "Eyeball R",
            Part::EyeR => "Eye R",
            Part::BrowL => "Brow L",
            Part::BrowR => "Brow R",
            Part::Nose => "Nose",
            Part::Mouth => "Mouth",
            Part::MouthInside => "Inside of mouth",
            Part::EarL => "Ear L",
            Part::EarR => "Ear R",
            Part::Blush => "Blush",
            Part::HairFront => "Hair front",
            Part::HairSideL => "Hair side L",
            Part::HairSideR => "Hair side R",
            Part::HairBack => "Hair back",
            Part::Neck => "Neck",
            Part::Torso => "Torso",
            Part::ArmL => "Arm L",
            Part::ArmR => "Arm R",
            Part::HandL => "Hand L",
            Part::HandR => "Hand R",
            Part::LegL => "Leg L",
            Part::LegR => "Leg R",
            Part::Clothing => "Clothing",
            Part::Accessory => "Accessory",
        }
    }

    pub fn group(self) -> Group {
        match self {
            Part::Face | Part::EyeL | Part::EyeR | Part::EyeballL | Part::EyeballR | Part::BrowL | Part::BrowR | Part::Nose | Part::Mouth | Part::MouthInside | Part::EarL | Part::EarR | Part::Blush => Group::Face,
            Part::HairFront | Part::HairSideL | Part::HairSideR | Part::HairBack => Group::Hair,
            Part::Neck | Part::Torso | Part::ArmL | Part::ArmR | Part::HandL | Part::HandR | Part::LegL | Part::LegR => Group::Body,
            Part::Clothing => Group::Clothing,
            Part::Accessory => Group::Accessory,
        }
    }

    /// The other half of a left / right pair.
    pub fn mirror(self) -> Option<Part> {
        Some(match self {
            Part::EyeL => Part::EyeR,
            Part::EyeR => Part::EyeL,
            Part::EyeballL => Part::EyeballR,
            Part::EyeballR => Part::EyeballL,
            Part::BrowL => Part::BrowR,
            Part::BrowR => Part::BrowL,
            Part::EarL => Part::EarR,
            Part::EarR => Part::EarL,
            Part::HairSideL => Part::HairSideR,
            Part::HairSideR => Part::HairSideL,
            Part::ArmL => Part::ArmR,
            Part::ArmR => Part::ArmL,
            Part::HandL => Part::HandR,
            Part::HandR => Part::HandL,
            Part::LegL => Part::LegR,
            Part::LegR => Part::LegL,
            _ => return None,
        })
    }

    pub fn is_left(self) -> bool {
        matches!(self, Part::EyeL | Part::EyeballL | Part::BrowL | Part::EarL | Part::HairSideL | Part::ArmL | Part::HandL | Part::LegL)
    }

    /// The color code on the model: one color per group, the right half a little darker.
    pub fn color(self) -> Color32 {
        let [r, g, b] = self.group().rgb();
        let k = if self.mirror().is_some() && !self.is_left() { 0.72 } else { 1.0 };
        Color32::from_rgb((r as f32 * k) as u8, (g as f32 * k) as u8, (b as f32 * k) as u8)
    }
}

// ---------- suggestions ----------

#[derive(Clone, PartialEq, Debug)]
pub struct Suggestion {
    pub layer: LayerId,
    pub part: Part,
    /// 0..1: how sure it is (names are sure, shapes only hint).
    pub confidence: f32,
    pub reason: String,
}

/// Does this layer hold the drawing of a part (a pixel or vector layer, not a folder or an effect)?
pub fn is_part_layer(l: &Layer) -> bool {
    matches!(l.kind, LayerKind::Raster | LayerKind::Vector) && l.text.is_none()
}

fn tokens(name: &str) -> Vec<String> {
    // split "eyeL" and "Eye_L.001" into words
    let mut out = Vec::new();
    let mut cur = String::new();
    let mut prev_lower = false;
    for c in name.chars() {
        if c.is_alphabetic() {
            if c.is_uppercase() && prev_lower && !cur.is_empty() {
                out.push(std::mem::take(&mut cur));
            }
            cur.extend(c.to_lowercase());
            prev_lower = c.is_lowercase();
        } else {
            if !cur.is_empty() {
                out.push(std::mem::take(&mut cur));
            }
            prev_lower = false;
        }
    }
    if !cur.is_empty() {
        out.push(cur);
    }
    out
}

#[derive(Clone, Copy, PartialEq, Debug)]
enum Side {
    L,
    R,
}

fn side_of(toks: &[String]) -> Option<Side> {
    for t in toks {
        match t.as_str() {
            "l" | "left" | "kreisais" | "kreisa" | "kreisie" => return Some(Side::L),
            "r" | "right" | "labais" | "laba" | "labie" => return Some(Side::R),
            _ => {}
        }
    }
    None
}

fn has(toks: &[String], words: &[&str]) -> bool {
    toks.iter().any(|t| words.contains(&t.as_str()))
}

/// What the name says: the part and whether it needs a side (decided by the caller when the name has none).
fn from_name(name: &str) -> Option<(NamePart, f32)> {
    let toks = tokens(name);
    if toks.is_empty() {
        return None;
    }
    let side = side_of(&toks);
    let one = |p: Part| Some((NamePart::One(p), 0.9));
    let pair = |l: Part, r: Part| {
        Some(match side {
            Some(Side::L) => (NamePart::One(l), 0.9),
            Some(Side::R) => (NamePart::One(r), 0.9),
            None => (NamePart::Pair(l, r), 0.6),
        })
    };
    if has(&toks, &["teeth", "tongue", "inner"]) || (has(&toks, &["mouth", "mute"]) && has(&toks, &["inside", "in", "open", "inner"])) {
        return one(Part::MouthInside);
    }
    if has(&toks, &["eyebrow", "eyebrows", "brow", "brows", "uzacs", "uzacis"]) {
        return pair(Part::BrowL, Part::BrowR);
    }
    if has(&toks, &["eyeball", "eyeballs", "iris", "irises", "pupil", "pupils"]) {
        return pair(Part::EyeballL, Part::EyeballR);
    }
    if has(&toks, &["eye", "eyes", "eyelash", "eyelid", "acs", "acis"]) {
        return pair(Part::EyeL, Part::EyeR);
    }
    if has(&toks, &["mouth", "lips", "lip", "mute", "lupas"]) {
        return one(Part::Mouth);
    }
    if has(&toks, &["nose", "deguns"]) {
        return one(Part::Nose);
    }
    if has(&toks, &["ear", "ears", "auss", "ausis"]) && !has(&toks, &["earring", "earrings"]) {
        return pair(Part::EarL, Part::EarR);
    }
    if has(&toks, &["blush", "cheek", "cheeks"]) {
        return one(Part::Blush);
    }
    if has(&toks, &["hair", "bangs", "fringe", "ponytail", "braid", "mati", "hairstyle"]) {
        if has(&toks, &["back", "behind", "rear", "aizmugure"]) {
            return one(Part::HairBack);
        }
        if has(&toks, &["side", "sides", "sidelock", "sidelocks", "mala"]) {
            return pair(Part::HairSideL, Part::HairSideR);
        }
        if has(&toks, &["front", "bangs", "fringe"]) {
            return one(Part::HairFront);
        }
        return Some((NamePart::One(Part::HairFront), 0.5));
    }
    if has(&toks, &["bangs", "fringe"]) {
        return one(Part::HairFront);
    }
    if has(&toks, &["neck", "kakls"]) {
        return one(Part::Neck);
    }
    if has(&toks, &["hand", "hands", "palm", "fingers", "plauksta"]) {
        return pair(Part::HandL, Part::HandR);
    }
    if has(&toks, &["arm", "arms", "roka"]) {
        return pair(Part::ArmL, Part::ArmR);
    }
    if has(&toks, &["leg", "legs", "kaja", "thigh"]) {
        return pair(Part::LegL, Part::LegR);
    }
    if has(&toks, &["body", "torso", "chest", "kermenis", "ķermenis"]) {
        return one(Part::Torso);
    }
    if has(&toks, &["face", "head", "seja", "galva"]) {
        return one(Part::Face);
    }
    if has(&toks, &["shirt", "top", "dress", "skirt", "jacket", "coat", "pants", "trousers", "clothes", "clothing", "outfit", "sleeve", "sleeves", "shoe", "shoes", "boots", "sock", "socks", "kleita", "bikses", "drebes", "jaka", "svari"]) {
        return one(Part::Clothing);
    }
    if has(&toks, &["ribbon", "bow", "glasses", "hat", "earring", "earrings", "necklace", "accessory", "accessories", "halo", "horn", "horns", "wing", "wings", "tail", "scarf", "cap", "crown", "mask"]) {
        return one(Part::Accessory);
    }
    None
}

/// The letters of a name, lower case, without the word copy: "Eyelinbotl Copy 2" is "eyelinbotl".
fn compact(name: &str) -> String {
    let s: String = name.to_lowercase().chars().filter(|c| c.is_alphabetic()).collect();
    s.replace("copy", "")
}

/// A side written into the name without a space: "Rightleg", "earshadowl", "Irisl", "thumbR".
fn loose_side(name: &str) -> Option<Side> {
    let s = compact(name);
    if s.starts_with("left") && s.len() > 4 {
        return Some(Side::L);
    }
    if s.starts_with("right") && s.len() > 5 {
        return Some(Side::R);
    }
    // a lone letter at the end after a word that names a thing or what is done to it
    const BEFORE: [&str; 22] = ["shadow", "light", "highlight", "hightlight", "hilight", "bot", "top", "upper", "bottom", "lower", "lid", "lash", "line", "white", "iris", "brow", "eye", "ear", "hand", "thumb", "leg", "arm"];
    let (head, last) = s.split_at(s.len().saturating_sub(1));
    if head.len() >= 3 && BEFORE.iter().any(|w| head.ends_with(w)) {
        return match last {
            "l" => Some(Side::L),
            "r" => Some(Side::R),
            _ => None,
        };
    }
    None
}

fn side_of_name(name: &str) -> Option<Side> {
    side_of(&tokens(name)).or_else(|| loose_side(name))
}

/// A looser reading of a name for messy names ("Eyelinbotl", "Rightearline", "Hairhilight"): pieces of the letters are looked for.
fn from_name_loose(name: &str) -> Option<(NamePart, f32)> {
    let s = compact(name);
    if s.len() < 3 {
        return None;
    }
    let c = |w: &str| s.contains(w);
    let side = side_of_name(name);
    let pair = |l: Part, r: Part| {
        Some(match side {
            Some(Side::L) => (NamePart::One(l), 0.75),
            Some(Side::R) => (NamePart::One(r), 0.75),
            None => (NamePart::Pair(l, r), 0.55),
        })
    };
    let one = |p: Part| Some((NamePart::One(p), 0.7));
    if c("teeth") || c("tongue") || c("innermouth") {
        return one(Part::MouthInside);
    }
    if c("eyebrow") || s.ends_with("brow") || s.ends_with("brows") {
        return pair(Part::BrowL, Part::BrowR);
    }
    if c("iris") || c("pupil") || c("eyeball") {
        return pair(Part::EyeballL, Part::EyeballR);
    }
    if c("eyelash") || c("eyelid") || c("eyeliner") || c("eyelin") || c("eyewhite") || c("eyeshadow") || c("undereye") || c("sclera") || s == "eye" || s == "eyes" || s.starts_with("eyes") || s.starts_with("eye") && !c("eyebrow") {
        return pair(Part::EyeL, Part::EyeR);
    }
    if c("toplip") || c("botlip") || c("bptlip") || c("bottomlip") || c("upperlip") || c("lowerlip") || s.starts_with("lip") || s.ends_with("lips") || s.ends_with("mouth") || s.starts_with("mouth") {
        return one(Part::Mouth);
    }
    if s.starts_with("nose") || s.ends_with("nose") || c("noseshadow") {
        return one(Part::Nose);
    }
    if (s.starts_with("ear") || s.ends_with("ear") || c("earshadow") || c("earline") || c("rightear") || c("leftear")) && !c("earring") && !c("heart") && !c("pearl") {
        return pair(Part::EarL, Part::EarR);
    }
    if c("blush") || c("cheek") {
        return one(Part::Blush);
    }
    if c("hair") || c("bangs") || c("fringe") || c("ponytail") || c("braid") {
        if c("back") {
            return one(Part::HairBack);
        }
        if c("side") {
            return pair(Part::HairSideL, Part::HairSideR);
        }
        if c("front") || c("bangs") || c("fringe") || c("upper") {
            return one(Part::HairFront);
        }
        return Some((NamePart::One(Part::HairFront), 0.5));
    }
    if c("neck") {
        return one(Part::Neck);
    }
    if c("hand") || c("finger") || c("thumb") || c("palm") {
        return pair(Part::HandL, Part::HandR);
    }
    if c("torso") || s == "body" || c("chest") {
        return one(Part::Torso);
    }
    if s.ends_with("leg") || s.ends_with("legs") || c("thigh") {
        return pair(Part::LegL, Part::LegR);
    }
    if s.contains("face") || s.contains("skin") || s == "head" || s.starts_with("head") {
        return one(Part::Face);
    }
    const CLOTHES: [&str; 18] = ["shirt", "skirt", "dress", "jacket", "coat", "pants", "lace", "sleeve", "collar", "cape", "cloak", "glove", "shorts", "bra", "sock", "boot", "shoe", "corset"];
    if CLOTHES.iter().any(|w| c(w)) {
        return one(Part::Clothing);
    }
    const EXTRAS: [&str; 13] = ["necklace", "earring", "ribbon", "crown", "horn", "wing", "tail", "halo", "mask", "glasses", "scarf", "petal", "flower"];
    if EXTRAS.iter().any(|w| c(w)) {
        return one(Part::Accessory);
    }
    None
}

fn is_feature(p: &NamePart) -> bool {
    let f = |p: Part| matches!(p, Part::EyeL | Part::EyeR | Part::EyeballL | Part::EyeballR | Part::BrowL | Part::BrowR | Part::Nose | Part::Mouth | Part::MouthInside | Part::EarL | Part::EarR | Part::Blush);
    match p {
        NamePart::One(p) => f(*p),
        NamePart::Pair(l, r) => f(*l) || f(*r),
    }
}

fn is_skin_like(p: &NamePart) -> bool {
    matches!(p, NamePart::One(Part::Face | Part::Neck | Part::Torso | Part::Clothing | Part::Accessory))
}

/// What the name of a layer says, or failing that what the folders around it say (a layer called "Highlight" in the folder "EyeR"),
/// with the side from any of them.
fn classify_with_context(layers: &[Layer], l: &Layer) -> Option<(NamePart, f32, String)> {
    let own = from_name(&l.name).or_else(|| from_name_loose(&l.name)).map(|(p, c)| (p, c, format!("the name “{}”", l.name)));
    let mut side = side_of_name(&l.name);
    let mut anc: Option<(NamePart, f32, String)> = None;
    let mut p = l.parent;
    let mut guard = 0;
    while let Some(pid) = p {
        guard += 1;
        let Some(f) = layers.iter().find(|x| x.id == pid) else { break };
        if guard > 32 {
            break;
        }
        if side.is_none() {
            side = side_of_name(&f.name);
        }
        if anc.is_none() {
            if let Some((np, c)) = from_name(&f.name).or_else(|| from_name_loose(&f.name)) {
                anc = Some((np, c * 0.85, format!("the folder “{}”", f.name)));
            }
        }
        p = f.parent;
    }
    let chosen = match (own, anc) {
        // the skin of the mouth inside is not the skin of the face
        (Some(o), Some(a)) if is_skin_like(&o.0) && is_feature(&a.0) => Some(a),
        (Some(o), Some(a)) if o.1 < 0.6 && a.1 >= o.1 => Some(a),
        (Some(o), _) => Some(o),
        (None, a) => a,
    };
    chosen.map(|(np, c, why)| match (np, side) {
        (NamePart::Pair(l, _), Some(Side::L)) => (NamePart::One(l), c.max(0.75), why),
        (NamePart::Pair(_, r), Some(Side::R)) => (NamePart::One(r), c.max(0.75), why),
        (np, _) => (np, c, why),
    })
}

#[derive(Clone, Copy, PartialEq, Debug)]
enum NamePart {
    One(Part),
    /// A part with a left and a right half, and the name doesn't say which one.
    Pair(Part, Part),
}

/// Your own naming rule: a layer whose name fits the pattern gets this tag.
/// `*` stands for any letters ("eye*L" fits "Eye_big_L"); with no `*` the pattern fits any name that contains it. Capitals don't matter.
#[derive(Clone, PartialEq, Debug, Serialize, Deserialize)]
pub struct NameRule {
    pub pattern: String,
    pub part: Part,
}

pub fn rule_matches(pattern: &str, name: &str) -> bool {
    let (p, n) = (pattern.trim().to_lowercase(), name.to_lowercase());
    if p.is_empty() {
        return false;
    }
    if !p.contains('*') {
        return n.contains(&p);
    }
    // pieces between the stars must appear in order; the first and last stick to the ends when there is no star there
    let pieces: Vec<&str> = p.split('*').collect();
    let mut at = 0usize;
    for (i, piece) in pieces.iter().enumerate() {
        if piece.is_empty() {
            continue;
        }
        if i == 0 {
            if !n.starts_with(piece) {
                return false;
            }
            at = piece.len();
        } else if i == pieces.len() - 1 {
            return n.len() >= at + piece.len() && n.ends_with(piece);
        } else {
            match n[at..].find(piece) {
                Some(k) => at += k + piece.len(),
                None => return false,
            }
        }
    }
    true
}

/// The first rule that fits a name.
pub fn rule_for<'a>(rules: &'a [NameRule], name: &str) -> Option<&'a NameRule> {
    rules.iter().find(|r| rule_matches(&r.pattern, name))
}

/// Tag layers by your rules (and nothing else). Returns how many layers changed.
#[cfg(test)]
pub fn apply_rules(layers: &mut [Layer], rules: &[NameRule], overwrite: bool) -> usize {
    let mut n = 0;
    for l in layers.iter_mut().filter(|l| is_part_layer(l)) {
        if l.part.is_some() && !overwrite {
            continue;
        }
        if let Some(r) = rule_for(rules, &l.name) {
            if l.part != Some(r.part) {
                l.part = Some(r.part);
                n += 1;
            }
        }
    }
    n
}

fn rules_path() -> PathBuf {
    if cfg!(test) {
        return std::env::temp_dir().join(format!("seviath_name_rules_{}.json", std::process::id()));
    }
    dirs::config_dir().unwrap_or_else(std::env::temp_dir).join("SeviathStudio").join("part-name-rules.json")
}

pub fn load_rules() -> Vec<NameRule> {
    let p = rules_path();
    let mut v: Vec<NameRule> = std::fs::metadata(&p).ok().filter(|m| m.len() < 1 << 20).and_then(|_| std::fs::read(&p).ok()).and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or_default();
    v.truncate(200);
    for r in &mut v {
        r.pattern = r.pattern.chars().filter(|c| !c.is_control()).take(60).collect();
    }
    v.retain(|r| !r.pattern.trim().is_empty());
    v
}

pub fn save_rules(rules: &[NameRule]) {
    let p = rules_path();
    if let Some(d) = p.parent() {
        let _ = std::fs::create_dir_all(d);
    }
    if let Ok(t) = serde_json::to_string_pretty(rules) {
        let _ = std::fs::write(p, t);
    }
}

/// Suggest tags for layers that have none. `include_tagged`: also look at layers that already have one.
#[cfg(test)]
pub fn suggest(layers: &[Layer], doc_w: u32, doc_h: u32, include_tagged: bool) -> Vec<Suggestion> {
    suggest_with(layers, doc_w, doc_h, include_tagged, &[])
}

/// Like `suggest`, with your own naming rules first.
pub fn suggest_with(layers: &[Layer], doc_w: u32, doc_h: u32, include_tagged: bool, rules: &[NameRule]) -> Vec<Suggestion> {
    let (dw, dh) = (doc_w.max(1) as f32, doc_h.max(1) as f32);
    let mut out = Vec::new();
    for l in layers.iter().filter(|l| is_part_layer(l)) {
        if l.part.is_some() && !include_tagged {
            continue;
        }
        if let Some(r) = rule_for(rules, &l.name) {
            out.push(Suggestion { layer: l.id, part: r.part, confidence: 0.95, reason: format!("your rule “{}”", r.pattern) });
            continue;
        }
        let bounds = layer_bounds(l, doc_w, doc_h);
        // where is the paint: centre and size as shares of the page
        let geo = bounds.map(|(x0, y0, x1, y1)| {
            let (w, h) = ((x1 - x0) as f32 / dw, (y1 - y0) as f32 / dh);
            let (cx, cy) = ((x0 + x1) as f32 / 2.0 / dw, (y0 + y1) as f32 / 2.0 / dh);
            (cx, cy, w, h)
        });
        // viewer's right is the character's left
        let side_from_place = |cx: f32| if cx > 0.5 { Part::EyeL } else { Part::EyeR };
        if let Some((np, conf, said)) = classify_with_context(layers, l) {
            let s = match np {
                NamePart::One(p) => Suggestion { layer: l.id, part: p, confidence: conf, reason: said },
                NamePart::Pair(left, right) => {
                    let (part, why) = match geo {
                        Some((cx, ..)) if (cx - 0.5).abs() > 0.03 => (if cx > 0.5 { left } else { right }, "the name, and it sits on the right of the picture"),
                        _ => (left, "the name (the side is a guess)"),
                    };
                    let _ = side_from_place;
                    Suggestion { layer: l.id, part, confidence: if geo.is_some_and(|g| (g.0 - 0.5).abs() > 0.03) { 0.8 } else { conf.min(0.5) }, reason: format!("{why}: “{}”", l.name) }
                }
            };
            out.push(s);
            continue;
        }
        let Some((cx, cy, w, h)) = geo else { continue };
        let off = (cx - 0.5).abs();
        let guess = if w < 0.2 && h < 0.07 && (0.12..0.45).contains(&cy) && off > 0.04 && h < w * 0.8 {
            // thin and wide, high up, away from the middle: a brow when it is above eye height, else an eye
            if cy < 0.28 { Some((if cx > 0.5 { Part::BrowL } else { Part::BrowR }, "thin and wide, high on the face, to one side")) } else { Some((if cx > 0.5 { Part::EyeL } else { Part::EyeR }, "small and wide, on the face, to one side")) }
        } else if w < 0.18 && h < 0.09 && off < 0.04 && (0.3..0.6).contains(&cy) {
            Some((Part::Mouth, "small, in the middle of the face"))
        } else if w > 0.3 && h > 0.15 && cy < 0.35 {
            Some((Part::HairBack, "big and high up"))
        } else {
            None
        };
        if let Some((part, why)) = guess {
            out.push(Suggestion { layer: l.id, part, confidence: 0.35, reason: format!("{why} (a guess from the shape)") });
        }
    }
    out
}

// ---------- pairs and the checklist ----------

/// Left / right pairs: (the left half's tag, the left layer, the right layer).
pub fn pairs(layers: &[Layer]) -> Vec<(Part, Option<LayerId>, Option<LayerId>)> {
    let find = |p: Part| layers.iter().find(|l| l.part == Some(p)).map(|l| l.id);
    Part::ALL
        .iter()
        .filter(|p| p.is_left())
        .map(|l| (*l, find(*l), find(l.mirror().unwrap_or(*l))))
        .filter(|(_, a, b)| a.is_some() || b.is_some())
        .collect()
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Level {
    /// Rigging will go badly without it.
    Needs,
    /// Worth a look.
    Tip,
}

#[derive(Clone, PartialEq, Debug)]
pub struct Check {
    pub level: Level,
    pub text: String,
    /// The layer to jump to.
    pub jump: Option<LayerId>,
}

pub fn checklist(layers: &[Layer]) -> Vec<Check> {
    let mut v = Vec::new();
    let part_layers: Vec<&Layer> = layers.iter().filter(|l| is_part_layer(l) && !l.tiles.is_empty()).collect();
    let untagged: Vec<&&Layer> = part_layers.iter().filter(|l| l.part.is_none()).collect();
    if !untagged.is_empty() {
        let names: Vec<&str> = untagged.iter().take(3).map(|l| l.name.as_str()).collect();
        v.push(Check { level: Level::Needs, text: format!("{} layers have no tag yet ({}{})", untagged.len(), names.join(", "), if untagged.len() > 3 { ", …" } else { "" }), jump: Some(untagged[0].id) });
    }
    let find = |p: Part| layers.iter().find(|l| l.part == Some(p));
    if part_layers.iter().all(|l| l.part.is_none()) && !part_layers.is_empty() {
        return v; // nothing is tagged: the other lines would only repeat this one
    }
    if find(Part::Face).is_none() {
        v.push(Check { level: Level::Tip, text: "No face base is tagged (the skin the eyes sit on)".into(), jump: None });
    }
    for (left, l, r) in pairs(layers) {
        let right = left.mirror().unwrap_or(left);
        match (l, r) {
            (Some(_), None) => v.push(Check { level: Level::Needs, text: format!("{} has no {}", left.label(), right.label()), jump: l }),
            (None, Some(_)) => v.push(Check { level: Level::Needs, text: format!("{} has no {}", right.label(), left.label()), jump: r }),
            _ => {}
        }
    }
    let any_eye = find(Part::EyeL).is_some() || find(Part::EyeR).is_some();
    if find(Part::Face).is_some() && !any_eye {
        v.push(Check { level: Level::Tip, text: "No eyes are tagged".into(), jump: None });
    }
    if find(Part::Mouth).is_some() && find(Part::MouthInside).is_none() {
        v.push(Check { level: Level::Tip, text: "The mouth has no inside (teeth and tongue are needed to open it)".into(), jump: find(Part::Mouth).map(|l| l.id) });
    }
    if find(Part::HairFront).is_some() && find(Part::HairBack).is_none() {
        v.push(Check { level: Level::Tip, text: "There is hair in front but no hair behind the head (a layer behind the hair)".into(), jump: find(Part::HairFront).map(|l| l.id) });
    }
    for l in layers.iter().filter(|l| l.part.is_some()) {
        if !l.visible {
            v.push(Check { level: Level::Tip, text: format!("“{}” is tagged but hidden", l.name), jump: Some(l.id) });
        } else if l.tiles.is_empty() && l.vector.is_none() {
            v.push(Check { level: Level::Tip, text: format!("“{}” is tagged but empty", l.name), jump: Some(l.id) });
        }
    }
    v
}

// ---------- tag templates ----------

/// A set of tags by layer name, kept to use on another project with the same layer names.
#[derive(Clone, PartialEq, Debug, Serialize, Deserialize)]
pub struct Template {
    pub name: String,
    pub tags: Vec<(String, Part)>,
}

pub fn template_from(name: &str, layers: &[Layer]) -> Template {
    Template { name: name.chars().filter(|c| !c.is_control()).take(40).collect(), tags: layers.iter().filter_map(|l| l.part.map(|p| (l.name.to_lowercase(), p))).collect() }
}

/// Put a template's tags on the layers with the same names (not the ones that already have a tag unless `overwrite`).
/// Returns how many layers got a tag.
pub fn apply_template(layers: &mut [Layer], t: &Template, overwrite: bool) -> usize {
    let mut n = 0;
    for l in layers.iter_mut().filter(|l| is_part_layer(l)) {
        if l.part.is_some() && !overwrite {
            continue;
        }
        if let Some((_, p)) = t.tags.iter().find(|(name, _)| *name == l.name.to_lowercase()) {
            if l.part != Some(*p) {
                l.part = Some(*p);
                n += 1;
            }
        }
    }
    n
}

pub fn templates_dir() -> PathBuf {
    if cfg!(test) {
        return std::env::temp_dir().join(format!("seviath_part_templates_{}", std::process::id()));
    }
    dirs::config_dir().unwrap_or_else(std::env::temp_dir).join("SeviathStudio").join("part-templates")
}

fn file_name(name: &str) -> String {
    let s: String = name.chars().map(|c| if c.is_alphanumeric() || c == ' ' || c == '-' || c == '_' { c } else { '_' }).collect();
    format!("{}.json", s.trim())
}

pub fn save_template_in(dir: &Path, t: &Template) -> Result<(), String> {
    if t.name.trim().is_empty() {
        return Err("Give the template a name.".into());
    }
    if t.tags.is_empty() {
        return Err("Tag some layers first.".into());
    }
    std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    std::fs::write(dir.join(file_name(&t.name)), serde_json::to_string_pretty(t).map_err(|e| e.to_string())?).map_err(|e| e.to_string())
}

pub fn list_templates_in(dir: &Path) -> Vec<Template> {
    let Ok(rd) = std::fs::read_dir(dir) else { return Vec::new() };
    let mut v: Vec<Template> = rd
        .flatten()
        .filter(|e| e.path().extension().is_some_and(|x| x == "json") && e.metadata().map(|m| m.len() < 1 << 20).unwrap_or(false))
        .filter_map(|e| serde_json::from_slice::<Template>(&std::fs::read(e.path()).ok()?).ok())
        .map(|mut t| {
            t.tags.truncate(2000);
            t.name = t.name.chars().filter(|c| !c.is_control()).take(40).collect();
            t
        })
        .filter(|t| !t.name.trim().is_empty())
        .collect();
    v.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));
    v
}

pub fn delete_template_in(dir: &Path, name: &str) {
    let _ = std::fs::remove_file(dir.join(file_name(name)));
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::doc::{Document, TILE};
    use std::sync::Arc;

    /// A page 1000 x 1000 with a layer named `name` that has paint in the box (x0,y0)-(x1,y1).
    fn layer(d: &mut Document, name: &str, b: (usize, usize, usize, usize)) -> LayerId {
        let mut l = d.new_layer(name, LayerKind::Raster);
        let id = l.id;
        for ty in b.1 / TILE..=(b.3 - 1) / TILE {
            for tx in b.0 / TILE..=(b.2 - 1) / TILE {
                let mut px = vec![eframe::egui::Color32::TRANSPARENT; TILE * TILE];
                for j in 0..TILE {
                    for i in 0..TILE {
                        let (x, y) = (tx * TILE + i, ty * TILE + j);
                        if x >= b.0 && x < b.2 && y >= b.1 && y < b.3 {
                            px[j * TILE + i] = eframe::egui::Color32::from_rgb(200, 100, 100);
                        }
                    }
                }
                l.tiles.insert((tx as i32, ty as i32), Arc::new(px));
            }
        }
        d.layers.insert(0, l);
        id
    }

    fn doc() -> Document {
        Document::new(1000, 1000)
    }

    #[test]
    fn names_say_what_a_layer_is_in_english_and_latvian() {
        let p = |n: &str| from_name(n).map(|x| x.0);
        assert_eq!(p("eye L"), Some(NamePart::One(Part::EyeL)));
        assert_eq!(p("EyeR"), Some(NamePart::One(Part::EyeR)));
        assert_eq!(p("brow_left"), Some(NamePart::One(Part::BrowL)));
        assert_eq!(p("Eye"), Some(NamePart::Pair(Part::EyeL, Part::EyeR)));
        assert_eq!(p("iris L"), Some(NamePart::One(Part::EyeballL)));
        assert_eq!(p("eyeball"), Some(NamePart::Pair(Part::EyeballL, Part::EyeballR)));
        assert_eq!(p("hair front"), Some(NamePart::One(Part::HairFront)));
        assert_eq!(p("Back hair"), Some(NamePart::One(Part::HairBack)));
        assert_eq!(p("bangs"), Some(NamePart::One(Part::HairFront)));
        assert_eq!(p("hair side R"), Some(NamePart::One(Part::HairSideR)));
        assert_eq!(p("mouth inside"), Some(NamePart::One(Part::MouthInside)));
        assert_eq!(p("teeth"), Some(NamePart::One(Part::MouthInside)));
        assert_eq!(p("Mouth"), Some(NamePart::One(Part::Mouth)));
        assert_eq!(p("uzacis kreisais"), Some(NamePart::One(Part::BrowL)));
        assert_eq!(p("acs labais"), Some(NamePart::One(Part::EyeR)));
        assert_eq!(p("earring"), Some(NamePart::One(Part::Accessory)), "an earring is not an ear");
        assert_eq!(p("red dress"), Some(NamePart::One(Part::Clothing)));
        assert_eq!(p("Layer 12"), None);
        assert_eq!(p(""), None);
    }

    #[test]
    fn the_side_comes_from_the_place_when_the_name_has_none() {
        let mut d = doc();
        let right_of_page = layer(&mut d, "eye", (600, 300, 700, 340));
        let left_of_page = layer(&mut d, "eye copy", (300, 300, 400, 340));
        let s = suggest(&d.layers, 1000, 1000, false);
        let of = |id| s.iter().find(|x| x.layer == id).unwrap();
        assert_eq!(of(right_of_page).part, Part::EyeL, "the character's left is on the right of the picture");
        assert_eq!(of(left_of_page).part, Part::EyeR);
        assert!(of(right_of_page).confidence >= 0.7);
    }

    #[test]
    fn shapes_and_places_give_weak_guesses_only_for_unnamed_layers() {
        let mut d = doc();
        let mouth = layer(&mut d, "Layer 3", (450, 480, 550, 520));
        let brow = layer(&mut d, "Layer 4", (600, 220, 720, 245));
        let blob = layer(&mut d, "Layer 5", (100, 600, 300, 800));
        let s = suggest(&d.layers, 1000, 1000, false);
        let of = |id| s.iter().find(|x| x.layer == id);
        assert_eq!(of(mouth).unwrap().part, Part::Mouth);
        assert_eq!(of(brow).unwrap().part, Part::BrowL);
        assert!(of(blob).is_none(), "nothing to say about an odd shape");
        assert!(s.iter().all(|x| x.confidence <= 0.4));
        assert!(s.iter().all(|x| x.reason.contains("guess")));
    }

    #[test]
    fn tagged_layers_are_left_alone_unless_asked() {
        let mut d = doc();
        let id = layer(&mut d, "mouth", (450, 480, 550, 520));
        d.layers[0].part = Some(Part::Nose);
        assert!(suggest(&d.layers, 1000, 1000, false).is_empty());
        let s = suggest(&d.layers, 1000, 1000, true);
        assert_eq!(s.len(), 1);
        assert_eq!((s[0].layer, s[0].part), (id, Part::Mouth));
        // folders and the paper are never suggested
        let mut f = d.new_layer("Hair", LayerKind::Group);
        f.part = None;
        d.layers.insert(0, f);
        assert_eq!(suggest(&d.layers, 1000, 1000, true).len(), 1);
    }

    #[test]
    fn left_and_right_halves_are_paired_and_the_checklist_notices_missing_things() {
        let mut d = doc();
        let a = layer(&mut d, "a", (600, 300, 700, 340));
        let b = layer(&mut d, "b", (300, 300, 400, 340));
        let c = layer(&mut d, "c", (450, 480, 550, 520));
        let hair = layer(&mut d, "d", (100, 50, 900, 300));
        let idx = |id: LayerId| d.layers.iter().position(|l| l.id == id).unwrap();
        let (ia, ib, ic, ih) = (idx(a), idx(b), idx(c), idx(hair));
        d.layers[ia].part = Some(Part::EyeL);
        d.layers[ic].part = Some(Part::Mouth);
        d.layers[ih].part = Some(Part::HairFront);
        let p = pairs(&d.layers);
        assert_eq!(p, vec![(Part::EyeL, Some(a), None)]);
        let texts: Vec<String> = checklist(&d.layers).iter().map(|c| c.text.clone()).collect();
        assert!(texts.iter().any(|t| t.contains("1 layers have no tag") || t.contains("1 layer")), "{texts:?}");
        assert!(texts.iter().any(|t| t.contains("Eye L has no Eye R")), "{texts:?}");
        assert!(texts.iter().any(|t| t.contains("inside")), "{texts:?}");
        assert!(texts.iter().any(|t| t.contains("hair behind")), "{texts:?}");
        assert!(texts.iter().any(|t| t.contains("face base")), "{texts:?}");
        d.layers[ib].part = Some(Part::EyeR);
        let c2 = checklist(&d.layers);
        assert!(!c2.iter().any(|c| c.text.contains("has no Eye")), "the pair is complete now");
        assert_eq!(pairs(&d.layers), vec![(Part::EyeL, Some(a), Some(b))]);
        // a hidden tagged layer is pointed out and can be jumped to
        d.layers[ib].visible = false;
        let c3 = checklist(&d.layers);
        let hidden = c3.iter().find(|c| c.text.contains("hidden")).unwrap();
        assert_eq!(hidden.jump, Some(b));
        assert!(c3.iter().all(|c| c.level == Level::Tip), "everything is tagged and paired: only tips are left");
    }

    #[test]
    fn an_untouched_drawing_gets_one_line_not_a_wall_of_warnings() {
        let mut d = doc();
        layer(&mut d, "a", (100, 100, 200, 200));
        layer(&mut d, "b", (300, 300, 400, 400));
        let c = checklist(&d.layers);
        assert_eq!(c.len(), 1);
        assert_eq!(c[0].level, Level::Needs);
    }

    #[test]
    fn templates_carry_tags_to_layers_with_the_same_names() {
        let mut d = doc();
        layer(&mut d, "Eye L", (600, 300, 700, 340));
        layer(&mut d, "Mouth", (450, 480, 550, 520));
        layer(&mut d, "Extra", (10, 10, 50, 50));
        for l in d.layers.iter_mut() {
            l.part = match l.name.as_str() {
                "Eye L" => Some(Part::EyeL),
                "Mouth" => Some(Part::Mouth),
                _ => None,
            };
        }
        let t = template_from("My model", &d.layers);
        assert_eq!(t.tags.len(), 2);
        let mut other = doc();
        layer(&mut other, "mouth", (450, 480, 550, 520));
        layer(&mut other, "eye l", (600, 300, 700, 340));
        layer(&mut other, "Hair", (10, 10, 50, 50));
        assert_eq!(apply_template(&mut other.layers, &t, false), 2, "names match ignoring capitals");
        assert_eq!(other.layers.iter().find(|l| l.name == "eye l").unwrap().part, Some(Part::EyeL));
        assert_eq!(apply_template(&mut other.layers, &t, false), 0, "nothing changes the second time");
        let dir = std::env::temp_dir().join(format!("seviath_pt_{}", std::process::id()));
        save_template_in(&dir, &t).unwrap();
        assert_eq!(list_templates_in(&dir), vec![t.clone()]);
        assert!(save_template_in(&dir, &Template { name: "".into(), tags: t.tags.clone() }).is_err());
        assert!(save_template_in(&dir, &Template { name: "Empty".into(), tags: vec![] }).is_err());
        delete_template_in(&dir, "My model");
        assert!(list_templates_in(&dir).is_empty());
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn your_own_name_rules_tag_layers_and_win_over_the_built_in_names() {
        assert!(rule_matches("eye", "Big Eye L"));
        assert!(rule_matches("EYE*L", "eye_big_l"));
        assert!(!rule_matches("eye*L", "eye_big_r"));
        assert!(rule_matches("*_hi", "skin_hi"));
        assert!(!rule_matches("*_hi", "hi_skin"));
        assert!(rule_matches("a*b*c", "a--b--c"));
        assert!(!rule_matches("a*b*c", "a--c--b"));
        assert!(!rule_matches("", "anything") && !rule_matches("   ", "anything"));
        let rules = vec![NameRule { pattern: "lashes*".into(), part: Part::EyeL }, NameRule { pattern: "sparkle".into(), part: Part::Accessory }];
        let mut d = doc();
        layer(&mut d, "lashes top", (600, 300, 700, 340));
        layer(&mut d, "star sparkle 2", (10, 10, 50, 50));
        layer(&mut d, "Eye R", (300, 300, 400, 340));
        layer(&mut d, "Layer 7", (100, 700, 140, 740));
        // suggestions put the rules first (with high confidence)
        let s = suggest_with(&d.layers, 1000, 1000, false, &rules);
        let by_name = |n: &str| s.iter().find(|x| d.layers.iter().any(|l| l.id == x.layer && l.name == n));
        assert_eq!(by_name("lashes top").unwrap().part, Part::EyeL);
        assert!(by_name("lashes top").unwrap().reason.contains("your rule"));
        assert_eq!(by_name("Eye R").unwrap().part, Part::EyeR, "the built-in names still work");
        assert!(by_name("Layer 7").is_none());
        // applying only your rules
        let n = apply_rules(&mut d.layers, &rules, false);
        assert_eq!(n, 2);
        assert_eq!(d.layers.iter().find(|l| l.name == "Eye R").unwrap().part, None, "rules only: the built-in names are not used");
        // tagged layers keep their tag unless asked
        let id = d.layers.iter().position(|l| l.name == "lashes top").unwrap();
        d.layers[id].part = Some(Part::Nose);
        assert_eq!(apply_rules(&mut d.layers, &rules, false), 0);
        assert_eq!(apply_rules(&mut d.layers, &rules, true), 1);
        // kept in a file
        save_rules(&rules);
        assert_eq!(load_rules(), rules);
        std::fs::remove_file(rules_path()).ok();
    }

    #[test]
    fn colors_group_the_parts_and_the_right_half_is_darker() {
        assert_eq!(Part::EyeL.group(), Group::Face);
        assert_eq!(Part::HairBack.group(), Group::Hair);
        assert!(Part::EyeR.color().r() < Part::EyeL.color().r());
        assert_eq!(Part::Mouth.color(), Part::Nose.color());
        for p in Part::ALL {
            assert_eq!(p.mirror().and_then(|m| m.mirror()), p.mirror().map(|_| p), "{p:?}");
        }
    }
}
