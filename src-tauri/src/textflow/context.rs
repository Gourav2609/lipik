//! Which app the user is dictating into, and what kind of app it is.

use super::store::StyleCategory;
use std::sync::Mutex;

#[derive(Clone, Debug, Default)]
pub struct AppContext {
    pub bundle_id: Option<String>,
    /// The app's name as macOS shows it.
    pub name: Option<String>,
    /// Focused window title; read only for browsers, where the site decides
    /// the category.
    pub title: Option<String>,
}

static CURRENT: Mutex<Option<AppContext>> = Mutex::new(None);

/// Called when a recording starts; the capture runs on that background thread.
pub fn capture_for_recording() {
    *CURRENT.lock().unwrap_or_else(|e| e.into_inner()) = None;
    let context = capture(true);
    *CURRENT.lock().unwrap_or_else(|e| e.into_inner()) = Some(context);
}

/// The app in front right now, without the window title (which can be slow
/// to read).
pub fn front_app() -> AppContext {
    capture(false)
}

/// The context captured at recording start, or a quick bundle-id-only read
/// when that capture has not finished.
pub fn current() -> AppContext {
    CURRENT
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .clone()
        .unwrap_or_else(|| capture(false))
}

const BROWSERS: &[&str] = &[
    "com.google.Chrome",
    "com.google.Chrome.canary",
    "com.apple.Safari",
    "com.apple.SafariTechnologyPreview",
    "company.thebrowser.Browser",
    "company.thebrowser.dia",
    "com.brave.Browser",
    "org.mozilla.firefox",
    "com.microsoft.edgemac",
    "com.operasoftware.Opera",
    "com.vivaldi.Vivaldi",
    "app.zen-browser.zen",
    "org.chromium.Chromium",
];

const PERSONAL_APPS: &[&str] = &[
    "com.apple.MobileSMS",
    "net.whatsapp.WhatsApp",
    "desktop.WhatsApp",
    "ru.keepcoder.Telegram",
    "org.telegram.desktop",
    "com.tdesktop.Telegram",
    "com.facebook.archon",
    "com.hnc.Discord",
    "org.whispersystems.signal-desktop",
    "jp.naver.line.mac",
    "com.viber.osx",
];

const WORK_APPS: &[&str] = &[
    "com.tinyspeck.slackmacgap",
    "com.microsoft.teams",
    "com.microsoft.teams2",
    "com.linear",
    "notion.id",
    "com.atlassian.jira",
    "com.atlassian.trello",
    "com.asana.app",
    "com.clickup.desktop-app",
    "com.basecamp.bc3-mac",
];

const EMAIL_APPS: &[&str] = &[
    "com.apple.mail",
    "com.microsoft.Outlook",
    "com.readdle.smartemail-Mac",
    "com.superhuman.electron",
    "it.bloop.airmail2",
    "com.mimestream.Mimestream",
    "com.google.Gmail",
];

const AI_APPS: &[&str] = &[
    "com.openai.chat",
    "com.anthropic.claudefordesktop",
    "com.todesktop.230313mzl4w4u92",
    "com.exafunction.windsurf",
    "ai.perplexity.mac",
];

const DOCUMENT_APPS: &[&str] = &[
    "com.apple.Notes",
    "com.apple.iWork.Pages",
    "com.microsoft.Word",
    "md.obsidian",
    "com.apple.TextEdit",
    "net.shinyfrog.bear",
    "com.craft.craft",
];

const EMAIL_SITES: &[&str] = &[
    "gmail",
    "inbox",
    "outlook",
    "yahoo mail",
    "proton mail",
    "fastmail",
    "superhuman",
];
const WORK_SITES: &[&str] = &[
    "slack",
    "linear",
    "notion",
    "jira",
    "confluence",
    "microsoft teams",
    "asana",
    "trello",
    "clickup",
    "github",
];
const PERSONAL_SITES: &[&str] = &[
    "whatsapp",
    "messenger",
    "telegram",
    "discord",
    "instagram",
    "signal",
];
const AI_SITES: &[&str] = &["chatgpt", "claude", "gemini", "perplexity", "copilot"];
const DOCUMENT_SITES: &[&str] = &[
    "google docs",
    "- docs",
    "microsoft word",
    "dropbox paper",
    "quip",
];

