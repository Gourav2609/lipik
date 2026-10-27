//! Calls to the local model: the built-in engine (engine.rs) or Ollama.

use once_cell::sync::Lazy;
use serde::Serialize;
use serde_json::json;
use std::path::PathBuf;
use std::time::Duration;
use tauri::AppHandle;

use super::engine::{self, ENGINE};
use crate::settings::CleanupEngine;

/// Dictations fit a small context, which keeps the model smaller in memory;
/// the clean-up prompt with its examples is about 1,600 tokens. Longer text
/// gets the Notetaker's size. Ollama reloads the model (about 3 s) whenever a
/// request asks for a different size, so a request keeps a larger size the
/// model is already loaded with.
pub(super) const SMALL_CONTEXT: u32 = 4_096;
const LARGE_CONTEXT: u32 = 16_384;
const KEEP_ALIVE: &str = "1h";

pub(super) static CLIENT: Lazy<reqwest::Client> = Lazy::new(|| {
    reqwest::Client::builder()
        .connect_timeout(Duration::from_millis(500))
        .build()
        .unwrap_or_default()
});

/// Where clean-up requests go. Ollama's URL and model are shared with the
/// Notetaker settings; for the built-in engine `model` is the model's name.
pub struct Endpoint {
    pub(super) url: String,
    pub(super) model: String,
    pub(super) builtin: Option<PathBuf>,
}

#[cfg(test)]
impl Endpoint {
    pub fn local(model: &str) -> Self {
        Self {
            url: "http://127.0.0.1:11434".to_string(),
            model: model.to_string(),
            builtin: None,
        }
    }
}

fn ollama(notetaker: crate::notetaker::NotetakerSettings) -> Endpoint {
    Endpoint {
        url: notetaker.ollama_url.trim_end_matches('/').to_string(),
        model: notetaker.ollama_model,
        builtin: None,
    }
}

/// The Ollama endpoint from the settings, whatever the clean-up engine.
pub fn ollama_endpoint(app: &AppHandle) -> Endpoint {
    ollama(crate::settings::get_settings(app).notetaker)
}

/// The endpoint of the clean-up engine in use. The built-in one carries the
/// path of its model file (which may not be downloaded yet).
pub fn endpoint(app: &AppHandle) -> Endpoint {
    let settings = crate::settings::get_settings(app);
    if super::cleanup_model::engine_from(app, &settings) == CleanupEngine::Ollama {
        return ollama(settings.notetaker);
    }
    Endpoint {
        url: String::new(),
        model: engine::builtin_model().name.to_string(),
        builtin: Some(engine::model_path(app).unwrap_or_default()),
    }
}

/// One turn of a chat request.
#[derive(Serialize)]
pub struct Message {
    pub role: &'static str,
    pub content: String,
}

impl Message {
    pub fn new(role: &'static str, content: impl Into<String>) -> Self {
        Self {
            role,
            content: content.into(),
        }
    }
}

/// The context size a chat needs: about 3 characters per token, with room
/// for a reply 1.5 times the last (user) message.
pub fn messages_context(messages: &[Message]) -> u32 {
    let (user, earlier) = messages.split_last().map_or((0, 0), |(last, earlier)| {
        (
            last.content.len(),
            earlier.iter().map(|m| m.content.len()).sum(),
        )
    });
    let estimate = (earlier + user * 5 / 2) / 3 + 64;
    if estimate <= SMALL_CONTEXT as usize {
        SMALL_CONTEXT
    } else {
        LARGE_CONTEXT
    }
}

pub(super) fn strip_think(s: &str) -> &str {
    if let Some(rest) = s.trim_start().strip_prefix("<think>") {
        if let Some(end) = rest.find("</think>") {
            return rest[end + "</think>".len()..].trim_start();
        }
    }
    s
}

/// One chat turn; gives up after `timeout`. Uses the size the model is
/// loaded with when that is large enough, since asking for another size
/// reloads the model.
pub async fn chat(
    endpoint: &Endpoint,
    system: &str,
    user: &str,
    timeout: Duration,
) -> Result<String, String> {
    let messages = [Message::new("system", system), Message::new("user", user)];
    let needed = messages_context(&messages);
    let context = loaded_context(endpoint, needed)
        .await
        .map_or(needed, |n| n.max(needed));
    chat_messages(endpoint, &messages, context, timeout, None).await
}

