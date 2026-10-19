// SPDX-License-Identifier: GPL-3.0-or-later
// Tracking over the network: what a program or a phone says about the face of the person, read from UDP packets.
//   VMC          OSC messages (the protocol of VMC and Waidayo and many others): blend shapes by name and the head bone.
//   iFacialMocap text packets from the phone app: blend shapes by name, head and eye turns.
//   OpenSeeFace  the binary packets of OpenSeeFace's facetracker: head turn, eye blink, features of the mouth and brows.
// A packet becomes a `Frame`: the head turn in degrees and the blend shapes by their ARKit names.

use std::collections::HashMap;
use std::net::UdpSocket;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// One reading of the face.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Frame {
    /// Head turn in degrees: yaw (left and right), pitch (up and down), roll (tilt).
    pub head: Option<[f32; 3]>,
    /// Head place in centimetres (x right, y up, z towards the camera).
    pub pos: Option<[f32; 3]>,
    /// Blend shapes 0 to 1 by their ARKit names with a small first letter ("eyeBlinkLeft", "jawOpen"...); a model's own names are kept as sent.
    pub blend: HashMap<String, f32>,
    /// Where the eyes look, in degrees: (left eye yaw, left eye pitch, right eye yaw, right eye pitch).
    pub eyes: Option<[f32; 4]>,
    /// The body and the hands from the camera program (`/Seviath/Body name value`): arm angles in degrees, the tilt of the shoulders,
    /// how open each hand and finger is (0 to 1).
    pub body: HashMap<String, f32>,
}

impl Frame {
    pub fn get(&self, name: &str) -> Option<f32> {
        self.blend.get(name).copied()
    }
}

/// The first letter small: "EyeBlinkLeft" and "eyeBlinkLeft" are the same shape.
pub fn norm_name(s: &str) -> String {
    let mut c = s.chars();
    match c.next() {
        Some(f) => f.to_lowercase().collect::<String>() + c.as_str(),
        None => String::new(),
    }
}

/// A quaternion (x, y, z, w) as the turn of a head in degrees (yaw, pitch, roll).
pub fn quat_to_euler(q: [f32; 4]) -> [f32; 3] {
    let (x, y, z, w) = (q[0], q[1], q[2], q[3]);
    let sinr = 2.0 * (w * x + y * z);
    let cosr = 1.0 - 2.0 * (x * x + y * y);
    let pitch = sinr.atan2(cosr);
    let sinp = 2.0 * (w * y - z * x);
    let yaw = if sinp.abs() >= 1.0 { std::f32::consts::FRAC_PI_2.copysign(sinp) } else { sinp.asin() };
    let siny = 2.0 * (w * z + x * y);
    let cosy = 1.0 - 2.0 * (y * y + z * z);
    let roll = siny.atan2(cosy);
    // the head faces +Z in a VMC model: turning it about the up axis (y) is the yaw, about x the pitch, about z the roll
    [yaw.to_degrees(), pitch.to_degrees(), roll.to_degrees()]
}

// ---- OSC (for VMC) ----

#[derive(Clone, Debug, PartialEq)]
pub enum Arg {
    Int(i32),
    Float(f32),
    Str(String),
}

fn pad4(n: usize) -> usize {
    n.div_ceil(4) * 4
}

fn osc_str(d: &[u8], at: usize) -> Option<(String, usize)> {
    let end = d.get(at..)?.iter().position(|b| *b == 0)? + at;
    Some((String::from_utf8_lossy(&d[at..end]).to_string(), pad4(end + 1 - at) + at))
}

/// The messages in an OSC packet (a message, or a bundle of them): (address, arguments).
pub fn parse_osc(d: &[u8]) -> Vec<(String, Vec<Arg>)> {
    let mut out = Vec::new();
    parse_osc_into(d, &mut out, 0);
    out
}

