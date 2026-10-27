//! Adding Lipik's meeting tools (`lipik --mcp`) to AI apps. Each app keeps
//! its MCP servers in its own file; Lipik edits only the "lipik" entry, keeps
//! a copy of the file as it was before the first edit, and never touches an
//! app that is not installed. Apps that cannot run local tools (ChatGPT) get
//! the copy-a-note route instead.

use serde::Serialize;
use serde_json::{json, Map, Value};
use specta::Type;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

const ENTRY: &str = "lipik";
const BACKUP_SUFFIX: &str = ".before-lipik";
const INSTALLED: &str = "/Applications/Lipik.app/Contents/MacOS/lipik";
const CLAUDE_TIMEOUT: Duration = Duration::from_secs(30);

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum AiAppHow {
    /// Lipik edits the app's MCP settings file.
    File,
    /// Lipik runs the app's own command (`claude mcp add`).
    Command,
    /// The app cannot run local tools: copy a note instead.
    Copy,
}

#[derive(Clone, Debug, Serialize, Type)]
pub struct AiApp {
    pub id: String,
    pub name: String,
    pub installed: bool,
    pub connected: bool,
    pub how: AiAppHow,
    /// The settings file Lipik edits, shown to the user.
    pub file: Option<String>,
    /// What to do after connecting ("Restart Claude"), or why it can't.
    pub note: Option<String>,
    /// The same setup to paste by hand.
    pub manual: String,
}

#[derive(Clone, Copy)]
enum Kind {
    /// `{ "<key>": { "lipik": { "command", "args" } } }`
    Json {
        key: &'static str,
        vscode: bool,
    },
    Toml,
    ClaudeCode,
    CopyOnly,
}

struct Spec {
    id: &'static str,
    name: &'static str,
    kind: Kind,
    /// Settings file, from the home folder.
    file: &'static str,
    /// Any of these existing (from home, or absolute) means installed.
    marks: &'static [&'static str],
    note: Option<&'static str>,
}

const SPECS: &[Spec] = &[
    Spec {
        id: "claude_desktop",
        name: "Claude",
        kind: Kind::Json { key: "mcpServers", vscode: false },
        file: "Library/Application Support/Claude/claude_desktop_config.json",
        marks: &["/Applications/Claude.app", "Library/Application Support/Claude"],
        note: Some("Quit and reopen Claude to load Lipik."),
    },
    Spec {
        id: "claude_code",
        name: "Claude Code",
        kind: Kind::ClaudeCode,
        file: ".claude.json",
        marks: &[".claude.json", ".claude"],
        note: Some("New Claude Code sessions will see Lipik."),
    },
    Spec {
        id: "cursor",
        name: "Cursor",
        kind: Kind::Json { key: "mcpServers", vscode: false },
        file: ".cursor/mcp.json",
        marks: &["/Applications/Cursor.app", ".cursor"],
        note: None,
    },
    Spec {
        id: "windsurf",
        name: "Windsurf",
        kind: Kind::Json { key: "mcpServers", vscode: false },
        file: ".codeium/windsurf/mcp_config.json",
        marks: &["/Applications/Windsurf.app", ".codeium/windsurf"],
        note: None,
    },
    Spec {
        id: "vscode",
        name: "VS Code",
        kind: Kind::Json { key: "servers", vscode: true },
        file: "Library/Application Support/Code/User/mcp.json",
        marks: &["/Applications/Visual Studio Code.app"],
        note: Some("Start Lipik from the MCP servers list in Copilot Chat."),
    },
    Spec {
        id: "codex",
        name: "Codex",
        kind: Kind::Toml,
        file: ".codex/config.toml",
        marks: &["/Applications/Codex.app", ".codex"],
        note: None,
    },
    Spec {
        id: "gemini",
        name: "Gemini CLI",
        kind: Kind::Json { key: "mcpServers", vscode: false },
        file: ".gemini/settings.json",
        marks: &[".gemini"],
        note: None,
    },
    Spec {
        id: "chatgpt",
        name: "ChatGPT",
        kind: Kind::CopyOnly,
        file: "",
        marks: &["/Applications/ChatGPT.app"],
        note: Some("ChatGPT can't use tools on your Mac. Open a note and use Copy as Markdown, then paste it into ChatGPT."),
    },
];

fn home() -> Option<PathBuf> {
    std::env::var_os("HOME").map(PathBuf::from)
}

fn from_home(home: &Path, p: &str) -> PathBuf {
    if p.starts_with('/') {
        PathBuf::from(p)
    } else {
        home.join(p)
    }
}

