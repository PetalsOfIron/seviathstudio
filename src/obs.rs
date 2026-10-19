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
    /// OBS's virtual camera is on (the camera other programs such as Discord, Zoom or a browser can pick).
    pub virtualcam: bool,
}

#[derive(Clone, PartialEq, Debug)]
pub enum Cmd {
    Scene(String),
    StartStream,
    StopStream,
    ToggleStream,
    StartRecord,
    StopRecord,
    /// Make a scene with the model (and a background colour behind it) and show it.
    SetupModel { url: String, width: u32, height: u32, background: [u8; 3] },
    StartVirtualCam,
    StopVirtualCam,
    ToggleVirtualCam,
    ToggleRecord,
    Refresh,
    Close,
}

pub struct Obs {
    tx: Sender<Cmd>,
    pub state: Arc<Mutex<ObsState>>,
}

pub fn b64(bytes: &[u8]) -> String {
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

/// The name of the scene made for the model.
pub const MODEL_SCENE: &str = "SeviathStudio";

/// One batch of requests, run in order: a scene, a colour behind, the model as a browser source, then that scene is shown and the virtual
/// camera started. A part that is there already (the scene, the sources) is only updated; one that fails does not stop the rest.
pub fn setup_batch(url: &str, width: u32, height: u32, background: [u8; 3]) -> Value {
    // OBS colours are 0xAABBGGRR
    let abgr = 0xFF00_0000u32 | (background[2] as u32) << 16 | (background[1] as u32) << 8 | background[0] as u32;
    let bg_settings = json!({ "color": abgr, "width": width, "height": height });
    let css = "body { background-color: rgba(0, 0, 0, 0); margin: 0px auto; overflow: hidden; }";
    let model_settings = json!({ "url": url, "width": width, "height": height, "fps": 30, "css": css, "reroute_audio": false });
    json!({ "op": 8, "d": { "requestId": "setup", "haltOnFailure": false, "executionType": 0, "requests": [
        { "requestType": "CreateScene", "requestData": { "sceneName": MODEL_SCENE } },
        { "requestType": "CreateInput", "requestData": { "sceneName": MODEL_SCENE, "inputName": "Seviath background", "inputKind": "color_source", "inputSettings": bg_settings, "sceneItemEnabled": true } },
        { "requestType": "SetInputSettings", "requestData": { "inputName": "Seviath background", "inputSettings": bg_settings, "overlay": true } },
        { "requestType": "CreateInput", "requestData": { "sceneName": MODEL_SCENE, "inputName": "Seviath model", "inputKind": "browser_source", "inputSettings": model_settings, "sceneItemEnabled": true } },
        { "requestType": "SetInputSettings", "requestData": { "inputName": "Seviath model", "inputSettings": model_settings, "overlay": true } },
        { "requestType": "SetCurrentProgramScene", "requestData": { "sceneName": MODEL_SCENE } },
        { "requestType": "StartVirtualCam" },
    ] } })
}

/// Where OBS Studio is installed on this computer (the program file), if it is.
pub fn find_install() -> Option<std::path::PathBuf> {
    let mut roots: Vec<std::path::PathBuf> = ["ProgramFiles", "ProgramFiles(x86)", "ProgramW6432"].iter().filter_map(|k| std::env::var_os(k)).map(std::path::PathBuf::from).collect();
    roots.push("C:\\Program Files".into());
    find_install_in(&roots)
}

pub fn find_install_in(roots: &[std::path::PathBuf]) -> Option<std::path::PathBuf> {
    roots.iter().map(|r| r.join("obs-studio").join("bin").join("64bit").join("obs64.exe")).find(|p| p.exists())
}

/// Start OBS (it must be started from its own folder). Returns at once.
pub fn launch(exe: &std::path::Path) -> Result<(), String> {
    let dir = exe.parent().ok_or("OBS has no folder?")?;
    let mut cmd = std::process::Command::new(exe);
    cmd.current_dir(dir).arg("--disable-shutdown-check");
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        // not as a child of this program's window
        cmd.creation_flags(0x0000_0008);
    }
    cmd.spawn().map(|_| ()).map_err(|e| format!("Could not start OBS: {e}"))
}

