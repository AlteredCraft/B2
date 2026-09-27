//! Model provisioning, the work behind `b2 init`: fetch the model into the shared cache and
//! verify it by loading and embedding once. Idempotent.

use crate::config::{EmbedConfig, Source};
use crate::model::{files_present, LocalEmbedder, REQUIRED_FILES};
use crate::{EmbedError, Result};
use b2_core::embed::Embedder;
use hf_hub::api::sync::ApiBuilder;
use std::path::{Path, PathBuf};

/// What `provision` did, for a friendly CLI summary.
#[derive(Debug, Clone, serde::Serialize)]
pub struct ProvisionReport {
    pub model: String,
    pub model_dir: PathBuf,
    /// The model's embedding dimension, read back from the loaded model.
    pub dim: usize,
    /// True if everything was already present and loadable (no download happened).
    pub already_present: bool,
}

/// Provision the model named by `config` into its cache dir, reporting progress through
/// `log` (the CLI owns stdout). Verifying here catches a corrupt download before the
/// first `search`.
pub fn provision(config: &EmbedConfig, mut log: impl FnMut(&str)) -> Result<ProvisionReport> {
    let model_dir = config.model_dir();

    if files_present(&model_dir) {
        if let Ok(e) = LocalEmbedder::load(config) {
            log(&format!("Model '{}' already installed.", config.model));
            return Ok(ProvisionReport {
                model: config.model.clone(),
                model_dir,
                dim: e.dim(),
                already_present: true,
            });
        }
        log("Existing model files are incomplete; re-fetching.");
    }

    std::fs::create_dir_all(&model_dir)?;
    match &config.source {
        Source::Local(dir) => fetch_local(dir, &model_dir, &mut log)?,
        Source::Hf { repo, endpoint } => {
            fetch_hf(repo, endpoint.as_deref(), config, &model_dir, &mut log)?
        }
    }

    log("Verifying model…");
    let embedder = LocalEmbedder::load(config)?;
    let probe = embedder
        .embed("b2 model verification probe")
        .map_err(|e| EmbedError::Load(e.to_string()))?;
    if probe.len() != embedder.dim() || !probe.iter().all(|x| x.is_finite()) {
        return Err(EmbedError::Load(
            "verification embedding was malformed".into(),
        ));
    }
    log(&format!(
        "Model '{}' ready ({} dims) at {}.",
        config.model,
        embedder.dim(),
        model_dir.display()
    ));

    Ok(ProvisionReport {
        model: config.model.clone(),
        model_dir,
        dim: embedder.dim(),
        already_present: false,
    })
}

/// Copy the required files from a local model directory (fully-offline install).
fn fetch_local(src: &Path, model_dir: &Path, log: &mut impl FnMut(&str)) -> Result<()> {
    for f in REQUIRED_FILES {
        let from = src.join(f);
        if !from.is_file() {
            return Err(EmbedError::Config(format!(
                "local source is missing {f} (looked in {})",
                src.display()
            )));
        }
        log(&format!("Copying {f}…"));
        std::fs::copy(&from, model_dir.join(f))?;
    }
    Ok(())
}

/// Download the required files from Hugging Face into the shared cache, then copy
/// them into the flat model dir so "installed?" stays a plain file check.
fn fetch_hf(
    repo: &str,
    endpoint: Option<&str>,
    config: &EmbedConfig,
    model_dir: &Path,
    log: &mut impl FnMut(&str),
) -> Result<()> {
    let mut builder = ApiBuilder::new()
        .with_progress(false)
        .with_cache_dir(config.cache_dir.join(".hf-cache"));
    if let Some(ep) = endpoint {
        builder = builder.with_endpoint(ep.to_string());
    }
    let api = builder
        .build()
        .map_err(|e| EmbedError::Download(e.to_string()))?
        .model(repo.to_string());

    for f in REQUIRED_FILES {
        log(&format!("Downloading {f} from {repo}…"));
        let cached = api
            .get(f)
            .map_err(|e| EmbedError::Download(format!("{f}: {e}")))?;
        std::fs::copy(&cached, model_dir.join(f))?;
    }
    Ok(())
}
