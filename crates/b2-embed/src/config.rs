//! The global embedder config — where the model comes from and where it's cached.
//!
//! Resolution order: the TOML at `$XDG_CONFIG_HOME/b2/config.toml` (if present)
//! over compiled defaults; the `HF_ENDPOINT` env var (the standard Hugging Face
//! mirror knob) overrides the endpoint on top. A vault with no config file gets a
//! working default — zero-config is the happy path (the fail-fast config rule
//! applies to the *model files*, via `b2 init`, not to this file).

use crate::{EmbedError, Result};
use serde::Deserialize;
use std::path::{Path, PathBuf};

/// The default model: BAAI/bge-base-en-v1.5 — BERT-family, 768-dim, ungated, and
/// validated in the spike. `dim` is *not* set here; it is read authoritatively from
/// the model's own `config.json` (`hidden_size`) at load, so config can never lie
/// about it.
pub const DEFAULT_MODEL: &str = "BAAI/bge-base-en-v1.5";

/// bge's retrieval instruction, prepended to *queries only* (asymmetric retrieval,
/// index-engine.md §5). Documents are embedded verbatim. Empty ⇒ symmetric.
pub const DEFAULT_QUERY_PREFIX: &str = "Represent this sentence for searching relevant passages: ";

/// Where the model files are fetched from when `b2 init` provisions them.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Source {
    /// A Hugging Face repo (default), optionally through a mirror `endpoint`.
    Hf {
        repo: String,
        endpoint: Option<String>,
    },
    /// A local directory holding the model files — a fully-offline install.
    Local(PathBuf),
}

/// The resolved embedder configuration.
#[derive(Debug, Clone)]
pub struct EmbedConfig {
    /// Model identifier — recorded as `meta.embed_model_id`, so changing it is a
    /// model swap that re-embeds on the next `reindex` (index-engine.md §8).
    pub model: String,
    /// Where to fetch the files from.
    pub source: Source,
    /// The shared, machine-level cache dir (XDG data). One copy per machine, *not*
    /// per-vault `.b2/` — the model is a runtime dep, not vault data.
    pub cache_dir: PathBuf,
    /// The query-side prompt prefix (empty ⇒ symmetric embedding).
    pub query_prefix: String,
}

/// The `[embedder]` table as written in TOML. Every field optional → any subset
/// overrides the defaults.
#[derive(Debug, Default, Deserialize)]
struct RawFile {
    #[serde(default)]
    embedder: RawEmbedder,
}

#[derive(Debug, Default, Deserialize)]
struct RawEmbedder {
    model: Option<String>,
    /// Free-form: a URL (mirror endpoint), a local path, or an alternate repo id.
    source: Option<String>,
    cache_dir: Option<String>,
    query_prefix: Option<String>,
}

impl EmbedConfig {
    /// Load from the standard config path, falling back to defaults where the file
    /// (or any field) is absent. `HF_ENDPOINT` overrides the mirror endpoint last.
    pub fn load() -> Result<Self> {
        let raw = match Self::config_path() {
            Some(p) if p.is_file() => {
                let text = std::fs::read_to_string(&p)?;
                toml::from_str::<RawFile>(&text)
                    .map_err(|e| EmbedError::Config(format!("{}: {e}", p.display())))?
            }
            _ => RawFile::default(),
        };
        Ok(Self::from_raw(raw.embedder))
    }

    /// The standard config file location: `$XDG_CONFIG_HOME/b2/config.toml`.
    pub fn config_path() -> Option<PathBuf> {
        dirs::config_dir().map(|d| d.join("b2").join("config.toml"))
    }

    fn from_raw(e: RawEmbedder) -> Self {
        let model = e.model.unwrap_or_else(|| DEFAULT_MODEL.to_string());
        let endpoint_env = std::env::var("HF_ENDPOINT").ok().filter(|s| !s.is_empty());
        let source = Self::resolve_source(&model, e.source, endpoint_env);
        let cache_dir = e
            .cache_dir
            .map(PathBuf::from)
            .unwrap_or_else(default_cache_dir);
        let query_prefix = e
            .query_prefix
            .unwrap_or_else(|| DEFAULT_QUERY_PREFIX.to_string());
        Self {
            model,
            source,
            cache_dir,
            query_prefix,
        }
    }

    /// Interpret the free-form `source` string (locked as one field): a URL is a
    /// mirror endpoint; an existing filesystem path is a local install; anything
    /// else is an alternate repo id. Absent ⇒ the default HF repo == `model`.
    fn resolve_source(model: &str, source: Option<String>, endpoint_env: Option<String>) -> Source {
        match source {
            Some(s) if is_url(&s) => Source::Hf {
                repo: model.to_string(),
                endpoint: Some(s),
            },
            Some(s) if Path::new(&s).is_dir() => Source::Local(PathBuf::from(s)),
            Some(s) => Source::Hf {
                repo: s,
                endpoint: endpoint_env,
            },
            None => Source::Hf {
                repo: model.to_string(),
                endpoint: endpoint_env,
            },
        }
    }

    /// The flat directory the model's files live in under the cache — predictable
    /// (`<cache_dir>/<sanitized-model>`) so "is it installed?" is a plain file check.
    pub fn model_dir(&self) -> PathBuf {
        self.cache_dir.join(sanitize(&self.model))
    }
}

/// Default XDG cache: `~/.local/share/b2/models` (falls back to `./.b2-models` only
/// if no home/data dir is discoverable, which is not expected on a normal machine).
pub fn default_cache_dir() -> PathBuf {
    dirs::data_dir()
        .map(|d| d.join("b2").join("models"))
        .unwrap_or_else(|| PathBuf::from(".b2-models"))
}

fn is_url(s: &str) -> bool {
    s.starts_with("http://") || s.starts_with("https://")
}

/// Make a filesystem-safe directory name from a repo id (`a/b` → `a_b`).
pub fn sanitize(model: &str) -> String {
    model
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.') {
                c
            } else {
                '_'
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_when_no_file() {
        let c = EmbedConfig::from_raw(RawEmbedder::default());
        assert_eq!(c.model, DEFAULT_MODEL);
        assert!(matches!(c.source, Source::Hf { endpoint: None, .. }));
        assert!(c.query_prefix.starts_with("Represent"));
    }

    #[test]
    fn url_source_is_a_mirror_endpoint() {
        let e = RawEmbedder {
            source: Some("https://hf-mirror.com".into()),
            ..Default::default()
        };
        let c = EmbedConfig::from_raw(e);
        assert_eq!(
            c.source,
            Source::Hf {
                repo: DEFAULT_MODEL.into(),
                endpoint: Some("https://hf-mirror.com".into())
            }
        );
    }

    #[test]
    fn alternate_repo_source() {
        let e = RawEmbedder {
            source: Some("BAAI/bge-small-en-v1.5".into()),
            ..Default::default()
        };
        // model id stays what `model` says; source repo is the override.
        let c = EmbedConfig::from_raw(e);
        assert!(matches!(c.source, Source::Hf { repo, .. } if repo == "BAAI/bge-small-en-v1.5"));
    }

    #[test]
    fn sanitize_repo_id() {
        assert_eq!(sanitize("BAAI/bge-base-en-v1.5"), "BAAI_bge-base-en-v1.5");
    }
}