fn title_has(title: &str, needles: &[&str]) -> bool {
    needles.iter().any(|n| title.contains(n))
}

/// The insights app type: ai, email, work, personal, document or other.
pub fn app_type(context: &AppContext) -> &'static str {
    let Some(bundle) = context.bundle_id.as_deref() else {
        return "other";
    };
    let in_list = |list: &[&str]| list.contains(&bundle);
    if in_list(EMAIL_APPS) {
        "email"
    } else if in_list(PERSONAL_APPS) {
        "personal"
    } else if in_list(WORK_APPS) {
        "work"
    } else if in_list(AI_APPS) {
        "ai"
    } else if in_list(DOCUMENT_APPS) {
        "document"
    } else if in_list(BROWSERS) {
        let title = context.title.as_deref().unwrap_or("").to_lowercase();
        if title_has(&title, EMAIL_SITES) {
            "email"
        } else if title_has(&title, PERSONAL_SITES) {
            "personal"
        } else if title_has(&title, WORK_SITES) {
            "work"
        } else if title_has(&title, AI_SITES) {
            "ai"
        } else if title_has(&title, DOCUMENT_SITES) {
            "document"
        } else {
            "other"
        }
    } else {
        "other"
    }
}

fn type_label(app_type: &str) -> &'static str {
    match app_type {
        "email" => "Email",
        "personal" => "Chat",
        "work" => "Work chat",
        "ai" => "AI assistant",
        "document" => "Document",
        _ => "App",
    }
}

/// How clean-up names the app: its own name, with the kind of site for a
/// browser tab ("Safari (Email)"), or the app type when the name is unknown.
pub fn app_label(name: Option<&str>, bundle_id: Option<&str>, app_type: &str) -> String {
    let browser = bundle_id.is_some_and(|b| BROWSERS.contains(&b));
    match name.map(str::trim).filter(|n| !n.is_empty()) {
        Some(name) if browser && app_type != "other" => {
            format!("{name} ({})", type_label(app_type))
        }
        Some(name) => name.to_string(),
        None => type_label(app_type).to_string(),
    }
}

/// The style category that decides how clean-up formats the text.
pub fn style_category(app_type: &str) -> StyleCategory {
    match app_type {
        "personal" => StyleCategory::Personal,
        "work" => StyleCategory::Work,
        "email" => StyleCategory::Email,
        _ => StyleCategory::Other,
    }
}

#[cfg(target_os = "macos")]
fn capture(with_title: bool) -> AppContext {
    use objc2_app_kit::NSWorkspace;

    let workspace = NSWorkspace::sharedWorkspace();
    let Some(app) = workspace.frontmostApplication() else {
        return AppContext::default();
    };
    let bundle_id = app.bundleIdentifier().map(|s| s.to_string());
    let name = app.localizedName().map(|s| s.to_string());
    let title = match &bundle_id {
        Some(bundle) if with_title && BROWSERS.contains(&bundle.as_str()) => {
            ax::focused_window_title(app.processIdentifier())
        }
        _ => None,
    };
    AppContext {
        bundle_id,
        name,
        title,
    }
}

#[cfg(not(target_os = "macos"))]
fn capture(_with_title: bool) -> AppContext {
    AppContext::default()
}

#[cfg(target_os = "macos")]
mod ax {
    use std::ffi::{c_char, c_void, CStr, CString};

    type CfTypeRef = *const c_void;
    const UTF8: u32 = 0x0800_0100;

    #[link(name = "ApplicationServices", kind = "framework")]
    unsafe extern "C" {
        fn AXUIElementCreateApplication(pid: i32) -> CfTypeRef;
        fn AXUIElementCopyAttributeValue(
            element: CfTypeRef,
            attribute: CfTypeRef,
            value: *mut CfTypeRef,
        ) -> i32;
        fn AXUIElementSetMessagingTimeout(element: CfTypeRef, seconds: f32) -> i32;
    }

