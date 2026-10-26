//! The live transcript of one stream (microphone or system audio). The VAD
//! finds each utterance; it is streamed to a transcription worker of its own
//! while it is spoken, so words show within about a second, and becomes a
//! segment when the speaker pauses. A model that can't stream gets each
//! utterance decoded once it ends, on the dictation engine.

use serde::Serialize;
use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};
use tauri::{AppHandle, Emitter};
use tauri_specta::Event;
use transcribe_cpp::StreamOptions;

use super::audio::{energy_probs, samples_to_ms, SharedFeed, RATE};
use super::{transcribe, NotetakerManager, NotetakerTranscript, Segment};
use crate::audio_toolkit::SpeechScorer;
use crate::engine_supervisor::{StreamHandle, StreamProgress};
use crate::managers::transcription::SideEngine;

pub const LIVE_TEXT_EVENT: &str = "notetaker-live-text";

const POLL: Duration = Duration::from_millis(100);
const SPEECH_ON: f32 = 0.5;
const SPEECH_OFF: f32 = 0.35;
/// Audio kept from before the VAD hears speech, so the first word is whole.
const PREROLL_MS: u64 = 300;
const END_PAUSE_MS: u64 = 700;
/// Past `LONG_MS` a shorter pause ends the utterance; at `MAX_MS` it is cut.
const LONG_MS: u64 = 20_000;
const LONG_PAUSE_MS: u64 = 250;
const MAX_MS: u64 = 30_000;
const TRAIL_PAD_MS: u64 = 200;
/// Less speech than this (a cough, a click) gives no segment.
const MIN_VOICED_MS: i64 = 250;
/// A stream's worker is stopped after this long without speech and loaded
/// again at the next utterance (the audio waits in the feed meanwhile).
const IDLE_UNLOAD: Duration = Duration::from_secs(300);

const fn ms(ms: u64) -> u64 {
    ms * RATE as u64 / 1000
}

#[derive(Debug, PartialEq)]
pub enum Step {
    /// Speech started at this sample of the stream.
    Begin { start: u64 },
    Audio(Vec<f32>),
    End { end: u64, voiced: u64 },
}

struct Utterance {
    start: u64,
    last_voice: u64,
    quiet: u64,
    voiced: u64,
}

/// Cuts a stream into utterances from per-frame speech probabilities.
pub struct Gate {
    frame: usize,
    preroll: VecDeque<f32>,
    /// Samples of the stream seen so far.
    pos: u64,
    utterance: Option<Utterance>,
}

impl Gate {
    pub fn new(frame: usize) -> Self {
        Self {
            frame,
            preroll: VecDeque::new(),
            pos: 0,
            utterance: None,
        }
    }

    /// `frames` are whole frames, `probs` one per frame.
    pub fn push(&mut self, frames: &[f32], probs: &[f32]) -> Vec<Step> {
        let mut steps = Vec::new();
        let mut audio = Vec::new();
        for (f, &p) in frames.chunks(self.frame).zip(probs) {
            let n = f.len() as u64;
            let end = self.pos + n;
            match self.utterance.as_mut() {
                None if p >= SPEECH_ON => {
                    let start = self.pos - self.preroll.len() as u64;
                    steps.push(Step::Begin { start });
                    audio.extend(self.preroll.drain(..));
                    audio.extend_from_slice(f);
                    self.utterance = Some(Utterance {
                        start,
                        last_voice: end,
                        quiet: 0,
                        voiced: n,
                    });
                }
                None => {
                    self.preroll.extend(f);
                    let excess = self.preroll.len().saturating_sub(ms(PREROLL_MS) as usize);
                    self.preroll.drain(..excess);
                }
                Some(u) => {
                    audio.extend_from_slice(f);
                    if p >= SPEECH_OFF {
                        u.quiet = 0;
                        u.last_voice = end;
                        if p >= SPEECH_ON {
                            u.voiced += n;
                        }
                    } else {
                        u.quiet += n;
                    }
                    let len = end - u.start;
                    let pause = if len >= ms(LONG_MS) { LONG_PAUSE_MS } else { END_PAUSE_MS };
                    if u.quiet >= ms(pause) || len >= ms(MAX_MS) {
                        let step = Step::End {
                            end: (u.last_voice + ms(TRAIL_PAD_MS)).min(end),
                            voiced: u.voiced,
                        };
                        if !audio.is_empty() {
                            steps.push(Step::Audio(std::mem::take(&mut audio)));
                        }
                        steps.push(step);
                        self.utterance = None;
                    }
                }
            }
            self.pos = end;
        }
        if !audio.is_empty() {
            steps.push(Step::Audio(audio));
        }
        steps
    }

