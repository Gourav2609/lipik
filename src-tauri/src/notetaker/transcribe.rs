use anyhow::{anyhow, Result};
use std::collections::HashSet;
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, TryLockError};
use std::time::Duration;
use tauri::{AppHandle, Manager};

use super::audio::{samples_to_ms, Piece, SpeechTrack, RATE};
use super::diarize::{self, Speakers};
use super::Segment;
use crate::audio_toolkit::SpeechScorer;
use crate::managers::audio::AudioRecordingManager;
use crate::managers::transcription::{SideEngine, TranscriptionManager};
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

/// Transcribes one piece with a model loaded beside dictation, never
/// delaying a dictation.
pub fn transcribe_side(app: &AppHandle, side: &SideEngine, audio: Vec<f32>) -> Result<String> {
    let _turn = wait_for_turn(app, None)?;
    side.engine
        .transcribe(audio, side.run.clone())
        .map(|t| t.text.trim().to_string())
        .map_err(|e| anyhow!("{e}"))
}

/// Up to `count` lines spread over the meeting, each at least `min_ms` long.
fn spread<'a>(segments: &'a [Segment], count: usize, min_ms: i64) -> Vec<&'a Segment> {
    let long: Vec<&Segment> = segments
        .iter()
        .filter(|s| s.end_ms - s.start_ms >= min_ms)
        .collect();
    if long.len() <= count {
        return long;
    }
    (0..count)
        .map(|i| long[(2 * i + 1) * long.len() / (2 * count)])
        .collect()
}

/// The audio of `[start_ms, end_ms)` from a stream WAV that began at
/// `offset_ms` on the meeting's clock.
fn read_span(path: &Path, offset_ms: i64, start_ms: i64, end_ms: i64) -> Result<Vec<f32>> {
    let mut reader = hound::WavReader::open(path)?;
    let to_sample = |ms: i64| ((ms - offset_ms).max(0) as u64 * RATE as u64 / 1000) as u32;
    let from = to_sample(start_ms).min(reader.duration());
    let len = to_sample(end_ms).saturating_sub(from) as usize;
    reader.seek(from)?;
    reader
        .samples::<i16>()
        .take(len)
        .map(|s| Ok(s? as f32 / i16::MAX as f32))
        .collect()
}

/// Every line heard again by `hear` from its stream's audio, in pieces cut
/// by `rules`, each piece keeping its line's speaker key and label. A piece
/// `hear` fails on counts as heard empty; a line whose audio can't be read
/// or in which nothing is heard keeps its text, and one without text (a
/// stretch the first pass wrote nothing for) is left out. A piece heard in
/// such a stretch is left out too when `is_echo` says it is the call heard
/// by the mic. `streams` maps a source to its WAV and offset; `scorer` makes
/// the speech detector for each line. `keep_going` gets the share of the
/// lines' time done and can stop the rest, which then keep their text.
pub fn redo_lines(
    lines: &[Segment],
    streams: &[(&str, &Path, i64)],
    rules: super::audio::CutRules,
    hints: &[String],
    scorer: impl Fn() -> Option<Box<dyn SpeechScorer>>,
    mut hear: impl FnMut(Vec<f32>) -> Result<String>,
    mut is_echo: impl FnMut(&Segment) -> bool,
    mut keep_going: impl FnMut(f32) -> bool,
) -> Vec<Segment> {
    let keep = |line: &Segment, out: &mut Vec<Segment>| {
        if !line.text.trim().is_empty() {
            out.push(line.clone());
        }
    };
    let total: i64 = lines.iter().map(|s| (s.end_ms - s.start_ms).max(0)).sum();
    let mut done = 0;
    let mut out = Vec::new();
    for (n, line) in lines.iter().enumerate() {
        if !keep_going(done as f32 / total.max(1) as f32) {
            for line in &lines[n..] {
                keep(line, &mut out);
            }
            break;
        }
        done += (line.end_ms - line.start_ms).max(0);
        let audio = streams
            .iter()
            .find(|(source, ..)| *source == line.source)
            .ok_or_else(|| anyhow!("no audio"))
            .and_then(|(_, path, offset)| read_span(path, *offset, line.start_ms, line.end_ms));
        let audio = match audio {
            Ok(audio) => audio,
            Err(e) => {
                log::warn!("Notetaker kept a line it could not read again: {e}");
                keep(line, &mut out);
                continue;
            }
        };
        let stretch = line.text.trim().is_empty();
        let mut track = SpeechTrack::new(scorer());
        track.push(&audio);
        let mut heard = Vec::new();
        while let Some(piece) = track.take(rules, true) {
            let text = match hear(piece.audio.clone()) {
                Ok(text) => text,
                Err(e) => {
                    log::warn!("Notetaker could not hear a piece again: {e}");
                    continue;
                }
            };
            if text.trim().is_empty() {
                continue;
            }
            let start_ms = line.start_ms + samples_to_ms(piece.start_sample);
            let segment = Segment {
                start_ms,
                end_ms: (start_ms + samples_to_ms(piece.audio.len() as u64)).min(line.end_ms),
                text: text.trim().to_string(),
                ..line.clone()
            };
            if stretch && is_echo(&segment) {
                continue;
            }
            if super::tidy::heard_nothing(&segment.text, &piece, track.has_detector(), hints) {
                continue;
            }
            heard.push(segment);
        }
        if heard.is_empty() {
            keep(line, &mut out);
        } else {
            out.extend(heard);
        }
    }
    out
}

