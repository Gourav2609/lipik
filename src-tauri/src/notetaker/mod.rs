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
mod live;
pub mod mcp;
mod store;
mod transcribe;

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
        }
    }
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, Type, PartialEq)]
pub struct Attendee {
    pub name: Option<String>,
    pub email: Option<String>,
    pub is_me: bool,
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
                if let Err(e) = this.process(&id, transcribe) {
                    log::error!("Notetaker processing of {id} failed: {e}");
                    this.fail(&id, &e.to_string());
                }
                let _ = this.store.clear_process_attempts(&id);
                this.processing.lock().unwrap().retain(|m| m != &id);
            });
        if let Err(e) = spawned {
            self.processing.lock().unwrap().retain(|m| m != &id);
            self.fail(&id, &format!("Could not start processing: {e}"));
            return Err(e.into());
        }
        Ok(())
    }

    /// Final transcript of both streams (when `transcribe`), then notes.
    fn process(&self, id: &str, transcribe: bool) -> Result<()> {
        let meeting = self
            .store
            .meeting(id)?
            .ok_or_else(|| anyhow!("Meeting {id} is gone"))?;
        let dir = self.meeting_dir(id);

        if transcribe {
            let system_wav = dir.join("system.wav");
            let heard_call = self
                .store
                .segments(id)?
                .iter()
                .any(|s| s.source == "system");
            let speakers = (system_wav.exists() && heard_call)
                .then(|| self.find_speakers(id, &system_wav))
                .flatten();
            let sources = [
                (
                    "mic",
                    dir.join("mic.wav"),
                    meeting.mic_offset_ms.unwrap_or(0),
                ),
                (
                    "system",
                    dir.join("system.wav"),
                    meeting.system_offset_ms.unwrap_or(0),
                ),
            ];
            let present: Vec<_> = sources.iter().filter(|(_, p, _)| p.exists()).collect();
            let mut results: Vec<(&str, Vec<Segment>)> = Vec::new();
            let mut failed = None;
            for (n, (source, path, offset)) in present.iter().enumerate() {
                let parts = present.len() as f32;
                let emit = |fraction: f32| {
                    let _ = NotetakerProcessing {
                        meeting_id: id.to_string(),
                        stage: ProcessingStage::Transcribing,
                        progress: (n as f32 + fraction) / parts,
                    }
                    .emit(&self.app);
                };
                let split = speakers.as_ref().filter(|_| *source == "system");
                match transcribe::transcribe_file(&self.app, path, *offset, source, split, emit) {
                    Ok(segments) => results.push((*source, segments)),
                    Err(e) => {
                        failed = Some(e);
                        break;
                    }
                }
            }
            transcribe::release_model(&self.app);
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
                    if let Some(speakers) = &speakers {
                        self.name_known_speakers(id, &meeting, speakers, &mut merged);
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
        self.write_notes(id)
    }

    /// Splits the call audio by voice. Any failure (no models, offline on
    /// first use, a crash in the model) keeps one "Them" for the call.
    fn find_speakers(&self, id: &str, wav: &std::path::Path) -> Option<diarize::Speakers> {
        let _ = NotetakerProcessing {
            meeting_id: id.to_string(),
            stage: ProcessingStage::Speakers,
            progress: 0.0,
        }
        .emit(&self.app);
        let started = Instant::now();
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            diarize::detect(&self.app, wav)
        }))
        .unwrap_or_else(|_| Err(anyhow!("speaker detection panicked")));
        match result {
            Ok(speakers) => {
                log::info!(
                    "Notetaker found {} speakers in {id} in {:.1}s",
                    speakers.count(),
                    started.elapsed().as_secs_f32()
                );
                Some(speakers)
            }
            Err(e) => {
                log::warn!("Notetaker kept one speaker for the call audio of {id}: {e}");
                None
            }
        }
    }

    /// Keeps the meeting's voices (for naming them later) and names the
    /// speakers a saved voice or the calendar already identifies.
    fn name_known_speakers(
        &self,
        id: &str,
        meeting: &Meeting,
        speakers: &diarize::Speakers,
        segments: &mut [Segment],
    ) {
        let voices: Vec<(String, Vec<f32>, i64)> = speakers
            .voices
            .iter()
            .enumerate()
            .filter_map(|(n, v)| Some((diarize::speaker_key(n), v.clone()?, speakers.speech_ms[n])))
            .collect();
        if let Err(e) = self.store.save_meeting_voices(id, &voices) {
            log::warn!("Notetaker could not save the voices of {id}: {e}");
        }
        let profiles = self.store.voice_profiles().unwrap_or_default();
        for (n, name) in diarize::known_names(speakers, &profiles, &meeting.attendees) {
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
        let _ = NotetakerProcessing {
            meeting_id: id.to_string(),
            stage: ProcessingStage::Summarizing,
            progress: 0.0,
        }
        .emit(&self.app);

        let thoughts = self.store.thoughts(id)?;
        match ai::summarize(&model, &meeting, &thoughts, &segments) {
            Ok((summary, guesses)) => {
                let own = self.own_names(&meeting);
                let mut changed = 0;
                for (key, name) in diarize::names_from_guesses(&segments, &guesses, &own) {
                    let default = diarize::default_label(&key);
                    changed += self
                        .store
                        .name_speaker_if_default(id, &key, &default, &name)?;
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

    /// Meetings left recording or processing by a crash or quit are finished
    /// from their WAV files. One whose processing was cut off twice is marked
    /// failed instead, in case it is what stops the app.
    pub fn resume_unfinished(self: &Arc<Self>) {
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

    pub fn list(&self, filter: &NotetakerFilter) -> Result<Vec<Meeting>> {
        self.store.list(filter)
    }

    pub fn get(&self, id: &str) -> Result<Option<MeetingDetail>> {
        self.store.detail(id)
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
        let mut own: Vec<String> = meeting
            .attendees
            .iter()
            .filter(|a| a.is_me)
            .flat_map(|a| [a.name.clone(), a.email.clone()])
            .flatten()
            .collect();
        own.extend(user_full_name());
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
        if self.processing.lock().unwrap().iter().any(|m| m == id) {
            return Err(anyhow!("Speakers can be named once the notes are written"));
        }
        self.store.rename_speaker(id, speaker_key, name)?;
        if let Err(e) =
            self.store
                .set_voice_named(id, speaker_key, !diarize::is_default_label(name))
        {
            log::warn!("Notetaker could not remember a voice: {e}");
        }
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
        Ok(self.store.voice_profiles()?.len() as u32)
    }

    pub fn forget_voices(&self) -> Result<()> {
        self.store.forget_voices()
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
        if self.processing.lock().unwrap().iter().any(|m| m == id) {
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

/// Processing runs behind dictation: macOS gives this thread (and threads it
/// starts) a lower priority.
fn background_priority() {
    #[cfg(target_os = "macos")]
    // SAFETY: sets the calling thread's own QoS class; no pointers involved.
    unsafe {
        libc::pthread_set_qos_class_self_np(libc::qos_class_t::QOS_CLASS_UTILITY, 0);
    }
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

/// Moves system audio from the tap to the WAV. Builds the tap again when its
/// audio stops (output device gone) or the default output device changes;
/// the writer fills the gap with silence.
#[cfg(target_os = "macos")]
fn system_worker(
    first: (coreaudio::SystemTap, std::sync::mpsc::Receiver<Vec<f32>>),
    mut writer: StreamWriter,
    stop: &AtomicBool,
) {
    use std::sync::mpsc::RecvTimeoutError;
    const STALL: Duration = Duration::from_secs(2);

    let mut resampler = audio::Resampler::new(first.0.sample_rate);
    let mut current = Some(first);
    let mut out = Vec::new();
    let mut last_audio = Instant::now();
    let mut last_device_check = Instant::now();

    while !stop.load(Ordering::Relaxed) {
        let Some((tap, rx)) = current.as_ref() else {
            std::thread::sleep(Duration::from_secs(1));
            match coreaudio::SystemTap::start(&tap_name()) {
                Ok(next) => {
                    log::info!("Notetaker system audio reconnected");
                    resampler = audio::Resampler::new(next.0.sample_rate);
                    writer.set_failed(None);
                    last_audio = Instant::now();
                    last_device_check = Instant::now();
                    current = Some(next);
                }
                Err(e) => log::debug!("Notetaker system audio not back yet: {e}"),
            }
            continue;
        };
        match rx.recv_timeout(Duration::from_millis(250)) {
            Ok(block) => {
                last_audio = Instant::now();
                out.clear();
                resampler.process(&block, &mut out);
                writer.write(&out);
            }
            Err(RecvTimeoutError::Timeout) | Err(RecvTimeoutError::Disconnected) => {}
        }
        let device_changed = last_device_check.elapsed() >= STALL && {
            last_device_check = Instant::now();
            coreaudio::default_output_device() != Some(tap.output_device)
        };
        let stalled = last_audio.elapsed() >= STALL;
        if stalled || device_changed {
            if stalled {
                log::warn!("Notetaker system audio stopped; reconnecting");
                writer.set_failed(Some(
                    "System audio stopped (output device changed or gone). Reconnecting…"
                        .to_string(),
                ));
            } else {
                log::info!("Notetaker: output device changed, rebuilding the system audio tap");
            }
            current = None;
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
            detect::spawn(manager);
        }
        Err(e) => log::error!("Notetaker unavailable: {e}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
}
