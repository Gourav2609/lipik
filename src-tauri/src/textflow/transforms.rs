//! ⌥1..⌥9 transforms: copy the selection, rewrite it with the local model and
//! paste the result over it.

use super::llm;
use super::pipeline::transform_system_prompt;
use super::store::TextFlow;
use crate::actions::ShortcutAction;
use crate::settings::{ClipboardHandling, ShortcutBinding};
use serde::Serialize;
use std::collections::HashSet;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;
use std::time::{Duration, Instant};
use tauri::{AppHandle, Emitter, Manager};
use tauri_plugin_clipboard_manager::ClipboardExt;

const TRANSFORM_TIMEOUT: Duration = Duration::from_secs(60);

static READY: AtomicBool = AtomicBool::new(false);
static REGISTERED: Mutex<Option<HashSet<u32>>> = Mutex::new(None);
static BUSY: AtomicBool = AtomicBool::new(false);

pub struct TransformAction(pub u32);

impl ShortcutAction for TransformAction {
    fn start(&self, app: &AppHandle, _binding_id: &str, _shortcut_str: &str) {
        run_shortcut(app, self.0);
    }

    fn stop(&self, _app: &AppHandle, _binding_id: &str, _shortcut_str: &str) {}
}

fn binding(index: u32) -> ShortcutBinding {
    #[cfg(target_os = "macos")]
    let keys = format!("option+{index}");
    #[cfg(not(target_os = "macos"))]
    let keys = format!("alt+{index}");
    ShortcutBinding {
        id: format!("transform_{index}"),
        name: format!("Transform {index}"),
        description: "Run a transform on the selected text".to_string(),
        default_binding: keys.clone(),
        current_binding: keys,
    }
}

/// Called once the app's shortcuts are initialized.
pub fn shortcuts_ready(app: &AppHandle) {
    READY.store(true, Ordering::SeqCst);
    sync_shortcuts(app);
}

/// Registers ⌥n for every transform with a shortcut while transforms are
/// opted in, and unregisters the rest.
pub fn sync_shortcuts(app: &AppHandle) {
    if !READY.load(Ordering::SeqCst) {
        return;
    }
    let Some(tf) = app.try_state::<TextFlow>() else {
        return;
    };
    let wanted: HashSet<u32> = tf.read(|d| {
        if !d.transforms.opt_in {
            return HashSet::new();
        }
        d.transforms
            .items
            .iter()
            .filter_map(|t| t.shortcut_index)
            .filter(|n| (1..=9).contains(n))
            .collect()
    });
    let mut registered = REGISTERED.lock().unwrap_or_else(|e| e.into_inner());
    let current = registered.get_or_insert_with(HashSet::new);
    for index in current.difference(&wanted).copied().collect::<Vec<_>>() {
        match crate::shortcut::unregister_shortcut(app, binding(index)) {
            Ok(()) => {
                current.remove(&index);
            }
            Err(e) => log::warn!("Failed to unregister transform shortcut {index}: {e}"),
        }
    }
    for index in wanted
        .difference(&current.clone())
        .copied()
        .collect::<Vec<_>>()
    {
        match crate::shortcut::register_shortcut(app, binding(index)) {
            Ok(()) => {
                current.insert(index);
            }
            Err(e) => log::warn!("Failed to register transform shortcut {index}: {e}"),
        }
    }
}

#[derive(Serialize, Clone)]
struct TransformStatus {
    /// running, done, no_selection or error
    state: &'static str,
    name: String,
    error: Option<String>,
}

fn emit_status(app: &AppHandle, state: &'static str, name: &str, error: Option<String>) {
    let _ = app.emit(
        "transform-status",
        TransformStatus {
            state,
            name: name.to_string(),
            error,
        },
    );
}

fn run_shortcut(app: &AppHandle, index: u32) {
    if BUSY.swap(true, Ordering::SeqCst) {
        return;
    }
    let app = app.clone();
    let spawned = std::thread::Builder::new()
        .name("transform".into())
        .spawn(move || {
            run(&app, index);
            BUSY.store(false, Ordering::SeqCst);
        });
    if spawned.is_err() {
        BUSY.store(false, Ordering::SeqCst);
    }
}

