// SPDX-License-Identifier: GPL-3.0-or-later
// Sound for the Animate tab: reading a sound file (WAV, MP3, OGG, FLAC, AAC), the picture of its waveform, how loud it is at every
// frame of the animation (for lip sync), and playing it while the timeline plays. Playing happens in a thread of its own that owns the
// sound card; when there is no sound card the animation still works, silently.

use std::io::Cursor;
use std::sync::mpsc::{Receiver, Sender, channel};
use std::sync::{Arc, Mutex};

use rodio::Source;

const MAX_SECONDS: f32 = 3600.0;

/// A sound, as 16 bit numbers, the channels interleaved.
#[derive(Clone)]
pub struct Audio {
    pub channels: u16,
    pub rate: u32,
    pub samples: Arc<Vec<i16>>,
}

impl Audio {
    pub fn decode(bytes: Vec<u8>) -> Result<Audio, String> {
        let dec = rodio::Decoder::new(Cursor::new(bytes)).map_err(|e| format!("This is not a sound file I can read ({e}). WAV, MP3, OGG, FLAC and AAC work."))?;
        let (channels, rate) = (dec.channels(), dec.sample_rate());
        if channels == 0 || rate == 0 {
            return Err("The sound has no channels.".into());
        }
        let limit = (MAX_SECONDS * rate as f32) as usize * channels as usize;
        let samples: Vec<i16> = dec.take(limit).collect();
        if samples.is_empty() {
            return Err("The sound file is empty.".into());
        }
        Ok(Audio { channels, rate, samples: Arc::new(samples) })
    }

    pub fn from_pcm(channels: u16, rate: u32, samples: Vec<i16>) -> Audio {
        Audio { channels: channels.max(1), rate: rate.max(1), samples: Arc::new(samples) }
    }

    pub fn seconds(&self) -> f32 {
        self.samples.len() as f32 / (self.channels as f32 * self.rate as f32)
    }

    /// The sound at one moment, the channels mixed, from -1 to 1.
    fn mono(&self, i: usize) -> f32 {
        let c = self.channels as usize;
        let s = &self.samples[(i * c).min(self.samples.len())..((i + 1) * c).min(self.samples.len())];
        if s.is_empty() { 0.0 } else { s.iter().map(|x| *x as f32).sum::<f32>() / (s.len() as f32 * 32768.0) }
    }

    fn frames(&self) -> usize {
        self.samples.len() / self.channels as usize
    }

    /// The lowest and highest value in each of `n` equal pieces of the time from `from` to `to` seconds (for drawing the waveform).
    pub fn peaks(&self, from: f32, to: f32, n: usize) -> Vec<(f32, f32)> {
        let rate = self.rate as f32;
        (0..n)
            .map(|k| {
                let a = ((from + (to - from) * k as f32 / n as f32) * rate).floor();
                let b = ((from + (to - from) * (k + 1) as f32 / n as f32) * rate).ceil();
                if b <= 0.0 || a >= self.frames() as f32 {
                    return (0.0, 0.0);
                }
                let (a, b) = (a.max(0.0) as usize, (b as usize).min(self.frames()));
                // a long piece is looked at with steps so that drawing stays fast
                let step = ((b.saturating_sub(a)) / 64).max(1);
                let (mut lo, mut hi) = (0.0f32, 0.0f32);
                let mut i = a;
                while i < b {
                    let v = self.mono(i);
                    lo = lo.min(v);
                    hi = hi.max(v);
                    i += step;
                }
                (lo, hi)
            })
            .collect()
    }

    /// How loud the sound is at every frame of an animation at `fps`, starting at the frame `offset` (negative: the sound starts
    /// before the first frame): 0 for quiet to 1 for the loudest. `frames` values.
    pub fn loudness(&self, fps: f32, offset: f32, frames: usize) -> Vec<f32> {
        let rate = self.rate as f32;
        let rms: Vec<f32> = (0..frames)
            .map(|f| {
                let (t0, t1) = ((f as f32 - offset) / fps, (f as f32 + 1.0 - offset) / fps);
                let (a, b) = ((t0 * rate).floor().max(0.0) as usize, ((t1 * rate).ceil().max(0.0) as usize).min(self.frames()));
                if b <= a {
                    return 0.0;
                }
                let step = ((b - a) / 256).max(1);
                let (mut sum, mut n) = (0.0f32, 0u32);
                let mut i = a;
                while i < b {
                    let v = self.mono(i);
                    sum += v * v;
                    n += 1;
                    i += step;
                }
                (sum / n.max(1) as f32).sqrt()
            })
            .collect();
        // the loud parts, not the one click that is louder than everything: scale by a high percentile
        let mut sorted: Vec<f32> = rms.iter().copied().filter(|v| *v > 1e-4).collect();
        if sorted.is_empty() {
            return vec![0.0; frames];
        }
        sorted.sort_by(|a, b| a.total_cmp(b));
        let top = sorted[((sorted.len() - 1) as f32 * 0.95) as usize].max(1e-4);
        rms.iter().map(|v| (v / top).clamp(0.0, 1.0)).collect()
    }

    /// The mouth opening for every frame, from the loudness: quiet frames close the mouth, a little smoothing so it does not flutter.
    pub fn mouth_open(&self, fps: f32, offset: f32, frames: usize) -> Vec<f32> {
        let loud = self.loudness(fps, offset, frames);
        let mut out = Vec::with_capacity(frames);
        let mut v = 0.0f32;
        for l in loud {
            let target = ((l - 0.12) / 0.6).clamp(0.0, 1.0);
            // opens quickly, closes a little slower
            v += (target - v) * if target > v { 0.7 } else { 0.45 };
            out.push(if v < 0.04 { 0.0 } else { v });
        }
        out
    }
}

