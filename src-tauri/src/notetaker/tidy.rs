//! Clean-ups on a meeting's transcript. Speech models loop on a phrase, write
//! a stock phrase for a piece with no speech ("So, thank you."), read out the
//! words they were asked to listen for, and mishear the user's own words
//! ("brimvail" for a dictionary "Brimvale"). Changing a real word is worse
//! than leaving a garbled one, so every step here holds back when unsure.

use super::audio::Piece;
use super::diarize;
use super::Segment;
use once_cell::sync::Lazy;
use std::collections::{HashMap, HashSet};

/// Longest phrase, in words, checked for repeats.
const MAX_REPEAT_WORDS: usize = 12;
/// How many times in a row a phrase must be said to be kept once: three
/// words or more, two words, one word.
const LONG_PHRASE_TIMES: usize = 3;
const TWO_WORD_TIMES: usize = 5;
const WORD_TIMES: usize = 8;

fn plain(word: &str) -> String {
    word.chars()
        .filter(|c| diarize::word_char(*c))
        .flat_map(char::to_lowercase)
        .collect()
}

fn is_number(key: &str) -> bool {
    key.chars().all(|c| c.is_ascii_digit()) || super::hinglish::number_word(key)
}

/// `text` with a phrase of 3 or more words said 3 or more times in a row,
/// two words said 5 or more times, or one word 8 or more times, kept once,
/// and how many such runs there were. Numbers ("one, one, one") stay.
pub fn collapse_repeats(text: &str) -> (String, usize) {
    let words: Vec<&str> = text.split_whitespace().collect();
    let keys: Vec<String> = words.iter().map(|w| plain(w)).collect();
    let same = |a: usize, b: usize, len: usize| {
        (0..len).all(|k| !keys[a + k].is_empty() && keys[a + k] == keys[b + k])
    };
    let mut kept: Vec<String> = Vec::with_capacity(words.len());
    let mut runs = 0;
    let mut i = 0;
    while i < words.len() {
        let mut run = None;
        for len in 1..=MAX_REPEAT_WORDS.min(words.len() - i) {
            let need = match len {
                1 => WORD_TIMES,
                2 => TWO_WORD_TIMES,
                _ => LONG_PHRASE_TIMES,
            };
            if i + need * len > words.len() || keys[i..i + len].iter().any(|k| is_number(k)) {
                continue;
            }
            let mut times = 1;
            while i + (times + 1) * len <= words.len() && same(i, i + times * len, len) {
                times += 1;
            }
            if times >= need {
                run = Some((len, i + times * len));
                break;
            }
        }
        match run {
            Some((len, end)) => {
                kept.extend(words[i..i + len - 1].iter().map(|w| w.to_string()));
                let last = words[i + len - 1];
                let core = last.trim_end_matches(|c: char| !c.is_alphanumeric());
                let mark = &words[end - 1][words[end - 1]
                    .trim_end_matches(|c: char| !c.is_alphanumeric())
                    .len()..];
                kept.push(format!("{core}{mark}"));
                runs += 1;
                i = end;
            }
            None => {
                kept.push(words[i].to_string());
                i += 1;
            }
        }
    }
    if runs == 0 {
        return (text.to_string(), 0);
    }
    (kept.join(" "), runs)
}

/// Whole lines speech models write for a piece with no speech in it.
const STOCK_PHRASES: &[&str] = &[
    "so",
    "so thank you",
    "thank you",
    "thank you so much",
    "thank you very much",
    "thanks for watching",
    "thank you for watching",
    "thank you for watching this video",
    "please subscribe",
    "like and subscribe",
    "see you in the next video",
    "i m gonna go to the phone",
    "i m",
    "you",
    "bye",
    "bye bye",
    "hopefully",
    "you better than that",
];

/// Whether the whole line is one of [`STOCK_PHRASES`].
pub fn stock_phrase(text: &str) -> bool {
    let words: Vec<String> = text
        .split(|c: char| !diarize::word_char(c))
        .filter(|w| !w.is_empty())
        .map(str::to_lowercase)
        .collect();
    STOCK_PHRASES.contains(&words.join(" ").as_str())
}

