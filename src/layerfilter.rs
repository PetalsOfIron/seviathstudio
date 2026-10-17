// SPDX-License-Identifier: GPL-3.0-or-later
// Layer panel filters (plan §W2): a name search plus stackable chips, each of which can be turned
// into its NOT ("not locked"). Filters can be saved as presets inside the project.

use crate::doc::{Blend, Layer, LayerKind};
use crate::files::project::{blend_from_name, blend_name};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ChipKind {
    Raster,
    Vector,
    Folder,
    Clipped,
    Visible,
    Locked,
    Empty,
    Favorite,
    Blend(Blend),
}

impl ChipKind {
    /// The chips that are always offered (blend modes are added from their own menu).
    pub const BASIC: [ChipKind; 8] = [
        ChipKind::Raster,
        ChipKind::Vector,
        ChipKind::Folder,
        ChipKind::Clipped,
        ChipKind::Visible,
        ChipKind::Locked,
        ChipKind::Empty,
        ChipKind::Favorite,
    ];

    pub fn label(self) -> String {
        match self {
            ChipKind::Raster => "Pixel".into(),
            ChipKind::Vector => "Vector".into(),
            ChipKind::Folder => "Folder".into(),
            ChipKind::Clipped => "Clipped".into(),
            ChipKind::Visible => "Visible".into(),
            ChipKind::Locked => "Locked".into(),
            ChipKind::Empty => "Empty".into(),
            ChipKind::Favorite => "Favorite".into(),
            ChipKind::Blend(b) => b.name().to_owned(),
        }
    }

    fn key(self) -> String {
        match self {
            ChipKind::Raster => "raster".into(),
            ChipKind::Vector => "vector".into(),
            ChipKind::Folder => "folder".into(),
            ChipKind::Clipped => "clipped".into(),
            ChipKind::Visible => "visible".into(),
            ChipKind::Locked => "locked".into(),
            ChipKind::Empty => "empty".into(),
            ChipKind::Favorite => "favorite".into(),
            ChipKind::Blend(b) => format!("blend:{}", blend_name(b)),
        }
    }

    fn from_key(k: &str) -> Option<ChipKind> {
        Some(match k {
            "raster" => ChipKind::Raster,
            "vector" => ChipKind::Vector,
            "folder" => ChipKind::Folder,
            "clipped" => ChipKind::Clipped,
            "visible" => ChipKind::Visible,
            "locked" => ChipKind::Locked,
            "empty" => ChipKind::Empty,
            "favorite" => ChipKind::Favorite,
            _ => ChipKind::Blend(blend_from_name(k.strip_prefix("blend:")?)),
        })
    }

