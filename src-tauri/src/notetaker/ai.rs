use anyhow::{anyhow, bail, Result};
use serde_json::{json, Value};
use std::collections::HashSet;
use std::path::PathBuf;
use std::time::Duration;

use crate::settings::CleanupEngine;
use crate::textflow::engine::{self, ENGINE};

use super::diarize::{self, SpeakerGuess};
use super::{ActionItem, Attendee, Citation, Meeting, NoteSection, NoteSummary, Segment};

const CONTEXT_TOKENS: u32 = 16_384;
/// Transcript tokens per summary call, leaving room for the instructions,
/// the user's thoughts and the reply.
const PART_TOKENS: usize = 9_000;
/// "My notes" sent with each summary call.
const THOUGHTS_CHARS: usize = 4_000;
const ASK_CONTEXT_CHARS: usize = 24_000;
/// Longest wait for one short plain-text reply.
const TEXT_TIMEOUT: Duration = Duration::from_secs(20);

/// Shown while the built-in model downloads; notes are written once it lands.
pub const NOTES_WAIT_FOR_MODEL: &str =
    "The AI model is still downloading. Notes will be written by themselves when it's ready.";

/// The model that writes notes and answers questions: the built-in engine
/// or Ollama, whichever the engine setting picks for clean-up.
pub enum Model {
    Ollama(Ollama),
    Builtin { path: PathBuf, name: String },
}

impl Model {
    pub fn for_app(app: &tauri::AppHandle) -> Self {
        let settings = crate::settings::get_settings(app);
        match crate::textflow::cleanup_model::engine_from(app, &settings) {
            // Clean-up still undecided and the built-in model on its way:
            // notes wait for it rather than blaming a missing Ollama.
            CleanupEngine::Ollama
                if settings.cleanup_engine.is_none()
                    && crate::textflow::cleanup_model::builtin_downloading() =>
            {
                Model::Builtin {
                    path: engine::model_path(app).unwrap_or_default(),
                    name: engine::builtin_model().name.to_string(),
                }
            }
            CleanupEngine::Ollama => Model::Ollama(Ollama::new(
                &settings.notetaker.ollama_url,
                &settings.notetaker.ollama_model,
            )),
            CleanupEngine::Builtin => Model::Builtin {
                path: engine::model_path(app).unwrap_or_default(),
                name: engine::builtin_model().name.to_string(),
            },
        }
    }

    pub fn model(&self) -> &str {
        match self {
            Model::Ollama(ollama) => ollama.model(),
            Model::Builtin { name, .. } => name,
        }
    }

    /// Why notes can't be written right now, or `None` when the model is
    /// there.
    pub fn unavailable_reason(&self) -> Option<String> {
        match self {
            Model::Ollama(ollama) => ollama.unavailable_reason(),
            Model::Builtin { path, .. } if !path.is_file() => {
                Some(if crate::textflow::cleanup_model::builtin_downloading() {
                    NOTES_WAIT_FOR_MODEL.to_string()
                } else {
                    "The AI model isn't downloaded yet. Download it in Settings → Meetings."
                        .to_string()
                })
            }
            Model::Builtin { .. } => None,
        }
    }

    /// A plain-text reply to one user message, within 20 s. Asking for JSON
    /// made the model translate Hinglish lines into English far more often.
    pub(super) fn chat_text(&self, user: &str) -> Result<String> {
        let path = match self {
            Model::Ollama(ollama) => return ollama.chat_text(user),
            Model::Builtin { path, .. } => path,
        };
        let body = engine::chat_body(&[json!({ "role": "user", "content": user })], None);
        let content = tauri::async_runtime::block_on(async {
            let request = async {
                let conn = ENGINE.ensure(path, CONTEXT_TOKENS).await?;
                let reply = engine::post_chat(&conn, &body, false).await;
                ENGINE.touch();
                reply
            };
            tokio::time::timeout(TEXT_TIMEOUT, request)
                .await
                .map_err(|_| "The AI model took longer than 20 seconds".to_string())?
        })
        .map_err(|e| anyhow!(e))?;
        Ok(without_thinking(&content).to_string())
    }

