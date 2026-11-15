//! Every few hours, tells lipik.pages.dev that this copy of Lipik is in use, so
//! we know how many people use it and how. The ping carries a random ID made
//! on first launch (`install_id`), the app version, the macOS version, the
//! chip type ("arm64" or "x86_64") and how far setup got (`setup_step`).
//! Since 0.1.17 also how much it is used (`Totals`: counts of dictations,
//! words, meetings and meeting minutes) and five settings (`picks`: speech
//! model, speech engine, language, clean-up, meeting notes on or off). Never
//! audio, text, names or file paths. It can be turned off in Settings
//! (`usage_ping_enabled`).
//!
//! Nothing is sent until the user answers the tick box on the welcome
//! screen. With it ticked, pings start then, before the rest of setup, so we
//! can see where setup stops. Someone who ticked it before 0.1.17 agreed to
//! less: the counts and settings are added only once they have seen the
//! "What's new" note that says so (`told_about_details`). Debug builds never
//! ping.
//!
//! Setup can also send an email address, on its own, when the user asks to
//! hear about new versions.

use crate::settings::{get_settings, update_settings, AppSettings, SetupStep};
use log::debug;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::path::PathBuf;
use std::sync::Mutex;
use std::time::{Duration, SystemTime};
use tauri::AppHandle;

const PING: &str = "https://lipik.pages.dev/api/ping";
const EMAIL: &str = "https://lipik.pages.dev/api/email";
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

/// The first version whose "What's new" note and welcome screen say that
/// counts and settings are sent.
const DETAILS_SINCE: (u32, u32, u32) = (0, 1, 17);
const TOTALS_FILE: &str = "usage.json";
static TOTALS: Mutex<()> = Mutex::new(());

/// How much this copy has been used since 0.1.17, kept in the data folder.
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Totals {
    pub dictations: u64,
    pub words: u64,
    pub meetings: u64,
    pub meeting_minutes: u64,
}

fn totals_path(app: &AppHandle) -> Option<PathBuf> {
    crate::portable::app_data_dir(app)
        .ok()
        .map(|dir| dir.join(TOTALS_FILE))
}

fn read_totals(path: &std::path::Path) -> Totals {
    std::fs::read_to_string(path)
        .ok()
        .and_then(|text| serde_json::from_str(&text).ok())
        .unwrap_or_default()
}

fn add_to_totals(app: &AppHandle, add: impl FnOnce(&mut Totals)) {
    let Some(path) = totals_path(app) else {
        return;
    };
    let _one_at_a_time = TOTALS.lock().unwrap_or_else(|e| e.into_inner());
    let mut totals = read_totals(&path);
    add(&mut totals);
    let written = serde_json::to_string(&totals)
        .map_err(std::io::Error::other)
        .and_then(|text| std::fs::write(&path, text));
    if let Err(e) = written {
        debug!("Could not save the usage counts: {e}");
    }
}

/// Counts one finished dictation of `words` words.
pub fn count_dictation(app: &AppHandle, words: i64) {
    add_to_totals(app, |t| {
        t.dictations += 1;
        t.words += words.max(0) as u64;
    });
}

/// Counts one recorded meeting that ran for `ms`.
pub fn count_meeting(app: &AppHandle, ms: i64) {
    add_to_totals(app, |t| {
        t.meetings += 1;
        t.meeting_minutes += (ms.max(0) as u64 + 30_000) / 60_000;
    });
}

/// Whether the user has been told that counts and settings are sent: the
/// welcome screen of a fresh install says so (and stamps the running
/// version as seen), an update says so in its "What's new" note.
fn told_about_details(settings: &AppSettings) -> bool {
    let mut parts = settings
        .whats_new_last_seen_version
        .split(['.', '-'])
        .map(|part| part.parse::<u32>().unwrap_or(0));
    let seen = (
        parts.next().unwrap_or(0),
        parts.next().unwrap_or(0),
        parts.next().unwrap_or(0),
    );
    seen >= DETAILS_SINCE
}

/// The five settings sent with the counts: names of choices, never text. A
/// model file the user added themselves is sent as "custom", not by name.
fn picks(settings: &AppSettings, custom_model: bool) -> Value {
    json!({
        "model": if custom_model { "custom" } else { settings.selected_model.as_str() },
        "engine": settings.speech_engine,
        "language": settings.selected_language,
        "cleanup": settings.cleanup_engine,
        "meeting_notes": settings.notetaker.detect_calls,
    })
}

fn has_custom_model(app: &AppHandle, settings: &AppSettings) -> bool {
    use tauri::Manager;
    app.try_state::<std::sync::Arc<crate::managers::model::ModelManager>>()
        .and_then(|models| models.get_model_info(&settings.selected_model))
        .is_some_and(|model| model.is_custom)
}

fn ping_body(settings: &AppSettings, details: Option<(Totals, bool)>) -> Value {
    let mut body = json!({
        "id": settings.install_id,
        "version": env!("CARGO_PKG_VERSION"),
        "os": os_version(),
        "arch": arch(),
        "step": settings.setup_step,
    });
    if let Some((totals, custom_model)) = details {
        body["use"] = json!(totals);
        body["picks"] = picks(settings, custom_model);
    }
    body
}

fn may_ping(settings: &AppSettings) -> bool {
    settings.usage_ping_answered && settings.usage_ping_enabled
}

/// Records that setup got as far as `step`. An earlier step never replaces a
/// later one.
pub fn reach_step(app: &AppHandle, step: SetupStep) {
    update_settings(app, |settings| {
        moves_forward(settings.setup_step, step)
            .map(|step| settings.setup_step = step)
            .is_some()
    });
}