/// The speech detector's highest score in a piece below which it heard no
/// speech worth a line, and the speech it may have heard at most.
const NO_SPEECH_PEAK: f32 = 0.7;
const NO_SPEECH_MS: u64 = 400;
/// Speech the detector may have heard in a piece the model only read its
/// hints out for.
const READ_OUT_SPEECH_MS: u64 = 1_000;
/// A line of this many different hint words, and nothing else, is the model
/// reading out its hints.
const READ_OUT_WORDS: usize = 3;

fn speech_ms(piece: &Piece) -> u64 {
    piece.speech_samples * 1000 / super::audio::RATE as u64
}

/// Whether the speech detector heard next to no speech in `piece`. Only a
/// real detector counts, never loudness.
fn no_speech(piece: &Piece, detector: bool) -> bool {
    detector && piece.peak < NO_SPEECH_PEAK && speech_ms(piece) <= NO_SPEECH_MS
}

/// Whether the line is only `hints` (at least 3 different ones) and
/// nothing else: the model reading out the words it was asked to listen for.
pub fn read_out(text: &str, hints: &[String]) -> bool {
    let hint_words: HashSet<String> = hints
        .iter()
        .flat_map(|h| h.split(|c: char| !diarize::word_char(c)))
        .filter(|w| !w.is_empty())
        .map(str::to_lowercase)
        .collect();
    let words: Vec<String> = text
        .split(|c: char| !diarize::word_char(c))
        .filter(|w| !w.is_empty())
        .map(str::to_lowercase)
        .collect();
    let distinct: HashSet<&String> = words.iter().collect();
    !words.is_empty()
        && words.iter().all(|w| hint_words.contains(w))
        && distinct.len() >= READ_OUT_WORDS
}

/// Whether the model's `text` for `piece` is not speech: a stock phrase for
/// a piece the detector heard no speech in, or a read-out of the `hints` it
/// was given for a piece with little speech. `detector` says whether a real
/// speech detector cut the piece.
pub fn heard_nothing(text: &str, piece: &Piece, detector: bool, hints: &[String]) -> bool {
    let stock = stock_phrase(text) && no_speech(piece, detector);
    let hints_read = !hints.is_empty()
        && detector
        && speech_ms(piece) <= READ_OUT_SPEECH_MS
        && read_out(text, hints);
    if stock || hints_read {
        log::debug!("Notetaker dropped a line with no speech: {text}");
    }
    stock || hints_read
}

/// Romanised Hindi words, never taken for a misheard dictionary word. See
/// the file's header for where they come from.
const HINDI_LEXICON: &str = include_str!("hindi_lexicon.txt");
static LEXICON: Lazy<HashSet<&'static str>> = Lazy::new(|| {
    HINDI_LEXICON
        .lines()
        .filter(|l| !l.starts_with('#') && !l.is_empty())
        .collect()
});

/// The user's own words (dictionary entries) a misheard word may be put
/// back as.
pub struct Vocabulary {
    terms: Vec<Term>,
    /// Words never taken for a term: names of the people in the meeting.
    keep: HashSet<String>,
}

struct Term {
    text: String,
    letters: String,
    /// Has a capital: a name, brand or acronym.
    proper: bool,
    /// A person's name: one of the meeting's names, or one that sounds like
    /// a Hindi first name. A lowercase word never becomes one.
    person: bool,
    /// Written like a brand or acronym ("ZoomInfo", "POC"): heard as two
    /// words spelt exactly the same, it is always this entry.
    branded: bool,
    /// How each way of saying it sounds: the word, and an acronym's
    /// letters. Empty for entries of several words, which only match when
    /// heard exactly.
    shapes: Vec<Shape>,
}

#[derive(Clone)]
struct Shape {
    first: Option<char>,
    /// The first consonant as said ("j" and "z" are one), which a name or
    /// brand must share with what was heard: "saurav" is not "Gourav".
    lead: Option<char>,
    skeleton: Vec<char>,
    sound: Vec<char>,
}

/// Letter names, for acronyms said letter by letter.
const LETTER_NAMES: [&str; 26] = [
    "ay", "bee", "see", "dee", "ee", "ef", "jee", "aych", "eye", "jay", "kay", "el", "em", "en",
    "o", "pee", "kyu", "ar", "es", "tee", "yu", "vee", "dablyu", "eks", "vy", "zed",
];