    fn chat_json(&self, system: &str, user: &str) -> Result<Value> {
        let path = match self {
            Model::Ollama(ollama) => return ollama.chat_json(system, user),
            Model::Builtin { path, .. } => path,
        };
        let body = builtin_body(system, user);
        let content = tauri::async_runtime::block_on(async {
            let request = async {
                let conn = ENGINE.ensure(path, CONTEXT_TOKENS).await?;
                let reply = engine::post_chat(&conn, &body, false).await;
                ENGINE.touch();
                reply
            };
            tokio::time::timeout(Duration::from_secs(600), request)
                .await
                .map_err(|_| "The AI model took longer than 10 minutes".to_string())?
        })
        .map_err(|e| anyhow!(e))?;
        parse_json_reply(&content)
    }
}

/// A notes request to the built-in engine: the clean-up request's settings
/// with the reply held to one JSON object.
fn builtin_body(system: &str, user: &str) -> Value {
    let messages = [
        json!({ "role": "system", "content": system }),
        json!({ "role": "user", "content": user }),
    ];
    let mut body = engine::chat_body(&messages, None);
    body["response_format"] = json!({ "type": "json_object" });
    body
}

pub struct Ollama {
    url: String,
    model: String,
}

impl Ollama {
    pub fn new(url: &str, model: &str) -> Self {
        Self {
            url: url.trim_end_matches('/').to_string(),
            model: model.to_string(),
        }
    }

    pub fn model(&self) -> &str {
        &self.model
    }

    /// Why notes can't be written right now, or `None` when Ollama is up and
    /// has the model.
    pub fn unavailable_reason(&self) -> Option<String> {
        let url = format!("{}/api/tags", self.url);
        let result: Result<Value> = tauri::async_runtime::block_on(async {
            let client = reqwest::Client::builder()
                .timeout(Duration::from_secs(3))
                .build()?;
            Ok(client
                .get(&url)
                .send()
                .await?
                .error_for_status()?
                .json()
                .await?)
        });
        let tags = match result {
            Ok(tags) => tags,
            Err(_) => {
                return Some(format!(
                    "Ollama is not running at {}. Start it (ollama serve) and install the model (ollama pull {}).",
                    self.url, self.model
                ))
            }
        };
        let wanted = self.model.trim_end_matches(":latest");
        let installed = tags["models"].as_array().into_iter().flatten().any(|m| {
            m["name"]
                .as_str()
                .is_some_and(|n| n == self.model || n.trim_end_matches(":latest") == wanted)
        });
        (!installed).then(|| {
            format!(
                "The model {} is not installed in Ollama. Run: ollama pull {}",
                self.model, self.model
            )
        })
    }

    fn chat_text(&self, user: &str) -> Result<String> {
        let body = json!({
            "model": self.model,
            "stream": false,
            "think": false,
            "options": { "temperature": 0.2 },
            "messages": [{ "role": "user", "content": user }],
        });
        let url = format!("{}/api/chat", self.url);
        let response: Value = tauri::async_runtime::block_on(async {
            let client = reqwest::Client::builder().timeout(TEXT_TIMEOUT).build()?;
            let response = client.post(&url).json(&body).send().await?;
            let status = response.status();
            let value: Value = response.json().await?;
            if !status.is_success() {
                bail!(
                    "Ollama returned {status}: {}",
                    value["error"].as_str().unwrap_or("unknown error")
                );
            }
            Ok::<Value, anyhow::Error>(value)
        })?;
        let content = response["message"]["content"]
            .as_str()
            .ok_or_else(|| anyhow!("Ollama sent no message"))?;
        Ok(without_thinking(content).to_string())
    }

    fn chat_json(&self, system: &str, user: &str) -> Result<Value> {
        let body = json!({
            "model": self.model,
            "stream": false,
            "think": false,
            "format": "json",
            "options": { "temperature": 0.2, "num_ctx": CONTEXT_TOKENS },
            "messages": [
                { "role": "system", "content": system },
                { "role": "user", "content": user },
            ],
        });
        let url = format!("{}/api/chat", self.url);
        let response: Value = tauri::async_runtime::block_on(async {
            let client = reqwest::Client::builder()
                .timeout(Duration::from_secs(600))
                .build()?;
            let response = client.post(&url).json(&body).send().await?;
            let status = response.status();
            let value: Value = response.json().await?;
            if !status.is_success() {
                bail!(
                    "Ollama returned {status}: {}",
                    value["error"].as_str().unwrap_or("unknown error")
                );
            }
            Ok::<Value, anyhow::Error>(value)
        })?;
        if response["prompt_eval_count"].as_u64().unwrap_or(0) >= CONTEXT_TOKENS as u64 - 16 {
            log::warn!("Notetaker prompt filled the Ollama context; its start may have been cut");
        }
        let content = response["message"]["content"]
            .as_str()
            .ok_or_else(|| anyhow!("Ollama sent no message"))?;
        parse_json_reply(content)
    }
}

