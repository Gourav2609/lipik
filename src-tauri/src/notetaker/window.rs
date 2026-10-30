//! Who Slack shows as talking in a huddle, read through Accessibility while a
//! meeting linked to a Slack call records (setting `meeting_window_names`).
//! What is read:
//! - To find the way to the huddle and to the signed-in user's button: each
//!   element's role, DOM classes and children only, never its text. Subtrees
//!   whose classes mark messages, threads or the composer are not entered.
//! - The description of that one button, "User: <name>", once per huddle.
//! - Inside the huddle container (`p-huddle_window_content`): the role,
//!   classes, description and value of its elements, which give each tile's
//!   name and whether Slack marks it as talking.
//! - The app's window list and whether the huddle's window is minimised.
//!
//! Each sample is a line of `window-speakers.jsonl` in the meeting folder;
//! the speaker naming turns them into names after the voice split
//! (`diarize::window_names`).

use serde::{Deserialize, Serialize};
use std::path::Path;

pub const FILE: &str = "window-speakers.jsonl";
pub const SLACK: &str = "com.tinyspeck.slackmacgap";

/// One reading of the huddle, in meeting time. `gap`: the huddle could not
/// be read (no huddle, its window on another Space or minimised).
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Sample {
    pub ms_since_meeting_start: i64,
    #[serde(default)]
    pub speaking: Vec<String>,
    #[serde(default)]
    pub present: Vec<String>,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub gap: bool,
    /// The signed-in user's name as Slack shows it ("User: <name>").
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub user: Option<String>,
}

/// The samples of a meeting, in time order; unreadable lines are left out.
pub fn load(dir: &Path) -> Vec<Sample> {
    std::fs::read_to_string(dir.join(FILE))
        .map(|text| parse(&text))
        .unwrap_or_default()
}

pub fn parse(text: &str) -> Vec<Sample> {
    let mut samples: Vec<Sample> = text
        .lines()
        .filter_map(|line| serde_json::from_str(line).ok())
        .collect();
    samples.sort_by_key(|s| s.ms_since_meeting_start);
    samples
}

const CONTAINER: &str = "p-huddle_window_content";
const USER_BUTTON: &str = "p-ia__nav__user__button";
const USER_PREFIX: &str = "User: ";
const TILE: &str = "p-huddle_peer_tile";
const ACTIVE: &str = "active_speaker";
const NAME_OVERLAY: &str = "name_overlay";
/// Subtrees whose classes mention these hold messages, never the huddle.
const SKIP_WORDS: [&str; 10] = [
    "message",
    "composer",
    "thread",
    "conversation",
    "channel_sidebar",
    "chat",
    "inbox",
    "reply",
    "comment",
    "texty",
];
/// A table's columns repeat the cells of its rows; text inputs and menus
/// never hold the huddle.
const SKIP_ROLES: [&str; 6] = [
    "AXColumn",
    "AXTextArea",
    "AXTextField",
    "AXMenu",
    "AXMenuBar",
    "AXScrollBar",
];
const MAX_DEPTH: u16 = 60;
const MAX_HUDDLE_NODES: usize = 400;
const TILE_DEPTH: u16 = 3;
const MAX_TILE_NODES: usize = 16;

/// What one element says about itself, read in one Accessibility call.
/// `description` and `value` stay empty when only the outline is read.
pub(super) struct Info<E> {
    pub role: String,
    pub classes: Vec<String>,
    pub description: Option<String>,
    pub value: Option<String>,
    pub children: Vec<E>,
}

pub(super) trait Element: Clone {
    /// Role, classes and children only.
    fn outline(&self) -> Option<Info<Self>>;
    /// Role, classes, description, value and children.
    fn info(&self) -> Option<Info<Self>>;
    fn description(&self) -> Option<String>;
}

fn has_class<E>(info: &Info<E>, class: &str) -> bool {
    info.classes.iter().any(|c| c == class)
}

fn class_contains<E>(info: &Info<E>, part: &str) -> bool {
    info.classes.iter().any(|c| c.contains(part))
}

fn skipped<E>(info: &Info<E>) -> bool {
    SKIP_ROLES.contains(&info.role.as_str())
        || info.classes.iter().any(|c| {
            let c = c.to_lowercase();
            SKIP_WORDS.iter().any(|w| c.contains(w))
        })
}

