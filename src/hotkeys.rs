// SPDX-License-Identifier: GPL-3.0-or-later
// Hotkeys for the Stream tab: a key of the keyboard (it works while another program, a game, has the focus, on Windows), a note of a
// MIDI keyboard or pad, or a button of a game pad turns an expression on or off. The inputs are read here and turned into plain events
// ("this was pressed", "this was let go"); the bindings say what each one does.

use std::collections::HashSet;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use serde::{Deserialize, Serialize};

/// Something a person can press.
#[derive(Clone, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub enum Input {
    /// A key of the keyboard (the Windows key number) with the modifier keys that must be down.
    Key { vk: u32, ctrl: bool, shift: bool, alt: bool },
    /// A MIDI note on a channel (1 to 16).
    Midi { channel: u8, note: u8 },
    /// A button of a game pad, by its name ("South", "LeftTrigger"...).
    Pad { button: String },
    /// Something another program asked for through Seviath Link (a Stream Deck button, a game event).
    Link { name: String },
}

impl Input {
    pub fn label(&self) -> String {
        match self {
            Input::Key { vk, ctrl, shift, alt } => {
                let mut s = String::new();
                for (on, n) in [(*ctrl, "Ctrl+"), (*shift, "Shift+"), (*alt, "Alt+")] {
                    if on {
                        s += n;
                    }
                }
                s + &key_name(*vk)
            }
            Input::Midi { channel, note } => format!("MIDI note {} ({}), channel {channel}", note, note_name(*note)),
            Input::Pad { button } => format!("Pad {button}"),
            Input::Link { name } => format!("Link {name}"),
        }
    }
}

/// What a press does.
#[derive(Clone, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub enum Action {
    /// The expression goes on, or off when it was on.
    Toggle(String),
    /// The expression goes on and every other one off.
    Show(String),
    /// The expression is on while the key is held.
    Hold(String),
    /// Every expression off.
    Clear,
    /// OBS Studio goes to this scene.
    ObsScene(String),
    /// OBS starts or stops streaming.
    ObsStream,
    /// OBS starts or stops recording.
    ObsRecord,
    /// OBS's virtual camera goes on or off.
    ObsVirtualCam,
}

impl Action {
    pub fn label(&self) -> String {
        match self {
            Action::Toggle(n) => format!("Switch {n} on or off"),
            Action::Show(n) => format!("Show only {n}"),
            Action::Hold(n) => format!("{n} while held"),
            Action::Clear => "All expressions off".into(),
            Action::ObsScene(n) => format!("OBS scene {n}"),
            Action::ObsStream => "OBS start or stop streaming".into(),
            Action::ObsRecord => "OBS start or stop recording".into(),
            Action::ObsVirtualCam => "OBS virtual camera on or off".into(),
        }
    }
}

#[derive(Clone, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub struct Binding {
    pub input: Input,
    pub action: Action,
}

#[derive(Clone, PartialEq, Eq, Debug)]
pub struct InputEvent {
    pub input: Input,
    pub pressed: bool,
}

pub fn note_name(n: u8) -> String {
    const NAMES: [&str; 12] = ["C", "C#", "D", "D#", "E", "F", "F#", "G", "G#", "A", "A#", "B"];
    format!("{}{}", NAMES[(n % 12) as usize], (n as i32 / 12) - 1)
}

pub fn key_name(vk: u32) -> String {
    match vk {
        0x30..=0x39 | 0x41..=0x5A => ((vk as u8) as char).to_string(),
        0x60..=0x69 => format!("Num {}", vk - 0x60),
        0x70..=0x87 => format!("F{}", vk - 0x6F),
        0x20 => "Space".into(),
        0x0D => "Enter".into(),
        0x09 => "Tab".into(),
        0x08 => "Backspace".into(),
        0x25 => "Left".into(),
        0x26 => "Up".into(),
        0x27 => "Right".into(),
        0x28 => "Down".into(),
        0x2D => "Insert".into(),
        0x2E => "Delete".into(),
        0x24 => "Home".into(),
        0x23 => "End".into(),
        0x21 => "Page up".into(),
        0x22 => "Page down".into(),
        0x6A => "Num *".into(),
        0x6B => "Num +".into(),
        0x6D => "Num -".into(),
        0x6E => "Num .".into(),
        0x6F => "Num /".into(),
        0xBA => ";".into(),
        0xBB => "=".into(),
        0xBC => ",".into(),
        0xBD => "-".into(),
        0xBE => ".".into(),
        0xBF => "/".into(),
        0xC0 => "`".into(),
        0xDB => "[".into(),
        0xDC => "\\".into(),
        0xDD => "]".into(),
        0xDE => "'".into(),
        _ => format!("Key {vk}"),
    }
}

