// SPDX-License-Identifier: GPL-3.0-or-later
// The Animations page of the Generate tab: 22 ready motions and a generator (a motion, a mood, a speed, a strength, a little chance, or a
// few words) for the rigged 3D character. The motion plays on the character in the middle; the ones you add are saved in the game
// file (.glb). VRM files for VTubers carry no animations, because the VTuber program moves the model itself.

use eframe::egui::{self, RichText, TextureHandle, TextureOptions, Ui, vec2};

use super::SeviathApp;
use crate::motion::{CATEGORIES, Clip, MOODS, MOTIONS, Motion, Style, from_words, info_of};
use crate::theme::*;
use crate::vrm::export::{Format, Summary};
use crate::widgets::{self, HelpExt, section_title};

pub struct AnimState {
    /// The category shown (None: all of them).
    pub category: Option<usize>,
    pub motion: Motion,
    pub style: Style,
    pub words: String,
    pub playing: bool,
    pub time: f32,
    pub yaw: f32,
    /// The animations that go in the game file.
    pub added: Vec<Clip>,
    pub fps: u32,
    view: Option<((i32, i32, u64, String), TextureHandle)>,
    pub message: Option<String>,
}

impl Default for AnimState {
    fn default() -> Self {
        AnimState { category: None, motion: Motion::Walk, style: Style::default(), words: String::new(), playing: true, time: 0.0, yaw: 20.0, added: Vec::new(), fps: 30, view: None, message: None }
    }
}

impl AnimState {
    pub fn clip(&self) -> Clip {
        Clip::new(self.motion, self.style)
    }
}

impl SeviathApp {
    pub(super) fn gen_animations_left(&mut self, ui: &mut Ui) {
        section_title(ui, "Describe an animation");
        ui.label(RichText::new("Write what the character does and how: a tired old man walks slowly, an angry fast punch, a happy dance.").size(11.5).color(TEXT_DIM));
        ui.add(egui::TextEdit::singleline(&mut self.genr.anim.words).desired_width(f32::INFINITY).hint_text("a scared character sneaks"));
        if ui.add_enabled(!self.genr.anim.words.trim().is_empty(), egui::Button::new(RichText::new("Make it from the words").strong())).help("Finds the animation and the mood in the words, and sets the sliders. It uses the 22 animations and the moods, so only what they can do.").clicked() {
            let seed = self.genr.anim.style.seed.wrapping_add(7) | 1;
            match from_words(&self.genr.anim.words, seed) {
                Some(c) => {
                    self.genr.anim.motion = c.motion;
                    self.genr.anim.style = c.style;
                    self.genr.anim.time = 0.0;
                    self.genr.anim.message = Some(format!("{}: speed {:.1}, strength {:.1}.", c.name, c.style.speed, c.style.strength));
                }
                None => self.genr.anim.message = Some("No animation was found in the words. Try a word like walk, run, jump, wave, dance, punch, bow or sneak.".into()),
            }
        }

        ui.add_space(8.0);
        section_title(ui, "Change it");
        let st = &mut self.genr.anim.style;
        ui.add(egui::Slider::new(&mut st.speed, 0.4..=2.5).text("Speed")).help("Faster or slower than the ready animation.");
        ui.add(egui::Slider::new(&mut st.strength, 0.0..=1.8).text("Strength")).help("How big the movements are. 0 is standing still, 1 is as made, more is exaggerated.");
        ui.label(RichText::new("Mood").size(12.0).color(TEXT_DIM));
        ui.horizontal_wrapped(|ui| {
            for (m, name, _) in MOODS {
                if widgets::chip(ui, name, st.mood == m).help("How the body is held all the time: a sad character hangs the head, an angry one pulls the shoulders up.").clicked() {
                    st.mood = m;
                }
            }
        });
        ui.horizontal(|ui| {
            if ui.button("Another variation").help("A small different change of the animation: the same motion, but not exactly the same. Press again for another.").clicked() {
                let next = st.seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223) | 1;
                st.seed = next;
            }
            if st.seed != 0 && ui.small_button("None").help("Back to the plain animation.").clicked() {
                st.seed = 0;
            }
        });

