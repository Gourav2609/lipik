//! The rules step of clean-up and the checks on the model's reply.
//! `training/cleanup/guards.py` mirrors this file for the clean-up benchmark;
//! both run the examples in `rules_examples.json`.
//!
//! `apply` runs on every dictation clean-up handles: it is the whole clean-up
//! for short dictations, the text the model is given for longer ones, and
//! what is pasted when the model's reply is rejected. `check` decides whether
//! the reply may be pasted. Fillers ("um", "uh") are not handled here: the
//! transcriber's filler step removes them, following the user's switch.

use super::{commands::is_common_word, hindi};
use crate::audio_toolkit::builtin_filler_words;
use once_cell::sync::Lazy;
use regex::{Match, Regex, RegexBuilder};
use std::collections::HashSet;

/// One stray word is tolerated ("a", a fixed mishearing); 2 or more rejects.
const MAX_ADDED_WORDS: usize = 1;
/// 3 or more dropped words without a correction cue rejects.
const MAX_DROPPED_WORDS: usize = 2;
/// A correction ("no", "actually") drops words on purpose, but never more
/// than this share of the transcript's words, nor more than
/// `CUE_MIN_DROPPED` when the transcript is short.
const CUE_MAX_DROPPED_SHARE: f64 = 0.4;
const CUE_MIN_DROPPED: usize = 3;
/// A reply longer than the transcript times this, plus `TOO_LONG_EXTRA`
/// characters, is rejected.
const TOO_LONG_TIMES: f64 = 1.5;
const TOO_LONG_EXTRA: usize = 40;
/// The most said numbers that one written number may join ("nine eight
/// seven ... zero" -> 9876543210).
const MAX_JOINED_NUMBERS: usize = 12;

const UNITS: [&str; 20] = [
    "zero",
    "one",
    "two",
    "three",
    "four",
    "five",
    "six",
    "seven",
    "eight",
    "nine",
    "ten",
    "eleven",
    "twelve",
    "thirteen",
    "fourteen",
    "fifteen",
    "sixteen",
    "seventeen",
    "eighteen",
    "nineteen",
];
/// 20, 30, ... 90.
const TENS: [&str; 8] = [
    "twenty", "thirty", "forty", "fifty", "sixty", "seventy", "eighty", "ninety",
];
/// 1st to 19th.
const ORDINAL_UNITS: [&str; 19] = [
    "first",
    "second",
    "third",
    "fourth",
    "fifth",
    "sixth",
    "seventh",
    "eighth",
    "ninth",
    "tenth",
    "eleventh",
    "twelfth",
    "thirteenth",
    "fourteenth",
    "fifteenth",
    "sixteenth",
    "seventeenth",
    "eighteenth",
    "nineteenth",
];
/// 20th, 30th.
const ORDINAL_TENS: [&str; 2] = ["twentieth", "thirtieth"];

/// Hindi number words count for the added-word check only ("aath baje" ->
/// "8 baje"). "do" (2) is left out: it is far more often the English verb.
const HINDI_NUMBERS: &[(&str, u64)] = &[
    ("ek", 1),
    ("teen", 3),
    ("chaar", 4),
    ("char", 4),
    ("paanch", 5),
    ("panch", 5),
    ("chhe", 6),
    ("chhah", 6),
    ("che", 6),
    ("saat", 7),
    ("aath", 8),
    ("nau", 9),
    ("das", 10),
    ("gyarah", 11),
    ("barah", 12),
    ("pandrah", 15),
    ("bees", 20),
    ("tees", 30),
    ("pachaas", 50),
    ("sau", 100),
];

const SCALES: &[(&str, u64)] = &[
    ("hundred", 100),
    ("thousand", 1_000),
    ("lakh", 100_000),
    ("million", 1_000_000),
];

/// Words the model may add without counting: "p.m." written as "PM".
const FREE_WORDS: &[&str] = &["pm", "am"];

/// Endings a spoken web or email address may have.
const TLDS: &[&str] = &[
    "com", "org", "net", "io", "dev", "ai", "app", "co", "in", "uk", "me", "so", "xyz", "gg",
    "tech",
];
/// Endings that are also English words: an address ending in one needs an
/// "@", a second "." or a "/" ("a red dot in the corner" is no address).
const WORD_TLDS: &[&str] = &[
    "in", "so", "me", "co", "app", "ai", "it", "is", "us", "at", "to", "do", "go", "be",
];

/// Words a good clean-up may remove besides fillers: spoken punctuation and
/// address words, number words (written as digits) and list cue words.
const DROPPABLE: &[&str] = &[
    "like",
    "you",
    "know",
    "comma",
    "period",
    "full",
    "stop",
    "question",
    "mark",
    "exclamation",
    "point",
    "new",
    "line",
    "newline",
    "paragraph",
    "colon",
    "dot",
    "at",
    "underscore",
    "dash",
    "slash",
    "number",
    "bullet",
    "and",
    "hundred",
    "thousand",
    "lakh",
    "dollars",
    "rupees",
    "percent",
    "a",
    "p",
    "m",
];

