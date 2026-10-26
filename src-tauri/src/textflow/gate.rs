//! Which parts of a dictation still need the local model after the rules
//! step. Most sentences come out of the rules step finished; the model is
//! needed only for self-corrections, "you know" and "like", words said
//! twice, lists, spoken punctuation the rules left, numbers to write as
//! digits, spelled-out letters, dictionary near misses, Hinglish, run-on
//! sentences and questions without a "?". Mirrored in
//! `training/cleanup/guards.py`; both run the `parts` examples in
//! `rules_examples.json`.

use super::{rules, text};
use once_cell::sync::Lazy;
use regex::Regex;
use std::collections::HashSet;

/// A stretch of this many words with no comma is a run-on the model splits.
const RUN_ON_WORDS: usize = 20;
/// A longer sentence whose only problems are single words or short phrases
/// sends just the comma-separated pieces around them.
const NARROW_MIN_WORDS: usize = 16;
/// A sentence this short that starts with a cue ("Scratch that.", "No
/// wait.") also brings the sentence after it, which says what replaces the
/// wrong part.
const CUE_ALONE_WORDS: usize = 3;
/// When the parts make up this share of the words, the whole text goes in one
/// request, as before the gate.
pub const WHOLE_SHARE: f64 = 0.7;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Reason {
    List,
    Correction,
    SpokenMark,
    Hinglish,
    RunOn,
    Question,
    Filler,
    Repeat,
    Number,
    Letters,
    Vocabulary,
}

/// A part of the rules text for the model, by byte range. A fragment is a
/// piece from inside a sentence (see `fit_fragment`).
#[derive(Debug, Clone, PartialEq)]
pub struct Part {
    pub start: usize,
    pub end: usize,
    pub fragment: bool,
    pub reasons: Vec<Reason>,
}

struct Hit {
    start: usize,
    end: usize,
    reason: Reason,
}

static CUE: Lazy<Regex> =
    Lazy::new(|| Regex::new(&format!(r"(?i)\b({})\b", rules::CORRECTION_CUE_WORDS)).unwrap());
/// A correction at the start of a sentence, maybe after one short lead-in
/// word ("Oh wait, ...", "Okay, scratch that, ...", "Well, actually, ...").
static OPENING_CUE: Lazy<Regex> = Lazy::new(|| {
    Regex::new(&format!(
        r"(?i)^\W*((oh|okay|ok|well|hmm+|uh+|um+|and|but|so)\W+)?({})\b",
        rules::CORRECTION_CUE_WORDS
    ))
    .unwrap()
});
static YOU_KNOW: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"(?i)(\b(do|did|don't|does|would|if)\s+)?\byou know\b").unwrap());
static LIKE: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r"(?i)(^|[,;]\s*|\b(is|was|were|are|be|been|it's|that's|he's|she's|they're|we're|i'm)\s+)like\b|\blike\s*,|\blike\s+(really|so|super|totally|literally|around|about|maybe|just|kind|honestly)\b").unwrap()
});
static FILLER: Lazy<Regex> = Lazy::new(|| Regex::new(r"(?i)\b(hum|hmm+|uh+|um+|erm)\b").unwrap());
static LIST: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r"(?i)\b(bullet|number (one|1|won)|firstly|secondly|point (one|1))\b|(^|\s)(1|one)[.):]\s[\s\S]*\s(2|two)[.):]?\s|(^|\s)2[.):]?\s[\s\S]*\s3[.):]?\s|\bone,[\s\S]*\btwo,[\s\S]*\bthree\b|\bfirst\b[\s\S]*\bsecond(ly)?\b,").unwrap()
});
static SPOKEN_MARK: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r"(?i)\b(comma|period|full stop|question mark|exclamation (mark|point)|new ?line|new paragraph|colon|semicolon|open bracket|close bracket|dash|dot|slash|underscore|at the rate)\b").unwrap()
});
static UNIT: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r"(?i)^(percent|per|dollars?|bucks|rupees?|lakhs?|crores?|hundred|thousand|million|billion|pm|am|o'clock)$").unwrap()
});
static TIME_WORD: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"(?i)^(at|by|around|till|until|from)$").unwrap());
static LATE_QUESTION: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r"(?i),\s*(can|could|would|will|do|did)\s+(you|we|someone|anyone)\b").unwrap()
});
static CHUNK: Lazy<Regex> = Lazy::new(|| Regex::new(r"[^,;:]+").unwrap());
static COMMON_WORDS: Lazy<HashSet<&'static str>> =
    Lazy::new(|| include_str!("common_words.txt").lines().collect());

