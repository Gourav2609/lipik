//! Pill (macOS): the overlay panel is a transparent strip along the docking edge
//! of the work area (bottom by default, or left or right) that stays visible. Only the pill's hit
//! area takes the mouse; everywhere else the strip ignores cursor events so clicks
//! reach the apps underneath. A webview that ignores the cursor gets no mouse
//! events, so hover is detected here by polling the cursor and sent to the page.
//! While the pill is dragged the panel covers the whole display, so the page
//! can dim it and draw the drop targets.

use crate::settings::{get_settings, write_settings, OverlayStyle, PasteMethod, PillSide};
use crate::TranscriptionCoordinator;
use serde::Serialize;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Mutex;
use std::time::Duration;
use tauri::{AppHandle, Emitter, Manager};

pub const STRIP_WIDTH: f64 = 490.0;
const POLL_MS: u64 = 16;
/// How long the pill offers Undo after a cancel.
const UNDO_MS: u64 = 5000;
/// The audio stays until the page says the offer ended (it can pause the
/// countdown), but never longer than this.
const KEEP_MAX_MS: u64 = 60_000;

/// Pill hit area in strip coordinates (logical points), reported by the overlay page.
static HIT_RECT: Mutex<(f64, f64, f64, f64)> = Mutex::new((0.0, 0.0, 0.0, 0.0));
static STARTED: AtomicBool = AtomicBool::new(false);
/// Set when the page reports a hit area (also on every page load), so the poll
/// re-sends the hover state even if the cursor did not cross the edge.
static RESEND: AtomicBool = AtomicBool::new(false);
static DRAGGING: AtomicBool = AtomicBool::new(false);

/// Audio of the last cancelled dictation: (token, binding id, samples).
static KEPT: Mutex<Option<(u64, String, Vec<f32>)>> = Mutex::new(None);
static KEPT_TOKEN: AtomicU64 = AtomicU64::new(0);

#[derive(Clone, Serialize)]
struct PillResult<'a> {
    text: &'a str,
    inserted: bool,
}

#[derive(Clone, Serialize)]
struct PillCancelled {
    undo_ms: u64,
}

/// Logical (x, y, width, height) of the panel: a strip on the docking edge of
/// the primary display's work area, or the whole display while dragging.
pub fn strip_frame(app: &AppHandle) -> Option<(f64, f64, f64, f64)> {
    let monitor = app.primary_monitor().ok()??;
    let scale = monitor.scale_factor();
    if DRAGGING.load(Ordering::SeqCst) {
        let (pos, size) = (monitor.position(), monitor.size());
        return Some((
            pos.x as f64 / scale,
            pos.y as f64 / scale,
            size.width as f64 / scale,
            size.height as f64 / scale,
        ));
    }
    let wa = monitor.work_area();
    let (left, top) = (wa.position.x as f64 / scale, wa.position.y as f64 / scale);
    let (width, height) = (wa.size.width as f64 / scale, wa.size.height as f64 / scale);
    Some(match get_settings(app).pill_side {
        PillSide::Left => (left, top, STRIP_WIDTH, height),
        PillSide::Right => (left + width - STRIP_WIDTH, top, STRIP_WIDTH, height),
        PillSide::Bottom => (left, top + height - STRIP_WIDTH, width, STRIP_WIDTH),
    })
}

/// Moves and sizes the panel to `strip_frame`.
pub fn place(app: &AppHandle) {
    if let (Some(win), Some((x, y, width, height))) =
        (app.get_webview_window("recording_overlay"), strip_frame(app))
    {
        let _ = win.set_size(tauri::LogicalSize::new(width, height));
        let _ = win.set_position(tauri::LogicalPosition::new(x, y));
    }
    RESEND.store(true, Ordering::SeqCst);
}

fn emit_side(app: &AppHandle, side: PillSide) {
    let _ = app.emit_to("recording_overlay", "pill-side", side);
}

#[tauri::command]
#[specta::specta]
pub fn set_pill_hit_rect(app: AppHandle, x: f64, y: f64, width: f64, height: f64) {
    if let Ok(mut rect) = HIT_RECT.lock() {
        *rect = (x, y, width, height);
    }
    RESEND.store(true, Ordering::SeqCst);
    emit_side(&app, get_settings(&app).pill_side);
}