/// A depth-first search, by outline only, for the element carrying the
/// class `target`. It can stop and go on in the next sample, so no sample
/// takes long.
pub(super) struct Search<E> {
    target: &'static str,
    stack: Vec<(E, usize, u16)>,
}

pub(super) enum Step<E> {
    /// The element and the index of the window it is in.
    Found(E, usize),
    Pending,
    Exhausted,
}

impl<E: Element> Search<E> {
    pub fn new(windows: &[E], target: &'static str) -> Self {
        Self {
            target,
            stack: windows
                .iter()
                .enumerate()
                .rev()
                .map(|(w, e)| (e.clone(), w, 0))
                .collect(),
        }
    }

    pub fn huddle(windows: &[E]) -> Self {
        Self::new(windows, CONTAINER)
    }

    pub fn user(windows: &[E]) -> Self {
        Self::new(windows, USER_BUTTON)
    }

    pub fn step(&mut self, mut more: impl FnMut() -> bool) -> Step<E> {
        while more() {
            let Some((e, window, depth)) = self.stack.pop() else {
                return Step::Exhausted;
            };
            let Some(info) = e.outline() else { continue };
            if has_class(&info, self.target) {
                return Step::Found(e, window);
            }
            if depth >= MAX_DEPTH || skipped(&info) {
                continue;
            }
            for child in info.children.into_iter().rev() {
                self.stack.push((child, window, depth + 1));
            }
        }
        if self.stack.is_empty() {
            Step::Exhausted
        } else {
            Step::Pending
        }
    }
}

/// "User: Priya" on Slack's signed-in user button gives "Priya".
pub(super) fn user_name<E: Element>(button: &E) -> Option<String> {
    let name = button.description()?;
    let name = name.strip_prefix(USER_PREFIX)?.trim();
    (!name.is_empty()).then(|| name.to_string())
}

#[derive(Clone, Debug, Default, PartialEq)]
pub(super) struct Tiles {
    pub speaking: Vec<String>,
    pub present: Vec<String>,
    /// Every tile was read before the time limit.
    pub complete: bool,
}

/// "View Priya's profile" gives "Priya".
fn profile_name(description: &str) -> Option<String> {
    let rest = description.strip_prefix("View ")?;
    let name = rest
        .strip_suffix("'s profile")
        .or_else(|| rest.strip_suffix("\u{2019}s profile"))?
        .trim();
    (!name.is_empty()).then(|| name.to_string())
}

fn push_once(list: &mut Vec<String>, name: &str) {
    if !list.iter().any(|n| n == name) {
        list.push(name.to_string());
    }
}

fn read_tile<E: Element>(tile: Info<E>, tiles: &mut Tiles, more: &mut impl FnMut() -> bool) {
    let mut name = tile.description.as_deref().and_then(profile_name);
    let mut speaking = class_contains(&tile, ACTIVE);
    let mut stack: Vec<(E, u16, bool)> = tile
        .children
        .into_iter()
        .rev()
        .map(|c| (c, 1, false))
        .collect();
    let mut visited = 0;
    while let Some((e, depth, in_name)) = stack.pop() {
        visited += 1;
        if visited > MAX_TILE_NODES || !more() {
            break;
        }
        let Some(info) = e.info() else { continue };
        speaking |= class_contains(&info, ACTIVE);
        if name.is_none() && in_name && info.role == "AXStaticText" {
            name = info
                .value
                .as_deref()
                .map(str::trim)
                .filter(|v| !v.is_empty())
                .map(str::to_string);
        }
        if depth < TILE_DEPTH {
            let in_name = in_name || class_contains(&info, NAME_OVERLAY);
            for child in info.children.into_iter().rev() {
                stack.push((child, depth + 1, in_name));
            }
        }
    }
    let Some(name) = name else { return };
    push_once(&mut tiles.present, &name);
    if speaking {
        push_once(&mut tiles.speaking, &name);
    }
}