fn is_digits(s: &str) -> bool {
    !s.is_empty() && s.bytes().all(|b| b.is_ascii_digit())
}

fn len(s: &str) -> usize {
    s.chars().count()
}

/// Number words to write as digits: above ten, before a unit ("eight
/// percent", "two hundred") or after "at"/"by" ("at eleven"); digits before
/// a spelled unit ("12 percent") or a time said as one number ("by 815").
fn number_hits(sentence: &str) -> Vec<(usize, usize)> {
    let ts = text::word_spans(sentence);
    let word = |i: Option<usize>| i.and_then(|i| ts.get(i)).map_or("", |t| t.2.as_str());
    let mut out = Vec::new();
    for (i, (a, b, w)) in ts.iter().enumerate() {
        let (next, prev) = (word(Some(i + 1)), word(i.checked_sub(1)));
        let hit = match rules::number_word(w) {
            Some(n) => n > 10 || UNIT.is_match(next) || TIME_WORD.is_match(prev),
            None => {
                is_digits(w)
                    && ((UNIT.is_match(next) && next != "am" && next != "pm")
                        || ((3..=4).contains(&w.len()) && TIME_WORD.is_match(prev)))
            }
        };
        if hit {
            out.push((*a, *b));
        }
    }
    out
}

/// Spelled-out letters: "a p i", "graph q l", "n variable".
fn letter_hits(sentence: &str) -> Vec<(usize, usize)> {
    let pieces: Vec<(usize, usize, String)> = sentence
        .split_whitespace()
        .map(|p| {
            let start = p.as_ptr() as usize - sentence.as_ptr() as usize;
            let bare = p.trim_matches(|c| ".,;:!?\"()".contains(c)).to_lowercase();
            (start, start + p.len(), bare)
        })
        .collect();
    let single: Vec<bool> = pieces
        .iter()
        .map(|(_, _, w)| len(w) == 1 && w.chars().all(char::is_alphabetic))
        .collect();
    (0..pieces.len())
        .filter(|&i| {
            single[i]
                && (!matches!(pieces[i].2.as_str(), "a" | "i") || single.get(i + 1) == Some(&true))
        })
        .map(|i| (pieces[i].0, pieces[i].1))
        .collect()
}

/// A word or a run of two or three words said twice in a row.
fn repeat_hits(sentence: &str) -> Vec<(usize, usize)> {
    let ts = text::word_spans(sentence);
    let mut out = Vec::new();
    for n in 1..=3 {
        for i in 0..(ts.len() + 1).saturating_sub(2 * n) {
            let same = (0..n).all(|k| ts[i + k].2 == ts[i + n + k].2);
            if same && !(n == 1 && is_digits(&ts[i].2)) {
                out.push((ts[i].0, ts[i + 2 * n - 1].1));
            }
        }
    }
    out
}

/// A dictionary word as the near-miss check needs it, prepared once per
/// dictation.
struct DictWord {
    word: String,
    len: usize,
    common: bool,
    letters: Letters,
}

/// The dictionary's words (4+ letters) by length, every word in it, and the
/// pieces of 4+ letters of its uncommon words, for `vocab_hits`.
struct Dictionary {
    by_len: Vec<Vec<DictWord>>,
    all: HashSet<String>,
    uncommon: HashSet<String>,
    pieces: HashSet<String>,
}

