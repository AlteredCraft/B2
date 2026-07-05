//! The `Vault` façade — the one typed core API the CLI and tests are clients of
//! (vision-and-scope testability stack, point 1). This slice's contract:
//! `open` / `reindex` / `neighbors` / `search`, resolving a note by path **or**
//! `b2id`, against the golden-vault fixture. Fully deterministic (FakeEmbedder),
//! so it proves the plumbing, not model quality.

mod common;

use b2_core::vault::Vault;
use b2_core::Error;
use common::{golden_vault_copy, MEMORY_ID, SRS_ID};
use std::path::{Path, PathBuf};

/// A reindexed golden vault under a temp dir; returns (vault, vault_root).
fn reindexed(dir: &Path) -> (Vault, PathBuf) {
    let root = dir.join("vault");
    golden_vault_copy(&root);
    let vault = Vault::open(&root).unwrap();
    vault.reindex().unwrap();
    (vault, root)
}

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
    let root = tmp.path().join("vault");
    golden_vault_copy(&root);
    let vault = Vault::open(&root).unwrap();

    let report = vault.reindex().unwrap();
    assert_eq!(report.indexed, 2, "golden vault has two notes");
    // both golden notes already carry a b2id → nothing is stamped.
    assert_eq!(report.stamped, 0);

    // a second reindex still indexes both and stamps nothing.
    let again = vault.reindex().unwrap();
    assert_eq!(again.indexed, 2);
    assert_eq!(again.stamped, 0);
}

#[test]
fn reindex_stamps_a_note_missing_a_b2id() {
    let tmp = tempfile::TempDir::new().unwrap();
    let root = tmp.path().join("vault");
    golden_vault_copy(&root);
    // an extra note with no b2id → reindex must stamp exactly it.
    std::fs::write(
        root.join("orphan.md"),
        "---\ntype: note\ntitle: Orphan\n---\nbody\n",
    )
    .unwrap();

    let vault = Vault::open(&root).unwrap();
    let report = vault.reindex().unwrap();
    assert_eq!(report.indexed, 3);
    assert_eq!(report.stamped, 1);
    // the stamp is durable in the note's frontmatter (the id travels in the file).
    let stamped = std::fs::read_to_string(root.join("orphan.md")).unwrap();
    assert!(
        stamped.contains("b2id:"),
        "the missing b2id must be written to disk"
    );
}

#[test]
fn neighbors_of_memory_are_inbound_resolved_to_paths_and_titles() {
    let tmp = tempfile::TempDir::new().unwrap();
    let (vault, _root) = reindexed(tmp.path());

    let ns = vault.neighbors(MEMORY_ID).unwrap();
    let mut labels: Vec<&str> = ns.iter().map(|n| n.label.as_str()).collect();
    labels.sort_unstable();
    assert_eq!(labels, vec!["elaborated-by", "referenced-by"]);

    // every neighbor is the SRS note, inbound, resolved to its path + title.
    assert!(ns.iter().all(|n| n.b2id == SRS_ID));
    assert!(ns.iter().all(|n| n.direction == "inbound"));
    assert!(ns.iter().all(|n| n.path == "notes/spaced-repetition.md"));
    assert!(ns
        .iter()
        .all(|n| n.title.as_deref() == Some("Spaced repetition")));
    // the typed `elaborates` edge carries its explanation through.
    assert!(ns.iter().any(|n| n.relation == "elaborates"
        && n.explanation.as_deref() == Some("applies the forgetting curve")));
}

#[test]
fn neighbors_of_srs_are_outbound_and_ref_forms_agree() {
    let tmp = tempfile::TempDir::new().unwrap();
    let (vault, _root) = reindexed(tmp.path());

    // by path, by path-without-.md, and by b2id must all resolve to the same set.
    let by_path = vault.neighbors("notes/spaced-repetition.md").unwrap();
    let by_stem = vault.neighbors("notes/spaced-repetition").unwrap();
    let by_id = vault.neighbors(SRS_ID).unwrap();

    for ns in [&by_path, &by_stem, &by_id] {
        let mut labels: Vec<&str> = ns.iter().map(|n| n.label.as_str()).collect();
        labels.sort_unstable();
        // outbound labels are the verbs themselves.
        assert_eq!(labels, vec!["elaborates", "references"]);
        assert!(ns.iter().all(|n| n.b2id == MEMORY_ID));
        assert!(ns.iter().all(|n| n.direction == "outbound"));
        assert!(ns.iter().all(|n| n.path == "concepts/memory.md"));
        assert!(ns
            .iter()
            .all(|n| n.title.as_deref() == Some("Human memory")));
    }
    assert_eq!(by_path.len(), by_id.len());
    assert_eq!(by_stem.len(), by_id.len());
}

#[test]
fn unknown_ref_is_note_not_found() {
    let tmp = tempfile::TempDir::new().unwrap();
    let (vault, _root) = reindexed(tmp.path());

    let err = vault.neighbors("does/not/exist").unwrap_err();
    assert!(matches!(err, Error::NoteNotFound(r) if r == "does/not/exist"));
}

#[test]
fn search_finds_the_note_with_a_snippet_and_is_note_level() {
    let tmp = tempfile::TempDir::new().unwrap();
    let (vault, _root) = reindexed(tmp.path());

    let hits = vault.search("forgetting", 10).unwrap();
    assert!(!hits.is_empty());

    // 'forgetting' lives only in spaced-repetition — it must surface, resolved to
    // its note with a non-empty snippet showing the matched term.
    let srs = hits
        .iter()
        .find(|h| h.b2id == SRS_ID)
        .expect("SRS must be a hit for 'forgetting'");
    assert_eq!(srs.path, "notes/spaced-repetition.md");
    assert_eq!(srs.title.as_deref(), Some("Spaced repetition"));
    assert!(srs.snippet.contains("forgetting"));
    assert!(srs.score > 0.0);

    // results are note-level: no note appears twice.
    let mut ids: Vec<&str> = hits.iter().map(|h| h.b2id.as_str()).collect();
    ids.sort_unstable();
    let deduped = {
        let mut v = ids.clone();
        v.dedup();
        v
    };
    assert_eq!(ids, deduped, "search results must be deduped by note");
}

#[test]
fn search_before_reindex_is_empty() {
    let tmp = tempfile::TempDir::new().unwrap();
    let root = tmp.path().join("vault");
    golden_vault_copy(&root);
    let vault = Vault::open(&root).unwrap();

    // no reindex → no chunks → no hits (and no error).
    let hits = vault.search("forgetting", 10).unwrap();
    assert!(hits.is_empty());
}
