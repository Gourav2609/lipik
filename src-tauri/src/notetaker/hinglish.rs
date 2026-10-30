//! Hinglish meetings. Pieces go through Qwen3-ASR, which keeps Hindi words
//! (in Devanagari) and names that the English models turn into English
//! look-alikes. The clean-up model then writes each line in Roman letters;
//! a line it fails on, translates, or whose numbers it changes is written by
//! letter rules instead ([`transliterate`]), so no Devanagari is left.
//! Names of the people in the meeting are put back in Roman letters before
//! and after it, because it reads names such as "तरुण" as Hindi words ("young").
//!
//! Each meeting decides for itself unless the setting says English or
//! Hinglish: with Qwen3-ASR on the Mac, a few pieces are heard by it first;
//! without it, the English transcript is checked for words that are not
//! English, and the model is fetched to redo the meeting.

use anyhow::{anyhow, Result};
use std::collections::HashSet;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tauri::{AppHandle, Manager};

use super::ai;
use super::audio::CutRules;
use super::diarize;
use super::Segment;
use crate::managers::model::ModelManager;
use crate::managers::transcription::SideEngine;

/// Qwen3-ASR models by catalog id, best first. The model list knows each
/// quant file by its own id; see [`model_ids`].
pub const MODELS: &[&str] = &["Qwen3-ASR-1.7B", "Qwen3-ASR-0.6B"];
/// The one fetched when a meeting sounds Hinglish.
pub const DOWNLOAD: &str = MODELS[0];

/// Model list ids of every quant of the Qwen3-ASR models, best model first
/// and each model's default quant first.
pub fn model_ids() -> Vec<String> {
    MODELS
        .iter()
        .flat_map(|id| crate::catalog::model_ids(id))
        .collect()
}

/// Model list id of the file fetched when a meeting sounds Hinglish.
pub fn download_id() -> Result<String> {
    crate::catalog::model_ids(DOWNLOAD)
        .into_iter()
        .next()
        .ok_or_else(|| anyhow!("the Hinglish speech model is missing from the model list"))
}

/// Whether a model list id is one of the Qwen3-ASR files, any quant.
pub fn is_model(model_id: &str) -> bool {
    model_ids().iter().any(|id| id == model_id)
}

/// The best Qwen3-ASR file that `ready` says is on this Mac.
fn first_ready(ready: impl Fn(&str) -> bool) -> Option<String> {
    model_ids().into_iter().find(|id| ready(id))
}

/// Qwen3-ASR stops early on pieces longer than about 15 s, dropping the rest.
pub const RULES: CutRules = CutRules {
    min_s: 2.0,
    max_s: 10.0,
    pause_ms: 400,
};

/// The hint the standup test ran with: Qwen3-ASR still writes Hindi in
/// Devanagari and English in English with it.
pub const LANGUAGE: &str = "en";

/// A reply this much shorter than the line has dropped words.
const MIN_KEPT_WORDS: f32 = 0.6;

/// Pieces heard by Qwen3-ASR before choosing, spread over the meeting.
pub const PROBE_PIECES: usize = 6;
/// Longest probe piece.
pub const PROBE_MAX_MS: i64 = 10_000;
/// Shortest line worth probing.
pub const PROBE_MIN_MS: i64 = 2_000;
/// Share of Devanagari words in the probe pieces that means Hinglish. Six
/// pieces of a Hinglish standup gave 0.41 to 0.51; 164 of 165 English clips
/// by Indian speakers gave none, the other 0.06 for one clip.
pub const PROBE_HINDI: f32 = 0.15;

/// Share of words (3+ letters, not all capitals) in an English transcript
/// that are neither common English words nor names, above which Hindi was
/// likely spoken. Hinglish meetings gave 0.077 to 0.164; English meetings
/// 0.018 to 0.038 (0.065 for one short, noisy test call).
pub const ODD_WORDS_HINDI: f32 = 0.07;
/// Fewer words than this say too little to judge.
pub const ODD_WORDS_MIN: usize = 200;
/// English jargon is not English either (a jargon-heavy standup gave 0.205
/// non-English words), so Hindi also needs Hindi words the English model
/// still wrote: at least 3, and 0.2% of all words. The Hinglish standup had
/// 4 (0.29%); jargon and English meetings 0 to 5, at most 0.17% for a
/// meeting that was mostly English.
pub const HINDI_WORDS_SHARE: f32 = 0.002;
pub const HINDI_WORDS_MIN: usize = 3;
/// ... or the English model wrote few words for the speech it was given: it
/// leaves out most Hindi, while jargon still comes out as words. Words per
/// minute of speech (as the speech detector heard it, call plus mic with the
/// call taken out): English meetings 159 to 169, meetings partly in Hindi
/// 80 to 97, Hinglish meetings 37 to 68 (33 and 59 for two under 3
/// minutes).
pub const FEW_WORDS_PER_MIN: f32 = 100.0;
/// So few words per minute of speech mean Hindi even when the transcript is
/// too short for [`ODD_WORDS_MIN`]. Hinglish meetings went down to 37.
pub const VERY_FEW_WORDS_PER_MIN: f32 = 50.0;
/// ... as long as this share of non-English words (of at least
/// [`VERY_FEW_JUDGED_MIN`]) is there too: an English meeting where a video
/// or music played also gets few words for its speech.
pub const VERY_FEW_ODD_WORDS: f32 = 0.05;
pub const VERY_FEW_JUDGED_MIN: usize = 50;
/// Less speech than this says too little about the word rate.
pub const RATE_MIN_SPEECH_MS: i64 = 3 * 60_000;
/// Everyday Hindi words in Roman letters; those that are also English words
/// don't count.
const HINDI_SIGNAL: &[&str] = &[
    "hai", "hain", "ka", "ki", "ke", "se", "mein", "nahi", "nahin", "toh", "kya", "matlab",
    "accha", "acha", "achha", "theek", "thik", "haan", "kar", "karna", "karo", "raha", "rahe",
    "rahi", "tha", "thi", "hoga", "hogi", "wala", "wali", "bhi", "aur", "lekin", "abhi", "sab",
    "kuch", "yaar", "bhai", "ji", "bata", "batao", "bolo", "dekho", "chalo", "aap", "tum", "mujhe",
    "mera", "meri", "kaise", "kyun", "phir",
];

/// The best Qwen3-ASR model on this Mac, if any.
pub fn downloaded_model(app: &AppHandle) -> Option<String> {
    let models = app.try_state::<Arc<ModelManager>>()?;
    first_ready(|id| {
        models
            .get_model_info(id)
            .is_some_and(|m| m.is_downloaded && !m.is_downloading)
    })
}

/// Most dictionary words and names given to Qwen3-ASR to listen for.
const MAX_DICTIONARY_HOTWORDS: usize = 45;
const MAX_NAME_HOTWORDS: usize = 15;

/// Words for Qwen3-ASR to listen for: some of the user's dictionary and
/// the meeting's names, each once, with room kept for the names.
pub fn hotwords(dictionary: &[String], names: &[String]) -> Vec<String> {
    let mut words: Vec<String> = Vec::new();
    let mut add = |list: &[String], most: usize| {
        let mut added = 0;
        for word in list.iter().map(|w| w.trim()) {
            if added == most {
                break;
            }
            if !word.is_empty() && !words.iter().any(|w| w.eq_ignore_ascii_case(word)) {
                words.push(word.to_string());
                added += 1;
            }
        }
    };
    let dictionary: Vec<String> = dictionary
        .iter()
        .filter(|w| !w.contains('@'))
        .cloned()
        .collect();
    add(&dictionary, MAX_DICTIONARY_HOTWORDS);
    add(&name_parts(names), MAX_NAME_HOTWORDS);
    words
}

/// Qwen3-ASR loaded beside dictation, listening for `hotwords`, or `None`
/// when it isn't downloaded or doesn't load.
pub fn engine(app: &AppHandle, hotwords: Vec<String>) -> Option<SideEngine> {
    let model = downloaded_model(app)?;
    let tm = super::transcribe::transcription_manager(app).ok()?;
    match tm.side_batch_engine(&model, LANGUAGE) {
        Ok(mut engine) => {
            engine.run.vocabulary = hotwords;
            Some(engine)
        }
        Err(e) => {
            log::warn!("Notetaker: {model} did not load: {e}");
            None
        }
    }
}

/// Share of words written in Devanagari.
pub fn devanagari_share<'a>(texts: impl IntoIterator<Item = &'a str>) -> f32 {
    let (mut hindi, mut all) = (0usize, 0usize);
    for text in texts {
        for word in text.split(|c: char| !diarize::word_char(c)) {
            if word.is_empty() {
                continue;
            }
            all += 1;
            hindi += usize::from(has_devanagari(word));
        }
    }
    if all == 0 {
        0.0
    } else {
        hindi as f32 / all as f32
    }
}

