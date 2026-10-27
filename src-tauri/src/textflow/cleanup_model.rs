//! The clean-up model for onboarding and Settings: which engine runs it
//! (built-in or Ollama), whether it is there, and its download (the GGUF
//! file for the built-in engine, or through Ollama).

use super::engine::{self, BuiltinModel, ENGINE};
use super::llm::{self, CLIENT};
use crate::managers::model::{HttpDownloadEvent, HttpDownloadOutcome, ModelManager};
use crate::settings::CleanupEngine;
use futures_util::future::{select, Either};
use futures_util::StreamExt;
use hf_hub::api::tokio::CancellationToken;
use once_cell::sync::Lazy;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use specta::Type;
use std::sync::Mutex;
use std::time::Duration;
use tauri::AppHandle;
use tauri_specta::Event;
use tokio::sync::Notify;

/// Ollama sends a progress line at least every few seconds while it
/// downloads; this long without one means the download has stalled.
const IDLE_LIMIT: Duration = Duration::from_secs(60);

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum CleanupModelState {
    NoOllama,
    NoModel,
    /// The model is plain `qwen3:4b`, which is too slow for dictation.
    SlowModel,
    Downloading,
    /// The built-in model is downloaded and its checksum is being checked.
    Verifying,
    Ready,
}

#[derive(Clone, Debug, Serialize, Type)]
pub struct CleanupModelStatus {
    pub engine: CleanupEngine,
    /// Whether this build has the built-in engine (macOS only).
    pub builtin_available: bool,
    pub state: CleanupModelState,
    /// The configured model, or while `downloading` the model being downloaded.
    pub model: String,
    /// Bytes so far while `state` is `downloading`.
    pub progress: Option<CleanupModelPull>,
    /// The built-in model's download size in bytes; 0 with Ollama.
    pub size: u64,
}

/// Download progress of the clean-up model, in bytes over all its files.
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize, Type, Event)]
pub struct CleanupModelPull {
    pub completed: u64,
    pub total: u64,
}

/// The running download, if any. Every download, and every cancel, gets a
/// new generation; a download only writes while its generation is current.
#[derive(Default)]
struct Pulls {
    generation: u64,
    active: Option<Active>,
}

#[derive(Clone, Debug, PartialEq)]
struct Active {
    engine: CleanupEngine,
    model: String,
    progress: CleanupModelPull,
    verifying: bool,
}

impl Pulls {
    fn start(&mut self, engine: CleanupEngine, model: &str) -> u64 {
        self.generation += 1;
        self.active = Some(Active {
            engine,
            model: model.to_string(),
            progress: CleanupModelPull::default(),
            verifying: false,
        });
        self.generation
    }

    fn update(&mut self, generation: u64, progress: CleanupModelPull) -> bool {
        self.change(generation, |a| a.progress = progress)
    }

    fn verifying(&mut self, generation: u64) -> bool {
        self.change(generation, |a| a.verifying = true)
    }

    fn change(&mut self, generation: u64, f: impl FnOnce(&mut Active)) -> bool {
        match &mut self.active {
            Some(active) if self.generation == generation => {
                f(active);
                true
            }
            _ => false,
        }
    }

    fn finish(&mut self, generation: u64) {
        if self.generation == generation {
            self.active = None;
        }
    }

    fn cancel(&mut self) {
        self.generation += 1;
        self.active = None;
    }

    fn is_current(&self, generation: u64) -> bool {
        self.generation == generation && self.active.is_some()
    }

    fn builtin_running(&self) -> bool {
        self.active
            .as_ref()
            .is_some_and(|a| a.engine == CleanupEngine::Builtin)
    }
}

static PULLS: Lazy<Mutex<Pulls>> = Lazy::new(Default::default);
static CANCEL: Lazy<Notify> = Lazy::new(Notify::new);
/// Cancels the running download of the built-in model.
static BUILTIN_CANCEL: Lazy<Mutex<CancellationToken>> =
    Lazy::new(|| Mutex::new(CancellationToken::new()));
