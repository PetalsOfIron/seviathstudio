// SPDX-License-Identifier: GPL-3.0-or-later
// The session recorder. It films the picture itself (all layers and the paper, as the finished picture
// looks), never the app window: no panels, no cursor. Timelapse adds one frame after every finished
// change, real time takes frames at a steady rate. Frames are drawn and encoded to H.264 on a background
// thread and written straight into an .mp4 file, so a long session never fills the memory.

use std::io::BufWriter;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::mpsc::{self, SyncSender, TrySendError};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;

use openh264::OpenH264API;
use openh264::encoder::{BitRate, Encoder, EncoderConfig, FrameRate, IntraFramePeriod, RateControlMode, UsageType};
use openh264::formats::{RgbSliceU8, YUVBuffer};

use crate::doc::Document;
use crate::history::DocState;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum RecMode {
    /// One frame after every finished change: hours become a short clip.
    Timelapse,
    /// Frames at a steady rate while you draw.
    RealTime,
}

pub const QUALITIES: [(u32, &str); 4] = [(720, "720p (small file)"), (1080, "1080p (Full HD)"), (1440, "1440p (2K)"), (2160, "2160p (4K)")];

#[derive(Clone, Copy, Debug)]
pub struct RecConfig {
    pub mode: RecMode,
    /// The longest side of the video, pixels.
    pub max_side: u32,
    /// Timelapse: frames per second of the finished video. Real time: frames taken per second.
    pub fps: u32,
}

impl Default for RecConfig {
    fn default() -> Self {
        Self { mode: RecMode::Timelapse, max_side: 1080, fps: 12 }
    }
}

enum Msg {
    Frame(Box<(u32, u32, DocState)>),
    /// The picture did not change: the last frame again.
    Repeat,
    Finish,
}

#[derive(Default)]
struct Shared {
    frames: AtomicU32,
    dropped: AtomicU32,
    error: Mutex<Option<String>>,
}

/// A recording in progress.
pub struct Session {
    tx: SyncSender<Msg>,
    handle: Option<JoinHandle<()>>,
    shared: Arc<Shared>,
    pub cfg: RecConfig,
    pub path: PathBuf,
    pub paused: bool,
    /// Seconds spent recording (pauses are not counted).
    pub elapsed: f64,
    last_tick: Option<f64>,
    /// Real time: the document revision of the last frame.
    last_rev: u64,
    next_frame_at: f64,
}

pub struct Finished {
    pub path: PathBuf,
    pub frames: u32,
    pub dropped: u32,
    pub seconds: f32,
}

/// The size of the video for a picture: fits `max_side`, never bigger than the picture, both sides even.
pub fn video_size(w: u32, h: u32, max_side: u32) -> (u32, u32) {
    let k = (max_side as f32 / w.max(h).max(1) as f32).min(1.0);
    let even = |v: f32| ((v.round() as u32).max(16) / 2) * 2;
    (even(w as f32 * k), even(h as f32 * k))
}

fn temp_path() -> PathBuf {
    static N: AtomicU32 = AtomicU32::new(0);
    std::env::temp_dir().join(format!("seviath_recording_{}_{}.mp4", std::process::id(), N.fetch_add(1, Ordering::Relaxed)))
}

/// The picture of a state as plain RGB, fitted into `out` on white (the video never has see-through parts).
fn frame_rgb(w: u32, h: u32, s: DocState, out: (u32, u32)) -> Vec<u8> {
    let mut d = Document::from_parts(w, h, s.paper, s.layers);
    d.filter_presets = s.presets;
    d.meta = s.meta;
    let img = crate::files::flatten(&d);
    let (ow, oh) = out;
    let fitted = if (img.width(), img.height()) == out {
        img
    } else {
        let k = (ow as f32 / img.width() as f32).min(oh as f32 / img.height() as f32);
        let (nw, nh) = (((img.width() as f32 * k).round() as u32).clamp(1, ow), ((img.height() as f32 * k).round() as u32).clamp(1, oh));
        let small = image::imageops::resize(&img, nw, nh, image::imageops::FilterType::Triangle);
        let mut canvas = image::RgbaImage::from_pixel(ow, oh, image::Rgba([255, 255, 255, 0]));
        image::imageops::replace(&mut canvas, &small, ((ow - nw) / 2) as i64, ((oh - nh) / 2) as i64);
        canvas
    };
    let mut rgb = Vec::with_capacity((ow * oh * 3) as usize);
    for p in fitted.pixels() {
        let a = p[3] as u32;
        for c in &p.0[..3] {
            rgb.push(((*c as u32 * a + 255 * (255 - a) + 127) / 255) as u8);
        }
    }
    rgb
}