impl Dictionary {
    fn new(vocab: &[String]) -> Self {
        let all: HashSet<String> = vocab.iter().flat_map(|e| text::phrase_words(e)).collect();
        let mut by_len: Vec<Vec<DictWord>> = Vec::new();
        let (mut uncommon, mut pieces) = (HashSet::new(), HashSet::new());
        for w in all.iter().filter(|w| len(w) >= 4) {
            let common = COMMON_WORDS.contains(w.as_str());
            if !common {
                uncommon.insert(w.clone());
                pieces.extend(substrings(w));
            }
            let n = len(w);
            if by_len.len() <= n {
                by_len.resize_with(n + 1, Vec::new);
            }
            by_len[n].push(DictWord {
                word: w.clone(),
                len: n,
                common,
                letters: letters(w),
            });
        }
        Self {
            by_len,
            all,
            uncommon,
            pieces,
        }
    }

    /// Words whose length allows at most one change per `per` letters from a
    /// word of `n` letters.
    fn near_length(&self, n: usize, per: usize) -> impl Iterator<Item = &DictWord> {
        let lo = (n * per).div_ceil(per + 1);
        let hi = (n * per / (per - 1)).min(self.by_len.len().saturating_sub(1));
        self.by_len.get(lo..=hi).into_iter().flatten().flatten()
    }
}

/// Every run of 4 or more characters of `word`.
fn substrings(word: &str) -> Vec<String> {
    let chars: Vec<char> = word.chars().collect();
    let mut out = Vec::new();
    for i in 0..chars.len() {
        for j in i + 4..=chars.len() {
            out.push(chars[i..j].iter().collect());
        }
    }
    out
}

/// How often each letter a-z (and anything else) occurs, and which occur.
#[derive(Clone, Copy)]
struct Letters {
    counts: [u16; 27],
    present: u32,
}

fn letters(word: &str) -> Letters {
    let mut counts = [0u16; 27];
    for c in word.chars() {
        let i = if c.is_ascii_lowercase() {
            (c as u8 - b'a') as usize
        } else {
            26
        };
        counts[i] = counts[i].saturating_add(1);
    }
    let present = (0..27)
        .filter(|&i| counts[i] > 0)
        .fold(0, |m, i| m | 1 << i);
    Letters { counts, present }
}

/// Whether `heard` is at most one change per `per` letters away from the
/// dictionary word. The length and letter-count gaps are lower bounds of the
/// edit distance, so most pairs are ruled out without computing it.
fn close(w: &DictWord, heard: &str, heard_len: usize, heard_letters: &Letters, per: usize) -> bool {
    let within = |bound: usize| bound * per <= w.len;
    let missing = (w.letters.present ^ heard_letters.present).count_ones() as usize;
    if !within(w.len.abs_diff(heard_len)) || !within(missing.div_ceil(2)) {
        return false;
    }
    let gap: usize = (0..27)
        .map(|i| (w.letters.counts[i] as usize).abs_diff(heard_letters.counts[i] as usize))
        .sum();
    within(gap.div_ceil(2)) && within(strsim::levenshtein(&w.word, heard))
}

/// A word that looks like a dictionary word but is not spelled as one: one
/// change per 2 letters for a single word (or containing it, or inside it),
/// one per 3 letters for two or three words run together.
fn vocab_hits(sentence: &str, dictionary: &Dictionary) -> Vec<(usize, usize)> {
    if dictionary.by_len.is_empty() {
        return Vec::new();
    }
    let ts: Vec<(usize, usize, String)> = text::word_spans(sentence)
        .into_iter()
        .filter(|t| !dictionary.all.contains(&t.2))
        .collect();
    let mut out = Vec::new();
    for n in 2..=3 {
        for i in 0..(ts.len() + 1).saturating_sub(n) {
            let word: String = ts[i..i + n].iter().map(|t| t.2.as_str()).collect();
            let (size, l) = (len(&word), letters(&word));
            if dictionary
                .near_length(size, 3)
                .any(|w| close(w, &word, size, &l, 3))
            {
                out.push((ts[i].0, ts[i + n - 1].1));
            }
        }
    }
    for (a, b, h) in &ts {
        let size = len(h);
        if size < 4 || COMMON_WORDS.contains(h.as_str()) {
            continue;
        }
        let l = letters(h);
        let hit = dictionary.pieces.contains(h)
            || substrings(h)
                .iter()
                .any(|p| dictionary.uncommon.contains(p))
            || dictionary
                .near_length(size, 2)
                .any(|w| !w.common && close(w, h, size, &l, 2));
        if hit {
            out.push((*a, *b));
        }
    }
    out
}