/// A MIDI control change (a knob or a slider of a controller) as a named number from 0 to 1.
pub fn parse_midi_cc(msg: &[u8]) -> Option<(String, f32)> {
    let (status, number, value) = (*msg.first()?, *msg.get(1)?, *msg.get(2)?);
    (status & 0xF0 == 0xB0).then(|| (format!("MIDI knob {number} ch{}", (status & 0x0F) + 1), value as f32 / 127.0))
}

/// The movement of a game pad stick or trigger, with the small drift of a stick at rest taken away (and the rest stretched to fill 1).
pub fn pad_axis(value: f32) -> f32 {
    const DEAD: f32 = 0.12;
    if value.abs() < DEAD { 0.0 } else { value.signum() * ((value.abs() - DEAD) / (1.0 - DEAD)).min(1.0) }
}

/// A MIDI message as an event: a note on with a force, or a note off.
pub fn parse_midi(msg: &[u8]) -> Option<InputEvent> {
    let (status, note, vel) = (*msg.first()?, *msg.get(1)?, msg.get(2).copied().unwrap_or(0));
    let channel = (status & 0x0F) + 1;
    match status & 0xF0 {
        0x90 => Some(InputEvent { input: Input::Midi { channel, note }, pressed: vel > 0 }),
        0x80 => Some(InputEvent { input: Input::Midi { channel, note }, pressed: false }),
        _ => None,
    }
}

/// What the bindings make of events: the action and whether it was a press (true) or a release (false).
pub fn dispatch(bindings: &[Binding], events: &[InputEvent]) -> Vec<(Action, bool)> {
    let mut out = Vec::new();
    for e in events {
        for b in bindings.iter().filter(|b| b.input == e.input) {
            match (&b.action, e.pressed) {
                (Action::Hold(_), _) => out.push((b.action.clone(), e.pressed)),
                (_, true) => out.push((b.action.clone(), true)),
                _ => {}
            }
        }
    }
    out
}

#[cfg(windows)]
fn key_down(vk: u32) -> bool {
    unsafe extern "system" {
        fn GetAsyncKeyState(v: i32) -> i16;
    }
    // SAFETY: a plain call into Windows with a number; it reads the state of one key.
    unsafe { (GetAsyncKeyState(vk as i32) as u16) & 0x8000 != 0 }
}

#[cfg(not(windows))]
fn key_down(_vk: u32) -> bool {
    false
}

fn modifiers() -> (bool, bool, bool) {
    (key_down(0x11), key_down(0x10), key_down(0x12))
}

/// Where the inputs come from: the keyboard (looked at on every call of `poll`), a MIDI port and the game pads (each read in its own
/// thread).
pub struct Hub {
    queue: Arc<Mutex<Vec<InputEvent>>>,
    midi: Option<midir::MidiInputConnection<()>>,
    pub midi_name: Option<String>,
    pub midi_ports: Vec<String>,
    pub pad_names: Arc<Mutex<Vec<String>>>,
    /// The sticks, triggers and knobs: name and where they are now (sticks from -1 to 1, triggers and knobs from 0 to 1).
    pub axes: Arc<Mutex<std::collections::BTreeMap<String, f32>>>,
    stop: Arc<AtomicBool>,
    held_keys: HashSet<usize>,
    pressed_scan: HashSet<u32>,
    /// Waiting for the next press to be taken as an input (to bind it).
    pub learn: bool,
    pub learned: Option<Input>,
    /// The web address other programs call (Seviath Link), while it is on.
    pub link: Option<crate::link::LinkServer>,
    /// Look at the keyboard (off in tests, where a key held by the person at the computer would get in).
    pub keyboard: bool,
}

