//! Helpers the eval-harness examples share (`eval`, `calibrate`, `stability`, and
//! `b2-llm`'s `groundedness`, which pulls this file in by `#[path]`).
//!
//! Cargo discovers examples as `examples/*.rs` and `examples/*/main.rs`, so this
//! directory is a module, never an example of its own. Every example that says
//! `mod common;` compiles the whole file and uses a different subset of it, hence
//! the `dead_code` allowance — the same posture as `b2-core`'s `tests/common`.
//!
//! What lives here is plumbing and the harness's **own** restatements of engine
//! maths. The restatements stay restatements on purpose: they exist so an
//! instrument can cross-check the engine, and a call into the engine would turn
//! each check into a comparison of a number with itself. Nothing here may import
//! beyond what `b2-llm`'s dev-dependencies also carry (b2-core, b2-embed,
//! serde_json, tempfile).
#![allow(dead_code)]

use b2_core::vault::{NoteSummary, SearchEvidenceView};
use std::error::Error;
use std::path::{Path, PathBuf};

/// The strength-band landmarks the desktop paints (`ui/src/strength.ts`, GH #182):
/// `●●●` at or above the labelled-mate population's upper quartile, `●●○` at or
/// above the retired leader bar. Restated constants, not imports — the bands are
/// UI copy, and `make eval`'s calibration block is the instrument their values are
/// re-measured by (and what a loner anchor's always-served cards *claim*, GH #197's
/// A2 readout).
pub const BAND_STRONG_Z: f64 = 2.52;
pub const BAND_CLEAR_Z: f64 = 1.96;

/// The strength band a z paints — see [`BAND_STRONG_Z`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Band {
    Strong,
    Clear,
    Near,
}

impl Band {
    /// The band `z` falls in. A non-finite z compares false everywhere and reads
    /// as [`Band::Near`], the band that claims least.
    pub fn of(z: f64) -> Self {
        if z >= BAND_STRONG_Z {
            Band::Strong
        } else if z >= BAND_CLEAR_Z {
            Band::Clear
        } else {
            Band::Near
        }
    }

    pub fn glyph(self) -> &'static str {
        match self {
            Band::Strong => "●●●",
            Band::Clear => "●●○",
            Band::Near => "●○○",
        }
    }
}

/// Z-score a population of squared distances, oriented nearer = higher — the
/// harness's own restatement of the arithmetic `discover::candidates` applies to
/// the stage-2 best-pair distances (GH #192), kept so the engine's z can be
/// cross-checked rather than merely trusted. `None` when no meaningful statistic
/// exists (under two values, or zero variance), mirroring the engine's own
/// inertness guard; a caller replaying a larger minimum population applies it
/// before calling.
pub fn passage_z(d2: &[f64]) -> Option<Vec<f64>> {
    if d2.len() < 2 {
        return None;
    }
    let n = d2.len() as f64;
    let mean = d2.iter().sum::<f64>() / n;
    let var = d2.iter().map(|d| (d - mean).powi(2)).sum::<f64>() / (n - 1.0);
    let sd = var.sqrt();
    (sd > 0.0).then(|| d2.iter().map(|d| (mean - d) / sd).collect())
}

/// A `similar` score is negated L2 distance between L2-normalized vectors (the
/// real embedder normalizes every row), so it converts exactly: `cos = 1 − d²/2`.
/// Cosine is the unit the floor ruling is stated in and the unit that survives a
/// model swap comparison, so the piles are recorded in it.
pub fn cosine_of(score: f64) -> f64 {
    1.0 - (score * score) / 2.0
}

/// (min, median, max) of a pile, or `None` while it's empty.
pub fn pile_stats(pile: &[f64]) -> Option<(f64, f64, f64)> {
    if pile.is_empty() {
        return None;
    }
    let mut sorted = pile.to_vec();
    sorted.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let mid = sorted.len() / 2;
    let median = if sorted.len().is_multiple_of(2) {
        (sorted[mid - 1] + sorted[mid]) / 2.0
    } else {
        sorted[mid]
    };
    Some((sorted[0], median, sorted[sorted.len() - 1]))
}

/// A note's title as a search query: its frontmatter title, else its file slug with
/// `-`/`_` read as spaces (`drone-comb` → "drone comb"). `None` when that is blank.
///
/// Titles are the label-free positives both search benches share (GH #201): a
/// note's own title names material the vault demonstrably holds, so nothing read
/// off one can be relabelled to clear it.
pub fn title_query(note: &NoteSummary) -> Option<String> {
    let title = note.title.clone().unwrap_or_else(|| {
        Path::new(&note.path)
            .file_stem()
            .map(|s| s.to_string_lossy().replace(['-', '_'], " "))
            .unwrap_or_default()
    });
    (!title.trim().is_empty()).then_some(title)
}