/// How often Ollama is asked again while the engine is not chosen yet.
const PICK_RETRY_EVERY: Duration = Duration::from_secs(30);
/// The engine offered in onboarding and Settings while the setting is unset:
/// Ollama once it answered with the clean-up model, else the built-in one.
static OFFERED: Lazy<Mutex<CleanupEngine>> = Lazy::new(|| Mutex::new(CleanupEngine::Builtin));

const SLOW_MODEL: &str = "qwen3:4b thinks before answering and is too slow for dictation; use qwen3:4b-instruct or qwen3:8b.";

/// Plain `qwen3:4b` on Ollama is a thinking model that ignores
/// `think: false` and takes 40-110 s per clean-up.
fn is_thinking_model(model: &str) -> bool {
    matches!(model.trim(), "qwen3:4b" | "qwen3:4b:latest")
}

/// Whether an `/api/tags` reply lists `model`. A name without a tag means
/// `:latest`, as in Ollama.
fn has_model(tags: &Value, model: &str) -> bool {
    let model = model.trim();
    let full = if model.contains(':') {
        model.to_string()
    } else {
        format!("{model}:latest")
    };
    tags["models"].as_array().is_some_and(|models| {
        models.iter().any(|m| {
            [&m["name"], &m["model"]]
                .iter()
                .filter_map(|n| n.as_str())
                .any(|n| n == full || n == model)
        })
    })
}

#[derive(Debug, PartialEq)]
enum PullStep {
    Progress(CleanupModelPull),
    Status,
    Success,
}

/// Adds up the per-file progress lines of a streamed `/api/pull` reply.
#[derive(Default)]
struct PullProgress {
    files: Vec<(String, u64, u64)>,
}

impl PullProgress {
    fn total(&self) -> CleanupModelPull {
        self.files
            .iter()
            .fold(CleanupModelPull::default(), |sum, (_, completed, total)| {
                CleanupModelPull {
                    completed: sum.completed + completed.min(total),
                    total: sum.total + total,
                }
            })
    }

    fn apply(&mut self, line: &str) -> Result<PullStep, String> {
        let line = line.trim();
        if line.is_empty() {
            return Ok(PullStep::Status);
        }
        let value: Value =
            serde_json::from_str(line).map_err(|_| "Ollama sent an unexpected reply.")?;
        if let Some(error) = value["error"].as_str() {
            return Err(format!("Ollama said: {error}"));
        }
        if value["status"] == "success" {
            return Ok(PullStep::Success);
        }
        let (Some(digest), Some(total)) = (value["digest"].as_str(), value["total"].as_u64())
        else {
            return Ok(PullStep::Status);
        };
        let completed = value["completed"].as_u64().unwrap_or(0);
        match self.files.iter_mut().find(|(d, _, _)| d == digest) {
            Some(file) => {
                file.1 = file.1.max(completed);
                file.2 = total;
            }
            None => self.files.push((digest.to_string(), completed, total)),
        }
        Ok(PullStep::Progress(self.total()))
    }
}

fn per_mille(p: CleanupModelPull) -> u64 {
    (p.completed * 1000).checked_div(p.total).unwrap_or(0)
}

/// Which engine clean-up and notes requests go to: the setting, or while it
/// is unset, the built-in engine only once its model file is there and
/// Ollama otherwise, so someone who used Ollama before never loses it (and
/// a request to a missing Ollama fails at once).
pub fn engine_from(app: &AppHandle, settings: &crate::settings::AppSettings) -> CleanupEngine {
    settings.cleanup_engine.unwrap_or_else(|| {
        if builtin_downloaded(app) {
            CleanupEngine::Builtin
        } else {
            CleanupEngine::Ollama
        }
    })
}

fn builtin_downloaded(app: &AppHandle) -> bool {
    ENGINE.is_available() && engine::model_path(app).is_some_and(|p| p.is_file())
}

