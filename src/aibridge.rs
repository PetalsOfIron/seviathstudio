// SPDX-License-Identifier: GPL-3.0-or-later
// The AI bridge: the program does not contain an AI, it talks to one the user already has. A provider is a program on this computer
// (Ollama) or a service with a free key (Google's Gemini through its OpenAI-compatible address, or any other OpenAI-compatible server).
// Any number can be set up; one is used at a time. Everything the AI says is checked before it is used: a brush is read into the
// brush settings and clamped to what they allow, colours are read as hex numbers, and nothing the AI writes is run.

use std::time::Duration;

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::brush::BrushSettings;

#[derive(Clone, Copy, PartialEq, Eq, Debug, Default, Serialize, Deserialize)]
pub enum Kind {
    /// Ollama on this computer or the network (port 11434).
    #[default]
    Ollama,
    /// Anything that speaks the OpenAI chat protocol (Gemini's compatible address, LM Studio, llama.cpp server, OpenRouter...).
    OpenAi,
}

#[derive(Clone, PartialEq, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Provider {
    pub name: String,
    pub kind: Kind,
    /// The base address, like `http://localhost:11434`.
    pub url: String,
    pub model: String,
    /// The key, kept in the settings file on this computer. Empty for a program on this computer.
    pub key: String,
}

impl Provider {
    pub fn ollama() -> Provider {
        Provider { name: "Ollama (this computer)".into(), kind: Kind::Ollama, url: "http://localhost:11434".into(), model: "llama3.2".into(), key: String::new() }
    }

    pub fn gemini() -> Provider {
        Provider { name: "Gemini (free key)".into(), kind: Kind::OpenAi, url: "https://generativelanguage.googleapis.com/v1beta/openai".into(), model: "gemini-2.0-flash".into(), key: String::new() }
    }

    pub fn openai_compatible() -> Provider {
        Provider { name: "Other server".into(), kind: Kind::OpenAi, url: "http://localhost:1234/v1".into(), model: "local-model".into(), key: String::new() }
    }

    fn endpoint(&self) -> String {
        let base = self.url.trim().trim_end_matches('/');
        match self.kind {
            Kind::Ollama => format!("{base}/api/chat"),
            Kind::OpenAi => {
                if base.ends_with("/chat/completions") {
                    base.to_string()
                } else {
                    format!("{base}/chat/completions")
                }
            }
        }
    }

    /// The request body for a question, with pictures (PNG bytes) when the model can look at them.
    pub fn request_body(&self, system: &str, user: &str, images: &[Vec<u8>]) -> Value {
        match self.kind {
            Kind::Ollama => {
                let mut msg = json!({ "role": "user", "content": user });
                if !images.is_empty() {
                    msg["images"] = json!(images.iter().map(|i| crate::obs::b64(i)).collect::<Vec<_>>());
                }
                json!({ "model": self.model, "stream": false, "messages": [{ "role": "system", "content": system }, msg] })
            }
            Kind::OpenAi => {
                let content = if images.is_empty() {
                    json!(user)
                } else {
                    let mut parts = vec![json!({ "type": "text", "text": user })];
                    parts.extend(images.iter().map(|i| json!({ "type": "image_url", "image_url": { "url": format!("data:image/png;base64,{}", crate::obs::b64(i)) } })));
                    json!(parts)
                };
                json!({ "model": self.model, "messages": [{ "role": "system", "content": system }, { "role": "user", "content": content }] })
            }
        }
    }

    /// What the AI said, from its answer.
    pub fn reply_text(&self, answer: &Value) -> Option<String> {
        match self.kind {
            Kind::Ollama => answer["message"]["content"].as_str().map(str::to_string),
            Kind::OpenAi => answer["choices"][0]["message"]["content"].as_str().map(str::to_string),
        }
    }

