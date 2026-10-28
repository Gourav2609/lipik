use super::insights::{self, Insights};
use super::notes::{Note, Notes};
use super::store::{
    default_transforms, new_id, AutoApply, Data, Dictionary, DictionaryWord, PendingFix,
    Replacement, Snippet, StyleCategory, StyleKind, StyleSettings, TextFlow, Transform,
    TransformSettings,
};
use super::{menu, text, transforms};
use crate::managers::history::HistoryManager;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Duration;
use tauri::{AppHandle, Emitter, Manager, State};

fn changed(app: &AppHandle, kind: &str) {
    let _ = app.emit("textflow-changed", kind);
}

fn required(value: &str, what: &str) -> Result<String, String> {
    let value = value.trim();
    if text::phrase_words(value).is_empty() && !text::has_symbols(value) {
        return Err(format!("{what} can't be empty"));
    }
    Ok(value.to_string())
}

fn same(a: &str, b: &str) -> bool {
    if text::has_symbols(a) || text::has_symbols(b) {
        let exact = |s: &str| {
            s.split_whitespace()
                .collect::<Vec<_>>()
                .join(" ")
                .to_lowercase()
        };
        return exact(a) == exact(b);
    }
    text::phrase_words(a) == text::phrase_words(b)
}

static SYNC_GENERATION: AtomicU64 = AtomicU64::new(0);

/// Copies the dictionary words into Lipik's `custom_words`. Waits a moment
/// first so an import of many words rewrites the settings once.
fn sync_custom_words(app: &AppHandle) {
    let generation = SYNC_GENERATION.fetch_add(1, Ordering::SeqCst) + 1;
    let app = app.clone();
    std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(300));
        if SYNC_GENERATION.load(Ordering::SeqCst) != generation {
            return;
        }
        let Some(tf) = app.try_state::<TextFlow>() else {
            return;
        };
        let words = tf.read(|d| d.dictionary.words.iter().map(|w| w.word.clone()).collect());
        let mut settings = crate::settings::get_settings(&app);
        settings.custom_words = words;
        crate::settings::write_settings(&app, settings);
    });
}

fn update_dictionary(
    app: &AppHandle,
    tf: &TextFlow,
    f: impl FnOnce(&mut Dictionary) -> Result<(), String>,
) -> Result<Dictionary, String> {
    let dictionary = tf.update(|d| {
        f(&mut d.dictionary)?;
        Ok(d.dictionary.clone())
    })?;
    sync_custom_words(app);
    changed(app, "dictionary");
    Ok(dictionary)
}

#[tauri::command]
#[specta::specta]
pub fn get_dictionary(tf: State<'_, TextFlow>) -> Dictionary {
    tf.read(|d| d.dictionary.clone())
}

#[tauri::command]
#[specta::specta]
pub fn add_dictionary_word(
    app: AppHandle,
    tf: State<'_, TextFlow>,
    word: String,
) -> Result<Dictionary, String> {
    let word = required(&word, "Word")?;
    update_dictionary(&app, &tf, |d| {
        if d.words.iter().any(|w| same(&w.word, &word)) {
            return Err(format!("\"{word}\" is already in your dictionary"));
        }
        d.words.insert(
            0,
            DictionaryWord {
                id: new_id(),
                word,
                starred: false,
                auto_learned: false,
                shared: false,
            },
        );
        Ok(())
    })
}

#[tauri::command]
#[specta::specta]
pub fn update_dictionary_word(
    app: AppHandle,
    tf: State<'_, TextFlow>,
    id: String,
    word: String,
    starred: bool,
) -> Result<Dictionary, String> {
    let word = required(&word, "Word")?;
    update_dictionary(&app, &tf, |d| {
        if d.words.iter().any(|w| w.id != id && same(&w.word, &word)) {
            return Err(format!("\"{word}\" is already in your dictionary"));
        }
        let entry = d
            .words
            .iter_mut()
            .find(|w| w.id == id)
            .ok_or("Word not found")?;
        if entry.word != word {
            entry.auto_learned = false;
        }
        entry.word = word;
        entry.starred = starred;
        Ok(())
    })
}

