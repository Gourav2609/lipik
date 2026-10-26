use anyhow::{anyhow, Result};
use std::collections::HashSet;
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, TryLockError};
use std::time::Duration;
use tauri::{AppHandle, Manager};

use super::audio::{samples_to_ms, Piece, SpeechTrack, RATE};
use super::Segment;
use crate::audio_toolkit::SpeechScorer;
use crate::managers::audio::AudioRecordingManager;
use crate::managers::transcription::TranscriptionManager;
use crate::settings::get_settings;

static ASR_LOCK: Mutex<()> = Mutex::new(());

pub const ME: &str = "Me";
pub const THEM: &str = "Them";

pub fn speaker_for(source: &str) -> &'static str {
    if source == "mic" {
        ME
    } else {
        THEM
    }
}

pub fn new_scorer(app: &AppHandle) -> Option<Box<dyn SpeechScorer>> {
    let backend = get_settings(app).vad_backend;
    match crate::managers::audio::create_detector(backend, app) {
        Ok(scorer) => Some(scorer),
        Err(e) => {
            log::warn!("Notetaker VAD unavailable, using loudness: {e}");
            None
        }
    }
}

fn dictation_busy(app: &AppHandle) -> bool {
    let recording = app
        .try_state::<Arc<AudioRecordingManager>>()
        .is_some_and(|a| a.is_recording());
    let streaming = app
        .try_state::<Arc<TranscriptionManager>>()
        .is_some_and(|t| t.is_streaming());
    recording || streaming
}

/// Waits until no dictation is recording or streaming and no other notetaker
/// piece is being decoded. `cancel` ends the wait early.
fn wait_for_turn(app: &AppHandle, cancel: Option<&AtomicBool>) -> Result<MutexGuard<'static, ()>> {
    loop {
        if cancel.is_some_and(|c| c.load(Ordering::Relaxed)) {
            return Err(anyhow!("cancelled"));
        }
        if !dictation_busy(app) {
            match ASR_LOCK.try_lock() {
                Ok(guard) => return Ok(guard),
                Err(TryLockError::Poisoned(e)) => return Ok(e.into_inner()),
                Err(TryLockError::WouldBlock) => {}
            }
        }
        std::thread::sleep(Duration::from_millis(100));
    }
}

pub fn transcription_manager(app: &AppHandle) -> Result<Arc<TranscriptionManager>> {
    Ok(app
        .try_state::<Arc<TranscriptionManager>>()
        .ok_or_else(|| anyhow!("Transcription is not ready"))?
        .inner()
        .clone())
}

/// Transcribes one piece with the dictation model, never delaying a
/// dictation. The model stays loaded between pieces; call `release_model`
/// when the meeting is done.
pub fn transcribe_piece(
    app: &AppHandle,
    audio: Vec<f32>,
    cancel: Option<&AtomicBool>,
) -> Result<String> {
    let _turn = wait_for_turn(app, cancel)?;
    let tm = transcription_manager(app)?;
    tm.initiate_model_load();
    match tm.transcribe_keep_loaded(audio.clone()) {
        Ok(text) => Ok(text),
        Err(first) => {
            log::warn!("Notetaker transcription failed, retrying once: {first}");
            std::thread::sleep(Duration::from_millis(500));
            tm.initiate_model_load();
            tm.transcribe_keep_loaded(audio)
        }
    }
}

/// Applies the user's "unload immediately" setting once the notetaker has no
/// more pieces to decode.
pub fn release_model(app: &AppHandle) {
    if dictation_busy(app) {
        return;
    }
    if let Ok(tm) = transcription_manager(app) {
        tm.maybe_unload_immediately("notetaker");
    }
}

pub fn piece_segment(piece: &Piece, offset_ms: i64, source: &str, text: String) -> Segment {
    let start_ms = offset_ms + samples_to_ms(piece.start_sample);
    Segment {
        source: source.to_string(),
        speaker: speaker_for(source).to_string(),
        start_ms,
        end_ms: start_ms + samples_to_ms(piece.audio.len() as u64),
        text,
    }
}

/// Every piece of speech in a finished stream WAV, transcribed. Reads the file
/// in blocks so a long meeting is never held in memory. `progress` gets the
/// fraction of the file done.
pub fn transcribe_file(
    app: &AppHandle,
    path: &Path,
    offset_ms: i64,
    source: &str,
    mut progress: impl FnMut(f32),
) -> Result<Vec<Segment>> {
    let mut reader = hound::WavReader::open(path)?;
    let total = reader.duration().max(1) as f32;
    let mut track = SpeechTrack::new(new_scorer(app));
    let mut out = Vec::new();
    let mut block = Vec::with_capacity(8 * RATE);
    let mut read = 0usize;
    let mut samples = reader.samples::<i16>();
    loop {
        block.clear();
        for s in samples.by_ref().take(8 * RATE) {
            block.push(s? as f32 / i16::MAX as f32);
        }
        let done = block.is_empty();
        read += block.len();
        track.push(&block);
        while let Some(piece) = track.take(super::audio::FINAL_RULES, done) {
            let text = transcribe_piece(app, piece.audio.clone(), None)?;
            if !text.trim().is_empty() {
                out.push(piece_segment(
                    &piece,
                    offset_ms,
                    source,
                    text.trim().to_string(),
                ));
            }
            progress(read as f32 / total);
        }
        if done {
            break;
        }
    }
    progress(1.0);
    Ok(out)
}