/// Is OBS running now? (Its settings are only read when it starts, and written when it closes.)
pub fn is_running() -> bool {
    let mut cmd = std::process::Command::new("tasklist");
    cmd.args(["/FI", "IMAGENAME eq obs64.exe", "/NH"]);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x0800_0000);
    }
    cmd.output().map(|o| String::from_utf8_lossy(&o.stdout).to_lowercase().contains("obs64.exe")).unwrap_or(false)
}

/// Switch the WebSocket server of OBS on with this password, by writing its settings file (`config_dir` is the folder `obs-studio` in
/// the user's application data). The old file is kept next to it. OBS must not be running.
pub fn enable_websocket(config_dir: &std::path::Path, password: &str) -> Result<std::path::PathBuf, String> {
    let folder = config_dir.join("plugin_config").join("obs-websocket");
    std::fs::create_dir_all(&folder).map_err(|e| format!("Could not make {}: {e}", folder.display()))?;
    let file = folder.join("config.json");
    if file.exists() {
        let _ = std::fs::copy(&file, folder.join("config.json.before-seviath"));
    }
    let cfg = json!({ "alerts_enabled": false, "auth_required": true, "first_load": false, "server_enabled": true, "server_password": password, "server_port": 4455 });
    std::fs::write(&file, serde_json::to_string_pretty(&cfg).unwrap_or_default()).map_err(|e| format!("Could not write {}: {e}", file.display()))?;
    Ok(file)
}