#[tauri::command]
#[specta::specta]
pub fn delete_dictionary_word(
    app: AppHandle,
    tf: State<'_, TextFlow>,
    id: String,
) -> Result<Dictionary, String> {
    update_dictionary(&app, &tf, |d| {
        d.words.retain(|w| w.id != id);
        Ok(())
    })
}

#[tauri::command]
#[specta::specta]
pub fn add_replacement(
    app: AppHandle,
    tf: State<'_, TextFlow>,
    from: String,
    to: String,
) -> Result<Dictionary, String> {
    let from = required(&from, "The word to replace")?;
    let to = to.trim().to_string();
    update_dictionary(&app, &tf, |d| {
        if d.replacements.iter().any(|r| same(&r.from, &from)) {
            return Err(format!("\"{from}\" already has a replacement"));
        }
        d.replacements.insert(
            0,
            Replacement {
                id: new_id(),
                from,
                to,
                auto_learned: false,
            },
        );
        Ok(())
    })
}

#[tauri::command]
#[specta::specta]
pub fn update_replacement(
    app: AppHandle,
    tf: State<'_, TextFlow>,
    id: String,
    from: String,
    to: String,
) -> Result<Dictionary, String> {
    let from = required(&from, "The word to replace")?;
    let to = to.trim().to_string();
    update_dictionary(&app, &tf, |d| {
        if d.replacements
            .iter()
            .any(|r| r.id != id && same(&r.from, &from))
        {
            return Err(format!("\"{from}\" already has a replacement"));
        }
        let entry = d
            .replacements
            .iter_mut()
            .find(|r| r.id == id)
            .ok_or("Replacement not found")?;
        if entry.from != from || entry.to != to {
            entry.auto_learned = false;
        }
        entry.from = from;
        entry.to = to;
        Ok(())
    })
}

#[tauri::command]
#[specta::specta]
pub fn delete_replacement(
    app: AppHandle,
    tf: State<'_, TextFlow>,
    id: String,
) -> Result<Dictionary, String> {
    update_dictionary(&app, &tf, |d| {
        d.replacements.retain(|r| r.id != id);
        Ok(())
    })
}

const COMMON_WORDS: &str = include_str!("common_words.txt");
const MAX_LEARNED_WORDS: usize = 5;
const MAX_PENDING_FIXES: usize = 200;
const MAX_DIFF_CELLS: usize = 4_000_000;

pub(super) fn is_common_word(word: &str) -> bool {
    let lower = word.to_lowercase().replace('’', "'");
    let parts: Vec<&str> = lower.split(['-', '/']).filter(|p| !p.is_empty()).collect();
    !parts.is_empty() && parts.iter().all(|p| COMMON_WORDS.lines().any(|l| l == *p))
}

/// A word worth adding to the dictionary: a name, jargon or a non-English
/// (e.g. Hinglish) word, never a common English word, number, URL or email.
/// Words of 3 letters or fewer only as acronyms: the fuzzy word match would
/// turn English words like "hi" into a short learned word like "hai".
fn learnable(word: &str) -> Option<&str> {
    let word = word
        .strip_suffix("'s")
        .or_else(|| word.strip_suffix("’s"))
        .unwrap_or(word);
    let len = word.chars().count();
    let first = word.chars().next()?;
    let acronym = !word.chars().any(char::is_lowercase);
    let usable = (2..=40).contains(&len)
        && (len > 3 || acronym)
        && word.chars().any(char::is_alphabetic)
        && !first.is_ascii_digit()
        && !word.contains('@')
        && !word.contains("://")
        && !word.to_lowercase().starts_with("www.");
    (usable && !is_common_word(word)).then_some(word)
}

/// The words of a transcript without the punctuation around them. A leading
/// "." and trailing "#" or "+" stay, so ".NET", "C#" and "C++" survive.
fn correction_words(text: &str) -> Vec<&str> {
    text.split_whitespace()
        .map(|w| {
            w.trim_start_matches(['"', '\'', '(', '[', '{', '“', '‘', '¿', '¡'])
                .trim_end_matches([
                    '.', ',', '!', '?', ';', ':', '"', '\'', ')', ']', '}', '”', '’', '…',
                ])
        })
        .filter(|w| !w.is_empty())
        .collect()
}