/// Shapes shorter than this must have the same outline, but for a vowel
/// at the end.
const SHORT_SHAPE: usize = 6;
const SAME_OUTLINE: f64 = 0.75;
const SAME_SOUND_LONG: f64 = 0.45;
/// How alike the spelling by sound must be for short shapes, and for a
/// capitalised word at the start of a sentence.
const SAME_SOUND_STRICT: f64 = 0.7;
const MIN_TERM_LETTERS: usize = 3;

/// The word as vowels ('V') and consonant groups that transcribers mix up,
/// so "bandar" and "vendor" agree ("1V53V6") and "kitchen" does not match
/// "kaychorn" ("2V32V5" against "2V2V65").
fn skeleton(word: &str) -> Vec<char> {
    let mut out: Vec<char> = Vec::new();
    for unit in diarize::name_sound(&word.to_lowercase().replace("qu", "kw")) {
        let group = if unit.is_uppercase() {
            Some('V')
        } else {
            crate::textflow::sound_key(&unit.to_string()).chars().next()
        };
        if let Some(g) = group.filter(|g| out.last() != Some(g)) {
            out.push(g);
        }
    }
    out
}

fn shape(word: &str) -> Shape {
    let lower = word.to_lowercase().replace("qu", "kw");
    let sound = diarize::name_sound(&lower);
    Shape {
        first: crate::textflow::sound_key(&lower).chars().next(),
        lead: sound
            .iter()
            .find(|c| c.is_lowercase())
            .map(|&c| if c == 'z' { 'j' } else { c }),
        skeleton: skeleton(word),
        sound,
    }
}

fn likeness(a: &[char], b: &[char]) -> f64 {
    let longest = a.len().max(b.len());
    if longest == 0 {
        return 0.0;
    }
    let (a, b): (String, String) = (a.iter().collect(), b.iter().collect());
    1.0 - strsim::levenshtein(&a, &b) as f64 / longest as f64
}

/// The longest run of `a`'s letters found in order in `b`.
fn common_run(a: &str, b: &str) -> usize {
    let (a, b): (Vec<char>, Vec<char>) = (a.chars().collect(), b.chars().collect());
    let mut row = vec![0usize; b.len() + 1];
    for x in &a {
        let mut diagonal = 0;
        for (j, y) in b.iter().enumerate() {
            let above = row[j + 1];
            row[j + 1] = if x == y {
                diagonal + 1
            } else {
                above.max(row[j])
            };
            diagonal = above;
        }
    }
    row[b.len()]
}

impl Shape {
    /// How well a heard word fits this shape, if it fits. `strict` asks for
    /// the short-word rule whatever the length.
    fn fit(&self, heard: &Shape, strict: bool, proper: bool) -> Option<f64> {
        if self.first.is_none() || self.first != heard.first || (proper && self.lead != heard.lead)
        {
            return None;
        }
        let outline = likeness(&self.skeleton, &heard.skeleton);
        let sound = likeness(&self.sound, &heard.sound);
        let bare = |s: &[char]| s.strip_suffix(&['V']).map_or(s.len(), <[char]>::len);
        let same_outline =
            self.skeleton[..bare(&self.skeleton)] == heard.skeleton[..bare(&heard.skeleton)];
        let consonants = |s: &[char]| -> String { s.iter().filter(|c| **c != 'V').collect() };
        let (ours, theirs) = (consonants(&self.skeleton), consonants(&heard.skeleton));
        // A mishearing adds or changes sounds; it rarely drops one.
        let keeps_sounds = theirs.chars().count() >= ours.chars().count()
            && common_run(&ours, &theirs) + 1 >= ours.chars().count();
        let fits = if strict || self.skeleton.len() < SHORT_SHAPE {
            same_outline && sound >= SAME_SOUND_STRICT
        } else {
            keeps_sounds && outline >= SAME_OUTLINE && sound >= SAME_SOUND_LONG
        };
        fits.then_some(outline + sound)
    }
}

fn letters(word: &str) -> String {
    word.chars()
        .filter(|c| c.is_alphanumeric())
        .flat_map(char::to_lowercase)
        .collect()
}

/// A common English word, or one with an ending added ("blocker").
fn english(word: &str) -> bool {
    let common = crate::textflow::commands::is_common_word;
    common(word)
        || ["s", "es", "ed", "er", "ers", "ing"].iter().any(|end| {
            word.strip_suffix(end)
                .is_some_and(|stem| stem.chars().count() >= MIN_TERM_LETTERS && common(stem))
        })
}

