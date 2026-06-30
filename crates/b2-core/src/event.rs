//! The event-log tier (planning/data-model.md §4): the durable, append-only
//! source of truth for *history* + review state, behind a thin `append`/`read_all`
//! sink so the JSONL backing can later become an append-only SQLite log with no
//! data-model change. `replay(log) ⇒ review state` ([replay](crate::replay)) is
//! what makes `index = projection of (Markdown ∪ log)` hold: the pending queue and
//! rejection tombstones are the one part of the index not derivable from Markdown.

use crate::error::Result;
use serde::{Deserialize, Serialize};
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};

/// A consequential operation B2 performed, recorded with verbose payloads (model
/// id, confidence, evidence) — cheap to write, there if ever wanted. Only the
/// `suggestion.*` events drive review-state replay; the rest are history only.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "event")]
pub enum Event {
    /// B2 stamped a missing `b2id` into a note (its one always-allowed write).
    #[serde(rename = "b2id.stamped")]
    B2idStamped { b2id: String, path: String },

    /// An agent proposed a typed edge. Self-contained, so replay can rebuild the
    /// `edges` (suggested) + `edge_provenance` rows purely from the log.
    #[serde(rename = "suggestion.generated")]
    SuggestionGenerated {
        edge_id: String,
        src_id: String,
        dst_id: String,
        dst_path_raw: String,
        edge_type: String,
        explanation: Option<String>,
        by: String,
        source: Option<String>,
        confidence: Option<f64>,
        created: String,
    },

    /// A human accepted a suggestion. On replay this is a NO-OP for the queue —
    /// the active edge re-derives from the inline Markdown (Flow ①), so the
    /// suggested row simply leaves the queue (data-model.md §4).
    #[serde(rename = "suggestion.accepted")]
    SuggestionAccepted { edge_id: String, decided: String },

    /// A human rejected a suggestion — remembered as a tombstone so the same
    /// pair+type isn't proposed again.
    #[serde(rename = "suggestion.rejected")]
    SuggestionRejected { edge_id: String, decided: String },
}

/// Where consequential events go, and where replay reads them back. Behind a
/// trait so the durable JSONL sink and test doubles are interchangeable.
pub trait EventSink {
    /// Append one event durably.
    fn append(&self, event: &Event) -> Result<()>;
    /// Read every event, in append order (for replay).
    fn read_all(&self) -> Result<Vec<Event>>;
}

/// Default sink: drops every event and has no history. Used where the durable log
/// isn't wanted (and by the step 1–3 paths that predate it).
#[derive(Debug, Default, Clone, Copy)]
pub struct NullSink;

impl EventSink for NullSink {
    fn append(&self, _event: &Event) -> Result<()> {
        Ok(())
    }
    fn read_all(&self) -> Result<Vec<Event>> {
        Ok(Vec::new())
    }
}

/// The durable, in-vault append-only log: JSONL at `<vault>/.b2/log/events.jsonl`
/// (a dotfolder Obsidian and the vault scanner both ignore). One event per line,
/// in append order — line order *is* sequence order.
#[derive(Debug, Clone)]
pub struct JsonlSink {
    path: PathBuf,
}

impl JsonlSink {
    /// Open (creating `.b2/log/`) the log for `vault_root`.
    pub fn in_vault(vault_root: &Path) -> Result<Self> {
        let dir = vault_root.join(".b2").join("log");
        fs::create_dir_all(&dir)?;
        Ok(Self {
            path: dir.join("events.jsonl"),
        })
    }
}

impl EventSink for JsonlSink {
    fn append(&self, event: &Event) -> Result<()> {
        let line = serde_json::to_string(event)?;
        let mut f = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.path)?;
        writeln!(f, "{line}")?;
        Ok(())
    }

    fn read_all(&self) -> Result<Vec<Event>> {
        if !self.path.exists() {
            return Ok(Vec::new());
        }
        let text = fs::read_to_string(&self.path)?;
        let mut events = Vec::new();
        for line in text.lines() {
            if line.trim().is_empty() {
                continue;
            }
            events.push(serde_json::from_str(line)?);
        }
        Ok(events)
    }
}