/// Longest stretch without lines heard again in one go.
const UNHEARD_MAX_MS: i64 = 120_000;
/// Shorter stretches between lines are not heard again.
const UNHEARD_MIN_MS: i64 = 2_000;

/// `lines` and, for each of `streams` (source, and where its audio starts
/// and ends on the meeting's clock), a line with no text over every stretch
/// of 2 s or more its lines leave uncovered, so hearing the lines again also
/// hears speech the first pass wrote nothing for. In time order. Call
/// stretches are cut where `call_turns` (the voice split, on the meeting's
/// clock) change speaker and take that speaker; parts no turn covers, and
/// stretches nobody split, are "Them". Mic stretches are cut the same way
/// by `mic_turns` (the people in the room) and are otherwise "Me". Longer
/// ones are cut into equal parts of at most two minutes.
pub fn with_unheard_spans(
    lines: &[Segment],
    streams: &[(&str, i64, i64)],
    call_turns: &[diarize::Turn],
    mic_turns: &[diarize::Turn],
) -> Vec<Segment> {
    let mut out = lines.to_vec();
    for &(source, stream_start, stream_end) in streams {
        let mut own: Vec<&Segment> = lines.iter().filter(|s| s.source == source).collect();
        own.sort_by_key(|s| s.start_ms);
        let mut gaps = Vec::new();
        let mut from = stream_start;
        for s in &own {
            if s.start_ms > from {
                gaps.push((from, s.start_ms.min(stream_end)));
            }
            from = from.max(s.end_ms);
        }
        gaps.push((from, stream_end));
        let turns = if source == "system" {
            call_turns
        } else {
            mic_turns
        };
        for (start, end) in gaps {
            if end - start < UNHEARD_MIN_MS {
                continue;
            }
            for (at, to, speaker) in by_speaker(start, end, turns) {
                let (speaker_key, speaker) = match speaker {
                    Some(n) => diarize::key_and_label(n),
                    None => (
                        diarize::key_for(source).to_string(),
                        speaker_for(source).to_string(),
                    ),
                };
                let count = (to - at + UNHEARD_MAX_MS - 1) / UNHEARD_MAX_MS;
                for n in 0..count {
                    out.push(Segment {
                        source: source.to_string(),
                        speaker: speaker.clone(),
                        speaker_key: speaker_key.clone(),
                        start_ms: at + (to - at) * n / count,
                        end_ms: at + (to - at) * (n + 1) / count,
                        text: String::new(),
                    });
                }
            }
        }
    }
    out.sort_by(|a, b| (a.start_ms, &a.source).cmp(&(b.start_ms, &b.source)));
    out
}