/// The word before a spoken "at" that makes it a plain "at", not an email's
/// "@" ("the docs are at docs dot ...").
const NOT_A_USER: &[&str] = &[
    "is",
    "are",
    "am",
    "was",
    "were",
    "be",
    "me",
    "us",
    "him",
    "her",
    "them",
    "it",
    "you",
    "look",
    "here",
    "there",
    "go",
    "meet",
    "available",
    "live",
    "find",
    "docs",
    "page",
    "site",
    "online",
];

/// After these, spoken punctuation and address words are ordinary words
/// ("a new line of shoes", "a dot in the middle").
const DETERMINERS: &[&str] = &[
    "a", "an", "the", "this", "that", "my", "our", "your", "his", "her", "its", "their", "some",
    "any",
];

/// "colon" before one of these is the organ.
const COLON_NOUNS: &[&str] = &[
    "cancer",
    "surgery",
    "screening",
    "polyp",
    "polyps",
    "cleanse",
    "health",
    "exam",
    "biopsy",
];

/// "What we do is ...", "When you have time, ...": a statement, not a question.
const SUBJECT_PRONOUNS: &[&str] = &["i", "you", "we", "they", "he", "she", "it"];

/// Scripts a reply may not bring in when the transcript has none of them.
const SCRIPTS: &[&str] = &[
    "Latin",
    "Devanagari",
    "Bengali",
    "Gurmukhi",
    "Gujarati",
    "Oriya",
    "Tamil",
    "Telugu",
    "Kannada",
    "Malayalam",
    "Arabic",
    "Hebrew",
    "Cyrillic",
    "Greek",
    "Han",
    "Hiragana",
    "Katakana",
    "Hangul",
    "Thai",
];

fn lookup(table: &[(&str, u64)], word: &str) -> Option<u64> {
    table.iter().find(|(w, _)| *w == word).map(|(_, v)| *v)
}

fn index_of(table: &[&str], word: &str) -> Option<u64> {
    table.iter().position(|w| *w == word).map(|i| i as u64)
}

fn cardinal(word: &str) -> Option<u64> {
    index_of(&UNITS, word).or_else(|| index_of(&TENS, word).map(|i| (i + 2) * 10))
}

fn ordinal(word: &str) -> Option<u64> {
    index_of(&ORDINAL_UNITS, word)
        .map(|i| i + 1)
        .or_else(|| index_of(&ORDINAL_TENS, word).map(|i| (i + 2) * 10))
}

/// Any number word; "oh" is the zero of "nine oh five".
pub(super) fn number_word(word: &str) -> Option<u64> {
    cardinal(word)
        .or_else(|| (word == "oh").then_some(0))
        .or_else(|| ordinal(word))
        .or_else(|| lookup(HINDI_NUMBERS, word))
}

fn is_word_char(c: char) -> bool {
    c.is_alphanumeric() || c == '_'
}

fn is_digits(s: &str) -> bool {
    !s.is_empty() && s.bytes().all(|b| b.is_ascii_digit())
}

fn is_filler(word: &str) -> bool {
    builtin_filler_words("en").any(|f| f == word)
}

/// A digit string without its leading zeros ("007" -> "7", "000" -> "0").
fn no_leading_zeros(s: &str) -> &str {
    let t = s.trim_start_matches('0');
    if t.is_empty() {
        "0"
    } else {
        t
    }
}

static WORD: Lazy<Regex> = Lazy::new(|| Regex::new(r"[\p{L}\p{M}\p{N}']+").unwrap());

/// Lowercase word tokens: runs of letters (with their combining marks, so
/// Devanagari words stay whole), digits and apostrophes. Emails, URLs,
/// "Next.js" and "3:30" come apart into pieces; "1,500" stays one number.
fn words(text: &str) -> Vec<String> {
    let chars: Vec<char> = text.to_lowercase().replace('’', "'").chars().collect();
    let thousands_follow = |j: usize| {
        j + 3 <= chars.len()
            && chars[j..j + 3].iter().all(char::is_ascii_digit)
            && chars.get(j + 3).is_none_or(|&c| !is_word_char(c))
    };
    let joined: String = chars
        .iter()
        .enumerate()
        .filter(|&(i, &c)| {
            !(c == ',' && i > 0 && chars[i - 1].is_ascii_digit() && thousands_follow(i + 1))
        })
        .map(|(_, &c)| c)
        .collect();
    WORD.find_iter(&joined)
        .map(|m| m.as_str().trim_matches('\''))
        .filter(|w| !w.is_empty())
        .map(String::from)
        .collect()
}