fn without_thinking(content: &str) -> &str {
    match content.find("</think>") {
        Some(end) => &content[end + "</think>".len()..],
        None => content,
    }
}

fn parse_json_reply(content: &str) -> Result<Value> {
    let text = without_thinking(content);
    let start = text.find('{').ok_or_else(|| anyhow!("Reply had no JSON"))?;
    let end = text
        .rfind('}')
        .ok_or_else(|| anyhow!("Reply had no JSON"))?;
    Ok(serde_json::from_str(&text[start..=end])?)
}

pub fn clock(ms: i64) -> String {
    let s = ms.max(0) / 1000;
    if s >= 3600 {
        format!("{}:{:02}:{:02}", s / 3600, s / 60 % 60, s % 60)
    } else {
        format!("{:02}:{:02}", s / 60, s % 60)
    }
}

fn me_name(attendees: &[Attendee]) -> Option<String> {
    attendees
        .iter()
        .find(|a| a.is_me)
        .and_then(diarize::display_name)
}

fn transcript_lines(segments: &[Segment]) -> Vec<String> {
    segments
        .iter()
        .enumerate()
        .map(|(i, s)| format!("#{i} [{}] {}: {}", clock(s.start_ms), s.speaker, s.text))
        .collect()
}

/// A cautious token count: English runs about 4 characters per token, while
/// other scripts (Devanagari, CJK) can take a token or more per character.
fn approx_tokens(text: &str) -> usize {
    let (ascii, other) =
        text.chars().fold(
            (0, 0),
            |(a, o), c| if c.is_ascii() { (a + 1, o) } else { (a, o + 1) },
        );
    ascii / 4 + other + 1
}

fn split_parts(lines: &[String]) -> Vec<String> {
    let mut parts = vec![String::new()];
    let mut tokens = 0;
    for line in lines {
        let line_tokens = approx_tokens(line);
        if tokens > 0 && tokens + line_tokens > PART_TOKENS {
            parts.push(String::new());
            tokens = 0;
        }
        let current = parts.last_mut().unwrap();
        current.push_str(line);
        current.push('\n');
        tokens += line_tokens;
    }
    parts
}

fn capped_thoughts(thoughts: &str) -> String {
    let thoughts = thoughts.trim();
    match thoughts.char_indices().nth(THOUGHTS_CHARS) {
        Some((end, _)) => format!("{}…", &thoughts[..end]),
        None => thoughts.to_string(),
    }
}

const NOTES_SYSTEM: &str = "You write meeting notes from a transcript. Lines look like '#index [time] speaker: text'. \
'Me' is the user who recorded the meeting. 'Speaker 1', 'Speaker 2' and so on are different people heard through \
the computer's speakers, told apart by voice; 'Them' is everyone heard through the speakers when they could not be told apart. \
Write in the language the meeting was mostly held in. Be concrete: names, numbers, dates and decisions as said. \
Never invent facts that are not in the transcript. Reply with JSON only.";

const NOTES_SHAPE: &str = r#"{
  "title": "short title naming what was discussed (max 8 words), never just the app or the word meeting",
  "overview": "2-4 sentences: what the meeting was about and where it landed",
  "sections": [{"heading": "topic", "bullets": ["what was said about it"]}],
  "key_points": ["the most important points"],
  "decisions": ["decisions that were made"],
  "action_items": [{"text": "what has to be done", "owner": "person or null", "due": "when, or null"}],
  "speakers": [{"speaker": "Speaker 2", "name": "the person's name", "lines": [12, 30]}]
}"#;

