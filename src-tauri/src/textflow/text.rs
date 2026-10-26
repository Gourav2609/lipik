//! Pure text helpers for the dictation pipeline: phrase matching on word
//! tokens (case-insensitive, punctuation-tolerant), word counts and word diffs.
//! Phrases with symbols such as "C#" or ".NET" match only their exact
//! characters, so they never match a bare "c" or "net".

struct Token {
    start: usize,
    end: usize,
    lower: String,
}

fn is_apostrophe(c: char) -> bool {
    c == '\'' || c == '’'
}

fn tokens(text: &str) -> Vec<Token> {
    let chars: Vec<(usize, char)> = text.char_indices().collect();
    let is_word = |i: usize| {
        let c = chars[i].1;
        c.is_alphanumeric()
            || (is_apostrophe(c)
                && i > 0
                && chars[i - 1].1.is_alphanumeric()
                && i + 1 < chars.len()
                && chars[i + 1].1.is_alphanumeric())
    };
    let mut out = Vec::new();
    let mut i = 0;
    while i < chars.len() {
        if !is_word(i) {
            i += 1;
            continue;
        }
        let start = chars[i].0;
        while i < chars.len() && is_word(i) {
            i += 1;
        }
        let end = chars.get(i).map_or(text.len(), |c| c.0);
        let lower = text[start..end].to_lowercase().replace('’', "'");
        out.push(Token { start, end, lower });
    }
    out
}

/// Lowercased word tokens of a phrase, as used for matching.
pub fn phrase_words(phrase: &str) -> Vec<String> {
    tokens(phrase).into_iter().map(|t| t.lower).collect()
}

/// Whether `phrase` holds a symbol that word matching would drop. Commas and
/// end punctuation don't count, nor a `.`, `-` or `/` between two letters or
/// digits ("Node.js", "e-mail").
pub fn has_symbols(phrase: &str) -> bool {
    let chars: Vec<char> = phrase
        .trim()
        .trim_end_matches(['.', ',', '!', '?', ';', ':'])
        .chars()
        .collect();
    chars.iter().enumerate().any(|(i, &c)| {
        if c.is_alphanumeric() || c.is_whitespace() || is_apostrophe(c) {
            return false;
        }
        if matches!(c, ',' | '!' | '?' | ';' | ':') {
            return false;
        }
        let between = i > 0
            && i + 1 < chars.len()
            && chars[i - 1].is_alphanumeric()
            && chars[i + 1].is_alphanumeric();
        !(matches!(c, '.' | '-' | '/') && between)
    })
}

/// A phrase ready for matching.
pub enum Pattern {
    Words(Vec<String>),
    /// Exact characters, ignoring case; whitespace matches any whitespace.
    Literal(Vec<char>),
}

impl Pattern {
    pub fn new(phrase: &str) -> Self {
        if has_symbols(phrase) {
            Pattern::Literal(phrase.trim().chars().collect())
        } else {
            Pattern::Words(phrase_words(phrase))
        }
    }
}

struct Hit {
    start: usize,
    end: usize,
    rule: usize,
}

fn same_char(a: char, b: char) -> bool {
    a == b || a.to_lowercase().eq(b.to_lowercase())
}

/// Byte end of `pattern` matched at char index `i`, if it matches there.
fn literal_at(chars: &[(usize, char)], text_len: usize, i: usize, pattern: &[char]) -> Option<usize> {
    let mut j = i;
    let mut p = 0;
    while p < pattern.len() {
        let (_, c) = *chars.get(j)?;
        if pattern[p].is_whitespace() {
            if !c.is_whitespace() {
                return None;
            }
            while p < pattern.len() && pattern[p].is_whitespace() {
                p += 1;
            }
            while chars.get(j).is_some_and(|(_, c)| c.is_whitespace()) {
                j += 1;
            }
        } else {
            if !same_char(pattern[p], c) {
                return None;
            }
            p += 1;
            j += 1;
        }
    }
    if chars.get(j).is_some_and(|(_, c)| c.is_alphanumeric()) {
        return None;
    }
    Some(chars.get(j).map_or(text_len, |c| c.0))
}