/// The engine onboarding and Settings show: the setting, or while it is
/// unset the one `pick_engine` offers.
fn offered_engine(app: &AppHandle) -> CleanupEngine {
    if !ENGINE.is_available() {
        return CleanupEngine::Ollama;
    }
    crate::settings::get_settings(app)
        .cleanup_engine
        .unwrap_or_else(|| *OFFERED.lock().unwrap())
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum BuiltinFile {
    Missing,
    Downloading,
    Ready,
}

/// The engine for a user who has not chosen one, and whether to save it.
/// `ollama`: whether Ollama answered, and if so whether it has the clean-up
/// model. Only a downloaded built-in model or Ollama with the model settles
/// it; otherwise the built-in engine is offered and Ollama asked again.
fn default_engine(builtin: BuiltinFile, ollama: Option<bool>) -> (CleanupEngine, bool) {
    match (builtin, ollama) {
        (BuiltinFile::Ready, _) => (CleanupEngine::Builtin, true),
        (BuiltinFile::Downloading, _) => (CleanupEngine::Builtin, false),
        (BuiltinFile::Missing, Some(true)) => (CleanupEngine::Ollama, true),
        (BuiltinFile::Missing, _) => (CleanupEngine::Builtin, false),
    }
}

/// While the user has not chosen an engine: people who already use Ollama
/// with the clean-up model keep it (saved as their choice), a downloaded
/// built-in model makes the built-in engine the choice, and Ollama is asked
/// again every 30 s in case it starts later. Builds or Macs without the
/// built-in engine use Ollama.
pub fn pick_engine(app: &AppHandle) {
    if !ENGINE.is_available() || crate::settings::get_settings(app).cleanup_engine.is_some() {
        return;
    }
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        loop {
            if crate::settings::get_settings(&app).cleanup_engine.is_some() {
                return;
            }
            let file = if builtin_downloaded(&app) {
                BuiltinFile::Ready
            } else if PULLS.lock().unwrap().builtin_running() {
                BuiltinFile::Downloading
            } else {
                BuiltinFile::Missing
            };
            let ollama = if file == BuiltinFile::Missing {
                let endpoint = llm::ollama_endpoint(&app);
                ollama_tags(&endpoint.url).await.map(|tags| {
                    has_model(&tags, &endpoint.model) && !is_thinking_model(&endpoint.model)
                })
            } else {
                None
            };
            let (picked, settled) = default_engine(file, ollama);
            *OFFERED.lock().unwrap() = picked;
            if settled {
                let mut settings = crate::settings::get_settings(&app);
                if settings.cleanup_engine.is_none() {
                    settings.cleanup_engine = Some(picked);
                    crate::settings::write_settings(&app, settings);
                    log::info!("Clean-up engine: {picked:?} (picked for this user)");
                }
                return;
            }
            tokio::time::sleep(PICK_RETRY_EVERY).await;
        }
    });
}

async fn ollama_tags(url: &str) -> Option<Value> {
    CLIENT
        .get(format!("{url}/api/tags"))
        .timeout(Duration::from_millis(1500))
        .send()
        .await
        .ok()?
        .json::<Value>()
        .await
        .ok()
}

#[tauri::command]
#[specta::specta]
pub async fn cleanup_model_status(app: AppHandle) -> CleanupModelStatus {
    let engine = offered_engine(&app);
    let pulling = PULLS
        .lock()
        .unwrap()
        .active
        .clone()
        .filter(|a| a.engine == engine);
    let builtin = engine::builtin_model();
    let size = if engine == CleanupEngine::Builtin {
        builtin.size
    } else {
        0
    };
    let status = |state, model: String, progress| CleanupModelStatus {
        engine,
        builtin_available: ENGINE.is_available(),
        state,
        model,
        progress,
        size,
    };
    if let Some(active) = pulling {
        let state = if active.verifying {
            CleanupModelState::Verifying
        } else {
            CleanupModelState::Downloading
        };
        return status(state, active.model, Some(active.progress));
    }
    if engine == CleanupEngine::Builtin {
        let state = if builtin_downloaded(&app) {
            CleanupModelState::Ready
        } else {
            CleanupModelState::NoModel
        };
        return status(state, builtin.name.to_string(), None);
    }
    let endpoint = llm::ollama_endpoint(&app);
    let state = match ollama_tags(&endpoint.url).await {
        None => CleanupModelState::NoOllama,
        Some(_) if is_thinking_model(&endpoint.model) => CleanupModelState::SlowModel,
        Some(tags) if has_model(&tags, &endpoint.model) => CleanupModelState::Ready,
        Some(_) => CleanupModelState::NoModel,
    };
    status(state, endpoint.model, None)
}

