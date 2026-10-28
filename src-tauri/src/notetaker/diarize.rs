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

/// How alike the spellings of a heard word and an invited first name must be.
const SAME_NAME_SPELLING: f64 = 0.6;
/// Invited first names shorter than this must be heard exactly.
const MIN_FUZZY_NAME_CHARS: usize = 3;
/// Everyday Hindi and Hinglish words that sound like names ("rahi" like
/// Raahi), never taken for a misspelt name.
const HINGLISH_WORDS: &[&str] = &[
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
fn name_sound(word: &str) -> Vec<char> {
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
fn word_char(c: char) -> bool {
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
/// or someone else saying it right before that speaker talks (being
/// addressed). A name on the invite is written as
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
                let Some(line) = segments.get(i).filter(|_| points_at(i, key)) else {
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
    let mut scored: Vec<Evidence> = Vec::new();
    for e in evidence.into_values() {
        if e.lines.len() >= 2 || (e.invited && e.introduced) {
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
    let mut keys = HashSet::new();
    let mut picked: Vec<String> = Vec::new();
    let mut out = Vec::new();
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
fn own_set(own_names: &[String]) -> HashSet<String> {
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
        let segments = vec![
            seg("me", "Thanks Gotam, can you take the next one?"),
            seg("s2", "Sure, the pipeline is done."),
            seg("s1", "Okay Gotam, what about the dashboard?"),
            seg("s2", "It ships on Friday."),
        ];
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
            let segments = vec![
                seg("me", text),
                seg("s1", "ठीक है"),
                seg("me", text),
                seg("s1", "हाँ"),
            ];
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

    #[test]
    fn apply_name_skips_renamed_lines() {
        let mut segments = vec![seg("s1", "a"), seg("s1", "b"), seg("s2", "c")];
        segments[1].speaker = "Asha".into();
        apply_name(&mut segments, "s1", "Priya");
        let labels: Vec<&str> = segments.iter().map(|s| s.speaker.as_str()).collect();
        assert_eq!(labels, vec!["Priya", "Asha", "Speaker 2"]);
    }
}