enum Cmd {
    Play(Audio, f32),
    Stop,
}

/// Plays sounds in a thread of its own.
pub struct Player {
    tx: Sender<Cmd>,
    pub working: Arc<Mutex<Option<bool>>>,
}

impl Player {
    pub fn new() -> Player {
        let (tx, rx): (Sender<Cmd>, Receiver<Cmd>) = channel();
        let working = Arc::new(Mutex::new(None));
        let w2 = working.clone();
        std::thread::spawn(move || {
            // the sound card is opened the first time a sound is played
            let mut out: Option<(rodio::OutputStream, rodio::OutputStreamHandle)> = None;
            let mut sink: Option<rodio::Sink> = None;
            while let Ok(cmd) = rx.recv() {
                match cmd {
                    Cmd::Stop => {
                        if let Some(s) = sink.take() {
                            s.stop();
                        }
                    }
                    Cmd::Play(audio, from) => {
                        if let Some(s) = sink.take() {
                            s.stop();
                        }
                        if out.is_none() {
                            out = rodio::OutputStream::try_default().ok();
                            *w2.lock().unwrap() = Some(out.is_some());
                        }
                        let Some((_, handle)) = &out else { continue };
                        let skip = ((from.max(0.0) * audio.rate as f32) as usize) * audio.channels as usize;
                        if skip >= audio.samples.len() {
                            continue;
                        }
                        if let Ok(s) = rodio::Sink::try_new(handle) {
                            s.append(rodio::buffer::SamplesBuffer::new(audio.channels, audio.rate, audio.samples[skip..].to_vec()));
                            sink = Some(s);
                        }
                    }
                }
            }
        });
        Player { tx, working }
    }

    /// Play from `from` seconds into the sound.
    pub fn play(&self, audio: &Audio, from: f32) {
        let _ = self.tx.send(Cmd::Play(audio.clone(), from));
    }

    pub fn stop(&self) {
        let _ = self.tx.send(Cmd::Stop);
    }

    /// Did the sound card open? None until a sound was played.
    pub fn sound_card(&self) -> Option<bool> {
        *self.working.lock().unwrap()
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    /// A WAV file: `seconds` of a sine at `hz`, loud for the first half and quiet after it.
    pub(crate) fn wav(seconds: f32, rate: u32) -> Vec<u8> {
        let n = (seconds * rate as f32) as usize;
        let mut data = Vec::new();
        for i in 0..n {
            let t = i as f32 / rate as f32;
            let amp = if i < n / 2 { 0.8 } else { 0.02 };
            let v = ((t * 440.0 * std::f32::consts::TAU).sin() * amp * 32767.0) as i16;
            data.extend(v.to_le_bytes());
        }
        let mut out = Vec::new();
        out.extend(b"RIFF");
        out.extend((36 + data.len() as u32).to_le_bytes());
        out.extend(b"WAVEfmt ");
        out.extend(16u32.to_le_bytes());
        out.extend(1u16.to_le_bytes());
        out.extend(1u16.to_le_bytes());
        out.extend(rate.to_le_bytes());
        out.extend((rate * 2).to_le_bytes());
        out.extend(2u16.to_le_bytes());
        out.extend(16u16.to_le_bytes());
        out.extend(b"data");
        out.extend((data.len() as u32).to_le_bytes());
        out.extend(data);
        out
    }

    #[test]
    fn a_wav_is_read_drawn_and_measured() {
        let a = Audio::decode(wav(2.0, 8000)).unwrap();
        assert_eq!((a.channels, a.rate), (1, 8000));
        assert!((a.seconds() - 2.0).abs() < 0.01);
        // the waveform: loud in the first second, quiet in the second
        let p = a.peaks(0.0, 2.0, 4);
        assert!(p[0].1 > 0.6 && p[1].1 > 0.6 && p[2].1 < 0.1 && p[3].1 < 0.1, "{p:?}");
        assert!(p[0].0 < -0.6);
        assert_eq!(a.peaks(5.0, 6.0, 2), [(0.0, 0.0), (0.0, 0.0)], "nothing after the end");
        // loudness at 10 frames a second: 20 frames, the first half full, the second near zero
        let l = a.loudness(10.0, 0.0, 20);
        assert!(l[..10].iter().all(|v| *v > 0.9), "{l:?}");
        assert!(l[10..].iter().all(|v| *v < 0.1), "{l:?}");
        // the sound starting 5 frames later: the first 5 frames are silent
        let l = a.loudness(10.0, 5.0, 20);
        assert!(l[..5].iter().all(|v| *v == 0.0) && l[6] > 0.9);
        // the mouth opens with the loud part and is shut for the quiet one
        let m = a.mouth_open(10.0, 0.0, 20);
        assert!(m[3] > 0.8 && m[15] == 0.0, "{m:?}");
        assert!(Audio::decode(b"not a sound".to_vec()).is_err());
        assert_eq!(Audio::from_pcm(1, 8000, vec![0; 8000]).loudness(10.0, 0.0, 5), [0.0; 5], "silence stays silent");
    }

    #[test]
    fn two_channels_are_mixed() {
        let mut s = Vec::new();
        for _ in 0..100 {
            s.extend([16000i16, -16000]);
        }
        let a = Audio::from_pcm(2, 100, s);
        assert!((a.seconds() - 1.0).abs() < 1e-3);
        assert!(a.peaks(0.0, 1.0, 1)[0].1.abs() < 1e-3, "the two channels cancel");
    }
}
