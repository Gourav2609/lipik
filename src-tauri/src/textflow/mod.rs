//! Lipik's text features: dictionary, snippets, style clean-up, transforms,
//! scratchpad notes, insights and the pill menu. See CONTRACT.md.

pub mod cleanup_model;
pub mod commands;
mod context;

/// The bundle id of the app in front, from the system rather than
/// Accessibility, so it is known for every app.
pub fn front_bundle_id() -> Option<String> {
    context::front_app().bundle_id
}

pub mod early;
pub mod engine;
mod gate;
pub mod hindi;
mod insights;
mod llm;
mod menu;
mod notes;
mod pipeline;
mod rules;
mod store;
mod text;
pub mod transforms;

pub use pipeline::{
    auto_transform, clean_up, end_sentence, prepare, record_meta, uses_llm, waits_on_llm,
    DictationMeta, Target,
};

pub(crate) use rules::sound_key;
use store::{DictionaryWord, TextFlow};
use tauri::{AppHandle, Manager};

/// Dictionary words and replacements that look like email addresses (the
/// user's own, usually).
pub fn dictionary_emails(app: &AppHandle) -> Vec<String> {
    let Some(tf) = app.try_state::<TextFlow>() else {
        return Vec::new();
    };
    tf.read(|d| {
        d.dictionary
            .words
            .iter()
            .map(|w| w.word.trim().to_string())
            .chain(
                d.dictionary
                    .replacements
                    .iter()
                    .map(|r| r.to.trim().to_string()),
            )
            .filter(|w| w.contains('@') && !w.contains(char::is_whitespace))
            .collect()
    })
}

/// Dictionary words that are not email addresses.
pub fn dictionary_words(app: &AppHandle) -> Vec<String> {
    let Some(tf) = app.try_state::<TextFlow>() else {
        return Vec::new();
    };
    tf.read(|d| {
        d.dictionary
            .words
            .iter()
            .map(|w| w.word.trim().to_string())
            .filter(|w| !w.is_empty() && !w.contains('@'))
            .collect()
    })
}

pub fn init(app: &AppHandle) {
    let dir = match crate::portable::app_data_dir(app) {
        Ok(dir) => dir,
        Err(e) => {
            log::error!("textflow: no app data folder ({e}); text features are off");
            return;
        }
    };
    let _ = std::fs::create_dir_all(&dir);
    let (textflow, existed) = TextFlow::load(dir.join("textflow.json"));
    if !existed {
        let words = crate::settings::get_settings(app).custom_words;
        let imported = textflow.update(|d| {
            d.dictionary.words = words
                .into_iter()
                .filter(|w| !w.trim().is_empty())
                .map(|word| DictionaryWord {
                    id: store::new_id(),
                    word,
                    starred: false,
                    auto_learned: false,
                    shared: false,
                })
                .collect();
            Ok(())
        });
        if let Err(e) = imported {
            log::warn!("textflow: could not create textflow.json ({e})");
        }
    }
    app.manage(textflow);
    app.manage(notes::Notes::new(dir.join("scratchpad.db")));
    menu::keep_hidden_after_dictation(app);
    engine::init();
    cleanup_model::pick_engine(app);
}

/// Runs when the app quits: stops the built-in clean-up engine.
pub fn shutdown() {
    engine::ENGINE.stop();
}

/// Runs when a dictation starts recording: remembers the target app, tells
/// the pill which microphone is used and wakes the local model if needed.
pub fn on_recording_start(app: &AppHandle) {
    menu::recording_started();
    let wake = uses_llm(app);
    let app = app.clone();
    std::thread::spawn(move || {
        if wake {
            llm::warm_up(&app);
        }
        context::capture_for_recording();
        menu::emit_pill_mic(&app);
    });
}
