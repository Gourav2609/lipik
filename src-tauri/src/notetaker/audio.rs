use anyhow::{anyhow, Result};
use handy_recorder::{AudioChunk, Channels, Recorder, RecorderConfig, Sink};
use hound::{SampleFormat, WavSpec, WavWriter};
use std::fs::File;
use std::io::BufWriter;
use std::path::Path;
use std::sync::{Arc, Mutex};
use std::time::Instant;
use tauri::{AppHandle, Emitter};

use crate::audio_toolkit::SpeechScorer;

pub const RATE: usize = 16_000;
const WAV_FLUSH_SAMPLES: usize = 5 * RATE;
/// A stream this far behind the wall clock (device gone, blocks dropped) is
/// padded with silence, so both files stay on the same timeline.
const GAP_FILL_SAMPLES: u64 = RATE as u64;
/// Live audio waiting for transcription is capped at this; when transcription
/// falls behind, the oldest half is dropped (the final pass still covers it).
const LIVE_BACKLOG: usize = 120 * RATE;

pub type Wav = WavWriter<BufWriter<File>>;

pub fn create_wav(path: &Path) -> Result<Wav> {
    let spec = WavSpec {
        channels: 1,
        sample_rate: RATE as u32,
        bits_per_sample: 16,
        sample_format: SampleFormat::Int,
    };
    Ok(WavWriter::create(path, spec)?)
}

/// Audio of one stream that the live transcriber has not taken yet, plus when
/// the stream's first sample arrived. Filled by the capture thread.
#[derive(Default)]
pub struct StreamFeed {
    pub pending: Vec<f32>,
    /// Samples dropped from the front of `pending` since the live transcriber
    /// last took it.
    pub dropped: u64,
    pub first_at: Option<Instant>,
    pub samples: u64,
    pub silent_samples: u64,
    pub failed: Option<String>,
}

pub type SharedFeed = Arc<Mutex<StreamFeed>>;

/// Writes 16 kHz mono audio to the stream's WAV and hands it to the feed. The
/// header is rewritten every few seconds so a crash still leaves a readable
/// file.
pub struct StreamWriter {
    wav: Option<Wav>,
    feed: SharedFeed,
    since_flush: usize,
    started: Option<Instant>,
    written: u64,
}

impl StreamWriter {
    pub fn new(wav: Wav, feed: SharedFeed) -> Self {
        Self {
            wav: Some(wav),
            feed,
            since_flush: 0,
            started: None,
            written: 0,
        }
    }

    pub fn write(&mut self, samples: &[f32]) {
        if samples.is_empty() {
            return;
        }
        let now = Instant::now();
        let started = *self.started.get_or_insert(now);
        let due = (now.saturating_duration_since(started).as_secs_f64() * RATE as f64) as u64;
        let mut behind = due.saturating_sub(self.written + samples.len() as u64);
        if behind > GAP_FILL_SAMPLES {
            log::warn!("Notetaker stream fell {} ms behind; filling with silence", samples_to_ms(behind));
            let zeros = vec![0.0; RATE];
            while behind > 0 {
                let n = behind.min(RATE as u64) as usize;
                self.append(&zeros[..n], now);
                behind -= n as u64;
            }
        }
        self.append(samples, now);
    }

    fn append(&mut self, samples: &[f32], now: Instant) {
        if let Some(wav) = self.wav.as_mut() {
            for &s in samples {
                let v = (s.clamp(-1.0, 1.0) * i16::MAX as f32) as i16;
                if let Err(e) = wav.write_sample(v) {
                    log::error!("Notetaker WAV write failed: {e}");
                    self.wav = None;
                    break;
                }
            }
            self.since_flush += samples.len();
            if self.since_flush >= WAV_FLUSH_SAMPLES {
                self.since_flush = 0;
                if let Some(wav) = self.wav.as_mut() {
                    let _ = wav.flush();
                }
            }
        }
        self.written += samples.len() as u64;
        let silent = samples.iter().all(|&s| s == 0.0);
        let mut feed = self.feed.lock().unwrap_or_else(|e| e.into_inner());
        feed.first_at.get_or_insert(now);
        feed.pending.extend_from_slice(samples);
        if feed.pending.len() > LIVE_BACKLOG {
            let excess = feed.pending.len() - LIVE_BACKLOG / 2;
            feed.pending.drain(..excess);
            feed.dropped += excess as u64;
        }
        feed.samples += samples.len() as u64;
        if silent {
            feed.silent_samples += samples.len() as u64;
        }
    }

