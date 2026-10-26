// SPDX-License-Identifier: GPL-3.0-or-later
// Layer organizer (plan §W2, §W3): filter chips with NOT, sorting, saved filter presets,
// selecting many layers for batch edits, favorites, "oops" restore of a deleted layer,
// and the floating quick bar (Ctrl+L).

use crate::widgets::HelpExt;
use std::collections::{HashMap, HashSet};

use eframe::egui::{self, Color32, Id, Pos2, Response, RichText, Sense, Stroke, Ui, Vec2, vec2};

use super::DrawState;
use crate::doc::{Blend, Drop, Layer, LayerId, LayerKind, TAG_COLORS};
use crate::icons::{self, Icon};
use crate::layerfilter::{ChipKind, LayerFilter, Preset, SortBy};
use crate::theme::*;
use crate::widgets::{self, section_title};

pub const SEARCH_ID: &str = "layer_search";
const MAX_PRESETS: usize = 20;

#[derive(Default)]
pub struct LayerUi {
    pub filter: LayerFilter,
    pub sort: SortBySafe,
    pub filters_open: bool,
    pub preset_name: String,
    /// Layers picked together (Ctrl / Shift + click, "select all matching"). Empty = just the active one.
    pub multi: HashSet<LayerId>,
    /// The last layer deleted from the panel, to put back: (the layers with their folder contents, stack position).
    pub deleted: Option<(Vec<Layer>, usize)>,
    /// Layers that were active before, newest first.
    pub recent: Vec<LayerId>,
    pub last_active: LayerId,
    /// Per layer: (pixel revision last seen, when it changed), for "recently edited".
    pub stamps: HashMap<LayerId, (u64, u64)>,
    pub counter: u64,
    pub focus_search: bool,
    pub quick: Option<Pos2>,
    pub quick_age: u32,
}

/// `SortBy` with a default (the stack order).
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct SortBySafe(pub SortBy);

impl Default for SortBySafe {
    fn default() -> Self {
        SortBySafe(SortBy::Z)
    }
}

/// A chip with three looks: off, on, and NOT.
fn tri_chip(ui: &mut Ui, label: &str, state: Option<bool>) -> Response {
    match state {
        Some(true) => {
            let r = ui.add(
                egui::Button::new(RichText::new(format!("not {label}")).size(12.5).color(Color32::from_rgb(255, 170, 170)))
                    .fill(Color32::from_rgb(92, 38, 46))
                    .corner_radius(11)
                    .min_size(vec2(0.0, 22.0)),
            );
            r.help("Must NOT be this. Click to switch this filter off.")
        }
        Some(false) => widgets::chip(ui, label, true).help("Must be this. Click again for NOT."),
        None => widgets::chip(ui, label, false).help("Click: only these. Click twice: NOT these."),
    }
}

impl DrawState {
    pub(super) fn layer_filtering(&self) -> bool {
        self.lui.filter.is_active() || self.lui.sort.0 != SortBy::Z
    }

    /// Row order of the panel: the folder tree, or a flat list when filtered or sorted.
    pub(super) fn layer_order(&self) -> Vec<usize> {
        let n = self.doc.layers.len();
        if !self.layer_filtering() {
            return (0..n).filter(|&i| !self.doc.collapsed_away(i)).collect();
        }
        let f = &self.lui.filter;
        let mut v: Vec<usize> = (0..n)
            .filter(|&i| {
                let l = &self.doc.layers[i];
                f.matches(l, l.kind == LayerKind::Group && self.doc.block_len(i) > 1)
            })
            .collect();
        match self.lui.sort.0 {
            SortBy::Z => {}
            SortBy::Name => v.sort_by_key(|&i| self.doc.layers[i].name.to_lowercase()),
            // labeled layers first, grouped by label; unlabeled last
            SortBy::Label => v.sort_by_key(|&i| {
                let t = self.doc.layers[i].tag;
                if t == 0 { 99 } else { t }
            }),
            SortBy::Recent => v.sort_by_key(|&i| std::cmp::Reverse(self.lui.stamps.get(&self.doc.layers[i].id).map(|s| s.1).unwrap_or(0))),
        }
        v
    }