/// Spans where `after` replaced words of `before` (removed, added), from a
/// word-level diff. Empty for a rewrite: more than half of an 8+ word text changed.
fn replaced_spans<'a>(before: &[&'a str], after: &[&'a str]) -> Vec<(Vec<&'a str>, Vec<&'a str>)> {
    let (n, m) = (before.len(), after.len());
    if n == 0 || m == 0 || (n + 1) * (m + 1) > MAX_DIFF_CELLS {
        return Vec::new();
    }
    let mut lcs = vec![vec![0u32; m + 1]; n + 1];
    for i in (0..n).rev() {
        for j in (0..m).rev() {
            lcs[i][j] = if before[i] == after[j] {
                lcs[i + 1][j + 1] + 1
            } else {
                lcs[i + 1][j].max(lcs[i][j + 1])
            };
        }
    }
    let mut spans = Vec::new();
    let (mut removed, mut added) = (Vec::new(), Vec::new());
    let mut changed = 0;
    let (mut i, mut j) = (0, 0);
    while i < n || j < m {
        if i < n && j < m && before[i] == after[j] {
            if !removed.is_empty() || !added.is_empty() {
                spans.push((std::mem::take(&mut removed), std::mem::take(&mut added)));
            }
            i += 1;
            j += 1;
        } else if j < m && (i == n || lcs[i][j + 1] >= lcs[i + 1][j]) {
            added.push(after[j]);
            j += 1;
        } else {
            removed.push(before[i]);
            changed += 1;
            i += 1;
        }
    }
    if !removed.is_empty() || !added.is_empty() {
        spans.push((removed, added));
    }
    if n >= 8 && changed * 2 > n {
        return Vec::new();
    }
    spans
        .into_iter()
        .filter(|(r, a)| (1..=4).contains(&r.len()) && (1..=3).contains(&a.len()))
        .collect()
}

#[derive(Default)]
pub struct Learned {
    pub words: Vec<String>,
    pub replacements: Vec<(String, String)>,
}

fn learn(data: &mut Data, entry_id: i64, before: &str, after: &str) -> Learned {
    let mut learned = Learned::default();
    let spans = replaced_spans(&correction_words(before), &correction_words(after));
    for (removed, added) in spans {
        let new_words: Vec<&str> = added.iter().filter_map(|w| learnable(w)).collect();
        if new_words.is_empty() {
            continue;
        }
        for word in new_words {
            let known = data.dictionary.words.iter().any(|w| same(&w.word, word))
                || learned.words.iter().any(|w| same(w, word));
            if !known && learned.words.len() < MAX_LEARNED_WORDS {
                learned.words.push(word.to_string());
            }
        }

        if removed.len() == 1 && is_common_word(removed[0]) {
            continue;
        }
        let from = removed.join(" ");
        let to = added.join(" ");
        if data
            .dictionary
            .replacements
            .iter()
            .any(|r| same(&r.from, &from))
        {
            continue;
        }
        let pending = match data
            .pending_fixes
            .iter()
            .position(|p| same(&p.from, &from) && p.to == to)
        {
            Some(i) => &mut data.pending_fixes[i],
            None => {
                data.pending_fixes.push(PendingFix {
                    from: from.clone(),
                    to: to.clone(),
                    entry_ids: Vec::new(),
                });
                data.pending_fixes.last_mut().unwrap()
            }
        };
        if !pending.entry_ids.contains(&entry_id) {
            pending.entry_ids.push(entry_id);
        }
        if pending.entry_ids.len() >= 2 {
            data.pending_fixes
                .retain(|p| !(same(&p.from, &from) && p.to == to));
            learned.replacements.push((from, to));
        }
    }
    let overflow = data.pending_fixes.len().saturating_sub(MAX_PENDING_FIXES);
    data.pending_fixes.drain(..overflow);

    for word in learned.words.iter().rev() {
        data.dictionary.words.insert(
            0,
            DictionaryWord {
                id: new_id(),
                word: word.clone(),
                starred: false,
                auto_learned: true,
                shared: false,
            },
        );
    }
    for (from, to) in &learned.replacements {
        data.dictionary.replacements.insert(
            0,
            Replacement {
                id: new_id(),
                from: from.clone(),
                to: to.clone(),
                auto_learned: true,
            },
        );
    }
    learned
}