    /// Does this layer have the property? (`has_children` only matters for folders.)
    fn holds(self, l: &Layer, has_children: bool) -> bool {
        match self {
            ChipKind::Raster => l.kind == LayerKind::Raster,
            ChipKind::Vector => l.kind == LayerKind::Vector,
            ChipKind::Folder => l.kind == LayerKind::Group,
            ChipKind::Clipped => l.clip,
            ChipKind::Visible => l.visible,
            ChipKind::Locked => l.locked,
            ChipKind::Empty => match l.kind {
                LayerKind::Raster | LayerKind::Vector => l.tiles.is_empty(),
                LayerKind::Group => !has_children,
                LayerKind::Paper | LayerKind::Adjust => false,
            },
            ChipKind::Favorite => l.fav,
            ChipKind::Blend(b) => l.blend == b,
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Chip {
    pub kind: ChipKind,
    /// "not": the layer must NOT have the property.
    pub not: bool,
}

#[derive(Clone, Default, PartialEq, Debug)]
pub struct LayerFilter {
    pub query: String,
    /// Color label to show (0 = any).
    pub label: u8,
    pub chips: Vec<Chip>,
}

impl LayerFilter {
    pub fn is_active(&self) -> bool {
        !self.query.trim().is_empty() || self.label != 0 || !self.chips.is_empty()
    }

    /// How many conditions are on (shown on the Filters button).
    pub fn count(&self) -> usize {
        usize::from(self.label != 0) + self.chips.len()
    }

    pub fn clear(&mut self) {
        *self = LayerFilter::default();
    }

    pub fn chip_state(&self, kind: ChipKind) -> Option<bool> {
        self.chips.iter().find(|c| c.kind == kind).map(|c| c.not)
    }

    /// Click on a chip: off, then on, then NOT, then off again.
    pub fn cycle(&mut self, kind: ChipKind) {
        match self.chips.iter().position(|c| c.kind == kind) {
            None => self.chips.push(Chip { kind, not: false }),
            Some(i) if !self.chips[i].not => self.chips[i].not = true,
            Some(i) => {
                self.chips.remove(i);
            }
        }
    }

    pub fn matches(&self, l: &Layer, has_children: bool) -> bool {
        let q = self.query.trim().to_lowercase();
        (q.is_empty() || l.name.to_lowercase().contains(&q))
            && (self.label == 0 || l.tag == self.label)
            && self.chips.iter().all(|c| c.kind.holds(l, has_children) != c.not)
    }

    pub fn chip_keys(&self) -> Vec<String> {
        self.chips.iter().map(|c| format!("{}{}", if c.not { "!" } else { "" }, c.kind.key())).collect()
    }

    pub fn from_parts(query: String, label: u8, chips: &[String]) -> LayerFilter {
        let chips = chips
            .iter()
            .filter_map(|k| {
                let (not, k) = match k.strip_prefix('!') {
                    Some(r) => (true, r),
                    None => (false, k.as_str()),
                };
                ChipKind::from_key(k).map(|kind| Chip { kind, not })
            })
            .collect();
        LayerFilter { query: query.chars().take(80).collect(), label: label.min(7), chips }
    }
}

#[derive(Clone, PartialEq, Debug)]
pub struct Preset {
    pub name: String,
    pub filter: LayerFilter,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SortBy {
    Z,
    Name,
    Label,
    Recent,
}

impl SortBy {
    pub const ALL: [(SortBy, &'static str); 4] =
        [(SortBy::Z, "Stack order"), (SortBy::Name, "Name"), (SortBy::Label, "Color label"), (SortBy::Recent, "Recently edited")];
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::doc::Document;

    fn layer(d: &mut Document, name: &str, f: impl FnOnce(&mut Layer)) {
        let mut l = d.new_layer(name, LayerKind::Raster);
        f(&mut l);
        let top = d.layers[0].id;
        d.insert_layer(l, top);
    }

    #[test]
    fn chips_stack_and_not_works() {
        let mut d = Document::new(100, 100);
        layer(&mut d, "Hair front", |l| l.tag = 3);
        layer(&mut d, "Hair back", |l| {
            l.tag = 3;
            l.locked = true;
        });
        layer(&mut d, "Skin", |l| l.blend = Blend::Multiply);
        let names = |f: &LayerFilter| d.layers.iter().filter(|l| f.matches(l, false)).map(|l| l.name.clone()).collect::<Vec<_>>();

        let mut f = LayerFilter { query: "hair".into(), ..Default::default() };
        assert_eq!(names(&f).len(), 2);
        f.label = 3;
        f.cycle(ChipKind::Locked); // on
        assert_eq!(names(&f), ["Hair back"]);
        f.cycle(ChipKind::Locked); // NOT
        assert_eq!(names(&f), ["Hair front"]);
        f.cycle(ChipKind::Locked); // off
        assert_eq!(names(&f).len(), 2);

        let mut m = LayerFilter::default();
        m.cycle(ChipKind::Blend(Blend::Multiply));
        assert_eq!(names(&m), ["Skin"]);
        let mut empty = LayerFilter::default();
        empty.cycle(ChipKind::Empty);
        assert_eq!(names(&empty).len(), 4, "all three new layers and the empty first layer");
    }

    #[test]
    fn presets_round_trip_through_keys() {
        let mut f = LayerFilter { query: "skin".into(), label: 2, chips: Vec::new() };
        f.cycle(ChipKind::Favorite);
        f.cycle(ChipKind::Locked);
        f.cycle(ChipKind::Locked);
        f.cycle(ChipKind::Blend(Blend::SoftLight));
        let back = LayerFilter::from_parts(f.query.clone(), f.label, &f.chip_keys());
        assert_eq!(back, f);
        assert!(f.chip_keys().contains(&"!locked".to_string()));
    }
}