    /// Track edits (for "recently edited") and which layers were active (for the quick bar).
    pub(super) fn track_layer_use(&mut self) {
        for l in &self.doc.layers {
            let e = self.lui.stamps.entry(l.id).or_insert((l.rev, 0));
            if e.0 != l.rev {
                self.lui.counter += 1;
                *e = (l.rev, self.lui.counter);
            }
        }
        if self.active != self.lui.last_active {
            let old = self.lui.last_active;
            self.lui.recent.retain(|r| *r != old && *r != self.active);
            if old != 0 {
                self.lui.recent.insert(0, old);
            }
            self.lui.recent.truncate(8);
            self.lui.last_active = self.active;
        }
        let doc = &self.doc;
        self.lui.multi.retain(|id| doc.index_of(*id).is_some());
        if self.lui.multi.len() < 2 && !self.lui.multi.contains(&self.active) {
            self.lui.multi.clear();
        }
    }

    /// The layers a batch edit applies to, bottom to top of the stack order.
    pub(super) fn selected_layers(&self) -> Vec<LayerId> {
        if self.lui.multi.len() > 1 {
            self.doc.layers.iter().filter(|l| self.lui.multi.contains(&l.id) && l.kind != LayerKind::Paper).map(|l| l.id).collect()
        } else {
            vec![self.active]
        }
    }

    pub(super) fn is_multi(&self) -> bool {
        self.lui.multi.len() > 1
    }

    // ---------- Actions ----------

    /// Delete a layer from the panel, remembering it for "Oops".
    pub(super) fn delete_layer(&mut self, id: LayerId) {
        let Some(i) = self.doc.index_of(id) else { return };
        if self.doc.layers[i].kind == LayerKind::Paper {
            self.notice = Some("The paper can't be deleted — hide it with the eye instead.");
            return;
        }
        let n = self.doc.block_len(i);
        let gone: Vec<Layer> = self.doc.layers[i..i + n].to_vec();
        let name = gone[0].name.clone();
        if self.doc.delete(id) {
            self.lui.deleted = Some((gone, i));
            self.label_hint = Some(format!("Delete {name}"));
        }
    }

    /// Put the last deleted layer (with its folder contents) back where it was.
    pub(super) fn restore_deleted(&mut self) {
        let Some((mut layers, at)) = self.lui.deleted.take() else { return };
        let at = at.min(self.doc.layers.len().saturating_sub(1));
        // the folder it lived in may be gone too
        let top_parent = layers[0].parent.filter(|p| self.doc.index_of(*p).is_some());
        layers[0].parent = top_parent;
        let name = layers[0].name.clone();
        let first = layers[0].id;
        self.doc.layers.splice(at..at, layers);
        self.doc.mark_all_dirty();
        self.active = first;
        self.label_hint = Some(format!("Restore {name}"));
    }

    pub(super) fn toggle_favorite(&mut self, id: LayerId) {
        if let Some(i) = self.doc.index_of(id) {
            if self.doc.layers[i].kind != LayerKind::Paper {
                self.doc.layers[i].fav = !self.doc.layers[i].fav;
            }
        }
    }

    /// A new empty layer just above or below `id`; `clipped` makes it a clipping mask on `id` (above only).
    pub(super) fn new_layer_beside(&mut self, id: LayerId, above: bool, clipped: bool) {
        let name = if clipped { self.next_name_pub("Shade") } else { self.next_name_pub("Layer") };
        let mut l = self.doc.new_layer(&name, LayerKind::Raster);
        l.clip = clipped;
        let new = if above {
            let at = self.doc.index_of(id).unwrap_or(0);
            let parent = self.doc.layers.get(at).and_then(|t| t.parent);
            l.parent = parent;
            let nid = l.id;
            self.doc.layers.insert(at, l);
            self.doc.mark_all_dirty();
            Some(nid)
        } else {
            self.doc.insert_below(l, id)
        };
        match new {
            Some(nid) => self.active = nid,
            None => self.notice = Some("Nothing can go below the paper."),
        }
    }

    /// Select exactly the pixels this layer has paint on.
    pub(super) fn select_layer_pixels(&mut self, id: LayerId) {
        let Some(i) = self.doc.index_of(id) else { return };
        let (w, h) = (self.doc.w, self.doc.h);
        let l = &self.doc.layers[i];
        if l.kind != LayerKind::Raster {
            self.notice = Some("Only pixel layers have pixels to select.");
            return;
        }
        let name = l.name.clone();
        let img = crate::files::layer_rgba(l, 0, 0, w, h);
        let px: Vec<u8> = img.pixels().map(|p| p.0[3]).collect();
        let sel = crate::select::Selection::from_dense(&crate::select::Dense { x0: 0, y0: 0, w: w as usize, h: h as usize, px }, w, h);
        if sel.is_none() {
            self.notice = Some("This layer is empty.");
            return;
        }
        self.doc.selection = sel;
        self.label_hint = Some(format!("Select pixels · {name}"));
    }