fn parse_osc_into(d: &[u8], out: &mut Vec<(String, Vec<Arg>)>, depth: usize) {
    if depth > 4 || d.len() < 4 {
        return;
    }
    if d.starts_with(b"#bundle\0") {
        let mut at = 16;
        while at + 4 <= d.len() {
            let n = u32::from_be_bytes([d[at], d[at + 1], d[at + 2], d[at + 3]]) as usize;
            at += 4;
            if n == 0 || at + n > d.len() {
                break;
            }
            parse_osc_into(&d[at..at + n], out, depth + 1);
            at += n;
        }
        return;
    }
    let Some((addr, at)) = osc_str(d, 0) else { return };
    if !addr.starts_with('/') {
        return;
    }
    let Some((tags, mut at)) = osc_str(d, at) else { return };
    let mut args = Vec::new();
    for t in tags.trim_start_matches(',').chars() {
        match t {
            'i' => {
                let Some(b) = d.get(at..at + 4) else { return };
                args.push(Arg::Int(i32::from_be_bytes([b[0], b[1], b[2], b[3]])));
                at += 4;
            }
            'f' => {
                let Some(b) = d.get(at..at + 4) else { return };
                args.push(Arg::Float(f32::from_be_bytes([b[0], b[1], b[2], b[3]])));
                at += 4;
            }
            's' => {
                let Some((s, next)) = osc_str(d, at) else { return };
                args.push(Arg::Str(s));
                at = next;
            }
            'T' | 'F' | 'N' => {}
            _ => return,
        }
    }
    out.push((addr, args));
}

fn arg_f(a: &Arg) -> Option<f32> {
    match a {
        Arg::Float(f) => Some(*f),
        Arg::Int(i) => Some(*i as f32),
        Arg::Str(_) => None,
    }
}

/// The face in a VMC packet: the blend shapes sent (`/VMC/Ext/Blend/Val name value`) and the head bone (`/VMC/Ext/Bone/Pos Head ...`).
pub fn parse_vmc(d: &[u8]) -> Option<Frame> {
    let mut f = Frame::default();
    let mut any = false;
    for (addr, args) in parse_osc(d) {
        match addr.as_str() {
            "/VMC/Ext/Blend/Val" => {
                if let (Some(Arg::Str(n)), Some(v)) = (args.first(), args.get(1).and_then(arg_f)) {
                    f.blend.insert(norm_name(n), v.clamp(0.0, 1.0));
                    any = true;
                }
            }
            "/VMC/Ext/Bone/Pos" => {
                if let (Some(Arg::Str(n)), true) = (args.first(), args.len() >= 8) {
                    let v: Vec<f32> = args[1..8].iter().filter_map(arg_f).collect();
                    if v.len() == 7 {
                        match n.as_str() {
                            "Head" => {
                                f.head = Some(quat_to_euler([v[3], v[4], v[5], v[6]]));
                                any = true;
                            }
                            "LeftEye" | "RightEye" => {
                                let e = quat_to_euler([v[3], v[4], v[5], v[6]]);
                                let cur = f.eyes.unwrap_or([0.0; 4]);
                                f.eyes = Some(if n == "LeftEye" { [e[0], e[1], cur[2], cur[3]] } else { [cur[0], cur[1], e[0], e[1]] });
                                any = true;
                            }
                            _ => {}
                        }
                    }
                }
            }
            // the camera program of this app sends the turn of the head in degrees and its place in centimetres
            "/Seviath/Head" if args.len() >= 3 => {
                let v: Vec<f32> = args.iter().filter_map(arg_f).collect();
                if v.len() >= 3 {
                    f.head = Some([v[0], v[1], v[2]]);
                    any = true;
                }
            }
            "/Seviath/Body" => {
                if let (Some(Arg::Str(n)), Some(v)) = (args.first(), args.get(1).and_then(arg_f)) {
                    f.body.insert(n.clone(), v);
                    any = true;
                }
            }
            "/Seviath/Pos" if args.len() >= 3 => {
                let v: Vec<f32> = args.iter().filter_map(arg_f).collect();
                if v.len() >= 3 {
                    f.pos = Some([v[0], v[1], v[2]]);
                    any = true;
                }
            }
            "/VMC/Ext/Root/Pos" if args.len() >= 4 => {
                let v: Vec<f32> = args[1..4].iter().filter_map(arg_f).collect();
                if v.len() == 3 {
                    f.pos = Some([v[0] * 100.0, v[1] * 100.0, v[2] * 100.0]);
                    any = true;
                }
            }
            _ => {}
        }
    }
    any.then_some(f)
}