/// Whether an English transcript sounds like Hindi was spoken; see
/// [`sounds_hindi`]. `speech` is how much speech the English model was given
/// per source; only those sources' words count for the word rate (a mic
/// that still holds the call is left out).
pub fn hindi_likely(segments: &[Segment], names: &[String], speech: &[(&str, i64)]) -> bool {
    let (odd, judged) = odd_words(segments, names);
    let words_of = |s: &Segment| -> Vec<String> {
        s.text
            .split(|c: char| !c.is_ascii_alphabetic())
            .filter(|w| !w.is_empty())
            .map(str::to_lowercase)
            .collect()
    };
    let words: Vec<String> = segments.iter().flat_map(words_of).collect();
    let signal: HashSet<&str> = HINDI_SIGNAL
        .iter()
        .copied()
        .filter(|w| !crate::textflow::commands::is_common_word(w))
        .collect();
    let hindi = words.iter().filter(|w| signal.contains(w.as_str())).count();
    let rate_words: usize = segments
        .iter()
        .filter(|s| speech.iter().any(|(source, _)| *source == s.source))
        .map(|s| words_of(s).len())
        .sum();
    let speech_ms: i64 = speech.iter().map(|(_, ms)| ms).sum();
    let share = odd as f32 / judged.max(1) as f32;
    log::info!(
        "Notetaker: non-English words {share:.3} of {judged}, Hindi words {hindi} of {}, {:.0} words per minute of {:.1} min of speech",
        words.len(),
        words_per_minute(rate_words, speech_ms),
        speech_ms as f32 / 60_000.0
    );
    sounds_hindi(share, judged, hindi, words.len(), (rate_words, speech_ms))
}

fn words_per_minute(words: usize, speech_ms: i64) -> f32 {
    words as f32 * 60_000.0 / speech_ms.max(1) as f32
}

/// Hindi was likely spoken when enough of the `judged` words are not English
/// (`odd`, [`ODD_WORDS_HINDI`]) and either the English model still wrote
/// some Hindi words or it wrote few words for the speech
/// ([`FEW_WORDS_PER_MIN`]); or when it wrote very few words for a lot of
/// speech ([`VERY_FEW_WORDS_PER_MIN`]) and some are not English.
/// `rate` is the words and the ms of speech they came from.
fn sounds_hindi(odd: f32, judged: usize, hindi: usize, words: usize, rate: (usize, i64)) -> bool {
    let (rate_words, speech_ms) = rate;
    let rate = (speech_ms >= RATE_MIN_SPEECH_MS).then(|| words_per_minute(rate_words, speech_ms));
    let hindi_words = hindi >= HINDI_WORDS_MIN && hindi as f32 >= words as f32 * HINDI_WORDS_SHARE;
    let few_words = rate.is_some_and(|r| r < FEW_WORDS_PER_MIN);
    let very_few_words = rate.is_some_and(|r| r < VERY_FEW_WORDS_PER_MIN);
    (judged >= ODD_WORDS_MIN && odd >= ODD_WORDS_HINDI && (hindi_words || few_words))
        || (very_few_words && judged >= VERY_FEW_JUDGED_MIN && odd >= VERY_FEW_ODD_WORDS)
}

/// Share of the transcript's words that are not English (see
/// [`ODD_WORDS_HINDI`]), or `None` when it is too short to tell.
#[cfg(test)]
fn odd_word_share(segments: &[Segment], names: &[String]) -> Option<f32> {
    let (odd, judged) = odd_words(segments, names);
    (judged >= ODD_WORDS_MIN).then(|| odd as f32 / judged as f32)
}

/// How many of the transcript's words (3+ letters, not all capitals) are
/// neither common English words nor names, and how many there are.
fn odd_words(segments: &[Segment], names: &[String]) -> (usize, usize) {
    let names: HashSet<String> = names
        .iter()
        .flat_map(|n| n.split(|c: char| !c.is_alphabetic()))
        .map(str::to_lowercase)
        .collect();
    let mut words: Vec<String> = Vec::new();
    for s in segments {
        words.extend(
            s.text
                .split(|c: char| !c.is_ascii_alphabetic())
                .filter(|w| w.len() >= 3 && w.chars().any(|c| c.is_ascii_lowercase()))
                .map(str::to_lowercase),
        );
    }
    let mut known: std::collections::HashMap<&str, bool> = Default::default();
    let odd = words
        .iter()
        .filter(|w| {
            !*known.entry(w.as_str()).or_insert_with(|| {
                names.contains(w.as_str()) || crate::textflow::commands::is_common_word(w)
            })
        })
        .count();
    (odd, words.len())
}

fn is_devanagari(c: char) -> bool {
    ('\u{0900}'..='\u{097F}').contains(&c)
}

pub fn has_devanagari(text: &str) -> bool {
    text.chars().any(is_devanagari)
}

fn consonant(c: char) -> Option<&'static str> {
    Some(match c {
        'क' | '\u{0958}' => "k",
        'ख' | '\u{0959}' => "kh",
        'ग' | '\u{095A}' => "g",
        'घ' => "gh",
        'ङ' | 'ञ' | 'ण' | 'न' => "n",
        'च' => "ch",
        'छ' => "chh",
        'ज' => "j",
        '\u{095B}' => "z",
        'झ' => "jh",
        'ट' | 'त' => "t",
        'ठ' | 'थ' => "th",
        'ड' | 'द' => "d",
        '\u{095C}' => "r",
        'ढ' | 'ध' => "dh",
        '\u{095D}' => "rh",
        'प' => "p",
        'फ' => "ph",
        '\u{095E}' => "f",
        'ब' => "b",
        'भ' => "bh",
        'म' => "m",
        'य' | '\u{095F}' => "y",
        'र' => "r",
        'ल' | 'ळ' => "l",
        'व' => "v",
        'श' | 'ष' => "sh",
        'स' => "s",
        'ह' => "h",
        _ => return None,
    })
}

fn vowel(c: char) -> Option<&'static str> {
    Some(match c {
        'अ' => "a",
        'आ' | 'ा' => "aa",
        'इ' | 'ि' => "i",
        'ई' | 'ी' => "ii",
        'उ' | 'ु' => "u",
        'ऊ' | 'ू' => "uu",
        'ऋ' | 'ृ' => "ri",
        'ए' | 'े' | 'ऍ' | 'ॅ' => "e",
        'ऐ' | 'ै' => "ai",
        'ओ' | 'ो' | 'ऑ' | 'ॉ' => "o",
        'औ' | 'ौ' => "au",
        _ => return None,
    })
}

/// A vowel the way Hinglish is typed: long vowels doubled inside a word
/// ("theek", "kaam", "hoon") and single at its end ("karna", "nahi", "bhai");
/// "ए" after a vowel takes a "y" ("gaye", "liye").
fn typed_vowel(c: char, at_end: bool, after_vowel: bool) -> Option<String> {
    let text = match (vowel(c)?, at_end) {
        ("aa", true) => "a",
        ("ii", true) => "i",
        ("ii", false) => "ee",
        ("uu", true) => "u",
        ("uu", false) => "oo",
        (v, _) => v,
    };
    let y = after_vowel && c == 'ए';
    Some(format!("{}{text}", if y { "y" } else { "" }))
}

/// How a consonant ends in writing.
#[derive(Clone, Copy, PartialEq)]
enum Ends {
    /// No sign: it carries the vowel "a".
    Inherent,
    Vowel,
    /// Virama: no vowel.
    Bare,
}

/// A Devanagari word in plain Roman letters, close enough to compare with a
/// name by sound: "तरुण" -> "tarun", "मेहता" -> "mehtaa", "दीपिका" ->
/// "diipikaa". The vowel a consonant carries is not said at the end of the
/// word, nor between a vowel and a consonant that has a vowel of its own
/// ("करना" -> "karnaa"), except in the first syllable.
fn roman_letters(word: &str) -> String {
    spell(word, false)
}

/// `word` as Hinglish is typed: "ठीक" -> "theek", "करना" -> "karna",
/// "हूँ" -> "hoon". Letters that are not Devanagari stay.
fn typed_letters(word: &str) -> String {
    spell(word, true)
}

