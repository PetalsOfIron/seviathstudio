// SPDX-License-Identifier: GPL-3.0-or-later
// The workbench for rigging by hand, the same in the Rig, Physics and Animate tabs: a row of tools along the top (every kind of changer, meshes,
// skin and bones, glue, shapes), the parts and the parameters (the sliders) on the left, and a look at the rig on the right (Rig tab only).
// A tool opens a small window of its own where its details are set: the grid of a warp, the pivot of a rotation, what it sits inside. Nothing
// here needs the parts to be tagged: tags only name the parts for the physics and the expressions.

use crate::widgets::HelpExt;
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
    pub(super) const ALL: [NewKind; 12] = [NewKind::Rotation, NewKind::Warp, NewKind::Line, NewKind::Soft, NewKind::Path, NewKind::Pins, NewKind::Move, NewKind::Scale, NewKind::Flip, NewKind::Fade, NewKind::Order, NewKind::Wave];

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

    pub(super) fn about(self) -> &'static str {
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
    /// The points of the mesh of the picked part, one by one.
    Points,
    /// The Physics tab: a window to look at the physics on its own.
    PhysicsCheck,
    /// The Physics tab: the groups of physics (inputs, pendulum, outputs).
    PhysicsGroups,
    /// Make parts swing: the kinds, the material and the chains of the picked parts, in a window of its own.
    PhysicsParts,
    /// Things that are not the base of a rig, each in a window of its own: click zones, mesh groups, anchors, the settings of the lit view, memory and notes.
    Zones,
    MeshGroups,
    Anchors,
    LitSettings,
    Notes,
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
            Tool::Points => "Mesh points".into(),
            Tool::PhysicsCheck => "Check the physics".into(),
            Tool::PhysicsGroups => "Physics settings".into(),
            Tool::PhysicsParts => "Hair and cloth flowing".into(),
            Tool::Zones => "Click zones".into(),
            Tool::MeshGroups => "Mesh groups".into(),
            Tool::Anchors => "Anchors".into(),
            Tool::LitSettings => "Lit view".into(),
            Tool::Notes => "Memory and notes".into(),
        }
    }
}

/// What the workbench remembers.
pub struct Bench {
    /// The tool window that is open.
    pub open: Option<Tool>,
    /// Physics and Animate tabs: the picture in the middle is the one the rig is edited on (drag its points there).
    pub edit_here: bool,
    /// The new changer being set up.
    pub name: String,
    pub parent: Option<Id>,
    pub line_points: usize,
    pub smooth: bool,
    pub flip_vertical: bool,
    pub pivot: Option<[f32; 2]>,
}

impl Default for Bench {
    fn default() -> Self {
        Self { open: None, edit_here: false, name: String::new(), parent: None, line_points: 5, smooth: true, flip_vertical: false, pivot: None }
    }
}

impl SeviathApp {
    /// The row of tools along the top.
    pub(super) fn rig_toolbar(&mut self, ui: &mut Ui, with_physics: bool, with_edit_here: bool) {
        let before = self.rigs.bench.open;
        self.rig_toolbar_row(ui, with_physics, with_edit_here);
        self.tool_changed(before);
    }

    /// Another tool was picked: what the last one was waiting for on the picture (a path, a pin, a pivot) is over.
    pub(super) fn tool_changed(&mut self, before: Option<Tool>) {
        if self.rigs.bench.open != before {
            self.rigs.mode = match self.rigs.bench.open {
                Some(Tool::Brush) => super::rigtab::Mode::Brush,
                Some(Tool::Points) => super::rigtab::Mode::Points,
                _ => super::rigtab::Mode::Look,
            };
            self.rigs.stroke.clear();
            self.rigs.pt_sel.clear();
        }
    }