// ---- iFacialMocap ----

/// The packet of the phone app: `name&value|name&value|=head#rx,ry,rz,tx,ty,tz|rightEye#x,y,z|leftEye#x,y,z`.
pub fn parse_ifacial(text: &str) -> Option<Frame> {
    let mut f = Frame::default();
    let mut any = false;
    for part in text.split('|') {
        let part = part.trim();
        if let Some((n, v)) = part.split_once('&') {
            if let Ok(v) = v.trim().parse::<f32>() {
                // the app sends values from 0 to 100
                f.blend.insert(norm_name(&n.replace('_', "")), (if v > 1.0 { v / 100.0 } else { v }).clamp(0.0, 1.0));
                any = true;
            }
        } else if let Some(rest) = part.strip_prefix("=head#") {
            let v: Vec<f32> = rest.split(',').filter_map(|x| x.trim().parse().ok()).collect();
            if v.len() >= 3 {
                // rx pitch, ry yaw, rz roll, in degrees
                f.head = Some([v[1], v[0], v[2]]);
                if v.len() >= 6 {
                    f.pos = Some([v[3], v[4], v[5]]);
                }
                any = true;
            }
        } else if let Some((name, rest)) = part.split_once('#') {
            let v: Vec<f32> = rest.split(',').filter_map(|x| x.trim().parse().ok()).collect();
            if v.len() >= 2 && (name == "rightEye" || name == "leftEye") {
                let cur = f.eyes.unwrap_or([0.0; 4]);
                f.eyes = Some(if name == "leftEye" { [v[1], v[0], cur[2], cur[3]] } else { [cur[0], cur[1], v[1], v[0]] });
                any = true;
            }
        }
    }
    // the phone names its shapes with an underscore for the side: mouthSmile_L; ARKit says mouthSmileLeft
    let renames: Vec<(String, f32)> = f
        .blend
        .iter()
        .filter_map(|(k, v)| {
            let k2 = k.strip_suffix('L').map(|b| format!("{b}Left")).or_else(|| k.strip_suffix('R').map(|b| format!("{b}Right")))?;
            Some((k2, *v))
        })
        .collect();
    for (k, v) in renames {
        f.blend.insert(k, v);
    }
    any.then_some(f)
}

// ---- OpenSeeFace ----

/// The packet of OpenSeeFace's facetracker (one face): the head turn in degrees, how open the eyes are, and the mouth.
pub fn parse_osf(d: &[u8]) -> Option<Frame> {
    // 8 time, 4 id, 4 width, 4 height, 4 right eye, 4 left eye, 1 success, 4 pnp error, 16 quaternion, 12 euler, 12 translation,
    // 66 confidences, 66*2 landmarks, 70*3 3d points, 14 features
    let fl = |at: usize| d.get(at..at + 4).map(|b| f32::from_le_bytes([b[0], b[1], b[2], b[3]]));
    if d.len() < 8 + 4 + 8 + 8 + 1 + 4 + 16 + 12 + 12 {
        return None;
    }
    if d[8 + 4 + 8 + 8] == 0 {
        return None;
    }
    let mut at = 8 + 4 + 8;
    let (right_eye, left_eye) = (fl(at)?, fl(at + 4)?);
    at += 8 + 1 + 4;
    at += 16;
    let euler = [fl(at)?, fl(at + 4)?, fl(at + 8)?];
    at += 12;
    let trans = [fl(at)?, fl(at + 4)?, fl(at + 8)?];
    at += 12;
    let mut f = Frame { head: Some([euler[1], euler[0], euler[2]]), pos: Some(trans), ..Default::default() };
    f.blend.insert("eyeBlinkRight".into(), (1.0 - right_eye).clamp(0.0, 1.0));
    f.blend.insert("eyeBlinkLeft".into(), (1.0 - left_eye).clamp(0.0, 1.0));
    // the features come after the confidences, landmarks and 3d points
    let features_at = at + 66 * 4 + 66 * 2 * 4 + 70 * 3 * 4;
    if let (Some(open), Some(wide)) = (fl(features_at + 12 * 4), fl(features_at + 13 * 4)) {
        f.blend.insert("jawOpen".into(), open.clamp(0.0, 1.0));
        f.blend.insert("mouthSmileLeft".into(), (wide.max(0.0) * 0.5).clamp(0.0, 1.0));
        f.blend.insert("mouthSmileRight".into(), (wide.max(0.0) * 0.5).clamp(0.0, 1.0));
    }
    Some(f)
}

