//! The global relator config — which backend, which model, where it lives.
//!
//! Resolution mirrors the embedder's ([`b2_embed::EmbedConfig`]): the `[relator]`
//! table in the TOML at `$XDG_CONFIG_HOME/b2/config.toml` (if present) over compiled
//! defaults. A vault with no config file gets a working default — zero-config is the
//! happy path. The **API key is not read from here** — it comes from the environment
//! (`ANTHROPIC_API_KEY`), because a secret does not belong in a config file (the
//! repo-wide logging/secrets policy).

use crate::{RelateError, Result};
use serde::Deserialize;

/// The default relator model. Per Anthropic's guidance we default to the most
/// capable model and let the user downgrade deliberately; for a high-volume
/// classification run, override `model` to `claude-haiku-4-5` in the config to trade
/// some quality for cost.
pub const DEFAULT_MODEL: &str = "claude-opus-4-8";

/// The default API base URL. Overridable to a gateway/proxy via `base_url`.
pub const DEFAULT_BASE_URL: &str = "https://api.anthropic.com";

/// Per-request output cap. A typed verdict + a one-sentence explanation is small, so
/// a modest ceiling keeps latency and cost down without truncating the tool call.
pub const DEFAULT_MAX_TOKENS: u32 = 1024;

/// Which relator backend to use. The seam is pluggable (Claude first); a local
/// backend can be added here later without touching `b2-core` or the CLI's flow.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Backend {
    /// The Anthropic Messages API (the shipped default).
    Claude,
}

/// The resolved relator configuration.
#[derive(Debug, Clone)]
pub struct RelateConfig {
    pub backend: Backend,
    /// Model identifier — recorded in provenance `by` as `agent:<model>`, so a swap
    /// is observable there (like the embedder's `embed_model_id`).
    pub model: String,
    /// API base URL (no trailing slash needed).
    pub base_url: String,
    /// Per-request `max_tokens`.
    pub max_tokens: u32,
}

/// The `[relator]` table as written in TOML. Every field optional → any subset
/// overrides the defaults.
#[derive(Debug, Default, Deserialize)]
struct RawFile {
    #[serde(default)]
    relator: RawRelator,
}

#[derive(Debug, Default, Deserialize)]
struct RawRelator {
    backend: Option<String>,
    model: Option<String>,
    base_url: Option<String>,
    max_tokens: Option<u32>,
}

impl RelateConfig {
    /// Load from the standard config path, falling back to defaults where the file
    /// (or any field) is absent.
    pub fn load() -> Result<Self> {
        let raw = match Self::config_path() {
            Some(p) if p.is_file() => {
                let text = std::fs::read_to_string(&p)?;
                toml::from_str::<RawFile>(&text)
                    .map_err(|e| RelateError::Config(format!("{}: {e}", p.display())))?
            }
            _ => RawFile::default(),
        };
        Self::from_raw(raw.relator)
    }

    /// The standard config file location — the same file the embedder reads,
    /// `$XDG_CONFIG_HOME/b2/config.toml` (its `[relator]` table).
    pub fn config_path() -> Option<std::path::PathBuf> {
        dirs::config_dir().map(|d| d.join("b2").join("config.toml"))
    }

    fn from_raw(r: RawRelator) -> Result<Self> {
        let backend = match r.backend.as_deref() {
            None | Some("claude") => Backend::Claude,
            Some(other) => {
                return Err(RelateError::Config(format!(
                    "unknown relator backend '{other}'; only 'claude' is supported"
                )))
            }
        };
        Ok(Self {
            backend,
            model: r.model.unwrap_or_else(|| DEFAULT_MODEL.to_string()),
            base_url: r.base_url.unwrap_or_else(|| DEFAULT_BASE_URL.to_string()),
            max_tokens: r.max_tokens.unwrap_or(DEFAULT_MAX_TOKENS),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_when_no_file() {
        let c = RelateConfig::from_raw(RawRelator::default()).unwrap();
        assert_eq!(c.backend, Backend::Claude);
        assert_eq!(c.model, DEFAULT_MODEL);
        assert_eq!(c.base_url, DEFAULT_BASE_URL);
        assert_eq!(c.max_tokens, DEFAULT_MAX_TOKENS);
    }

    #[test]
    fn overrides_apply() {
        let r = RawRelator {
            model: Some("claude-haiku-4-5".into()),
            base_url: Some("https://proxy.example".into()),
            max_tokens: Some(512),
            ..Default::default()
        };
        let c = RelateConfig::from_raw(r).unwrap();
        assert_eq!(c.model, "claude-haiku-4-5");
        assert_eq!(c.base_url, "https://proxy.example");
        assert_eq!(c.max_tokens, 512);
    }

    #[test]
    fn unknown_backend_is_a_config_error() {
        let r = RawRelator {
            backend: Some("ollama".into()),
            ..Default::default()
        };
        assert!(matches!(
            RelateConfig::from_raw(r),
            Err(RelateError::Config(_))
        ));
    }

    #[test]
    fn parses_a_relator_table() {
        let text = r#"
            [relator]
            model = "claude-sonnet-5"
            max_tokens = 800
        "#;
        let raw: RawFile = toml::from_str(text).unwrap();
        let c = RelateConfig::from_raw(raw.relator).unwrap();
        assert_eq!(c.model, "claude-sonnet-5");
        assert_eq!(c.max_tokens, 800);
    }
}