/// Every number the speaker said, as digit strings, in order. Digits count
/// as themselves; number words combine ("two thousand four hundred and
/// fifty" -> 2450, "fifteen hundred" -> 1500, "twenty twenty seven" -> 20, 27).
fn spoken_numbers(tokens: &[String]) -> Vec<String> {
    let mut out = Vec::new();
    let (mut cur, mut total, mut active) = (0u64, 0u64, false);
    let flush = |out: &mut Vec<String>, cur: &mut u64, total: &mut u64, active: &mut bool| {
        if *active {
            out.push((*total + *cur).to_string());
        }
        (*cur, *total, *active) = (0, 0, false);
    };
    for t in tokens {
        if is_digits(t) {
            flush(&mut out, &mut cur, &mut total, &mut active);
            out.push(no_leading_zeros(t).to_string());
        } else if let Some(v) = number_word(t) {
            if !active {
                (cur, active) = (v, true);
            } else if cur >= 100 && cur % 100 == 0 {
                cur += v;
            } else if (20..100).contains(&cur) && cur % 10 == 0 && v < 10 {
                cur += v;
            } else if cur == 0 && total != 0 {
                cur = v;
            } else {
                flush(&mut out, &mut cur, &mut total, &mut active);
                (cur, active) = (v, true);
            }
        } else if let (Some(scale), true) = (lookup(SCALES, t), active) {
            let base = if cur == 0 { 1 } else { cur };
            if scale == 100 {
                cur = base.saturating_mul(100);
            } else {
                total = total.saturating_add(base.saturating_mul(scale));
                cur = 0;
            }
        } else if t == "and" && active {
            continue;
        } else {
            flush(&mut out, &mut cur, &mut total, &mut active);
        }
    }
    flush(&mut out, &mut cur, &mut total, &mut active);
    out
}

/// A digit token in the reply is fine when it is one of the numbers said, or
/// several said numbers written together (phone numbers, "twenty twenty
/// seven" -> 2027).
fn number_ok(token: &str, said: &[String]) -> bool {
    if token.trim_start_matches('0').is_empty() && said.iter().any(|s| s == "0") {
        return true;
    }
    let t = no_leading_zeros(token);
    if said.iter().any(|s| s == t) {
        return true;
    }
    for i in 0..said.len() {
        let mut joined = String::new();
        for s in &said[i..said.len().min(i + MAX_JOINED_NUMBERS)] {
            joined.push_str(s);
            if joined == token || joined == t {
                return true;
            }
            if joined.len() >= token.len() {
                break;
            }
        }
    }
    false
}

static DIGIT_GROUP: Lazy<Regex> = Lazy::new(|| Regex::new(r"[0-9]+(?:[:.,-][0-9]+)+").unwrap());
static DIGITS: Lazy<Regex> = Lazy::new(|| Regex::new(r"[0-9]+").unwrap());

/// Words in the reply that the speaker never said. Allowed: any word of the
/// transcript, words of a vocabulary entry, digits for numbers that were said,
/// a word joining 2-4 neighbouring transcript words ("A P I" -> "API"), and
/// "pm"/"am".
fn added_words(transcript: &str, reply: &str, vocab: &[String]) -> Vec<String> {
    let t_words = words(transcript);
    let mut allowed: HashSet<String> = t_words.iter().cloned().collect();
    allowed.extend(FREE_WORDS.iter().map(|w| w.to_string()));
    for entry in vocab {
        allowed.extend(words(entry));
    }
    for n in 2..=4 {
        for window in t_words.windows(n) {
            allowed.insert(window.concat());
        }
    }
    let said = spoken_numbers(&t_words);
    // "8:15", "2.4.1", "9876-543210": when the digits written together are a
    // number that was said ("815"), every piece is fine.
    for m in DIGIT_GROUP.find_iter(reply) {
        let joined: String = m.as_str().chars().filter(char::is_ascii_digit).collect();
        if number_ok(&joined, &said) {
            allowed.extend(DIGITS.find_iter(m.as_str()).map(|d| d.as_str().to_string()));
        }
    }
    let mut added: Vec<String> = Vec::new();
    for w in words(reply) {
        if allowed.contains(&w) || (is_digits(&w) && number_ok(&w, &said)) || added.contains(&w) {
            continue;
        }
        added.push(w);
    }
    added
}

/// Different transcript words missing from the reply, ignoring fillers,
/// droppable words, number words and words that were joined ("A P I" ->
/// "API").
fn dropped_words(transcript: &str, reply: &str) -> Vec<String> {
    let reply_words = words(reply);
    let joined = reply_words.concat();
    let reply_words: HashSet<String> = reply_words.into_iter().collect();
    let mut missing: Vec<String> = Vec::new();
    for w in words(transcript) {
        if reply_words.contains(&w)
            || DROPPABLE.contains(&w.as_str())
            || is_filler(&w)
            || number_word(&w).is_some()
            || is_digits(&w)
            || missing.contains(&w)
        {
            continue;
        }
        if w.chars().count() <= 2 && joined.contains(&w) {
            continue;
        }
        missing.push(w);
    }
    missing
}

pub(super) fn hindi_words(text: &str) -> HashSet<String> {
    words(text)
        .into_iter()
        .filter(|w| {
            hindi::WORDS.contains(&w.as_str()) && !hindi::ALSO_ENGLISH.contains(&w.as_str())
        })
        .collect()
}

