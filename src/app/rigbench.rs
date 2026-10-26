// SPDX-License-Identifier: GPL-3.0-or-later
// The workbench for rigging by hand, the same in the Rig, Physics and Animate tabs: a row of tools along the top (every kind of changer, meshes,
// skin and bones, glue, shapes), the parts and the parameters (the sliders) on the left, and the automatic rig on the right (Rig tab only).
// A tool opens a small window of its own where its details are set: the grid of a warp, the pivot of a rotation, what it sits inside. Nothing
// here needs the parts to be tagged: tags are only for the automatic rig.

use eframe::egui::{self, Align2, Color32, RichText, Sense, Ui, vec2};

use super::SeviathApp;
use crate::doc::LayerId;
use crate::parts;
use crate::rig::{self, Id, Kind};
use crate::theme::*;
use crate::widgets::{self, section_title};

/// The kinds of changer that the top bar can make.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum NewKind {
    Rotation,
    Warp,
    Line,
    Soft,
    Path,
    Pins,
    Move,
    Scale,
    Flip,
    Fade,
    Order,
    Wave,
}

impl NewKind {
    const ALL: [NewKind; 12] = [NewKind::Rotation, NewKind::Warp, NewKind::Line, NewKind::Soft, NewKind::Path, NewKind::Pins, NewKind::Move, NewKind::Scale, NewKind::Flip, NewKind::Fade, NewKind::Order, NewKind::Wave];

    pub fn word(self) -> &'static str {
        match self {
            NewKind::Rotation => "Rotation",
            NewKind::Warp => "Warp",
            NewKind::Line => "Line",
            NewKind::Soft => "Soft",
            NewKind::Path => "Path",
            NewKind::Pins => "Pins",
            NewKind::Move => "Move",
            NewKind::Scale => "Scale",
            NewKind::Flip => "Flip",
            NewKind::Fade => "Fade",
            NewKind::Order => "Order",
            NewKind::Wave => "Wave",
        }
    }

    fn about(self) -> &'static str {
        match self {
            NewKind::Rotation => "Turns the picked parts around a point (the middle of them to start with)",
            NewKind::Warp => "A grid over the picked parts: move its points to bend them",
            NewKind::Line => "A curve along the picked parts: they bend along it (hair strands, tails, ribbons)",
            NewKind::Soft => "A soft push around a point (cheeks, eyes, belly)",
            NewKind::Path => "Paths drawn on the picture: drag a spot of a path and the picture bends like a line. For cloth, and for things that are not physics, like closing an eye",
            NewKind::Pins => "Pins on the picture: drag a pin and the picture follows smoothly without breaking. Pins you do not move hold it still",
            NewKind::Move => "Shifts the picked parts",
            NewKind::Scale => "Makes the picked parts wider or taller about their middle",
            NewKind::Flip => "Turns the picked parts over, through flat",
            NewKind::Fade => "Makes the picked parts see-through: show or hide a part with a slider",
            NewKind::Order => "Moves the picked parts forward or back in the stack (the draw order)",
            NewKind::Wave => "A wave runs through the picked parts: flags, seaweed, hair ends",
        }
    }
}

/// The tools of the top bar.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Tool {
    Meshes,
    Skin,
    New(NewKind),
    Changers,
    Glue,
    Shapes,
    /// The brush that pushes the points of the picked changer.
    Brush,
    /// The Physics tab: a window to look at the physics on its own.
    PhysicsCheck,
    /// The Physics tab: the groups of physics (inputs, pendulum, outputs).
    PhysicsGroups,
}

impl Tool {
    fn title(self) -> String {
        match self {
            Tool::Meshes => "Meshes".into(),
            Tool::Skin => "Skin and bones".into(),
            Tool::New(k) => format!("New {} changer", k.word()),
            Tool::Changers => "Changers".into(),
            Tool::Glue => "Glue".into(),
            Tool::Shapes => "Shapes of a slider".into(),
            Tool::Brush => "Push brush".into(),
            Tool::PhysicsCheck => "Check the physics".into(),
            Tool::PhysicsGroups => "Physics settings".into(),
        }
    }
}