/// The tiles under the huddle container, read while `more` allows; None when
/// the element is gone or no longer the huddle.
pub(super) fn read_huddle<E: Element>(
    container: &E,
    mut more: impl FnMut() -> bool,
) -> Option<Tiles> {
    let info = container.outline()?;
    if !has_class(&info, CONTAINER) {
        return None;
    }
    let mut tiles = Tiles::default();
    let mut stack: Vec<E> = info.children.into_iter().rev().collect();
    let mut visited = 0;
    while let Some(e) = stack.pop() {
        visited += 1;
        if visited > MAX_HUDDLE_NODES || !more() {
            return Some(tiles);
        }
        let Some(info) = e.info() else { continue };
        if has_class(&info, TILE) {
            read_tile(info, &mut tiles, &mut more);
        } else if !skipped(&info) {
            stack.extend(info.children.into_iter().rev());
        }
    }
    tiles.complete = more();
    Some(tiles)
}

/// Time to read one huddle's tiles: 20 ms, raised to 60 ms for the huddle
/// once three readings in a row ran out of it.
#[derive(Debug, Default)]
pub(super) struct ReadLimit {
    raised: bool,
    cut_in_row: u32,
}

impl ReadLimit {
    pub fn limit(&self) -> std::time::Duration {
        std::time::Duration::from_millis(if self.raised { 60 } else { 20 })
    }

    pub fn after(&mut self, cut: bool) {
        self.cut_in_row = if cut { self.cut_in_row + 1 } else { 0 };
        self.raised |= self.cut_in_row >= 3;
    }
}

/// Counts for the meeting's log line.
#[derive(Debug, Default)]
pub(super) struct Stats {
    pub reads: u32,
    pub gaps: u32,
    /// Readings that ran out of time.
    pub cut: u32,
    /// Readings while the huddle's window was on another Space or minimised.
    pub away: u32,
    pub user_found: bool,
}

impl Stats {
    pub fn read(&mut self, sample: &Sample) {
        self.reads += 1;
        self.gaps += sample.gap as u32;
        self.user_found |= sample.user.is_some();
    }

    pub fn summary(&self) -> String {
        let mut line = format!(
            "Slack names: {} reads, {} gaps, user {}",
            self.reads,
            self.gaps,
            if self.user_found {
                "found"
            } else {
                "not found"
            }
        );
        if self.cut > 0 {
            line += &format!(", {} cut short", self.cut);
        }
        if self.away > 0 {
            line += &format!(", {} out of reach", self.away);
        }
        line
    }
}

#[cfg(target_os = "macos")]
pub(super) use live::record;

#[cfg(target_os = "macos")]
mod live {
    use super::*;
    use std::io::Write;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::time::{Duration, Instant};

    const INTERVAL: Duration = Duration::from_millis(700);
    /// Search time per sample while the huddle is not found yet.
    const SEARCH_BUDGET: Duration = Duration::from_millis(8);
    /// Search time per sample for Slack's signed-in user button.
    const USER_BUDGET: Duration = Duration::from_millis(4);
    const PID_REFRESH: Duration = Duration::from_secs(5);

    #[derive(Default)]
    struct Reader {
        pid: Option<i32>,
        pid_checked: Option<Instant>,
        app: Option<ax::Ax>,
        windows: Vec<ax::Ax>,
        search: Option<Search<ax::Ax>>,
        huddle: Option<(ax::Ax, ax::Ax)>,
        user: Option<String>,
        user_search: Option<Search<ax::Ax>>,
        read_limit: ReadLimit,
        said_cut: bool,
        said_away: bool,
        stats: Stats,
    }

    impl Reader {
        fn app(&mut self) -> Option<ax::Ax> {
            let stale = self.pid_checked.is_none_or(|t| t.elapsed() >= PID_REFRESH);
            if stale && (self.app.is_none() || self.huddle.is_none()) {
                self.pid_checked = Some(Instant::now());
                let pid = slack_pid();
                if pid != self.pid {
                    *self = Reader {
                        pid,
                        pid_checked: self.pid_checked,
                        stats: std::mem::take(&mut self.stats),
                        ..Default::default()
                    };
                    if let Some(pid) = pid {
                        self.app = ax::app(pid);
                        if let Some(app) = &self.app {
                            ax::build_web_tree(app);
                        }
                    }
                }
            }
            self.app.clone()
        }