/// True when the transcript looks like Hinglish (2+ different Hindi words)
/// and the reply kept fewer than half of them: the model translated it.
/// Different words, not occurrences, so a correction that drops "nahi nahi"
/// is not counted.
fn hinglish_lost(transcript: &str, reply: &str) -> bool {
    let before = hindi_words(transcript);
    let kept = before.intersection(&hindi_words(reply)).count();
    before.len() >= 2 && kept * 2 < before.len()
}

static SCRIPT_PATTERNS: Lazy<Vec<Regex>> = Lazy::new(|| {
    SCRIPTS
        .iter()
        .map(|s| Regex::new(&format!(r"\p{{{s}}}")).unwrap())
        .collect()
});

/// True when the reply is written partly in a script the transcript does
/// not use ("ok" said, "ठीक है" written).
fn new_script(transcript: &str, reply: &str) -> bool {
    SCRIPT_PATTERNS
        .iter()
        .any(|re| re.is_match(reply) && !re.is_match(transcript))
}

fn too_long(reply: &str, transcript: &str) -> bool {
    reply.chars().count() as f64
        > transcript.chars().count() as f64 * TOO_LONG_TIMES + TOO_LONG_EXTRA as f64
}

/// Words a speaker uses to take something back. Shared with the gate.
pub(super) const CORRECTION_CUE_WORDS: &str =
    "no|nope|sorry|actually|wait|i mean|i meant|meant|scratch that|nahi|rather|make that";
static CORRECTION_CUES: Lazy<Regex> =
    Lazy::new(|| Regex::new(&format!(r"(?i)\b({CORRECTION_CUE_WORDS})\b")).unwrap());

/// Why a model reply was rejected.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Why {
    /// Much longer than the transcript.
    TooLong,
    /// Written partly in a script the speaker did not use.
    NewScript,
    /// The model translated Hinglish.
    HinglishLost,
    /// The model added this many words the speaker never said.
    Added(usize),
    /// The model dropped this many of the speaker's words.
    Dropped(usize),
}

/// Whether the model's reply may be pasted. It is compared with the
/// transcript the model was asked to clean (after dictionary replacements
/// and snippets, before the rules step).
pub fn check(transcript: &str, reply: &str, vocab: &[String]) -> Result<(), Why> {
    check_part(transcript, reply, vocab, transcript)
}

/// `check` for one part of a dictation: after a correction cue the part may
/// drop up to 40% of the words of the `whole` dictation, as a reply for the
/// whole text could ("Add Priya. Scratch that. Add Kabir." drops most of
/// its own words).
pub fn check_part(transcript: &str, reply: &str, vocab: &[String], whole: &str) -> Result<(), Why> {
    if too_long(reply, transcript) {
        return Err(Why::TooLong);
    }
    if new_script(transcript, reply) {
        return Err(Why::NewScript);
    }
    if hinglish_lost(transcript, reply) {
        return Err(Why::HinglishLost);
    }
    let added = added_words(transcript, reply, vocab).len();
    if added > MAX_ADDED_WORDS {
        return Err(Why::Added(added));
    }
    let dropped = dropped_words(transcript, reply).len();
    let limit = if CORRECTION_CUES.is_match(transcript) {
        (words(whole).len() as f64 * CUE_MAX_DROPPED_SHARE).max(CUE_MIN_DROPPED as f64)
    } else {
        MAX_DROPPED_WORDS as f64
    };
    if dropped as f64 > limit {
        return Err(Why::Dropped(dropped));
    }
    Ok(())
}

/// Like `Regex::replace_all`, for patterns that need a look-behind or
/// look-ahead, which the regex crate lacks: a match counts only when `ok`
/// accepts it, and otherwise the search goes on one character later, as
/// Python's `re` does.
fn replace_checked(
    re: &Regex,
    text: &str,
    ok: impl Fn(Match) -> bool,
    rep: impl Fn(Match) -> String,
) -> String {
    let next = |i: usize| i + text[i..].chars().next().map_or(1, char::len_utf8);
    let mut out = String::with_capacity(text.len());
    let (mut copied, mut pos) = (0, 0);
    while pos <= text.len() {
        let Some(m) = re.find_at(text, pos) else {
            break;
        };
        if ok(m) {
            out.push_str(&text[copied..m.start()]);
            out.push_str(&rep(m));
            copied = m.end();
            pos = if m.end() > m.start() {
                m.end()
            } else {
                next(m.end())
            };
        } else {
            pos = next(m.start());
        }
    }
    out.push_str(&text[copied..]);
    out
}

fn char_before(text: &str, i: usize) -> Option<char> {
    text[..i].chars().next_back()
}