pub(crate) struct Writer {
    enc: Encoder,
    mux: Box<minimp4::Mp4Muxer<BufWriter<std::fs::File>>>,
    size: (u32, u32),
    fps: u32,
    last: Option<Vec<u8>>,
}

impl Writer {
    pub(crate) fn new(path: &Path, size: (u32, u32), fps: u32) -> Result<Self, String> {
        let file = std::fs::File::create(path).map_err(|e| format!("Can't make the video file: {e}"))?;
        let bps = ((size.0 as f64 * size.1 as f64 * fps as f64 * 0.12) as u32).clamp(600_000, 60_000_000);
        let cfg = EncoderConfig::new()
            .usage_type(UsageType::ScreenContentRealTime)
            .rate_control_mode(RateControlMode::Quality)
            .bitrate(BitRate::from_bps(bps))
            .max_frame_rate(FrameRate::from_hz(fps as f32))
            .skip_frames(false)
            .intra_frame_period(IntraFramePeriod::from_num_frames(fps * 2));
        let enc = Encoder::with_api_config(OpenH264API::from_source(), cfg).map_err(|e| format!("The video encoder didn't start: {e}"))?;
        // the muxer keeps a pointer to itself, so it must stay where it is
        let mut mux = Box::new(minimp4::Mp4Muxer::new(BufWriter::new(file)));
        mux.init_video(size.0 as i32, size.1 as i32, false, "SeviathStudio");
        mux.write_comment("Drawn in SeviathStudio");
        Ok(Self { enc, mux, size, fps, last: None })
    }

    pub(crate) fn write(&mut self, rgb: Vec<u8>) -> Result<(), String> {
        let yuv = YUVBuffer::from_rgb_source(RgbSliceU8::new(&rgb, (self.size.0 as usize, self.size.1 as usize)));
        let bits = self.enc.encode(&yuv).map_err(|e| format!("Couldn't encode a frame: {e}"))?;
        let data = bits.to_vec();
        if !data.is_empty() {
            self.mux.write_video_with_fps(&data, self.fps);
        }
        self.last = Some(rgb);
        Ok(())
    }

    pub(crate) fn finish(self) {
        self.mux.close();
    }
}

fn work(rx: mpsc::Receiver<Msg>, mut w: Writer, shared: Arc<Shared>) {
    let mut failed = false;
    for m in rx {
        if failed {
            continue;
        }
        let r = match m {
            Msg::Frame(b) => {
                let (dw, dh, s) = *b;
                let rgb = frame_rgb(dw, dh, s, w.size);
                w.write(rgb)
            }
            Msg::Repeat => match w.last.clone() {
                Some(l) => w.write(l),
                None => Ok(()),
            },
            Msg::Finish => break,
        };
        match r {
            Ok(()) => {
                shared.frames.fetch_add(1, Ordering::Relaxed);
            }
            Err(e) => {
                *shared.error.lock().unwrap() = Some(e);
                failed = true;
            }
        }
    }
    w.finish();
}

impl Session {
    /// Start a recording of a picture of this size. `first` is the picture as it is now.
    pub fn start(cfg: RecConfig, w: u32, h: u32, first: DocState, now: f64) -> Result<Session, String> {
        let size = video_size(w, h, cfg.max_side);
        let path = temp_path();
        let shared = Arc::new(Shared::default());
        let (tx, rx) = mpsc::sync_channel::<Msg>(12);
        let (ready_tx, ready_rx) = mpsc::channel::<Result<(), String>>();
        let (sh, p2, fps) = (shared.clone(), path.clone(), cfg.fps.clamp(1, 60));
        // the muxer is made on its own thread (it can't be moved between threads)
        let handle = std::thread::Builder::new()
            .name("recorder".into())
            .spawn(move || match Writer::new(&p2, size, fps) {
                Ok(w) => {
                    let _ = ready_tx.send(Ok(()));
                    work(rx, w, sh)
                }
                Err(e) => {
                    let _ = ready_tx.send(Err(e));
                }
            })
            .map_err(|e| e.to_string())?;
        ready_rx.recv().map_err(|_| "The recorder didn't start.".to_string())??;
        let mut s = Session { tx, handle: Some(handle), shared, cfg, path, paused: false, elapsed: 0.0, last_tick: Some(now), last_rev: u64::MAX, next_frame_at: now };
        s.push(w, h, first);
        Ok(s)
    }

