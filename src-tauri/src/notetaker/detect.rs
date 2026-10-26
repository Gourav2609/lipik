//! Call detection (which call apps hold the microphone) and the heads-up
//! before calendar meetings. Polls every 2 s; reads only, never records.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tauri_specta::Event;

use super::{
    calendar, now_ms, settings, DetectedCall, NotetakerCallDetected, NotetakerCallEnded,
    NotetakerManager, NotetakerMeetingSoon,
};

const POLL: Duration = Duration::from_secs(2);
/// Polls in a row before a call counts as started or ended.
const CONFIRM_POLLS: u32 = 2;
/// Polls without the microphone before the call a recording is linked to
/// counts as ended (~46 s): some apps release the microphone while muted.
const LINKED_END_POLLS: u32 = 23;
const CALENDAR_REFRESH: Duration = Duration::from_secs(60);

/// Bundle id prefixes of apps whose microphone use means a call.
const CALL_APPS: &[(&str, &str)] = &[
    ("us.zoom.xos", "Zoom"),
    ("com.microsoft.teams2", "Microsoft Teams"),
    ("com.microsoft.teams", "Microsoft Teams"),
    ("com.tinyspeck.slackmacgap", "Slack"),
    ("com.apple.FaceTime", "FaceTime"),
    ("com.apple.avconferenced", "FaceTime"),
    ("com.cisco.webexmeetingsapp", "Webex"),
    ("com.webex.meetingmanager", "Webex"),
    ("Cisco-Systems.Spark", "Webex"),
    ("com.google.Chrome", "Google Chrome"),
    ("com.apple.Safari", "Safari"),
    ("com.apple.WebKit.GPU", "Safari"),
    ("company.thebrowser.Browser", "Arc"),
    ("com.brave.Browser", "Brave"),
    ("com.microsoft.edgemac", "Microsoft Edge"),
    ("org.mozilla.firefox", "Firefox"),
    ("org.chromium.Chromium", "Chromium"),
    ("com.vivaldi.Vivaldi", "Vivaldi"),
    ("com.operasoftware.Opera", "Opera"),
    ("company.thebrowser.dia", "Dia"),
    ("app.zen-browser.zen", "Zen"),
    ("com.hnc.Discord", "Discord"),
];

/// The call app a bundle id belongs to: the app itself or one of its helpers
/// (browsers capture the microphone in a helper process).
pub fn call_app(bundle_id: &str) -> Option<(&'static str, &'static str)> {
    CALL_APPS.iter().copied().find(|(prefix, _)| {
        bundle_id == *prefix
            || bundle_id
                .strip_prefix(prefix)
                .is_some_and(|rest| rest.starts_with('.'))
    })
}

/// Apps seen using the microphone that are not call apps, logged once each.
#[cfg(target_os = "macos")]
static NOT_CALLS: std::sync::LazyLock<std::sync::Mutex<HashSet<String>>> =
    std::sync::LazyLock::new(Default::default);

/// The bundle path of the outermost app in an executable path:
/// `/Applications/Brave Browser.app/.../Brave Browser Helper.app/...` gives
/// `/Applications/Brave Browser.app`.
#[cfg(any(target_os = "macos", test))]
fn outer_app_path(exe: &str) -> Option<&str> {
    exe.find(".app/").map(|i| &exe[..i + 4])
}

/// The bundle id of the app a process runs inside. A helper (a browser's
/// audio or renderer process, Firefox's plugin-container) may report its own
/// id, or none, to Core Audio.
#[cfg(target_os = "macos")]
fn owning_app_bundle_id(pid: i32) -> Option<String> {
    let mut buf = vec![0u8; libc::PROC_PIDPATHINFO_MAXSIZE as usize];
    let n = unsafe { libc::proc_pidpath(pid, buf.as_mut_ptr().cast(), buf.len() as u32) };
    if n <= 0 {
        return None;
    }
    let exe = std::str::from_utf8(&buf[..n as usize]).ok()?;
    let path = objc2_foundation::NSString::from_str(outer_app_path(exe)?);
    let bundle = objc2_foundation::NSBundle::bundleWithPath(&path)?;
    Some(bundle.bundleIdentifier()?.to_string())
}