// ---- receiving ----

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Protocol {
    Vmc,
    IFacial,
    OpenSeeFace,
}

impl Protocol {
    pub fn label(self) -> &'static str {
        match self {
            Protocol::Vmc => "VMC (Waidayo, VSeeFace, and others)",
            Protocol::IFacial => "iFacialMocap",
            Protocol::OpenSeeFace => "OpenSeeFace",
        }
    }

    pub fn default_port(self) -> u16 {
        match self {
            Protocol::Vmc => 39539,
            Protocol::IFacial => 49983,
            Protocol::OpenSeeFace => 11573,
        }
    }

    pub fn parse(self, d: &[u8]) -> Option<Frame> {
        match self {
            Protocol::Vmc => parse_vmc(d),
            Protocol::IFacial => parse_ifacial(&String::from_utf8_lossy(d)),
            Protocol::OpenSeeFace => parse_osf(d),
        }
    }
}

/// What a receiver has heard.
#[derive(Default)]
pub struct Heard {
    pub last: Option<Frame>,
    pub at: Option<Instant>,
    pub packets: u64,
    /// Packets a second, over the last second or so.
    pub rate: f32,
    window: Vec<Instant>,
}

impl Heard {
    pub fn fresh(&self, within: Duration) -> bool {
        self.at.is_some_and(|t| t.elapsed() < within)
    }
}

/// Listens on a UDP port in a thread of its own until it is dropped.
pub struct Receiver {
    pub protocol: Protocol,
    pub port: u16,
    pub heard: Arc<Mutex<Heard>>,
    stop: Arc<AtomicBool>,
    thread: Option<std::thread::JoinHandle<()>>,
}

impl Receiver {
    pub fn start(protocol: Protocol, port: u16) -> Result<Receiver, String> {
        let socket = UdpSocket::bind(("0.0.0.0", port)).map_err(|e| format!("Could not listen on port {port}: {e}"))?;
        socket.set_read_timeout(Some(Duration::from_millis(150))).map_err(|e| e.to_string())?;
        let heard = Arc::new(Mutex::new(Heard::default()));
        let stop = Arc::new(AtomicBool::new(false));
        let (h2, s2) = (heard.clone(), stop.clone());
        let thread = std::thread::spawn(move || {
            let mut buf = vec![0u8; 65536];
            while !s2.load(Ordering::Relaxed) {
                let n = match socket.recv_from(&mut buf) {
                    Ok((n, _)) => n,
                    Err(_) => continue,
                };
                if let Some(frame) = protocol.parse(&buf[..n]) {
                    let mut h = h2.lock().unwrap();
                    // a VMC sender sends the blend shapes and the bones in separate packets: keep what the last packets said
                    let merged = match (&h.last, protocol) {
                        (Some(old), Protocol::Vmc) => {
                            let mut m = old.clone();
                            m.blend.extend(frame.blend);
                            m.head = frame.head.or(m.head);
                            m.pos = frame.pos.or(m.pos);
                            m.eyes = frame.eyes.or(m.eyes);
                            m
                        }
                        _ => frame,
                    };
                    h.last = Some(merged);
                    let now = Instant::now();
                    h.at = Some(now);
                    h.packets += 1;
                    h.window.push(now);
                    h.window.retain(|t| now.duration_since(*t) < Duration::from_secs(1));
                    h.rate = h.window.len() as f32;
                }
            }
        });
        Ok(Receiver { protocol, port, heard, stop, thread: Some(thread) })
    }