        /// Looks for Slack's signed-in user button on every sample, with
        /// its own time, until its name is read.
        fn look_for_user(&mut self, app: &ax::Ax) {
            if self.user.is_some() {
                return;
            }
            let started = Instant::now();
            let search = self
                .user_search
                .get_or_insert_with(|| Search::user(&ax::windows(app)));
            match search.step(|| started.elapsed() < USER_BUDGET) {
                Step::Found(button, _) => {
                    self.user = user_name(&button);
                    self.user_search = None;
                }
                Step::Exhausted => self.user_search = None,
                Step::Pending => {}
            }
        }

        fn new_huddle(&mut self, huddle: ax::Ax, window: ax::Ax) {
            self.huddle = Some((huddle, window));
            self.user = None;
            self.user_search = None;
            self.read_limit = ReadLimit::default();
            self.said_cut = false;
            self.said_away = false;
        }

        fn sample(&mut self, ms: i64) -> Sample {
            let sample = self.read(ms);
            self.stats.read(&sample);
            sample
        }

        fn read(&mut self, ms: i64) -> Sample {
            let started = Instant::now();
            let gap = Sample {
                ms_since_meeting_start: ms,
                gap: true,
                ..Default::default()
            };
            let Some(app) = self.app() else { return gap };
            if let Some((huddle, window)) = &self.huddle {
                let shown =
                    ax::windows(&app).iter().any(|w| ax::same(w, window)) && !ax::minimized(window);
                if !shown && huddle.outline().is_some() {
                    self.stats.away += 1;
                    if !self.said_away {
                        self.said_away = true;
                        log::info!("Notetaker: the Slack huddle is out of reach (another Space or minimised)");
                    }
                    return gap;
                }
                let limit = self.read_limit.limit();
                let read = read_huddle(huddle, || started.elapsed() < limit);
                match read.filter(|_| shown) {
                    Some(tiles) => {
                        self.read_limit.after(!tiles.complete);
                        if !tiles.complete {
                            self.stats.cut += 1;
                            if !self.said_cut {
                                self.said_cut = true;
                                log::info!(
                                    "Notetaker: reading the Slack huddle took over {} ms",
                                    limit.as_millis()
                                );
                            }
                            return gap;
                        }
                        if tiles.present.is_empty() {
                            return gap;
                        }
                        self.look_for_user(&app);
                        return Sample {
                            ms_since_meeting_start: ms,
                            speaking: tiles.speaking,
                            present: tiles.present,
                            gap: false,
                            user: self.user.clone(),
                        };
                    }
                    None => self.huddle = None,
                }
            }
            if self.search.is_none() {
                self.windows = ax::windows(&app);
                self.search = Some(Search::huddle(&self.windows));
            }
            let step = self
                .search
                .as_mut()
                .map(|s| s.step(|| started.elapsed() < SEARCH_BUDGET));
            match step {
                Some(Step::Found(huddle, w)) => {
                    self.search = None;
                    let window = self.windows[w].clone();
                    self.new_huddle(huddle, window);
                }
                Some(Step::Exhausted) => self.search = None,
                _ => {}
            }
            gap
        }
    }

    fn slack_pid() -> Option<i32> {
        use objc2_app_kit::NSWorkspace;
        NSWorkspace::sharedWorkspace()
            .runningApplications()
            .iter()
            .find(|app| {
                app.bundleIdentifier()
                    .is_some_and(|b| b.to_string() == SLACK)
            })
            .map(|app| app.processIdentifier())
    }

