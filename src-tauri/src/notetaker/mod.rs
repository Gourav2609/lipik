//! Notetaker: records a meeting as two streams (microphone = "Me", system
//! audio = "Them"), transcribes it live and again at the end, and writes notes
//! with a local model (built in, or Ollama). See CONTRACT.md for the commands
//! and events.

mod ai;
pub mod ai_apps;
mod audio;
pub mod calendar;
pub mod commands;
#[cfg(target_os = "macos")]
mod coreaudio;
mod detect;
mod diarize;
mod echo;
mod hinglish;
mod live;
pub mod mcp;
mod store;
mod tidy;
mod transcribe;
mod window;

use anyhow::{anyhow, Result};
use serde::{Deserialize, Serialize};
use specta::Type;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};
use tauri::{AppHandle, Manager};
use tauri_specta::Event;

use audio::{create_wav, MicCapture, SharedFeed, StreamWriter};
use calendar::CalendarEvent;
use store::Store;

#[derive(Clone, Debug, Serialize, Deserialize, Type)]
#[serde(default)]
pub struct NotetakerSettings {
    /// Watch which apps use the microphone and announce calls.
    pub detect_calls: bool,
    /// Start recording by itself when a call is detected.
    pub auto_start: bool,
    /// Heads-up before a calendar meeting starts; 0 turns it off.
    pub notify_before_seconds: u32,
    /// Recording stops by itself after this long.
    pub max_recording_minutes: u32,
    pub ollama_url: String,
    pub ollama_model: String,
    /// What people speak in meetings; Hinglish keeps Hindi words and names
    /// in the final transcript.
    pub language: MeetingLanguage,
    /// Read who Slack shows as talking in a huddle to name speakers.
    pub meeting_window_names: bool,
    /// Learn the user's voice from their dictation recordings, to tell them
    /// apart from other people at the same Mac.
    pub learn_own_voice: bool,
}

/// `Auto` decides per meeting by listening for Hindi; see `hinglish`.
#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize, Type, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum MeetingLanguage {
    #[default]
    Auto,
    English,
    Hinglish,
}

impl Default for NotetakerSettings {
    fn default() -> Self {
        Self {
            detect_calls: true,
            auto_start: false,
            notify_before_seconds: 60,
            max_recording_minutes: 120,
            ollama_url: "http://127.0.0.1:11434".to_string(),
            ollama_model: "qwen3:8b".to_string(),
            language: MeetingLanguage::default(),
            meeting_window_names: false,
            learn_own_voice: true,
        }
    }
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, Type, PartialEq)]
pub struct Attendee {
    pub name: Option<String>,
    pub email: Option<String>,
    pub is_me: bool,
    /// A room, resource or group, as the calendar marks it.
    #[serde(default)]
    pub not_person: bool,
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, Type, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum MeetingStatus {
    Recording,
    Processing,
    Ready,
    Failed,
}

impl MeetingStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Recording => "recording",
            Self::Processing => "processing",
            Self::Ready => "ready",
            Self::Failed => "failed",
        }
    }

    pub fn parse(s: &str) -> Self {
        match s {
            "recording" => Self::Recording,
            "processing" => Self::Processing,
            "failed" => Self::Failed,
            "ready" => Self::Ready,
            other => {
                log::warn!("Notetaker: unknown meeting status {other:?}, reading it as ready");
                Self::Ready
            }
        }
    }
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, Type, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum SummaryStatus {
    Pending,
    Ready,
    Unavailable,
    Failed,
}

impl SummaryStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Pending => "pending",
            Self::Ready => "ready",
            Self::Unavailable => "unavailable",
            Self::Failed => "failed",
        }
    }

    pub fn parse(s: &str) -> Self {
        match s {
            "ready" => Self::Ready,
            "unavailable" => Self::Unavailable,
            "failed" => Self::Failed,
            _ => Self::Pending,
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, Type)]
pub struct Meeting {
    pub id: String,
    pub title: String,
    /// The title was made up (app name or AI) rather than given by the user or
    /// the calendar; notes may replace it.
    pub title_auto: bool,
    pub start_ms: i64,
    pub end_ms: Option<i64>,
    pub app: Option<String>,
    pub bundle_id: Option<String>,
    pub calendar_event_id: Option<String>,
    pub attendees: Vec<Attendee>,
    pub folder: Option<String>,
    pub status: MeetingStatus,
    pub summary_status: SummaryStatus,
    pub summary_error: Option<String>,
    pub mic_offset_ms: Option<i64>,
    pub system_offset_ms: Option<i64>,
    /// Set while the meeting is being redone in Hinglish.
    #[serde(default)]
    pub hinglish_redo: Option<HinglishRedo>,
    /// Why the Hinglish redo the user asked for failed, until it is
    /// dismissed or started again.
    #[serde(default)]
    pub hinglish_error: Option<String>,
    /// Its recording is still on disk, so it can be redone.
    #[serde(default)]
    pub has_audio: bool,
}

/// Why a meeting is being redone in Hinglish: Hindi was heard in it, or the
/// user asked.
#[derive(Clone, Copy, Debug, Serialize, Deserialize, Type, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum HinglishRedo {
    Auto,
    Asked,
}

#[derive(Clone, Debug, Serialize, Deserialize, Type, PartialEq)]
pub struct Segment {
    /// "mic" or "system".
    pub source: String,
    /// What to show: "Me", "Them" (call audio not split by voice),
    /// "Speaker 1", "Speaker 2", … or a name.
    pub speaker: String,
    /// Which voice this is, fixed while `speaker` may be renamed: "me" (mic),
    /// "them" (call audio not split), or "s1", "s2", … in order of first
    /// appearance. Every line of one key shares one label.
    pub speaker_key: String,
    /// From the start of the meeting.
    pub start_ms: i64,
    pub end_ms: i64,
    pub text: String,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, Type)]
pub struct NoteSection {
    #[serde(default)]
    pub heading: String,
    #[serde(default)]
    pub bullets: Vec<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize, Type)]
pub struct ActionItem {
    pub text: String,
    pub owner: Option<String>,
    pub due: Option<String>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, Type)]
pub struct NoteSummary {
    pub title: Option<String>,
    pub overview: String,
    pub sections: Vec<NoteSection>,
    pub key_points: Vec<String>,
    pub decisions: Vec<String>,
    pub action_items: Vec<ActionItem>,
    /// The transcript words behind each decision and proposal, kept for
    /// checking, not shown.
    #[serde(default)]
    pub quotes: Vec<Quote>,
}

#[derive(Clone, Debug, Serialize, Deserialize, Type)]
pub struct Quote {
    /// The decision, or the proposal line as shown.
    pub item: String,
    pub quote: String,
}

#[derive(Clone, Debug, Serialize, Deserialize, Type)]
pub struct MeetingDetail {
    pub meeting: Meeting,
    pub segments: Vec<Segment>,
    /// The user's own notes ("My notes"), markdown.
    pub thoughts: String,
    pub summary: Option<NoteSummary>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, Type)]
pub struct NotetakerFilter {
    /// Matches title, transcript, notes and the user's thoughts.
    pub query: Option<String>,
    pub folder: Option<String>,
    pub after_ms: Option<i64>,
    pub before_ms: Option<i64>,
    pub limit: Option<u32>,
    pub offset: Option<u32>,
}

#[derive(Clone, Debug, Serialize, Deserialize, Type)]
pub struct FolderInfo {
    pub name: String,
    pub count: u32,
}

#[derive(Clone, Debug, Serialize, Deserialize, Type)]
pub struct Citation {
    /// The [n] marker used in the answer text.
    pub marker: u32,
    pub meeting_id: String,
    pub meeting_title: String,
    pub start_ms: i64,
    pub quote: String,
}

#[derive(Clone, Debug, Serialize, Deserialize, Type)]
pub struct ChatMessage {
    pub id: i64,
    pub thread_id: String,
    /// `None` for questions asked across all meetings.
    pub meeting_id: Option<String>,
    /// "user" or "assistant".
    pub role: String,
    pub content: String,
    pub citations: Vec<Citation>,
    pub created_ms: i64,
}

#[derive(Clone, Debug, Serialize, Deserialize, Type)]
pub struct ChatThread {
    pub thread_id: String,
    pub meeting_id: Option<String>,
    pub started_ms: i64,
    pub updated_ms: i64,
    pub first_question: String,
}

#[derive(Clone, Debug, Serialize, Deserialize, Type)]
pub struct AskAnswer {
    pub thread_id: String,
    pub question: ChatMessage,
    pub answer: ChatMessage,
}

#[derive(Clone, Debug, Serialize, Deserialize, Type, PartialEq)]
pub struct DetectedCall {
    pub app: String,
    pub bundle_id: String,
}

#[derive(Clone, Debug, Serialize, Deserialize, Type, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum StreamState {
    Off,
    Starting,
    Capturing,
    /// Running but only digital silence so far: for system audio this usually
    /// means the "System Audio Recording" permission was denied.
    Silent,
    Unavailable,
}

#[derive(Clone, Debug, Serialize, Deserialize, Type)]
pub struct StreamStatus {
    pub state: StreamState,
    pub detail: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize, Type)]
pub struct NotetakerStatus {
    pub recording: bool,
    /// A start is still opening the devices (macOS may be showing the System
    /// Audio Recording prompt). `recording` is false until it finishes.
    pub starting: bool,
    pub meeting_id: Option<String>,
    pub title: Option<String>,
    pub started_ms: Option<i64>,
    pub elapsed_ms: i64,
    pub mic: StreamStatus,
    pub system: StreamStatus,
    pub active_calls: Vec<DetectedCall>,
    /// Meetings whose final transcript or notes are being made.
    pub processing: Vec<String>,
    pub calendar_access: calendar::CalendarAccess,
}

#[derive(Clone, Debug, Serialize, Deserialize, Type, tauri_specta::Event)]
pub struct NotetakerCallDetected {
    pub app: String,
    pub bundle_id: String,
}

#[derive(Clone, Debug, Serialize, Deserialize, Type, tauri_specta::Event)]
pub struct NotetakerCallEnded {
    pub app: String,
    pub bundle_id: String,
}

#[derive(Clone, Debug, Serialize, Deserialize, Type, tauri_specta::Event)]
pub struct NotetakerMeetingSoon {
    pub event: CalendarEvent,
    pub starts_in_seconds: i64,
}

#[derive(Clone, Debug, Serialize, Deserialize, Type, tauri_specta::Event)]
pub struct NotetakerTranscript {
    pub meeting_id: String,
    pub segments: Vec<Segment>,
    /// `false`: new live segments to append. `true`: the whole final
    /// transcript, replacing everything shown so far.
    pub replace: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum ProcessingStage {
    /// Telling the voices on the call apart (before transcribing).
    Speakers,
    Transcribing,
    Summarizing,
    /// Hindi was heard (or the user asked): the meeting is being transcribed
    /// and its notes written again with the Hinglish speech model. The
    /// meeting keeps its transcript and notes until then.
    Improving,
    /// The Hinglish speech model is being downloaded for a redo; progress is
    /// the share downloaded.
    Downloading,
}

#[derive(Clone, Debug, Serialize, Deserialize, Type, tauri_specta::Event)]
pub struct NotetakerProcessing {
    pub meeting_id: String,
    pub stage: ProcessingStage,
    /// 0..1 for transcribing; speakers and summarizing have no progress
    /// and send 0.
    pub progress: f32,
}

#[derive(Clone, Debug, Serialize, Deserialize, Type, tauri_specta::Event)]
pub struct NotetakerNoteReady {
    pub meeting_id: String,
    pub summary_status: SummaryStatus,
    pub summary_error: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize, Type, tauri_specta::Event)]
pub struct NotetakerState {
    pub recording: bool,
    pub meeting_id: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize, Type, tauri_specta::Event)]
pub struct NotetakerStreamChanged {
    pub meeting_id: String,
    pub mic: StreamStatus,
    pub system: StreamStatus,
}

struct Session {
    meeting_id: String,
    title: String,
    started: Instant,
    start_ms: i64,
    /// Stop when this app's call ends.
    linked_bundle: Option<String>,
    stop: Arc<AtomicBool>,
    mic: Option<MicCapture>,
    mic_feed: SharedFeed,
    mic_error: Option<String>,
    system_feed: SharedFeed,
    system_error: Option<String>,
    system_stop: Option<Box<dyn FnOnce() + Send>>,
    live: Option<JoinHandle<()>>,
}

pub struct NotetakerManager {
    app: AppHandle,
    store: Store,
    dir: PathBuf,
    session: Mutex<Option<Session>>,
    /// Set while `start` opens the devices, which it does without holding the
    /// session lock (a permission prompt can keep a device open waiting).
    starting: StartGate,
    processing: Mutex<Vec<String>>,
    /// Meetings being redone with the Hinglish speech model.
    improving: Mutex<Vec<String>>,
    /// Meetings deleted while being redone; the redo stops.
    cancelled: Mutex<Vec<String>>,
    /// Meetings whose redo waits for the Hinglish model download.
    fetch_waiters: Mutex<Vec<String>>,
    /// Whether a redo started that download (no one else was downloading
    /// the model then), so a delete may cancel it.
    fetch_ours: AtomicBool,
    calls: Mutex<Vec<DetectedCall>>,
}

pub fn now_ms() -> i64 {
    chrono::Utc::now().timestamp_millis()
}

fn new_meeting_id() -> String {
    let now = chrono::Local::now();
    format!(
        "{}-{:04x}",
        now.format("%Y%m%d-%H%M%S"),
        now.timestamp_subsec_nanos() & 0xffff
    )
}

pub fn settings(app: &AppHandle) -> NotetakerSettings {
    crate::settings::get_settings(app).notetaker
}

/// Exact zeros this long mean the stream hears nothing: for the microphone
/// usually a permission problem; for system audio only worth a hint while a
/// call is going on (the other side may simply not have joined yet).
const MIC_SILENT_SAMPLES: u64 = 10 * audio::RATE as u64;
const SYSTEM_SILENT_SAMPLES: u64 = 30 * audio::RATE as u64;

fn stream_status(
    feed: &SharedFeed,
    error: &Option<String>,
    system: bool,
    call_active: bool,
) -> StreamStatus {
    if let Some(error) = error {
        return StreamStatus {
            state: StreamState::Unavailable,
            detail: Some(error.clone()),
        };
    }
    let f = feed.lock().unwrap_or_else(|e| e.into_inner());
    if let Some(failed) = &f.failed {
        return StreamStatus {
            state: StreamState::Unavailable,
            detail: Some(failed.clone()),
        };
    }
    let all_silent = f.silent_samples == f.samples;
    let (state, detail) = if f.samples == 0 {
        (StreamState::Starting, None)
    } else if system && all_silent && call_active && f.samples > SYSTEM_SILENT_SAMPLES {
        (
            StreamState::Silent,
            Some(format!("Nothing heard from the call yet. If others are talking, allow {} under System Settings > Privacy & Security > System Audio Recording Only.", crate::brand::APP_NAME)),
        )
    } else if !system && all_silent && f.samples > MIC_SILENT_SAMPLES {
        (
            StreamState::Silent,
            Some(
                "Nothing heard from the microphone yet. Check the microphone and its permission."
                    .to_string(),
            ),
        )
    } else {
        (StreamState::Capturing, None)
    };
    StreamStatus { state, detail }
}

/// Stream status while nothing records: `starting` while a start opens the
/// devices, else off.
fn idle_stream(starting: bool, system: bool) -> StreamStatus {
    if !starting {
        return StreamStatus {
            state: StreamState::Off,
            detail: None,
        };
    }
    StreamStatus {
        state: StreamState::Starting,
        detail: (system && cfg!(target_os = "macos")).then(|| {
            format!(
                "Starting. If macOS asks, allow {} under System Audio Recording.",
                crate::brand::APP_NAME
            )
        }),
    }
}

struct PendingStart {
    meeting_id: String,
    /// Stop or Delete came while the start waited on a device or prompt.
    cancelled: bool,
}

/// The start in progress, if any.
#[derive(Default)]
struct StartGate(Mutex<Option<PendingStart>>);