/// `lipik` itself, as AI apps should start it with `--mcp`: this copy when it
/// lives in an Applications folder, else the installed app. Never a copy that
/// goes away: the mounted .dmg, the temporary copy macOS runs an app from
/// before it is moved ("App Translocation"), or a dev build.
fn server_command() -> Result<String, String> {
    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    let path = exe.to_string_lossy().into_owned();
    let home_apps = home().map(|h| h.join("Applications").to_string_lossy().into_owned());
    let in_applications = path.starts_with("/Applications/")
        || home_apps.is_some_and(|a| path.starts_with(&format!("{a}/")));
    if in_applications && !path.contains("AppTranslocation") {
        return Ok(path);
    }
    if Path::new(INSTALLED).is_file() {
        return Ok(INSTALLED.to_string());
    }
    Err("Move Lipik to your Applications folder and open it from there, then connect.".into())
}

fn entry(vscode: bool, command: &str) -> Value {
    if vscode {
        json!({ "type": "stdio", "command": command, "args": ["--mcp"] })
    } else {
        json!({ "command": command, "args": ["--mcp"] })
    }
}

fn manual(kind: Kind, file: &str, command: &str) -> String {
    match kind {
        Kind::Json { key, vscode } => format!(
            "Add to ~/{file}:\n{}",
            serde_json::to_string_pretty(&json!({ key: { ENTRY: entry(vscode, command) } }))
                .unwrap_or_default()
        ),
        Kind::Toml => format!("Add to ~/{file}:\n{}", toml_block(command)),
        Kind::ClaudeCode => {
            format!(
                "Run in a terminal:\nclaude mcp add --scope user {ENTRY} -- \"{command}\" --mcp"
            )
        }
        Kind::CopyOnly => String::new(),
    }
}

fn toml_block(command: &str) -> String {
    format!(
        "[mcp_servers.{ENTRY}]\ncommand = {}\nargs = [\"--mcp\"]\n",
        toml_edit::Value::from(command)
    )
}

/// The `claude` command; the app gets a short PATH, so look where the
/// installers put it too.
fn claude_cli(home: &Path) -> Option<PathBuf> {
    let candidates = [
        home.join(".local/bin/claude"),
        home.join(".claude/local/claude"),
        PathBuf::from("/opt/homebrew/bin/claude"),
        PathBuf::from("/usr/local/bin/claude"),
    ];
    candidates.into_iter().find(|p| p.is_file())
}

fn read_json(path: &Path) -> Result<Map<String, Value>, String> {
    match std::fs::read_to_string(path) {
        Ok(text) if text.trim().is_empty() => Ok(Map::new()),
        Ok(text) => match serde_json::from_str::<Value>(&text) {
            Ok(Value::Object(map)) => Ok(map),
            _ => Err(format!(
                "Lipik couldn't read {} (it may have comments). Add Lipik by hand instead.",
                path.display()
            )),
        },
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Map::new()),
        Err(e) => Err(format!("Couldn't open {}: {e}", path.display())),
    }
}

/// Saves `text` over `path` in one step (a temporary file renamed over it,
/// so a crash never leaves half a file), through a symlink to the real file
/// and with its permissions, keeping the file as it was before Lipik first
/// changed it.
fn write_with_backup(path: &Path, text: &str) -> Result<(), String> {
    let target = std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
    let permissions = std::fs::metadata(&target).ok().map(|m| m.permissions());
    if permissions.is_some() {
        let backup = PathBuf::from(format!("{}{BACKUP_SUFFIX}", path.display()));
        if !backup.exists() {
            std::fs::copy(&target, &backup)
                .map_err(|e| format!("Couldn't back up {}: {e}", path.display()))?;
        }
    } else if let Some(dir) = target.parent() {
        std::fs::create_dir_all(dir)
            .map_err(|e| format!("Couldn't create {}: {e}", dir.display()))?;
    }
    let temp = PathBuf::from(format!("{}.lipik-saving", target.display()));
    let saved = std::fs::write(&temp, text)
        .and_then(|_| match permissions {
            Some(p) => std::fs::set_permissions(&temp, p),
            None => Ok(()),
        })
        .and_then(|_| std::fs::rename(&temp, &target));
    if let Err(e) = saved {
        let _ = std::fs::remove_file(&temp);
        return Err(format!("Couldn't save {}: {e}", path.display()));
    }
    Ok(())
}

