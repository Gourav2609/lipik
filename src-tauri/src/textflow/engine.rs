//! The built-in clean-up engine: llama.cpp's `llama-server`, shipped next to
//! the app binary (tauri externalBin, built by scripts/fetch-llama-server.sh)
//! and reached over its OpenAI-style API on a free 127.0.0.1 port. It runs as
//! a separate process because transcribe.cpp already links its own ggml, and
//! two copies in one process crash.
//!
//! The server starts when a dictation starts recording (or on the first
//! request), is started again if it has died, stops after an hour without
//! requests (as Ollama's keep-alive did) and stops when the app quits.

use once_cell::sync::Lazy;
use serde::Serialize;
use serde_json::{json, Value};
use std::net::TcpListener;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::Mutex;
use std::time::{Duration, Instant};

const BINARY_NAME: &str = "llama-server";
const IDLE_LIMIT: Duration = Duration::from_secs(60 * 60);
/// Loading the 8B model from a cold disk takes several seconds; the first
/// start also compiles the Metal shaders.
const START_TIMEOUT: Duration = Duration::from_secs(120);
const IDLE_CHECK: Duration = Duration::from_secs(30);
const START_BACKOFF: Duration = Duration::from_secs(5 * 60);

/// A GGUF file the built-in engine runs.
#[derive(Debug, PartialEq)]
pub struct BuiltinModel {
    pub name: &'static str,
    pub repo: &'static str,
    pub revision: &'static str,
    pub file: &'static str,
    pub sha256: &'static str,
    pub size: u64,
}

impl BuiltinModel {
    pub fn url(&self) -> String {
        format!(
            "https://huggingface.co/{}/resolve/{}/{}",
            self.repo, self.revision, self.file
        )
    }
}

/// Qwen's own GGUF of Qwen3-8B (Apache 2.0). A hybrid thinking model: the
/// requests and the server turn thinking off.
pub const QWEN3_8B: BuiltinModel = BuiltinModel {
    name: "Qwen3 8B",
    repo: "Qwen/Qwen3-8B-GGUF",
    revision: "7c41481f57cb95916b40956ab2f0b139b296d974",
    file: "Qwen3-8B-Q4_K_M.gguf",
    sha256: "d98cdcbd03e17ce47681435b5150e34c1417f50b5c0019dd560e4882c5745785",
    size: 5_027_783_488,
};

/// Unsloth's GGUF of Qwen3-4B-Instruct-2507 (Apache 2.0; Qwen publishes no
/// GGUF of it). Not plain Qwen3-4B, which thinks out loud.
pub const QWEN3_4B_INSTRUCT: BuiltinModel = BuiltinModel {
    name: "Qwen3 4B Instruct",
    repo: "unsloth/Qwen3-4B-Instruct-2507-GGUF",
    revision: "a06e946bb6b655725eafa393f4a9745d460374c9",
    file: "Qwen3-4B-Instruct-2507-Q4_K_M.gguf",
    sha256: "3605803b982cb64aead44f6c1b2ae36e3acdb41d8e46c8a94c6533bc4c67e597",
    size: 2_497_281_120,
};

/// Macs with at least this much memory get the 8B model.
const BIG_MODEL_MEMORY: u64 = 16 * 1024 * 1024 * 1024;

pub fn model_for_memory(bytes: u64) -> &'static BuiltinModel {
    if bytes >= BIG_MODEL_MEMORY {
        &QWEN3_8B
    } else {
        &QWEN3_4B_INSTRUCT
    }
}

/// The model for this Mac.
pub fn builtin_model() -> &'static BuiltinModel {
    model_for_memory(physical_memory())
}

#[cfg(target_os = "macos")]
fn physical_memory() -> u64 {
    let mut bytes: u64 = 0;
    let mut len = std::mem::size_of::<u64>();
    let ok = unsafe {
        libc::sysctlbyname(
            c"hw.memsize".as_ptr(),
            (&mut bytes as *mut u64).cast(),
            &mut len,
            std::ptr::null_mut(),
            0,
        )
    } == 0;
    if ok {
        bytes
    } else {
        0
    }
}

#[cfg(not(target_os = "macos"))]
fn physical_memory() -> u64 {
    0
}