/// Downloads the clean-up model of the engine onboarding and Settings show
/// (the built-in engine's GGUF file, or through Ollama) and loads it.
/// Progress comes as `CleanupModelPull` events. `Ok(false)`: cancelled, or
/// replaced by a newer download.
#[tauri::command]
#[specta::specta]
pub async fn pull_cleanup_model(app: AppHandle) -> Result<bool, String> {
    let engine = offered_engine(&app);
    let ollama = llm::ollama_endpoint(&app);
    let model = match engine {
        CleanupEngine::Builtin => engine::builtin_model().name.to_string(),
        CleanupEngine::Ollama if is_thinking_model(&ollama.model) => {
            return Err(SLOW_MODEL.to_string())
        }
        CleanupEngine::Ollama => ollama.model.clone(),
    };
    let generation = PULLS.lock().unwrap().start(engine, &model);
    let result = match engine {
        CleanupEngine::Builtin => download_builtin(&app, engine::builtin_model(), generation).await,
        CleanupEngine::Ollama => pull(&app, &ollama.url, &ollama.model, generation).await,
    };
    PULLS.lock().unwrap().finish(generation);
    match result {
        Ok(true) => {
            log::info!("cleanup model: {model} downloaded");
            if super::uses_llm(&app) {
                llm::warm_up(&app);
            }
            Ok(true)
        }
        Ok(false) => Ok(false),
        Err(e) => {
            log::warn!("cleanup model: download of {model} failed: {e}");
            Err(e)
        }
    }
}

#[tauri::command]
#[specta::specta]
pub fn cancel_cleanup_model_pull() {
    PULLS.lock().unwrap().cancel();
    CANCEL.notify_waiters();
    BUILTIN_CANCEL.lock().unwrap().cancel();
}

/// Switches the engine for clean-up and notes. Leaving the built-in engine
/// stops its server; moving to it unloads Ollama's model so the two don't
/// both hold memory. A running download is cancelled.
#[tauri::command]
#[specta::specta]
pub fn set_cleanup_engine(app: AppHandle, engine: CleanupEngine) {
    let mut settings = crate::settings::get_settings(&app);
    if settings.cleanup_engine == Some(engine) {
        return;
    }
    settings.cleanup_engine = Some(engine);
    crate::settings::write_settings(&app, settings);
    cancel_cleanup_model_pull();
    match engine {
        CleanupEngine::Ollama => ENGINE.stop(),
        CleanupEngine::Builtin => {
            let ollama = llm::ollama_endpoint(&app);
            tauri::async_runtime::spawn(async move {
                let _ = CLIENT
                    .post(format!("{}/api/generate", ollama.url))
                    .json(&json!({ "model": ollama.model, "keep_alive": 0 }))
                    .timeout(Duration::from_secs(5))
                    .send()
                    .await;
            });
        }
    }
    if super::uses_llm(&app) {
        llm::warm_up(&app);
    }
}

/// Stops a running Ollama download of any model other than `model`, for
/// when the model setting changes.
pub fn cancel_pull_unless(model: &str) {
    let mut pulls = PULLS.lock().unwrap();
    if pulls
        .active
        .as_ref()
        .is_some_and(|a| a.engine == CleanupEngine::Ollama && a.model != model.trim())
    {
        pulls.cancel();
        drop(pulls);
        CANCEL.notify_waiters();
    }
}

/// Room the download needs besides the model itself.
const SPARE_DISK: u64 = 1_000_000_000;

