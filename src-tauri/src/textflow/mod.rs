//! Lipik's text features: dictionary, snippets, style clean-up, transforms,
//! scratchpad notes, insights and the pill menu. See CONTRACT.md.

pub mod commands;
mod context;
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
    auto_transform, clean_up, prepare, record_meta, uses_llm, waits_on_llm, DictationMeta, Target,
};

use store::{DictionaryWord, TextFlow};
use tauri::{AppHandle, Manager};

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