#[cfg(target_os = "macos")]
fn current_call_apps() -> Vec<DetectedCall> {
    let own = std::process::id() as i32;
    let mut seen = HashSet::new();
    super::coreaudio::processes_using_microphone()
        .into_iter()
        .filter(|p| p.pid != own)
        .filter_map(|p| {
            call_app(&p.bundle_id).or_else(|| {
                let owner = owning_app_bundle_id(p.pid)?;
                let found = call_app(&owner);
                if found.is_none() && NOT_CALLS.lock().unwrap().insert(owner.clone()) {
                    log::info!(
                        "Notetaker: microphone used by '{}' in {owner}, not a call app",
                        p.bundle_id
                    );
                }
                found
            })
        })
        .filter(|(prefix, _)| seen.insert(*prefix))
        .map(|(prefix, app)| DetectedCall {
            app: app.to_string(),
            bundle_id: prefix.to_string(),
        })
        .collect()
}

#[cfg(not(target_os = "macos"))]
fn current_call_apps() -> Vec<DetectedCall> {
    Vec::new()
}

#[derive(Default)]
struct Tracker {
    /// Consecutive polls each app was seen (positive) or missed after being
    /// confirmed (counted in `missing`).
    seen: HashMap<String, (DetectedCall, u32)>,
    missing: HashMap<String, u32>,
    confirmed: HashSet<String>,
}

enum Change {
    Started(DetectedCall),
    Ended(DetectedCall),
}

impl Tracker {
    /// `linked`: the call the current recording stops with.
    fn update(&mut self, now: Vec<DetectedCall>, linked: Option<&str>) -> Vec<Change> {
        let mut changes = Vec::new();
        let present: HashSet<String> = now.iter().map(|c| c.bundle_id.clone()).collect();
        for call in now {
            let entry = self.seen.entry(call.bundle_id.clone()).or_insert((call, 0));
            entry.1 += 1;
            self.missing.remove(&entry.0.bundle_id);
            if entry.1 >= CONFIRM_POLLS && self.confirmed.insert(entry.0.bundle_id.clone()) {
                changes.push(Change::Started(entry.0.clone()));
            }
        }
        let gone: Vec<String> = self
            .seen
            .keys()
            .filter(|b| !present.contains(*b))
            .cloned()
            .collect();
        for bundle in gone {
            if !self.confirmed.contains(&bundle) {
                self.seen.remove(&bundle);
                continue;
            }
            let needed = if linked == Some(bundle.as_str()) {
                LINKED_END_POLLS
            } else {
                CONFIRM_POLLS
            };
            let misses = self.missing.entry(bundle.clone()).or_insert(0);
            *misses += 1;
            if *misses >= needed {
                self.missing.remove(&bundle);
                self.confirmed.remove(&bundle);
                if let Some((call, _)) = self.seen.remove(&bundle) {
                    changes.push(Change::Ended(call));
                }
            }
        }
        changes
    }

    fn active(&self) -> Vec<DetectedCall> {
        self.confirmed
            .iter()
            .filter_map(|b| self.seen.get(b).map(|(c, _)| c.clone()))
            .collect()
    }
}

pub fn spawn(manager: Arc<NotetakerManager>) {
    let _ = std::thread::Builder::new()
        .name("notetaker-detect".into())
        .spawn(move || run(manager));
}