/// Short or very common words that say nothing about where a line came from.
const COMMON: &[&str] = &[
    "a", "an", "the", "and", "or", "but", "so", "to", "of", "in", "on", "at", "for", "with", "is",
    "it", "its", "that", "this", "i", "you", "we", "they", "he", "she", "me", "my", "be", "do",
    "have", "yes", "yeah", "no", "ok", "okay", "right", "um", "uh", "like", "just", "think",
    "know", "s", "t",
];
/// Their words are placed in time by spreading a line evenly over its span;
/// this much slack covers that guess.
const ECHO_SLACK_MS: i64 = 2_000;

fn word_list(text: &str) -> Vec<String> {
    text.split(|c: char| !c.is_alphanumeric())
        .filter(|w| !w.is_empty())
        .map(|w| w.to_lowercase())
        .collect()
}

/// A mic line repeats what the speakers played at the same moment: most of
/// its word pairs are in the system words spoken during its time span. Short
/// replies ("yeah", "I think so") are never treated as echo.
fn is_echo(mic: &Segment, system: &[(&Segment, Vec<String>)]) -> bool {
    let mine = word_list(&mic.text);
    let meaningful = mine
        .iter()
        .filter(|w| !COMMON.contains(&w.as_str()))
        .count();
    if mine.len() < 4 || meaningful < 2 {
        return false;
    }
    let mut theirs: HashSet<(&str, &str)> = HashSet::new();
    for (s, words) in system {
        let overlap = mic.end_ms.min(s.end_ms) - mic.start_ms.max(s.start_ms);
        if overlap <= -ECHO_SLACK_MS || words.len() < 2 {
            continue;
        }
        let span = (s.end_ms - s.start_ms).max(1);
        let n = words.len() as i64;
        let at = |ms: i64| ((ms - s.start_ms) * n / span).clamp(0, n) as usize;
        let from = at(mic.start_ms - ECHO_SLACK_MS);
        let to = (at(mic.end_ms + ECHO_SLACK_MS) + 1).min(words.len());
        if to > from {
            theirs.extend(
                words[from..to]
                    .windows(2)
                    .map(|p| (p[0].as_str(), p[1].as_str())),
            );
        }
    }
    let pairs = mine.len() - 1;
    let shared = mine
        .windows(2)
        .filter(|p| theirs.contains(&(p[0].as_str(), p[1].as_str())))
        .count();
    shared as f32 / pairs as f32 >= 0.6
}

/// Mic and system segments in time order. A mic segment that repeats what the
/// speakers played at the same time (no headphones) is dropped.
pub fn merge_streams(mic: Vec<Segment>, system: Vec<Segment>) -> Vec<Segment> {
    let system_words: Vec<(&Segment, Vec<String>)> =
        system.iter().map(|s| (s, word_list(&s.text))).collect();
    let mut merged: Vec<Segment> = mic
        .into_iter()
        .filter(|m| !word_list(&m.text).is_empty() && !is_echo(m, &system_words))
        .collect();
    drop(system_words);
    merged.extend(system);
    merged.sort_by_key(|s| (s.start_ms, s.end_ms));
    merged
}

#[cfg(test)]
mod tests {
    use super::*;

    fn seg(source: &str, start: i64, end: i64, text: &str) -> Segment {
        Segment {
            source: source.into(),
            speaker: speaker_for(source).into(),
            start_ms: start,
            end_ms: end,
            text: text.into(),
        }
    }

    #[test]
    fn merge_orders_and_drops_echo() {
        let mic = vec![
            seg("mic", 0, 4_000, "Hi everyone, let's start"),
            seg(
                "mic",
                10_000,
                14_000,
                "we should ship the release on friday",
            ),
        ];
        let system = vec![
            seg("system", 5_000, 9_000, "Sounds good to me"),
            seg(
                "system",
                9_500,
                15_000,
                "We should ship the release on Friday, right?",
            ),
        ];
        let merged = merge_streams(mic, system);
        let texts: Vec<&str> = merged.iter().map(|s| s.text.as_str()).collect();
        assert_eq!(
            texts,
            vec![
                "Hi everyone, let's start",
                "Sounds good to me",
                "We should ship the release on Friday, right?"
            ]
        );
        assert_eq!(merged[0].speaker, ME);
        assert_eq!(merged[1].speaker, THEM);
    }

    #[test]
    fn merge_keeps_short_replies_and_other_words() {
        let system = vec![seg(
            "system",
            0,
            25_000,
            "So I think we should move the launch, okay, yeah, and I think so too, then we can review the budget numbers next week with finance",
        )];
        let mic = vec![
            seg("mic", 3_000, 4_000, "yeah okay"),
            seg("mic", 5_000, 6_000, "I think so"),
            seg(
                "mic",
                20_000,
                24_000,
                "the launch should move to march instead",
            ),
        ];
        let merged = merge_streams(mic, system);
        assert_eq!(merged.iter().filter(|s| s.source == "mic").count(), 3);
    }
}