/// What in this sentence (rules text, trimmed) needs the model. A hit that
/// covers the whole sentence means the sentence goes whole.
fn hits(sentence: &str, dictionary: &Dictionary) -> Vec<Hit> {
    let whole = |reason| Hit {
        start: 0,
        end: sentence.len(),
        reason,
    };
    let mut out = Vec::new();
    if CUE.is_match(sentence) {
        out.push(whole(Reason::Correction));
    }
    if SPOKEN_MARK.is_match(sentence) {
        out.push(whole(Reason::SpokenMark));
    }
    if !rules::hindi_words(sentence).is_empty() {
        out.push(whole(Reason::Hinglish));
    }
    if sentence
        .split([',', ';', ':'])
        .any(|c| c.split_whitespace().count() > RUN_ON_WORDS)
    {
        out.push(whole(Reason::RunOn));
    }
    if !sentence.ends_with('?')
        && (rules::starts_like_question(&rules::OPENER.replace(sentence, ""))
            || LATE_QUESTION.is_match(sentence))
    {
        out.push(whole(Reason::Question));
    }
    let local = |spans: Vec<(usize, usize)>, reason| {
        spans
            .into_iter()
            .map(move |(start, end)| Hit { start, end, reason })
    };
    let you_know = YOU_KNOW
        .captures_iter(sentence)
        .filter(|c| c.get(1).is_none())
        .map(|c| (c.get(0).unwrap().start(), c.get(0).unwrap().end()))
        .collect();
    let fillers = LIKE
        .find_iter(sentence)
        .chain(FILLER.find_iter(sentence))
        .map(|m| (m.start(), m.end()))
        .collect();
    out.extend(local(you_know, Reason::Filler));
    out.extend(local(fillers, Reason::Filler));
    out.extend(local(repeat_hits(sentence), Reason::Repeat));
    out.extend(local(number_hits(sentence), Reason::Number));
    out.extend(local(letter_hits(sentence), Reason::Letters));
    out.extend(local(vocab_hits(sentence, dictionary), Reason::Vocabulary));
    out
}

/// The comma-separated pieces around each hit, one piece either side.
fn windows(sentence: &str, hits: &[Hit]) -> Vec<(usize, usize)> {
    let chunks: Vec<(usize, usize)> = CHUNK
        .find_iter(sentence)
        .filter(|m| !m.as_str().trim().is_empty())
        .map(|m| (m.start(), m.end()))
        .collect();
    let last = chunks.len() - 1;
    let mut spans: Vec<(usize, usize)> = hits.iter().map(|h| (h.start, h.end)).collect();
    spans.sort_unstable();
    let mut out: Vec<(usize, usize)> = Vec::new();
    for (start, end) in spans {
        let i = chunks.iter().position(|c| start < c.1).unwrap_or(last);
        let j = chunks.iter().position(|c| end <= c.1).unwrap_or(last);
        let (lo, hi) = (chunks[i.saturating_sub(1)].0, chunks[(j + 1).min(last)].1);
        match out.last_mut() {
            Some(prev) if lo <= prev.1 + 1 => prev.1 = prev.1.max(hi),
            _ => out.push((lo, hi)),
        }
    }
    out
}

/// (start, end) of each sentence: up to ".", "?" or "!" followed by a space
/// or the end ("2.5" and "docs.google.com" go on), or up to a line break.
fn sentences(text: &str) -> Vec<(usize, usize)> {
    let bytes = text.as_bytes();
    let is_mark = |b: u8| matches!(b, b'.' | b'?' | b'!');
    let (mut out, mut start, mut i) = (Vec::new(), 0, 0);
    while i < bytes.len() {
        if bytes[i] == b'\n' {
            out.push((start, i));
            i += 1;
            start = i;
        } else if is_mark(bytes[i]) {
            let mut j = i;
            while j < bytes.len() && is_mark(bytes[j]) {
                j += 1;
            }
            if text[j..].chars().next().is_none_or(char::is_whitespace) {
                out.push((start, j));
                start = j;
            }
            i = j;
        } else {
            i += 1;
        }
    }
    out.push((start, text.len()));
    out
}

