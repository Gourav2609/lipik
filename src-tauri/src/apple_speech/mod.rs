//! Apple's on-device dictation (DictationTranscriber, macOS 26+) as a second
//! speech engine. The Swift API runs in a small helper process
//! (`src-tauri/apple-speech/main.swift`) that the app keeps running between
//! dictations; see README.md here for the protocol and the engine choice.

pub mod commands;
pub mod pick;

use anyhow::Result;
use log::{debug, info, warn};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use specta::Type;
use std::io::{BufRead, BufReader, Write};
use std::path::PathBuf;
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{mpsc, Arc, Mutex, TryLockError};
use std::thread;
use std::time::{Duration, Instant};

const HELPER_NAME: &str = "apple-speech";
const START_TIMEOUT: Duration = Duration::from_secs(10);
const PREPARE_TIMEOUT: Duration = Duration::from_secs(30);
const DOWNLOAD_TIMEOUT: Duration = Duration::from_secs(600);
const WARM_UP_SAMPLES: usize = 8_000;
const QUEUE_POLL: Duration = Duration::from_millis(20);

#[derive(Serialize, Deserialize, Debug, Clone, Type)]
pub struct AppleSpeechStatus {
    /// The helper ships with this build and the OS has DictationTranscriber.
    pub supported: bool,
    /// The locale Apple resolved the setting to, once it is ready.
    pub locale: Option<String>,
    pub ready: bool,
    /// Why it is not ready: "unsupported_os", "unsupported_locale",
    /// "assets_missing", "helper_missing", "busy" or "failed".
    pub code: Option<String>,
    pub error: Option<String>,
}

struct Helper {
    stdin: ChildStdin,
    lines: mpsc::Receiver<String>,
    next_id: u64,
}

/// Lets a caller give up on one transcription. `skip` drops it if it has not
/// reached the helper yet; `kill` also stops the helper if it is running it.
#[derive(Default)]
pub struct JobControl {
    pub skip: AtomicBool,
    pub kill: AtomicBool,
}

#[derive(Debug)]
pub struct HelperFailure {
    pub code: String,
    pub message: String,
}

impl std::fmt::Display for HelperFailure {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{} ({})", self.message, self.code)
    }
}

impl std::error::Error for HelperFailure {}

fn failure(code: &str, message: impl Into<String>) -> anyhow::Error {
    HelperFailure {
        code: code.to_string(),
        message: message.into(),
    }
    .into()
}

/// The helper process, started on first use and restarted after a crash or
/// a request that took too long. Requests are serialised. The child sits in
/// its own lock so it can be stopped while a request is waiting on it.
#[derive(Clone, Default)]
pub struct AppleSpeech {
    helper: Arc<Mutex<Option<Helper>>>,
    child: Arc<Mutex<Option<Child>>>,
    unsupported: Arc<Mutex<Option<(String, String)>>>,
}

fn helper_path() -> Option<PathBuf> {
    let path = std::env::current_exe().ok()?.parent()?.join(HELPER_NAME);
    path.is_file().then_some(path)
}

impl AppleSpeech {
    pub fn new() -> Self {
        Self::default()
    }

    /// False once the helper is known to be missing or the OS too old; never
    /// starts the helper.
    pub fn maybe_supported(&self) -> bool {
        cfg!(target_os = "macos") && self.unsupported.lock().unwrap().is_none()
    }

    fn kill_child(&self) {
        if let Some(mut child) = self.child.lock().unwrap().take() {
            let _ = child.kill();
            let _ = child.wait();
        }
    }

    fn reset(&self, slot: &mut Option<Helper>) {
        *slot = None;
        self.kill_child();
    }

