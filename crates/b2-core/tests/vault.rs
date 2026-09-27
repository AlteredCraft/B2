//! The `Vault` façade's core reads: `open`, `reindex`, `neighbors` and `search`, resolving
//! a note by its path with or without the `.md`.

mod common;

use b2_core::vault::Vault;
use b2_core::Error;
use common::{golden_vault_copy, opened_vault, reindexed_vault, MEMORY_PATH, SRS_PATH};
use std::fs;

#[test]
fn open_creates_the_b2_dir_and_index() {
    let tmp = tempfile::TempDir::new().unwrap();
    let root = tmp.path().join("vault");
    golden_vault_copy(&root);

    let _vault = Vault::open(&root).unwrap();

    assert!(root.join(".b2").is_dir(), ".b2/ must exist");
    assert!(root.join(".b2/b2.sqlite").is_file(), "index must exist");
}

#[test]
fn reindex_reports_counts_and_is_idempotent() {
    let tmp = tempfile::TempDir::new().unwrap();
    let (vault, root) = opened_vault(tmp.path());

    let before: Vec<String> = ["concepts/memory.md", "notes/spaced-repetition.md"]
        .iter()
        .map(|p| fs::read_to_string(root.join(p)).unwrap())
        .collect();

    let report = vault.reindex().unwrap();
    assert_eq!(report.indexed, 2, "golden vault has two notes");
    assert_eq!(report.embedded, 2, "both are fresh to the index");

    // Unchanged chunks hash to vectors already stored (M4).
    let again = vault.reindex().unwrap();
    assert_eq!(again.indexed, 2);
    assert_eq!(again.embedded, 0);

    // W1: indexing writes nothing to the vault.
    for (path, was) in ["concepts/memory.md", "notes/spaced-repetition.md"]
        .iter()
        .zip(&before)
    {
        assert_eq!(
            &fs::read_to_string(root.join(path)).unwrap(),
            was,
            "{path} must be byte-identical — a reindex reads, it does not write"
        );
    }
}

#[test]
fn neighbors_of_memory_are_inbound_resolved_to_paths_and_titles() {
    let tmp = tempfile::TempDir::new().unwrap();
    let (vault, _root) = reindexed_vault(tmp.path());

    let ns = vault.neighbors(MEMORY_PATH).unwrap();
    let mut labels: Vec<&str> = ns.iter().map(|n| n.label.as_str()).collect();
    labels.sort_unstable();
    assert_eq!(labels, vec!["referenced-by", "supported-by"]);

    // Title is the filename (data-model.md §1).
    assert!(ns.iter().all(|n| n.path == SRS_PATH));
    assert!(ns.iter().all(|n| n.direction == "inbound"));
    assert!(ns.iter().all(|n| n.path == "notes/spaced-repetition.md"));
    assert!(ns
        .iter()
        .all(|n| n.title.as_deref() == Some("spaced-repetition")));
    assert!(ns.iter().any(|n| n.relation == "supports"
        && n.explanation.as_deref() == Some("applies the forgetting curve")));
}

#[test]
fn neighbors_of_srs_are_outbound_and_ref_forms_agree() {
    let tmp = tempfile::TempDir::new().unwrap();
    let (vault, _root) = reindexed_vault(tmp.path());

    let by_path = vault.neighbors("notes/spaced-repetition.md").unwrap();
    let by_stem = vault.neighbors("notes/spaced-repetition").unwrap();

    for ns in [&by_path, &by_stem] {
        let mut labels: Vec<&str> = ns.iter().map(|n| n.label.as_str()).collect();
        labels.sort_unstable();
        // Outbound labels are the verbs themselves.
        assert_eq!(labels, vec!["references", "supports"]);
        assert!(ns.iter().all(|n| n.path == MEMORY_PATH));
        assert!(ns.iter().all(|n| n.direction == "outbound"));
        assert!(ns.iter().all(|n| n.path == "concepts/memory.md"));
        assert!(ns.iter().all(|n| n.title.as_deref() == Some("memory")));
    }
    assert_eq!(by_path.len(), by_stem.len());
}

/// Every resolving op refuses an unknown ref the same way, echoing it back: the one
/// refusal adapters map to "not found".
#[test]
fn unknown_ref_is_note_not_found_on_every_resolving_op() {
    let tmp = tempfile::TempDir::new().unwrap();
    let (vault, _root) = reindexed_vault(tmp.path());

    const MISSING: &str = "does/not/exist";
    let refusals = [
        ("read", vault.read(MISSING).err()),
        ("neighbors", vault.neighbors(MISSING).err()),
        ("explain", vault.explain(MISSING).err()),
        ("similar", vault.similar(MISSING, 5).err()),
    ];
    for (op, err) in refusals {
        assert!(
            matches!(err, Some(Error::NoteNotFound(ref r)) if r == MISSING),
            "{op} must refuse an unknown ref as NoteNotFound, got {err:?}"
        );
    }
}

#[test]
fn search_finds_the_note_with_a_snippet_and_is_note_level() {
    let tmp = tempfile::TempDir::new().unwrap();
    let (vault, _root) = reindexed_vault(tmp.path());

    let hits = vault.search("forgetting", 10).unwrap();
    assert!(!hits.is_empty());

    // 'forgetting' lives only in spaced-repetition.
    let srs = hits
        .iter()
        .find(|h| h.path == SRS_PATH)
        .expect("SRS must be a hit for 'forgetting'");
    assert_eq!(srs.path, "notes/spaced-repetition.md");
    assert_eq!(srs.title.as_deref(), Some("spaced-repetition"));
    assert!(srs.snippet.contains("forgetting"));
    assert!(srs.score > 0.0);

    let mut ids: Vec<&str> = hits.iter().map(|h| h.path.as_str()).collect();
    ids.sort_unstable();
    let deduped = {
        let mut v = ids.clone();
        v.dedup();
        v
    };
    assert_eq!(ids, deduped, "search results must be deduped by note");
}

/// Before the first reindex, reads answer empty, never an error.
#[test]
fn reads_before_reindex_are_empty_not_errors() {
    let tmp = tempfile::TempDir::new().unwrap();
    let (vault, _) = opened_vault(tmp.path());

    assert!(vault.search("forgetting", 10).unwrap().is_empty());
    assert!(vault.list_notes().unwrap().is_empty());
}