fn moves_forward(current: SetupStep, reached: SetupStep) -> Option<SetupStep> {
    (reached > current).then_some(reached)
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
            if due && may_ping(&settings) {
                let details = told_about_details(&settings)
                    .then(|| totals_path(&app))
                    .flatten()
                    .map(|path| (read_totals(&path), has_custom_model(&app, &settings)));
                let body = ping_body(&settings, details);
                match tauri::async_runtime::block_on(post(PING, body)) {
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
pub fn reach_setup_step(app: AppHandle, step: SetupStep) {
    reach_step(&app, step);
}

/// The welcome screen's tick box, sent when the user presses Continue.
#[tauri::command]
#[specta::specta]
pub fn answer_usage_ping(app: AppHandle, enabled: bool) {
    update_settings(&app, |settings| {
        settings.usage_ping_answered = true;
        settings.usage_ping_enabled = enabled;
        true
    });
}

#[tauri::command]
#[specta::specta]
pub fn change_usage_ping_setting(app: AppHandle, enabled: bool) {
    update_settings(&app, |settings| {
        settings.usage_ping_enabled = enabled;
        true
    });
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

    fn settings(seen: &str) -> AppSettings {
        let mut settings = crate::settings::get_default_settings();
        settings.install_id = "id-1".into();
        settings.setup_step = SetupStep::ModelReady;
        settings.whats_new_last_seen_version = seen.into();
        settings
    }

    #[test]
    fn ping_sends_only_id_version_os_arch_and_step_until_told_about_more() {
        let body = ping_body(&settings("0.1.16"), None);
        let keys: Vec<&str> = body
            .as_object()
            .unwrap()
            .keys()
            .map(String::as_str)
            .collect();
        assert_eq!(keys, ["id", "version", "os", "arch", "step"]);
        assert_eq!(body["id"], "id-1");
        assert_eq!(body["step"], "model_ready");
        assert!(["arm64", "x86_64"].contains(&body["arch"].as_str().unwrap()));
    }

    #[test]
    fn counts_and_settings_are_names_and_numbers_only() {
        let totals = Totals {
            dictations: 12,
            words: 340,
            meetings: 2,
            meeting_minutes: 61,
        };
        let body = ping_body(&settings("0.1.17"), Some((totals, false)));
        assert_eq!(
            body["use"],
            json!({"dictations": 12, "words": 340, "meetings": 2, "meeting_minutes": 61})
        );
        let picks = body["picks"].as_object().unwrap();
        let keys: Vec<&str> = picks.keys().map(String::as_str).collect();
        assert_eq!(
            keys,
            ["model", "engine", "language", "cleanup", "meeting_notes"]
        );
        assert_eq!(picks["engine"], "auto");
        assert!(picks["meeting_notes"].is_boolean());
        let own_file = ping_body(&settings("0.1.17"), Some((totals, true)));
        assert_eq!(own_file["picks"]["model"], "custom");
    }

    #[test]
    fn details_wait_for_the_note_that_announces_them() {
        for (seen, told) in [
            ("", false),
            ("0.1.16", false),
            ("0.1.17", true),
            ("0.1.17-beta.1", true),
            ("0.2.0", true),
            ("1.0.0", true),
            ("nonsense", false),
        ] {
            assert_eq!(told_about_details(&settings(seen)), told, "{seen:?}");
        }
    }

    #[test]
    fn totals_survive_a_missing_or_broken_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(TOTALS_FILE);
        assert_eq!(read_totals(&path), Totals::default());
        std::fs::write(&path, "{").unwrap();
        assert_eq!(read_totals(&path), Totals::default());
        std::fs::write(&path, r#"{"dictations":3,"later_field":1}"#).unwrap();
        assert_eq!(read_totals(&path).dictations, 3);
        assert_eq!(read_totals(&path).words, 0);
    }

    #[test]
    fn setup_steps_are_in_order_and_named_as_the_server_expects() {
        let steps = [
            SetupStep::Installed,
            SetupStep::Permissions,
            SetupStep::ModelReady,
            SetupStep::FirstDictation,
        ];
        assert!(steps.windows(2).all(|w| w[0] < w[1]));
        let names: Vec<Value> = steps.iter().map(|s| json!(s)).collect();
        assert_eq!(
            names,
            ["installed", "permissions", "model_ready", "first_dictation"]
        );
        assert_eq!(SetupStep::default(), SetupStep::Installed);
    }

    #[test]
    fn no_ping_until_the_welcome_tick_box_is_answered_yes() {
        let mut settings = crate::settings::get_default_settings();
        assert!(settings.usage_ping_enabled);
        assert!(!may_ping(&settings));
        settings.usage_ping_answered = true;
        assert!(may_ping(&settings));
        settings.usage_ping_enabled = false;
        assert!(!may_ping(&settings));
    }

    #[test]
    fn setup_step_never_goes_back() {
        let mut step = SetupStep::Installed;
        for reached in [
            SetupStep::ModelReady,
            SetupStep::Permissions,
            SetupStep::Installed,
            SetupStep::FirstDictation,
            SetupStep::ModelReady,
        ] {
            if let Some(next) = moves_forward(step, reached) {
                step = next;
            }
        }
        assert_eq!(step, SetupStep::FirstDictation);
        assert_eq!(
            moves_forward(SetupStep::ModelReady, SetupStep::Permissions),
            None
        );
        assert_eq!(
            moves_forward(SetupStep::ModelReady, SetupStep::ModelReady),
            None
        );
        assert_eq!(
            moves_forward(SetupStep::Permissions, SetupStep::ModelReady),
            Some(SetupStep::ModelReady)
        );
    }
}