/// Mic button on the pill: same as a toggle press of the transcribe shortcut.
#[tauri::command]
#[specta::specta]
pub fn pill_toggle_dictation(app: AppHandle) {
    crate::signal_handle::send_transcription_input(&app, "transcribe", "pill");
}

/// ✓ button: stop the current recording and transcribe it.
#[tauri::command]
#[specta::specta]
pub fn pill_finish(app: AppHandle) {
    if let Some(coordinator) = app.try_state::<TranscriptionCoordinator>() {
        coordinator.finish("pill");
    }
}

/// X button: cancel the dictation, keeping its audio for Undo.
#[tauri::command]
#[specta::specta]
pub fn pill_cancel(app: AppHandle) {
    crate::utils::cancel_current_operation(&app);
}

/// Undo: transcribe the kept audio. False when the audio is gone or another
/// dictation is running.
#[tauri::command]
#[specta::specta]
pub async fn pill_undo_cancel(app: AppHandle) -> bool {
    let Some((_, binding_id, samples)) = KEPT.lock().ok().and_then(|mut kept| kept.take()) else {
        log::debug!("Undo: the cancelled audio is gone");
        return false;
    };
    let reply = match app.try_state::<TranscriptionCoordinator>() {
        Some(coordinator) => coordinator.resume(&binding_id, samples),
        None => return false,
    };
    tauri::async_runtime::spawn_blocking(move || reply.recv().unwrap_or(false))
        .await
        .unwrap_or(false)
}

/// The pill's Undo offer ended.
#[tauri::command]
#[specta::specta]
pub fn pill_undo_expired() {
    if let Ok(mut kept) = KEPT.lock() {
        *kept = None;
    }
}

#[tauri::command]
#[specta::specta]
pub fn pill_copy_text(app: AppHandle, text: String) -> Result<(), String> {
    crate::clipboard::write_text_to_clipboard(&app, &text)
}

#[tauri::command]
#[specta::specta]
pub fn set_pill_side(app: AppHandle, side: PillSide) {
    let mut settings = get_settings(&app);
    if settings.pill_side != side {
        settings.pill_side = side;
        write_settings(&app, settings);
        place(&app);
    }
    emit_side(&app, side);
}

/// While dragging, the panel covers the whole primary display.
#[tauri::command]
#[specta::specta]
pub fn set_pill_dragging(app: AppHandle, dragging: bool) {
    if DRAGGING.swap(dragging, Ordering::SeqCst) != dragging {
        place(&app);
    }
}

/// Keeps a cancelled dictation's audio for Undo and tells the pill.
pub fn keep_cancelled(app: &AppHandle, binding_id: String, samples: Vec<f32>) {
    let token = KEPT_TOKEN.fetch_add(1, Ordering::SeqCst) + 1;
    if let Ok(mut kept) = KEPT.lock() {
        *kept = Some((token, binding_id, samples));
    }
    let _ = app.emit_to(
        "recording_overlay",
        "pill-cancelled",
        PillCancelled { undo_ms: UNDO_MS },
    );
    std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(KEEP_MAX_MS));
        if let Ok(mut kept) = KEPT.lock() {
            if kept.as_ref().is_some_and(|(t, _, _)| *t == token) {
                *kept = None;
            }
        }
    });
}

pub fn emit_result(app: &AppHandle, text: &str, inserted: bool) {
    let _ = app.emit_to(
        "recording_overlay",
        "pill-result",
        PillResult { text, inserted },
    );
}

/// Whether pasting would land in a text field. Anything Accessibility cannot
/// answer counts as yes, so pasting keeps working where it gives no answer.
/// Always yes while the pill is hidden, since nothing else would show the text.
pub fn text_field_has_focus(app: &AppHandle) -> bool {
    let settings = get_settings(app);
    if settings.paste_method == PasteMethod::None || settings.overlay_style == OverlayStyle::None {
        return true;
    }
    #[cfg(target_os = "macos")]
    {
        ax::focused_element_is_editable().unwrap_or(true)
    }
    #[cfg(not(target_os = "macos"))]
    {
        true
    }
}