    /// The newest reading if it is not older than `within`.
    pub fn latest(&self, within: Duration) -> Option<Frame> {
        let h = self.heard.lock().unwrap();
        if h.fresh(within) { h.last.clone() } else { None }
    }
}

impl Drop for Receiver {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    pub fn osc_message(addr: &str, args: &[Arg]) -> Vec<u8> {
        let mut out = Vec::new();
        let mut put = |s: &str, out: &mut Vec<u8>| {
            out.extend(s.as_bytes());
            out.push(0);
            while out.len() % 4 != 0 {
                out.push(0);
            }
        };
        put(addr, &mut out);
        let tags: String = std::iter::once(',').chain(args.iter().map(|a| match a { Arg::Int(_) => 'i', Arg::Float(_) => 'f', Arg::Str(_) => 's' })).collect();
        put(&tags, &mut out);
        for a in args {
            match a {
                Arg::Int(i) => out.extend(i.to_be_bytes()),
                Arg::Float(f) => out.extend(f.to_be_bytes()),
                Arg::Str(s) => put(s, &mut out),
            }
        }
        out
    }

    pub fn osc_bundle(msgs: &[Vec<u8>]) -> Vec<u8> {
        let mut out = b"#bundle\0".to_vec();
        out.extend([0u8; 8]);
        for m in msgs {
            out.extend((m.len() as u32).to_be_bytes());
            out.extend(m);
        }
        out
    }

    #[test]
    fn a_vmc_bundle_gives_blend_shapes_and_the_turn_of_the_head() {
        let q = [0.0f32, (15.0f32.to_radians() / 2.0).sin(), 0.0, (15.0f32.to_radians() / 2.0).cos()];
        let pkt = osc_bundle(&[
            osc_message("/VMC/Ext/Blend/Val", &[Arg::Str("EyeBlinkLeft".into()), Arg::Float(0.75)]),
            osc_message("/VMC/Ext/Blend/Val", &[Arg::Str("jawOpen".into()), Arg::Float(0.4)]),
            osc_message("/VMC/Ext/Blend/Apply", &[]),
            osc_message("/VMC/Ext/Bone/Pos", &[Arg::Str("Head".into()), Arg::Float(0.0), Arg::Float(1.5), Arg::Float(0.0), Arg::Float(q[0]), Arg::Float(q[1]), Arg::Float(q[2]), Arg::Float(q[3])]),
        ]);
        let f = parse_vmc(&pkt).unwrap();
        assert_eq!(f.get("eyeBlinkLeft"), Some(0.75), "the name is the ARKit one with a small first letter");
        assert_eq!(f.get("jawOpen"), Some(0.4));
        let h = f.head.unwrap();
        assert!((h[0] - 15.0).abs() < 0.1 && h[1].abs() < 0.1 && h[2].abs() < 0.1, "yaw 15: {h:?}");
        // nothing useful: no frame
        assert!(parse_vmc(&osc_message("/VMC/Ext/OK", &[Arg::Int(1)])).is_none());
        assert!(parse_vmc(b"garbage").is_none());
        assert!(parse_vmc(&[]).is_none());
        // a message with too few bytes for its arguments does not panic
        let mut cut = osc_message("/VMC/Ext/Blend/Val", &[Arg::Str("A".into()), Arg::Float(1.0)]);
        cut.truncate(cut.len() - 2);
        let _ = parse_vmc(&cut);
    }