    /// Ends the utterance in progress, if any.
    pub fn flush(&mut self) -> Option<Step> {
        let u = self.utterance.take()?;
        Some(Step::End {
            end: (u.last_voice + ms(TRAIL_PAD_MS)).min(self.pos),
            voiced: u.voiced,
        })
    }

    /// Moves past audio that was dropped; call `flush` first.
    pub fn skip(&mut self, samples: u64) {
        self.preroll.clear();
        self.pos += samples;
    }
}

#[derive(Clone, Serialize)]
struct LiveText {
    meeting_id: String,
    source: String,
    speaker: String,
    start_ms: i64,
    text: String,
    tentative: String,
    active: bool,
}

enum Decoder {
    Stream(StreamHandle),
    /// The utterance's audio, decoded in one run when it ends.
    Whole(Vec<f32>),
}

struct Current {
    start: u64,
    decoder: Decoder,
}

/// One stream's live transcription, run on its own thread until `stop`.
struct Source<'a> {
    manager: &'a NotetakerManager,
    id: &'a str,
    source: &'static str,
    stop: &'a AtomicBool,
    offset_ms: i64,
    side: Option<SideEngine>,
    /// The model can't stream (or failed to load): decode whole utterances.
    no_side: bool,
    last_speech: Instant,
    current: Option<Current>,
}

