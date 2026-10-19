// SPDX-License-Identifier: GPL-3.0-or-later
// ComfyUI bridge: the program does not make pictures itself, it asks a ComfyUI that the user already runs on this computer (or the network),
// the same way aibridge.rs asks Ollama. A workflow (the graph that ComfyUI saves with "Save (API format)") has places for the words, the
// size and the seed; the picture that comes back is put on a new layer. Nothing the server says is run: only picture bytes are read, and
// they must open as a picture.

use std::time::{Duration, Instant};

use serde_json::{Value, json};

/// The graph used when the user has none of their own: a plain text to picture with one checkpoint.
pub const DEFAULT_WORKFLOW: &str = r#"{
  "3": {"class_type": "KSampler", "inputs": {"seed": "{{seed}}", "steps": 20, "cfg": 7, "sampler_name": "euler", "scheduler": "normal", "denoise": 1, "model": ["4", 0], "positive": ["6", 0], "negative": ["7", 0], "latent_image": ["5", 0]}},
  "4": {"class_type": "CheckpointLoaderSimple", "inputs": {"ckpt_name": "{{checkpoint}}"}},
  "5": {"class_type": "EmptyLatentImage", "inputs": {"width": "{{width}}", "height": "{{height}}", "batch_size": 1}},
  "6": {"class_type": "CLIPTextEncode", "inputs": {"text": "{{prompt}}", "clip": ["4", 1]}},
  "7": {"class_type": "CLIPTextEncode", "inputs": {"text": "{{negative}}", "clip": ["4", 1]}},
  "8": {"class_type": "VAEDecode", "inputs": {"samples": ["3", 0], "vae": ["4", 2]}},
  "9": {"class_type": "SaveImage", "inputs": {"filename_prefix": "SeviathStudio", "images": ["8", 0]}}
}"#;

/// What goes into the places of a workflow.
#[derive(Clone, Debug)]
pub struct Fill {
    pub prompt: String,
    pub negative: String,
    pub checkpoint: String,
    pub seed: u64,
    pub width: u32,
    pub height: u32,
}

/// The workflow with its places filled. A text that is only `{{seed}}`, `{{width}}` or `{{height}}` becomes a number; in other texts the
/// places are replaced where they stand. Anything else is left as it is.
pub fn fill_workflow(template: &str, f: &Fill) -> Result<Value, String> {
    let mut v: Value = serde_json::from_str(template).map_err(|e| format!("The workflow is not readable ({e}). Save it from ComfyUI with Save (API format)."))?;
    if !v.is_object() {
        return Err("The workflow must be the API format of ComfyUI (an object of nodes).".into());
    }
    fn walk(v: &mut Value, f: &Fill) {
        match v {
            Value::String(s) => {
                match s.as_str() {
                    "{{seed}}" => *v = json!(f.seed),
                    "{{width}}" => *v = json!(f.width),
                    "{{height}}" => *v = json!(f.height),
                    _ => {
                        *s = s
                            .replace("{{prompt}}", &f.prompt)
                            .replace("{{negative}}", &f.negative)
                            .replace("{{checkpoint}}", &f.checkpoint)
                            .replace("{{seed}}", &f.seed.to_string())
                            .replace("{{width}}", &f.width.to_string())
                            .replace("{{height}}", &f.height.to_string());
                    }
                }
            }
            Value::Array(a) => a.iter_mut().for_each(|x| walk(x, f)),
            Value::Object(o) => o.values_mut().for_each(|x| walk(x, f)),
            _ => {}
        }
    }
    walk(&mut v, f);
    Ok(v)
}

/// A picture ComfyUI made: where to ask for it.
#[derive(Clone, Debug, PartialEq)]
pub struct Made {
    pub filename: String,
    pub subfolder: String,
    pub kind: String,
}

#[derive(Clone, Debug)]
pub struct Comfy {
    pub url: String,
}

impl Comfy {
    pub fn new(url: &str) -> Comfy {
        Comfy { url: url.trim().trim_end_matches('/').to_string() }
    }