impl StartGate {
    fn lock(&self) -> std::sync::MutexGuard<'_, Option<PendingStart>> {
        self.0.lock().unwrap_or_else(|e| e.into_inner())
    }

    fn begin(&self, meeting_id: &str) -> bool {
        let mut pending = self.lock();
        if pending.is_some() {
            return false;
        }
        *pending = Some(PendingStart {
            meeting_id: meeting_id.to_string(),
            cancelled: false,
        });
        true
    }

    /// A start is opening the devices and nothing has cancelled it.
    fn waiting(&self) -> bool {
        self.lock().as_ref().is_some_and(|p| !p.cancelled)
    }

    fn cancelled(&self) -> bool {
        self.lock().as_ref().is_some_and(|p| p.cancelled)
    }

    /// Cancels the start in progress (only when it is for meeting `only`, if
    /// given) and returns its meeting id.
    fn cancel(&self, only: Option<&str>) -> Option<String> {
        let mut pending = self.lock();
        let p = pending
            .as_mut()
            .filter(|p| only.is_none_or(|id| id == p.meeting_id))?;
        p.cancelled = true;
        Some(p.meeting_id.clone())
    }

    fn finish(&self) {
        *self.lock() = None;
    }
}

struct ClearOnDrop<'a>(&'a StartGate);

impl Drop for ClearOnDrop<'_> {
    fn drop(&mut self) {
        self.0.finish();
    }
}

impl NotetakerManager {
    pub fn new(app: &AppHandle) -> Result<Arc<Self>> {
        let dir = crate::portable::app_data_dir(app)?.join("notetaker");
        std::fs::create_dir_all(&dir)?;
        let store = Store::open(&dir.join("notetaker.db"))?;
        Ok(Arc::new(Self {
            app: app.clone(),
            store,
            dir,
            session: Mutex::new(None),
            starting: StartGate::default(),
            processing: Mutex::new(Vec::new()),
            improving: Mutex::new(Vec::new()),
            cancelled: Mutex::new(Vec::new()),
            fetch_waiters: Mutex::new(Vec::new()),
            fetch_ours: AtomicBool::new(false),
            calls: Mutex::new(Vec::new()),
        }))
    }

    fn meeting_dir(&self, id: &str) -> PathBuf {
        self.dir.join(id)
    }

    /// `stopped` is the meeting a stop just ended, sent as the id when nothing
    /// records now so listeners know which meeting stopped.
    fn emit_state(&self, stopped: Option<String>) {
        let session = self.session.lock().unwrap();
        let _ = NotetakerState {
            recording: session.is_some(),
            meeting_id: session.as_ref().map(|s| s.meeting_id.clone()).or(stopped),
        }
        .emit(&self.app);
    }

    pub fn active_calls(&self) -> Vec<DetectedCall> {
        self.calls.lock().unwrap().clone()
    }

    pub fn recording_meeting(&self) -> Option<String> {
        self.session
            .lock()
            .unwrap()
            .as_ref()
            .map(|s| s.meeting_id.clone())
    }

    /// The call app the current recording stops with.
    pub fn linked_bundle(&self) -> Option<String> {
        self.session
            .lock()
            .unwrap()
            .as_ref()
            .and_then(|s| s.linked_bundle.clone())
    }

    fn stream_states(&self, id: &str) -> Option<(StreamStatus, StreamStatus)> {
        let call_active = !self.calls.lock().unwrap().is_empty();
        let session = self.session.lock().unwrap();
        let s = session.as_ref().filter(|s| s.meeting_id == id)?;
        Some((
            stream_status(&s.mic_feed, &s.mic_error, false, call_active),
            stream_status(&s.system_feed, &s.system_error, true, call_active),
        ))
    }

    pub fn status(&self) -> NotetakerStatus {
        let active_calls = self.active_calls();
        let call_active = !active_calls.is_empty();
        let session = self.session.lock().unwrap();
        let starting = session.is_none() && self.starting.waiting();
        let off = idle_stream(starting, false);
        let system_off = idle_stream(starting, true);
        NotetakerStatus {
            recording: session.is_some(),
            starting,
            meeting_id: session.as_ref().map(|s| s.meeting_id.clone()),
            title: session.as_ref().map(|s| s.title.clone()),
            started_ms: session.as_ref().map(|s| s.start_ms),
            elapsed_ms: session
                .as_ref()
                .map_or(0, |s| s.started.elapsed().as_millis() as i64),
            mic: session.as_ref().map_or(off.clone(), |s| {
                stream_status(&s.mic_feed, &s.mic_error, false, call_active)
            }),
            system: session.as_ref().map_or(system_off, |s| {
                stream_status(&s.system_feed, &s.system_error, true, call_active)
            }),
            active_calls,
            processing: self.processing.lock().unwrap().clone(),
            calendar_access: calendar::access(),
        }
    }

    /// Starts recording. Without a title the current calendar event's title
    /// is used, else the call's app name. A call detected at this moment is
    /// linked, so the recording stops when that call ends.
    pub fn start(
        self: &Arc<Self>,
        title: Option<String>,
        call: Option<DetectedCall>,
    ) -> Result<Meeting> {
        let id = new_meeting_id();
        if !self.starting.begin(&id) {
            return Err(anyhow!("A recording is already starting"));
        }
        let _clear = ClearOnDrop(&self.starting);
        self.start_unlocked(id, title, call)
    }

    /// Undoes a start that failed after its meeting was saved: closes the
    /// streams that opened and removes the meeting and its folder.
    fn abandon_start(
        &self,
        id: &str,
        mic: Option<MicCapture>,
        system_stop: Option<Box<dyn FnOnce() + Send>>,
    ) {
        if let Some(mic) = mic {
            mic.stop();
        }
        if let Some(stop) = system_stop {
            stop();
        }
        let _ = self.store.delete(id);
        let _ = std::fs::remove_dir_all(self.meeting_dir(id));
    }

    /// Undoes a start that Stop or Delete cancelled while it waited.
    fn cancel_start(
        &self,
        id: &str,
        mic: Option<MicCapture>,
        system_stop: Option<Box<dyn FnOnce() + Send>>,
    ) -> anyhow::Error {
        log::info!("Notetaker: start of meeting {id} was cancelled");
        self.abandon_start(id, mic, system_stop);
        self.emit_state(None);
        anyhow!("Recording was cancelled")
    }

    fn start_unlocked(
        self: &Arc<Self>,
        id: String,
        title: Option<String>,
        call: Option<DetectedCall>,
    ) -> Result<Meeting> {
        if let Some(s) = self.session.lock().unwrap().as_ref() {
            return Err(anyhow!("Already recording meeting {}", s.meeting_id));
        }

        let start_ms = now_ms();
        let started = Instant::now();
        let (window_start, window_end) = (start_ms - 3_600_000, start_ms + 3_600_000);
        let events = calendar::events_between(window_start, window_end);
        let event = calendar::current_event(&events, start_ms, 10 * 60_000).cloned();
        let call = call.or_else(|| self.active_calls().last().cloned());

        let given = title
            .map(|t| t.trim().to_string())
            .filter(|t| !t.is_empty());
        let (title, title_auto) = match (given, &event, &call) {
            (Some(t), _, _) => (t, false),
            (None, Some(e), _) => (e.title.clone(), false),
            (None, None, Some(c)) => (format!("{} call", c.app), true),
            (None, None, None) => ("New note".to_string(), true),
        };

        let dir = self.meeting_dir(&id);
        std::fs::create_dir_all(&dir)?;

        let meeting = Meeting {
            id: id.clone(),
            title: title.clone(),
            title_auto,
            start_ms,
            end_ms: None,
            app: call.as_ref().map(|c| c.app.clone()),
            bundle_id: call.as_ref().map(|c| c.bundle_id.clone()),
            calendar_event_id: event.as_ref().map(|e| e.id.clone()),
            attendees: event.map(|e| e.attendees).unwrap_or_default(),
            folder: None,
            status: MeetingStatus::Recording,
            summary_status: SummaryStatus::Pending,
            summary_error: None,
            mic_offset_ms: None,
            system_offset_ms: None,
            hinglish_redo: None,
            hinglish_error: None,
            has_audio: false,
        };
        self.store.insert_meeting(&meeting)?;

        let app_settings = crate::settings::get_settings(&self.app);
        let mic_name = app_settings
            .clamshell_microphone
            .clone()
            .filter(|_| crate::helpers::clamshell::is_clamshell().unwrap_or(false))
            .or(app_settings.selected_microphone.clone());
        let mic_feed = SharedFeed::default();
        let (mic, mic_error) = match create_wav(&dir.join("mic.wav")).and_then(|wav| {
            MicCapture::start(
                mic_name,
                app_settings.selected_channel,
                StreamWriter::new(wav, mic_feed.clone()),
                self.app.clone(),
            )
        }) {
            Ok(mic) => (Some(mic), None),
            Err(e) => {
                log::error!("Notetaker could not open the microphone: {e}");
                (None, Some(e.to_string()))
            }
        };
        if self.starting.cancelled() {
            return Err(self.cancel_start(&id, mic, None));
        }

        let system_feed = SharedFeed::default();
        let (system_stop, system_error) = match start_system_audio(&dir, system_feed.clone()) {
            Ok(stop) => (Some(stop), None),
            Err(e) => {
                log::warn!("Notetaker system audio unavailable: {e}");
                (None, Some(e.to_string()))
            }
        };

        if self.starting.cancelled() {
            return Err(self.cancel_start(&id, mic, system_stop));
        }
        if mic.is_none() && system_stop.is_none() {
            self.abandon_start(&id, None, None);
            return Err(anyhow!(
                "Could not record: {}",
                mic_error.unwrap_or_default()
            ));
        }

        let mut session = self.session.lock().unwrap();
        if self.starting.cancelled() {
            drop(session);
            return Err(self.cancel_start(&id, mic, system_stop));
        }
        let stop = Arc::new(AtomicBool::new(false));
        let spawned = {
            let this = self.clone();
            let id = id.clone();
            let stop = stop.clone();
            let streams = vec![("mic", mic_feed.clone()), ("system", system_feed.clone())];
            std::thread::Builder::new()
                .name("notetaker-live".into())
                .spawn(move || this.live_loop(&id, started, streams, stop))
        };
        let live = match spawned {
            Ok(live) => live,
            Err(e) => {
                drop(session);
                log::error!("Notetaker could not start the live transcript: {e}");
                self.abandon_start(&id, mic, system_stop);
                return Err(anyhow!("Could not start recording: {e}"));
            }
        };

        *session = Some(Session {
            meeting_id: id.clone(),
            title,
            started,
            start_ms,
            linked_bundle: call.map(|c| c.bundle_id),
            stop,
            mic,
            mic_feed,
            mic_error,
            system_feed,
            system_error,
            system_stop,
            live: Some(live),
        });
        drop(session);
        self.emit_state(None);
        log::info!("Notetaker started meeting {id}");
        Ok(meeting)
    }

    /// Stops recording and starts the final transcript and notes in the
    /// background. Returns the meeting id.
    pub fn stop(self: &Arc<Self>) -> Result<String> {
        self.stop_inner(None, true)
    }

    /// Stops the recording (only when it is meeting `only`, if given). Without
    /// `process` the meeting is left as recorded, for delete. A start still
    /// opening the devices is cancelled instead: it undoes itself when the
    /// device or prompt it waits on returns.
    fn stop_inner(self: &Arc<Self>, only: Option<&str>, process: bool) -> Result<String> {
        let session = {
            let mut guard = self.session.lock().unwrap();
            let Some(current) = guard.as_ref().map(|s| s.meeting_id.clone()) else {
                return self
                    .starting
                    .cancel(only)
                    .ok_or_else(|| anyhow!("Not recording"));
            };
            if only.is_some_and(|id| id != current) {
                return Err(anyhow!("Not recording that meeting"));
            }
            guard.take().unwrap()
        };
        session.stop.store(true, Ordering::Relaxed);
        if let Some(mic) = session.mic {
            mic.stop();
        }
        if let Some(stop) = session.system_stop {
            stop();
        }
        if let Some(live) = session.live {
            let _ = live.join();
        }
        let id = session.meeting_id;
        let _ = self.store.set_end(&id, now_ms());
        let offset = |feed: &SharedFeed| {
            feed.lock()
                .unwrap_or_else(|e| e.into_inner())
                .first_at
                .map(|t| t.saturating_duration_since(session.started).as_millis() as i64)
        };
        let _ =
            self.store
                .set_offsets(&id, offset(&session.mic_feed), offset(&session.system_feed));
        if process {
            let _ = self.store.set_status(&id, MeetingStatus::Processing);
        }
        self.emit_state(Some(id.clone()));
        log::info!("Notetaker stopped meeting {id}");
        if process {
            self.process_in_background(id.clone(), true)?;
        } else {
            transcribe::release_model(&self.app);
        }
        Ok(id)
    }

    pub fn stop_if_linked(self: &Arc<Self>, bundle_id: &str) {
        let linked = self
            .session
            .lock()
            .unwrap()
            .as_ref()
            .is_some_and(|s| s.linked_bundle.as_deref() == Some(bundle_id));
        if linked {
            log::info!("Notetaker: call in {bundle_id} ended, stopping");
            let _ = self.stop();
        }
    }

    /// A call detected while a recording with no call runs (started before
    /// joining) becomes that recording's call: it names the note and stops
    /// the recording when it ends.
    pub fn link_call(&self, call: &DetectedCall) {
        let id = {
            let mut session = self.session.lock().unwrap();
            let Some(s) = session.as_mut().filter(|s| s.linked_bundle.is_none()) else {
                return;
            };
            s.linked_bundle = Some(call.bundle_id.clone());
            s.meeting_id.clone()
        };
        log::info!("Notetaker: recording {id} linked to the {} call", call.app);
        let _ = self.store.set_call(&id, &call.app, &call.bundle_id);
        let _ = self
            .store
            .set_auto_title(&id, &format!("{} call", call.app));
        let Ok(Some(meeting)) = self.store.meeting(&id) else {
            return;
        };
        if let Some(s) = self
            .session
            .lock()
            .unwrap()
            .as_mut()
            .filter(|s| s.meeting_id == id)
        {
            s.title = meeting.title;
        }
    }

    /// Opens the microphone again after it failed (unplugged, woke from
    /// sleep). The session lock is not held while the device opens.
    fn reopen_mic_if_failed(&self, id: &str) {
        let mut mic = {
            let mut session = self.session.lock().unwrap();
            let Some(s) = session.as_mut().filter(|s| s.meeting_id == id) else {
                return;
            };
            if s.mic_feed
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .failed
                .is_none()
            {
                return;
            }
            let Some(mic) = s.mic.take() else { return };
            mic
        };
        match mic.reopen() {
            Ok(()) => log::info!("Notetaker reopened the microphone"),
            Err(e) => log::debug!("Notetaker could not reopen the microphone yet: {e}"),
        }
        let mut session = self.session.lock().unwrap();
        match session.as_mut().filter(|s| s.meeting_id == id) {
            Some(s) => s.mic = Some(mic),
            None => mic.stop(),
        }
    }

