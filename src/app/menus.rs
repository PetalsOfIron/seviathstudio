// SPDX-License-Identifier: GPL-3.0-or-later
// Dropdown menus that open when the mouse rests on their name (and close when it leaves): the bar of the Draw tab and the bar of the Rig tab, where every tool,
// every switch and the file options are one hover away.

use eframe::egui::{self, Margin, RichText, Sense, Ui, vec2};

use super::SeviathApp;
use super::rigbench::{NewKind, Tool};
use crate::theme::*;
use crate::widgets::{self, HelpExt};

/// Which hover menu is open (the id of its name).
fn open_id() -> egui::Id {
    egui::Id::new("hover_menu_open")
}

/// One line of a dropdown: a tick when it is on, the name, and the help on hover. True when it was clicked.
pub(super) fn menu_item(ui: &mut Ui, label: &str, on: bool, tip: &str) -> bool {
    let (rect, resp) = ui.allocate_exact_size(vec2(ui.available_width().max(230.0), 24.0), Sense::click());
    if ui.is_rect_visible(rect) {
        let p = ui.painter();
        if resp.hovered() {
            p.rect_filled(rect, 4.0, HOVER);
        }
        if on {
            p.circle_filled(rect.left_center() + vec2(10.0, 0.0), 4.0, EMERALD);
        }
        p.text(rect.left_center() + vec2(24.0, 0.0), egui::Align2::LEFT_CENTER, label, egui::FontId::proportional(13.0), TEXT);
    }
    resp.help(tip).clicked()
}

/// A small grey heading inside a dropdown.
pub(super) fn menu_heading(ui: &mut Ui, text: &str) {
    ui.add_space(2.0);
    ui.label(RichText::new(text).size(11.0).color(TEXT_DIM));
}

impl SeviathApp {
    /// A menu that opens when the pointer rests on its name, and stays open while the pointer is on the name or on the list. A click in the list closes it.
    pub(super) fn hover_menu(&mut self, ui: &mut Ui, label: &str, active: bool, items: impl FnOnce(&mut Self, &mut Ui)) {
        let ctx = ui.ctx().clone();
        let id = egui::Id::new(("hover_menu", label));
        let head = widgets::chip(ui, label, active);
        let current: Option<egui::Id> = ctx.data(|d| d.get_temp(open_id()));
        if head.hovered() || head.clicked() {
            ctx.data_mut(|d| d.insert_temp(open_id(), id));
        } else if current != Some(id) {
            return;
        }
        let area = egui::Area::new(id.with("list")).order(egui::Order::Foreground).fixed_pos(head.rect.left_bottom()).constrain(true).show(&ctx, |ui| {
            egui::Frame::popup(ui.style()).inner_margin(Margin::same(6)).show(ui, |ui| {
                ui.set_min_width(240.0);
                items(self, ui);
            });
        });
        let zone = head.rect.union(area.response.rect).expand(4.0);
        let over = ctx.pointer_hover_pos().is_some_and(|p| zone.contains(p));
        let popup = egui::Popup::is_any_open(&ctx);
        let clicked = ctx.input(|i| i.pointer.any_released()) && over && !head.hovered();
        if !popup && (!over || clicked) {
            ctx.data_mut(|d| d.remove::<egui::Id>(open_id()));
        }
        ctx.request_repaint();
    }

    /// Open or close a rig tool (the same as pressing its name on the bench).
    pub(super) fn pick_tool(&mut self, tool: Tool) {
        let bench = &mut self.rigs.bench;
        bench.open = if bench.open == Some(tool) { None } else { Some(tool) };
        bench.name.clear();
        bench.parent = None;
        bench.pivot = None;
    }

    fn tool_item(&mut self, ui: &mut Ui, tool: Tool, label: &str, tip: &str) {
        if menu_item(ui, label, self.rigs.bench.open == Some(tool), tip) {
            self.pick_tool(tool);
        }
    }

    /// The tools in a panel of their own under the dropdowns (it can be folded away with the button of the dropdown bar).
    pub(super) fn rig_tool_panel(&mut self, ui: &mut Ui, id: &str) {
        if self.rigs.hide_tools {
            return;
        }
        let before = self.rigs.bench.open;
        egui::Panel::top(id.to_owned()).resizable(false).frame(egui::Frame::new().fill(PANEL).inner_margin(Margin::symmetric(8, 4))).show(ui, |ui| {
            ui.horizontal_wrapped(|ui| {
                ui.spacing_mut().item_spacing = vec2(4.0, 4.0);
                self.rig_tool_strip(ui);
            });
        });
        self.tool_changed(before);
    }