/// Hindi words that are also first names ("desh ka gaurav", "prem se").
const HINDI_NAME_WORDS: &[&str] = &[
    "gaurav", "prem", "raj", "deepak", "suraj", "vijay", "kiran", "asha", "pooja", "puja", "anand",
    "sagar", "kamal", "aman", "chanda", "chandni", "jyoti", "roshni", "shanti", "nisha", "usha",
    "sundar", "mohan", "ram", "shyam", "ganga", "lakshmi", "prakash", "sneha", "preeti", "pyar",
    "vikas", "vinay", "vishwas", "vimal", "nirmal", "amar", "ajay", "arjun", "sapna", "khushi",
    "pankaj", "akash", "aakash", "pawan", "neel", "dev", "devi", "uday", "bharat", "hira", "heera",
    "moti", "sona", "chandan", "barkha", "basant", "bahar",
];

/// An English or Hindi word: never misheard, as far as we can tell.
fn known(lower: &str) -> bool {
    HINDI_NAME_WORDS.contains(&lower)
        || crate::textflow::hindi::WORDS.contains(&lower)
        || diarize::HINGLISH_WORDS.contains(&lower)
        || LEXICON.contains(lower)
        || english(lower)
}

/// Whether two heard words write a brand's capitals: they split where the
/// brand's next capital starts, and one of them has the brand's capitals
/// ("Zoom info" and "search API" do; "linked in" does not). An acronym's
/// parts must both be capitals.
fn same_capitals(first: &str, second: &str, brand: &str) -> bool {
    let brand: Vec<char> = brand.chars().filter(|c| c.is_alphanumeric()).collect();
    let (a, b): (Vec<char>, Vec<char>) = (
        first.chars().filter(|c| c.is_alphanumeric()).collect(),
        second.chars().filter(|c| c.is_alphanumeric()).collect(),
    );
    if a.len() + b.len() != brand.len() || a.is_empty() || b.is_empty() {
        return false;
    }
    if brand.iter().all(|c| !c.is_lowercase()) {
        return a.iter().chain(&b).all(|c| !c.is_lowercase());
    }
    brand[a.len()].is_uppercase() && (a == brand[..a.len()] || b == brand[a.len()..])
}

/// Whether a dictionary word (lowercase letters) is a person's name.
fn person_name(word: &str, names: &HashSet<String>) -> bool {
    let sound = diarize::name_sound(word);
    names.contains(word)
        || LEXICON.contains(word)
        || HINDI_NAME_WORDS
            .iter()
            .any(|n| diarize::name_sound(n) == sound)
}

fn capitalised(word: &str) -> bool {
    word.chars().next().is_some_and(char::is_uppercase)
}

/// Per-word answers kept while a meeting is tidied.
#[derive(Default)]
pub struct Seen {
    known: HashMap<String, bool>,
    terms: HashMap<(String, bool), Option<usize>>,
}

impl Seen {
    fn known(&mut self, word: &str) -> bool {
        let lower = word.to_lowercase();
        if let Some(&k) = self.known.get(&lower) {
            return k;
        }
        let k = known(&lower);
        self.known.insert(lower, k);
        k
    }
}

/// How a heard word may be treated.
#[derive(Clone, Copy, PartialEq)]
enum Heard {
    /// Not a candidate: English, Hindi, a name, too short.
    Keep,
    /// Lowercase and unknown.
    Odd,
    /// Capitalised only because it starts a sentence, and unknown: may
    /// become a capitalised term heard almost exactly.
    OddCapital,
}

impl Vocabulary {
    /// Dictionary `words`; `names` of the people in the meeting are never
    /// changed.
    pub fn new(words: &[String], names: &[String]) -> Self {
        let keep: HashSet<String> = names
            .iter()
            .flat_map(|n| n.split(|c: char| !c.is_alphabetic()))
            .filter(|p| !p.is_empty())
            .map(str::to_lowercase)
            .collect();
        let mut terms: Vec<Term> = Vec::new();
        for word in words.iter().map(|w| w.trim()) {
            let alpha = letters(word);
            if alpha.chars().count() < MIN_TERM_LETTERS
                || word.contains(['@', '/'])
                || terms.iter().any(|t| t.text == word)
            {
                continue;
            }
            let single = !word.contains(char::is_whitespace);
            let mut shapes = Vec::new();
            if single && alpha.chars().all(|c| c.is_ascii_alphabetic()) {
                shapes.push(shape(&alpha));
                if word.chars().all(|c| c.is_ascii_uppercase()) {
                    let spoken: String = alpha
                        .bytes()
                        .map(|b| LETTER_NAMES[(b - b'a') as usize])
                        .collect();
                    shapes.push(shape(&spoken));
                }
            }
            let person = single
                && word.starts_with(char::is_uppercase)
                && !word.chars().skip(1).any(char::is_uppercase)
                && person_name(&alpha, &keep);
            terms.push(Term {
                text: word.to_string(),
                letters: alpha,
                proper: word.chars().any(char::is_uppercase),
                person,
                branded: single
                    && (word.chars().skip(1).any(char::is_uppercase)
                        || word
                            .chars()
                            .filter(|c| c.is_alphabetic())
                            .all(char::is_uppercase)),
                shapes,
            });
        }
        Self { terms, keep }
    }

