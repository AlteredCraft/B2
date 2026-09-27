//! Cumulative per-model embedding time, shown in Settings so a model swap can be judged on
//! its real speed. The adapter times the embed pass because `b2-core` has no clock.
//!
//! A bucket covers the model's current stint: [`reset`] drops it on a switch to that model,
//! since the swap re-embeds the whole corpus (ADR-0007). Best-effort throughout.

use crate::state_file;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::Path;

/// The ledger's state file (see [`state_file`]).
const STATS_FILE: &str = "embed-stats.json";

/// One model's accumulated embedding cost; `total_ms / chunks` is its throughput.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ModelStat {
    /// Milliseconds spent embedding, excluding the model load.
    pub total_ms: u64,
    /// Total chunks embedded across those runs.
    pub chunks: u64,
    /// How many embed runs contributed to this total.
    pub runs: u64,
}

/// One model's row as the Settings pane reads it (`ui/src/types.ts`'s `EmbedStat`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct EmbedStat {
    pub model: String,
    #[serde(flatten)]
    pub stat: ModelStat,
}

/// The on-disk ledger: model id → its cumulative stat.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
struct StatsFile {
    models: BTreeMap<String, ModelStat>,
}

/// The whole ledger, one row per model. A missing or corrupt file reads as no history.
pub fn read_all() -> Vec<EmbedStat> {
    let Some(path) = state_file::path(STATS_FILE) else {
        return Vec::new();
    };
    read_from(&path)
        .models
        .into_iter()
        .map(|(model, stat)| EmbedStat { model, stat })
        .collect()
}

/// [`read_all`] against an explicit path.
fn read_from(path: &Path) -> StatsFile {
    std::fs::read_to_string(path)
        .ok()
        .and_then(|t| serde_json::from_str(&t).ok())
        .unwrap_or_default()
}

/// Serialize the ledger and rewrite it at `path`.
fn write_ledger(path: &Path, file: &StatsFile) -> std::io::Result<()> {
    let text = serde_json::to_string_pretty(file).map_err(std::io::Error::other)?;
    state_file::write(path, text.as_bytes())
}

/// Add one embed run to `model`'s running total. Best-effort: failures go to stderr.
pub fn record(model: &str, elapsed_ms: u64, chunks: u64) {
    state_file::update(STATS_FILE, "record embed stats", |path| {
        record_to(path, model, elapsed_ms, chunks)
    });
}

/// [`record`] against an explicit path. Creates the file on first use.
fn record_to(path: &Path, model: &str, elapsed_ms: u64, chunks: u64) -> std::io::Result<()> {
    let mut file = read_from(path);
    let entry = file.models.entry(model.to_string()).or_default();
    entry.total_ms = entry.total_ms.saturating_add(elapsed_ms);
    entry.chunks = entry.chunks.saturating_add(chunks);
    entry.runs = entry.runs.saturating_add(1);
    write_ledger(path, &file)
}

/// Forget `model`'s total, called when the user switches to it: the next reindex re-embeds
/// the whole corpus, which must not stack onto the old bucket. Other models keep their
/// history. Best-effort.
pub fn reset(model: &str) {
    state_file::update(STATS_FILE, "reset embed stats", |path| {
        reset_in(path, model)
    });
}

/// [`reset`] against an explicit path. Writes nothing when the model has no history.
fn reset_in(path: &Path, model: &str) -> std::io::Result<()> {
    let mut file = read_from(path);
    if file.models.remove(model).is_none() {
        return Ok(());
    }
    write_ledger(path, &file)
}

#[cfg(test)]
mod tests {
    //! Hermetic: every case runs against a tempfile, never the real data dir.

    use super::*;

    #[test]
    fn record_accumulates_across_runs_per_model() {
        let tmp = tempfile::TempDir::new().unwrap();
        // Parent dir does not exist yet.
        let path = tmp.path().join("state/b2/embed-stats.json");

        record_to(&path, "m/base", 1000, 40).unwrap();
        record_to(&path, "m/base", 2500, 60).unwrap();
        record_to(&path, "m/small", 300, 50).unwrap();

        let ledger: BTreeMap<_, _> = read_from(&path).models;
        let base = &ledger["m/base"];
        assert_eq!(base.total_ms, 3500, "two runs' ms sum");
        assert_eq!(base.chunks, 100);
        assert_eq!(base.runs, 2);
        let small = &ledger["m/small"];
        assert_eq!(small.total_ms, 300);
        assert_eq!(small.runs, 1);
    }

    #[test]
    fn reset_drops_only_the_switched_to_models_bucket() {
        let tmp = tempfile::TempDir::new().unwrap();
        let path = tmp.path().join("state/b2/embed-stats.json");

        record_to(&path, "m/base", 1000, 40).unwrap();
        record_to(&path, "m/small", 300, 50).unwrap();

        reset_in(&path, "m/base").unwrap();

        let ledger = read_from(&path).models;
        assert!(
            !ledger.contains_key("m/base"),
            "reset model's bucket is gone"
        );
        assert_eq!(
            ledger["m/small"].chunks, 50,
            "other model survives the reset"
        );

        // The next run starts from zero, not stacked onto the old 40.
        record_to(&path, "m/base", 2000, 60).unwrap();
        let base = &read_from(&path).models["m/base"];
        assert_eq!(base.chunks, 60);
        assert_eq!(base.runs, 1);
    }

    #[test]
    fn reset_is_a_noop_for_unknown_model_or_missing_file() {
        let tmp = tempfile::TempDir::new().unwrap();

        let absent = tmp.path().join("absent/embed-stats.json");
        reset_in(&absent, "m/base").unwrap();
        assert!(
            !absent.exists(),
            "reset must not create a file when there's no history"
        );

        let path = tmp.path().join("embed-stats.json");
        record_to(&path, "m/base", 1000, 40).unwrap();
        reset_in(&path, "m/never-embedded").unwrap();
        assert_eq!(
            read_from(&path).models["m/base"].chunks,
            40,
            "an unrelated bucket is intact after a no-op reset"
        );
    }

    /// The Settings pane reads these four keys (`ui/src/types.ts`'s `EmbedStat`).
    #[test]
    fn a_ledger_row_crosses_ipc_with_the_ui_field_names() {
        let row = EmbedStat {
            model: "m/base".into(),
            stat: ModelStat {
                total_ms: 3500,
                chunks: 100,
                runs: 2,
            },
        };
        assert_eq!(
            serde_json::to_value(&row).unwrap(),
            serde_json::json!({"model": "m/base", "total_ms": 3500, "chunks": 100, "runs": 2})
        );
    }

    #[test]
    fn missing_or_corrupt_file_reads_as_empty() {
        let tmp = tempfile::TempDir::new().unwrap();
        assert!(read_from(&tmp.path().join("absent.json")).models.is_empty());
        let bad = tmp.path().join("bad.json");
        std::fs::write(&bad, "not json {{{").unwrap();
        assert!(read_from(&bad).models.is_empty());
    }
}