    fn agent(&self, secs: u64) -> ureq::Agent {
        ureq::AgentBuilder::new().timeout_connect(Duration::from_secs(4)).timeout(Duration::from_secs(secs)).build()
    }

    fn check(&self) -> Result<(), String> {
        if self.url.is_empty() {
            return Err("There is no address for ComfyUI (usually http://127.0.0.1:8188).".into());
        }
        Ok(())
    }

    fn get_json(&self, path: &str) -> Result<Value, String> {
        self.check()?;
        match self.agent(30).get(&format!("{}{path}", self.url)).call() {
            Ok(r) => r.into_json::<Value>().map_err(|e| format!("ComfyUI answered with something that is not readable ({e}).")),
            Err(ureq::Error::Status(code, _)) => Err(format!("ComfyUI answered {code} for {path}.")),
            Err(_) => Err("Could not reach ComfyUI. Is it running, and is the address right?".into()),
        }
    }

    /// The checkpoints ComfyUI has (for the list in the window).
    pub fn checkpoints(&self) -> Result<Vec<String>, String> {
        let info = self.get_json("/object_info/CheckpointLoaderSimple")?;
        let list = info.pointer("/CheckpointLoaderSimple/input/required/ckpt_name/0").and_then(|v| v.as_array()).ok_or("ComfyUI did not list its checkpoints.")?;
        Ok(list.iter().filter_map(|v| v.as_str().map(str::to_owned)).collect())
    }

    /// Put the workflow in the queue; the answer is the number of the job.
    pub fn queue(&self, workflow: &Value) -> Result<String, String> {
        self.check()?;
        let body = json!({ "prompt": workflow, "client_id": "seviathstudio" });
        match self.agent(30).post(&format!("{}/prompt", self.url)).send_json(body) {
            Ok(r) => {
                let a: Value = r.into_json().map_err(|e| e.to_string())?;
                a.get("prompt_id").and_then(|v| v.as_str()).map(str::to_owned).ok_or_else(|| "ComfyUI did not give a job number.".to_string())
            }
            Err(ureq::Error::Status(code, r)) => {
                // ComfyUI says what is wrong with a workflow (a missing checkpoint, a node it does not know)
                let text = r.into_string().unwrap_or_default();
                let why = serde_json::from_str::<Value>(&text).ok().and_then(|v| v.pointer("/error/message").and_then(|m| m.as_str()).map(str::to_owned));
                Err(format!("ComfyUI did not accept the workflow ({code}){}", why.map(|w| format!(": {w}")).unwrap_or_default()))
            }
            Err(_) => Err("Could not reach ComfyUI. Is it running, and is the address right?".into()),
        }
    }

    /// Wait for the job to finish and say which pictures it made.
    pub fn wait(&self, id: &str, limit: Duration, every: Duration) -> Result<Vec<Made>, String> {
        let t = Instant::now();
        loop {
            let h = self.get_json(&format!("/history/{id}"))?;
            if let Some(job) = h.get(id) {
                if job.pointer("/status/status_str").and_then(|s| s.as_str()) == Some("error") {
                    return Err("ComfyUI stopped with an error while making the picture (look at its window).".into());
                }
                let mut out = Vec::new();
                if let Some(nodes) = job.get("outputs").and_then(|o| o.as_object()) {
                    for node in nodes.values() {
                        for im in node.get("images").and_then(|i| i.as_array()).into_iter().flatten() {
                            out.push(Made {
                                filename: im.get("filename").and_then(|v| v.as_str()).unwrap_or_default().to_owned(),
                                subfolder: im.get("subfolder").and_then(|v| v.as_str()).unwrap_or_default().to_owned(),
                                kind: im.get("type").and_then(|v| v.as_str()).unwrap_or("output").to_owned(),
                            });
                        }
                    }
                }
                if !out.is_empty() {
                    return Ok(out);
                }
            }
            if t.elapsed() > limit {
                return Err("ComfyUI is taking too long. It may still be working: look at its window.".into());
            }
            std::thread::sleep(every);
        }
    }

