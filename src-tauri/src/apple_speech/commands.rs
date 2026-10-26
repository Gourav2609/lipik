use super::AppleSpeechStatus;
use crate::managers::transcription::TranscriptionManager;
use crate::settings::{self, SpeechEngine};
use std::sync::Arc;
use tauri::{AppHandle, Manager};

#[tauri::command]
#[specta::specta]
pub fn change_speech_engine_setting(app: AppHandle, engine: SpeechEngine) -> Result<(), String> {
    let mut settings = settings::get_settings(&app);
    settings.speech_engine = engine;
    settings::write_settings(&app, settings);
    if let Some(tm) = app.try_state::<Arc<TranscriptionManager>>() {
        if engine == SpeechEngine::Parakeet {
            tm.apple_speech().shutdown();
        } else {
            tm.warm_apple_speech();
        }
    }
    Ok(())
}

#[tauri::command]
#[specta::specta]
pub fn change_apple_locale_setting(app: AppHandle, locale: String) -> Result<(), String> {
    let locale = locale.trim();
    if locale.is_empty() {
        return Err("Locale is empty".to_string());
    }
    let mut settings = settings::get_settings(&app);
    settings.apple_locale = locale.to_string();
    settings::write_settings(&app, settings);
    if let Some(tm) = app.try_state::<Arc<TranscriptionManager>>() {
        tm.warm_apple_speech();
    }
    Ok(())
}

/// Whether Apple's dictation can run for the configured locale. With
/// `download`, missing speech assets are fetched first (needs a network).
#[tauri::command]
#[specta::specta]
pub async fn get_apple_speech_status(
    app: AppHandle,
    download: bool,
) -> Result<AppleSpeechStatus, String> {
    let tm = app
        .try_state::<Arc<TranscriptionManager>>()
        .map(|s| Arc::clone(&s))
        .ok_or_else(|| "Transcription is not ready".to_string())?;
    let locale = settings::get_settings(&app).apple_locale;
    tauri::async_runtime::spawn_blocking(move || tm.apple_speech().status(&locale, download))
        .await
        .map_err(|e| e.to_string())
}
