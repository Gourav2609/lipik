//! The bundled, offline model catalog.
//!
//! `catalog.json` is vendored from an MIT-licensed npm package (see `NOTICE`
//! beside it); edit it here when models change. It lists every model
//! transcribe.cpp publishes, with pinned revisions, per-quant hashes and benchmark results, and marks
//! the curated subset. It is compiled into the binary so Lipik ships a
//! complete model list with zero network access.
//!
//! Every supported transcription model is offered for download at the
//! package's default quant, independently of recommendation ranking. The files
//! come from Lipik's own Hugging Face repos (`lipik/<name>`), byte-for-byte
//! copies of the ones first published under `handy-computer/<name>`.
//!
//! Each entry is normalised into a [`ModelDescriptor`] — the same source-agnostic
//! shape every other producer (HF discovery, on-disk scans, the legacy table)
//! yields — so the catalog is "just another producer". Its explicit capabilities
//! become a [`CapabilityProbe`] with confident `Some(..)` values; the runtime
//! `GgufHeaderProber` is the same shape with `None` where a header omits a key,
//! which is why the two are interchangeable (the catalog is a baked probe).

use std::collections::{BTreeSet, HashMap};

use once_cell::sync::Lazy;
use serde::Deserialize;

use crate::managers::model::{default_quant_file, ModelDescriptor, ModelSource, QuantFile};
use crate::managers::model_capabilities::{CapabilityProbe, Compatibility};

/// Catalog schema this loader understands. The package bumps it on any
/// breaking change; additive fields are ignored.
const SCHEMA: u32 = 2;

/// Lipik's Hugging Face organisation, which holds the catalog's files.
const ORG: &str = "lipik";

/// Where the catalog's files were published before Lipik copied them into
/// [`ORG`]. Copies downloaded from there are the same bytes, so they keep
/// working without a new download.
const LEGACY_ORG: &str = "handy-computer";

/// Lipik's list order and onboarding set, keyed by catalog id. Kept from the
/// previous generator until onboarding moves to the package's recommendations.
/// `(id, rank, recommended)`: rank is the editorial sort position (lower =
/// higher); recommended is the small badged onboarding subset.
const LIPIK_RANKING: &[(&str, u32, bool)] = &[
    ("parakeet-unified-en-0.6b", 1, true),
    ("nemotron-3.5-asr-streaming-0.6b", 2, true),
    ("canary-180m-flash", 3, true),
    ("cohere-transcribe-03-2026", 4, true),
    ("whisper-medium", 5, true),
    ("Voxtral-Mini-4B-Realtime-2602", 6, false),
    ("parakeet-tdt-0.6b-v3", 7, false),
    ("parakeet-tdt-0.6b-v2", 8, false),
    ("Qwen3-ASR-0.6B", 9, false),
    ("Fun-ASR-MLT-Nano-2512", 10, false),
];

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct CatalogRoot {
    schema: u32,
    /// The transcribe.cpp release the catalog was built from.
    #[cfg_attr(not(test), allow(dead_code))]
    transcribe_cpp: String,
    models: Vec<CatalogModel>,
    /// Benchmark results keyed by catalog id.
    #[serde(default)]
    results: HashMap<String, ModelResults>,
}