    pub fn set_failed(&self, reason: Option<String>) {
        self.feed.lock().unwrap_or_else(|e| e.into_inner()).failed = reason;
    }

    pub fn finish(mut self) {
        if let Some(wav) = self.wav.take() {
            if let Err(e) = wav.finalize() {
                log::error!("Notetaker WAV finalize failed: {e}");
            }
        }
    }
}

/// Sends the microphone's RMS level (dBFS) to the pill's level meter as
/// `notetaker-level`, about 30 times a second.
struct LevelTap {
    app: AppHandle,
    sum: f64,
    count: usize,
}

impl LevelTap {
    fn new(app: AppHandle) -> Self {
        Self { app, sum: 0.0, count: 0 }
    }

    fn push(&mut self, samples: &[f32], rate: u32) {
        self.sum += samples.iter().map(|&s| (s as f64) * (s as f64)).sum::<f64>();
        self.count += samples.len();
        if self.count < (rate / 40).max(1) as usize {
            return;
        }
        let rms = (self.sum / self.count as f64).sqrt();
        let db = if rms > 1e-5 { 20.0 * rms.log10() } else { -100.0 };
        self.sum = 0.0;
        self.count = 0;
        let _ = self.app.emit_to("recording_overlay", "notetaker-level", db as f32);
    }
}

pub struct MicSink(pub StreamWriter, LevelTap);

impl Sink for MicSink {
    fn process_chunk(&mut self, chunk: AudioChunk<'_>) {
        let real = chunk.valid_frames * chunk.channels as usize;
        self.0.write(&chunk.samples[..real]);
        self.1.push(&chunk.samples[..real], chunk.sample_rate);
    }
}

/// The notetaker's own recorder on the dictation microphone. After a failure
/// (device unplugged) it can be opened again on the same WAV.
pub struct MicCapture {
    device: Option<String>,
    channel: Option<u16>,
    recorder: Option<Recorder<MicSink>>,
    /// The writer while no recorder holds it.
    idle: Option<StreamWriter>,
    app: AppHandle,
}

impl MicCapture {
    pub fn start(
        device: Option<String>,
        channel: Option<u16>,
        writer: StreamWriter,
        app: AppHandle,
    ) -> Result<Self> {
        let mut mic = Self {
            device,
            channel,
            recorder: None,
            idle: Some(writer),
            app,
        };
        mic.open()?;
        Ok(mic)
    }

    fn open(&mut self) -> Result<()> {
        let writer = self
            .idle
            .take()
            .ok_or_else(|| anyhow!("The microphone recording was lost"))?;
        let device = self.device.as_deref().and_then(|name| {
            handy_recorder::list_input_devices()
                .ok()?
                .into_iter()
                .find(|d| d.name == name)
                .map(|d| d.id)
        });
        let config = RecorderConfig {
            device,
            channels: self.channel.map_or(Channels::MixToMono, Channels::Only),
            ..RecorderConfig::speech()
        };
        let feed = writer.feed.clone();
        let on_failure = feed.clone();
        let recorder = match Recorder::open_with_failure_handler(config, move |error| {
            log::error!("Notetaker microphone failed: {error}");
            on_failure.lock().unwrap_or_else(|e| e.into_inner()).failed = Some(error.to_string());
        }) {
            Ok(recorder) => recorder,
            Err(e) => {
                self.idle = Some(writer);
                return Err(e.into());
            }
        };
        if let Err(e) = recorder.start(MicSink(writer, LevelTap::new(self.app.clone()))) {
            self.idle = Some(e.sink.0);
            return Err(anyhow!("Failed to start microphone: {}", e.error));
        }
        feed.lock().unwrap_or_else(|e| e.into_inner()).failed = None;
        self.recorder = Some(recorder);
        Ok(())
    }

    /// Closes the failed recorder and opens the microphone again (the system
    /// default when the chosen one is gone).
    pub fn reopen(&mut self) -> Result<()> {
        if let Some(recorder) = self.recorder.take() {
            match recorder.stop() {
                Ok(stopped) => self.idle = Some(stopped.sink.0),
                Err(e) => log::error!("Notetaker microphone stop failed: {e}"),
            }
        }
        self.open()
    }