/// The lowercase word right before byte `i`, skipping spaces.
fn word_before(text: &str, i: usize) -> String {
    let before = text[..i].trim_end();
    let start = before
        .char_indices()
        .rev()
        .find(|&(_, c)| !is_word_char(c))
        .map_or(0, |(j, c)| j + c.len_utf8());
    before[start..].to_lowercase()
}

/// The lowercase word right after byte `i`, skipping spaces.
fn word_after(text: &str, i: usize) -> String {
    text[i..]
        .trim_start()
        .chars()
        .take_while(|&c| is_word_char(c))
        .collect::<String>()
        .to_lowercase()
}

fn after_determiner(text: &str, i: usize) -> bool {
    DETERMINERS.contains(&word_before(text, i).as_str())
}

static SPOKEN_MARKS: Lazy<Vec<(Regex, &'static str)>> = Lazy::new(|| {
    [
        ("new paragraph", "\n\n"),
        ("new line|newline", "\n"),
        ("question mark", "?"),
        ("exclamation (?:mark|point)", "!"),
        ("full stop", "."),
        ("comma", ","),
        ("colon", ":"),
    ]
    .into_iter()
    .map(|(said, mark)| {
        let re = Regex::new(&format!(r"(?i)[\s,.;:!?]*\b(?:{said})\b[,.;:!?]*")).unwrap();
        (re, mark)
    })
    .collect()
});
static SPOKEN_PERIOD: Lazy<Regex> = Lazy::new(|| Regex::new(r"(?i)[\s,]*\bperiod\b\.?").unwrap());

/// Spoken punctuation becomes the mark, replacing whatever punctuation the
/// transcriber put around the spoken word, except after "a", "the", "my" and
/// the like ("a new line of shoes"). "period" counts only at the very end or
/// before a line break, so "the trial period ends" keeps its word.
fn spoken_punctuation(text: &str) -> String {
    let mut text = text.to_string();
    for (re, mark) in SPOKEN_MARKS.iter() {
        let organ =
            |m: Match| *mark == ":" && COLON_NOUNS.contains(&word_after(&text, m.end()).as_str());
        let replaced = replace_checked(
            re,
            &text,
            |m| !after_determiner(&text, m.start()) && !organ(m),
            |_| mark.to_string(),
        );
        text = replaced;
    }
    replace_checked(
        &SPOKEN_PERIOD,
        &text,
        |m| {
            let rest = &text[m.end()..];
            let spaces: String = rest.chars().take_while(|c| c.is_whitespace()).collect();
            !after_determiner(&text, m.start())
                && (spaces.contains('\n') || spaces.len() == rest.len())
        },
        |_| ".".to_string(),
    )
}

static ADDRESS: Lazy<Regex> = Lazy::new(|| {
    Regex::new(
        r"(?i)[a-z0-9][\w.-]*(?:\s+(?:dot|underscore|dash|at|slash)\s+[a-z0-9][\w.-]*|/[\w-]+)+",
    )
    .unwrap()
});
static ADDRESS_SEPARATOR: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"(?i)\s+(dot|underscore|dash|at|slash)\s+").unwrap());

/// "rivaan underscore m at quavia dot com" -> "rivaan_m@quavia.com", or
/// `None` when the span is not an address: no known ending, an ending that is
/// an English word with nothing else to show an address ("a red dot in"), a
/// first word like "a" or "the", two "@", or "look at x dot com".
fn join_address(span: &str) -> Option<String> {
    let mut pieces: Vec<&str> = Vec::new();
    let mut last = 0;
    for caps in ADDRESS_SEPARATOR.captures_iter(span) {
        let m = caps.get(0).expect("group 0");
        pieces.push(&span[last..m.start()]);
        pieces.push(caps.get(1).expect("group 1").as_str());
        last = m.end();
    }
    pieces.push(&span[last..]);
    let first = pieces[0].to_lowercase();
    if DETERMINERS.contains(&first.as_str()) {
        return None;
    }
    if pieces.len() >= 3
        && pieces[1].eq_ignore_ascii_case("at")
        && NOT_A_USER.contains(&first.as_str())
    {
        let rest = join_address(&pieces[2..].join(" "))?;
        return Some(format!("{} at {rest}", pieces[0]));
    }
    let mut out = pieces[0].to_string();
    for pair in pieces[1..].chunks(2) {
        out.push(match pair[0].to_lowercase().as_str() {
            "dot" => '.',
            "underscore" => '_',
            "dash" => '-',
            "at" => '@',
            _ => '/',
        });
        out.push_str(pair[1]);
    }
    let out = out.trim_end_matches('.').to_lowercase();
    let trail = &span[span.trim_end_matches('.').len()..];
    let (email, path) = (out.contains('@'), out.contains('/'));
    if out.matches('@').count() > 1 || (email && path) {
        return None;
    }
    let domain = out.rsplit('@').next().unwrap_or(&out);
    let host = domain.split('/').next().unwrap_or(domain);
    let (name, tld) = host.rsplit_once('.')?;
    if !TLDS.contains(&tld) {
        return None;
    }
    if WORD_TLDS.contains(&tld) && !email && !path && !name.contains('.') {
        return None;
    }
    Some(format!("{out}{trail}"))
}

