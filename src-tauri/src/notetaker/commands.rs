use std::sync::Arc;
use tauri::{AppHandle, Manager as _, State};

use super::calendar::{self, CalendarAccess, CalendarEvent, CalendarSource};
use super::{
    AskAnswer, ChatMessage, ChatThread, FolderInfo, Meeting, MeetingDetail, NotetakerFilter,
    NotetakerManager, NotetakerSettings, NotetakerStatus,
};

type Manager<'a> = State<'a, Arc<NotetakerManager>>;

async fn blocking<T: Send + 'static>(
    f: impl FnOnce() -> anyhow::Result<T> + Send + 'static,
) -> Result<T, String> {
    tauri::async_runtime::spawn_blocking(f)
        .await
        .map_err(|e| e.to_string())?
        .map_err(|e| e.to_string())
}

#[tauri::command]
#[specta::specta]
pub async fn notetaker_start(
    manager: Manager<'_>,
    title: Option<String>,
) -> Result<Meeting, String> {
    let m = manager.inner().clone();
    blocking(move || {
        let meeting = m.start(title, None)?;
        crate::notepad::recording_started(&m.app, &meeting.id, true);
        Ok(meeting)
    })
    .await
}

#[tauri::command]
#[specta::specta]
pub async fn notetaker_stop(manager: Manager<'_>) -> Result<String, String> {
    let m = manager.inner().clone();
    blocking(move || m.stop()).await
}

#[tauri::command]
#[specta::specta]
pub async fn notetaker_status(manager: Manager<'_>) -> Result<NotetakerStatus, String> {
    let m = manager.inner().clone();
    blocking(move || Ok(m.status())).await
}

#[tauri::command]
#[specta::specta]
pub async fn notetaker_list(
    manager: Manager<'_>,
    filter: Option<NotetakerFilter>,
) -> Result<Vec<Meeting>, String> {
    let m = manager.inner().clone();
    blocking(move || m.list(&filter.unwrap_or_default())).await
}

#[tauri::command]
#[specta::specta]
pub async fn notetaker_get(
    manager: Manager<'_>,
    id: String,
) -> Result<Option<MeetingDetail>, String> {
    let m = manager.inner().clone();
    blocking(move || m.get(&id)).await
}

#[tauri::command]
#[specta::specta]
pub async fn notetaker_save_thoughts(
    manager: Manager<'_>,
    id: String,
    markdown: String,
) -> Result<(), String> {
    let m = manager.inner().clone();
    blocking(move || m.save_thoughts(&id, &markdown)).await
}

#[tauri::command]
#[specta::specta]
pub async fn notetaker_delete(manager: Manager<'_>, id: String) -> Result<(), String> {
    let m = manager.inner().clone();
    blocking(move || m.delete(&id)).await
}

#[tauri::command]
#[specta::specta]
pub async fn notetaker_rename(
    manager: Manager<'_>,
    id: String,
    title: String,
) -> Result<(), String> {
    let m = manager.inner().clone();
    blocking(move || m.rename(&id, &title)).await
}

/// Names one speaker (`speaker_key` from a segment) on every line of the
/// meeting.
#[tauri::command]
#[specta::specta]
pub async fn notetaker_rename_speaker(
    manager: Manager<'_>,
    meeting_id: String,
    speaker_key: String,
    name: String,
) -> Result<(), String> {
    let m = manager.inner().clone();
    blocking(move || m.rename_speaker(&meeting_id, &speaker_key, &name)).await
}

/// How many people have a saved voice (from speakers the user named).
#[tauri::command]
#[specta::specta]
pub async fn notetaker_saved_voices(manager: Manager<'_>) -> Result<u32, String> {
    let m = manager.inner().clone();
    blocking(move || m.saved_voices()).await
}

/// Forgets every saved voice, in all meetings.
#[tauri::command]
#[specta::specta]
pub async fn notetaker_forget_voices(manager: Manager<'_>) -> Result<(), String> {
    let m = manager.inner().clone();
    blocking(move || m.forget_voices()).await
}

#[tauri::command]
#[specta::specta]
pub async fn notetaker_move_to_folder(
    manager: Manager<'_>,
    id: String,
    folder: Option<String>,
) -> Result<(), String> {
    let m = manager.inner().clone();
    blocking(move || m.move_to_folder(&id, folder.as_deref())).await
}

