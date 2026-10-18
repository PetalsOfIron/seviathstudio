// SPDX-License-Identifier: GPL-3.0-or-later
// Editing the hair of a character on an AI shape. The hair is made of strands: every strand is a chain of joints from the scalp down
// (a bone for each joint, swung by a spring bone in a game or a VRM app). The app makes strands by itself from the hair it finds
// on the drawing; the user can then drag their joints, add a strand by clicking its joints, delete a strand, change how stiff it
// is, and correct which parts of the picture are hair with a brush that adds to the hair or takes away from it.

use serde::{Deserialize, Serialize};

/// One strand: its joints on the picture from the scalp down (page pixels), how far to the front (1) or the back (-1) of the head
/// it hangs, and how it swings.
#[derive(Clone, PartialEq, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct HairChain {
    pub joints: Vec<[f32; 2]>,
    pub layer: f32,
    pub stiffness: f32,
    pub gravity: f32,
    pub drag: f32,
}

impl Default for HairChain {
    fn default() -> Self {
        HairChain { joints: Vec::new(), layer: 1.0, stiffness: 0.55, gravity: 0.08, drag: 0.35 }
    }
}

/// One dab of the hair brush on the picture: adds to the hair or takes away from it.
#[derive(Clone, PartialEq, Debug, Serialize, Deserialize)]
pub struct HairDab {
    pub at: [f32; 2],
    pub radius: f32,
    pub add: bool,
}

#[derive(Clone, PartialEq, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct HairEdit {
    /// The strands the user has made or changed. Empty: the app makes them by itself.
    pub chains: Vec<HairChain>,
    /// Corrections of where the hair is, applied in order over what the app found.
    pub dabs: Vec<HairDab>,
}

impl HairEdit {
    pub fn is_default(&self) -> bool {
        *self == HairEdit::default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hair_edits_are_saved_and_old_projects_read_as_untouched() {
        assert!(serde_json::from_str::<HairEdit>("{}").unwrap().is_default());
        let e = HairEdit { chains: vec![HairChain { joints: vec![[10.0, 20.0], [12.0, 60.0]], layer: -1.0, ..Default::default() }], dabs: vec![HairDab { at: [5.0, 6.0], radius: 9.0, add: false }] };
        let text = serde_json::to_string(&e).unwrap();
        assert_eq!(serde_json::from_str::<HairEdit>(&text).unwrap(), e);
        assert!(!e.is_default());
    }
}