    fn heard(&self, word: &str, starts_sentence: bool, seen: &mut Seen) -> Heard {
        let usable = word.chars().count() >= MIN_TERM_LETTERS
            && word.chars().all(|c| c.is_ascii_alphabetic())
            && !self.keep.contains(&word.to_lowercase())
            && !self.terms.iter().any(|t| t.letters == word.to_lowercase());
        let capital = capitalised(word);
        if !usable || (capital && !starts_sentence) || word.chars().skip(1).any(char::is_uppercase)
        {
            return Heard::Keep;
        }
        match (seen.known(word), capital) {
            (true, _) => Heard::Keep,
            (false, true) => Heard::OddCapital,
            (false, false) => Heard::Odd,
        }
    }

    /// The term a heard word (or two written together) stands for: the only
    /// one it fits, or the clearly best.
    fn term_for(&self, heard: &str, how: Heard, seen: &mut Seen) -> Option<&str> {
        let key = (heard.to_lowercase(), how == Heard::OddCapital);
        if let Some(found) = seen.terms.get(&key) {
            return found.map(|i| self.terms[i].text.as_str());
        }
        let strict = how == Heard::OddCapital;
        let heard_shape = shape(heard);
        let mut fits: Vec<(f64, usize)> = self
            .terms
            .iter()
            .enumerate()
            .filter(|(_, t)| if strict { t.proper } else { !t.person })
            .filter_map(|(i, t)| {
                t.shapes
                    .iter()
                    .filter_map(|s| s.fit(&heard_shape, strict, t.proper))
                    .reduce(f64::max)
                    .map(|f| (f, i))
            })
            .collect();
        fits.sort_by(|a, b| b.0.total_cmp(&a.0));
        let found = match fits.as_slice() {
            [] => None,
            [(_, only)] => Some(*only),
            [(best, i), (next, _), ..] => (best > next).then_some(*i),
        };
        seen.terms.insert(key, found);
        found.map(|i| self.terms[i].text.as_str())
    }

    /// The entry spelt by two heard words written together or apart:
    /// "brim vale" is "Brimvale".
    fn spaced_term(&self, first: &str, second: &str) -> Option<&Term> {
        let joined = format!("{}{}", letters(first), letters(second));
        self.terms.iter().find(|t| t.letters == joined)
    }