    #[test]
    fn the_body_numbers_of_the_camera_program_are_read_as_inputs() {
        let mut d = osc_message("/Seviath/Body", &[Arg::Str("ArmLeftAngle".into()), Arg::Float(90.0)]);
        d.extend(osc_message("/Seviath/Body", &[Arg::Str("HandRightOpen".into()), Arg::Float(0.25)]));
        // one message at a time (a bundle is made of them)
        let f = parse_vmc(&osc_message("/Seviath/Body", &[Arg::Str("ArmLeftAngle".into()), Arg::Float(90.0)])).unwrap();
        assert_eq!(f.body.get("ArmLeftAngle"), Some(&90.0));
        let ch = crate::track::channels(&f);
        assert_eq!(ch.get("ArmLeftAngle"), Some(&90.0));
    }

    #[test]
    fn a_packet_made_by_the_camera_program_of_the_app_is_read() {
        // made by tools/ai/track.py: two blend shapes and the head turn
        let hex = "2362756e646c65000000000000000000000000242f564d432f4578742f426c656e642f56616c00002c7366006a61774f70656e003f0000000000002c2f564d432f4578742f426c656e642f56616c00002c736600657965426c696e6b4c656674000000003e800000000000242f536576696174682f486561640000002c6666660000000041200000c0a0000040000000";
        let bytes: Vec<u8> = (0..hex.len()).step_by(2).map(|i| u8::from_str_radix(&hex[i..i + 2], 16).unwrap()).collect();
        let f = parse_vmc(&bytes).unwrap();
        assert_eq!(f.get("jawOpen"), Some(0.5));
        assert_eq!(f.get("eyeBlinkLeft"), Some(0.25));
        assert_eq!(f.head, Some([10.0, -5.0, 2.0]));
    }

    #[test]
    fn quaternions_turn_into_degrees_about_each_axis() {
        let q = |axis: usize, deg: f32| { let mut v = [0.0f32, 0.0, 0.0, (deg.to_radians() / 2.0).cos()]; v[axis] = (deg.to_radians() / 2.0).sin(); v };
        let (yaw, pitch, roll) = (quat_to_euler(q(1, 20.0)), quat_to_euler(q(0, -10.0)), quat_to_euler(q(2, 5.0)));
        assert!((yaw[0] - 20.0).abs() < 0.1 && (pitch[1] + 10.0).abs() < 0.1 && (roll[2] - 5.0).abs() < 0.1, "{yaw:?} {pitch:?} {roll:?}");
        assert_eq!(quat_to_euler([0.0, 0.0, 0.0, 1.0]), [0.0, 0.0, 0.0]);
    }

    #[test]
    fn the_text_of_ifacialmocap_gives_shapes_head_and_eyes() {
        let t = "mouthSmile_L&60|mouthSmile_R&20|jawOpen&35|eyeBlink_L&100|=head#-5.5,12.0,3.0,1.0,2.0,-3.0|rightEye#4.0,-2.0,0.0|leftEye#6.0,-1.0,0.0";
        let f = parse_ifacial(t).unwrap();
        assert!((f.get("mouthSmileLeft").unwrap() - 0.6).abs() < 1e-5, "{:?}", f.blend);
        assert!((f.get("mouthSmileRight").unwrap() - 0.2).abs() < 1e-5);
        assert_eq!(f.get("eyeBlinkLeft"), Some(1.0));
        assert!((f.get("jawOpen").unwrap() - 0.35).abs() < 1e-5);
        assert_eq!(f.head, Some([12.0, -5.5, 3.0]));
        assert_eq!(f.pos, Some([1.0, 2.0, -3.0]));
        assert_eq!(f.eyes, Some([-1.0, 6.0, -2.0, 4.0]));
        assert!(parse_ifacial("nothing here").is_none());
    }