    /// Runs the live transcript (one thread per stream, see `live`) and
    /// watches the recording: reopens a lost microphone, reports stream
    /// changes, stops at the maximum length.
    fn live_loop(
        self: Arc<Self>,
        id: &str,
        started: Instant,
        streams: Vec<(&'static str, SharedFeed)>,
        stop: Arc<AtomicBool>,
    ) {
        let workers: Vec<JoinHandle<()>> = streams
            .into_iter()
            .filter_map(|(source, feed)| {
                let this = self.clone();
                let id = id.to_string();
                let stop = stop.clone();
                std::thread::Builder::new()
                    .name(format!("notetaker-live-{source}"))
                    .spawn(move || live::run(&this, &id, source, feed, started, &stop))
                    .inspect_err(|e| {
                        log::error!("Notetaker live {source} transcript did not start: {e}")
                    })
                    .ok()
            })
            .collect();
        #[cfg(target_os = "macos")]
        let window = settings(&self.app)
            .meeting_window_names
            .then(|| {
                let this = self.clone();
                let dir = self.meeting_dir(id);
                let stop = stop.clone();
                std::thread::Builder::new()
                    .name("notetaker-window".into())
                    .spawn(move || window::record(|| this.linked_bundle(), &dir, started, &stop))
                    .inspect_err(|e| log::warn!("Notetaker huddle reader did not start: {e}"))
                    .ok()
            })
            .flatten();
        let max = Duration::from_secs(settings(&self.app).max_recording_minutes.max(1) as u64 * 60);
        let mut last_status = String::new();
        let mut next_mic_retry = Instant::now();

        while !stop.load(Ordering::Relaxed) {
            std::thread::sleep(Duration::from_millis(500));
            if Instant::now() >= next_mic_retry {
                next_mic_retry = Instant::now() + Duration::from_secs(5);
                self.reopen_mic_if_failed(id);
            }

            if let Some((mic, system)) = self.stream_states(id) {
                let key = format!(
                    "{:?}{:?}{:?}{:?}",
                    mic.state, system.state, mic.detail, system.detail
                );
                if key != last_status {
                    last_status = key;
                    let _ = NotetakerStreamChanged {
                        meeting_id: id.to_string(),
                        mic,
                        system,
                    }
                    .emit(&self.app);
                }
            }

            if started.elapsed() >= max {
                log::info!("Notetaker reached the maximum recording length");
                let this = self.clone();
                let id = id.to_string();
                std::thread::spawn(move || {
                    let _ = this.stop_inner(Some(&id), true);
                });
                break;
            }
        }
        for worker in workers {
            let _ = worker.join();
        }
        #[cfg(target_os = "macos")]
        if let Some(window) = window {
            let _ = window.join();
        }
    }

    fn fail(&self, id: &str, error: &str) {
        let _ = self.store.set_status(id, MeetingStatus::Failed);
        let _ = self
            .store
            .set_summary_status(id, SummaryStatus::Failed, Some(error));
        let _ = NotetakerNoteReady {
            meeting_id: id.to_string(),
            summary_status: SummaryStatus::Failed,
            summary_error: Some(error.to_string()),
        }
        .emit(&self.app);
    }

    fn process_in_background(self: &Arc<Self>, id: String, transcribe: bool) -> Result<()> {
        {
            let mut processing = self.processing.lock().unwrap();
            if processing.contains(&id) {
                return Err(anyhow!("This meeting is already being processed"));
            }
            processing.push(id.clone());
        }
        let this = self.clone();
        let thread_id = id.clone();
        let spawned = std::thread::Builder::new()
            .name("notetaker-final".into())
            .spawn(move || {
                background_priority();
                let id = thread_id;
                let _ = this.store.add_process_attempt(&id);
                let wants_hinglish = match this.process(&id, transcribe) {
                    Ok(wants) => wants,
                    Err(e) => {
                        log::error!("Notetaker processing of {id} failed: {e}");
                        this.fail(&id, &e.to_string());
                        false
                    }
                };
                let _ = this.store.clear_process_attempts(&id);
                this.processing.lock().unwrap().retain(|m| m != &id);
                if wants_hinglish {
                    this.improve_with_hinglish(id);
                }
            });
        if let Err(e) = spawned {
            self.processing.lock().unwrap().retain(|m| m != &id);
            self.fail(&id, &format!("Could not start processing: {e}"));
            return Err(e.into());
        }
        Ok(())
    }

    /// Final transcript of both streams (when `transcribe`), then notes.
    /// True when Hindi was likely spoken but the Hinglish speech model isn't
    /// on this Mac: the caller fetches it and redoes the meeting.
    fn process(&self, id: &str, transcribe: bool) -> Result<bool> {
        let meeting = self
            .store
            .meeting(id)?
            .ok_or_else(|| anyhow!("Meeting {id} is gone"))?;
        let dir = self.meeting_dir(id);
        let language = settings(&self.app).language;
        let improving = self.improving.lock().unwrap().iter().any(|m| m == id);
        let mut wants_hinglish = false;
        let mut names = diarize::invitees(&meeting.attendees);
        names.extend(own_names_in(&meeting));
        let dictionary = crate::textflow::dictionary_words(&self.app);

        if transcribe && improving {
            let language = if dir.join(HINGLISH_ASKED).exists() {
                MeetingLanguage::Hinglish
            } else {
                language
            };
            if !self.redo_in_hinglish(id, &meeting, &dir, language)? {
                return Ok(false);
            }
        } else if transcribe {
            let system_wav = dir.join("system.wav");
            let live = self.store.segments(id)?;
            let heard_call = live.iter().any(|s| s.source == "system");
            let hinglish = if language != MeetingLanguage::English {
                hinglish::engine(&self.app, hinglish::hotwords(&dictionary, &names))
            } else {
                None
            };
            self.emit_stage(id, ProcessingStage::Speakers, 0.0);
            let speakers = (system_wav.exists() && heard_call)
                .then(|| self.find_speakers(id, &system_wav))
                .flatten();
            save_call_turns(&dir, speakers.as_ref().map(|s| s.turns.as_slice()));
            let cleaned_mic = dir.join(CLEANED_MIC);
            let _ = std::fs::remove_file(&cleaned_mic);
            let mic = mic_for_transcript(&dir, &meeting, &cleaned_mic);
            let room = self.find_room_voices(id, &meeting, &mic, speakers.as_ref(), &live);
            save_turns(
                &dir.join(MIC_TURNS),
                room.as_ref().map(|s| s.turns.as_slice()),
            );
            let sources = [
                ("mic", mic.path.clone(), meeting.mic_offset_ms.unwrap_or(0)),
                (
                    "system",
                    dir.join("system.wav"),
                    meeting.system_offset_ms.unwrap_or(0),
                ),
            ];
            let present: Vec<_> = sources.iter().filter(|(_, p, _)| p.exists()).collect();
            let hinglish = hinglish.filter(|engine| {
                if language == MeetingLanguage::Hinglish {
                    return true;
                }
                let streams: Vec<(&str, &std::path::Path, i64)> = present
                    .iter()
                    .map(|(s, p, o)| (*s, p.as_path(), *o))
                    .collect();
                let share = transcribe::hindi_in_sample(&live, &streams, |audio| {
                    transcribe::transcribe_side(&self.app, engine, audio)
                });
                log::info!("Notetaker heard {share:.2} Hindi words in a sample of {id}");
                share >= hinglish::PROBE_HINDI
            });
            let mut results: Vec<(&str, Vec<Segment>)> = Vec::new();
            let mut speech: Vec<(&str, i64)> = Vec::new();
            let mut failed = None;
            for (n, (source, path, offset)) in present.iter().enumerate() {
                let parts = present.len() as f32;
                let scale = if hinglish.is_some() {
                    HEARING_SHARE
                } else {
                    1.0
                };
                let emit = |fraction: f32| {
                    self.emit_stage(
                        id,
                        ProcessingStage::Transcribing,
                        scale * (n as f32 + fraction) / parts,
                    )
                };
                let split = if *source == "system" {
                    speakers.as_ref()
                } else {
                    room.as_ref()
                };
                match transcribe::transcribe_file(
                    &self.app,
                    path,
                    *offset,
                    source,
                    split,
                    hinglish.as_ref(),
                    emit,
                ) {
                    Ok((segments, heard_ms)) => {
                        results.push((*source, segments));
                        if *source != "mic" || mic.clean {
                            speech.push((*source, heard_ms));
                        }
                    }
                    Err(e) => {
                        failed = Some(e);
                        break;
                    }
                }
            }
            transcribe::release_model(&self.app);
            let _ = std::fs::remove_file(&cleaned_mic);
            let used_hinglish = hinglish.is_some();
            drop(hinglish);
            match failed {
                None => {
                    let (mut mic, mut system) = (Vec::new(), Vec::new());
                    for (source, segments) in results {
                        if source == "mic" {
                            mic = segments;
                        } else {
                            system = segments;
                        }
                    }
                    let mut merged = transcribe::merge_streams(mic, system);
                    if used_hinglish {
                        self.romanize(id, &meeting, &mut merged, &names);
                    } else if language != MeetingLanguage::English
                        && hinglish::downloaded_model(&self.app).is_none()
                    {
                        wants_hinglish = language == MeetingLanguage::Hinglish
                            || (!self.dir.join(HINGLISH_MODEL_DELETED).exists()
                                && hinglish::hindi_likely(&merged, &names, &speech));
                    }
                    tidy::tidy(&mut merged, &tidy::Vocabulary::new(&dictionary, &names));
                    if speakers.is_some() || room.is_some() {
                        self.name_known_speakers(
                            id,
                            &meeting,
                            speakers.as_ref(),
                            room.as_ref(),
                            &mut merged,
                        );
                    }
                    if merged.is_empty() && !self.store.segments(id)?.is_empty() {
                        log::warn!(
                            "Notetaker final pass heard no speech; keeping the live transcript"
                        );
                    } else {
                        self.store.replace_final_segments(id, &merged)?;
                    }
                }
                Some(e) => {
                    log::warn!("Notetaker final pass failed, keeping the live transcript: {e}")
                }
            }
            let _ = NotetakerTranscript {
                meeting_id: id.to_string(),
                segments: self.store.segments(id)?,
                replace: true,
            }
            .emit(&self.app);
        }
        self.store.set_status(id, MeetingStatus::Ready)?;
        self.write_notes(id)?;
        Ok(wants_hinglish)
    }

    /// The meeting's lines heard again by the Hinglish speech model, line by
    /// line, so every line keeps its speaker and every name the user gave
    /// stays (renames made meanwhile included). Lines it hears nothing in
    /// keep their text. Stretches without lines are heard too, since the
    /// English model writes nothing for much of the Hindi it hears: call
    /// stretches with the speakers of the voice split, mic stretches only
    /// when the call could be taken out of the mic, and a mic piece that
    /// sounds like the call is left out.
    /// Under Auto it first samples the meeting like a meeting with the model
    /// already on the Mac does; when that hears too little Hindi (the English
    /// transcript only looked like Hindi), nothing changes and it returns
    /// false.
    fn redo_in_hinglish(
        &self,
        id: &str,
        meeting: &Meeting,
        dir: &std::path::Path,
        language: MeetingLanguage,
    ) -> Result<bool> {
        let mut names = diarize::invitees(&meeting.attendees);
        names.extend(own_names_in(meeting));
        let dictionary = crate::textflow::dictionary_words(&self.app);
        let side = hinglish::engine(&self.app, hinglish::hotwords(&dictionary, &names))
            .ok_or_else(|| anyhow!("The Hinglish speech model did not load"))?;
        let lines = self.store.segments(id)?;
        let cleaned_mic = dir.join(CLEANED_MIC);
        let _ = std::fs::remove_file(&cleaned_mic);
        let mic = mic_for_transcript(dir, meeting, &cleaned_mic);
        let (raw_mic, system) = (dir.join("mic.wav"), dir.join("system.wav"));
        let (mic_offset, system_offset) = (
            meeting.mic_offset_ms.unwrap_or(0),
            meeting.system_offset_ms.unwrap_or(0),
        );
        let streams = [
            ("mic", mic.path.as_path(), mic_offset),
            ("system", system.as_path(), system_offset),
        ];
        if language != MeetingLanguage::Hinglish {
            let share = transcribe::hindi_in_sample(&lines, &streams, |audio| {
                transcribe::transcribe_side(&self.app, &side, audio)
            });
            log::info!(
                "Notetaker heard {share:.2} Hindi words in a sample of {id} before redoing it"
            );
            if share < hinglish::PROBE_HINDI {
                let _ = std::fs::remove_file(&cleaned_mic);
                return Ok(false);
            }
        }
        let spans: Vec<(&str, i64, i64)> = sources_to_fill(&mic, raw_mic.exists(), system.exists())
            .into_iter()
            .filter_map(|source| {
                let (path, offset) = if source == "mic" {
                    (&raw_mic, mic_offset)
                } else {
                    (&system, system_offset)
                };
                let samples = hound::WavReader::open(path).ok()?.duration();
                Some((
                    source,
                    offset,
                    offset + audio::samples_to_ms(samples as u64),
                ))
            })
            .collect();
        let call_turns = if spans.iter().any(|(source, ..)| *source == "system") {
            self.call_turns(id, dir, &lines)
        } else {
            Vec::new()
        };
        let call_turns: Vec<diarize::Turn> = call_turns
            .into_iter()
            .map(|t| diarize::Turn {
                start_ms: t.start_ms + system_offset,
                end_ms: t.end_ms + system_offset,
                ..t
            })
            .collect();
        let mic_turns: Vec<diarize::Turn> = load_turns(&dir.join(MIC_TURNS))
            .unwrap_or_default()
            .into_iter()
            .map(|t| diarize::Turn {
                start_ms: t.start_ms + mic_offset,
                end_ms: t.end_ms + mic_offset,
                ..t
            })
            .collect();
        let todo = transcribe::with_unheard_spans(&lines, &spans, &call_turns, &mic_turns);
        let redone = transcribe::redo_lines(
            &todo,
            &streams,
            hinglish::RULES,
            &side.run.vocabulary,
            || transcribe::new_scorer(&self.app),
            |audio| transcribe::transcribe_side(&self.app, &side, audio),
            |piece| {
                piece.source == "mic"
                    && mic.echo_delay_ms.is_some_and(|delay| {
                        transcribe::call_in_mic(
                            (&raw_mic, &mic.path, mic_offset),
                            (&system, system_offset),
                            delay.round() as i64,
                            piece,
                        )
                    })
            },
            |fraction| {
                self.emit_stage(id, ProcessingStage::Transcribing, HEARING_SHARE * fraction);
                !self.cancelled(id)
            },
        );
        let _ = std::fs::remove_file(&cleaned_mic);
        drop(side);
        if self.cancelled(id) {
            return Err(anyhow!("cancelled"));
        }
        let mut redone = redone;
        redone.sort_by_key(|s| (s.start_ms, s.end_ms));
        self.romanize(id, meeting, &mut redone, &names);
        tidy::tidy(&mut redone, &tidy::Vocabulary::new(&dictionary, &names));
        {
            let _renames_wait = self.improving.lock().unwrap();
            if self.cancelled(id) {
                return Err(anyhow!("cancelled"));
            }
            keep_labels(&mut redone, &self.store.segments(id)?);
            self.store.replace_final_segments(id, &redone)?;
        }
        let _ = NotetakerTranscript {
            meeting_id: id.to_string(),
            segments: self.store.segments(id)?,
            replace: true,
        }
        .emit(&self.app);
        Ok(true)
    }

    /// Lines with Devanagari in Roman letters, with progress, within
    /// `hinglish::romanize_time` of the meeting's length; a delete stops it.
    fn romanize(&self, id: &str, meeting: &Meeting, lines: &mut [Segment], names: &[String]) {
        let model = ai::Model::for_app(&self.app);
        let model = model.unavailable_reason().is_none().then_some(&model);
        let length = meeting.end_ms.unwrap_or(meeting.start_ms) - meeting.start_ms;
        hinglish::romanize(
            model,
            lines,
            names,
            hinglish::romanize_time(length),
            |done| {
                let progress = HEARING_SHARE + (1.0 - HEARING_SHARE) * done;
                self.emit_stage(id, ProcessingStage::Transcribing, progress);
                !self.cancelled(id)
            },
        );
    }

    /// Takes a deleted meeting off the Hinglish model download. True when the
    /// download is now useless: a redo started it and no other waits for it.
    fn leave_download(&self, id: &str) -> bool {
        leave(
            &mut self.fetch_waiters.lock().unwrap(),
            id,
            self.fetch_ours.load(Ordering::Relaxed),
        )
    }

    /// Whether Auto may fetch the Hinglish model: the user never deleted it
    /// nor cancelled its download.
    fn may_fetch_model(&self) -> bool {
        settings(&self.app).language == MeetingLanguage::Hinglish
            || hinglish::downloaded_model(&self.app).is_some()
            || !self.dir.join(HINGLISH_MODEL_DELETED).exists()
    }

    /// Whether the Hinglish model may be fetched for this meeting's redo:
    /// Auto may fetch it, or the user asked for the redo.
    fn may_fetch_for(&self, id: &str) -> bool {
        self.may_fetch_model() || self.meeting_dir(id).join(HINGLISH_ASKED).exists()
    }

    fn end_redo(&self, id: &str) {
        end_redo_in(&self.meeting_dir(id));
    }

    /// The voice split of the call (WAV time): the one saved by the final
    /// pass, else, when the call lines were split by voice, the split made
    /// again (the same audio splits the same way) and saved.
    fn call_turns(&self, id: &str, dir: &std::path::Path, lines: &[Segment]) -> Vec<diarize::Turn> {
        if let Some(turns) = load_call_turns(dir) {
            return turns;
        }
        let split = lines
            .iter()
            .any(|s| s.source == "system" && s.speaker_key != diarize::THEM_KEY);
        if !split {
            return Vec::new();
        }
        self.emit_stage(id, ProcessingStage::Speakers, 0.0);
        let turns = self
            .find_speakers(id, &dir.join("system.wav"))
            .map(|s| s.turns)
            .unwrap_or_default();
        save_call_turns(dir, Some(&turns));
        turns
    }

    /// Hears a finished meeting again with the Hinglish speech model, as
    /// when Auto finds Hindi in it, fetching the model first if needed. For
    /// a meeting that was transcribed in English before the language was
    /// set to Hinglish, or that Auto got wrong.
    pub fn redo_in_hinglish_now(self: &Arc<Self>, id: &str) -> Result<()> {
        let meeting = self
            .store
            .meeting(id)?
            .ok_or_else(|| anyhow!("No meeting with id {id}"))?;
        if self.recording_meeting().as_deref() == Some(id)
            || meeting.status == MeetingStatus::Recording
        {
            return Err(anyhow!("This meeting is still being recorded"));
        }
        if self.processing.lock().unwrap().iter().any(|m| m == id)
            || self.improving.lock().unwrap().iter().any(|m| m == id)
        {
            return Err(anyhow!("This meeting is already being processed"));
        }
        let dir = self.meeting_dir(id);
        if !has_audio(&dir) {
            return Err(anyhow!("The audio of this meeting is gone"));
        }
        std::fs::write(dir.join(HINGLISH_ASKED), "")?;
        let _ = std::fs::remove_file(dir.join(HINGLISH_REDO));
        self.improve_with_hinglish(id.to_string());
        Ok(())
    }

    /// Whether the meeting was deleted while it was being redone.
    fn cancelled(&self, id: &str) -> bool {
        self.cancelled.lock().unwrap().iter().any(|m| m == id)
    }

    fn emit_stage(&self, id: &str, stage: ProcessingStage, progress: f32) {
        let improving = self.improving.lock().unwrap().iter().any(|m| m == id);
        let _ = NotetakerProcessing {
            meeting_id: id.to_string(),
            stage: if improving {
                ProcessingStage::Improving
            } else {
                stage
            },
            progress,
        }
        .emit(&self.app);
    }

    /// Fetches the Hinglish speech model without asking, then transcribes
    /// the meeting again with it, names its speakers again and rewrites its
    /// notes. Until that is done the meeting keeps what it has.
    fn improve_with_hinglish(self: &Arc<Self>, id: String) {
        let marker = self.meeting_dir(&id).join(HINGLISH_REDO);
        if !self.may_fetch_for(&id) {
            log::info!("Notetaker: the Hinglish model was declined, not redoing {id}");
            self.end_redo(&id);
            return;
        }
        let tries = std::fs::read_to_string(&marker)
            .ok()
            .and_then(|t| t.trim().parse::<u32>().ok())
            .unwrap_or(0);
        if tries >= MAX_HINGLISH_TRIES {
            log::warn!("Notetaker gave up redoing {id} in Hinglish after {tries} tries");
            self.end_redo(&id);
            return;
        }
        {
            let mut improving = self.improving.lock().unwrap();
            if improving.contains(&id) {
                return;
            }
            improving.push(id.clone());
        }
        if let Err(e) = std::fs::write(&marker, (tries + 1).to_string()) {
            log::warn!("Notetaker could not note the Hinglish redo of {id}: {e}");
        }
        let _ = std::fs::remove_file(self.meeting_dir(&id).join(HINGLISH_ERROR));
        let this = self.clone();
        let thread_id = id.clone();
        let spawned = std::thread::Builder::new()
            .name("notetaker-hinglish".into())
            .spawn(move || {
                background_priority();
                let id = thread_id;
                this.emit_stage(&id, ProcessingStage::Improving, 0.0);
                log::info!("Notetaker: redoing {id} in Hinglish");
                match this.redo_with_hinglish(&id) {
                    Ok(()) => this.end_redo(&id),
                    Err(e) => {
                        log::warn!("Notetaker could not redo {id} in Hinglish: {e}");
                        let dir = this.meeting_dir(&id);
                        if dir.join(HINGLISH_ASKED).exists() && !this.cancelled(&id) {
                            let _ = std::fs::write(dir.join(HINGLISH_ERROR), e.to_string());
                        }
                        after_failed_redo(
                            &dir,
                            e.is::<DownloadCancelled>(),
                            this.may_fetch_for(&id),
                        );
                    }
                }
                this.improving.lock().unwrap().retain(|m| m != &id);
                this.cancelled.lock().unwrap().retain(|m| m != &id);
                if let Ok(Some(m)) = this.store.meeting(&id) {
                    let _ = NotetakerNoteReady {
                        meeting_id: id.clone(),
                        summary_status: m.summary_status,
                        summary_error: m.summary_error,
                    }
                    .emit(&this.app);
                }
            });
        if let Err(e) = spawned {
            log::warn!("Notetaker could not start the Hinglish pass: {e}");
            self.improving.lock().unwrap().retain(|m| m != &id);
        }
    }

    fn redo_with_hinglish(&self, id: &str) -> Result<()> {
        if hinglish::downloaded_model(&self.app).is_none() {
            self.fetch_hinglish_model(id)?;
        }
        loop {
            if self.cancelled(id) {
                return Err(anyhow!("cancelled"));
            }
            {
                let mut processing = self.processing.lock().unwrap();
                if !processing.iter().any(|m| m == id) {
                    processing.push(id.to_string());
                    break;
                }
            }
            std::thread::sleep(Duration::from_secs(1));
        }
        let result = self.process(id, true).map(|_| ());
        self.processing.lock().unwrap().retain(|m| m != id);
        result
    }

    /// Downloads the Hinglish speech model for the redo of meeting `id`,
    /// sending the download's progress as the meeting's `Downloading` stage.
    fn fetch_hinglish_model(&self, id: &str) -> Result<()> {
        use tauri::Listener;

        let models = self
            .app
            .try_state::<Arc<crate::managers::model::ModelManager>>()
            .ok_or_else(|| anyhow!("the model list is not ready yet"))?
            .inner()
            .clone();
        let model_id = hinglish::download_id()?;
        let info = models
            .get_model_info(&model_id)
            .ok_or_else(|| anyhow!("the Hinglish speech model is missing from the model list"))?;
        if !info.is_downloaded {
            let needed = info.size_mb * 1_000_000 + crate::textflow::cleanup_model::SPARE_DISK;
            let free = crate::textflow::cleanup_model::free_disk(&self.dir);
            if free.is_some_and(|free| free < needed) {
                return Err(anyhow!(
                    "not enough free disk for the Hinglish speech model ({} MB free, {} MB needed)",
                    free.unwrap_or(0) / 1_000_000,
                    needed / 1_000_000
                ));
            }
        }
        log::info!("Notetaker: fetching {model_id} for {id}");
        {
            let mut waiters = self.fetch_waiters.lock().unwrap();
            if waiters.is_empty() {
                self.fetch_ours
                    .store(!info.is_downloading, Ordering::Relaxed);
            }
            waiters.push(id.to_string());
        }
        // Share downloaded (0..1), as f32 bits.
        let done = Arc::new(std::sync::atomic::AtomicU32::new(0));
        let listener = {
            let (done, model_id) = (done.clone(), model_id.clone());
            self.app.listen("model-download-progress", move |event| {
                let Ok(p) = serde_json::from_str::<crate::managers::model::DownloadProgress>(
                    event.payload(),
                ) else {
                    return;
                };
                if p.model_id == model_id {
                    done.store((p.percentage as f32 / 100.0).to_bits(), Ordering::Relaxed);
                }
            })
        };
        let fetching = Arc::new(AtomicBool::new(true));
        let ticker = {
            let (app, id, fetching, done) = (
                self.app.clone(),
                id.to_string(),
                fetching.clone(),
                done.clone(),
            );
            std::thread::spawn(move || {
                while fetching.load(Ordering::Relaxed) {
                    let _ = NotetakerProcessing {
                        meeting_id: id.clone(),
                        stage: ProcessingStage::Downloading,
                        progress: f32::from_bits(done.load(Ordering::Relaxed)),
                    }
                    .emit(&app);
                    std::thread::sleep(Duration::from_secs(1));
                }
            })
        };
        let fetched = tauri::async_runtime::block_on(models.download_model(&model_id));
        self.app.unlisten(listener);
        self.fetch_waiters.lock().unwrap().retain(|m| m != id);
        fetching.store(false, Ordering::Relaxed);
        let _ = ticker.join();
        fetched?;
        if hinglish::downloaded_model(&self.app).is_none() {
            return Err(DownloadCancelled.into());
        }
        Ok(())
    }

    /// Splits the call audio by voice. Any failure (no models, offline on
    /// first use, a crash in the model) keeps one "Them" for the call.
    fn find_speakers(&self, id: &str, wav: &std::path::Path) -> Option<diarize::Speakers> {
        let started = Instant::now();
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            diarize::detect(&self.app, wav)
        }))
        .unwrap_or_else(|_| Err(anyhow!("speaker detection panicked")));
        match result {
            Ok(speakers) => {
                log::info!(
                    "Notetaker found {} speakers in {id} ({}) in {:.1}s",
                    speakers.count(),
                    wav.file_name().unwrap_or_default().to_string_lossy(),
                    started.elapsed().as_secs_f32()
                );
                Some(speakers)
            }
            Err(e) => {
                log::warn!(
                    "Notetaker kept one speaker for {} of {id}: {e}",
                    wav.file_name().unwrap_or_default().to_string_lossy()
                );
                None
            }
        }
    }

    /// The voice split of the microphone when it holds people in the room
    /// besides the user (`diarize::room_voices`), numbered after the call's
    /// voices. Only for a mic the call is kept out of.
    fn find_room_voices(
        &self,
        id: &str,
        meeting: &Meeting,
        mic: &MicAudio,
        call: Option<&diarize::Speakers>,
        live: &[Segment],
    ) -> Option<diarize::Speakers> {
        if !mic.clean || !mic.path.exists() {
            return None;
        }
        let own = self.own_voice();
        let call_speech = call_speech_ms(call, live);
        let no_call = diarize::Speakers::default();
        let call = call.unwrap_or(&no_call);
        let offset = meeting.mic_offset_ms.unwrap_or(0);
        let lines: Vec<(i64, i64)> = live
            .iter()
            .filter(|s| s.source == "mic")
            .map(|s| (s.start_ms - offset, s.end_ms - offset))
            .collect();
        let mic_speech: i64 = lines.iter().map(|(s, e)| e - s).sum();
        if !diarize::quiet_call(call_speech, mic_speech) {
            let Some(own) = own.as_deref() else {
                log::info!("Notetaker kept the mic of {id} as the user: a call, and no voice from dictation");
                return None;
            };
            let shift = mic_to_call_ms(meeting);
            let during = diarize::mic_during_call(&lines, &call.turns, shift);
            let windows = diarize::lone_mic_windows(&lines, &call.turns, shift);
            let checked = (during <= diarize::CHECK_MAX_DURING_CALL)
                .then(|| diarize::detect_window_voices(&self.app, &mic.path, &windows));
            match checked {
                Some(Ok(voices)) if diarize::only_user(&voices, own) => {
                    log::info!(
                        "Notetaker kept the mic of {id} as the user: {} checked stretches sound like them",
                        voices.len()
                    );
                    return None;
                }
                Some(Err(e)) => log::warn!("Notetaker could not check the mic of {id}: {e}"),
                _ => {}
            }
        }
        let split = self.find_speakers(id, &mic.path)?;
        let room = diarize::room_voices(
            &split,
            own.as_deref(),
            call,
            mic_to_call_ms(meeting),
            call_speech,
        );
        log::info!(
            "Notetaker heard {} people besides the user in the mic of {id} (own voice: {})",
            room.as_ref().map_or(0, |r| r.count() - call.count()),
            own.is_some()
        );
        room
    }

    /// The user's voice, made from their newest dictation recordings and
    /// made again once it is a week old. None while learning it is off, and
    /// when there is too little dictation left (the saved one is then
    /// dropped). When the speaker models fail, the saved one is used.
    fn own_voice(&self) -> Option<Vec<f32>> {
        if !settings(&self.app).learn_own_voice {
            return None;
        }
        let saved = self.store.own_voice().ok().flatten();
        if let Some((voice, at)) = &saved {
            if now_ms() - at < OWN_VOICE_MAX_AGE_MS {
                return Some(voice.clone());
            }
        }
        let clips = crate::portable::app_data_dir(&self.app)
            .map(|d| dictation_clips(&d.join("recordings")))
            .unwrap_or_default();
        let made = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            diarize::detect_own_voice(&self.app, &clips)
        }))
        .unwrap_or_else(|_| Err(anyhow!("speaker detection panicked")));
        match made {
            Ok(Some(voice)) => {
                if let Err(e) = self.store.save_own_voice(&voice, now_ms()) {
                    log::warn!("Notetaker could not keep the user's voice: {e}");
                }
                Some(voice)
            }
            Ok(None) => {
                if let Err(e) = self.store.forget_own_voice() {
                    log::warn!("Notetaker could not forget the user's voice: {e}");
                }
                None
            }
            Err(e) => {
                log::warn!("Notetaker could not learn the user's voice from dictation: {e}");
                saved.map(|(voice, _)| voice)
            }
        }
    }

    /// Keeps the meeting's voices (for naming them later) and names the
    /// speakers a saved voice or the calendar already identifies: the call's
    /// voices (`call`) and the people in the room (`room`).
    fn name_known_speakers(
        &self,
        id: &str,
        meeting: &Meeting,
        call: Option<&diarize::Speakers>,
        room: Option<&diarize::Speakers>,
        segments: &mut [Segment],
    ) {
        let mut speakers = call.cloned().unwrap_or_default();
        if let Some(room) = room {
            for n in speakers.count()..room.count() {
                speakers.voices.push(room.voices[n].clone());
                speakers.speech_ms.push(room.speech_ms[n]);
            }
        }
        let voices: Vec<(String, Vec<f32>, i64)> = speakers
            .voices
            .iter()
            .enumerate()
            .filter_map(|(n, v)| Some((diarize::speaker_key(n), v.clone()?, speakers.speech_ms[n])))
            .collect();
        if let Err(e) = self.store.save_meeting_voices(id, &voices) {
            log::warn!("Notetaker could not save the voices of {id}: {e}");
        }
        let profiles = self.store.voice_profiles(Some(id)).unwrap_or_default();
        let mut names = diarize::known_names(&speakers, &profiles, &meeting.attendees);
        let window = window_namings(
            &self.meeting_dir(id),
            meeting,
            call.map_or(&[][..], |c| c.turns.as_slice()),
            room.map_or(&[][..], |r| r.turns.as_slice()),
            segments,
            &own_names_in(meeting),
        );
        let from_voice = |name: &str| profiles.iter().any(|p| p.name == name);
        for (key, ..) in diarize::add_window_names(&mut names, &from_voice, &window) {
            log::info!(
                "Notetaker kept the saved voice for {key} of {id}: the huddle window showed someone else"
            );
        }
        if !window.is_empty() {
            log::info!(
                "Notetaker named {:?} of {id} from the huddle window",
                window.iter().map(|n| n.key.as_str()).collect::<Vec<_>>()
            );
        }
        for (n, name) in names {
            diarize::apply_name(segments, &diarize::speaker_key(n), &name);
        }
    }

    fn write_notes(&self, id: &str) -> Result<()> {
        let meeting = self
            .store
            .meeting(id)?
            .ok_or_else(|| anyhow!("Meeting {id} is gone"))?;
        let segments = self.store.segments(id)?;
        let finish = |status: SummaryStatus, error: Option<String>| -> Result<()> {
            self.store
                .set_summary_status(id, status, error.as_deref())?;
            let _ = NotetakerNoteReady {
                meeting_id: id.to_string(),
                summary_status: status,
                summary_error: error,
            }
            .emit(&self.app);
            Ok(())
        };
        if segments.is_empty() {
            return finish(
                SummaryStatus::Unavailable,
                Some("Nothing was said in this recording.".to_string()),
            );
        }

        let model = ai::Model::for_app(&self.app);
        if let Some(reason) = model.unavailable_reason() {
            return finish(SummaryStatus::Unavailable, Some(reason));
        }
        self.emit_stage(id, ProcessingStage::Summarizing, 0.0);

        let thoughts = self.store.thoughts(id)?;
        match ai::summarize(&model, &meeting, &thoughts, &segments) {
            Ok(_) if self.cancelled(id) => Ok(()),
            Ok((summary, guesses)) => {
                let own = self.own_names(&meeting);
                let invited = diarize::invitees(&meeting.attendees);
                let (named, verdicts) = diarize::judge_guesses(&segments, &guesses, &own, &invited);
                log::info!(
                    "Notetaker named {} speakers of {id} ({} guesses rejected): {:?}",
                    named.len(),
                    verdicts.iter().filter(|v| v.rejected.is_some()).count(),
                    named
                        .iter()
                        .map(|n| (n.key.as_str(), n.rule, n.lines.len()))
                        .collect::<Vec<_>>()
                );
                let mut changed = 0;
                for n in named {
                    let default = diarize::default_label(&n.key);
                    changed += self
                        .store
                        .name_speaker_if_default(id, &n.key, &default, &n.name)?;
                }
                if changed > 0 {
                    let _ = NotetakerTranscript {
                        meeting_id: id.to_string(),
                        segments: self.store.segments(id)?,
                        replace: true,
                    }
                    .emit(&self.app);
                }
                if let Some(title) = &summary.title {
                    self.store.set_auto_title(id, title)?;
                }
                self.store.save_summary(id, &summary, model.model())?;
                finish(SummaryStatus::Ready, None)
            }
            Err(e) => finish(SummaryStatus::Failed, Some(format!("Notes failed: {e}"))),
        }
    }

    /// Hides why the meeting's Hinglish redo failed.
    pub fn dismiss_hinglish_error(&self, id: &str) -> Result<()> {
        match std::fs::remove_file(self.meeting_dir(id).join(HINGLISH_ERROR)) {
            Err(e) if e.kind() != std::io::ErrorKind::NotFound => Err(e.into()),
            _ => Ok(()),
        }
    }

    /// Writes the notes again (once the model is there, or the thoughts
    /// changed). The transcript is kept, unless processing failed: then the
    /// final transcript is made again too.
    pub fn regenerate(self: &Arc<Self>, id: &str) -> Result<()> {
        let meeting = self
            .store
            .meeting(id)?
            .ok_or_else(|| anyhow!("No meeting with id {id}"))?;
        if self.recording_meeting().as_deref() == Some(id) {
            return Err(anyhow!("This meeting is still being recorded"));
        }
        if self.processing.lock().unwrap().iter().any(|m| m == id) {
            return Err(anyhow!("This meeting is already being processed"));
        }
        let transcribe = meeting.status == MeetingStatus::Failed;
        if transcribe {
            let _ = self.store.set_status(id, MeetingStatus::Processing);
        }
        let _ = self
            .store
            .set_summary_status(id, SummaryStatus::Pending, None);
        self.process_in_background(id.to_string(), transcribe)
    }

    /// Writes the notes of meetings that ended while the AI model was still
    /// downloading.
    pub fn write_notes_that_waited(self: &Arc<Self>) {
        let Ok(meetings) = self.store.meetings_with_status(&[MeetingStatus::Ready]) else {
            return;
        };
        for m in meetings {
            if m.summary_status == SummaryStatus::Unavailable
                && m.summary_error.as_deref() == Some(ai::NOTES_WAIT_FOR_MODEL)
            {
                log::info!(
                    "Notetaker: writing notes for {} now the model is here",
                    m.id
                );
                let _ = self.regenerate(&m.id);
            }
        }
    }

    /// Meetings left recording or processing by a crash or quit are finished
    /// from their WAV files. One whose processing was cut off twice is marked
    /// failed instead, in case it is what stops the app.
    pub fn resume_unfinished(self: &Arc<Self>) {
        remove_cleaned_mics(&self.dir, &self.processing.lock().unwrap());
        let Ok(unfinished) = self
            .store
            .meetings_with_status(&[MeetingStatus::Recording, MeetingStatus::Processing])
        else {
            return;
        };
        for m in unfinished {
            if m.end_ms.is_none() {
                let _ = self.store.set_end(&m.id, now_ms());
            }
            if self.store.process_attempts(&m.id).unwrap_or(0) >= 2 {
                log::warn!(
                    "Notetaker: processing of {} was cut off twice, marking it failed",
                    m.id
                );
                let _ = self.store.clear_process_attempts(&m.id);
                self.fail(
                    &m.id,
                    "Processing this meeting was interrupted twice. Use Regenerate to try again.",
                );
                continue;
            }
            log::info!("Notetaker finishing interrupted meeting {}", m.id);
            let _ = self.store.set_status(&m.id, MeetingStatus::Processing);
            let _ = self.process_in_background(m.id, true);
        }
    }

    /// Hinglish redos cut off by a quit start again.
    pub fn resume_hinglish_redos(self: &Arc<Self>) {
        for id in redos_left(&self.dir) {
            log::info!("Notetaker resuming the Hinglish redo of {id}");
            self.improve_with_hinglish(id);
        }
    }

    pub fn list(&self, filter: &NotetakerFilter) -> Result<Vec<Meeting>> {
        let mut meetings = self.store.list(filter)?;
        meetings.iter_mut().for_each(|m| self.add_disk_state(m));
        Ok(meetings)
    }

    pub fn get(&self, id: &str) -> Result<Option<MeetingDetail>> {
        let mut detail = self.store.detail(id)?;
        if let Some(d) = &mut detail {
            self.add_disk_state(&mut d.meeting);
        }
        Ok(detail)
    }

    fn add_disk_state(&self, meeting: &mut Meeting) {
        let dir = self.meeting_dir(&meeting.id);
        let improving = self.improving.lock().unwrap().contains(&meeting.id);
        meeting.hinglish_redo = redo_kind(&dir, improving);
        meeting.hinglish_error = (!improving)
            .then(|| std::fs::read_to_string(dir.join(HINGLISH_ERROR)).ok())
            .flatten();
        meeting.has_audio = has_audio(&dir);
    }

    pub fn save_thoughts(&self, id: &str, markdown: &str) -> Result<()> {
        if self.store.meeting(id)?.is_none() {
            return Err(anyhow!("No meeting with id {id}"));
        }
        self.store.save_thoughts(id, markdown)
    }

    pub fn rename(&self, id: &str, title: &str) -> Result<()> {
        let title = title.trim();
        if title.is_empty() {
            return Err(anyhow!("The title can't be empty"));
        }
        self.store.rename(id, title)
    }

    /// The user's own names and emails: never a name for someone on the call.
    fn own_names(&self, meeting: &Meeting) -> Vec<String> {
        let mut own = own_names_in(meeting);
        own.extend(crate::textflow::dictionary_emails(&self.app));
        own
    }

    /// Names one speaker on every line of a meeting. A voice-split speaker's
    /// voice then counts toward that name, so later meetings name it too.
    pub fn rename_speaker(&self, id: &str, speaker_key: &str, name: &str) -> Result<()> {
        let name = name.trim();
        if name.is_empty() {
            return Err(anyhow!("The name can't be empty"));
        }
        let improving = self.improving.lock().unwrap();
        let redoing = improving.iter().any(|m| m == id);
        if !redoing && self.processing.lock().unwrap().iter().any(|m| m == id) {
            return Err(anyhow!("Speakers can be named once the notes are written"));
        }
        self.store.rename_speaker(id, speaker_key, name)?;
        if let Err(e) =
            self.store
                .set_voice_named(id, speaker_key, !diarize::is_default_label(name))
        {
            log::warn!("Notetaker could not remember a voice: {e}");
        }
        drop(improving);
        let _ = NotetakerTranscript {
            meeting_id: id.to_string(),
            segments: self.store.segments(id)?,
            replace: true,
        }
        .emit(&self.app);
        Ok(())
    }

    /// How many people have a saved voice.
    pub fn saved_voices(&self) -> Result<u32> {
        Ok(self.store.voice_profiles(None)?.len() as u32)
    }

    /// Forgets every saved voice and the user's own, and stops learning the
    /// user's voice until `learn_own_voice` is turned on again.
    pub fn forget_voices(&self) -> Result<()> {
        self.store.forget_voices()?;
        let mut all = crate::settings::get_settings(&self.app);
        all.notetaker.learn_own_voice = false;
        crate::settings::write_settings(&self.app, all);
        Ok(())
    }

    /// Drops the user's voice when learning it is turned off.
    pub fn settings_changed(&self, settings: &NotetakerSettings) -> Result<()> {
        if !settings.learn_own_voice {
            self.store.forget_own_voice()?;
        }
        Ok(())
    }

    pub fn move_to_folder(&self, id: &str, folder: Option<&str>) -> Result<()> {
        self.store
            .move_to_folder(id, folder.map(str::trim).filter(|f| !f.is_empty()))
    }

    pub fn folders(&self) -> Result<Vec<FolderInfo>> {
        self.store.folders()
    }

    pub fn delete(self: &Arc<Self>, id: &str) -> Result<()> {
        let _ = self.stop_inner(Some(id), false);
        let redoing = {
            let improving = self.improving.lock().unwrap();
            let redoing = improving.iter().any(|m| m == id);
            if redoing {
                self.cancelled.lock().unwrap().push(id.to_string());
            }
            redoing
        };
        if redoing {
            if self.leave_download(id) {
                if let Some(models) = self
                    .app
                    .try_state::<Arc<crate::managers::model::ModelManager>>()
                {
                    if let Ok(model_id) = hinglish::download_id() {
                        if models
                            .get_model_info(&model_id)
                            .is_some_and(|m| m.is_downloading)
                        {
                            let _ = models.cancel_download(&model_id);
                        }
                    }
                }
            }
        } else if self.processing.lock().unwrap().iter().any(|m| m == id) {
            return Err(anyhow!("This meeting is still being processed"));
        }
        self.store.delete(id)?;
        let dir = self.meeting_dir(id);
        if dir.exists() {
            std::fs::remove_dir_all(dir)?;
        }
        Ok(())
    }

    pub fn ask(
        &self,
        question: &str,
        meeting_id: Option<&str>,
        thread_id: Option<&str>,
    ) -> Result<AskAnswer> {
        let question = question.trim();
        if question.is_empty() {
            return Err(anyhow!("Ask a question"));
        }
        let model = ai::Model::for_app(&self.app);
        if let Some(reason) = model.unavailable_reason() {
            return Err(anyhow!(reason));
        }

        let now = now_ms();
        let keywords = ai::keywords(question);
        let meetings = match meeting_id {
            Some(id) => vec![self
                .store
                .meeting(id)?
                .ok_or_else(|| anyhow!("No meeting with id {id}"))?],
            None => self.store.list(&NotetakerFilter {
                limit: Some(200),
                ..Default::default()
            })?,
        };
        let mut scored = Vec::new();
        for m in &meetings {
            let passage = |start_ms: i64, text: String| ai::Passage {
                meeting_id: m.id.clone(),
                meeting_title: m.title.clone(),
                meeting_start_ms: m.start_ms,
                start_ms,
                text,
            };
            let summary_boost = if meeting_id.is_some() { 100.0 } else { 1.0 };
            if let Some(summary) = self.store.summary(&m.id)? {
                let mut text = summary.overview.clone();
                for d in &summary.decisions {
                    text.push_str(&format!("\nDecision: {d}"));
                }
                for a in &summary.action_items {
                    text.push_str(&format!(
                        "\nAction: {}{}",
                        a.text,
                        a.owner
                            .as_ref()
                            .map(|o| format!(" ({o})"))
                            .unwrap_or_default()
                    ));
                }
                let score = ai::score_passage(&text, &keywords, m.start_ms, now) + summary_boost;
                scored.push((score, passage(0, text)));
            }
            let thoughts = self.store.thoughts(&m.id)?;
            if !thoughts.trim().is_empty() {
                let score =
                    ai::score_passage(&thoughts, &keywords, m.start_ms, now) + summary_boost;
                scored.push((score, passage(0, format!("My notes: {thoughts}"))));
            }
            for window in self.store.segments(&m.id)?.chunks(6) {
                let text = window
                    .iter()
                    .map(|s| format!("{}: {}", s.speaker, s.text))
                    .collect::<Vec<_>>()
                    .join("\n");
                let score = ai::score_passage(&text, &keywords, m.start_ms, now);
                scored.push((score, passage(window[0].start_ms, text)));
            }
        }
        let passages = ai::pick_passages(scored);

        let thread_id = thread_id
            .map(str::to_string)
            .unwrap_or_else(|| format!("t{}", new_meeting_id()));
        let history: Vec<(String, String)> = self
            .store
            .chat_messages(&thread_id)?
            .into_iter()
            .rev()
            .take(6)
            .rev()
            .map(|m| (m.role, m.content))
            .collect();
        let (text, citations) = ai::answer(&model, question, &history, &passages)?;

        let mut question_msg = ChatMessage {
            id: 0,
            thread_id: thread_id.clone(),
            meeting_id: meeting_id.map(str::to_string),
            role: "user".to_string(),
            content: question.to_string(),
            citations: Vec::new(),
            created_ms: now,
        };
        question_msg.id = self.store.add_chat_message(&question_msg)?;
        let mut answer_msg = ChatMessage {
            id: 0,
            thread_id: thread_id.clone(),
            meeting_id: meeting_id.map(str::to_string),
            role: "assistant".to_string(),
            content: text,
            citations,
            created_ms: now_ms(),
        };
        answer_msg.id = self.store.add_chat_message(&answer_msg)?;
        Ok(AskAnswer {
            thread_id,
            question: question_msg,
            answer: answer_msg,
        })
    }

    pub fn chat_threads(&self, meeting_id: Option<&str>) -> Result<Vec<ChatThread>> {
        self.store.chat_threads(meeting_id)
    }

    pub fn chat_messages(&self, thread_id: &str) -> Result<Vec<ChatMessage>> {
        self.store.chat_messages(thread_id)
    }
}