    /// Every tool and every kind of changer as a button on one line, so none of them is hidden in a menu.
    pub(super) fn rig_tool_strip(&mut self, ui: &mut Ui) {
        let open = self.rigs.bench.open;
        let mut pick: Option<Tool> = None;
        let mut chip = |ui: &mut Ui, tool: Tool, label: &str, tip: &str| {
            if widgets::chip(ui, label, open == Some(tool)).help(tip).clicked() {
                pick = Some(tool);
            }
        };
        chip(ui, Tool::Meshes, "Meshes", "Cut layers into triangles so they can bend");
        chip(ui, Tool::Points, "Points", "The points of the mesh of the picked part, one by one");
        chip(ui, Tool::Skin, "Skin and bones", "One mesh over the whole drawing, bones on top, and the weights of every bone");
        ui.separator();
        for k in NewKind::ALL {
            chip(ui, Tool::New(k), k.word(), k.about());
        }
        ui.separator();
        chip(ui, Tool::Changers, "Changers", "Every changer there is: pick one, put it inside another, lock it, delete it, copy it to the other side");
        chip(ui, Tool::Brush, "Brush", "Push the points of the picked changer with a brush");
        chip(ui, Tool::Shapes, "Shapes", "Say how the picked slider moves the picked changer");
        chip(ui, Tool::Glue, "Glue", "Keeps the seam of two parts closed when they move differently");
        if let Some(t) = pick {
            self.pick_tool(t);
        }
    }

    /// The dropdowns of the Rig tab (and of the animation and the physics pages, which are part of it).
    pub(super) fn rig_menus(&mut self, ui: &mut Ui, with_edit_here: bool) {
        let open = self.rigs.bench.open;
        self.file_menu(ui);
        self.hover_menu(ui, "Mesh", matches!(open, Some(Tool::Meshes | Tool::Points | Tool::Glue)), |s, ui| {
            s.tool_item(ui, Tool::Meshes, "Meshes", "Cut layers into triangles so they can bend");
            s.tool_item(ui, Tool::Points, "Points", "The points of the mesh of the picked part, one by one: pick with a click or a box, move, weld, line up, relax, turn an edge, add, delete, triangulate");
            s.tool_item(ui, Tool::Glue, "Glue", "Keeps the seam of two parts closed when they move differently");
        });
        self.hover_menu(ui, "Bones", matches!(open, Some(Tool::Skin)), |s, ui| {
            s.tool_item(ui, Tool::Skin, "Skin and bones", "One mesh over the whole drawing, bones on top, and the weights of every bone. Opens the window with the weight brush, the automatic weights and the volume option");
        });
        // skinning is the flowing of hair and cloth
        self.hover_menu(ui, "Skinning", matches!(open, Some(Tool::PhysicsParts)), |s, ui| {
            s.tool_item(ui, Tool::PhysicsParts, "Hair and cloth flowing", "Pick the hair or the cloth, say what it is and make it flow: it swings and lags behind the head and the body");
        });
        self.hover_menu(ui, "New changer", matches!(open, Some(Tool::New(_))), |s, ui| {
            for k in NewKind::ALL {
                s.tool_item(ui, Tool::New(k), k.word(), k.about());
            }
        });
        self.hover_menu(ui, "Edit", matches!(open, Some(Tool::Changers | Tool::Brush | Tool::Shapes)), |s, ui| {
            s.tool_item(ui, Tool::Changers, "Changers", "Every changer there is: pick one, put it inside another (or drag it onto another), lock it, delete it, copy it to the other side");
            s.tool_item(ui, Tool::Brush, "Brush", "Push the points of the picked changer with a brush (warp, line, pins, paths, soft): pick the changer and a slider, then drag over the picture");
            s.tool_item(ui, Tool::Shapes, "Shapes", "Say how the picked slider moves the picked changer: save the shape at each key point");
        });
        // the physics opens in windows of its own: the rest of the page stays as it is
        self.hover_menu(ui, "Physics", matches!(open, Some(Tool::PhysicsGroups | Tool::PhysicsCheck)), |s, ui| {
            s.tool_item(ui, Tool::PhysicsGroups, "Physics settings", "The groups of swinging chains: which sliders push a chain (inputs), how it hangs and swings (pendulum), which sliders it moves (outputs)");
            s.tool_item(ui, Tool::PhysicsCheck, "Check the physics", "A window with the picture and the physics running, to look at them on their own");
        });
        let more = matches!(open, Some(Tool::Zones | Tool::MeshGroups | Tool::Anchors | Tool::LitSettings | Tool::Notes));
        self.hover_menu(ui, "More", more, |s, ui| {
            s.tool_item(ui, Tool::Zones, "Click zones", "Boxes or ovals in front of everything that take clicks in the program that shows the model: pat the head, poke a cheek");
            s.tool_item(ui, Tool::MeshGroups, "Mesh groups", "A part carries other parts as it bends (a mole, a blush, brows on the face)");
            s.tool_item(ui, Tool::Anchors, "Anchors", "Temporary points on the picture to measure from or to put a pivot on; they never go into an exported model");
            s.tool_item(ui, Tool::LitSettings, "Lit view settings", "The light, its strength, the rim light and the shine, and which layers are maps");
            s.tool_item(ui, Tool::Notes, "Memory and notes", "How much memory the rig uses, specks, saving the rig as text, and the notes that go in the model");
            if menu_item(ui, "Pre-rig improver", false, "Finds stray pixels, tiny holes, nearly invisible pixels and light or dark edges in the drawing. Nothing changes until you say so") {
                s.open_prerig();
            }
        });
        let animate = self.rigs.animate;
        self.hover_menu(ui, "Animation", animate, |s, ui| {
            if menu_item(ui, "Animation of the rig", animate, "Turns on the timeline, the tracks of the sliders, the clips, the sound and the saving of video. Frame by frame animation is the timeline of the Draw tab") {
                s.rigs.animate = !animate;
                if !animate {
                    s.rigs.physics_page = false;
                }
            }
        });
        let view = self.rigs.bench.edit_here || self.rigs.lit_on || self.rigs.show_bounds || self.rigs.show_stress || self.rigs.show_swing || self.rigs.hit_show;
        self.hover_menu(ui, "View", view, |s, ui| {
            if with_edit_here {
                let on = s.rigs.bench.edit_here;
                if menu_item(ui, "Edit the rig on the picture", on, "Shows the rig editor in the middle (drag the points of a changer, pick parts by clicking them) instead of the finished picture") {
                    s.rigs.bench.edit_here = !on;
                }
            }
            let on = s.rigs.lit_on;
            if menu_item(ui, "Lit view", on, "Shows the picture lit by one light, with the layers named normal, emissive, rough and occlusion as maps. Nothing in the layers is changed") {
                s.rigs.lit_on = !on;
            }
            let on = s.rigs.show_bounds;
            if menu_item(ui, "Bounds of every layer", on, "A rectangle round the paint of every layer, so forgotten leftovers show up") {
                s.rigs.show_bounds = !on;
            }
            let on = s.rigs.show_stress;
            if menu_item(ui, "Stretch of the triangles", on, "Colors the triangles by how much they are stretched or squeezed: blue none, red a lot") {
                s.rigs.show_stress = !on;
            }
            let on = s.rigs.show_swing;
            if menu_item(ui, "The swing of the chains", on, "Draws the chains of the physics with arrows for how fast each weight goes; drag a weight to yank it") {
                s.rigs.show_swing = !on;
            }
            let on = s.rigs.hit_show;
            if menu_item(ui, "Click zones", on, "Draws the outlines of the click zones on the picture") {
                s.rigs.hit_show = !on;
            }
        });
    }

