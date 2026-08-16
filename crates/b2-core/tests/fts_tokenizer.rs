//! The FTS tokenizer (GH #157): `porter unicode61` is the shipped default (schema
//! v5 — the A/B's verdict, GH #157), and
//! [`Vault::rebuild_fts`] swaps `chunks_fts`'s tokenizer over the identical stored
//! chunk text — the lever that lets the eval harness keep the unstemmed ablation
//! measurable without re-chunking or re-embedding anything (the tokenizer only
//! touches the lexical half — index-engine.md §3).
//!
//! These tests run on the BM25-only fallback path (projected, never embedded), so
//! every match below is the tokenizer's alone.

use b2_core::db::FtsTokenizer;
use b2_core::vault::Vault;
use std::fs;
use std::path::Path;

/// A purpose-built vault: one stub note whose body says "pedals" and "ascents" —
/// the surface forms the eval corpus's pre-verdict misses hinged on — projected
/// but deliberately not embedded.
fn projected_vault(dir: &Path) -> Vault {
    let root = dir.join("vault");
    fs::create_dir_all(&root).unwrap();
    fs::write(
        root.join("bicycle.md"),
        "# Bicycle\n\nA bicycle is driven by pedals. Low gearing makes long ascents manageable.\n",
    )
    .unwrap();
    let vault = Vault::open(&root).unwrap();
    vault.project(false).unwrap();
    vault
}

fn hit_paths(vault: &Vault, query: &str) -> Vec<String> {
    vault
        .search(query, 10)
        .unwrap()
        .into_iter()
        .map(|r| r.path)
        .collect()
}

#[test]
fn the_shipped_default_matches_inflected_query_terms() {
    let tmp = tempfile::tempdir().unwrap();
    let vault = projected_vault(tmp.path());

    // The note says "pedals" and "ascents"; a stemmed lexical half resolves every
    // inflection out of the box — the exact queries unstemmed BM25 scored ✗>10 on.
    assert_eq!(hit_paths(&vault, "pedalling"), vec!["bicycle.md"]);
    assert_eq!(hit_paths(&vault, "ascent"), vec!["bicycle.md"]);
    // And the surface forms still match, stemming being a widening, not a swap.
    assert_eq!(hit_paths(&vault, "pedals"), vec!["bicycle.md"]);
}

#[test]
fn unicode61_rebuild_restores_literal_only_matching_and_back() {
    let tmp = tempfile::tempdir().unwrap();
    let vault = projected_vault(tmp.path());

    // The ablation arm: same chunk rows, unstemmed tokenizer — surface forms only.
    vault.rebuild_fts(FtsTokenizer::Unicode61).unwrap();
    assert!(hit_paths(&vault, "pedalling").is_empty());
    assert_eq!(hit_paths(&vault, "pedals"), vec!["bicycle.md"]);

    // Round trip: the lever leaves no residue, so the eval's ablation arm can
    // hand the vault back to the shipped default untainted.
    vault.rebuild_fts(FtsTokenizer::PorterUnicode61).unwrap();
    assert_eq!(hit_paths(&vault, "pedalling"), vec!["bicycle.md"]);
}

#[test]
fn incremental_ingest_stays_in_sync_after_a_rebuild() {
    let tmp = tempfile::tempdir().unwrap();
    let vault = projected_vault(tmp.path());
    vault.rebuild_fts(FtsTokenizer::Unicode61).unwrap();

    // The chunks_ai/ad/au triggers live on `chunks` and reference `chunks_fts` by
    // name; a rebuild recreates the table they point at, so ingest after the swap
    // must keep FTS in sync exactly as before it.
    let root = tmp.path().join("vault");
    fs::write(
        root.join("volcano.md"),
        "# Volcano\n\nA volcano erupts when magma reaches the surface.\n",
    )
    .unwrap();
    vault.project(false).unwrap();

    assert_eq!(hit_paths(&vault, "magma"), vec!["volcano.md"]);
}
