//! `lipik --mcp`: meeting notes for AI apps (Claude, Cursor, Codex, ...) over
//! the Model Context Protocol's stdio transport, one JSON-RPC message per
//! line. Read-only, local only, and no Tauri: the app may be running and
//! writing at the same time. Started by the AI app, never by Lipik.

use super::{store::Store, Meeting, MeetingDetail, NotetakerFilter, Segment};
use chrono::{Local, NaiveDate, TimeZone};
use serde_json::{json, Value};
use std::io::{BufRead, Write};
use std::path::PathBuf;

const IDENTIFIER: &str = "com.gourav.lipik";
const LINK_PREFIX: &str = "lipik://meeting/";
/// The protocol version answered when a client asks for one we do not know.
const PROTOCOL_VERSION: &str = "2025-06-18";
const KNOWN_VERSIONS: &[&str] = &["2024-11-05", "2025-03-26", "2025-06-18"];
const LIST_DEFAULT: u32 = 20;
const LIST_MAX: u32 = 100;

pub fn is_invocation() -> bool {
    std::env::args().nth(1).as_deref() == Some("--mcp")
}

/// Runs until the AI app closes stdin. Returns the process exit code.
pub fn run() -> i32 {
    crate::portable::init();
    let db = data_dir().map(|d| d.join("notetaker").join("notetaker.db"));
    let stdin = std::io::stdin();
    let mut stdout = std::io::stdout();
    for line in stdin.lock().lines() {
        let Ok(line) = line else { break };
        if line.trim().is_empty() {
            continue;
        }
        let reply = match serde_json::from_str::<Value>(&line) {
            Ok(msg) => handle(&msg, db.as_ref()),
            Err(_) => Some(error(Value::Null, -32700, "Parse error")),
        };
        if let Some(reply) = reply {
            if writeln!(stdout, "{reply}")
                .and_then(|_| stdout.flush())
                .is_err()
            {
                break;
            }
        }
    }
    0
}

fn data_dir() -> Option<PathBuf> {
    if let Some(dir) = crate::portable::data_dir() {
        return Some(dir.clone());
    }
    let home = std::env::var_os("HOME").map(PathBuf::from);
    if cfg!(target_os = "macos") {
        return home.map(|h| h.join("Library/Application Support").join(IDENTIFIER));
    }
    if cfg!(target_os = "windows") {
        return std::env::var_os("APPDATA").map(|a| PathBuf::from(a).join(IDENTIFIER));
    }
    std::env::var_os("XDG_DATA_HOME")
        .map(PathBuf::from)
        .or_else(|| home.map(|h| h.join(".local/share")))
        .map(|d| d.join(IDENTIFIER))
}

fn handle(msg: &Value, db: Option<&PathBuf>) -> Option<Value> {
    let id = msg.get("id").cloned();
    let method = msg
        .get("method")
        .and_then(Value::as_str)
        .unwrap_or_default();
    // Notifications (no id) never get a reply.
    let id = id?;
    let params = msg.get("params").cloned().unwrap_or(Value::Null);
    Some(match method {
        "initialize" => {
            let asked = params
                .get("protocolVersion")
                .and_then(Value::as_str)
                .unwrap_or(PROTOCOL_VERSION);
            let version = if KNOWN_VERSIONS.contains(&asked) {
                asked
            } else {
                PROTOCOL_VERSION
            };
            result(
                id,
                json!({
                    "protocolVersion": version,
                    "capabilities": { "tools": {} },
                    "serverInfo": { "name": "lipik", "version": env!("CARGO_PKG_VERSION") },
                    "instructions": "Meeting notes recorded by Lipik on this computer. \
                        A link like lipik://meeting/<id> names one meeting: pass it to get_meeting. \
                        Speaker 'Me' is the Lipik user; 'Others' is everyone else on the call.",
                }),
            )
        }
        "ping" => result(id, json!({})),
        "tools/list" => result(id, json!({ "tools": tools() })),
        "tools/call" => {
            let name = params
                .get("name")
                .and_then(Value::as_str)
                .unwrap_or_default();
            let args = params.get("arguments").cloned().unwrap_or(json!({}));
            let (text, failed) = match call(name, &args, db) {
                Ok(text) => (text, false),
                Err(e) => (e, true),
            };
            result(
                id,
                json!({ "content": [{ "type": "text", "text": text }], "isError": failed }),
            )
        }
        _ => error(id, -32601, "Method not found"),
    })
}

