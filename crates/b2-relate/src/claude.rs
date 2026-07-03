//! The Claude-backed [`Relator`]: one candidate pair → one forced-tool Messages-API
//! call → a typed verdict. The real precision gate of connection discovery.

use crate::config::{Backend, RelateConfig};
use crate::prompt;
use crate::{RelateError, Result};
use b2_core::relate::{Candidate, NoteCtx, Proposal, Relator};
use b2_core::Error as CoreError;
use serde_json::Value;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

/// Cumulative token usage across a relator's calls in one run — reported from each
/// Messages-API response's `usage` block. The CLI reads it after a run to print a cost
/// summary (transient — this is not persisted; see the audit-log discussion in the
/// discovery docs).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Usage {
    pub input_tokens: u64,
    pub output_tokens: u64,
    /// Successful API calls whose usage was counted.
    pub calls: u64,
}

/// A relator that classifies a candidate pair by calling the Anthropic Messages API.
/// Owns its own HTTP client, endpoint, key, and model so [`relate`](Relator::relate)
/// is a self-contained request. Token usage is accumulated across calls (atomics, so
/// `&self` is enough) and read back via [`usage`](Self::usage).
pub struct ClaudeRelator {
    agent: ureq::Agent,
    endpoint: String,
    api_key: String,
    model: String,
    max_tokens: u32,
    input_tokens: AtomicU64,
    output_tokens: AtomicU64,
    calls: AtomicU64,
}

impl ClaudeRelator {
    /// Build from resolved [`RelateConfig`]. Reads `ANTHROPIC_API_KEY` from the
    /// environment and **fails fast** with [`RelateError::MissingApiKey`] if it is
    /// unset or empty — so a missing key surfaces before any generation, never as a
    /// mid-run 401.
    pub fn from_config(config: &RelateConfig) -> Result<Self> {
        // Backend is Claude-only today; the match is the pluggable seam that a local
        // backend would extend (a new arm returning a different `Relator`).
        match config.backend {
            Backend::Claude => {}
        }
        let api_key = std::env::var("ANTHROPIC_API_KEY")
            .ok()
            .filter(|k| !k.trim().is_empty())
            .ok_or(RelateError::MissingApiKey)?;
        let agent = ureq::AgentBuilder::new()
            .timeout(Duration::from_secs(60))
            .build();
        let base = config.base_url.trim_end_matches('/');
        Ok(Self {
            agent,
            endpoint: format!("{base}/v1/messages"),
            api_key,
            model: config.model.clone(),
            max_tokens: config.max_tokens,
            input_tokens: AtomicU64::new(0),
            output_tokens: AtomicU64::new(0),
            calls: AtomicU64::new(0),
        })
    }

    /// Cumulative token usage across every call this relator has made so far.
    pub fn usage(&self) -> Usage {
        Usage {
            input_tokens: self.input_tokens.load(Ordering::Relaxed),
            output_tokens: self.output_tokens.load(Ordering::Relaxed),
            calls: self.calls.load(Ordering::Relaxed),
        }
    }
}

impl Relator for ClaudeRelator {
    fn model_id(&self) -> &str {
        &self.model
    }

    fn relate(&self, anchor: &NoteCtx, candidate: &Candidate) -> b2_core::Result<Option<Proposal>> {
        let body = prompt::build_request(&self.model, self.max_tokens, anchor, candidate);
        let resp = self
            .agent
            .post(&self.endpoint)
            .set("x-api-key", &self.api_key)
            .set("anthropic-version", "2023-06-01")
            .set("content-type", "application/json")
            .send_json(body)
            .map_err(|e| CoreError::Relator(describe(e)))?;
        let parsed: Value = resp
            .into_json()
            .map_err(|e| CoreError::Relator(format!("reading response: {e}")))?;
        self.record_usage(&parsed);
        prompt::interpret(&parsed).map_err(CoreError::Relator)
    }
}

impl ClaudeRelator {
    /// Fold this response's `usage` block into the cumulative counters. Best-effort —
    /// a missing/partial block just adds zero; usage is a report, never load-bearing.
    fn record_usage(&self, body: &Value) {
        let Some(u) = body.get("usage") else { return };
        let it = u.get("input_tokens").and_then(Value::as_u64).unwrap_or(0);
        let ot = u.get("output_tokens").and_then(Value::as_u64).unwrap_or(0);
        self.input_tokens.fetch_add(it, Ordering::Relaxed);
        self.output_tokens.fetch_add(ot, Ordering::Relaxed);
        self.calls.fetch_add(1, Ordering::Relaxed);
    }
}

/// A short, internal-only description of a transport/HTTP failure. The API's error
/// body may echo request content, so it is deliberately **not** included — the CLI
/// shows only a generic, actionable message (and this detail only under `B2_DEBUG`).
fn describe(err: ureq::Error) -> String {
    match err {
        ureq::Error::Status(code, _) => format!("HTTP {code}"),
        ureq::Error::Transport(t) => format!("transport error: {t}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Live smoke test — **out of CI** (needs a real key + network + spend). Run
    /// manually: `ANTHROPIC_API_KEY=… cargo test -p b2-relate -- --ignored`. Proves
    /// the wiring end-to-end: a real call returns a well-formed verdict (a core-verb
    /// proposal or a decline), never a transport/parse error.
    #[test]
    #[ignore]
    fn live_classifies_a_pair() {
        if std::env::var("ANTHROPIC_API_KEY").is_err() {
            eprintln!("skipping: ANTHROPIC_API_KEY not set");
            return;
        }
        let relator = ClaudeRelator::from_config(&RelateConfig {
            backend: Backend::Claude,
            model: crate::DEFAULT_MODEL.to_string(),
            base_url: crate::DEFAULT_BASE_URL.to_string(),
            max_tokens: crate::DEFAULT_MAX_TOKENS,
        })
        .expect("key is set");

        let anchor = NoteCtx {
            b2id: "A",
            title: Some("Espresso extraction"),
            text: "Espresso is brewed by forcing hot water through finely-ground coffee under pressure.",
        };
        let candidate = Candidate {
            note: NoteCtx {
                b2id: "B",
                title: Some("Grind size"),
                text: "Grind size controls extraction: too fine over-extracts and tastes bitter.",
            },
            evidence_chunk: "too fine over-extracts and tastes bitter",
            signal: "semantic:maxsim",
            score: 0.9,
        };

        let verdict = relator
            .relate(&anchor, &candidate)
            .expect("no transport error");
        if let Some(p) = verdict {
            assert!(
                b2_core::relation::is_core(&p.edge_type),
                "expected a core verb, got {:?}",
                p.edge_type
            );
            assert!((0.0..=1.0).contains(&p.confidence));
        }
        // Usage was captured from the response.
        let u = relator.usage();
        assert_eq!(u.calls, 1);
        assert!(u.input_tokens > 0 && u.output_tokens > 0, "tokens recorded");
    }
}