    /// Apply a change to every selected layer.
    fn batch(&mut self, label: &str, f: impl Fn(&mut Layer)) {
        let ids = self.selected_layers();
        for id in &ids {
            if let Some(i) = self.doc.index_of(*id) {
                f(&mut self.doc.layers[i]);
            }
        }
        self.doc.mark_all_dirty();
        self.label_hint = Some(format!("{label} · {} layers", ids.len()));
    }

    fn batch_into_folder(&mut self) {
        let ids = self.selected_layers();
        // skip layers whose folder is also picked: they travel with it
        let ids: Vec<LayerId> = ids.iter().copied().filter(|id| !ids.iter().any(|o| o != id && self.doc.is_inside(*id, *o))).collect();
        let Some(&top) = ids.first() else { return };
        let name = self.next_name_pub("Folder");
        let Some(folder) = self.doc.group(top, &name) else { return };
        // bottom first, each to the top of the folder: the original order is kept
        for id in ids.iter().rev().filter(|id| **id != top) {
            self.doc.move_layer(*id, Drop::Into(folder));
        }
        self.lui.multi.clear();
        self.active = folder;
        self.label_hint = Some(format!("Put {} layers into a folder", ids.len()));
    }

    pub(super) fn batch_delete(&mut self) {
        let ids = self.selected_layers();
        let mut n = 0;
        for id in &ids {
            if self.doc.delete(*id) {
                n += 1;
            }
        }
        self.lui.multi.clear();
        self.lui.deleted = None;
        self.label_hint = Some(format!("Delete {n} layers"));
    }

    // ---------- Panel parts ----------