/// The share of a query's term IDF the vault carries, read off the view's own
/// per-term weights ([`b2_core::vault::QueryTermView::idf`]) rather than a second
/// copy of the formula. `None` when no term carries any weight (the lexical half
/// abstaining, not scoring zero).
///
/// `make eval`'s labelled bake-off keeps its own restatement of this arithmetic
/// on purpose, as a drift check against the engine; one such check is the check,
/// so every other bench reads the engine's weights through here.
pub fn term_coverage(view: &SearchEvidenceView) -> Option<f64> {
    let total: f64 = view.terms.iter().map(|t| t.idf).sum();
    (total > f64::EPSILON).then(|| {
        view.terms
            .iter()
            .filter(|t| t.df >= 1)
            .map(|t| t.idf)
            .fold(0.0, |a, b| a + b)
            / total
    })
}

/// A throwaway vault holding a copy of one **flat** corpus directory — the eval
/// corpora are single-level, so only regular files are copied: `fs::copy` errors
/// on a directory, and a future subfolder (or any stray non-file) must not abort
/// the run. The temp dir lives exactly as long as this value, so declare the
/// `Vault` opened on [`Self::root`] after it (locals drop in reverse order).
pub struct ScratchVault {
    _tmp: tempfile::TempDir,
    root: PathBuf,
}

impl ScratchVault {
    pub fn copy_flat(corpus_dir: &Path) -> std::io::Result<Self> {
        let tmp = tempfile::TempDir::new()?;
        let root = tmp.path().join("vault");
        std::fs::create_dir_all(&root)?;
        for entry in std::fs::read_dir(corpus_dir)? {
            let entry = entry?;
            if entry.file_type()?.is_file() {
                std::fs::copy(entry.path(), root.join(entry.file_name()))?;
            }
        }
        Ok(Self { _tmp: tmp, root })
    }

    pub fn root(&self) -> &Path {
        &self.root
    }
}

/// Whether `name` was passed as a bare flag.
pub fn has_flag(args: &[String], name: &str) -> bool {
    args.iter().any(|a| a == name)
}

/// A typo'd flag must not run a *different* measurement than the one asked for.
///
/// `known` lists every accepted flag; `valued` the ones among them that take a
/// value as the next argument (`--vault path`; the `--vault=path` form needs no
/// entry). Positional arguments are refused too — every flag-driven example here
/// takes none.
pub fn reject_unknown_flags(
    args: &[String],
    known: &[&str],
    valued: &[&str],
) -> Result<(), Box<dyn Error>> {
    let mut skip_next = false;
    for arg in args {
        if skip_next {
            skip_next = false;
            continue;
        }
        if !arg.starts_with("--") {
            return Err(format!("unexpected argument {arg:?}").into());
        }
        let (name, inline_value) = match arg.split_once('=') {
            Some((name, _)) => (name, true),
            None => (arg.as_str(), false),
        };
        if !known.contains(&name) {
            return Err(format!("unknown flag {arg:?}; known: {}", known.join(" ")).into());
        }
        // `--sweep=1` would pass the name check and then match no `has_flag` —
        // the same silent wrong measurement as a typo, so a switch takes no value.
        if inline_value && !valued.contains(&name) {
            return Err(format!("flag {name} takes no value (got {arg:?})").into());
        }
        skip_next = !inline_value && valued.contains(&name);
    }
    Ok(())
}

/// Append one row to a results log (creating it on first run). Append-only, so
/// runs accumulate into one dataset — the same convention as `B2_LOG_FILE`.
pub fn append_result(path: &Path, row: &serde_json::Value) -> Result<(), Box<dyn Error>> {
    use std::io::Write;
    let mut f = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)?;
    writeln!(f, "{row}")?;
    Ok(())
}

/// The repo's short commit hash, best-effort (`None` outside a git checkout).
pub fn git_short_sha() -> Option<String> {
    let out = std::process::Command::new("git")
        .args(["rev-parse", "--short", "HEAD"])
        .current_dir(env!("CARGO_MANIFEST_DIR"))
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    Some(String::from_utf8_lossy(&out.stdout).trim().to_string())
}

/// `s` cut to at most `max` chars, an ellipsis marking the cut. Counted in chars
/// and cut on a char boundary (byte-index arithmetic lands mid-codepoint on text
/// with em dashes or °C), and saturating, so `max == 0` yields a bare ellipsis
/// rather than an underflow panic.
pub fn truncate(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        s.to_string()
    } else {
        let cut: String = s.chars().take(max.saturating_sub(1)).collect();
        format!("{cut}…")
    }
}
