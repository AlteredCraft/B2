//! Shared helpers for the integration tests (golden-vault fixtures).
//!
//! Every test binary that says `mod common;` compiles this whole file, so it stays small
//! and dependency-light. The tracing capture in `logging.rs` and `discover_query_count.rs`
//! stays there, so the other binaries don't link `tracing-subscriber`.
#![allow(dead_code)]

use b2_core::embed::{Embedder, FakeEmbedder};
use b2_core::ingest::ingest_vault;
use b2_core::open;
use b2_core::vault::Vault;
use b2_core::Result;
use rusqlite::Connection;
use std::fs;
use std::ops::ControlFlow;
use std::path::{Path, PathBuf};

/// The two golden-vault notes (data-model.md §8), by path, which is their identity (L1,
/// GH #170).
pub const MEMORY_PATH: &str = "concepts/memory.md";
pub const SRS_PATH: &str = "notes/spaced-repetition.md";

pub fn copy_dir(src: &Path, dst: &Path) {
    fs::create_dir_all(dst).unwrap();
    for entry in fs::read_dir(src).unwrap() {
        let entry = entry.unwrap();
        let from = entry.path();
        let to = dst.join(entry.file_name());
        if from.is_dir() {
            copy_dir(&from, &to);
        } else {
            fs::copy(&from, &to).unwrap();
        }
    }
}

/// Copy the committed golden vault into `dst`, so no test can mutate the repo fixtures.
pub fn golden_vault_copy(dst: &Path) {
    let src = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/golden-vault");
    copy_dir(&src, dst);
}

// --- façade fixtures -------------------------------------------------------------

/// A golden-vault copy under `dir/vault`, opened but not reindexed.
pub fn opened_vault(dir: &Path) -> (Vault, PathBuf) {
    let root = dir.join("vault");
    golden_vault_copy(&root);
    let vault = Vault::open(&root).unwrap();
    (vault, root)
}

/// A golden-vault copy under `dir/vault`, opened and reindexed with the fake embedder.
pub fn reindexed_vault(dir: &Path) -> (Vault, PathBuf) {
    let (vault, root) = opened_vault(dir);
    vault.reindex().unwrap();
    (vault, root)
}

// --- index read-back -------------------------------------------------------------

/// A second connection onto a vault's index, for rows the façade doesn't surface.
pub fn index_conn(root: &Path) -> Connection {
    open(&root.join(".b2").join("b2.sqlite")).unwrap()
}

/// Row count of `table`.
pub fn count(conn: &Connection, table: &str) -> i64 {
    conn.query_row(&format!("SELECT COUNT(*) FROM {table}"), [], |r| r.get(0))
        .unwrap()
}

/// A note's inbound set as sorted `(label, src_path)` pairs.
pub fn inbound(vault: &Vault, note_ref: &str) -> Vec<(String, String)> {
    let mut ns: Vec<(String, String)> = vault
        .neighbors(note_ref)
        .unwrap()
        .into_iter()
        .filter(|n| n.direction == "inbound")
        .map(|n| (n.label, n.path))
        .collect();
    ns.sort();
    ns
}

// --- vault authoring and chat callbacks ------------------------------------------

/// Write a minimal note at `vault/name` with `body` under a small frontmatter.
pub fn write_note(vault: &Path, name: &str, body: &str) {
    fs::write(
        vault.join(name),
        format!("---\ntype: note\ntitle: {name}\n---\n{body}\n"),
    )
    .unwrap();
}

/// A token callback that keeps streaming and discards every token.
pub fn keep_streaming() -> impl FnMut(&str) -> ControlFlow<()> {
    |_| ControlFlow::Continue(())
}

/// A token callback that keeps streaming and appends every token to `buf`.
pub fn stream_into(buf: &mut String) -> impl FnMut(&str) -> ControlFlow<()> + '_ {
    move |tok: &str| {
        buf.push_str(tok);
        ControlFlow::Continue(())
    }
}