    /// `text` with misheard dictionary words written as the entry, and what
    /// was changed (heard, entry).
    pub fn fix(&self, text: &str, seen: &mut Seen) -> (String, Vec<(String, String)>) {
        if self.terms.is_empty() {
            return (text.to_string(), Vec::new());
        }
        let tokens: Vec<&str> = text.split(' ').collect();
        let core = |t: &str| -> (usize, usize) {
            let start = t.find(|c: char| c.is_alphanumeric()).unwrap_or(t.len());
            let end = t.rfind(|c: char| c.is_alphanumeric()).map_or(start, |i| {
                i + t[i..].chars().next().map_or(1, char::len_utf8)
            });
            (start, end.max(start))
        };
        let word_of = |t: &str| {
            let (s, e) = core(t);
            t[s..e].to_string()
        };
        let starts_sentence = |i: usize| {
            i == 0
                || tokens[..i]
                    .iter()
                    .rev()
                    .find(|t| !t.is_empty())
                    .is_none_or(|t| t.ends_with(['.', '?', '!']))
        };
        let mut out: Vec<String> = Vec::with_capacity(tokens.len());
        let mut fixed = Vec::new();
        let mut i = 0;
        while i < tokens.len() {
            let word = word_of(tokens[i]);
            let how = self.heard(&word, starts_sentence(i), seen);
            let single = (how != Heard::Keep)
                .then(|| self.term_for(&word, how, seen))
                .flatten();
            let next = tokens
                .get(i + 1)
                .map(|t| word_of(t))
                .filter(|n| !n.is_empty());
            let pair = next.filter(|_| single.is_none() && core(tokens[i]).1 == tokens[i].len());
            let joined = pair.as_ref().and_then(|next| {
                let how_next = self.heard(next, false, seen);
                let plain_pair = !capitalised(next) && (!capitalised(&word) || starts_sentence(i));
                let one_unknown = !seen.known(&word) || !seen.known(next);
                let spaced = self
                    .spaced_term(&word, next)
                    .filter(|t| {
                        if t.branded {
                            same_capitals(&word, next, &t.text)
                        } else {
                            !t.proper && plain_pair && one_unknown
                        }
                    })
                    .map(|t| t.text.as_str());
                spaced.or_else(|| {
                    (how == Heard::Odd && how_next == Heard::Odd)
                        .then(|| self.term_for(&format!("{word}{next}"), Heard::Odd, seen))
                        .flatten()
                })
            });
            if let (Some(term), Some(next)) = (joined, &pair) {
                let (start, _) = core(tokens[i]);
                let (_, end) = core(tokens[i + 1]);
                fixed.push((format!("{word} {next}"), term.to_string()));
                out.push(format!(
                    "{}{term}{}",
                    &tokens[i][..start],
                    &tokens[i + 1][end..]
                ));
                i += 2;
                continue;
            }
            match single {
                Some(term) => {
                    let (start, end) = core(tokens[i]);
                    fixed.push((word.clone(), term.to_string()));
                    out.push(format!(
                        "{}{term}{}",
                        &tokens[i][..start],
                        &tokens[i][end..]
                    ));
                }
                None => out.push(tokens[i].to_string()),
            }
            i += 1;
        }
        (out.join(" "), fixed)
    }
}

/// What [`tidy`] changed.
#[derive(Debug, Default, PartialEq)]
pub struct Tidied {
    pub repeats: usize,
    pub fixed: Vec<(String, String)>,
}

/// Repeated phrases kept once and misheard dictionary words put right, on
/// every line.
pub fn tidy(lines: &mut [Segment], vocabulary: &Vocabulary) -> Tidied {
    let mut done = Tidied::default();
    let mut seen = Seen::default();
    for s in lines.iter_mut() {
        let (text, runs) = collapse_repeats(&s.text);
        done.repeats += runs;
        let (text, fixed) = vocabulary.fix(&text, &mut seen);
        done.fixed.extend(fixed);
        s.text = text;
    }
    if done.repeats > 0 || !done.fixed.is_empty() {
        log::info!(
            "Notetaker tidied the transcript: {} repeats, {} words put right",
            done.repeats,
            done.fixed.len()
        );
    }
    done
}

#[cfg(test)]
mod tests {
    use super::*;

    fn words(list: &[&str]) -> Vec<String> {
        list.iter().map(|w| w.to_string()).collect()
    }

    #[test]
    fn a_long_phrase_said_three_times_is_kept_once() {
        assert_eq!(
            collapse_repeats(
                "Clear ho gaya yeh wala cheez yeh wala cheez yeh wala cheez ki evidence"
            ),
            ("Clear ho gaya yeh wala cheez ki evidence".to_string(), 1)
        );
        assert_eq!(
            collapse_repeats("main bhi to main bhi to main bhi to, iske liye"),
            ("main bhi to, iske liye".to_string(), 1)
        );
        assert_eq!(
            collapse_repeats("Haan haan haan haan haan haan haan haan, chalo"),
            ("Haan, chalo".to_string(), 1)
        );
    }

    #[test]
    fn ordinary_repeats_stay() {
        for line in [
            "Nahi nahi nahi nahi yeh vo locking wala tha",
            "One. One. One. One. One. One.",
            "ek do teen ek do teen ek do teen",
            "left right left right left right left right",
            "no no no no no no",
            "haan haan haan haan haan",
            "Conflict ho gaya. Conflict ho gaya. Lekin do tarah ke conflict hain.",
            "I can do this. I can do that. I can do this. I can do that.",
        ] {
            assert_eq!(collapse_repeats(line), (line.to_string(), 0), "{line}");
        }
    }