pub fn start(app: &AppHandle) {
    if STARTED.swap(true, Ordering::SeqCst) {
        return;
    }
    let app = app.clone();
    std::thread::spawn(move || {
        let mut was_inside: Option<bool> = None;
        let mut last_cursor = (f64::NAN, f64::NAN);
        loop {
            std::thread::sleep(Duration::from_millis(POLL_MS));
            let Some(win) = app.get_webview_window("recording_overlay") else {
                continue;
            };
            let (Ok(cursor), Ok(origin), Ok(scale)) =
                (win.cursor_position(), win.outer_position(), win.scale_factor())
            else {
                continue;
            };
            let x = (cursor.x - origin.x as f64) / scale;
            let y = (cursor.y - origin.y as f64) / scale;
            let (rx, ry, rw, rh) = HIT_RECT.lock().map(|r| *r).unwrap_or_default();
            let hit = x >= rx && x <= rx + rw && y >= ry && y <= ry + rh;
            // A press on the pill keeps the mouse until release, so a drag
            // can leave the hit area without losing its events.
            let inside = hit || (was_inside == Some(true) && left_button_down());
            if was_inside != Some(inside) || RESEND.swap(false, Ordering::SeqCst) {
                was_inside = Some(inside);
                let _ = win.set_ignore_cursor_events(!inside);
                let _ = win.emit_to("recording_overlay", "pill-hover", inside);
            }
            // WebKit sends no mouse-move events to this non-activating panel, so the
            // page finds the button under the cursor from these positions.
            if inside && (x, y) != last_cursor {
                last_cursor = (x, y);
                let _ = win.emit_to("recording_overlay", "pill-cursor", (x, y));
            }
        }
    });
}

#[cfg(target_os = "macos")]
fn left_button_down() -> bool {
    #[link(name = "CoreGraphics", kind = "framework")]
    unsafe extern "C" {
        fn CGEventSourceButtonState(state: i32, button: u32) -> bool;
    }
    // SAFETY: a read-only query; 0 is the combined session state, 0 the left button.
    unsafe { CGEventSourceButtonState(0, 0) }
}

#[cfg(not(target_os = "macos"))]
fn left_button_down() -> bool {
    false
}

#[cfg(target_os = "macos")]
mod ax {
    use std::ffi::{c_char, c_void, CStr, CString};
    use std::time::Duration;

    type CfTypeRef = *const c_void;
    type AxError = i32;

    const AX_ERROR_NO_VALUE: AxError = -25212;
    const UTF8: u32 = 0x0800_0100;
    const EDITABLE_ROLES: [&str; 3] = ["AXTextField", "AXTextArea", "AXComboBox"];
    /// Focus on one of these is clearly not on text. Generic roles (window,
    /// group, unknown) are left out: terminals and half-built Electron trees
    /// report them for real text views.
    const NON_TEXT_ROLES: [&str; 14] = [
        "AXButton",
        "AXCheckBox",
        "AXRadioButton",
        "AXPopUpButton",
        "AXSlider",
        "AXIncrementor",
        "AXScrollBar",
        "AXColorWell",
        "AXList",
        "AXTable",
        "AXOutline",
        "AXImage",
        "AXLink",
        "AXWebArea",
    ];

    #[link(name = "ApplicationServices", kind = "framework")]
    unsafe extern "C" {
        fn AXUIElementCreateSystemWide() -> CfTypeRef;
        fn AXUIElementCopyAttributeValue(
            element: CfTypeRef,
            attribute: CfTypeRef,
            value: *mut CfTypeRef,
        ) -> AxError;
        fn AXUIElementSetAttributeValue(
            element: CfTypeRef,
            attribute: CfTypeRef,
            value: CfTypeRef,
        ) -> AxError;
        fn AXUIElementSetMessagingTimeout(element: CfTypeRef, seconds: f32) -> AxError;
    }

