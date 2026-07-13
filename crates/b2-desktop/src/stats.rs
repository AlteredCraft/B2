//! Cumulative, per-model **embedding time** — the desktop's persistent "how much has
//! embedding cost with this model" ledger, surfaced in the Settings pane so a model swap
//! can be judged on its real speed (embed-perf work, 2026-07-13).
//!
//! Host-owned state, exactly like [`persist_last_vault`](crate::persist_last_vault):
//! `b2-core` stays **wall-clock-free** (the determinism rule), so the *adapter* times the
//! embed pass and accumulates the total here. Keyed by model id — switch models and each
//! bucket fills independently, so their totals (and derived throughput) are directly
//! comparable. It lives under the same `dirs` data dir as `last-vault` and the model
//! cache. Purely diagnostic: **best-effort**, and a read/write failure never fails an
//! embed (a corrupt/missing file just reads as "no history").

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

/// One model's accumulated embedding cost. `total_ms / chunks` is the throughput the
/// Settings pane shows; `runs` counts the embed passes that contributed.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ModelStat {
    /// Total wall-clock milliseconds spent embedding chunks with this model (the model
    /// *load* is excluded — the adapter starts the clock after the model is loaded, so
    /// this is embedding throughput, not one-time setup).
    pub total_ms: u64,
    /// Total chunks embedded across those runs — the throughput denominator.
    pub chunks: u64,
    /// How many embed runs contributed to this total.
    pub runs: u64,
}

/// The on-disk ledger: model id → its cumulative stat.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
struct StatsFile {
    models: BTreeMap<String, ModelStat>,
}

/// The ledger's path: `<data-dir>/b2/embed-stats.json` (the same vendor dir as
/// `last-vault` and the model cache). `None` when the platform has no data dir, in which
/// case recording is silently skipped.
fn stats_file() -> Option<PathBuf> {
    dirs::data_dir().map(|d| d.join("b2").join("embed-stats.json"))
}

/// The whole ledger, one `(model_id, stat)` per model, or empty when there's no data dir
/// / no file / an unreadable-or-corrupt file — stats are never load-bearing, so a bad
/// file degrades cleanly to "no history" rather than surfacing an error.
pub fn read_all() -> Vec<(String, ModelStat)> {
    let Some(path) = stats_file() else {
        return Vec::new();
    };
    read_from(&path).models.into_iter().collect()
}

/// [`read_all`] against an explicit path — the testable core. A missing or malformed file
/// reads as the empty ledger.
fn read_from(path: &Path) -> StatsFile {
    std::fs::read_to_string(path)
        .ok()
        .and_then(|t| serde_json::from_str(&t).ok())
        .unwrap_or_default()
}

/// Add one embed run's `(elapsed_ms, chunks)` to `model`'s running total. Best-effort:
/// a missing data dir or a write failure is logged to stderr and swallowed — recording a
/// measurement must never fail the embed the user actually asked for.
pub fn record(model: &str, elapsed_ms: u64, chunks: u64) {
    let Some(path) = stats_file() else {
        eprintln!("[b2] embed stats: no platform data directory; not recording");
        return;
    };
    if let Err(e) = record_to(&path, model, elapsed_ms, chunks) {
        eprintln!("[b2] embed stats: could not record ({e})");
    }
}

/// [`record`] against an explicit path — the testable core. Read-modify-write: load the
/// ledger, fold the run into `model`'s bucket (saturating, so a pathological total can't
/// panic), and rewrite. Creates the file (and parent dir) on first use.
fn record_to(path: &Path, model: &str, elapsed_ms: u64, chunks: u64) -> std::io::Result<()> {
    let mut file = read_from(path);
    let entry = file.models.entry(model.to_string()).or_default();
    entry.total_ms = entry.total_ms.saturating_add(elapsed_ms);
    entry.chunks = entry.chunks.saturating_add(chunks);
    entry.runs = entry.runs.saturating_add(1);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let text = serde_json::to_string_pretty(&file).map_err(std::io::Error::other)?;
    std::fs::write(path, text)
}

#[cfg(test)]
mod tests {
    //! Hermetic: every case runs against a tempfile, never the real data dir (which only
    //! the production `record`/`read_all` wrappers resolve).

    use super::*;

    #[test]
    fn record_accumulates_across_runs_per_model() {
        let tmp = tempfile::TempDir::new().unwrap();
        // Parent dir does not exist yet — the first record must `mkdir -p`.
        let path = tmp.path().join("state/b2/embed-stats.json");

        record_to(&path, "m/base", 1000, 40).unwrap();
        record_to(&path, "m/base", 2500, 60).unwrap();
        record_to(&path, "m/small", 300, 50).unwrap();

        let ledger: BTreeMap<_, _> = read_from(&path).models;
        let base = &ledger["m/base"];
        assert_eq!(base.total_ms, 3500, "two runs' ms sum");
        assert_eq!(base.chunks, 100);
        assert_eq!(base.runs, 2);
        // A different model accumulates in its own bucket, untouched by base.
        let small = &ledger["m/small"];
        assert_eq!(small.total_ms, 300);
        assert_eq!(small.runs, 1);
    }

    #[test]
    fn missing_or_corrupt_file_reads_as_empty() {
        let tmp = tempfile::TempDir::new().unwrap();
        // No file at all.
        assert!(read_from(&tmp.path().join("absent.json")).models.is_empty());
        // A garbage file is treated as no history, never an error.
        let bad = tmp.path().join("bad.json");
        std::fs::write(&bad, "not json {{{").unwrap();
        assert!(read_from(&bad).models.is_empty());
    }
}