/// The helper is built for macOS 13.3 and later (its minimum OS version).
#[cfg(target_os = "macos")]
fn os_supported() -> bool {
    let mut buf = [0u8; 32];
    let mut len = buf.len();
    let ok = unsafe {
        libc::sysctlbyname(
            c"kern.osproductversion".as_ptr(),
            buf.as_mut_ptr().cast(),
            &mut len,
            std::ptr::null_mut(),
            0,
        )
    } == 0;
    ok && version_at_least(
        std::str::from_utf8(&buf[..len])
            .unwrap_or("")
            .trim_end_matches('\0'),
        (13, 3),
    )
}

#[cfg(not(target_os = "macos"))]
fn os_supported() -> bool {
    false
}

fn version_at_least(version: &str, (major, minor): (u32, u32)) -> bool {
    let mut parts = version.trim().split('.').map(|p| p.parse::<u32>().ok());
    match (parts.next().flatten(), parts.next().flatten().or(Some(0))) {
        (Some(a), Some(b)) => (a, b) >= (major, minor),
        _ => false,
    }
}

/// Where downloaded clean-up models live: a folder inside the speech models
/// folder, which only lists files at its top level.
pub fn models_dir(app: &tauri::AppHandle) -> Option<PathBuf> {
    crate::portable::app_data_dir(app)
        .ok()
        .map(|dir| dir.join("models").join("cleanup"))
}

pub fn model_path(app: &tauri::AppHandle) -> Option<PathBuf> {
    models_dir(app).map(|dir| dir.join(builtin_model().file))
}

/// The bundled `llama-server`, next to the app binary.
pub fn binary_path() -> Option<PathBuf> {
    let path = std::env::current_exe().ok()?.parent()?.join(BINARY_NAME);
    path.is_file().then_some(path)
}

/// How to reach a running server.
#[derive(Clone, Debug, PartialEq)]
pub struct Conn {
    pub url: String,
    pub key: String,
    pub ctx: u32,
}

struct Server {
    child: Child,
    model: PathBuf,
    conn: Conn,
    ready: bool,
    spawned: Instant,
    last_used: Instant,
}

/// Why a server did not start, and whether it exited by itself (worth one
/// more try on another port).
struct StartError(String, bool);

impl Server {
    fn alive(&mut self) -> bool {
        matches!(self.child.try_wait(), Ok(None))
    }