    /// Samples the huddle every 700 ms while the recording is linked to a
    /// Slack call, until `stop`. A slow sample makes the next ones wait
    /// rather than pile up.
    pub(in crate::notetaker) fn record(
        linked_bundle: impl Fn() -> Option<String>,
        dir: &Path,
        started: Instant,
        stop: &AtomicBool,
    ) {
        let mut reader = Reader::default();
        let mut file: Option<std::fs::File> = None;
        let mut took_us: Vec<u128> = Vec::new();
        let mut next = Instant::now();
        while !stop.load(Ordering::Relaxed) {
            let now = Instant::now();
            if now < next {
                std::thread::sleep((next - now).min(Duration::from_millis(100)));
                continue;
            }
            next = (next + INTERVAL).max(now);
            if linked_bundle().as_deref() != Some(SLACK) {
                continue;
            }
            let ms = started.elapsed().as_millis() as i64;
            let t = Instant::now();
            let sample = reader.sample(ms);
            took_us.push(t.elapsed().as_micros());
            if file.is_none() {
                file = std::fs::OpenOptions::new()
                    .create(true)
                    .append(true)
                    .open(dir.join(FILE))
                    .inspect_err(|e| log::warn!("Notetaker could not keep huddle speakers: {e}"))
                    .ok();
            }
            if let (Some(f), Ok(line)) = (file.as_mut(), serde_json::to_string(&sample)) {
                let _ = writeln!(f, "{line}");
            }
        }
        if !took_us.is_empty() {
            took_us.sort_unstable();
            log::info!(
                "Notetaker {} (median {} us, slowest {} us)",
                reader.stats.summary(),
                took_us[took_us.len() / 2],
                took_us[took_us.len() - 1]
            );
        }
    }

    /// Runs the reader `samples` times against the running Slack and returns
    /// each sample with its time, then the windows seen and the elements and
    /// time of one whole search, for checking cost by hand.
    #[cfg(test)]
    pub(in crate::notetaker) fn probe(
        samples: usize,
    ) -> (Vec<(Sample, Duration)>, usize, usize, Duration) {
        let mut reader = Reader::default();
        let taken = (0..samples)
            .map(|i| {
                let t = Instant::now();
                let s = reader.sample(i as i64 * INTERVAL.as_millis() as i64);
                let took = t.elapsed();
                std::thread::sleep(INTERVAL.saturating_sub(took));
                (s, took)
            })
            .collect();
        let windows = reader
            .app()
            .map(|app| ax::windows(&app))
            .unwrap_or_default();
        let mut visited = 0;
        let t = Instant::now();
        Search::huddle(&windows).step(|| {
            visited += 1;
            true
        });
        (taken, windows.len(), visited, t.elapsed())
    }

    mod ax {
        use super::super::{Element, Info};
        use crate::ax::{cf_string, copy, retain, string_ref, Cf, CfTypeRef};

        const OUTLINE: [&str; 3] = ["AXRole", "AXDOMClassList", "AXChildren"];
        const FULL: [&str; 5] = [
            "AXRole",
            "AXDOMClassList",
            "AXChildren",
            "AXDescription",
            "AXValue",
        ];
        /// Slack answering slowly must not hold up a sample.
        const TIMEOUT_S: f32 = 0.05;

        #[link(name = "ApplicationServices", kind = "framework")]
        unsafe extern "C" {
            fn AXUIElementCreateApplication(pid: i32) -> CfTypeRef;
            fn AXUIElementCopyMultipleAttributeValues(
                element: CfTypeRef,
                attributes: CfTypeRef,
                options: u32,
                values: *mut CfTypeRef,
            ) -> i32;
            fn AXUIElementSetAttributeValue(
                element: CfTypeRef,
                attribute: CfTypeRef,
                value: CfTypeRef,
            ) -> i32;
            fn AXUIElementSetMessagingTimeout(element: CfTypeRef, seconds: f32) -> i32;
        }

        #[repr(C)]
        struct CfArrayCallBacks {
            _private: [u8; 0],
        }

        #[link(name = "CoreFoundation", kind = "framework")]
        unsafe extern "C" {
            static kCFBooleanTrue: CfTypeRef;
            static kCFTypeArrayCallBacks: CfArrayCallBacks;
            fn CFArrayCreate(
                allocator: CfTypeRef,
                values: *const CfTypeRef,
                count: isize,
                callbacks: *const CfArrayCallBacks,
            ) -> CfTypeRef;
            fn CFArrayGetCount(array: CfTypeRef) -> isize;
            fn CFArrayGetValueAtIndex(array: CfTypeRef, index: isize) -> CfTypeRef;
            fn CFArrayGetTypeID() -> usize;
            fn CFGetTypeID(value: CfTypeRef) -> usize;
            fn CFEqual(a: CfTypeRef, b: CfTypeRef) -> u8;
        }

        pub type Ax = Cf;