    /// Ask a question and wait for the answer (call it from a thread of its own).
    pub fn chat(&self, system: &str, user: &str, images: &[Vec<u8>]) -> Result<String, String> {
        if self.url.trim().is_empty() || self.model.trim().is_empty() {
            return Err("This AI has no address or no model name yet.".into());
        }
        let mut req = ureq::post(&self.endpoint()).timeout(Duration::from_secs(180)).set("Content-Type", "application/json");
        if !self.key.trim().is_empty() {
            req = req.set("Authorization", &format!("Bearer {}", self.key.trim()));
        }
        let answer: Value = match req.send_json(self.request_body(system, user, images)) {
            Ok(r) => r.into_json().map_err(|e| format!("The answer could not be read: {e}"))?,
            Err(ureq::Error::Status(code, r)) => {
                let body = r.into_string().unwrap_or_default();
                let short: String = body.chars().take(200).collect();
                return Err(match code {
                    401 | 403 => "The AI did not accept the key.".to_string(),
                    404 => format!("The AI does not know that address or model ({short})."),
                    429 => "The free limit of this AI is used up for now. Wait a bit, or add another AI.".to_string(),
                    _ => format!("The AI answered with an error {code}: {short}"),
                });
            }
            Err(e) => return Err(format!("Could not reach the AI at {} ({e}). Is it running?", self.url)),
        };
        self.reply_text(&answer).ok_or_else(|| "The AI answered, but with nothing I can read.".to_string())
    }
}

/// The first JSON value (object or list) in a text, also when the AI wrapped it in words or a code fence.
pub fn extract_json(text: &str) -> Option<Value> {
    for (i, c) in text.char_indices() {
        if c == '{' || c == '[' {
            let mut it = serde_json::Deserializer::from_str(&text[i..]).into_iter::<Value>();
            if let Some(Ok(v)) = it.next() {
                return Some(v);
            }
        }
    }
    None
}

/// Colours written as #RRGGBB in a text, in order, without doubles.
pub fn colors_from(text: &str) -> Vec<[u8; 3]> {
    let b = text.as_bytes();
    let mut out: Vec<[u8; 3]> = Vec::new();
    let mut i = 0;
    while i + 7 <= b.len() {
        if b[i] == b'#' {
            if let Some(hex) = text.get(i + 1..i + 7).filter(|h| h.chars().all(|c| c.is_ascii_hexdigit())) {
                let v = u32::from_str_radix(hex, 16).unwrap_or(0);
                let c = [(v >> 16) as u8, (v >> 8) as u8, v as u8];
                if !out.contains(&c) {
                    out.push(c);
                }
                i += 7;
                continue;
            }
        }
        i += 1;
    }
    out.truncate(16);
    out
}

/// A brush from what the AI wrote: the fields it named are taken, the rest are the defaults, and everything is brought inside what the
/// brush allows.
pub fn brush_from(text: &str) -> Result<BrushSettings, String> {
    let v = extract_json(text).ok_or("The AI did not write a brush I can read.")?;
    let obj = match &v {
        Value::Object(_) => v.clone(),
        Value::Array(a) => a.iter().find(|x| x.is_object()).cloned().ok_or("The AI did not write a brush I can read.")?,
        _ => return Err("The AI did not write a brush I can read.".into()),
    };
    // fields of the wrong kind are dropped one by one, so one wrong number does not lose the whole brush
    let mut base = serde_json::to_value(BrushSettings::default()).map_err(|e| e.to_string())?;
    let mut taken = 0;
    if let (Some(b), Some(o)) = (base.as_object_mut(), obj.as_object()) {
        for (k, val) in o {
            if let Some(old) = b.get(k) {
                let same = std::mem::discriminant(old) == std::mem::discriminant(val) || (old.is_number() && val.is_number());
                if same {
                    b.insert(k.clone(), val.clone());
                    taken += 1;
                }
            }
        }
    }
    if taken == 0 {
        return Err("The AI wrote a brush with none of the settings I know.".into());
    }
    let s: BrushSettings = serde_json::from_value(base).map_err(|e| format!("The brush could not be read: {e}"))?;
    Ok(s.sanitized())
}

