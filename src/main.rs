// SPDX-License-Identifier: GPL-3.0-or-later
// SeviathStudio — native Rust version (egui). Being ported from the Svelte app tab by tab.

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")] // no console window in release builds

mod actions;
mod ai;
mod anim;
mod aisetup;
mod app;
mod blend;
mod brush;
mod canvas;
mod check;
mod colorpick;
mod colorprofiles;
mod doc;
mod effects;
mod fill;
mod fonts;
mod filters;
mod gradient;
mod files;
mod handfont;
mod history;
mod import;
mod randomizer;
mod recorder;
mod refs;
mod rig;
mod select;
mod icons;
mod layerfilter;
mod layout;
mod light;
mod lightfx;
mod material;
mod meta;
mod oklab;
mod outputs;
mod parts;
mod pigment;
mod prerig;
mod processlog;
mod settings;
mod signature;
mod shapes;
mod settings_ui;
mod snake;
mod stroke;
mod symmetry;
mod text;
mod tabs;
mod theme;
mod track;
mod trace;
mod transform;
mod vector;
mod view;
mod vrm;
mod wet;
mod widgets;

#[cfg(test)]
mod screens;

fn main() -> eframe::Result<()> {
    let options = eframe::NativeOptions {
        viewport: eframe::egui::ViewportBuilder::default()
            .with_title("SeviathStudio")
            .with_inner_size([1440.0, 900.0])
            .with_min_inner_size([960.0, 600.0])
            .with_icon(eframe::egui::IconData { rgba: snake::icon_rgba(256), width: 256, height: 256 }),
        ..Default::default()
    };
    eframe::run_native("SeviathStudio", options, Box::new(|cc| Ok(Box::new(app::SeviathApp::new(cc)))))
}