/// Non-overlapping matches, left to right, preferring the longest phrase at
/// each position.
fn find_hits(text: &str, patterns: &[Pattern]) -> Vec<Hit> {
    let text_tokens = tokens(text);
    let text_chars: Vec<(usize, char)> = text.char_indices().collect();
    let mut candidates = Vec::new();
    for (rule, pattern) in patterns.iter().enumerate() {
        match pattern {
            Pattern::Words(words) if !words.is_empty() => {
                for i in 0..text_tokens.len() {
                    let fits = i + words.len() <= text_tokens.len()
                        && words.iter().zip(&text_tokens[i..]).all(|(w, t)| *w == t.lower);
                    if fits {
                        candidates.push(Hit {
                            start: text_tokens[i].start,
                            end: text_tokens[i + words.len() - 1].end,
                            rule,
                        });
                    }
                }
            }
            Pattern::Literal(chars) if !chars.is_empty() => {
                for i in 0..text_chars.len() {
                    if i > 0 && text_chars[i - 1].1.is_alphanumeric() {
                        continue;
                    }
                    if let Some(end) = literal_at(&text_chars, text.len(), i, chars) {
                        candidates.push(Hit {
                            start: text_chars[i].0,
                            end,
                            rule,
                        });
                    }
                }
            }
            _ => {}
        }
    }
    candidates.sort_by_key(|h| (h.start, std::cmp::Reverse(h.end)));
    let mut hits: Vec<Hit> = Vec::new();
    for hit in candidates {
        if hits.last().map_or(true, |last| hit.start >= last.end) {
            hits.push(hit);
        }
    }
    hits
}

fn capitalize_like(original: &str, replacement: &str) -> String {
    let starts_upper = original.chars().next().is_some_and(char::is_uppercase);
    let mut chars = replacement.chars();
    match chars.next() {
        Some(first) if starts_upper && first.is_lowercase() => {
            first.to_uppercase().chain(chars).collect()
        }
        _ => replacement.to_string(),
    }
}

/// Replaces whole-word occurrences of each `(from, to)` pair, ignoring case.
/// A match at a capitalised position keeps the capital. Returns the new text
/// and how many replacements were made.
pub fn apply_replacements(text: &str, pairs: &[(&str, &str)]) -> (String, usize) {
    if pairs.is_empty() {
        return (text.to_string(), 0);
    }
    let patterns: Vec<Pattern> = pairs.iter().map(|(from, _)| Pattern::new(from)).collect();
    let hits = find_hits(text, &patterns);
    let mut out = String::with_capacity(text.len());
    let mut last = 0;
    for hit in &hits {
        out.push_str(&text[last..hit.start]);
        out.push_str(&capitalize_like(&text[hit.start..hit.end], pairs[hit.rule].1));
        last = hit.end;
    }
    out.push_str(&text[last..]);
    (out, hits.len())
}

/// Replaces each spoken trigger phrase with its expansion, verbatim. Commas or
/// periods the transcriber put between the trigger's words don't stop a
/// match. When the dictation is nothing but one trigger, only the expansion
/// is returned (no trailing period from the transcriber).
pub fn apply_snippets(text: &str, snippets: &[(&str, &str)]) -> (String, usize) {
    if snippets.is_empty() {
        return (text.to_string(), 0);
    }
    let patterns: Vec<Pattern> = snippets
        .iter()
        .map(|(trigger, _)| Pattern::new(trigger))
        .collect();
    let hits = find_hits(text, &patterns);
    if let [hit] = hits.as_slice() {
        let rest = format!("{}{}", &text[..hit.start], &text[hit.end..]);
        if !rest.chars().any(char::is_alphanumeric) {
            return (snippets[hit.rule].1.to_string(), 1);
        }
    }
    let mut out = String::with_capacity(text.len());
    let mut last = 0;
    for hit in &hits {
        out.push_str(&text[last..hit.start]);
        out.push_str(snippets[hit.rule].1);
        last = hit.end;
    }
    out.push_str(&text[last..]);
    (out, hits.len())
}