#[tauri::command]
#[specta::specta]
pub async fn notetaker_folders(manager: Manager<'_>) -> Result<Vec<FolderInfo>, String> {
    let m = manager.inner().clone();
    blocking(move || m.folders()).await
}

#[tauri::command]
#[specta::specta]
pub async fn notetaker_regenerate(manager: Manager<'_>, id: String) -> Result<(), String> {
    let m = manager.inner().clone();
    blocking(move || m.regenerate(&id)).await
}

/// Hears the meeting again with the Hinglish speech model (fetched first if
/// needed), then writes its notes again.
#[tauri::command]
#[specta::specta]
pub async fn notetaker_redo_in_hinglish(manager: Manager<'_>, id: String) -> Result<(), String> {
    let m = manager.inner().clone();
    blocking(move || m.redo_in_hinglish_now(&id)).await
}

/// Hides why the meeting's Hinglish redo failed.
#[tauri::command]
#[specta::specta]
pub async fn notetaker_dismiss_hinglish_error(
    manager: Manager<'_>,
    id: String,
) -> Result<(), String> {
    let m = manager.inner().clone();
    blocking(move || m.dismiss_hinglish_error(&id)).await
}

/// Today's and tomorrow's calendar events. Asks for calendar access the first
/// time.
#[tauri::command]
#[specta::specta]
pub async fn notetaker_upcoming() -> Result<Vec<CalendarEvent>, String> {
    blocking(|| {
        calendar::request_access();
        let (start, end) = calendar::upcoming_window();
        Ok(calendar::events_between(start, end))
    })
    .await
}

#[tauri::command]
#[specta::specta]
pub fn notetaker_calendar_access() -> CalendarAccess {
    calendar::access()
}

#[tauri::command]
#[specta::specta]
pub async fn notetaker_request_calendar_access() -> Result<CalendarAccess, String> {
    blocking(|| Ok(calendar::request_access())).await
}

/// The Calendar app's accounts and their calendars; empty without full access.
#[tauri::command]
#[specta::specta]
pub async fn notetaker_calendar_sources() -> Result<Vec<CalendarSource>, String> {
    blocking(|| Ok(calendar::sources())).await
}

/// Answers a question from one meeting (`meeting_id`) or all of them
/// (`None`). Pass the `thread_id` of an earlier answer to continue that chat.
#[tauri::command]
#[specta::specta]
pub async fn notetaker_ask(
    manager: Manager<'_>,
    question: String,
    meeting_id: Option<String>,
    thread_id: Option<String>,
) -> Result<AskAnswer, String> {
    let m = manager.inner().clone();
    blocking(move || m.ask(&question, meeting_id.as_deref(), thread_id.as_deref())).await
}

#[tauri::command]
#[specta::specta]
pub async fn notetaker_chats(
    manager: Manager<'_>,
    meeting_id: Option<String>,
) -> Result<Vec<ChatThread>, String> {
    let m = manager.inner().clone();
    blocking(move || m.chat_threads(meeting_id.as_deref())).await
}

#[tauri::command]
#[specta::specta]
pub async fn notetaker_chat(
    manager: Manager<'_>,
    thread_id: String,
) -> Result<Vec<ChatMessage>, String> {
    let m = manager.inner().clone();
    blocking(move || m.chat_messages(&thread_id)).await
}

#[tauri::command]
#[specta::specta]
pub fn notetaker_get_settings(app: AppHandle) -> NotetakerSettings {
    super::settings(&app)
}

#[tauri::command]
#[specta::specta]
pub fn notetaker_set_settings(app: AppHandle, settings: NotetakerSettings) -> Result<(), String> {
    if settings.ollama_url.trim().is_empty() || settings.ollama_model.trim().is_empty() {
        return Err("Ollama address and model can't be empty".to_string());
    }
    if !(1..=600).contains(&settings.max_recording_minutes) {
        return Err("Maximum recording length must be 1 to 600 minutes".to_string());
    }
    if settings.notify_before_seconds > 3600 {
        return Err("The meeting reminder can be at most 60 minutes ahead".to_string());
    }
    crate::textflow::cleanup_model::cancel_pull_unless(&settings.ollama_model);
    if let Some(manager) = app.try_state::<Arc<NotetakerManager>>() {
        manager
            .settings_changed(&settings)
            .map_err(|e| e.to_string())?;
    }
    let mut all = crate::settings::get_settings(&app);
    all.notetaker = settings;
    crate::settings::write_settings(&app, all);
    Ok(())
}