/// One chat request with the given turns and context size, and at most
/// `max_tokens` in the reply when given.
pub async fn chat_messages(
    endpoint: &Endpoint,
    messages: &[Message],
    context: u32,
    timeout: Duration,
    max_tokens: Option<u32>,
) -> Result<String, String> {
    if let Some(model) = &endpoint.builtin {
        let request = async {
            let conn = ENGINE.ensure(model, context).await?;
            let reply = engine::post_chat(
                &conn,
                &engine::chat_body(messages, max_tokens),
                max_tokens.is_some(),
            )
            .await;
            ENGINE.touch();
            reply
        };
        return tokio::time::timeout(timeout, request)
            .await
            .map_err(|_| format!("The clean-up engine took longer than {timeout:?}"))?;
    }
    let mut options = json!({ "temperature": 0.2, "num_ctx": context });
    if let Some(n) = max_tokens {
        options["num_predict"] = json!(n);
    }
    let body = json!({
        "model": endpoint.model,
        "stream": false,
        "think": false,
        "keep_alive": KEEP_ALIVE,
        "options": options,
        "messages": messages,
    });
    let request = async {
        let response = CLIENT
            .post(format!("{}/api/chat", endpoint.url))
            .json(&body)
            .send()
            .await
            .map_err(|e| format!("Ollama request failed: {e}"))?;
        if !response.status().is_success() {
            return Err(format!("Ollama returned {}", response.status()));
        }
        let value: serde_json::Value = response
            .json()
            .await
            .map_err(|e| format!("Bad Ollama response: {e}"))?;
        reply_text(&value, max_tokens.is_some())
    };
    tokio::time::timeout(timeout, request)
        .await
        .map_err(|_| format!("Ollama took longer than {timeout:?}"))?
}

/// The reply in an Ollama chat response. With a length cap, a reply that
/// stopped at the cap is cut off and is an error.
fn reply_text(value: &serde_json::Value, capped: bool) -> Result<String, String> {
    if capped && value["done_reason"] == "length" {
        return Err("Ollama reply stopped at the length cap".to_string());
    }
    value["message"]["content"]
        .as_str()
        .map(|s| strip_think(s).trim().to_string())
        .ok_or_else(|| "Ollama response had no message".to_string())
}

/// The context size the model is loaded with, or `None` when it is not
/// loaded. Ollama versions that don't report the size give `needed`.
pub async fn loaded_context(endpoint: &Endpoint, needed: u32) -> Option<u32> {
    if let Some(model) = &endpoint.builtin {
        return ENGINE.loaded(model).map(|conn| conn.ctx);
    }
    let response = CLIENT
        .get(format!("{}/api/ps", endpoint.url))
        .timeout(Duration::from_millis(300))
        .send()
        .await
        .ok()?;
    let value = response.json::<serde_json::Value>().await.ok()?;
    let latest = format!("{}:latest", endpoint.model);
    value["models"].as_array()?.iter().find_map(|m| {
        let name = m["name"].as_str().unwrap_or_default();
        (name == endpoint.model || name == latest).then(|| {
            m["context_length"]
                .as_u64()
                .and_then(|n| u32::try_from(n).ok())
                .unwrap_or(needed)
        })
    })
}

/// Loads the model ahead of a request so the first clean-up is not slow. It
/// sends the clean-up instructions and examples too, so the engine keeps them
/// in its prompt cache and the real clean-up only reads the new words.
pub fn warm_up(app: &AppHandle) {
    let endpoint = endpoint(app);
    let messages = super::pipeline::warm_up_messages(app);
    if let Some(model) = endpoint.builtin {
        if !model.is_file() {
            return;
        }
        tauri::async_runtime::spawn(async move {
            let context = ENGINE
                .loaded(&model)
                .map_or(SMALL_CONTEXT, |conn| conn.ctx.max(SMALL_CONTEXT));
            let warm = async {
                let conn = ENGINE.ensure(&model, context).await?;
                engine::post_chat(&conn, &engine::chat_body(&messages, Some(1)), false).await
            };
            if let Err(e) = tokio::time::timeout(Duration::from_secs(180), warm)
                .await
                .unwrap_or_else(|_| Err("timed out".to_string()))
            {
                log::warn!("Clean-up engine warm-up failed: {e}");
            }
        });
        return;
    }
    tauri::async_runtime::spawn(async move {
        // Keep a larger size the model already has (after a Notetaker summary
        // or a long dictation) instead of reloading it smaller.
        let context = loaded_context(&endpoint, SMALL_CONTEXT)
            .await
            .map_or(SMALL_CONTEXT, |n| n.max(SMALL_CONTEXT));
        let _ = CLIENT
            .post(format!("{}/api/chat", endpoint.url))
            .json(&json!({
                "model": endpoint.model,
                "stream": false,
                "think": false,
                "keep_alive": KEEP_ALIVE,
                "options": { "num_ctx": context, "num_predict": 1 },
                "messages": messages,
            }))
            .timeout(Duration::from_secs(60))
            .send()
            .await;
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A reply cut off at the length cap is unfinished text, not a reply.
    #[test]
    fn reply_cut_at_the_cap_is_an_error() {
        let done = json!({ "message": { "content": "Hi there." }, "done_reason": "stop" });
        assert_eq!(reply_text(&done, true), Ok("Hi there.".to_string()));
        let cut = json!({ "message": { "content": "Hi th" }, "done_reason": "length" });
        assert!(reply_text(&cut, true).is_err());
    }
}
