//! The FTS tokenizer (GH #157): `porter unicode61` is the default, and
//! [`Vault::rebuild_fts`] swaps it over the same chunk text, so the eval can measure the
//! unstemmed ablation without re-embedding. BM25-only here, so every match is the
//! tokenizer's.

use b2_core::db::FtsTokenizer;
use b2_core::vault::Vault;
use std::fs;
use std::path::Path;

/// One note saying "pedals" and "ascents", projected but not embedded.
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

    assert_eq!(hit_paths(&vault, "pedalling"), vec!["bicycle.md"]);
    assert_eq!(hit_paths(&vault, "ascent"), vec!["bicycle.md"]);
    // Stemming widens; surface forms still match.
    assert_eq!(hit_paths(&vault, "pedals"), vec!["bicycle.md"]);
}

#[test]
fn unicode61_rebuild_restores_literal_only_matching_and_back() {
    let tmp = tempfile::tempdir().unwrap();
    let vault = projected_vault(tmp.path());

    vault.rebuild_fts(FtsTokenizer::Unicode61).unwrap();
    assert!(hit_paths(&vault, "pedalling").is_empty());
    assert_eq!(hit_paths(&vault, "pedals"), vec!["bicycle.md"]);

    // The round trip leaves no residue.
    vault.rebuild_fts(FtsTokenizer::PorterUnicode61).unwrap();
    assert_eq!(hit_paths(&vault, "pedalling"), vec!["bicycle.md"]);
}

#[test]
fn incremental_ingest_stays_in_sync_after_a_rebuild() {
    let tmp = tempfile::tempdir().unwrap();
    let vault = projected_vault(tmp.path());
    vault.rebuild_fts(FtsTokenizer::Unicode61).unwrap();

    // The chunks triggers reference `chunks_fts` by name, and a rebuild recreates it.
    let root = tmp.path().join("vault");
    fs::write(
        root.join("volcano.md"),
        "# Volcano\n\nA volcano erupts when magma reaches the surface.\n",
    )
    .unwrap();
    vault.project(false).unwrap();

    assert_eq!(hit_paths(&vault, "magma"), vec!["volcano.md"]);
}