/// Parts of a stretch shorter than this are not heard on their own.
const SHORT_PART_MS: i64 = 2_000;
/// How far a short part of one speaker is widened either way, so words at
/// its edges are heard whole.
const SHORT_PART_PAD_MS: i64 = 250;

/// `start..end` cut where `turns` begin and end, each part with the speaker
/// whose turn covers it (None where no turn does), neighbours of one speaker
/// joined. A short part no turn covers goes to the part before it (else the
/// one after); a short part that is still alone is widened by
/// [`SHORT_PART_PAD_MS`] each way within `start..end`.
fn by_speaker(start: i64, end: i64, turns: &[diarize::Turn]) -> Vec<(i64, i64, Option<usize>)> {
    let mut cuts = vec![start, end];
    for t in turns {
        cuts.extend(
            [t.start_ms, t.end_ms]
                .into_iter()
                .filter(|&c| c > start && c < end),
        );
    }
    cuts.sort_unstable();
    cuts.dedup();
    let mut parts: Vec<(i64, i64, Option<usize>)> = Vec::new();
    for w in cuts.windows(2) {
        let (a, b) = (w[0], w[1]);
        let speaker = turns
            .iter()
            .filter(|t| t.start_ms < b && t.end_ms > a)
            .max_by_key(|t| b.min(t.end_ms) - a.max(t.start_ms))
            .map(|t| t.speaker)
            .filter(|&n| n != diarize::UNKNOWN);
        parts.push((a, b, speaker));
    }
    for i in 0..parts.len() {
        let (a, b, speaker) = parts[i];
        if speaker.is_none() && b - a < SHORT_PART_MS {
            let neighbour = if i > 0 {
                parts.get(i - 1)
            } else {
                parts.get(i + 1)
            };
            if let Some(&(.., n)) = neighbour {
                parts[i].2 = n;
            }
        }
    }
    let mut joined: Vec<(i64, i64, Option<usize>)> = Vec::new();
    for part in parts {
        match joined.last_mut() {
            Some(last) if last.2 == part.2 => last.1 = part.1,
            _ => joined.push(part),
        }
    }
    for part in &mut joined {
        if part.2.is_some() && part.1 - part.0 < SHORT_PART_MS {
            part.0 = (part.0 - SHORT_PART_PAD_MS).max(start);
            part.1 = (part.1 + SHORT_PART_PAD_MS).min(end);
        }
    }
    joined
}

/// Searched either side of the measured delay when checking a mic piece for
/// the call: the delay drifts during a call.
const ECHO_SEARCH_MS: i64 = 1_000;

/// Whether `piece` (meeting clock) of the mic is the call it heard, judged
/// from the raw and the cleaned mic audio (both starting at `mic.2`) against
/// the call `delay_ms` earlier; see [`super::echo::is_echo_piece`].
pub fn call_in_mic(
    mic: (&Path, &Path, i64),
    system: (&Path, i64),
    delay_ms: i64,
    piece: &Segment,
) -> bool {
    let (raw, cleaned, offset) = mic;
    let read = |path: &Path| read_span(path, offset, piece.start_ms.max(offset), piece.end_ms);
    let (heard, left) = match (read(raw), read(cleaned)) {
        (Ok(heard), Ok(left)) => (heard, left),
        _ => return false,
    };
    let from = piece.start_ms.max(offset) - delay_ms - ECHO_SEARCH_MS;
    let lead = (system.1 - from).max(0);
    let played = match read_span(
        system.0,
        system.1,
        from + lead,
        piece.end_ms - delay_ms + ECHO_SEARCH_MS,
    ) {
        Ok(played) => played,
        Err(_) => return false,
    };
    let mut call = vec![0.0; (lead.max(0) as usize) * RATE / 1000];
    call.extend(played);
    super::echo::is_echo_piece(&heard, &left, &call)
}