impl Hub {
    pub fn new() -> Hub {
        let queue: Arc<Mutex<Vec<InputEvent>>> = Arc::new(Mutex::new(Vec::new()));
        let pad_names = Arc::new(Mutex::new(Vec::new()));
        let stop = Arc::new(AtomicBool::new(false));
        let axes: Arc<Mutex<std::collections::BTreeMap<String, f32>>> = Arc::new(Mutex::new(Default::default()));
        {
            let (q, names, stop, ax) = (queue.clone(), pad_names.clone(), stop.clone(), axes.clone());
            std::thread::spawn(move || {
                let Ok(mut g) = gilrs::Gilrs::new() else { return };
                let mut last_names = std::time::Instant::now() - std::time::Duration::from_secs(10);
                while !stop.load(Ordering::Relaxed) {
                    while let Some(ev) = g.next_event() {
                        let (button, pressed) = match ev.event {
                            gilrs::EventType::ButtonPressed(b, _) => (b, true),
                            gilrs::EventType::ButtonReleased(b, _) => (b, false),
                            gilrs::EventType::AxisChanged(a, v, _) => {
                                if let Ok(mut m) = ax.lock() {
                                    m.insert(format!("Pad {a:?}"), pad_axis(v));
                                }
                                continue;
                            }
                            // the analog triggers also report how far they are pressed
                            gilrs::EventType::ButtonChanged(b, v, _) if format!("{b:?}").contains("Trigger") => {
                                if let Ok(mut m) = ax.lock() {
                                    m.insert(format!("Pad {b:?}"), v.clamp(0.0, 1.0));
                                }
                                continue;
                            }
                            _ => continue,
                        };
                        if let Ok(mut q) = q.lock() {
                            if q.len() < 256 {
                                q.push(InputEvent { input: Input::Pad { button: format!("{button:?}") }, pressed });
                            }
                        }
                    }
                    if last_names.elapsed().as_secs() >= 2 {
                        last_names = std::time::Instant::now();
                        if let Ok(mut n) = names.lock() {
                            *n = g.gamepads().map(|(_, p)| p.name().to_string()).collect();
                        }
                    }
                    std::thread::sleep(std::time::Duration::from_millis(8));
                }
            });
        }
        let mut hub = Hub { queue, midi: None, midi_name: None, midi_ports: Vec::new(), pad_names, axes, stop, held_keys: HashSet::new(), pressed_scan: HashSet::new(), learn: false, learned: None, link: None, keyboard: true };
        hub.refresh_midi_ports();
        hub
    }

    pub fn refresh_midi_ports(&mut self) {
        self.midi_ports = midir::MidiInput::new("SeviathStudio list").map(|m| m.ports().iter().filter_map(|p| m.port_name(p).ok()).collect()).unwrap_or_default();
    }

    pub fn connect_midi(&mut self, name: &str) -> Result<(), String> {
        self.disconnect_midi();
        let input = midir::MidiInput::new("SeviathStudio").map_err(|e| e.to_string())?;
        let port = input.ports().into_iter().find(|p| input.port_name(p).is_ok_and(|n| n == name)).ok_or("That MIDI device is not there any more.")?;
        let q = self.queue.clone();
        let axes = self.axes.clone();
        let conn = input
            .connect(
                &port,
                "seviath-in",
                move |_stamp, msg, _| {
                    if let Some((name, v)) = parse_midi_cc(msg) {
                        if let Ok(mut m) = axes.lock() {
                            m.insert(name, v);
                        }
                    }
                    if let Some(e) = parse_midi(msg) {
                        if let Ok(mut q) = q.lock() {
                            if q.len() < 256 {
                                q.push(e);
                            }
                        }
                    }
                },
                (),
            )
            .map_err(|e| format!("Could not open the MIDI device: {e}"))?;
        self.midi = Some(conn);
        self.midi_name = Some(name.to_string());
        Ok(())
    }

    /// Switch Seviath Link on at a port (on this computer only) or off.
    pub fn set_link(&mut self, port: Option<u16>) -> Result<(), String> {
        self.link = None;
        if let Some(p) = port {
            self.link = Some(crate::link::LinkServer::start(p, self.queue.clone())?);
        }
        Ok(())
    }