/// What the AI is told when it designs a brush: the settings that exist, with their default values.
pub fn brush_system_prompt() -> String {
    let fields = serde_json::to_string(&BrushSettings::default()).unwrap_or_default();
    format!("You design brushes for a digital painting program. Reply with one JSON object and nothing else. The object may only use these settings, with values of the same kind as the defaults shown (sizes are in pixels, most others run from 0 to 1, booleans are true or false). Change only what the description needs and leave the rest out. Defaults: {fields}")
}

pub const COLORS_SYSTEM: &str = "You choose colours for artists. Reply with a JSON list of 6 colours written as \"#RRGGBB\" strings and nothing else.";

pub const CHECK_SYSTEM: &str = "You are a kind, exact art teacher. Look at the drawing and list the real problems you can see: anatomy and proportions, perspective and horizon, light direction and shadows, composition, colour and contrast, and anything that looks unfinished. Give at most eight short numbered points, each with how to fix it. If the drawing is good in some way, say so first in one sentence.";

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Read, Write};
    use std::net::TcpListener;

    #[test]
    fn json_and_colours_are_found_in_chatty_answers() {
        let t = "Sure! Here is your brush:\n```json\n{\"size\": 12.5, \"flow\": 0.5}\n```\nEnjoy.";
        assert_eq!(extract_json(t).unwrap()["size"], 12.5);
        assert_eq!(extract_json("a list [1, 2] there").unwrap(), json!([1, 2]));
        assert!(extract_json("no json here {oops").is_none());
        assert_eq!(colors_from("warm: #FFAA00, #ffaa00 and #112233; not #12345 or #GGGGGG"), [[255, 170, 0], [17, 34, 51]]);
        assert!(colors_from("").is_empty());
    }

    #[test]
    fn a_brush_from_the_ai_is_taken_field_by_field_and_clamped() {
        let d = BrushSettings::default();
        let b = brush_from("{\"size\": 40, \"opacity\": 0.3, \"nonsense\": 5, \"hardness\": \"very\"}").unwrap();
        assert_eq!(b.size, 40.0);
        assert!((b.opacity - 0.3).abs() < 1e-6);
        assert_eq!(b.hardness, d.hardness, "a field of the wrong kind is dropped, the others stay");
        // absurd numbers are brought inside
        let big = brush_from("{\"size\": 99999999, \"opacity\": 7}").unwrap();
        assert!(big.size < 99999999.0 && big.opacity <= 1.0, "{} {}", big.size, big.opacity);
        // a list holding a brush works; words alone do not
        assert!(brush_from("[{\"size\": 10}]").is_ok());
        assert!(brush_from("I cannot do that").is_err());
        assert!(brush_from("{\"colour\": 1}").unwrap_err().contains("none of the settings"));
        assert!(brush_system_prompt().contains("\"size\""));
    }

    #[test]
    fn the_two_kinds_of_provider_write_and_read_their_own_protocol() {
        let o = Provider::ollama();
        assert_eq!(o.endpoint(), "http://localhost:11434/api/chat");
        let body = o.request_body("sys", "hi", &[vec![1, 2, 3]]);
        assert_eq!(body["stream"], false);
        assert_eq!(body["messages"][1]["images"][0], "AQID");
        assert_eq!(o.reply_text(&json!({"message": {"content": "ok"}})).as_deref(), Some("ok"));
        let g = Provider::gemini();
        assert_eq!(g.endpoint(), "https://generativelanguage.googleapis.com/v1beta/openai/chat/completions");
        let body = g.request_body("sys", "hi", &[vec![1, 2, 3]]);
        assert_eq!(body["messages"][1]["content"][1]["image_url"]["url"], "data:image/png;base64,AQID");
        assert_eq!(g.request_body("s", "hi", &[])["messages"][1]["content"], "hi");
        assert_eq!(g.reply_text(&json!({"choices": [{"message": {"content": "yes"}}]})).as_deref(), Some("yes"));
        let other = Provider { url: "http://x/v1/chat/completions".into(), ..Provider::openai_compatible() };
        assert_eq!(other.endpoint(), "http://x/v1/chat/completions");
    }

    /// A small program that answers like Ollama and like an OpenAI server, and remembers what it was sent.
    fn fake_ai(listener: TcpListener, seen: std::sync::Arc<std::sync::Mutex<Vec<String>>>) {
        std::thread::spawn(move || {
            for stream in listener.incoming().flatten().take(6) {
                let mut s = stream;
                let mut buf = vec![0u8; 65536];
                let mut n = 0;
                let mut want = 0usize;
                loop {
                    let k = s.read(&mut buf[n..]).unwrap_or(0);
                    if k == 0 {
                        break;
                    }
                    n += k;
                    let text = String::from_utf8_lossy(&buf[..n]).to_string();
                    if let Some(p) = text.find("\r\n\r\n") {
                        let head = text[..p].to_lowercase();
                        want = head.lines().find_map(|l| l.strip_prefix("content-length:").and_then(|v| v.trim().parse().ok())).unwrap_or(0);
                        if n >= p + 4 + want {
                            break;
                        }
                    }
                }
                let text = String::from_utf8_lossy(&buf[..n]).to_string();
                seen.lock().unwrap().push(text.clone());
                let first = text.lines().next().unwrap_or("").to_string();
                let (status, body) = if text.to_lowercase().contains("authorization: bearer wrong") {
                    ("401 Unauthorized", "{}".to_string())
                } else if first.contains("/api/chat") {
                    ("200 OK", json!({"message": {"role": "assistant", "content": "{\"size\": 33}"}}).to_string())
                } else if first.contains("/chat/completions") {
                    ("200 OK", json!({"choices": [{"message": {"content": "#FF0000 #00FF00"}}]}).to_string())
                } else {
                    ("404 Not Found", "{}".to_string())
                };
                let r = format!("HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len());
                let _ = s.write_all(r.as_bytes());
            }
        });
    }

    #[test]
    fn it_asks_a_program_on_this_computer_and_reads_the_answer() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let seen = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
        fake_ai(listener, seen.clone());
        let o = Provider { url: format!("http://127.0.0.1:{port}"), ..Provider::ollama() };
        let reply = o.chat("system words", "make a thick brush", &[]).unwrap();
        assert_eq!(brush_from(&reply).unwrap().size, 33.0);
        let req = seen.lock().unwrap()[0].clone();
        assert!(req.starts_with("POST /api/chat") && req.contains("make a thick brush") && req.contains("system words") && req.contains("\"llama3.2\""), "{req}");
        // an OpenAI style server with a key
        let g = Provider { url: format!("http://127.0.0.1:{port}/v1"), key: "secret".into(), ..Provider::openai_compatible() };
        let reply = g.chat("s", "colours please", &[]).unwrap();
        assert_eq!(colors_from(&reply).len(), 2);
        assert!(seen.lock().unwrap()[1].to_lowercase().contains("authorization: bearer secret"));
        // a refused key and an address that is not there are said in words
        let bad = Provider { key: "wrong".into(), ..g.clone() };
        assert!(bad.chat("s", "x", &[]).unwrap_err().contains("did not accept the key"));
        let nothing = Provider { url: format!("http://127.0.0.1:{}", TcpListener::bind("127.0.0.1:0").unwrap().local_addr().unwrap().port()), ..Provider::ollama() };
        assert!(nothing.chat("s", "x", &[]).unwrap_err().contains("Could not reach the AI"));
        assert!(Provider::default().chat("s", "x", &[]).unwrap_err().contains("no address"));
    }
}