    #[test]
    fn stock_phrases_are_whole_lines_only() {
        assert!(stock_phrase("So, thank you."));
        assert!(stock_phrase("Thank you for watching!"));
        assert!(stock_phrase("I'm gonna go to the phone."));
        assert!(!stock_phrase("So, thank you for the update on Zorvex."));
        assert!(!stock_phrase("Okay, so ahead."));
    }

    fn piece(speech_ms: u64, peak: f32) -> Piece {
        Piece {
            start_sample: 0,
            audio: vec![0.0; 32_000],
            speech_samples: speech_ms * 16,
            peak,
        }
    }

    #[test]
    fn a_stock_phrase_goes_only_when_the_detector_heard_no_speech() {
        assert!(heard_nothing(
            "So, thank you.",
            &piece(260, 0.55),
            true,
            &[]
        ));
        assert!(!heard_nothing("Thank you.", &piece(600, 0.95), true, &[]));
        assert!(!heard_nothing("Bye.", &piece(300, 0.9), true, &[]));
        assert!(
            !heard_nothing("So, thank you.", &piece(260, 0.55), false, &[]),
            "loudness alone never drops a line"
        );
        assert!(!heard_nothing("So we start.", &piece(260, 0.55), true, &[]));
    }

    #[test]
    fn a_read_out_of_the_hints_goes_only_when_hints_were_given() {
        let hints = words(&["Brimvale", "Kaychorn", "Search Grid", "Varun"]);
        let quiet = piece(500, 0.6);
        assert!(heard_nothing(
            "Brimvale Kaychorn Search Grid.",
            &quiet,
            true,
            &hints
        ));
        assert!(!heard_nothing(
            "Brimvale Kaychorn Search Grid.",
            &quiet,
            true,
            &[]
        ));
        assert!(!read_out("Brimvale Brimvale Kaychorn", &hints));
        assert!(!read_out("Brimvale Kaychorn", &hints));
        assert!(!read_out("Brimvale ka Kaychorn aur Varun", &hints));
        assert!(!heard_nothing(
            "Brimvale Kaychorn Varun",
            &piece(3_000, 0.99),
            true,
            &hints
        ));
    }

    fn vocab() -> Vocabulary {
        Vocabulary::new(
            &words(&[
                "Brimvale",
                "Plivora",
                "Login",
                "ZoomInfo",
                "SearchAPI",
                "Kaychorn",
                "Qwen",
                "LLM",
                "taxonomy",
                "vendor",
                "tenant",
                "flowchart",
                "Zentrix API",
                "POC",
                "Gourav",
                "a@b.co",
                "AI",
            ]),
            &words(&["Varun Mehta"]),
        )
    }

    fn fix(text: &str) -> String {
        vocab().fix(text, &mut Seen::default()).0
    }

    #[test]
    fn misheard_terms_are_put_right() {
        assert_eq!(fix("ye flushart banana hai"), "ye flowchart banana hai");
        assert_eq!(fix("hamare textonimi ke saath"), "hamare taxonomy ke saath");
        assert_eq!(
            fix("har vendor ka tendenti hai"),
            "har vendor ka tenant hai"
        );
    }

    #[test]
    fn a_lowercase_word_never_becomes_a_person_name() {
        for text in ["gorav ne bola", "desh ka gourab", "varun ko bolo"] {
            assert_eq!(fix(text), text, "{text}");
        }
        let names = Vocabulary::new(&words(&["Tavish"]), &words(&["Tavish Rao"]));
        assert_eq!(
            names.fix("tavis ne bola", &mut Seen::default()).0,
            "tavis ne bola"
        );
        assert_eq!(fix("kal poce kar raha tha"), "kal POC kar raha tha");
        assert_eq!(fix("kechorn wala data"), "Kaychorn wala data");
        assert_eq!(fix("pli vora ka data"), "Plivora ka data");
        for text in ["ye zentrix api dono", "kal Quinn se baat hui"] {
            assert_eq!(fix(text), text, "{text}");
        }
        assert_eq!(
            collapse_repeats("theek hai theek hai theek hai theek hai theek hai."),
            ("theek hai.".to_string(), 1)
        );
    }