fn result(id: Value, result: Value) -> Value {
    json!({ "jsonrpc": "2.0", "id": id, "result": result })
}

fn error(id: Value, code: i64, message: &str) -> Value {
    json!({ "jsonrpc": "2.0", "id": id, "error": { "code": code, "message": message } })
}

fn tools() -> Value {
    let read_only = json!({ "readOnlyHint": true, "openWorldHint": false });
    json!([
        {
            "name": "list_meetings",
            "description": "Recent meetings recorded by Lipik, newest first, with date, length, attendees and a lipik://meeting/ link for get_meeting.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "limit": { "type": "integer", "description": "How many (default 20, at most 100)." },
                    "after": { "type": "string", "description": "Only meetings on or after this day, YYYY-MM-DD." },
                    "before": { "type": "string", "description": "Only meetings before this day, YYYY-MM-DD." }
                }
            },
            "annotations": read_only,
        },
        {
            "name": "search_meetings",
            "description": "Meetings whose title, transcript, notes or summary contain the words.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "query": { "type": "string", "description": "Words to look for." },
                    "limit": { "type": "integer", "description": "How many (default 20, at most 100)." }
                },
                "required": ["query"]
            },
            "annotations": read_only,
        },
        {
            "name": "get_meeting",
            "description": "One meeting's summary, decisions, next steps, the user's own notes and the full transcript with speakers, as Markdown.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "meeting": { "type": "string", "description": "A lipik://meeting/<id> link or the meeting id." },
                    "transcript": { "type": "boolean", "description": "Include the transcript (default true)." },
                    "part": { "type": "integer", "description": "Which part of a long transcript (default 1); the answer says how many parts there are." }
                },
                "required": ["meeting"]
            },
            "annotations": read_only,
        }
    ])
}

fn call(name: &str, args: &Value, db: Option<&PathBuf>) -> Result<String, String> {
    let store = || -> Result<Option<Store>, String> {
        match db {
            Some(path) if path.exists() => Store::open_read_only(path)
                .map(Some)
                .map_err(|e| format!("Could not read Lipik's meeting notes: {e}")),
            _ => Ok(None),
        }
    };
    let limit = args
        .get("limit")
        .and_then(Value::as_u64)
        .map_or(LIST_DEFAULT, |n| (n as u32).clamp(1, LIST_MAX));
    match name {
        "list_meetings" => {
            let filter = NotetakerFilter {
                after_ms: day_ms(args.get("after"))?,
                before_ms: day_ms(args.get("before"))?,
                limit: Some(limit),
                ..Default::default()
            };
            let Some(store) = store()? else {
                return Ok(NO_MEETINGS.to_string());
            };
            let meetings = store.list(&filter).map_err(|e| e.to_string())?;
            Ok(meeting_list(&meetings))
        }
        "search_meetings" => {
            let query = args
                .get("query")
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|q| !q.is_empty())
                .ok_or("Give some words to search for.")?;
            let Some(store) = store()? else {
                return Ok(NO_MEETINGS.to_string());
            };
            let meetings = search_words(&store, query, limit)?;
            if meetings.is_empty() {
                return Ok(format!("No meetings mention \"{query}\"."));
            }
            Ok(meeting_list(&meetings))
        }
        "get_meeting" => {
            let wanted = args
                .get("meeting")
                .and_then(Value::as_str)
                .map(meeting_id)
                .filter(|id| !id.is_empty())
                .ok_or("Give a lipik://meeting/ link or a meeting id.")?;
            let part =
                (args.get("transcript").and_then(Value::as_bool) != Some(false)).then(|| {
                    args.get("part")
                        .and_then(Value::as_u64)
                        .map_or(1, |n| n.max(1) as usize)
                });
            let Some(store) = store()? else {
                return Err(NO_MEETINGS.to_string());
            };
            let detail = store
                .detail(&wanted)
                .map_err(|e| format!("Could not read Lipik's meeting notes: {e}"))?
                .ok_or_else(|| format!("No meeting {wanted} in Lipik on this computer."))?;
            Ok(meeting_markdown(&detail, part))
        }
        _ => Err(format!("Unknown tool {name}.")),
    }
}

