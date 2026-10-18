// SPDX-License-Identifier: GPL-3.0-or-later
// Remote control of OBS Studio (obs-websocket 5, which is part of OBS since version 28): switch the scene, start and stop the stream
// and the recording, from a hotkey or a button. The program talks to OBS on this computer (or another one on the network) over a
// WebSocket; the password is the one set in OBS under Tools, WebSocket Server Settings. Nothing is sent anywhere else.

use std::net::TcpStream;
use std::sync::mpsc::{Receiver, Sender, channel};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use tungstenite::{Message, WebSocket, stream::MaybeTlsStream};

#[derive(Clone, PartialEq, Debug, Default)]
pub enum Status {
    #[default]
    Off,
    Connecting,
    Connected,
    Failed(String),
}

#[derive(Clone, Debug, Default)]
pub struct ObsState {
    pub status: Status,
    pub scenes: Vec<String>,
    pub current: Option<String>,
    pub streaming: bool,
    pub recording: bool,
}

#[derive(Clone, PartialEq, Debug)]
pub enum Cmd {
    Scene(String),
    StartStream,
    StopStream,
    ToggleStream,
    StartRecord,
    StopRecord,
    ToggleRecord,
    Refresh,
    Close,
}

pub struct Obs {
    tx: Sender<Cmd>,
    pub state: Arc<Mutex<ObsState>>,
}

fn b64(bytes: &[u8]) -> String {
    const T: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::new();
    for c in bytes.chunks(3) {
        let n = (c[0] as u32) << 16 | (*c.get(1).unwrap_or(&0) as u32) << 8 | *c.get(2).unwrap_or(&0) as u32;
        out.push(T[(n >> 18) as usize & 63] as char);
        out.push(T[(n >> 12) as usize & 63] as char);
        out.push(if c.len() > 1 { T[(n >> 6) as usize & 63] as char } else { '=' });
        out.push(if c.len() > 2 { T[n as usize & 63] as char } else { '=' });
    }
    out
}

/// The answer OBS wants to the challenge it sent: base64 of the SHA-256 of (base64 of the SHA-256 of password and salt) and the challenge.
pub fn authentication(password: &str, salt: &str, challenge: &str) -> String {
    let secret = b64(&Sha256::digest(format!("{password}{salt}").as_bytes()));
    b64(&Sha256::digest(format!("{secret}{challenge}").as_bytes()))
}

type Socket = WebSocket<MaybeTlsStream<TcpStream>>;

fn send(ws: &mut Socket, v: Value) -> Result<(), String> {
    ws.send(Message::text(v.to_string())).map_err(|e| e.to_string())
}

fn request(ws: &mut Socket, kind: &str, id: &str, data: Value) -> Result<(), String> {
    send(ws, json!({"op": 6, "d": {"requestType": kind, "requestId": id, "requestData": data}}))
}

/// Read one message; None when nothing came within the wait.
fn read(ws: &mut Socket) -> Result<Option<Value>, String> {
    match ws.read() {
        Ok(Message::Text(t)) => Ok(serde_json::from_str(t.as_ref()).ok()),
        Ok(Message::Close(_)) => Err("OBS closed the connection.".into()),
        Ok(_) => Ok(None),
        Err(tungstenite::Error::Io(e)) if matches!(e.kind(), std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut) => Ok(None),
        Err(e) => Err(e.to_string()),
    }
}

fn set_timeout(ws: &mut Socket, ms: u64) {
    if let MaybeTlsStream::Plain(s) = ws.get_mut() {
        let _ = s.set_read_timeout(Some(Duration::from_millis(ms)));
    }
}

fn handle(msg: &Value, state: &Arc<Mutex<ObsState>>) {
    let op = msg["op"].as_i64().unwrap_or(-1);
    let d = &msg["d"];
    let mut st = state.lock().unwrap();
    match op {
        7 => match d["requestType"].as_str() {
            Some("GetSceneList") => {
                let data = &d["responseData"];
                st.scenes = data["scenes"].as_array().map(|a| a.iter().rev().filter_map(|s| s["sceneName"].as_str().map(str::to_string)).collect()).unwrap_or_default();
                st.current = data["currentProgramSceneName"].as_str().map(str::to_string);
            }
            Some("GetStreamStatus") => st.streaming = d["responseData"]["outputActive"].as_bool().unwrap_or(false),
            Some("GetRecordStatus") => st.recording = d["responseData"]["outputActive"].as_bool().unwrap_or(false),
            _ => {}
        },
        5 => match d["eventType"].as_str() {
            Some("CurrentProgramSceneChanged") => st.current = d["eventData"]["sceneName"].as_str().map(str::to_string),
            Some("StreamStateChanged") => st.streaming = d["eventData"]["outputActive"].as_bool().unwrap_or(st.streaming),
            Some("RecordStateChanged") => st.recording = d["eventData"]["outputActive"].as_bool().unwrap_or(st.recording),
            Some("SceneListChanged") => {}
            _ => {}
        },
        _ => {}
    }
}