    pub fn disconnect_midi(&mut self) {
        if let Some(c) = self.midi.take() {
            c.close();
        }
        self.midi_name = None;
    }

    /// Set a stick or knob as if a device moved it.
    pub fn set_axis(&self, name: &str, value: f32) {
        if let Ok(mut m) = self.axes.lock() {
            m.insert(name.to_string(), value);
        }
    }

    /// The sticks, triggers and knobs that have moved so far, with where they are.
    pub fn axis_values(&self) -> Vec<(String, f32)> {
        self.axes.lock().map(|m| m.iter().map(|(k, v)| (k.clone(), *v)).collect()).unwrap_or_default()
    }

    /// Put an event in as if it came from a device.
    pub fn push(&self, e: InputEvent) {
        if let Ok(mut q) = self.queue.lock() {
            q.push(e);
        }
    }

    /// Look at the keyboard: the keys of the bindings, or every key while waiting to learn one. Presses and releases go to the queue.
    fn scan_keyboard(&mut self, bindings: &[Binding]) {
        let (ctrl, shift, alt) = modifiers();
        for (i, b) in bindings.iter().enumerate() {
            let Input::Key { vk, ctrl: c, shift: s, alt: a } = &b.input else { continue };
            let down = key_down(*vk) && (*c == ctrl && *s == shift && *a == alt);
            let was = self.held_keys.contains(&i);
            if down && !was {
                self.held_keys.insert(i);
                self.push(InputEvent { input: b.input.clone(), pressed: true });
            } else if !down && was {
                self.held_keys.remove(&i);
                self.push(InputEvent { input: b.input.clone(), pressed: false });
            }
        }
        self.held_keys.retain(|i| *i < bindings.len());
        if self.learn {
            let mut now: HashSet<u32> = HashSet::new();
            for vk in (0x08u32..=0xFE).filter(|v| !matches!(v, 0x10..=0x12 | 0xA0..=0xA5 | 0x5B | 0x5C | 0x01..=0x06)) {
                if key_down(vk) {
                    now.insert(vk);
                    if !self.pressed_scan.contains(&vk) {
                        self.learned = Some(Input::Key { vk, ctrl, shift, alt });
                        self.learn = false;
                    }
                }
            }
            self.pressed_scan = now;
        } else {
            self.pressed_scan.clear();
        }
    }

    /// Everything that was pressed or let go since the last call. While learning, the first press is kept as `learned` instead.
    pub fn poll(&mut self, bindings: &[Binding]) -> Vec<InputEvent> {
        if self.keyboard {
            self.scan_keyboard(bindings);
        }
        let mut events: Vec<InputEvent> = self.queue.lock().map(|mut q| std::mem::take(&mut *q)).unwrap_or_default();
        if self.learn {
            if let Some(i) = events.iter().position(|e| e.pressed) {
                self.learned = Some(events[i].input.clone());
                self.learn = false;
            }
            events.clear();
        } else if self.learned.is_some() && !self.learn {
            // a learned input waits to be taken
        }
        events
    }
}

impl Drop for Hub {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        self.disconnect_midi();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key(vk: u32) -> Input {
        Input::Key { vk, ctrl: false, shift: false, alt: false }
    }

    #[test]
    fn inputs_are_named_for_people() {
        assert_eq!(key(0x41).label(), "A");
        assert_eq!(Input::Key { vk: 0x70, ctrl: true, shift: false, alt: true }.label(), "Ctrl+Alt+F1");
        assert_eq!(key(0x60).label(), "Num 0");
        assert_eq!(key(0x20).label(), "Space");
        assert_eq!(Input::Midi { channel: 1, note: 60 }.label(), "MIDI note 60 (C4), channel 1");
        assert_eq!(Input::Pad { button: "South".into() }.label(), "Pad South");
        assert_eq!(Input::Link { name: "cs2.kill".into() }.label(), "Link cs2.kill");
        assert_eq!(note_name(21), "A0");
        assert_eq!(Action::Hold("Blush".into()).label(), "Blush while held");
        assert_eq!(Action::ObsScene("Main".into()).label(), "OBS scene Main");
    }