        ui.add_space(8.0);
        section_title(ui, "Ready animations");
        ui.horizontal_wrapped(|ui| {
            if widgets::chip(ui, "All", self.genr.anim.category.is_none()).help("Show every animation.").clicked() {
                self.genr.anim.category = None;
            }
            for (i, c) in CATEGORIES.iter().enumerate() {
                if widgets::chip(ui, c, self.genr.anim.category == Some(i)).help("Show only this kind of animation.").clicked() {
                    self.genr.anim.category = Some(i);
                }
            }
        });
        ui.add_space(4.0);
        for info in MOTIONS {
            if let Some(c) = self.genr.anim.category {
                if info.category != CATEGORIES[c] {
                    continue;
                }
            }
            let on = self.genr.anim.motion == info.motion;
            ui.horizontal(|ui| {
                let label = format!("{}  ({:.1} s{})", info.name, info.seconds, if info.looping { ", loop" } else { "" });
                if ui.selectable_label(on, label).help(info.about).clicked() {
                    self.genr.anim.motion = info.motion;
                    self.genr.anim.time = 0.0;
                }
            });
        }
    }

    pub(super) fn gen_animations_center(&mut self, ui: &mut Ui) {
        section_title(ui, "Preview");
        let clip = self.genr.anim.clip();
        let d = clip.seconds();
        let info = info_of(clip.motion);
        ui.label(RichText::new(format!("{}  ·  {:.2} s{}", clip.name, d, if clip.looping() { "  ·  loops" } else { "" })).size(12.5).color(TEXT));
        ui.label(RichText::new(info.about).size(11.5).color(TEXT_DIM));
        let Some(model) = self.rigs.game.model.as_ref() else {
            ui.add_space(10.0);
            ui.label(RichText::new("There is no 3D character yet. Make one on the VRM characters page (pictures, then Make the 3D character); it shows here and moves.").size(12.0).color(GOLD));
            return;
        };
        // time: a loop goes round, a single go stays at its end for a moment
        let dt = ui.input(|i| i.stable_dt).min(0.1);
        if self.genr.anim.playing {
            self.genr.anim.time += dt;
            ui.ctx().request_repaint_after(std::time::Duration::from_millis(40));
        }
        let span = if clip.looping() { d } else { d + 0.5 };
        let secs = if span > 0.0 { self.genr.anim.time.rem_euclid(span) } else { 0.0 };
        let side_h = (ui.available_height() - 90.0).clamp(300.0, 620.0) as u32 / 4 * 4;
        let side_w = side_h * 3 / 4 / 4 * 4;
        let key = ((secs * 30.0) as i32, self.genr.anim.yaw.round() as i32, self.rigs.game.made_count(), format!("{:?}{:?}", self.genr.anim.motion, self.genr.anim.style));
        if self.genr.anim.view.as_ref().map(|v| &v.0) != Some(&key) {
            let img = crate::vrm::preview::render_motion(model, self.genr.anim.yaw, side_w, side_h, &clip.at(secs), 1.45);
            let color = egui::ColorImage::from_rgba_unmultiplied([img.width() as usize, img.height() as usize], img.as_raw());
            self.genr.anim.view = Some((key, ui.ctx().load_texture("gen_motion_view", color, TextureOptions::LINEAR)));
        }
        if let Some((_, tex)) = &self.genr.anim.view {
            let resp = ui.add(egui::Image::new((tex.id(), vec2(side_w as f32, side_h as f32))).corner_radius(6.0).sense(egui::Sense::drag()));
            if resp.dragged() {
                self.genr.anim.yaw -= resp.drag_delta().x * 0.6;
            }
            resp.help("Drag to turn the character round while it moves.");
        }
        ui.horizontal(|ui| {
            if ui.button(if self.genr.anim.playing { "Pause" } else { "Play" }).help("Stop or start the animation.").clicked() {
                self.genr.anim.playing = !self.genr.anim.playing;
            }
            let mut t = secs;
            if ui.add(egui::Slider::new(&mut t, 0.0..=d.max(0.1)).show_value(false)).help("Go to a moment of the animation.").changed() {
                self.genr.anim.time = t;
                self.genr.anim.playing = false;
            }
            ui.label(RichText::new(format!("{secs:.2} / {d:.2} s")).size(11.5).color(TEXT_DIM));
        });
    }

    pub(super) fn gen_animations_right(&mut self, ui: &mut Ui) {
        section_title(ui, "In the game file");
        let clip = self.genr.anim.clip();
        if ui.button(RichText::new("Add this animation").strong()).help("Puts the animation as it is now (with the speed, the strength and the mood) in the list that goes in the game file.").clicked() {
            let mut c = clip.clone();
            let same = self.genr.anim.added.iter().filter(|a| a.name.starts_with(&c.name)).count();
            if same > 0 {
                c.name = format!("{} {}", c.name, same + 1);
            }
            self.genr.anim.added.push(c);
        }
        if ui.button("Add all the ready animations").help("Every ready animation as it is, without any change.").clicked() {
            for i in MOTIONS {
                if !self.genr.anim.added.iter().any(|a| a.name == i.name) {
                    self.genr.anim.added.push(Clip::new(i.motion, Style::default()));
                }
            }
        }
        let mut remove = None;
        for (i, c) in self.genr.anim.added.iter().enumerate() {
            ui.horizontal(|ui| {
                ui.label(RichText::new(format!("{}  ({:.1} s)", c.name, c.seconds())).size(12.0));
                if ui.small_button("Show").help("Play this one in the preview.").clicked() {
                    self.genr.anim.motion = c.motion;
                    self.genr.anim.style = c.style;
                    self.genr.anim.time = 0.0;
                }
                if ui.small_button("Remove").help("Take it out of the list.").clicked() {
                    remove = Some(i);
                }
            });
        }
        if let Some(i) = remove {
            self.genr.anim.added.remove(i);
        }
        if self.genr.anim.added.is_empty() {
            ui.label(RichText::new("Nothing added yet.").size(11.5).color(TEXT_DIM));
        } else if ui.small_button("Remove all").help("Empty the list.").clicked() {
            self.genr.anim.added.clear();
        }
        ui.add_space(8.0);
        section_title(ui, "Save");
        ui.horizontal(|ui| {
            ui.label(RichText::new("Frames a second").size(12.0).color(TEXT_DIM));
            for f in [24u32, 30, 60] {
                if widgets::chip(ui, &format!("{f}"), self.genr.anim.fps == f).help("How many poses are saved for every second of the animation.").clicked() {
                    self.genr.anim.fps = f;
                }
            }
        });
        let have = self.rigs.game.model.is_some() && !self.genr.anim.added.is_empty();
        if ui.add_enabled(have, egui::Button::new(RichText::new("Save the game file (.glb)").strong())).help("The character with its skeleton, skin and face shapes and all the animations of the list in one .glb file. Unity, Unreal, Godot and Blender list the animations by their names.").clicked() {
            self.gen_save_game_glb(false);
        }
        if ui.add_enabled(have, egui::Button::new("One file for each animation...")).help("Pick a folder: every animation is saved as its own .glb with the character in it.").clicked() {
            self.gen_save_game_glb(true);
        }
        ui.add_space(6.0);
        ui.label(RichText::new("VRM files for VTubers carry no animations: the VTuber program moves the model. Only the game file (.glb) has them.").size(11.0).color(TEXT_DIM));
        if let Some(m) = &self.genr.anim.message {
            ui.label(RichText::new(m).size(11.5).color(TEXT_DIM));
        }
    }

    /// The game file with some animations.
    pub fn game_glb_with(&mut self, clips: &[Clip], fps: f32) -> Result<(Vec<u8>, Summary), String> {
        if self.rigs.game.model.is_none() {
            self.game_make();
        }
        let m = self.game_model_for_games().ok_or("There is no character to save.")?;
        let bytes = crate::vrm::export::export_full(&m, Format::Glb, None, clips, fps)?;
        let summary = crate::vrm::export::validate(&bytes)?;
        Ok((bytes, summary))
    }

    fn gen_save_game_glb(&mut self, each: bool) {
        let fps = self.genr.anim.fps as f32;
        let clips = self.genr.anim.added.clone();
        let stem = if self.rigs.game.name.trim().is_empty() { "character".to_string() } else { self.rigs.game.name.trim().replace(' ', "_") };
        if each {
            let Some(dir) = rfd::FileDialog::new().set_title("A folder for the files").set_directory(self.settings.projects_folder()).pick_folder() else { return };
            let mut done = 0;
            for c in &clips {
                let name: String = c.name.chars().map(|ch| if ch.is_alphanumeric() { ch } else { '_' }).collect();
                match self.game_glb_with(std::slice::from_ref(c), fps) {
                    Ok((bytes, _)) => {
                        if std::fs::write(dir.join(format!("{stem}_{name}.glb")), bytes).is_ok() {
                            done += 1;
                        }
                    }
                    Err(e) => {
                        self.genr.anim.message = Some(e);
                        return;
                    }
                }
            }
            self.genr.anim.message = Some(format!("Saved {done} files in {}.", dir.display()));
        } else {
            match self.game_glb_with(&clips, fps) {
                Ok((bytes, _)) => {
                    if let Some(p) = rfd::FileDialog::new().set_title("Save the game file").set_directory(self.settings.projects_folder()).set_file_name(format!("{stem}_animated.glb")).add_filter("glTF binary", &["glb"]).save_file() {
                        self.genr.anim.message = Some(match std::fs::write(&p, bytes) {
                            Ok(()) => format!("Saved {} with {} animations.", p.display(), clips.len()),
                            Err(e) => format!("Could not save the file: {e}"),
                        });
                    }
                }
                Err(e) => self.genr.anim.message = Some(e),
            }
        }
    }
}
