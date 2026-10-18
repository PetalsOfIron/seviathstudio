// SPDX-License-Identifier: GPL-3.0-or-later
// Seviath Link: a small web address on this computer that other programs can call to make something happen here. A Stream Deck button,
// a Touch Portal button or any program that can open a web address (or an OBS hotkey script) calls `/trigger/<name>`; a game can send
// its state: Counter-Strike 2 posts what is going on to `/gsi` and Link turns changes into events (`cs2.kill`, `cs2.death`,
// `cs2.round_live`, `cs2.bomb_planted`). Every call becomes an input named `Link: <name>` that a hotkey binding can use, like a key or a
// MIDI note. Only this computer can call it (localhost), and it only makes inputs: it cannot read or change anything else.

use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use serde_json::Value;

use crate::hotkeys::{Input, InputEvent};

const MAX_BODY: usize = 256 * 1024;

const HELP: &str = "SeviathStudio Link\n\nGET or POST /trigger/<name>   an input named Link: <name> is pressed and let go\nGET or POST /press/<name>     pressed\nGET or POST /release/<name>   let go\nPOST /gsi                     Counter-Strike 2 game state (events cs2.kill, cs2.death, cs2.round_<phase>, cs2.bomb_<state>)\n";

/// What CS2 told us last, to see what changed.
#[derive(Default, Clone, PartialEq, Debug)]
pub struct Cs2 {
    health: Option<i64>,
    kills: Option<i64>,
    phase: Option<String>,
    bomb: Option<String>,
}

/// The events a new state of CS2 makes, and the state to compare the next one with.
pub fn cs2_events(prev: &Cs2, body: &Value) -> (Vec<String>, Cs2) {
    let now = Cs2 {
        health: body["player"]["state"]["health"].as_i64(),
        kills: body["player"]["match_stats"]["kills"].as_i64(),
        phase: body["round"]["phase"].as_str().map(str::to_string),
        bomb: body["round"]["bomb"].as_str().map(str::to_string),
    };
    let mut out = Vec::new();
    if let (Some(a), Some(b)) = (prev.kills, now.kills) {
        for _ in 0..(b - a).clamp(0, 5) {
            out.push("cs2.kill".to_string());
        }
    }
    if let (Some(a), Some(b)) = (prev.health, now.health) {
        if a > 0 && b <= 0 {
            out.push("cs2.death".into());
        }
    }
    if now.phase != prev.phase {
        if let Some(p) = &now.phase {
            out.push(format!("cs2.round_{p}"));
        }
    }
    if now.bomb != prev.bomb {
        if let Some(b) = &now.bomb {
            out.push(format!("cs2.bomb_{b}"));
        }
    }
    (out, now)
}

/// The text for the file Counter-Strike 2 reads to know where to send its state.
pub fn cs2_config(port: u16) -> String {
    format!("\"SeviathStudio\"\n{{\n \"uri\" \"http://127.0.0.1:{port}/gsi\"\n \"timeout\" \"5.0\"\n \"buffer\" \"0.1\"\n \"throttle\" \"0.1\"\n \"heartbeat\" \"10.0\"\n \"data\"\n {{\n  \"provider\" \"1\"\n  \"map\" \"1\"\n  \"round\" \"1\"\n  \"player_id\" \"1\"\n  \"player_state\" \"1\"\n  \"player_match_stats\" \"1\"\n }}\n}}\n")
}

pub struct LinkServer {
    pub port: u16,
    stop: Arc<AtomicBool>,
    thread: Option<std::thread::JoinHandle<()>>,
    /// The name of the last thing that was called (for the panel).
    pub last: Arc<Mutex<Option<String>>>,
}

fn name_of(path: &str, prefix: &str) -> Option<String> {
    let raw = path.strip_prefix(prefix)?;
    let raw = raw.split('?').next().unwrap_or("");
    // %20 and friends
    let mut out = Vec::new();
    let b = raw.as_bytes();
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%' && i + 3 <= b.len() {
            if let Some(v) = std::str::from_utf8(&b[i + 1..i + 3]).ok().and_then(|h| u8::from_str_radix(h, 16).ok()) {
                out.push(v);
                i += 3;
                continue;
            }
        }
        out.push(if b[i] == b'+' { b' ' } else { b[i] });
        i += 1;
    }
    let s: String = String::from_utf8_lossy(&out).chars().filter(|c| !c.is_control()).take(60).collect();
    (!s.trim().is_empty()).then(|| s.trim().to_string())
}