fn trimmed(text: &str, start: usize, end: usize) -> (usize, usize) {
    let s = &text[start..end];
    let lead = s.len() - s.trim_start().len();
    (start + lead, start + lead + s.trim().len())
}

/// The parts of `text` (rules text) the model should clean, in order; empty
/// when nothing needs it. A list sends the whole text. A sentence that
/// starts with a correction ("Actually no, ...") brings the sentence before
/// it, and when it is only the cue ("Scratch that.") the one after it too.
/// Neighbouring sentences go together.
pub fn parts(text: &str, vocab: &[String]) -> Vec<Part> {
    if LIST.is_match(text) {
        return vec![Part {
            start: 0,
            end: text.len(),
            fragment: false,
            reasons: vec![Reason::List],
        }];
    }
    let sentences: Vec<(usize, usize)> = sentences(text)
        .into_iter()
        .filter(|&(a, b)| !text[a..b].trim().is_empty())
        .map(|(a, b)| trimmed(text, a, b))
        .collect();
    let dictionary = Dictionary::new(vocab);
    let mut parts: Vec<Part> = Vec::new();
    for (k, &(a, b)) in sentences.iter().enumerate() {
        if parts.last().is_some_and(|p| b <= p.end) {
            continue;
        }
        let s = &text[a..b];
        let hits = hits(s, &dictionary);
        if hits.is_empty() {
            continue;
        }
        let mut reasons: Vec<Reason> = Vec::new();
        for h in &hits {
            if !reasons.contains(&h.reason) {
                reasons.push(h.reason);
            }
        }
        let local = hits.iter().all(|h| h.end - h.start < s.len());
        let words = s.split_whitespace().count();
        if local && words > NARROW_MIN_WORDS {
            let pieces = windows(s, &hits);
            let sent: usize = pieces
                .iter()
                .map(|&(x, y)| s[x..y].split_whitespace().count())
                .sum();
            if (sent as f64) < WHOLE_SHARE * words as f64 {
                for (x, y) in pieces {
                    let (x, y) = trimmed(s, x, y);
                    let fragment = !(x == 0 && y == s.len());
                    parts.push(Part {
                        start: a + x,
                        end: a + y,
                        fragment,
                        reasons: reasons.clone(),
                    });
                }
                continue;
            }
        }
        let (mut start, mut end) = (a, b);
        if reasons.contains(&Reason::Correction) && OPENING_CUE.is_match(s) {
            if k > 0 {
                start = sentences[k - 1].0;
            }
            if words <= CUE_ALONE_WORDS && k + 1 < sentences.len() {
                end = sentences[k + 1].1;
            }
        }
        // Take in every earlier part this one overlaps (a correction reaching
        // back can cover several pieces) or directly follows.
        while let Some(p) = parts.last() {
            let overlaps = start < p.end;
            if !overlaps && (p.fragment || start > p.end + 1 || text[p.end..start].contains('\n')) {
                break;
            }
            let prev = parts.pop().unwrap();
            start = start.min(prev.start);
            let mut all = prev.reasons;
            all.extend(
                reasons
                    .iter()
                    .filter(|r| !all.contains(r))
                    .copied()
                    .collect::<Vec<_>>(),
            );
            reasons = all;
        }
        parts.push(Part {
            start,
            end,
            fragment: false,
            reasons,
        });
    }
    parts
}

/// Whether `part` can replace text after `pos`: in order, inside the text
/// and on character boundaries.
pub fn fits(text: &str, pos: usize, part: &Part) -> bool {
    pos <= part.start
        && part.start <= part.end
        && part.end <= text.len()
        && text.is_char_boundary(part.start)
        && text.is_char_boundary(part.end)
}