/// One model as written in `catalog.json`. Only the fields Lipik needs are
/// declared; serde ignores the rest (license, parameters, …).
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct CatalogModel {
    /// Stable package id: the HF repo name without `-gguf`.
    id: String,
    name: String,
    /// Editorial copy; empty for models that aren't curated.
    description: String,
    /// HF repo id, e.g. `lipik/whisper-small-gguf`.
    repository: String,
    /// Commit the files and hashes are pinned to, so downloaded bytes provably
    /// match the hashes.
    revision: String,
    /// The loader architecture (`general.architecture`).
    family: String,
    languages: Vec<String>,
    capabilities: CatalogCaps,
    /// Key of the default file in `files`.
    quant: String,
    /// `asr`, `langid`, `vad` or `diarize`; only `asr` models transcribe.
    role: String,
    /// Oldest transcribe.cpp release that can load these files.
    #[cfg_attr(not(test), allow(dead_code))]
    min_engine: Option<String>,
    /// Every published file, keyed by quant.
    files: HashMap<String, CatalogFile>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct CatalogCaps {
    streaming: bool,
    translate: bool,
    language_detection: bool,
    #[serde(default)]
    timestamps: bool,
}

#[derive(Deserialize)]
struct CatalogFile {
    filename: String,
    size: u64,
    sha256: String,
}

#[derive(Deserialize, Default)]
struct ModelResults {
    /// Error percent per language identity, measured at the default quant.
    #[serde(default)]
    accuracy: HashMap<String, AccuracyCell>,
    #[serde(default)]
    performance: Performance,
}

#[derive(Deserialize)]
struct AccuracyCell {
    error: f64,
}

#[derive(Deserialize, Default)]
struct Performance {
    /// Real-time factor on the benchmark GPU (Vulkan).
    accelerated: Option<f64>,
    cpu: Option<f64>,
}

impl CatalogModel {
    fn default_file(&self) -> Option<&CatalogFile> {
        self.files.get(&self.quant)
    }

    /// Lipik uses a flat models directory for manual installs, so only
    /// transcription models with root-level default files can be offered.
    fn is_offered(&self) -> bool {
        self.role == "asr"
            && self
                .default_file()
                .is_some_and(|file| !file.filename.contains('/'))
    }

    /// Files ordered largest first, so the list reads F32 → Q4 like the HF repo.
    fn quant_files(&self) -> Vec<QuantFile> {
        let mut files: Vec<QuantFile> = self
            .files
            .iter()
            .map(|(quant, file)| QuantFile {
                filename: file.filename.clone(),
                quant: quant.clone(),
                size_bytes: file.size,
            })
            .collect();
        files.sort_by_key(|f| std::cmp::Reverse(f.size_bytes));
        files
    }
}

// ───────────────────────── scores ─────────────────────────
// The list's 0–100 bars, from the package's benchmarks with the formulas the
// previous generator used.

/// speed = 100·(1 − e^(−rtf/8)), growing toward 100.
const SPEED_SCALE: f64 = 8.0;
/// accuracy = 100·e^(−error/15), decaying from 100.
const ACCURACY_SCALE: f64 = 15.0;
/// No benchmark speed (too heavy to measure): assume slow.
const SPEED_FLOOR: f64 = 10.0;

fn speed_score(results: Option<&ModelResults>) -> f32 {
    let rtf = results.and_then(|r| r.performance.accelerated.or(r.performance.cpu));
    let score = match rtf {
        Some(rtf) if rtf > 0.0 => 100.0 * (1.0 - (-rtf / SPEED_SCALE).exp()),
        _ => SPEED_FLOOR,
    };
    score.round() as f32
}

/// Accuracy on English when measured, else on the model's first language.
fn accuracy_score(model: &CatalogModel, results: Option<&ModelResults>) -> f32 {
    let Some(results) = results else {
        return 0.0;
    };
    let headline = std::iter::once("en".to_string())
        .chain(model.languages.first().map(|l| language_identity(l)))
        .find_map(|language| results.accuracy.get(&language));
    headline
        .map(|cell| (100.0 * (-cell.error / ACCURACY_SCALE).exp()).round() as f32)
        .unwrap_or(0.0)
}

/// The package's language identity (its `languageIdentity`): base code, with
/// the aliases its benchmark keys use.
fn language_identity(language: &str) -> String {
    let base = language
        .split(['-', '_'])
        .next()
        .unwrap_or(language)
        .to_ascii_lowercase();
    match base.as_str() {
        "tl" => "fil".to_string(),
        "no" => "nb".to_string(),
        "iw" => "he".to_string(),
        "jw" => "jv".to_string(),
        _ => base,
    }
}

// ───────────────────────── descriptions ─────────────────────────

/// A factual one-liner for models the package has no editorial copy for.
fn generated_description(model: &CatalogModel) -> String {
    let mut features = Vec::new();
    if model.capabilities.translate {
        features.push("translation");
    }
    if model.capabilities.language_detection {
        features.push("auto language detection");
    }
    if model.capabilities.streaming {
        features.push("streaming");
    }
    if model.capabilities.timestamps {
        features.push("timestamps");
    }

    let languages: BTreeSet<String> = model
        .languages
        .iter()
        .map(|l| language_identity(l))
        .collect();
    let base = if languages.len() > 1 {
        format!("{}-language speech-to-text", languages.len())
    } else {
        let name = match languages.iter().next().map(String::as_str) {
            None | Some("en") => "English",
            Some("ar") => "Arabic",
            Some("ja") => "Japanese",
            Some("ko") => "Korean",
            Some("ru") => "Russian",
            Some("uk") => "Ukrainian",
            Some("vi") => "Vietnamese",
            Some("zh") => "Chinese",
            Some(other) => other,
        };
        format!("{name} speech-to-text")
    };

    if features.is_empty() {
        format!("{base}.")
    } else {
        format!("{base} with {}.", features.join(", "))
    }
}

impl From<&CatalogModel> for ModelDescriptor {
    fn from(m: &CatalogModel) -> Self {
        // The default download file. Its name is folded into the id so a catalog
        // entry collides (dedups) with the very same file later discovered in
        // the HF cache — both compute `"{repo_id}/{filename}"`.
        let files = m.quant_files();
        let default_filename = default_quant_file(&files, Some(&m.quant))
            .map(|f| f.filename.clone())
            .unwrap_or_default();
        let ranking = LIPIK_RANKING.iter().find(|(id, _, _)| *id == m.id);
        let results = ROOT.results.get(&m.id);

        ModelDescriptor {
            id: format!("{}/{}", m.repository, default_filename),
            source: ModelSource::HuggingFace {
                repo_id: m.repository.clone(),
                // Acquire at the pin: `resolve/<sha>` is immutable (CDN-friendly)
                // and guarantees the bytes match the catalog's hashes.
                revision: m.revision.clone(),
            },
            name: m.name.clone(),
            description: if m.description.is_empty() {
                generated_description(m)
            } else {
                m.description.clone()
            },
            caps: CapabilityProbe {
                verdict: Compatibility::Compatible, // published by transcribe.cpp
                display_name: None,
                architecture: Some(m.family.clone()),
                variant: None,
                languages: Some(m.languages.clone()),
                supports_streaming: Some(m.capabilities.streaming),
                supports_translation: Some(m.capabilities.translate),
                supports_language_detect: Some(m.capabilities.language_detection),
            },
            files,
            default_quant: Some(m.quant.clone()),
            // ModelInfo / the UI bars use 0.0–1.0.
            speed_score: speed_score(results) / 100.0,
            accuracy_score: accuracy_score(m, results) / 100.0,
            recommended_rank: ranking.map(|(_, rank, _)| *rank),
            recommended: ranking.is_some_and(|(_, _, recommended)| *recommended),
        }
    }
}

/// The raw parsed catalog. Kept alive (not consumed) so data that deliberately
/// stays out of [`ModelDescriptor`] (hashes, results) can
/// be looked up separately.
static ROOT: Lazy<CatalogRoot> = Lazy::new(|| {
    let root: CatalogRoot = serde_json::from_str(include_str!("catalog.json"))
        .expect("bundled catalog.json is valid JSON matching the catalog schema");
    assert_eq!(
        root.schema, SCHEMA,
        "bundled catalog.json has an unsupported schema"
    );
    root
});

/// Transcription models: the only role Lipik can load. Language ID, VAD and
/// diarization models are published alongside them but produce no text.
fn transcription_models() -> impl Iterator<Item = &'static CatalogModel> {
    ROOT.models.iter().filter(|m| m.role == "asr")
}