fn notes_prompt(
    meeting: &Meeting,
    thoughts: &str,
    transcript: &str,
    part: Option<(usize, usize)>,
) -> String {
    let attendees = diarize::invitees(&meeting.attendees);
    // A made-up name ("Slack call") would only be copied back as the title.
    let name = if meeting.title_auto {
        "untitled; name it after what was discussed"
    } else {
        meeting.title.as_str()
    };
    let mut prompt = format!(
        "Meeting: {}\nDate: {}\n",
        name,
        chrono::DateTime::from_timestamp_millis(meeting.start_ms)
            .map(|d| d
                .with_timezone(&chrono::Local)
                .format("%A %-d %B %Y, %H:%M")
                .to_string())
            .unwrap_or_default()
    );
    if let Some(me) = me_name(&meeting.attendees) {
        prompt.push_str(&format!("'Me' is {me}.\n"));
    }
    if !attendees.is_empty() {
        prompt.push_str(&format!("Invited: {}\n", attendees.join(", ")));
    }
    if let Some((i, n)) = part {
        prompt.push_str(&format!("This is part {i} of {n} of the transcript.\n"));
    }
    if !thoughts.trim().is_empty() {
        prompt.push_str(&format!(
            "\nThe user's own notes from the meeting (cover these topics first):\n{}\n",
            capped_thoughts(thoughts)
        ));
    }
    prompt.push_str(&format!(
        "\nTranscript:\n{transcript}\n\
For 'speakers', only name a 'Speaker N' when the transcript makes it clear who that is: \
someone says their name right before they speak, or they introduce themselves. \
'lines' are the line numbers (#index) where that name is said. Never name 'Me' or 'Them'. \
Use names from the invite list when they match, even if the transcript spells them differently. Leave out anyone you are not sure about.\n\
Reply with JSON in this shape:\n{NOTES_SHAPE}"
    ));
    prompt
}

/// What one reply holds. Each field is read on its own, so a `null` or an
/// odd value loses only that field.
struct RawNotes {
    summary: NoteSummary,
    speakers: Vec<SpeakerGuess>,
}

fn text(v: &Value) -> Option<String> {
    v.as_str()
        .map(str::trim)
        .filter(|s| !s.is_empty() && *s != "null" && *s != "None")
        .map(str::to_string)
}

fn texts(v: &Value) -> Vec<String> {
    v.as_array()
        .into_iter()
        .flatten()
        .filter_map(text)
        .collect()
}

fn items(v: &Value) -> impl Iterator<Item = &Value> {
    v.as_array().into_iter().flatten()
}

/// `12`, `"12"` or `"#12"`: the model may copy the transcript's own form.
fn segment_index(v: &Value) -> Option<usize> {
    v.as_u64()
        .map(|n| n as usize)
        .or_else(|| v.as_str()?.trim().trim_start_matches('#').parse().ok())
}

fn raw_notes(v: &Value) -> RawNotes {
    let summary = NoteSummary {
        title: text(&v["title"]),
        overview: text(&v["overview"]).unwrap_or_default(),
        sections: items(&v["sections"])
            .filter_map(|s| {
                let heading = text(&s["heading"]).unwrap_or_default();
                let bullets = texts(&s["bullets"]);
                (!heading.is_empty() || !bullets.is_empty())
                    .then_some(NoteSection { heading, bullets })
            })
            .collect(),
        key_points: texts(&v["key_points"]),
        decisions: texts(&v["decisions"]),
        action_items: items(&v["action_items"])
            .filter_map(|a| {
                Some(ActionItem {
                    text: text(&a["text"])?,
                    owner: text(&a["owner"]),
                    due: text(&a["due"]),
                })
            })
            .collect(),
    };
    let speakers = items(&v["speakers"])
        .filter_map(|s| {
            Some(SpeakerGuess {
                speaker: text(&s["speaker"])?,
                name: text(&s["name"])?,
                lines: items(&s["lines"]).filter_map(segment_index).collect(),
            })
        })
        .collect();
    RawNotes { summary, speakers }
}

