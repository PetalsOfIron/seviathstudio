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

/// The vowels, as the two lowest bands of energy a mouth makes for them (Hz): the first (how open the mouth is) and the second (how far forward the tongue is).
pub const VOWELS: [(&str, f32, f32); 5] = [("A", 750.0, 1250.0), ("I", 300.0, 2300.0), ("U", 320.0, 850.0), ("E", 520.0, 1900.0), ("O", 500.0, 950.0)];

/// How open the mouth is for each vowel (1 is wide open), and how stretched (+1) or round (-1) it is.
const VOWEL_OPEN: [f32; 5] = [1.0, 0.35, 0.35, 0.6, 0.7];
const VOWEL_FORM: [f32; 5] = [0.2, 1.0, -1.0, 0.6, -0.6];

/// The power of one frequency in a piece of sound (Goertzel), with a Hann window.
fn band_power(x: &[f32], rate: f32, hz: f32) -> f32 {
    let n = x.len();
    if n < 8 {
        return 0.0;
    }
    let w = 2.0 * std::f32::consts::PI * hz / rate;
    let c = 2.0 * w.cos();
    let (mut s1, mut s2) = (0.0f32, 0.0f32);
    for (i, v) in x.iter().enumerate() {
        let hann = 0.5 - 0.5 * (2.0 * std::f32::consts::PI * i as f32 / (n - 1) as f32).cos();
        let s0 = v * hann + c * s1 - s2;
        s2 = s1;
        s1 = s0;
    }
    (s1 * s1 + s2 * s2 - c * s1 * s2).max(0.0) / (n * n) as f32
}

impl Audio {
    /// How much of each vowel (A, I, U, E, O: they add up to 1, or to 0 for silence) the sound has at every frame of an animation at `fps`, from where the
    /// energy sits: the loudest place between 250 and 900 Hz and between 900 and 2800 Hz are matched with the nearest vowels.
    pub fn vowels(&self, fps: f32, offset: f32, frames: usize) -> Vec<[f32; 5]> {
        let rate = self.rate as f32;
        let loud = self.loudness(fps, offset, frames);
        // the highest band can only be measured when the sound has it (half of the rate)
        let top = (rate / 2.0 - 100.0).min(2800.0);
        (0..frames)
            .map(|f| {
                if loud[f] < 0.1 {
                    return [0.0; 5];
                }
                // a piece of 40 ms in the middle of the frame
                let mid = ((f as f32 + 0.5 - offset) / fps) * rate;
                let len = (0.04 * rate) as usize;
                let a = (mid as isize - len as isize / 2).max(0) as usize;
                let b = (a + len).min(self.frames());
                if b <= a + 8 {
                    return [0.0; 5];
                }
                let piece: Vec<f32> = (a..b).map(|i| self.mono(i)).collect();
                let peak = |lo: f32, hi: f32, step: f32| -> f32 {
                    let (mut best, mut at) = (-1.0f32, lo);
                    let mut hz = lo;
                    while hz <= hi.min(top) {
                        let p = band_power(&piece, rate, hz);
                        if p > best {
                            (best, at) = (p, hz);
                        }
                        hz += step;
                    }
                    at
                };
                let f1 = peak(250.0, 900.0, 25.0);
                let f2 = peak(f1 + 200.0, 2800.0, 50.0);
                let mut w = [0.0f32; 5];
                for (k, (_, v1, v2)) in VOWELS.iter().enumerate() {
                    let d = (f1 / v1).ln().powi(2) + (f2 / v2).ln().powi(2);
                    w[k] = (-d / 0.12).exp();
                }
                let sum: f32 = w.iter().sum();
                if sum < 1e-6 {
                    return [0.0; 5];
                }
                w.map(|x| x / sum)
            })
            .collect()
    }

    /// The mouth for every frame from the vowels: how open (0 to 1) and how stretched or round (-1 round, 1 stretched), moving smoothly.
    pub fn mouth_shape(&self, fps: f32, offset: f32, frames: usize) -> Vec<(f32, f32)> {
        let loud = self.mouth_open(fps, offset, frames);
        let mut out = Vec::with_capacity(frames);
        let (mut open, mut form) = (0.0f32, 0.0f32);
        for (v, l) in self.vowels(fps, offset, frames).into_iter().zip(loud) {
            let sum: f32 = v.iter().sum();
            let (to_open, to_form) = if sum < 1e-6 { (0.0, 0.0) } else { ((0..5).map(|k| v[k] * VOWEL_OPEN[k]).sum::<f32>() * l.max(0.3), (0..5).map(|k| v[k] * VOWEL_FORM[k]).sum::<f32>()) };
            open += (to_open - open) * if to_open > open { 0.7 } else { 0.45 };
            form += (to_form - form) * 0.5;
            out.push((if open < 0.04 { 0.0 } else { open }, form));
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
                        // the tests play test tones: they must not come out of the speakers
                        if cfg!(test) {
                            *w2.lock().unwrap() = Some(false);
                            continue;
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

    /// A sound made of two sines, the way a vowel has two bands, for `seconds`.
    fn vowel_sound(f1: f32, f2: f32, seconds: f32, rate: u32) -> Vec<i16> {
        (0..(seconds * rate as f32) as usize)
            .map(|i| {
                let t = i as f32 / rate as f32;
                (((t * f1 * std::f32::consts::TAU).sin() + 0.7 * (t * f2 * std::f32::consts::TAU).sin()) * 9000.0) as i16
            })
            .collect()
    }

    #[test]
    fn the_vowels_are_told_apart_by_where_the_energy_sits() {
        let rate = 16000;
        let mut s = vowel_sound(750.0, 1250.0, 0.5, rate);
        s.extend(vowel_sound(300.0, 2300.0, 0.5, rate));
        s.extend(vowel_sound(320.0, 850.0, 0.5, rate));
        let a = Audio::from_pcm(1, rate, s);
        let v = a.vowels(10.0, 0.0, 15);
        let best = |f: usize| (0..5).max_by(|x, y| v[f][*x].total_cmp(&v[f][*y])).unwrap();
        assert_eq!((best(2), best(7), best(12)), (0, 1, 2), "A, then I, then U: {:?} {:?} {:?}", v[2], v[7], v[12]);
        assert!((v[2].iter().sum::<f32>() - 1.0).abs() < 1e-3);
        // the mouth: wide open for the A, stretched for the I, round for the U
        let m = a.mouth_shape(10.0, 0.0, 15);
        assert!(m[3].0 > m[8].0, "A opens wider than I: {m:?}");
        assert!(m[8].1 > 0.4 && m[13].1 < -0.4, "I is stretched, U is round: {m:?}");
        // silence has no vowel
        let quiet = Audio::from_pcm(1, rate, vec![0; 8000]);
        assert!(quiet.vowels(10.0, 0.0, 5).iter().all(|v| *v == [0.0; 5]));
        assert!(quiet.mouth_shape(10.0, 0.0, 5).iter().all(|m| *m == (0.0, 0.0)));
    }
}
