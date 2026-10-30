//! Who spoke when on the call audio (system.wav). speakrs runs the pyannote
//! community-1 pipeline on the CPU; the result labels the final transcript's
//! system lines "Speaker 1", "Speaker 2", … and says where a piece must be cut
//! so one line never holds two people.

use anyhow::{anyhow, bail, Result};
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use tauri::{AppHandle, Manager};

use super::audio::RATE;
use super::{Attendee, Segment};
use crate::managers::model::ModelManager;

/// One diarization (and model download) at a time: it uses several cores
/// and ~0.5 GB per half hour of call audio.
static DIARIZE_LOCK: Mutex<()> = Mutex::new(());

/// A voice with less speech than this is too little to be its own person;
/// it is folded into the speaker it sounds most like.
const MIN_SPEECH_MS: i64 = 25_000;
/// A small cluster joins a big one only if their voices are at least this
/// alike; otherwise its lines stay "Them".
const FOLD_MIN_SIMILARITY: f32 = 0.5;
/// Turns shorter than this still label lines but never cut a piece.
const MIN_CUT_TURN_MS: i64 = 1_000;
/// Longer call audio is not split: the whole file is held in memory and the
/// clustering grows with the square of its length.
const MAX_AUDIO_SECONDS: u32 = 3 * 3600;
/// Cosine similarity at which a voice is taken to be a saved voice profile.
/// Measured on real calls: one person across meetings 0.85-0.98, different
/// voices in one call up to 0.75.
const VOICE_MATCH: f32 = 0.8;
/// The best profile must beat the next one by this much.
const VOICE_MARGIN: f32 = 0.05;
/// Shorter call audio is not worth splitting.
const MIN_AUDIO_SAMPLES: usize = 5 * RATE;

pub const ME_KEY: &str = "me";
pub const THEM_KEY: &str = "them";
/// A turn whose voice matched no speaker: its lines stay "Them".
pub const UNKNOWN: usize = usize::MAX;
/// A turn of the user in the voice split of the microphone: its lines are "Me".
pub const ME: usize = usize::MAX - 1;

/// The key of the n-th speaker (0-based), in order of first appearance.
pub fn speaker_key(n: usize) -> String {
    format!("s{}", n + 1)
}

/// The key and default label of a speaker index (`UNKNOWN` is "Them").
pub fn key_and_label(n: usize) -> (String, String) {
    if n == UNKNOWN {
        return (THEM_KEY.to_string(), super::transcribe::THEM.to_string());
    }
    if n == ME {
        return (ME_KEY.to_string(), super::transcribe::ME.to_string());
    }
    let key = speaker_key(n);
    let label = default_label(&key);
    (key, label)
}

/// The key for a line nobody split by voice.
pub fn key_for(source: &str) -> &'static str {
    if source == "mic" {
        ME_KEY
    } else {
        THEM_KEY
    }
}

/// The label a speaker has until someone names it.
pub fn default_label(key: &str) -> String {
    match key {
        ME_KEY => super::transcribe::ME.to_string(),
        THEM_KEY => super::transcribe::THEM.to_string(),
        other => match other
            .strip_prefix('s')
            .and_then(|n| n.parse::<usize>().ok())
        {
            Some(n) => format!("Speaker {n}"),
            None => other.to_string(),
        },
    }
}

/// "Me", "Them" or "Speaker N": a label that names nobody.
pub fn is_default_label(label: &str) -> bool {
    let label = label.trim();
    label.eq_ignore_ascii_case(super::transcribe::ME)
        || label.eq_ignore_ascii_case(super::transcribe::THEM)
        || label
            .strip_prefix("Speaker ")
            .is_some_and(|n| n.parse::<usize>().is_ok())
}

/// A stretch of one speaker, in WAV time.
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Turn {
    pub start_ms: i64,
    pub end_ms: i64,
    pub speaker: usize,
}

/// The speakers of one call, numbered by first appearance.
#[derive(Clone, Debug, Default)]
pub struct Speakers {
    /// In time order, adjacent turns of one speaker merged.
    pub turns: Vec<Turn>,
    /// Average voice embedding per speaker, when the model gave one.
    pub voices: Vec<Option<Vec<f32>>>,
    /// Milliseconds of speech per speaker.
    pub speech_ms: Vec<i64>,
}

impl Speakers {
    pub fn count(&self) -> usize {
        self.voices.len()
    }

    /// WAV sample positions where the speaker changes: a piece must end
    /// there. Turns under a second are too short to trust for a cut.
    pub fn change_samples(&self) -> Vec<u64> {
        let long: Vec<&Turn> = self
            .turns
            .iter()
            .filter(|t| t.end_ms - t.start_ms >= MIN_CUT_TURN_MS)
            .collect();
        long.windows(2)
            .filter(|w| w[0].speaker != w[1].speaker)
            .map(|w| {
                let at = if w[1].start_ms > w[0].end_ms {
                    (w[0].end_ms + w[1].start_ms) / 2
                } else {
                    w[1].start_ms
                };
                at.max(0) as u64 * RATE as u64 / 1000
            })
            .collect()
    }

    /// The speaker heard most during `start_ms..end_ms` (WAV time); with no
    /// overlap, the nearest turn's speaker.
    pub fn speaker_at(&self, start_ms: i64, end_ms: i64) -> Option<usize> {
        let mut overlap: HashMap<usize, i64> = HashMap::new();
        for t in &self.turns {
            let o = end_ms.min(t.end_ms) - start_ms.max(t.start_ms);
            if o > 0 {
                *overlap.entry(t.speaker).or_default() += o;
            }
        }
        let best = overlap
            .into_iter()
            .max_by_key(|&(i, o)| (o, std::cmp::Reverse(i)))
            .map(|(i, _)| i);
        best.or_else(|| {
            self.turns
                .iter()
                .min_by_key(|t| (t.start_ms - end_ms).max(start_ms - t.end_ms).max(0))
                .map(|t| t.speaker)
        })
    }
}

pub fn cosine(a: &[f32], b: &[f32]) -> f32 {
    if a.len() != b.len() || a.is_empty() {
        return 0.0;
    }
    let dot: f32 = a.iter().zip(b).map(|(x, y)| x * y).sum();
    let na: f32 = a.iter().map(|x| x * x).sum::<f32>().sqrt();
    let nb: f32 = b.iter().map(|x| x * x).sum::<f32>().sqrt();
    if na == 0.0 || nb == 0.0 {
        0.0
    } else {
        dot / (na * nb)
    }
}

/// Raw clusters to speakers: clusters with under `MIN_SPEECH_MS` of speech
/// join the big cluster whose voice is closest, if it is close enough;
/// otherwise their turns become `UNKNOWN`. Speakers are then numbered by first
/// appearance. `turns` are (start_ms, end_ms, cluster).
pub fn tidy(turns: &[(i64, i64, usize)], voices: &[Option<Vec<f32>>]) -> Speakers {
    let clusters = turns
        .iter()
        .map(|t| t.2 + 1)
        .max()
        .unwrap_or(0)
        .max(voices.len());
    let mut speech = vec![0i64; clusters];
    for &(s, e, c) in turns {
        speech[c] += (e - s).max(0);
    }
    let voice = |c: usize| voices.get(c).and_then(|v| v.as_deref());
    let largest = (0..clusters).max_by_key(|&c| (speech[c], std::cmp::Reverse(c)));
    let mut big: Vec<usize> = (0..clusters)
        .filter(|&c| speech[c] >= MIN_SPEECH_MS)
        .collect();
    if big.is_empty() {
        big.extend(largest.filter(|&c| speech[c] > 0));
    }
    let target = |c: usize| -> Option<usize> {
        if big.is_empty() {
            return None;
        }
        if big.contains(&c) {
            return Some(c);
        }
        let closest = voice(c).and_then(|v| {
            big.iter()
                .filter_map(|&b| voice(b).map(|bv| (b, cosine(v, bv))))
                .max_by(|a, b| a.1.total_cmp(&b.1))
        });
        Some(match closest {
            Some((b, sim)) if sim >= FOLD_MIN_SIMILARITY => b,
            _ => UNKNOWN,
        })
    };

    let mut sorted: Vec<(i64, i64, usize)> = turns
        .iter()
        .filter(|t| t.1 > t.0)
        .filter_map(|&(s, e, c)| Some((s, e, target(c)?)))
        .collect();
    sorted.sort_by_key(|t| (t.0, t.1));

    let mut number: HashMap<usize, usize> = HashMap::new();
    let mut out = Speakers::default();
    for (s, e, c) in sorted {
        let n = if c == UNKNOWN {
            UNKNOWN
        } else {
            let next = number.len();
            let n = *number.entry(c).or_insert(next);
            if n == out.voices.len() {
                out.voices.push(None);
                out.speech_ms.push(0);
            }
            out.speech_ms[n] += e - s;
            n
        };
        match out.turns.last_mut() {
            Some(last) if last.speaker == n && s <= last.end_ms + 1_000 => {
                last.end_ms = last.end_ms.max(e)
            }
            _ => out.turns.push(Turn {
                start_ms: s,
                end_ms: e,
                speaker: n,
            }),
        }
    }
    // A folded cluster's voice is left out: it is someone else's, or noise.
    for (&c, &n) in &number {
        out.voices[n] = voice(c).map(<[f32]>::to_vec);
    }
    out
}

/// Average embedding per cluster from the per-window embeddings and their
/// cluster ids (-1 = none). Windows with no usable embedding are skipped.
fn cluster_voices(
    embeddings: &ndarray::Array3<f32>,
    clusters: &ndarray::Array2<i32>,
    count: usize,
) -> Vec<Option<Vec<f32>>> {
    let dim = embeddings.shape()[2];
    let mut sums = vec![vec![0f64; dim]; count];
    let mut n = vec![0usize; count];
    for ((chunk, local), &c) in clusters.indexed_iter() {
        if c < 0
            || c as usize >= count
            || chunk >= embeddings.shape()[0]
            || local >= embeddings.shape()[1]
        {
            continue;
        }
        let e = embeddings.slice(ndarray::s![chunk, local, ..]);
        if e.iter().any(|x| !x.is_finite()) || e.iter().all(|&x| x == 0.0) {
            continue;
        }
        let norm = e.iter().map(|x| x * x).sum::<f32>().sqrt();
        for (s, x) in sums[c as usize].iter_mut().zip(e.iter()) {
            *s += (*x / norm) as f64;
        }
        n[c as usize] += 1;
    }
    sums.into_iter()
        .zip(n)
        .map(|(s, n)| (n > 0).then(|| s.into_iter().map(|x| (x / n as f64) as f32).collect()))
        .collect()
}

fn read_wav(path: &Path) -> Result<Vec<f32>> {
    let mut reader = hound::WavReader::open(path)?;
    let spec = reader.spec();
    if spec.sample_rate != RATE as u32 || spec.channels != 1 {
        bail!(
            "expected 16 kHz mono, got {} Hz x{}",
            spec.sample_rate,
            spec.channels
        );
    }
    let mut out = Vec::with_capacity(reader.duration() as usize);
    for s in reader.samples::<i16>() {
        out.push(s? as f32 / i16::MAX as f32);
    }
    Ok(out)
}

/// Keeps speaker detection to about half the cores so dictation stays quick.
/// Both pools read their size once, so this must run before their first use.
fn limit_threads() {
    static ONCE: std::sync::Once = std::sync::Once::new();
    ONCE.call_once(|| {
        let half = std::thread::available_parallelism()
            .map_or(2, |n| n.get() / 2)
            .max(1);
        if std::env::var_os("MATRIXMULTIPLY_THREADS").is_none() {
            std::env::set_var("MATRIXMULTIPLY_THREADS", half.to_string());
        }
        if let Err(e) = rayon::ThreadPoolBuilder::new()
            .num_threads(half)
            .thread_name(|i| format!("speakers-{i}"))
            .build_global()
        {
            log::debug!("Notetaker speaker threads already set up: {e}");
        }
    });
}

/// Finds the speakers in a call's system.wav, downloading the models first
/// if needed. One run (and one download) at a time.
pub fn detect(app: &AppHandle, wav: &Path) -> Result<Speakers> {
    let _turn = DIARIZE_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let dir = models_dir(app)?;
    run_unlocked(&dir, wav)
}

/// Diarizes a 16 kHz mono WAV with the models in `models_dir`.
pub fn run(models_dir: &Path, wav: &Path) -> Result<Speakers> {
    let _turn = DIARIZE_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    run_unlocked(models_dir, wav)
}

fn run_unlocked(models_dir: &Path, wav: &Path) -> Result<Speakers> {
    let seconds = hound::WavReader::open(wav)?.duration() / RATE as u32;
    if seconds > MAX_AUDIO_SECONDS {
        bail!("{} min of audio is over the 3 hour limit", seconds / 60);
    }
    split(models_dir, read_wav(wav)?)
}

/// The speakers in 16 kHz mono audio, numbered by first appearance, with
/// small clusters folded as `tidy` does. Fails on under 5 s of audio or when
/// nobody speaks.
fn split(models_dir: &Path, audio: Vec<f32>) -> Result<Speakers> {
    limit_threads();
    if audio.len() < MIN_AUDIO_SAMPLES {
        bail!("too little audio");
    }
    let mut pipeline =
        speakrs::OwnedDiarizationPipeline::from_dir(models_dir, speakrs::ExecutionMode::Cpu)
            .map_err(|e| anyhow!("speaker models did not load: {e}"))?;
    let result = pipeline
        .run(&audio)
        .map_err(|e| anyhow!("speaker detection failed: {e}"))?;
    drop(audio);
    let turns: Vec<(i64, i64, usize)> = result
        .exclusive_segments
        .iter()
        .filter_map(|s| {
            let cluster = s.speaker.strip_prefix("SPEAKER_")?.parse().ok()?;
            Some((
                (s.start * 1000.0).round() as i64,
                (s.end * 1000.0).round() as i64,
                cluster,
            ))
        })
        .collect();
    let count = result.discrete_diarization.ncols();
    let voices = cluster_voices(&result.embeddings, &result.hard_clusters, count);
    let speakers = tidy(&turns, &voices);
    if speakers.count() == 0 {
        bail!("no speech found in the audio");
    }
    Ok(speakers)
}

/// The speaker models' folder, downloading what is missing (about 33 MB).
/// Callers hold `DIARIZE_LOCK`.
fn models_dir(app: &AppHandle) -> Result<PathBuf> {
    let manager = app
        .try_state::<Arc<ModelManager>>()
        .ok_or_else(|| anyhow!("models are not ready"))?
        .inner()
        .clone();
    tauri::async_runtime::block_on(manager.ensure_speaker_models())
}

/// Dictation speech used for the user's voice, at most.
const OWN_VOICE_MS: usize = 120_000;
/// Taken from one dictation clip, at most, so the voice comes from several.
const OWN_CLIP_MAX_MS: usize = 30_000;
/// Clips shorter than this are skipped: a word or two says little.
const OWN_CLIP_MIN_MS: usize = 2_000;
/// Less speech than this makes no voice.
const OWN_VOICE_MIN_MS: usize = 20_000;

/// The user's voice from their dictation recordings (`clips`, newest
/// first): the main voice of up to 30 s from each clip, until two minutes.
/// None when they hold too little speech.
pub fn detect_own_voice(app: &AppHandle, clips: &[PathBuf]) -> Result<Option<Vec<f32>>> {
    let _turn = DIARIZE_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let dir = models_dir(app)?;
    own_voice_unlocked(&dir, clips)
}

/// `detect_own_voice` with the models in `models_dir`.
#[cfg(test)]
pub fn own_voice(models_dir: &Path, clips: &[PathBuf]) -> Result<Option<Vec<f32>>> {
    let _turn = DIARIZE_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    own_voice_unlocked(models_dir, clips)
}

fn own_voice_unlocked(models_dir: &Path, clips: &[PathBuf]) -> Result<Option<Vec<f32>>> {
    let ms = |n: usize| n * RATE / 1000;
    let mut audio: Vec<f32> = Vec::new();
    for clip in clips {
        if audio.len() >= ms(OWN_VOICE_MS) {
            break;
        }
        let Ok(mut samples) = read_wav(clip) else {
            continue;
        };
        if samples.len() < ms(OWN_CLIP_MIN_MS) {
            continue;
        }
        samples.truncate(ms(OWN_CLIP_MAX_MS));
        audio.extend(samples);
        audio.extend(std::iter::repeat_n(0.0, ms(500)));
    }
    if audio.len() < ms(OWN_VOICE_MIN_MS) {
        return Ok(None);
    }
    let speakers = split(models_dir, audio)?;
    let main = (0..speakers.count())
        .filter(|&n| speakers.speech_ms[n] >= OWN_VOICE_MIN_MS as i64)
        .max_by_key(|&n| speakers.speech_ms[n]);
    Ok(main.and_then(|n| speakers.voices[n].clone()))
}

