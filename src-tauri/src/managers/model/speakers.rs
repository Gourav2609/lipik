//! The notetaker's speaker-detection models (speakrs, pinned revision), kept
//! in `models/speakers/` and fetched with the same resumable, hash-checked
//! transport as the transcription models.

use super::download::HttpDownloadOutcome;
use super::ModelManager;
use anyhow::{bail, Result};
use hf_hub::api::tokio::CancellationToken;
use std::fs;
use std::path::PathBuf;

const REPO: &str = "avencera/speakrs-models";
/// The revision speakrs 0.7.0 itself loads (`speakrs::models::MODEL_REVISION`).
const REVISION: &str = "67f963b6a1d94fcd4678948237b3cc8bd69bf1d8";

struct SpeakerModelFile {
    name: &'static str,
    size: u64,
    sha256: &'static str,
}

/// What the native CPU pipeline loads (about 33 MB in all).
const FILES: &[SpeakerModelFile] = &[
    SpeakerModelFile {
        name: "segmentation-3.0.safetensors",
        size: 5_897_576,
        sha256: "88e7883cf56f679170e162419efddf970f929915281ebce80e351c42b5a7115d",
    },
    SpeakerModelFile {
        name: "wespeaker-multimask-tail.safetensors",
        size: 26_527_656,
        sha256: "b97b090ceebcf10924732ad4552a2d867d76d38dcc1409a13687a6ba71ff1ef3",
    },
    SpeakerModelFile {
        name: "plda_lda.npy",
        size: 131_200,
        sha256: "e20c9b012bebd1aabda5a38a127e63a43cf35debdc502715fc143e2fb6bc3c4b",
    },
    SpeakerModelFile {
        name: "plda_tr.npy",
        size: 131_200,
        sha256: "e700b68cb319de3fafb5fa093eb9222c23c447084741f8d3a533640d425510ee",
    },
    SpeakerModelFile {
        name: "plda_mu.npy",
        size: 1_152,
        sha256: "d286d48acf99bbc1ed1502fed0a3e361ae5626ce1870c8be9f7397c5e47886c6",
    },
    SpeakerModelFile {
        name: "plda_psi.npy",
        size: 1_152,
        sha256: "d7128c9ed2f28a9781971805131129f077c04f948e2df12e52dcdb99f2b4e5f5",
    },
    SpeakerModelFile {
        name: "plda_mean1.npy",
        size: 2_176,
        sha256: "e424c0c352182aa8e0f555dec1f3b30e29a20b9ed6b25d339f112af92e51e36f",
    },
    SpeakerModelFile {
        name: "plda_mean2.npy",
        size: 640,
        sha256: "6f6fb708a2037197b5b84ffeaa8f140cb878088fbecd6ab042ad26a7691bd2cf",
    },
    SpeakerModelFile {
        name: "wespeaker-voxceleb-resnet34.min_num_samples.txt",
        size: 4,
        sha256: "e4df891c484d7abb985dadf539fa1883a646dab6337af5cae4159c587b7050cc",
    },
];

impl ModelManager {
    pub fn speaker_models_dir(&self) -> PathBuf {
        self.models_dir.join("speakers")
    }

    /// Downloads whichever speaker model files are missing and returns their
    /// folder. A file is renamed into place only after its hash checked out,
    /// so a file of the right size is a good one.
    pub async fn ensure_speaker_models(&self) -> Result<PathBuf> {
        let dir = self.speaker_models_dir();
        fs::create_dir_all(&dir)?;
        for file in FILES {
            let path = dir.join(file.name);
            if path.metadata().is_ok_and(|m| m.len() == file.size) {
                continue;
            }
            let partial = dir.join(format!("{}.partial", file.name));
            let url = format!(
                "https://huggingface.co/{REPO}/resolve/{REVISION}/{}",
                file.name
            );
            let outcome = Self::download_http_resumable_with_events(
                "speakers",
                &url,
                &partial,
                Some(file.size),
                Some(file.sha256),
                &CancellationToken::new(),
                &|_| {},
            )
            .await?;
            match outcome {
                HttpDownloadOutcome::Completed => fs::rename(&partial, &path)?,
                HttpDownloadOutcome::Cancelled => bail!("Speaker model download was cancelled"),
            }
        }
        Ok(dir)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pins_the_revision_speakrs_loads() {
        assert_eq!(REVISION, speakrs::models::MODEL_REVISION);
        assert_eq!(FILES.len(), 9);
        let total: u64 = FILES.iter().map(|f| f.size).sum();
        assert!((32_000_000..34_000_000).contains(&total), "{total}");
    }
}