fn spell(word: &str, typed: bool) -> String {
    let chars: Vec<char> = word.chars().collect();
    let letter_at = |i: usize| chars.get(i).is_some_and(|&c| is_devanagari(c));
    let mut units: Vec<(String, Ends)> = Vec::new();
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        if typed && chars[i..].starts_with(&['ज', '\u{094D}', 'ञ']) {
            units.push(("g".into(), Ends::Bare));
            units.push(("y".into(), Ends::Inherent));
            i += 3;
            if let Some(&m) = chars.get(i).filter(|m| vowel(**m).is_some()) {
                let unit = units.last_mut().unwrap();
                unit.0
                    .push_str(&typed_vowel(m, !letter_at(i + 1), false).unwrap_or_default());
                unit.1 = Ends::Vowel;
                i += 1;
            }
            continue;
        }
        let nukta = chars.get(i + 1) == Some(&'\u{093C}');
        let base = match (c, nukta) {
            ('ज', true) => Some("z"),
            ('फ', true) => Some("f"),
            ('ड', true) => Some(if typed { "d" } else { "r" }),
            ('ढ', true) if typed => Some("dh"),
            ('\u{095C}', _) if typed => Some("d"),
            ('\u{095D}', _) if typed => Some("dh"),
            _ => consonant(c),
        };
        i += 1 + usize::from(nukta && base.is_some());
        if let Some(base) = base {
            let mut text = base.to_string();
            let ends = match chars.get(i) {
                Some('\u{094D}') => {
                    i += 1;
                    Ends::Bare
                }
                Some(&m) if vowel(m).is_some() => {
                    let v = if typed {
                        typed_vowel(m, !letter_at(i + 1), false).unwrap_or_default()
                    } else {
                        vowel(m).unwrap_or_default().to_string()
                    };
                    text.push_str(&v);
                    i += 1;
                    Ends::Vowel
                }
                _ => Ends::Inherent,
            };
            units.push((text, ends));
        } else if vowel(c).is_some() {
            let v = if typed {
                let after_vowel = units.last().is_some_and(|(_, e)| *e != Ends::Bare);
                let at_end = !units.is_empty() && !letter_at(i);
                typed_vowel(c, at_end, after_vowel).unwrap_or_default()
            } else {
                vowel(c).unwrap_or_default().to_string()
            };
            units.push((v, Ends::Vowel));
        } else if matches!(c, 'ं' | 'ँ' | 'ः') {
            let sound = match c {
                'ः' => 'h',
                _ if typed
                    && chars
                        .get(i)
                        .is_some_and(|n| matches!(n, 'प' | 'फ' | 'ब' | 'भ' | 'म')) =>
                {
                    'm'
                }
                _ => 'n',
            };
            match units.last_mut() {
                Some((text, Ends::Inherent)) => {
                    text.push('a');
                    text.push(sound);
                    units.last_mut().unwrap().1 = Ends::Vowel;
                }
                Some((text, _)) => {
                    if typed && c != 'ः' && !letter_at(i) {
                        if text.ends_with("ee") {
                            text.truncate(text.len() - 2);
                            text.push('i');
                        } else if text.ends_with('e') {
                            text.push('i');
                        }
                    }
                    text.push(sound);
                }
                None => {}
            }
        } else if ('०'..='९').contains(&c) {
            let digit = char::from_digit(c as u32 - '०' as u32, 10).unwrap_or('0');
            units.push((digit.to_string(), Ends::Bare));
        } else if !is_devanagari(c) {
            units.push((c.to_string(), Ends::Bare));
        }
    }
    let mut out = String::new();
    let mut said_vowel = false;
    for n in 0..units.len() {
        let (text, ends) = &units[n];
        out.push_str(text);
        let voiced = match ends {
            Ends::Vowel => true,
            Ends::Bare => false,
            Ends::Inherent => {
                let next_voiced = units.get(n + 1).is_some_and(|(_, e)| match e {
                    Ends::Vowel => true,
                    Ends::Inherent => n + 2 < units.len(),
                    Ends::Bare => false,
                });
                let last = n + 1 == units.len();
                let keep = !last && (n == 0 || !said_vowel || !next_voiced);
                if keep {
                    out.push('a');
                }
                keep
            }
        };
        said_vowel = voiced;
    }
    if typed {
        out = out.replace("chchh", "cch").replace("chch", "cch");
    }
    out
}

/// English words written in Devanagari ("डेटा"), in English ("data").
static LOANWORDS: once_cell::sync::Lazy<std::collections::HashMap<&'static str, &'static str>> =
    once_cell::sync::Lazy::new(|| {
        include_str!("loanwords.txt")
            .lines()
            .filter(|l| !l.starts_with('#'))
            .filter_map(|l| l.split_once('\t'))
            .collect()
    });

/// Everyday words whose usual Roman spelling the letter rules miss.
const TYPED_WORDS: &[(&str, &str)] = &[
    ("नहीं", "nahi"),
    ("नही", "nahi"),
    ("में", "mein"),
    ("मैं", "main"),
    ("हाँ", "haan"),
    ("हां", "haan"),
    ("तो", "toh"),
    ("यह", "yeh"),
    ("वह", "woh"),
    ("क्यों", "kyun"),
    ("क्यूँ", "kyun"),
    ("क्योंकि", "kyunki"),
    ("यहाँ", "yahan"),
    ("यहां", "yahan"),
    ("वहाँ", "wahan"),
    ("वहां", "wahan"),
    ("कहाँ", "kahan"),
    ("कहां", "kahan"),
    ("अच्छा", "accha"),
    ("वाला", "wala"),
    ("वाली", "wali"),
    ("वाले", "wale"),
    ("गए", "gaye"),
    ("गई", "gayi"),
    ("गयी", "gayi"),
    ("भाई", "bhai"),
    ("ज़्यादा", "zyada"),
    ("ज्यादा", "zyada"),
    ("चाहिए", "chahiye"),
];

/// Every Devanagari word in `text` in Roman letters the way Hinglish is
/// typed; the rest stays as it is. A line that starts in Devanagari starts
/// with a capital.
pub fn transliterate(text: &str) -> String {
    if !has_devanagari(text) {
        return text.to_string();
    }
    let mut out = String::with_capacity(text.len());
    let mut word = String::new();
    let flush = |word: &mut String, out: &mut String| {
        if has_devanagari(word) {
            let typed = LOANWORDS
                .get(word.as_str())
                .or_else(|| {
                    TYPED_WORDS
                        .iter()
                        .find(|(w, _)| *w == word.as_str())
                        .map(|(_, t)| t)
                })
                .map_or_else(|| typed_letters(word), |t| t.to_string());
            out.push_str(&typed);
        } else {
            out.push_str(word);
        }
        word.clear();
    };
    for c in text.chars() {
        if diarize::word_char(c) {
            word.push(c);
        } else {
            flush(&mut word, &mut out);
            out.push(match c {
                '\u{0964}' | '\u{0965}' => '.',
                _ => c,
            });
        }
    }
    flush(&mut word, &mut out);
    let starts_devanagari = text
        .chars()
        .find(|c| diarize::word_char(*c))
        .is_some_and(is_devanagari);
    if starts_devanagari {
        if let Some(first) = out.chars().next().filter(|c| c.is_lowercase()) {
            out = first.to_uppercase().chain(out.chars().skip(1)).collect();
        }
    }
    out
}

/// The parts of the meeting's names worth protecting, as written.
fn name_parts(names: &[String]) -> Vec<String> {
    let mut parts: Vec<String> = Vec::new();
    for word in names
        .iter()
        .flat_map(|n| n.split_whitespace())
        .filter(|w| !w.contains('@'))
    {
        let word: String = word.chars().filter(|c| c.is_alphabetic()).collect();
        if word.chars().count() >= 3
            && word.is_ascii()
            && !parts.iter().any(|p| p.eq_ignore_ascii_case(&word))
        {
            parts.push(word);
        }
    }
    parts
}

/// The name part a Devanagari word spells, if exactly one sounds like it.
/// Everyday Hindi words never count.
fn name_for<'a>(word: &str, parts: &'a [String]) -> Option<&'a String> {
    let roman = roman_letters(word);
    if roman.chars().count() < 3
        || diarize::HINGLISH_WORDS.contains(&roman.as_str())
        || crate::textflow::hindi::WORDS.contains(&roman.as_str())
    {
        return None;
    }
    let sound = diarize::name_sound(&roman);
    let found: Vec<&String> = parts
        .iter()
        .filter(|p| diarize::name_sound(p) == sound)
        .collect();
    (found.len() == 1).then(|| found[0])
}