/// A mic voice at least this like the user's voice from dictation is the
/// user. Measured on a meeting of four at one laptop: the user 0.71-0.75,
/// the others 0.29-0.52.
const OWN_MATCH: f32 = 0.6;
/// During a call, another mic voice counts as someone else only when it is
/// under this like the user's voice from dictation.
const OTHER_THAN_OWN: f32 = 0.5;
/// A mic voice at least this like a call voice is the call heard through the
/// speakers.
const CALL_ECHO: f32 = 0.6;
/// So is a mic voice heard at least this share of its time while the call
/// talks. On a recorded call the echo left in the mic measured 105% (turns
/// are joined over short gaps), the user 21%.
const CALL_OVERLAP: f64 = 0.5;
/// A call with less speech than this, or under a tenth of the mic's, leaves
/// the room as the meeting.
const QUIET_CALL_MS: i64 = 60_000;
/// During a call, someone else on the mic needs this share of its speech.
const ROOM_SHARE: f64 = 0.1;
/// With no voice from dictation, the mic voice heard most is the user only
/// with this share of the mic's speech.
const MOST_SPEECH_SHARE: f64 = 0.4;

/// Whether a call with `call_speech_ms` of speech leaves the room as the
/// meeting, for a mic with `mic_speech_ms`.
pub fn quiet_call(call_speech_ms: i64, mic_speech_ms: i64) -> bool {
    call_speech_ms < QUIET_CALL_MS || call_speech_ms * 10 < mic_speech_ms
}

/// The people in the room, from the voice split of the microphone (`mic`,
/// with the call taken out). Mic voices that are the call heard through the
/// speakers (like a call voice, or heard mostly while the call talks; `call`
/// is in its WAV's time, `mic_to_call_ms` after the mic's) are left out of
/// every choice and stay the user's.
///
/// During a call, the mic stays "Me" unless the user's voice from dictation
/// (`own`) matches one mic voice and another is clearly someone else (unlike
/// it, with a tenth of the mic's speech). With no call, the user is the mic
/// voice matching `own`; with no `own`, the voice heard most if it has 40% of
/// the mic's speech; otherwise nobody is "Me". Every other voice matching
/// `own` is the user too (the model split them in two).
///
/// The user's turns become `ME`; other voices become speakers numbered after
/// the call's voices, in order of first appearance, with `voices` and
/// `speech_ms` at those numbers. None keeps the whole mic "Me".
pub fn room_voices(
    mic: &Speakers,
    own: Option<&[f32]>,
    call: &Speakers,
    mic_to_call_ms: i64,
    call_speech_ms: i64,
) -> Option<Speakers> {
    if mic.count() < 2 {
        return None;
    }
    let first = call.count();
    let total: i64 = mic.speech_ms.iter().sum();
    let voice = |n: usize| mic.voices[n].as_deref();
    let during_call = |n: usize| -> f64 {
        let ms: i64 = mic
            .turns
            .iter()
            .filter(|t| t.speaker == n)
            .map(|t| {
                overlap_ms(
                    t.start_ms + mic_to_call_ms,
                    t.end_ms + mic_to_call_ms,
                    &call.turns,
                )
            })
            .sum();
        ms as f64 / mic.speech_ms[n].max(1) as f64
    };
    let call_like = |n: usize| {
        voice(n).is_some_and(|v| {
            call.voices
                .iter()
                .flatten()
                .any(|c| cosine(v, c) >= CALL_ECHO)
        }) || during_call(n) >= CALL_OVERLAP
    };
    let people: Vec<usize> = (0..mic.count())
        .filter(|&n| voice(n).is_some() && !call_like(n))
        .collect();
    let like_own = |n: usize| match (voice(n), own) {
        (Some(v), Some(own)) => cosine(v, own),
        _ => f32::MIN,
    };
    let by_own = people
        .iter()
        .copied()
        .filter(|&n| like_own(n) >= OWN_MATCH)
        .max_by(|&a, &b| like_own(a).total_cmp(&like_own(b)));

    let (user, room): (Option<usize>, Vec<usize>) = if !quiet_call(call_speech_ms, total) {
        let user = by_own?;
        let room = people
            .iter()
            .copied()
            .filter(|&n| {
                like_own(n) < OTHER_THAN_OWN && mic.speech_ms[n] as f64 >= ROOM_SHARE * total as f64
            })
            .collect();
        (Some(user), room)
    } else {
        let user = match own {
            Some(_) => by_own,
            None => people
                .iter()
                .copied()
                .max_by_key(|&n| (mic.speech_ms[n], std::cmp::Reverse(n)))
                .filter(|&n| mic.speech_ms[n] as f64 >= MOST_SPEECH_SHARE * total as f64),
        };
        let room = people
            .iter()
            .copied()
            .filter(|&n| Some(n) != user && like_own(n) < OWN_MATCH)
            .collect();
        (user, room)
    };
    if room.is_empty() {
        return None;
    }
    let number = |n: usize| match room.iter().position(|&r| r == n) {
        Some(k) => first + k,
        None if user.is_some() => ME,
        None => UNKNOWN,
    };
    let mut out = Speakers {
        turns: Vec::new(),
        voices: vec![None; first],
        speech_ms: vec![0; first],
    };
    for &n in &room {
        out.voices.push(mic.voices[n].clone());
        out.speech_ms.push(mic.speech_ms[n]);
    }
    for t in &mic.turns {
        let speaker = match t.speaker {
            UNKNOWN => number(UNKNOWN),
            n => number(n),
        };
        match out.turns.last_mut() {
            Some(last) if last.speaker == speaker && t.start_ms <= last.end_ms + 1_000 => {
                last.end_ms = last.end_ms.max(t.end_ms)
            }
            _ => out.turns.push(Turn {
                speaker,
                ..t.clone()
            }),
        }
    }
    Some(out)
}

/// At most this many stretches of the user alone on the mic are checked.
const CHECK_WINDOWS: usize = 20;
/// With fewer than this, the check says nothing.
const CHECK_MIN_WINDOWS: usize = 5;
/// A checked stretch is at least this long
const CHECK_MIN_MS: i64 = 2_000;
/// and its middle part this long at most.
const CHECK_MAX_MS: i64 = 6_000;
/// The speaker model hears this much audio at once.
const EMBED_SAMPLES: usize = 10 * RATE;

/// Up to 20 stretches of mic speech (`lines`, mic WAV time) while the call
/// (`call`, its WAV `mic_to_call_ms` after the mic's) is silent, spread
/// over the meeting: lines of at least 2 s, their middle 6 s at most.
pub fn lone_mic_windows(
    lines: &[(i64, i64)],
    call: &[Turn],
    mic_to_call_ms: i64,
) -> Vec<(i64, i64)> {
    let lone: Vec<(i64, i64)> = lines
        .iter()
        .filter(|&&(s, e)| {
            e - s >= CHECK_MIN_MS && overlap_ms(s + mic_to_call_ms, e + mic_to_call_ms, call) == 0
        })
        .map(|&(s, e)| {
            let cut = (e - s - CHECK_MAX_MS).max(0) / 2;
            (s + cut, e - cut)
        })
        .collect();
    let n = lone.len().min(CHECK_WINDOWS);
    (0..n).map(|i| lone[i * lone.len() / n]).collect()
}

/// The checked stretches together must be at least this like the user's
/// voice from dictation
const CHECK_TOGETHER: f32 = 0.65;
/// and none of them under this. One short stretch is noisy: on a call
/// with only the user on the mic they measured 0.47-0.73.
const CHECK_EACH: f32 = 0.55;
/// The stretches are only where the call is silent, so someone at the
/// laptop who talks only while the call talks is never in them: with more
/// than this share of the mic's speech during call speech, the mic is split.
pub const CHECK_MAX_DURING_CALL: f64 = 0.2;

/// The share of mic speech (`lines`, mic WAV time) heard while the call
/// (`call`, its WAV `mic_to_call_ms` after the mic's) talks.
pub fn mic_during_call(lines: &[(i64, i64)], call: &[Turn], mic_to_call_ms: i64) -> f64 {
    let total: i64 = lines.iter().map(|(s, e)| e - s).sum();
    let during: i64 = lines
        .iter()
        .map(|&(s, e)| overlap_ms(s + mic_to_call_ms, e + mic_to_call_ms, call))
        .sum();
    during as f64 / total.max(1) as f64
}

/// Whether the checked stretches sound like the user's voice from
/// dictation, so the mic holds nobody else and need not be split: their
/// average voice is like it and no single stretch is far from it.
pub fn only_user(window_voices: &[Vec<f32>], own: &[f32]) -> bool {
    if window_voices.len() < CHECK_MIN_WINDOWS
        || window_voices.iter().any(|v| cosine(v, own) < CHECK_EACH)
    {
        return false;
    }
    let mut sum = vec![0f32; own.len()];
    for v in window_voices {
        let norm = v
            .iter()
            .map(|x| x * x)
            .sum::<f32>()
            .sqrt()
            .max(f32::MIN_POSITIVE);
        for (s, x) in sum.iter_mut().zip(v) {
            *s += x / norm;
        }
    }
    cosine(&sum, own) >= CHECK_TOGETHER
}

/// The voice of each stretch (WAV ms) of a 16 kHz mono WAV. A stretch is
/// repeated to fill the 10 s the speaker model hears.
pub fn detect_window_voices(
    app: &AppHandle,
    wav: &Path,
    windows: &[(i64, i64)],
) -> Result<Vec<Vec<f32>>> {
    let _turn = DIARIZE_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let dir = models_dir(app)?;
    window_voices_unlocked(&dir, wav, windows)
}

/// `detect_window_voices` with the models in `models_dir`.
#[cfg(test)]
pub fn window_voices(
    models_dir: &Path,
    wav: &Path,
    windows: &[(i64, i64)],
) -> Result<Vec<Vec<f32>>> {
    let _turn = DIARIZE_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    window_voices_unlocked(models_dir, wav, windows)
}

fn window_voices_unlocked(
    models_dir: &Path,
    wav: &Path,
    windows: &[(i64, i64)],
) -> Result<Vec<Vec<f32>>> {
    limit_threads();
    let mut model = speakrs::inference::EmbeddingModel::new(
        models_dir.join("wespeaker-voxceleb-resnet34.onnx"),
    )
    .map_err(|e| anyhow!("speaker model did not load: {e}"))?;
    let mut reader = hound::WavReader::open(wav)?;
    let mut out = Vec::new();
    for &(start, end) in windows {
        let at = |ms: i64| (ms.max(0) as usize * RATE / 1000) as u32;
        reader.seek(at(start))?;
        let piece: Vec<f32> = reader
            .samples::<i16>()
            .take((at(end) - at(start)) as usize)
            .map(|s| s.map(|s| s as f32 / i16::MAX as f32))
            .collect::<Result<_, _>>()?;
        if piece.is_empty() {
            continue;
        }
        let filled: Vec<f32> = piece.iter().copied().cycle().take(EMBED_SAMPLES).collect();
        let voice = model
            .embed(&filled)
            .map_err(|e| anyhow!("speaker model failed: {e}"))?;
        out.push(voice.to_vec());
    }
    Ok(out)
}

/// How much of `start_ms..end_ms` the turns cover.
pub fn overlap_ms(start_ms: i64, end_ms: i64, turns: &[Turn]) -> i64 {
    turns
        .iter()
        .map(|t| (end_ms.min(t.end_ms) - start_ms.max(t.start_ms)).max(0))
        .sum()
}

/// A saved voice: its name and average embedding.
pub struct VoiceProfile {
    pub name: String,
    pub voice: Vec<f32>,
}

/// How alike the spellings of a heard word and an invited first name must be.
const SAME_NAME_SPELLING: f64 = 0.6;
/// Invited first names shorter than this must be heard exactly.
const MIN_FUZZY_NAME_CHARS: usize = 3;
/// Everyday Hindi and Hinglish words that sound like names ("rahi" like
/// Raahi), never taken for a misspelt name.
pub(super) const HINGLISH_WORDS: &[&str] = &[
    "karna", "karne", "karo", "kar", "kara", "kare", "karte", "karta", "karti", "mera", "meri",
    "mere", "tera", "teri", "tere", "kya", "kaun", "kabhi", "kahan", "kaise", "kitna", "abhi",
    "aaj", "kal", "haan", "nahi", "bhai", "yaar", "accha", "achha", "theek", "thik", "matlab",
    "sab", "kuch", "woh", "yeh", "unka", "uska", "apna", "toh", "phir", "bas", "wala", "wali",
    "raha", "rahi", "kaam", "sahi", "bilkul",
];
/// Words that make up a group's name or mailbox ("Data Team", "all-hands@").
const GROUP_WORDS: &[&str] = &[
    "team",
    "data",
    "eng",
    "engineering",
    "all",
    "hands",
    "everyone",
    "leads",
    "founders",
    "sales",
    "support",
    "ops",
    "dev",
    "hr",
    "admin",
    "info",
    "contact",
    "hello",
    "managers",
    "group",
    "the",
    "and",
];
/// A local part this long with no dot or underscore is rarely one person.
const MAX_PLAIN_LOCAL_CHARS: usize = 12;

/// Letters-only words: "room4a" -> ["room", "a"].
fn letter_words(text: &str) -> Vec<String> {
    text.split(|c: char| !c.is_alphabetic())
        .filter(|w| !w.is_empty())
        .map(str::to_lowercase)
        .collect()
}

/// A booked room: a Google resource calendar, or a name or address with a
/// room word or a "(12)" capacity in it.
fn is_room(a: &Attendee) -> bool {
    let email = a.email.as_deref().map(str::trim).unwrap_or("");
    if email
        .rsplit_once('@')
        .is_some_and(|(_, host)| host.to_lowercase().starts_with("resource.calendar."))
    {
        return true;
    }
    let name = a.name.as_deref().unwrap_or("").to_lowercase();
    let local = email.split('@').next().unwrap_or("");
    let numbered = [name.as_str(), local].iter().any(|text| {
        text.split('(').skip(1).any(|rest| {
            rest.split_once(')')
                .is_some_and(|(n, _)| !n.is_empty() && n.chars().all(|c| c.is_ascii_digit()))
        })
    });
    numbered
        || letter_words(&name)
            .into_iter()
            .chain(letter_words(local))
            .any(|w| matches!(w.as_str(), "room" | "conference" | "conf" | "boardroom"))
}

/// A name with its "(...)" parts taken out: "Priya Shah (Engineering)" ->
/// "Priya Shah".
fn without_brackets(name: &str) -> String {
    let mut out = String::new();
    let mut depth = 0usize;
    for c in name.chars() {
        match c {
            '(' => depth += 1,
            ')' => depth = depth.saturating_sub(1),
            _ if depth == 0 => out.push(c),
            _ => {}
        }
    }
    out.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// A display name made only of group words ("Data Team", "The Sales Team").
fn is_group_name(name: &str) -> bool {
    let words = letter_words(name);
    !words.is_empty() && words.iter().all(|w| GROUP_WORDS.contains(&w.as_str()))
}

/// A mailbox local part made only of group words: "all-hands", "eng.leads",
/// and run together only when it has no separator ("datateam").
fn is_group_local(local: &str) -> bool {
    let parts = letter_words(local);
    match parts.as_slice() {
        [one] => made_of_group_words(one),
        _ => !parts.is_empty() && parts.iter().all(|p| GROUP_WORDS.contains(&p.as_str())),
    }
}

fn made_of_group_words(word: &str) -> bool {
    word.is_empty()
        || GROUP_WORDS
            .iter()
            .any(|g| word.strip_prefix(g).is_some_and(made_of_group_words))
}

/// "varun.kapoor+cal" -> "Varun Kapoor"; None for addresses that are not a
/// person ("noreply", "team-data", "all-hands", "a").
fn name_from_email(email: &str) -> Option<String> {
    let local = email.trim().split('@').next()?.to_lowercase();
    let local = local.split('+').next().unwrap_or("");
    let not_person = [
        "noreply",
        "no-reply",
        "donotreply",
        "do-not-reply",
        "calendar",
    ];
    let group = is_group_local(local);
    let long_plain = !local.contains(['.', '_']) && local.chars().count() > MAX_PLAIN_LOCAL_CHARS;
    if not_person.contains(&local) || group || long_plain {
        return None;
    }
    let parts: Vec<String> = local
        .split(|c: char| matches!(c, '.' | '_' | '-') || c.is_ascii_digit())
        .filter(|p| !p.is_empty())
        .map(|p| {
            let mut chars = p.chars();
            chars
                .next()
                .map(|c| c.to_uppercase().chain(chars).collect())
                .unwrap_or_default()
        })
        .collect();
    if parts.iter().map(|p| p.chars().count()).sum::<usize>() < 2 {
        return None;
    }
    Some(parts.join(" "))
}

/// The name to call an attendee by: the calendar's name unless it is just
/// the email, else one made from the email. None for rooms, groups and
/// mailboxes.
pub fn display_name(a: &Attendee) -> Option<String> {
    if a.not_person || is_room(a) {
        return None;
    }
    let name = without_brackets(a.name.as_deref().unwrap_or(""));
    if !name.is_empty() && !name.contains('@') {
        return (!is_group_name(&name)).then_some(name);
    }
    let email = a
        .email
        .as_deref()
        .filter(|e| e.contains('@'))
        .or(Some(name.as_str()))?;
    name_from_email(email)
}

/// Everyone invited except the user and rooms, by display name, once each.
pub fn invitees(attendees: &[Attendee]) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for name in attendees
        .iter()
        .filter(|a| !a.is_me)
        .filter_map(display_name)
    {
        if !out.iter().any(|n| n.eq_ignore_ascii_case(&name)) {
            out.push(name);
        }
    }
    out
}

