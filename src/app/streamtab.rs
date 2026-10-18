// SPDX-License-Identifier: GPL-3.0-or-later
// The Stream tab: the face from a phone, a program or the camera moves the character live. Tracking sources are switched on here (any mix;
// the first that is sending wins, then the next), a mapping says which input moves which slider, the character blinks and breathes by
// itself when nothing tracks the face, and the model goes out to other programs: a window with a see-through background, and a
// browser source for OBS.

use eframe::egui::{self, Color32, Rect, RichText, Sense, Stroke, Ui, pos2, vec2};

use super::SeviathApp;
use crate::outputs::BrowserSource;
use crate::rig::Id;
use crate::theme::*;
use crate::track::net::{Protocol, Receiver};
use crate::track::{CHANNELS, Map, TrackMapping, TrackState, channels};
use crate::widgets::{self, section_title};

const FRESH: std::time::Duration = std::time::Duration::from_millis(600);

pub struct SourceSlot {
    pub label: &'static str,
    /// None is the camera program of the app (it sends to its own port).
    pub protocol: Option<Protocol>,
    pub port: u16,
    pub enabled: bool,
    pub rx: Option<Receiver>,
    pub error: Option<String>,
    pub child: Option<std::process::Child>,
    pub camera: i32,
}

pub struct StreamUi {
    pub sources: Vec<SourceSlot>,
    pub state: TrackState,
    /// The inputs as they are now, for the meters.
    pub inputs: Vec<(&'static str, f32)>,
    pub tracked: bool,
    pub server: Option<BrowserSource>,
    pub server_port: u16,
    pub server_side: u32,
    last_sent: f64,
    pub window: bool,
    pub on_top: bool,
    pub click_through: bool,
    pub message: Option<String>,
    pub add_param: Option<Id>,
    pub add_input: usize,
    pub bg: u8,
}

impl Default for StreamUi {
    fn default() -> Self {
        let slot = |label, protocol: Option<Protocol>, port| SourceSlot { label, protocol, port, enabled: false, rx: None, error: None, child: None, camera: 0 };
        StreamUi {
            sources: vec![
                slot("Camera (this program)", None, 39540),
                slot("VMC (Waidayo, VSeeFace, and others)", Some(Protocol::Vmc), Protocol::Vmc.default_port()),
                slot("iFacialMocap", Some(Protocol::IFacial), Protocol::IFacial.default_port()),
                slot("OpenSeeFace", Some(Protocol::OpenSeeFace), Protocol::OpenSeeFace.default_port()),
            ],
            state: TrackState::default(),
            inputs: Vec::new(),
            tracked: false,
            server: None,
            server_port: 8765,
            server_side: 720,
            last_sent: 0.0,
            window: false,
            on_top: true,
            click_through: false,
            message: None,
            add_param: None,
            add_input: 0,
            bg: 0,
        }
    }
}

impl Drop for SourceSlot {
    fn drop(&mut self) {
        if let Some(mut c) = self.child.take() {
            let _ = c.kill();
            let _ = c.wait();
        }
    }
}

impl SeviathApp {
    pub(super) fn stream_ui(&mut self, ui: &mut Ui) {
        let ctx = ui.ctx().clone();
        self.rig_init();
        self.stream_drive(&ctx);
        self.phys_tick(&ctx);
        self.rig_render(&ctx);
        self.stream_outputs(&ctx);
        egui::Panel::left("stream_sources").default_size(300.0).size_range(250.0..=420.0).resizable(true).frame(egui::Frame::new().fill(PANEL).inner_margin(egui::Margin::same(10))).show(ui, |ui| {
            egui::ScrollArea::vertical().id_salt("stream_left").show(ui, |ui| self.stream_sources_ui(ui));
        });
        egui::Panel::right("stream_mapping").default_size(340.0).size_range(280.0..=520.0).resizable(true).frame(egui::Frame::new().fill(PANEL).inner_margin(egui::Margin::same(10))).show(ui, |ui| {
            egui::ScrollArea::vertical().id_salt("stream_right").show(ui, |ui| self.stream_mapping_ui(ui));
        });
        egui::CentralPanel::default().frame(egui::Frame::new().fill(self.stream_bg())).show(ui, |ui| self.stream_view(ui));
    }