/// Every transcription model Lipik can download, normalised into descriptors.
pub static CATALOG: Lazy<Vec<ModelDescriptor>> = Lazy::new(|| {
    transcription_models()
        .filter(|m| m.is_offered())
        .map(ModelDescriptor::from)
        .collect()
});

fn offered_models() -> impl Iterator<Item = &'static CatalogModel> {
    transcription_models().filter(|m| m.is_offered())
}

/// Size and sha256 of a file of an offered catalog model, any listed quant.
pub fn file_digest(repo_id: &str, filename: &str) -> Option<(u64, &'static str)> {
    offered_models()
        .filter(|m| m.repository == repo_id)
        .flat_map(|m| m.files.values())
        .find(|f| f.filename == filename)
        .map(|f| (f.size, f.sha256.as_str()))
}

/// The repo a catalog repo's files were first published in, for finding copies
/// downloaded before the move.
pub fn legacy_repo(repo_id: &str) -> Option<String> {
    let name = repo_id.strip_prefix(ORG)?.strip_prefix('/')?;
    Some(format!("{LEGACY_ORG}/{name}"))
}

/// The catalog repo that took over `repo_id`, when it is a legacy repo of an
/// offered model.
pub fn current_repo(repo_id: &str) -> Option<&'static str> {
    let name = repo_id.strip_prefix(LEGACY_ORG)?.strip_prefix('/')?;
    offered_models()
        .map(|m| m.repository.as_str())
        .find(|repo| repo.strip_prefix(ORG).and_then(|r| r.strip_prefix('/')) == Some(name))
}