/// `text` with every Devanagari spelling of a name in `names` written as the
/// name: "तरुण आप बताओ" -> "Tarun आप बताओ".
pub fn protect_names(text: &str, names: &[String]) -> String {
    let parts = name_parts(names);
    if parts.is_empty() || !has_devanagari(text) {
        return text.to_string();
    }
    let mut out = String::with_capacity(text.len());
    let mut word = String::new();
    let flush = |word: &mut String, out: &mut String| {
        if word.chars().all(is_devanagari) {
            if let Some(name) = name_for(word, &parts) {
                out.push_str(name);
                word.clear();
                return;
            }
        }
        out.push_str(word);
        word.clear();
    };
    for c in text.chars() {
        if diarize::word_char(c) {
            word.push(c);
        } else {
            flush(&mut word, &mut out);
            out.push(c);
        }
    }
    flush(&mut word, &mut out);
    out
}

fn prompt(names: &[String]) -> String {
    let mut prompt = String::from(
        "Rewrite this line of a meeting transcript in Roman letters, the way Indians type \
Hinglish on WhatsApp. Hindi words get simple Roman spelling (hai, nahi, kya, matlab, abhi, \
karna). A word that is really English keeps its normal English spelling even when it is \
written in Hindi script, for example: मैसेज -> message, इश्यू -> issue, मीटिंग -> meeting, \
टीम -> team, कॉल -> call, प्रोजेक्ट -> project, डैशबोर्ड -> dashboard, डेटा -> data, \
क्लाइंट -> client, डेडलाइन -> deadline. Words already in Roman letters stay exactly as they \
are. Do not translate, do not add, drop or reorder words, no explanations. Return only the \
rewritten line.",
    );
    let parts = name_parts(names);
    if !parts.is_empty() {
        prompt.push_str(&format!(
            " These are names of people in the meeting: {}. Keep them exactly as written; \
never turn a name into a Hindi word.",
            parts.join(", ")
        ));
    }
    prompt
}

fn words_of(text: &str) -> impl Iterator<Item = &str> {
    text.split(|c: char| !diarize::word_char(c))
        .filter(|w| !w.is_empty())
}

/// Hindi words a line can hardly do without, in Devanagari.
const HINDI_GLUE: &[&str] = &[
    "है",
    "हैं",
    "नहीं",
    "क्या",
    "तो",
    "का",
    "के",
    "की",
    "को",
    "में",
    "से",
    "था",
    "थी",
    "थे",
    "भी",
    "ही",
    "ना",
    "हम",
    "आप",
    "और",
    "कर",
    "रहा",
    "रहे",
    "वो",
    "ये",
];

/// The reply put the line's Hindi into English: of the Hindi glue words in
/// the line, fewer than a third came back as Hindi in Roman letters.
fn translated(line: &str, reply: &str) -> bool {
    let glue = words_of(line).filter(|w| HINDI_GLUE.contains(w)).count();
    let hindi = words_of(reply)
        .map(str::to_lowercase)
        .filter(|w| {
            crate::textflow::hindi::WORDS.contains(&w.as_str())
                || diarize::HINGLISH_WORDS.contains(&w.as_str())
                || matches!(
                    w.as_str(),
                    "vo" | "wo" | "ye" | "hum" | "ham" | "na" | "bhi"
                )
        })
        .count();
    glue >= 3 && hindi * 3 < glue
}

/// Number words in English, Roman Hindi and Devanagari, by value. Words
/// that are more often something else ("saath", with) are left out on both
/// sides.
const NUMBER_WORDS: &[(&str, u64)] = &[
    ("zero", 0),
    ("one", 1),
    ("two", 2),
    ("three", 3),
    ("four", 4),
    ("five", 5),
    ("six", 6),
    ("seven", 7),
    ("eight", 8),
    ("nine", 9),
    ("ten", 10),
    ("eleven", 11),
    ("twelve", 12),
    ("thirteen", 13),
    ("fourteen", 14),
    ("fifteen", 15),
    ("sixteen", 16),
    ("seventeen", 17),
    ("eighteen", 18),
    ("nineteen", 19),
    ("twenty", 20),
    ("thirty", 30),
    ("forty", 40),
    ("fifty", 50),
    ("sixty", 60),
    ("seventy", 70),
    ("eighty", 80),
    ("ninety", 90),
    ("hundred", 100),
    ("thousand", 1_000),
    ("million", 1_000_000),
    ("billion", 1_000_000_000),
    ("ek", 1),
    ("do", 2),
    ("teen", 3),
    ("char", 4),
    ("chaar", 4),
    ("panch", 5),
    ("paanch", 5),
    ("chhe", 6),
    ("chhah", 6),
    ("chheh", 6),
    ("saat", 7),
    ("aath", 8),
    ("nau", 9),
    ("das", 10),
    ("gyarah", 11),
    ("barah", 12),
    ("baarah", 12),
    ("terah", 13),
    ("chaudah", 14),
    ("pandrah", 15),
    ("solah", 16),
    ("satrah", 17),
    ("atharah", 18),
    ("unnis", 19),
    ("bees", 20),
    ("tees", 30),
    ("chalis", 40),
    ("chaalis", 40),
    ("chalees", 40),
    ("pachas", 50),
    ("pachaas", 50),
    ("sau", 100),
    ("hazar", 1_000),
    ("hazaar", 1_000),
    ("lakh", 100_000),
    ("crore", 10_000_000),
    ("karod", 10_000_000),
    ("एक", 1),
    ("दो", 2),
    ("तीन", 3),
    ("चार", 4),
    ("पांच", 5),
    ("पाँच", 5),
    ("छह", 6),
    ("छः", 6),
    ("छे", 6),
    ("सात", 7),
    ("आठ", 8),
    ("नौ", 9),
    ("दस", 10),
    ("ग्यारह", 11),
    ("बारह", 12),
    ("तेरह", 13),
    ("चौदह", 14),
    ("पंद्रह", 15),
    ("पन्द्रह", 15),
    ("सोलह", 16),
    ("सत्रह", 17),
    ("अठारह", 18),
    ("उन्नीस", 19),
    ("बीस", 20),
    ("तीस", 30),
    ("चालीस", 40),
    ("पचास", 50),
    ("सौ", 100),
    ("हज़ार", 1_000),
    ("हजार", 1_000),
    ("लाख", 100_000),
    ("करोड़", 10_000_000),
    ("\u{0915}\u{0930}\u{094B}\u{0921}\u{093C}", 10_000_000),
];

/// Words that add to the number after them: "साढ़े तीन" = 3.5, "सवा दो" =
/// 2.25, in quarters.
const PLUS_QUARTERS: &[(&str, u64)] = &[
    ("साढ़े", 2),
    ("\u{0938}\u{093E}\u{0922}\u{093C}\u{0947}", 2),
    ("saadhe", 2),
    ("sadhe", 2),
    ("saade", 2),
    ("सवा", 1),
    ("sawa", 1),
    ("sava", 1),
];

/// Fractions said as one word, in quarters: "डेढ़" = 1.5, "ढाई" = 2.5.
const QUARTERS: &[(&str, u64)] = &[
    ("डेढ़", 6),
    ("\u{0921}\u{0947}\u{0922}\u{093C}", 6),
    ("dedh", 6),
    ("derh", 6),
    ("ढाई", 10),
    ("dhai", 10),
    ("dhaai", 10),
];

/// "एक बार" / "ek baar" is "once", not a count of one.
const TIMES_WORDS: &[&str] = &["बार", "baar", "bar"];

/// Whether `word` (lowercase) is a number word.
pub fn number_word(word: &str) -> bool {
    NUMBER_WORDS.iter().any(|(w, _)| *w == word) || QUARTERS.iter().any(|(w, _)| *w == word)
}

static NUMBER_TOKEN: once_cell::sync::Lazy<regex::Regex> = once_cell::sync::Lazy::new(|| {
    regex::Regex::new(r"[0-9०-९]+(?:[.,][0-9०-९]+)*|[\p{L}\p{M}]+").unwrap()
});

/// A number in quarters written out: 14 -> "3.5".
fn quarters_text(q: u64) -> String {
    match q % 4 {
        0 => (q / 4).to_string(),
        2 => format!("{}.5", q / 4),
        r => format!("{}.{}", q / 4, r * 25),
    }
}