/// What the workbench remembers.
pub struct Bench {
    /// The tool window that is open.
    pub open: Option<Tool>,
    /// The Rig tab shows the 3D character page instead of the 2D rig.
    pub view3d: bool,
    /// Physics and Animate tabs: the picture in the middle is the one the rig is edited on (drag its points there).
    pub edit_here: bool,
    /// The new changer being set up.
    pub name: String,
    pub parent: Option<Id>,
    pub line_points: usize,
    pub smooth: bool,
    pub flip_vertical: bool,
    pub pivot: Option<[f32; 2]>,
    /// The Animate tab shows the 3D character (posed by the bone sliders) instead of the 2D picture.
    pub anim_3d: bool,
    /// The 3D page shows the drawing (to drag the face marks and the hair) instead of the 3D character.
    pub picture: bool,
}

impl Default for Bench {
    fn default() -> Self {
        Self { open: None, view3d: false, edit_here: false, name: String::new(), parent: None, line_points: 5, smooth: true, flip_vertical: false, pivot: None, anim_3d: false, picture: false }
    }
}

impl SeviathApp {
    /// The row of tools along the top. `with_view_switch`: the Rig tab also has the switch between the 2D rig and the 3D character.
    pub(super) fn rig_toolbar(&mut self, ui: &mut Ui, with_view_switch: bool, with_physics: bool, with_edit_here: bool) {
        let before = self.rigs.bench.open;
        self.rig_toolbar_row(ui, with_view_switch, with_physics, with_edit_here);
        // another tool: what the last one was waiting for on the picture (a path, a pin, a pivot) is over
        if self.rigs.bench.open != before {
            self.rigs.mode = if self.rigs.bench.open == Some(Tool::Brush) { super::rigtab::Mode::Brush } else { super::rigtab::Mode::Look };
            self.rigs.stroke.clear();
        }
    }

