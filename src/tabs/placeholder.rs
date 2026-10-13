// SPDX-License-Identifier: GPL-3.0-or-later
// Page for tabs that are not ported to the Rust version yet: says plainly what will be there.

use eframe::egui::{self, Align2, CornerRadius, FontId, Margin, RichText, Sense, Stroke, Ui, Vec2, pos2, vec2};

use super::Tab;
use crate::icons;
use crate::snake;
use crate::theme::*;

struct Plan {
    title: &'static str,
    subtitle: &'static str,
    features: &'static [(&'static str, &'static str)],
}

fn plan(tab: Tab) -> Plan {
    match tab {
        Tab::Parts => Plan {
            title: "Parts",
            subtitle: "Organize before rigging: layer list on the left, model in the middle, tags on the right.",
            features: &[
                ("Tag layers as face, hair, body, clothing, accessories", "Phase 1"),
                ("Automatic suggestions by name, shape and position", "Phase 1"),
                ("L/R pairs linked automatically, color codes on the model", "Phase 1"),
                ("Checklist: inside of mouth, layer behind hair, untagged layers", "Phase 1"),
                ("Pre-rig improver: stray pixels, holes, edge halos, color expansion", "Phase 2"),
                ("Tag layouts saved as templates, \"Ready to rig\" button", "Phase 2"),
            ],
        },
        Tab::Rig => Plan {
            title: "Rig",
            subtitle: "Changers, parameters and keyforms. You give the direction, the program does the routine.",
            features: &[
                ("Mesh, Rotation and Line changers in a simple hierarchy", "Phase 1"),
                ("Parameters with keyforms, combined head X × Y, all corners at once", "Phase 1"),
                ("\"This is a face\", then \"Rig face\": head XYZ, eyes, brows, mouth A/I/U/E/O", "Phase 2"),
                ("Other-side generation, asymmetry mode, pairs", "Phase 1"),
                ("Handles instead of 200 mesh points, beginner / pro mode", "Phase 2"),
                ("Auto-play parameters and quality checker with Fix buttons", "Phase 2"),
            ],
        },
        Tab::Physics => Plan {
            title: "Physics",
            subtitle: "Tick what should move. The type is recognized and physics applied by length and material.",
            features: &[
                ("Hair, cloth, hanging accessory, chain, soft, elastic, animal parts, solid", "Phase 2"),
                ("Stiffness from root to tip, slight random difference per strand", "Phase 2"),
                ("Test panel: head shake, jump, turn, wind themes", "Phase 2"),
                ("Collision groups and physics materials (\"this outfit is silk\")", "Phase 3"),
            ],
        },
        Tab::Animate => Plan {
            title: "Animate",
            subtitle: "Animate by moving parameters over time — no new frames to draw.",
            features: &[
                ("Timeline with keyframes, Bézier easing graphs, onion skin, loops", "Phase 2"),
                ("Record with your face, lip sync from audio", "Phase 3"),
                ("Animation auto-generation from a target pose", "Phase 3"),
                ("Export MP4, WebM with alpha, GIF, PNG sequence, sprite sheet, Lottie", "Phase 2"),
            ],
        },
        Tab::Stream => Plan {
            title: "Stream",
            subtitle: "Use the model live: tracking, expressions on hotkeys, props and outputs.",
            features: &[
                ("Camera face and hand tracking, VMC / OpenSeeFace input", "Phase 3"),
                ("Auto breathing and blinking, look at mouse", "Phase 3"),
                ("Expressions and props on hotkeys, scenes", "Phase 3"),
                ("Transparent window, OBS source, virtual camera, Spout2", "Phase 3"),
            ],
        },
        Tab::Draw => unreachable!("Draw tab is implemented"),
    }
}

pub fn ui(ui: &mut Ui, tab: Tab) {
    let plan = plan(tab);
    let accent = tab.accent();
    egui::CentralPanel::default()
        .frame(egui::Frame::new().fill(BG))
        .show(ui, |ui| {
            let full = ui.max_rect();
            snake::scales(ui.painter(), full, EMERALD.gamma_multiply(0.05));

            egui::ScrollArea::vertical().show(ui, |ui| {
                ui.add_space(48.0);
                ui.vertical_centered(|ui| {
                    ui.set_max_width(640.0);
                    egui::Frame::new()
                        .fill(PANEL)
                        .stroke(Stroke::new(1.0, LINE))
                        .corner_radius(CornerRadius::same(16))
                        .inner_margin(Margin::same(28))
                        .show(ui, |ui| {
                            ui.set_width(584.0);
                            let (r, _) = ui.allocate_exact_size(vec2(584.0, 84.0), Sense::hover());
                            let c = pos2(r.center().x, r.center().y);
                            ui.painter().circle_filled(c, 38.0, accent.gamma_multiply(0.14));
                            ui.painter().circle_stroke(c, 38.0, Stroke::new(1.5, accent));
                            icons::paint(ui.painter(), egui::Rect::from_center_size(c, Vec2::splat(38.0)), tab.icon(), accent);

                            ui.add_space(6.0);
                            ui.label(RichText::new(plan.title).size(26.0).strong().color(TEXT));
                            ui.label(RichText::new(plan.subtitle).size(14.0).color(TEXT_DIM));
                            ui.add_space(18.0);

                            for (feature, phase) in plan.features {
                                let (row, _) = ui.allocate_exact_size(vec2(584.0, 30.0), Sense::hover());
                                let p = ui.painter();
                                p.circle_filled(pos2(row.left() + 8.0, row.center().y), 3.0, GOLD);
                                p.text(
                                    pos2(row.left() + 20.0, row.center().y),
                                    Align2::LEFT_CENTER,
                                    feature,
                                    FontId::proportional(13.5),
                                    TEXT,
                                );
                                let chip = egui::Rect::from_center_size(pos2(row.right() - 34.0, row.center().y), vec2(64.0, 20.0));
                                p.rect_filled(chip, CornerRadius::same(10), accent.gamma_multiply(0.18));
                                p.text(chip.center(), Align2::CENTER_CENTER, *phase, FontId::proportional(11.5), accent);
                            }

                            ui.add_space(18.0);
                            ui.label(
                                RichText::new(
                                    "Not in the Rust version yet. The Draw tab comes first; this tab follows in the plan's order.",
                                )
                                .size(12.5)
                                .italics()
                                .color(TEXT_DIM),
                            );
                        });
                });
                ui.add_space(48.0);
            });
        });
}
