use anyhow::{anyhow, bail, Result};
use serde_json::{json, Value};
use std::collections::HashSet;
use std::path::PathBuf;
use std::time::Duration;

use crate::settings::CleanupEngine;
use crate::textflow::engine::{self, ENGINE};

use super::diarize::{self, SpeakerGuess};
use super::{ActionItem, Attendee, Citation, Meeting, NoteSection, NoteSummary, Quote, Segment};

const CONTEXT_TOKENS: u32 = 16_384;
/// Transcript tokens per summary call. Parts well under the context gave more
/// specific notes than nearly full ones on long Hinglish meetings.
const PART_TOKENS: usize = 6_000;
/// Longest reply to one part's notes or to the merge, about twice the
/// longest seen on real meetings: a model that starts repeating itself stops
/// within minutes instead of running into the 10-minute limit.
const PART_REPLY_TOKENS: u32 = 3_000;
const MERGE_REPLY_TOKENS: u32 = 4_000;
const ANSWER_REPLY_TOKENS: u32 = 2_000;
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

    /// A JSON reply of at most `max_tokens`; a longer one is an error.
    fn chat_json(&self, system: &str, user: &str, max_tokens: u32) -> Result<Value> {
        let path = match self {
            Model::Ollama(ollama) => return ollama.chat_json(system, user, max_tokens),
            Model::Builtin { path, .. } => path,
        };
        let body = builtin_body(system, user, max_tokens);
        let content = tauri::async_runtime::block_on(async {
            let request = async {
                let conn = ENGINE.ensure(path, CONTEXT_TOKENS).await?;
                let reply = engine::post_chat(&conn, &body, true).await;
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
fn builtin_body(system: &str, user: &str, max_tokens: u32) -> Value {
    let messages = [
        json!({ "role": "system", "content": system }),
        json!({ "role": "user", "content": user }),
    ];
    let mut body = engine::chat_body(&messages, Some(max_tokens));
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

    fn chat_json(&self, system: &str, user: &str, max_tokens: u32) -> Result<Value> {
        let body = json!({
            "model": self.model,
            "stream": false,
            "think": false,
            "format": "json",
            "options": { "temperature": 0.2, "num_ctx": CONTEXT_TOKENS, "num_predict": max_tokens },
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
        if response["done_reason"] == "length" {
            bail!(engine::CUT_OFF);
        }
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
These are labels, not names, and one label can hold several people. The transcript comes from speech recognition, so some words are misheard. \
Write in the language the meeting was mostly held in. Write only what the transcript says: never invent names, dates, deadlines, \
numbers, decisions, or work that was done. Reply with JSON only.";

const NOTES_SHAPE: &str = r#"{
  "title": "short title naming what was discussed (max 8 words), never just the app or the word meeting",
  "overview": "2-3 specific sentences: what was discussed, what was shown or found, where it landed",
  "decisions": [{"text": "what the people agreed to do or not do", "quote": "the words where it was agreed, copied exactly", "line": 41}],
  "proposals": [{"proposal": "the idea put forward", "by": "the person's name, or null", "pushback": "the disagreement as said, or null", "outcome": "agreed / rejected / changed to ... / left open", "quote": "the words where it was put forward, copied exactly", "line": 12}],
  "open_questions": [{"text": "a question someone asked that got no answer", "line": 7}],
  "numbers": [{"value": "the number exactly as written in the line", "what": "what it counts, specifically", "unclear": false, "line": 33}],
  "key_points": ["specific facts explained or shown: how something works, what was found"],
  "action_items": [{"text": "what has to be done", "owner": "a name said in the meeting, or null", "due": "the deadline words exactly as said, or null", "line": 20}],
  "speakers": [{"speaker": "Speaker 2", "name": "the person's name", "lines": [12, 30]}]
}"#;

const NOTES_RULES: &str = "Rules:\n\
- decisions: what the people agreed to do or not to do. Agreement often sounds like 'okay, let's do that', \
'theek hai, kar dete hain', 'main add kar dunga', 'don't start it until X is ready', 'for now leave it, but write it down', \
or a change one person asks for in the other's work that they accept. Example: 'Kavya: the menu should list prices too. \
Arjun: haan, main add kar dunga.' is the decision 'Add prices to the menu'. A suggestion that was argued over and not \
settled, or that nobody answered, is a proposal, not a decision.\n\
- proposals: ideas someone put forward, especially ones argued over. 'by' is a name only if the transcript says who it was. \
'pushback' is what someone said against it, close to their words, or null; never guess a reason. 'outcome' says whether it was settled.\n\
- 'quote' is 4 to 15 words copied exactly from one line, or two lines in a row, of the transcript. For a decision, \
quote the words where it was agreed or settled, not where the topic came up. A decision or \
proposal without a real quote is thrown away, and so is a decision whose quote says the opposite (like 'we won't do it').\n\
- 'line' is the #index of the line the item comes from.\n\
- open_questions: only questions about the work that someone asked aloud and nobody answered. Not side remarks, \
and not questions you think are worth asking.\n\
- numbers: counts, percentages, amounts and results said aloud, copied exactly as written in that line, with what each \
one counts, like '12 of 40 stores reordered in March'. Skip numbers said while adding up aloud, and any number when \
you can't say what it counts. Set 'unclear' to true when the number looks misheard. Never add numbers up or work out new ones.\n\
- key_points: at most 6 specific facts given: how a system or step works, what was tested and what came out. \
Name the actual things (tables, tools, screens, steps), not 'the process' or 'the data'.\n\
- action_items: only work someone said will be done. 'owner' is a person's name said in the transcript, or 'Me' when the \
recorder says they will do it; never 'Speaker 1', 'Them' or 'the team'. 'due' is only a deadline someone said, \
copied exactly (like 'Friday tak'); otherwise null. Never make up a date.\n\
- Describe work the way it was described: do not call testing or rules 'training a model' unless a model was trained.\n\
- Leave out sentences that would fit any meeting, such as 'The team emphasized the importance of data integrity', \
'The team agreed to maintain a structured approach', 'There was a discussion on how to improve the process', \
'The team acknowledged the challenges' or 'Further testing and review is needed', and notes like 'Mention of X' \
or 'Discussion about X'. If you can't say something specific, say nothing.\n\
- Empty lists are fine.\n";

fn notes_prompt(
    meeting: &Meeting,
    thoughts: &str,
    transcript: &str,
    part: Option<(usize, usize)>,
) -> String {
    let attendees = diarize::invitees(&meeting.attendees);
    let mut prompt = format!(
        "Meeting: {}\nDate: {}\n",
        meeting_name(meeting),
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
        prompt.push_str(&format!(
            "This is part {i} of {n} of the transcript. Something not settled in this part may be settled later: \
keep it as a proposal or open question.\n"
        ));
    }
    if !thoughts.trim().is_empty() {
        prompt.push_str(&format!(
            "\nThe user's own notes from the meeting (cover these topics first):\n{}\n",
            capped_thoughts(thoughts)
        ));
    }
    prompt.push_str(&format!(
        "\nTranscript:\n{transcript}\n{NOTES_RULES}\
For 'speakers', only name a 'Speaker N' when the transcript makes it clear who that is: \
someone says their name right before they speak, or they introduce themselves. \
'lines' are the line numbers (#index) where that name is said. Never name 'Me' or 'Them'. \
Use names from the invite list when they match, even if the transcript spells them differently. Leave out anyone you are not sure about.\n\
Reply with JSON in this shape:\n{NOTES_SHAPE}"
    ));
    prompt
}

/// A made-up name ("Slack call") would only be copied back as the title.
fn meeting_name(meeting: &Meeting) -> &str {
    if meeting.title_auto {
        "untitled; name it after what was discussed"
    } else {
        meeting.title.as_str()
    }
}

/// One reply in the `NOTES_SHAPE` form. The parts of a long meeting go to
/// the merge in this form, so a proposal arrives with its outcome and is not
/// read as a decision.
#[derive(Clone, Debug, Default, serde::Serialize)]
struct Notes {
    title: Option<String>,
    overview: String,
    decisions: Vec<Decision>,
    proposals: Vec<Proposal>,
    open_questions: Vec<String>,
    numbers: Vec<Number>,
    key_points: Vec<String>,
    action_items: Vec<Action>,
}

#[derive(Clone, Debug, serde::Serialize)]
struct Action {
    #[serde(flatten)]
    item: ActionItem,
    /// The transcript line (#index) the model says it comes from.
    #[serde(skip)]
    line: Option<usize>,
}

#[derive(Clone, Debug, serde::Serialize)]
struct Decision {
    text: String,
    quote: Option<String>,
}

#[derive(Clone, Debug, serde::Serialize)]
struct Proposal {
    proposal: String,
    by: Option<String>,
    pushback: Option<String>,
    outcome: Option<String>,
    quote: Option<String>,
}

#[derive(Clone, Debug, serde::Serialize)]
struct Number {
    value: String,
    what: Option<String>,
    unclear: bool,
    /// The transcript line (#index) the model says it comes from.
    #[serde(skip)]
    line: Option<usize>,
}

/// Ends `s` with a full stop unless it already ends a sentence.
fn sentence(s: &str) -> String {
    let s = s.trim();
    if s.ends_with(['.', '!', '?', '।']) {
        s.to_string()
    } else {
        format!("{s}.")
    }
}

impl Proposal {
    fn line(&self) -> String {
        let mut line = match &self.by {
            Some(by) => sentence(&format!("{by} proposed: {}", self.proposal)),
            None => sentence(&self.proposal),
        };
        if let Some(pushback) = &self.pushback {
            line.push_str(&format!(" Pushback: {}", sentence(pushback)));
        }
        if let Some(outcome) = &self.outcome {
            line.push_str(&format!(" Outcome: {}", sentence(outcome)));
        }
        line
    }
}

impl Number {
    fn line(&self) -> String {
        let mut line = match &self.what {
            Some(what) => format!("{}: {what}", self.value),
            None => self.value.clone(),
        };
        if self.unclear && !line.contains("(unclear)") {
            line.push_str(" (unclear)");
        }
        line
    }
}

impl Notes {
    fn is_empty(&self) -> bool {
        self.overview.is_empty()
            && self.decisions.is_empty()
            && self.proposals.is_empty()
            && self.open_questions.is_empty()
            && self.numbers.is_empty()
            && self.key_points.is_empty()
            && self.action_items.is_empty()
    }

    /// The stored form: proposals, open questions and numbers become
    /// sections with fixed headings.
    fn into_summary(self) -> NoteSummary {
        let mut sections = Vec::new();
        let mut add = |heading: &str, bullets: Vec<String>| {
            if !bullets.is_empty() {
                sections.push(NoteSection {
                    heading: heading.to_string(),
                    bullets,
                });
            }
        };
        add(
            "Proposals and debates",
            self.proposals.iter().map(Proposal::line).collect(),
        );
        add("Open questions", self.open_questions);
        add("Numbers", self.numbers.iter().map(Number::line).collect());
        let quotes = self
            .decisions
            .iter()
            .filter_map(|d| Some((d.text.clone(), d.quote.clone()?)))
            .chain(
                self.proposals
                    .iter()
                    .filter_map(|p| Some((p.line(), p.quote.clone()?))),
            )
            .map(|(item, quote)| Quote { item, quote })
            .collect();
        NoteSummary {
            title: self.title,
            overview: self.overview,
            sections,
            key_points: self.key_points,
            decisions: self.decisions.into_iter().map(|d| d.text).collect(),
            action_items: self.action_items.into_iter().map(|a| a.item).collect(),
            quotes,
        }
    }

    /// Drops owners the transcript never says (speaker labels, names nobody
    /// said), pushback that is only a label or a word or two, asides like "When?"
    /// listed as open questions, generic filler the model copies from the
    /// prompt's own examples, and repeated numbers.
    /// `me_is_one_person` is false when every line is 'Me' (a room around
    /// one microphone), where 'Me' is everyone.
    fn tidy(&mut self, said: &HashSet<String>, me_is_one_person: bool) {
        let named = |name: &String| name_was_said(name, said, me_is_one_person);
        for a in &mut self.action_items {
            a.item.owner = a.item.owner.take().filter(named);
        }
        for p in &mut self.proposals {
            p.by = p.by.take().filter(named);
            p.pushback = p
                .pushback
                .take()
                .filter(|b| b.split_whitespace().count() >= 3);
        }
        self.open_questions
            .retain(|q| q.contains('?') && q.split_whitespace().count() >= 4);
        for list in [&mut self.open_questions, &mut self.key_points] {
            list.retain(|s| !is_filler(s));
        }
        self.decisions.retain(|d| !is_filler(&d.text));
        self.proposals.retain(|p| !is_filler(&p.proposal));
        self.action_items.retain(|a| !is_filler(&a.item.text));
        let mut seen = HashSet::new();
        self.numbers.retain(|n| {
            seen.insert((
                n.value.to_lowercase(),
                n.what.as_deref().map(str::to_lowercase),
            ))
        });
    }
}

/// Whether the number's value is written on the line it cites or the lines
/// either side (a sentence can run over two lines).
fn number_is_on_line(n: &Number, segments: &[Segment]) -> bool {
    let Some(line) = n.line else {
        return false;
    };
    let nearby = segments
        .iter()
        .skip(line.saturating_sub(1))
        .take(if line == 0 { 2 } else { 3 });
    number_heard(n, nearby)
}

/// Whether the number's value is written in `lines`. Only the parts of the
/// value with digits are compared when it has any.
fn number_heard<'a>(n: &Number, lines: impl IntoIterator<Item = &'a Segment>) -> bool {
    let heard = said_words(lines.into_iter().map(|s| s.text.as_str()));
    let words: Vec<String> = n
        .value
        .split(|c: char| !c.is_alphanumeric())
        .filter(|w| !w.is_empty())
        .map(str::to_lowercase)
        .collect();
    let digits: Vec<&String> = words
        .iter()
        .filter(|w| w.chars().any(|c| c.is_ascii_digit()))
        .collect();
    let wanted = if digits.is_empty() {
        words.iter().collect()
    } else {
        digits
    };
    !wanted.is_empty() && wanted.iter().all(|w| heard.contains(*w))
}

impl Notes {
    /// Keeps decisions and proposals whose quote is in the transcript. A
    /// decision whose quote refuses what it says becomes an unsettled
    /// proposal.
    fn check_quotes(&mut self, segments: &[Segment]) {
        let lines: Vec<HashSet<String>> = segments
            .iter()
            .map(|s| sound_words(&s.text).collect())
            .collect();
        let found =
            |quote: &Option<String>| quote.as_deref().is_some_and(|q| quote_found(q, &lines));
        for d in std::mem::take(&mut self.decisions) {
            if !found(&d.quote) {
                continue;
            }
            if refuses(&d.text, d.quote.as_deref().unwrap_or_default()) {
                self.proposals.push(Proposal {
                    proposal: d.text,
                    by: None,
                    pushback: None,
                    outcome: Some("not settled".to_string()),
                    quote: d.quote,
                });
            } else {
                self.decisions.push(d);
            }
        }
        self.proposals.retain(|p| found(&p.quote));
    }

    /// Keeps a due date only when its words, in order, are on the line the
    /// action cites or within two lines of it.
    fn check_dues(&mut self, segments: &[Segment]) {
        for a in &mut self.action_items {
            let line = a.line;
            a.item.due = a.item.due.take().filter(|d| due_near(d, line, segments));
        }
    }
}

/// A word by its consonant sounds, so Hinglish spellings of one word agree
/// ("thik" and "theek", "dunga" and "doonga"). Words with digits stay as they
/// are; words with no consonant sound ("hai", "he") share one key.
fn sound_word(word: &str) -> String {
    if word.chars().any(|c| c.is_ascii_digit()) {
        return word.to_lowercase();
    }
    let key = crate::textflow::sound_key(word);
    if key.is_empty() {
        "_".to_string()
    } else {
        key
    }
}

fn sound_words(text: &str) -> impl Iterator<Item = String> + '_ {
    text.split(|c: char| !c.is_alphanumeric())
        .filter(|w| !w.is_empty())
        .map(sound_word)
}

/// At least 70% of the quote's words (and at least 3) are on one line or
/// two lines in a row, compared by sound.
fn quote_found(quote: &str, lines: &[HashSet<String>]) -> bool {
    let words: Vec<String> = sound_words(quote).collect();
    if words.len() < 3 {
        return false;
    }
    let need = (words.len() * 7).div_ceil(10);
    (0..lines.len()).any(|i| {
        let next = lines.get(i + 1);
        let hits = words
            .iter()
            .filter(|w| lines[i].contains(*w) || next.is_some_and(|n| n.contains(*w)))
            .count();
        hits >= need
    })
}

fn due_near(due: &str, line: Option<usize>, segments: &[Segment]) -> bool {
    let Some(line) = line.filter(|&l| l < segments.len()) else {
        return false;
    };
    let wanted: Vec<String> = sound_words(due).collect();
    let nearby: Vec<String> = segments[line.saturating_sub(2)..(line + 3).min(segments.len())]
        .iter()
        .flat_map(|s| sound_words(&s.text))
        .collect();
    !wanted.is_empty() && nearby.windows(wanted.len()).any(|w| w == wanted.as_slice())
}

const NEGATION: &[&str] = &[
    "not", "no", "nahi", "nahin", "nahiin", "nhi", "mat", "never", "dont", "wont", "cant",
];
/// Words that make a "don't" a condition ("don't launch until it's ready").
const CONDITION: &[&str] = &["until", "till", "unless", "before"];
/// Words too common to tie a negation to the decision's own action.
const COMMON: &[&str] = &[
    "the", "and", "for", "with", "after", "this", "that", "will", "should", "hai", "kar", "karo",
    "karna", "hum", "aur",
];

fn lower_words(text: &str) -> Vec<String> {
    text.to_lowercase()
        .replace('\u{2019}', "'")
        .split(|c: char| !c.is_alphanumeric() && c != '\'')
        .map(|w| w.trim_matches('\''))
        .filter(|w| !w.is_empty())
        .map(str::to_string)
        .collect()
}

fn is_negation(word: &str) -> bool {
    NEGATION.contains(&word) || word.ends_with("n't")
}

/// Whether the quote refuses the decision's own action: a negation within
/// two words of a word the decision also uses. A decision that is itself a
/// "don't" or a "remove", and a quote that sets a condition, never count.
fn refuses(decision: &str, quote: &str) -> bool {
    const AWAY: &[&str] = &[
        "without", "instead", "stop", "skip", "avoid", "drop", "remove",
    ];
    let own_words = lower_words(decision);
    if own_words
        .iter()
        .any(|w| is_negation(w) || AWAY.contains(&w.as_str()))
    {
        return false;
    }
    let words = lower_words(quote);
    let text = words.join(" ");
    if words.iter().any(|w| CONDITION.contains(&w.as_str())) || text.contains("jab tak") {
        return false;
    }
    let own: HashSet<String> = own_words
        .iter()
        .filter(|w| w.chars().count() >= 3 && !COMMON.contains(&w.as_str()))
        .map(|w| sound_word(w))
        .filter(|k| k != "_")
        .collect();
    let negations: Vec<usize> = (0..words.len())
        .filter(|&i| is_negation(&words[i]))
        .collect();
    words.iter().enumerate().any(|(j, w)| {
        !is_negation(w)
            && own.contains(&sound_word(w))
            && negations.iter().any(|&i| i.abs_diff(j) <= 2)
    })
}

/// Whether two notes items say the same thing: most of the shorter one's
/// words (by sound) are in the other.
fn same_item(a: &str, b: &str) -> bool {
    let keys = |t: &str| -> HashSet<String> {
        lower_words(t)
            .iter()
            .filter(|w| w.chars().count() >= 3 && !COMMON.contains(&w.as_str()))
            .map(|w| sound_word(w))
            .filter(|k| k != "_")
            .collect()
    };
    let (a, b) = (keys(a), keys(b));
    let shorter = a.len().min(b.len());
    shorter > 0 && a.intersection(&b).count() * 5 >= shorter * 3
}

/// The merge's notes with decisions and proposals tied back to the checked
/// ones they came from: an item that matches none (the merge made it up, or
/// moved a proposal to decisions) is dropped, a part's item the merge left
/// out comes back, and quotes and due dates are the parts' own.
fn reconcile(mut merged: Notes, parts: &[Notes]) -> Notes {
    let part_decisions: Vec<&Decision> = parts.iter().flat_map(|p| &p.decisions).collect();
    let mut used = vec![false; part_decisions.len()];
    let mut decisions = Vec::new();
    for d in merged.decisions {
        if let Some(i) =
            (0..part_decisions.len()).find(|&i| same_item(&d.text, &part_decisions[i].text))
        {
            if !used[i] {
                used[i] = true;
                decisions.push(Decision {
                    text: d.text,
                    quote: part_decisions[i].quote.clone(),
                });
            }
        }
    }
    decisions.extend(
        (0..part_decisions.len())
            .filter(|&i| !used[i])
            .map(|i| part_decisions[i].clone()),
    );
    merged.decisions = decisions;

    let part_proposals: Vec<&Proposal> = parts.iter().flat_map(|p| &p.proposals).collect();
    let mut used = vec![false; part_proposals.len()];
    let mut proposals = Vec::new();
    for p in merged.proposals {
        if let Some(i) =
            (0..part_proposals.len()).find(|&i| same_item(&p.proposal, &part_proposals[i].proposal))
        {
            if !used[i] {
                used[i] = true;
                proposals.push(Proposal {
                    quote: part_proposals[i].quote.clone(),
                    ..p
                });
            }
        }
    }
    proposals.extend(
        (0..part_proposals.len())
            .filter(|&i| !used[i])
            .map(|i| part_proposals[i].clone()),
    );
    merged.proposals = proposals;

    let dues: HashSet<String> = parts
        .iter()
        .flat_map(|p| &p.action_items)
        .filter_map(|a| a.item.due.as_deref())
        .map(|d| sound_words(d).collect::<Vec<_>>().join(" "))
        .collect();
    for a in &mut merged.action_items {
        a.item.due = a
            .item
            .due
            .take()
            .filter(|d| dues.contains(&sound_words(d).collect::<Vec<_>>().join(" ")));
    }
    merged
}

/// Part notes one after another, for when the merge fails.
fn joined(parts: Vec<Notes>) -> Notes {
    let mut all = Notes::default();
    for p in parts {
        all.title = all.title.or(p.title);
        if !p.overview.is_empty() {
            if !all.overview.is_empty() {
                all.overview.push(' ');
            }
            all.overview.push_str(&p.overview);
        }
        all.decisions.extend(p.decisions);
        all.proposals.extend(p.proposals);
        all.open_questions.extend(p.open_questions);
        all.numbers.extend(p.numbers);
        all.key_points.extend(p.key_points);
        all.action_items.extend(p.action_items);
    }
    all
}

/// Phrases of notes that would fit any meeting.
const FILLER: &[&str] = &[
    "structured approach",
    "importance of",
    "acknowledged the challenge",
    "there was a discussion",
    "further testing and review",
    "mention of",
    "discussion about",
    "current process",
];

fn is_filler(s: &str) -> bool {
    let lower = s.to_lowercase();
    FILLER.iter().any(|f| lower.contains(f))
}

/// Lowercase words of the transcript and the known names.
fn said_words<'a>(texts: impl IntoIterator<Item = &'a str>) -> HashSet<String> {
    texts
        .into_iter()
        .flat_map(|t| t.split(|c: char| !c.is_alphanumeric()))
        .filter(|w| !w.is_empty())
        .map(str::to_lowercase)
        .collect()
}

fn name_was_said(name: &str, said: &HashSet<String>, me_is_one_person: bool) -> bool {
    let lower = name.trim().to_lowercase();
    if lower == "me" {
        return me_is_one_person;
    }
    let label = lower.starts_with("speaker")
        || lower.contains("not said")
        || matches!(
            lower.as_str(),
            "them"
                | "unknown"
                | "team"
                | "the team"
                | "everyone"
                | "all"
                | "someone"
                | "n/a"
                | "tbd"
        );
    !label
        && lower
            .split(|c: char| !c.is_alphanumeric())
            .any(|w| w.chars().count() >= 3 && said.contains(w))
}

/// What one reply holds. Each field is read on its own, so a `null` or an
/// odd value loses only that field.
struct RawNotes {
    notes: Notes,
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

/// Items written as strings or as `{"text": ..., "line": ...}`.
fn statements(v: &Value) -> Vec<String> {
    items(v)
        .filter_map(|item| text(item).or_else(|| text(&item["text"])))
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

/// A decision as an object, or as a bare string (which has no quote).
fn decision(v: &Value) -> Option<Decision> {
    if let Some(text) = text(v) {
        return Some(Decision { text, quote: None });
    }
    Some(Decision {
        text: text(&v["text"])?,
        quote: text(&v["quote"]),
    })
}

/// A proposal as an object, or as a bare string.
fn proposal(v: &Value) -> Option<Proposal> {
    if let Some(s) = text(v) {
        return Some(Proposal {
            proposal: s,
            by: None,
            pushback: None,
            outcome: None,
            quote: None,
        });
    }
    Some(Proposal {
        proposal: text(&v["proposal"])?,
        by: text(&v["by"]),
        pushback: text(&v["pushback"]),
        outcome: text(&v["outcome"]),
        quote: text(&v["quote"]),
    })
}

/// A number as an object, or as a bare string.
fn number(v: &Value) -> Option<Number> {
    if let Some(s) = text(v) {
        return Some(Number {
            unclear: s.contains("(unclear)"),
            value: s,
            what: None,
            line: None,
        });
    }
    let value = match &v["value"] {
        Value::Number(n) => n.to_string(),
        other => text(other)?,
    };
    Some(Number {
        value,
        what: text(&v["what"]),
        unclear: v["unclear"] == true || v["unclear"] == "true",
        line: segment_index(&v["line"]),
    })
}

fn raw_notes(v: &Value) -> RawNotes {
    let notes = Notes {
        title: text(&v["title"]),
        overview: text(&v["overview"]).unwrap_or_default(),
        decisions: items(&v["decisions"]).filter_map(decision).collect(),
        proposals: items(&v["proposals"]).filter_map(proposal).collect(),
        open_questions: statements(&v["open_questions"]),
        numbers: items(&v["numbers"]).filter_map(number).collect(),
        key_points: texts(&v["key_points"]),
        action_items: items(&v["action_items"])
            .filter_map(|a| {
                Some(Action {
                    item: ActionItem {
                        text: text(&a["text"])?,
                        owner: text(&a["owner"]),
                        due: text(&a["due"]),
                    },
                    line: segment_index(&a["line"]),
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
    RawNotes { notes, speakers }
}

/// Parses one reply; a reply with no notes at all is an error.
fn notes_from(value: &Value, what: &str) -> Result<RawNotes> {
    let raw = raw_notes(value);
    if raw.notes.is_empty() {
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

fn merge_prompt(meeting: &Meeting, parts: &[Notes]) -> Result<String> {
    Ok(format!(
        "Meeting: {}\nThese are notes for consecutive parts of one meeting, in order:\n{}\n\n\
Merge them into one set of notes for the whole meeting:\n\
- Keep every decision, proposal, open question, number and action item. Remove repeats and combine items about the same thing.\n\
- Keep at most 8 key points, the most specific ones, and none that repeat a decision or proposal.\n\
- A proposal stays a proposal with its pushback and outcome. It goes under decisions only if a part says its outcome was agreed. \
Never turn a proposal, suggestion or question into a decision yourself.\n\
- If a later part answers an open question, drop the question.\n\
- Copy numbers exactly as they are, with 'unclear' as it is. Never add them up or work out new ones.\n\
- Copy owners, due dates and quotes exactly; null stays null.\n\
- Write the title and overview for the whole meeting, specific, with no generic sentences.\n\
Leave 'speakers' empty.\n\
Reply with JSON in this shape:\n{NOTES_SHAPE}",
        meeting_name(meeting),
        serde_json::to_string_pretty(parts)?
    ))
}

/// Parts merged at once; more are merged in groups, then the groups'
/// notes merged, so no merge prompt outgrows the context.
const MERGE_GROUP: usize = 4;

/// Notes for a meeting, plus the names the model offered for speakers (each
/// with the lines that say it; `diarize::names_from_guesses` checks them).
pub fn summarize(
    model: &Model,
    meeting: &Meeting,
    thoughts: &str,
    segments: &[Segment],
) -> Result<(NoteSummary, Vec<SpeakerGuess>)> {
    let parts = part_notes(model, meeting, thoughts, segments)?;
    let mut notes = merged(model, meeting, parts.notes);
    if !parts.skipped.is_empty() {
        let list: Vec<String> = parts.skipped.iter().map(usize::to_string).collect();
        notes.overview.push_str(&format!(
            " (Part {} of {} of the transcript is not in these notes: the AI model's notes for it ran too long.)",
            list.join(", "),
            parts.total
        ));
    }
    Ok((
        checked(notes, meeting, segments).into_summary(),
        parts.speakers,
    ))
}

struct Parts {
    notes: Vec<Notes>,
    speakers: Vec<SpeakerGuess>,
    /// Parts (from 1) left out because their notes ran too long twice.
    skipped: Vec<usize>,
    total: usize,
}

/// Checked notes for each part of the transcript, in order.
fn part_notes(
    model: &Model,
    meeting: &Meeting,
    thoughts: &str,
    segments: &[Segment],
) -> Result<Parts> {
    let parts = split_parts(&transcript_lines(segments));
    let mut out = Parts {
        notes: Vec::new(),
        speakers: Vec::new(),
        skipped: Vec::new(),
        total: parts.len(),
    };
    for (i, part) in parts.iter().enumerate() {
        let numbering = (parts.len() > 1).then_some((i + 1, parts.len()));
        let value = match notes_reply(
            model,
            &notes_prompt(meeting, thoughts, part, numbering),
            PART_REPLY_TOKENS,
        ) {
            Err(e) if e.to_string() == engine::CUT_OFF && parts.len() > 1 => {
                log::warn!(
                    "Notetaker: notes for part {} ran too long twice; leaving it out",
                    i + 1
                );
                out.skipped.push(i + 1);
                continue;
            }
            reply => reply?,
        };
        let what = match numbering {
            Some((i, n)) => format!("part {i} of {n}"),
            None => "the meeting".to_string(),
        };
        let mut raw = notes_from(&value, &what)?;
        raw.notes.numbers.retain(|n| number_heard(n, segments));
        for n in &mut raw.notes.numbers {
            n.unclear |= !number_is_on_line(n, segments);
        }
        raw.notes.check_quotes(segments);
        raw.notes.check_dues(segments);
        out.speakers.extend(raw.speakers);
        out.notes.push(raw.notes);
    }
    if out.notes.is_empty() {
        bail!("The AI model's notes ran too long for every part of this meeting. Try Regenerate.");
    }
    Ok(out)
}

/// A notes reply; one cut off at `max_tokens` (the model kept repeating
/// itself) is asked for once more, shorter.
fn notes_reply(model: &Model, prompt: &str, max_tokens: u32) -> Result<Value> {
    match model.chat_json(NOTES_SYSTEM, prompt, max_tokens) {
        Err(e) if e.to_string() == engine::CUT_OFF => {
            log::warn!("Notetaker: the notes reply ran too long; asking for a shorter one");
            let shorter = format!(
                "{prompt}\nYour first reply ran too long and was cut off. Keep every list to at most 8 items \
and every quote under 15 words."
            );
            model.chat_json(NOTES_SYSTEM, &shorter, max_tokens)
        }
        reply => reply,
    }
}

/// One set of notes from the parts' notes, merged `MERGE_GROUP` at a time.
fn merged(model: &Model, meeting: &Meeting, parts: Vec<Notes>) -> Notes {
    let mut level = parts;
    while level.len() > 1 {
        let mut next = Vec::new();
        let mut rest = level.into_iter();
        loop {
            let group: Vec<Notes> = rest.by_ref().take(MERGE_GROUP).collect();
            match group.len() {
                0 => break,
                1 => next.extend(group),
                _ => next.push(merge_group(model, meeting, group)),
            }
        }
        level = next;
    }
    level.pop().unwrap_or_default()
}

fn merge_group(model: &Model, meeting: &Meeting, group: Vec<Notes>) -> Notes {
    let reply = merge_prompt(meeting, &group)
        .and_then(|prompt| notes_reply(model, &prompt, MERGE_REPLY_TOKENS))
        .and_then(|reply| notes_from(&reply, "the merged notes"));
    match reply {
        Ok(raw) => reconcile(raw.notes, &group),
        Err(e) => {
            log::warn!("Notetaker: merging part notes failed ({e}); joining them instead");
            joined(group)
        }
    }
}

/// `notes` with only owners the transcript says, and without filler.
fn checked(mut notes: Notes, meeting: &Meeting, segments: &[Segment]) -> Notes {
    let mut known = diarize::invitees(&meeting.attendees);
    known.extend(me_name(&meeting.attendees));
    let said = said_words(
        segments
            .iter()
            .flat_map(|s| [s.text.as_str(), s.speaker.as_str()])
            .chain(known.iter().map(String::as_str)),
    );
    notes.tidy(&said, segments.iter().any(|s| s.speaker != "Me"));
    notes
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
    let value = model.chat_json(system, &prompt, ANSWER_REPLY_TOKENS)?;
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
            hinglish_error: None,
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
        let body = builtin_body("Write notes.", "Transcript", 3_000);
        assert_eq!(body["response_format"], json!({ "type": "json_object" }));
        assert_eq!(body["chat_template_kwargs"]["enable_thinking"], false);
        assert_eq!(body["messages"][0]["role"], "system");
        assert_eq!(body["messages"][1]["content"], "Transcript");
        assert_eq!(body["max_tokens"], 3_000);
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
                "decisions": ["Ship on the 3rd", null],
                "proposals": null,
                "action_items": [{"text": null}, {"text": "Send deck", "owner": "null"}],
                "speakers": [{"speaker": "Speaker 2", "name": "Priya", "lines": ["#12", 14]},
                             {"speaker": "Speaker 1", "name": null}]}"##,
        )
        .unwrap();
        let raw = notes_from(&v, "the meeting").unwrap();
        assert_eq!(raw.notes.overview, "Planned the launch.");
        assert_eq!(raw.notes.decisions[0].text, "Ship on the 3rd");
        assert_eq!(raw.notes.decisions.len(), 1);
        assert!(raw.notes.proposals.is_empty());
        assert_eq!(raw.notes.action_items.len(), 1);
        assert!(raw.notes.action_items[0].item.owner.is_none());
        assert_eq!(
            raw.speakers,
            vec![SpeakerGuess {
                speaker: "Speaker 2".into(),
                name: "Priya".into(),
                lines: vec![12, 14],
            }]
        );
        assert!(notes_from(&json!({"title": "x"}), "the meeting").is_err());
        assert!(notes_from(&json!({"open_questions": ["Who pays?"]}), "the meeting").is_ok());
    }

    #[test]
    fn proposals_questions_and_numbers_become_sections() {
        let v = json!({
            "overview": "Compared two bakeries for the office order.",
            "decisions": ["Order from the corner bakery"],
            "proposals": [
                {"proposal": "Switch to weekly orders", "by": "Kavya",
                 "pushback": "Arjun: too much waste", "outcome": "left open"},
                "Try a tasting day"
            ],
            "open_questions": ["Who signs the invoice?"],
            "numbers": [
                {"value": 12, "what": "boxes a week", "unclear": false},
                {"value": "forty", "what": "people", "unclear": true},
                "300 rupees a box"
            ],
            "action_items": [{"text": "Call the bakery", "owner": "Kavya", "due": null}]
        });
        let summary = notes_from(&v, "the meeting").unwrap().notes.into_summary();
        let headings: Vec<&str> = summary
            .sections
            .iter()
            .map(|s| s.heading.as_str())
            .collect();
        assert_eq!(
            headings,
            ["Proposals and debates", "Open questions", "Numbers"]
        );
        assert_eq!(
            summary.sections[0].bullets,
            [
                "Kavya proposed: Switch to weekly orders. Pushback: Arjun: too much waste. Outcome: left open.",
                "Try a tasting day.",
            ]
        );
        assert_eq!(
            summary.sections[2].bullets,
            [
                "12: boxes a week",
                "forty: people (unclear)",
                "300 rupees a box"
            ]
        );
        assert_eq!(summary.decisions, ["Order from the corner bakery"]);
    }

    #[test]
    fn empty_sections_are_left_out() {
        let summary = notes_from(&json!({"overview": "Quick hello."}), "the meeting")
            .unwrap()
            .notes
            .into_summary();
        assert!(summary.sections.is_empty());
    }

    fn action(owner: &str, due: Option<&str>, line: Option<usize>) -> Action {
        Action {
            item: ActionItem {
                text: "Call the bakery".into(),
                owner: Some(owner.into()),
                due: due.map(Into::into),
            },
            line,
        }
    }

    fn seg(text: &str) -> Segment {
        Segment {
            source: "mic".into(),
            speaker: "Me".into(),
            speaker_key: String::new(),
            start_ms: 0,
            end_ms: 0,
            text: text.into(),
        }
    }

    #[test]
    fn owners_nobody_said_are_dropped() {
        let said = said_words(["Kavya will call the bakery", "Arjun"]);
        let mut notes = Notes {
            action_items: [
                "Speaker 2",
                "Kavya",
                "Me",
                "Rohan",
                "The team",
                "Owner not said",
            ]
            .map(|o| action(o, None, None))
            .into(),
            proposals: vec![Proposal {
                proposal: "Weekly orders".into(),
                by: Some("Speaker 1".into()),
                pushback: None,
                outcome: None,
                quote: None,
            }],
            ..Default::default()
        };
        notes.tidy(&said, true);
        let kept: Vec<Option<&str>> = notes
            .action_items
            .iter()
            .map(|a| a.item.owner.as_deref())
            .collect();
        assert_eq!(kept, [None, Some("Kavya"), Some("Me"), None, None, None]);
        assert!(notes.proposals[0].by.is_none());

        notes.action_items = vec![action("Me", None, None)];
        notes.tidy(&said, false);
        assert!(notes.action_items[0].item.owner.is_none());
    }

    #[test]
    fn due_dates_must_be_said_near_the_action() {
        let segments: Vec<Segment> = [
            "we need fresh bread every week",
            "Kavya: main call kar lungi",
            "by Friday sab ready hona chahiye",
            "okay",
            "chalo",
            "chalo",
            "next month we look at prices",
        ]
        .map(seg)
        .into();
        let mut notes = Notes {
            action_items: vec![
                action("Kavya", Some("by Friday"), Some(1)),
                action("Kavya", Some("by friday"), Some(3)),
                action("Kavya", Some("2026-11-05"), Some(1)),
                action("Kavya", Some("next month"), Some(1)),
                action("Kavya", Some("Friday by"), Some(1)),
                action("Kavya", Some("by Friday"), None),
            ],
            ..Default::default()
        };
        notes.check_dues(&segments);
        let dues: Vec<Option<&str>> = notes
            .action_items
            .iter()
            .map(|a| a.item.due.as_deref())
            .collect();
        assert_eq!(
            dues,
            [Some("by Friday"), Some("by friday"), None, None, None, None]
        );
    }

    #[test]
    fn label_pushback_asides_and_repeated_numbers_are_dropped() {
        let proposal = |pushback: &str| Proposal {
            proposal: "Weekly orders".into(),
            by: None,
            pushback: Some(pushback.into()),
            outcome: None,
            quote: None,
        };
        let number = |value: &str, what: &str| Number {
            value: value.into(),
            what: Some(what.into()),
            unclear: false,
            line: None,
        };
        let mut notes = Notes {
            proposals: vec![
                proposal("Speaker 1"),
                proposal("nahi hai"),
                proposal("Arjun said it wastes food"),
            ],
            open_questions: vec![
                "When? When?".into(),
                "Who signs the invoice?".into(),
                "kal dekh lenge sab log yahan aa jayenge".into(),
            ],
            numbers: vec![
                number("12", "boxes a week"),
                number("12", "Boxes a week"),
                number("12", "people"),
            ],
            ..Default::default()
        };
        notes.tidy(&HashSet::new(), true);
        let pushback: Vec<Option<&str>> = notes
            .proposals
            .iter()
            .map(|p| p.pushback.as_deref())
            .collect();
        assert_eq!(pushback, [None, None, Some("Arjun said it wastes food")]);
        assert_eq!(notes.open_questions, ["Who signs the invoice?"]);
        assert_eq!(notes.numbers.len(), 2);

        notes.decisions = [
            "Use a structured approach to plan the menu",
            "Ship on the 3rd",
        ]
        .map(|text| Decision {
            text: text.into(),
            quote: None,
        })
        .into();
        notes.key_points = vec!["The team emphasized the importance of data integrity".into()];
        notes.tidy(&HashSet::new(), true);
        assert_eq!(notes.decisions.len(), 1);
        assert_eq!(notes.decisions[0].text, "Ship on the 3rd");
        assert!(notes.key_points.is_empty());
    }

    #[test]
    fn numbers_not_written_on_their_line_are_unclear() {
        let seg = |text: &str| Segment {
            source: "mic".into(),
            speaker: "Me".into(),
            speaker_key: String::new(),
            start_ms: 0,
            end_ms: 0,
            text: text.into(),
        };
        let segments = [
            seg("we ordered for the office"),
            seg("twelve boxes came, 85% were fine"),
            seg("and 3 were late"),
        ];
        let n = |value: &str, line: Option<usize>| Number {
            value: value.into(),
            what: None,
            unclear: false,
            line,
        };
        assert!(number_is_on_line(&n("85%", Some(1)), &segments));
        assert!(number_is_on_line(&n("3", Some(1)), &segments));
        assert!(number_is_on_line(&n("twelve", Some(1)), &segments));
        assert!(!number_is_on_line(&n("3", Some(0)), &segments));
        assert!(!number_is_on_line(&n("90%", Some(1)), &segments));
        assert!(!number_is_on_line(&n("85%", None), &segments));
        assert!(!number_is_on_line(&n("85%", Some(40)), &segments));
        assert!(number_heard(&n("3", None), &segments));
        assert!(!number_heard(&n("12 of 40", None), &segments));
    }

    #[test]
    fn decisions_need_a_quote_that_agrees() {
        let seg = |text: &str| Segment {
            source: "mic".into(),
            speaker: "Me".into(),
            speaker_key: String::new(),
            start_ms: 0,
            end_ms: 0,
            text: text.into(),
        };
        let segments = [
            seg("Kavya: the menu should list prices too."),
            seg("Arjun: haan theek hai, main add kar dunga"),
            seg("tum bol rahe ho tasting day karna chahiye, lekin tasting day nahi karenge"),
            seg("and we stop ordering from the old bakery"),
        ];
        let decision = |text: &str, quote: Option<&str>| Decision {
            text: text.into(),
            quote: quote.map(Into::into),
        };
        let mut notes = Notes {
            decisions: vec![
                decision(
                    "Add prices to the menu",
                    Some("menu should list prices too. haan theek hai"),
                ),
                decision(
                    "Hold a tasting day",
                    Some("karna chahiye, lekin tasting day nahi karenge"),
                ),
                decision(
                    "Stop ordering from the old bakery",
                    Some("we stop ordering from the old bakery"),
                ),
                decision("Hire a chef", Some("we will hire a new chef next month")),
                decision("Order cake", None),
                decision("Add a menu", Some("menu prices")),
            ],
            proposals: vec![
                Proposal {
                    proposal: "List prices".into(),
                    by: None,
                    pushback: None,
                    outcome: None,
                    quote: Some("the menu should list prices too".into()),
                },
                Proposal {
                    proposal: "Weekly orders".into(),
                    by: None,
                    pushback: None,
                    outcome: None,
                    quote: Some("let us order every week".into()),
                },
            ],
            ..Default::default()
        };
        notes.check_quotes(&segments);
        let decisions: Vec<&str> = notes.decisions.iter().map(|d| d.text.as_str()).collect();
        assert_eq!(
            decisions,
            [
                "Add prices to the menu",
                "Stop ordering from the old bakery"
            ]
        );
        let proposals: Vec<(&str, Option<&str>)> = notes
            .proposals
            .iter()
            .map(|p| (p.proposal.as_str(), p.outcome.as_deref()))
            .collect();
        assert_eq!(
            proposals,
            [
                ("List prices", None),
                ("Hold a tasting day", Some("not settled"))
            ]
        );

        let summary = notes.into_summary();
        assert_eq!(summary.decisions.len(), 2);
        assert_eq!(summary.quotes.len(), 4);
        assert_eq!(summary.quotes[0].item, "Add prices to the menu");
        assert!(!summary.sections[0].bullets[1].contains("nahi karenge"));
    }

    #[test]
    fn only_a_refusal_of_the_decisions_own_action_demotes_it() {
        assert!(!refuses(
            "Remove the export button",
            "export button ki zarurat nahi, hata do"
        ));
        assert!(!refuses(
            "Launch after the API is ready",
            "don't launch until the API is ready"
        ));
        assert!(refuses("Do manual review", "manual review nahi karenge"));
        assert!(refuses(
            "Send the deck to the client",
            "we won't send the deck"
        ));
        assert!(!refuses("Send the deck", "haan theek hai, deck bhej dunga"));
        assert!(!refuses("No weekly orders", "weekly orders nahi chahiye"));
    }

    fn quoted(text: &str, quote: &str) -> Decision {
        Decision {
            text: text.into(),
            quote: Some(quote.into()),
        }
    }

    #[test]
    fn merge_keeps_the_parts_checked_decisions() {
        let parts = vec![
            Notes {
                decisions: vec![quoted(
                    "Add prices to the menu",
                    "haan prices add kar dunga",
                )],
                proposals: vec![Proposal {
                    proposal: "Switch to weekly orders".into(),
                    by: None,
                    pushback: None,
                    outcome: Some("left open".into()),
                    quote: Some("weekly orders kar sakte hain".into()),
                }],
                action_items: vec![action("Kavya", Some("by Friday"), Some(2))],
                ..Default::default()
            },
            Notes {
                decisions: vec![quoted(
                    "Keep the corner bakery",
                    "corner bakery hi rakhenge",
                )],
                ..Default::default()
            },
        ];
        let merge = Notes {
            decisions: vec![
                Decision {
                    text: "Add prices to the menu card".into(),
                    quote: None,
                },
                Decision {
                    text: "Switch to weekly orders".into(),
                    quote: None,
                },
                Decision {
                    text: "Hire a new chef".into(),
                    quote: None,
                },
            ],
            action_items: vec![
                action("Kavya", Some("by Friday"), None),
                action("Arjun", Some("tomorrow"), None),
            ],
            ..Default::default()
        };
        let notes = reconcile(merge, &parts);
        let decisions: Vec<(&str, Option<&str>)> = notes
            .decisions
            .iter()
            .map(|d| (d.text.as_str(), d.quote.as_deref()))
            .collect();
        assert_eq!(
            decisions,
            [
                (
                    "Add prices to the menu card",
                    Some("haan prices add kar dunga")
                ),
                ("Keep the corner bakery", Some("corner bakery hi rakhenge")),
            ]
        );
        assert_eq!(notes.proposals.len(), 1);
        assert_eq!(notes.proposals[0].outcome.as_deref(), Some("left open"));
        let dues: Vec<Option<&str>> = notes
            .action_items
            .iter()
            .map(|a| a.item.due.as_deref())
            .collect();
        assert_eq!(dues, [Some("by Friday"), None]);
    }

    #[test]
    fn a_failed_merge_joins_the_parts_in_order() {
        let model = Model::Ollama(Ollama::new("http://127.0.0.1:9", "none"));
        let parts: Vec<Notes> = (1..=6)
            .map(|i| Notes {
                overview: format!("Part {i}."),
                decisions: vec![quoted(&format!("Decision {i}"), "words")],
                ..Default::default()
            })
            .collect();
        let notes = merged(&model, &meeting("Office lunch", false), parts);
        let decisions: Vec<&str> = notes.decisions.iter().map(|d| d.text.as_str()).collect();
        assert_eq!(
            decisions,
            [
                "Decision 1",
                "Decision 2",
                "Decision 3",
                "Decision 4",
                "Decision 5",
                "Decision 6"
            ]
        );
        assert_eq!(
            notes.overview,
            "Part 1. Part 2. Part 3. Part 4. Part 5. Part 6."
        );
    }

    #[test]
    fn quotes_match_hinglish_spellings() {
        let lines: Vec<HashSet<String>> = ["haan thik hai, main add kar doonga"]
            .iter()
            .map(|t| sound_words(t).collect())
            .collect();
        assert!(quote_found("haan theek he, main add kar dunga", &lines));
        assert!(!quote_found("we will order a new oven", &lines));
    }

    #[test]
    fn decisions_and_questions_may_carry_lines() {
        let v = json!({
            "decisions": [{"text": "Flag them, don't overwrite", "line": 4}, "Keep Friday"],
            "open_questions": [{"text": "Who signs?", "line": "#9"}, {"line": 3}],
            "numbers": [{"value": "12", "what": "boxes", "line": "#2"}]
        });
        let notes = notes_from(&v, "the meeting").unwrap().notes;
        let decisions: Vec<&str> = notes.decisions.iter().map(|d| d.text.as_str()).collect();
        assert_eq!(decisions, ["Flag them, don't overwrite", "Keep Friday"]);
        assert_eq!(notes.open_questions, ["Who signs?"]);
        assert_eq!(notes.numbers[0].line, Some(2));
    }

    #[test]
    fn notes_prompt_asks_for_the_new_sections_and_bans_filler() {
        let prompt = notes_prompt(
            &meeting("Data Standup", false),
            "",
            "#0 Me: hi",
            Some((1, 2)),
        );
        for key in [
            "\"decisions\"",
            "\"proposals\"",
            "\"open_questions\"",
            "\"numbers\"",
        ] {
            assert!(prompt.contains(key), "{key}");
        }
        assert!(prompt.contains("importance of data integrity"));
        assert!(prompt.contains("keep it as a proposal or open question"));
    }

    #[test]
    fn merge_keeps_proposals_apart_from_decisions() {
        let parts = [
            Notes {
                overview: "Part one.".into(),
                proposals: vec![Proposal {
                    proposal: "Weekly orders".into(),
                    by: None,
                    pushback: Some("too much waste".into()),
                    outcome: Some("left open".into()),
                    quote: None,
                }],
                ..Default::default()
            },
            Notes {
                overview: "Part two.".into(),
                decisions: vec![Decision {
                    text: "Order from the corner bakery".into(),
                    quote: None,
                }],
                ..Default::default()
            },
        ];
        let prompt = merge_prompt(&meeting("Office lunch", false), &parts).unwrap();
        let start = prompt.find('[').unwrap();
        let end = prompt.find("\n\nMerge").unwrap();
        let sent: Value = serde_json::from_str(&prompt[start..end]).unwrap();
        assert_eq!(sent[0]["proposals"][0]["outcome"], "left open");
        assert_eq!(sent[0]["decisions"], json!([]));
        assert_eq!(
            sent[1]["decisions"][0]["text"],
            "Order from the corner bakery"
        );
        assert!(prompt.contains("Never turn a proposal"));
    }

    /// Writes notes for one saved meeting with the built-in model, for
    /// comparing prompts on real meetings (kept private):
    /// LIPIK_TEST_DB=<copy of notetaker.db> LIPIK_TEST_MEETING=<id>
    /// LIPIK_TEST_GGUF=<model file> LIPIK_TEST_OUT=<out.json> cargo test --lib
    /// notes_check -- --ignored --nocapture. Needs a llama-server next to the
    /// test binary. LIPIK_TEST_SEGMENTS=<file> first cleans the transcript as
    /// a new recording would (Roman letters, repeats), saving it to that file,
    /// or reads it from there when it exists.
    #[test]
    #[ignore]
    fn notes_check() {
        struct StopEngine;
        impl Drop for StopEngine {
            fn drop(&mut self) {
                ENGINE.stop();
            }
        }
        let _stop = StopEngine;
        let var = |k: &str| std::env::var(k).unwrap_or_else(|_| panic!("{k} not set"));
        let id = var("LIPIK_TEST_MEETING");
        let store =
            super::super::store::Store::open_read_only(std::path::Path::new(&var("LIPIK_TEST_DB")))
                .unwrap();
        let meeting = store.meeting(&id).unwrap().expect("no such meeting");
        let mut segments = store.segments(&id).unwrap();
        let thoughts = store.thoughts(&id).unwrap();
        let model = Model::Builtin {
            path: PathBuf::from(var("LIPIK_TEST_GGUF")),
            name: "eval".to_string(),
        };
        if let Ok(file) = std::env::var("LIPIK_TEST_SEGMENTS") {
            match std::fs::read(&file) {
                Ok(bytes) => segments = serde_json::from_slice(&bytes).unwrap(),
                Err(_) => {
                    let names = diarize::invitees(&meeting.attendees);
                    let length = meeting.end_ms.unwrap_or(meeting.start_ms) - meeting.start_ms;
                    super::super::hinglish::romanize(
                        Some(&model),
                        &mut segments,
                        &names,
                        super::super::hinglish::romanize_time(length),
                        |_| true,
                    );
                    super::super::tidy::tidy(
                        &mut segments,
                        &super::super::tidy::Vocabulary::new(&[], &names),
                    );
                    std::fs::write(&file, serde_json::to_vec_pretty(&segments).unwrap()).unwrap();
                }
            }
        }
        let started = std::time::Instant::now();
        let parts = part_notes(&model, &meeting, &thoughts, &segments).unwrap();
        let out = var("LIPIK_TEST_OUT");
        std::fs::write(
            format!("{out}.parts.json"),
            serde_json::to_string_pretty(&parts.notes).unwrap(),
        )
        .unwrap();
        println!("skipped parts: {:?}", parts.skipped);
        let notes = merged(&model, &meeting, parts.notes);
        let summary = checked(notes, &meeting, &segments).into_summary();
        println!("{:.0} s", started.elapsed().as_secs_f32());
        std::fs::write(out, serde_json::to_string_pretty(&summary).unwrap()).unwrap();
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
