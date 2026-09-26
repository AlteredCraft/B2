//! Shared helpers for the integration tests (golden-vault fixtures).
//!
//! Every test binary that says `mod common;` gets this whole file, so it is deliberately
//! small and dependency-light. Anything only one file wants stays in that file — including,
//! on purpose, the tracing `MakeWriter` capture `tests/logging.rs` and
//! `tests/discover_query_count.rs` each define: hoisting it would make all ~28 test binaries
//! link `tracing-subscriber` to serve two that already need their own binary anyway.
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

/// Vault-relative paths of the two golden-vault notes (data-model.md §8) — which
/// **are** their identities (L1, GH #170), so these constants are what the suite
/// asserts against. The `MEMORY_ID`/`SRS_ID` ULIDs they replaced, and the
/// `FixedId`/`SeqId` generators that made a stamped id assertable, went with the
/// stamp: the core mints nothing, so there is no injected id seam left to fake.
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

/// Copy the committed golden vault into `dst`, so no test can mutate the repo
/// fixtures. Ingest no longer writes to a vault at all (W1), so this is now belt
/// and braces rather than the load-bearing guard it was — kept because a test that
/// edits a committed fixture is a bad idea under any write posture, and CI's
/// `git diff --exit-code` step would fail on one regardless.
pub fn golden_vault_copy(dst: &Path) {
    let src = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/golden-vault");
    copy_dir(&src, dst);
}

// --- façade fixtures -------------------------------------------------------------

/// A golden-vault copy under `dir/vault`, opened but **not** reindexed — the
/// index-free starting point (structure reads, "before the first reindex" cases).
/// Returns `(vault, vault_root)`.
pub fn opened_vault(dir: &Path) -> (Vault, PathBuf) {
    let root = dir.join("vault");
    golden_vault_copy(&root);
    let vault = Vault::open(&root).unwrap();
    (vault, root)
}

/// A golden-vault copy under `dir/vault`, opened and fully reindexed (projected +
/// fake-embedded) — the ordinary starting point for façade tests. Returns
/// `(vault, vault_root)`.
pub fn reindexed_vault(dir: &Path) -> (Vault, PathBuf) {
    let (vault, root) = opened_vault(dir);
    vault.reindex().unwrap();
    (vault, root)
}

// --- index read-back -------------------------------------------------------------

/// A second connection onto a vault root's index, for assertions the façade does
/// not surface (raw rows, table counts).
pub fn index_conn(root: &Path) -> Connection {
    open(&root.join(".b2").join("b2.sqlite")).unwrap()
}

/// Row count of `table`.
pub fn count(conn: &Connection, table: &str) -> i64 {
    conn.query_row(&format!("SELECT COUNT(*) FROM {table}"), [], |r| r.get(0))
        .unwrap()
}

/// A note's inbound set as sortable `(label, src_path)` pairs — the shape the graph
/// exposes, and the thing a move must carry to the destination intact.
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

/// A token callback that keeps streaming and discards every token — the plain,
/// uncancelled run.
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

/// Ingest the golden vault into a standalone `dir/b2.sqlite`, for the module-level
/// tests that drive `ingest_vault` directly instead of going through the façade.
/// The embedder is explicit because the dimension is load-bearing in some suites.
pub fn ingest_golden(dir: &Path, embedder: &FakeEmbedder) -> Connection {
    let vault = dir.join("vault");
    golden_vault_copy(&vault);
    let conn = open(&dir.join("b2.sqlite")).unwrap();
    ingest_vault(&conn, &vault, embedder).unwrap();
    conn
}

// --- a geometric embedder, for the suites that need real distances --------------
//
// The fake embedder's hash vectors have no geometry, so discovery's statistics can't be
// exercised on them. These hand-placed vectors can. Shared by `discover_surfacing.rs` and
// `explain_similar.rs`.

/// Hand-placed unit vectors keyed by a `VEC:<tag>` marker in the chunk text.
/// Dim 4: axis 0 is the "anchor topic", axis 2 the "noise topic"; small designed
/// offsets on axes 1/3 give the noise cloud a nonzero, controlled variance.
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
            // The buried-gem pair (see `a_buried_gem_outranks_and_is_served`):
            // `MID` is one middling chunk, nearer the anchor than a split note's
            // *centroid* but further than that note's best *chunk* (`NEAR`, whose
            // other half `FAR` drags the centroid away).
            "MID" => vec![0.8, 0.6, 0.0, 0.0],
            "NEAR" => vec![0.995, 0.0998, 0.0, 0.0],
            "FAR" => vec![0.0, 0.0, 1.0, 0.0],
            // The noise cloud: all near axis 2, fanned evenly on axis 3 so the
            // diffuse anchor sees one smooth spread of distances (max-z ≈ 1.6
            // for 13 evenly spaced values — under the retired leader gate by
            // construction, which is what made that anchor's pane dark).
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

/// A note long enough to chunk in two, each half carrying its own `VEC:` tag.
/// ~1400 chars per half against the 450-token (≈1800-char) target: short enough
/// that the two halves don't make a third chunk, long enough that the H2 between
/// them is inside the backscan and becomes the boundary. The 15% overlap re-shares
/// only filler, leaving each chunk's *first* `VEC:` its own.
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