#[cfg(unix)]
fn free_disk(dir: &std::path::Path) -> Option<u64> {
    use std::os::unix::ffi::OsStrExt;
    let path = std::ffi::CString::new(dir.as_os_str().as_bytes()).ok()?;
    let mut stat: libc::statvfs = unsafe { std::mem::zeroed() };
    (unsafe { libc::statvfs(path.as_ptr(), &mut stat) } == 0).then(|| {
        #[allow(clippy::unnecessary_cast)]
        let bytes = stat.f_bavail as u64 * stat.f_frsize as u64;
        bytes
    })
}

#[cfg(not(unix))]
fn free_disk(_dir: &std::path::Path) -> Option<u64> {
    None
}

const GB: f64 = 1e9;

/// Downloads the built-in model into the models folder with the app's
/// resumable downloader: a cancelled or broken download continues where it
/// stopped, and the file is checked against its SHA-256 before it is used.
async fn download_builtin(
    app: &AppHandle,
    model: &'static BuiltinModel,
    generation: u64,
) -> Result<bool, String> {
    let dir = engine::models_dir(app).ok_or("Lipik has no data folder")?;
    let path = dir.join(model.file);
    if path.is_file() {
        return Ok(true);
    }
    std::fs::create_dir_all(&dir)
        .map_err(|e| format!("Could not create {}: {e}", dir.display()))?;
    let partial = dir.join(format!("{}.partial", model.file));
    let have = partial.metadata().map_or(0, |m| m.len());
    if let Some(free) = free_disk(&dir) {
        let needed = model.size.saturating_sub(have) + SPARE_DISK;
        if free < needed {
            return Err(format!(
                "There isn't enough free disk space. The model needs {:.1} GB; this Mac has {:.1} GB free.",
                needed as f64 / GB,
                free as f64 / GB
            ));
        }
    }
    let cancel = CancellationToken::new();
    *BUILTIN_CANCEL.lock().unwrap() = cancel.clone();
    if !PULLS.lock().unwrap().is_current(generation) {
        return Ok(false);
    }
    let shown = Mutex::new(None);
    let emit = |event: HttpDownloadEvent<'_>| {
        let p = match event {
            HttpDownloadEvent::Progress(p) => p,
            HttpDownloadEvent::VerificationStarted => {
                PULLS.lock().unwrap().verifying(generation);
                return;
            }
            HttpDownloadEvent::VerificationCompleted => return,
        };
        let progress = CleanupModelPull {
            completed: p.downloaded,
            total: p.total,
        };
        if !PULLS.lock().unwrap().update(generation, progress) {
            cancel.cancel();
            return;
        }
        let mut shown = shown.lock().unwrap();
        if *shown != Some(per_mille(progress)) {
            *shown = Some(per_mille(progress));
            let _ = progress.emit(app);
        }
    };
    let outcome = ModelManager::download_http_resumable_with_events(
        model.file,
        &model.url(),
        &partial,
        Some(model.size),
        Some(model.sha256),
        &cancel,
        &emit,
    )
    .await
    .map_err(|e| format!("Check your internet, then try again. ({e})"))?;
    match outcome {
        HttpDownloadOutcome::Cancelled => Ok(false),
        HttpDownloadOutcome::Completed => {
            // A second download of the same finished file may have renamed it.
            if let Err(e) = std::fs::rename(&partial, &path) {
                if !path.is_file() {
                    return Err(format!("Could not save the model: {e}"));
                }
            }
            Ok(PULLS.lock().unwrap().is_current(generation))
        }
    }
}

