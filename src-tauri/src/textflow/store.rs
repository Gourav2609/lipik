//! Dictionary, snippets, style and transforms, kept in memory and saved to
//! `textflow.json` in the app data folder on every change.

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use specta::Type;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::RwLock;

#[derive(Serialize, Deserialize, Debug, Clone, Type)]
pub struct DictionaryWord {
    pub id: String,
    pub word: String,
    pub starred: bool,
    pub auto_learned: bool,
    pub shared: bool,
}

#[derive(Serialize, Deserialize, Debug, Clone, Type)]
pub struct Replacement {
    pub id: String,
    pub from: String,
    pub to: String,
    #[serde(default)]
    pub auto_learned: bool,
}

#[derive(Serialize, Deserialize, Debug, Clone, Default, Type)]
#[serde(default)]
pub struct Dictionary {
    pub words: Vec<DictionaryWord>,
    pub replacements: Vec<Replacement>,
}

#[derive(Serialize, Deserialize, Debug, Clone, Type)]
pub struct Snippet {
    pub id: String,
    pub trigger: String,
    pub expansion: String,
}

#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq, Default, Type)]
#[serde(rename_all = "snake_case")]
pub enum StyleKind {
    #[default]
    Formal,
    Casual,
    VeryCasual,
}

#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq, Type)]
#[serde(rename_all = "snake_case")]
pub enum StyleCategory {
    Personal,
    Work,
    Email,
    Other,
}

impl StyleCategory {
    pub fn as_str(self) -> &'static str {
        match self {
            StyleCategory::Personal => "personal",
            StyleCategory::Work => "work",
            StyleCategory::Email => "email",
            StyleCategory::Other => "other",
        }
    }
}

#[derive(Serialize, Deserialize, Debug, Clone, Type)]
#[serde(default)]
pub struct StyleSettings {
    pub personal: StyleKind,
    pub work: StyleKind,
    pub email: StyleKind,
    pub other: StyleKind,
    pub auto_cleanup: bool,
}

impl Default for StyleSettings {
    fn default() -> Self {
        Self {
            personal: StyleKind::default(),
            work: StyleKind::default(),
            email: StyleKind::default(),
            other: StyleKind::default(),
            auto_cleanup: true,
        }
    }
}

impl StyleSettings {
    pub fn for_category(&self, category: StyleCategory) -> StyleKind {
        match category {
            StyleCategory::Personal => self.personal,
            StyleCategory::Work => self.work,
            StyleCategory::Email => self.email,
            StyleCategory::Other => self.other,
        }
    }
}

#[derive(Serialize, Deserialize, Debug, Clone, Type)]
pub struct Transform {
    pub id: String,
    pub name: String,
    pub description: String,
    pub prompt: String,
    /// 1..=9 for ⌥1..⌥9, or none.
    pub shortcut_index: Option<u32>,
}

#[derive(Serialize, Deserialize, Debug, Clone, Default, Type)]
#[serde(default)]
pub struct AutoApply {
    pub enabled: bool,
    pub transform_id: Option<String>,
}

#[derive(Serialize, Deserialize, Debug, Clone, Type)]
#[serde(default)]
pub struct TransformSettings {
    pub opt_in: bool,
    pub auto_apply: AutoApply,
    pub items: Vec<Transform>,
}

impl Default for TransformSettings {
    fn default() -> Self {
        Self {
            opt_in: false,
            auto_apply: AutoApply::default(),
            items: default_transforms(),
        }
    }
}