    #[test]
    fn midi_messages_become_presses_and_releases() {
        assert_eq!(parse_midi(&[0x90, 60, 100]), Some(InputEvent { input: Input::Midi { channel: 1, note: 60 }, pressed: true }));
        assert_eq!(parse_midi(&[0x93, 61, 0]), Some(InputEvent { input: Input::Midi { channel: 4, note: 61 }, pressed: false }), "a note on with no force is a release");
        assert_eq!(parse_midi(&[0x80, 60, 64]), Some(InputEvent { input: Input::Midi { channel: 1, note: 60 }, pressed: false }));
        assert_eq!(parse_midi(&[0xB0, 7, 100]), None, "a control change is not a button");
        assert_eq!(parse_midi(&[]), None);
        assert_eq!(parse_midi(&[0x90]), None);
        assert_eq!(parse_midi_cc(&[0xB1, 7, 127]), Some(("MIDI knob 7 ch2".to_string(), 1.0)));
        assert_eq!(parse_midi_cc(&[0xB0, 1, 0]).unwrap().1, 0.0);
        assert_eq!(parse_midi_cc(&[0x90, 60, 100]), None, "a note is not a knob");
        // a stick at rest does not drift, and the rest of its way is stretched to 1
        assert_eq!(pad_axis(0.05), 0.0);
        assert_eq!(pad_axis(-0.1), 0.0);
        assert!((pad_axis(1.0) - 1.0).abs() < 1e-6 && (pad_axis(-1.0) + 1.0).abs() < 1e-6);
        assert!(pad_axis(0.5) > 0.4 && pad_axis(0.5) < 0.5);
    }

    #[test]
    fn bindings_turn_events_into_actions() {
        let bindings = vec![
            Binding { input: key(0x31), action: Action::Toggle("Happy".into()) },
            Binding { input: Input::Pad { button: "South".into() }, action: Action::Hold("Blush".into()) },
            Binding { input: Input::Midi { channel: 1, note: 60 }, action: Action::Clear },
        ];
        let ev = |input: Input, pressed| InputEvent { input, pressed };
        // a toggle acts on the press only
        assert_eq!(dispatch(&bindings, &[ev(key(0x31), true), ev(key(0x31), false)]), [(Action::Toggle("Happy".into()), true)]);
        // a hold acts on both
        let pad = Input::Pad { button: "South".into() };
        assert_eq!(dispatch(&bindings, &[ev(pad.clone(), true), ev(pad, false)]), [(Action::Hold("Blush".into()), true), (Action::Hold("Blush".into()), false)]);
        // what is not bound does nothing
        assert!(dispatch(&bindings, &[ev(key(0x32), true), ev(Input::Midi { channel: 2, note: 60 }, true)]).is_empty());
        assert_eq!(dispatch(&bindings, &[ev(Input::Midi { channel: 1, note: 60 }, true)]), [(Action::Clear, true)]);
        // two bindings on one input both act
        let two = vec![bindings[0].clone(), Binding { input: key(0x31), action: Action::Show("Sad".into()) }];
        assert_eq!(dispatch(&two, &[ev(key(0x31), true)]).len(), 2);
    }

    #[test]
    fn the_hub_hands_over_pushed_events_and_learns_the_next_press() {
        let mut hub = Hub::new();
        hub.keyboard = false;
        let e = InputEvent { input: Input::Midi { channel: 1, note: 64 }, pressed: true };
        hub.push(e.clone());
        assert_eq!(hub.poll(&[]), [e.clone()]);
        assert!(hub.poll(&[]).is_empty(), "taken once");
        // learning keeps the first press and hands nothing else on
        hub.learn = true;
        hub.push(InputEvent { input: Input::Midi { channel: 1, note: 70 }, pressed: false });
        hub.push(e.clone());
        hub.push(InputEvent { input: Input::Pad { button: "East".into() }, pressed: true });
        assert!(hub.poll(&[]).is_empty());
        assert_eq!(hub.learned, Some(e.input));
        assert!(!hub.learn);
        // a MIDI device that is not there is refused with a sentence
        assert!(hub.connect_midi("No such device").is_err());
    }
}