    /// Add a frame (the timelapse calls this after every finished change).
    pub fn push(&mut self, w: u32, h: u32, state: DocState) {
        if self.paused {
            return;
        }
        match self.tx.try_send(Msg::Frame(Box::new((w, h, state)))) {
            Err(TrySendError::Full(_)) => {
                self.shared.dropped.fetch_add(1, Ordering::Relaxed);
            }
            _ => {}
        }
    }

    /// Real time: call every frame of the app. Takes a frame when it is time; a new picture when
    /// something changed (`rev`), otherwise the last one again.
    pub fn tick(&mut self, now: f64, rev: u64, snapshot: impl FnOnce() -> (u32, u32, DocState)) {
        if let Some(t) = self.last_tick.replace(now) {
            if !self.paused {
                self.elapsed += (now - t).clamp(0.0, 1.0);
            }
        }
        if self.paused || self.cfg.mode != RecMode::RealTime || now < self.next_frame_at {
            return;
        }
        self.next_frame_at = (self.next_frame_at + 1.0 / self.cfg.fps.max(1) as f64).max(now - 0.5);
        if rev != self.last_rev {
            self.last_rev = rev;
            let (w, h, s) = snapshot();
            self.push(w, h, s);
        } else if let Err(TrySendError::Full(_)) = self.tx.try_send(Msg::Repeat) {
            self.shared.dropped.fetch_add(1, Ordering::Relaxed);
        }
    }

    pub fn pause(&mut self, paused: bool, now: f64) {
        self.paused = paused;
        self.last_tick = Some(now);
        self.next_frame_at = now;
    }

    pub fn frames(&self) -> u32 {
        self.shared.frames.load(Ordering::Relaxed)
    }

    pub fn error(&self) -> Option<String> {
        self.shared.error.lock().unwrap().clone()
    }

    /// Length of the finished video so far, seconds.
    pub fn video_seconds(&self) -> f32 {
        self.frames() as f32 / self.cfg.fps.max(1) as f32
    }

    /// Stop: the last picture is held for a second, then the file is closed.
    pub fn finish(mut self, last: Option<(u32, u32, DocState)>) -> Result<Finished, String> {
        if let Some((w, h, s)) = last {
            // blocking here is fine: the user pressed Stop
            let _ = self.tx.send(Msg::Frame(Box::new((w, h, s))));
        }
        for _ in 0..self.cfg.fps.min(30) {
            let _ = self.tx.send(Msg::Repeat);
        }
        let _ = self.tx.send(Msg::Finish);
        if let Some(h) = self.handle.take() {
            h.join().map_err(|_| "The recorder stopped unexpectedly.".to_string())?;
        }
        if let Some(e) = self.error() {
            let _ = std::fs::remove_file(&self.path);
            return Err(e);
        }
        let frames = self.frames();
        Ok(Finished { path: self.path.clone(), frames, dropped: self.shared.dropped.load(Ordering::Relaxed), seconds: frames as f32 / self.cfg.fps.max(1) as f32 })
    }

    /// Throw the recording away.
    pub fn cancel(mut self) {
        let _ = self.tx.send(Msg::Finish);
        if let Some(h) = self.handle.take() {
            let _ = h.join();
        }
        let _ = std::fs::remove_file(&self.path);
    }
}