/// The mic recording with the call taken out, kept only while it is being
/// transcribed.
const CLEANED_MIC: &str = "mic-no-echo.wav";
/// In a meeting's folder while it waits to be redone in Hinglish; holds how
/// many times the redo was started.
const HINGLISH_REDO: &str = "hinglish-redo";
const MAX_HINGLISH_TRIES: u32 = 3;
/// Beside `HINGLISH_REDO` when the user asked for the redo: it runs even
/// when Auto would not (no sample first, model fetched even if declined).
const HINGLISH_ASKED: &str = "hinglish-asked";
/// In the notetaker folder once the user deleted a Hinglish speech model or
/// cancelled its download: Auto never downloads one again (choosing
/// Hinglish still does).
const HINGLISH_MODEL_DELETED: &str = "hinglish-model-deleted";
/// In a meeting's folder after a redo the user asked for failed: why, shown
/// on the meeting until it is dismissed or the redo starts again.
const HINGLISH_ERROR: &str = "hinglish-error";
/// Share of the progress bar for hearing the audio in a Hinglish pass; the
/// rest is writing it in Roman letters.
const HEARING_SHARE: f32 = 0.8;

/// Remembers that the user deleted a Hinglish speech model or cancelled its
/// download.
pub fn note_model_declined(app: &AppHandle, model_id: &str) {
    if !hinglish::is_model(model_id) {
        return;
    }
    if let Ok(dir) = crate::portable::app_data_dir(app) {
        let _ = std::fs::write(dir.join("notetaker").join(HINGLISH_MODEL_DELETED), model_id);
    }
}