        /// The items of a borrowed CFArray, borrowed from it.
        fn items(array: CfTypeRef) -> Vec<CfTypeRef> {
            // SAFETY: array is live and checked to be a CFArray.
            unsafe {
                if array.is_null() || CFGetTypeID(array) != CFArrayGetTypeID() {
                    return Vec::new();
                }
                (0..CFArrayGetCount(array))
                    .map(|i| CFArrayGetValueAtIndex(array, i))
                    .filter(|v| !v.is_null())
                    .collect()
            }
        }

        fn names(attributes: &[&str]) -> Option<Cf> {
            let names: Vec<Cf> = attributes.iter().map(|a| cf_string(a)).collect();
            let raw: Vec<CfTypeRef> = names.iter().map(|n| n.0).collect();
            // SAFETY: raw holds live CFStrings; the array retains them.
            let array = Cf(unsafe {
                CFArrayCreate(
                    std::ptr::null(),
                    raw.as_ptr(),
                    raw.len() as isize,
                    &kCFTypeArrayCallBacks,
                )
            });
            (!array.0.is_null()).then_some(array)
        }

        thread_local! {
            static OUTLINE_NAMES: Option<Cf> = names(&OUTLINE);
            static FULL_NAMES: Option<Cf> = names(&FULL);
        }

        fn read(element: &Ax, full: bool) -> Option<Info<Ax>> {
            let mut values: CfTypeRef = std::ptr::null();
            let read = |names: &Option<Cf>| {
                let names = names.as_ref()?;
                // SAFETY: element and names are live; values receives a +1 array.
                Some(unsafe {
                    AXUIElementSetMessagingTimeout(element.0, TIMEOUT_S);
                    AXUIElementCopyMultipleAttributeValues(element.0, names.0, 0, &mut values)
                })
            };
            let error = if full {
                FULL_NAMES.with(read)
            } else {
                OUTLINE_NAMES.with(read)
            }?;
            let values = Cf(values);
            if error != 0 || values.0.is_null() {
                return None;
            }
            let v = items(values.0);
            if v.len() != if full { FULL.len() } else { OUTLINE.len() } {
                return None;
            }
            Some(Info {
                role: string_ref(v[0]).unwrap_or_default(),
                classes: items(v[1]).into_iter().filter_map(string_ref).collect(),
                children: items(v[2]).into_iter().map(retain).collect(),
                description: full.then(|| string_ref(v[3])).flatten(),
                value: full.then(|| string_ref(v[4])).flatten(),
            })
        }

        impl Element for Ax {
            fn outline(&self) -> Option<Info<Self>> {
                read(self, false)
            }

            fn info(&self) -> Option<Info<Self>> {
                read(self, true)
            }

            fn description(&self) -> Option<String> {
                // SAFETY: self is a live AXUIElement.
                unsafe { AXUIElementSetMessagingTimeout(self.0, TIMEOUT_S) };
                string_ref(copy(self, "AXDescription").ok()?.0)
            }
        }

        pub fn app(pid: i32) -> Option<Ax> {
            // SAFETY: returns a +1 reference or null.
            let app = Cf(unsafe { AXUIElementCreateApplication(pid) });
            if app.0.is_null() {
                return None;
            }
            // SAFETY: app is a live AXUIElement.
            unsafe { AXUIElementSetMessagingTimeout(app.0, TIMEOUT_S) };
            Some(app)
        }

        /// Slack (Electron) builds its web content's tree only once asked.
        pub fn build_web_tree(app: &Ax) {
            let name = cf_string("AXManualAccessibility");
            // SAFETY: app and name are live; kCFBooleanTrue is a CF constant.
            unsafe { AXUIElementSetAttributeValue(app.0, name.0, kCFBooleanTrue) };
        }

        /// The app's windows on the current Space.
        pub fn windows(app: &Ax) -> Vec<Ax> {
            copy(app, "AXWindows")
                .map(|array| items(array.0).into_iter().map(retain).collect())
                .unwrap_or_default()
        }

        pub fn same(a: &Ax, b: &Ax) -> bool {
            // SAFETY: both are live CF references.
            unsafe { CFEqual(a.0, b.0) != 0 }
        }