    fn kill(mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

pub struct Engine {
    binary: Option<PathBuf>,
    idle_limit: Duration,
    start_timeout: Duration,
    /// How long a failed start is remembered, so a model that can't load is
    /// not loaded again on every recording.
    backoff: Duration,
    failed: Mutex<Option<(Instant, PathBuf, String)>>,
    server: Mutex<Option<Server>>,
    /// Held while a server starts, so two callers never start two servers.
    starting: tokio::sync::Mutex<()>,
}

pub static ENGINE: Lazy<Engine> =
    Lazy::new(|| Engine::new(binary_path().filter(|_| os_supported())));

/// The `llama-server` arguments. Flash attention where the GPU supports it,
/// one slot so the clean-up instructions stay cached between requests, no
/// host-memory prompt cache (it reserves up to 8 GB), no thinking, no
/// network or web page, the model's file name hidden behind an alias, and
/// no web page allowed to read its replies (CORS origin that never matches).
pub fn server_args(model: &Path, port: u16, ctx: u32) -> Vec<String> {
    let mut args: Vec<String> = vec!["--model".into(), model.to_string_lossy().into_owned()];
    args.extend(
        [
            "--host",
            "127.0.0.1",
            "--port",
            &port.to_string(),
            "--ctx-size",
            &ctx.to_string(),
            "--parallel",
            "1",
            "--flash-attn",
            "auto",
            "--cache-prompt",
            "--cache-ram",
            "0",
            "--reasoning",
            "off",
            "--reasoning-format",
            "deepseek",
            "--alias",
            "lipik-cleanup",
            "--cors-origins",
            "http://lipik.invalid",
            "--no-cors-credentials",
            "--no-webui",
            "--offline",
        ]
        .map(String::from),
    );
    args
}

fn free_port() -> std::io::Result<u16> {
    Ok(TcpListener::bind(("127.0.0.1", 0))?.local_addr()?.port())
}

fn new_key() -> std::io::Result<String> {
    use std::io::Read;
    let mut bytes = [0u8; 16];
    std::fs::File::open("/dev/urandom")?.read_exact(&mut bytes)?;
    Ok(bytes.iter().map(|b| format!("{b:02x}")).collect())
}

impl Engine {
    pub fn new(binary: Option<PathBuf>) -> Self {
        Self {
            binary,
            idle_limit: IDLE_LIMIT,
            start_timeout: START_TIMEOUT,
            backoff: START_BACKOFF,
            failed: Mutex::new(None),
            server: Mutex::new(None),
            starting: tokio::sync::Mutex::new(()),
        }
    }

    /// Whether this build has the engine and this macOS can run it.
    pub fn is_available(&self) -> bool {
        self.binary.is_some()
    }

    /// The running, loaded server for `model`, or `None`. Never starts one.
    pub fn loaded(&self, model: &Path) -> Option<Conn> {
        let mut slot = self.server.lock().unwrap();
        let server = slot.as_mut()?;
        if !server.alive() {
            log::warn!("Clean-up engine: llama-server stopped by itself");
            slot.take();
            return None;
        }
        (server.ready && server.model == model).then(|| server.conn.clone())
    }

    /// A running server for `model` with at least `ctx` tokens of context,
    /// started (or restarted) when needed. Counts as use for the idle timer.
    /// A server that is still loading keeps loading when the caller gives
    /// up, and the next call waits for it instead of starting another.
    pub async fn ensure(&self, model: &Path, ctx: u32) -> Result<Conn, String> {
        let _starting = self.starting.lock().await;
        let loading = {
            let mut slot = self.server.lock().unwrap();
            let usable = slot.as_mut().is_some_and(|server| {
                server.alive() && server.model == model && server.conn.ctx >= ctx
            });
            match slot.as_mut() {
                Some(server) if usable => {
                    if server.ready {
                        server.last_used = Instant::now();
                        return Ok(server.conn.clone());
                    }
                    Some((server.conn.clone(), server.spawned))
                }
                _ => {
                    if let Some(old) = slot.take() {
                        old.kill();
                    }
                    None
                }
            }
        };
        if let Some((conn, spawned)) = loading {
            return self.started(model, self.wait_ready(conn, spawned).await);
        }
        let binary = self
            .binary
            .clone()
            .ok_or("The clean-up engine is not part of this build")?;
        if !model.is_file() {
            return Err("The clean-up model isn't downloaded".to_string());
        }
        if let Some((at, failed_model, error)) = &*self.failed.lock().unwrap() {
            if failed_model == model && at.elapsed() < self.backoff {
                return Err(format!("{error}; it will be tried again in a few minutes"));
            }
        }
        // A port taken between the check and the server's bind makes the
        // server exit at once; one more port is tried.
        let mut result = Err(StartError(String::new(), false));
        for _ in 0..2 {
            let (conn, spawned) = self.spawn(&binary, model, ctx)?;
            result = self.wait_ready(conn, spawned).await;
            if !matches!(result, Err(StartError(_, true))) {
                break;
            }
        }
        self.started(model, result)
    }

    /// Remembers a failed start for the backoff, and forgets it on success.
    fn started(&self, model: &Path, result: Result<Conn, StartError>) -> Result<Conn, String> {
        let mut failed = self.failed.lock().unwrap();
        match result {
            Ok(conn) => {
                *failed = None;
                Ok(conn)
            }
            Err(StartError(e, _)) => {
                log::warn!("Clean-up engine did not start: {e}");
                *failed = Some((Instant::now(), model.to_path_buf(), e.clone()));
                Err(e)
            }
        }
    }

    fn spawn(&self, binary: &Path, model: &Path, ctx: u32) -> Result<(Conn, Instant), String> {
        let port = free_port().map_err(|e| format!("No free port: {e}"))?;
        let key = new_key().map_err(|e| format!("No random key for the clean-up engine: {e}"))?;
        let mut command = Command::new(binary);
        command
            .args(server_args(model, port, ctx))
            .env_clear()
            .env("LLAMA_API_KEY", &key)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        for var in ["HOME", "TMPDIR", "PATH"] {
            if let Some(value) = std::env::var_os(var) {
                command.env(var, value);
            }
        }
        let child = command
            .spawn()
            .map_err(|e| format!("Could not start the clean-up engine: {e}"))?;
        let conn = Conn {
            url: format!("http://127.0.0.1:{port}"),
            key,
            ctx,
        };
        let spawned = Instant::now();
        *self.server.lock().unwrap() = Some(Server {
            child,
            model: model.to_path_buf(),
            conn: conn.clone(),
            ready: false,
            spawned,
            last_used: spawned,
        });
        Ok((conn, spawned))
    }

    /// Waits until the server at `conn` answers `/health`, at most the start
    /// timeout after it was spawned; stops it after that.
    async fn wait_ready(&self, conn: Conn, spawned: Instant) -> Result<Conn, StartError> {
        let gone = || {
            StartError(
                "The clean-up engine was stopped while it started".into(),
                false,
            )
        };
        loop {
            {
                let mut slot = self.server.lock().unwrap();
                let Some(server) = slot.as_mut().filter(|s| s.conn == conn) else {
                    return Err(gone());
                };
                if let Ok(Some(status)) = server.child.try_wait() {
                    slot.take();
                    return Err(StartError(
                        format!("The clean-up engine exited ({status})"),
                        true,
                    ));
                }
            }
            if spawned.elapsed() > self.start_timeout {
                let mut slot = self.server.lock().unwrap();
                if slot.as_ref().is_some_and(|s| s.conn == conn) {
                    if let Some(server) = slot.take() {
                        server.kill();
                    }
                }
                return Err(StartError(
                    format!(
                        "The clean-up engine didn't load the model within {} s",
                        self.start_timeout.as_secs()
                    ),
                    false,
                ));
            }
            if health_ok(&conn).await {
                break;
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
        let mut slot = self.server.lock().unwrap();
        let server = slot.as_mut().filter(|s| s.conn == conn).ok_or_else(gone)?;
        server.ready = true;
        server.last_used = Instant::now();
        log::info!(
            "Clean-up engine ready in {:?} (context {})",
            spawned.elapsed(),
            conn.ctx
        );
        Ok(conn)
    }

    /// Marks a request, for the idle timer.
    pub fn touch(&self) {
        if let Some(server) = self.server.lock().unwrap().as_mut() {
            server.last_used = Instant::now();
        }
    }

    pub fn stop(&self) {
        if let Some(server) = self.server.lock().unwrap().take() {
            server.kill();
            log::info!("Clean-up engine stopped");
        }
    }

    /// Stops the server when it has had no request for the idle limit.
    pub fn stop_if_idle(&self) -> bool {
        let idle = {
            let mut slot = self.server.lock().unwrap();
            if slot
                .as_ref()
                .is_some_and(|s| s.last_used.elapsed() >= self.idle_limit)
            {
                slot.take()
            } else {
                None
            }
        };
        let Some(server) = idle else {
            return false;
        };
        server.kill();
        log::info!("Clean-up engine stopped after an idle hour");
        true
    }

    #[cfg(test)]
    fn pid(&self) -> Option<u32> {
        self.server.lock().unwrap().as_ref().map(|s| s.child.id())
    }
}

async fn health_ok(conn: &Conn) -> bool {
    super::llm::CLIENT
        .get(format!("{}/health", conn.url))
        .timeout(Duration::from_secs(2))
        .send()
        .await
        .is_ok_and(|r| r.status().is_success())
}

/// The body of a chat request: the clean-up's temperature and length cap,
/// the prompt kept in the server's cache for the next request, and Qwen3's
/// thinking turned off in its chat template.
pub fn chat_body<M: Serialize>(messages: &[M], max_tokens: Option<u32>) -> Value {
    let mut body = json!({
        "messages": messages,
        "stream": false,
        "temperature": 0.2,
        "cache_prompt": true,
        "chat_template_kwargs": { "enable_thinking": false },
    });
    if let Some(n) = max_tokens {
        body["max_tokens"] = json!(n);
    }
    body
}

/// The reply in a chat completion. With a length cap, a reply that stopped
/// at the cap is cut off and is an error.
pub fn reply_text(value: &Value, capped: bool) -> Result<String, String> {
    let choice = &value["choices"][0];
    if capped && choice["finish_reason"] == "length" {
        return Err("The clean-up engine's reply stopped at the length cap".to_string());
    }
    choice["message"]["content"]
        .as_str()
        .map(|s| super::llm::strip_think(s).trim().to_string())
        .ok_or_else(|| "The clean-up engine's response had no message".to_string())
}

/// One chat request to a running server.
pub async fn post_chat(conn: &Conn, body: &Value, capped: bool) -> Result<String, String> {
    let response = super::llm::CLIENT
        .post(format!("{}/v1/chat/completions", conn.url))
        .bearer_auth(&conn.key)
        .json(body)
        .send()
        .await
        .map_err(|e| format!("Clean-up engine request failed: {e}"))?;
    if !response.status().is_success() {
        return Err(format!("Clean-up engine returned {}", response.status()));
    }
    let value: Value = response
        .json()
        .await
        .map_err(|e| format!("Bad clean-up engine response: {e}"))?;
    reply_text(&value, capped)
}

/// Process ids of `binary` whose parent is gone (adopted by launchd), from
/// `ps -axo pid=,ppid=,comm=` output.
#[cfg(unix)]
fn orphans(ps: &str, binary: &Path) -> Vec<i32> {
    let binary = binary.to_string_lossy();
    ps.lines()
        .filter_map(|line| {
            let (pid, rest) = line.trim_start().split_once(char::is_whitespace)?;
            let (ppid, command) = rest.trim_start().split_once(char::is_whitespace)?;
            let pid = pid.parse::<i32>().ok()?;
            (ppid == "1" && command.trim() == binary).then_some(pid)
        })
        .collect()
}

/// Stops servers left running by an earlier run of this app that crashed.
#[cfg(unix)]
fn kill_orphans(binary: &Path) {
    let Ok(output) = Command::new("/bin/ps")
        .args(["-axo", "pid=,ppid=,comm="])
        .output()
    else {
        return;
    };
    for pid in orphans(&String::from_utf8_lossy(&output.stdout), binary) {
        log::info!("Clean-up engine: stopping a server left by an earlier run");
        unsafe {
            libc::kill(pid, libc::SIGTERM);
        }
    }
}

#[cfg(not(unix))]
fn kill_orphans(_binary: &Path) {}

/// At app start: clears servers an earlier crash left behind and starts the
/// idle timer.
pub fn init() {
    let engine: &'static Engine = &ENGINE;
    let Some(binary) = engine.binary.clone() else {
        return;
    };
    std::thread::Builder::new()
        .name("cleanup-engine-idle".into())
        .spawn(move || {
            kill_orphans(&binary);
            loop {
                std::thread::sleep(IDLE_CHECK);
                engine.stop_if_idle();
            }
        })
        .ok();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn picks_the_model_by_memory() {
        let gb = 1024 * 1024 * 1024;
        assert_eq!(model_for_memory(8 * gb), &QWEN3_4B_INSTRUCT);
        assert_eq!(model_for_memory(16 * gb - 1), &QWEN3_4B_INSTRUCT);
        assert_eq!(model_for_memory(16 * gb), &QWEN3_8B);
        assert_eq!(model_for_memory(24 * gb), &QWEN3_8B);
        assert_eq!(model_for_memory(0), &QWEN3_4B_INSTRUCT);
        assert!(QWEN3_8B.url().ends_with(
            "/Qwen/Qwen3-8B-GGUF/resolve/7c41481f57cb95916b40956ab2f0b139b296d974/Qwen3-8B-Q4_K_M.gguf"
        ));
    }

    #[test]
    fn needs_macos_13_3() {
        assert!(version_at_least("13.3", (13, 3)));
        assert!(version_at_least("13.3.1", (13, 3)));
        assert!(version_at_least("26.0", (13, 3)));
        assert!(version_at_least("14", (13, 3)));
        assert!(!version_at_least("13.2.1", (13, 3)));
        assert!(!version_at_least("12.7", (13, 3)));
        assert!(!version_at_least("", (13, 3)));
        assert!(!cfg!(target_os = "macos") || os_supported());
    }

    #[test]
    fn server_listens_on_localhost_with_one_cached_slot() {
        let args = server_args(Path::new("/m/q.gguf"), 4321, 4096);
        let joined = args.join(" ");
        assert!(joined.starts_with("--model /m/q.gguf --host 127.0.0.1 --port 4321"));
        for flag in [
            "--ctx-size 4096",
            "--parallel 1",
            "--flash-attn auto",
            "--cache-prompt",
            "--cache-ram 0",
            "--reasoning off",
            "--alias lipik-cleanup",
            "--cors-origins http://lipik.invalid",
            "--offline",
        ] {
            assert!(joined.contains(flag), "{flag} missing from {joined}");
        }
    }

    #[test]
    fn request_turns_thinking_off_and_keeps_the_prompt_cached() {
        let messages = [
            super::super::llm::Message::new("system", "Clean up."),
            super::super::llm::Message::new("user", "um hi"),
        ];
        let body = chat_body(&messages, Some(40));
        assert_eq!(
            body,
            json!({
                "messages": [
                    { "role": "system", "content": "Clean up." },
                    { "role": "user", "content": "um hi" },
                ],
                "stream": false,
                "temperature": 0.2,
                "cache_prompt": true,
                "chat_template_kwargs": { "enable_thinking": false },
                "max_tokens": 40,
            })
        );
        assert!(chat_body(&messages, None).get("max_tokens").is_none());
    }

    #[test]
    fn reads_the_reply_and_never_returns_thinking() {
        let reply = |content: &str, finish: &str| json!({ "choices": [{ "message": { "role": "assistant", "content": content }, "finish_reason": finish }] });
        assert_eq!(
            reply_text(&reply("Hi there.", "stop"), true),
            Ok("Hi there.".to_string())
        );
        assert_eq!(
            reply_text(&reply("<think>\nhmm\n</think>\n\nHi there.", "stop"), false),
            Ok("Hi there.".to_string())
        );
        assert!(reply_text(&reply("Hi th", "length"), true).is_err());
        assert_eq!(
            reply_text(&reply("Hi th", "length"), false),
            Ok("Hi th".to_string())
        );
        assert!(reply_text(&json!({ "choices": [] }), false).is_err());
    }

    #[cfg(unix)]
    #[test]
    fn finds_only_orphaned_copies_of_our_binary() {
        let ps = "  101     1 /Applications/Lipik.app/Contents/MacOS/llama-server\n\
                  \x20 102   500 /Applications/Lipik.app/Contents/MacOS/llama-server\n\
                  \x20 103     1 /opt/homebrew/bin/llama-server\n\
                  \x20 104     1 /Applications/My Apps/Lipik.app/Contents/MacOS/llama-server\n\
                  garbage line\n";
        assert_eq!(
            orphans(
                ps,
                Path::new("/Applications/Lipik.app/Contents/MacOS/llama-server")
            ),
            vec![101]
        );
        assert_eq!(
            orphans(
                ps,
                Path::new("/Applications/My Apps/Lipik.app/Contents/MacOS/llama-server")
            ),
            vec![104]
        );
    }

    /// A stand-in for llama-server: a Python HTTP server that answers
    /// /health after `delay` seconds and echoes chat requests back.
    #[cfg(unix)]
    mod fake_server {
        use super::*;
        use std::os::unix::fs::PermissionsExt;

        const SERVER: &str = r#"
import json, sys, time, os
from http.server import BaseHTTPRequestHandler, HTTPServer
port = int(sys.argv[sys.argv.index("--port") + 1])
ready_at = time.time() + DELAY
class H(BaseHTTPRequestHandler):
    def log_message(self, *a): pass
    def send(self, code, body):
        data = json.dumps(body).encode()
        self.send_response(code)
        self.send_header("Content-Type", "application/json")
        self.send_header("Content-Length", str(len(data)))
        self.end_headers()
        self.wfile.write(data)
    def do_GET(self):
        if time.time() < ready_at:
            self.send(503, {"error": {"message": "Loading model"}})
        else:
            self.send(200, {"status": "ok"})
    def do_POST(self):
        body = json.loads(self.rfile.read(int(self.headers["Content-Length"])))
        if self.headers.get("Authorization") != "Bearer " + os.environ["LLAMA_API_KEY"]:
            return self.send(401, {"error": "key"})
        echo = {"path": self.path, "body": body, "args": sys.argv[1:]}
        self.send(200, {"choices": [{"message": {"content": json.dumps(echo)}, "finish_reason": "stop"}]})
HTTPServer(("127.0.0.1", port), H).serve_forever()
"#;

        fn python() -> Option<PathBuf> {
            let out = Command::new("/bin/sh")
                .args(["-c", "command -v python3"])
                .output()
                .ok()?;
            let path = String::from_utf8_lossy(&out.stdout).trim().to_string();
            (!path.is_empty()).then(|| PathBuf::from(path))
        }

        fn script(dir: &Path, name: &str, body: &str) -> PathBuf {
            let path = dir.join(name);
            std::fs::write(&path, body).unwrap();
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
            path
        }

        struct Setup {
            _dir: tempfile::TempDir,
            model: PathBuf,
            server: PathBuf,
        }

        fn setup(delay_s: f32) -> Option<Setup> {
            let python = python()?;
            let dir = tempfile::tempdir().unwrap();
            let model = dir.path().join("model.gguf");
            std::fs::write(&model, b"GGUF").unwrap();
            let code = SERVER.replace("DELAY", &delay_s.to_string());
            std::fs::write(dir.path().join("server.py"), code).unwrap();
            let server = script(
                dir.path(),
                "llama-server",
                &format!(
                    "#!/bin/sh\nexec '{}' '{}' \"$@\"\n",
                    python.display(),
                    dir.path().join("server.py").display()
                ),
            );
            Some(Setup {
                _dir: dir,
                model,
                server,
            })
        }

        fn gone(pid: u32) -> bool {
            unsafe { libc::kill(pid as i32, 0) != 0 }
        }

        fn run<F: std::future::Future>(f: F) -> F::Output {
            tauri::async_runtime::block_on(f)
        }

        #[test]
        fn starts_waits_for_health_and_answers_requests() {
            let Some(s) = setup(0.3) else { return };
            let engine = Engine::new(Some(s.server.clone()));
            assert_eq!(engine.loaded(&s.model), None);
            let conn = run(engine.ensure(&s.model, 4096)).expect("server starts");
            assert_eq!(conn.ctx, 4096);
            assert_eq!(engine.loaded(&s.model), Some(conn.clone()));

            let messages = [super::super::super::llm::Message::new("user", "hi")];
            let reply = run(post_chat(&conn, &chat_body(&messages, Some(9)), true)).unwrap();
            let echo: Value = serde_json::from_str(&reply).unwrap();
            assert_eq!(echo["path"], "/v1/chat/completions");
            assert_eq!(echo["body"]["max_tokens"], 9);
            assert_eq!(echo["body"]["cache_prompt"], true);
            let args = echo["args"].as_array().unwrap();
            assert!(args.contains(&json!(s.model.to_string_lossy())));

            let again = run(engine.ensure(&s.model, 4096)).unwrap();
            assert_eq!(again, conn, "a running server is reused");

            let pid = engine.pid().unwrap();
            engine.stop();
            assert!(gone(pid));
            assert_eq!(engine.loaded(&s.model), None);
        }

        #[test]
        fn restarts_a_server_that_died_or_is_too_small() {
            let Some(s) = setup(0.0) else { return };
            let engine = Engine::new(Some(s.server.clone()));
            let first = run(engine.ensure(&s.model, 4096)).unwrap();
            let pid = engine.pid().unwrap();
            unsafe { libc::kill(pid as i32, libc::SIGKILL) };
            std::thread::sleep(Duration::from_millis(200));
            assert_eq!(engine.loaded(&s.model), None, "a dead server is noticed");

            let second = run(engine.ensure(&s.model, 4096)).unwrap();
            assert_ne!(engine.pid(), Some(pid));
            assert_ne!(second.key, first.key);

            let pid = engine.pid().unwrap();
            let larger = run(engine.ensure(&s.model, 16384)).unwrap();
            assert_eq!(larger.ctx, 16384);
            assert!(gone(pid), "the smaller server is stopped");
            engine.stop();
        }

        #[test]
        fn a_start_its_caller_gave_up_on_keeps_loading() {
            let Some(s) = setup(1.0) else { return };
            let engine = Engine::new(Some(s.server.clone()));
            let gave_up = run(async {
                tokio::time::timeout(Duration::from_millis(300), engine.ensure(&s.model, 4096))
                    .await
            });
            assert!(gave_up.is_err());
            let pid = engine.pid().expect("still loading");
            assert_eq!(engine.loaded(&s.model), None);
            run(engine.ensure(&s.model, 4096)).expect("the same server finishes loading");
            assert_eq!(engine.pid(), Some(pid));
            engine.stop();
        }

        #[test]
        fn stops_after_the_idle_limit() {
            let Some(s) = setup(0.0) else { return };
            let mut engine = Engine::new(Some(s.server.clone()));
            engine.idle_limit = Duration::from_millis(300);
            run(engine.ensure(&s.model, 4096)).unwrap();
            let pid = engine.pid().unwrap();
            assert!(!engine.stop_if_idle());
            std::thread::sleep(Duration::from_millis(200));
            engine.touch();
            std::thread::sleep(Duration::from_millis(200));
            assert!(!engine.stop_if_idle(), "a request resets the timer");
            std::thread::sleep(Duration::from_millis(200));
            assert!(engine.stop_if_idle());
            assert!(gone(pid));
        }

        #[test]
        fn a_server_that_never_loads_is_stopped() {
            let Some(s) = setup(30.0) else { return };
            let mut engine = Engine::new(Some(s.server.clone()));
            engine.start_timeout = Duration::from_millis(800);
            let err = run(engine.ensure(&s.model, 4096)).unwrap_err();
            assert!(err.contains("didn't load"), "{err}");
            assert_eq!(engine.pid(), None);
        }

        #[test]
        fn a_server_that_exits_is_an_error() {
            let dir = tempfile::tempdir().unwrap();
            let model = dir.path().join("model.gguf");
            std::fs::write(&model, b"GGUF").unwrap();
            let server = script(dir.path(), "llama-server", "#!/bin/sh\nexit 3\n");
            let engine = Engine::new(Some(server));
            let err = run(engine.ensure(&model, 4096)).unwrap_err();
            assert!(err.contains("exited"), "{err}");
            assert_eq!(engine.pid(), None);
        }

        #[test]
        fn a_failed_start_is_not_tried_again_at_once() {
            let dir = tempfile::tempdir().unwrap();
            let model = dir.path().join("model.gguf");
            std::fs::write(&model, b"GGUF").unwrap();
            let count = dir.path().join("starts");
            let server = script(
                dir.path(),
                "llama-server",
                &format!("#!/bin/sh\necho x >> '{}'\nexit 3\n", count.display()),
            );
            let mut engine = Engine::new(Some(server));
            engine.backoff = Duration::from_millis(400);
            let starts = || std::fs::read_to_string(&count).map_or(0, |s| s.lines().count());
            assert!(run(engine.ensure(&model, 4096)).is_err());
            assert_eq!(starts(), 2, "one more port is tried after an exit");
            let err = run(engine.ensure(&model, 4096)).unwrap_err();
            assert!(err.contains("tried again"), "{err}");
            assert_eq!(starts(), 2, "no start during the backoff");
            std::thread::sleep(Duration::from_millis(500));
            assert!(run(engine.ensure(&model, 4096)).is_err());
            assert_eq!(starts(), 4);
        }

        #[test]
        fn needs_the_binary_and_the_model() {
            let dir = tempfile::tempdir().unwrap();
            let missing = dir.path().join("none.gguf");
            let err = run(Engine::new(None).ensure(&missing, 4096)).unwrap_err();
            assert!(err.contains("not part of this build"), "{err}");
            let server = script(dir.path(), "llama-server", "#!/bin/sh\nexit 0\n");
            let err = run(Engine::new(Some(server)).ensure(&missing, 4096)).unwrap_err();
            assert!(err.contains("isn't downloaded"), "{err}");
        }
    }
}