/// How many times any of `phrases` occurs in `text` as whole words.
pub fn count_phrases(text: &str, phrases: &[Pattern]) -> usize {
    if phrases.is_empty() {
        return 0;
    }
    find_hits(text, phrases).len()
}

pub fn count_words(text: &str) -> usize {
    text.split_whitespace().count()
}

/// Word-level edit distance (insertions, deletions and substitutions of
/// whitespace-separated words).
pub fn word_diff(a: &str, b: &str) -> usize {
    if a == b {
        return 0;
    }
    let a: Vec<&str> = a.split_whitespace().collect();
    let b: Vec<&str> = b.split_whitespace().collect();
    if a.len().saturating_mul(b.len()) > 4_000_000 {
        return a.len().abs_diff(b.len());
    }
    let mut prev: Vec<usize> = (0..=b.len()).collect();
    let mut cur = vec![0; b.len() + 1];
    for (i, wa) in a.iter().enumerate() {
        cur[0] = i + 1;
        for (j, wb) in b.iter().enumerate() {
            let cost = usize::from(wa != wb);
            cur[j + 1] = (prev[j] + cost).min(prev[j + 1] + 1).min(cur[j] + 1);
        }
        std::mem::swap(&mut prev, &mut cur);
    }
    prev[b.len()]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn replacement_is_case_insensitive_and_whole_word() {
        let (out, n) = apply_replacements("Btw the btwx is here, btw.", &[("btw", "by the way")]);
        assert_eq!(out, "By the way the btwx is here, by the way.");
        assert_eq!(n, 2);
    }

    #[test]
    fn multi_word_replacement() {
        let (out, _) = apply_replacements("ask open AI now", &[("open ai", "OpenAI")]);
        assert_eq!(out, "ask OpenAI now");
    }

    #[test]
    fn snippet_tolerates_punctuation_between_words() {
        let snippets = [("my email", "me@example.com")];
        let (out, n) = apply_snippets("Send it to my, email please.", &snippets);
        assert_eq!(out, "Send it to me@example.com please.");
        assert_eq!(n, 1);
    }

    #[test]
    fn whole_dictation_snippet_drops_trailing_period() {
        let snippets = [("my address", "1 Main St\nSpringfield")];
        let (out, _) = apply_snippets("My address.", &snippets);
        assert_eq!(out, "1 Main St\nSpringfield");
    }

    #[test]
    fn longest_snippet_wins() {
        let snippets = [("my address", "short"), ("my home address", "long")];
        let (out, _) = apply_snippets("here is my home address ok", &snippets);
        assert_eq!(out, "here is long ok");
    }

    #[test]
    fn apostrophes_stay_inside_words() {
        let (out, _) = apply_replacements("don't 'dont'", &[("dont", "do not")]);
        assert_eq!(out, "don't 'do not'");
    }

    #[test]
    fn counts_dictionary_phrases() {
        let phrases = vec![Pattern::new("Lipik"), Pattern::new("Priya Raman")];
        assert_eq!(count_phrases("lipik wrote to Priya Raman. Lipik!", &phrases), 3);
    }

    #[test]
    fn word_diff_counts_word_edits() {
        assert_eq!(word_diff("um so I think", "So I think"), 2);
        assert_eq!(word_diff("same text", "same text"), 0);
        assert_eq!(word_diff("", "two words"), 2);
    }

    #[test]
    fn symbol_phrases_match_only_their_exact_characters() {
        let pairs = [("C#", "C sharp"), (".NET", "dotnet")];
        let (out, n) = apply_replacements("c and C# on .net, not ASP.NET or net", &pairs);
        assert_eq!(out, "c and C sharp on dotnet, not ASP.NET or net");
        assert_eq!(n, 2);
        assert!(has_symbols("C++") && has_symbols(".NET"));
        assert!(!has_symbols("Node.js") && !has_symbols("e-mail") && !has_symbols("btw."));
    }

    #[test]
    fn unicode_text_keeps_byte_boundaries() {
        let (out, n) = apply_replacements("नमस्ते btw दोस्त", &[("btw", "by the way")]);
        assert_eq!(out, "नमस्ते by the way दोस्त");
        assert_eq!(n, 1);
    }
}