/// Learns from a transcript the user corrected in History: new spellings
/// become dictionary words, and a fix seen in two dictations becomes a
/// replacement. Logs counts only, never the words.
pub fn learn_from_correction(app: &AppHandle, entry_id: i64, before: &str, after: &str) -> Learned {
    if replaced_spans(&correction_words(before), &correction_words(after)).is_empty() {
        return Learned::default();
    }
    let Some(tf) = app.try_state::<TextFlow>() else {
        return Learned::default();
    };
    let result = tf.update(|d| Ok(learn(d, entry_id, before, after)));
    match result {
        Ok(learned) => {
            if !learned.words.is_empty() || !learned.replacements.is_empty() {
                log::info!(
                    "textflow: learned {} words and {} replacements from a correction",
                    learned.words.len(),
                    learned.replacements.len()
                );
                sync_custom_words(app);
                changed(app, "dictionary");
            }
            learned
        }
        Err(e) => {
            log::warn!("textflow: could not save what a correction taught ({e})");
            Learned::default()
        }
    }
}

fn update_snippets(
    app: &AppHandle,
    tf: &TextFlow,
    f: impl FnOnce(&mut Vec<Snippet>) -> Result<(), String>,
) -> Result<Vec<Snippet>, String> {
    let snippets = tf.update(|d| {
        f(&mut d.snippets)?;
        Ok(d.snippets.clone())
    })?;
    changed(app, "snippets");
    Ok(snippets)
}

#[tauri::command]
#[specta::specta]
pub fn get_snippets(tf: State<'_, TextFlow>) -> Vec<Snippet> {
    tf.read(|d| d.snippets.clone())
}

#[tauri::command]
#[specta::specta]
pub fn add_snippet(
    app: AppHandle,
    tf: State<'_, TextFlow>,
    trigger: String,
    expansion: String,
) -> Result<Vec<Snippet>, String> {
    let trigger = required(&trigger, "Trigger")?;
    if expansion.trim().is_empty() {
        return Err("Expansion can't be empty".into());
    }
    update_snippets(&app, &tf, |s| {
        if s.iter().any(|x| same(&x.trigger, &trigger)) {
            return Err(format!("\"{trigger}\" is already a snippet"));
        }
        s.insert(
            0,
            Snippet {
                id: new_id(),
                trigger,
                expansion,
            },
        );
        Ok(())
    })
}

#[tauri::command]
#[specta::specta]
pub fn update_snippet(
    app: AppHandle,
    tf: State<'_, TextFlow>,
    id: String,
    trigger: String,
    expansion: String,
) -> Result<Vec<Snippet>, String> {
    let trigger = required(&trigger, "Trigger")?;
    if expansion.trim().is_empty() {
        return Err("Expansion can't be empty".into());
    }
    update_snippets(&app, &tf, |s| {
        if s.iter().any(|x| x.id != id && same(&x.trigger, &trigger)) {
            return Err(format!("\"{trigger}\" is already a snippet"));
        }
        let entry = s
            .iter_mut()
            .find(|x| x.id == id)
            .ok_or("Snippet not found")?;
        entry.trigger = trigger;
        entry.expansion = expansion;
        Ok(())
    })
}

#[tauri::command]
#[specta::specta]
pub fn delete_snippet(
    app: AppHandle,
    tf: State<'_, TextFlow>,
    id: String,
) -> Result<Vec<Snippet>, String> {
    update_snippets(&app, &tf, |s| {
        s.retain(|x| x.id != id);
        Ok(())
    })
}

fn update_style(
    app: &AppHandle,
    tf: &TextFlow,
    f: impl FnOnce(&mut StyleSettings),
) -> Result<StyleSettings, String> {
    let style = tf.update(|d| {
        f(&mut d.style);
        Ok(d.style.clone())
    })?;
    changed(app, "style");
    Ok(style)
}

#[tauri::command]
#[specta::specta]
pub fn get_style_settings(tf: State<'_, TextFlow>) -> StyleSettings {
    tf.read(|d| d.style.clone())
}