fn run(host: String, port: u16, password: String, rx: Receiver<Cmd>, state: Arc<Mutex<ObsState>>) -> Result<(), String> {
    let (mut ws, _) = tungstenite::connect(format!("ws://{host}:{port}")).map_err(|e| format!("Could not reach OBS at {host}:{port} ({e}). Is OBS open with the WebSocket server on?"))?;
    set_timeout(&mut ws, 5000);
    let hello = loop {
        match read(&mut ws)? {
            Some(m) if m["op"] == 0 => break m,
            Some(_) => continue,
            None => return Err("OBS did not say hello.".into()),
        }
    };
    let mut identify = json!({"rpcVersion": 1, "eventSubscriptions": 1 | 4 | 64});
    if let Some(a) = hello["d"].get("authentication").filter(|a| !a.is_null()) {
        if password.is_empty() {
            return Err("OBS wants a password (Tools, WebSocket Server Settings in OBS).".into());
        }
        identify["authentication"] = json!(authentication(&password, a["salt"].as_str().unwrap_or(""), a["challenge"].as_str().unwrap_or("")));
    }
    send(&mut ws, json!({"op": 1, "d": identify}))?;
    loop {
        match read(&mut ws) {
            Ok(Some(m)) if m["op"] == 2 => break,
            Ok(Some(_)) => continue,
            Ok(None) => return Err("OBS did not accept the connection (wrong password?).".into()),
            Err(e) => return Err(if e.contains("Connection") || e.contains("closed") { "OBS refused the password.".into() } else { e }),
        }
    }
    state.lock().unwrap().status = Status::Connected;
    set_timeout(&mut ws, 60);
    for (kind, id) in [("GetSceneList", "scenes"), ("GetStreamStatus", "stream"), ("GetRecordStatus", "record")] {
        request(&mut ws, kind, id, json!({}))?;
    }
    loop {
        while let Ok(cmd) = rx.try_recv() {
            let (st_stream, st_record) = {
                let s = state.lock().unwrap();
                (s.streaming, s.recording)
            };
            match cmd {
                Cmd::Close => {
                    let _ = ws.close(None);
                    return Ok(());
                }
                Cmd::Scene(name) => request(&mut ws, "SetCurrentProgramScene", "scene", json!({"sceneName": name}))?,
                Cmd::StartStream => request(&mut ws, "StartStream", "x", json!({}))?,
                Cmd::StopStream => request(&mut ws, "StopStream", "x", json!({}))?,
                Cmd::ToggleStream => request(&mut ws, if st_stream { "StopStream" } else { "StartStream" }, "x", json!({}))?,
                Cmd::StartRecord => request(&mut ws, "StartRecord", "x", json!({}))?,
                Cmd::StopRecord => request(&mut ws, "StopRecord", "x", json!({}))?,
                Cmd::ToggleRecord => request(&mut ws, if st_record { "StopRecord" } else { "StartRecord" }, "x", json!({}))?,
                Cmd::Refresh => {
                    for (kind, id) in [("GetSceneList", "scenes"), ("GetStreamStatus", "stream"), ("GetRecordStatus", "record")] {
                        request(&mut ws, kind, id, json!({}))?;
                    }
                }
            }
        }
        if let Some(m) = read(&mut ws)? {
            handle(&m, &state);
        }
    }
}

impl Obs {
    pub fn connect(host: &str, port: u16, password: &str) -> Obs {
        let (tx, rx) = channel();
        let state = Arc::new(Mutex::new(ObsState { status: Status::Connecting, ..Default::default() }));
        let (h, p, s2) = (host.to_string(), password.to_string(), state.clone());
        std::thread::spawn(move || {
            let r = run(h, port, p, rx, s2.clone());
            let mut st = s2.lock().unwrap();
            st.status = match r {
                Ok(()) => Status::Off,
                Err(e) => Status::Failed(e),
            };
        });
        Obs { tx, state }
    }

    pub fn send(&self, cmd: Cmd) {
        let _ = self.tx.send(cmd);
    }

    pub fn snapshot(&self) -> ObsState {
        self.state.lock().unwrap().clone()
    }
}