/// Whether the parts are in order, don't overlap and lie inside the text.
pub fn in_order(text: &str, parts: &[Part]) -> bool {
    let mut pos = 0;
    parts.iter().all(|p| {
        let ok = fits(text, pos, p);
        pos = p.end;
        ok
    })
}

/// Rules text pasted without the model gets a final period when it ends in a
/// word (formal style; the model would have added one).
pub fn end_mark(text: &str) -> String {
    let t = text.trim_end();
    match t.chars().last() {
        Some(c) if c.is_alphanumeric() || matches!(c, '"' | '\'' | ')') => format!("{t}."),
        _ => text.to_string(),
    }
}

/// A piece from inside a sentence fits back between the words around it: it
/// starts in lowercase when the piece did (unless the reply starts with a
/// name or "I"), and gets no end mark or trailing comma when the piece had
/// no end mark.
pub fn fit_fragment(core: &str, reply: &str) -> String {
    let mut out = reply.to_string();
    let first_word = reply
        .split_whitespace()
        .next()
        .unwrap_or("")
        .trim_matches(|c: char| !c.is_alphanumeric() && c != '\'')
        .to_lowercase();
    let ordinary = first_word != "i" && COMMON_WORDS.contains(first_word.as_str());
    if core.chars().next().is_some_and(char::is_lowercase) && ordinary {
        if let Some(r) = reply.chars().next() {
            out = format!("{}{}", r.to_lowercase(), &reply[r.len_utf8()..]);
        }
    }
    if !core.ends_with(['.', '!', '?']) {
        out = out.trim_end_matches(['.', '!', '?', ',', ';']).to_string();
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::textflow::rules::tests::{examples, vocab};

    fn shown(text: &str, parts: &[Part]) -> Vec<(String, bool)> {
        parts
            .iter()
            .map(|p| (text[p.start..p.end].to_string(), p.fragment))
            .collect()
    }

    /// Texts that must not call the model (no parts) and ones that must,
    /// with the parts sent.
    #[test]
    fn parts_examples() {
        for e in examples("parts") {
            let text = e[0].as_str().unwrap();
            let want: Vec<(String, bool)> = serde_json::from_value(e[1].clone()).unwrap();
            assert_eq!(shown(text, &parts(text, &vocab())), want, "{text:?}");
        }
    }

    /// A big dictionary and a long dictation must stay quick: this runs
    /// before every paste.
    #[test]
    fn big_dictionary_is_quick() {
        let syllables = [
            "ka", "ri", "vo", "len", "mu", "sha", "tor", "pi", "ze", "dan",
        ];
        let vocab: Vec<String> = (0..1500)
            .map(|i| {
                let s = |k: usize| syllables[(i / k) % syllables.len()];
                format!("{}{}{}{}", s(1), s(7), s(31), s(97))
            })
            .collect();
        let sentence = "So the team went over the plan for the launch, and we agreed that \
            the new page should go live next week, after the last round of checks. ";
        let text = sentence.repeat(31);
        parts(&text, &vocab[..1]);
        let started = std::time::Instant::now();
        parts(&text, &vocab);
        let took = started.elapsed();
        // About 10 ms in a release build (it was 0.6 s); debug builds are
        // much slower.
        let limit = if cfg!(debug_assertions) { 1500 } else { 30 };
        assert!(took < std::time::Duration::from_millis(limit), "{took:?}");
    }

    #[test]
    fn end_mark_only_after_a_word() {
        assert_eq!(end_mark("The build is green"), "The build is green.");
        assert_eq!(end_mark("Is it green?"), "Is it green?");
        assert_eq!(end_mark("Send it to [[S0]]"), "Send it to [[S0]]");
    }

    #[test]
    fn fragments_fit_back_into_the_sentence() {
        for e in examples("fit_fragment") {
            let (core, reply) = (e[0].as_str().unwrap(), e[1].as_str().unwrap());
            assert_eq!(
                fit_fragment(core, reply),
                e[2].as_str().unwrap(),
                "{core:?}"
            );
        }
    }
}