pub fn default_transforms() -> Vec<Transform> {
    vec![
        Transform {
            id: "polish".to_string(),
            name: "Clear writing".to_string(),
            description: "Fix grammar and cut extra words".to_string(),
            prompt: "Rewrite the text so it reads clearly and naturally. Fix grammar, \
spelling and punctuation, tighten wordy sentences and split run-on sentences. Keep the \
author's meaning, tone, language and point of view. Do not add new facts, greetings or \
sign-offs. Keep lists as lists. Return only the rewritten text."
                .to_string(),
            shortcut_index: Some(1),
        },
        Transform {
            id: "prompt_engineer".to_string(),
            name: "Prompt writer".to_string(),
            description: "Turn notes into a prompt for an AI assistant".to_string(),
            prompt: "Turn the text into a clear, well structured prompt for an AI assistant. \
State the goal first, then the context, constraints and the expected output format as \
short sections or bullet points. Keep every requirement from the original and do not \
invent new ones. Write in the same language as the text. Return only the prompt."
                .to_string(),
            shortcut_index: Some(2),
        },
    ]
}

/// Built-in names and descriptions saved by earlier builds, swapped for the
/// current text. Names or descriptions the user changed are kept. Old values
/// are matched by the first 8 bytes of their SHA-256.
fn update_old_descriptions(items: &mut [Transform]) {
    const OLD_DESCRIPTIONS: &[u64] = &[
        0x5f3b345b8337249b,
        0x96c645b8eade0cb5,
        0xd692973c9b800e03,
        0xc3f06f5ba3c5fb1a,
    ];
    const OLD_NAMES: &[u64] = &[0x770341d6e89e04e8, 0xcbf3383f585c7698, 0xde318f3c089ba739];
    let defaults = default_transforms();
    for item in items.iter_mut() {
        let Some(default) = defaults.iter().find(|d| d.id == item.id) else {
            continue;
        };
        if OLD_DESCRIPTIONS.contains(&text_hash(&item.description)) {
            item.description = default.description.clone();
        }
        if OLD_NAMES.contains(&text_hash(&item.name)) {
            item.name = default.name.clone();
        }
    }
}

fn text_hash(text: &str) -> u64 {
    let digest = Sha256::digest(text.as_bytes());
    u64::from_be_bytes(digest[..8].try_into().expect("8 bytes"))
}

/// A word fix seen in History corrections but not yet made a replacement:
/// it becomes one once two different dictations had it.
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct PendingFix {
    pub from: String,
    pub to: String,
    pub entry_ids: Vec<i64>,
}

#[derive(Serialize, Deserialize, Debug, Clone, Default)]
#[serde(default)]
pub struct Data {
    pub dictionary: Dictionary,
    pub snippets: Vec<Snippet>,
    pub style: StyleSettings,
    pub transforms: TransformSettings,
    pub pending_fixes: Vec<PendingFix>,
    /// Clean-up was off by default in earlier builds. Stores saved before
    /// this flag existed get it switched on once; after that the user's
    /// choice stays. Dev builds wrote it as `cleanup_on_by_default`.
    #[serde(alias = "cleanup_on_by_default")]
    pub cleanup_default_applied: bool,
}

pub struct TextFlow {
    path: PathBuf,
    data: RwLock<Data>,
}

static ID_COUNTER: AtomicU64 = AtomicU64::new(0);

pub fn new_id() -> String {
    let millis = chrono::Utc::now().timestamp_millis();
    let n = ID_COUNTER.fetch_add(1, Ordering::Relaxed);
    format!("{millis:x}{n:04x}")
}

impl TextFlow {
    /// Loads `path`. The bool is false when there was no usable file: a
    /// first run, or a file that could not be read, which is kept beside it
    /// as `textflow.json.bad-<time>` so the next save can't overwrite it.
    pub fn load(path: PathBuf) -> (Self, bool) {
        let loaded = match std::fs::read_to_string(&path) {
            Ok(raw) => Some(serde_json::from_str::<Data>(&raw).map_err(|e| e.to_string())),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => None,
            Err(e) => Some(Err(e.to_string())),
        };
        let fresh = Data {
            cleanup_default_applied: true,
            ..Data::default()
        };
        let mut turned_on_cleanup = false;
        let (data, existed) = match loaded {
            None => (fresh, false),
            Some(Ok(mut data)) => {
                update_old_descriptions(&mut data.transforms.items);
                if !data.cleanup_default_applied {
                    data.style.auto_cleanup = true;
                    data.cleanup_default_applied = true;
                    turned_on_cleanup = true;
                }
                (data, true)
            }
            Some(Err(e)) => {
                let backup = path.with_extension(format!(
                    "json.bad-{}",
                    chrono::Utc::now().timestamp()
                ));
                log::error!("textflow: could not load {path:?} ({e}); moved it to {backup:?}");
                if let Err(e) = std::fs::rename(&path, &backup) {
                    log::error!("textflow: could not move the bad file ({e})");
                }
                (fresh, false)
            }
        };
        let textflow = Self {
            path,
            data: RwLock::new(data),
        };
        if turned_on_cleanup {
            if let Err(e) = textflow.update(|_| Ok(())) {
                log::warn!("textflow: could not save clean-up turned on ({e})");
            }
        }
        (textflow, existed)
    }