fn push(queue: &Mutex<Vec<InputEvent>>, name: &str, press: bool, release: bool) {
    if let Ok(mut q) = queue.lock() {
        if q.len() > 256 {
            return;
        }
        let input = Input::Link { name: name.to_string() };
        if press {
            q.push(InputEvent { input: input.clone(), pressed: true });
        }
        if release {
            q.push(InputEvent { input, pressed: false });
        }
    }
}

fn respond(mut s: TcpStream, queue: &Mutex<Vec<InputEvent>>, cs2: &Mutex<Cs2>, last: &Mutex<Option<String>>) {
    let _ = s.set_read_timeout(Some(Duration::from_millis(800)));
    let mut buf = Vec::new();
    let mut tmp = [0u8; 4096];
    let (mut head_end, mut want) = (None, 0usize);
    loop {
        let n = s.read(&mut tmp).unwrap_or(0);
        if n == 0 {
            break;
        }
        buf.extend_from_slice(&tmp[..n]);
        if head_end.is_none() {
            if let Some(p) = buf.windows(4).position(|w| w == b"\r\n\r\n") {
                head_end = Some(p + 4);
                let head = String::from_utf8_lossy(&buf[..p]).to_lowercase();
                want = head.lines().find_map(|l| l.strip_prefix("content-length:").and_then(|v| v.trim().parse::<usize>().ok())).unwrap_or(0).min(MAX_BODY);
            }
        }
        if let Some(h) = head_end {
            if buf.len() >= h + want {
                break;
            }
        }
        if buf.len() > MAX_BODY + 8192 {
            break;
        }
    }
    let head_end = head_end.unwrap_or(buf.len());
    let head = String::from_utf8_lossy(&buf[..head_end.min(buf.len())]).to_string();
    let mut parts = head.lines().next().unwrap_or("").split_whitespace();
    let (method, path) = (parts.next().unwrap_or(""), parts.next().unwrap_or("/"));
    let body = &buf[head_end.min(buf.len())..];
    let (status, text) = if let Some(n) = name_of(path, "/trigger/") {
        push(queue, &n, true, true);
        *last.lock().unwrap() = Some(n);
        ("200 OK", "ok\n".to_string())
    } else if let Some(n) = name_of(path, "/press/") {
        push(queue, &n, true, false);
        *last.lock().unwrap() = Some(n);
        ("200 OK", "ok\n".to_string())
    } else if let Some(n) = name_of(path, "/release/") {
        push(queue, &n, false, true);
        ("200 OK", "ok\n".to_string())
    } else if path.starts_with("/gsi") && method == "POST" {
        match serde_json::from_slice::<Value>(body) {
            Ok(v) => {
                let mut state = cs2.lock().unwrap();
                let (events, now) = cs2_events(&state, &v);
                *state = now;
                for e in events {
                    push(queue, &e, true, true);
                    *last.lock().unwrap() = Some(e);
                }
                ("200 OK", "ok\n".to_string())
            }
            Err(_) => ("400 Bad Request", "not JSON\n".to_string()),
        }
    } else if path == "/" || path.is_empty() {
        ("200 OK", HELP.to_string())
    } else {
        ("404 Not Found", "not found\n".to_string())
    };
    let r = format!("HTTP/1.1 {status}\r\nContent-Type: text/plain; charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{text}", text.len());
    let _ = s.write_all(r.as_bytes());
}

impl LinkServer {
    /// Start on `port` (this computer only); every call puts events in `queue`.
    pub fn start(port: u16, queue: Arc<Mutex<Vec<InputEvent>>>) -> Result<LinkServer, String> {
        let listener = TcpListener::bind(("127.0.0.1", port)).map_err(|e| format!("Could not open port {port}: {e}"))?;
        listener.set_nonblocking(true).map_err(|e| e.to_string())?;
        let stop = Arc::new(AtomicBool::new(false));
        let last = Arc::new(Mutex::new(None));
        let cs2 = Arc::new(Mutex::new(Cs2::default()));
        let (s2, l2) = (stop.clone(), last.clone());
        let thread = std::thread::spawn(move || {
            while !s2.load(Ordering::Relaxed) {
                match listener.accept() {
                    Ok((stream, _)) => {
                        let (q, c, l) = (queue.clone(), cs2.clone(), l2.clone());
                        std::thread::spawn(move || respond(stream, &q, &c, &l));
                    }
                    Err(_) => std::thread::sleep(Duration::from_millis(10)),
                }
            }
        });
        Ok(LinkServer { port, stop, thread: Some(thread), last })
    }
}