/// Every word must appear somewhere in the meeting ("budget review" finds
/// "review the budget").
fn search_words(store: &Store, query: &str, limit: u32) -> Result<Vec<Meeting>, String> {
    let mut found: Option<Vec<Meeting>> = None;
    for word in query.split_whitespace() {
        let filter = NotetakerFilter {
            query: Some(word.to_string()),
            limit: Some(1000),
            ..Default::default()
        };
        let hits = store.list(&filter).map_err(|e| e.to_string())?;
        found = Some(match found {
            None => hits,
            Some(prev) => prev
                .into_iter()
                .filter(|m| hits.iter().any(|h| h.id == m.id))
                .collect(),
        });
    }
    let mut found = found.unwrap_or_default();
    found.truncate(limit as usize);
    Ok(found)
}

/// About 15,000 words: AI apps cap how much one tool answer may hold.
const TRANSCRIPT_PART_CHARS: usize = 60_000;

/// One part of a long transcript, cut between speakers where it can be.
fn transcript_part(text: &str, part: usize) -> (String, usize) {
    let mut parts: Vec<&str> = Vec::new();
    let mut rest = text;
    while rest.len() > TRANSCRIPT_PART_CHARS {
        let mut cut = TRANSCRIPT_PART_CHARS;
        while !rest.is_char_boundary(cut) {
            cut -= 1;
        }
        let cut = rest[..cut].rfind("\n\n").filter(|&i| i > 0).unwrap_or(cut);
        parts.push(&rest[..cut]);
        rest = rest[cut..].trim_start();
    }
    parts.push(rest);
    let total = parts.len();
    (
        parts.get(part - 1).copied().unwrap_or_default().to_string(),
        total,
    )
}

const NO_MEETINGS: &str = "No meetings recorded with Lipik on this computer yet.";

/// "lipik://meeting/20261027-163442-a728", with or without the prefix.
pub fn meeting_id(link_or_id: &str) -> String {
    let s = link_or_id.trim();
    s.strip_prefix(LINK_PREFIX)
        .unwrap_or(s)
        .trim_end_matches('/')
        .to_string()
}

pub fn meeting_link(id: &str) -> String {
    format!("{LINK_PREFIX}{id}")
}

fn day_ms(v: Option<&Value>) -> Result<Option<i64>, String> {
    let Some(text) = v.and_then(Value::as_str) else {
        return Ok(None);
    };
    let day = NaiveDate::parse_from_str(text.trim(), "%Y-%m-%d")
        .map_err(|_| format!("\"{text}\" is not a day like 2026-10-27."))?;
    let start = day.and_hms_opt(0, 0, 0).expect("midnight");
    Ok(Local
        .from_local_datetime(&start)
        .earliest()
        .map(|t| t.timestamp_millis()))
}

fn when(ms: i64) -> String {
    Local
        .timestamp_millis_opt(ms)
        .single()
        .map_or_else(String::new, |t| t.format("%a %-d %b %Y, %H:%M").to_string())
}

fn minutes(m: &Meeting) -> Option<i64> {
    m.end_ms
        .map(|end| ((end - m.start_ms) as f64 / 60_000.0).round() as i64)
}

fn attendee_names(m: &Meeting) -> Vec<String> {
    m.attendees
        .iter()
        .filter_map(|a| a.name.clone().or_else(|| a.email.clone()))
        .collect()
}