    fn rig_toolbar_row(&mut self, ui: &mut Ui, with_view_switch: bool, with_physics: bool, with_edit_here: bool) {
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing = vec2(4.0, 4.0);
            if with_view_switch {
                if widgets::chip(ui, "2D rig", !self.rigs.bench.view3d).on_hover_text("Rig a drawing by hand or automatically: meshes, changers, sliders").clicked() {
                    self.rigs.bench.view3d = false;
                }
                if widgets::chip(ui, "3D character", self.rigs.bench.view3d).on_hover_text("Turn the drawing into a 3D character with a skeleton, for games and VRM apps").clicked() {
                    self.rigs.bench.view3d = true;
                }
                ui.separator();
            }
            if with_view_switch && self.rigs.bench.view3d {
                return;
            }
            ui.label(RichText::new("Rig by hand").size(12.0).color(TEXT_DIM));
            let mut set = |ui: &mut Ui, bench: &mut Bench, tool: Tool, label: &str, tip: &str| {
                if widgets::chip(ui, label, bench.open == Some(tool)).on_hover_text(tip).clicked() {
                    bench.open = if bench.open == Some(tool) { None } else { Some(tool) };
                    bench.name.clear();
                    bench.parent = None;
                    bench.pivot = None;
                }
            };
            let bench = &mut self.rigs.bench;
            set(ui, bench, Tool::Meshes, "Meshes", "Cut layers into triangles so they can bend");
            set(ui, bench, Tool::Skin, "Skin and bones", "One mesh over the whole drawing, bones on top, and the weights of every bone");
            ui.separator();
            for k in NewKind::ALL {
                set(ui, bench, Tool::New(k), k.word(), k.about());
            }
            ui.separator();
            set(ui, bench, Tool::Changers, "Changers", "Every changer there is: pick one, put it inside another, delete it, copy it to the other side");
            set(ui, bench, Tool::Brush, "Brush", "Push the points of the picked changer with a brush (warp, line, pins, paths, soft): pick the changer and a slider, then drag over the picture");
            set(ui, bench, Tool::Shapes, "Shapes", "Say how the picked slider moves the picked changer: save the shape at each key point");
            set(ui, bench, Tool::Glue, "Glue", "Keeps the seam of two parts closed when they move differently");
            if with_physics {
                ui.separator();
                set(ui, bench, Tool::PhysicsGroups, "Physics settings", "Groups of swinging chains like Live2D: which sliders push a chain, how it swings, which sliders it moves. Make as many as you like");
                set(ui, bench, Tool::PhysicsCheck, "Check the physics", "A window with the picture and the physics running, to look at them on their own");
            }
            if with_edit_here {
                ui.separator();
                let on = self.rigs.bench.edit_here;
                if widgets::chip(ui, if on { "Editing the rig on the picture" } else { "Edit the rig on the picture" }, on).on_hover_text("Shows the rig editor in the middle (drag the points of a changer, pick parts by clicking them) instead of the finished picture").clicked() {
                    self.rigs.bench.edit_here = !on;
                    self.rigs.bench.anim_3d = false;
                }
                let on3 = self.rigs.bench.anim_3d;
                if widgets::chip(ui, "3D view", on3).on_hover_text("Shows the 3D character, posed by the 3D bones sliders on the left (make them on the Rig tab, 3D character page): key them like any slider").clicked() {
                    self.rigs.bench.anim_3d = !on3;
                    self.rigs.bench.edit_here = false;
                }
            }
        });
    }

    /// The window of the open tool.
    pub(super) fn rig_tool_window(&mut self, ctx: &egui::Context, now: f64) {
        let Some(tool) = self.rigs.bench.open else { return };
        let mut open = true;
        let high = (ctx.content_rect().height() - 170.0).max(300.0);
        egui::Window::new(RichText::new(tool.title()).strong())
            .id(egui::Id::new("rig_tool_window"))
            .open(&mut open)
            .collapsible(false)
            .default_pos(ctx.content_rect().left_top() + vec2(420.0, 120.0))
            .default_width(380.0)
            .default_height(high.min(760.0))
            .show(ctx, |ui| {
                egui::ScrollArea::vertical().max_height(high).auto_shrink([false, true]).show(ui, |ui| match tool {
                    Tool::Meshes => self.rig_mesh_body(ui, now),
                    Tool::Skin => self.rig_skin_body(ui, now),
                    Tool::New(k) => self.rig_new_changer_body(ui, now, k),
                    Tool::Changers => self.rig_changers_body(ui, now),
                    Tool::Glue => self.rig_glue_body(ui, now),
                    Tool::Shapes => self.rig_keyforms_body(ui, now),
                    Tool::Brush => self.rig_brush_body(ui),
                    Tool::PhysicsCheck => self.physics_check_body(ui),
                    Tool::PhysicsGroups => self.physics_groups_body(ui, now),
                });
            });
        if !open {
            self.rigs.bench.open = None;
        }
    }

    /// The middle of the Rig tab's 3D page: the 3D character, or the drawing when the face marks or the hair are being dragged.
    pub(super) fn rig_3d_center(&mut self, ui: &mut Ui, now: f64, picked: &mut Option<(LayerId, bool, bool)>) {
        let marks = self.rigs.face_edit || self.rigs.hair_edit;
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing = vec2(4.0, 4.0);
            if widgets::chip(ui, "3D character", !self.rigs.bench.picture && !marks).clicked() {
                self.rigs.bench.picture = false;
                self.rigs.face_edit = false;
                self.rigs.hair_edit = false;
            }
            if widgets::chip(ui, "The drawing (face marks, hair)", self.rigs.bench.picture || marks).on_hover_text("The drawing, to drag the eyes and the mouth or the strands of the hair").clicked() {
                self.rigs.bench.picture = true;
            }
        });
        if self.rigs.bench.picture || marks {
            self.rig_view(ui, now, picked);
        } else {
            self.game_preview_ui(ui);
        }
    }

    /// The push brush: how big and how strong.
    fn rig_brush_body(&mut self, ui: &mut Ui) {
        ui.label(RichText::new("Pick a changer with points (warp, line, pins, paths or soft) and a slider on the left, then drag over the picture: the points under the brush follow, the nearer the middle the more. The shape is saved at the value of the slider when you let go.").size(11.5).color(TEXT_DIM));
        ui.horizontal(|ui| {
            ui.label(RichText::new("Size").size(12.0).color(TEXT_DIM));
            ui.add(egui::Slider::new(&mut self.rigs.brush_r, 10.0..=800.0).logarithmic(true).suffix(" px"));
        });
        ui.horizontal(|ui| {
            ui.label(RichText::new("Strength").size(12.0).color(TEXT_DIM));
            ui.add(egui::Slider::new(&mut self.rigs.brush_s, 0.05..=1.0));
        });
        let ok_changer = self.rigs.sel_changer.is_some();
        let ok_slider = self.rigs.sel_param.is_some();
        ui.label(RichText::new(format!("Changer: {}.  Slider: {}.", if ok_changer { "picked" } else { "none yet" }, if ok_slider { "picked" } else { "none yet" })).size(11.5).color(if ok_changer && ok_slider { TEXT_DIM } else { GOLD }));
    }

    /// The picked parts, in words (a new changer takes them).
    fn picked_words(&self, ui: &mut Ui) -> bool {
        let names: Vec<&str> = self.draw.doc.layers.iter().filter(|l| self.rigs.selected.contains(&l.id)).map(|l| l.name.as_str()).collect();
        if names.is_empty() {
            ui.label(RichText::new("Pick the parts first: click them in the list on the left, or on the picture.").size(11.5).color(GOLD));
            false
        } else {
            let shown: Vec<&str> = names.iter().take(4).copied().collect();
            let more = if names.len() > 4 { format!(" and {} more", names.len() - 4) } else { String::new() };
            ui.label(RichText::new(format!("Parts: {}{more}", shown.join(", "))).size(11.5).color(TEXT_DIM));
            true
        }
    }

    /// A new changer, set up in a window: name, what it sits inside, and what is special about its kind.
    fn rig_new_changer_body(&mut self, ui: &mut Ui, now: f64, kind: NewKind) {
        ui.label(RichText::new(kind.about()).size(11.5).color(TEXT_DIM));
        let have = self.picked_words(ui);
        let rig = self.draw.doc.rig.clone();
        let n = rig.changers.len() + 1;
        let default_name = format!("{} {n}", kind.word());
        ui.horizontal(|ui| {
            ui.label(RichText::new("Name").size(12.0).color(TEXT_DIM));
            ui.add(egui::TextEdit::singleline(&mut self.rigs.bench.name).hint_text(default_name.clone()).desired_width(180.0).char_limit(40));
        });
        // what it sits inside
        let parents: Vec<(Id, String)> = rig.changers.iter().map(|c| (c.id, c.name.clone())).collect();
        ui.horizontal(|ui| {
            ui.label(RichText::new("Inside").size(12.0).color(TEXT_DIM));
            let cur = self.rigs.bench.parent.filter(|p| parents.iter().any(|x| x.0 == *p));
            egui::ComboBox::from_id_salt("new_changer_parent")
                .selected_text(cur.and_then(|p| parents.iter().find(|x| x.0 == p)).map(|x| x.1.clone()).unwrap_or_else(|| "nothing: on its own".into()))
                .width(180.0)
                .show_ui(ui, |ui| {
                    if ui.selectable_label(cur.is_none(), "nothing: on its own").clicked() {
                        self.rigs.bench.parent = None;
                    }
                    for (id, name) in &parents {
                        if ui.selectable_label(cur == Some(*id), name).clicked() {
                            self.rigs.bench.parent = Some(*id);
                        }
                    }
                });
        });
        // what is special about the kind
        match kind {
            NewKind::Warp => {
                ui.horizontal(|ui| {
                    ui.label(RichText::new("Grid").size(12.0).color(TEXT_DIM));
                    ui.add(egui::DragValue::new(&mut self.rigs.cols_rows.0).range(1..=rig::MAX_WARP).suffix(" across"));
                    ui.add(egui::DragValue::new(&mut self.rigs.cols_rows.1).range(1..=rig::MAX_WARP).suffix(" down"));
                });
                ui.checkbox(&mut self.rigs.bench.smooth, "Bend smoothly (a curve through the points)");
            }
            NewKind::Line => {
                ui.horizontal(|ui| {
                    ui.label(RichText::new("Points on the curve").size(12.0).color(TEXT_DIM));
                    ui.add(egui::DragValue::new(&mut self.rigs.bench.line_points).range(2..=16));
                });
            }
            NewKind::Flip => {
                ui.checkbox(&mut self.rigs.bench.flip_vertical, "Turn over top to bottom (not left to right)");
            }
            NewKind::Rotation => {
                ui.label(RichText::new("The pivot starts in the middle of the parts. Drag it on the picture afterwards (Changers tool, Move the pivot).").size(11.5).color(TEXT_DIM));
            }
            NewKind::Path => {
                ui.label(RichText::new("Press the button, then drag on the picture to draw each path.").size(11.5).color(TEXT_DIM));
            }
            NewKind::Pins => {
                ui.label(RichText::new("Four pins start in the corners. Click the picture to add more.").size(11.5).color(TEXT_DIM));
            }
            _ => {}
        }
        if kind != NewKind::Path {
            // a part without a mesh gets one: nothing has to be done first
            let lacking = self.rigs.selected.iter().filter(|id| self.draw.doc.rig.mesh_of(**id).is_none()).count();
            if lacking > 0 && have {
                ui.label(RichText::new(format!("{lacking} of the parts have no mesh yet: they get one when the changer is made.")).size(11.5).color(TEXT_DIM));
            }
        }
        ui.add_space(4.0);
        let label = if kind == NewKind::Path { "Draw paths on the picture".to_string() } else { format!("Make the {} changer", kind.word()) };
        if ui.add_enabled(have, egui::Button::new(RichText::new(label).strong())).clicked() {
            self.make_new_changer(kind, now);
        }
    }

    /// Make the changer that the window describes, around the picked parts.
    pub(super) fn make_new_changer(&mut self, kind: NewKind, now: f64) {
        if kind == NewKind::Path {
            self.rigs.mode = super::rigtab::Mode::DrawPath;
            self.rigs.stroke.clear();
            return;
        }
        let sel: Vec<LayerId> = self.rigs.selected.clone();
        // a part without a mesh gets one first
        let lacking: Vec<LayerId> = sel.iter().copied().filter(|id| self.draw.doc.rig.mesh_of(*id).is_none()).collect();
        if !lacking.is_empty() {
            self.make_meshes(lacking, now);
        }
        let Some(b) = self.selection_box() else {
            self.rigs.message = Some("Nothing to make the changer around: pick parts that have paint.".into());
            return;
        };
        let center = [(b[0] + b[2]) / 2.0, (b[1] + b[3]) / 2.0];
        let left_mid = [b[0], center[1]];
        let bench = &self.rigs.bench;
        let n = self.draw.doc.rig.changers.len() + 1;
        let name = if bench.name.trim().is_empty() { format!("{} {n}", kind.word()) } else { bench.name.trim().to_owned() };
        let parent = bench.parent.filter(|p| self.draw.doc.rig.changer(*p).is_some());
        let (cols, rows) = self.rigs.cols_rows;
        let (smooth, vertical, points) = (bench.smooth, bench.flip_vertical, bench.line_points.clamp(2, 16));
        let k = match kind {
            NewKind::Rotation => Kind::Rotation { pivot: bench.pivot.unwrap_or(center) },
            NewKind::Warp => Kind::Warp { rect: b, cols, rows },
            NewKind::Line => {
                let x = center[0];
                let pts: Vec<[f32; 2]> = (0..points).map(|i| [x, b[1] + (b[3] - b[1]) * i as f32 / (points - 1) as f32]).collect();
                Kind::Line { pts, width: ((b[2] - b[0]) / 2.0).max(20.0) }
            }
            NewKind::Soft => Kind::Soft { center, radius: ((b[2] - b[0]).max(b[3] - b[1]) / 2.0).max(20.0) },
            NewKind::Pins => Kind::Pins { pts: vec![[b[0], b[1]], [b[2], b[1]], [b[0], b[3]], [b[2], b[3]]] },
            NewKind::Move => Kind::Move,
            NewKind::Scale => Kind::Scale { center },
            NewKind::Flip => Kind::Flip { center, vertical },
            NewKind::Fade => Kind::Fade,
            NewKind::Order => Kind::Order,
            NewKind::Wave => Kind::Wave { center: left_mid },
            NewKind::Path => return,
        };
        let mut new_id = 0;
        let word = kind.word();
        self.rig_edit(&format!("New {word} changer"), now, |r| {
            let id = r.add_changer(&name, k, parent);
            if kind == NewKind::Warp && smooth {
                r.smooth.push(id);
            }
            for m in r.meshes.iter_mut().filter(|m| sel.contains(&m.layer)) {
                m.parent = Some(id);
            }
            new_id = id;
        });
        self.rigs.sel_changer = Some(new_id);
        self.rigs.warp_edit = None;
        self.rigs.vals_edit = None;
        self.rigs.vals_live = false;
        if kind == NewKind::Pins {
            self.rigs.mode = super::rigtab::Mode::AddPin;
        }
        self.rigs.bench.name.clear();
        self.rigs.message = Some(format!("{name} is made. Drag its points on the picture, then say which slider moves it (Shapes)."));
    }

    /// The Physics tab's window: the picture with the physics running.
    fn physics_check_body(&mut self, ui: &mut Ui) {
        ui.horizontal_wrapped(|ui| {
            ui.checkbox(&mut self.rigs.phys.on, "Physics on");
            if ui.button("Reset").on_hover_text("Things hang still again").clicked() {
                self.rigs.phys.sim.reset();
                self.rigs.phys.offs.clear();
            }
        });
        // the wind, to see how things swing in it
        let rig = self.draw.doc.rig.clone();
        let mut world = rig.world.clone();
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing = vec2(4.0, 4.0);
            ui.label(RichText::new("Wind").size(12.0).color(TEXT_DIM));
            for k in crate::rig::physics::Wind::ALL {
                if widgets::chip(ui, k.label(), world.wind == k).clicked() {
                    world.wind = k;
                }
            }
        });
        if world != rig.world {
            let now = ui.ctx().input(|i| i.time);
            self.rig_edit("Wind", now, |r| r.world = world);
        }
        ui.label(RichText::new("The sliders on the left move the character here, and the things you made swing as they would in the end.").size(11.5).color(TEXT_DIM));
        match &self.rigs.image {
            Some(t) => {
                let avail = ui.available_width().min(380.0);
                let s = t.size_vec2();
                let k = (avail / s.x).min(560.0 / s.y).min(1.0).max(0.05);
                ui.image((t.id(), s * k));
            }
            None => {
                ui.label(RichText::new("There is nothing to draw yet.").color(TEXT_DIM));
            }
        }
    }

    /// The parts, in a column of their own (the parameters are the column next to it). Returns the part that was clicked.
    pub(super) fn rig_left_panel(&mut self, ui: &mut Ui, ctx: &egui::Context, _now: f64, id: &str, show_params: bool, physics_labels: bool) -> Option<(LayerId, bool, bool)> {
        let mut picked: Option<(LayerId, bool, bool)> = None;
        let mut toggle: Option<LayerId> = None;
        section_title(ui, "Parts");
        ui.label(RichText::new("Pick the parts to rig or move. The eye hides a layer. No tags are needed.").size(11.5).color(TEXT_DIM));
        self.rig_visibility_buttons(ui);
        let full = ui.available_height();
        let parts_h = full;
        egui::ScrollArea::vertical().id_salt((id, "parts")).max_height(parts_h).show(ui, |ui| {
            for l in self.draw.doc.layers.iter().filter(|l| parts::is_part_layer(l)) {
                let on = self.rigs.selected.contains(&l.id);
                let (rect, resp) = ui.allocate_exact_size(vec2(ui.available_width(), 24.0), Sense::click());
                if on {
                    ui.painter().rect_filled(rect, 4.0, HOVER);
                }
                let dot = l.part.map(|p| p.color()).unwrap_or(Color32::from_gray(70));
                ui.painter().circle_filled(rect.left_center() + vec2(34.0, 0.0), 5.0, dot);
                ui.painter().text(rect.left_center() + vec2(46.0, 0.0), Align2::LEFT_CENTER, &l.name, egui::FontId::proportional(13.0), if l.visible { TEXT } else { TEXT_DIM });
                let mark = if physics_labels {
                    self.draw.doc.rig.physics.iter().find(|c| self.draw.doc.rig.mesh_of(l.id).and_then(|m| m.parent) == Some(c.changer)).map(|c| c.kind.label().to_string())
                } else {
                    self.draw.doc.rig.mesh_of(l.id).map(|_| "mesh".to_string())
                };
                if let Some(m) = mark {
                    ui.painter().text(rect.right_center() - vec2(6.0, 0.0), Align2::RIGHT_CENTER, m, egui::FontId::proportional(11.0), VENOM);
                }
                if super::rigtab::eye_toggle(ui, rect, l.id, l.visible) {
                    toggle = Some(l.id);
                } else if resp.clicked() {
                    let m = ctx.input(|i| i.modifiers);
                    picked = Some((l.id, m.command || m.ctrl, m.shift));
                }
            }
        });
        if let Some(id) = toggle {
            self.toggle_layer_visible(id);
        }
        let _ = show_params;
        picked
    }

    /// The parameters (all the sliders), in a column of their own next to the parts.
    pub(super) fn rig_params_column(&mut self, ui: &mut Ui, now: f64, id: &str) {
        section_title(ui, "Parameters");
        egui::ScrollArea::vertical().id_salt((id, "params")).show(ui, |ui| self.rig_params_body(ui, now));
    }
}

#[cfg(test)]
impl SeviathApp {
    /// Open a tool of the top bar by name: meshes, skin, changers, glue, shapes, check, new:<Kind>, or 3d (the 3D page).
    pub fn test_open_tool(&mut self, which: &str) {
        self.rigs.mode = if which == "brush" { super::rigtab::Mode::Brush } else { super::rigtab::Mode::Look };
        let b = &mut self.rigs.bench;
        b.view3d = which == "3d";
        b.open = match which {
            "meshes" => Some(Tool::Meshes),
            "skin" => Some(Tool::Skin),
            "changers" => Some(Tool::Changers),
            "glue" => Some(Tool::Glue),
            "shapes" => Some(Tool::Shapes),
            "check" => Some(Tool::PhysicsCheck),
            "groups" => Some(Tool::PhysicsGroups),
            "brush" => Some(Tool::Brush),
            w if w.starts_with("new:") => NewKind::ALL.iter().copied().find(|k| k.word() == &w[4..]).map(Tool::New),
            _ => None,
        };
    }
}