    pub fn stop(mut self) {
        if let Some(recorder) = self.recorder.take() {
            match recorder.stop() {
                Ok(stopped) => self.idle = Some(stopped.sink.0),
                Err(e) => log::error!("Notetaker microphone stop failed: {e}"),
            }
        }
        if let Some(writer) = self.idle.take() {
            writer.finish();
        }
    }
}

/// Mono float audio at any rate to 16 kHz: a one-pole low-pass, then linear
/// interpolation. Good enough for speech recognition.
pub struct Resampler {
    step: f64,
    pos: f64,
    prev: f32,
    lp: f32,
    alpha: f32,
}

impl Resampler {
    pub fn new(input_rate: f64) -> Self {
        let step = input_rate / RATE as f64;
        let cutoff = 0.45 * RATE as f64;
        let rc = 1.0 / (2.0 * std::f64::consts::PI * cutoff);
        let dt = 1.0 / input_rate;
        Self {
            step,
            pos: 0.0,
            prev: 0.0,
            lp: 0.0,
            alpha: if step > 1.0 { (dt / (rc + dt)) as f32 } else { 1.0 },
        }
    }

    pub fn process(&mut self, input: &[f32], out: &mut Vec<f32>) {
        for &x in input {
            self.lp += self.alpha * (x - self.lp);
            let cur = self.lp;
            while self.pos < 1.0 {
                out.push(self.prev + (cur - self.prev) * self.pos as f32);
                self.pos += self.step;
            }
            self.pos -= 1.0;
            self.prev = cur;
        }
    }
}

/// How a stream is cut into pieces to transcribe.
#[derive(Clone, Copy)]
pub struct CutRules {
    pub min_s: f32,
    pub max_s: f32,
    pub pause_ms: u32,
}

pub const FINAL_RULES: CutRules = CutRules {
    min_s: 3.0,
    max_s: 25.0,
    pause_ms: 700,
};

const SPEECH_ON: f32 = 0.5;
const SPEECH_OFF: f32 = 0.35;
const EDGE_PAD_MS: usize = 200;
const MIN_SPEECH_MS: usize = 250;

pub struct Piece {
    pub start_sample: u64,
    pub audio: Vec<f32>,
}

/// A stream's audio scored for speech, cut into pieces at pauses. Pieces with
/// no speech are dropped; leading and trailing silence is trimmed.
pub struct SpeechTrack {
    scorer: Option<Box<dyn SpeechScorer>>,
    frame: usize,
    pending: Vec<f32>,
    probs: Vec<f32>,
    base: u64,
}

impl SpeechTrack {
    pub fn new(scorer: Option<Box<dyn SpeechScorer>>) -> Self {
        let frame = scorer.as_ref().map_or(512, |s| s.frame_samples());
        Self {
            scorer,
            frame,
            pending: Vec::new(),
            probs: Vec::new(),
            base: 0,
        }
    }

    pub fn push(&mut self, samples: &[f32]) {
        self.pending.extend_from_slice(samples);
        let scored = self.probs.len() * self.frame;
        let whole = (self.pending.len() - scored) / self.frame * self.frame;
        if whole == 0 {
            return;
        }
        let frames = &self.pending[scored..scored + whole];
        let probs = match self.scorer.as_mut().map(|s| s.score(frames)) {
            Some(Ok(p)) => p,
            Some(Err(e)) => {
                log::warn!("Notetaker VAD failed, using loudness instead: {e}");
                self.scorer = None;
                energy_probs(frames, self.frame)
            }
            None => energy_probs(frames, self.frame),
        };
        self.probs.extend(probs);
    }

    /// The next piece, once its end is certain. `flush` takes everything left.
    pub fn take(&mut self, rules: CutRules, flush: bool) -> Option<Piece> {
        let min_f = ((rules.min_s * RATE as f32) as usize / self.frame).max(1);
        let max_f = (rules.max_s * RATE as f32) as usize / self.frame;
        let pause_f = (rules.pause_ms as usize * RATE / 1000)
            .div_ceil(self.frame)
            .max(1);
        loop {
            let frames = self.probs.len();
            if frames == 0 {
                if flush {
                    self.base += self.pending.len() as u64;
                    self.pending.clear();
                }
                return None;
            }
            let cut = self
                .find_pause(min_f, max_f.min(frames), pause_f)
                .or_else(|| (frames >= max_f).then(|| self.quietest(max_f, max_f / 4)))
                .or_else(|| flush.then_some(frames))?;
            if let Some(piece) = self.cut_at(cut) {
                return Some(piece);
            }
        }
    }