    #[test]
    fn a_brand_spelt_exactly_as_two_words_is_joined() {
        assert_eq!(
            fix("ye Zoom Info aur search API dono"),
            "ye ZoomInfo aur SearchAPI dono"
        );
        assert_eq!(fix("ek P OC chahiye"), "ek POC chahiye");
        assert_eq!(fix("ye Zoom info hai"), "ye ZoomInfo hai");
        for text in [
            "ye zoom info hai",
            "we search api docs",
            "we Zoo mInfo",
            "ek p oc",
        ] {
            assert_eq!(fix(text), text);
        }
        let linked = Vocabulary::new(&words(&["LinkedIn"]), &[]);
        assert_eq!(
            linked.fix("we linked in the doc", &mut Seen::default()).0,
            "we linked in the doc"
        );
        let plain = Vocabulary::new(&words(&["Keychain", "Login"]), &[]);
        for text in ["the key chain is lost", "please log in"] {
            assert_eq!(plain.fix(text, &mut Seen::default()).0, text);
        }
    }

    #[test]
    fn real_words_and_names_stay() {
        for text in [
            "bandar ne kela khaya",
            "maine pochha tha",
            "Lipika ne bola, Trina bhi aayi",
            "Saurav aur saurav dono",
            "kal Quinn se baat hui",
            "Adam ko pas kiya, kaun bolega",
            "kitchen mein coin gira, queen ne dekha",
            "kisi ne bola tha ki bas ho gaya",
            "Varun ka vendor",
            "LLM aur AI dono",
            "teenon ne bola, zamini par",
            "ye blocker hai",
            "kaychon wala data",
            "desh ka gaurav hai, Gaurav bola",
            "log in karo, key chain lao",
        ] {
            assert_eq!(fix(text), text, "{text}");
        }
    }

    #[test]
    fn a_capital_at_the_start_of_a_sentence_needs_a_close_match_to_a_brand() {
        assert_eq!(fix("Kechorn wala data"), "Kaychorn wala data");
        assert_eq!(fix("Flushart banana hai"), "Flushart banana hai");
        assert_eq!(fix("ok. Kechorn wala"), "ok. Kaychorn wala");
    }

    #[test]
    fn tidy_reports_what_it_changed() {
        let line = |text: &str| Segment {
            source: "mic".into(),
            speaker: "Me".into(),
            speaker_key: "me".into(),
            start_ms: 0,
            end_ms: 2_000,
            text: text.into(),
        };
        let mut lines = vec![line("main bhi to main bhi to main bhi to flushart dekho")];
        let done = tidy(&mut lines, &vocab());
        assert_eq!(lines[0].text, "main bhi to flowchart dekho");
        assert_eq!(done.repeats, 1);
        assert_eq!(
            done.fixed,
            vec![("flushart".to_string(), "flowchart".to_string())]
        );
        let mut lines = vec![line("flushart dekho")];
        let none = Vocabulary::new(&[], &[]);
        assert_eq!(tidy(&mut lines, &none), Tidied::default());
    }

    fn letters_for(mut n: usize) -> String {
        let mut out = String::new();
        loop {
            out.push((b'a' + (n % 26) as u8) as char);
            n /= 26;
            if n == 0 {
                return out;
            }
        }
    }

    /// cargo test --release --lib tidy_speed -- --ignored
    #[test]
    #[ignore]
    fn tidy_speed() {
        let dictionary: Vec<String> = (0..300)
            .map(|n| {
                format!(
                    "Zorbex{}",
                    ["ta", "vo", "mi", "ka", "ru"][n % 5].repeat(1 + n % 3)
                )
            })
            .collect();
        let vocabulary = Vocabulary::new(&dictionary, &[]);
        let line = |n: usize| Segment {
            source: "mic".into(),
            speaker: "Me".into(),
            speaker_key: "me".into(),
            start_ms: n as i64,
            end_ms: n as i64 + 1,
            text: format!(
                "matlab bandar wala flushart kal dekhenge plorvi{} ke saath haan {} theek hai",
                letters_for(n % 700),
                letters_for(n % 400 + 700)
            ),
        };
        let mut lines: Vec<Segment> = (0..3_000).map(line).collect();
        let started = std::time::Instant::now();
        tidy(&mut lines, &vocabulary);
        let took = started.elapsed();
        println!("tidy of 3000 lines with 300 words: {took:?}");
        assert!(took < std::time::Duration::from_secs(1));
    }
}