    /// The picture itself, checked to open as a picture.
    pub fn fetch(&self, m: &Made) -> Result<image::RgbaImage, String> {
        self.check()?;
        let enc = |s: &str| -> String { s.bytes().map(|b| if b.is_ascii_alphanumeric() || b"-_.~".contains(&b) { (b as char).to_string() } else { format!("%{b:02X}") }).collect() };
        let path = format!("/view?filename={}&subfolder={}&type={}", enc(&m.filename), enc(&m.subfolder), enc(&m.kind));
        let r = self.agent(60).get(&format!("{}{path}", self.url)).call().map_err(|_| "The picture could not be fetched from ComfyUI.".to_string())?;
        let mut bytes = Vec::new();
        std::io::Read::read_to_end(&mut r.into_reader().take(256 << 20), &mut bytes).map_err(|e| e.to_string())?;
        image::load_from_memory(&bytes).map(|i| i.to_rgba8()).map_err(|_| "What ComfyUI sent is not a picture.".to_string())
    }

    /// The whole way: fill, queue, wait, fetch the first picture.
    pub fn make(&self, template: &str, f: &Fill) -> Result<image::RgbaImage, String> {
        let wf = fill_workflow(template, f)?;
        let id = self.queue(&wf)?;
        let made = self.wait(&id, Duration::from_secs(20 * 60), Duration::from_millis(700))?;
        self.fetch(&made[0])
    }
}