    /// The dropdowns of the Draw tab: the animation (frame by frame) lives here.
    pub(super) fn draw_menus(&mut self, ui: &mut Ui) {
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing = vec2(4.0, 4.0);
            self.file_menu(ui);
            self.filter_menu(ui);
            self.layout_menu(ui);
            let open = self.rigs.anim.timeline;
            self.hover_menu(ui, "Animate", open, |s, ui| {
                let on = s.rigs.anim.timeline;
                if menu_item(ui, "Timeline", on, "Shows or hides the timeline at the bottom: every layer can be a frame, like the timeline of a photo editor") {
                    s.rigs.anim.timeline = !on;
                    s.rigs.anim.playing = false;
                }
                menu_heading(ui, "Frames");
                if menu_item(ui, "Frames from the layers", false, "Makes one frame of every layer of the picture, the bottom one first") {
                    s.rigs.anim.timeline = true;
                    s.timeline_from_layers();
                }
                if menu_item(ui, "New frame", false, "A new empty drawing after the last frame") {
                    s.rigs.anim.timeline = true;
                    s.timeline_new_frame(false);
                }
                if menu_item(ui, "Copy the frame", false, "A new frame with a copy of the drawing that shows now") {
                    s.rigs.anim.timeline = true;
                    s.timeline_new_frame(true);
                }
                menu_heading(ui, "Playing");
                let playing = s.rigs.anim.playing;
                if menu_item(ui, if playing { "Stop" } else { "Play" }, playing, "Plays the frames at their speed") {
                    s.rigs.anim.timeline = true;
                    s.rigs.anim.playing = !playing;
                    s.rigs.anim.dir = 1.0;
                }
                let onion = s.draw.doc.rig.anim.onion.on;
                if menu_item(ui, "Onion skin", onion, "Shows the frames before (red) and after (green) faintly, to draw the next one by") {
                    s.anim_mut("Onion skin").onion.on = !onion;
                }
                menu_heading(ui, "The rig");
                if menu_item(ui, "Animate the rig...", false, "Goes to the Rig tab with its animation on: tracks of the sliders, clips, sound, saving video") {
                    s.tab = crate::tabs::Tab::Rig;
                    s.rigs.animate = true;
                    s.rigs.physics_page = false;
                }
            });
        });
    }

    /// A strip of dropdowns on top of the Draw tab.
    pub(super) fn draw_menu_bar(&mut self, ui: &mut Ui) {
        egui::Panel::top("draw_menus").exact_size(34.0).resizable(false).frame(egui::Frame::new().fill(PANEL).inner_margin(Margin::symmetric(8, 4))).show(ui, |ui| {
            self.draw_menus(ui);
        });
    }
}