    #[link(name = "CoreFoundation", kind = "framework")]
    unsafe extern "C" {
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

    struct Cf(CfTypeRef);

    impl Drop for Cf {
        fn drop(&mut self) {
            if !self.0.is_null() {
                // SAFETY: every Cf holds a reference returned by a Create/Copy call.
                unsafe { CFRelease(self.0) };
            }
        }
    }

    fn copy(element: &Cf, attribute: &str) -> Option<Cf> {
        let name = CString::new(attribute).ok()?;
        // SAFETY: name is a valid NUL-terminated string.
        let name = Cf(unsafe { CFStringCreateWithCString(std::ptr::null(), name.as_ptr(), UTF8) });
        let mut value: CfTypeRef = std::ptr::null();
        // SAFETY: element and name are live; value receives a +1 reference.
        let error = unsafe { AXUIElementCopyAttributeValue(element.0, name.0, &mut value) };
        (error == 0 && !value.is_null()).then(|| Cf(value))
    }

    fn string(value: &Cf) -> Option<String> {
        // SAFETY: value is live and read as a string only after its type is
        // checked; the buffer is NUL-terminated on success.
        unsafe {
            if CFGetTypeID(value.0) != CFStringGetTypeID() {
                return None;
            }
            let mut buffer = [0 as c_char; 1024];
            if CFStringGetCString(value.0, buffer.as_mut_ptr(), buffer.len() as isize, UTF8) == 0 {
                return None;
            }
            Some(
                CStr::from_ptr(buffer.as_ptr())
                    .to_string_lossy()
                    .into_owned(),
            )
        }
    }

    pub fn focused_window_title(pid: i32) -> Option<String> {
        // SAFETY: returns a +1 reference, released by Cf.
        let app = Cf(unsafe { AXUIElementCreateApplication(pid) });
        if app.0.is_null() {
            return None;
        }
        // SAFETY: app is a live AXUIElement.
        unsafe { AXUIElementSetMessagingTimeout(app.0, 0.3) };
        let window = copy(&app, "AXFocusedWindow")?;
        string(&copy(&window, "AXTitle")?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ctx(bundle: &str, title: Option<&str>) -> AppContext {
        AppContext {
            bundle_id: Some(bundle.to_string()),
            name: None,
            title: title.map(str::to_string),
        }
    }

    #[test]
    fn apps_map_to_types() {
        assert_eq!(app_type(&ctx("com.tinyspeck.slackmacgap", None)), "work");
        assert_eq!(app_type(&ctx("com.apple.MobileSMS", None)), "personal");
        assert_eq!(app_type(&ctx("com.apple.mail", None)), "email");
        assert_eq!(app_type(&ctx("com.mitchellh.ghostty", None)), "other");
    }

    #[test]
    fn browser_uses_window_title() {
        assert_eq!(
            app_type(&ctx("com.google.Chrome", Some("Inbox (3) - Gmail"))),
            "email"
        );
        assert_eq!(app_type(&ctx("com.apple.Safari", Some("ChatGPT"))), "ai");
        assert_eq!(app_type(&ctx("com.apple.Safari", None)), "other");
        assert_eq!(style_category("ai"), StyleCategory::Other);
    }

    #[test]
    fn app_label_names_the_app() {
        let slack = Some("com.tinyspeck.slackmacgap");
        assert_eq!(app_label(Some("Slack"), slack, "work"), "Slack");
        assert_eq!(
            app_label(Some("Safari"), Some("com.apple.Safari"), "email"),
            "Safari (Email)"
        );
        assert_eq!(
            app_label(Some("Safari"), Some("com.apple.Safari"), "other"),
            "Safari"
        );
        assert_eq!(app_label(None, slack, "work"), "Work chat");
        assert_eq!(app_label(None, None, "other"), "App");
    }
}