        pub fn minimized(window: &Ax) -> bool {
            // SAFETY: kCFBooleanTrue is a CF constant; the value is live.
            copy(window, "AXMinimized").is_ok_and(|v| unsafe { CFEqual(v.0, kCFBooleanTrue) != 0 })
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;
    use std::time::Duration;

    thread_local! {
        /// Every description and value the reader was given.
        static READ_TEXT: RefCell<Vec<String>> = const { RefCell::new(Vec::new()) };
    }

    fn note_read(texts: &[&Option<String>]) {
        READ_TEXT.with(|r| {
            r.borrow_mut()
                .extend(texts.iter().filter_map(|t| (*t).clone()))
        });
    }

    /// An element of a hand-made Accessibility snapshot, in the probe's
    /// short keys: role, DOM classes, description, value, children.
    #[derive(Clone, Debug, Default, Deserialize)]
    struct Node {
        #[serde(default)]
        r: String,
        #[serde(default)]
        dc: String,
        de: Option<String>,
        va: Option<String>,
        #[serde(default)]
        k: Vec<Node>,
        #[serde(default)]
        gone: bool,
    }

    impl Element for Node {
        fn outline(&self) -> Option<Info<Self>> {
            (!self.gone).then(|| Info {
                role: self.r.clone(),
                classes: self.dc.split_whitespace().map(str::to_string).collect(),
                description: None,
                value: None,
                children: self.k.clone(),
            })
        }

        fn info(&self) -> Option<Info<Self>> {
            note_read(&[&self.de, &self.va]);
            let mut info = self.outline()?;
            info.description = self.de.clone();
            info.value = self.va.clone();
            Some(info)
        }

        fn description(&self) -> Option<String> {
            note_read(&[&self.de]);
            self.de.clone()
        }
    }

    fn windows() -> Vec<Node> {
        serde_json::from_str(include_str!("window_ax_fixture.json")).unwrap()
    }

    fn find(windows: &[Node]) -> (Node, usize) {
        match Search::huddle(windows).step(|| true) {
            Step::Found(huddle, w) => (huddle, w),
            _ => panic!("huddle not found"),
        }
    }

    #[test]
    fn finds_the_huddle_but_never_looks_inside_messages() {
        let windows = windows();
        let (huddle, w) = find(&windows);
        assert_eq!(w, 1, "the decoy inside the message pane was skipped");
        assert_eq!(huddle.de.as_deref(), Some("Gallery"));
    }

    #[test]
    fn reads_names_and_who_is_talking_from_the_tiles() {
        let (huddle, _) = find(&windows());
        let tiles = read_huddle(&huddle, || true).unwrap();
        assert_eq!(tiles.present, ["Mohan", "Priya", "Asha"]);
        assert_eq!(
            tiles.speaking,
            ["Mohan"],
            "table columns repeat cells and are skipped"
        );
        assert!(tiles.complete);
    }

    #[test]
    fn reads_the_signed_in_user_from_its_button() {
        let windows = windows();
        let Step::Found(button, _) = Search::user(&windows).step(|| true) else {
            panic!("user button not found");
        };
        assert_eq!(user_name(&button).as_deref(), Some("Mohan"));
    }

    #[test]
    fn no_text_outside_the_huddle_is_read_but_the_users_name() {
        READ_TEXT.with(|r| r.borrow_mut().clear());
        let windows = windows();
        let (huddle, _) = find(&windows);
        read_huddle(&huddle, || true).unwrap();
        if let Step::Found(button, _) = Search::user(&windows).step(|| true) {
            user_name(&button);
        }
        let read = READ_TEXT.with(|r| r.borrow().clone());
        assert!(read.iter().any(|t| t == "User: Mohan"));
        assert!(
            read.iter().all(|t| !t.contains("SECRET")),
            "text outside the huddle was read: {read:?}"
        );
    }

    #[test]
    fn a_reading_cut_short_by_the_time_limit_is_not_complete() {
        let (huddle, _) = find(&windows());
        let mut left = 6;
        let tiles = read_huddle(&huddle, || {
            left -= 1;
            left >= 0
        })
        .unwrap();
        assert!(!tiles.complete);
    }

    #[test]
    fn a_search_cut_short_goes_on_in_the_next_sample() {
        let windows = windows();
        let mut search = Search::huddle(&windows);
        let mut steps = 0;
        let found = loop {
            steps += 1;
            let mut left = 3;
            match search.step(|| {
                left -= 1;
                left >= 0
            }) {
                Step::Found(_, w) => break w,
                Step::Pending => continue,
                Step::Exhausted => panic!("huddle not found"),
            }
        };
        assert_eq!(found, 1);
        assert!(steps > 2);
    }

    #[test]
    fn no_huddle_is_searched_to_the_end() {
        let windows = vec![windows().remove(0)];
        assert!(matches!(
            Search::huddle(&windows).step(|| true),
            Step::Exhausted
        ));
    }

    #[test]
    fn a_huddle_that_closed_or_changed_reads_as_none() {
        let (mut huddle, _) = find(&windows());
        huddle.dc = "p-view_contents".into();
        assert_eq!(read_huddle(&huddle, || true), None);
        huddle.gone = true;
        assert_eq!(read_huddle(&huddle, || true), None);
    }

    #[test]
    fn three_cut_readings_in_a_row_raise_the_read_limit() {
        let mut limit = ReadLimit::default();
        assert_eq!(limit.limit(), Duration::from_millis(20));
        for cut in [true, true, false, true, true] {
            limit.after(cut);
        }
        assert_eq!(limit.limit(), Duration::from_millis(20));
        limit.after(true);
        assert_eq!(limit.limit(), Duration::from_millis(60));
        limit.after(false);
        assert_eq!(limit.limit(), Duration::from_millis(60));
    }

    #[test]
    fn the_meetings_readings_are_summed_up_in_one_line() {
        let mut stats = Stats::default();
        for _ in 0..84 {
            stats.read(&Sample::default());
        }
        stats.user_found = true;
        assert_eq!(stats.summary(), "Slack names: 84 reads, 0 gaps, user found");
        stats.read(&Sample {
            gap: true,
            ..Default::default()
        });
        stats.cut += 2;
        stats.away += 1;
        stats.user_found = false;
        assert_eq!(
            stats.summary(),
            "Slack names: 85 reads, 1 gaps, user not found, 2 cut short, 1 out of reach"
        );
    }

    #[test]
    fn profile_descriptions_give_the_name() {
        assert_eq!(
            profile_name("View Priya's profile").as_deref(),
            Some("Priya")
        );
        assert_eq!(
            profile_name("View Mohan Lal\u{2019}s profile").as_deref(),
            Some("Mohan Lal")
        );
        assert_eq!(profile_name("video is off, audio is on"), None);
        assert_eq!(profile_name("View 's profile"), None);
    }

    #[test]
    fn samples_round_trip_and_gaps_stay_short() {
        let seen = Sample {
            ms_since_meeting_start: 700,
            speaking: vec!["Priya".into()],
            present: vec!["Mohan".into(), "Priya".into()],
            gap: false,
            user: Some("Mohan".into()),
        };
        let line = serde_json::to_string(&seen).unwrap();
        assert!(!line.contains("gap"));
        let gap = Sample {
            ms_since_meeting_start: 0,
            gap: true,
            ..Default::default()
        };
        let gap_line = serde_json::to_string(&gap).unwrap();
        assert!(!gap_line.contains("user"));
        let text = format!("{line}\nnot json\n{gap_line}\n");
        assert_eq!(parse(&text), vec![gap, seen]);
    }

    /// Reads the running Slack by hand (no huddle needed) and prints the
    /// time per sample: cargo test --lib window_reader_cost -- --ignored
    /// --nocapture
    #[cfg(target_os = "macos")]
    #[test]
    #[ignore]
    fn window_reader_cost() {
        let (samples, windows, visited, search) = live::probe(30);
        let us: Vec<u128> = samples.iter().map(|(_, t)| t.as_micros()).collect();
        let found = samples.iter().filter(|(s, _)| !s.gap).count();
        let mut sorted = us.clone();
        sorted.sort_unstable();
        println!(
            "WINDOW samples={} with_huddle={found} median_us={} max_us={} each_us={us:?}",
            us.len(),
            sorted[us.len() / 2],
            sorted[us.len() - 1]
        );
        println!(
            "WINDOW windows={windows} one_whole_search: elements={visited} ms={}",
            search.as_millis()
        );
    }
}