fn first_word(name: &str) -> Option<String> {
    words(name).into_iter().next()
}

fn surname(name: &str) -> Vec<String> {
    words(name).into_iter().skip(1).collect()
}

/// A heard word that may be an invited first name the speech model misspelt
/// ("Gotam" for "Gautam"). Short names and common English, Hindi and
/// Hinglish words must match exactly.
fn sounds_like_name(word: &str, first: &str) -> bool {
    if word == first {
        return true;
    }
    word.chars().count() >= MIN_FUZZY_NAME_CHARS
        && first.chars().count() >= MIN_FUZZY_NAME_CHARS
        && !HINGLISH_WORDS.contains(&word)
        && !crate::textflow::commands::is_common_word(word)
        && name_sound(word) == name_sound(first)
        && strsim::normalized_levenshtein(word, first) >= SAME_NAME_SPELLING
}

/// A name in one spelling of how it sounds, so spelling variants agree
/// ("Gourav" and "Gaurav", "Deepak" and "Dipak", "Priya" and "Pria") while
/// every consonant and vowel sound stays apart ("Rajesh" is not "Rakesh",
/// "Komal" is not "Kamal"). Vowels are capitals, consonants lowercase.
pub(super) fn name_sound(word: &str) -> Vec<char> {
    let c: Vec<char> = word
        .to_lowercase()
        .chars()
        .filter(|c| c.is_alphabetic())
        .collect();
    let vowel = |i: usize| matches!(c.get(i), Some('a' | 'e' | 'i' | 'o' | 'u'));
    let mut key: Vec<char> = Vec::new();
    let mut i = 0;
    while i < c.len() {
        let after_vowel = i > 0 && vowel(i - 1);
        let (unit, len) = match (c[i], c.get(i + 1)) {
            ('a', Some('a')) => (Some('A'), 2),
            ('e', Some('e')) | ('i', Some('i')) => (Some('I'), 2),
            ('o', Some('o')) => (Some('U'), 2),
            ('a' | 'o', Some('u')) => (Some('O'), 2),
            ('o', Some('w')) if !vowel(i + 2) => (Some('O'), 2),
            ('a' | 'e', Some('i')) => (Some('E'), 2),
            ('a', Some('y')) if !vowel(i + 2) => (Some('E'), 2),
            ('a', _) => (Some('A'), 1),
            ('e', _) => (Some('E'), 1),
            ('i', _) => (Some('I'), 1),
            ('o', _) => (Some('O'), 1),
            ('u', _) => (Some('U'), 1),
            ('p', Some('h')) => (Some('f'), 2),
            ('b' | 'd' | 't' | 'k' | 'g' | 's', Some('h')) => (Some(c[i]), 2),
            ('c', Some('k')) => (Some('k'), 2),
            ('c' | 'q', _) => (Some('k'), 1),
            ('w', _) => (Some('v'), 1),
            ('y', _) if after_vowel && vowel(i + 1) => (None, 1),
            ('h', _) if after_vowel && !vowel(i + 1) => (None, 1),
            (other, _) => (Some(other), 1),
        };
        if let Some(unit) = unit.filter(|u| key.last() != Some(u)) {
            key.push(unit);
        }
        i += len;
    }
    key
}

/// The one invitee a name stands for: the same full name, else the same
/// first name, else a first name that sounds alike, never with a different
/// surname. None when it fits nobody or more than one invitee.
fn invitee_for<'a>(name: &str, invitees: &'a [String]) -> Option<&'a String> {
    let first = first_word(name)?;
    let lower = name.trim().to_lowercase();
    let ours = surname(name);
    let same_surname = |i: &String| {
        let theirs = surname(i);
        ours.is_empty() || theirs.is_empty() || ours == theirs
    };
    let only = |found: Vec<&'a String>| (found.len() == 1).then(|| found[0]);
    let tiers: [&dyn Fn(&String) -> bool; 3] = [
        &|i| i.to_lowercase() == lower,
        &|i| same_surname(i) && first_word(i).as_deref() == Some(first.as_str()),
        &|i| same_surname(i) && first_word(i).is_some_and(|f| sounds_like_name(&first, &f)),
    ];
    for fits in tiers {
        let found: Vec<&String> = invitees.iter().filter(|i| fits(i)).collect();
        if !found.is_empty() {
            return only(found);
        }
    }
    None
}

/// Two labels that name the same person: the same full or first name.
fn same_person(a: &str, b: &str) -> bool {
    a.trim().eq_ignore_ascii_case(b.trim())
        || first_word(a).is_some_and(|f| first_word(b).as_deref() == Some(f.as_str()))
}

/// Whether the name at `at` in a line's words is the speaker naming
/// themselves, at the start of the line after nothing but greetings: "this
/// is Gautam", "I'm Gautam Verma from data", "Gautam here", "Gautam this
/// side", "main Gautam bol raha hoon", "mera naam Gautam hai". `surname`
/// is the rest of the name, which may follow the first name. Any other word
/// after the name ("this is Naresh, our new joiner") or before it ("everyone,
/// this is Naresh") means it is not an introduction; "Gautam here" must
/// start the line and end it or go on with "from".
fn introduces(words: &[String], at: usize, surname: &[String]) -> bool {
    let word = |i: usize| words.get(i).map(String::as_str);
    let mut start = 0;
    loop {
        match (word(start), word(start + 1)) {
            (
                Some(
                    "hi" | "hey" | "hello" | "haan" | "ji" | "yes" | "yeah" | "ok" | "okay" | "so",
                ),
                _,
            ) => start += 1,
            (Some("good"), Some("morning" | "afternoon" | "evening")) => start += 2,
            _ => break,
        }
    }
    if start > at {
        return false;
    }
    let mut end = at + 1;
    for s in surname {
        if word(end) != Some(s.as_str()) {
            break;
        }
        end += 1;
    }
    let speaking = |i: usize| {
        (i..=i + 1).any(|b| {
            word(b) == Some("bol")
                && (b == i
                    || !matches!(
                        word(i),
                        Some("ko" | "se" | "ka" | "ki" | "ke" | "ne" | "par" | "pe" | "tak")
                    ))
                && matches!(word(b + 1), Some("raha" | "rahi"))
                && (b + 2..=i + 3).any(|h| matches!(word(h), Some("hoon" | "hun" | "hu")))
        })
    };
    let lead: Vec<&str> = words[start..at].iter().map(String::as_str).collect();
    match lead.as_slice() {
        ["this" | "it", "is"]
        | ["it", "s"]
        | ["i", "am" | "m"]
        | ["im" | "myself"]
        | ["my", "name", "is"] => match word(end) {
            None | Some("here" | "speaking" | "from") => true,
            Some("this") => word(end + 1) == Some("side"),
            _ => speaking(end),
        },
        // Not after any word: "Okay Gautam, here..." speaks to Gautam.
        [] if at == 0 => {
            let next = match (word(end), word(end + 1)) {
                (Some("here" | "speaking"), _) => end + 1,
                (Some("this"), Some("side")) => end + 2,
                _ => return false,
            };
            matches!(word(next), None | Some("from"))
        }
        ["main" | "mai"] => speaking(end),
        ["mera", "naam"] => matches!(word(end), None | Some("hai")),
        _ => false,
    }
}

/// Names known before the notes are written: a saved voice first, then the
/// calendar when exactly one other person was invited and one voice was heard
/// (never when a room or group mailbox was invited too: more people may come).
pub fn known_names(
    speakers: &Speakers,
    profiles: &[VoiceProfile],
    attendees: &[Attendee],
) -> HashMap<usize, String> {
    let mut names: HashMap<usize, String> = HashMap::new();
    let mut matches: Vec<(f32, usize, usize)> = Vec::new();
    for (i, v) in speakers.voices.iter().enumerate() {
        let Some(v) = v else { continue };
        let mut sims: Vec<(f32, usize)> = profiles
            .iter()
            .enumerate()
            .map(|(p, profile)| (cosine(v, &profile.voice), p))
            .collect();
        sims.sort_by(|a, b| b.0.total_cmp(&a.0));
        let second = sims.get(1).map_or(f32::MIN, |s| s.0);
        if let Some(&(best, p)) = sims.first() {
            if best >= VOICE_MATCH && best - second >= VOICE_MARGIN {
                matches.push((best, i, p));
            }
        }
    }
    matches.sort_by(|a, b| b.0.total_cmp(&a.0));
    let mut used = HashSet::new();
    for (_, i, p) in matches {
        if names.contains_key(&i) || used.contains(&p) {
            continue;
        }
        used.insert(p);
        names.insert(i, profiles[p].name.clone());
    }

    let invited = invitees(attendees);
    let hidden = attendees
        .iter()
        .any(|a| !a.is_me && display_name(a).is_none());
    if speakers.count() == 1 && invited.len() == 1 && !hidden && !names.contains_key(&0) {
        names.insert(0, invited[0].clone());
    }
    names
}

/// Time step for matching the huddle window with the audio.
const WINDOW_STEP_MS: i64 = 100;
/// A window sample stands for the time until the next one, at most this
/// long: a stalled reader says nothing about the time it missed.
const WINDOW_SAMPLE_MS: i64 = 1_500;
/// Speech a voice needs while the window shows one other person talking.
const WINDOW_MIN_MS: i64 = 10_000;
/// Share of that speech one name needs.
const WINDOW_SHARE: f64 = 0.7;
/// The tile shown talking while only the microphone speaks is the user's
/// when it is shown for at least this long and at least `WINDOW_OWN_RATIO`
/// times as long as any other tile.
const WINDOW_OWN_MS: i64 = 6_000;
const WINDOW_OWN_RATIO: i64 = 3;
/// Slack's talking marks trail the voice: right after a mark changes, the
/// window may still show the person who just stopped.
const WINDOW_MARK_LAG_MS: i64 = 1_500;

/// Names from the call window's "talking" marks (`window::Sample`, meeting
/// time): a call voice (turns in meeting time) is named X when at least
/// `WINDOW_MIN_MS` of its speech falls where the window shows exactly one
/// person other than the user talking, and X has at least `WINDOW_SHARE` of
/// it. A tile that a second voice also talks under for `WINDOW_MIN_MS`
/// (e.g. a room on one laptop) names neither.
/// The user's tiles: those whose full name is one of `own` (emails are
/// ignored), and the tile Slack's signed-in label ("User: X") fits, equal
/// or one a whole-word prefix of the other. The tile shown talking while
/// only the microphone (`mic` spans) speaks picks among several that fit;
/// when none fits, or it disagrees, it is the user's too.
pub fn window_names(
    samples: &[super::window::Sample],
    turns: &[(i64, i64, String)],
    mic: &[(i64, i64)],
    own: &[String],
) -> Vec<Naming> {
    let end = turns
        .iter()
        .map(|t| t.1)
        .chain(mic.iter().map(|m| m.1))
        .chain(
            samples
                .iter()
                .map(|s| s.ms_since_meeting_start + WINDOW_SAMPLE_MS),
        )
        .max()
        .unwrap_or(0);
    let steps = (end.max(0) / WINDOW_STEP_MS + 1) as usize;
    let index = |ms: i64| ((ms.max(0) / WINDOW_STEP_MS) as usize).min(steps);

    let mut keys: Vec<&str> = Vec::new();
    let mut voice: Vec<Option<usize>> = vec![None; steps];
    for (start, end, key) in turns {
        let k = match keys.iter().position(|k| k == key) {
            Some(k) => k,
            None => {
                keys.push(key);
                keys.len() - 1
            }
        };
        for v in &mut voice[index(*start)..index(*end).max(index(*start))] {
            *v = Some(k);
        }
    }
    let mut on_mic = vec![false; steps];
    for &(start, end) in mic {
        for m in &mut on_mic[index(start)..index(end).max(index(start))] {
            *m = true;
        }
    }
    let mut shown: Vec<Option<usize>> = vec![None; steps];
    let mut settled: Vec<i64> = vec![0; samples.len()];
    let mut marks: Option<&[String]> = None;
    let mut changed_at = 0;
    for (n, s) in samples.iter().enumerate() {
        if s.gap {
            marks = None;
            continue;
        }
        if marks != Some(s.speaking.as_slice()) {
            changed_at = s.ms_since_meeting_start;
            marks = Some(&s.speaking);
        }
        settled[n] = changed_at + WINDOW_MARK_LAG_MS;
        let start = s.ms_since_meeting_start;
        let until = samples
            .get(n + 1)
            .map_or(i64::MAX, |next| next.ms_since_meeting_start)
            .min(start + WINDOW_SAMPLE_MS);
        for at in &mut shown[index(start)..index(until).max(index(start))] {
            *at = Some(n);
        }
    }

    let full = |name: &str| {
        name.split_whitespace()
            .collect::<Vec<_>>()
            .join(" ")
            .to_lowercase()
    };
    let own: HashSet<String> = own
        .iter()
        .filter(|o| !o.contains('@'))
        .map(|o| full(o))
        .collect();
    let labels: Vec<Vec<String>> = samples
        .iter()
        .filter_map(|s| s.user.as_deref())
        .map(|u| full(u).split(' ').map(str::to_string).collect())
        .collect::<HashSet<_>>()
        .into_iter()
        .collect();
    let tiles: HashSet<&str> = samples
        .iter()
        .flat_map(|s| s.present.iter().chain(&s.speaking))
        .map(String::as_str)
        .collect();
    let labelled: HashSet<&str> = tiles
        .iter()
        .copied()
        .filter(|t| {
            let words: Vec<String> = full(t).split(' ').map(str::to_string).collect();
            labels
                .iter()
                .any(|l| words.starts_with(l) || l.starts_with(&words))
        })
        .collect();

    let mut mic_alone: HashMap<&str, i64> = HashMap::new();
    for i in 0..steps {
        let Some(n) = shown[i] else { continue };
        if !on_mic[i] || voice[i].is_some() || (i as i64) * WINDOW_STEP_MS < settled[n] {
            continue;
        }
        for name in &samples[n].speaking {
            *mic_alone.entry(name).or_default() += WINDOW_STEP_MS;
        }
    }
    let mut by_mic: Vec<(&str, i64)> = mic_alone.into_iter().collect();
    by_mic.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(b.0)));
    let on_mic_alone = by_mic.first().and_then(|&(name, ms)| {
        let next = by_mic.get(1).map_or(0, |n| n.1);
        (ms >= WINDOW_OWN_MS && ms >= WINDOW_OWN_RATIO * next).then_some(name)
    });

    let mut users: HashSet<&str> = tiles
        .iter()
        .copied()
        .filter(|t| own.contains(&full(t)))
        .collect();
    match on_mic_alone {
        Some(mic) if labelled.is_empty() || labelled.contains(mic) => {
            users.insert(mic);
        }
        Some(mic) => {
            users.insert(mic);
            users.extend(&labelled);
        }
        None => users.extend(&labelled),
    }

    let mut heard: Vec<HashMap<&str, i64>> = vec![HashMap::new(); keys.len()];
    for i in 0..steps {
        let (Some(k), Some(n)) = (voice[i], shown[i]) else {
            continue;
        };
        let others: Vec<&str> = samples[n]
            .speaking
            .iter()
            .map(String::as_str)
            .filter(|n| !users.contains(n))
            .collect();
        if let [name] = others[..] {
            *heard[k].entry(name).or_default() += WINDOW_STEP_MS;
        }
    }
    let shared = |name: &str| {
        let mut under: Vec<i64> = heard
            .iter()
            .filter_map(|names| names.get(name).copied())
            .filter(|&ms| ms >= WINDOW_MIN_MS)
            .collect();
        under.sort_unstable_by(|a, b| b.cmp(a));
        under.len() > 1
    };
    let mut out: Vec<Naming> = Vec::new();
    for (k, names) in heard.iter().enumerate() {
        let total: i64 = names.values().sum();
        let best = names.iter().max_by(|a, b| a.1.cmp(b.1).then(b.0.cmp(a.0)));
        let Some((&name, &ms)) = best else { continue };
        let most = heard
            .iter()
            .all(|other| other.get(name).is_none_or(|&o| o <= ms));
        if ms >= WINDOW_MIN_MS && ms as f64 >= WINDOW_SHARE * total as f64 && most && !shared(name)
        {
            out.push(Naming {
                key: keys[k].to_string(),
                name: name.to_string(),
                rule: NameRule::Window,
                lines: Vec::new(),
            });
        }
    }
    out.sort_by(|a, b| a.key.cmp(&b.key));
    out
}