async fn pull(app: &AppHandle, url: &str, model: &str, generation: u64) -> Result<bool, String> {
    let current = || PULLS.lock().unwrap().is_current(generation);
    let response = CLIENT
        .post(format!("{url}/api/pull"))
        .json(&json!({ "model": model, "name": model, "stream": true }))
        .send()
        .await
        .map_err(|_| "Ollama isn't running. Open Ollama, then try again.".to_string())?;
    if !response.status().is_success() {
        return Err(format!("Ollama returned {}.", response.status()));
    }
    let mut stream = response.bytes_stream();
    let mut buffer: Vec<u8> = Vec::new();
    let mut progress = PullProgress::default();
    let mut shown = None;
    loop {
        let cancelled = CANCEL.notified();
        if !current() {
            return Ok(false);
        }
        let next = tokio::time::timeout(IDLE_LIMIT, stream.next());
        let chunk = match select(Box::pin(next), Box::pin(cancelled)).await {
            Either::Left((Ok(Some(chunk)), _)) => {
                chunk.map_err(|_| "The connection to Ollama broke. Try again.")?
            }
            Either::Left((Ok(None), _)) => break,
            Either::Left((Err(_), _)) => {
                return Err(
                    "Nothing arrived for a minute. Check your internet, then try again."
                        .to_string(),
                )
            }
            Either::Right(_) => return Ok(false),
        };
        buffer.extend_from_slice(&chunk);
        while let Some(end) = buffer.iter().position(|b| *b == b'\n') {
            let line: Vec<u8> = buffer.drain(..=end).collect();
            match progress.apply(&String::from_utf8_lossy(&line))? {
                PullStep::Success => return Ok(current()),
                PullStep::Status => {}
                PullStep::Progress(p) => {
                    if !PULLS.lock().unwrap().update(generation, p) {
                        return Ok(false);
                    }
                    if shown != Some(per_mille(p)) {
                        shown = Some(per_mille(p));
                        let _ = p.emit(app);
                    }
                }
            }
        }
    }
    match progress.apply(&String::from_utf8_lossy(&buffer))? {
        PullStep::Success => Ok(current()),
        _ => Err("Ollama stopped before it finished. Try again.".to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_the_model_in_the_tags_reply() {
        let tags = json!({ "models": [
            { "name": "qwen3:8b", "model": "qwen3:8b", "size": 5225388164u64 },
            { "name": "llama3.2:latest", "model": "llama3.2:latest" },
        ]});
        assert!(has_model(&tags, "qwen3:8b"));
        assert!(has_model(&tags, " qwen3:8b "));
        assert!(has_model(&tags, "llama3.2"));
        assert!(has_model(&tags, "llama3.2:latest"));
        assert!(!has_model(&tags, "qwen3:4b"));
        assert!(!has_model(&tags, "qwen3"));
        assert!(!has_model(&json!({ "models": [] }), "qwen3:8b"));
        assert!(!has_model(&json!({}), "qwen3:8b"));
    }

    #[test]
    fn adds_up_progress_over_all_files() {
        let mut p = PullProgress::default();
        let lines = [
            r#"{"status":"pulling manifest"}"#,
            r#"{"status":"pulling a3de86cd1c13","digest":"sha256:a3de","total":5000}"#,
            r#"{"status":"pulling a3de86cd1c13","digest":"sha256:a3de","total":5000,"completed":2000}"#,
            r#"{"status":"pulling ae370d884f10","digest":"sha256:ae37","total":1000,"completed":1000}"#,
            r#"{"status":"pulling a3de86cd1c13","digest":"sha256:a3de","total":5000,"completed":4000}"#,
        ];
        let steps: Vec<_> = lines.iter().map(|l| p.apply(l).unwrap()).collect();
        assert_eq!(steps[0], PullStep::Status);
        let at = |completed, total| PullStep::Progress(CleanupModelPull { completed, total });
        assert_eq!(steps[1], at(0, 5000));
        assert_eq!(steps[2], at(2000, 5000));
        assert_eq!(steps[3], at(3000, 6000));
        assert_eq!(steps[4], at(5000, 6000));
        assert_eq!(per_mille(p.total()), 833);
    }

    #[test]
    fn reads_the_end_of_a_pull() {
        let mut p = PullProgress::default();
        for line in [
            r#"{"status":"verifying sha256 digest"}"#,
            r#"{"status":"writing manifest"}"#,
            "",
        ] {
            assert_eq!(p.apply(line), Ok(PullStep::Status));
        }
        assert_eq!(p.apply("{\"status\":\"success\"}\n"), Ok(PullStep::Success));
    }

    #[test]
    fn already_downloaded_model_finishes_at_once() {
        let mut p = PullProgress::default();
        let reply = [
            r#"{"status":"pulling manifest"}"#,
            r#"{"status":"pulling a3de86cd1c13","digest":"sha256:a3de","total":5000,"completed":5000}"#,
            r#"{"status":"verifying sha256 digest"}"#,
            r#"{"status":"writing manifest"}"#,
            r#"{"status":"removing unused layers"}"#,
            r#"{"status":"success"}"#,
        ];
        let last = reply.iter().map(|l| p.apply(l).unwrap()).last();
        assert_eq!(last, Some(PullStep::Success));
        assert_eq!(per_mille(p.total()), 1000);
    }

    #[test]
    fn errors_and_bad_lines_stop_the_pull() {
        let mut p = PullProgress::default();
        assert_eq!(
            p.apply(r#"{"error":"pull model manifest: file does not exist"}"#),
            Err("Ollama said: pull model manifest: file does not exist".to_string())
        );
        assert!(p.apply("not json").is_err());
    }

    #[test]
    fn progress_never_goes_backwards_or_past_the_total() {
        let mut p = PullProgress::default();
        p.apply(r#"{"digest":"a","total":100,"completed":60}"#)
            .unwrap();
        p.apply(r#"{"digest":"a","total":100,"completed":40}"#)
            .unwrap();
        assert_eq!(p.total().completed, 60);
        p.apply(r#"{"digest":"a","total":100,"completed":140}"#)
            .unwrap();
        assert_eq!(p.total().completed, 100);
        assert_eq!(per_mille(CleanupModelPull::default()), 0);
    }

    #[test]
    fn a_cancelled_download_can_not_write_or_clear_a_newer_one() {
        let mut pulls = Pulls::default();
        let p = |completed| CleanupModelPull {
            completed,
            total: 10,
        };
        let first = pulls.start(CleanupEngine::Ollama, "qwen3:8b");
        assert!(pulls.update(first, p(3)));
        pulls.cancel();
        assert!(!pulls.is_current(first));
        assert!(!pulls.update(first, p(4)));
        assert!(pulls.active.is_none());

        let second = pulls.start(CleanupEngine::Builtin, "Qwen3 4B Instruct");
        assert!(!pulls.update(first, p(5)));
        pulls.finish(first);
        assert!(pulls.builtin_running());
        assert_eq!(
            pulls
                .active
                .as_ref()
                .map(|a| (a.model.as_str(), a.progress)),
            Some(("Qwen3 4B Instruct", CleanupModelPull::default()))
        );
        assert!(pulls.update(second, p(6)));
        assert!(!pulls.verifying(first));
        assert!(pulls.verifying(second));
        assert!(pulls.active.as_ref().is_some_and(|a| a.verifying));
        pulls.finish(second);
        assert!(pulls.active.is_none());
        assert!(!pulls.is_current(second));
    }

    #[test]
    fn existing_ollama_users_keep_ollama_and_everyone_else_gets_built_in() {
        use BuiltinFile::{Downloading, Missing, Ready};
        use CleanupEngine::{Builtin, Ollama};
        assert_eq!(default_engine(Missing, Some(true)), (Ollama, true));
        assert_eq!(default_engine(Ready, Some(true)), (Builtin, true));
        assert_eq!(default_engine(Ready, None), (Builtin, true));
        // Not settled: built-in is offered and Ollama asked again later.
        assert_eq!(default_engine(Missing, Some(false)), (Builtin, false));
        assert_eq!(default_engine(Missing, None), (Builtin, false));
        // A download in progress is not saved as the choice yet.
        assert_eq!(default_engine(Downloading, Some(true)), (Builtin, false));
        assert_eq!(default_engine(Downloading, None), (Builtin, false));
    }

    #[test]
    fn free_disk_space_is_read() {
        let free = free_disk(&std::env::temp_dir());
        assert!(!cfg!(unix) || free.is_some_and(|b| b > 0));
    }

    #[test]
    fn never_pulls_the_thinking_qwen3_4b() {
        assert!(is_thinking_model("qwen3:4b"));
        assert!(is_thinking_model(" qwen3:4b "));
        assert!(!is_thinking_model("qwen3:4b-instruct"));
        assert!(!is_thinking_model("qwen3:8b"));
    }
}
