//! Shared helpers for the integration tests (golden-vault fixtures).
//!
//! Every test binary that says `mod common;` gets this whole file, so it is
//! deliberately small and dependency-light: fixture setup and the two or three
//! read-back shims almost every file needs. Anything that only one file wants
//! (a purpose-built vault, a bespoke row snapshot) stays in that file.
//!
//! One thing that is **not** here on purpose: the tracing `MakeWriter` capture
//! `tests/logging.rs` and `tests/discover_query_count.rs` each define. Hoisting it
//! would make all ~28 test binaries link `tracing-subscriber` to serve two, and
//! those two are already documented as needing their own binary (tracing's global
//! callsite-interest cache races across parallel test threads). The 20 duplicated
//! lines are the price of that isolation.
#![allow(dead_code)]

use b2_core::embed::FakeEmbedder;
use b2_core::ingest::ingest_vault;
use b2_core::open;
use b2_core::vault::Vault;
use rusqlite::Connection;
use std::fs;
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
