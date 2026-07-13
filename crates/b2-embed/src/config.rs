//! The global embedder config — where the model comes from and where it's cached.
//!
//! Resolution order: the TOML at `$XDG_CONFIG_HOME/b2/config.toml` (if present)
//! over compiled defaults; the `HF_ENDPOINT` env var (the standard Hugging Face
//! mirror knob) overrides the endpoint on top. A vault with no config file gets a
//! working default — zero-config is the happy path (the fail-fast config rule
//! applies to the *model files*, via `b2 init`, not to this file).

use crate::{EmbedError, Result};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

/// The default model: BAAI/bge-base-en-v1.5 — BERT-family, 768-dim, ungated, and
/// validated in the spike. `dim` is *not* set here; it is read authoritatively from
/// the model's own `config.json` (`hidden_size`) at load, so config can never lie
/// about it.
pub const DEFAULT_MODEL: &str = "BAAI/bge-base-en-v1.5";

/// One embedding model B2 knows how to run, as offered in the desktop's settings
/// picker. [`AVAILABLE_MODELS`] is the **single source of truth** for "which models B2
/// supports": adding one is a single entry here — no host or UI change, the picker
/// fills automatically. `id` is the Hugging Face repo id, which is exactly what lands
/// in `meta.embed_model_id`, so choosing a different one *is* the model swap that
/// re-embeds on the next `reindex` (index-engine.md §8).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ModelInfo {
    /// Repo id == `meta.embed_model_id` (e.g. `BAAI/bge-base-en-v1.5`).
    pub id: &'static str,
    /// Human label for the picker (e.g. `BGE Base EN v1.5`).
    pub label: &'static str,
    /// Embedding dimension, for display. Authoritatively re-read from the model's own
    /// `config.json` at load; this is only the picker's at-a-glance number.
    pub dim: usize,
    /// One-line "why pick this" shown under the picker.
    pub description: &'static str,
}

/// Every model the picker can offer. Each is BERT-family + ungated with the three
/// [`REQUIRED_FILES`](crate::model::REQUIRED_FILES), so it drops into the same
/// [`LocalEmbedder`](crate::LocalEmbedder) with no code change — the real dim is read from
/// the model's own `config.json` at load, and the query prefix is shared. `dim` here is
/// display-only. More land here as they're vetted.
pub const AVAILABLE_MODELS: &[ModelInfo] = &[
    ModelInfo {
        id: DEFAULT_MODEL,
        label: "BGE Base EN v1.5",
        dim: 768,
        description: "Balanced quality and size (768-dim). B2's default.",
    },
    ModelInfo {
        id: "BAAI/bge-small-en-v1.5",
        label: "BGE Small EN v1.5",
        dim: 384,
        description: "Smaller and faster (384-dim); modestly lower retrieval quality. \
                      Good for large vaults where the full embed is slow.",
    },
];

/// The registry entry for `id`, or `None` if it isn't a model B2 supports — the guard
/// [`EmbedConfig::set_model`] uses to refuse writing a config the loader can't provision.
pub fn find_model(id: &str) -> Option<&'static ModelInfo> {
    AVAILABLE_MODELS.iter().find(|m| m.id == id)
}