/// The command a JSON settings file starts Lipik with, if it has Lipik.
fn json_command(path: &Path, key: &str) -> Option<String> {
    let root = read_json(path).ok()?;
    let lipik = root.get(key)?.get(ENTRY)?;
    Some(
        lipik
            .get("command")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string(),
    )
}

fn set_json(path: &Path, key: &str, value: Option<Value>) -> Result<(), String> {
    let mut root = read_json(path)?;
    let servers = root
        .entry(key.to_string())
        .or_insert_with(|| Value::Object(Map::new()));
    let Value::Object(servers) = servers else {
        return Err(format!(
            "\"{key}\" in {} isn't a list of servers.",
            path.display()
        ));
    };
    match value {
        Some(v) => {
            servers.insert(ENTRY.to_string(), v);
        }
        None => {
            if servers.remove(ENTRY).is_none() {
                return Ok(());
            }
        }
    }
    let text = serde_json::to_string_pretty(&Value::Object(root)).map_err(|e| e.to_string())?;
    write_with_backup(path, &(text + "\n"))
}

fn read_toml(path: &Path) -> Result<toml_edit::DocumentMut, String> {
    let text = match std::fs::read_to_string(path) {
        Ok(t) => t,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => String::new(),
        Err(e) => return Err(format!("Couldn't open {}: {e}", path.display())),
    };
    text.parse().map_err(|_| {
        format!(
            "Lipik couldn't read {}. Add Lipik by hand instead.",
            path.display()
        )
    })
}

/// The command `config.toml` starts Lipik with, if it has Lipik.
fn toml_command(path: &Path) -> Option<String> {
    let doc = read_toml(path).ok()?;
    let lipik = doc.get("mcp_servers")?.as_table_like()?.get(ENTRY)?;
    let command = lipik.as_table_like().and_then(|t| t.get("command"));
    Some(
        command
            .and_then(|c| c.as_str())
            .unwrap_or_default()
            .to_string(),
    )
}

/// Adds or removes only `mcp_servers.lipik`; comments, order and layout of
/// everything else stay as they were.
fn set_toml(path: &Path, add: bool, command: &str) -> Result<(), String> {
    let mut doc = read_toml(path)?;
    if !add && toml_command(path).is_none() {
        return Ok(());
    }
    let inline = doc.get("mcp_servers").is_some_and(|i| i.is_inline_table());
    let servers = doc.entry("mcp_servers").or_insert_with(|| {
        let mut t = toml_edit::Table::new();
        t.set_implicit(true);
        toml_edit::Item::Table(t)
    });
    let Some(servers) = servers.as_table_like_mut() else {
        return Err(format!(
            "\"mcp_servers\" in {} isn't a list of servers.",
            path.display()
        ));
    };
    servers.remove(ENTRY);
    if add {
        let mut args = toml_edit::Array::new();
        args.push("--mcp");
        let mut lipik = toml_edit::InlineTable::new();
        lipik.insert("command", command.into());
        lipik.insert("args", toml_edit::Value::Array(args));
        let item = if inline {
            toml_edit::Item::Value(toml_edit::Value::InlineTable(lipik))
        } else {
            toml_edit::Item::Table(lipik.into_table())
        };
        servers.insert(ENTRY, item);
    }
    write_with_backup(path, &doc.to_string())
}

/// Runs `claude` with a PATH that has the folder `node` usually lives in (an
/// app started from the Finder gets only the system folders), giving up after
/// a while.
fn run_claude(cli: &Path, args: &[&str]) -> Result<(), String> {
    let mut path = std::env::var("PATH").unwrap_or_default();
    for dir in [
        cli.parent().map(|d| d.to_string_lossy().into_owned()),
        Some("/opt/homebrew/bin".into()),
        Some("/usr/local/bin".into()),
    ]
    .into_iter()
    .flatten()
    {
        path = format!("{dir}:{path}");
    }
    let mut child = Command::new(cli)
        .args(args)
        .env("PATH", path)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| format!("Couldn't run claude: {e}"))?;
    let started = Instant::now();
    loop {
        match child.try_wait().map_err(|e| e.to_string())? {
            Some(status) if status.success() => return Ok(()),
            Some(_) => {
                let mut err = String::new();
                if let Some(mut stderr) = child.stderr.take() {
                    let _ = std::io::Read::read_to_string(&mut stderr, &mut err);
                }
                return Err(format!("claude mcp {} failed: {}", args[1], err.trim()));
            }
            None if started.elapsed() > CLAUDE_TIMEOUT => {
                let _ = child.kill();
                return Err(
                    "claude didn't answer. Try again, or run the line below in a terminal.".into(),
                );
            }
            None => std::thread::sleep(Duration::from_millis(100)),
        }
    }
}

