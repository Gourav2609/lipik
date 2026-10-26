//! Checks that Ollama runs and has the clean-up model, and downloads the
//! model through Ollama for onboarding and Settings.

use super::llm::{self, CLIENT};
use futures_util::future::{select, Either};
use futures_util::StreamExt;
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
    Ready,
}

#[derive(Clone, Debug, Serialize, Type)]
pub struct CleanupModelStatus {
    pub state: CleanupModelState,
    /// The configured model, or while `downloading` the model being downloaded.
    pub model: String,
    /// Bytes so far while `state` is `downloading`.
    pub progress: Option<CleanupModelPull>,
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
    active: Option<(String, CleanupModelPull)>,
}

impl Pulls {
    fn start(&mut self, model: &str) -> u64 {
        self.generation += 1;
        self.active = Some((model.to_string(), CleanupModelPull::default()));
        self.generation
    }

    fn update(&mut self, generation: u64, progress: CleanupModelPull) -> bool {
        match &mut self.active {
            Some((_, p)) if self.generation == generation => {
                *p = progress;
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
}

static PULLS: Lazy<Mutex<Pulls>> = Lazy::new(Default::default);
static CANCEL: Lazy<Notify> = Lazy::new(Notify::new);

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

#[tauri::command]
#[specta::specta]
pub async fn cleanup_model_status(app: AppHandle) -> CleanupModelStatus {
    let endpoint = llm::endpoint(&app);
    let model = endpoint.model.clone();
    if let Some((pulling, progress)) = PULLS.lock().unwrap().active.clone() {
        return CleanupModelStatus {
            state: CleanupModelState::Downloading,
            model: pulling,
            progress: Some(progress),
        };
    }
    let tags = async {
        CLIENT
            .get(format!("{}/api/tags", endpoint.url))
            .timeout(Duration::from_millis(1500))
            .send()
            .await
            .ok()?
            .json::<Value>()
            .await
            .ok()
    }
    .await;
    let state = match tags {
        None => CleanupModelState::NoOllama,
        Some(_) if is_thinking_model(&model) => CleanupModelState::SlowModel,
        Some(tags) if has_model(&tags, &model) => CleanupModelState::Ready,
        Some(_) => CleanupModelState::NoModel,
    };
    CleanupModelStatus {
        state,
        model,
        progress: None,
    }
}

/// Downloads the clean-up model through Ollama and loads it. Progress comes
/// as `CleanupModelPull` events. `Ok(false)`: cancelled, or replaced by a
/// newer download.
#[tauri::command]
#[specta::specta]
pub async fn pull_cleanup_model(app: AppHandle) -> Result<bool, String> {
    let endpoint = llm::endpoint(&app);
    if is_thinking_model(&endpoint.model) {
        return Err(SLOW_MODEL.to_string());
    }
    let generation = PULLS.lock().unwrap().start(&endpoint.model);
    let result = pull(&app, &endpoint.url, &endpoint.model, generation).await;
    PULLS.lock().unwrap().finish(generation);
    match result {
        Ok(true) => {
            log::info!("cleanup model: {} downloaded", endpoint.model);
            if super::uses_llm(&app) {
                llm::warm_up(&app);
            }
            Ok(true)
        }
        Ok(false) => Ok(false),
        Err(e) => {
            log::warn!("cleanup model: download of {} failed: {e}", endpoint.model);
            Err(e)
        }
    }
}

#[tauri::command]
#[specta::specta]
pub fn cancel_cleanup_model_pull() {
    PULLS.lock().unwrap().cancel();
    CANCEL.notify_waiters();
}

/// Stops a running download of any model other than `model`, for when the
/// model setting changes.
pub fn cancel_pull_unless(model: &str) {
    let mut pulls = PULLS.lock().unwrap();
    if pulls
        .active
        .as_ref()
        .is_some_and(|(m, _)| m != model.trim())
    {
        pulls.cancel();
        drop(pulls);
        CANCEL.notify_waiters();
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
        let first = pulls.start("qwen3:8b");
        assert!(pulls.update(first, p(3)));
        pulls.cancel();
        assert!(!pulls.is_current(first));
        assert!(!pulls.update(first, p(4)));
        assert!(pulls.active.is_none());

        let second = pulls.start("qwen3:4b-instruct");
        assert!(!pulls.update(first, p(5)));
        pulls.finish(first);
        assert_eq!(
            pulls.active,
            Some(("qwen3:4b-instruct".to_string(), CleanupModelPull::default()))
        );
        assert!(pulls.update(second, p(6)));
        pulls.finish(second);
        assert!(pulls.active.is_none());
        assert!(!pulls.is_current(second));
    }

    #[test]
    fn never_pulls_the_thinking_qwen3_4b() {
        assert!(is_thinking_model("qwen3:4b"));
        assert!(is_thinking_model(" qwen3:4b "));
        assert!(!is_thinking_model("qwen3:4b-instruct"));
        assert!(!is_thinking_model("qwen3:8b"));
    }
}
