// SPDX-License-Identifier: GPL-3.0-or-later
// Showing the model in other programs. The *browser source* is a tiny web server on this computer: OBS (or any browser) loads a page
// from it, and the page keeps asking for the newest picture of the model, a PNG with a see-through background, so the model appears
// over a stream without a green screen. Nothing leaves the computer: the address is localhost.

use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

const PAGE: &str = r#"<!doctype html>
<html><head><meta charset="utf-8"><title>SeviathStudio model</title>
<style>html,body{margin:0;height:100%;background:transparent;overflow:hidden}img{width:100%;height:100%;object-fit:contain;display:block}</style></head>
<body><img id="m" alt="">
<script>
let n = 0;
async function tick() {
  try {
    const r = await fetch('/frame.png?' + (n++), {cache: 'no-store'});
    if (r.ok) { const b = await r.blob(); const u = URL.createObjectURL(b); const i = document.getElementById('m'); const old = i.src; i.src = u; if (old.startsWith('blob:')) URL.revokeObjectURL(old); }
    setTimeout(tick, 16);
  } catch (e) { setTimeout(tick, 500); }
}
tick();
</script></body></html>"#;

struct Shared {
    frame: Mutex<Vec<u8>>,
    /// How many pictures were asked for, and how many connections came.
    served: AtomicU64,
    stop: AtomicBool,
}

/// The browser source server. It stops when it is dropped.
pub struct BrowserSource {
    pub port: u16,
    shared: Arc<Shared>,
    thread: Option<std::thread::JoinHandle<()>>,
}

fn respond(mut s: TcpStream, shared: &Shared) {
    let _ = s.set_read_timeout(Some(Duration::from_millis(500)));
    let mut buf = [0u8; 2048];
    let n = s.read(&mut buf).unwrap_or(0);
    let req = String::from_utf8_lossy(&buf[..n]);
    let path = req.lines().next().and_then(|l| l.split_whitespace().nth(1)).unwrap_or("/");
    let path = path.split('?').next().unwrap_or("/");
    let (status, ctype, body): (&str, &str, Vec<u8>) = match path {
        "/" | "/index.html" => ("200 OK", "text/html; charset=utf-8", PAGE.as_bytes().to_vec()),
        "/frame.png" => {
            let f = shared.frame.lock().unwrap().clone();
            if f.is_empty() {
                ("404 Not Found", "text/plain", b"no picture yet".to_vec())
            } else {
                shared.served.fetch_add(1, Ordering::Relaxed);
                ("200 OK", "image/png", f)
            }
        }
        _ => ("404 Not Found", "text/plain", b"not found".to_vec()),
    };
    let head = format!("HTTP/1.1 {status}\r\nContent-Type: {ctype}\r\nContent-Length: {}\r\nCache-Control: no-store\r\nAccess-Control-Allow-Origin: *\r\nConnection: close\r\n\r\n", body.len());
    let _ = s.write_all(head.as_bytes());
    let _ = s.write_all(&body);
}

impl BrowserSource {
    /// Start serving on `port` (on this computer only).
    pub fn start(port: u16) -> Result<BrowserSource, String> {
        let listener = TcpListener::bind(("127.0.0.1", port)).map_err(|e| format!("Could not open port {port}: {e}"))?;
        listener.set_nonblocking(true).map_err(|e| e.to_string())?;
        let shared = Arc::new(Shared { frame: Mutex::new(Vec::new()), served: AtomicU64::new(0), stop: AtomicBool::new(false) });
        let s2 = shared.clone();
        let thread = std::thread::spawn(move || {
            while !s2.stop.load(Ordering::Relaxed) {
                match listener.accept() {
                    Ok((stream, _)) => {
                        let s3 = s2.clone();
                        std::thread::spawn(move || respond(stream, &s3));
                    }
                    Err(_) => std::thread::sleep(Duration::from_millis(8)),
                }
            }
        });
        Ok(BrowserSource { port, shared, thread: Some(thread) })
    }

    /// The newest picture of the model (the bytes of a PNG).
    pub fn set_frame(&self, png: Vec<u8>) {
        *self.shared.frame.lock().unwrap() = png;
    }

    pub fn served(&self) -> u64 {
        self.shared.served.load(Ordering::Relaxed)
    }

    pub fn url(&self) -> String {
        format!("http://localhost:{}/", self.port)
    }
}

impl Drop for BrowserSource {
    fn drop(&mut self) {
        self.shared.stop.store(true, Ordering::Relaxed);
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}

/// A picture as PNG bytes, fast (the pictures go out many times a second).
pub fn png_bytes(img: &image::RgbaImage) -> Vec<u8> {
    use image::ImageEncoder;
    let mut out = Vec::new();
    let enc = image::codecs::png::PngEncoder::new_with_quality(&mut out, image::codecs::png::CompressionType::Fast, image::codecs::png::FilterType::NoFilter);
    let _ = enc.write_image(img.as_raw(), img.width(), img.height(), image::ExtendedColorType::Rgba8);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn get(port: u16, path: &str) -> (String, Vec<u8>) {
        let mut s = TcpStream::connect(("127.0.0.1", port)).unwrap();
        s.set_read_timeout(Some(Duration::from_secs(3))).unwrap();
        s.write_all(format!("GET {path} HTTP/1.1\r\nHost: localhost\r\n\r\n").as_bytes()).unwrap();
        let mut all = Vec::new();
        let _ = s.read_to_end(&mut all);
        let split = all.windows(4).position(|w| w == b"\r\n\r\n").unwrap_or(0);
        (String::from_utf8_lossy(&all[..split]).to_string(), all[(split + 4).min(all.len())..].to_vec())
    }

    #[test]
    fn the_browser_source_serves_its_page_and_the_newest_see_through_picture() {
        let Ok(src) = BrowserSource::start(48_732) else { return };
        let (head, body) = get(src.port, "/");
        assert!(head.starts_with("HTTP/1.1 200") && head.contains("text/html"));
        assert!(String::from_utf8_lossy(&body).contains("/frame.png"), "the page asks for pictures");
        // no picture yet
        assert!(get(src.port, "/frame.png").0.starts_with("HTTP/1.1 404"));
        let mut img = image::RgbaImage::new(8, 6);
        img.put_pixel(3, 2, image::Rgba([200, 10, 10, 255]));
        src.set_frame(png_bytes(&img));
        let (head, body) = get(src.port, "/frame.png?5");
        assert!(head.contains("image/png") && head.contains("no-store"));
        let back = image::load_from_memory(&body).unwrap().to_rgba8();
        assert_eq!(back.dimensions(), (8, 6));
        assert_eq!(back.get_pixel(3, 2).0, [200, 10, 10, 255]);
        assert_eq!(back.get_pixel(0, 0).0[3], 0, "the background stays see-through");
        assert_eq!(src.served(), 1);
        // the newest picture replaces the old one
        img.put_pixel(3, 2, image::Rgba([10, 10, 200, 255]));
        src.set_frame(png_bytes(&img));
        let back = image::load_from_memory(&get(src.port, "/frame.png").1).unwrap().to_rgba8();
        assert_eq!(back.get_pixel(3, 2).0, [10, 10, 200, 255]);
        assert!(get(src.port, "/other").0.starts_with("HTTP/1.1 404"));
        assert_eq!(src.url(), "http://localhost:48732/");
        // a second server cannot take the same port; after the first is dropped it can
        assert!(BrowserSource::start(48_732).is_err());
        drop(src);
        assert!(BrowserSource::start(48_732).is_ok());
    }
}