    fn rig_toolbar_row(&mut self, ui: &mut Ui, _with_physics: bool, with_edit_here: bool) {
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing = vec2(4.0, 4.0);
            self.rig_menus(ui, with_edit_here);
            let hide = self.rigs.hide_tools;
            if widgets::chip(ui, if hide { "Show the tools" } else { "Hide the tools" }, false).help("Folds the line of tools below away to leave more room for the picture, or brings it back").clicked() {
                self.rigs.hide_tools = !hide;
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
            .order(egui::Order::Foreground)
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
                    Tool::Points => self.rig_points_body(ui, now),
                    Tool::PhysicsCheck => self.physics_check_body(ui),
                    Tool::PhysicsGroups => self.physics_groups_body(ui, now),
                    Tool::PhysicsParts => self.physics_side(ui, now),
                    Tool::Zones => self.rig_hit_section(ui, now),
                    Tool::MeshGroups => self.rig_group_section(ui, now),
                    Tool::Anchors => self.rig_anchor_section(ui, now),
                    Tool::LitSettings => self.rig_lit_section(ui),
                    Tool::Notes => self.rig_memory_section(ui, now),
                });
            });
        if !open {
            self.rigs.bench.open = None;
        }
    }

    /// The push brush: how big and how strong.
    fn rig_brush_body(&mut self, ui: &mut Ui) {
        ui.horizontal(|ui| {
            for (m, label, tip) in [
                (super::rigtab::BrushMode::Push, "Push", "The points under the brush follow the drag, like pushing and pulling the shape"),
                (super::rigtab::BrushMode::Expand, "Expand", "The points under the brush bulge away from its middle while you drag or hold: a fisheye swelling (a cheek, a ball)"),
                (super::rigtab::BrushMode::Pinch, "Pinch", "The points under the brush are drawn toward its middle: the other way of a swelling"),
            ] {
                if widgets::chip(ui, label, self.rigs.brush_mode == m).help(tip).clicked() {
                    self.rigs.brush_mode = m;
                }
            }
        });
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
                    if ui.selectable_label(cur.is_none(), "nothing: on its own").help("Nothing moves it: it works on its own.").clicked() {
                        self.rigs.bench.parent = None;
                    }
                    for (id, name) in &parents {
                        if ui.selectable_label(cur == Some(*id), name).help("Pick this one.").clicked() {
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
                ui.checkbox(&mut self.rigs.bench.smooth, "Bend smoothly (a curve through the points)").help("The line is a smooth curve through the points, not straight pieces.");
            }
            NewKind::Line => {
                ui.horizontal(|ui| {
                    ui.label(RichText::new("Points on the curve").size(12.0).color(TEXT_DIM));
                    ui.add(egui::DragValue::new(&mut self.rigs.bench.line_points).range(2..=16));
                });
            }
            NewKind::Flip => {
                ui.checkbox(&mut self.rigs.bench.flip_vertical, "Turn over top to bottom (not left to right)").help("Flip top to bottom instead of left to right.");
            }
            NewKind::Rotation => {
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
            ui.checkbox(&mut self.rigs.phys.on, "Physics on").help("Turn the physics on or off.");
            if ui.button("Reset").help("Things hang still again").clicked() {
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
                if widgets::chip(ui, k.label(), world.wind == k).help("Pick how the wind blows.").clicked() {
                    world.wind = k;
                }
            }
        });
        if world != rig.world {
            let now = ui.ctx().input(|i| i.time);
            self.rig_edit("Wind", now, |r| r.world = world);
        }
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
        self.rigs.mode = match which {
            "brush" => super::rigtab::Mode::Brush,
            "points" => super::rigtab::Mode::Points,
            _ => super::rigtab::Mode::Look,
        };
        let b = &mut self.rigs.bench;
        b.open = match which {
            "meshes" => Some(Tool::Meshes),
            "skin" => Some(Tool::Skin),
            "changers" => Some(Tool::Changers),
            "glue" => Some(Tool::Glue),
            "shapes" => Some(Tool::Shapes),
            "check" => Some(Tool::PhysicsCheck),
            "groups" => Some(Tool::PhysicsGroups),
            "brush" => Some(Tool::Brush),
            "points" => Some(Tool::Points),
            w if w.starts_with("new:") => NewKind::ALL.iter().copied().find(|k| k.word() == &w[4..]).map(Tool::New),
            _ => None,
        };
    }
}