/// Share of Devanagari words when `hear` (Qwen3-ASR) hears a few lines
/// spread over the meeting. `streams` maps a source to its WAV and offset.
pub fn hindi_in_sample(
    live: &[Segment],
    streams: &[(&str, &Path, i64)],
    mut hear: impl FnMut(Vec<f32>) -> Result<String>,
) -> f32 {
    let mut heard = Vec::new();
    for s in spread(
        live,
        super::hinglish::PROBE_PIECES,
        super::hinglish::PROBE_MIN_MS,
    ) {
        let Some((_, path, offset)) = streams.iter().find(|(source, ..)| *source == s.source)
        else {
            continue;
        };
        let end = s.end_ms.min(s.start_ms + super::hinglish::PROBE_MAX_MS);
        match read_span(path, *offset, s.start_ms, end).and_then(&mut hear) {
            Ok(text) => heard.push(text),
            Err(e) => log::warn!("Notetaker could not sample a piece for Hindi: {e}"),
        }
    }
    super::hinglish::devanagari_share(heard.iter().map(String::as_str))
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

/// A transcribed piece as a line. With `speakers` (system audio split by
/// voice), the line goes to the speaker heard most during the piece.
pub fn piece_segment(
    piece: &Piece,
    offset_ms: i64,
    source: &str,
    text: String,
    speakers: Option<&Speakers>,
) -> Segment {
    let wav_start = samples_to_ms(piece.start_sample);
    let wav_end = wav_start + samples_to_ms(piece.audio.len() as u64);
    let (speaker_key, speaker) = match speakers.and_then(|s| s.speaker_at(wav_start, wav_end)) {
        Some(n) => diarize::key_and_label(n),
        None => (
            diarize::key_for(source).to_string(),
            speaker_for(source).to_string(),
        ),
    };
    Segment {
        source: source.to_string(),
        speaker,
        speaker_key,
        start_ms: offset_ms + wav_start,
        end_ms: offset_ms + wav_end,
        text,
    }
}

/// Every piece of speech in a finished stream WAV, transcribed. Reads the file
/// in blocks so a long meeting is never held in memory. `progress` gets the
/// fraction of the file done. With `speakers`, pieces also end where the
/// speaker changes and each line is labelled with its speaker. With
/// `hinglish` (Qwen3-ASR), pieces are shorter and go to it instead of the
/// dictation model. Also returns how much speech the pieces held, in ms.
pub fn transcribe_file(
    app: &AppHandle,
    path: &Path,
    offset_ms: i64,
    source: &str,
    speakers: Option<&Speakers>,
    hinglish: Option<&SideEngine>,
    mut progress: impl FnMut(f32),
) -> Result<(Vec<Segment>, i64)> {
    let rules = match hinglish {
        Some(_) => super::hinglish::RULES,
        None => super::audio::FINAL_RULES,
    };
    let mut reader = hound::WavReader::open(path)?;
    let total = reader.duration().max(1) as f32;
    let mut track = SpeechTrack::new(new_scorer(app));
    if let Some(s) = speakers {
        track.set_cuts(s.change_samples());
    }
    let mut out = Vec::new();
    let hints = hinglish.map_or(&[][..], |side| side.run.vocabulary.as_slice());
    let mut speech = 0u64;
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
        while let Some(piece) = track.take(rules, done) {
            speech += piece.speech_samples;
            let text = match hinglish {
                Some(side) => transcribe_side(app, side, piece.audio.clone()).unwrap_or_else(|e| {
                    log::warn!("Notetaker skipped a piece Qwen3-ASR could not decode: {e}");
                    String::new()
                }),
                None => transcribe_piece(app, piece.audio.clone(), None)?,
            };
            if !text.trim().is_empty() {
                if !super::tidy::heard_nothing(&text, &piece, track.has_detector(), hints) {
                    out.push(piece_segment(
                        &piece,
                        offset_ms,
                        source,
                        text.trim().to_string(),
                        speakers,
                    ));
                }
            }
            progress(read as f32 / total);
        }
        if done {
            break;
        }
    }
    progress(1.0);
    Ok((out, samples_to_ms(speech)))
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
            speaker_key: diarize::key_for(source).into(),
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
    fn pieces_take_the_speaker_heard_most() {
        let speakers = diarize::tidy(&[(0, 30_000, 0), (30_000, 60_000, 1)], &[None, None]);
        let piece = Piece {
            start_sample: 28 * RATE as u64,
            audio: vec![0.0; 10 * RATE],
            speech_samples: 0,
            peak: 0.0,
        };
        let s = piece_segment(&piece, 1_500, "system", "hi".into(), Some(&speakers));
        assert_eq!(
            (s.speaker.as_str(), s.speaker_key.as_str()),
            ("Speaker 2", "s2")
        );
        assert_eq!((s.start_ms, s.end_ms), (29_500, 39_500));
        let plain = piece_segment(&piece, 0, "system", "hi".into(), None);
        assert_eq!(
            (plain.speaker.as_str(), plain.speaker_key.as_str()),
            ("Them", "them")
        );
        let mic = piece_segment(&piece, 0, "mic", "hi".into(), None);
        assert_eq!(mic.speaker_key, "me");
    }

    #[test]
    fn redone_lines_keep_their_speakers() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("system.wav");
        let spec = hound::WavSpec {
            channels: 1,
            sample_rate: RATE as u32,
            bits_per_sample: 16,
            sample_format: hound::SampleFormat::Int,
        };
        let mut wav = hound::WavWriter::create(&path, spec).unwrap();
        for i in 0..30 * RATE {
            let loud = (i / RATE) % 6 != 5;
            let x = if loud {
                (i as f32 * 0.3).sin() * 8_000.0
            } else {
                0.0
            };
            wav.write_sample(x as i16).unwrap();
        }
        wav.finalize().unwrap();
        let mut renamed = seg("system", 1_000, 25_000, "old words");
        renamed.speaker = "Priya".into();
        renamed.speaker_key = "s2".into();
        let lines = vec![renamed, seg("mic", 0, 2_000, "kept")];
        let mut heard = 0;
        let out = redo_lines(
            &lines,
            &[("system", path.as_path(), 0)],
            crate::notetaker::hinglish::RULES,
            &[],
            || None,
            |_| {
                heard += 1;
                if heard == 2 {
                    return Err(anyhow!("output began repeating"));
                }
                Ok(format!("piece {heard}"))
            },
            |_| false,
            |_| true,
        );
        let system: Vec<&Segment> = out.iter().filter(|s| s.source == "system").collect();
        assert!(system.len() >= 3, "{} pieces", system.len());
        assert!(system
            .iter()
            .all(|s| s.speaker == "Priya" && s.speaker_key == "s2"));
        assert!(system
            .iter()
            .all(|s| s.start_ms >= 1_000 && s.end_ms <= 25_000 && s.end_ms - s.start_ms <= 10_500));
        assert_eq!(out.last().unwrap().text, "kept");
    }

    #[test]
    fn the_sample_measures_devanagari_in_what_was_heard() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("system.wav");
        let spec = hound::WavSpec {
            channels: 1,
            sample_rate: RATE as u32,
            bits_per_sample: 16,
            sample_format: hound::SampleFormat::Int,
        };
        let mut wav = hound::WavWriter::create(&path, spec).unwrap();
        for i in 0..60 * RATE {
            wav.write_sample(((i as f32 * 0.3).sin() * 8_000.0) as i16)
                .unwrap();
        }
        wav.finalize().unwrap();
        let lines: Vec<Segment> = (0..10)
            .map(|i| seg("system", i * 6_000, i * 6_000 + 5_000, "x"))
            .collect();
        let streams = [("system", path.as_path(), 0)];
        let mut asked = 0;
        let english = hindi_in_sample(&lines, &streams, |_| {
            asked += 1;
            Ok("so we ship the release on friday".into())
        });
        assert_eq!(asked, crate::notetaker::hinglish::PROBE_PIECES);
        assert!(english < crate::notetaker::hinglish::PROBE_HINDI);
        let hinglish = hindi_in_sample(&lines, &streams, |_| Ok("तो हम friday को ship करेंगे".into()));
        assert!(hinglish >= crate::notetaker::hinglish::PROBE_HINDI);
    }

    #[test]
    fn a_stopped_redo_keeps_the_lines_left() {
        let lines = vec![
            seg("mic", 0, 2_000, "one"),
            seg("mic", 2_000, 4_000, "two"),
            seg("mic", 4_000, 6_000, "three"),
        ];
        let out = redo_lines(
            &lines,
            &[],
            crate::notetaker::hinglish::RULES,
            &[],
            || None,
            |_| Ok("new".into()),
            |_| false,
            |done| done < 0.3,
        );
        let texts: Vec<&str> = out.iter().map(|s| s.text.as_str()).collect();
        assert_eq!(texts, vec!["one", "two", "three"]);
    }

    fn turn(start_ms: i64, end_ms: i64, speaker: usize) -> diarize::Turn {
        diarize::Turn {
            start_ms,
            end_ms,
            speaker,
        }
    }

    fn spans(lines: &[Segment]) -> Vec<(&str, i64, i64, &str, &str)> {
        lines
            .iter()
            .filter(|s| s.text.is_empty())
            .map(|s| {
                (
                    s.source.as_str(),
                    s.start_ms,
                    s.end_ms,
                    s.speaker_key.as_str(),
                    s.speaker.as_str(),
                )
            })
            .collect()
    }

    #[test]
    fn stretches_take_their_speakers_from_the_voice_split() {
        let mut named = seg("system", 20_000, 24_000, "hello");
        named.speaker = "Priya Nair".into();
        named.speaker_key = "s2".into();
        let lines = vec![named];
        // Call audio from 5 s to 200 s on the meeting clock. Speaker 1 talks
        // 5-12 s, nobody 12-15 s, speaker 2 (Priya) 15-20 s and 24-60 s,
        // speaker 3 from 60 s to 190 s.
        let turns = [
            turn(5_000, 12_000, 0),
            turn(15_000, 20_000, 1),
            turn(24_000, 60_000, 1),
            turn(60_000, 190_000, 2),
        ];
        let all = with_unheard_spans(&lines, &[("system", 5_000, 200_000)], &turns, &[]);
        assert_eq!(
            spans(&all),
            vec![
                ("system", 5_000, 12_000, "s1", "Speaker 1"),
                ("system", 12_000, 15_000, "them", THEM),
                ("system", 15_000, 20_000, "s2", "Speaker 2"),
                ("system", 24_000, 60_000, "s2", "Speaker 2"),
                ("system", 60_000, 125_000, "s3", "Speaker 3"),
                ("system", 125_000, 190_000, "s3", "Speaker 3"),
                ("system", 190_000, 200_000, "them", THEM),
            ]
        );
    }

    #[test]
    fn short_call_stretches_join_their_speaker_or_widen() {
        let lines = vec![
            seg("system", 0, 10_000, "pehle"),
            seg("system", 30_000, 40_000, "baad"),
        ];
        // Speaker 1 pauses half a second at 15 s, speaker 2 says one word at
        // 20-21 s, and nobody talks in the last 0.4 s of the stretch.
        let turns = [
            turn(10_000, 15_000, 0),
            turn(15_500, 20_000, 0),
            turn(20_000, 21_000, 1),
            turn(21_000, 29_600, 0),
        ];
        let all = with_unheard_spans(&lines, &[("system", 0, 40_000)], &turns, &[]);
        assert_eq!(
            spans(&all),
            vec![
                ("system", 10_000, 20_000, "s1", "Speaker 1"),
                ("system", 19_750, 21_250, "s2", "Speaker 2"),
                ("system", 21_000, 30_000, "s1", "Speaker 1"),
            ]
        );
    }

    #[test]
    fn stretches_start_where_each_stream_starts_and_mic_ones_are_me() {
        let lines = vec![seg("mic", 3_000, 9_000, "one")];
        let all = with_unheard_spans(
            &lines,
            &[("mic", 1_500, 12_000), ("system", 2_000, 8_000)],
            &[],
            &[],
        );
        assert_eq!(
            spans(&all),
            vec![
                ("system", 2_000, 8_000, "them", THEM),
                ("mic", 9_000, 12_000, "me", ME),
            ]
        );
        let starts: Vec<i64> = all.iter().map(|s| s.start_ms).collect();
        assert!(starts.windows(2).all(|w| w[0] <= w[1]), "{starts:?}");
    }

    #[test]
    fn mic_stretches_take_the_people_in_the_room() {
        let lines = vec![seg("mic", 0, 4_000, "pehle")];
        // The user talks until 10 s, then someone in the room (speaker 2,
        // after one call voice) until 20 s.
        let room = [turn(4_000, 10_000, diarize::ME), turn(10_000, 20_000, 1)];
        let all = with_unheard_spans(&lines, &[("mic", 0, 20_000)], &[], &room);
        assert_eq!(
            spans(&all),
            vec![
                ("mic", 4_000, 10_000, "me", ME),
                ("mic", 10_000, 20_000, "s2", "Speaker 2"),
            ]
        );
    }

    fn tone_wav(dir: &Path, name: &str, seconds: usize) -> std::path::PathBuf {
        let path = dir.join(name);
        let spec = hound::WavSpec {
            channels: 1,
            sample_rate: RATE as u32,
            bits_per_sample: 16,
            sample_format: hound::SampleFormat::Int,
        };
        let mut wav = hound::WavWriter::create(&path, spec).unwrap();
        for i in 0..seconds * RATE {
            wav.write_sample(((i as f32 * 0.3).sin() * 8_000.0) as i16)
                .unwrap();
        }
        wav.finalize().unwrap();
        path
    }

    #[test]
    fn only_new_stretches_go_through_the_echo_check() {
        let dir = tempfile::tempdir().unwrap();
        let mic = tone_wav(dir.path(), "mic.wav", 12);
        let mut stretch = seg("mic", 6_000, 12_000, "");
        stretch.text.clear();
        let lines = vec![seg("mic", 0, 6_000, "pehle"), stretch];
        let mut checked = Vec::new();
        let out = redo_lines(
            &lines,
            &[("mic", mic.as_path(), 0)],
            crate::notetaker::hinglish::RULES,
            &[],
            || None,
            |_| Ok("suna".into()),
            |piece| {
                checked.push(piece.start_ms);
                true
            },
            |_| true,
        );
        assert!(!checked.is_empty());
        assert!(checked.iter().all(|&ms| ms >= 6_000), "{checked:?}");
        assert!(
            out.iter().all(|s| s.start_ms < 6_000),
            "echo pieces dropped"
        );
    }

    #[test]
    fn redo_progress_follows_the_audio_heard() {
        let lines = vec![
            seg("mic", 0, 1_000, "one"),
            seg("mic", 1_000, 10_000, "two"),
            seg("mic", 10_000, 11_000, "three"),
        ];
        let mut seen = Vec::new();
        redo_lines(
            &lines,
            &[],
            crate::notetaker::hinglish::RULES,
            &[],
            || None,
            |_| Ok("new".into()),
            |_| false,
            |done| {
                seen.push(done);
                true
            },
        );
        let rounded: Vec<i32> = seen.iter().map(|d| (d * 100.0).round() as i32).collect();
        assert_eq!(rounded, vec![0, 9, 91]);
    }

    #[test]
    fn samples_are_long_lines_spread_over_the_meeting() {
        let lines: Vec<Segment> = (0..20)
            .map(|i| {
                seg(
                    "system",
                    i * 10_000,
                    i * 10_000 + if i % 2 == 0 { 5_000 } else { 1_000 },
                    "x",
                )
            })
            .collect();
        let starts: Vec<i64> = spread(&lines, 3, 2_000)
            .iter()
            .map(|s| s.start_ms)
            .collect();
        assert_eq!(starts, vec![20_000, 100_000, 160_000]);
        assert_eq!(spread(&lines[..4], 6, 2_000).len(), 2);
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