/// Adds the window's names to those known from saved voices and the
/// calendar. The window beats the calendar; a saved voice beats the window,
/// both on its own voice and for a name the window would give another.
/// Returns each disagreement with a saved voice as (key, saved, window).
pub fn add_window_names(
    known: &mut HashMap<usize, String>,
    from_voice: &dyn Fn(&str) -> bool,
    window: &[Naming],
) -> Vec<(String, String, String)> {
    let mut kept = Vec::new();
    for n in window {
        let Some(i) = n
            .key
            .strip_prefix('s')
            .and_then(|i| i.parse::<usize>().ok())
            .and_then(|i| i.checked_sub(1))
        else {
            continue;
        };
        let same = |a: &str| a.trim().eq_ignore_ascii_case(n.name.trim());
        if let Some(current) = known.get(&i) {
            if same(current) {
                continue;
            }
            if from_voice(current) {
                kept.push((n.key.clone(), current.clone(), n.name.clone()));
                continue;
            }
        }
        let elsewhere: Vec<usize> = known
            .iter()
            .filter(|&(&other, name)| other != i && same(name))
            .map(|(&other, _)| other)
            .collect();
        if let Some(&other) = elsewhere.iter().find(|&&o| from_voice(&known[&o])) {
            kept.push((speaker_key(other), known[&other].clone(), n.name.clone()));
            continue;
        }
        for other in elsewhere {
            known.remove(&other);
        }
        known.insert(i, n.name.clone());
    }
    kept
}

/// A voice counts as answering a name said before it only if it talks at
/// least `HANDOVER_TALK_MS` within `HANDOVER_WINDOW_MS` after that line: a
/// "Yeah." from someone else between the name and the person named is not
/// the person named.
const HANDOVER_WINDOW_MS: i64 = 15_000;
const HANDOVER_TALK_MS: i64 = 3_000;
/// How near a call line must be to a microphone line to be its echo.
const ECHO_NEAR_MS: i64 = 3_000;
/// Words soon after a name that hand that person the floor ("Varun aap
/// bata do", "Varun aap bolo").
const HANDOVER_WORDS: &[&str] = &["bata", "batao", "bataiye", "bataye", "bolo", "boliye"];
/// Word pairs that do the same ("Varun, go ahead", "Varun your update").
const HANDOVER_PAIRS: &[(&str, &str)] = &[
    ("go", "ahead"),
    ("go", "next"),
    ("your", "turn"),
    ("your", "update"),
    ("any", "update"),
    ("you", "next"),
];

/// The first line by another split voice (call, or a person in the room)
/// in the window after line `i`. The user's microphone lines between are
/// skipped: without headphones they are often the call heard again.
fn next_voice(segments: &[Segment], i: usize) -> Option<&Segment> {
    let line = segments.get(i)?;
    segments[i + 1..]
        .iter()
        .take_while(|s| s.start_ms < line.end_ms + HANDOVER_WINDOW_MS)
        .find(|s| {
            (s.source != "mic" || s.speaker_key.starts_with('s'))
                && s.speaker_key != line.speaker_key
        })
}

/// How long `key` talks in the window after line `i` ends.
fn talk_after(segments: &[Segment], i: usize, key: &str) -> i64 {
    let Some(line) = segments.get(i) else {
        return 0;
    };
    let (from, to) = (line.end_ms, line.end_ms + HANDOVER_WINDOW_MS);
    segments[i + 1..]
        .iter()
        .take_while(|s| s.start_ms < to)
        .filter(|s| s.speaker_key == key)
        .map(|s| (s.end_ms.min(to) - s.start_ms.max(from)).max(0))
        .sum()
}

/// A word of a line with what surrounds it, for telling whether a name in
/// it is someone spoken to.
struct LineWord {
    lower: String,
    capitalised: bool,
    /// Which sentence of the line (split at . ? ! and ।).
    sentence: usize,
    /// The first character before and after the word that is not a space.
    before: Option<char>,
    after: Option<char>,
}

fn ends_sentence(c: char) -> bool {
    matches!(c, '.' | '?' | '!' | '।')
}

/// The words of `text`, as `words` splits it, with their surroundings.
fn line_words(text: &str) -> Vec<LineWord> {
    let mut spans: Vec<(usize, usize)> = Vec::new();
    let mut start = None;
    for (i, c) in text.char_indices() {
        match (word_char(c), start) {
            (true, None) => start = Some(i),
            (false, Some(st)) => {
                spans.push((st, i));
                start = None;
            }
            _ => {}
        }
    }
    if let Some(st) = start {
        spans.push((st, text.len()));
    }
    let mut sentence = 0;
    let mut out = Vec::new();
    for (k, &(st, en)) in spans.iter().enumerate() {
        let gap_before = &text[spans.get(k.wrapping_sub(1)).map_or(0, |p| p.1)..st];
        if k > 0 && gap_before.chars().any(ends_sentence) {
            sentence += 1;
        }
        let word = &text[st..en];
        out.push(LineWord {
            lower: word.to_lowercase(),
            capitalised: word.chars().next().is_some_and(char::is_uppercase),
            sentence,
            before: text[..st].trim_end().chars().last(),
            after: text[en..].trim_start().chars().next(),
        });
    }
    out
}

/// Words that may come before a name spoken to ("but Varun, ...").
const FILLERS: &[&str] = &[
    "but", "so", "and", "okay", "ok", "haan", "acha", "achha", "yes", "yeah", "right", "now",
    "chalo",
];

/// Whether the word at `at` starts its sentence, or follows at most two
/// fillers that start it ("but Varun", "okay so Varun", "theek hai Varun").
fn after_fillers(ws: &[LineWord], at: usize) -> bool {
    let here = ws[at].sentence;
    let mut i = at;
    let mut fillers = 0;
    while i > 0 && ws[i - 1].sentence == here {
        let two = i >= 2
            && ws[i - 2].sentence == here
            && matches!(ws[i - 2].lower.as_str(), "theek" | "thik")
            && ws[i - 1].lower == "hai";
        if two {
            i -= 2;
        } else if FILLERS.contains(&ws[i - 1].lower.as_str()) {
            i -= 1;
        } else {
            return false;
        }
        fillers += 1;
        if fillers > 2 {
            return false;
        }
    }
    true
}

/// Whether the name at `at` hands its person the floor, within its own
/// sentence: a hand-over word or pair in the next three words ("Varun aap
/// bata do", "Varun your update"), "over to Varun", or Varun spoken to:
/// starting the sentence (after at most two fillers) with "," or "?" right
/// after it, also after "ji"/"bhai" ("Varun, any update?", "Varun?",
/// "but Varun, ...", "Varun ji, ..."), or ending
/// it right after a comma ("Any questions, Varun?", "Thanks, Varun ji.").
/// A name in a statement or plain question ("Varun did that", "Did you
/// sync with Varun?") is not.
fn addressed(ws: &[LineWord], at: usize) -> bool {
    let here = ws[at].sentence;
    let same = |i: usize| ws.get(i).filter(|w| w.sentence == here);
    let word = |i: usize| same(i).map(|w| w.lower.as_str());
    let starts = after_fillers(ws, at);
    let honorific = matches!(word(at + 1), Some("ji" | "bhai"));
    let last = if honorific { at + 1 } else { at };
    let handed = (at + 1..at + 4).any(|i| {
        word(i).is_some_and(|w| HANDOVER_WORDS.contains(&w))
            || word(i)
                .zip(word(i + 1))
                .is_some_and(|pair| HANDOVER_PAIRS.contains(&pair))
    });
    handed
        || (at >= 2 && word(at - 2) == Some("over") && word(at - 1) == Some("to"))
        || (starts && matches!(ws[last].after, Some(',' | '?')))
        || (ws[at].before == Some(',') && same(last + 1).is_none())
}

/// A microphone line that repeats the call (no headphones): a call line
/// within 3 s of it says the name `first` and shares at least half of its
/// words.
fn is_echo(segments: &[Segment], i: usize, first: &str) -> bool {
    let line = &segments[i];
    if line.source != "mic" {
        return false;
    }
    let mine: HashSet<String> = words(&line.text).into_iter().collect();
    if mine.is_empty() {
        return false;
    }
    segments.iter().any(|s| {
        s.source != "mic"
            && s.start_ms < line.end_ms + ECHO_NEAR_MS
            && s.end_ms > line.start_ms - ECHO_NEAR_MS
            && {
                let theirs: HashSet<String> = words(&s.text).into_iter().collect();
                theirs
                    .iter()
                    .any(|w| w == first || sounds_like_name(w, first))
                    && mine.intersection(&theirs).count() * 2 >= mine.len()
            }
    })
}

/// Every (line, voice, invitee) where a line hands an invitee the floor and
/// that voice speaks next (see `next_voice`) and answers (see
/// `HANDOVER_TALK_MS`).
pub(super) fn hand_overs(
    segments: &[Segment],
    invited: &[String],
    own: &HashSet<String>,
) -> Vec<(usize, String, String)> {
    let mut out = Vec::new();
    for (i, line) in segments.iter().enumerate() {
        let Some(next) = next_voice(segments, i) else {
            continue;
        };
        if !next.speaker_key.starts_with('s')
            || talk_after(segments, i, &next.speaker_key) < HANDOVER_TALK_MS
        {
            continue;
        }
        let ws = line_words(&line.text);
        for name in invited {
            let Some(first) = first_word(name).filter(|f| !own.contains(f)) else {
                continue;
            };
            if is_echo(segments, i, &first) {
                continue;
            }
            let shared = invited
                .iter()
                .any(|other| other != name && first_word(other).as_deref() == Some(&first));
            let surname = surname(name);
            let called = (0..ws.len()).any(|p| {
                let w = &ws[p];
                let heard =
                    w.lower == first || (w.capitalised && sounds_like_name(&w.lower, &first));
                let full = !shared
                    || (!surname.is_empty()
                        && surname
                            .iter()
                            .enumerate()
                            .all(|(k, s)| ws.get(p + 1 + k).is_some_and(|w| &w.lower == s)));
                heard && full && addressed(&ws, p)
            });
            if called {
                out.push((i, next.speaker_key.clone(), name.clone()));
            }
        }
    }
    out
}

/// Names given from hand-overs alone, with no model guess: an invitee goes
/// to voice V when at least two hand-overs to them are answered by V, at
/// least two thirds of all their answered hand-overs are answered by V, and
/// V answers no hand-over to anyone else. Only voices still on their default
/// label, never a name a voice already has or the user's.
fn handed_names(
    segments: &[Segment],
    hand_overs: &[(usize, String, String)],
    own: &HashSet<String>,
) -> Vec<Naming> {
    let taken = named_labels(segments);
    let on_default = |key: &str| {
        segments
            .iter()
            .any(|s| s.speaker_key == key && s.speaker == default_label(key))
    };
    let mut out: Vec<Naming> = Vec::new();
    let mut names: Vec<&String> = hand_overs.iter().map(|(_, _, n)| n).collect();
    names.sort();
    names.dedup();
    for name in names {
        let to_name: Vec<&(usize, String, String)> =
            hand_overs.iter().filter(|(_, _, n)| n == name).collect();
        let mut keys: Vec<&String> = to_name.iter().map(|(_, k, _)| k).collect();
        keys.sort();
        keys.dedup();
        for key in keys {
            let lines: Vec<usize> = to_name
                .iter()
                .filter(|(_, k, _)| k == key)
                .map(|(i, ..)| *i)
                .collect();
            let elsewhere = hand_overs
                .iter()
                .any(|(_, k, n)| k == key && !same_person(n, name));
            if lines.len() >= 2
                && lines.len() * 3 >= to_name.len() * 2
                && !elsewhere
                && on_default(key)
                && !own.contains(&name.trim().to_lowercase())
                && !taken.iter().any(|t| same_person(t, name))
                && !out
                    .iter()
                    .any(|n| n.key == *key || same_person(&n.name, name))
            {
                out.push(Naming {
                    key: key.clone(),
                    name: name.clone(),
                    rule: NameRule::HandedOver,
                    lines,
                });
            }
        }
    }
    out
}

/// A name the notes model offered for one speaker label, with the transcript
/// lines (indexes) where it says that name.
#[derive(Clone, Debug, PartialEq)]
pub struct SpeakerGuess {
    pub speaker: String,
    pub name: String,
    pub lines: Vec<usize>,
}

/// A letter, digit or Indic sign: the vowel signs and virama of Devanagari
/// and other Indic scripts are marks, not letters, but belong to the word.
pub(super) fn word_char(c: char) -> bool {
    c.is_alphanumeric()
        || matches!(c, '\u{0300}'..='\u{036F}' | '\u{0900}'..='\u{0DFF}')
            && !matches!(c, '\u{0964}' | '\u{0965}')
}

fn words(text: &str) -> Vec<String> {
    text.split(|c: char| !word_char(c))
        .filter(|w| !w.is_empty())
        .map(str::to_lowercase)
        .collect()
}

/// How a speaker got its name once the notes were written.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NameRule {
    Introduction,
    Addressed,
    /// Hand-overs alone, without a model guess.
    HandedOver,
    /// The call app's window showed who was talking (`window_names`).
    Window,
}

/// A name given to a speaker, with the lines that back it.
#[derive(Clone, Debug, PartialEq)]
pub struct Naming {
    pub key: String,
    pub name: String,
    pub rule: NameRule,
    pub lines: Vec<usize>,
}

/// What became of one guess of the notes model: the speaker key and name it
/// stood for, the lines that back it, and why it named nobody.
#[derive(Clone, Debug, PartialEq)]
pub struct GuessVerdict {
    pub speaker: String,
    pub guessed: String,
    pub key: Option<String>,
    pub name: Option<String>,
    pub lines: Vec<usize>,
    pub rejected: Option<&'static str>,
}