/// The user cancelled the Hinglish model's download during a redo.
#[derive(Debug)]
struct DownloadCancelled;

impl std::fmt::Display for DownloadCancelled {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "the download of the Hinglish speech model was cancelled")
    }
}

impl std::error::Error for DownloadCancelled {}

fn end_redo_in(dir: &std::path::Path) {
    let _ = std::fs::remove_file(dir.join(HINGLISH_REDO));
    let _ = std::fs::remove_file(dir.join(HINGLISH_ASKED));
}

/// After a redo of the meeting in `dir` failed: a cancelled download ends
/// it, even one the user asked for (the cancel is remembered for Auto, and
/// asking again starts afresh); otherwise it starts again on the next launch
/// while the model may still be fetched for it.
fn after_failed_redo(dir: &std::path::Path, download_cancelled: bool, may_fetch: bool) {
    if download_cancelled || !may_fetch {
        end_redo_in(dir);
    }
}

fn has_audio(dir: &std::path::Path) -> bool {
    dir.join("mic.wav").exists() || dir.join("system.wav").exists()
}

fn redo_kind(dir: &std::path::Path, improving: bool) -> Option<HinglishRedo> {
    improving.then(|| {
        if dir.join(HINGLISH_ASKED).exists() {
            HinglishRedo::Asked
        } else {
            HinglishRedo::Auto
        }
    })
}

/// The voice split of the call, kept for a later Hinglish redo.
const CALL_TURNS: &str = "call-turns.json";
/// The voice split of the microphone, when it held people in the room.
const MIC_TURNS: &str = "mic-turns.json";
/// The user's voice from dictation is made again when older than this.
const OWN_VOICE_MAX_AGE_MS: i64 = 7 * 24 * 3600 * 1000;