/// A password for OBS (letters and numbers), made from the clock and the process, not guessable by a program on the network, and not
/// something worth remembering: the program keeps it for this session.
pub fn make_password() -> String {
    let mut h = Sha256::new();
    h.update(std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_nanos()).unwrap_or(0).to_le_bytes());
    h.update(std::process::id().to_le_bytes());
    h.update(format!("{:p}", &h as *const _));
    let b = b64(&h.finalize());
    b.chars().filter(|c| c.is_ascii_alphanumeric()).take(16).collect()
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
            Some("GetVirtualCamStatus") => st.virtualcam = d["responseData"]["outputActive"].as_bool().unwrap_or(false),
            _ => {}
        },
        5 => match d["eventType"].as_str() {
            Some("CurrentProgramSceneChanged") => st.current = d["eventData"]["sceneName"].as_str().map(str::to_string),
            Some("StreamStateChanged") => st.streaming = d["eventData"]["outputActive"].as_bool().unwrap_or(st.streaming),
            Some("RecordStateChanged") => st.recording = d["eventData"]["outputActive"].as_bool().unwrap_or(st.recording),
            Some("VirtualcamStateChanged") => st.virtualcam = d["eventData"]["outputActive"].as_bool().unwrap_or(st.virtualcam),
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
    for (kind, id) in [("GetSceneList", "scenes"), ("GetStreamStatus", "stream"), ("GetRecordStatus", "record"), ("GetVirtualCamStatus", "vcam")] {
        request(&mut ws, kind, id, json!({}))?;
    }
    loop {
        while let Ok(cmd) = rx.try_recv() {
            let (st_stream, st_record, st_vcam) = {
                let s = state.lock().unwrap();
                (s.streaming, s.recording, s.virtualcam)
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
                Cmd::StartVirtualCam => request(&mut ws, "StartVirtualCam", "x", json!({}))?,
                Cmd::StopVirtualCam => request(&mut ws, "StopVirtualCam", "x", json!({}))?,
                Cmd::ToggleVirtualCam => request(&mut ws, if st_vcam { "StopVirtualCam" } else { "StartVirtualCam" }, "x", json!({}))?,
                Cmd::SetupModel { url, width, height, background } => send(&mut ws, setup_batch(&url, width, height, background))?,
                Cmd::StopRecord => request(&mut ws, "StopRecord", "x", json!({}))?,
                Cmd::ToggleRecord => request(&mut ws, if st_record { "StopRecord" } else { "StartRecord" }, "x", json!({}))?,
                Cmd::Refresh => {
                    for (kind, id) in [("GetSceneList", "scenes"), ("GetStreamStatus", "stream"), ("GetRecordStatus", "record"), ("GetVirtualCamStatus", "vcam")] {
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
                if v["op"] == 8 {
                    let kinds: Vec<String> = v["d"]["requests"].as_array().unwrap().iter().map(|r| r["requestType"].as_str().unwrap_or("").to_string()).collect();
                    seen.lock().unwrap().push(format!("batch {}", kinds.join(",")));
                    ws.send(Message::text(json!({"op": 9, "d": {"requestId": "setup", "results": []}}).to_string())).unwrap();
                    ws.send(Message::text(json!({"op": 5, "d": {"eventType": "VirtualcamStateChanged", "eventData": {"outputActive": true}}}).to_string())).unwrap();
                    continue;
                }
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
    fn the_virtual_camera_is_set_up_with_the_model_scene_and_started() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let seen = Arc::new(Mutex::new(Vec::new()));
        fake_obs(listener, "secret", seen.clone());
        let obs = Obs::connect("127.0.0.1", port, "secret");
        wait(&obs, |s| s.status == Status::Connected);
        obs.send(Cmd::SetupModel { url: "http://localhost:8765/".into(), width: 1280, height: 720, background: [0, 177, 64] });
        let s = wait(&obs, |s| s.virtualcam);
        assert!(s.virtualcam, "the event says the virtual camera is on");
        let batch = seen.lock().unwrap().iter().find(|l| l.starts_with("batch")).cloned().expect("one batch was sent");
        assert_eq!(batch, "batch CreateScene,CreateInput,SetInputSettings,CreateInput,SetInputSettings,SetCurrentProgramScene,StartVirtualCam");
        drop(obs);
        // what is in the batch: the browser source with the address, and the colour as OBS writes it (0xAABBGGRR)
        let b = setup_batch("http://localhost:8765/", 1280, 720, [0, 177, 64]);
        let reqs = b["d"]["requests"].as_array().unwrap();
        assert_eq!(reqs[3]["requestData"]["inputKind"], "browser_source");
        assert_eq!(reqs[3]["requestData"]["inputSettings"]["url"], "http://localhost:8765/");
        assert_eq!(reqs[1]["requestData"]["inputSettings"]["color"], 0xFF40B100u32);
        assert_eq!(b["d"]["haltOnFailure"], false);
    }

    #[test]
    fn obs_is_found_and_its_websocket_is_switched_on_with_the_old_file_kept() {
        let dir = std::env::temp_dir().join(format!("seviath_obs_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        assert!(find_install_in(&[dir.clone()]).is_none());
        let exe = dir.join("obs-studio").join("bin").join("64bit").join("obs64.exe");
        std::fs::create_dir_all(exe.parent().unwrap()).unwrap();
        std::fs::write(&exe, b"x").unwrap();
        assert_eq!(find_install_in(&[std::path::PathBuf::from("nowhere"), dir.clone()]), Some(exe));
        // the settings of OBS: an old file is kept, the new one has the server on and the password in it
        let cfg = dir.join("config");
        let folder = cfg.join("plugin_config").join("obs-websocket");
        std::fs::create_dir_all(&folder).unwrap();
        std::fs::write(folder.join("config.json"), "{\"server_enabled\": false}").unwrap();
        let file = enable_websocket(&cfg, "pw12345").unwrap();
        let v: Value = serde_json::from_str(&std::fs::read_to_string(&file).unwrap()).unwrap();
        assert_eq!((v["server_enabled"].as_bool(), v["server_password"].as_str(), v["server_port"].as_u64(), v["auth_required"].as_bool()), (Some(true), Some("pw12345"), Some(4455), Some(true)));
        assert_eq!(std::fs::read_to_string(folder.join("config.json.before-seviath")).unwrap(), "{\"server_enabled\": false}");
        // passwords are different every time and plain letters and numbers
        let (a, b) = (make_password(), make_password());
        assert!(a.len() == 16 && a.chars().all(|c| c.is_ascii_alphanumeric()), "{a}");
        assert_ne!(a, b);
        let _ = std::fs::remove_dir_all(&dir);
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