/// Names in a meeting list; get_meeting shows them all.
const LIST_PEOPLE: usize = 8;

fn short_list(people: &[String]) -> String {
    if people.len() <= LIST_PEOPLE {
        return people.join(", ");
    }
    format!(
        "{} and {} more",
        people[..LIST_PEOPLE].join(", "),
        people.len() - LIST_PEOPLE
    )
}

fn meeting_list(meetings: &[Meeting]) -> String {
    if meetings.is_empty() {
        return NO_MEETINGS.to_string();
    }
    meetings
        .iter()
        .map(|m| {
            let mut line = format!("- {} | {}", m.title, when(m.start_ms));
            match minutes(m) {
                Some(n) => line.push_str(&format!(" | {n} min")),
                None => line.push_str(" | still recording"),
            }
            let people = attendee_names(m);
            if !people.is_empty() {
                line.push_str(&format!(" | with {}", short_list(&people)));
            }
            line.push_str(&format!(" | {}", meeting_link(&m.id)));
            line
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn speaker(name: &str) -> &str {
    match name {
        "Them" => "Others",
        other => other,
    }
}

fn transcript(segments: &[Segment]) -> String {
    let mut blocks: Vec<(String, Vec<String>)> = Vec::new();
    for s in segments {
        let text = s.text.trim();
        if text.is_empty() {
            continue;
        }
        match blocks.last_mut() {
            Some((who, lines)) if who == speaker(&s.speaker) => lines.push(text.to_string()),
            _ => blocks.push((speaker(&s.speaker).to_string(), vec![text.to_string()])),
        }
    }
    blocks
        .iter()
        .map(|(who, lines)| format!("{who}: {}", lines.join(" ")))
        .collect::<Vec<_>>()
        .join("\n\n")
}

/// The note as Markdown, in the notes window's "Copy as Markdown" layout,
/// with the date, attendees and link on top for the AI.
/// `part`: which part of the transcript to add (`None`: no transcript).
pub fn meeting_markdown(d: &MeetingDetail, part: Option<usize>) -> String {
    let m = &d.meeting;
    let mut out = vec![format!(
        "# {}",
        if m.title.is_empty() {
            "Untitled"
        } else {
            &m.title
        }
    )];
    let mut about = when(m.start_ms);
    if let Some(n) = minutes(m) {
        about.push_str(&format!(", {n} min"));
    } else {
        about.push_str(", still recording");
    }
    out.push(about);
    let people = attendee_names(m);
    if !people.is_empty() {
        out.push(format!("Attendees: {}", people.join(", ")));
    }
    out.push(meeting_link(&m.id));
    out.push(String::new());
    if !d.thoughts.trim().is_empty() {
        out.extend([
            "## My notes".into(),
            String::new(),
            d.thoughts.trim().into(),
            String::new(),
        ]);
    }
    if let Some(s) = &d.summary {
        if !s.overview.trim().is_empty() {
            out.extend([s.overview.trim().to_string(), String::new()]);
        }
        let mut list = |heading: &str, items: Vec<String>| {
            if items.is_empty() {
                return;
            }
            out.push(format!("## {heading}"));
            out.extend(items.into_iter().map(|b| format!("- {b}")));
            out.push(String::new());
        };
        for section in &s.sections {
            let heading = if section.heading.is_empty() {
                "Notes"
            } else {
                &section.heading
            };
            list(heading, section.bullets.clone());
        }
        list("Key points", s.key_points.clone());
        list("Decisions", s.decisions.clone());
        list(
            "Next steps",
            s.action_items
                .iter()
                .map(|a| {
                    let owner = a
                        .owner
                        .as_ref()
                        .map(|o| format!("{o}: "))
                        .unwrap_or_default();
                    let due = a
                        .due
                        .as_ref()
                        .map(|d| format!(" (by {d})"))
                        .unwrap_or_default();
                    format!("{owner}{}{due}", a.text)
                })
                .collect(),
        );
    } else {
        out.extend(["No summary yet.".into(), String::new()]);
    }
    if let Some(part) = part.filter(|_| !d.segments.is_empty()) {
        let (text, total) = transcript_part(&transcript(&d.segments), part);
        if total > 1 {
            out.extend([
                format!("## Transcript (part {part} of {total})"),
                String::new(),
                text,
            ]);
            if part < total {
                out.extend([
                    String::new(),
                    format!("(Ask get_meeting with part {} for more.)", part + 1),
                ]);
            }
        } else {
            out.extend(["## Transcript".into(), String::new(), text]);
        }
    }
    out.join("\n").trim().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ask(msg: Value) -> Option<Value> {
        handle(&msg, None)
    }

    #[test]
    fn initialize_lists_tools_and_answers_without_meetings() {
        let init = ask(json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-03-26"}})).unwrap();
        assert_eq!(init["result"]["protocolVersion"], "2025-03-26");
        assert_eq!(init["result"]["serverInfo"]["name"], "lipik");
        assert!(ask(json!({"jsonrpc":"2.0","method":"notifications/initialized"})).is_none());
        let list = ask(json!({"jsonrpc":"2.0","id":2,"method":"tools/list"})).unwrap();
        let names: Vec<&str> = list["result"]["tools"]
            .as_array()
            .unwrap()
            .iter()
            .map(|t| t["name"].as_str().unwrap())
            .collect();
        assert_eq!(names, ["list_meetings", "search_meetings", "get_meeting"]);
        let call = ask(json!({"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"list_meetings","arguments":{}}})).unwrap();
        assert_eq!(call["result"]["content"][0]["text"], NO_MEETINGS);
        assert_eq!(call["result"]["isError"], false);
        let bad = ask(json!({"jsonrpc":"2.0","id":4,"method":"nope"})).unwrap();
        assert_eq!(bad["error"]["code"], -32601);
    }

    #[test]
    fn bad_arguments_are_tool_errors() {
        let call = ask(json!({"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"list_meetings","arguments":{"after":"yesterday"}}})).unwrap();
        assert_eq!(call["result"]["isError"], true);
        let call = ask(json!({"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"get_meeting","arguments":{}}})).unwrap();
        assert_eq!(call["result"]["isError"], true);
    }

    #[test]
    fn links_and_ids_name_the_same_meeting() {
        assert_eq!(
            meeting_id("lipik://meeting/20261027-163442-a728"),
            "20261027-163442-a728"
        );
        assert_eq!(meeting_id(" 20261027-163442-a728 "), "20261027-163442-a728");
        assert_eq!(meeting_link("abc"), "lipik://meeting/abc");
    }

    #[test]
    fn long_transcripts_come_in_parts() {
        let block = format!("Me: {}", "word ".repeat(2_000));
        let text = vec![block; 40].join("\n\n");
        let (first, total) = transcript_part(&text, 1);
        assert!(total >= 4, "{total}");
        assert!(first.len() <= TRANSCRIPT_PART_CHARS);
        assert!(first.ends_with("word "), "cut between speakers");
        let (last, _) = transcript_part(&text, total);
        assert!(!last.is_empty());
        assert_eq!(transcript_part(&text, total + 1).0, "");
        assert_eq!(transcript_part("short", 1), ("short".to_string(), 1));
    }

    #[test]
    fn transcript_groups_speakers() {
        let seg = |speaker: &str, text: &str| Segment {
            source: "mic".into(),
            speaker: speaker.into(),
            speaker_key: String::new(),
            start_ms: 0,
            end_ms: 0,
            text: text.into(),
        };
        let t = transcript(&[
            seg("Me", "Hi."),
            seg("Me", "Ready?"),
            seg("Them", "Yes."),
            seg("Priya", "Go."),
        ]);
        assert_eq!(t, "Me: Hi. Ready?\n\nOthers: Yes.\n\nPriya: Go.");
    }
}
