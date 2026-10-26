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
    /// The second virtual camera (Unity Capture): the sender while it is on, and how it is set.
    pub ucap: Option<crate::vcam::Sender>,
    pub ucap_upside: bool,
    pub ucap_alpha: bool,
    pub ucap_last: f64,
    pub ucap_status: String,
    pub ucap_registered: Option<bool>,
    /// The background of the Stream tab when it is a colour of your own (bg 4).
    pub bg_color: Color32,
    /// Where keys, pad buttons and MIDI notes come from, and which expressions are on.
    pub hub: Option<crate::hotkeys::Hub>,
    pub expr: crate::expressions::ExprState,
    pub new_expr: String,
    pub hk_action: u8,
    pub hk_expr: String,
    pub hk_input: Option<crate::hotkeys::Input>,
    /// Emotes: cut to the face, and which sizes.
    pub emote_face: bool,
    pub emote_sizes: [bool; 5],
    /// The test of how the model looks to viewers: the side of the picture, how hard it is squeezed (0 to 2), the result and its size.
    pub test_side: u32,
    pub test_quality: u8,
    pub test_pic: Option<(egui::TextureHandle, usize)>,
    /// OBS Studio remote control: where it is, the connection, and the scene a new hotkey would switch to.
    pub obs: Option<crate::obs::Obs>,
    /// A clip being recorded of the model: the video, its size, the time of the last picture and how many there are.
    pub rec: Option<(crate::recorder::Writer, (u32, u32), f64, u64)>,
    pub rec_path: std::path::PathBuf,
    pub link_port: u16,
    pub link_on: bool,
    /// Sending the face and expressions to VRChat over OSC.
    pub vrc: Option<crate::vrchat::VrChat>,
    pub vrc_host: String,
    pub vrc_port: u16,
    pub obs_host: String,
    pub obs_port: u16,
    pub obs_password: String,
    pub hk_scene: String,
    /// Where OBS Studio is on this computer (looked for when the panel is first shown), and what the last step of the set up said.
    pub obs_exe: Option<Option<std::path::PathBuf>>,
    pub vcam_message: Option<String>,
    /// Looking for OBS being open, connecting to it by itself when it is (with what its own settings say), and how it is going.
    pub obs_watch: Option<crate::obs::Watcher>,
    pub obs_auto: bool,
    obs_tries: u32,
    obs_last_try: f64,
    obs_was_running: bool,
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
            ucap: None,
            ucap_upside: true,
            ucap_alpha: false,
            ucap_last: 0.0,
            ucap_status: String::new(),
            ucap_registered: None,
            bg_color: Color32::from_rgb(40, 60, 90),
            hub: None,
            expr: Default::default(),
            new_expr: String::new(),
            hk_action: 0,
            hk_expr: String::new(),
            hk_input: None,
            emote_face: true,
            emote_sizes: [true, true, true, false, false],
            test_side: 720,
            test_quality: 1,
            test_pic: None,
            obs: None,
            rec: None,
            rec_path: std::path::PathBuf::new(),
            link_port: 39560,
            link_on: false,
            vrc: None,
            vrc_host: "127.0.0.1".into(),
            vrc_port: 9000,
            obs_host: "localhost".into(),
            obs_port: 4455,
            obs_password: String::new(),
            hk_scene: String::new(),
            obs_exe: None,
            vcam_message: None,
            obs_watch: None,
            // the tests must not reach for an OBS that happens to be open on the computer
            obs_auto: !cfg!(test),
            obs_tries: 0,
            obs_last_try: 0.0,
            obs_was_running: false,
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
        if self.rigs.stream.hub.is_none() {
            self.rigs.stream.hub = Some(crate::hotkeys::Hub::new());
        }
        self.stream_obs_watch(&ctx);
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
            4 => self.rigs.stream.bg_color,
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
        // sticks, triggers and knobs of the pads and MIDI devices are inputs too
        let device_axes = self.rigs.stream.hub.as_ref().map(|h| h.axis_values()).unwrap_or_default();
        for (name, v) in &device_axes {
            ch.insert(crate::track::intern(name), *v);
        }
        let rig = self.draw.doc.rig.clone();
        let map = &rig.tracking;
        let mouse = ctx.input(|i| i.pointer.hover_pos()).map(|p| {
            let r = ctx.content_rect();
            [((p.x - r.left()) / r.width() * 2.0 - 1.0).clamp(-1.0, 1.0), ((p.y - r.top()) / r.height() * 2.0 - 1.0).clamp(-1.0, 1.0)]
        });
        let mut state = std::mem::take(&mut self.rigs.stream.state);
        state.fill_in(&mut ch, map, now, mouse);
        // what the expressions did last frame is taken off before the face sets the sliders again
        self.rigs.stream.expr.undo(&mut self.rigs.values);
        if !map.maps.is_empty() {
            state.apply(map, &ch, &rig, &mut self.rigs.values, dt);
        }
        self.rigs.stream.state = state;
        // keys, pad buttons and MIDI notes switch expressions
        let events = match self.rigs.stream.hub.as_mut() {
            Some(h) => h.poll(&rig.hotkeys),
            None => Vec::new(),
        };
        for (action, pressed) in crate::hotkeys::dispatch(&rig.hotkeys, &events) {
            let e = &mut self.rigs.stream.expr;
            match action {
                crate::hotkeys::Action::Toggle(n) => e.toggle(&n),
                crate::hotkeys::Action::Show(n) => e.show_only(&n),
                crate::hotkeys::Action::Hold(n) => e.set(&n, pressed),
                crate::hotkeys::Action::Clear => e.clear(),
                crate::hotkeys::Action::ObsScene(n) => {
                    if let Some(o) = &self.rigs.stream.obs {
                        o.send(crate::obs::Cmd::Scene(n));
                    }
                }
                crate::hotkeys::Action::ObsStream => {
                    if let Some(o) = &self.rigs.stream.obs {
                        o.send(crate::obs::Cmd::ToggleStream);
                    }
                }
                crate::hotkeys::Action::ObsRecord => {
                    if let Some(o) = &self.rigs.stream.obs {
                        o.send(crate::obs::Cmd::ToggleRecord);
                    }
                }
                crate::hotkeys::Action::ObsVirtualCam => {
                    if let Some(o) = &self.rigs.stream.obs {
                        o.send(crate::obs::Cmd::ToggleVirtualCam);
                    }
                }
            }
        }
        let defs = crate::expressions::defs(&rig);
        self.rigs.stream.expr.apply(&defs, &rig, &mut self.rigs.values, dt);
        self.rigs.expr_layers = self.rigs.stream.expr.layer_levels(&defs);
        if self.rigs.stream.vrc.is_some() {
            let levels: Vec<(String, f32)> = defs.iter().map(|d| (d.name.clone(), self.rigs.stream.expr.level(&d.name))).collect();
            let st = &mut self.rigs.stream;
            let (inputs, tracked) = (st.inputs.clone(), st.tracked);
            if let Some(v) = st.vrc.as_mut() {
                v.send_frame(&inputs, &levels, tracked);
            }
        }
        if !rig.hotkeys.is_empty() || self.rigs.stream.hub.as_ref().is_some_and(|h| h.learn) {
            ctx.request_repaint_after(std::time::Duration::from_millis(16));
        }
        if self.rigs.stream.expr.any() {
            ctx.request_repaint();
        }
        let mut inputs: Vec<(&'static str, f32)> = CHANNELS.iter().filter_map(|c| ch.get(c.0).map(|v| (c.0, *v))).collect();
        inputs.truncate(40);
        self.rigs.stream.inputs = inputs;
        // the face moves all the time while a source sends
        let any_on = self.rigs.stream.sources.iter().any(|s| s.enabled) || rig.tracking.auto_blink || rig.tracking.breathing || rig.tracking.look_at_mouse;
        if any_on {
            ctx.request_repaint();
        }
    }

    /// Notice OBS being open, and connect to it by itself with what its own WebSocket settings say (while Connect by itself is on).
    fn stream_obs_watch(&mut self, ctx: &egui::Context) {
        let now = ctx.input(|i| i.time);
        if self.rigs.stream.obs_watch.is_none() {
            self.rigs.stream.obs_watch = Some(crate::obs::Watcher::start());
        }
        let running = self.rigs.stream.obs_watch.as_ref().map(|w| w.running()).unwrap_or(false);
        let st = &mut self.rigs.stream;
        if running != st.obs_was_running {
            st.obs_was_running = running;
            st.obs_tries = 0;
            if !running {
                // OBS was closed: the connection is over
                st.obs = None;
            } else {
                // OBS was started: look for it again (it may have just been installed)
                st.obs_exe = None;
            }
        }
        ctx.request_repaint_after(std::time::Duration::from_millis(1500));
        if !st.obs_auto || !running {
            return;
        }
        let status = st.obs.as_ref().map(|o| o.snapshot().status);
        let failed = matches!(status, Some(crate::obs::Status::Failed(_)));
        if (st.obs.is_none() || failed) && st.obs_tries < 8 && now - st.obs_last_try > 4.0 {
            let cfg = crate::obs::user_config_dir().and_then(|d| crate::obs::read_websocket_config(&d));
            st.obs_last_try = now;
            match cfg {
                Some(c) if c.enabled => {
                    st.obs_tries += 1;
                    st.obs_host = "localhost".into();
                    st.obs_port = c.port;
                    if let Some(p) = &c.password {
                        st.obs_password = p.clone();
                    }
                    st.obs = Some(crate::obs::Obs::connect("localhost", c.port, &st.obs_password));
                    st.vcam_message = Some("OBS is open: connecting to it.".into());
                }
                _ => {
                    // its remote control is off: say how to switch it on, once
                    st.obs_tries = 8;
                    st.vcam_message = Some("OBS is open, but its remote control is off. Press Let this program control OBS (close OBS first), or switch it on in OBS: Tools, WebSocket Server Settings.".into());
                }
            }
        }
    }

    /// Outside the Stream tab the expressions are not on: the sliders and layers are as they were set.
    pub(super) fn stream_release(&mut self) {
        if self.rigs.stream.expr.has_applied() {
            self.rigs.stream.expr.undo(&mut self.rigs.values);
        }
        if !self.rigs.expr_layers.is_empty() {
            self.rigs.expr_layers.clear();
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

        self.stream_expr_ui(ui);
        self.stream_obs_ui(ui);
        self.stream_vcam_ui(ui);
        self.stream_link_ui(ui);
        self.stream_vrchat_ui(ui);
        self.stream_hotkeys_ui(ui);

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

    fn stream_expr_ui(&mut self, ui: &mut Ui) {
        section_title(ui, "Expressions");
        let rig = self.draw.doc.rig.clone();
        let defs = crate::expressions::defs(&rig);
        if defs.is_empty() {
            ui.label(RichText::new("None yet. Pose the sliders (Rig tab, or the Animate tab), give the look a name here and save it. A Live2D model's own expressions show up here too.").size(11.5).color(TEXT_DIM));
        }
        let now = ui.ctx().input(|i| i.time);
        let mut delete: Option<String> = None;
        let mut fades: Vec<(String, f32)> = Vec::new();
        // a layer made to show or hide by an expression: (expression, layer, 0 nothing / 1 show / 2 hide)
        let mut layer_edit: Option<(String, u64, u8)> = None;
        let layer_names: Vec<(u64, String)> = self.draw.doc.layers.iter().filter(|l| matches!(l.kind, crate::doc::LayerKind::Raster | crate::doc::LayerKind::Vector)).map(|l| (l.id, l.name.clone())).collect();
        for d in &defs {
            let on = self.rigs.stream.expr.is_on(&d.name);
            let level = self.rigs.stream.expr.level(&d.name);
            ui.horizontal(|ui| {
                if widgets::chip(ui, &d.name, on).on_hover_text("Click to switch it on or off").clicked() {
                    self.rigs.stream.expr.toggle(&d.name);
                }
                let (r, _) = ui.allocate_exact_size(vec2(50.0, 6.0), Sense::hover());
                ui.painter().rect_filled(r, 3.0, Color32::from_rgb(28, 36, 33));
                ui.painter().rect_filled(Rect::from_min_size(r.min, vec2(r.width() * level, r.height())), 3.0, VENOM);
                if d.own {
                    let mut fade = d.fade_in;
                    if ui.add(egui::DragValue::new(&mut fade).range(0.0..=5.0).speed(0.02).suffix(" s")).on_hover_text("How long it takes to come and go").changed() {
                        fades.push((d.name.clone(), fade));
                    }
                    if ui.small_button("Delete").clicked() {
                        delete = Some(d.name.clone());
                    }
                }
            });
            if d.own && !layer_names.is_empty() {
                egui::CollapsingHeader::new(RichText::new("Layers it shows or hides").size(11.5).color(TEXT_DIM)).id_salt(("expr_layers", &d.name)).show(ui, |ui| {
                    egui::ScrollArea::vertical().max_height(140.0).id_salt(("expr_layers_scroll", &d.name)).show(ui, |ui| {
                        for (id, name) in &layer_names {
                            let mode = if d.show.contains(id) { 1 } else if d.hide.contains(id) { 2 } else { 0 };
                            ui.horizontal(|ui| {
                                ui.label(RichText::new(name).size(11.5));
                                for (m, label) in [(1u8, "Show"), (2, "Hide")] {
                                    if widgets::chip(ui, label, mode == m).clicked() {
                                        layer_edit = Some((d.name.clone(), *id, if mode == m { 0 } else { m }));
                                    }
                                }
                            });
                        }
                    });
                });
            }
        }
        if let Some((n, id, m)) = layer_edit {
            self.rig_edit("Expression layers", now, |r| {
                if let Some(e) = r.own_exprs.iter_mut().find(|e| e.name == n) {
                    e.show.retain(|x| *x != id);
                    e.hide.retain(|x| *x != id);
                    match m {
                        1 => e.show.push(id),
                        2 => e.hide.push(id),
                        _ => {}
                    }
                }
            });
        }
        if let Some(n) = delete {
            self.rig_edit("Delete expression", now, |r| r.own_exprs.retain(|e| e.name != n));
            self.rigs.stream.expr.set(&n, false);
        }
        if !fades.is_empty() {
            self.rig_edit("Expression fade", now, |r| {
                for (n, f) in &fades {
                    if let Some(e) = r.own_exprs.iter_mut().find(|e| e.name == *n) {
                        e.fade_in = *f;
                        e.fade_out = *f;
                    }
                }
            });
        }
        ui.horizontal(|ui| {
            ui.add(egui::TextEdit::singleline(&mut self.rigs.stream.new_expr).hint_text("Name").desired_width(110.0));
            let name = self.rigs.stream.new_expr.trim().to_string();
            if ui.add_enabled(!name.is_empty(), egui::Button::new("Save the sliders as an expression")).on_hover_text("Every slider that is not at its middle value now becomes part of the expression").clicked() {
                let base = self.rigs.stream.expr.without_effects(&self.rigs.values);
                let values: Vec<(crate::rig::Id, f32)> = rig.params.iter().filter_map(|p| base.0.get(&p.id).filter(|v| (**v - p.default).abs() > 0.01).map(|v| (p.id, *v))).collect();
                let n = values.len();
                self.rig_edit("Save expression", now, |r| {
                    r.own_exprs.retain(|e| e.name != name);
                    r.own_exprs.push(crate::expressions::OwnExpr { name: name.clone(), values, ..Default::default() });
                });
                self.rigs.stream.message = Some(format!("Saved {name}: {n} sliders."));
                self.rigs.stream.new_expr.clear();
            }
        });
        if ui.small_button("All expressions off").clicked() {
            self.rigs.stream.expr.clear();
        }
    }

    fn stream_vrchat_ui(&mut self, ui: &mut Ui) {
        section_title(ui, "VRChat");
        ui.label(RichText::new("Sends the face and the expressions to VRChat (OSC). An avatar with parameters named like the inputs (FaceAngleX, EyeOpenLeft, MouthOpen...) and Expr_<expression> follows them. In VRChat switch OSC on in the radial menu, Options, OSC.").size(11.5).color(TEXT_DIM));
        let on = self.rigs.stream.vrc.is_some();
        ui.horizontal_wrapped(|ui| {
            if widgets::chip(ui, "Send to VRChat", on).clicked() {
                if on {
                    self.rigs.stream.vrc = None;
                } else {
                    match crate::vrchat::VrChat::new(&self.rigs.stream.vrc_host, self.rigs.stream.vrc_port) {
                        Ok(v) => self.rigs.stream.vrc = Some(v),
                        Err(e) => self.rigs.stream.message = Some(e),
                    }
                }
            }
            ui.add_enabled(!on, egui::TextEdit::singleline(&mut self.rigs.stream.vrc_host).desired_width(100.0));
            ui.add_enabled(!on, egui::DragValue::new(&mut self.rigs.stream.vrc_port).range(1..=65535));
            if let Some(v) = &self.rigs.stream.vrc {
                ui.label(RichText::new(format!("{} messages sent", v.sent)).size(11.0).color(TEXT_DIM));
            }
        });
    }

    /// The virtual camera other programs can pick (Discord, Zoom, a browser, a game): OBS Studio makes it, this program sets OBS up.
    fn stream_vcam_ui(&mut self, ui: &mut Ui) {
        section_title(ui, "Virtual camera");
        ui.label(RichText::new("Shows the model as a camera that Discord, Zoom, a browser and other programs can pick, through the free OBS Studio (its OBS Virtual Camera). This program makes the scene in OBS and starts the camera. The camera has no see-through background, so the colour chosen under Show the model in other programs is put behind the model.").size(11.5).color(TEXT_DIM));
        if self.rigs.stream.obs_exe.is_none() {
            self.rigs.stream.obs_exe = Some(crate::obs::find_install());
        }
        let exe = self.rigs.stream.obs_exe.clone().flatten();
        let running = self.rigs.stream.obs_was_running;
        match &exe {
            Some(p) => {
                ui.label(RichText::new(format!("OBS Studio is installed: {}", p.display())).size(11.5).color(VENOM));
                ui.horizontal_wrapped(|ui| {
                    ui.label(RichText::new(if running { "OBS is open now." } else { "OBS is not open." }).size(11.5).color(if running { VENOM } else { TEXT_DIM }));
                    ui.checkbox(&mut self.rigs.stream.obs_auto, "Connect to it by itself").on_hover_text("When OBS is open, this program connects to it using the WebSocket settings OBS keeps itself");
                });
            }
            None => {
                ui.label(RichText::new("OBS Studio is not installed (or not in the usual place).").size(11.5).color(GOLD));
                ui.horizontal_wrapped(|ui| {
                    if ui.button("Get OBS Studio (free)").on_hover_text("Opens obsproject.com/download").clicked() {
                        ui.ctx().open_url(egui::OpenUrl::new_tab("https://obsproject.com/download"));
                    }
                    if ui.button("Install it with winget").on_hover_text("Opens a window that runs: winget install OBSProject.OBSStudio. Windows asks you to allow it.").clicked() {
                        let mut cmd = std::process::Command::new("cmd");
                        cmd.args(["/C", "start", "", "cmd", "/K", "winget install --id OBSProject.OBSStudio -e --accept-package-agreements --accept-source-agreements"]);
                        self.rigs.stream.vcam_message = Some(match cmd.spawn() {
                            Ok(_) => "A window installs OBS Studio. When it says it is done, press Look again.".to_string(),
                            Err(e) => format!("Could not start winget ({e}). Use Get OBS Studio instead."),
                        });
                    }
                    if ui.button("Look again").clicked() {
                        self.rigs.stream.obs_exe = None;
                    }
                });
            }
        }
        let connected = self.rigs.stream.obs.as_ref().map(|o| o.snapshot().status == crate::obs::Status::Connected).unwrap_or(false);
        if exe.is_some() && !connected {
            ui.horizontal_wrapped(|ui| {
                if ui.button("Let this program control OBS").on_hover_text("Switches the WebSocket server of OBS on with a password made now (OBS must be closed; its old settings file is kept). The password is put in the OBS box above.").clicked() {
                    self.rigs.stream.vcam_message = Some(if crate::obs::is_running() {
                        "Close OBS first: it only reads this setting when it starts.".to_string()
                    } else {
                        let appdata = std::env::var_os("APPDATA").map(std::path::PathBuf::from).unwrap_or_default().join("obs-studio");
                        let pw = crate::obs::make_password();
                        match crate::obs::enable_websocket(&appdata, &pw) {
                            Ok(_) => {
                                self.rigs.stream.obs_password = pw;
                                self.rigs.stream.obs_host = "localhost".into();
                                self.rigs.stream.obs_port = 4455;
                                "Done. Now start OBS, then press Connect above.".to_string()
                            }
                            Err(e) => e,
                        }
                    });
                }
                if ui.button("Start OBS").clicked() {
                    if let Some(p) = &exe {
                        self.rigs.stream.vcam_message = Some(match crate::obs::launch(p) {
                            Ok(()) => "OBS is starting. When its window is there, press Connect above.".to_string(),
                            Err(e) => e,
                        });
                    }
                }
            });
        }
        if connected {
            let on = self.rigs.stream.obs.as_ref().map(|o| o.snapshot().virtualcam).unwrap_or(false);
            ui.horizontal_wrapped(|ui| {
                if !on && ui.button("Make the virtual camera").on_hover_text("Makes the scene in OBS with the model and the background colour, shows it and starts the camera").clicked() {
                    // the model has to be served for OBS to show it
                    if self.rigs.stream.server.is_none() {
                        match BrowserSource::start(self.rigs.stream.server_port) {
                            Ok(s) => self.rigs.stream.server = Some(s),
                            Err(e) => self.rigs.stream.vcam_message = Some(e),
                        }
                    }
                    if let Some(s) = &self.rigs.stream.server {
                        let (w, h) = (self.draw.doc.w as f32, self.draw.doc.h as f32);
                        let side = self.rigs.stream.server_side.clamp(360, 1080) as f32;
                        let k = side / w.max(h);
                        let (vw, vh) = (((w * k) as u32 / 2 * 2).max(2), ((h * k) as u32 / 2 * 2).max(2));
                        let bg = self.stream_bg();
                        if let Some(o) = &self.rigs.stream.obs {
                            o.send(crate::obs::Cmd::SetupModel { url: s.url(), width: vw, height: vh, background: [bg.r(), bg.g(), bg.b()] });
                            self.rigs.stream.vcam_message = Some("The camera starts. In Discord, Zoom or the browser pick the camera called OBS Virtual Camera.".into());
                        }
                    }
                }
                if on && ui.button("Stop the virtual camera").clicked() {
                    if let Some(o) = &self.rigs.stream.obs {
                        o.send(crate::obs::Cmd::StopVirtualCam);
                    }
                }
                ui.label(RichText::new(if on { "The virtual camera is on: pick OBS Virtual Camera in the other program." } else { "The virtual camera is off." }).size(11.5).color(if on { VENOM } else { TEXT_DIM }));
            });
        }
        if let Some(m) = &self.rigs.stream.vcam_message {
            ui.label(RichText::new(m).size(11.5).color(TEXT_DIM));
        }
        self.stream_unity_capture_ui(ui);
    }

    /// The second virtual camera: Unity Capture (no OBS needed).
    fn stream_unity_capture_ui(&mut self, ui: &mut Ui) {
        ui.add_space(6.0);
        ui.label(RichText::new("Another virtual camera: Unity Capture").strong());
        ui.label(RichText::new("A free camera called Unity Video Capture (MIT licence) that works without OBS. It is installed once, then this program sends the model to it.").size(11.5).color(TEXT_DIM));
        let registered = *self.rigs.stream.ucap_registered.get_or_insert_with(crate::vcam::registered);
        if !registered {
            ui.label(RichText::new("It is not installed on this computer yet.").size(11.5).color(GOLD));
            ui.horizontal_wrapped(|ui| {
                if ui.button("Download it").on_hover_text("Opens File, Downloads: press Download next to Unity Capture").clicked() {
                    self.open_downloads();
                }
                if ui.button("Check again").on_hover_text("After running Install.bat in the Install folder").clicked() {
                    self.rigs.stream.ucap_registered = None;
                }
            });
            ui.label(RichText::new("After the download: open Downloads, UnityCapture, Install and run Install.bat (it asks to allow changes), then press Check again.").size(11.0).color(TEXT_DIM));
            return;
        }
        let on = self.rigs.stream.ucap.is_some();
        ui.horizontal_wrapped(|ui| {
            if widgets::chip(ui, "Send the model to Unity Video Capture", on).clicked() {
                self.rigs.stream.ucap = if on { None } else { Some(crate::vcam::Sender::new(0)) };
                self.rigs.stream.ucap_status.clear();
            }
            ui.checkbox(&mut self.rigs.stream.ucap_upside, "Upside down").on_hover_text("The camera wants the rows from the bottom. Turn this off if the picture comes out upside down in the other program");
            ui.checkbox(&mut self.rigs.stream.ucap_alpha, "See-through").on_hover_text("For programs that take a see-through camera (OBS does). Others show black behind the model; leave it off and the colour chosen above is put behind it");
        });
        if on {
            let msg = if self.rigs.stream.ucap_status.is_empty() { "Waiting for a program to use the camera: pick Unity Video Capture in Discord, Zoom, a browser or OBS." } else { self.rigs.stream.ucap_status.as_str() };
            ui.label(RichText::new(msg).size(11.5).color(TEXT_DIM));
        }
    }

    fn stream_link_ui(&mut self, ui: &mut Ui) {
        section_title(ui, "Link");
        ui.label(RichText::new("Lets other programs on this computer press inputs here: a Stream Deck or Touch Portal button that opens a web address, a game that sends its state. Then bind them to expressions and OBS actions below (Learn an input, then call the address).").size(11.5).color(TEXT_DIM));
        let on = self.rigs.stream.link_on;
        ui.horizontal_wrapped(|ui| {
            if widgets::chip(ui, "Link", on).clicked() {
                let port = self.rigs.stream.link_port;
                let want = !on;
                if let Some(h) = self.rigs.stream.hub.as_mut() {
                    match h.set_link(want.then_some(port)) {
                        Ok(()) => self.rigs.stream.link_on = want,
                        Err(e) => {
                            self.rigs.stream.message = Some(e);
                            self.rigs.stream.link_on = false;
                        }
                    }
                }
            }
            ui.label(RichText::new("Port").size(11.5).color(TEXT_DIM));
            ui.add_enabled(!on, egui::DragValue::new(&mut self.rigs.stream.link_port).range(1024..=65535));
        });
        if on {
            let port = self.rigs.stream.link_port;
            ui.label(RichText::new(format!("http://127.0.0.1:{port}/trigger/<name>")).monospace().size(11.5).color(VENOM));
            if let Some(l) = self.rigs.stream.hub.as_ref().and_then(|h| h.link.as_ref()).and_then(|l| l.last.lock().ok().and_then(|g| g.clone())) {
                ui.label(RichText::new(format!("Last heard: {l}")).size(11.5).color(TEXT_DIM));
            }
            egui::CollapsingHeader::new(RichText::new("Counter-Strike 2").size(11.5).color(TEXT_DIM)).id_salt("link_cs2").show(ui, |ui| {
                ui.label(RichText::new("Save this as gamestate_integration_seviath.cfg in the cfg folder of the game (steamapps/common/Counter-Strike Global Offensive/game/csgo/cfg). Events: cs2.kill, cs2.death, cs2.round_live, cs2.round_freezetime, cs2.round_over, cs2.bomb_planted.").size(11.0).color(TEXT_DIM));
                let cfg = crate::link::cs2_config(port);
                ui.add(egui::TextEdit::multiline(&mut cfg.clone()).font(egui::TextStyle::Monospace).desired_rows(8).desired_width(f32::INFINITY));
                if ui.small_button("Copy").clicked() {
                    ui.ctx().copy_text(cfg);
                }
            });
        }
    }

    fn stream_obs_ui(&mut self, ui: &mut Ui) {
        section_title(ui, "OBS Studio");
        ui.label(RichText::new("Switch scenes and start or stop the stream and the recording from here or from a hotkey. In OBS: Tools, WebSocket Server Settings, switch it on. The password is not saved.").size(11.5).color(TEXT_DIM));
        let snap = self.rigs.stream.obs.as_ref().map(|o| o.snapshot());
        ui.horizontal_wrapped(|ui| {
            ui.add_enabled(snap.is_none(), egui::TextEdit::singleline(&mut self.rigs.stream.obs_host).desired_width(110.0));
            ui.add_enabled(snap.is_none(), egui::DragValue::new(&mut self.rigs.stream.obs_port).range(1..=65535));
            ui.add_enabled(snap.is_none(), egui::TextEdit::singleline(&mut self.rigs.stream.obs_password).password(true).hint_text("Password").desired_width(90.0));
            if snap.is_none() {
                if ui.button("Connect").clicked() {
                    self.rigs.stream.obs = Some(crate::obs::Obs::connect(&self.rigs.stream.obs_host, self.rigs.stream.obs_port, &self.rigs.stream.obs_password));
                }
            } else if ui.button("Disconnect").clicked() {
                self.rigs.stream.obs = None;
            }
        });
        if let Some(s) = &snap {
            ui.ctx().request_repaint_after(std::time::Duration::from_millis(250));
            match &s.status {
                crate::obs::Status::Connecting => {
                    ui.label(RichText::new("Connecting...").size(11.5).color(GOLD));
                }
                crate::obs::Status::Failed(e) => {
                    ui.label(RichText::new(e).size(11.5).color(Color32::from_rgb(255, 120, 100)));
                }
                crate::obs::Status::Connected => {
                    ui.horizontal_wrapped(|ui| {
                        for name in &s.scenes {
                            if widgets::chip(ui, name, s.current.as_deref() == Some(name.as_str())).clicked() {
                                if let Some(o) = &self.rigs.stream.obs {
                                    o.send(crate::obs::Cmd::Scene(name.clone()));
                                }
                            }
                        }
                    });
                    ui.horizontal_wrapped(|ui| {
                        if widgets::chip(ui, if s.streaming { "Streaming: stop" } else { "Start streaming" }, s.streaming).clicked() {
                            if let Some(o) = &self.rigs.stream.obs {
                                o.send(crate::obs::Cmd::ToggleStream);
                            }
                        }
                        if widgets::chip(ui, if s.recording { "Recording: stop" } else { "Start recording" }, s.recording).clicked() {
                            if let Some(o) = &self.rigs.stream.obs {
                                o.send(crate::obs::Cmd::ToggleRecord);
                            }
                        }
                    });
                }
                crate::obs::Status::Off => {
                    ui.label(RichText::new("Disconnected.").size(11.5).color(TEXT_DIM));
                }
            }
        }
    }

    fn stream_hotkeys_ui(&mut self, ui: &mut Ui) {
        section_title(ui, "Hotkeys");
        ui.label(RichText::new("A key, a MIDI note or a pad button switches an expression. Keys work while a game has the focus (Windows). Keep this tab open while streaming.").size(11.5).color(TEXT_DIM));
        let rig = self.draw.doc.rig.clone();
        let now = ui.ctx().input(|i| i.time);
        // devices
        let (ports, current, pads) = match &self.rigs.stream.hub {
            Some(h) => (h.midi_ports.clone(), h.midi_name.clone(), h.pad_names.lock().map(|p| p.clone()).unwrap_or_default()),
            None => (Vec::new(), None, Vec::new()),
        };
        ui.horizontal_wrapped(|ui| {
            ui.label(RichText::new("MIDI device").size(12.0).color(TEXT_DIM));
            let label = current.clone().unwrap_or_else(|| if ports.is_empty() { "none found".into() } else { "off".into() });
            egui::ComboBox::from_id_salt("midi_port").selected_text(label).width(160.0).show_ui(ui, |ui| {
                if ui.selectable_label(current.is_none(), "Off").clicked() {
                    if let Some(h) = self.rigs.stream.hub.as_mut() {
                        h.disconnect_midi();
                    }
                }
                for p in &ports {
                    if ui.selectable_label(current.as_deref() == Some(p.as_str()), p).clicked() {
                        if let Some(h) = self.rigs.stream.hub.as_mut() {
                            if let Err(e) = h.connect_midi(p) {
                                self.rigs.stream.message = Some(e);
                            }
                        }
                    }
                }
            });
            if ui.small_button("Look again").clicked() {
                if let Some(h) = self.rigs.stream.hub.as_mut() {
                    h.refresh_midi_ports();
                }
            }
        });
        ui.label(RichText::new(if pads.is_empty() { "No game pad found (it is looked for every two seconds).".to_string() } else { format!("Game pad: {}", pads.join(", ")) }).size(11.5).color(TEXT_DIM));
        // what is bound
        let mut remove: Option<usize> = None;
        for (i, b) in rig.hotkeys.iter().enumerate() {
            ui.horizontal_wrapped(|ui| {
                ui.label(RichText::new(b.input.label()).monospace().size(11.5).color(VENOM));
                ui.label(RichText::new(b.action.label()).size(12.0));
                if ui.small_button("Delete").clicked() {
                    remove = Some(i);
                }
            });
        }
        if let Some(i) = remove {
            self.rig_edit("Delete hotkey", now, |r| {
                if i < r.hotkeys.len() {
                    r.hotkeys.remove(i);
                }
            });
        }
        // a new one
        let learning = self.rigs.stream.hub.as_ref().is_some_and(|h| h.learn);
        if let Some(h) = self.rigs.stream.hub.as_mut() {
            if let Some(i) = h.learned.take() {
                self.rigs.stream.hk_input = Some(i);
            }
        }
        ui.horizontal_wrapped(|ui| {
            let label = match (&self.rigs.stream.hk_input, learning) {
                (_, true) => "Press a key, a pad button or a MIDI note...".to_string(),
                (Some(i), _) => i.label(),
                (None, _) => "Learn an input".to_string(),
            };
            if widgets::chip(ui, &label, learning).on_hover_text("Click, then press what should switch the expression").clicked() {
                if let Some(h) = self.rigs.stream.hub.as_mut() {
                    h.learn = !learning;
                }
            }
        });
        let names: Vec<String> = crate::expressions::defs(&rig).into_iter().map(|d| d.name).collect();
        ui.horizontal_wrapped(|ui| {
            const ACTIONS: [&str; 8] = ["Switch on or off", "Show only", "While held", "All off", "OBS scene", "OBS stream on or off", "OBS recording on or off", "OBS virtual camera on or off"];
            egui::ComboBox::from_id_salt("hk_action").selected_text(ACTIONS[self.rigs.stream.hk_action as usize % 8]).width(150.0).show_ui(ui, |ui| {
                for (i, n) in ACTIONS.iter().enumerate() {
                    if ui.selectable_label(self.rigs.stream.hk_action == i as u8, *n).clicked() {
                        self.rigs.stream.hk_action = i as u8;
                    }
                }
            });
            if self.rigs.stream.hk_action == 4 {
                let scenes: Vec<String> = self.rigs.stream.obs.as_ref().map(|o| o.snapshot().scenes).unwrap_or_default();
                if scenes.is_empty() {
                    ui.add(egui::TextEdit::singleline(&mut self.rigs.stream.hk_scene).hint_text("Scene name").desired_width(110.0));
                } else {
                    let sel = if scenes.contains(&self.rigs.stream.hk_scene) { self.rigs.stream.hk_scene.clone() } else { scenes[0].clone() };
                    egui::ComboBox::from_id_salt("hk_scene").selected_text(sel.clone()).width(120.0).show_ui(ui, |ui| {
                        for n in &scenes {
                            if ui.selectable_label(&sel == n, n).clicked() {
                                self.rigs.stream.hk_scene = n.clone();
                            }
                        }
                    });
                    self.rigs.stream.hk_scene = sel;
                }
            }
            if self.rigs.stream.hk_action < 3 {
                let sel = if names.contains(&self.rigs.stream.hk_expr) { self.rigs.stream.hk_expr.clone() } else { names.first().cloned().unwrap_or_default() };
                egui::ComboBox::from_id_salt("hk_expr").selected_text(sel.clone()).width(120.0).show_ui(ui, |ui| {
                    for n in &names {
                        if ui.selectable_label(&sel == n, n).clicked() {
                            self.rigs.stream.hk_expr = n.clone();
                        }
                    }
                });
                self.rigs.stream.hk_expr = sel;
            }
            let ready = self.rigs.stream.hk_input.is_some()
                && match self.rigs.stream.hk_action {
                    0..=2 => !self.rigs.stream.hk_expr.is_empty(),
                    4 => !self.rigs.stream.hk_scene.trim().is_empty(),
                    _ => true,
                };
            if ui.add_enabled(ready, egui::Button::new("Add")).clicked() {
                let n = self.rigs.stream.hk_expr.clone();
                let action = match self.rigs.stream.hk_action {
                    0 => crate::hotkeys::Action::Toggle(n),
                    1 => crate::hotkeys::Action::Show(n),
                    2 => crate::hotkeys::Action::Hold(n),
                    4 => crate::hotkeys::Action::ObsScene(self.rigs.stream.hk_scene.trim().to_string()),
                    5 => crate::hotkeys::Action::ObsStream,
                    6 => crate::hotkeys::Action::ObsRecord,
                    7 => crate::hotkeys::Action::ObsVirtualCam,
                    _ => crate::hotkeys::Action::Clear,
                };
                if let Some(input) = self.rigs.stream.hk_input.take() {
                    self.rig_edit("Add hotkey", now, |r| r.hotkeys.push(crate::hotkeys::Binding { input, action }));
                }
            }
        });
    }

    /// What follows your face, chosen in one place under Calibrate: the camera of this computer (MediaPipe), or an app that sends it.
    fn stream_tracker_picker(&mut self, ui: &mut Ui) {
        let current = self.rigs.stream.sources.iter().position(|s| s.enabled);
        let names: Vec<String> = self
            .rigs
            .stream
            .sources
            .iter()
            .map(|s| match s.protocol {
                None => "Camera of this computer (MediaPipe)".to_string(),
                Some(_) => s.label.to_string(),
            })
            .collect();
        let name_of = |i: usize| -> String { names[i].clone() };
        ui.label(RichText::new("Follow the face with").size(12.0).color(TEXT_DIM));
        ui.horizontal(|ui| {
            let mut pick: Option<Option<usize>> = None;
            egui::ComboBox::from_id_salt("stream_tracker").selected_text(current.map(&name_of).unwrap_or_else(|| "nothing (it blinks by itself)".into())).width(240.0).show_ui(ui, |ui| {
                if ui.selectable_label(current.is_none(), "nothing (it blinks by itself)").clicked() {
                    pick = Some(None);
                }
                for i in 0..self.rigs.stream.sources.len() {
                    if ui.selectable_label(current == Some(i), name_of(i)).clicked() {
                        pick = Some(Some(i));
                    }
                }
            });
            if let Some(p) = pick {
                // one tracker at a time here; the list on the left can still mix several
                for i in 0..self.rigs.stream.sources.len() {
                    if Some(i) != p && self.rigs.stream.sources[i].enabled {
                        self.stream_set_source(i, false);
                    }
                }
                if let Some(i) = p {
                    if self.rigs.stream.sources[i].protocol.is_none() && !crate::aisetup::tracker_ready() {
                        self.rigs.stream.sources[i].error = Some("The camera tracking files are missing: File, Downloads, Tracking models and the AI files card in the Rig tab.".into());
                    } else {
                        self.stream_set_source(i, true);
                    }
                }
            }
        });
        ui.label(RichText::new("Every tracker gives the same numbers (the turn of the head, the eyes, the mouth, the brows), so they all move 2D and 3D characters alike.").size(11.0).color(TEXT_DIM));
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
        self.stream_tracker_picker(ui);
        egui::CollapsingHeader::new(RichText::new("Advanced: which slider follows what").size(13.0).strong()).id_salt("stream_mapping_adv").default_open(false).show(ui, |ui| {
        ui.label(RichText::new("Make the mapping does this for you. Here you can change it: every line says which slider moves with which part of your face.").size(11.5).color(TEXT_DIM));
        if m.maps.is_empty() {
            ui.label(RichText::new("None yet. Press Make the mapping, or add one below.").size(11.5).color(TEXT_DIM));
        }
        let mut remove: Option<usize> = None;
        let device_inputs: Vec<(String, f32)> = self.rigs.stream.hub.as_ref().map(|h| h.axis_values()).unwrap_or_default();
        for (i, mp) in m.maps.iter_mut().enumerate() {
            let name = rig.param(mp.param).map(|p| p.name.clone()).unwrap_or_else(|| "(gone)".into());
            egui::CollapsingHeader::new(RichText::new(format!("{name}  from  {}", mp.input)).size(12.0)).id_salt(("map", i)).show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.label(RichText::new("Input").size(11.5).color(TEXT_DIM));
                    egui::ComboBox::from_id_salt(("map_in", i)).selected_text(mp.input.clone()).width(150.0).show_ui(ui, |ui| {
                        for (name, _) in &device_inputs {
                            if ui.selectable_label(mp.input == *name, format!("{name}  (device)")).clicked() {
                                mp.input = name.clone();
                                let sticky = name.starts_with("Pad ") && !name.contains("Trigger") && !name.contains("Z");
                                (mp.in_lo, mp.in_hi) = if sticky { (-1.0, 1.0) } else { (0.0, 1.0) };
                            }
                        }
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
            // the face inputs first, then the sticks and knobs that have moved
            let total = CHANNELS.len() + device_inputs.len();
            let ci = self.rigs.stream.add_input.min(total - 1);
            let label_of = |i: usize| -> String { if i < CHANNELS.len() { CHANNELS[i].0.to_string() } else { device_inputs[i - CHANNELS.len()].0.clone() } };
            egui::ComboBox::from_id_salt("map_add_input").selected_text(label_of(ci)).width(130.0).show_ui(ui, |ui| {
                for i in 0..total {
                    if ui.selectable_label(ci == i, label_of(i)).clicked() {
                        self.rigs.stream.add_input = i;
                    }
                }
            });
            if ui.add_enabled(sel.is_some(), egui::Button::new("Add")).clicked() {
                if let Some((id, _, lo, hi)) = sel.and_then(|id| names.iter().find(|n| n.0 == id)).cloned() {
                    let (input, in_lo, in_hi) = if ci < CHANNELS.len() {
                        (CHANNELS[ci].0.to_string(), CHANNELS[ci].2, CHANNELS[ci].3)
                    } else {
                        let n = label_of(ci);
                        let sticky = n.starts_with("Pad ") && !n.contains("Trigger") && !n.contains("Z");
                        (n, if sticky { -1.0 } else { 0.0 }, 1.0)
                    };
                    m.maps.push(Map { param: id, input, in_lo, in_hi, out_lo: lo, out_hi: hi, clamp: true, smoothing: 8.0, invert: false });
                }
            }
        });
        });
        if m != rig.tracking {
            let now = ui.ctx().input(|i| i.time);
            self.rig_edit("Tracking mapping", now, |r| r.tracking = m);
        }
        if let Some(msg) = &self.rigs.stream.message {
            ui.label(RichText::new(msg).size(11.5).color(TEXT_DIM));
        }
        self.stream_output_ui(ui);
        self.stream_ready_ui(ui);
        self.stream_test_ui(ui);
        self.stream_clip_ui(ui);
        self.stream_emotes_ui(ui);
    }

    /// What is ready for going live and what is not.
    pub(super) fn stream_checklist(&self) -> Vec<(bool, String)> {
        let rig = &self.draw.doc.rig;
        let st = &self.rigs.stream;
        let mut v = vec![
            (!rig.meshes.is_empty(), "The character has parts that move (Rig tab)".to_string()),
            (st.sources.iter().any(|s| s.enabled), "A tracking source is on".to_string()),
            (st.tracked, "A face is being followed right now".to_string()),
            (!rig.tracking.maps.is_empty(), "The face moves sliders (the mapping is made)".to_string()),
            (!rig.tracking.neutral.is_empty(), "The face is calibrated".to_string()),
            (st.server.is_some() || st.window, "The model goes out: the OBS browser source or the model window is on".to_string()),
        ];
        let obs = st.obs.as_ref().map(|o| o.snapshot().status);
        v.push((obs == Some(crate::obs::Status::Connected), format!("OBS Studio is connected{} (only needed for scene switching and the stream buttons)", if st.obs_was_running { "" } else { ": it is not open" })));
        v.push((!crate::expressions::defs(rig).is_empty(), "There are expressions to switch".to_string()));
        v.push((!rig.hotkeys.is_empty(), "There are hotkeys for them".to_string()));
        v
    }

    fn stream_ready_ui(&mut self, ui: &mut Ui) {
        section_title(ui, "Ready to go live?");
        let list = self.stream_checklist();
        let done = list.iter().filter(|(ok, _)| *ok).count();
        for (ok, text) in &list {
            ui.horizontal_wrapped(|ui| {
                ui.label(RichText::new(if *ok { "Yes" } else { "Not yet" }).size(11.5).color(if *ok { VENOM } else { GOLD }));
                ui.label(RichText::new(text).size(11.5).color(if *ok { TEXT } else { TEXT_DIM }));
            });
        }
        ui.label(RichText::new(format!("{done} of {} ready.", list.len())).size(11.5).color(TEXT_DIM));
    }

    /// Make the picture of the model the way viewers get it: a smaller size and squeezed, so what a bad connection does can be seen.
    pub(super) fn stream_test_picture(&mut self, ctx: &egui::Context) -> Option<usize> {
        let (w, h) = (self.draw.doc.w, self.draw.doc.h);
        let side = self.rigs.stream.test_side.clamp(120, 2160);
        let k = (side as f32 / w.max(h) as f32).min(1.0);
        let size = (((w as f32 * k).round() as u32).max(8), ((h as f32 * k).round() as u32).max(8));
        let bg = self.stream_bg();
        let img = self.rig_still(size, Some(bg))?;
        let rgb = image::DynamicImage::ImageRgba8(img).to_rgb8();
        let quality = [20u8, 45, 75][self.rigs.stream.test_quality.min(2) as usize];
        let mut bytes = Vec::new();
        image::codecs::jpeg::JpegEncoder::new_with_quality(&mut bytes, quality).encode_image(&rgb).ok()?;
        let back = image::load_from_memory(&bytes).ok()?.to_rgba8();
        let ci = egui::ColorImage::from_rgba_unmultiplied([back.width() as usize, back.height() as usize], back.as_raw());
        let tex = ctx.load_texture("stream_test", ci, egui::TextureOptions::LINEAR);
        self.rigs.stream.test_pic = Some((tex, bytes.len()));
        Some(bytes.len())
    }

    fn stream_test_ui(&mut self, ui: &mut Ui) {
        section_title(ui, "Test the look");
        ui.label(RichText::new("Shows the model smaller and squeezed, as it looks to viewers with a slow connection.").size(11.5).color(TEXT_DIM));
        ui.horizontal_wrapped(|ui| {
            for (side, name) in [(480u32, "480p"), (720, "720p"), (1080, "1080p")] {
                if widgets::chip(ui, name, self.rigs.stream.test_side == side).clicked() {
                    self.rigs.stream.test_side = side;
                }
            }
            for (i, name) in ["Squeezed hard", "Squeezed", "Good"].iter().enumerate() {
                if widgets::chip(ui, name, self.rigs.stream.test_quality == i as u8).clicked() {
                    self.rigs.stream.test_quality = i as u8;
                }
            }
        });
        if ui.button("Make the test picture").clicked() {
            let ctx = ui.ctx().clone();
            if self.stream_test_picture(&ctx).is_none() {
                self.rigs.stream.message = Some("There is nothing to draw: the rig has no meshes yet.".into());
            }
        }
        if let Some((tex, bytes)) = &self.rigs.stream.test_pic {
            let w = ui.available_width().min(320.0);
            let s = tex.size_vec2();
            ui.image((tex.id(), egui::vec2(w, w * s.y / s.x.max(1.0))));
            ui.label(RichText::new(format!("{} KB for one picture", bytes / 1024)).size(11.0).color(TEXT_DIM));
        }
    }

    /// Start a clip of the model (video file) at the stream picture rate, or stop the one that is recording.
    fn stream_clip_ui(&mut self, ui: &mut Ui) {
        section_title(ui, "Clip");
        ui.label(RichText::new("Records the model, as it moves with your face, to a video (MP4) on the background colour chosen above.").size(11.5).color(TEXT_DIM));
        match self.rigs.stream.rec.as_ref().map(|r| r.3) {
            Some(n) => {
                let secs = n as f32 / self.settings.stream_fps.clamp(5, 60) as f32;
                ui.horizontal(|ui| {
                    if ui.button("Stop and save").clicked() {
                        self.stream_stop_clip();
                    }
                    ui.label(RichText::new(format!("Recording {secs:.1} s")).size(12.0).color(Color32::from_rgb(255, 120, 100)));
                });
            }
            None => {
                if ui.button("Record a clip...").clicked() {
                    if let Some(p) = rfd::FileDialog::new().set_title("Save the clip").set_directory(self.settings.projects_folder()).set_file_name("clip.mp4").add_filter("MP4 video", &["mp4"]).save_file() {
                        let (w, h) = (self.draw.doc.w, self.draw.doc.h);
                        let size = crate::recorder::video_size(w, h, self.rigs.stream.server_side.min(1080));
                        let fps = self.settings.stream_fps.clamp(5, 60);
                        match crate::recorder::Writer::new(&p, size, fps) {
                            Ok(wr) => {
                                self.rigs.stream.rec = Some((wr, size, 0.0, 0));
                                self.rigs.stream.rec_path = p;
                            }
                            Err(e) => self.rigs.stream.message = Some(e),
                        }
                    }
                }
            }
        }
    }

    pub(super) fn stream_stop_clip(&mut self) {
        if let Some((w, _, _, n)) = self.rigs.stream.rec.take() {
            w.finish();
            self.rigs.stream.message = Some(format!("Saved the clip ({n} pictures): {}", self.rigs.stream.rec_path.display()));
        }
    }

    fn stream_emotes_ui(&mut self, ui: &mut Ui) {
        section_title(ui, "Emotes");
        ui.label(RichText::new("A picture of the character for every expression (and the plain face), with a see-through background, at the sizes chat services ask for.").size(11.5).color(TEXT_DIM));
        ui.horizontal_wrapped(|ui| {
            ui.checkbox(&mut self.rigs.stream.emote_face, "Cut to the face");
            for (i, (name, _)) in crate::emotes::SIZES.iter().enumerate() {
                ui.checkbox(&mut self.rigs.stream.emote_sizes[i], *name);
            }
        });
        if ui.add_enabled(self.rigs.stream.emote_sizes.iter().any(|s| *s), egui::Button::new("Save the emotes...")).clicked() {
            if let Some(dir) = rfd::FileDialog::new().set_title("Folder for the emotes").set_directory(self.settings.projects_folder()).pick_folder() {
                self.rigs.stream.message = Some(match self.stream_save_emotes(&dir) {
                    Ok(n) => format!("Saved {n} pictures in {}.", dir.display()),
                    Err(e) => e,
                });
            }
        }
    }

    /// Draw the plain face and every expression fully on, cut and shrunk, into a folder. Returns how many pictures were saved.
    pub(super) fn stream_save_emotes(&mut self, dir: &std::path::Path) -> Result<usize, String> {
        let rig = self.draw.doc.rig.clone();
        let defs = crate::expressions::defs(&rig);
        let saved = self.rigs.values.clone();
        let base = self.rigs.stream.expr.without_effects(&saved);
        let (w, h) = (self.draw.doc.w as f32, self.draw.doc.h as f32);
        let k = (1024.0 / w.max(h)).min(1.0);
        let size = (((w * k).round() as u32).max(8), ((h * k).round() as u32).max(8));
        // the face: the box around the parts that make it
        let mut lo = [f32::MAX; 2];
        let mut hi = [f32::MIN; 2];
        for l in &self.draw.doc.layers {
            use crate::parts::Part::*;
            if !matches!(l.part, Some(Face | EyeL | EyeR | EyeballL | EyeballR | BrowL | BrowR | Nose | Mouth | MouthInside | Blush | HairFront)) {
                continue;
            }
            if let Some(m) = rig.mesh_of(l.id) {
                for v in &m.verts {
                    for a in 0..2 {
                        lo[a] = lo[a].min(v[a] * k);
                        hi[a] = hi[a].max(v[a] * k);
                    }
                }
            }
        }
        let around = (self.rigs.stream.emote_face && lo[0] < hi[0] && lo[1] < hi[1]).then_some([lo[0], lo[1], hi[0], hi[1]]);
        let mut poses: Vec<(String, crate::rig::Values)> = vec![("Neutral".into(), base.clone())];
        poses.extend(defs.iter().map(|d| (d.name.clone(), crate::expressions::pose_with(d, &rig, &base))));
        std::fs::create_dir_all(dir).map_err(|e| format!("Could not make the folder: {e}"))?;
        let sizes: Vec<u32> = crate::emotes::SIZES.iter().enumerate().filter(|(i, _)| self.rigs.stream.emote_sizes[*i]).map(|(_, s)| s.1).collect();
        let mut count = 0;
        let mut error = None;
        for (name, values) in poses {
            self.rigs.values = values;
            let Some(img) = self.rig_still(size, None) else {
                error = Some("There is nothing to draw: the rig has no meshes yet.".to_string());
                break;
            };
            let cut = crate::emotes::square_cut(&img, around, if around.is_some() { 1.35 } else { 1.0 });
            for s in &sizes {
                let path = dir.join(format!("{}_{s}.png", crate::emotes::file_name(&name)));
                if let Err(e) = crate::emotes::shrink(&cut, *s).save(&path) {
                    error = Some(format!("Could not save {}: {e}", path.display()));
                    break;
                }
                count += 1;
            }
            if error.is_some() {
                break;
            }
        }
        self.rigs.values = saved;
        match error {
            Some(e) => Err(e),
            None => Ok(count),
        }
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
            ui.label(RichText::new("or any colour").size(11.5).color(TEXT_DIM));
            let mut c = self.rigs.stream.bg_color;
            if egui::color_picker::color_edit_button_srgba(ui, &mut c, egui::color_picker::Alpha::Opaque).changed() {
                self.rigs.stream.bg_color = c;
                self.rigs.stream.bg = 4;
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
        // a clip of the model
        if let Some((_, size, last, _)) = self.rigs.stream.rec.as_ref().map(|r| (0, r.1, r.2, 0)) {
            let fps = self.settings.stream_fps.clamp(5, 60) as f64;
            if now - last >= 1.0 / fps {
                let bg = self.stream_bg();
                if let Some(img) = self.rig_still(size, Some(bg)) {
                    let mut rgb = Vec::with_capacity((img.width() * img.height() * 3) as usize);
                    for p in img.pixels() {
                        rgb.extend_from_slice(&p.0[..3]);
                    }
                    let mut stop: Option<String> = None;
                    if let Some(r) = self.rigs.stream.rec.as_mut() {
                        r.2 = now;
                        match r.0.write(rgb) {
                            Ok(()) => r.3 += 1,
                            Err(e) => stop = Some(e),
                        }
                    }
                    if let Some(e) = stop {
                        self.stream_stop_clip();
                        self.rigs.stream.message = Some(format!("The clip stopped: {e}"));
                    }
                }
            }
            ctx.request_repaint();
        }
        // the second virtual camera
        if self.rigs.stream.ucap.is_some() && now - self.rigs.stream.ucap_last >= 1.0 / self.settings.stream_fps.clamp(5, 60) as f64 {
            self.rigs.stream.ucap_last = now;
            let (w, h) = (self.draw.doc.w, self.draw.doc.h);
            let k = (self.rigs.stream.server_side.clamp(256, 1080) as f32 / w.max(h) as f32).min(1.0);
            // an even size, as cameras like it
            let size = (((w as f32 * k) as u32 / 2 * 2).max(8), ((h as f32 * k) as u32 / 2 * 2).max(8));
            let paper = if self.rigs.stream.ucap_alpha { None } else { Some(self.stream_bg()) };
            if let Some(img) = self.rig_still(size, paper) {
                let upside = self.rigs.stream.ucap_upside;
                let sent = self.rigs.stream.ucap.as_mut().map(|s| s.send(&img, upside));
                self.rigs.stream.ucap_status = match sent {
                    Some(crate::vcam::Sent::Delivered) | Some(crate::vcam::Sent::Skipped) => "Sending the model: a program is using the camera.".into(),
                    Some(crate::vcam::Sent::TooBig) => "The picture is too big for the camera (more than 4K).".into(),
                    _ => String::new(),
                };
            }
            ctx.request_repaint();
        }
        // the browser source gets a new picture about 30 times a second
        if self.rigs.stream.server.is_some() && now - self.rigs.stream.last_sent >= 1.0 / self.settings.stream_fps.clamp(1, 120) as f64 {
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
    pub fn test_checklist(&self) -> Vec<(bool, String)> {
        self.stream_checklist()
    }

    #[cfg(test)]
    pub fn test_stream_test_picture(&mut self, ctx: &egui::Context) -> Option<usize> {
        self.stream_test_picture(ctx)
    }

    #[cfg(test)]
    pub fn test_stop_clip(&mut self) {
        self.stream_stop_clip();
    }

    #[cfg(test)]
    pub fn test_stream_clip(&mut self, path: &std::path::Path) -> Result<(), String> {
        let (w, h) = (self.draw.doc.w, self.draw.doc.h);
        let size = crate::recorder::video_size(w, h, 240);
        let wr = crate::recorder::Writer::new(path, size, 30)?;
        self.rigs.stream.rec = Some((wr, size, 0.0, 0));
        self.rigs.stream.rec_path = path.to_path_buf();
        Ok(())
    }

    #[cfg(test)]
    pub fn test_stream_emotes(&mut self, dir: &std::path::Path) -> Result<usize, String> {
        self.stream_save_emotes(dir)
    }

    #[cfg(test)]
    pub fn test_stream_source(&mut self, i: usize, on: bool) {
        self.stream_set_source(i, on);
    }
}