/// A model id saved before the move (`"{legacy repo}/{filename}"`) as the id
/// of the same file now, or `None` when the id needs no change.
pub fn current_model_id(model_id: &str) -> Option<String> {
    let (name, filename) = model_id
        .strip_prefix(LEGACY_ORG)?
        .strip_prefix('/')?
        .split_once('/')?;
    let repo = current_repo(&format!("{LEGACY_ORG}/{name}"))?;
    file_digest(repo, filename).map(|_| format!("{repo}/{filename}"))
}

/// Hugging Face repo of an offered catalog model, by catalog id.
pub fn repository_of(id: &str) -> Option<&'static str> {
    offered_models()
        .find(|m| m.id == id)
        .map(|m| m.repository.as_str())
}

/// Model registry ids (`"{repo}/{filename}"`) of every quant of an offered
/// catalog model, by catalog id: the default quant first, then largest to
/// smallest. Empty when the id isn't offered.
pub fn model_ids(id: &str) -> Vec<String> {
    let Some(model) = offered_models().find(|m| m.id == id) else {
        return Vec::new();
    };
    let default = model.default_file().map(|f| f.filename.as_str());
    let mut files = model.quant_files();
    files.sort_by_key(|f| Some(f.filename.as_str()) != default);
    files
        .into_iter()
        .map(|f| format!("{}/{}", model.repository, f.filename))
        .collect()
}

/// The catalog descriptor + specific `files[]` entry owning `filename`,
/// matched across every listed quant (not just the default) of every offered
/// transcription model. `repo_id`, when given, must also match — the HF-cache
/// scan uses it to keep a foreign repo that happens to reuse a catalog
/// filename from masquerading as ours.
pub fn file_in_catalog(
    filename: &str,
    repo_id: Option<&str>,
) -> Option<(&'static ModelDescriptor, &'static QuantFile)> {
    let catalog: &'static Vec<ModelDescriptor> = Lazy::force(&CATALOG);
    catalog.iter().find_map(|d| {
        if let Some(repo) = repo_id {
            match &d.source {
                ModelSource::HuggingFace { repo_id: r, .. } if r == repo => {}
                _ => return None,
            }
        }
        d.files
            .iter()
            .find(|f| f.filename == filename)
            .map(|f| (d, f))
    })
}

/// Editorial recommended rank keyed by descriptor id (the same id the model
/// registry uses). Built once from the catalog.
static RANK_BY_ID: Lazy<HashMap<String, u32>> = Lazy::new(|| {
    CATALOG
        .iter()
        .filter_map(|d| d.recommended_rank.map(|r| (d.id.clone(), r)))
        .collect()
});