/// Ingest the golden vault into a standalone `dir/b2.sqlite`, bypassing the façade. The
/// embedder is explicit because some suites depend on its dimension.
pub fn ingest_golden(dir: &Path, embedder: &FakeEmbedder) -> Connection {
    let vault = dir.join("vault");
    golden_vault_copy(&vault);
    let conn = open(&dir.join("b2.sqlite")).unwrap();
    ingest_vault(&conn, &vault, embedder).unwrap();
    conn
}

// --- a geometric embedder, for the suites that need real distances --------------
//
// The fake embedder's hash vectors have no geometry, so discovery's statistics need these.

/// Hand-placed unit vectors keyed by a `VEC:<tag>` marker in the chunk text. Axis 0 is
/// the anchor topic, axis 2 the noise topic; offsets on axes 1 and 3 give controlled variance.
pub struct GeometricEmbedder;

impl GeometricEmbedder {
    pub fn vector_for(text: &str) -> Vec<f32> {
        let tag_start = text.find("VEC:").map(|i| i + 4);
        let tag: String = tag_start
            .map(|s| {
                text[s..]
                    .chars()
                    .take_while(|c| c.is_ascii_alphanumeric())
                    .collect()
            })
            .unwrap_or_default();
        let v: Vec<f32> = match tag.as_str() {
            // The anchor and its one true mate: nearly parallel.
            "ANCHOR" => vec![1.0, 0.0, 0.0, 0.0],
            "MATE" => vec![0.995, 0.0998, 0.0, 0.0],
            // A diffuse anchor living inside the noise cloud itself.
            "DIFFUSE" => vec![0.0, 0.0, 1.0, 0.0],
            // The buried gem: `MID` is nearer the anchor than the split note's centroid,
            // but further than its best chunk `NEAR` (`FAR` drags the centroid away).
            "MID" => vec![0.8, 0.6, 0.0, 0.0],
            "NEAR" => vec![0.995, 0.0998, 0.0, 0.0],
            "FAR" => vec![0.0, 0.0, 1.0, 0.0],
            // The noise cloud: near axis 2, fanned evenly on axis 3, so the diffuse
            // anchor sees one smooth spread of distances.
            t if t.starts_with('N') => {
                let i: f32 = t[1..].parse().unwrap_or(0.0);
                vec![0.0, 0.0, 1.0, 0.05 + i * 0.03]
            }
            _ => vec![0.0, 1.0, 0.0, 0.0],
        };
        let norm = v.iter().map(|x| x * x).sum::<f32>().sqrt();
        v.into_iter().map(|x| x / norm).collect()
    }
}

impl Embedder for GeometricEmbedder {
    fn model_id(&self) -> &str {
        "test-geometric-v1"
    }
    fn dim(&self) -> usize {
        4
    }
    fn embed(&self, text: &str) -> Result<Vec<f32>> {
        Ok(Self::vector_for(text))
    }
}

/// One geometric note: a single passage carrying the `VEC:<tag>` marker
/// [`GeometricEmbedder`] places.
pub fn write_geometric_note(vault: &Path, name: &str, tag: &str) {
    fs::write(
        vault.join(name),
        format!("---\ntype: note\ntitle: {name}\n---\ntopic marker VEC:{tag} body.\n"),
    )
    .unwrap();
}

/// A note that chunks in two at its H2, each half with its own `VEC:` tag. ~1400 chars
/// per half against the ~1800-char target puts the H2 inside the backscan; the overlap
/// shares only filler, so each chunk's first `VEC:` is its own.
pub fn write_split_note(vault: &Path, name: &str, first: &str, second: &str) {
    let filler = "alpha beta gamma delta epsilon zeta eta theta iota kappa. ".repeat(24);
    fs::write(
        vault.join(name),
        format!(
            "---\ntype: note\ntitle: {name}\n---\nmarker VEC:{first} {filler}\n\n\
             ## Second section\n\nmarker VEC:{second} {filler}\n"
        ),
    )
    .unwrap();
}