fn run(app: &AppHandle, index: u32) {
    let Some(tf) = app.try_state::<TextFlow>() else {
        return;
    };
    let transform = tf.read(|d| {
        d.transforms
            .opt_in
            .then(|| {
                d.transforms
                    .items
                    .iter()
                    .find(|t| t.shortcut_index == Some(index))
                    .cloned()
            })
            .flatten()
    });
    let Some(transform) = transform else {
        return;
    };

    wait_for_modifiers_release();
    let clipboard = app.clipboard();
    let saved = save_clipboard(app);
    let _ = clipboard.clear();
    if let Err(e) = send_copy(app) {
        restore(app, saved);
        emit_status(app, "error", &transform.name, Some(e));
        return;
    }
    let deadline = Instant::now() + Duration::from_millis(600);
    let mut selection = None;
    while Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(30));
        if let Ok(text) = clipboard.read_text() {
            if !text.trim().is_empty() {
                selection = Some(text);
                break;
            }
        }
    }
    let Some(selection) = selection else {
        restore(app, saved);
        emit_status(app, "no_selection", &transform.name, None);
        return;
    };

    emit_status(app, "running", &transform.name, None);
    crate::utils::show_processing_overlay(app);
    let result = tauri::async_runtime::block_on(llm::chat(
        &llm::endpoint(app),
        &transform_system_prompt(&transform.prompt),
        &selection,
        TRANSFORM_TIMEOUT,
    ));
    crate::utils::hide_recording_overlay(app);

    let output = match result {
        Ok(out) if !out.trim().is_empty() => out,
        Ok(_) => {
            restore(app, saved);
            emit_status(app, "error", &transform.name, Some("Empty result".into()));
            return;
        }
        Err(e) => {
            restore(app, saved);
            emit_status(app, "error", &transform.name, Some(e));
            return;
        }
    };

    // paste() keeps whatever is on the clipboard when it starts, so put the
    // user's original clipboard back first.
    restore(app, saved.clone());
    match crate::utils::paste(output, app.clone()) {
        Ok(()) => emit_status(app, "done", &transform.name, None),
        Err(e) => emit_status(app, "error", &transform.name, Some(e)),
    }
    if crate::settings::get_settings(app).clipboard_handling == ClipboardHandling::DontModify {
        restore(app, saved);
    }
}

#[derive(Clone)]
enum Saved {
    Text(String),
    Image(tauri::image::Image<'static>),
    Empty,
}

/// What the clipboard held before the copy. An image is read only when there
/// is no text, as Lipik's paste does.
fn save_clipboard(app: &AppHandle) -> Saved {
    let clipboard = app.clipboard();
    if let Some(text) = clipboard.read_text().ok().filter(|t| !t.is_empty()) {
        return Saved::Text(text);
    }
    match clipboard.read_image() {
        Ok(image) => Saved::Image(image.to_owned()),
        Err(_) => Saved::Empty,
    }
}

fn restore(app: &AppHandle, saved: Saved) {
    let clipboard = app.clipboard();
    let _ = match saved {
        Saved::Text(text) => clipboard.write_text(text),
        Saved::Image(image) => clipboard.write_image(&image),
        Saved::Empty => clipboard.clear(),
    };
}

fn send_copy(app: &AppHandle) -> Result<(), String> {
    use enigo::{Direction, Key, Keyboard};

    // kVK_ANSI_C: the physical C key, as Lipik used for V before it resolved
    // the keyboard layout.
    #[cfg(target_os = "macos")]
    let (modifier, c_key) = (Key::Meta, Key::Other(8));
    #[cfg(target_os = "windows")]
    let (modifier, c_key) = (Key::Control, Key::Other(0x43));
    #[cfg(target_os = "linux")]
    let (modifier, c_key) = (Key::Control, Key::Unicode('c'));

    let (tx, rx) = std::sync::mpsc::sync_channel(1);
    let app_for_main = app.clone();
    app.run_on_main_thread(move || {
        let result = (|| {
            let state = app_for_main
                .try_state::<crate::input::EnigoState>()
                .ok_or("Keyboard input is not ready yet")?;
            let mut enigo = state.0.lock().map_err(|e| e.to_string())?;
            enigo
                .key(modifier, Direction::Press)
                .map_err(|e| e.to_string())?;
            let clicked = enigo.key(c_key, Direction::Click);
            let released = enigo.key(modifier, Direction::Release);
            clicked.and(released).map_err(|e| e.to_string())
        })();
        let _ = tx.send(result);
    })
    .map_err(|e| e.to_string())?;
    rx.recv().map_err(|_| "Copy was not sent".to_string())?
}

/// The shortcut's ⌥ is usually still down when it fires; a copy sent then
/// would reach the app as ⌥⌘C.
#[cfg(target_os = "macos")]
fn wait_for_modifiers_release() {
    #[link(name = "CoreGraphics", kind = "framework")]
    unsafe extern "C" {
        fn CGEventSourceFlagsState(state: i32) -> u64;
    }
    const HID_SYSTEM_STATE: i32 = 1;
    const MODIFIERS: u64 = 0x0008_0000 | 0x0004_0000 | 0x0002_0000 | 0x0010_0000;
    let deadline = Instant::now() + Duration::from_millis(1500);
    // SAFETY: plain query of the current keyboard modifier state.
    while unsafe { CGEventSourceFlagsState(HID_SYSTEM_STATE) } & MODIFIERS != 0
        && Instant::now() < deadline
    {
        std::thread::sleep(Duration::from_millis(15));
    }
}

#[cfg(not(target_os = "macos"))]
fn wait_for_modifiers_release() {
    std::thread::sleep(Duration::from_millis(150));
}