/// Recommended rank for a model id (lower = higher priority). Returns
/// `u32::MAX` for unranked/unknown ids so they sort last in an ascending sort.
pub fn rank_of(model_id: &str) -> u32 {
    RANK_BY_ID.get(model_id).copied().unwrap_or(u32::MAX)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::managers::model_capabilities::KNOWN_ARCHES;

    #[test]
    fn catalog_parses_and_offers_models() {
        assert!(!CATALOG.is_empty(), "bundled catalog should offer models");
    }

    #[test]
    fn ids_are_unique() {
        let mut ids: Vec<&str> = CATALOG.iter().map(|d| d.id.as_str()).collect();
        ids.sort_unstable();
        let before = ids.len();
        ids.dedup();
        assert_eq!(before, ids.len(), "catalog descriptor ids must be unique");
    }

    #[test]
    fn only_transcription_models_are_loaded() {
        // Language ID (ecapa_tdnn), VAD (silero_vad) and diarization
        // (sortformer) models are published but produce no transcription text.
        for arch in ["ecapa_tdnn", "silero_vad", "sortformer"] {
            assert!(
                CATALOG
                    .iter()
                    .all(|d| d.caps.architecture.as_deref() != Some(arch)),
                "{arch} must not be in the transcription catalog"
            );
        }
    }

    #[test]
    fn all_flat_asr_models_are_offered_at_the_package_default() {
        for m in transcription_models()
            .filter(|m| m.default_file().is_some_and(|f| !f.filename.contains('/')))
        {
            let id = format!("{}/{}", m.repository, m.default_file().unwrap().filename);
            let desc = CATALOG
                .iter()
                .find(|d| d.id == id)
                .expect("ASR model is offered");
            assert_eq!(desc.default_quant.as_deref(), Some(m.quant.as_str()));
            assert_eq!(m.quant, "Q8_0");
        }
    }

    #[test]
    fn scores_are_normalised_0_to_1() {
        for d in CATALOG.iter() {
            assert!((0.0..=1.0).contains(&d.speed_score), "{} speed", d.id);
            assert!((0.0..=1.0).contains(&d.accuracy_score), "{} acc", d.id);
        }
    }

    #[test]
    fn every_description_is_filled() {
        for d in CATALOG.iter() {
            assert!(!d.description.is_empty(), "{}: empty description", d.id);
        }
    }

    #[test]
    fn lipik_ranking_names_offered_models() {
        for (id, _, _) in LIPIK_RANKING {
            let model = transcription_models()
                .find(|m| m.id == *id)
                .unwrap_or_else(|| panic!("ranked model {id} is not in the catalog"));
            assert!(model.is_offered(), "ranked model {id} is not offered");
        }
        assert_eq!(
            CATALOG.iter().filter(|d| d.recommended).count(),
            LIPIK_RANKING.iter().filter(|(_, _, r)| *r).count()
        );
    }

    #[test]
    fn catalog_matches_the_engine_lipik_ships() {
        // Every model must be loadable by the transcribe-cpp crate we link.
        let cargo = include_str!("../../Cargo.toml");
        let engine = cargo
            .lines()
            .find_map(|line| {
                line.strip_prefix("transcribe-cpp = { version = \"")
                    .and_then(|rest| rest.split('"').next())
            })
            .expect("Cargo.toml pins a transcribe-cpp version");
        let parse = |v: &str| -> Vec<u32> {
            v.trim_start_matches('v')
                .split(['.', '-', '+'])
                .take(3)
                .map(|p| p.parse().unwrap_or(0))
                .collect()
        };
        assert!(
            parse(&ROOT.transcribe_cpp) <= parse(engine),
            "catalog built for transcribe.cpp {} but Lipik links {}",
            ROOT.transcribe_cpp,
            engine
        );
        for m in transcription_models() {
            if let Some(min) = &m.min_engine {
                assert!(
                    parse(min) <= parse(engine),
                    "{} needs transcribe.cpp {min}, Lipik links {engine}",
                    m.id
                );
            }
        }
    }

    #[test]
    fn catalog_architectures_are_known_to_capability_probe() {
        let missing: BTreeSet<&str> = CATALOG
            .iter()
            .filter_map(|d| d.caps.architecture.as_deref())
            .filter(|arch| !KNOWN_ARCHES.contains(arch))
            .collect();

        assert!(
            missing.is_empty(),
            "catalog architecture(s) missing from KNOWN_ARCHES: {:?}",
            missing
        );
    }

    #[test]
    fn generated_descriptions_read_naturally() {
        let gigaam = transcription_models()
            .find(|m| m.id == "gigaam-v3-e2e-ctc")
            .unwrap();
        assert!(generated_description(gigaam).starts_with("Russian speech-to-text"));
        let whisper = transcription_models()
            .find(|m| m.id == "whisper-small")
            .unwrap();
        assert!(generated_description(whisper).contains("-language speech-to-text with"));
    }
}
