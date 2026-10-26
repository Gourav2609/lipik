//! The "auto" engine's choice between the local model's text and Apple's.
//!
//! The local model (Parakeet) only knows English, so on Hindi speech it writes
//! made-up English-looking words ("Kalki meeting cancel cardo, Rahulko
//! Batadina"). Its text is kept when it reads as English:
//!
//! - it is not empty,
//! - none of its words is a common Hindi word written in Roman letters
//!   ("hai", "ki", "nahi", ...); a word that is also English ("mere", "se")
//!   only counts when the text has two of them, and
//! - at least 75% of its words are common English words or the user's own
//!   dictionary words.
//!
//! Otherwise Apple's text is used. Acronyms (all caps, up to 5 letters) and
//! words with digits are not counted. On the 160-clip benchmark this kept all
//! 80 English clips and switched 75 of 80 Hinglish ones.

use crate::textflow::hindi::{ALSO_ENGLISH, WORDS as HINDI_WORDS};
use std::collections::HashSet;
use std::sync::LazyLock;

const COMMON_WORDS: &str = include_str!("../textflow/common_words.txt");

const MIN_ENGLISH_SHARE: f64 = 0.75;

static ENGLISH: LazyLock<HashSet<&'static str>> = LazyLock::new(|| {
    COMMON_WORDS
        .lines()
        .filter(|l| !l.is_empty() && !l.starts_with('#'))
        .collect()
});

/// True when the local model's text should be kept rather than Apple's.
pub fn local_text_looks_english(text: &str, custom_words: &[String]) -> bool {
    let custom: HashSet<String> = custom_words
        .iter()
        .flat_map(|w| w.split_whitespace())
        .map(str::to_lowercase)
        .collect();
    let mut counted = 0usize;
    let mut english = 0usize;
    let mut shared = 0usize;
    for raw in text.split_whitespace() {
        let word = raw
            .trim_matches(|c: char| !c.is_alphanumeric() && c != '\'' && c != '’')
            .replace('’', "'");
        if word.is_empty() || word.chars().any(|c| c.is_ascii_digit()) {
            continue;
        }
        if word.len() <= 5 && word.chars().all(|c| c.is_ascii_uppercase()) {
            continue;
        }
        let lower = word.to_lowercase();
        let lower = lower
            .strip_suffix("'s")
            .unwrap_or(&lower)
            .trim_matches('\'');
        if lower.is_empty() {
            continue;
        }
        if ALSO_ENGLISH.contains(&lower) {
            shared += 1;
            if shared >= 2 {
                return false;
            }
        } else if HINDI_WORDS.contains(&lower) {
            return false;
        }
        counted += 1;
        let known = lower
            .split('-')
            .filter(|p| !p.is_empty())
            .all(|p| ENGLISH.contains(p) || custom.contains(p));
        if known || custom.contains(lower) {
            english += 1;
        }
    }
    counted > 0 && english as f64 >= MIN_ENGLISH_SHARE * counted as f64
}

#[cfg(test)]
mod tests {
    use super::local_text_looks_english;

    fn english(text: &str) -> bool {
        local_text_looks_english(text, &[])
    }

    #[test]
    fn keeps_english_with_names_and_acronyms() {
        assert!(english("Priya, can you review my PR before the stand-up?"));
        assert!(english(
            "Rahul pushed the fix to staging, please check the JIRA ticket."
        ));
        assert!(english("Rohit will rotate the API keys before the audit."));
        assert!(english("Schedule the review for 15th March at 3 p.m."));
        assert!(english(
            "Let's sync on Slack about the onboarding flow tomorrow."
        ));
    }

    #[test]
    fn switches_on_hindi_function_words() {
        assert!(!english("Kal 11 baji meeting hai, 5 log is 1."));
        assert!(!english("Ankit, Jir ticket abhitak at nahi huhe."));
        assert!(!english("Rohit Bhai, Apka Code Review Ho Gaya Hei."));
    }

    #[test]
    fn one_word_shared_with_english_is_not_enough() {
        assert!(english("It was a mere formality, nothing more."));
        assert!(english("The price per se is fine, ship it."));
        assert!(!english("Agar tum kal free ho toh call karo."));
        assert!(!english("Mere se baat karo"));
    }

    #[test]
    fn switches_on_made_up_words() {
        assert!(!english("Kalki meeting Kansal Kadu, Rahulko Bata Dena."));
        assert!(!english("Priya, PR review karki merge cardina please"));
        assert!(!english("Thud Jol Dikaro, release ajai nikal nahai."));
    }

    #[test]
    fn empty_text_is_not_kept() {
        assert!(!english(""));
        assert!(!english("  ...  "));
    }

    #[test]
    fn dictionary_words_count_as_english() {
        let text = "Karthik, ping Vikram about the Zorblax rollout.";
        assert!(!english(text));
        let words = [
            "Karthik".to_string(),
            "Zorblax".to_string(),
            "Vikram".to_string(),
        ];
        assert!(local_text_looks_english(text, &words));
    }
}