fn save_call_turns(dir: &std::path::Path, turns: Option<&[diarize::Turn]>) {
    save_turns(&dir.join(CALL_TURNS), turns);
}

fn save_turns(path: &std::path::Path, turns: Option<&[diarize::Turn]>) {
    let saved = match turns {
        Some(turns) => serde_json::to_vec(turns)
            .map_err(anyhow::Error::from)
            .and_then(|json| Ok(std::fs::write(path, json)?)),
        None => {
            let _ = std::fs::remove_file(path);
            Ok(())
        }
    };
    if let Err(e) = saved {
        log::warn!("Notetaker could not keep the voice split: {e}");
    }
}

fn load_call_turns(dir: &std::path::Path) -> Option<Vec<diarize::Turn>> {
    load_turns(&dir.join(CALL_TURNS))
}

fn load_turns(path: &std::path::Path) -> Option<Vec<diarize::Turn>> {
    serde_json::from_slice(&std::fs::read(path).ok()?).ok()
}

/// From a moment in mic.wav to the same moment in system.wav.
fn mic_to_call_ms(meeting: &Meeting) -> i64 {
    meeting.mic_offset_ms.unwrap_or(0) - meeting.system_offset_ms.unwrap_or(0)
}

/// Speech on the call: the voice split's, else that of the live lines.
fn call_speech_ms(call: Option<&diarize::Speakers>, live: &[Segment]) -> i64 {
    match call {
        Some(c) => c.speech_ms.iter().sum(),
        None => live
            .iter()
            .filter(|s| s.source == "system")
            .map(|s| (s.end_ms - s.start_ms).max(0))
            .sum(),
    }
}

/// Dictation recordings in `dir`, newest first.
fn dictation_clips(dir: &std::path::Path) -> Vec<PathBuf> {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut clips: Vec<(std::time::SystemTime, PathBuf)> = entries
        .flatten()
        .filter(|e| e.path().extension().is_some_and(|x| x == "wav"))
        .filter_map(|e| Some((e.metadata().ok()?.modified().ok()?, e.path())))
        .collect();
    clips.sort_by(|a, b| b.0.cmp(&a.0));
    clips.into_iter().map(|(_, p)| p).collect()
}

/// The mic audio to transcribe and how it relates to the call.
struct MicAudio {
    path: PathBuf,
    /// Holds none of the call: it was taken out, the mic never heard it, or
    /// there was no call audio.
    clean: bool,
    /// From the call audio to its sound in the mic, when the mic heard it.
    echo_delay_ms: Option<f32>,
}

/// The streams whose stretches without lines a Hinglish redo hears: the
/// call, and the mic only when the call could be kept out of it.
fn sources_to_fill(mic: &MicAudio, mic_exists: bool, system_exists: bool) -> Vec<&'static str> {
    let mut sources = Vec::new();
    if mic_exists && mic.clean {
        sources.push("mic");
    }
    if system_exists {
        sources.push("system");
    }
    sources
}

/// Takes `id` off `waiters`; true when it waited on a download a redo
/// started (`ours`) and nobody waits any more.
fn leave(waiters: &mut Vec<String>, id: &str, ours: bool) -> bool {
    let was_waiting = waiters.iter().any(|m| m == id);
    waiters.retain(|m| m != id);
    was_waiting && waiters.is_empty() && ours
}

/// Meetings whose Hinglish redo has not finished.
fn redos_left(root: &std::path::Path) -> Vec<String> {
    let Ok(entries) = std::fs::read_dir(root) else {
        return Vec::new();
    };
    let mut ids: Vec<String> = entries
        .flatten()
        .filter(|e| e.path().join(HINGLISH_REDO).is_file())
        .filter_map(|e| e.file_name().into_string().ok())
        .collect();
    ids.sort();
    ids
}

/// Every line takes the label its speaker has in `current`, so names the
/// user gave stay.
fn keep_labels(lines: &mut [Segment], current: &[Segment]) {
    let mut labels: std::collections::HashMap<&str, &str> = Default::default();
    for s in current {
        labels
            .entry(s.speaker_key.as_str())
            .or_insert(s.speaker.as_str());
    }
    for line in lines {
        if let Some(label) = labels.get(line.speaker_key.as_str()) {
            line.speaker = label.to_string();
        }
    }
}

/// Deletes cleaned mic copies left by a crash, except in meetings `busy` being
/// processed.
fn remove_cleaned_mics(root: &std::path::Path, busy: &[String]) {
    let Ok(entries) = std::fs::read_dir(root) else {
        return;
    };
    for entry in entries.flatten() {
        let name = entry.file_name();
        if busy.iter().any(|id| name.to_str() == Some(id)) {
            continue;
        }
        let _ = std::fs::remove_file(entry.path().join(CLEANED_MIC));
    }
}

/// mic.wav without the call it heard from the speakers, written to `cleaned`;
/// mic.wav itself when it heard none, or (not clean) when that fails.
fn mic_for_transcript(
    dir: &std::path::Path,
    meeting: &Meeting,
    cleaned: &std::path::Path,
) -> MicAudio {
    let (mic, system) = (dir.join("mic.wav"), dir.join("system.wav"));
    let as_is = |clean: bool| MicAudio {
        path: mic.clone(),
        clean,
        echo_delay_ms: None,
    };
    if !mic.exists() || !system.exists() {
        return as_is(true);
    }
    let mic_starts_later =
        meeting.mic_offset_ms.unwrap_or(0) - meeting.system_offset_ms.unwrap_or(0);
    match echo::remove_call_echo(&mic, &system, mic_starts_later, cleaned) {
        Ok(Some(r)) => {
            log::info!(
                "Notetaker took the call out of the mic: {:.0} ms delay, heard in {} windows",
                r.delay_ms,
                r.windows
            );
            MicAudio {
                path: cleaned.to_path_buf(),
                clean: true,
                echo_delay_ms: Some(r.delay_ms),
            }
        }
        Ok(None) => as_is(true),
        Err(e) => {
            log::warn!("Notetaker could not take the call out of the mic: {e}");
            let _ = std::fs::remove_file(cleaned);
            as_is(false)
        }
    }
}

/// Processing runs behind dictation: macOS gives this thread (and threads it
/// starts) a lower priority.
fn background_priority() {
    #[cfg(target_os = "macos")]
    // SAFETY: sets the calling thread's own QoS class; no pointers involved.
    unsafe {
        libc::pthread_set_qos_class_self_np(libc::qos_class_t::QOS_CLASS_UTILITY, 0);
    }
}

/// Names the huddle window gives the voices of the call (`turns`) and of the
/// room (`room`), both in their WAV's time.
fn window_namings(
    dir: &std::path::Path,
    meeting: &Meeting,
    turns: &[diarize::Turn],
    room: &[diarize::Turn],
    segments: &[Segment],
    own: &[String],
) -> Vec<diarize::Naming> {
    let samples = window::load(dir);
    if samples.is_empty() {
        return Vec::new();
    }
    let in_meeting = |turns: &[diarize::Turn], offset: i64| -> Vec<(i64, i64, String)> {
        turns
            .iter()
            .filter(|t| t.speaker != diarize::UNKNOWN && t.speaker != diarize::ME)
            .map(|t| {
                (
                    t.start_ms + offset,
                    t.end_ms + offset,
                    diarize::speaker_key(t.speaker),
                )
            })
            .collect()
    };
    let mut turns = in_meeting(turns, meeting.system_offset_ms.unwrap_or(0));
    turns.extend(in_meeting(room, meeting.mic_offset_ms.unwrap_or(0)));
    let mic: Vec<(i64, i64)> = segments
        .iter()
        .filter(|s| s.source == "mic" && s.speaker_key == diarize::ME_KEY)
        .map(|s| (s.start_ms, s.end_ms))
        .collect();
    diarize::window_names(&samples, &turns, &mic, own)
}

/// The user's names and emails from the invite and the Mac account.
fn own_names_in(meeting: &Meeting) -> Vec<String> {
    let mut own: Vec<String> = meeting
        .attendees
        .iter()
        .filter(|a| a.is_me)
        .flat_map(|a| [a.name.clone(), a.email.clone()])
        .flatten()
        .collect();
    own.extend(user_full_name());
    own
}

/// The macOS account's full name ("Gourav Sharma"), read once.
fn user_full_name() -> Option<String> {
    static NAME: std::sync::OnceLock<Option<String>> = std::sync::OnceLock::new();
    NAME.get_or_init(|| {
        if !cfg!(target_os = "macos") {
            return None;
        }
        let out = std::process::Command::new("/usr/bin/id")
            .arg("-F")
            .output()
            .ok()?;
        let name = String::from_utf8(out.stdout).ok()?.trim().to_string();
        (out.status.success() && !name.is_empty()).then_some(name)
    })
    .clone()
}

#[cfg(target_os = "macos")]
fn tap_name() -> String {
    format!("{} Notetaker", crate::brand::APP_NAME)
}

#[cfg(target_os = "macos")]
fn start_system_audio(dir: &std::path::Path, feed: SharedFeed) -> Result<Box<dyn FnOnce() + Send>> {
    let first = coreaudio::SystemTap::start(&tap_name())?;
    let wav = create_wav(&dir.join("system.wav"))?;
    let stop = Arc::new(AtomicBool::new(false));
    let worker = {
        let stop = stop.clone();
        std::thread::Builder::new()
            .name("notetaker-system".into())
            .spawn(move || system_worker(first, StreamWriter::new(wav, feed), &stop))?
    };
    Ok(Box::new(move || {
        stop.store(true, Ordering::Relaxed);
        let _ = worker.join();
    }))
}

/// Waits before building the system audio tap again while it delivers
/// nothing on the same output device.
#[cfg(any(target_os = "macos", test))]
const TAP_RETRY_WAITS: [Duration; 5] = [
    Duration::from_secs(1),
    Duration::from_secs(2),
    Duration::from_secs(5),
    Duration::from_secs(15),
    Duration::from_secs(60),
];

/// While another app plays sound, a quiet tap is built again at least this
/// often.
#[cfg(any(target_os = "macos", test))]
const TAP_RETRY_RUNNING: Duration = Duration::from_secs(3);

/// When to build the system audio tap again. Audio that stops for 2 s is
/// retried after `TAP_RETRY_WAITS`, or within `TAP_RETRY_RUNNING` while
/// another app plays sound (so the tap should hear it). A new tap delivers
/// nothing until something plays, so before its first audio it is rebuilt
/// only while another app plays. A new default output device is built at
/// once, which is not a try.
#[cfg(any(target_os = "macos", test))]
struct TapRetry {
    last_audio: Instant,
    quiet: bool,
    tries: usize,
    since: Instant,
    now_please: bool,
    heard_once: bool,
}

#[cfg(any(target_os = "macos", test))]
impl TapRetry {
    const STALL: Duration = Duration::from_secs(2);

    fn new(now: Instant) -> Self {
        Self {
            last_audio: now,
            quiet: false,
            tries: 0,
            since: now,
            now_please: false,
            heard_once: false,
        }
    }

    /// Audio arrived; true when it ends a quiet spell.
    fn heard(&mut self, now: Instant) -> bool {
        self.last_audio = now;
        self.tries = 0;
        self.heard_once = true;
        std::mem::replace(&mut self.quiet, false)
    }

    /// True once, when the audio has just stopped.
    fn went_quiet(&mut self, now: Instant) -> bool {
        if self.quiet || now.duration_since(self.last_audio) < Self::STALL {
            return false;
        }
        self.quiet = true;
        self.tries = 0;
        self.since = now;
        true
    }

    fn device_changed(&mut self, now: Instant) {
        self.last_audio = now;
        self.quiet = false;
        self.tries = 0;
        self.now_please = true;
    }

    /// Whether to build the tap now: 0 for a new output device, else which
    /// try of this quiet spell (1 = the first). `playing`: another app plays
    /// sound.
    fn due(&mut self, now: Instant, playing: bool) -> Option<usize> {
        if std::mem::take(&mut self.now_please) {
            self.since = now;
            return Some(0);
        }
        if !self.quiet || (!self.heard_once && !playing) {
            return None;
        }
        let mut wait = TAP_RETRY_WAITS[self.tries.min(TAP_RETRY_WAITS.len() - 1)];
        if playing {
            wait = wait.min(TAP_RETRY_RUNNING);
        }
        if now < self.since + wait {
            return None;
        }
        self.tries += 1;
        self.since = now;
        Some(self.tries)
    }
}

/// Moves system audio from the tap to the WAV, building the tap again as
/// `TapRetry` says; the writer fills gaps with silence.
#[cfg(target_os = "macos")]
fn system_worker(
    first: (coreaudio::SystemTap, std::sync::mpsc::Receiver<Vec<f32>>),
    mut writer: StreamWriter,
    stop: &AtomicBool,
) {
    use std::sync::mpsc::RecvTimeoutError;
    const DEVICE_CHECK: Duration = Duration::from_secs(2);
    const POLL: Duration = Duration::from_millis(250);

    let mut resampler = audio::Resampler::new(first.0.sample_rate);
    let mut device = Some(first.0.output_device);
    let mut current = Some(first);
    let mut out = Vec::new();
    let mut retry = TapRetry::new(Instant::now());
    let mut last_device_check = Instant::now();
    let mut heard_any = false;

    while !stop.load(Ordering::Relaxed) {
        match current.as_ref().map(|(_, rx)| rx.recv_timeout(POLL)) {
            Some(Ok(block)) => {
                if retry.heard(Instant::now()) && heard_any {
                    log::info!("Notetaker system audio is back");
                    writer.set_failed(None);
                }
                heard_any = true;
                out.clear();
                resampler.process(&block, &mut out);
                writer.write(&out);
            }
            Some(Err(RecvTimeoutError::Timeout)) => {}
            Some(Err(RecvTimeoutError::Disconnected)) | None => std::thread::sleep(POLL),
        }
        if last_device_check.elapsed() >= DEVICE_CHECK {
            last_device_check = Instant::now();
            let now_device = coreaudio::default_output_device();
            if now_device != device {
                device = now_device;
                log::info!("Notetaker: output device changed, rebuilding the system audio tap");
                retry.device_changed(Instant::now());
            }
        }
        let now = Instant::now();
        retry.went_quiet(now);
        let playing = retry.quiet && coreaudio::others_playing();
        let Some(attempt) = retry.due(now, playing) else {
            continue;
        };
        match attempt {
            0 => {}
            1 if heard_any => {
                log::warn!("Notetaker system audio stopped; rebuilding the tap");
                writer.set_failed(Some("System audio stopped. Reconnecting…".to_string()));
            }
            n => log::debug!("Notetaker rebuilding the quiet system audio tap (try {n})"),
        }
        current = None;
        match coreaudio::SystemTap::start(&tap_name()) {
            Ok(next) => {
                resampler = audio::Resampler::new(next.0.sample_rate);
                device = Some(next.0.output_device);
                current = Some(next);
            }
            Err(e) => log::debug!("Notetaker system audio not back yet: {e}"),
        }
    }
    drop(current);
    writer.finish();
}

#[cfg(not(target_os = "macos"))]
fn start_system_audio(
    _dir: &std::path::Path,
    _feed: SharedFeed,
) -> Result<Box<dyn FnOnce() + Send>> {
    Err(anyhow!("System audio capture is only available on macOS"))
}