impl Source<'_> {
    fn app(&self) -> &AppHandle {
        &self.manager.app
    }

    fn emit_text(&self, start: u64, text: &str, tentative: &str, active: bool) {
        let _ = self.app().emit(
            LIVE_TEXT_EVENT,
            LiveText {
                meeting_id: self.id.to_string(),
                source: self.source.to_string(),
                speaker: transcribe::speaker_for(self.source).to_string(),
                start_ms: self.offset_ms + samples_to_ms(start),
                text: text.to_string(),
                tentative: tentative.to_string(),
                active,
            },
        );
    }

    fn ensure_side(&mut self) {
        if self.side.is_some() || self.no_side {
            return;
        }
        let loaded = transcribe::transcription_manager(self.app())
            .and_then(|tm| tm.side_stream_engine());
        match loaded {
            Ok(side) => self.side = Some(side),
            Err(e) => {
                log::info!(
                    "Notetaker live {} transcript decodes whole utterances: {e}",
                    self.source
                );
                self.no_side = true;
            }
        }
    }

    fn begin(&mut self, start: u64) {
        self.emit_text(start, "", "", true);
        self.ensure_side();
        let app = self.app().clone();
        let template = LiveText {
            meeting_id: self.id.to_string(),
            source: self.source.to_string(),
            speaker: transcribe::speaker_for(self.source).to_string(),
            start_ms: self.offset_ms + samples_to_ms(start),
            text: String::new(),
            tentative: String::new(),
            active: true,
        };
        let on_progress = move |progress: StreamProgress| {
            if let Some(t) = progress.text {
                let _ = app.emit(
                    LIVE_TEXT_EVENT,
                    LiveText {
                        text: t.committed,
                        tentative: t.tentative,
                        ..template.clone()
                    },
                );
            }
        };
        let stream = self.side.as_ref().map(|side| {
            side.engine
                .start_stream(side.run.clone(), StreamOptions::default(), on_progress)
        });
        let decoder = match stream {
            Some(Ok(handle)) => Decoder::Stream(handle),
            Some(Err(e)) => {
                log::warn!("Notetaker live {} stream did not start: {e}", self.source);
                self.side = None;
                Decoder::Whole(Vec::new())
            }
            None => Decoder::Whole(Vec::new()),
        };
        self.current = Some(Current { start, decoder });
    }

    fn audio(&mut self, samples: Vec<f32>) {
        match self.current.as_mut().map(|c| &mut c.decoder) {
            Some(Decoder::Stream(handle)) => handle.feed(samples),
            Some(Decoder::Whole(buf)) => buf.extend(samples),
            None => {}
        }
    }

    fn end(&mut self, end: u64, voiced: u64) {
        let Some(current) = self.current.take() else {
            return;
        };
        self.last_speech = Instant::now();
        let enough = samples_to_ms(voiced) >= MIN_VOICED_MS;
        let text = match current.decoder {
            Decoder::Stream(handle) if enough => match handle.finalize(false) {
                Ok(Some(done)) => done.text.full,
                Ok(None) => {
                    log::warn!("Notetaker live {} stream was lost", self.source);
                    self.side = None;
                    String::new()
                }
                Err(e) => {
                    log::warn!("Notetaker live {} stream stopped: {e}", self.source);
                    String::new()
                }
            },
            Decoder::Whole(buf) if enough => {
                match transcribe::transcribe_piece(self.app(), buf, Some(self.stop)) {
                    Ok(text) => text,
                    Err(e) => {
                        log::debug!("Notetaker live {} utterance skipped: {e}", self.source);
                        String::new()
                    }
                }
            }
            _ => String::new(),
        };
        let text = text.trim();
        if !text.is_empty() {
            let start_ms = self.offset_ms + samples_to_ms(current.start);
            let segment = Segment {
                source: self.source.to_string(),
                speaker: transcribe::speaker_for(self.source).to_string(),
                start_ms,
                end_ms: self.offset_ms + samples_to_ms(end),
                text: text.to_string(),
            };
            let _ = self.manager.store.add_live_segment(self.id, &segment);
            let _ = NotetakerTranscript {
                meeting_id: self.id.to_string(),
                segments: vec![segment],
                replace: false,
            }
            .emit(self.app());
        }
        self.emit_text(current.start, "", "", false);
    }

    fn apply(&mut self, steps: Vec<Step>) {
        for step in steps {
            match step {
                Step::Begin { start } => self.begin(start),
                Step::Audio(samples) => self.audio(samples),
                Step::End { end, voiced } => self.end(end, voiced),
            }
        }
    }
}

fn score(scorer: &mut Option<Box<dyn SpeechScorer>>, frames: &[f32], frame: usize) -> Vec<f32> {
    match scorer.as_mut().map(|s| s.score(frames)) {
        Some(Ok(p)) => p,
        Some(Err(e)) => {
            log::warn!("Notetaker VAD failed, using loudness instead: {e}");
            *scorer = None;
            energy_probs(frames, frame)
        }
        None => energy_probs(frames, frame),
    }
}