impl Drop for Obs {
    fn drop(&mut self) {
        let _ = self.tx.send(Cmd::Close);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::TcpListener;

    #[test]
    fn base64_and_the_password_answer_are_as_obs_documents_them() {
        assert_eq!(b64(b""), "");
        assert_eq!(b64(b"f"), "Zg==");
        assert_eq!(b64(b"fo"), "Zm8=");
        assert_eq!(b64(b"foo"), "Zm9v");
        assert_eq!(b64(b"foobar"), "Zm9vYmFy");
        // the example of the obs-websocket protocol document
        assert_eq!(authentication("supersecretpassword", "lM1GncleQOaCu9lT1yeUZhFYnqhsLLP1G5lAGo3ixaI=", "+IxH4CnCiqpX1rM9scsNynZzbOe4KhDeYcTNS3PDaeY="), "1Ct943GAT+6YQUUX47Ia/ncufilbe6+oD6lY+5kaCu4=");
    }

    /// A little OBS: says hello with a password challenge, checks the answer, lists two scenes and obeys scene changes and stream starts.
    fn fake_obs(listener: TcpListener, password: &'static str, seen: Arc<Mutex<Vec<String>>>) {
        std::thread::spawn(move || {
            let Ok((stream, _)) = listener.accept() else { return };
            let Ok(mut ws) = tungstenite::accept(stream) else { return };
            let (salt, challenge) = ("saltsaltsalt", "challengechallenge");
            ws.send(Message::text(json!({"op": 0, "d": {"obsWebSocketVersion": "5.0.0", "rpcVersion": 1, "authentication": {"challenge": challenge, "salt": salt}}}).to_string())).unwrap();
            let Ok(Message::Text(t)) = ws.read() else { return };
            let v: Value = serde_json::from_str(t.as_ref()).unwrap();
            if v["d"]["authentication"] != json!(authentication(password, salt, challenge)) {
                let _ = ws.close(None);
                return;
            }
            ws.send(Message::text(json!({"op": 2, "d": {"negotiatedRpcVersion": 1}}).to_string())).unwrap();
            while let Ok(Message::Text(t)) = ws.read() {
                let v: Value = serde_json::from_str(t.as_ref()).unwrap();
                if v["op"] != 6 {
                    continue;
                }
                let (kind, data) = (v["d"]["requestType"].as_str().unwrap_or("").to_string(), v["d"]["requestData"].clone());
                seen.lock().unwrap().push(format!("{kind} {}", data));
                let reply = match kind.as_str() {
                    "GetSceneList" => json!({"scenes": [{"sceneName": "Be right back"}, {"sceneName": "Main"}], "currentProgramSceneName": "Main"}),
                    "GetStreamStatus" | "GetRecordStatus" => json!({"outputActive": false}),
                    _ => json!({}),
                };
                ws.send(Message::text(json!({"op": 7, "d": {"requestType": kind, "requestId": v["d"]["requestId"], "requestStatus": {"result": true, "code": 100}, "responseData": reply}}).to_string())).unwrap();
                if kind == "SetCurrentProgramScene" {
                    ws.send(Message::text(json!({"op": 5, "d": {"eventType": "CurrentProgramSceneChanged", "eventData": {"sceneName": data["sceneName"]}}}).to_string())).unwrap();
                }
                if kind == "StartStream" {
                    ws.send(Message::text(json!({"op": 5, "d": {"eventType": "StreamStateChanged", "eventData": {"outputActive": true}}}).to_string())).unwrap();
                }
            }
        });
    }

    fn wait(obs: &Obs, f: impl Fn(&ObsState) -> bool) -> ObsState {
        for _ in 0..200 {
            let s = obs.snapshot();
            if f(&s) {
                return s;
            }
            std::thread::sleep(Duration::from_millis(25));
        }
        obs.snapshot()
    }

    #[test]
    fn it_logs_in_lists_scenes_switches_and_starts_the_stream() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let seen = Arc::new(Mutex::new(Vec::new()));
        fake_obs(listener, "secret", seen.clone());
        let obs = Obs::connect("127.0.0.1", port, "secret");
        let s = wait(&obs, |s| !s.scenes.is_empty());
        assert_eq!(s.status, Status::Connected, "{:?}", s.status);
        assert_eq!(s.scenes, ["Main", "Be right back"], "in the order OBS shows them");
        assert_eq!(s.current.as_deref(), Some("Main"));
        obs.send(Cmd::Scene("Be right back".into()));
        let s = wait(&obs, |s| s.current.as_deref() == Some("Be right back"));
        assert_eq!(s.current.as_deref(), Some("Be right back"), "the event moved the current scene");
        obs.send(Cmd::ToggleStream);
        let s = wait(&obs, |s| s.streaming);
        assert!(s.streaming);
        assert!(seen.lock().unwrap().iter().any(|l| l.starts_with("StartStream")), "{:?}", seen.lock().unwrap());
        drop(obs);
    }

    #[test]
    fn a_wrong_password_and_a_missing_obs_are_said_in_words() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        fake_obs(listener, "secret", Arc::new(Mutex::new(Vec::new())));
        let obs = Obs::connect("127.0.0.1", port, "nope");
        let s = wait(&obs, |s| matches!(s.status, Status::Failed(_)));
        assert!(matches!(&s.status, Status::Failed(m) if m.contains("password")), "{:?}", s.status);
        // nothing listens there
        let free = TcpListener::bind("127.0.0.1:0").unwrap().local_addr().unwrap().port();
        let obs = Obs::connect("127.0.0.1", free, "");
        let s = wait(&obs, |s| matches!(s.status, Status::Failed(_)));
        assert!(matches!(&s.status, Status::Failed(m) if m.contains("Could not reach OBS")), "{:?}", s.status);
    }
}