impl Drop for LinkServer {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn call(port: u16, method: &str, path: &str, body: &str) -> String {
        let mut s = TcpStream::connect(("127.0.0.1", port)).unwrap();
        s.set_read_timeout(Some(Duration::from_secs(3))).unwrap();
        s.write_all(format!("{method} {path} HTTP/1.1\r\nHost: x\r\nContent-Length: {}\r\n\r\n{body}", body.len()).as_bytes()).unwrap();
        let mut out = String::new();
        let _ = s.read_to_string(&mut out);
        out
    }

    #[test]
    fn names_are_read_from_the_address() {
        assert_eq!(name_of("/trigger/Happy", "/trigger/").as_deref(), Some("Happy"));
        assert_eq!(name_of("/trigger/Big%20smile?x=1", "/trigger/").as_deref(), Some("Big smile"));
        assert_eq!(name_of("/trigger/a+b", "/trigger/").as_deref(), Some("a b"));
        assert_eq!(name_of("/trigger/", "/trigger/"), None);
        assert_eq!(name_of("/other/x", "/trigger/"), None);
    }

    #[test]
    fn cs2_changes_become_events() {
        let a = json!({"player": {"state": {"health": 100}, "match_stats": {"kills": 2}}, "round": {"phase": "freezetime", "bomb": "carried"}});
        let (events, s1) = cs2_events(&Cs2::default(), &a);
        assert_eq!(events, ["cs2.round_freezetime", "cs2.bomb_carried"], "the first state has no kills to count");
        let b = json!({"player": {"state": {"health": 100}, "match_stats": {"kills": 4}}, "round": {"phase": "live", "bomb": "planted"}});
        let (events, s2) = cs2_events(&s1, &b);
        assert_eq!(events, ["cs2.kill", "cs2.kill", "cs2.round_live", "cs2.bomb_planted"]);
        let c = json!({"player": {"state": {"health": 0}, "match_stats": {"kills": 4}}, "round": {"phase": "live", "bomb": "planted"}});
        assert_eq!(cs2_events(&s2, &c).0, ["cs2.death"]);
        // nothing changed: nothing happens
        assert!(cs2_events(&s2, &b).0.is_empty());
        assert!(cs2_config(39560).contains("http://127.0.0.1:39560/gsi"));
    }

    #[test]
    fn a_call_from_a_stream_deck_or_a_game_becomes_inputs() {
        let queue = Arc::new(Mutex::new(Vec::new()));
        let free = TcpListener::bind("127.0.0.1:0").unwrap().local_addr().unwrap().port();
        let srv = LinkServer::start(free, queue.clone()).unwrap();
        assert!(call(free, "GET", "/", "").contains("SeviathStudio Link"));
        assert!(call(free, "GET", "/trigger/Hello", "").starts_with("HTTP/1.1 200"));
        {
            let q = queue.lock().unwrap();
            let hello = Input::Link { name: "Hello".into() };
            assert_eq!(*q, [InputEvent { input: hello.clone(), pressed: true }, InputEvent { input: hello, pressed: false }]);
        }
        queue.lock().unwrap().clear();
        assert!(call(free, "POST", "/press/Hold", "").starts_with("HTTP/1.1 200"));
        assert_eq!(queue.lock().unwrap().len(), 1, "only pressed");
        queue.lock().unwrap().clear();
        let a = json!({"player": {"state": {"health": 100}, "match_stats": {"kills": 0}}}).to_string();
        let b = json!({"player": {"state": {"health": 100}, "match_stats": {"kills": 1}}}).to_string();
        assert!(call(free, "POST", "/gsi", &a).starts_with("HTTP/1.1 200"));
        assert!(call(free, "POST", "/gsi", &b).starts_with("HTTP/1.1 200"));
        let q = queue.lock().unwrap();
        assert!(q.iter().any(|e| e.input == Input::Link { name: "cs2.kill".into() } && e.pressed), "{q:?}");
        drop(q);
        assert!(call(free, "POST", "/gsi", "not json").starts_with("HTTP/1.1 400"));
        assert!(call(free, "GET", "/nothing", "").starts_with("HTTP/1.1 404"));
        assert_eq!(srv.last.lock().unwrap().as_deref(), Some("cs2.kill"));
        // a second server cannot take the same port
        assert!(LinkServer::start(free, queue.clone()).is_err());
    }
}
