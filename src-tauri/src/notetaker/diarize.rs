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

/// The key of the n-th speaker (0-based), in order of first appearance.
pub fn speaker_key(n: usize) -> String {
    format!("s{}", n + 1)
}

/// The key and default label of a speaker index (`UNKNOWN` is "Them").
pub fn key_and_label(n: usize) -> (String, String) {
    if n == UNKNOWN {
        return (THEM_KEY.to_string(), super::transcribe::THEM.to_string());
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
#[derive(Clone, Debug, PartialEq)]
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
        bail!(
            "{} min of call audio is over the 3 hour limit",
            seconds / 60
        );
    }
    limit_threads();
    let audio = read_wav(wav)?;
    if audio.len() < MIN_AUDIO_SAMPLES {
        bail!("too little call audio");
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
        bail!("no speech found in the call audio");
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

/// A saved voice: its name and average embedding.
pub struct VoiceProfile {
    pub name: String,
    pub voice: Vec<f32>,
}

/// Names known before the notes are written: a saved voice first, then the
/// calendar when exactly one other person was invited and one voice was heard.
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

    let others: Vec<&Attendee> = attendees.iter().filter(|a| !a.is_me).collect();
    if speakers.count() == 1 && others.len() == 1 && !names.contains_key(&0) {
        if let Some(name) = others[0]
            .name
            .as_deref()
            .map(str::trim)
            .filter(|n| !n.is_empty())
        {
            names.insert(0, name.to_string());
        }
    }
    names
}

/// A name the notes model offered for one speaker label, with the transcript
/// lines (indexes) where it says that name.
#[derive(Clone, Debug, PartialEq)]
pub struct SpeakerGuess {
    pub speaker: String,
    pub name: String,
    pub lines: Vec<usize>,
}

fn words(text: &str) -> Vec<String> {
    text.split(|c: char| !c.is_alphanumeric())
        .filter(|w| !w.is_empty())
        .map(str::to_lowercase)
        .collect()
}

/// Names from the notes model, kept only when the evidence holds up: the
/// name is said in at least two of the lines it points to, each either said
/// by that speaker (an introduction) or said by someone else right before
/// that speaker talks (being addressed). Never the user's own name (`own`:
/// names and emails) or an email, and never a name another speaker claims as
/// strongly. Only voice-split speakers still on their default label are
/// named. Returns (speaker key, name).
pub fn names_from_guesses(
    segments: &[Segment],
    guesses: &[SpeakerGuess],
    own_names: &[String],
) -> Vec<(String, String)> {
    let mut own: HashSet<String> = HashSet::new();
    for v in own_names {
        own.insert(v.trim().to_lowercase());
        if let Some(first) = words(v).first() {
            own.insert(first.clone());
        }
    }
    let reserved = ["me", "them", "you", "others", "speaker", "unknown", "user"];

    let mut label_key: HashMap<&str, &str> = HashMap::new();
    for s in segments {
        if s.source == "system"
            && s.speaker_key.starts_with('s')
            && s.speaker == default_label(&s.speaker_key)
        {
            label_key.insert(&s.speaker, &s.speaker_key);
        }
    }

    let points_at = |i: usize, key: &str| {
        segments.get(i).is_some_and(|s| {
            s.speaker_key == key
                || segments
                    .get(i + 1)
                    .is_some_and(|next| next.speaker_key == key)
        })
    };
    let mut evidence: HashMap<(String, String), (String, HashSet<usize>)> = HashMap::new();
    for g in guesses {
        let name = g.name.trim();
        let lower = name.to_lowercase();
        let Some(first) = words(name).into_iter().next() else {
            continue;
        };
        if name.contains('@')
            || own.contains(&lower)
            || own.contains(&first)
            || reserved.contains(&first.as_str())
        {
            continue;
        }
        let Some(&key) = label_key.get(g.speaker.trim()) else {
            continue;
        };
        let said: HashSet<usize> = g
            .lines
            .iter()
            .copied()
            .filter(|&i| {
                points_at(i, key)
                    && segments
                        .get(i)
                        .is_some_and(|s| words(&s.text).contains(&first))
            })
            .collect();
        evidence
            .entry((key.to_string(), lower))
            .or_insert_with(|| (name.to_string(), HashSet::new()))
            .1
            .extend(said);
    }

    let mut scored: Vec<(usize, String, String)> = evidence
        .into_iter()
        .filter(|(_, (_, lines))| lines.len() >= 2)
        .map(|((key, _), (name, lines))| (lines.len(), key, name))
        .collect();
    let contested = |a: &(usize, String, String), list: &[(usize, String, String)]| {
        list.iter()
            .any(|b| b.0 == a.0 && ((b.1 == a.1) != b.2.eq_ignore_ascii_case(&a.2)))
    };
    scored.sort_by(|a, b| b.0.cmp(&a.0).then(a.1.cmp(&b.1)));
    let mut keys = HashSet::new();
    let mut taken = HashSet::new();
    let mut out = Vec::new();
    for entry in &scored {
        let (_, key, name) = entry;
        if keys.contains(key) || taken.contains(&name.to_lowercase()) {
            continue;
        }
        keys.insert(key.clone());
        taken.insert(name.to_lowercase());
        if !contested(entry, &scored) {
            out.push((key.clone(), name.clone()));
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
            },
            Attendee {
                name: Some("Asha".into()),
                email: Some("asha@x.com".into()),
                is_me: false,
            },
        ];
        let names = known_names(&one, &[], &attendees);
        assert_eq!(names.get(&0).map(String::as_str), Some("Asha"));
        assert!(known_names(&speakers, &[], &attendees).is_empty());
    }

    #[test]
    fn guessed_names_need_two_lines_and_never_the_user() {
        let segments = vec![
            seg("me", "Priya, can you share the deck?"),
            seg("s1", "Sure, sharing now."),
            seg("me", "Thanks Priya, and the numbers?"),
            seg("s1", "Here they are."),
            seg("s2", "Hi, I'm Ravi from finance."),
            seg("s2", "Gourav, the budget is fine."),
            seg("me", "Gourav here, thanks."),
        ];
        let me = vec![
            "Gourav Sharma".to_string(),
            "gourav@keychain.com".to_string(),
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
                guess("Speaker 2", "gourav@keychain.com", &[5, 6]),
                guess("Speaker 2", "Gourav", &[5, 6]),
                guess("Me", "Gourav", &[5, 6]),
            ],
            &me,
        );
        assert_eq!(names, vec![("s1".to_string(), "Priya".to_string())]);

        let made_up = names_from_guesses(&segments, &[guess("Speaker 2", "Meera", &[4, 5])], &me);
        assert!(made_up.is_empty(), "the name must be said in those lines");
        let elsewhere = names_from_guesses(&segments, &[guess("Speaker 2", "Priya", &[0, 2])], &me);
        assert!(
            elsewhere.is_empty(),
            "Priya is addressed right before Speaker 1, not Speaker 2"
        );
    }

    #[test]
    fn names_differing_only_in_case_agree() {
        let segments = vec![seg("s1", "I'm Asha"), seg("s1", "Asha again")];
        let guess = |name: &str, lines: &[usize]| SpeakerGuess {
            speaker: "Speaker 1".into(),
            name: name.into(),
            lines: lines.to_vec(),
        };
        let names = names_from_guesses(&segments, &[guess("Asha", &[0]), guess("asha", &[1])], &[]);
        assert_eq!(names.len(), 1);
        assert!(names[0].1.eq_ignore_ascii_case("asha"));
    }

    #[test]
    fn a_tie_between_names_names_nobody() {
        let segments = vec![
            seg("s1", "Asha and Bela are here"),
            seg("s1", "Asha and Bela again"),
        ];
        let guess = |name: &str| SpeakerGuess {
            speaker: "Speaker 1".into(),
            name: name.into(),
            lines: vec![0, 1],
        };
        assert!(names_from_guesses(&segments, &[guess("Asha"), guess("Bela")], &[]).is_empty());
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

    #[test]
    fn apply_name_skips_renamed_lines() {
        let mut segments = vec![seg("s1", "a"), seg("s1", "b"), seg("s2", "c")];
        segments[1].speaker = "Asha".into();
        apply_name(&mut segments, "s1", "Priya");
        let labels: Vec<&str> = segments.iter().map(|s| s.speaker.as_str()).collect();
        assert_eq!(labels, vec!["Priya", "Asha", "Speaker 2"]);
    }
}
