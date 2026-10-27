//! Asks GitHub which Lipik release is the latest, so the app can say when a
//! newer one is out. The request carries nothing about the user; it can be
//! turned off in Settings (`release_check_enabled`).

use crate::settings::{get_settings, write_settings};
use serde::Serialize;
use specta::Type;
use std::time::Duration;
use tauri::AppHandle;

const LATEST: &str = "https://api.github.com/repos/Gourav2609/lipik/releases/latest";

#[derive(Clone, Debug, Serialize, Type)]
pub struct NewRelease {
    pub version: String,
    /// The release page, with the notes and the download.
    pub url: String,
}

/// "0.1.10" is newer than "0.1.9"; anything that isn't plain numbers is not.
fn newer(candidate: &str, current: &str) -> bool {
    let parts = |v: &str| -> Option<Vec<u64>> {
        v.trim_start_matches('v')
            .split('.')
            .map(|p| p.parse().ok())
            .collect()
    };
    match (parts(candidate), parts(current)) {
        (Some(a), Some(b)) => a > b,
        _ => false,
    }
}

#[tauri::command]
#[specta::specta]
pub async fn check_for_new_release(app: AppHandle) -> Option<NewRelease> {
    if !get_settings(&app).release_check_enabled {
        return None;
    }
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(10))
        .user_agent(format!("Lipik/{}", env!("CARGO_PKG_VERSION")))
        .build()
        .ok()?;
    let release: serde_json::Value = client
        .get(LATEST)
        .send()
        .await
        .ok()?
        .error_for_status()
        .ok()?
        .json()
        .await
        .ok()?;
    let version = release["tag_name"].as_str()?.trim_start_matches('v');
    if !newer(version, env!("CARGO_PKG_VERSION")) {
        return None;
    }
    Some(NewRelease {
        version: version.to_string(),
        url: release["html_url"].as_str()?.to_string(),
    })
}

#[tauri::command]
#[specta::specta]
pub fn change_release_check_setting(app: AppHandle, enabled: bool) {
    let mut settings = get_settings(&app);
    settings.release_check_enabled = enabled;
    write_settings(&app, settings);
}

#[cfg(test)]
mod tests {
    use super::newer;

    #[test]
    fn compares_versions_as_numbers() {
        assert!(newer("0.1.10", "0.1.9"));
        assert!(newer("v0.2.0", "0.1.9"));
        assert!(!newer("0.1.2", "0.1.2"));
        assert!(!newer("0.1.1", "0.1.2"));
        assert!(!newer("0.2.0-beta", "0.1.2"));
    }
}
