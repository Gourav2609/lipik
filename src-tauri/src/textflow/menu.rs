//! The pill's right-click menu actions and the microphone label shown when a
//! recording starts.

use crate::managers::history::HistoryManager;
use serde::Serialize;
use std::sync::atomic::{AtomicI64, AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Duration;
use tauri::{AppHandle, Emitter, Listener, Manager};

static HIDE_GENERATION: AtomicU64 = AtomicU64::new(0);
static RECORDING_GENERATION: AtomicU64 = AtomicU64::new(0);
/// Unix ms until which the pill stays hidden; 0 when shown.
static HIDDEN_UNTIL: AtomicI64 = AtomicI64::new(0);

#[derive(Serialize, Clone)]
struct PillMic {
    name: String,
    recommended: bool,
}

fn set_pill_visible(app: &AppHandle, visible: bool) {
    let app_for_main = app.clone();
    let _ = app.run_on_main_thread(move || {
        if let Some(window) = app_for_main.get_webview_window("recording_overlay") {
            let _ = if visible {
                window.show()
            } else {
                window.hide()
            };
        }
    });
    let until = HIDDEN_UNTIL.load(Ordering::SeqCst);
    let _ = app.emit("pill-hidden", (until > 0).then_some(until));
}

pub fn hide_for(app: &AppHandle, minutes: u32) {
    let generation = HIDE_GENERATION.fetch_add(1, Ordering::SeqCst) + 1;
    if minutes == 0 {
        HIDDEN_UNTIL.store(0, Ordering::SeqCst);
        set_pill_visible(app, true);
        return;
    }
    let wait = Duration::from_secs(u64::from(minutes) * 60);
    HIDDEN_UNTIL.store(
        chrono::Utc::now().timestamp_millis() + wait.as_millis() as i64,
        Ordering::SeqCst,
    );
    set_pill_visible(app, false);
    let app = app.clone();
    std::thread::spawn(move || {
        std::thread::sleep(wait);
        if HIDE_GENERATION.load(Ordering::SeqCst) == generation {
            HIDDEN_UNTIL.store(0, Ordering::SeqCst);
            set_pill_visible(&app, true);
        }
    });
}

pub fn hidden_until() -> Option<i64> {
    let until = HIDDEN_UNTIL.load(Ordering::SeqCst);
    (until > chrono::Utc::now().timestamp_millis()).then_some(until)
}

pub fn recording_started() {
    RECORDING_GENERATION.fetch_add(1, Ordering::SeqCst);
}

/// Every dictation shows the overlay again. While the pill is hidden, hide it
/// once the dictation's fade-out is over, unless a new recording started.
pub fn keep_hidden_after_dictation(app: &AppHandle) {
    let handle = app.clone();
    app.listen_any("hide-overlay", move |_| {
        if hidden_until().is_none() {
            return;
        }
        let generation = RECORDING_GENERATION.load(Ordering::SeqCst);
        let app = handle.clone();
        std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(400));
            if hidden_until().is_none() || RECORDING_GENERATION.load(Ordering::SeqCst) != generation
            {
                return;
            }
            let app_for_main = app.clone();
            let _ = app.run_on_main_thread(move || {
                if let Some(window) = app_for_main.get_webview_window("recording_overlay") {
                    let _ = window.hide();
                }
            });
        });
    });
}

pub fn last_transcript(app: &AppHandle) -> Result<String, String> {
    let entry = app
        .state::<Arc<HistoryManager>>()
        .get_latest_completed_entry()
        .map_err(|e| e.to_string())?
        .ok_or_else(|| "No transcript yet".to_string())?;
    Ok(entry.final_text().to_string())
}

pub fn open_main_page(app: &AppHandle, page: String) -> Result<(), String> {
    crate::show_main_window(app);
    app.emit_to("main", "flow-open-page", page)
        .map_err(|e| e.to_string())
}

fn is_builtin(name: &str) -> bool {
    let name = name.to_lowercase();
    name.contains("macbook") || name.contains("built-in") || name.contains("internal")
}

/// Emits `pill-mic` with the input the recording uses, only when it differs
/// from the one named last time. Call off the main thread: listing devices
/// can take tens of milliseconds.
pub fn emit_pill_mic(app: &AppHandle) {
    let mut settings = crate::settings::get_settings(app);
    let name = settings.selected_microphone.clone().or_else(|| {
        lipik_recorder::list_input_devices()
            .ok()?
            .into_iter()
            .find(|d| d.is_default)
            .map(|d| d.name)
    });
    let Some(name) = name else {
        return;
    };
    if settings.last_pill_mic.as_deref() == Some(name.as_str()) {
        return;
    }
    settings.last_pill_mic = Some(name.clone());
    crate::settings::write_settings(app, settings);
    let recommended = is_builtin(&name);
    let payload = PillMic {
        name: if recommended {
            "Built-in mic".to_string()
        } else {
            name
        },
        recommended,
    };
    let _ = app.emit("pill-mic", payload);
}