    fn find_pause(&self, from: usize, to: usize, pause_f: usize) -> Option<usize> {
        let mut run_start = None;
        for i in from..to {
            if self.probs[i] < SPEECH_OFF {
                let start = *run_start.get_or_insert(i);
                if i + 1 - start >= pause_f {
                    return Some(start + pause_f / 2);
                }
            } else {
                run_start = None;
            }
        }
        None
    }

    fn quietest(&self, end: usize, window: usize) -> usize {
        let start = end.saturating_sub(window).max(1);
        (start..end)
            .min_by(|&a, &b| self.probs[a].total_cmp(&self.probs[b]))
            .unwrap_or(end)
    }

    fn cut_at(&mut self, frames: usize) -> Option<Piece> {
        let samples = (frames * self.frame).min(self.pending.len());
        let probs: Vec<f32> = self.probs.drain(..frames).collect();
        let audio: Vec<f32> = self.pending.drain(..samples).collect();
        let start = self.base;
        self.base += samples as u64;

        let speech_frames = probs.iter().filter(|&&p| p >= SPEECH_ON).count();
        if speech_frames * self.frame < MIN_SPEECH_MS * RATE / 1000 {
            return None;
        }
        let pad = EDGE_PAD_MS * RATE / 1000;
        let first = probs.iter().position(|&p| p >= SPEECH_ON).unwrap_or(0) * self.frame;
        let last = (probs.iter().rposition(|&p| p >= SPEECH_ON).unwrap_or(0) + 1) * self.frame;
        let from = first.saturating_sub(pad);
        let to = (last + pad).min(audio.len());
        Some(Piece {
            start_sample: start + from as u64,
            audio: audio[from..to].to_vec(),
        })
    }
}

pub fn energy_probs(frames: &[f32], frame: usize) -> Vec<f32> {
    frames
        .chunks(frame)
        .map(|f| {
            let rms = (f.iter().map(|s| s * s).sum::<f32>() / f.len() as f32).sqrt();
            if rms > 0.01 {
                1.0
            } else {
                0.0
            }
        })
        .collect()
}

pub fn samples_to_ms(samples: u64) -> i64 {
    (samples * 1000 / RATE as u64) as i64
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tone(seconds: f32) -> Vec<f32> {
        (0..(seconds * RATE as f32) as usize)
            .map(|i| 0.3 * (i as f32 * 0.05).sin())
            .collect()
    }

    fn silence(seconds: f32) -> Vec<f32> {
        vec![0.0; (seconds * RATE as f32) as usize]
    }

    #[test]
    fn cuts_at_pauses_and_drops_silence() {
        let mut track = SpeechTrack::new(None);
        let mut audio = tone(5.0);
        audio.extend(silence(2.0));
        audio.extend(tone(4.0));
        audio.extend(silence(10.0));
        track.push(&audio);

        let first = track.take(FINAL_RULES, false).expect("first piece");
        assert!(first.start_sample < RATE as u64 / 2);
        let secs = first.audio.len() as f32 / RATE as f32;
        assert!((4.9..5.6).contains(&secs), "first piece {secs}s");

        let second = track.take(FINAL_RULES, true).expect("second piece");
        let start = second.start_sample as f32 / RATE as f32;
        assert!((6.6..7.1).contains(&start), "second starts {start}s");
        assert!(track.take(FINAL_RULES, true).is_none());
    }

    #[test]
    fn hard_cuts_long_speech() {
        let mut track = SpeechTrack::new(None);
        track.push(&tone(70.0));
        let piece = track.take(FINAL_RULES, false).expect("hard cut");
        let secs = piece.audio.len() as f32 / RATE as f32;
        assert!(secs <= 25.5, "piece {secs}s");
    }

    #[test]
    fn resampler_keeps_duration() {
        let mut r = Resampler::new(48_000.0);
        let mut out = Vec::new();
        r.process(&vec![0.1; 48_000], &mut out);
        assert!((15_990..=16_010).contains(&out.len()), "{}", out.len());
    }
}