    fn stream_bg(&self) -> Color32 {
        match self.rigs.stream.bg {
            1 => Color32::from_rgb(0, 177, 64),
            2 => Color32::from_rgb(240, 240, 240),
            3 => Color32::from_rgb(20, 20, 24),
            _ => Color32::from_rgb(14, 20, 18),
        }
    }

    /// Read the sources, work out the inputs, and move the sliders.
    fn stream_drive(&mut self, ctx: &egui::Context) {
        let now = ctx.input(|i| i.time);
        let dt = ctx.input(|i| i.unstable_dt).clamp(0.001, 0.1);
        // a camera program that stopped by itself
        for s in &mut self.rigs.stream.sources {
            if let Some(c) = s.child.as_mut() {
                if let Ok(Some(_)) = c.try_wait() {
                    s.error = Some(format!("The camera program stopped. {}", crate::aisetup::tail(&crate::aisetup::root().join("track.log"), 2)));
                    s.child = None;
                    s.enabled = false;
                    s.rx = None;
                }
            }
        }
        let frame = self.rigs.stream.sources.iter().filter(|s| s.enabled).find_map(|s| s.rx.as_ref().and_then(|r| r.latest(FRESH)));
        self.rigs.stream.tracked = frame.is_some();
        let mut ch = frame.as_ref().map(channels).unwrap_or_default();
        let rig = self.draw.doc.rig.clone();
        let map = &rig.tracking;
        let mouse = ctx.input(|i| i.pointer.hover_pos()).map(|p| {
            let r = ctx.content_rect();
            [((p.x - r.left()) / r.width() * 2.0 - 1.0).clamp(-1.0, 1.0), ((p.y - r.top()) / r.height() * 2.0 - 1.0).clamp(-1.0, 1.0)]
        });
        let mut state = std::mem::take(&mut self.rigs.stream.state);
        state.fill_in(&mut ch, map, now, mouse);
        if !map.maps.is_empty() {
            state.apply(map, &ch, &rig, &mut self.rigs.values, dt);
        }
        self.rigs.stream.state = state;
        let mut inputs: Vec<(&'static str, f32)> = CHANNELS.iter().filter_map(|c| ch.get(c.0).map(|v| (c.0, *v))).collect();
        inputs.truncate(40);
        self.rigs.stream.inputs = inputs;
        // the face moves all the time while a source sends
        let any_on = self.rigs.stream.sources.iter().any(|s| s.enabled) || rig.tracking.auto_blink || rig.tracking.breathing || rig.tracking.look_at_mouse;
        if any_on {
            ctx.request_repaint();
        }
    }

    fn stream_set_source(&mut self, i: usize, on: bool) {
        let s = &mut self.rigs.stream.sources[i];
        s.error = None;
        if on == s.enabled && (s.rx.is_some() == on) {
            return;
        }
        if !on {
            s.enabled = false;
            s.rx = None;
            if let Some(mut c) = s.child.take() {
                let _ = c.kill();
                let _ = c.wait();
            }
            return;
        }
        let proto = s.protocol.unwrap_or(Protocol::Vmc);
        match Receiver::start(proto, s.port) {
            Ok(r) => {
                s.rx = Some(r);
                s.enabled = true;
            }
            Err(e) => {
                s.error = Some(e);
                s.enabled = false;
                return;
            }
        }
        // the camera: start the program that watches it
        if s.protocol.is_none() {
            let python = crate::aisetup::venv_python(&crate::aisetup::root());
            let (port, camera) = (s.port, s.camera);
            match crate::ai::start_tracker(&python.to_string_lossy(), camera, port) {
                Ok(c) => s.child = Some(c),
                Err(e) => {
                    s.error = Some(e);
                    s.enabled = false;
                    s.rx = None;
                }
            }
        }
    }

    fn stream_sources_ui(&mut self, ui: &mut Ui) {
        section_title(ui, "Tracking");
        ui.label(RichText::new("Switch on what sends your face. Any mix works: the first one in the list that is sending is used, and when it stops the next one takes over. When none sends, the character blinks and breathes by itself.").size(11.5).color(TEXT_DIM));
        let n = self.rigs.stream.sources.len();
        let mut change: Option<(usize, bool)> = None;
        let mut swap: Option<(usize, usize)> = None;
        for i in 0..n {
            let (label, enabled, rate, err, camera) = {
                let s = &self.rigs.stream.sources[i];
                let rate = s.rx.as_ref().map(|r| { let h = r.heard.lock().unwrap(); if h.fresh(FRESH) { h.rate } else { 0.0 } }).unwrap_or(0.0);
                (s.label, s.enabled, rate, s.error.clone(), s.protocol.is_none())
            };
            ui.horizontal(|ui| {
                if widgets::chip(ui, label, enabled).clicked() {
                    change = Some((i, !enabled));
                }
                let (txt, col) = if !enabled { ("off", TEXT_DIM) } else if rate > 0.0 { ("receiving", VENOM) } else { ("waiting", GOLD) };
                ui.label(RichText::new(if rate > 0.0 { format!("{txt} {rate:.0}/s") } else { txt.to_string() }).size(11.5).color(col));
                if i > 0 && ui.small_button("Up").on_hover_text("Use it before the one above").clicked() {
                    swap = Some((i, i - 1));
                }
            });
            ui.horizontal(|ui| {
                ui.add_space(14.0);
                ui.label(RichText::new("Port").size(11.5).color(TEXT_DIM));
                let s = &mut self.rigs.stream.sources[i];
                if ui.add_enabled(!s.enabled, egui::DragValue::new(&mut s.port).range(1024..=65535)).changed() {
                    s.error = None;
                }
                if camera {
                    ui.label(RichText::new("Camera").size(11.5).color(TEXT_DIM));
                    ui.add_enabled(!s.enabled, egui::DragValue::new(&mut s.camera).range(0..=8));
                }
            });
            if let Some(e) = err {
                ui.label(RichText::new(e).size(11.0).color(Color32::from_rgb(255, 120, 100)));
            }
        }
        if let Some((a, on)) = change {
            if on && self.rigs.stream.sources[a].protocol.is_none() && !crate::aisetup::tracker_ready() {
                self.rigs.stream.sources[a].error = Some("The camera tracking files are missing: use the Download the AI files card in the Rig tab (Rig for games), then come back.".into());
            } else {
                self.stream_set_source(a, on);
            }
        }
        if let Some((a, b)) = swap {
            self.rigs.stream.sources.swap(a, b);
        }
        ui.add_space(6.0);
        ui.label(RichText::new("From a phone: iFacialMocap sends to this computer (give it this computer's address). Waidayo and similar apps send VMC to port 39539. OpenSeeFace sends to 11573.").size(11.0).color(TEXT_DIM));

        section_title(ui, "What the face does now");
        let inputs = self.rigs.stream.inputs.clone();
        if inputs.is_empty() {
            ui.label(RichText::new("Nothing yet.").size(11.5).color(TEXT_DIM));
        }
        for (name, v) in inputs {
            let (lo, hi) = CHANNELS.iter().find(|c| c.0 == name).map(|c| (c.2, c.3)).unwrap_or((0.0, 1.0));
            ui.horizontal(|ui| {
                ui.label(RichText::new(name).size(11.0).color(TEXT_DIM));
                let (r, _) = ui.allocate_exact_size(vec2(ui.available_width().min(140.0), 8.0), Sense::hover());
                ui.painter().rect_filled(r, 3.0, Color32::from_rgb(28, 36, 33));
                let t = ((v - lo) / (hi - lo).max(1e-6)).clamp(0.0, 1.0);
                ui.painter().rect_filled(Rect::from_min_size(r.min, vec2(r.width() * t, r.height())), 3.0, VENOM);
                ui.label(RichText::new(format!("{v:.2}")).size(11.0).monospace());
            });
        }
        let _ = Stroke::NONE;
    }

    fn stream_mapping_ui(&mut self, ui: &mut Ui) {
        let rig = self.draw.doc.rig.clone();
        section_title(ui, "Behaviour");
        let mut m = rig.tracking.clone();
        ui.horizontal_wrapped(|ui| {
            ui.checkbox(&mut m.auto_blink, "Blink by itself").on_hover_text("When the eyes are not tracked");
            ui.checkbox(&mut m.breathing, "Breathe");
            ui.checkbox(&mut m.look_at_mouse, "Look at the mouse").on_hover_text("When the face is not tracked, the eyes and the head follow the pointer");
        });
        ui.horizontal_wrapped(|ui| {
            if ui.button("Calibrate").on_hover_text("Look straight at the camera, then press: how you look now becomes the middle").clicked() {
                let ch: std::collections::HashMap<&'static str, f32> = self.rigs.stream.inputs.iter().cloned().collect();
                m.calibrate(&ch);
            }
            if ui.button("Forget the calibration").clicked() {
                m.neutral.clear();
            }
            if ui.button("Make the mapping").on_hover_text("From the names of the sliders (or from the VTube Studio settings of a Live2D model)").clicked() {
                let keep = (m.auto_blink, m.breathing, m.look_at_mouse, m.neutral.clone());
                m = TrackMapping::auto(&rig);
                (m.auto_blink, m.breathing, m.look_at_mouse, m.neutral) = keep;
                self.rigs.stream.message = Some(format!("{} sliders are moved by the face now.", m.maps.len()));
            }
        });
        section_title(ui, "Sliders moved by the face");
        if m.maps.is_empty() {
            ui.label(RichText::new("None yet. Press Make the mapping, or add one below.").size(11.5).color(TEXT_DIM));
        }
        let mut remove: Option<usize> = None;
        for (i, mp) in m.maps.iter_mut().enumerate() {
            let name = rig.param(mp.param).map(|p| p.name.clone()).unwrap_or_else(|| "(gone)".into());
            egui::CollapsingHeader::new(RichText::new(format!("{name}  from  {}", mp.input)).size(12.0)).id_salt(("map", i)).show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.label(RichText::new("Input").size(11.5).color(TEXT_DIM));
                    egui::ComboBox::from_id_salt(("map_in", i)).selected_text(mp.input.clone()).width(150.0).show_ui(ui, |ui| {
                        for c in CHANNELS {
                            if ui.selectable_label(mp.input == c.0, format!("{}  ({})", c.0, c.1)).clicked() {
                                mp.input = c.0.to_string();
                                let (lo, hi) = (c.2, c.3);
                                mp.in_lo = lo;
                                mp.in_hi = hi;
                            }
                        }
                    });
                });
                ui.horizontal(|ui| {
                    ui.label(RichText::new("Input from").size(11.5).color(TEXT_DIM));
                    ui.add(egui::DragValue::new(&mut mp.in_lo).speed(0.05));
                    ui.label(RichText::new("to").size(11.5).color(TEXT_DIM));
                    ui.add(egui::DragValue::new(&mut mp.in_hi).speed(0.05));
                });
                ui.horizontal(|ui| {
                    ui.label(RichText::new("Slider from").size(11.5).color(TEXT_DIM));
                    ui.add(egui::DragValue::new(&mut mp.out_lo).speed(0.05));
                    ui.label(RichText::new("to").size(11.5).color(TEXT_DIM));
                    ui.add(egui::DragValue::new(&mut mp.out_hi).speed(0.05));
                });
                ui.horizontal(|ui| {
                    ui.label(RichText::new("Smoothing").size(11.5).color(TEXT_DIM));
                    ui.add(egui::DragValue::new(&mut mp.smoothing).range(0.0..=60.0).speed(0.2)).on_hover_text("0 follows at once; bigger is calmer and slower");
                    ui.checkbox(&mut mp.invert, "Turn it round");
                    ui.checkbox(&mut mp.clamp, "Hold it inside");
                });
                if ui.small_button("Remove").clicked() {
                    remove = Some(i);
                }
            });
        }
        if let Some(i) = remove {
            m.maps.remove(i);
        }
        // add one
        ui.add_space(4.0);
        ui.horizontal_wrapped(|ui| {
            let names: Vec<(Id, String, f32, f32)> = rig.params.iter().map(|p| (p.id, p.name.clone(), p.min, p.max)).collect();
            let sel = self.rigs.stream.add_param.filter(|id| names.iter().any(|n| n.0 == *id));
            let label = sel.and_then(|id| names.iter().find(|n| n.0 == id)).map(|n| n.1.clone()).unwrap_or_else(|| "a slider".into());
            egui::ComboBox::from_id_salt("map_add_param").selected_text(label).width(120.0).show_ui(ui, |ui| {
                for (id, n, _, _) in &names {
                    if ui.selectable_label(sel == Some(*id), n).clicked() {
                        self.rigs.stream.add_param = Some(*id);
                    }
                }
            });
            ui.label(RichText::new("from").size(11.5).color(TEXT_DIM));
            let ci = self.rigs.stream.add_input.min(CHANNELS.len() - 1);
            egui::ComboBox::from_id_salt("map_add_input").selected_text(CHANNELS[ci].0).width(130.0).show_ui(ui, |ui| {
                for (i, c) in CHANNELS.iter().enumerate() {
                    if ui.selectable_label(ci == i, c.0).clicked() {
                        self.rigs.stream.add_input = i;
                    }
                }
            });
            if ui.add_enabled(sel.is_some(), egui::Button::new("Add")).clicked() {
                if let Some((id, _, lo, hi)) = sel.and_then(|id| names.iter().find(|n| n.0 == id)).cloned() {
                    let c = CHANNELS[ci];
                    m.maps.push(Map { param: id, input: c.0.to_string(), in_lo: c.2, in_hi: c.3, out_lo: lo, out_hi: hi, clamp: true, smoothing: 8.0, invert: false });
                }
            }
        });
        if m != rig.tracking {
            let now = ui.ctx().input(|i| i.time);
            self.rig_edit("Tracking mapping", now, |r| r.tracking = m);
        }
        if let Some(msg) = &self.rigs.stream.message {
            ui.label(RichText::new(msg).size(11.5).color(TEXT_DIM));
        }
        self.stream_output_ui(ui);
    }

    fn stream_output_ui(&mut self, ui: &mut Ui) {
        section_title(ui, "Show the model in other programs");
        ui.horizontal_wrapped(|ui| {
            ui.label(RichText::new("Background here").size(12.0).color(TEXT_DIM));
            for (i, n) in ["Dark", "Green", "Light", "Black"].iter().enumerate() {
                if widgets::chip(ui, n, self.rigs.stream.bg == i as u8).clicked() {
                    self.rigs.stream.bg = i as u8;
                }
            }
        });
        // the window with a see-through background
        ui.horizontal_wrapped(|ui| {
            if widgets::chip(ui, "Model window", self.rigs.stream.window).on_hover_text("A window with only the model on a see-through background: capture it in OBS with Window Capture, or put it over the screen").clicked() {
                self.rigs.stream.window = !self.rigs.stream.window;
            }
            ui.checkbox(&mut self.rigs.stream.on_top, "On top");
            ui.checkbox(&mut self.rigs.stream.click_through, "Mouse goes through");
        });
        // the browser source
        let running = self.rigs.stream.server.is_some();
        ui.horizontal_wrapped(|ui| {
            if widgets::chip(ui, "OBS browser source", running).on_hover_text("A page on this computer that shows the model with a see-through background. In OBS add a Browser source with the address below").clicked() {
                if running {
                    self.rigs.stream.server = None;
                } else {
                    match BrowserSource::start(self.rigs.stream.server_port) {
                        Ok(s) => self.rigs.stream.server = Some(s),
                        Err(e) => self.rigs.stream.message = Some(e),
                    }
                }
            }
            ui.label(RichText::new("Port").size(11.5).color(TEXT_DIM));
            ui.add_enabled(!running, egui::DragValue::new(&mut self.rigs.stream.server_port).range(1024..=65535));
            ui.label(RichText::new("Size").size(11.5).color(TEXT_DIM));
            ui.add(egui::DragValue::new(&mut self.rigs.stream.server_side).range(128..=2160).speed(8.0).suffix(" px"));
        });
        if let Some(s) = &self.rigs.stream.server {
            ui.horizontal_wrapped(|ui| {
                ui.label(RichText::new(s.url()).monospace().size(12.0).color(VENOM));
                if ui.small_button("Copy").clicked() {
                    ui.ctx().copy_text(s.url());
                }
                ui.label(RichText::new(format!("{} pictures sent", s.served())).size(11.0).color(TEXT_DIM));
            });
        }
    }

    fn stream_view(&mut self, ui: &mut Ui) {
        let rect = ui.available_rect_before_wrap();
        ui.allocate_rect(rect, Sense::hover());
        let p = ui.painter_at(rect);
        match &self.rigs.image {
            Some(t) => {
                let tex = t.size_vec2();
                let fit = ((rect.width() - 24.0) / tex.x).min((rect.height() - 24.0) / tex.y).max(0.05);
                p.image(t.id(), Rect::from_center_size(rect.center(), tex * fit), Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)), Color32::WHITE);
            }
            None => {
                p.text(rect.center(), egui::Align2::CENTER_CENTER, "Make a rig first (Rig tab) to stream it", egui::FontId::proportional(14.0), TEXT_DIM);
            }
        }
        let (txt, col) = if self.rigs.stream.tracked { ("Tracking", VENOM) } else { ("Not tracked: idle", TEXT_DIM) };
        p.text(rect.left_top() + vec2(12.0, 10.0), egui::Align2::LEFT_TOP, txt, egui::FontId::proportional(13.0), col);
    }

    /// The model window and the browser source.
    fn stream_outputs(&mut self, ctx: &egui::Context) {
        let now = ctx.input(|i| i.time);
        // the browser source gets a new picture about 30 times a second
        if self.rigs.stream.server.is_some() && now - self.rigs.stream.last_sent >= 1.0 / 30.0 {
            self.rigs.stream.last_sent = now;
            let (w, h) = (self.draw.doc.w, self.draw.doc.h);
            let k = (self.rigs.stream.server_side as f32 / w.max(h) as f32).min(1.0);
            let size = (((w as f32 * k).round() as u32).max(8), ((h as f32 * k).round() as u32).max(8));
            if let Some(img) = self.rig_still(size, None) {
                let png = crate::outputs::png_bytes(&img);
                if let Some(s) = &self.rigs.stream.server {
                    s.set_frame(png);
                }
            }
            ctx.request_repaint();
        }
        if self.rigs.stream.window {
            let tex = self.rigs.image.clone();
            let (on_top, passthrough) = (self.rigs.stream.on_top, self.rigs.stream.click_through);
            let mut close = false;
            let mut builder = egui::ViewportBuilder::default().with_title("SeviathStudio model").with_inner_size([480.0, 720.0]).with_transparent(true).with_decorations(false);
            if on_top {
                builder = builder.with_always_on_top();
            }
            ctx.show_viewport_immediate(egui::ViewportId::from_hash_of("seviath_model_window"), builder, |ui, _class| {
                egui::CentralPanel::default().frame(egui::Frame::NONE).show(ui, |ui| {
                    let rect = ui.available_rect_before_wrap();
                    let resp = ui.allocate_rect(rect, Sense::click_and_drag());
                    if let Some(t) = &tex {
                        let size = t.size_vec2();
                        let fit = (rect.width() / size.x).min(rect.height() / size.y);
                        ui.painter().image(t.id(), Rect::from_center_size(rect.center(), size * fit), Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)), Color32::WHITE);
                    }
                    // drag the window by the model; double click closes it
                    if resp.drag_started() {
                        ui.ctx().send_viewport_cmd(egui::ViewportCommand::StartDrag);
                    }
                    if resp.double_clicked() {
                        close = true;
                    }
                });
                ui.ctx().send_viewport_cmd(egui::ViewportCommand::MousePassthrough(passthrough));
                if ui.ctx().input(|i| i.viewport().close_requested()) {
                    close = true;
                }
            });
            if close {
                self.rigs.stream.window = false;
            }
            ctx.request_repaint();
        }
    }

    #[cfg(test)]
    pub fn test_stream(&mut self) -> &mut StreamUi {
        &mut self.rigs.stream
    }

    #[cfg(test)]
    pub fn test_stream_source(&mut self, i: usize, on: bool) {
        self.stream_set_source(i, on);
    }
}