fn status(spec: &Spec, home: &Path, command: &Result<String, String>) -> AiApp {
    let file = (!spec.file.is_empty()).then(|| from_home(home, spec.file));
    let installed = spec.marks.iter().any(|m| from_home(home, m).exists());
    let saved = file.as_deref().and_then(|f| match spec.kind {
        Kind::Json { key, .. } => json_command(f, key),
        Kind::Toml => toml_command(f),
        Kind::ClaudeCode => json_command(f, "mcpServers"),
        Kind::CopyOnly => None,
    });
    let current = command.as_deref().ok();
    // An entry that starts a different copy of Lipik (moved, or an old
    // download) counts as not connected, so Connect fixes it.
    let connected = saved.is_some() && (current.is_none() || saved.as_deref() == current);
    let mut note = spec.note.map(String::from);
    let mut how = match spec.kind {
        Kind::Json { .. } | Kind::Toml => AiAppHow::File,
        Kind::ClaudeCode => AiAppHow::Command,
        Kind::CopyOnly => AiAppHow::Copy,
    };
    if spec.kind_is_claude_code() && claude_cli(home).is_none() {
        how = AiAppHow::Copy;
        note = Some(
            "Lipik couldn't find the claude command. Run the line below in a terminal.".into(),
        );
    }
    if saved.is_some() && !connected {
        note = Some("Set up for another copy of Lipik. Connect again to update it.".into());
    }
    if let Err(why) = command {
        if how != AiAppHow::Copy && !connected {
            note = Some(why.clone());
        }
    }
    AiApp {
        id: spec.id.into(),
        name: spec.name.into(),
        installed,
        connected,
        how,
        file: file.map(|f| f.display().to_string()),
        note,
        manual: manual(spec.kind, spec.file, current.unwrap_or(INSTALLED)),
    }
}

impl Spec {
    fn kind_is_claude_code(&self) -> bool {
        matches!(self.kind, Kind::ClaudeCode)
    }
}

fn change(id: &str, add: bool) -> Result<AiApp, String> {
    let home = home().ok_or("No home folder.")?;
    let spec = SPECS.iter().find(|s| s.id == id).ok_or("Unknown app.")?;
    if !spec.marks.iter().any(|m| from_home(&home, m).exists()) {
        return Err(format!("{} isn't installed.", spec.name));
    }
    let command = server_command();
    let file = from_home(&home, spec.file);
    if add {
        command.as_ref().map_err(Clone::clone)?;
    }
    let cmd = command.as_deref().unwrap_or(INSTALLED);
    match spec.kind {
        Kind::Json { key, vscode } => set_json(&file, key, add.then(|| entry(vscode, cmd)))?,
        Kind::Toml => set_toml(&file, add, cmd)?,
        Kind::ClaudeCode => {
            let cli = claude_cli(&home).ok_or("Lipik couldn't find the claude command.")?;
            let had = json_command(&file, "mcpServers").is_some();
            if had {
                run_claude(&cli, &["mcp", "remove", "--scope", "user", ENTRY])?;
            }
            if add {
                run_claude(
                    &cli,
                    &["mcp", "add", "--scope", "user", ENTRY, "--", cmd, "--mcp"],
                )?;
            }
        }
        Kind::CopyOnly => return Err(format!("{} can't use tools on your Mac.", spec.name)),
    }
    Ok(status(spec, &home, &command))
}

fn status_all() -> Vec<AiApp> {
    let Some(home) = home() else {
        return Vec::new();
    };
    let command = server_command();
    SPECS.iter().map(|s| status(s, &home, &command)).collect()
}