/// Parses one reply; a reply with neither an overview nor sections is an error.
fn notes_from(value: &Value, what: &str) -> Result<RawNotes> {
    let raw = raw_notes(value);
    if raw.summary.overview.is_empty() && raw.summary.sections.is_empty() {
        let keys: Vec<&str> = value
            .as_object()
            .map(|o| o.keys().map(String::as_str).collect())
            .unwrap_or_default();
        log::warn!(
            "Notetaker: the model's reply for {what} had no notes (keys: {keys:?}): {}",
            crate::utils::redact_text(&value.to_string())
        );
        bail!("The model's reply for {what} had no notes. Try Regenerate.");
    }
    Ok(raw)
}

/// Notes for a meeting, plus the names the model offered for speakers (each
/// with the lines that say it; `diarize::names_from_guesses` checks them).
pub fn summarize(
    model: &Model,
    meeting: &Meeting,
    thoughts: &str,
    segments: &[Segment],
) -> Result<(NoteSummary, Vec<SpeakerGuess>)> {
    let parts = split_parts(&transcript_lines(segments));
    let mut speakers = Vec::new();
    let mut partials = Vec::new();
    for (i, part) in parts.iter().enumerate() {
        let numbering = (parts.len() > 1).then_some((i + 1, parts.len()));
        let value = model.chat_json(
            NOTES_SYSTEM,
            &notes_prompt(meeting, thoughts, part, numbering),
        )?;
        let what = match numbering {
            Some((i, n)) => format!("part {i} of {n}"),
            None => "the meeting".to_string(),
        };
        let raw = notes_from(&value, &what)?;
        speakers.extend(raw.speakers);
        partials.push(raw.summary);
    }

    let summary = if partials.len() == 1 {
        partials.pop().unwrap()
    } else {
        let merged: Vec<Value> = partials
            .into_iter()
            .map(|p| {
                json!({
                    "overview": p.overview,
                    "sections": p.sections,
                    "key_points": p.key_points,
                    "decisions": p.decisions,
                    "action_items": p.action_items,
                })
            })
            .collect();
        let prompt = format!(
            "Meeting: {}\nThese are notes for consecutive parts of one meeting:\n{}\n\n\
Merge them into one set of notes for the whole meeting. Combine sections on the same topic, \
remove repeats, keep every decision and action item. Leave 'speakers' empty.\n\
Reply with JSON in this shape:\n{NOTES_SHAPE}",
            if meeting.title_auto {
                "untitled; name it after what was discussed"
            } else {
                meeting.title.as_str()
            },
            serde_json::to_string_pretty(&merged)?
        );
        notes_from(&model.chat_json(NOTES_SYSTEM, &prompt)?, "the merged notes")?.summary
    };

    Ok((summary, speakers))
}

/// A passage the answer may cite.
pub struct Passage {
    pub meeting_id: String,
    pub meeting_title: String,
    pub meeting_start_ms: i64,
    pub start_ms: i64,
    pub text: String,
}

const STOPWORDS: &[&str] = &[
    "the",
    "and",
    "for",
    "are",
    "was",
    "were",
    "what",
    "who",
    "whom",
    "when",
    "where",
    "which",
    "how",
    "did",
    "does",
    "about",
    "with",
    "from",
    "that",
    "this",
    "have",
    "has",
    "had",
    "you",
    "your",
    "our",
    "they",
    "them",
    "their",
    "there",
    "been",
    "will",
    "would",
    "could",
    "should",
    "can",
    "any",
    "all",
    "into",
    "out",
    "not",
    "but",
    "his",
    "her",
    "she",
    "him",
    "its",
    "say",
    "said",
    "tell",
    "give",
    "meeting",
    "meetings",
    "call",
    "calls",
    "please",
    "summarize",
    "summary",
    "week",
    "today",
    "yesterday",
];

pub fn keywords(question: &str) -> Vec<String> {
    question
        .split(|c: char| !c.is_alphanumeric())
        .map(|w| w.to_lowercase())
        .filter(|w| w.chars().count() >= 3 && !STOPWORDS.contains(&w.as_str()))
        .collect::<HashSet<_>>()
        .into_iter()
        .collect()
}

/// Keyword hits, weighted toward recent meetings (half-life 14 days).
pub fn score_passage(text: &str, keywords: &[String], meeting_start_ms: i64, now_ms: i64) -> f64 {
    let lower = text.to_lowercase();
    let hits: usize = keywords
        .iter()
        .map(|k| lower.matches(k.as_str()).count())
        .sum();
    let age_days = (now_ms - meeting_start_ms).max(0) as f64 / 86_400_000.0;
    let recency = 0.5f64.powf(age_days / 14.0);
    hits as f64 + 0.5 * recency
}