    /// Search box, label dots and the filter drawer. Returns the search box response.
    pub(super) fn filter_bar(&mut self, ui: &mut Ui) -> Response {
        let mut search = None;
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 3.0;
            let r = ui.add(
                egui::TextEdit::singleline(&mut self.lui.filter.query)
                    .id(Id::new(SEARCH_ID))
                    .hint_text("Search layers…")
                    .desired_width(ui.available_width() - 8.0 * 15.0),
            );
            if std::mem::take(&mut self.lui.focus_search) {
                r.request_focus();
                r.scroll_to_me(None);
            }
            search = Some(r);
            for (t, (c, name)) in TAG_COLORS.iter().enumerate() {
                let (r, resp) = ui.allocate_exact_size(Vec2::splat(12.0), Sense::click());
                let on = self.lui.filter.label as usize == t;
                if t == 0 {
                    ui.painter().circle_stroke(r.center(), 4.5, Stroke::new(1.2, if on { VENOM } else { TEXT_DIM }));
                } else {
                    ui.painter().circle_filled(r.center(), if on { 6.0 } else { 4.5 }, *c);
                    if on {
                        ui.painter().circle_stroke(r.center(), 6.0, Stroke::new(1.5, TEXT));
                    }
                }
                let tip = if t == 0 { "Show all labels".to_owned() } else { format!("Show only {name} layers") };
                if resp.help(tip).clicked() {
                    self.lui.filter.label = t as u8;
                }
            }
        });
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing = vec2(4.0, 4.0);
            let n = self.lui.filter.count();
            let label = if n > 0 { format!("Filters ({n})") } else { "Filters".to_owned() };
            if widgets::chip(ui, &label, self.lui.filters_open).help("Stack filters: type, state, blend mode…").clicked() {
                self.lui.filters_open = !self.lui.filters_open;
            }
            let sort = self.lui.sort.0;
            egui::ComboBox::from_id_salt("layer_sort")
                .selected_text(match sort {
                    SortBy::Z => "Sort: stack".to_owned(),
                    s => format!("Sort: {}", SortBy::ALL.iter().find(|a| a.0 == s).map(|a| a.1).unwrap_or("")),
                })
                .width(150.0)
                .show_ui(ui, |ui| {
                    for (s, label) in SortBy::ALL {
                        ui.selectable_value(&mut self.lui.sort, SortBySafe(s), label).help("Pick how the list is sorted.");
                    }
                });
            if self.lui.filter.is_active() || sort != SortBy::Z {
                if widgets::chip(ui, "Clear", false).help("Show every layer again").clicked() {
                    self.lui.filter.clear();
                    self.lui.sort = SortBySafe(SortBy::Z);
                }
                if widgets::chip(ui, "Select all shown", false).help("Pick every shown layer for batch edits").clicked() {
                    let ids: HashSet<LayerId> = self
                        .layer_order()
                        .into_iter()
                        .map(|i| &self.doc.layers[i])
                        .filter(|l| l.kind != LayerKind::Paper)
                        .map(|l| l.id)
                        .collect();
                    if let Some(first) = ids.iter().next() {
                        if !ids.contains(&self.active) {
                            self.active = *first;
                        }
                    }
                    self.lui.multi = ids;
                }
            }
        });
        if self.lui.filters_open {
            self.filter_drawer(ui);
        }
        search.unwrap()
    }

    fn filter_drawer(&mut self, ui: &mut Ui) {
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing = vec2(4.0, 4.0);
            for kind in ChipKind::BASIC {
                let st = self.lui.filter.chip_state(kind);
                if tri_chip(ui, &kind.label(), st).clicked() {
                    self.lui.filter.cycle(kind);
                }
            }
            let blends: Vec<ChipKind> = self.lui.filter.chips.iter().map(|c| c.kind).filter(|k| matches!(k, ChipKind::Blend(_))).collect();
            for kind in blends {
                let st = self.lui.filter.chip_state(kind);
                if tri_chip(ui, &kind.label(), st).clicked() {
                    self.lui.filter.cycle(kind);
                }
            }
        });
        ui.horizontal(|ui| {
            egui::ComboBox::from_id_salt("layer_blend_filter").selected_text("Add blend mode…").width(150.0).height(380.0).show_ui(ui, |ui| {
                for b in Blend::ALL {
                    if ui.selectable_label(false, b.name()).help("Use this blend mode.").clicked() && self.lui.filter.chip_state(ChipKind::Blend(b)).is_none() {
                        self.lui.filter.cycle(ChipKind::Blend(b));
                    }
                }
            });
        });
        // saved filters
        ui.label(RichText::new("Saved filters").size(12.0).color(TEXT_DIM));
        let mut apply = None;
        let mut remove = None;
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing = vec2(4.0, 4.0);
            if self.doc.filter_presets.is_empty() {
                ui.label(RichText::new("None yet — set up a filter, name it, press Save.").size(11.5).color(TEXT_DIM));
            }
            for (i, p) in self.doc.filter_presets.iter().enumerate() {
                let r = widgets::chip(ui, &p.name, p.filter == self.lui.filter).help("Click to use · right-click to delete");
                if r.clicked() {
                    apply = Some(i);
                }
                r.context_menu(|ui| {
                    if ui.button("Delete this saved filter").help("Forget this saved filter.").clicked() {
                        remove = Some(i);
                        ui.close();
                    }
                });
            }
        });
        ui.horizontal(|ui| {
            ui.add(egui::TextEdit::singleline(&mut self.lui.preset_name).hint_text("Name, e.g. Skin tones").desired_width(130.0).char_limit(40));
            let ok = self.lui.filter.is_active() && !self.lui.preset_name.trim().is_empty();
            if ui.add_enabled(ok, egui::Button::new("Save")).help("Saved inside this project").clicked() {
                let name = self.lui.preset_name.trim().to_owned();
                let preset = Preset { name: name.clone(), filter: self.lui.filter.clone() };
                if let Some(i) = self.doc.filter_presets.iter().position(|p| p.name == name) {
                    self.doc.filter_presets[i] = preset;
                } else if self.doc.filter_presets.len() < MAX_PRESETS {
                    self.doc.filter_presets.push(preset);
                } else {
                    self.notice = Some("You can keep up to 20 saved filters.");
                }
                self.lui.preset_name.clear();
            }
        });
        if let Some(i) = apply {
            self.lui.filter = self.doc.filter_presets[i].filter.clone();
        }
        if let Some(i) = remove {
            self.doc.filter_presets.remove(i);
        }
        ui.add_space(2.0);
    }

    /// Starred layers as a row of chips above the list.
    pub(super) fn favorites_row(&mut self, ui: &mut Ui) {
        let favs: Vec<(LayerId, String)> =
            self.doc.layers.iter().filter(|l| l.fav && l.kind != LayerKind::Paper).map(|l| (l.id, l.name.clone())).collect();
        if favs.is_empty() {
            return;
        }
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing = vec2(4.0, 4.0);
            let (r, _) = ui.allocate_exact_size(vec2(14.0, 14.0), Sense::hover());
            icons::paint(ui.painter(), r, Icon::Star, GOLD);
            for (id, name) in favs {
                if widgets::chip(ui, &name, id == self.active).help("Favorite layer").clicked() {
                    self.active = id;
                    self.lui.multi.clear();
                }
            }
        });
        ui.add_space(2.0);
    }

    /// What to do with several picked layers at once.
    pub(super) fn batch_bar(&mut self, ui: &mut Ui) {
        if !self.is_multi() {
            return;
        }
        let n = self.selected_layers().len();
        ui.add_space(4.0);
        egui::Frame::new().fill(CARD).corner_radius(8).inner_margin(8).show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal(|ui| {
                ui.label(RichText::new(format!("{n} layers picked")).strong().color(VENOM));
                if widgets::chip(ui, "Done", false).help("Go back to one layer").clicked() {
                    self.lui.multi.clear();
                }
            });
            ui.horizontal_wrapped(|ui| {
                ui.spacing_mut().item_spacing = vec2(4.0, 4.0);
                if widgets::chip(ui, "Show", false).help("Show it.").clicked() {
                    self.batch("Show", |l| l.visible = true);
                }
                if widgets::chip(ui, "Hide", false).help("Hide it.").clicked() {
                    self.batch("Hide", |l| l.visible = false);
                }
                if widgets::chip(ui, "Lock", false).help("Lock it, so it cannot be changed by accident.").clicked() {
                    self.batch("Lock", |l| l.locked = true);
                }
                if widgets::chip(ui, "Unlock", false).help("Unlock it, so it can be changed.").clicked() {
                    self.batch("Unlock", |l| l.locked = false);
                }
                if widgets::chip(ui, "Star", false).help("Add all to favorites").clicked() {
                    self.batch("Favorite", |l| l.fav = true);
                }
                if widgets::chip(ui, "Into folder", false).help("Put the layer into the folder.").clicked() {
                    self.batch_into_folder();
                }
            });
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 3.0;
                ui.label(RichText::new("Label").size(12.0).color(TEXT_DIM));
                for (t, (c, name)) in TAG_COLORS.iter().enumerate() {
                    let (r, resp) = ui.allocate_exact_size(Vec2::splat(16.0), Sense::click());
                    if t == 0 {
                        ui.painter().circle_stroke(r.center(), 5.5, Stroke::new(1.2, TEXT_DIM));
                    } else {
                        ui.painter().circle_filled(r.center(), 6.0, *c);
                    }
                    if resp.help(*name).clicked() {
                        self.batch("Color label", move |l| l.tag = t as u8);
                    }
                }
            });
            let mut opacity = self.doc.index_of(self.active).map(|i| self.doc.layers[i].opacity).unwrap_or(100.0);
            if widgets::slider_row(ui, "Opacity", &mut opacity, 0.0..=100.0, "%").changed() {
                self.batch("Opacity", move |l| l.opacity = opacity);
            }
            ui.horizontal(|ui| {
                ui.label(RichText::new("Blend").color(TEXT_DIM).size(12.5));
                egui::ComboBox::from_id_salt("batch_blend").selected_text("Set for all…").width(150.0).height(380.0).show_ui(ui, |ui| {
                    for b in Blend::ALL {
                        if ui.selectable_label(false, b.name()).help("Use this blend mode.").clicked() {
                            self.batch("Blend mode", move |l| {
                                if l.kind != LayerKind::Group || b != Blend::Normal {
                                    l.blend = b;
                                }
                            });
                        }
                    }
                });
                if ui.button(RichText::new("Delete").color(Color32::from_rgb(232, 110, 110))).help("Delete this. Undo brings it back.").clicked() {
                    self.batch_delete();
                }
            });
        });
    }

    /// Floating bar near the pointer (Ctrl+L): new layers, recent and favorite layers, "Oops".
    pub(super) fn layer_quick_bar(&mut self, ctx: &egui::Context) {
        let Some(at) = self.lui.quick else { return };
        self.lui.quick_age += 1;
        let mut close = ctx.input(|i| i.key_pressed(egui::Key::Escape));
        let recent: Vec<(LayerId, String)> = self
            .lui
            .recent
            .iter()
            .filter_map(|id| self.doc.index_of(*id).map(|i| (*id, self.doc.layers[i].name.clone())))
            .take(3)
            .collect();
        let favs: Vec<(LayerId, String)> =
            self.doc.layers.iter().filter(|l| l.fav && l.kind != LayerKind::Paper).map(|l| (l.id, l.name.clone())).take(8).collect();
        let oops = self.lui.deleted.as_ref().map(|(l, _)| l[0].name.clone());
        let area = egui::Area::new(Id::new("layer_quick_bar")).order(egui::Order::Foreground).fixed_pos(at).show(ctx, |ui| {
            egui::Frame::popup(ui.style()).show(ui, |ui| {
                ui.set_max_width(260.0);
                section_title(ui, "Quick layers");
                ui.horizontal_wrapped(|ui| {
                    ui.spacing_mut().item_spacing = vec2(4.0, 4.0);
                    if widgets::chip(ui, "New layer", false).help("Add a new empty layer.").clicked() {
                        self.add_layer(LayerKind::Raster);
                        close = true;
                    }
                    if widgets::chip(ui, "New vector", false).help("Add a new vector layer, for smooth lines you can change later.").clicked() {
                        self.add_layer(LayerKind::Vector);
                        close = true;
                    }
                    if widgets::chip(ui, "New folder", false).help("Make a new folder.").clicked() {
                        self.add_layer(LayerKind::Group);
                        close = true;
                    }
                });
                if !recent.is_empty() {
                    ui.label(RichText::new("Back to").size(12.0).color(TEXT_DIM));
                    ui.horizontal_wrapped(|ui| {
                        ui.spacing_mut().item_spacing = vec2(4.0, 4.0);
                        for (id, name) in &recent {
                            if widgets::chip(ui, name, false).help("Use this.").clicked() {
                                self.active = *id;
                                close = true;
                            }
                        }
                    });
                }
                if !favs.is_empty() {
                    ui.label(RichText::new("Favorites").size(12.0).color(TEXT_DIM));
                    ui.horizontal_wrapped(|ui| {
                        ui.spacing_mut().item_spacing = vec2(4.0, 4.0);
                        for (id, name) in &favs {
                            if widgets::chip(ui, name, *id == self.active).help("Pick this layer.").clicked() {
                                self.active = *id;
                                close = true;
                            }
                        }
                    });
                }
                if let Some(name) = oops {
                    ui.add_space(2.0);
                    if ui.button(format!("Oops — put back \"{name}\"")).help("Undo the delete and bring it back.").clicked() {
                        self.restore_deleted();
                        close = true;
                    }
                }
            });
        });
        // a click anywhere else closes it (not on the frame that opened it)
        let outside = ctx.input(|i| i.pointer.any_pressed()) && !area.response.contains_pointer() && self.lui.quick_age > 1;
        if close || outside {
            self.lui.quick = None;
        }
    }

    pub(super) fn next_name_pub(&mut self, base: &str) -> String {
        self.layer_counter += 1;
        format!("{base} {}", self.layer_counter)
    }

    #[cfg(test)]
    pub fn shown_layer_names(&self) -> Vec<String> {
        self.layer_order().into_iter().map(|i| self.doc.layers[i].name.clone()).collect()
    }

    #[cfg(test)]
    pub fn pending_label(&self) -> Option<String> {
        self.label_hint.clone()
    }

    #[cfg(test)]
    pub fn layer_ui_count(&self) -> usize {
        self.lui.multi.len()
    }

    #[cfg(test)]
    pub fn layer_ui_mut_ref(&self) -> &LayerUi {
        &self.lui
    }

    #[cfg(test)]
    pub fn active_name(&self) -> String {
        self.doc.layers[self.active_idx()].name.clone()
    }

    #[cfg(test)]
    pub fn layer_ui_mut(&mut self) -> &mut LayerUi {
        &mut self.lui
    }

    #[cfg(test)]
    pub fn test_delete_layer(&mut self, id: LayerId) {
        self.delete_layer(id);
    }

    #[cfg(test)]
    pub fn test_restore_deleted(&mut self) {
        self.restore_deleted();
    }

    #[cfg(test)]
    pub fn test_batch_hide(&mut self) {
        self.batch("Hide", |l| l.visible = false);
    }

    #[cfg(test)]
    pub fn test_select_pixels(&mut self, id: LayerId) {
        self.select_layer_pixels(id);
    }

    #[cfg(test)]
    pub fn test_layer_beside(&mut self, id: LayerId, above: bool, clipped: bool) {
        self.new_layer_beside(id, above, clipped);
    }

}