/// The numbers said in `text`, digits and number words alike, a run of
/// number words read as one number ("bees hazaar" = 20000, "साढ़े तीन लाख"
/// = 350000), sorted.
fn numbers(text: &str) -> Vec<String> {
    let tokens: Vec<String> = NUMBER_TOKEN
        .find_iter(text)
        .map(|m| {
            m.as_str()
                .chars()
                .map(|c| match c {
                    '०'..='९' => char::from_digit(c as u32 - '०' as u32, 10).unwrap_or(c),
                    _ => c.to_lowercase().next().unwrap_or(c),
                })
                .collect()
        })
        .collect();
    let lookup =
        |table: &[(&str, u64)], word: &str| table.iter().find(|(w, _)| *w == word).map(|(_, v)| *v);
    #[derive(Default)]
    struct Run {
        total: u64,
        part: u64,
        plus: u64,
        open: bool,
    }
    let mut found = Vec::new();
    let mut run = Run::default();
    let close = |run: &mut Run, found: &mut Vec<String>| {
        if run.open {
            found.push(quarters_text(run.total + run.part));
        }
        *run = Run::default();
    };
    for (n, token) in tokens.iter().enumerate() {
        let once = matches!(token.as_str(), "ek" | "एक")
            && tokens
                .get(n + 1)
                .is_some_and(|t| TIMES_WORDS.contains(&t.as_str()));
        if let Some(q) = lookup(PLUS_QUARTERS, token) {
            run.plus = q;
            continue;
        }
        let quarters = if once {
            None
        } else if token.starts_with(|c: char| c.is_ascii_digit()) {
            let plain = token.replace(',', "");
            match plain.parse::<f64>() {
                Ok(v) if (v * 4.0).fract() == 0.0 && v >= 0.0 => Some((v * 4.0) as u64),
                _ => {
                    close(&mut run, &mut found);
                    found.push(plain);
                    continue;
                }
            }
        } else {
            lookup(QUARTERS, token)
                .or_else(|| lookup(NUMBER_WORDS, token).map(|v| v.saturating_mul(4)))
        };
        let word = lookup(NUMBER_WORDS, token).is_some();
        match quarters {
            Some(q) if q >= 4_000 && run.open && word => {
                run.total = run
                    .total
                    .saturating_add(run.part.max(4).saturating_mul(q / 4));
                run.part = 0;
            }
            Some(400) if run.open && word => run.part = run.part.max(4).saturating_mul(100),
            Some(q) => {
                run.part = run.part.saturating_add(q + run.plus);
                run.plus = 0;
                run.open = true;
            }
            None => close(&mut run, &mut found),
        }
    }
    close(&mut run, &mut found);
    found.sort();
    found
}

/// Whether the reply says the same numbers as the line: the Roman step may
/// change how a number is written ("पाँच" -> "paanch" or "5"), never which.
fn same_numbers(line: &str, reply: &str) -> bool {
    numbers(line) == numbers(reply)
}

/// One line in Roman letters, asked again once when the reply translated
/// it. A reply that lost words or stayed translated is not used.
fn romanize_line(model: &ai::Model, prompt: &str, line: &str) -> Result<String> {
    let ask = |extra: &str| -> Result<String> {
        let reply = model.chat_text(&format!(
            "{prompt}{extra}\n\n<transcript>\n{line}\n</transcript>"
        ))?;
        let text = reply
            .replace("<transcript>", "")
            .replace("</transcript>", "");
        let text = text.trim();
        if words_of(text).count() as f32 <= words_of(line).count() as f32 * MIN_KEPT_WORDS {
            return Err(anyhow!("the reply dropped words"));
        }
        Ok(text.to_string())
    };
    let first = ask("")?;
    if !translated(line, &first) {
        return Ok(first);
    }
    let again = ask(" Your last answer translated the Hindi into English: keep every Hindi word in Hindi, only in Roman letters.")?;
    if translated(line, &again) {
        return Err(anyhow!("the reply translated the line"));
    }
    Ok(again)
}

/// Lines failing in a row after which the rest are not tried.
const MAX_FAILS_IN_ROW: usize = 3;

/// How long writing a meeting in Roman letters may take: half the meeting,
/// at least 5 minutes and at most an hour. A 19-minute Hinglish call needed
/// about 4 minutes; a stuck model is caught sooner by the per-line timeout.
pub fn romanize_time(meeting_ms: i64) -> Duration {
    Duration::from_millis((meeting_ms.max(0) / 2) as u64)
        .clamp(Duration::from_secs(300), Duration::from_secs(3600))
}

/// Every line with Devanagari in Roman letters, names kept, by the clean-up
/// model. See [`romanize_with`].
pub fn romanize(
    model: Option<&ai::Model>,
    segments: &mut [Segment],
    names: &[String],
    time: Duration,
    keep_going: impl FnMut(f32) -> bool,
) {
    let prompt = prompt(names);
    match model {
        Some(m) => romanize_with(
            segments,
            names,
            time,
            |line| romanize_line(m, &prompt, line),
            keep_going,
        ),
        None => romanize_with(
            segments,
            names,
            Duration::ZERO,
            |_| Err(anyhow!("no clean-up model")),
            keep_going,
        ),
    }
}