/// Move the finished file to where the user wants it (a copy when it is on another drive).
pub fn deliver(from: &Path, to: &Path) -> Result<(), String> {
    if std::fs::rename(from, to).is_ok() {
        return Ok(());
    }
    std::fs::copy(from, to).map_err(|e| format!("Couldn't save the video: {e}"))?;
    let _ = std::fs::remove_file(from);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn state_with_line(d: &mut Document, n: i32) -> DocState {
        // paint a bar into the first pixel layer
        let l = d.layers.iter_mut().find(|l| l.kind == crate::doc::LayerKind::Raster).unwrap();
        let mut t = vec![eframe::egui::Color32::TRANSPARENT; crate::doc::TILE * crate::doc::TILE];
        for y in 10..30 {
            for x in 0..(20 + n * 30) as usize {
                t[y * crate::doc::TILE + x.min(255)] = eframe::egui::Color32::from_rgb(200, 30, 30);
            }
        }
        l.tiles.insert((0, 0), Arc::new(t));
        d.snapshot()
    }

    #[test]
    fn the_video_fits_the_picture_and_never_gets_bigger() {
        assert_eq!(video_size(4000, 3000, 1080), (1080, 810));
        assert_eq!(video_size(300, 200, 1080), (300, 200), "small pictures stay small");
        assert_eq!(video_size(1001, 777, 720), (720, 558), "even sizes");
        assert_eq!(video_size(1, 1, 720), (16, 16));
    }

    #[test]
    fn frames_become_an_mp4_of_the_canvas_only() {
        let mut d = crate::files::testdoc::sample();
        let first = d.snapshot();
        let mut s = Session::start(RecConfig { fps: 10, max_side: 160, ..RecConfig::default() }, d.w, d.h, first, 0.0).unwrap();
        for i in 0..4 {
            let st = state_with_line(&mut d, i);
            s.push(d.w, d.h, st);
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
        let done = s.finish(None).unwrap();
        assert!(done.frames >= 5 + 10, "5 pictures and the closing hold: {}", done.frames);
        assert_eq!(done.dropped, 0);
        let bytes = std::fs::read(&done.path).unwrap();
        assert_eq!(&bytes[4..8], b"ftyp", "an mp4 file");
        let find = |tag: &[u8]| bytes.windows(4).position(|w| w == tag);
        assert!(find(b"moov").is_some() && find(b"mdat").is_some());
        let a = find(b"avc1").expect("H.264 video");
        let (w, h) = (u16::from_be_bytes([bytes[a + 28], bytes[a + 29]]), u16::from_be_bytes([bytes[a + 30], bytes[a + 31]]));
        assert_eq!((w as u32, h as u32), video_size(300, 200, 160), "the video is the picture, not a window");
        assert!(bytes.len() > 800, "{} bytes", bytes.len());
        let z = find(b"stsz").expect("a sample table");
        let samples = u32::from_be_bytes([bytes[z + 12], bytes[z + 13], bytes[z + 14], bytes[z + 15]]);
        assert_eq!(samples, done.frames, "every frame is in the file");
        // moving it
        let to = std::env::temp_dir().join(format!("seviath_rec_test_{}.mp4", std::process::id()));
        deliver(&done.path, &to).unwrap();
        assert!(to.exists() && !done.path.exists());
        std::fs::remove_file(to).ok();
    }

    #[test]
    fn real_time_takes_frames_at_a_steady_rate_and_repeats_when_nothing_changes() {
        let mut d = crate::files::testdoc::sample();
        let mut s = Session::start(RecConfig { mode: RecMode::RealTime, fps: 10, max_side: 120 }, d.w, d.h, d.snapshot(), 0.0).unwrap();
        let mut snaps = 0;
        // two seconds of app frames at 60 per second; the picture changes once
        for i in 1..=120 {
            let now = i as f64 / 60.0;
            let rev = if i > 60 { 1 } else { 0 };
            s.tick(now, rev, || {
                snaps += 1;
                (d.w, d.h, d.snapshot())
            });
            std::thread::sleep(std::time::Duration::from_millis(1));
        }
        assert_eq!(snaps, 2, "a picture is only drawn again when something changed");
        assert!((s.elapsed - 2.0).abs() < 0.1, "{}", s.elapsed);
        s.pause(true, 2.0);
        s.tick(5.0, 7, || panic!("nothing while paused"));
        assert!((s.elapsed - 2.0).abs() < 0.1, "paused time is not counted");
        let done = s.finish(None).unwrap();
        // about 20 frames while recording (some may be skipped if the machine is slow) plus the closing second
        assert!(done.frames >= 15, "{}", done.frames);
        std::fs::remove_file(&done.path).ok();
    }
}