    fn spawn(&self) -> Result<Helper> {
        let path = helper_path()
            .ok_or_else(|| failure("helper_missing", "Apple speech helper is not bundled"))?;
        let mut child = Command::new(&path)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|e| failure("failed", format!("Could not start {}: {e}", path.display())))?;
        let stdin = child.stdin.take().expect("piped stdin");
        let stdout = child.stdout.take().expect("piped stdout");
        let pid = child.id();
        self.kill_child();
        *self.child.lock().unwrap() = Some(child);
        let (tx, lines) = mpsc::channel();
        thread::Builder::new()
            .name("apple-speech-reader".into())
            .spawn(move || {
                for line in BufReader::new(stdout).lines() {
                    let Ok(line) = line else { break };
                    if tx.send(line).is_err() {
                        break;
                    }
                }
            })
            .inspect_err(|_| self.kill_child())?;
        let helper = Helper {
            stdin,
            lines,
            next_id: 1,
        };
        let hello = match helper.lines.recv_timeout(START_TIMEOUT) {
            Ok(line) => line,
            Err(mpsc::RecvTimeoutError::Disconnected) => {
                self.kill_child();
                return Err(failure(
                    "unsupported_os",
                    "Apple speech helper cannot run on this macOS",
                ));
            }
            Err(mpsc::RecvTimeoutError::Timeout) => {
                self.kill_child();
                return Err(failure("failed", "Apple speech helper did not start"));
            }
        };
        let hello: Value = serde_json::from_str(&hello)?;
        if hello["supported"] != Value::Bool(true) {
            self.kill_child();
            return Err(failure(
                "unsupported_os",
                "Apple on-device dictation needs macOS 26 or later",
            ));
        }
        info!("Apple speech helper started (pid {pid})");
        Ok(helper)
    }

    /// Sends one request. `timeout` counts from the call, so time spent
    /// waiting behind another request counts too.
    fn request(
        &self,
        header: Value,
        pcm: &[f32],
        timeout: Duration,
        control: Option<&JobControl>,
    ) -> Result<Value> {
        if let Some((code, message)) = self.unsupported.lock().unwrap().clone() {
            return Err(failure(&code, message));
        }
        let deadline = Instant::now() + timeout;
        let given_up = || {
            control.is_some_and(|c| c.skip.load(Ordering::SeqCst) || c.kill.load(Ordering::SeqCst))
        };
        let mut slot = loop {
            if given_up() {
                return Err(failure("cancelled", "Apple speech request dropped"));
            }
            match self.helper.try_lock() {
                Ok(slot) => break slot,
                Err(TryLockError::Poisoned(e)) => break e.into_inner(),
                Err(TryLockError::WouldBlock) => {}
            }
            if Instant::now() >= deadline {
                return Err(failure("busy", "Apple speech is busy with another request"));
            }
            thread::sleep(QUEUE_POLL);
        };
        if slot.is_none() {
            match self.spawn() {
                Ok(helper) => *slot = Some(helper),
                Err(e) => {
                    if let Some(f) = e.downcast_ref::<HelperFailure>() {
                        if f.code == "unsupported_os" || f.code == "helper_missing" {
                            *self.unsupported.lock().unwrap() =
                                Some((f.code.clone(), f.message.clone()));
                        }
                    }
                    return Err(e);
                }
            }
        }
        let helper = slot.as_mut().expect("helper started");
        let id = helper.next_id;
        helper.next_id += 1;
        let mut header = header;
        header["id"] = json!(id);
        let mut bytes = serde_json::to_vec(&header)?;
        bytes.push(b'\n');
        bytes.reserve(pcm.len() * 4);
        for sample in pcm {
            bytes.extend_from_slice(&sample.to_le_bytes());
        }
        if let Err(e) = helper.stdin.write_all(&bytes).and_then(|_| helper.stdin.flush()) {
            self.reset(&mut slot);
            return Err(failure("failed", format!("Apple speech helper exited: {e}")));
        }
        loop {
            if control.is_some_and(|c| c.kill.load(Ordering::SeqCst)) {
                self.reset(&mut slot);
                return Err(failure("cancelled", "Apple speech request cancelled"));
            }
            let left = deadline.saturating_duration_since(Instant::now());
            let wait = if control.is_some() { left.min(QUEUE_POLL) } else { left };
            match helper.lines.recv_timeout(wait) {
                Ok(line) => {
                    let reply: Value = match serde_json::from_str(&line) {
                        Ok(v) => v,
                        Err(_) => continue,
                    };
                    if reply["id"].as_u64() != Some(id) {
                        continue;
                    }
                    if reply["ok"] == Value::Bool(true) {
                        return Ok(reply);
                    }
                    return Err(failure(
                        reply["code"].as_str().unwrap_or("failed"),
                        reply["error"].as_str().unwrap_or("Apple speech failed"),
                    ));
                }
                Err(mpsc::RecvTimeoutError::Timeout) if Instant::now() < deadline => {}
                Err(mpsc::RecvTimeoutError::Timeout) => {
                    warn!("Apple speech helper timed out after {:?}; restarting it", timeout);
                    self.reset(&mut slot);
                    return Err(failure("failed", "Apple speech timed out"));
                }
                Err(mpsc::RecvTimeoutError::Disconnected) => {
                    self.reset(&mut slot);
                    return Err(failure("failed", "Apple speech helper exited"));
                }
            }
        }
    }

    /// Checks the locale and its on-device assets, optionally downloading
    /// them (needs a network), then warms the model with a short silent run.
    pub fn prepare(&self, locale: &str, download: bool) -> Result<String> {
        let timeout = if download {
            DOWNLOAD_TIMEOUT
        } else {
            PREPARE_TIMEOUT
        };
        let reply = self.request(
            json!({ "op": "prepare", "locale": locale, "download": download }),
            &[],
            timeout,
            None,
        )?;
        let resolved = reply["locale"].as_str().unwrap_or(locale).to_string();
        let started = Instant::now();
        self.transcribe(&vec![0.0; WARM_UP_SAMPLES], locale, &[], None)?;
        debug!(
            "Apple speech warmed for {} in {} ms",
            resolved,
            started.elapsed().as_millis()
        );
        Ok(resolved)
    }

    /// Transcribes 16 kHz mono audio. `words` are the user's dictionary
    /// words, passed to Apple as context.
    pub fn transcribe(
        &self,
        pcm: &[f32],
        locale: &str,
        words: &[String],
        control: Option<&JobControl>,
    ) -> Result<String> {
        let audio_secs = pcm.len() as f64 / 16_000.0;
        let timeout = PREPARE_TIMEOUT + Duration::from_secs_f64(audio_secs / 4.0);
        let reply = self.request(
            json!({ "op": "transcribe", "locale": locale, "samples": pcm.len(), "words": words }),
            pcm,
            timeout,
            control,
        )?;
        Ok(reply["text"].as_str().unwrap_or_default().to_string())
    }

    pub fn status(&self, locale: &str, download: bool) -> AppleSpeechStatus {
        match self.prepare(locale, download) {
            Ok(resolved) => AppleSpeechStatus {
                supported: true,
                locale: Some(resolved),
                ready: true,
                code: None,
                error: None,
            },
            Err(e) => {
                let (code, message) = match e.downcast_ref::<HelperFailure>() {
                    Some(f) => (f.code.clone(), f.message.clone()),
                    None => ("failed".to_string(), e.to_string()),
                };
                AppleSpeechStatus {
                    supported: !matches!(code.as_str(), "unsupported_os" | "helper_missing"),
                    locale: None,
                    ready: false,
                    code: Some(code),
                    error: Some(message),
                }
            }
        }
    }

    /// Stops the helper without waiting for a running request, which then
    /// fails; the next request starts it again.
    pub fn shutdown(&self) {
        self.kill_child();
        if let Ok(mut slot) = self.helper.try_lock() {
            slot.take();
        }
    }
}