/// Every line with Devanagari through `ask`, names kept. A line `ask`
/// fails on, or whose reply says other numbers, is written in Roman letters
/// by [`transliterate`], and so are all lines left once `ask` failed
/// `MAX_FAILS_IN_ROW` times in a row, `time` ran out, or `keep_going`
/// (given the share of lines done) said stop. No Devanagari is left.
pub fn romanize_with(
    segments: &mut [Segment],
    names: &[String],
    time: Duration,
    mut ask: impl FnMut(&str) -> Result<String>,
    mut keep_going: impl FnMut(f32) -> bool,
) {
    let started = Instant::now();
    let todo = segments.iter().filter(|s| has_devanagari(&s.text)).count();
    let (mut done, mut fails_in_row, mut failed, mut renumbered) = (0usize, 0usize, 0usize, 0usize);
    let mut trying = true;
    for s in segments.iter_mut().filter(|s| has_devanagari(&s.text)) {
        let line = protect_names(&s.text, names);
        trying = trying
            && fails_in_row < MAX_FAILS_IN_ROW
            && started.elapsed() < time
            && keep_going(done as f32 / todo.max(1) as f32);
        let roman = if trying {
            match ask(&line) {
                Ok(roman) if same_numbers(&line, &roman) => {
                    fails_in_row = 0;
                    protect_names(&roman, names)
                }
                Ok(roman) => {
                    fails_in_row = 0;
                    renumbered += 1;
                    log::debug!("Hinglish reply changed the numbers: {line} -> {roman}");
                    line
                }
                Err(e) => {
                    fails_in_row += 1;
                    failed += 1;
                    log::debug!("Hinglish line written by letter rules: {e}");
                    line
                }
            }
        } else {
            line
        };
        s.text = transliterate(&roman);
        done += 1;
    }
    if failed > 0 || renumbered > 0 || !trying {
        log::warn!(
            "Notetaker wrote Hinglish lines by letter rules: {failed} failed, {renumbered} changed numbers, stopped early: {}",
            !trying
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The model list a real start-up builds from the bundled catalog and an
    /// HF cache holding `files` (`(repo, filename)`).
    fn model_list(
        files: &[(&str, &str)],
    ) -> std::collections::HashMap<String, crate::managers::model::ModelInfo> {
        let cache = tempfile::TempDir::new().unwrap();
        for (repo, filename) in files {
            let dir = cache
                .path()
                .join(format!("models--{}", repo.replace('/', "--")));
            let snapshot = dir.join("snapshots").join("abc123");
            std::fs::create_dir_all(&snapshot).unwrap();
            std::fs::create_dir_all(dir.join("refs")).unwrap();
            std::fs::write(dir.join("refs").join("main"), "abc123").unwrap();
            std::fs::write(snapshot.join(filename), b"gguf").unwrap();
        }
        let mut list = std::collections::HashMap::new();
        ModelManager::seed_catalog_models(&mut list);
        ModelManager::discover_hf_cache_models_in(cache.path(), &mut list);
        // What the start-up disk check marks for the seeded default quants.
        for model in list.values_mut() {
            if let crate::managers::model::ModelSource::HuggingFace { repo_id, .. } = &model.source
            {
                if files.contains(&(repo_id.as_str(), model.filename.as_str())) {
                    model.is_downloaded = true;
                }
            }
        }
        list
    }

    fn ready_in(
        list: &std::collections::HashMap<String, crate::managers::model::ModelInfo>,
    ) -> impl Fn(&str) -> bool + '_ {
        |id| list.get(id).is_some_and(|m| m.is_downloaded)
    }

    #[test]
    fn the_download_is_the_default_quant_the_model_list_knows() {
        let id = download_id().unwrap();
        assert_eq!(id, "lipik/Qwen3-ASR-1.7B-gguf/Qwen3-ASR-1.7B-Q8_0.gguf");
        let list = model_list(&[]);
        assert!(list.contains_key(&id), "{id} is not in the model list");
        assert!(!list.contains_key(DOWNLOAD));
        for id in model_ids().iter().filter(|id| id.ends_with("-Q8_0.gguf")) {
            assert!(list.contains_key(id), "{id} is not in the model list");
        }
        assert!(first_ready(ready_in(&list)).is_none());
    }

    #[test]
    fn any_downloaded_quant_counts_best_model_first() {
        let q4 = "Qwen3-ASR-1.7B-Q4_K_M.gguf";
        let small = "Qwen3-ASR-0.6B-Q8_0.gguf";
        let list = model_list(&[("lipik/Qwen3-ASR-1.7B-gguf", q4)]);
        assert_eq!(
            first_ready(ready_in(&list)).as_deref(),
            Some("lipik/Qwen3-ASR-1.7B-gguf/Qwen3-ASR-1.7B-Q4_K_M.gguf")
        );
        let list = model_list(&[("lipik/Qwen3-ASR-0.6B-gguf", small)]);
        assert_eq!(
            first_ready(ready_in(&list)).as_deref(),
            Some("lipik/Qwen3-ASR-0.6B-gguf/Qwen3-ASR-0.6B-Q8_0.gguf")
        );
        let list = model_list(&[
            ("lipik/Qwen3-ASR-0.6B-gguf", small),
            ("lipik/Qwen3-ASR-1.7B-gguf", q4),
        ]);
        assert_eq!(
            first_ready(ready_in(&list)).as_deref(),
            Some("lipik/Qwen3-ASR-1.7B-gguf/Qwen3-ASR-1.7B-Q4_K_M.gguf")
        );
    }

    #[test]
    fn every_quant_id_is_a_hinglish_model() {
        let list = model_list(&[("lipik/Qwen3-ASR-1.7B-gguf", "Qwen3-ASR-1.7B-Q6_K.gguf")]);
        let qwen: Vec<&String> = list.keys().filter(|id| id.contains("Qwen3-ASR")).collect();
        assert_eq!(qwen.len(), 3, "{qwen:?}");
        assert!(qwen.iter().all(|id| is_model(id)));
        assert_eq!(model_ids().len(), 12);
        assert!(!is_model("Qwen3-ASR-1.7B"));
        assert!(!list
            .keys()
            .filter(|id| !id.contains("Qwen3-ASR"))
            .any(|id| is_model(id)));
    }

    fn names(list: &[&str]) -> Vec<String> {
        list.iter().map(|n| n.to_string()).collect()
    }

    #[test]
    fn devanagari_words_become_roman_letters() {
        assert_eq!(roman_letters("तरुण"), "tarun");
        assert_eq!(roman_letters("करना"), "karnaa");
        assert_eq!(roman_letters("मेहता"), "mehtaa");
        assert_eq!(roman_letters("कमलेश"), "kamlesh");
        assert_eq!(roman_letters("कोमल"), "komal");
        assert_eq!(roman_letters("दीपिका"), "diipikaa");
        assert_eq!(roman_letters("प्रिया"), "priyaa");
        assert_eq!(roman_letters("मुकुल"), "mukul");
        assert_eq!(roman_letters("ज़ैद"), "zaid");
        assert_eq!(roman_letters("अंकित"), "ankit");
    }

    #[test]
    fn invitee_names_in_devanagari_are_written_as_invited() {
        let invited = names(&["Varun Mehta", "Priya Nair", "Ankit"]);
        assert_eq!(
            protect_names("ठीक है वरुण आप बता दो", &invited),
            "ठीक है Varun आप बता दो"
        );
        assert_eq!(
            protect_names("प्रिया, your update? फिर अंकित", &invited),
            "Priya, your update? फिर Ankit"
        );
        assert_eq!(protect_names("मेहता जी", &invited), "Mehta जी");
    }

    #[test]
    fn everyday_hindi_words_are_never_names() {
        let invited = names(&["Tarun Shah", "Kal Ito"]);
        assert_eq!(
            protect_names("ये काम करना है, कल कर देंगे", &invited),
            "ये काम करना है, कल कर देंगे"
        );
        assert_eq!(protect_names("तरुण बता दो", &invited), "Tarun बता दो");
    }

    #[test]
    fn a_spelling_two_names_share_names_nobody() {
        let invited = names(&["Mukul Verma", "Mookul Iyer"]);
        assert_eq!(protect_names("मुकुल बोलो", &invited), "मुकुल बोलो");
    }

    fn line(text: &str) -> Segment {
        Segment {
            source: "system".into(),
            speaker: "Speaker 1".into(),
            speaker_key: "s1".into(),
            start_ms: 0,
            end_ms: 1_000,
            text: text.into(),
        }
    }

    #[test]
    fn devanagari_share_counts_words() {
        assert_eq!(devanagari_share(["ठीक है so we start", ""]), 0.4);
        assert_eq!(devanagari_share(["All English here."]), 0.0);
        assert_eq!(devanagari_share([""]), 0.0);
    }

    #[test]
    fn odd_words_show_hindi_in_an_english_transcript() {
        let english = line("we should ship the release on friday and then review the numbers with finance next week");
        let hinglish = line("Kalka kya result aya shaduka monday ka tomain bataya diya tha rerun karna dobara correction");
        let many = |s: &Segment| vec![s.clone(); 20];
        let names = vec!["Varun Mehta".to_string()];
        let english_share = odd_word_share(&many(&english), &names).unwrap();
        let hinglish_share = odd_word_share(&many(&hinglish), &names).unwrap();
        assert!(english_share < ODD_WORDS_HINDI, "{english_share}");
        assert!(hinglish_share > ODD_WORDS_HINDI, "{hinglish_share}");
        assert_eq!(odd_word_share(&[hinglish], &names), None);
        let named = line("Varun Varun Varun Mehta Mehta and the team");
        assert_eq!(odd_word_share(&vec![named; 40], &names), Some(0.0));
    }

    #[test]
    fn english_jargon_is_not_hindi() {
        let jargon = line("Merged the PR for webhook retries, the repo config had a stale env var so the deploy to staging failed, opened a Jira for the flaky onboarding test, Okta SAML callback throws, Terraform plan for VPC peering, GitHub Actions cache busted");
        let names = vec!["Varun Mehta".to_string()];
        let lines = vec![jargon; 20];
        assert!(odd_word_share(&lines, &names).unwrap() > ODD_WORDS_HINDI);
        let words = lines
            .iter()
            .flat_map(|s| s.text.split(|c: char| !c.is_ascii_alphabetic()))
            .filter(|w| !w.is_empty())
            .count() as i64;
        let speech_ms_at = |words_per_min: i64| words * 60_000 / words_per_min;
        assert!(!hindi_likely(
            &lines,
            &names,
            &[("system", speech_ms_at(150))]
        ));
        assert!(!hindi_likely(&lines, &names, &[]));
        let hinglish = line("Kalka kya result aya shaduka monday ka tomain bataya diya tha rerun karna dobara correction hai");
        assert!(hindi_likely(&vec![hinglish; 20], &names, &[]));
    }

    #[test]
    fn the_word_rate_counts_only_streams_whose_speech_was_measured() {
        let names = vec!["Varun Mehta".to_string()];
        let mut lines =
            vec![line("Kalka result aya shaduka monday tomain bataya rerun dobara correction"); 30];
        for s in lines.iter_mut().take(20) {
            s.source = "mic".into();
        }
        // The mic's echo could not be taken out, so only the call counts:
        // its 100 words in 3 minutes of speech are few.
        assert!(hindi_likely(&lines, &names, &[("system", 180_000)]));
        // Counting the mic's 200 words against the same speech hides that.
        assert!(!hindi_likely(
            &lines,
            &names,
            &[("system", 180_000), ("mic", 0)]
        ));
    }

    #[test]
    fn few_words_for_the_speech_mean_hindi() {
        let minute = 60_000;
        // (non-English share, words judged, Hindi words, words, speech).
        let english = [
            (0.023, 4000, 0, 5620, 35 * minute + 24_000),
            (0.026, 1300, 0, 1808, 10 * minute + 42_000),
        ];
        let partly_hindi = [
            (0.038, 2100, 5, 3018, 31 * minute + 6_000),
            (0.036, 5000, 20, 7020, 88 * minute + 6_000),
        ];
        let hinglish = [
            (0.087, 1000, 4, 1366, 26 * minute + 24_000),
            (0.077, 290, 2, 384, 7 * minute + 12_000),
            (0.164, 380, 2, 492, 13 * minute + 20_032),
        ];
        for (odd, judged, hindi, words, speech) in english.into_iter().chain(partly_hindi) {
            assert!(
                !sounds_hindi(odd, judged, hindi, words, (words, speech)),
                "{words}"
            );
        }
        for (odd, judged, hindi, words, speech) in hinglish {
            assert!(
                sounds_hindi(odd, judged, hindi, words, (words, speech)),
                "{words}"
            );
        }
        assert!(
            !sounds_hindi(0.164, 380, 2, 492, (492, 0)),
            "no speech measured"
        );
        assert!(
            !sounds_hindi(0.02, 1500, 0, 2000, (2000, 15 * minute)),
            "133 a minute"
        );
        assert!(
            sounds_hindi(0.09, 110, 0, 150, (150, 5 * minute)),
            "30 a minute, too few words for the share alone"
        );
        assert!(
            !sounds_hindi(0.03, 110, 0, 150, (150, 5 * minute)),
            "30 a minute of English: a video or music played"
        );
        assert!(
            !sounds_hindi(0.2, 10, 0, 12, (12, 5 * minute)),
            "too few words"
        );
        assert!(
            !sounds_hindi(0.2, 30, 0, 40, (40, 2 * minute)),
            "too little speech"
        );
    }

    #[test]
    fn romanizing_stops_after_three_failures_in_a_row() {
        let mut lines: Vec<Segment> = (0..6).map(|_| line("ठीक है वरुण")).collect();
        let names = vec!["Varun Mehta".to_string()];
        let mut asked = 0;
        romanize_with(
            &mut lines,
            &names,
            Duration::from_secs(60),
            |_| {
                asked += 1;
                Err(anyhow!("timed out"))
            },
            |_| true,
        );
        assert_eq!(asked, 3);
        assert!(
            lines.iter().all(|s| s.text == "Theek hai Varun"),
            "{lines:?}"
        );
        lines = (0..6).map(|_| line("ठीक है वरुण")).collect();

        let mut asked = 0;
        romanize_with(
            &mut lines,
            &names,
            Duration::from_secs(60),
            |_| {
                asked += 1;
                Ok("theek hai Varun".into())
            },
            |done| done < 0.5,
        );
        assert_eq!(asked, 3, "stops when told to");
        assert_eq!(romanize_time(30 * 60_000), Duration::from_secs(900));
        assert_eq!(romanize_time(60_000), Duration::from_secs(300));
        assert_eq!(romanize_time(4 * 3600_000), Duration::from_secs(3600));
    }

    #[test]
    fn devanagari_is_written_the_way_hinglish_is_typed() {
        for (hindi, roman) in [
            ("है", "hai"),
            ("नहीं", "nahi"),
            ("क्या", "kya"),
            ("ठीक", "theek"),
            ("करना", "karna"),
            ("मतलब", "matlab"),
            ("समझ", "samajh"),
            ("हूँ", "hoon"),
            ("थोड़ा", "thoda"),
            ("ज़्यादा", "zyada"),
            ("लिए", "liye"),
            ("गए", "gaye"),
            ("ज्ञान", "gyaan"),
            ("बच्चे", "bacche"),
            ("पूरा", "poora"),
            ("तीन", "teen"),
            ("२५", "25"),
        ] {
            assert_eq!(
                transliterate(hindi),
                roman.to_string()[..1].to_uppercase() + &roman[1..],
                "{hindi}"
            );
            assert_eq!(
                transliterate(&format!("ok {hindi}")),
                format!("ok {roman}"),
                "{hindi}"
            );
        }
    }

    #[test]
    fn a_whole_line_keeps_its_english_and_punctuation() {
        assert_eq!(
            transliterate("ठीक है, status क्या होना चाहिए exactly? लेकिन नहीं।"),
            "Theek hai, status kya hona chahiye exactly? lekin nahi."
        );
        assert_eq!(
            transliterate("Matlab kि does that hotे hain"),
            "Matlab ki does that hote hain"
        );
        assert_eq!(transliterate("All English."), "All English.");
    }

    #[test]
    fn numbers_are_compared_by_value() {
        let line = "पाँच failed हुए, बीस हज़ार rows में से 85% ठीक";
        assert!(same_numbers(
            line,
            "paanch failed hue, bees hazaar rows mein se 85% theek"
        ));
        assert!(same_numbers(
            line,
            "5 failed hue, 20,000 rows mein se 85% theek"
        ));
        assert!(!same_numbers(
            line,
            "kuch failed hue, bees hazaar rows mein se 85% theek"
        ));
        assert!(!same_numbers(
            line,
            "5 failed hue, 2,000 rows mein se 85% theek"
        ));
        assert!(!same_numbers("दो दिन", "two-three din"));
        assert!(same_numbers("बता दो", "bata do"));
        assert!(same_numbers("2.5 lakh", "2.5 lakh"));
        assert_eq!(numbers("तीन सौ पचास"), vec!["350"]);
        assert_eq!(numbers("one and two"), vec!["1", "2"]);
    }

    #[test]
    fn fractions_and_big_numbers_are_compared_by_value() {
        assert_eq!(numbers("डेढ़ लाख"), vec!["150000"]);
        assert_eq!(numbers("ढाई सौ"), vec!["250"]);
        assert_eq!(numbers("साढ़े तीन करोड़"), vec!["35000000"]);
        assert_eq!(numbers("सवा दो"), vec!["2.25"]);
        assert!(same_numbers("साढ़े तीन बजे", "3.5 baje"));
        assert!(same_numbers("डेढ़ लाख rows", "dedh lakh rows"));
        assert!(same_numbers("डेढ़ लाख rows", "1.5 lakh rows"));
        assert!(!same_numbers("डेढ़ लाख rows", "1 lakh rows"));
        assert!(!same_numbers("ढाई दिन", "2 din"));
    }

    #[test]
    fn once_is_not_a_count() {
        assert!(numbers("एक बार देख लो").is_empty());
        assert!(same_numbers("एक बार देख लो", "once dekh lo"));
        assert!(same_numbers("एक बार देख लो", "ek baar dekh lo"));
        assert_eq!(numbers("एक file"), vec!["1"]);
    }

    #[test]
    fn english_words_in_devanagari_come_back_in_english() {
        assert_eq!(
            transliterate("ये डेटा कंपनी के ऑफिस में है, स्टेटस ओके"),
            "Ye data company ke office mein hai, status ok"
        );
        assert_eq!(
            transliterate("मीटिंग में क्लाइंट का आइडिया, ईमेल भेजो"),
            "Meeting mein client ka idea, email bhejo"
        );
        assert_eq!(transliterate("ok पीओसी गूगल"), "ok POC Google");
        for line in include_str!("loanwords.txt")
            .lines()
            .filter(|l| !l.starts_with('#'))
        {
            let (hindi, english) = line.split_once('\t').expect(line);
            assert!(has_devanagari(hindi) && !has_devanagari(english), "{line}");
        }
    }

    #[test]
    fn nasal_endings_are_typed_the_usual_way() {
        assert_eq!(
            transliterate("ok कहीं यहीं करें बातें हूँ"),
            "ok kahin yahin karein baatein hoon"
        );
    }

    #[test]
    fn hotwords_keep_room_for_names() {
        let dictionary: Vec<String> = (0..80).map(|n| format!("Zorvik{n}")).collect();
        let words = hotwords(&dictionary, &names(&["Varun Mehta", "Ira Ostrova"]));
        assert_eq!(words.len(), 45 + 4);
        assert_eq!(&words[45..], ["Varun", "Mehta", "Ira", "Ostrova"]);
    }

    #[test]
    fn a_reply_with_other_numbers_is_written_by_letter_rules() {
        let mut lines = vec![line("पाँच failed हुए"), line("ठीक है")];
        romanize_with(
            &mut lines,
            &[],
            Duration::from_secs(60),
            |l| {
                Ok(if l.contains("failed") {
                    "Kuch failed hue".into()
                } else {
                    "Theek hai".into()
                })
            },
            |_| true,
        );
        assert_eq!(lines[0].text, "Paanch failed huye");
        assert_eq!(lines[1].text, "Theek hai");
    }

    #[test]
    fn hotwords_put_the_dictionary_first_and_skip_repeats() {
        let words = hotwords(
            &[
                "Brimvale".to_string(),
                "Kaychorn".to_string(),
                "".to_string(),
            ],
            &names(&["Varun Mehta", "varun@brimvale.co", "Brimvale"]),
        );
        assert_eq!(words, ["Brimvale", "Kaychorn", "Varun", "Mehta"]);
    }

    /// Rebuilds hindi_lexicon.txt from the Aksharantar Hindi files:
    /// LIPIK_AKSHARANTAR=<folder with hin_train/valid/test.json>
    /// cargo test --lib writes_hindi_lexicon -- --ignored
    #[test]
    #[ignore]
    fn writes_hindi_lexicon() {
        use std::collections::BTreeSet;
        let dir = std::path::PathBuf::from(std::env::var("LIPIK_AKSHARANTAR").unwrap());
        let roman = regex::Regex::new("^[a-z]{3,15}$").unwrap();
        let mut words = BTreeSet::new();
        for part in ["train", "valid", "test"] {
            let text = std::fs::read_to_string(dir.join(format!("hin_{part}.json"))).unwrap();
            for line in text.lines() {
                let row: serde_json::Value = serde_json::from_str(line).unwrap();
                if !matches!(
                    row["source"].as_str(),
                    Some("AK-Freq" | "Dakshina" | "Existing")
                ) {
                    continue;
                }
                let native = row["native word"].as_str().unwrap_or_default();
                let given = row["english word"]
                    .as_str()
                    .unwrap_or_default()
                    .to_lowercase();
                for word in [given, typed_letters(native)] {
                    if roman.is_match(&word) && !crate::textflow::commands::is_common_word(&word) {
                        words.insert(word);
                    }
                }
            }
        }
        let header = "# Romanised Hindi words: never taken for a misheard dictionary word.\n\
# Built by `writes_hindi_lexicon` (notetaker/hinglish.rs) from the Hindi part of\n\
# Aksharantar by AI4Bharat (https://huggingface.co/datasets/ai4bharat/Aksharantar),\n\
# its AK-Freq, Dakshina and Existing rows: the romanisations given there and\n\
# Lipik's own letter-rule spelling of each Hindi word. Aksharantar is released\n\
# under CC-BY 4.0 (manually collected data) and CC0 (existing sources); its\n\
# Dakshina rows come from the Google Dakshina dataset\n\
# (https://github.com/google-research-datasets/dakshina), CC BY-SA 4.0. This\n\
# file is shared under CC BY-SA 4.0 (https://creativecommons.org/licenses/by-sa/4.0/).\n\
# English words in the common-word list are left out.\n";
        let body: Vec<String> = words.into_iter().collect();
        std::fs::write(
            concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/src/notetaker/hindi_lexicon.txt"
            ),
            format!("{header}{}\n", body.join("\n")),
        )
        .unwrap();
        println!("LEXICON {} words", body.len());
    }

    /// Private check on a real meeting: LIPIK_TEST_DB=<copy of
    /// notetaker.db> LIPIK_TEST_MEETING=<id> LIPIK_TEST_AUDIO=<its folder>
    /// LIPIK_TEST_TERMS=<dictionary, comma-separated>, and optionally
    /// LIPIK_TEST_OLLAMA_MODEL to write the Devanagari lines with Ollama.
    #[test]
    #[ignore]
    fn meeting_check() {
        use super::super::{store::Store, tidy};
        let var = |k: &str| std::env::var(k).unwrap_or_else(|_| panic!("{k} is not set"));
        let id = var("LIPIK_TEST_MEETING");
        let store = Store::open_read_only(std::path::Path::new(&var("LIPIK_TEST_DB"))).unwrap();
        let meeting = store.meeting(&id).unwrap().expect("no such meeting");
        let mut lines = store.segments(&id).unwrap();
        let terms: Vec<String> = var("LIPIK_TEST_TERMS")
            .split(',')
            .map(|t| t.trim().to_string())
            .collect();
        let mut names = diarize::invitees(&meeting.attendees);
        names.extend(super::super::own_names_in(&meeting));
        let devanagari =
            |lines: &[Segment]| lines.iter().filter(|s| has_devanagari(&s.text)).count();
        println!(
            "CHECK lines {} with Devanagari {}",
            lines.len(),
            devanagari(&lines)
        );

        let dir = std::path::PathBuf::from(var("LIPIK_TEST_AUDIO"));
        let mut gone = Vec::new();
        for (source, offset) in [
            ("mic", meeting.mic_offset_ms.unwrap_or(0)),
            ("system", meeting.system_offset_ms.unwrap_or(0)),
        ] {
            let Ok(reader) = hound::WavReader::open(dir.join(format!("{source}.wav"))) else {
                continue;
            };
            let audio: Vec<f32> = reader
                .into_samples::<i16>()
                .map(|s| s.unwrap_or(0) as f32 / i16::MAX as f32)
                .collect();
            for (n, s) in lines.iter().enumerate().filter(|(_, s)| s.source == source) {
                if !tidy::stock_phrase(&s.text) {
                    continue;
                }
                let at = |ms: i64| (((ms - offset).max(0) * 16) as usize).min(audio.len());
                let vad = crate::audio_toolkit::SileroVad::new(
                    concat!(
                        env!("CARGO_MANIFEST_DIR"),
                        "/resources/models/silero-vad-v6.2-F32.gguf"
                    ),
                    0.3,
                )
                .unwrap();
                let mut track = super::super::audio::SpeechTrack::new(Some(Box::new(vad)));
                track.push(&audio[at(s.start_ms)..at(s.end_ms)]);
                let mut whole = super::super::audio::Piece {
                    start_sample: 0,
                    audio: Vec::new(),
                    speech_samples: 0,
                    peak: 0.0,
                };
                while let Some(p) = track.take(RULES, true) {
                    whole.speech_samples += p.speech_samples;
                    whole.peak = whole.peak.max(p.peak);
                }
                let drop = tidy::heard_nothing(&s.text, &whole, true, &[]);
                println!(
                    "CHECK stock phrase {:?}: speech {} ms, peak {:.2}, dropped {drop}",
                    s.text,
                    whole.speech_samples / 16,
                    whole.peak
                );
                if drop {
                    gone.push(n);
                }
            }
        }
        for n in gone.into_iter().rev() {
            lines.remove(n);
        }

        let before: Vec<String> = lines.iter().map(|s| s.text.clone()).collect();
        let prompt = prompt(&names);
        let model = std::env::var("LIPIK_TEST_OLLAMA_MODEL")
            .ok()
            .map(|m| ai::Model::Ollama(ai::Ollama::new("http://127.0.0.1:11434", &m)));
        romanize_with(
            &mut lines,
            &names,
            Duration::from_secs(if model.is_some() { 900 } else { 0 }),
            |line| {
                let model = model.as_ref().ok_or_else(|| anyhow!("no model"))?;
                let reply = romanize_line(model, &prompt, line);
                match &reply {
                    Ok(r) if !same_numbers(line, r) => {
                        println!(
                            "CHECK numbers changed: {:?} -> {:?}",
                            numbers(line),
                            numbers(r)
                        )
                    }
                    Ok(_) => println!("CHECK model wrote a line"),
                    Err(e) => println!("CHECK model failed: {e}"),
                }
                reply
            },
            |_| true,
        );
        for (b, s) in before.iter().zip(&lines) {
            if has_devanagari(b) {
                println!("CHECK roman: {}", s.text);
            }
        }
        println!("CHECK with Devanagari after {}", devanagari(&lines));
        let vocabulary = tidy::Vocabulary::new(&terms, &names);
        for s in &lines {
            let (text, runs) = tidy::collapse_repeats(&s.text);
            if runs > 0 {
                println!("CHECK repeat: {:?}\nCHECK    -> {:?}", s.text, text);
            }
        }
        let started = Instant::now();
        let done = tidy::tidy(&mut lines, &vocabulary);
        println!("CHECK tidy took {:?}", started.elapsed());
        println!("CHECK repeats collapsed {}", done.repeats);
        let mut fixes: std::collections::BTreeMap<String, usize> = Default::default();
        for (heard, term) in &done.fixed {
            *fixes.entry(format!("{heard} -> {term}")).or_default() += 1;
        }
        println!("CHECK dictionary fixes {}", done.fixed.len());
        for (fix, n) in fixes {
            println!("CHECK   {fix} x{n}");
        }
    }

    #[test]
    fn a_translated_reply_is_caught() {
        let line = "तो हमने कल काफी सारे manual लिखे भी थे और वो findings आई थी";
        assert!(translated(
            line,
            "So we wrote a lot of manuals yesterday and those findings came"
        ));
        assert!(!translated(
            line,
            "To humne kal kaafi saare manual likhe bhi the aur vo findings aayi thi"
        ));
        assert!(!translated("ठीक है", "Okay"));
    }

    #[test]
    fn the_prompt_names_the_people_to_keep() {
        let p = prompt(&names(&["Varun Mehta", "Al"]));
        assert!(p.contains("Varun, Mehta."));
        assert!(!prompt(&[]).contains("names of people"));
    }
}