/// Spoken emails and web addresses become real ones, in lowercase.
fn emails_and_urls(text: &str) -> String {
    replace_checked(
        &ADDRESS,
        text,
        |m| {
            char_before(text, m.start())
                .is_none_or(|c| !(is_word_char(c) || matches!(c, '.' | '@' | '/')))
                && !after_determiner(text, m.start())
        },
        |m| join_address(m.as_str()).unwrap_or_else(|| m.as_str().to_string()),
    )
}

static NUMBER_CORRECTION: Lazy<Regex> = Lazy::new(|| {
    let mut names: Vec<&str> = UNITS.iter().chain(&TENS).copied().collect();
    names.sort_by_key(|w| std::cmp::Reverse(w.len()));
    let num = format!(r"(?:\d+(?:[.:]\d+)?|{})", names.join("|"));
    Regex::new(&format!(
        r"(?i)\b{num}[,.]?\s+no[,.]?\s+(?:sorry[,.]?\s+|wait[,.]?\s+)?{num}\b"
    ))
    .unwrap()
});

/// "four, no, five" / "4 no 5" -> "5": a number, "no", another number is a
/// correction. The number kept is written in digits. "no one" is not a
/// correction ("I asked two, no one replied").
fn number_corrections(text: &str) -> String {
    fn kept(m: Match<'_>) -> &str {
        m.as_str()
            .rsplit(char::is_whitespace)
            .next()
            .unwrap_or_default()
    }
    replace_checked(
        &NUMBER_CORRECTION,
        text,
        |m| !kept(m).eq_ignore_ascii_case("one"),
        |m| {
            let kept = kept(m);
            cardinal(&kept.to_lowercase()).map_or_else(|| kept.to_string(), |v| v.to_string())
        },
    )
}

static CLOCK_TIME: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"(?i)\b(\d{1,2})[.:](\d{2})\s*([ap])\.?\s?m\b\.?").unwrap());
static HOUR_TIME: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"(?i)\b(\d{1,2})\s*([ap])\.\s?m\.").unwrap());

/// "3.30 p.m." / "3:30 pm" -> "3:30 PM", "5 p.m." -> "5 PM". A bare "1.10"
/// stays: it may be a price or a version.
fn times(text: &str) -> String {
    let t = CLOCK_TIME.replace_all(text, |c: &regex::Captures| {
        format!("{}:{} {}M", &c[1], &c[2], c[3].to_uppercase())
    });
    HOUR_TIME
        .replace_all(&t, |c: &regex::Captures| {
            format!("{} {}M", &c[1], c[2].to_uppercase())
        })
        .into_owned()
}

/// A match of a vocabulary entry, ignoring case, gets the entry's spelling
/// (not inside an email or web address). An entry made of common words
/// ("Linear") leaves an all-lowercase match alone: "a linear model".
fn vocab_case(text: &str, vocab: &[String]) -> String {
    let mut text = text.to_string();
    for entry in vocab.iter().filter(|e| !e.is_empty()) {
        // Most entries are not in the text; skip building their regex.
        if entry.is_ascii()
            && !text
                .to_ascii_lowercase()
                .contains(&entry.to_ascii_lowercase())
        {
            continue;
        }
        let Ok(re) = RegexBuilder::new(&regex::escape(entry))
            .case_insensitive(true)
            .build()
        else {
            continue;
        };
        let ordinary = entry.split_whitespace().all(is_common_word);
        let replaced = replace_checked(
            &re,
            &text,
            |m| {
                let free_before = char_before(&text, m.start())
                    .is_none_or(|c| !(is_word_char(c) || matches!(c, '.' | '@' | '/')));
                let mut rest = text[m.end()..].chars();
                let free_after = match rest.next() {
                    None => true,
                    Some(c) if is_word_char(c) || matches!(c, '@' | '/') => false,
                    Some('.') => !rest.next().is_some_and(is_word_char),
                    Some(_) => true,
                };
                let lowercase = !m.as_str().chars().any(char::is_uppercase);
                free_before && free_after && !(ordinary && lowercase)
            },
            |_| entry.clone(),
        );
        text = replaced;
    }
    text
}

static SENTENCE_GOES_ON: Lazy<Regex> = Lazy::new(|| Regex::new(r"[.?!]\s+\S").unwrap());
pub(super) static OPENER: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r"(?i)^((hey|hi|hello|okay|ok|so)\b[,]?\s+([A-Z][a-z]+,\s+)?)+").unwrap()
});
const QUESTION_AUX: &str = "(is|are|was|were|am|do|does|did|can|could|will|would|should|shall|have|has|time|much|many|long|often|far|about)";
static QUESTION_START: Lazy<Regex> = Lazy::new(|| {
    Regex::new(&format!(
        r"(?i)^((what's|where's|how's|who's|when's|why's|is|are|isn't|aren't)\b|(what|when|where|who|why|how|which)\s+{QUESTION_AUX}\b|(can|could|would|will|should|shall|do|does|did|have|has)(n't)?\s+(i|you|we|they|he|she|someone|anyone)\b)"
    ))
    .unwrap()
});
/// A question word, one more word, then a helper verb: "which one should".
static QUESTION_WORD_MIDDLE: Lazy<Regex> = Lazy::new(|| {
    Regex::new(&format!(
        r"(?i)^(what|when|where|who|why|how|which)\s+(\S+)\s+{QUESTION_AUX}\b"
    ))
    .unwrap()
});