    pub fn read<T>(&self, f: impl FnOnce(&Data) -> T) -> T {
        let data = self.data.read().unwrap_or_else(|e| e.into_inner());
        f(&data)
    }

    pub fn update<T>(&self, f: impl FnOnce(&mut Data) -> Result<T, String>) -> Result<T, String> {
        let mut data = self.data.write().unwrap_or_else(|e| e.into_inner());
        let mut next = data.clone();
        let out = f(&mut next)?;
        let json = serde_json::to_string_pretty(&next).map_err(|e| e.to_string())?;
        let tmp = self.path.with_extension("json.tmp");
        std::fs::write(&tmp, json).map_err(|e| format!("Failed to save: {e}"))?;
        std::fs::rename(&tmp, &self.path).map_err(|e| format!("Failed to save: {e}"))?;
        *data = next;
        Ok(out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn load_from(test: &str, json: Option<&str>) -> (TextFlow, PathBuf) {
        let dir = std::env::temp_dir().join(format!("lipik-textflow-test-{test}"));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("textflow.json");
        if let Some(json) = json {
            std::fs::write(&path, json).unwrap();
        }
        (TextFlow::load(path.clone()).0, path)
    }

    #[test]
    fn cleanup_is_on_for_new_and_untouched_stores_once() {
        let (new_install, new_path) = load_from("new", None);
        assert!(new_install.read(|d| d.style.auto_cleanup && d.cleanup_default_applied));
        let _ = std::fs::remove_dir_all(new_path.parent().unwrap());

        let (old, path) = load_from("old", Some(r#"{"style":{"auto_cleanup":false}}"#));
        assert!(old.read(|d| d.style.auto_cleanup));
        let saved: Data = serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        assert!(saved.style.auto_cleanup && saved.cleanup_default_applied);

        old.update(|d| {
            d.style.auto_cleanup = false;
            Ok(())
        })
        .unwrap();
        let reloaded = TextFlow::load(path.clone()).0;
        assert!(!reloaded.read(|d| d.style.auto_cleanup));
        let _ = std::fs::remove_dir_all(path.parent().unwrap());
    }

    #[test]
    fn the_dev_build_flag_name_is_still_read() {
        let json = r#"{"style":{"auto_cleanup":false},"cleanup_on_by_default":true}"#;
        let (store, path) = load_from("dev-flag", Some(json));
        assert!(store.read(|d| !d.style.auto_cleanup && d.cleanup_default_applied));
        let _ = std::fs::remove_dir_all(path.parent().unwrap());
    }

    #[test]
    fn old_built_in_text_is_replaced_and_user_text_kept() {
        let mut items = default_transforms();
        items[0].name = "Polish".to_string();
        items[0].description = "Make your writing clear, concise and well structured".to_string();
        items[1].name = "AI prompt".to_string();
        items[1].description = "My own words".to_string();
        update_old_descriptions(&mut items);
        let defaults = default_transforms();
        assert_eq!(items[0].name, defaults[0].name);
        assert_eq!(items[0].description, defaults[0].description);
        assert_eq!(items[1].name, "Prompt writer");
        assert_eq!(items[1].description, "My own words");
    }
}
