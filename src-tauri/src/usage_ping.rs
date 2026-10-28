//! Every few hours, tells lipik.refflux.dev that this copy of Lipik is in use, so
//! we know how many people use it. The ping carries exactly four things: a
//! random ID made on first launch (`install_id`), the app version, the macOS
//! version and the chip type ("arm64" or "x86_64"). Never audio, text, names
//! or file paths. It can be turned off in Settings (`usage_ping_enabled`).
//!
//! Nothing is sent before setup is finished, and debug builds never ping.
//!
//! Setup can also send an email address, on its own, when the user asks to
//! hear about new versions.

use crate::settings::{get_settings, write_settings};
use log::debug;
use serde_json::{json, Value};
use std::time::{Duration, SystemTime};
use tauri::AppHandle;

const PING: &str = "https://lipik.refflux.dev/api/ping";
const EMAIL: &str = "https://lipik.refflux.dev/api/email";
const FIRST_WAIT: Duration = Duration::from_secs(60);
const CHECK_EVERY: Duration = Duration::from_secs(15 * 60);
const SEND_EVERY: Duration = Duration::from_secs(3 * 60 * 60);

async fn post(url: &str, body: Value) -> Result<reqwest::Response, reqwest::Error> {
    reqwest::Client::builder()
        .timeout(Duration::from_secs(10))
        .user_agent(format!("Lipik/{}", env!("CARGO_PKG_VERSION")))
        .build()?
        .post(url)
        .json(&body)
        .send()
        .await
}

#[cfg(target_os = "macos")]
fn os_version() -> String {
    std::process::Command::new("sw_vers")
        .arg("-productVersion")
        .output()
        .ok()
        .and_then(|out| String::from_utf8(out.stdout).ok())
        .map(|v| v.trim().to_string())
        .unwrap_or_default()
}

#[cfg(not(target_os = "macos"))]
fn os_version() -> String {
    tauri_plugin_os::version().to_string()
}

fn arch() -> &'static str {
    match std::env::consts::ARCH {
        "aarch64" => "arm64",
        other => other,
    }
}

fn ping_body(install_id: &str) -> Value {
    json!({
        "id": install_id,
        "version": env!("CARGO_PKG_VERSION"),
        "os": os_version(),
        "arch": arch(),
    })
}

/// Sends the ping a minute after start-up, then whenever three hours have
/// passed since the server last answered (any status: a rejected ping is not
/// retried early). Wall-clock time, so a Mac that slept catches up within one
/// check.
pub fn start(app: &AppHandle) {
    if cfg!(debug_assertions) {
        return;
    }
    let app = app.clone();
    std::thread::spawn(move || {
        std::thread::sleep(FIRST_WAIT);
        let mut last_sent: Option<SystemTime> = None;
        loop {
            let due = last_sent.is_none_or(|t| t.elapsed().map_or(true, |e| e >= SEND_EVERY));
            let settings = get_settings(&app);
            if due && settings.usage_ping_enabled && settings.setup_finished {
                match tauri::async_runtime::block_on(post(PING, ping_body(&settings.install_id))) {
                    Ok(_) => last_sent = Some(SystemTime::now()),
                    Err(e) => debug!("Usage ping failed: {e}"),
                }
            }
            std::thread::sleep(CHECK_EVERY);
        }
    });
}

/// The server's check: `^[^\s@]{1,64}@[^\s@]+\.[^\s@]{2,}$`, at most 254
/// characters. Lengths count UTF-16 units, as JavaScript does.
fn looks_like_email(email: &str) -> bool {
    let units = |s: &str| s.encode_utf16().count();
    if units(email) > 254 || email.chars().any(char::is_whitespace) {
        return false;
    }
    let Some((local, domain)) = email.split_once('@') else {
        return false;
    };
    (1..=64).contains(&units(local))
        && !domain.contains('@')
        && domain
            .char_indices()
            .any(|(i, c)| c == '.' && i > 0 && units(&domain[i + 1..]) >= 2)
}

#[tauri::command]
#[specta::specta]
pub async fn submit_update_email(email: String, updates: bool) -> Result<(), String> {
    let email = email.trim();
    if !looks_like_email(email) {
        return Err("That doesn't look like an email address.".to_string());
    }
    let body = json!({ "email": email, "updates": updates });
    post(EMAIL, body)
        .await
        .and_then(reqwest::Response::error_for_status)
        .map(drop)
        .map_err(|e| {
            debug!("Saving the update email failed: {e}");
            "Couldn't save your email, try again.".to_string()
        })
}

#[tauri::command]
#[specta::specta]
pub fn change_usage_ping_setting(app: AppHandle, enabled: bool) {
    let mut settings = get_settings(&app);
    settings.usage_ping_enabled = enabled;
    write_settings(&app, settings);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn checks_email_shape() {
        assert!(looks_like_email("sam@example.com"));
        assert!(looks_like_email("sam.lee+news@mail.example.co.in"));
        assert!(!looks_like_email(""));
        assert!(!looks_like_email("sam"));
        assert!(!looks_like_email("sam@example"));
        assert!(!looks_like_email("@example.com"));
        assert!(!looks_like_email("sam@@example.com"));
        assert!(!looks_like_email("sam@a@example.com"));
        assert!(!looks_like_email("sam@example."));
        assert!(!looks_like_email("sam@x.c"));
        assert!(!looks_like_email(&format!(
            "{}@example.com",
            "a".repeat(65)
        )));
        assert!(looks_like_email(&format!("{}@example.com", "a".repeat(64))));
        assert!(!looks_like_email("sam lee@example.com"));
        assert!(!looks_like_email(&format!("a@{}.com", "b".repeat(250))));
    }

    #[test]
    fn ping_sends_only_id_version_os_and_arch() {
        let body = ping_body("id-1");
        let keys: Vec<&str> = body
            .as_object()
            .unwrap()
            .keys()
            .map(String::as_str)
            .collect();
        assert_eq!(keys, ["id", "version", "os", "arch"]);
        assert_eq!(body["id"], "id-1");
        assert!(["arm64", "x86_64"].contains(&body["arch"].as_str().unwrap()));
    }
}