/// Creates the manager, finishes interrupted meetings and starts call
/// detection. Called once from app setup.
pub fn init(app: &AppHandle) {
    match NotetakerManager::new(app) {
        Ok(manager) => {
            app.manage(manager.clone());
            manager.resume_unfinished();
            manager.resume_hinglish_redos();
            detect::spawn(manager);
        }
        Err(e) => log::error!("Notetaker unavailable: {e}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn redone_lines_keep_the_names_the_user_gave() {
        let line = |key: &str, speaker: &str, text: &str| Segment {
            source: "system".into(),
            speaker: speaker.into(),
            speaker_key: key.into(),
            start_ms: 0,
            end_ms: 1_000,
            text: text.into(),
        };
        let current = vec![
            line("s1", "Priya Nair", "old"),
            line("s2", "Speaker 2", "old"),
        ];
        let mut redone = vec![
            line("s1", "Speaker 1", "naya"),
            line("s2", "Speaker 2", "naya"),
            line("s3", "Speaker 3", "naya"),
        ];
        keep_labels(&mut redone, &current);
        let labels: Vec<&str> = redone.iter().map(|s| s.speaker.as_str()).collect();
        assert_eq!(labels, vec!["Priya Nair", "Speaker 2", "Speaker 3"]);
    }

    #[test]
    fn a_deleted_meeting_cancels_only_a_download_nobody_else_needs() {
        let mut waiters = vec!["a".to_string(), "b".to_string()];
        assert!(!leave(&mut waiters, "a", true), "b still waits");
        assert!(leave(&mut waiters, "b", true));
        let mut settings_download = vec!["c".to_string()];
        assert!(
            !leave(&mut settings_download, "c", false),
            "started from Settings"
        );
        assert!(!leave(&mut Vec::new(), "d", true), "never waited");
    }

    #[test]
    fn unfinished_hinglish_redos_are_found() {
        let root = tempfile::tempdir().unwrap();
        for id in ["20261001-090000-aaaa", "20261002-090000-bbbb"] {
            std::fs::create_dir(root.path().join(id)).unwrap();
        }
        std::fs::write(
            root.path().join("20261002-090000-bbbb").join(HINGLISH_REDO),
            "1",
        )
        .unwrap();
        assert_eq!(redos_left(root.path()), vec!["20261002-090000-bbbb"]);
        assert!(redos_left(&root.path().join("missing")).is_empty());
    }

    #[test]
    fn a_cancelled_download_ends_a_redo_the_user_asked_for() {
        let root = tempfile::tempdir().unwrap();
        for id in ["20261003-090000-cccc", "20261004-090000-dddd"] {
            let dir = root.path().join(id);
            std::fs::create_dir(&dir).unwrap();
            std::fs::write(dir.join(HINGLISH_REDO), "1").unwrap();
            std::fs::write(dir.join(HINGLISH_ASKED), "").unwrap();
        }
        let cancelled = root.path().join("20261003-090000-cccc");
        after_failed_redo(&cancelled, true, true);
        assert!(!cancelled.join(HINGLISH_ASKED).exists());
        let offline = root.path().join("20261004-090000-dddd");
        after_failed_redo(&offline, false, true);
        assert_eq!(redos_left(root.path()), vec!["20261004-090000-dddd"]);
        after_failed_redo(&offline, false, false);
        assert!(redos_left(root.path()).is_empty());
    }

    #[test]
    fn mic_stretches_are_filled_only_from_a_mic_without_the_call() {
        let clean = MicAudio {
            path: PathBuf::from("mic-no-echo.wav"),
            clean: true,
            echo_delay_ms: Some(120.0),
        };
        let raw = MicAudio {
            path: PathBuf::from("mic.wav"),
            clean: false,
            echo_delay_ms: None,
        };
        assert_eq!(sources_to_fill(&clean, true, true), vec!["mic", "system"]);
        assert_eq!(sources_to_fill(&raw, true, true), vec!["system"]);
        assert_eq!(sources_to_fill(&raw, true, false), Vec::<&str>::new());
        assert_eq!(sources_to_fill(&clean, false, true), vec!["system"]);
    }

    #[test]
    fn the_menu_knows_a_redo_the_user_asked_for() {
        let root = tempfile::tempdir().unwrap();
        let dir = root.path().join("20261005-090000-eeee");
        std::fs::create_dir(&dir).unwrap();
        assert!(!has_audio(&dir));
        assert_eq!(redo_kind(&dir, false), None);
        assert_eq!(redo_kind(&dir, true), Some(HinglishRedo::Auto));
        std::fs::write(dir.join("system.wav"), b"x").unwrap();
        std::fs::write(dir.join(HINGLISH_ASKED), "").unwrap();
        assert!(has_audio(&dir));
        assert_eq!(redo_kind(&dir, true), Some(HinglishRedo::Asked));
    }

    #[test]
    fn removes_leftover_cleaned_mics_except_while_processing() {
        let root = tempfile::tempdir().unwrap();
        for id in ["20261001-090000-aaaa", "20261002-090000-bbbb"] {
            std::fs::create_dir(root.path().join(id)).unwrap();
            std::fs::write(root.path().join(id).join(CLEANED_MIC), b"x").unwrap();
            std::fs::write(root.path().join(id).join("mic.wav"), b"x").unwrap();
        }
        remove_cleaned_mics(root.path(), &["20261002-090000-bbbb".to_string()]);
        let a = root.path().join("20261001-090000-aaaa");
        let b = root.path().join("20261002-090000-bbbb");
        assert!(!a.join(CLEANED_MIC).exists());
        assert!(a.join("mic.wav").exists());
        assert!(b.join(CLEANED_MIC).exists());
    }

    #[test]
    fn idle_streams_show_a_start_in_progress() {
        assert_eq!(idle_stream(false, true).state, StreamState::Off);
        assert!(idle_stream(false, true).detail.is_none());
        assert_eq!(idle_stream(true, false).state, StreamState::Starting);
        assert!(idle_stream(true, false).detail.is_none());
        assert_eq!(idle_stream(true, true).state, StreamState::Starting);
    }

    #[test]
    fn starting_flag_clears_when_a_start_panics() {
        let gate = StartGate::default();
        assert!(gate.begin("m1"));
        let _ = std::panic::catch_unwind(|| {
            let _clear = ClearOnDrop(&gate);
            panic!("start failed");
        });
        assert!(!gate.waiting());
        assert!(gate.begin("m2"));
    }

    #[test]
    fn only_one_start_at_a_time() {
        let gate = StartGate::default();
        assert!(gate.begin("m1"));
        assert!(!gate.begin("m2"));
        gate.finish();
        assert!(gate.begin("m2"));
    }

    #[test]
    fn stop_cancels_a_waiting_start() {
        let gate = StartGate::default();
        assert_eq!(gate.cancel(None), None);
        assert!(gate.begin("m1"));
        assert!(gate.waiting());
        assert!(!gate.cancelled());
        assert_eq!(gate.cancel(None).as_deref(), Some("m1"));
        assert!(gate.cancelled());
        assert!(!gate.waiting());
        assert!(!gate.begin("m2"));
    }

    #[test]
    fn delete_cancels_only_the_start_of_its_meeting() {
        let gate = StartGate::default();
        assert!(gate.begin("m1"));
        assert_eq!(gate.cancel(Some("m0")), None);
        assert!(!gate.cancelled());
        assert_eq!(gate.cancel(Some("m1")).as_deref(), Some("m1"));
        assert!(gate.cancelled());
    }

    #[test]
    fn a_cancel_does_not_carry_over_to_the_next_start() {
        let gate = StartGate::default();
        assert!(gate.begin("m1"));
        gate.cancel(None);
        {
            let _clear = ClearOnDrop(&gate);
        }
        assert_eq!(gate.cancel(None), None);
        assert!(gate.begin("m2"));
        assert!(!gate.cancelled());
        assert!(gate.waiting());
    }

    /// Runs the real notes and speaker naming on one stored meeting, by hand.
    /// Nothing is written to the database; the result goes to a local JSON
    /// file and only counts are printed.
    /// LIPIK_TEST_DB=<copy of notetaker.db> LIPIK_TEST_MEETING=<id>
    /// LIPIK_TEST_OUT=<out.json> cargo test --lib naming_check -- --ignored
    /// --nocapture. Optional: LIPIK_TEST_OLLAMA_URL, LIPIK_TEST_OLLAMA_MODEL,
    /// LIPIK_TEST_KEEP_LABELS=1 to start from the stored labels instead of
    /// "Speaker N" plus voice and calendar naming. LIPIK_TEST_QWEN=<Qwen3-ASR
    /// gguf> with LIPIK_TEST_AUDIO=<folder with copies of mic.wav and
    /// system.wav> and LIPIK_TRANSCRIBE_WORKER_EXE=<built lipik> first redoes
    /// the transcript in Hinglish the way a redo does (the result is also
    /// written to <out>.segments.json). LIPIK_TEST_SEGMENTS=<such a file>
    /// uses a transcript redone before. LIPIK_TEST_WINDOW=<folder with
    /// copies of window-speakers.jsonl and call-turns.json> adds the names
    /// from the huddle window.
    #[test]
    #[ignore]
    fn naming_check() {
        use serde_json::{json, Value};
        use std::collections::HashMap;

        let var = |k: &str| std::env::var(k).unwrap_or_else(|_| panic!("{k} is not set"));
        let id = var("LIPIK_TEST_MEETING");
        let store = Store::open_read_only(std::path::Path::new(&var("LIPIK_TEST_DB"))).unwrap();
        let meeting = store.meeting(&id).unwrap().expect("no such meeting");
        let invited = diarize::invitees(&meeting.attendees);
        let own = own_names_in(&meeting);
        let ollama = || {
            ai::Ollama::new(
                &std::env::var("LIPIK_TEST_OLLAMA_URL")
                    .unwrap_or_else(|_| "http://127.0.0.1:11434".to_string()),
                &std::env::var("LIPIK_TEST_OLLAMA_MODEL")
                    .unwrap_or_else(|_| "qwen3:8b".to_string()),
            )
        };
        let saved = std::env::var("LIPIK_TEST_SEGMENTS").ok();
        let stored = match std::env::var("LIPIK_TEST_QWEN") {
            _ if saved.is_some() => {
                let json = std::fs::read_to_string(saved.as_deref().unwrap_or_default()).unwrap();
                serde_json::from_str(&json).unwrap()
            }
            Ok(model) => {
                let engine = crate::engine_supervisor::EngineSupervisor::new(true);
                engine
                    .load(crate::engine_supervisor::LoadSpec {
                        path: model.into(),
                        backend: transcribe_cpp::Backend::Auto,
                        device: crate::engine_supervisor::DeviceSelector::Auto,
                    })
                    .unwrap();
                let run = transcribe_cpp::RunOptions {
                    language: Some(hinglish::LANGUAGE.to_string()),
                    ..Default::default()
                };
                let dir = PathBuf::from(var("LIPIK_TEST_AUDIO"));
                let cleaned = dir.join(CLEANED_MIC);
                let _ = std::fs::remove_file(&cleaned);
                let mic = mic_for_transcript(&dir, &meeting, &cleaned);
                println!("NAMING mic audio: {}", mic.path.display());
                let system = dir.join("system.wav");
                let streams = [
                    (
                        "mic",
                        mic.path.as_path(),
                        meeting.mic_offset_ms.unwrap_or(0),
                    ),
                    (
                        "system",
                        system.as_path(),
                        meeting.system_offset_ms.unwrap_or(0),
                    ),
                ];
                let lines = store.segments(&id).unwrap();
                let mut redone = transcribe::redo_lines(
                    &lines,
                    &streams,
                    hinglish::RULES,
                    &[],
                    || None,
                    |audio| {
                        engine
                            .transcribe(audio, run.clone())
                            .map(|t| t.text.trim().to_string())
                            .map_err(|e| anyhow!("{e}"))
                    },
                    |_| false,
                    |_| true,
                );
                drop(engine);
                let names: Vec<String> = invited.iter().chain(&own).cloned().collect();
                hinglish::romanize(
                    Some(&ai::Model::Ollama(ollama())),
                    &mut redone,
                    &names,
                    std::time::Duration::from_secs(600),
                    |_| true,
                );
                keep_labels(&mut redone, &lines);
                std::fs::write(
                    format!("{}.segments.json", var("LIPIK_TEST_OUT")),
                    serde_json::to_string_pretty(&redone).unwrap(),
                )
                .unwrap();
                redone
            }
            Err(_) => store.segments(&id).unwrap(),
        };

        let mut keys: Vec<String> = stored
            .iter()
            .filter(|s| s.speaker_key.starts_with('s'))
            .map(|s| s.speaker_key.clone())
            .collect();
        keys.sort_by_key(|k| k[1..].parse::<usize>().unwrap_or(usize::MAX));
        keys.dedup();
        let label_of = |segments: &[Segment], key: &str| {
            segments
                .iter()
                .find(|s| s.speaker_key == key)
                .map_or_else(|| diarize::default_label(key), |s| s.speaker.clone())
        };

        let mut segments = stored.clone();
        let mut rules: HashMap<String, &str> = HashMap::new();
        if std::env::var("LIPIK_TEST_KEEP_LABELS").is_ok() {
            for k in &keys {
                if !diarize::is_default_label(&label_of(&segments, k)) {
                    rules.insert(k.clone(), "stored");
                }
            }
        } else {
            for s in segments
                .iter_mut()
                .filter(|s| s.speaker_key.starts_with('s'))
            {
                s.speaker = diarize::default_label(&s.speaker_key);
            }
            let count = keys
                .iter()
                .filter_map(|k| k[1..].parse::<usize>().ok())
                .max()
                .unwrap_or(0);
            let speakers = diarize::Speakers {
                turns: Vec::new(),
                voices: (1..=count)
                    .map(|n| store.meeting_voice(&id, &format!("s{n}")).unwrap())
                    .collect(),
                speech_ms: vec![0; count],
            };
            let profiles = store.voice_profiles(Some(&id)).unwrap();
            let mut names = diarize::known_names(&speakers, &profiles, &meeting.attendees);
            let from_voice = |name: &str| profiles.iter().any(|p| p.name == name);
            let window = match std::env::var("LIPIK_TEST_WINDOW") {
                Ok(dir) => {
                    let dir = PathBuf::from(dir);
                    let turns = load_call_turns(&dir).expect("no call-turns.json");
                    window_namings(&dir, &meeting, &turns, &[], &segments, &own)
                }
                Err(_) => Vec::new(),
            };
            let kept = diarize::add_window_names(&mut names, &from_voice, &window);
            println!(
                "NAMING window named {} voices; saved voices kept over the window: {}",
                window.len(),
                kept.len()
            );
            for (n, name) in names {
                let key = diarize::speaker_key(n);
                let rule = if from_voice(&name) {
                    "voice"
                } else if window.iter().any(|w| w.key == key && w.name == name) {
                    "window"
                } else {
                    "calendar_1to1"
                };
                rules.insert(key.clone(), rule);
                diarize::apply_name(&mut segments, &key, &name);
            }
        }

        let model = ai::Model::Ollama(ollama());
        if let Some(reason) = model.unavailable_reason() {
            panic!("notes model unavailable: {reason}");
        }
        let thoughts = store.thoughts(&id).unwrap();
        let started = Instant::now();
        let (_, guesses) = ai::summarize(&model, &meeting, &thoughts, &segments).unwrap();
        let took = started.elapsed().as_secs_f32();
        let (named, verdicts) = diarize::judge_guesses(&segments, &guesses, &own, &invited);
        for n in &named {
            rules.insert(
                n.key.clone(),
                match n.rule {
                    diarize::NameRule::Introduction => "introduction",
                    diarize::NameRule::Addressed => "addressed",
                    diarize::NameRule::HandedOver => "handed_over",
                    diarize::NameRule::Window => "window",
                },
            );
            diarize::apply_name(&mut segments, &n.key, &n.name);
        }

        let line =
            |i: usize| json!({ "index": i, "text": segments.get(i).map(|s| s.text.as_str()) });
        let speakers: Vec<Value> = keys
            .iter()
            .map(|k| {
                let lines: Vec<&Segment> = segments.iter().filter(|s| &s.speaker_key == k).collect();
                let label = label_of(&segments, k);
                let speech_ms: i64 = lines.iter().map(|s| (s.end_ms - s.start_ms).max(0)).sum();
                let evidence: Vec<Value> = named
                    .iter()
                    .filter(|n| &n.key == k)
                    .flat_map(|n| n.lines.iter().map(|&i| line(i)))
                    .collect();
                let for_key = |v: &&diarize::GuessVerdict| {
                    v.key.as_deref() == Some(k.as_str())
                        || v.speaker.trim() == diarize::default_label(k)
                };
                let guesses: Vec<Value> = verdicts
                    .iter()
                    .zip(&guesses)
                    .filter(|(v, _)| for_key(v))
                    .map(|(v, g)| {
                        json!({
                            "guessed": v.guessed,
                            "as": v.name,
                            "model_lines": g.lines.iter().map(|&i| line(i)).collect::<Vec<_>>(),
                            "evidence_lines": v.lines,
                            "rejected": v.rejected,
                        })
                    })
                    .collect();
                json!({
                    "key": k,
                    "stored_label": label_of(&stored, k),
                    "label": label,
                    "speech_s": speech_ms as f64 / 1000.0,
                    "name": (!diarize::is_default_label(&label)).then_some(&label),
                    "rule": rules.get(k),
                    "evidence": evidence,
                    "guesses": guesses,
                    "first_segments_ms": lines.iter().take(3).map(|s| [s.start_ms, s.end_ms]).collect::<Vec<_>>(),
                })
            })
            .collect();
        let other_guesses: Vec<Value> = verdicts
            .iter()
            .filter(|v| {
                v.key.is_none()
                    && !keys
                        .iter()
                        .any(|k| v.speaker.trim() == diarize::default_label(k))
            })
            .map(|v| json!({ "speaker": v.speaker, "guessed": v.guessed, "rejected": v.rejected }))
            .collect();
        let out = json!({
            "meeting": id,
            "system_offset_ms": meeting.system_offset_ms,
            "invitees": invited,
            "notes_model_s": took,
            "speakers": speakers,
            "other_guesses": other_guesses,
            "hand_overs": diarize::hand_overs(&segments, &invited, &diarize::own_set(&own))
                .iter()
                .map(|(i, key, name)| json!({ "line": line(*i), "to": key, "name": name }))
                .collect::<Vec<_>>(),
        });
        std::fs::write(
            var("LIPIK_TEST_OUT"),
            serde_json::to_string_pretty(&out).unwrap(),
        )
        .unwrap();
        println!(
            "NAMING invitees={} speakers={} named={} guesses={} rejected={} notes_s={took:.0}",
            invited.len(),
            keys.len(),
            keys.iter()
                .filter(|k| !diarize::is_default_label(&label_of(&segments, k)))
                .count(),
            verdicts.len(),
            verdicts.iter().filter(|v| v.rejected.is_some()).count(),
        );
    }
    #[test]
    fn a_quiet_tap_is_rebuilt_after_growing_waits() {
        let t0 = Instant::now();
        let at = |ms: u64| t0 + Duration::from_millis(ms);
        let mut retry = TapRetry::new(t0);
        retry.heard(t0);
        assert!(!retry.went_quiet(at(1_900)));
        assert_eq!(retry.due(at(1_900), false), None);
        assert!(retry.went_quiet(at(2_000)));
        assert!(!retry.went_quiet(at(2_500)), "said once");
        let mut rebuilt = Vec::new();
        for ms in (2_000..200_000).step_by(250) {
            if let Some(n) = retry.due(at(ms), false) {
                rebuilt.push((n, ms));
            }
        }
        assert_eq!(
            rebuilt,
            vec![
                (1, 3_000),
                (2, 5_000),
                (3, 10_000),
                (4, 25_000),
                (5, 85_000),
                (6, 145_000)
            ]
        );

        assert!(retry.heard(at(200_000)), "back after a quiet spell");
        assert!(!retry.heard(at(200_100)));
        assert_eq!(
            retry.due(at(300_000), false),
            None,
            "no rebuilds while audio flows"
        );
    }

    #[test]
    fn a_quiet_tap_while_something_plays_is_rebuilt_every_few_seconds() {
        let t0 = Instant::now();
        let at = |ms: u64| t0 + Duration::from_millis(ms);
        let mut retry = TapRetry::new(t0);
        retry.heard(t0);
        assert!(retry.went_quiet(at(2_000)));
        let mut rebuilt = Vec::new();
        for ms in (2_000..20_000).step_by(250) {
            if retry.due(at(ms), true).is_some() {
                rebuilt.push(ms);
            }
        }
        assert_eq!(rebuilt, vec![3_000, 5_000, 8_000, 11_000, 14_000, 17_000]);
    }

    #[test]
    fn before_any_audio_the_tap_is_rebuilt_only_while_something_plays() {
        let t0 = Instant::now();
        let at = |ms: u64| t0 + Duration::from_millis(ms);
        let mut retry = TapRetry::new(t0);
        assert!(retry.went_quiet(at(2_000)));
        assert_eq!(retry.due(at(3_000), false), None);
        assert_eq!(
            retry.due(at(100_000), false),
            None,
            "nothing plays: nothing to hear"
        );
        assert_eq!(retry.due(at(100_000), true), Some(1));
        assert_eq!(retry.due(at(101_000), true), None);
        assert_eq!(retry.due(at(102_000), true), Some(2));
    }

    #[test]
    fn a_new_output_device_rebuilds_the_tap_at_once_without_counting_a_try() {
        let t0 = Instant::now();
        let at = |ms: u64| t0 + Duration::from_millis(ms);
        let mut retry = TapRetry::new(t0);
        retry.heard(t0);
        assert!(retry.went_quiet(at(2_000)));
        assert_eq!(retry.due(at(3_000), false), Some(1));
        retry.device_changed(at(4_000));
        assert_eq!(retry.due(at(4_000), false), Some(0), "not a try");
        assert_eq!(
            retry.due(at(5_000), false),
            None,
            "one rebuild for one change"
        );
        assert!(retry.went_quiet(at(6_000)), "silent on the new device too");
        assert_eq!(retry.due(at(7_000), false), Some(1), "tries start again");
    }

    /// Records system audio for 40 s the way a meeting does while macOS plays
    /// its "Glass" sound three times, and checks that the sounds are in the
    /// WAV, that silence between them does not rebuild the tap over and over,
    /// and that the private aggregate device is gone afterwards. Audible.
    /// cargo test --lib system_audio_live_check -- --ignored --nocapture
    #[cfg(target_os = "macos")]
    #[test]
    #[ignore]
    fn system_audio_live_check() {
        static LINES: Mutex<Vec<String>> = Mutex::new(Vec::new());
        struct Keep;
        impl log::Log for Keep {
            fn enabled(&self, m: &log::Metadata) -> bool {
                m.target().contains("notetaker")
            }
            fn log(&self, r: &log::Record) {
                if self.enabled(r.metadata()) {
                    LINES.lock().unwrap().push(r.args().to_string());
                }
            }
            fn flush(&self) {}
        }
        let _ = log::set_logger(&Keep);
        log::set_max_level(log::LevelFilter::Debug);
        let ours = || {
            coreaudio::device_uids()
                .iter()
                .filter(|u| u.starts_with(&tap_name()))
                .count()
        };
        assert_eq!(ours(), 0, "a tap device was there before the test");

        let dir = tempfile::tempdir().unwrap();
        let feed: SharedFeed = Default::default();
        let stop = start_system_audio(dir.path(), feed.clone()).unwrap();
        let during = ours();
        let dings = [6u64, 19, 32];
        let started = Instant::now();
        let mut per_second: Vec<(u64, u64)> = Vec::new();
        let mut running: Vec<u8> = Vec::new();
        let mut others: Vec<u8> = Vec::new();
        let mut next_ding = 0;
        for second in 0..40u64 {
            if dings.get(next_ding) == Some(&second) {
                next_ding += 1;
                std::process::Command::new("afplay")
                    .arg("/System/Library/Sounds/Glass.aiff")
                    .spawn()
                    .unwrap();
            }
            let until = started + Duration::from_secs(second + 1);
            std::thread::sleep(until.saturating_duration_since(Instant::now()));
            let samples = feed.lock().unwrap().samples;
            per_second.push((second, samples));
            running.push(
                coreaudio::default_output_device().is_some_and(coreaudio::device_running_somewhere)
                    as u8,
            );
            others.push(coreaudio::others_playing() as u8);
        }
        stop();
        let mut running_after: Vec<u8> = Vec::new();
        for _ in 0..6 {
            std::thread::sleep(Duration::from_millis(500));
            running_after.push(
                coreaudio::default_output_device().is_some_and(coreaudio::device_running_somewhere)
                    as u8,
            );
        }
        let stopped = Instant::now();
        let mut after = ours();
        while after > 0 && stopped.elapsed() < Duration::from_secs(5) {
            std::thread::sleep(Duration::from_millis(100));
            after = ours();
        }
        println!(
            "LIVE aggregate device listed for {} ms after stop",
            stopped.elapsed().as_millis()
        );
        let first_at = feed
            .lock()
            .unwrap()
            .first_at
            .map_or(0, |at| at.duration_since(started).as_secs());

        let audio: Vec<f32> = hound::WavReader::open(dir.path().join("system.wav"))
            .unwrap()
            .samples::<i16>()
            .map(|s| s.unwrap() as f32 / i16::MAX as f32)
            .collect();
        let loud: Vec<usize> = audio
            .chunks(audio::RATE)
            .enumerate()
            .filter(|(_, c)| c.iter().any(|x| x.abs() > 0.01))
            .map(|(n, _)| n)
            .collect();
        let delivered: Vec<u64> = per_second
            .windows(2)
            .map(|w| (w[1].1 - w[0].1) / (audio::RATE as u64 / 10))
            .collect();
        let lines = LINES.lock().unwrap().clone();
        let count = |text: &str| lines.iter().filter(|l| l.contains(text)).count();
        println!(
            "LIVE tap devices before 0, during {during}, after {after}; WAV {:.1} s; seconds with sound {loud:?}",
            audio.len() as f32 / audio::RATE as f32
        );
        println!("LIVE tenths of a second delivered per second: {delivered:?}");
        println!("LIVE output device running, per second: {running:?}");
        println!("LIVE another process playing, per second: {others:?}");
        println!("LIVE output device running after our tap is gone, per 0.5 s: {running_after:?}");
        println!(
            "LIVE log: stopped {}, back {}, rebuilds {}, not back {}, device changed {}",
            count("stopped"),
            count("is back"),
            count("rebuilding"),
            count("not back yet"),
            count("output device changed")
        );
        assert_eq!(during, 1, "one private aggregate device while recording");
        assert_eq!(after, 0, "the aggregate device is removed");
        for d in dings {
            assert!(
                loud.iter().any(|&s| (first_at + s as u64).abs_diff(d) <= 3),
                "no sound after the ding at {d} s"
            );
        }
        assert!(
            count("rebuilding") <= 5,
            "rebuilt {} times",
            count("rebuilding")
        );
    }

    /// Runs the voice split of the microphone on one stored meeting, by hand,
    /// and prints only counts and minutes per voice. Nothing is written.
    /// LIPIK_TEST_DB=<copy of notetaker.db> LIPIK_TEST_MEETING=<id>
    /// LIPIK_TEST_AUDIO=<folder with copies of mic.wav and system.wav>
    /// LIPIK_TEST_MODELS=<speaker models folder>
    /// LIPIK_TEST_RECORDINGS=<dictation recordings folder> cargo test --lib
    /// room_split_check -- --ignored --nocapture
    #[test]
    #[ignore]
    fn room_split_check() {
        let var = |k: &str| std::env::var(k).unwrap_or_else(|_| panic!("{k} is not set"));
        let id = var("LIPIK_TEST_MEETING");
        let store = Store::open_read_only(std::path::Path::new(&var("LIPIK_TEST_DB"))).unwrap();
        let meeting = store.meeting(&id).unwrap().expect("no such meeting");
        let models = PathBuf::from(var("LIPIK_TEST_MODELS"));
        let dir = PathBuf::from(var("LIPIK_TEST_AUDIO"));
        let minutes = |ms: i64| ms as f64 / 60_000.0;

        let started = Instant::now();
        let clips = dictation_clips(std::path::Path::new(&var("LIPIK_TEST_RECORDINGS")));
        let own = diarize::own_voice(&models, &clips).unwrap();
        println!(
            "ROOM own voice from {} clips: {} in {:.1}s",
            clips.len(),
            if own.is_some() { "built" } else { "none" },
            started.elapsed().as_secs_f32()
        );

        let lines = store.segments(&id).unwrap();
        let call = dir.join("system.wav");
        let call_speakers = lines
            .iter()
            .any(|s| s.source == "system")
            .then(|| diarize::run(&models, &call).ok())
            .flatten();
        let call_speech = call_speech_ms(call_speakers.as_ref(), &lines);
        println!(
            "ROOM call voices {} speech {:.1} min",
            call_speakers.as_ref().map_or(0, |s| s.count()),
            minutes(call_speech)
        );

        let cleaned = dir.join(CLEANED_MIC);
        let _ = std::fs::remove_file(&cleaned);
        let mic = mic_for_transcript(&dir, &meeting, &cleaned);
        let offset = meeting.mic_offset_ms.unwrap_or(0);
        let mic_lines: Vec<(i64, i64)> = lines
            .iter()
            .filter(|s| s.source == "mic")
            .map(|s| (s.start_ms - offset, s.end_ms - offset))
            .collect();
        let mic_speech: i64 = mic_lines.iter().map(|(s, e)| e - s).sum();
        if !diarize::quiet_call(call_speech, mic_speech) {
            let Some(own) = own.as_deref() else {
                println!("ROOM split skipped: a call and no own voice");
                return;
            };
            let call_turns = call_speakers
                .as_ref()
                .map(|s| s.turns.clone())
                .unwrap_or_default();
            let during =
                diarize::mic_during_call(&mic_lines, &call_turns, mic_to_call_ms(&meeting));
            println!("ROOM mic speech during call speech {:.0}%", 100.0 * during);
            let windows =
                diarize::lone_mic_windows(&mic_lines, &call_turns, mic_to_call_ms(&meeting));
            let voices = diarize::window_voices(&models, &mic.path, &windows).unwrap();
            let sims: Vec<String> = voices
                .iter()
                .map(|v| format!("{:.2}", diarize::cosine(v, own)))
                .collect();
            println!(
                "ROOM checked {} lone mic stretches, like own [{}]",
                voices.len(),
                sims.join(" ")
            );
            if during <= diarize::CHECK_MAX_DURING_CALL && diarize::only_user(&voices, own) {
                let _ = std::fs::remove_file(&cleaned);
                println!("ROOM split skipped: the stretches sound like the user");
                return;
            }
        }
        println!("ROOM split runs");
        let started = Instant::now();
        let split = diarize::run(&models, &mic.path).unwrap();
        let _ = std::fs::remove_file(&cleaned);
        println!(
            "ROOM mic clean={} split into {} voices in {:.1}s",
            mic.clean,
            split.count(),
            started.elapsed().as_secs_f32()
        );
        let call_voices = call_speakers
            .as_ref()
            .map(|s| s.voices.clone())
            .unwrap_or_default();
        let call_turns: Vec<diarize::Turn> = call_speakers
            .as_ref()
            .map(|s| s.turns.clone())
            .unwrap_or_default();
        let shift = mic_to_call_ms(&meeting);
        for n in 0..split.count() {
            let v = split.voices[n].as_deref().unwrap_or(&[]);
            let over: i64 = split
                .turns
                .iter()
                .filter(|t| t.speaker == n)
                .map(|t| diarize::overlap_ms(t.start_ms + shift, t.end_ms + shift, &call_turns))
                .sum();
            println!(
                "ROOM mic voice {} overlaps call speech {:.0}%",
                n + 1,
                100.0 * over as f64 / split.speech_ms[n].max(1) as f64
            );
            let others: Vec<String> = (0..split.count())
                .map(|m| {
                    format!(
                        "{:.2}",
                        diarize::cosine(v, split.voices[m].as_deref().unwrap_or(&[]))
                    )
                })
                .collect();
            let to_call: Vec<String> = call_voices
                .iter()
                .map(|c| format!("{:.2}", diarize::cosine(v, c.as_deref().unwrap_or(&[]))))
                .collect();
            println!(
                "ROOM mic voice {} speech {:.1} min, like own {:.2}, like mic voices [{}], like call voices [{}]",
                n + 1,
                minutes(split.speech_ms[n]),
                own.as_deref().map_or(f32::NAN, |o| diarize::cosine(v, o)),
                others.join(" "),
                to_call.join(" ")
            );
        }
        let room = diarize::room_voices(
            &split,
            own.as_deref(),
            call_speakers
                .as_ref()
                .unwrap_or(&diarize::Speakers::default()),
            shift,
            call_speech,
        );
        let Some(room) = room else {
            println!("ROOM no room voices: the mic stays Me");
            return;
        };
        let mut by_key: std::collections::BTreeMap<String, (usize, i64, i64)> = Default::default();
        for t in &room.turns {
            by_key
                .entry(diarize::key_and_label(t.speaker).0)
                .or_default()
                .2 += t.end_ms - t.start_ms;
        }
        for s in lines.iter().filter(|s| s.source == "mic") {
            let n = room
                .speaker_at(s.start_ms - offset, s.end_ms - offset)
                .unwrap_or(diarize::ME);
            let e = by_key.entry(diarize::key_and_label(n).0).or_default();
            e.0 += 1;
            e.1 += s.end_ms - s.start_ms;
        }
        for (key, (count, line_ms, turn_ms)) in by_key {
            println!(
                "ROOM {key}: {count} stored mic lines ({:.1} min), split turns {:.1} min",
                minutes(line_ms),
                minutes(turn_ms)
            );
        }
    }
}