pub(super) fn starts_like_question(text: &str) -> bool {
    QUESTION_START.is_match(text)
        || QUESTION_WORD_MIDDLE
            .captures(text)
            .is_some_and(|c| !SUBJECT_PRONOUNS.contains(&c[2].to_lowercase().as_str()))
}

/// A one-sentence text that starts like a question ("Can you...", "Hey, is
/// the...") ends with "?" instead of "." or nothing.
pub fn question_end(text: &str) -> String {
    if text.contains('\n')
        || SENTENCE_GOES_ON.is_match(text)
        || text.ends_with(['?', '…'])
        || text.ends_with("..")
        || !starts_like_question(&OPENER.replace(text, ""))
    {
        return text.to_string();
    }
    let kept = text.strip_suffix(['.', '!']).unwrap_or(text);
    format!("{kept}?")
}

static SPACE_BEFORE_MARK: Lazy<Regex> = Lazy::new(|| Regex::new(r"[ \t]+([,.?!:])").unwrap());
static MARK_BEFORE_LETTER: Lazy<Regex> = Lazy::new(|| Regex::new(r"[,?!][A-Za-z]").unwrap());
static COMMA_BEFORE_END: Lazy<Regex> = Lazy::new(|| Regex::new(r",([.?!])").unwrap());
static SPACES_AROUND_BREAK: Lazy<Regex> = Lazy::new(|| Regex::new(r"[ \t]*\n[ \t]*").unwrap());
static DOUBLE_SPACE: Lazy<Regex> = Lazy::new(|| Regex::new(r"[ \t]{2,}").unwrap());
static SENTENCE_START: Lazy<Regex> = Lazy::new(|| Regex::new(r"(?:^|\n|[.?!]\s)[a-z]").unwrap());
static WEB_ADDRESS_PART: Lazy<Regex> = Lazy::new(|| Regex::new(r"[/@]|\.[A-Za-z]{2,}").unwrap());

/// One space after ",", "?" and "!" when a letter follows, but not inside a
/// web address ("search?q=shoes").
fn space_after_marks(text: &str) -> String {
    replace_checked(
        &MARK_BEFORE_LETTER,
        text,
        |m| {
            let token = text[..m.start()]
                .rsplit(char::is_whitespace)
                .next()
                .unwrap_or_default();
            !WEB_ADDRESS_PART.is_match(token)
        },
        |m| {
            let (mark, letter) = m.as_str().split_at(1);
            format!("{mark} {letter}")
        },
    )
}

/// "?." -> "?": a run of sentence marks keeps its first one; "..." stays.
fn collapse_marks(text: &str) -> String {
    let is_mark = |c: char| matches!(c, '.' | '?' | '!');
    let chars: Vec<char> = text.chars().collect();
    let mut out = String::with_capacity(text.len());
    let mut i = 0;
    while i < chars.len() {
        if !is_mark(chars[i]) {
            out.push(chars[i]);
            i += 1;
            continue;
        }
        let end = i + chars[i..].iter().take_while(|&&c| is_mark(c)).count();
        if end - i >= 3 && chars[i..end].iter().all(|&c| c == '.') {
            out.push_str("...");
        } else {
            out.push(chars[i]);
        }
        i = end;
    }
    out
}

/// A capital at the start, after a line break and after ". ", "? " or "! ",
/// but not after "... ".
fn capitalize_sentences(text: &str) -> String {
    replace_checked(
        &SENTENCE_START,
        text,
        |m| !(m.as_str().starts_with('.') && char_before(text, m.start()) == Some('.')),
        |m| {
            let (head, letter) = m.as_str().split_at(m.len() - 1);
            format!("{head}{}", letter.to_uppercase())
        },
    )
}

/// An address or web address that ended up first keeps its lowercase.
fn lowercase_leading_address(text: &str) -> String {
    let mut chars = text.chars();
    let Some(first) = chars.next().filter(char::is_ascii_uppercase) else {
        return text.to_string();
    };
    let rest = chars.as_str();
    let after = rest.trim_start_matches(|c: char| is_word_char(c) || c == '.');
    if after.starts_with(['@', '/']) {
        format!("{}{rest}", first.to_ascii_lowercase())
    } else {
        text.to_string()
    }
}