use std::io::Read as _;

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Read, Write};
    use std::net::TcpListener;

    fn fill() -> Fill {
        Fill { prompt: "a fox in snow".into(), negative: "blurry".into(), checkpoint: "model.safetensors".into(), seed: 42, width: 512, height: 768 }
    }

    #[test]
    fn the_places_of_a_workflow_are_filled_with_numbers_where_a_number_belongs() {
        let wf = fill_workflow(DEFAULT_WORKFLOW, &fill()).unwrap();
        assert_eq!(wf["3"]["inputs"]["seed"], 42);
        assert_eq!(wf["5"]["inputs"]["width"], 512);
        assert_eq!(wf["5"]["inputs"]["height"], 768);
        assert_eq!(wf["6"]["inputs"]["text"], "a fox in snow");
        assert_eq!(wf["7"]["inputs"]["text"], "blurry");
        assert_eq!(wf["4"]["inputs"]["ckpt_name"], "model.safetensors");
        // links between nodes are left alone
        assert_eq!(wf["3"]["inputs"]["model"], json!(["4", 0]));
        // words inside a longer text, and a workflow that is not one
        let wf = fill_workflow(r#"{"1": {"class_type": "X", "inputs": {"text": "masterpiece, {{prompt}}, 4k"}}}"#, &fill()).unwrap();
        assert_eq!(wf["1"]["inputs"]["text"], "masterpiece, a fox in snow, 4k");
        assert!(fill_workflow("not json", &fill()).is_err());
        assert!(fill_workflow("[1, 2]", &fill()).is_err());
    }

    /// A small program that answers like ComfyUI: a job takes two questions before it is done, and the picture is a 2 x 2 PNG.
    fn fake_comfy(listener: TcpListener, seen: std::sync::Arc<std::sync::Mutex<Vec<String>>>) {
        std::thread::spawn(move || {
            let mut asked = 0;
            for stream in listener.incoming().flatten().take(12) {
                let mut s = stream;
                let mut buf = vec![0u8; 65536];
                let mut n = 0;
                loop {
                    let k = s.read(&mut buf[n..]).unwrap_or(0);
                    if k == 0 {
                        break;
                    }
                    n += k;
                    let text = String::from_utf8_lossy(&buf[..n]).to_string();
                    if let Some(p) = text.find("\r\n\r\n") {
                        let want: usize = text[..p].to_lowercase().lines().find_map(|l| l.strip_prefix("content-length:").and_then(|v| v.trim().parse().ok())).unwrap_or(0);
                        if n >= p + 4 + want {
                            break;
                        }
                    }
                }
                let text = String::from_utf8_lossy(&buf[..n]).to_string();
                seen.lock().unwrap().push(text.clone());
                let first = text.lines().next().unwrap_or("").to_string();
                let png = {
                    let img = image::RgbaImage::from_pixel(2, 2, image::Rgba([10, 200, 30, 255]));
                    let mut c = std::io::Cursor::new(Vec::new());
                    img.write_to(&mut c, image::ImageFormat::Png).unwrap();
                    c.into_inner()
                };
                let (status, ctype, body): (&str, &str, Vec<u8>) = if first.contains("/object_info/CheckpointLoaderSimple") {
                    ("200 OK", "application/json", json!({"CheckpointLoaderSimple": {"input": {"required": {"ckpt_name": [["a.safetensors", "b.ckpt"]]}}}}).to_string().into_bytes())
                } else if first.starts_with("POST /prompt") {
                    if text.contains("missing.safetensors") {
                        ("400 Bad Request", "application/json", json!({"error": {"message": "Value not in list: ckpt_name"}}).to_string().into_bytes())
                    } else {
                        ("200 OK", "application/json", json!({"prompt_id": "job1"}).to_string().into_bytes())
                    }
                } else if first.contains("/history/job1") {
                    asked += 1;
                    let v = if asked < 2 { json!({}) } else { json!({"job1": {"outputs": {"9": {"images": [{"filename": "out 1.png", "subfolder": "", "type": "output"}]}}, "status": {"status_str": "success"}}}) };
                    ("200 OK", "application/json", v.to_string().into_bytes())
                } else if first.contains("/view?filename=out%201.png") {
                    ("200 OK", "image/png", png)
                } else {
                    ("404 Not Found", "application/json", b"{}".to_vec())
                };
                let head = format!("HTTP/1.1 {status}\r\nContent-Type: {ctype}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n", body.len());
                let _ = s.write_all(head.as_bytes());
                let _ = s.write_all(&body);
            }
        });
    }

    #[test]
    fn a_picture_is_asked_for_waited_for_and_fetched_from_a_comfyui() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let seen = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
        fake_comfy(listener, seen.clone());
        let c = Comfy::new(&format!("http://127.0.0.1:{port}/"));
        assert_eq!(c.checkpoints().unwrap(), vec!["a.safetensors".to_string(), "b.ckpt".to_string()]);
        let wf = fill_workflow(DEFAULT_WORKFLOW, &fill()).unwrap();
        let id = c.queue(&wf).unwrap();
        assert_eq!(id, "job1");
        let made = c.wait(&id, Duration::from_secs(5), Duration::from_millis(10)).unwrap();
        assert_eq!(made, vec![Made { filename: "out 1.png".into(), subfolder: String::new(), kind: "output".into() }]);
        let img = c.fetch(&made[0]).unwrap();
        assert_eq!((img.width(), img.height()), (2, 2));
        // what was sent: the filled graph
        let sent = seen.lock().unwrap().iter().find(|t| t.starts_with("POST /prompt")).cloned().unwrap();
        assert!(sent.contains("a fox in snow") && sent.contains("\"client_id\":\"seviathstudio\"") && sent.contains("\"seed\":42"), "{sent}");
    }

    #[test]
    fn a_refused_workflow_and_a_missing_server_are_said_in_words() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        fake_comfy(listener, Default::default());
        let c = Comfy::new(&format!("http://127.0.0.1:{port}"));
        let bad = Fill { checkpoint: "missing.safetensors".into(), ..fill() };
        let err = c.make(DEFAULT_WORKFLOW, &bad).unwrap_err();
        assert!(err.contains("did not accept the workflow") && err.contains("Value not in list"), "{err}");
        let nothing = Comfy::new(&format!("http://127.0.0.1:{}", TcpListener::bind("127.0.0.1:0").unwrap().local_addr().unwrap().port()));
        assert!(nothing.checkpoints().unwrap_err().contains("Could not reach ComfyUI"));
        assert!(Comfy::new("").checkpoints().unwrap_err().contains("no address"));
    }

    #[test]
    fn a_whole_make_gives_the_picture() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        fake_comfy(listener, Default::default());
        let img = Comfy::new(&format!("http://127.0.0.1:{port}")).make(DEFAULT_WORKFLOW, &fill()).unwrap();
        assert_eq!(img.get_pixel(0, 0).0, [10, 200, 30, 255]);
    }
}