#[tauri::command]
#[specta::specta]
pub fn set_style(
    app: AppHandle,
    tf: State<'_, TextFlow>,
    category: StyleCategory,
    style: StyleKind,
) -> Result<StyleSettings, String> {
    update_style(&app, &tf, |s| match category {
        StyleCategory::Personal => s.personal = style,
        StyleCategory::Work => s.work = style,
        StyleCategory::Email => s.email = style,
        StyleCategory::Other => s.other = style,
    })
}

#[tauri::command]
#[specta::specta]
pub fn set_auto_cleanup(
    app: AppHandle,
    tf: State<'_, TextFlow>,
    enabled: bool,
) -> Result<StyleSettings, String> {
    update_style(&app, &tf, |s| s.auto_cleanup = enabled)
}

fn update_transforms(
    app: &AppHandle,
    tf: &TextFlow,
    f: impl FnOnce(&mut TransformSettings) -> Result<(), String>,
) -> Result<TransformSettings, String> {
    let settings = tf.update(|d| {
        f(&mut d.transforms)?;
        Ok(d.transforms.clone())
    })?;
    transforms::sync_shortcuts(app);
    changed(app, "transforms");
    Ok(settings)
}

#[tauri::command]
#[specta::specta]
pub fn get_transforms(tf: State<'_, TextFlow>) -> TransformSettings {
    tf.read(|d| d.transforms.clone())
}

/// Creates a transform when `id` is none, otherwise updates it. A new
/// transform without a shortcut gets the first free ⌥ number; a shortcut
/// taken by another transform moves to this one.
#[tauri::command]
#[specta::specta]
pub fn save_transform(
    app: AppHandle,
    tf: State<'_, TextFlow>,
    id: Option<String>,
    name: String,
    description: String,
    prompt: String,
    shortcut_index: Option<u32>,
) -> Result<TransformSettings, String> {
    let name = required(&name, "Name")?;
    if prompt.trim().is_empty() {
        return Err("Prompt can't be empty".into());
    }
    if shortcut_index.is_some_and(|n| !(1..=9).contains(&n)) {
        return Err("Shortcut must be between 1 and 9".into());
    }
    update_transforms(&app, &tf, |t| {
        let shortcut = match &id {
            Some(_) => shortcut_index,
            None => shortcut_index.or_else(|| {
                (1..=9).find(|n| !t.items.iter().any(|i| i.shortcut_index == Some(*n)))
            }),
        };
        if let Some(n) = shortcut {
            for other in t.items.iter_mut() {
                if Some(&other.id) != id.as_ref() && other.shortcut_index == Some(n) {
                    other.shortcut_index = None;
                }
            }
        }
        let transform = |id: String| Transform {
            id,
            name,
            description: description.trim().to_string(),
            prompt: prompt.trim().to_string(),
            shortcut_index: shortcut,
        };
        match id {
            Some(id) => {
                let item = t
                    .items
                    .iter_mut()
                    .find(|i| i.id == id)
                    .ok_or("Transform not found")?;
                *item = transform(id);
            }
            None => t.items.push(transform(new_id())),
        }
        Ok(())
    })
}

#[tauri::command]
#[specta::specta]
pub fn delete_transform(
    app: AppHandle,
    tf: State<'_, TextFlow>,
    id: String,
) -> Result<TransformSettings, String> {
    update_transforms(&app, &tf, |t| {
        t.items.retain(|i| i.id != id);
        if t.auto_apply.transform_id.as_deref() == Some(id.as_str()) {
            t.auto_apply = AutoApply::default();
        }
        Ok(())
    })
}

#[tauri::command]
#[specta::specta]
pub fn reset_transforms(
    app: AppHandle,
    tf: State<'_, TextFlow>,
) -> Result<TransformSettings, String> {
    update_transforms(&app, &tf, |t| {
        t.items = default_transforms();
        t.auto_apply = AutoApply::default();
        Ok(())
    })
}

#[tauri::command]
#[specta::specta]
pub fn set_transforms_opt_in(
    app: AppHandle,
    tf: State<'_, TextFlow>,
    enabled: bool,
) -> Result<TransformSettings, String> {
    update_transforms(&app, &tf, |t| {
        t.opt_in = enabled;
        Ok(())
    })
}