/// Off the main thread: reading `~/.claude.json` and running `claude` take a
/// while.
async fn blocking<T: Send + 'static>(f: impl FnOnce() -> T + Send + 'static) -> Result<T, String> {
    tauri::async_runtime::spawn_blocking(f)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
#[specta::specta]
pub async fn ai_apps_status() -> Vec<AiApp> {
    blocking(status_all).await.unwrap_or_default()
}

#[tauri::command]
#[specta::specta]
pub async fn connect_ai_app(id: String) -> Result<AiApp, String> {
    blocking(move || change(&id, true)).await?
}

#[tauri::command]
#[specta::specta]
pub async fn disconnect_ai_app(id: String) -> Result<AiApp, String> {
    blocking(move || change(&id, false)).await?
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("lipik-ai-apps-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn json_entry_is_added_and_removed_keeping_other_servers_and_key_order() {
        let dir = temp("json");
        let path = dir.join("claude_desktop_config.json");
        std::fs::write(
            &path,
            r#"{"theme":"dark","mcpServers":{"other":{"command":"x"}},"alpha":1}"#,
        )
        .unwrap();
        set_json(&path, "mcpServers", Some(entry(false, "/A/lipik"))).unwrap();
        let text = std::fs::read_to_string(&path).unwrap();
        assert!(text.find("theme").unwrap() < text.find("mcpServers").unwrap());
        assert!(text.find("mcpServers").unwrap() < text.find("alpha").unwrap());
        let after: Value = serde_json::from_str(&text).unwrap();
        assert_eq!(after["mcpServers"]["lipik"]["args"][0], "--mcp");
        assert_eq!(after["mcpServers"]["other"]["command"], "x");
        assert_eq!(
            json_command(&path, "mcpServers").as_deref(),
            Some("/A/lipik")
        );
        let backup = std::fs::read_to_string(format!("{}{BACKUP_SUFFIX}", path.display())).unwrap();
        assert!(!backup.contains("lipik"));
        set_json(&path, "mcpServers", None).unwrap();
        assert!(json_command(&path, "mcpServers").is_none());
        assert!(std::fs::read_to_string(&path).unwrap().contains("other"));
        assert!(!dir.join("claude_desktop_config.json.lipik-saving").exists());
    }

    #[test]
    fn unreadable_files_are_left_alone() {
        let dir = temp("bad");
        let json = dir.join("mcp.json");
        let original = "{ // my servers\n \"servers\": {} }";
        std::fs::write(&json, original).unwrap();
        assert!(set_json(&json, "servers", Some(entry(true, "/A/lipik"))).is_err());
        assert_eq!(std::fs::read_to_string(&json).unwrap(), original);
        let toml = dir.join("config.toml");
        std::fs::write(&toml, "model = \n").unwrap();
        assert!(set_toml(&toml, true, "/A/lipik").is_err());
        assert_eq!(std::fs::read_to_string(&toml).unwrap(), "model = \n");
    }

    #[test]
    fn toml_keeps_comments_and_layout() {
        let dir = temp("toml");
        let path = dir.join("config.toml");
        let original = "model = \"gpt-5\" # fast\n\n# GitHub server, token in 1Password\n[mcp_servers.github]\ncommand = \"gh\"\n";
        std::fs::write(&path, original).unwrap();
        set_toml(&path, true, "/A/lipik").unwrap();
        set_toml(&path, true, "/A/lipik").unwrap();
        let text = std::fs::read_to_string(&path).unwrap();
        assert_eq!(text.matches("[mcp_servers.lipik]").count(), 1, "{text}");
        assert!(text.contains("# GitHub server, token in 1Password"));
        assert_eq!(toml_command(&path).as_deref(), Some("/A/lipik"));
        set_toml(&path, false, "/A/lipik").unwrap();
        assert_eq!(std::fs::read_to_string(&path).unwrap(), original);
    }

    #[test]
    fn toml_handles_inline_tables_and_subtables() {
        let dir = temp("toml2");
        let inline = dir.join("inline.toml");
        std::fs::write(&inline, "mcp_servers = { other = { command = \"x\" } }\n").unwrap();
        set_toml(&inline, true, "/A/lipik").unwrap();
        let text = std::fs::read_to_string(&inline).unwrap();
        assert!(text.parse::<toml_edit::DocumentMut>().is_ok(), "{text}");
        assert_eq!(toml_command(&inline).as_deref(), Some("/A/lipik"));

        let sub = dir.join("sub.toml");
        std::fs::write(
            &sub,
            "[mcp_servers.lipik]\ncommand = \"/old\"\n\n[mcp_servers.lipik.env]\nA = \"1\"\n\n[mcp_servers.other]\ncommand = \"x\"\n",
        )
        .unwrap();
        set_toml(&sub, false, "/A/lipik").unwrap();
        let text = std::fs::read_to_string(&sub).unwrap();
        assert!(!text.contains("lipik"), "{text}");
        assert!(text.contains("[mcp_servers.other]"));
    }

    #[test]
    fn toml_written_for_a_new_file_is_a_table() {
        let dir = temp("toml3");
        let path = dir.join("config.toml");
        set_toml(&path, true, "/A/lipik").unwrap();
        let text = std::fs::read_to_string(&path).unwrap();
        assert!(text.starts_with("[mcp_servers.lipik]"), "{text}");
        assert!(!text.contains("[mcp_servers]\n"), "{text}");
    }
}
