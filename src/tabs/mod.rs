// SPDX-License-Identifier: GPL-3.0-or-later
// The main tabs. One project flows through all of them; each tab keeps its own state
// (zoom, tool, selection…) while you switch around.

pub mod draw;
pub mod placeholder;

use eframe::egui::Color32;

use crate::icons::Icon;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Tab {
    Draw,
    Parts,
    Rig,
    Physics,
    Animate,
    Stream,
    Mesh3d,
}

impl Tab {
    pub const ALL: [Tab; 7] = [Tab::Draw, Tab::Parts, Tab::Rig, Tab::Physics, Tab::Animate, Tab::Stream, Tab::Mesh3d];

    pub fn label(self) -> &'static str {
        match self {
            Tab::Draw => "Draw",
            Tab::Parts => "Parts",
            Tab::Rig => "Rig",
            Tab::Physics => "Physics",
            Tab::Animate => "Animate",
            Tab::Stream => "Stream",
            Tab::Mesh3d => "3D",
        }
    }

    pub fn icon(self) -> Icon {
        match self {
            Tab::Draw => Icon::Draw,
            Tab::Parts => Icon::Parts,
            Tab::Rig => Icon::Rig,
            Tab::Physics => Icon::Physics,
            Tab::Animate => Icon::Animate,
            Tab::Stream => Icon::Stream,
            Tab::Mesh3d => Icon::Generate,
        }
    }

    /// Each tab has its own color; the history panel uses it too.
    pub fn accent(self) -> Color32 {
        match self {
            Tab::Draw => Color32::from_rgb(46, 191, 113),
            Tab::Parts => Color32::from_rgb(214, 168, 76),
            Tab::Rig => Color32::from_rgb(64, 196, 196),
            Tab::Physics => Color32::from_rgb(92, 150, 240),
            Tab::Animate => Color32::from_rgb(172, 122, 240),
            Tab::Stream => Color32::from_rgb(240, 110, 120),
            Tab::Mesh3d => Color32::from_rgb(120, 220, 120),
        }
    }

    pub fn tip(self) -> &'static str {
        match self {
            Tab::Draw => "Draw the character in layers (Ctrl+1)",
            Tab::Parts => "Tag the layers before rigging: face, hair, body… (Ctrl+2)",
            Tab::Rig => "Make the character move with changers and parameters (Ctrl+3)",
            Tab::Physics => "Tick what should move: hair, cloth, earrings… (Ctrl+4)",
            Tab::Animate => "Animate parameters over time and export video (Ctrl+5)",
            Tab::Stream => "Use the model live with face tracking (Ctrl+6)",
            Tab::Mesh3d => "Make a 3D mesh from a text, a picture or a drawing, and rig it by hand (Ctrl+7)",
        }
    }
}