    #[link(name = "CoreFoundation", kind = "framework")]
    unsafe extern "C" {
        static kCFBooleanTrue: CfTypeRef;
        fn CFStringCreateWithCString(
            allocator: CfTypeRef,
            text: *const c_char,
            encoding: u32,
        ) -> CfTypeRef;
        fn CFStringGetCString(
            string: CfTypeRef,
            buffer: *mut c_char,
            size: isize,
            encoding: u32,
        ) -> u8;
        fn CFGetTypeID(value: CfTypeRef) -> usize;
        fn CFStringGetTypeID() -> usize;
        fn CFRelease(value: CfTypeRef);
    }

    /// An owned (+1) Core Foundation reference.
    struct Cf(CfTypeRef);

    impl Drop for Cf {
        fn drop(&mut self) {
            if !self.0.is_null() {
                // SAFETY: every Cf holds a reference returned by a Create/Copy call.
                unsafe { CFRelease(self.0) };
            }
        }
    }

    fn cf_string(text: &str) -> Cf {
        let text = CString::new(text).unwrap_or_default();
        // SAFETY: text is a valid NUL-terminated UTF-8 string.
        Cf(unsafe { CFStringCreateWithCString(std::ptr::null(), text.as_ptr(), UTF8) })
    }

    fn copy(element: &Cf, attribute: &str) -> Result<Cf, AxError> {
        let name = cf_string(attribute);
        let mut value: CfTypeRef = std::ptr::null();
        // SAFETY: element and name are live references; value receives a +1 reference.
        let error = unsafe { AXUIElementCopyAttributeValue(element.0, name.0, &mut value) };
        match (error, value.is_null()) {
            (0, false) => Ok(Cf(value)),
            (0, true) => Err(AX_ERROR_NO_VALUE),
            _ => Err(error),
        }
    }

    fn string(value: &Cf) -> Option<String> {
        // SAFETY: value is a live CF reference, read as a string only after
        // its type is checked; the buffer is NUL-terminated on success.
        unsafe {
            if CFGetTypeID(value.0) != CFStringGetTypeID() {
                return None;
            }
            let mut buffer = [0 as c_char; 64];
            if CFStringGetCString(value.0, buffer.as_mut_ptr(), buffer.len() as isize, UTF8) == 0 {
                return None;
            }
            Some(CStr::from_ptr(buffer.as_ptr()).to_string_lossy().into_owned())
        }
    }

    /// Some(true) when the focused element takes text, Some(false) when focus is
    /// clearly on something else, None when Accessibility gives no clear answer.
    pub fn focused_element_is_editable() -> Option<bool> {
        // SAFETY: returns a +1 reference, released by Cf.
        let system = Cf(unsafe { AXUIElementCreateSystemWide() });
        // SAFETY: system is a live AXUIElement.
        unsafe { AXUIElementSetMessagingTimeout(system.0, 0.5) };
        let app = copy(&system, "AXFocusedApplication").ok()?;
        let focused = match copy(&app, "AXFocusedUIElement") {
            Ok(focused) => focused,
            Err(AX_ERROR_NO_VALUE) => {
                // Chromium and Electron apps build their accessibility tree
                // only once asked to.
                let name = cf_string("AXManualAccessibility");
                // SAFETY: app and name are live; kCFBooleanTrue is a CF constant.
                unsafe { AXUIElementSetAttributeValue(app.0, name.0, kCFBooleanTrue) };
                std::thread::sleep(Duration::from_millis(100));
                match copy(&app, "AXFocusedUIElement") {
                    Ok(focused) => focused,
                    Err(AX_ERROR_NO_VALUE) => {
                        log::debug!("focus check: nothing focused -> popup");
                        return Some(false);
                    }
                    Err(error) => {
                        log::debug!("focus check: AX error {error} -> paste");
                        return None;
                    }
                }
            }
            Err(error) => {
                log::debug!("focus check: AX error {error} -> paste");
                return None;
            }
        };
        if copy(&focused, "AXEditableAncestor").is_ok() {
            return Some(true);
        }
        let role = copy(&focused, "AXRole").ok().and_then(|role| string(&role))?;
        let editable = if EDITABLE_ROLES.contains(&role.as_str()) {
            Some(true)
        } else if NON_TEXT_ROLES.contains(&role.as_str()) {
            Some(false)
        } else {
            None
        };
        log::debug!("focus check: role {role} -> {editable:?}");
        editable
    }
}