/// Keeps the best passages that fit the context, in time order.
pub fn pick_passages(mut scored: Vec<(f64, Passage)>) -> Vec<Passage> {
    scored.sort_by(|a, b| b.0.total_cmp(&a.0));
    let mut used = 0;
    let mut picked: Vec<Passage> = Vec::new();
    for (_, p) in scored {
        if used + p.text.len() > ASK_CONTEXT_CHARS {
            continue;
        }
        used += p.text.len();
        picked.push(p);
    }
    picked.sort_by_key(|p| (p.meeting_start_ms, p.start_ms));
    picked
}

pub fn answer(
    model: &Model,
    question: &str,
    history: &[(String, String)],
    passages: &[Passage],
) -> Result<(String, Vec<Citation>)> {
    let now = chrono::Local::now().format("%A %-d %B %Y, %H:%M");
    let mut context = String::new();
    for (i, p) in passages.iter().enumerate() {
        let day = chrono::DateTime::from_timestamp_millis(p.meeting_start_ms)
            .map(|d| {
                d.with_timezone(&chrono::Local)
                    .format("%a %-d %b")
                    .to_string()
            })
            .unwrap_or_default();
        context.push_str(&format!(
            "[{}] \"{}\" ({day}) at {}:\n{}\n\n",
            i + 1,
            p.meeting_title,
            clock(p.start_ms),
            p.text
        ));
    }
    let mut prompt = format!("Now: {now}\n\nPassages from the user's meetings:\n{context}");
    if !history.is_empty() {
        prompt.push_str("Earlier in this chat:\n");
        for (role, content) in history {
            prompt.push_str(&format!("{role}: {content}\n"));
        }
        prompt.push('\n');
    }
    prompt.push_str(&format!(
        "Question: {question}\n\nAnswer from the passages only. Put the passage number in brackets, like [2], \
after each claim it supports. If the passages don't answer it, say so. Use short markdown \
(bullets when listing). Reply with JSON: {{\"answer\": \"...\", \"used\": [1, 2]}}"
    ));
    let system =
        "You answer questions about the user's meetings using passages from their meeting notes \
and transcripts. 'Me' is the user. Answer in the language of the question. Reply with JSON only.";
    let value = model.chat_json(system, &prompt)?;
    let text = value["answer"]
        .as_str()
        .unwrap_or_default()
        .trim()
        .to_string();
    if text.is_empty() {
        bail!("The model gave an empty answer");
    }
    let mut used: Vec<usize> = value["used"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|v| v.as_u64().map(|n| n as usize))
        .collect();
    for n in 1..=passages.len() {
        if text.contains(&format!("[{n}]")) && !used.contains(&n) {
            used.push(n);
        }
    }
    used.sort_unstable();
    let citations = used
        .into_iter()
        .filter_map(|n| passages.get(n.wrapping_sub(1)).map(|p| (n, p)))
        .map(|(n, p)| Citation {
            marker: n as u32,
            meeting_id: p.meeting_id.clone(),
            meeting_title: p.meeting_title.clone(),
            start_ms: p.start_ms,
            quote: p.text.chars().take(240).collect(),
        })
        .collect();
    Ok((text, citations))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn meeting(title: &str, title_auto: bool) -> Meeting {
        Meeting {
            id: "m".to_string(),
            title: title.to_string(),
            title_auto,
            start_ms: 0,
            end_ms: None,
            app: Some("Slack".to_string()),
            bundle_id: None,
            calendar_event_id: None,
            attendees: Vec::new(),
            folder: None,
            status: crate::notetaker::MeetingStatus::Ready,
            summary_status: crate::notetaker::SummaryStatus::Ready,
            summary_error: None,
            mic_offset_ms: None,
            system_offset_ms: None,
            hinglish_redo: None,
            has_audio: false,
        }
    }

    #[test]
    fn made_up_title_is_not_shown_to_the_model() {
        let prompt = notes_prompt(&meeting("Slack call", true), "", "#0 Me: hi", None);
        assert!(!prompt.contains("Slack call"));
        let prompt = notes_prompt(&meeting("Data Standup", false), "", "#0 Me: hi", None);
        assert!(prompt.contains("Meeting: Data Standup"));
    }

    #[test]
    fn invite_list_shows_names_not_emails_or_rooms() {
        let person = |email: &str, is_me: bool| Attendee {
            name: Some(email.to_string()),
            email: Some(email.to_string()),
            is_me,
            ..Default::default()
        };
        let mut m = meeting("Data Standup", false);
        m.attendees = vec![
            person("priya.shah@example.com", true),
            person("varun.kapoor@example.com", false),
            person("kapil@example.com", false),
            Attendee {
                name: Some("Office-Room-7 (4)".to_string()),
                email: Some("c_1188abc@resource.calendar.google.com".to_string()),
                is_me: false,
                ..Default::default()
            },
        ];
        let prompt = notes_prompt(&m, "", "#0 Me: hi", None);
        assert!(prompt.contains("'Me' is Priya Shah.\n"));
        assert!(prompt.contains("Invited: Varun Kapoor, Kapil\n"));
        assert!(!prompt.contains("Office"));
    }

    #[test]
    fn built_in_notes_request_asks_for_json_without_thinking() {
        let body = builtin_body("Write notes.", "Transcript");
        assert_eq!(body["response_format"], json!({ "type": "json_object" }));
        assert_eq!(body["chat_template_kwargs"]["enable_thinking"], false);
        assert_eq!(body["messages"][0]["role"], "system");
        assert_eq!(body["messages"][1]["content"], "Transcript");
        assert!(body.get("max_tokens").is_none());
    }

    #[test]
    fn built_in_model_that_is_not_downloaded_says_so() {
        let model = Model::Builtin {
            path: PathBuf::from("/nonexistent/model.gguf"),
            name: "Qwen3 8B".to_string(),
        };
        assert!(model
            .unavailable_reason()
            .is_some_and(|r| r.contains("isn't downloaded")));
        assert_eq!(model.model(), "Qwen3 8B");
    }

    #[test]
    fn parses_reply_with_thinking_and_fences() {
        let v = parse_json_reply("<think>hmm</think>\n```json\n{\"a\": 1}\n```").unwrap();
        assert_eq!(v["a"], 1);
    }

    #[test]
    fn notes_survive_nulls_and_hash_segments() {
        let v: Value = serde_json::from_str(
            r##"{"title": null, "overview": "Planned the launch.",
                "sections": [{"heading": null, "bullets": ["a", null]}, "odd"],
                "key_points": null,
                "action_items": [{"text": null}, {"text": "Send deck", "owner": "null"}],
                "speakers": [{"speaker": "Speaker 2", "name": "Priya", "lines": ["#12", 14]},
                             {"speaker": "Speaker 1", "name": null}]}"##,
        )
        .unwrap();
        let raw = notes_from(&v, "the meeting").unwrap();
        assert_eq!(raw.summary.overview, "Planned the launch.");
        assert_eq!(raw.summary.sections.len(), 1);
        assert_eq!(raw.summary.sections[0].bullets, vec!["a"]);
        assert_eq!(raw.summary.action_items.len(), 1);
        assert!(raw.summary.action_items[0].owner.is_none());
        assert_eq!(
            raw.speakers,
            vec![SpeakerGuess {
                speaker: "Speaker 2".into(),
                name: "Priya".into(),
                lines: vec![12, 14],
            }]
        );
        assert!(notes_from(&json!({"title": "x"}), "the meeting").is_err());
    }

    #[test]
    fn parts_count_tokens_not_bytes() {
        let hindi = format!("#0 [00:00] Them: {}", "नमस्ते ".repeat(200));
        let parts = split_parts(&vec![hindi; 20]);
        assert!(parts.len() > 1, "{} parts", parts.len());
    }

    #[test]
    fn keywords_drop_stopwords() {
        let mut k = keywords("What did Priya say about the Q3 budget?");
        k.sort();
        assert_eq!(k, vec!["budget", "priya"]);
    }

    #[test]
    fn clock_formats() {
        assert_eq!(clock(65_000), "01:05");
        assert_eq!(clock(3_725_000), "1:02:05");
    }
}