/// Transcribes `feed` live until `stop`, then finishes the utterance in
/// progress. The microphone's worker loads at once (the user is about to
/// speak); system audio's only once the call plays something.
pub fn run(
    manager: &NotetakerManager,
    id: &str,
    source: &'static str,
    feed: SharedFeed,
    started: Instant,
    stop: &AtomicBool,
) {
    let mut scorer = transcribe::new_scorer(&manager.app);
    let frame = scorer.as_ref().map_or(512, |s| s.frame_samples());
    let mut gate = Gate::new(frame);
    let mut rest: Vec<f32> = Vec::new();
    let mut live = Source {
        manager,
        id,
        source,
        stop,
        offset_ms: 0,
        side: None,
        no_side: false,
        last_speech: Instant::now(),
        current: None,
    };
    let mut offset_known = false;
    let mut preloaded = false;

    loop {
        let stopping = stop.load(Ordering::Relaxed);
        let (samples, dropped, first_at) = {
            let mut f = feed.lock().unwrap_or_else(|e| e.into_inner());
            (
                std::mem::take(&mut f.pending),
                std::mem::take(&mut f.dropped),
                f.first_at,
            )
        };
        if !offset_known {
            if let Some(first) = first_at {
                offset_known = true;
                live.offset_ms = first.saturating_duration_since(started).as_millis() as i64;
                let (mic, system) = if source == "mic" {
                    (Some(live.offset_ms), None)
                } else {
                    (None, Some(live.offset_ms))
                };
                let _ = manager.store.set_offsets(id, mic, system);
            }
        }
        if !preloaded && (source == "mic" || samples.iter().any(|&s| s != 0.0)) {
            preloaded = true;
            live.ensure_side();
        }
        if dropped > 0 {
            log::warn!(
                "Notetaker live transcript skipped {} s of {source} audio to catch up",
                samples_to_ms(dropped) / 1000
            );
            if let Some(step) = gate.flush() {
                live.apply(vec![step]);
            }
            gate.skip(rest.len() as u64 + dropped);
            rest.clear();
        }
        rest.extend_from_slice(&samples);
        let whole = rest.len() / frame * frame;
        if whole > 0 {
            let frames: Vec<f32> = rest.drain(..whole).collect();
            let probs = score(&mut scorer, &frames, frame);
            let steps = gate.push(&frames, &probs);
            live.apply(steps);
        }
        if stopping {
            if let Some(step) = gate.flush() {
                live.apply(vec![step]);
            }
            break;
        }
        if live.side.is_some()
            && live.current.is_none()
            && live.last_speech.elapsed() >= IDLE_UNLOAD
        {
            log::debug!("Notetaker live {source} worker idle; stopping it");
            live.side = None;
        }
        std::thread::sleep(POLL);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const F: usize = 512;

    fn run_gate(probs: &[f32]) -> Vec<Step> {
        let mut gate = Gate::new(F);
        let frames = vec![0.1f32; probs.len() * F];
        let mut steps = gate.push(&frames, probs);
        steps.extend(gate.flush());
        steps
    }

    fn frames_for(ms_: u64) -> usize {
        (ms(ms_) as usize).div_ceil(F)
    }

    #[test]
    fn utterance_begins_with_preroll_and_ends_after_a_pause() {
        let mut probs = vec![0.0; 20];
        probs.extend(vec![0.9; 30]);
        probs.extend(vec![0.0; frames_for(END_PAUSE_MS) + 5]);
        let steps = run_gate(&probs);
        let begin = 20 * F as u64 - ms(PREROLL_MS) / F as u64 * F as u64;
        assert!(matches!(steps[0], Step::Begin { start } if start <= 20 * F as u64 && start >= begin - F as u64));
        let ends: Vec<_> = steps
            .iter()
            .filter_map(|s| match s {
                Step::End { end, voiced } => Some((*end, *voiced)),
                _ => None,
            })
            .collect();
        assert_eq!(ends.len(), 1);
        assert_eq!(ends[0].1, 30 * F as u64);
        assert_eq!(ends[0].0, 50 * F as u64 + ms(TRAIL_PAD_MS));
    }

    #[test]
    fn long_speech_is_cut_at_the_maximum() {
        let probs = vec![0.9; frames_for(MAX_MS) * 2 + 3];
        let steps = run_gate(&probs);
        let begins = steps.iter().filter(|s| matches!(s, Step::Begin { .. })).count();
        let ends = steps.iter().filter(|s| matches!(s, Step::End { .. })).count();
        assert_eq!(begins, 3);
        assert_eq!(ends, 3);
    }

    #[test]
    fn audio_fed_matches_the_utterance() {
        let mut probs = vec![0.0; 3];
        probs.extend(vec![0.9; 10]);
        let steps = run_gate(&probs);
        let fed: usize = steps
            .iter()
            .map(|s| match s {
                Step::Audio(a) => a.len(),
                _ => 0,
            })
            .sum();
        assert_eq!(fed, 13 * F);
        assert!(matches!(steps[0], Step::Begin { start: 0 }));
    }

    #[test]
    fn skip_keeps_the_timeline() {
        let mut gate = Gate::new(F);
        gate.push(&vec![0.0; 4 * F], &[0.0; 4]);
        gate.skip(10 * F as u64);
        let steps = gate.push(&vec![0.1; F], &[0.9]);
        assert!(matches!(steps[0], Step::Begin { start } if start == 14 * F as u64));
    }
}
