// SPDX-License-Identifier: GPL-3.0-or-later
// Sending the face and the expressions to VRChat over OSC. VRChat listens on port 9000 of this computer for `/avatar/parameters/<name>`
// messages with one number (a float, or a bool for switches); an avatar that has parameters with those names follows them. The face
// inputs go out under their VTube Studio names (`FaceAngleX`, `EyeOpenLeft`, `MouthOpen`...) and every expression as `Expr_<name>`
// (0 to 1), plus `Tracked` (true while a face is followed). Nothing is read from VRChat.

use std::collections::HashMap;
use std::net::UdpSocket;

/// One OSC message with one float (or bool) argument.
pub fn osc_message(address: &str, value: Arg) -> Vec<u8> {
    fn put(out: &mut Vec<u8>, s: &str) {
        out.extend(s.as_bytes());
        out.push(0);
        while out.len() % 4 != 0 {
            out.push(0);
        }
    }
    let mut out = Vec::new();
    put(&mut out, address);
    match value {
        Arg::Float(f) => {
            put(&mut out, ",f");
            out.extend(f.to_be_bytes());
        }
        Arg::Bool(b) => put(&mut out, if b { ",T" } else { ",F" }),
    }
    out
}

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Arg {
    Float(f32),
    Bool(bool),
}

/// A name VRChat accepts in an address: letters, numbers and underscore.
pub fn param_name(name: &str) -> String {
    let s: String = name.chars().map(|c| if c.is_ascii_alphanumeric() || c == '_' { c } else { '_' }).collect();
    if s.is_empty() { "_".into() } else { s }
}

pub struct VrChat {
    socket: UdpSocket,
    target: String,
    /// What was sent last, so only what changed goes out again (a number that did not move is not sent).
    last: HashMap<String, f32>,
    pub sent: u64,
}

impl VrChat {
    pub fn new(host: &str, port: u16) -> Result<VrChat, String> {
        let socket = UdpSocket::bind("0.0.0.0:0").map_err(|e| format!("Could not open a socket: {e}"))?;
        Ok(VrChat { socket, target: format!("{host}:{port}"), last: HashMap::new(), sent: 0 })
    }

    /// Send the numbers that changed since the last time (by more than a hundredth), and `Tracked`.
    pub fn send_frame(&mut self, inputs: &[(&str, f32)], expressions: &[(String, f32)], tracked: bool) {
        let mut messages: Vec<Vec<u8>> = Vec::new();
        let mut put = |last: &mut HashMap<String, f32>, name: String, v: f32| {
            let changed = last.get(&name).is_none_or(|old| (old - v).abs() > 0.01);
            if changed {
                last.insert(name.clone(), v);
                messages.push(osc_message(&format!("/avatar/parameters/{name}"), Arg::Float(v)));
            }
        };
        for (n, v) in inputs {
            put(&mut self.last, param_name(n), *v);
        }
        for (n, v) in expressions {
            put(&mut self.last, format!("Expr_{}", param_name(n)), *v);
        }
        let t = if tracked { 1.0 } else { 0.0 };
        if self.last.get("Tracked").is_none_or(|old| *old != t) {
            self.last.insert("Tracked".into(), t);
            messages.push(osc_message("/avatar/parameters/Tracked", Arg::Bool(tracked)));
        }
        for m in messages {
            if self.socket.send_to(&m, &self.target).is_ok() {
                self.sent += 1;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::track::net::{Arg as In, parse_osc};

    #[test]
    fn messages_are_written_the_way_osc_wants_them() {
        let m = osc_message("/avatar/parameters/MouthOpen", Arg::Float(0.5));
        assert_eq!(m.len() % 4, 0);
        assert_eq!(parse_osc(&m), [("/avatar/parameters/MouthOpen".to_string(), vec![In::Float(0.5)])]);
        let t = osc_message("/avatar/parameters/Tracked", Arg::Bool(true));
        assert!(t.windows(2).any(|w| w == b",T"));
        assert_eq!(param_name("Big smile!"), "Big_smile_");
        assert_eq!(param_name(""), "_");
    }

    #[test]
    fn the_face_and_expressions_go_out_and_only_what_changed_goes_again() {
        let rx = UdpSocket::bind("127.0.0.1:0").unwrap();
        rx.set_read_timeout(Some(std::time::Duration::from_millis(500))).unwrap();
        let port = rx.local_addr().unwrap().port();
        let mut v = VrChat::new("127.0.0.1", port).unwrap();
        v.send_frame(&[("MouthOpen", 0.8), ("FaceAngleX", -12.0)], &[("Happy".to_string(), 1.0)], true);
        let mut got: Vec<(String, f32)> = Vec::new();
        let mut buf = [0u8; 512];
        while let Ok(n) = rx.recv(&mut buf) {
            for (addr, args) in parse_osc(&buf[..n]) {
                if let Some(In::Float(f)) = args.first() {
                    got.push((addr, *f));
                } else {
                    got.push((addr, f32::NAN));
                }
            }
            if got.len() >= 4 {
                break;
            }
        }
        got.sort_by(|a, b| a.0.cmp(&b.0));
        let names: Vec<&str> = got.iter().map(|g| g.0.as_str()).collect();
        assert_eq!(names, ["/avatar/parameters/Expr_Happy", "/avatar/parameters/FaceAngleX", "/avatar/parameters/MouthOpen", "/avatar/parameters/Tracked"]);
        assert_eq!(got[2].1, 0.8);
        assert_eq!(v.sent, 4);
        // the same again: nothing; one number moved: only that one
        v.send_frame(&[("MouthOpen", 0.8), ("FaceAngleX", -12.0)], &[("Happy".to_string(), 1.0)], true);
        assert_eq!(v.sent, 4);
        v.send_frame(&[("MouthOpen", 0.2), ("FaceAngleX", -12.0)], &[("Happy".to_string(), 1.0)], true);
        assert_eq!(v.sent, 5);
    }
}