/// The deterministic clean-up: spoken punctuation, "four, no, five", spoken
/// emails and web addresses, times, vocabulary casing, spacing, capitals and
/// a question mark on one-sentence questions.
pub fn apply(text: &str, vocab: &[String]) -> String {
    let t = spoken_punctuation(text.trim());
    let t = number_corrections(&t);
    let t = emails_and_urls(&t);
    let t = times(&t);
    let t = vocab_case(&t, vocab);
    let t = SPACE_BEFORE_MARK.replace_all(&t, "$1");
    let t = space_after_marks(&t);
    let t = collapse_marks(&t);
    let t = COMMA_BEFORE_END.replace_all(&t, "$1");
    let t = SPACES_AROUND_BREAK.replace_all(&t, "\n");
    let t = DOUBLE_SPACE.replace_all(&t, " ");
    let t = t.trim_matches(|c: char| c.is_whitespace() || c == ',');
    lowercase_leading_address(&question_end(&capitalize_sentences(t)))
}

#[cfg(test)]
pub(super) mod tests {
    use super::*;
    use crate::audio_toolkit::{
        normalize_transcription_output, remove_filler_words, OutputLanguageEvidence,
    };
    use serde_json::Value;

    const EXAMPLES: &str = include_str!("rules_examples.json");

    pub(in crate::textflow) fn examples(section: &str) -> Vec<Vec<Value>> {
        let all: Value = serde_json::from_str(EXAMPLES).unwrap();
        serde_json::from_value(all[section].clone()).unwrap()
    }

    pub(in crate::textflow) fn vocab() -> Vec<String> {
        let all: Value = serde_json::from_str(EXAMPLES).unwrap();
        serde_json::from_value(all["vocab"].clone()).unwrap()
    }

    fn text(v: &Value) -> &str {
        v.as_str().unwrap()
    }

    fn list(v: &Value) -> Vec<String> {
        serde_json::from_value(v.clone()).unwrap()
    }

    #[test]
    fn words_examples() {
        for e in examples("words") {
            assert_eq!(words(text(&e[0])), list(&e[1]), "{:?}", e[0]);
        }
    }

    #[test]
    fn added_words_examples() {
        for e in examples("added") {
            let got = added_words(text(&e[0]), text(&e[1]), &vocab());
            assert_eq!(got, list(&e[2]), "{:?}", e[0]);
        }
    }

    #[test]
    fn hinglish_examples() {
        for e in examples("hinglish") {
            let got = hinglish_lost(text(&e[0]), text(&e[1]));
            assert_eq!(got, e[2].as_bool().unwrap(), "{:?}", e[1]);
        }
    }

    #[test]
    fn dropped_words_examples() {
        for e in examples("dropped") {
            assert_eq!(
                dropped_words(text(&e[0]), text(&e[1])),
                list(&e[2]),
                "{:?}",
                e[0]
            );
        }
    }

    #[test]
    fn check_examples() {
        for e in examples("check") {
            let got = match check(text(&e[0]), text(&e[1]), &vocab()) {
                Ok(()) => "ok",
                Err(Why::TooLong) => "TooLong",
                Err(Why::NewScript) => "NewScript",
                Err(Why::HinglishLost) => "HinglishLost",
                Err(Why::Added(_)) => "Added",
                Err(Why::Dropped(_)) => "Dropped",
            };
            assert_eq!(got, text(&e[2]), "{:?} -> {:?}", e[0], e[1]);
        }
    }

    #[test]
    fn check_part_examples() {
        for e in examples("check_part") {
            let got = check_part(text(&e[0]), text(&e[1]), &vocab(), text(&e[2]));
            let got = match got {
                Ok(()) => "ok",
                Err(Why::Dropped(_)) => "Dropped",
                Err(why) => panic!("{why:?}"),
            };
            assert_eq!(got, text(&e[3]), "{:?} -> {:?}", e[0], e[1]);
        }
    }

    #[test]
    fn apply_examples() {
        for e in examples("apply") {
            assert_eq!(apply(text(&e[0]), &vocab()), text(&e[1]), "{:?}", e[0]);
        }
    }

    /// The transcriber's English filler step, then the rules.
    #[test]
    fn fillers_then_apply_examples() {
        let english = OutputLanguageEvidence::UserSelected("en".to_string());
        for e in examples("fillers") {
            let without = remove_filler_words(text(&e[0]), &english, &None, true);
            let got = apply(&normalize_transcription_output(&without), &vocab());
            assert_eq!(got, text(&e[1]), "{:?}", e[0]);
        }
    }

    #[test]
    fn spoken_numbers_combine_words() {
        let said = |s: &str| spoken_numbers(&words(s));
        assert_eq!(said("two thousand four hundred and fifty"), ["2450"]);
        assert_eq!(said("fifteen hundred"), ["1500"]);
        assert_eq!(said("eight fifteen"), ["8", "15"]);
        assert_eq!(said("twenty twenty seven"), ["20", "27"]);
        assert_eq!(said("call 07 and forty five"), ["7", "45"]);
        assert_eq!(said("the twentieth and thirty first"), ["20", "31"]);
    }
}