#[tauri::command]
#[specta::specta]
pub fn set_auto_apply(
    app: AppHandle,
    tf: State<'_, TextFlow>,
    enabled: bool,
    transform_id: Option<String>,
) -> Result<TransformSettings, String> {
    update_transforms(&app, &tf, |t| {
        if let Some(id) = &transform_id {
            if !t.items.iter().any(|i| &i.id == id) {
                return Err("Transform not found".into());
            }
        }
        t.auto_apply = AutoApply {
            enabled,
            transform_id,
        };
        Ok(())
    })
}

#[tauri::command]
#[specta::specta]
pub async fn list_notes(notes: State<'_, Notes>) -> Result<Vec<Note>, String> {
    notes.list()
}

#[tauri::command]
#[specta::specta]
pub async fn get_note(notes: State<'_, Notes>, id: String) -> Result<Option<Note>, String> {
    notes.get(&id)
}

#[tauri::command]
#[specta::specta]
pub async fn save_note(
    notes: State<'_, Notes>,
    id: Option<String>,
    title: String,
    body: String,
) -> Result<String, String> {
    notes.save(id, &title, &body)
}

#[tauri::command]
#[specta::specta]
pub async fn delete_note(notes: State<'_, Notes>, id: String) -> Result<(), String> {
    notes.delete(&id)
}

#[tauri::command]
#[specta::specta]
pub async fn get_insights(history: State<'_, Arc<HistoryManager>>) -> Result<Insights, String> {
    let rows = history.dictation_stats().map_err(|e| e.to_string())?;
    Ok(insights::compute(&rows, chrono::Local::now().date_naive()))
}

/// Hides the pill for `minutes`; 0 shows it again now. Emits `pill-hidden`.
#[tauri::command]
#[specta::specta]
pub fn pill_hide_for(app: AppHandle, minutes: u32) {
    menu::hide_for(&app, minutes);
}

/// Unix ms until which the pill is hidden, or none.
#[tauri::command]
#[specta::specta]
pub fn pill_hidden_until() -> Option<i64> {
    menu::hidden_until()
}

#[tauri::command]
#[specta::specta]
pub async fn paste_last_transcript(app: AppHandle) -> Result<(), String> {
    let text = menu::last_transcript(&app)?;
    tauri::async_runtime::spawn_blocking(move || crate::utils::paste(text, app))
        .await
        .map_err(|e| e.to_string())?
}

#[tauri::command]
#[specta::specta]
pub fn open_main_page(app: AppHandle, page: String) -> Result<(), String> {
    menu::open_main_page(&app, page)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn learns_new_spellings_but_not_common_words() {
        let mut d = Data::default();
        let l = learn(
            &mut d,
            1,
            "I pushed the fix to lip pick on get hub, theek high",
            "I pushed the fix to Lipik on GitHub, theek hai",
        );
        assert_eq!(l.words, vec!["Lipik", "GitHub"]);
        assert!(d.dictionary.words.iter().all(|w| w.auto_learned));

        let l = learn(&mut d, 3, "deploy it on g c p", "deploy it on GCP");
        assert_eq!(l.words, vec!["GCP"]);

        let l = learn(
            &mut d,
            2,
            "Their going to the e mail",
            "They're going to the email",
        );
        assert!(l.words.is_empty());
    }

    #[test]
    fn same_fix_in_two_dictations_becomes_a_replacement() {
        let mut d = Data::default();
        learn(&mut d, 1, "open lip pick now", "open Lipik now");
        assert!(learn(&mut d, 1, "open lip pick now", "open Lipik now")
            .replacements
            .is_empty());
        let l = learn(&mut d, 2, "is lip pick running", "is Lipik running");
        assert_eq!(
            l.replacements,
            vec![("lip pick".to_string(), "Lipik".to_string())]
        );
        assert!(d.dictionary.replacements[0].auto_learned);
        assert!(d.pending_fixes.is_empty());
    }

    #[test]
    fn single_common_word_never_becomes_a_replacement() {
        let mut d = Data::default();
        learn(&mut d, 1, "yeah high", "yeah hai");
        learn(&mut d, 2, "okay high", "okay hai");
        assert!(d.dictionary.replacements.is_empty());
    }
}