/// Names from the notes model, kept only when the evidence holds up: the
/// name is said in at least two of the lines it points to, each either that
/// speaker introducing themselves (merely saying the name counts for nothing)
/// or someone else saying it right before that speaker is the next voice on
/// the call and goes on talking (being addressed). A voice addressed or handed the floor under
/// two different names gets none; see also `handed_names`, which needs no
/// guess and which a guess never overrides. A name on the invite is written as
/// invited, may be heard misspelt as a capitalised word, and one
/// introduction is enough for it.
/// Never the user's own name (`own`: names and emails) or an email, never a
/// name another speaker already has or claims as strongly. Only voice-split
/// speakers still on their default label are named.
pub fn judge_guesses(
    segments: &[Segment],
    guesses: &[SpeakerGuess],
    own_names: &[String],
    invited: &[String],
) -> (Vec<Naming>, Vec<GuessVerdict>) {
    let own = own_set(own_names);
    let reserved = ["me", "them", "you", "others", "speaker", "unknown", "user"];
    let taken = named_labels(segments);

    let mut label_key: HashMap<&str, &str> = HashMap::new();
    for s in segments {
        if s.speaker_key.starts_with('s') && s.speaker == default_label(&s.speaker_key) {
            label_key.insert(&s.speaker, &s.speaker_key);
        }
    }

    let points_at = |i: usize, key: &str, first: &str| {
        segments.get(i).is_some_and(|s| {
            s.speaker_key == key
                || (next_voice(segments, i).is_some_and(|next| next.speaker_key == key)
                    && talk_after(segments, i, key) >= HANDOVER_TALK_MS
                    && !is_echo(segments, i, first))
        })
    };
    struct Evidence {
        key: String,
        name: String,
        lines: HashSet<usize>,
        introduced: bool,
        invited: bool,
        guesses: Vec<usize>,
    }
    let mut evidence: HashMap<(String, String), Evidence> = HashMap::new();
    let mut verdicts: Vec<GuessVerdict> = Vec::new();
    for g in guesses {
        let mut verdict = GuessVerdict {
            speaker: g.speaker.clone(),
            guessed: g.name.clone(),
            key: None,
            name: None,
            lines: Vec::new(),
            rejected: None,
        };
        verdict.rejected = 'judge: {
            let guessed = g.name.trim();
            let Some(guessed_first) = first_word(guessed) else {
                break 'judge Some("no name");
            };
            if guessed.contains('@') {
                break 'judge Some("an email");
            }
            if own.contains(&guessed.to_lowercase()) || own.contains(&guessed_first) {
                break 'judge Some("the user's own name");
            }
            if reserved.contains(&guessed_first.as_str()) {
                break 'judge Some("not a name");
            }
            let invitee = invitee_for(guessed, invited);
            let name = invitee.map_or(guessed, String::as_str);
            let Some(first) = first_word(name) else {
                break 'judge Some("no name");
            };
            verdict.name = Some(name.to_string());
            if own.contains(&first) {
                break 'judge Some("the user's own name");
            }
            if taken.iter().any(|t| same_person(t, name)) {
                break 'judge Some("another speaker already has this name");
            }
            let Some(&key) = label_key.get(g.speaker.trim()) else {
                break 'judge Some("not a voice-split speaker on its default label");
            };
            verdict.key = Some(key.to_string());
            let surname = surname(name);
            let entry = evidence
                .entry((key.to_string(), name.to_lowercase()))
                .or_insert_with(|| Evidence {
                    key: key.to_string(),
                    name: name.to_string(),
                    lines: HashSet::new(),
                    introduced: false,
                    invited: invitee.is_some(),
                    guesses: Vec::new(),
                });
            let mut mentioned = false;
            for &i in &g.lines {
                let Some(line) = segments.get(i).filter(|_| points_at(i, key, &first)) else {
                    continue;
                };
                let raw: Vec<&str> = line
                    .text
                    .split(|c: char| !word_char(c))
                    .filter(|w| !w.is_empty())
                    .collect();
                let said: Vec<String> = raw.iter().map(|w| w.to_lowercase()).collect();
                let heard = |p: usize| {
                    let w = &said[p];
                    if invitee.is_some() {
                        let capitalised = raw[p].chars().next().is_some_and(char::is_uppercase);
                        !own.contains(w)
                            && (*w == first || (capitalised && sounds_like_name(w, &first)))
                    } else {
                        *w == first
                    }
                };
                let at: Vec<usize> = (0..said.len()).filter(|&p| heard(p)).collect();
                if at.is_empty() {
                    continue;
                }
                if line.speaker_key == key {
                    if !at.iter().any(|&p| introduces(&said, p, &surname)) {
                        mentioned = true;
                        continue;
                    }
                    entry.introduced = true;
                }
                verdict.lines.push(i);
                entry.lines.insert(i);
            }
            if verdict.lines.is_empty() {
                break 'judge Some(if mentioned {
                    "the speaker says the name without introducing themselves"
                } else {
                    "the name is not said in the lines it points to"
                });
            }
            entry.guesses.push(verdicts.len());
            None
        };
        verdicts.push(verdict);
    }

    let mut reject = |e: &Evidence, why: &'static str| {
        for &g in &e.guesses {
            verdicts[g].rejected = Some(why);
        }
    };
    let mut handed: HashMap<String, Vec<String>> = HashMap::new();
    for e in evidence.values() {
        if e.lines.iter().any(|&i| segments[i].speaker_key != e.key) {
            handed
                .entry(e.key.clone())
                .or_default()
                .push(e.name.clone());
        }
    }
    let hand_overs = hand_overs(segments, invited, &own);
    for (_, key, name) in &hand_overs {
        handed.entry(key.clone()).or_default().push(name.clone());
    }
    let fixed = handed_names(segments, &hand_overs, &own);
    let confused: HashSet<String> = handed
        .into_iter()
        .filter(|(_, names)| {
            names
                .iter()
                .any(|a| names.iter().any(|b| !same_person(a, b)))
        })
        .map(|(key, _)| key)
        .collect();

    let mut scored: Vec<Evidence> = Vec::new();
    for e in evidence.into_values() {
        if confused.contains(&e.key) {
            reject(&e, "addressed under two different names");
        } else if e.lines.len() >= 2 || (e.invited && e.introduced) {
            scored.push(e);
        } else if e.invited {
            reject(&e, "one line that is not an introduction");
        } else {
            reject(&e, "not invited and said in fewer than two lines");
        }
    }
    let contested = |a: &Evidence, list: &[Evidence]| {
        list.iter().any(|b| {
            b.lines.len() == a.lines.len()
                && ((b.key == a.key) != b.name.eq_ignore_ascii_case(&a.name))
        })
    };
    scored.sort_by(|a, b| b.lines.len().cmp(&a.lines.len()).then(a.key.cmp(&b.key)));
    let mut keys: HashSet<String> = fixed.iter().map(|n| n.key.clone()).collect();
    let mut picked: Vec<String> = fixed.iter().map(|n| n.name.clone()).collect();
    let mut out = fixed;
    for e in &scored {
        if keys.contains(&e.key) {
            reject(e, "a stronger name won for this speaker");
            continue;
        }
        if picked.iter().any(|p| same_person(p, &e.name)) {
            reject(e, "a stronger claim gave this name to another speaker");
            continue;
        }
        keys.insert(e.key.clone());
        picked.push(e.name.clone());
        if contested(e, &scored) {
            reject(e, "another claim is as strong");
            continue;
        }
        let mut lines: Vec<usize> = e.lines.iter().copied().collect();
        lines.sort_unstable();
        out.push(Naming {
            key: e.key.clone(),
            name: e.name.clone(),
            rule: if e.introduced {
                NameRule::Introduction
            } else {
                NameRule::Addressed
            },
            lines,
        });
    }
    (out, verdicts)
}

/// The user's names and emails, whole and by first word, lowercased.
pub(super) fn own_set(own_names: &[String]) -> HashSet<String> {
    let mut own: HashSet<String> = HashSet::new();
    for v in own_names {
        own.insert(v.trim().to_lowercase());
        if let Some(first) = first_word(v) {
            own.insert(first);
        }
    }
    own
}

/// Names the call's voice-split speakers already have.
fn named_labels(segments: &[Segment]) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for s in segments {
        if s.speaker_key.starts_with('s')
            && s.speaker != default_label(&s.speaker_key)
            && !out.contains(&s.speaker)
        {
            out.push(s.speaker.clone());
        }
    }
    out
}