    pub fn osf_packet(euler: [f32; 3], right_eye: f32, left_eye: f32, mouth_open: f32, mouth_wide: f32, success: bool) -> Vec<u8> {
        let mut d = Vec::new();
        d.extend(1.5f64.to_le_bytes());
        d.extend(0i32.to_le_bytes());
        d.extend(640f32.to_le_bytes());
        d.extend(480f32.to_le_bytes());
        d.extend(right_eye.to_le_bytes());
        d.extend(left_eye.to_le_bytes());
        d.push(success as u8);
        d.extend(0f32.to_le_bytes());
        for q in [0.0f32, 0.0, 0.0, 1.0] {
            d.extend(q.to_le_bytes());
        }
        for e in euler {
            d.extend(e.to_le_bytes());
        }
        for t in [1.0f32, 2.0, 3.0] {
            d.extend(t.to_le_bytes());
        }
        d.extend(vec![0u8; 66 * 4 + 66 * 2 * 4 + 70 * 3 * 4]);
        for i in 0..14 {
            let v = match i { 12 => mouth_open, 13 => mouth_wide, _ => 0.0 };
            d.extend(v.to_le_bytes());
        }
        d
    }

    #[test]
    fn an_openseeface_packet_gives_the_head_the_eyes_and_the_mouth() {
        let f = parse_osf(&osf_packet([10.0, -20.0, 5.0], 0.25, 1.0, 0.6, 0.4, true)).unwrap();
        // the packet says pitch, yaw, roll: the frame says yaw, pitch, roll
        assert_eq!(f.head, Some([-20.0, 10.0, 5.0]));
        assert_eq!(f.get("eyeBlinkRight"), Some(0.75));
        assert_eq!(f.get("eyeBlinkLeft"), Some(0.0));
        assert!((f.get("jawOpen").unwrap() - 0.6).abs() < 1e-5);
        assert!(f.get("mouthSmileLeft").unwrap() > 0.1);
        // no face found, or a cut packet: nothing
        assert!(parse_osf(&osf_packet([0.0; 3], 1.0, 1.0, 0.0, 0.0, false)).is_none());
        assert!(parse_osf(&[0u8; 20]).is_none());
        assert!(norm_name("EyeBlinkLeft") == "eyeBlinkLeft" && norm_name("") == "");
    }

    #[test]
    fn a_receiver_listens_on_a_port_and_keeps_the_newest_face() {
        let rx = Receiver::start(Protocol::Vmc, 0).err();
        // port 0 asks for any free port, which the receiver does not report: use a fixed one in the private range
        drop(rx);
        let port = 48_731;
        let rx = match Receiver::start(Protocol::Vmc, port) {
            Ok(r) => r,
            Err(_) => return,
        };
        assert!(rx.latest(Duration::from_secs(1)).is_none(), "nothing heard yet");
        let sock = UdpSocket::bind("127.0.0.1:0").unwrap();
        let blend = osc_message("/VMC/Ext/Blend/Val", &[Arg::Str("jawOpen".into()), Arg::Float(0.5)]);
        let bone = osc_message("/VMC/Ext/Bone/Pos", &[Arg::Str("Head".into()), Arg::Float(0.0), Arg::Float(0.0), Arg::Float(0.0), Arg::Float(0.0), Arg::Float(0.0), Arg::Float(0.0), Arg::Float(1.0)]);
        sock.send_to(&blend, ("127.0.0.1", port)).unwrap();
        sock.send_to(&bone, ("127.0.0.1", port)).unwrap();
        let mut got = None;
        for _ in 0..40 {
            std::thread::sleep(Duration::from_millis(25));
            if let Some(f) = rx.latest(Duration::from_secs(1)) {
                if f.head.is_some() && f.get("jawOpen").is_some() {
                    got = Some(f);
                    break;
                }
            }
        }
        let f = got.expect("the packets arrived and were put together");
        assert_eq!(f.get("jawOpen"), Some(0.5));
        assert!(rx.heard.lock().unwrap().packets >= 2);
        // old readings are not given
        assert!(rx.latest(Duration::from_nanos(1)).is_none());
    }
}
