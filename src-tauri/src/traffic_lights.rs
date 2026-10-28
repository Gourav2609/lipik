//! Places the macOS window buttons 16 pt from the left, centred in the 52 pt
//! header row. tao only applies `traffic_light_position` when
//! its own view draws, which never happens under a full-window webview, and
//! macOS lays the buttons out again on resize and focus changes, so this runs
//! after the window is shown and on those events.

use objc2::msg_send;
use objc2::runtime::AnyObject;
use objc2_foundation::NSRect;
use std::time::Duration;
use tauri::Manager;

const LEFT: f64 = 16.0;
const BAR_HEIGHT: f64 = 52.0;

pub fn place(window: &tauri::Window) {
    place_in(window, BAR_HEIGHT);
}

/// A light/dark switch makes macOS lay the title bar out again a moment
/// later, which puts the buttons back in its default spot.
pub fn place_after_theme_change(app: &tauri::AppHandle) {
    let app = app.clone();
    std::thread::spawn(move || {
        for ms in [0, 60, 250] {
            std::thread::sleep(Duration::from_millis(ms));
            if let Some(w) = app.get_webview_window("main") {
                place(&w.as_ref().window());
            }
            if let Some(w) = app.get_webview_window("notepad") {
                crate::notepad::place_traffic_lights(&w.as_ref().window());
            }
        }
    });
}

/// Same placement in a top bar `bar_height` pt tall.
pub fn place_in(window: &tauri::Window, bar_height: f64) {
    let Ok(ns_window) = window.ns_window() else {
        return;
    };
    let ns_window = ns_window as usize;
    let _ = window.run_on_main_thread(move || unsafe {
        place_on_main(ns_window as *mut AnyObject, bar_height)
    });
}

unsafe fn place_on_main(ns_window: *mut AnyObject, bar_height: f64) {
    let buttons: [*mut AnyObject; 3] = [
        msg_send![ns_window, standardWindowButton: 0usize],
        msg_send![ns_window, standardWindowButton: 1usize],
        msg_send![ns_window, standardWindowButton: 2usize],
    ];
    if buttons.iter().any(|b| b.is_null()) {
        return;
    }
    let title_bar: *mut AnyObject = msg_send![buttons[0], superview];
    let container: *mut AnyObject = msg_send![title_bar, superview];
    if title_bar.is_null() || container.is_null() {
        return;
    }

    let window_frame: NSRect = msg_send![ns_window, frame];
    let mut bar: NSRect = msg_send![container, frame];
    bar.size.height = bar_height;
    bar.origin.y = window_frame.size.height - bar_height;
    let _: () = msg_send![container, setFrame: bar];
    let mut inner: NSRect = msg_send![title_bar, frame];
    inner.origin.y = 0.0;
    inner.size.height = bar_height;
    let _: () = msg_send![title_bar, setFrame: inner];

    let first: NSRect = msg_send![buttons[0], frame];
    let second: NSRect = msg_send![buttons[1], frame];
    let gap = second.origin.x - first.origin.x;
    for (i, button) in buttons.iter().enumerate() {
        let mut frame: NSRect = msg_send![*button, frame];
        frame.origin.x = LEFT + gap * i as f64;
        frame.origin.y = (bar_height - frame.size.height) / 2.0;
        let _: () = msg_send![*button, setFrameOrigin: frame.origin];
    }
}
