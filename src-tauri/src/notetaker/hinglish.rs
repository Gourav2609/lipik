//! Hinglish meetings. Pieces go through Qwen3-ASR, which keeps Hindi words
//! (in Devanagari) and names that the English models turn into English
//! look-alikes. The clean-up model then writes each line in Roman letters.
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

/// Qwen3-ASR models, best first.
pub const MODELS: &[&str] = &["Qwen3-ASR-1.7B", "Qwen3-ASR-0.6B"];
/// The one fetched when a meeting sounds Hinglish.
pub const DOWNLOAD: &str = MODELS[0];

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
pub fn downloaded_model(app: &AppHandle) -> Option<&'static str> {
    let models = app.try_state::<Arc<ModelManager>>()?;
    MODELS.iter().copied().find(|id| {
        models
            .get_model_info(id)
            .is_some_and(|m| m.is_downloaded && !m.is_downloading)
    })
}

/// Qwen3-ASR loaded beside dictation, or `None` when it isn't downloaded
/// or doesn't load.
pub fn engine(app: &AppHandle) -> Option<SideEngine> {
    let model = downloaded_model(app)?;
    let tm = super::transcribe::transcription_manager(app).ok()?;
    match tm.side_batch_engine(model, LANGUAGE) {
        Ok(engine) => Some(engine),
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
    let chars: Vec<char> = word.chars().collect();
    let mut units: Vec<(String, Ends)> = Vec::new();
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        let nukta = chars.get(i + 1) == Some(&'\u{093C}');
        let base = match (c, nukta) {
            ('ज', true) => Some("z"),
            ('फ', true) => Some("f"),
            ('ड', true) => Some("r"),
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
                    text.push_str(vowel(m).unwrap_or_default());
                    i += 1;
                    Ends::Vowel
                }
                _ => Ends::Inherent,
            };
            units.push((text, ends));
        } else if let Some(v) = vowel(c) {
            units.push((v.to_string(), Ends::Vowel));
        } else if matches!(c, 'ं' | 'ँ' | 'ः') {
            let sound = if c == 'ः' { 'h' } else { 'n' };
            match units.last_mut() {
                Some((text, Ends::Inherent)) => {
                    text.push('a');
                    text.push(sound);
                    units.last_mut().unwrap().1 = Ends::Vowel;
                }
                Some((text, _)) => text.push(sound),
                None => {}
            }
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

/// How long writing a meeting in Roman letters may take: a tenth of the
/// meeting, at least 2 minutes and at most 10.
pub fn romanize_time(meeting_ms: i64) -> Duration {
    Duration::from_millis((meeting_ms.max(0) / 10) as u64)
        .clamp(Duration::from_secs(120), Duration::from_secs(600))
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

/// Every line with Devanagari through `ask`, names kept. A line `ask` fails
/// on keeps its Devanagari with the names in Roman letters, and so do all
/// lines left once `ask` failed `MAX_FAILS_IN_ROW` times in a row, `time`
/// ran out, or `keep_going` (given the share of lines done) said stop.
pub fn romanize_with(
    segments: &mut [Segment],
    names: &[String],
    time: Duration,
    mut ask: impl FnMut(&str) -> Result<String>,
    mut keep_going: impl FnMut(f32) -> bool,
) {
    let started = Instant::now();
    let todo = segments.iter().filter(|s| has_devanagari(&s.text)).count();
    let (mut done, mut fails_in_row, mut failed) = (0usize, 0usize, 0usize);
    let mut trying = true;
    for s in segments.iter_mut().filter(|s| has_devanagari(&s.text)) {
        let line = protect_names(&s.text, names);
        trying = trying
            && fails_in_row < MAX_FAILS_IN_ROW
            && started.elapsed() < time
            && keep_going(done as f32 / todo.max(1) as f32);
        s.text = if trying {
            match ask(&line) {
                Ok(roman) => {
                    fails_in_row = 0;
                    protect_names(&roman, names)
                }
                Err(e) => {
                    fails_in_row += 1;
                    failed += 1;
                    log::debug!("Hinglish line kept in Devanagari: {e}");
                    line
                }
            }
        } else {
            line
        };
        done += 1;
    }
    if failed > 0 || !trying {
        log::warn!(
            "Notetaker kept Hinglish lines in Devanagari: {failed} failed, stopped early: {}",
            !trying
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
        assert!(lines.iter().all(|s| s.text == "ठीक है Varun"));

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
        assert_eq!(romanize_time(30 * 60_000), Duration::from_secs(180));
        assert_eq!(romanize_time(60_000), Duration::from_secs(120));
        assert_eq!(romanize_time(4 * 3600_000), Duration::from_secs(600));
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