/// Gives every line of `key` still on its default label the name `name`.
pub fn apply_name(segments: &mut [Segment], key: &str, name: &str) {
    let default = default_label(key);
    for s in segments
        .iter_mut()
        .filter(|s| s.speaker_key == key && s.speaker == default)
    {
        s.speaker = name.to_string();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn seg(key: &str, text: &str) -> Segment {
        Segment {
            source: if key == ME_KEY { "mic" } else { "system" }.into(),
            speaker: default_label(key),
            speaker_key: key.into(),
            start_ms: 0,
            end_ms: 0,
            text: text.into(),
        }
    }

    #[test]
    fn labels_follow_keys() {
        assert_eq!(speaker_key(0), "s1");
        assert_eq!(default_label("s3"), "Speaker 3");
        assert_eq!(default_label("me"), "Me");
        assert_eq!(default_label("them"), "Them");
        assert_eq!(key_for("mic"), "me");
        assert_eq!(key_for("system"), "them");
        assert!(is_default_label("Speaker 12"));
        assert!(is_default_label("them"));
        assert!(!is_default_label("Speaker Priya"));
    }

    #[test]
    fn speakers_are_numbered_by_first_appearance() {
        let turns = [
            (40_000, 80_000, 2),
            (0, 30_000, 1),
            (30_000, 40_000, 0),
            (80_000, 120_000, 0),
        ];
        let voices = vec![None, None, None];
        let s = tidy(&turns, &voices);
        assert_eq!(s.count(), 3);
        let order: Vec<usize> = s.turns.iter().map(|t| t.speaker).collect();
        assert_eq!(order, vec![0, 1, 2, 1]);
        assert_eq!(s.speech_ms, vec![30_000, 50_000, 40_000]);
    }

    #[test]
    fn small_clusters_fold_into_the_closest_voice() {
        let turns = [
            (0, 60_000, 0),
            (60_000, 120_000, 1),
            (120_000, 125_000, 2),
            (125_000, 130_000, 3),
            (130_000, 135_000, 4),
        ];
        let voices = vec![
            Some(vec![1.0, 0.0]),
            Some(vec![0.0, 1.0]),
            Some(vec![0.1, 0.9]),
            None,
            Some(vec![-1.0, -0.2]),
        ];
        let s = tidy(&turns, &voices);
        assert_eq!(s.count(), 2);
        assert_eq!(s.speech_ms, vec![60_000, 65_000]);
        let shape: Vec<(i64, usize)> = s.turns.iter().map(|t| (t.end_ms, t.speaker)).collect();
        assert_eq!(
            shape,
            vec![(60_000, 0), (125_000, 1), (135_000, UNKNOWN)],
            "a close voice joins its neighbour; no voice or an unlike one stays unknown"
        );
        assert_eq!(s.voices[1], Some(vec![0.0, 1.0]));
        assert_eq!(key_and_label(UNKNOWN), ("them".into(), "Them".into()));
    }

    #[test]
    fn a_short_call_keeps_its_main_voice() {
        let s = tidy(&[(0, 8_000, 1), (9_000, 12_000, 0)], &[None, None]);
        assert_eq!(s.count(), 1);
        assert_eq!(s.turns.len(), 2);
        assert_eq!(s.turns[1].speaker, UNKNOWN);
        assert_eq!(s.speech_ms, vec![8_000]);
        assert_eq!(tidy(&[], &[]).count(), 0);
    }

    /// A mic split: voices on unit vectors at these angles (degrees), each
    /// talking `ms` in turn, in this order.
    fn mic_split(voices: &[(f32, i64)]) -> Speakers {
        let mut at = 0;
        let mut turns = Vec::new();
        for (n, &(_, ms)) in voices.iter().enumerate() {
            turns.push(Turn {
                start_ms: at,
                end_ms: at + ms,
                speaker: n,
            });
            at += ms + 2_000;
        }
        Speakers {
            turns,
            voices: voices
                .iter()
                .map(|&(deg, _)| Some(vec![deg.to_radians().cos(), deg.to_radians().sin()]))
                .collect(),
            speech_ms: voices.iter().map(|&(_, ms)| ms).collect(),
        }
    }

    /// A call with `n` voices and no turns.
    fn no_call(n: usize) -> Speakers {
        Speakers {
            turns: Vec::new(),
            voices: vec![None; n],
            speech_ms: vec![0; n],
        }
    }

    fn unit(deg: f32) -> Vec<f32> {
        vec![deg.to_radians().cos(), deg.to_radians().sin()]
    }

    fn speakers_of(room: &Speakers) -> Vec<String> {
        room.turns
            .iter()
            .map(|t| key_and_label(t.speaker).0)
            .collect()
    }

    #[test]
    fn a_meeting_in_the_room_splits_the_mic() {
        // Three people at the laptop; the user (at 0°) talks less than the
        // first colleague. No call audio.
        let mic = mic_split(&[(80.0, 600_000), (0.0, 300_000), (150.0, 200_000)]);
        let room = room_voices(&mic, Some(&unit(10.0)), &no_call(0), 0, 0).unwrap();
        assert_eq!(speakers_of(&room), vec!["s1", "me", "s2"]);
        assert_eq!(room.count(), 2);
        assert_eq!(room.speech_ms, vec![600_000, 200_000]);
        assert_eq!(key_and_label(ME), ("me".into(), "Me".into()));

        // Without the user's voice from dictation, the voice heard most is
        // the user when it has 40% of the speech.
        let room = room_voices(&mic, None, &no_call(0), 0, 0).unwrap();
        assert_eq!(speakers_of(&room), vec!["me", "s1", "s2"]);
    }

    #[test]
    fn with_no_clear_user_nobody_in_the_room_is_me() {
        let even = mic_split(&[(0.0, 300_000), (90.0, 300_000), (180.0, 300_000)]);
        let room = room_voices(&even, None, &no_call(0), 0, 0).unwrap();
        assert_eq!(speakers_of(&room), vec!["s1", "s2", "s3"]);
        let room = room_voices(&even, Some(&unit(270.0)), &no_call(0), 0, 0).unwrap();
        assert_eq!(
            speakers_of(&room),
            vec!["s1", "s2", "s3"],
            "the voice from dictation matches nobody"
        );
    }

    #[test]
    fn a_second_voice_like_the_users_joins_me() {
        let mic = mic_split(&[(0.0, 300_000), (50.0, 100_000), (54.0, 100_000)]);
        let room = room_voices(&mic, Some(&unit(0.0)), &no_call(0), 0, 0).unwrap();
        assert_eq!(speakers_of(&room), vec!["me", "me", "s1"]);
    }

    #[test]
    fn room_voices_are_numbered_after_the_call_voices() {
        let mic = mic_split(&[(0.0, 300_000), (90.0, 100_000)]);
        let room = room_voices(&mic, Some(&unit(0.0)), &no_call(2), 0, 0).unwrap();
        assert_eq!(speakers_of(&room), vec!["me", "s3"]);
        assert_eq!(room.voices.len(), 3);
        assert!(room.voices[..2].iter().all(Option::is_none));
    }

    #[test]
    fn during_a_call_the_mic_stays_the_user() {
        let mut call = no_call(1);
        call.voices = vec![Some(unit(120.0))];
        let call_ms = 1_200_000;
        let own = unit(0.0);
        let split = |mic: &Speakers, own: Option<&[f32]>, call: &Speakers, shift, call_ms| {
            room_voices(mic, own, call, shift, call_ms).map(|r| speakers_of(&r))
        };
        // The user split in two, and the call heard through the speakers.
        let mic = mic_split(&[(0.0, 300_000), (20.0, 100_000), (115.0, 80_000)]);
        assert_eq!(split(&mic, Some(&own), &call, 0, call_ms), None);
        // A little of someone else.
        let mic = mic_split(&[(0.0, 300_000), (270.0, 20_000)]);
        assert_eq!(split(&mic, Some(&own), &call, 0, call_ms), None);
        // Someone a bit like the user.
        let mic = mic_split(&[(0.0, 300_000), (57.0, 100_000)]);
        assert_eq!(split(&mic, Some(&own), &call, 0, call_ms), None);
        // Someone else at the laptop for a good part of the call.
        let mic = mic_split(&[(0.0, 300_000), (270.0, 100_000)]);
        assert_eq!(
            split(&mic, Some(&own), &call, 0, call_ms),
            Some(vec!["me".to_string(), "s2".to_string()])
        );
        // Without the user's voice from dictation, never.
        assert_eq!(split(&mic, None, &call, 0, call_ms), None);
        // A voice heard only while the call talks is the call, even when it
        // sounds like nobody on it (here the call starts 1 s before the mic).
        call.turns = vec![Turn {
            start_ms: 290_000,
            end_ms: 410_000,
            speaker: 0,
        }];
        assert_eq!(split(&mic, Some(&own), &call, 1_000, call_ms), None);
        assert!(split(&mic, Some(&own), &call, 100_000, call_ms).is_some());
        call.turns.clear();
        // A quiet call: a little of someone else counts.
        let mic = mic_split(&[(0.0, 300_000), (270.0, 20_000)]);
        assert!(split(&mic, Some(&own), &call, 0, 30_000).is_some());
    }

    #[test]
    fn lone_stretches_are_where_the_call_is_silent() {
        let lines: Vec<(i64, i64)> = vec![
            (0, 1_000),
            (2_000, 12_000),
            (20_000, 25_000),
            (30_000, 34_000),
        ];
        let call = [Turn {
            start_ms: 31_000,
            end_ms: 32_000,
            speaker: 0,
        }];
        assert_eq!(
            lone_mic_windows(&lines, &call, 0),
            vec![(4_000, 10_000), (20_000, 25_000)],
            "too short, the middle 6 s, whole, the call talking over it"
        );
        let many: Vec<(i64, i64)> = (0..100).map(|i| (i * 10_000, i * 10_000 + 3_000)).collect();
        let picked = lone_mic_windows(&many, &[], 0);
        assert_eq!(picked.len(), 20);
        assert_eq!((picked[0].0, picked[19].0), (0, 950_000));
    }

    #[test]
    fn the_mic_is_only_the_user_when_the_stretches_together_sound_like_them() {
        let own = unit(0.0);
        let spread: Vec<Vec<f32>> = (0..6)
            .map(|i| unit(if i % 2 == 0 { 55.0 } else { -55.0 }))
            .collect();
        assert!(only_user(&spread, &own), "noisy alone, the user together");
        assert!(!only_user(&spread[..4], &own), "too few to tell");
        let mut one_off: Vec<Vec<f32>> = (0..6)
            .map(|i| unit(if i % 2 == 0 { 30.0 } else { -30.0 }))
            .collect();
        one_off.push(unit(70.0));
        assert!(
            !only_user(&one_off, &own),
            "one stretch sounds like someone else"
        );
        assert!(
            !only_user(&vec![unit(50.0); 6], &own),
            "together not close enough"
        );
        let mut low: Vec<Vec<f32>> = (0..6)
            .map(|i| unit(if i % 2 == 0 { 20.0 } else { -20.0 }))
            .collect();
        low.push(unit(60.0));
        assert!(!only_user(&low, &own), "one stretch at 0.5 is too far");
    }

    #[test]
    fn mic_speech_while_the_call_talks_is_measured() {
        let lines = [(0, 10_000), (20_000, 30_000)];
        let call = [Turn {
            start_ms: 26_000,
            end_ms: 40_000,
            speaker: 0,
        }];
        assert_eq!(mic_during_call(&lines, &call, 0), 0.2);
        assert_eq!(mic_during_call(&lines, &call, 6_000), 0.5);
        assert_eq!(mic_during_call(&[], &call, 0), 0.0);
    }

    #[test]
    fn one_mic_voice_is_the_user() {
        let mic = mic_split(&[(0.0, 300_000)]);
        assert!(room_voices(&mic, None, &no_call(0), 0, 0).is_none());
    }

    #[test]
    fn lines_go_to_the_speaker_heard_most() {
        let s = tidy(
            &[(0, 30_000, 0), (30_000, 60_000, 1), (70_000, 100_000, 0)],
            &[None, None],
        );
        assert_eq!(s.speaker_at(25_000, 40_000), Some(1));
        assert_eq!(s.speaker_at(0, 10_000), Some(0));
        assert_eq!(s.speaker_at(62_000, 64_000), Some(1), "nearest when silent");
        let cuts = s.change_samples();
        assert_eq!(cuts, vec![30 * RATE as u64, 65 * RATE as u64]);
    }

    #[test]
    fn short_turns_label_lines_but_never_cut() {
        let s = tidy(
            &[
                (0, 30_000, 0),
                (30_000, 30_600, 1),
                (30_600, 60_000, 0),
                (60_000, 90_000, 1),
            ],
            &[None, None],
        );
        assert_eq!(s.turns.len(), 4);
        assert_eq!(s.change_samples(), vec![60 * RATE as u64]);
        assert_eq!(s.speaker_at(30_100, 30_500), Some(1));
    }

    #[test]
    fn voice_profiles_and_calendar_name_speakers() {
        let speakers = Speakers {
            turns: vec![],
            voices: vec![Some(vec![1.0, 0.0]), Some(vec![0.6, 0.8])],
            speech_ms: vec![60_000, 60_000],
        };
        let profiles = vec![
            VoiceProfile {
                name: "Priya".into(),
                voice: vec![0.98, 0.2],
            },
            VoiceProfile {
                name: "Ravi".into(),
                voice: vec![-1.0, 0.0],
            },
        ];
        let names = known_names(&speakers, &profiles, &[]);
        assert_eq!(names.get(&0).map(String::as_str), Some("Priya"));
        assert_eq!(names.len(), 1, "one profile names one speaker at most");
        let twins = vec![
            VoiceProfile {
                name: "Asha".into(),
                voice: vec![1.0, 0.01],
            },
            VoiceProfile {
                name: "Bela".into(),
                voice: vec![1.0, -0.01],
            },
        ];
        assert!(
            known_names(&speakers, &twins, &[]).is_empty(),
            "two profiles that close name nobody"
        );

        let one = Speakers {
            voices: vec![None],
            ..speakers.clone()
        };
        let attendees = vec![
            Attendee {
                name: Some("Gourav".into()),
                email: None,
                is_me: true,
                ..Default::default()
            },
            Attendee {
                name: Some("Asha".into()),
                email: Some("asha@x.com".into()),
                is_me: false,
                ..Default::default()
            },
        ];
        let names = known_names(&one, &[], &attendees);
        assert_eq!(names.get(&0).map(String::as_str), Some("Asha"));
        assert!(known_names(&speakers, &[], &attendees).is_empty());
        for other in [
            attendee(None, Some("team-data@x.com"), false),
            attendee(Some("Conf Room 4A"), Some("room4a@x.com"), false),
            attendee(Some("Data Team"), Some("dt@x.com"), false),
            Attendee {
                name: Some("Neha Gupta".into()),
                email: Some("neha.gupta@x.com".into()),
                not_person: true,
                ..Default::default()
            },
        ] {
            let mut with = attendees.clone();
            with.push(other);
            assert!(
                known_names(&one, &[], &with).is_empty(),
                "a group or room may bring more people"
            );
        }
    }

    #[test]
    fn guessed_names_need_two_lines_and_never_the_user() {
        let segments = spaced(vec![
            seg("me", "Priya, can you share the deck?"),
            seg("s1", "Sure, sharing now."),
            seg("me", "Thanks Priya, and the numbers?"),
            seg("s1", "Here they are."),
            seg("s2", "Hi, I'm Ravi from finance."),
            seg("s2", "Gourav, the budget is fine."),
            seg("me", "Gourav here, thanks."),
        ]);
        let me = vec![
            "Gourav Sharma".to_string(),
            "gourav@example.com".to_string(),
        ];
        let guess = |speaker: &str, name: &str, lines: &[usize]| SpeakerGuess {
            speaker: speaker.into(),
            name: name.into(),
            lines: lines.to_vec(),
        };
        let names = names_from_guesses(
            &segments,
            &[
                guess("Speaker 1", "Priya", &[0, 2]),
                guess("Speaker 2", "Ravi", &[4]),
                guess("Speaker 2", "gourav@example.com", &[5, 6]),
                guess("Speaker 2", "Gourav", &[5, 6]),
                guess("Me", "Gourav", &[5, 6]),
            ],
            &me,
            &[],
        );
        assert_eq!(names, vec![("s1".to_string(), "Priya".to_string())]);

        let made_up =
            names_from_guesses(&segments, &[guess("Speaker 2", "Meera", &[4, 5])], &me, &[]);
        assert!(made_up.is_empty(), "the name must be said in those lines");
        let elsewhere =
            names_from_guesses(&segments, &[guess("Speaker 2", "Priya", &[0, 2])], &me, &[]);
        assert!(
            elsewhere.is_empty(),
            "Priya is addressed right before Speaker 1, not Speaker 2"
        );
    }

    #[test]
    fn names_differing_only_in_case_agree() {
        let segments = vec![seg("s1", "I'm Asha"), seg("s1", "Asha here")];
        let guess = |name: &str, lines: &[usize]| SpeakerGuess {
            speaker: "Speaker 1".into(),
            name: name.into(),
            lines: lines.to_vec(),
        };
        let names = names_from_guesses(
            &segments,
            &[guess("Asha", &[0]), guess("asha", &[1])],
            &[],
            &[],
        );
        assert_eq!(names.len(), 1);
        assert!(names[0].1.eq_ignore_ascii_case("asha"));
    }

    #[test]
    fn a_tie_between_names_names_nobody() {
        let segments = vec![
            seg("me", "Asha and Bela, ready?"),
            seg("s1", "Yes."),
            seg("me", "Asha and Bela, the numbers?"),
            seg("s1", "Here."),
        ];
        let guess = |name: &str| SpeakerGuess {
            speaker: "Speaker 1".into(),
            name: name.into(),
            lines: vec![0, 1],
        };
        assert!(
            names_from_guesses(&segments, &[guess("Asha"), guess("Bela")], &[], &[]).is_empty()
        );
    }

    /// Lines one after another, 5 s each.
    fn spaced(lines: Vec<Segment>) -> Vec<Segment> {
        lines
            .into_iter()
            .enumerate()
            .map(|(i, s)| Segment {
                start_ms: i as i64 * 5_000,
                end_ms: (i as i64 + 1) * 5_000,
                ..s
            })
            .collect()
    }

    /// Lines one after another, each `ms` long.
    fn timed(lines: &[(&str, &str, i64)]) -> Vec<Segment> {
        let mut at = 0;
        lines
            .iter()
            .map(|&(key, text, ms)| {
                let s = Segment {
                    start_ms: at,
                    end_ms: at + ms,
                    ..seg(key, text)
                };
                at += ms;
                s
            })
            .collect()
    }

    #[test]
    fn a_short_reply_after_a_hand_over_is_not_the_person_named() {
        let call = |reply_ms: i64| {
            timed(&[
                ("s1", "Priya aap bata do, kal kya hua?", 3_000),
                ("s2", "Yeah.", reply_ms),
                ("s1", "Okay, so the release moved to Friday and the numbers look fine to me, nothing else from my side today.", 16_000),
                ("s1", "Priya, your update?", 2_000),
                ("s2", "Yeah.", reply_ms),
                ("s1", "Theek hai, then we close the sprint on Monday and start the planning after lunch.", 16_000),
                ("s3", "So yesterday I finished the dashboard and today I am writing the tests for the export.", 12_000),
            ])
        };
        let invited = invited(&["Priya Nair", "Rohan Das"]);
        let guesses = [guess("Speaker 2", "Priya", &[0, 3])];
        assert!(names_from_guesses(&call(2_000), &guesses, &[], &invited).is_empty());
        assert_eq!(
            names_from_guesses(&call(5_000), &guesses, &[], &invited),
            vec![("s2".to_string(), "Priya Nair".to_string())]
        );
    }

    #[test]
    fn a_voice_handed_two_names_gets_none() {
        let mut lines = vec![
            ("s1", "Priya aap bata do.", 3_000),
            (
                "s2",
                "Sure, the dashboard is done and the tests pass.",
                6_000,
            ),
            ("s1", "Thanks Priya, aur kuch? Aap bolo.", 3_000),
            ("s2", "No, that is all from me for today.", 6_000),
        ];
        let invited = invited(&["Priya Nair", "Rohan Das"]);
        let guesses = [guess("Speaker 2", "Priya", &[0, 2])];
        assert_eq!(
            names_from_guesses(&timed(&lines), &guesses, &[], &invited),
            vec![("s2".to_string(), "Priya Nair".to_string())]
        );
        lines.extend([
            ("s1", "Rohan, your update?", 3_000),
            (
                "s2",
                "I fixed the login bug and started on the export.",
                7_000,
            ),
        ]);
        assert!(names_from_guesses(&timed(&lines), &guesses, &[], &invited).is_empty());
    }

    #[test]
    fn a_microphone_line_between_does_not_hide_the_next_voice() {
        let lines = timed(&[
            ("s1", "Priya aap bata do.", 3_000),
            ("me", "Priya aap bata.", 1_000),
            (
                "s2",
                "Sure, the dashboard is done and the tests pass.",
                6_000,
            ),
            ("s1", "Thanks Priya, aur kuch? Aap bolo.", 3_000),
            ("s2", "No, that is all from me for today.", 6_000),
        ]);
        let invited = invited(&["Priya Nair"]);
        assert_eq!(
            names_from_guesses(
                &lines,
                &[guess("Speaker 2", "Priya", &[0, 3])],
                &[],
                &invited
            ),
            vec![("s2".to_string(), "Priya Nair".to_string())]
        );
    }

    #[test]
    fn repeated_hand_overs_name_a_voice_without_a_guess() {
        let invited = invited(&["Mohan Rao", "Priya Nair"]);
        let call =
            |lines: &[(&str, &str, i64)]| names_from_guesses(&timed(lines), &[], &[], &invited);
        let three = [
            ("s1", "Mohan, aap bata do.", 3_000),
            ("s4", "The export is done and the tests pass now.", 6_000),
            ("s1", "Okay. Mohan, your update on billing?", 3_000),
            ("s3", "Sorry, I was on mute, one second please.", 5_000),
            ("s1", "Theek hai. Mohan aap bolo.", 3_000),
            (
                "s4",
                "Billing ships on Friday, nothing else from me.",
                6_000,
            ),
        ];
        assert_eq!(
            call(&three),
            vec![("s4".to_string(), "Mohan Rao".to_string())]
        );
        assert!(call(&three[..2]).is_empty(), "one hand-over is not enough");
        let two_names = [
            ("s1", "Mohan, aap bata do.", 3_000),
            ("s4", "The export is done and the tests pass now.", 6_000),
            ("s1", "Priya, your update?", 3_000),
            (
                "s4",
                "Billing ships on Friday, nothing else from me.",
                6_000,
            ),
        ];
        assert!(call(&two_names).is_empty());
    }

    #[test]
    fn a_guess_never_overrides_names_from_hand_overs() {
        let invited = invited(&["Mohan Rao", "Priya Nair"]);
        let lines = timed(&[
            ("s1", "Mohan, aap bata do.", 3_000),
            ("s4", "The export is done and the tests pass now.", 6_000),
            ("s1", "Thanks Mohan. Mohan, aur kuch?", 3_000),
            ("s4", "No, that is all from me for today.", 6_000),
            ("s4", "Hi, I'm Priya.", 2_000),
            ("s4", "Priya here.", 2_000),
        ]);
        let names = names_from_guesses(
            &lines,
            &[guess("Speaker 4", "Priya", &[4, 5])],
            &[],
            &invited,
        );
        assert_eq!(names, vec![("s4".to_string(), "Mohan Rao".to_string())]);
    }

    fn addressed_at(text: &str, name: &str) -> bool {
        let ws = line_words(text);
        let p = ws.iter().position(|w| w.lower == name).unwrap();
        addressed(&ws, p)
    }

    #[test]
    fn a_name_spoken_to_is_found_by_its_punctuation() {
        assert!(addressed_at("Varun, any update?", "varun"));
        assert!(addressed_at("Varun?", "varun"));
        assert!(addressed_at("Okay. Varun, kya scene hai?", "varun"));
        assert!(addressed_at("Any questions, Varun?", "varun"));
        assert!(!addressed_at("Any questions Varun?", "varun"));
        assert!(addressed_at("That's it from me, Varun.", "varun"));
        assert!(addressed_at("Varun ji aap bataiye", "varun"));
        assert!(addressed_at("Thanks, Varun ji?", "varun"));
        assert!(addressed_at("Varun bhai, kya hua", "varun"));
        assert!(!addressed_at("Varun bhai kya hua", "varun"));
        assert!(!addressed_at("Did you sync with Varun?", "varun"));
        assert!(addressed_at("but Varun, I think you can take it", "varun"));
        assert!(addressed_at("Okay so Varun, any blockers?", "varun"));
        assert!(addressed_at("Theek hai Varun, aage?", "varun"));
        assert!(!addressed_at("but Varun will send it", "varun"));
        assert!(!addressed_at("okay so yes Varun, any blockers?", "varun"));
        assert!(!addressed_at("I think Varun, maybe later", "varun"));
        assert!(!addressed_at("Varun will send you the deck.", "varun"));
        assert!(!addressed_at("Varun shared your numbers too.", "varun"));
        assert!(addressed_at("Varun, go ahead.", "varun"));
        assert!(addressed_at("Okay Varun your turn", "varun"));
        assert!(!addressed_at("Varun did that yesterday.", "varun"));
        assert!(!addressed_at("We should ask Varun about it.", "varun"));
        assert!(!addressed_at("Ask Varun.", "varun"));
        assert!(!addressed_at("I told Varun, and then we left.", "varun"));
        assert!(!addressed_at("I'll sync with Varun.", "varun"));
        assert!(!addressed_at("I'll sync with Varun", "varun"));
    }

    #[test]
    fn hand_over_words_count_only_in_the_names_sentence() {
        assert!(!addressed_at("Thanks Varun. Priya, your update?", "varun"));
        assert!(addressed_at("Thanks Varun. Priya, your update?", "priya"));
        let invited = invited(&["Varun Rao", "Priya Nair"]);
        let lines = timed(&[
            ("s1", "Thanks Varun. Priya, your update?", 3_000),
            ("s4", "The export is done and the tests pass now.", 6_000),
        ]);
        let found: Vec<String> = hand_overs(&lines, &invited, &HashSet::new())
            .into_iter()
            .map(|(_, _, name)| name)
            .collect();
        assert_eq!(found, vec!["Priya Nair".to_string()]);
    }

    #[test]
    fn plain_questions_and_statements_twice_name_nobody() {
        let invited = invited(&["Mohan Rao"]);
        for (a, b) in [
            ("Did you sync with Mohan?", "Okay. Did you sync with Mohan?"),
            (
                "Mohan will send you the deck.",
                "Mohan shared your numbers too.",
            ),
        ] {
            let lines = timed(&[
                ("s1", a, 3_000),
                ("s4", "The export is done and the tests pass now.", 6_000),
                ("s1", b, 3_000),
                ("s4", "No, that is all from me for today.", 6_000),
            ]);
            assert!(
                names_from_guesses(&lines, &[], &[], &invited).is_empty(),
                "{a}"
            );
        }
    }

    #[test]
    fn a_mic_line_is_an_echo_only_if_the_call_said_the_name() {
        let invited = invited(&["Priya Nair"]);
        let mut lines = timed(&[
            ("s1", "Okay aap bata do na.", 3_000),
            (
                "s2",
                "Sure, the dashboard is done and the tests pass.",
                6_000,
            ),
        ]);
        lines.insert(
            1,
            Segment {
                start_ms: 500,
                end_ms: 2_900,
                ..seg("me", "Priya aap bata do")
            },
        );
        assert_eq!(hand_overs(&lines, &invited, &HashSet::new()).len(), 1);
    }

    #[test]
    fn a_plain_statement_twice_names_nobody() {
        let invited = invited(&["Mohan Rao"]);
        let lines = timed(&[
            ("s1", "I'll sync with Mohan.", 3_000),
            ("s4", "The export is done and the tests pass now.", 6_000),
            ("s1", "Okay, I'll sync with Mohan.", 3_000),
            ("s4", "No, that is all from me for today.", 6_000),
        ]);
        assert!(names_from_guesses(&lines, &[], &[], &invited).is_empty());
    }

    #[test]
    fn an_echo_on_the_microphone_counts_once() {
        let invited = invited(&["Priya Nair"]);
        let mut lines = timed(&[
            ("s1", "Priya aap bata do.", 3_000),
            (
                "s2",
                "Sure, the dashboard is done and the tests pass.",
                6_000,
            ),
        ]);
        lines.insert(
            1,
            Segment {
                start_ms: 500,
                end_ms: 2_900,
                ..seg("me", "Priya aap bata do")
            },
        );
        assert_eq!(hand_overs(&lines, &invited, &HashSet::new()).len(), 1);
        assert!(names_from_guesses(&lines, &[], &[], &invited).is_empty());
    }

    #[test]
    fn a_shared_first_name_needs_the_surname() {
        let invited = invited(&["Mohan Rao", "Mohan Das"]);
        let call = |a: &str, b: &str| {
            names_from_guesses(
                &timed(&[
                    ("s1", a, 3_000),
                    ("s4", "The export is done and the tests pass now.", 6_000),
                    ("s1", b, 3_000),
                    ("s4", "No, that is all from me for today.", 6_000),
                ]),
                &[],
                &[],
                &invited,
            )
        };
        assert!(call("Mohan, your update?", "Mohan, aap bolo.").is_empty());
        assert_eq!(
            call("Mohan Rao, your update?", "Okay. Mohan Rao, aap bolo."),
            vec![("s4".to_string(), "Mohan Rao".to_string())]
        );
    }

    #[test]
    fn being_spoken_to_twice_names_a_voice() {
        let invited = invited(&["Mohan Rao"]);
        let lines = timed(&[
            ("s1", "Okay. Mohan, any update?", 3_000),
            ("s4", "The export is done and the tests pass now.", 6_000),
            ("s1", "Good. Any questions, Mohan?", 3_000),
            ("s4", "No, that is all from me for today.", 6_000),
        ]);
        assert_eq!(
            names_from_guesses(&lines, &[], &[], &invited),
            vec![("s4".to_string(), "Mohan Rao".to_string())]
        );
        let mentioned = timed(&[
            ("s1", "Mohan did the export yesterday.", 3_000),
            ("s4", "The export is done and the tests pass now.", 6_000),
            ("s1", "We should ask Mohan about billing.", 3_000),
            ("s4", "No, that is all from me for today.", 6_000),
        ]);
        assert!(names_from_guesses(&mentioned, &[], &[], &invited).is_empty());
    }

    #[test]
    fn people_in_the_room_are_named_like_call_voices() {
        let invited = invited(&["Mohan Rao"]);
        let mut lines = timed(&[
            ("me", "Mohan, aap bata do.", 3_000),
            ("s2", "The export is done and the tests pass now.", 6_000),
            ("me", "Okay. Mohan, your update on billing?", 3_000),
            (
                "s2",
                "Billing ships on Friday, nothing else from me.",
                6_000,
            ),
        ]);
        for line in &mut lines {
            line.source = "mic".into();
        }
        assert_eq!(
            names_from_guesses(&lines, &[], &[], &invited),
            vec![("s2".to_string(), "Mohan Rao".to_string())]
        );
        let guess = guess("Speaker 2", "Mohan", &[0, 2]);
        let (named, _) = judge_guesses(&lines, &[guess], &[], &invited);
        assert_eq!(named.len(), 1);
        assert_eq!(named[0].key, "s2");
    }

    #[test]
    fn hand_over_words_follow_the_name() {
        assert!(addressed_at("Varun aap bata do", "varun"));
        assert!(addressed_at("okay over to Varun and then lunch", "varun"));
        assert!(!addressed_at("any questions Varun", "varun"));
        assert!(!addressed_at(
            "I think Varun will also get it done",
            "varun"
        ));
    }

    fn names_from_guesses(
        segments: &[Segment],
        guesses: &[SpeakerGuess],
        own_names: &[String],
        invited: &[String],
    ) -> Vec<(String, String)> {
        judge_guesses(segments, guesses, own_names, invited)
            .0
            .into_iter()
            .map(|n| (n.key, n.name))
            .collect()
    }

    fn guess(speaker: &str, name: &str, lines: &[usize]) -> SpeakerGuess {
        SpeakerGuess {
            speaker: speaker.into(),
            name: name.into(),
            lines: lines.to_vec(),
        }
    }

    fn invited(names: &[&str]) -> Vec<String> {
        names.iter().map(|n| n.to_string()).collect()
    }

    fn attendee(name: Option<&str>, email: Option<&str>, is_me: bool) -> Attendee {
        Attendee {
            name: name.map(Into::into),
            email: email.map(Into::into),
            is_me,
            ..Default::default()
        }
    }

    #[test]
    fn invitees_are_named_from_emails_without_rooms_or_mailboxes() {
        let email = |e: &str| attendee(Some(e), Some(e), false);
        let attendees = vec![
            attendee(
                Some("priya.shah@example.com"),
                Some("priya.shah@example.com"),
                true,
            ),
            email("varun.kapoor@example.com"),
            attendee(None, Some("kapil@example.com"), false),
            email("naresh_rao2+cal@example.com"),
            attendee(Some("Meera Iyer"), Some("m.iyer@example.com"), false),
            attendee(Some("Neha Gupta"), None, false),
            email("Varun.Kapoor@example.com"),
            attendee(
                Some("Office-Room-7 (4)"),
                Some("c_1188abc@resource.calendar.google.com"),
                false,
            ),
            attendee(Some("Board Room"), None, false),
            attendee(Some("Pantry (12)"), None, false),
            email("noreply@example.com"),
            email("team-data@example.com"),
            email("a@example.com"),
        ];
        assert_eq!(
            invitees(&attendees),
            invited(&[
                "Varun Kapoor",
                "Kapil",
                "Naresh Rao",
                "Meera Iyer",
                "Neha Gupta"
            ])
        );
        assert_eq!(display_name(&attendees[0]).as_deref(), Some("Priya Shah"));
    }

    #[test]
    fn misspelt_name_said_before_a_speaker_names_the_invitee() {
        let segments = spaced(vec![
            seg("me", "Thanks Gotam, can you take the next one?"),
            seg("s2", "Sure, the pipeline is done."),
            seg("s1", "Okay Gotam, what about the dashboard?"),
            seg("s2", "It ships on Friday."),
        ]);
        let names = names_from_guesses(
            &segments,
            &[guess("Speaker 2", "Gotam", &[0, 2])],
            &[],
            &invited(&["Gautam Verma", "Varun Mehta"]),
        );
        assert_eq!(names, vec![("s2".into(), "Gautam Verma".into())]);

        let once = names_from_guesses(
            &segments,
            &[guess("Speaker 2", "Gotam", &[0])],
            &[],
            &invited(&["Gautam Verma"]),
        );
        assert!(once.is_empty(), "being addressed once is not enough");
    }

    #[test]
    fn one_introduction_is_enough_only_for_an_invitee() {
        let segments = vec![
            seg("s1", "Hi, this is Gautam from the data team."),
            seg("s1", "The numbers look fine."),
            seg("s2", "Gautam, can you share the sheet?"),
            seg("s2", "Is Naresh here yet?"),
        ];
        let gautam = invited(&["Gautam Verma", "Naresh Rao"]);
        let names = names_from_guesses(
            &segments,
            &[guess("Speaker 1", "Gautam", &[0])],
            &[],
            &gautam,
        );
        assert_eq!(names, vec![("s1".into(), "Gautam Verma".into())]);
        assert!(
            names_from_guesses(&segments, &[guess("Speaker 1", "Gautam", &[0])], &[], &[])
                .is_empty(),
            "a name nobody invited still needs two lines"
        );
        assert!(
            names_from_guesses(
                &segments,
                &[guess("Speaker 2", "Gautam", &[2])],
                &[],
                &gautam
            )
            .is_empty(),
            "saying a name is not introducing yourself"
        );
        assert!(
            names_from_guesses(
                &segments,
                &[guess("Speaker 2", "Naresh", &[3])],
                &[],
                &gautam
            )
            .is_empty(),
            "asking whether someone is here is not introducing yourself"
        );
    }

    #[test]
    fn hinglish_introduction_with_a_misspelt_name() {
        let segments = vec![
            seg("s1", "haan main Nares bol raha hoon"),
            seg("s1", "kal ka deploy ho gaya"),
        ];
        let names = names_from_guesses(
            &segments,
            &[guess("Speaker 1", "Nares", &[0])],
            &[],
            &invited(&["Naresh Rao", "Priya Shah"]),
        );
        assert_eq!(names, vec![("s1".into(), "Naresh Rao".into())]);
    }

    #[test]
    fn misheard_names_need_a_close_sound_and_spelling() {
        assert!(sounds_like_name("gotam", "gautam"));
        assert!(sounds_like_name("nares", "naresh"));
        assert!(!sounds_like_name("near", "naresh"), "a common word");
        assert!(!sounds_like_name("kill", "gautam"));
        assert!(
            !sounds_like_name("on", "om"),
            "short names must match exactly"
        );
        assert!(sounds_like_name("om", "om"));
        assert!(!sounds_like_name("rahi", "raahi"), "an everyday Hindi word");
        assert!(!sounds_like_name("mera", "meera"));
        for (a, b) in [
            ("gaurav", "gourav"),
            ("gautam", "gotam"),
            ("sunil", "suneel"),
            ("vikas", "vikaas"),
            ("priya", "pria"),
            ("saurabh", "sourabh"),
            ("deepak", "dipak"),
            ("pooja", "puja"),
            ("shreya", "sreya"),
        ] {
            assert_eq!(name_sound(a), name_sound(b), "{a} sounds like {b}");
        }
        for (a, b) in [
            ("rajesh", "rakesh"),
            ("kavita", "savita"),
            ("geeta", "seeta"),
            ("sumit", "sunit"),
            ("neha", "meha"),
            ("jaya", "kaya"),
            ("kamal", "komal"),
            ("tania", "tina"),
            ("ankit", "ankita"),
            ("aman", "amin"),
            ("rohan", "rohin"),
        ] {
            assert_ne!(name_sound(a), name_sound(b), "{a} is not {b}");
        }
        for (heard, invited) in [
            ("komal", "kamal"),
            ("kamal", "komal"),
            ("tina", "tania"),
            ("ankita", "ankit"),
            ("ankit", "ankita"),
            ("amin", "aman"),
            ("rohin", "rohan"),
        ] {
            assert!(
                !sounds_like_name(heard, invited),
                "{heard} is not {invited}"
            );
        }
    }

    #[test]
    fn a_different_surname_is_a_different_person() {
        let people = invited(&["Varun Kapoor", "Priya Shah"]);
        assert_eq!(invitee_for("Varun Mehta", &people), None);
        assert_eq!(invitee_for("Varoon Mehta", &people), None);
        assert_eq!(
            invitee_for("Varun", &people).map(String::as_str),
            Some("Varun Kapoor")
        );
        assert_eq!(
            invitee_for("varun kapoor", &people).map(String::as_str),
            Some("Varun Kapoor")
        );
    }

    #[test]
    fn devanagari_names_stay_whole_words() {
        let people = invited(&["प्रिया शर्मा"]);
        let addressed = |text: &str| {
            let segments = spaced(vec![
                seg("me", text),
                seg("s1", "ठीक है"),
                seg("me", text),
                seg("s1", "हाँ"),
            ]);
            names_from_guesses(
                &segments,
                &[guess("Speaker 1", "प्रिया", &[0, 2])],
                &[],
                &people,
            )
        };
        assert!(addressed("प्रकाश, आगे बोलो").is_empty());
        assert_eq!(
            addressed("प्रिया, आगे बोलो"),
            vec![("s1".to_string(), "प्रिया शर्मा".to_string())]
        );
    }

    #[test]
    fn a_speaker_saying_a_name_is_not_given_it() {
        let segments = vec![
            seg("s1", "I will check with Gautam."),
            seg("s1", "Gautam sent me the sheet."),
        ];
        for people in [invited(&["Gautam Verma"]), Vec::new()] {
            assert!(names_from_guesses(
                &segments,
                &[guess("Speaker 1", "Gautam", &[0, 1])],
                &[],
                &people
            )
            .is_empty());
        }
    }

    #[test]
    fn misspelt_names_are_capitalised_and_never_everyday_words() {
        let people = invited(&["Gautam Verma", "Raahi Mehta"]);
        let rahi = vec![
            seg("s1", "main bol rahi hoon"),
            seg("s1", "haan, main bol rahi hoon kal"),
        ];
        assert!(
            names_from_guesses(&rahi, &[guess("Speaker 1", "Raahi", &[0, 1])], &[], &people)
                .is_empty()
        );
        let said = |line: &str| {
            names_from_guesses(
                &[seg("s1", line)],
                &[guess("Speaker 1", "Gautam", &[0])],
                &[],
                &people,
            )
        };
        let gautam = vec![("s1".to_string(), "Gautam Verma".to_string())];
        assert_eq!(said("This is Gotam."), gautam);
        assert!(said("this is gotam").is_empty(), "a lowercase misspelling");
        assert_eq!(said("this is gautam"), gautam, "the exact name");
    }

    fn intro(line: &str, name: &str) -> bool {
        let said = words(line);
        let name = words(name);
        let at = said.iter().position(|w| *w == name[0]).unwrap();
        introduces(&said, at, &name[1..])
    }

    #[test]
    fn only_a_line_that_starts_with_the_speakers_own_name_introduces() {
        for (line, name) in [
            ("This is Gautam.", "Gautam Verma"),
            ("Hi, it is Gautam here.", "Gautam Verma"),
            ("It's Gautam from the data team.", "Gautam Verma"),
            ("Hello, I am Gautam", "Gautam"),
            ("Okay so I'm Gautam Verma.", "Gautam Verma"),
            ("im Gautam speaking", "Gautam"),
            ("Good morning, my name is Gautam Verma.", "Gautam Verma"),
            ("Myself Gautam, this side.", "Gautam"),
            ("Gautam Verma here.", "Gautam Verma"),
            ("Gautam here from the data team", "Gautam"),
            ("Gautam speaking.", "Gautam"),
            ("Gautam this side", "Gautam"),
            ("haan main Naresh bol raha hoon", "Naresh Rao"),
            ("mai Neha hi bol rahi hun", "Neha"),
            ("Ji, mera naam Kapil hai.", "Kapil"),
            ("mera naam Kapil Dev", "Kapil Dev"),
        ] {
            assert!(intro(line, name), "{line}");
        }
        for (line, name) in [
            ("main Varun ko bol dunga", "Varun"),
            ("main Varun ko bol raha hoon", "Varun"),
            ("uska naam Kapil hai", "Kapil"),
            ("mera naam Kapil nahi hai", "Kapil"),
            ("Everyone, this is Naresh, our new joiner.", "Naresh"),
            ("This is Naresh, our new joiner.", "Naresh"),
            ("I told Varun.", "Varun"),
            ("Is Naresh here?", "Naresh"),
            ("Gautam, can you check?", "Gautam"),
            ("Hi Gautam, here is the deck.", "Gautam"),
            ("This is Gautam's laptop.", "Gautam"),
            ("Hey Gautam, here I have shared the screen.", "Gautam"),
            ("Hey, Gautam this side.", "Gautam"),
            ("Good morning Gautam, speaking of the launch", "Gautam"),
            ("Gautam here, joining late.", "Gautam"),
            ("Gautam speaking to the client now", "Gautam"),
            ("Haan Gautam, here is the link", "Gautam"),
            ("Okay Gautam, here.", "Gautam"),
            ("Ji Gautam speaking", "Gautam"),
            ("Yes, Gautam Verma here.", "Gautam Verma"),
        ] {
            assert!(!intro(line, name), "{line}");
        }
    }

    #[test]
    fn rooms_and_group_mailboxes_are_not_people() {
        let email = |e: &str| attendee(None, Some(e), false);
        let attendees = vec![
            attendee(Some("Conf Room 4A"), Some("room4a@corp.com"), false),
            email("room4a@corp.com"),
            attendee(Some("Boardroom"), Some("board@corp.com"), false),
            attendee(Some("Pantry (12)"), Some("pantry@corp.com"), false),
            email("data-eng@corp.com"),
            email("team-data@corp.com"),
            email("all-hands@corp.com"),
            email("datateam@corp.com"),
            email("support@corp.com"),
            email("abhinavkumarsingh@corp.com"),
            email("kapil@corp.com"),
            email("varun.kapoor@corp.com"),
            email("dev.sharma@corp.com"),
            email("hr_ops@corp.com"),
            email("sales.team@corp.com"),
            email("all.hands@corp.com"),
            email("eng.leads@corp.com"),
            email("founders@corp.com"),
            email("salesteam@corp.com"),
            email("dev.kumar.patel@corp.com"),
            email("data-raj@corp.com"),
            email("data-team@corp.com"),
            attendee(Some("Data Team"), Some("dt@corp.com"), false),
            attendee(Some("Priya Shah (Engineering)"), Some("ps@corp.com"), false),
            attendee(Some("Othello Ray"), None, false),
            attendee(Some("Andrea Leadsom"), None, false),
            attendee(Some("Maria Sales"), None, false),
            attendee(Some("Lim Eng Hock"), None, false),
            attendee(Some("Dev K."), None, false),
            attendee(Some("The Sales Team"), None, false),
            attendee(Some("Engineering"), None, false),
            Attendee {
                name: Some("Lab Kit".into()),
                not_person: true,
                ..Default::default()
            },
        ];
        assert_eq!(
            invitees(&attendees),
            invited(&[
                "Kapil",
                "Varun Kapoor",
                "Dev Sharma",
                "Dev Kumar Patel",
                "Data Raj",
                "Priya Shah",
                "Othello Ray",
                "Andrea Leadsom",
                "Maria Sales",
                "Lim Eng Hock",
                "Dev K.",
            ])
        );
    }

    #[test]
    fn two_speakers_never_get_the_same_name() {
        let segments = vec![seg("s1", "Hi, this is Gautam."), seg("s2", "Gautam here.")];
        let people = invited(&["Gautam Verma", "Varun Mehta"]);
        let both = names_from_guesses(
            &segments,
            &[
                guess("Speaker 1", "Gautam", &[0]),
                guess("Speaker 2", "Gautam", &[1]),
            ],
            &[],
            &people,
        );
        assert!(both.is_empty(), "an even claim names nobody");

        let mut named = segments.clone();
        apply_name(&mut named, "s1", "Gautam");
        let taken = names_from_guesses(&named, &[guess("Speaker 2", "Gautam", &[1])], &[], &people);
        assert!(taken.is_empty(), "Speaker 1 already has that name");
    }

    /// Real-call check, run by hand (prints counts and timings only):
    /// LIPIK_SPEAKER_MODELS=<dir> LIPIK_DIARIZE_WAV=<system.wav> cargo test --lib
    /// real_call -- --ignored --nocapture
    #[test]
    #[ignore]
    fn real_call() {
        let models = std::env::var("LIPIK_SPEAKER_MODELS").expect("LIPIK_SPEAKER_MODELS");
        let wav = std::env::var("LIPIK_DIARIZE_WAV").expect("LIPIK_DIARIZE_WAV");
        let seconds = hound::WavReader::open(&wav).unwrap().duration() as f32 / RATE as f32;
        let started = std::time::Instant::now();
        let s = run(Path::new(&models), Path::new(&wav)).unwrap();
        let took = started.elapsed().as_secs_f32();
        let mut closest = 0f32;
        for (i, a) in s.voices.iter().enumerate() {
            for b in &s.voices[i + 1..] {
                if let (Some(a), Some(b)) = (a, b) {
                    closest = closest.max(cosine(a, b));
                }
            }
        }
        let speech: Vec<i64> = s.speech_ms.iter().map(|ms| ms / 1000).collect();
        println!(
            "REAL audio_s={seconds:.0} run_s={took:.1} speakers={} turns={} changes={} speech_s={speech:?} closest_pair={closest:.2}",
            s.count(),
            s.turns.len(),
            s.change_samples().len()
        );
        if let Ok(out) = std::env::var("LIPIK_DIARIZE_VOICES") {
            let voices: Vec<Vec<f32>> = s.voices.iter().flatten().cloned().collect();
            std::fs::write(out, serde_json::to_string(&voices).unwrap()).unwrap();
        }
    }

    use super::super::window::{self, Sample};

    /// Window samples every 700 ms from `from` to `to`, `speaking` shown
    /// talking, Mohan and Priya present, Mohan signed in to Slack.
    fn shown(from: i64, to: i64, speaking: &[&str]) -> Vec<Sample> {
        (from..to)
            .step_by(700)
            .map(|ms| Sample {
                ms_since_meeting_start: ms,
                speaking: speaking.iter().map(|s| s.to_string()).collect(),
                present: vec!["Mohan".into(), "Priya".into()],
                gap: false,
                user: Some("Mohan".into()),
            })
            .collect()
    }

    /// The samples as read when Slack's signed-in name was not found.
    fn unsigned(mut samples: Vec<Sample>) -> Vec<Sample> {
        for s in &mut samples {
            s.user = None;
        }
        samples
    }

    fn turn(start: i64, end: i64, key: &str) -> (i64, i64, String) {
        (start, end, key.to_string())
    }

    fn mohan() -> Vec<String> {
        vec![
            "Mohan Lal".to_string(),
            "mohan.kumar@example.com".to_string(),
        ]
    }

    fn window_named(
        samples: &[Sample],
        turns: &[(i64, i64, String)],
        mic: &[(i64, i64)],
        own: &[String],
    ) -> Vec<(String, String)> {
        window_names(samples, turns, mic, own)
            .into_iter()
            .inspect(|n| assert_eq!(n.rule, NameRule::Window))
            .map(|n| (n.key, n.name))
            .collect()
    }

    #[test]
    fn the_window_names_the_voice_it_shows_talking() {
        let samples = window::parse(include_str!("window_speakers_fixture.jsonl"));
        let turns = [
            turn(2_300, 19_800, "s1"),
            turn(30_400, 44_500, "s1"),
            turn(45_000, 50_000, "s1"),
            turn(50_300, 57_900, "s2"),
        ];
        let mic = [(20_200, 29_600), (38_000, 41_000)];
        let named = window_named(&samples, &turns, &mic, &mohan());
        assert_eq!(named, [("s1".to_string(), "Priya".to_string())]);
        let named = window_named(&unsigned(samples), &turns, &mic, &[]);
        assert_eq!(
            named,
            [("s1".to_string(), "Priya".to_string())],
            "Mohan is found as the user by talking while only the microphone does"
        );
    }

    #[test]
    fn a_short_overlap_names_nobody() {
        let samples = shown(0, 9_000, &["Priya"]);
        let named = window_named(&samples, &[turn(0, 9_000, "s1")], &[], &mohan());
        assert!(named.is_empty());
    }

    #[test]
    fn a_mixed_voice_needs_most_of_its_speech_under_one_name() {
        let mut samples = shown(0, 12_600, &["Priya"]);
        samples.extend(shown(12_600, 21_000, &["Ravi"]));
        let named = window_named(&samples, &[turn(0, 21_000, "s1")], &[], &mohan());
        assert!(named.is_empty(), "60% Priya is not enough");
    }

    #[test]
    fn the_users_own_tile_never_names_a_call_voice() {
        let samples = shown(0, 20_000, &["Mohan"]);
        let turns = [turn(0, 20_000, "s1")];
        assert!(window_named(&samples, &turns, &[], &mohan()).is_empty());

        let mut samples = shown(0, 9_000, &["Mohan"]);
        samples.extend(shown(9_100, 29_000, &["Mohan", "Priya"]));
        let named = window_named(
            &unsigned(samples),
            &[turn(9_100, 29_000, "s1")],
            &[(0, 9_000)],
            &[],
        );
        assert_eq!(
            named,
            [("s1".to_string(), "Priya".to_string())],
            "talking over the user still counts for the other person"
        );
    }

    #[test]
    fn the_user_is_known_by_full_name_never_by_first_name_or_email() {
        let samples: Vec<Sample> = unsigned(shown(0, 20_000, &["Mohan Kumar"]));
        let turns = [turn(0, 20_000, "s1")];
        assert_eq!(
            window_named(&samples, &turns, &[], &mohan()),
            [("s1".to_string(), "Mohan Kumar".to_string())],
            "Mohan Lal's colleague Mohan Kumar is not the user"
        );
        let own = ["Mohan  kumar".to_string()];
        assert!(window_named(&samples, &turns, &[], &own).is_empty());
    }

    #[test]
    fn slacks_signed_in_name_is_the_user() {
        let mut samples = shown(0, 20_000, &["Priya"]);
        for s in &mut samples {
            s.user = Some("Priya".into());
        }
        let turns = [turn(0, 20_000, "s1")];
        assert!(window_named(&samples, &turns, &[], &[]).is_empty());
    }

    /// Samples with only `tiles` present, `speaking` talking, Slack's
    /// signed-in label `user`.
    fn tiles(from: i64, to: i64, tiles: &[&str], speaking: &[&str], user: &str) -> Vec<Sample> {
        shown(from, to, speaking)
            .into_iter()
            .map(|mut s| {
                s.present = tiles.iter().map(|t| t.to_string()).collect();
                s.user = Some(user.to_string());
                s
            })
            .collect()
    }

    #[test]
    fn a_voice_talking_over_the_user_never_gets_the_users_name() {
        let both = ["Mohan Kumar", "Priya"];
        for label in ["Mohan", "Mohan Kumar Rao"] {
            let mut samples = tiles(0, 10_000, &both, &["Mohan Kumar"], label);
            samples.extend(tiles(10_000, 30_000, &both, &["Mohan Kumar"], label));
            let turns = [turn(10_000, 30_000, "s1")];
            let mic = [(0, 30_000)];
            assert!(
                window_named(&samples, &turns, &mic, &[]).is_empty(),
                "label {label}: the call voice talked over Mohan Kumar's tile"
            );
        }
    }

    #[test]
    fn the_label_matching_two_tiles_is_settled_by_the_microphone() {
        let all = ["Mohan Kumar", "Mohan Lal"];
        let mut samples = tiles(0, 10_000, &all, &["Mohan Kumar"], "Mohan");
        samples.extend(tiles(10_000, 30_000, &all, &["Mohan Lal"], "Mohan"));
        let named = window_named(&samples, &[turn(10_000, 30_000, "s1")], &[(0, 10_000)], &[]);
        assert_eq!(named, [("s1".to_string(), "Mohan Lal".to_string())]);
    }

    #[test]
    fn a_label_matching_no_tile_falls_back_to_the_microphone() {
        let all = ["Mohan", "Priya"];
        let mut samples = tiles(0, 10_000, &all, &["Mohan"], "Gourav");
        samples.extend(tiles(10_000, 30_000, &all, &["Mohan"], "Gourav"));
        let named = window_named(&samples, &[turn(10_000, 30_000, "s1")], &[(0, 10_000)], &[]);
        assert!(named.is_empty(), "Mohan is the user by the microphone");
    }

    #[test]
    fn the_microphone_fallback_ignores_marks_that_trail_the_voice() {
        let mut samples = Vec::new();
        let mut mic = Vec::new();
        for j in 0..10 {
            let t = j * 3_000;
            samples.extend(shown(t, t + 1_400, &["Priya"]));
            samples.extend(shown(t + 1_400, t + 3_000, &[]));
            mic.push((t, t + 1_400));
        }
        samples.extend(shown(30_000, 50_000, &["Priya"]));
        let named = window_named(&unsigned(samples), &[turn(30_000, 50_000, "s1")], &mic, &[]);
        assert_eq!(
            named,
            [("s1".to_string(), "Priya".to_string())],
            "Priya's mark still on while the user starts talking does not make her the user"
        );
    }

    #[test]
    fn two_voices_one_name_goes_to_the_one_heard_more() {
        let samples = shown(0, 60_000, &["Priya"]);
        let turns = [turn(0, 9_000, "s1"), turn(9_000, 55_000, "s2")];
        let named = window_named(&samples, &turns, &[], &mohan());
        assert_eq!(named, [("s2".to_string(), "Priya".to_string())]);

        let turns = [turn(0, 11_000, "s1"), turn(11_000, 55_000, "s2")];
        assert!(
            window_named(&samples, &turns, &[], &mohan()).is_empty(),
            "a second voice with 10 s or more under the tile names neither"
        );

        let turns = [turn(0, 12_000, "s1"), turn(12_000, 30_000, "s2")];
        assert!(
            window_named(&samples, &turns, &[], &mohan()).is_empty(),
            "a tile shared by two voices that both talk a lot names neither"
        );

        let turns = [turn(0, 15_000, "s1"), turn(15_000, 30_000, "s2")];
        assert!(
            window_named(&samples, &turns, &[], &mohan()).is_empty(),
            "a tie names neither"
        );
    }

    #[test]
    fn gaps_and_a_stalled_reader_count_for_nothing() {
        let mut samples = shown(0, 5_000, &["Priya"]);
        samples.push(Sample {
            ms_since_meeting_start: 5_000,
            gap: true,
            ..Default::default()
        });
        samples.extend(shown(20_000, 23_000, &["Priya"]));
        let turns = [turn(0, 26_000, "s1")];
        assert!(window_named(&samples, &turns, &[], &mohan()).is_empty());

        let sparse: Vec<Sample> = shown(0, 30_000, &["Priya"])
            .into_iter()
            .step_by(10)
            .collect();
        assert!(
            window_named(&sparse, &[turn(0, 30_000, "s1")], &[], &mohan()).is_empty(),
            "one sample every 7 s stands for 1.5 s each"
        );
    }

    #[test]
    fn the_window_beats_the_calendar_but_not_a_saved_voice() {
        let naming = |key: &str, name: &str| Naming {
            key: key.into(),
            name: name.into(),
            rule: NameRule::Window,
            lines: Vec::new(),
        };
        let saved = |name: &str| name == "Asha";

        let mut known = HashMap::from([(0, "Ravi".to_string())]);
        let kept = add_window_names(&mut known, &saved, &[naming("s1", "Priya")]);
        assert_eq!(known, HashMap::from([(0, "Priya".to_string())]));
        assert!(kept.is_empty());

        let mut known = HashMap::from([(0, "Asha".to_string())]);
        let kept = add_window_names(&mut known, &saved, &[naming("s1", "Priya")]);
        assert_eq!(known, HashMap::from([(0, "Asha".to_string())]));
        assert_eq!(kept, [("s1".into(), "Asha".into(), "Priya".into())]);

        let mut known = HashMap::from([(1, "Asha".to_string())]);
        let kept = add_window_names(&mut known, &saved, &[naming("s1", "asha")]);
        assert_eq!(known, HashMap::from([(1, "Asha".to_string())]));
        assert_eq!(kept.len(), 1, "the name already belongs to a saved voice");

        let mut known = HashMap::from([(1, "Priya".to_string())]);
        add_window_names(&mut known, &saved, &[naming("s1", "Priya")]);
        assert_eq!(known, HashMap::from([(0, "Priya".to_string())]));
    }

    #[test]
    fn apply_name_skips_renamed_lines() {
        let mut segments = vec![seg("s1", "a"), seg("s1", "b"), seg("s2", "c")];
        segments[1].speaker = "Asha".into();
        apply_name(&mut segments, "s1", "Priya");
        let labels: Vec<&str> = segments.iter().map(|s| s.speaker.as_str()).collect();
        assert_eq!(labels, vec!["Priya", "Asha", "Speaker 2"]);
    }
}
