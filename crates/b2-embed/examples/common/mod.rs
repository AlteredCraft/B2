//! Helpers the eval-harness examples share (`eval`, `calibrate`, `stability`, and
//! `b2-llm`'s `groundedness`, which pulls this file in by `#[path]`).
//!
//! Each example uses a different subset, hence the `dead_code` allowance.
//!
//! Engine maths restated here stays restated: calling the engine would make each
//! cross-check compare a number with itself. Import nothing beyond `b2-llm`'s
//! dev-dependencies (b2-core, b2-embed, serde_json, tempfile).
#![allow(dead_code)]

use b2_core::vault::{NoteSummary, SearchEvidenceView};
use b2_embed::{provision, EmbedConfig, LocalEmbedder};
use std::error::Error;
use std::path::{Path, PathBuf};

/// The strength-band z landmarks the desktop paints (`ui/src/strength.ts`, GH #182),
/// restated because they are UI copy; `make eval`'s calibration block re-measures them.
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

/// Z-score squared distances, nearer = higher: a restatement of `discover::candidates`
/// (GH #192) to cross-check the engine. `None` under two values or at zero variance; a
/// caller with a larger minimum population applies it first.
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

/// A `similar` score is negated L2 distance between normalized vectors, so
/// `cos = 1 − d²/2` exactly. Piles are recorded in cosine, which compares across models.
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

/// A note's title as a search query: frontmatter title, else the file slug with `-`/`_` as
/// spaces. `None` when blank. Titles are the label-free positives the search benches share
/// (GH #201).
pub fn title_query(note: &NoteSummary) -> Option<String> {
    let title = note.title.clone().unwrap_or_else(|| {
        Path::new(&note.path)
            .file_stem()
            .map(|s| s.to_string_lossy().replace(['-', '_'], " "))
            .unwrap_or_default()
    });
    (!title.trim().is_empty()).then_some(title)
}

/// The share of a query's term IDF the vault carries, from the engine's own per-term
/// weights. `None` when no term has weight (the lexical half abstaining, not scoring zero).
/// `make eval`'s bake-off keeps the one independent restatement, as a drift check.
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

/// A throwaway vault copying the regular files of one flat corpus directory. The temp dir
/// lives as long as this value, so declare the `Vault` opened on [`Self::root`] after it.
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

/// Load the configured model, provisioning only when it won't load. Loading first avoids
/// paying for two loads per warm run, since `provision`'s fast path is itself a full load.
pub fn load_or_provision(config: &EmbedConfig) -> Result<LocalEmbedder, Box<dyn Error>> {
    if let Ok(embedder) = LocalEmbedder::load(config) {
        return Ok(embedder);
    }
    provision(config, |line| eprintln!("[init] {line}"))?;
    Ok(LocalEmbedder::load(config)?)
}

/// Whether `name` was passed as a bare flag.
pub fn has_flag(args: &[String], name: &str) -> bool {
    args.iter().any(|a| a == name)
}

/// Refuse unknown flags and positionals, so a typo can't run a different measurement.
/// `valued` lists the flags that take the next argument as a value.
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
        // `--sweep=1` would match no `has_flag`, so a switch takes no value.
        if inline_value && !valued.contains(&name) {
            return Err(format!("flag {name} takes no value (got {arg:?})").into());
        }
        skip_next = !inline_value && valued.contains(&name);
    }
    Ok(())
}

/// Append one row to a results log, creating it on first run.
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

/// `s` cut to at most `max` chars with an ellipsis. Counts chars, not bytes, so it never
/// splits a codepoint.
pub fn truncate(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        s.to_string()
    } else {
        let cut: String = s.chars().take(max.saturating_sub(1)).collect();
        format!("{cut}…")
    }
}