/// A registry model annotated for the settings picker: which one is configured now, and
/// which are already downloaded. Owned (unlike [`ModelInfo`]'s `&'static str`s) because
/// it crosses the IPC boundary as the `list_models` / `set_model` payload.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ModelChoice {
    pub id: String,
    pub label: String,
    pub dim: usize,
    pub description: String,
    /// The model this config currently resolves to (from config.toml, else the default).
    pub current: bool,
    /// Already provisioned into the shared cache (all required files present); when
    /// false, choosing it needs a `b2 init` before it can embed.
    pub installed: bool,
}

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

    /// Whether the model with repo id `model` is already provisioned in *this* config's
    /// cache — all [`REQUIRED_FILES`](crate::model::REQUIRED_FILES) present. A cheap
    /// file-existence check (no model load), so the settings picker can flag which
    /// choices are installed vs. still need `b2 init`. Uses the config's own `cache_dir`
    /// so a custom cache is honored, not just the default.
    pub fn is_model_provisioned(&self, model: &str) -> bool {
        let dir = self.cache_dir.join(sanitize(model));
        crate::model::REQUIRED_FILES
            .iter()
            .all(|f| dir.join(f).is_file())
    }

    /// The full registry annotated against this config — the data the settings picker
    /// renders. Pure (a function of `self` + [`AVAILABLE_MODELS`]), so both the mapping
    /// and the current/installed flags are unit-testable without the real config dir.
    pub fn model_choices(&self) -> Vec<ModelChoice> {
        AVAILABLE_MODELS
            .iter()
            .map(|m| ModelChoice {
                id: m.id.to_string(),
                label: m.label.to_string(),
                dim: m.dim,
                description: m.description.to_string(),
                current: m.id == self.model,
                installed: self.is_model_provisioned(m.id),
            })
            .collect()
    }

    /// Persist `model` as the configured embedder in the standard `config.toml`,
    /// preserving every other field and table already there. This is the **one config
    /// both adapters read** (`load`), so the CLI and desktop always agree on the model —
    /// a divergence would build the vault's vectors with one model and read them with
    /// another, which `search` refuses (`ModelMismatch`). Refuses an id not in
    /// [`AVAILABLE_MODELS`] rather than write a config the loader can't provision.
    pub fn set_model(model: &str) -> Result<()> {
        let path = Self::config_path()
            .ok_or_else(|| EmbedError::Config("no config directory on this platform".into()))?;
        Self::write_model(&path, model)
    }

    /// [`set_model`](Self::set_model) against an explicit path — the testable core (a
    /// tempfile stands in for the real config). Validates *before* touching the
    /// filesystem, so a rejected model leaves no file behind; creates the file (and its
    /// parent dir) when absent, giving zero-config users a minimal `[embedder]` table.
    fn write_model(path: &Path, model: &str) -> Result<()> {
        if find_model(model).is_none() {
            return Err(EmbedError::UnknownModel(model.to_string()));
        }
        // Read the existing document (if any) so other keys/tables survive the write.
        let mut doc: toml::Table = match std::fs::read_to_string(path) {
            Ok(text) => toml::from_str(&text)
                .map_err(|e| EmbedError::Config(format!("{}: {e}", path.display())))?,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => toml::Table::new(),
            Err(e) => return Err(EmbedError::Io(e)),
        };
        // Set (or create) `[embedder].model`, leaving any sibling fields (source,
        // cache_dir, query_prefix) intact. An existing non-table `embedder` is malformed;
        // replace it rather than propagate the corruption.
        let embedder = doc
            .entry("embedder".to_string())
            .or_insert_with(|| toml::Value::Table(toml::Table::new()));
        if !embedder.is_table() {
            *embedder = toml::Value::Table(toml::Table::new());
        }
        if let Some(table) = embedder.as_table_mut() {
            table.insert("model".to_string(), toml::Value::String(model.to_string()));
        }
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let text = toml::to_string(&doc)
            .map_err(|e| EmbedError::Config(format!("serialize config: {e}")))?;
        std::fs::write(path, text)?;
        Ok(())
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

    // --- model registry + selection (settings picker) ----------------------------

    #[test]
    fn registry_holds_the_default_with_unique_ids() {
        assert!(!AVAILABLE_MODELS.is_empty());
        assert!(
            find_model(DEFAULT_MODEL).is_some(),
            "default must be offered"
        );
        assert!(find_model("no/such-model").is_none());
        // No duplicate ids (the picker keys on id, and set_model validates against it).
        let mut ids: Vec<_> = AVAILABLE_MODELS.iter().map(|m| m.id).collect();
        ids.sort_unstable();
        ids.dedup();
        assert_eq!(ids.len(), AVAILABLE_MODELS.len());
    }

    #[test]
    fn model_choices_flag_current_and_installed() {
        let tmp = tempfile::TempDir::new().unwrap();
        // Default model, cache pointed at an empty temp dir → current but not installed.
        let mut c = EmbedConfig::from_raw(RawEmbedder::default());
        c.cache_dir = tmp.path().to_path_buf();
        let choices = c.model_choices();
        assert_eq!(choices.len(), AVAILABLE_MODELS.len());
        let current: Vec<_> = choices.iter().filter(|c| c.current).collect();
        assert_eq!(current.len(), 1, "exactly one model is current");
        assert_eq!(current[0].id, DEFAULT_MODEL);
        assert!(!current[0].installed, "empty cache ⇒ not installed");

        // Drop the required files into the model dir → the flag flips to installed.
        let dir = c.model_dir();
        std::fs::create_dir_all(&dir).unwrap();
        for f in crate::model::REQUIRED_FILES {
            std::fs::write(dir.join(f), b"x").unwrap();
        }
        let installed = c
            .model_choices()
            .into_iter()
            .find(|x| x.id == DEFAULT_MODEL)
            .unwrap();
        assert!(installed.installed);
    }

    #[test]
    fn write_model_round_trips_and_preserves_sibling_fields() {
        let tmp = tempfile::TempDir::new().unwrap();
        let path = tmp.path().join("b2/config.toml");
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        // A pre-existing config with other embedder fields set (and no model yet).
        std::fs::write(
            &path,
            "[embedder]\nsource = \"https://mirror.example\"\nquery_prefix = \"Q: \"\n",
        )
        .unwrap();

        EmbedConfig::write_model(&path, DEFAULT_MODEL).unwrap();

        let raw: RawFile = toml::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(raw.embedder.model.as_deref(), Some(DEFAULT_MODEL));
        // Siblings survive the write — set_model touches only `model`.
        assert_eq!(
            raw.embedder.source.as_deref(),
            Some("https://mirror.example")
        );
        assert_eq!(raw.embedder.query_prefix.as_deref(), Some("Q: "));
    }

    #[test]
    fn write_model_creates_missing_file_and_parent() {
        let tmp = tempfile::TempDir::new().unwrap();
        // Neither the file nor its parent dir exists yet — write must `mkdir -p`.
        let path = tmp.path().join("state/b2/config.toml");
        EmbedConfig::write_model(&path, DEFAULT_MODEL).unwrap();
        let raw: RawFile = toml::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(raw.embedder.model.as_deref(), Some(DEFAULT_MODEL));
    }

    #[test]
    fn write_model_rejects_unknown_and_leaves_no_file() {
        let tmp = tempfile::TempDir::new().unwrap();
        let path = tmp.path().join("config.toml");
        let err = EmbedConfig::write_model(&path, "definitely/not-a-model").unwrap_err();
        assert!(matches!(err, EmbedError::UnknownModel(_)));
        assert!(!path.exists(), "a rejected write must not create the file");
    }
}
