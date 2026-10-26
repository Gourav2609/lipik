//! The meeting notepad: a normal window docked to the right edge of the work
//! area, full visible height, 500 pt wide. It opens when a recording starts,
//! focused unless the start was a call auto-start, and only hides on close, so
//! a recording keeps going and reopening is instant.

use serde::{Deserialize, Serialize};
use specta::Type;
use std::sync::Mutex;
use tauri::{AppHandle, Manager, WebviewUrl, WebviewWindowBuilder};
use tauri_specta::Event;

use crate::notetaker::NotetakerManager;
use crate::settings::Theme;

const LABEL: &str = "notepad";
const WIDTH: f64 = 500.0;
/// Puts the window buttons' centres 26 pt from the top, level with the toolbar.
#[cfg(target_os = "macos")]
const BAR_HEIGHT: f64 = 52.0;

#[derive(Clone, Debug, Serialize, Deserialize, Type, tauri_specta::Event)]
pub struct NotepadMeeting {
    pub meeting_id: String,
}

#[derive(Default)]
struct Current(Mutex<Option<String>>);

/// Held across "is there a window? else build one", so two opens at once
/// can't both build it.
#[derive(Default)]
struct Building(Mutex<()>);

pub fn init(app: &AppHandle) {
    app.manage(Current::default());
    app.manage(Building::default());
}

/// Shows the notepad on a recording that just started. A start the user
/// didn't make (call auto-start) shows it without taking focus from the call.
pub fn recording_started(app: &AppHandle, meeting_id: &str, focus: bool) {
    if let Err(e) = show(app, Some(meeting_id.to_string()), focus) {
        log::error!("Failed to open the notepad: {e}");
    }
}

pub fn is_focused(app: &AppHandle) -> bool {
    app.get_webview_window(LABEL)
        .and_then(|w| w.is_focused().ok())
        .unwrap_or(false)
}

fn colors(app: &AppHandle) -> (tauri::Theme, tauri::window::Color) {
    match crate::settings::get_settings(app).theme {
        Theme::Light => (tauri::Theme::Light, tauri::window::Color(0xFB, 0xFC, 0xFD, 0xFF)),
        Theme::Dark | Theme::System => {
            (tauri::Theme::Dark, tauri::window::Color(0x15, 0x19, 0x23, 0xFF))
        }
    }
}

/// The notepad is dark unless the app theme is set to light.
pub fn apply_theme(app: &AppHandle) {
    if let Some(window) = app.get_webview_window(LABEL) {
        let (theme, color) = colors(app);
        let _ = window.set_theme(Some(theme));
        let _ = window.set_background_color(Some(color));
    }
}

/// Logical (x, y, height) of the window: right edge on the right edge of the
/// work area, top under the menu bar, bottom on the dock. On macOS a monitor's
/// physical rect is its global points times its own scale, so dividing by that
/// scale gives points even with mixed-scale monitors.
fn docked_frame(app: &AppHandle) -> Option<(f64, f64, f64)> {
    let monitor = app
        .cursor_position()
        .ok()
        .and_then(|p| app.monitor_from_point(p.x, p.y).ok().flatten())
        .or_else(|| app.primary_monitor().ok().flatten())?;
    let scale = monitor.scale_factor();
    let wa = monitor.work_area();
    let (left, top) = (wa.position.x as f64 / scale, wa.position.y as f64 / scale);
    let (width, height) = (wa.size.width as f64 / scale, wa.size.height as f64 / scale);
    Some((left + width - WIDTH, top, height))
}

fn create(app: &AppHandle, focus: bool) -> tauri::Result<tauri::WebviewWindow> {
    let (x, y, height) = docked_frame(app).unwrap_or((0.0, 0.0, 870.0));
    let (theme, color) = colors(app);
    let mut builder =
        WebviewWindowBuilder::new(app, LABEL, WebviewUrl::App("src/notepad/index.html".into()))
            .title("Notepad")
            .inner_size(WIDTH, height)
            .position(x, y)
            .resizable(true)
            .maximizable(false)
            .theme(Some(theme))
            .background_color(color)
            .focused(focus)
            .visible(false);

    #[cfg(target_os = "macos")]
    {
        builder = builder
            .title_bar_style(tauri::TitleBarStyle::Overlay)
            .hidden_title(true);
    }

    if let Some(data_dir) = crate::portable::data_dir() {
        builder = builder.data_directory(data_dir.join("webview"));
    }

    builder.build()
}

#[cfg(target_os = "macos")]
pub fn place_traffic_lights(window: &tauri::Window) {
    crate::traffic_lights::place_in(window, BAR_HEIGHT);
}

/// Shows and focuses the notepad on `meeting_id`, else on the meeting being
/// recorded, else on the meeting it showed last.
pub fn open(app: &AppHandle, meeting_id: Option<String>) -> tauri::Result<()> {
    show(app, meeting_id, true)
}

fn show(app: &AppHandle, meeting_id: Option<String>, focus: bool) -> tauri::Result<()> {
    let recording = app
        .try_state::<std::sync::Arc<NotetakerManager>>()
        .and_then(|m| m.recording_meeting());
    let id = {
        let current = app.state::<Current>();
        let mut current = current.0.lock().unwrap();
        if let Some(id) = meeting_id.or(recording) {
            *current = Some(id);
        }
        current.clone()
    };

    let window = {
        let building = app.state::<Building>();
        let _building = building.0.lock().unwrap();
        match app.get_webview_window(LABEL) {
            Some(window) => window,
            None => create(app, focus)?,
        }
    };
    if let Some(id) = id {
        let _ = NotepadMeeting { meeting_id: id }.emit_to(app, LABEL);
    }
    if focus {
        let _ = window.unminimize();
        window.show()?;
        window.set_focus()?;
    } else {
        window.show()?;
    }
    #[cfg(target_os = "macos")]
    place_traffic_lights(&window.as_ref().window());
    Ok(())
}

pub fn close(app: &AppHandle) -> tauri::Result<()> {
    match app.get_webview_window(LABEL) {
        Some(window) => window.hide(),
        None => Ok(()),
    }
}

#[tauri::command]
#[specta::specta]
pub async fn notepad_open(app: AppHandle, meeting_id: Option<String>) -> Result<(), String> {
    open(&app, meeting_id).map_err(|e| e.to_string())
}

#[tauri::command]
#[specta::specta]
pub async fn notepad_close(app: AppHandle) -> Result<(), String> {
    close(&app).map_err(|e| e.to_string())
}

/// The meeting the notepad shows; the page reads it once on load, then follows
/// `notepad-meeting`.
#[tauri::command]
#[specta::specta]
pub fn notepad_meeting(app: AppHandle) -> Option<String> {
    app.state::<Current>().0.lock().unwrap().clone()
}