fn run(manager: Arc<NotetakerManager>) {
    let mut tracker = Tracker::default();
    let mut events: Vec<calendar::CalendarEvent> = Vec::new();
    let mut last_refresh: Option<Instant> = None;
    let mut reminded: HashSet<String> = HashSet::new();

    loop {
        std::thread::sleep(POLL);
        let s = settings(&manager.app);

        if s.detect_calls {
            let changes = tracker.update(current_call_apps(), manager.linked_bundle().as_deref());
            *manager.calls.lock().unwrap() = tracker.active();
            for change in changes {
                match change {
                    Change::Started(call) => {
                        log::info!("Notetaker: call detected in {}", call.app);
                        manager.link_call(&call);
                        let _ = NotetakerCallDetected {
                            app: call.app.clone(),
                            bundle_id: call.bundle_id.clone(),
                        }
                        .emit(&manager.app);
                        if s.auto_start
                            && manager.recording_meeting().is_none()
                            && !crate::notepad::is_focused(&manager.app)
                        {
                            match manager.start(None, Some(call)) {
                                Ok(meeting) => crate::notepad::recording_started(
                                    &manager.app,
                                    &meeting.id,
                                    false,
                                ),
                                Err(e) => log::error!("Notetaker auto-start failed: {e}"),
                            }
                        }
                    }
                    Change::Ended(call) => {
                        log::info!("Notetaker: call ended in {}", call.app);
                        let _ = NotetakerCallEnded {
                            app: call.app.clone(),
                            bundle_id: call.bundle_id.clone(),
                        }
                        .emit(&manager.app);
                        manager.stop_if_linked(&call.bundle_id);
                    }
                }
            }
        } else if !tracker.confirmed.is_empty() {
            tracker = Tracker::default();
            manager.calls.lock().unwrap().clear();
        }

        if s.notify_before_seconds == 0 {
            continue;
        }
        if last_refresh.is_none_or(|t| t.elapsed() >= CALENDAR_REFRESH) {
            let now = now_ms();
            events = calendar::events_between(now - 60_000, now + 24 * 3_600_000);
            last_refresh = Some(Instant::now());
        }
        let now = now_ms();
        let lead = s.notify_before_seconds as i64 * 1000;
        for event in &events {
            let until = event.start_ms - now;
            if !event.all_day && until > 0 && until <= lead && reminded.insert(event.id.clone()) {
                let _ = NotetakerMeetingSoon {
                    event: event.clone(),
                    starts_in_seconds: until / 1000,
                }
                .emit(&manager.app);
            }
        }
        if reminded.len() > 500 {
            reminded.clear();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn call(bundle: &str) -> DetectedCall {
        DetectedCall {
            app: call_app(bundle).unwrap().1.to_string(),
            bundle_id: call_app(bundle).unwrap().0.to_string(),
        }
    }

    #[test]
    fn matches_apps_and_helpers() {
        assert_eq!(call_app("us.zoom.xos").unwrap().1, "Zoom");
        assert_eq!(
            call_app("com.google.Chrome.helper").unwrap().1,
            "Google Chrome"
        );
        assert_eq!(
            call_app("com.microsoft.teams2").unwrap().1,
            "Microsoft Teams"
        );
        assert!(call_app("com.google.Chromecast").is_none());
        assert!(call_app("com.apple.Music").is_none());
    }

    #[test]
    fn helpers_belong_to_their_outer_app() {
        assert_eq!(
            outer_app_path("/Applications/Brave Browser.app/Contents/Frameworks/Brave Browser Framework.framework/Versions/1/Helpers/Brave Browser Helper.app/Contents/MacOS/Brave Browser Helper"),
            Some("/Applications/Brave Browser.app")
        );
        assert_eq!(outer_app_path("/usr/libexec/coreaudiod"), None);
        assert_eq!(call_app("com.brave.Browser.helper").unwrap().1, "Brave");
        assert_eq!(
            call_app("com.google.Chrome.app.kjgfgldnnfoeklkmfkjfagphfepbbdan")
                .unwrap()
                .1,
            "Google Chrome"
        );
    }

    #[test]
    fn debounces_start_and_end() {
        let mut t = Tracker::default();
        assert!(t.update(vec![call("us.zoom.xos")], None).is_empty());
        let started = t.update(vec![call("us.zoom.xos")], None);
        assert!(matches!(started.as_slice(), [Change::Started(c)] if c.app == "Zoom"));
        assert!(t.update(vec![call("us.zoom.xos")], None).is_empty());
        assert!(t.update(vec![], None).is_empty());
        assert!(t.update(vec![call("us.zoom.xos")], None).is_empty());
        assert!(t.update(vec![], None).is_empty());
        let ended = t.update(vec![], None);
        assert!(matches!(ended.as_slice(), [Change::Ended(c)] if c.app == "Zoom"));
        assert!(t.active().is_empty());
    }

    #[test]
    fn linked_call_survives_a_muted_stretch() {
        let mut t = Tracker::default();
        let linked = Some("us.zoom.xos");
        t.update(vec![call("us.zoom.xos")], linked);
        t.update(vec![call("us.zoom.xos")], linked);
        for _ in 1..LINKED_END_POLLS {
            assert!(t.update(vec![], linked).is_empty());
        }
        assert!(t.update(vec![call("us.zoom.xos")], linked).is_empty());
        for _ in 1..LINKED_END_POLLS {
            assert!(t.update(vec![], linked).is_empty());
        }
        let ended = t.update(vec![], linked);
        assert!(matches!(ended.as_slice(), [Change::Ended(_)]));
    }
}
