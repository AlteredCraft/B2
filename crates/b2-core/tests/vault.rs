//! The `Vault` façade — the one typed core API the CLI and tests are clients of
//! (invariants.md). This slice's contract:
//! `open` / `reindex` / `neighbors` / `search`, resolving a note by path **or**
//! `b2id`, against the golden-vault fixture. Fully deterministic (FakeEmbedder),
//! so it proves the plumbing, not model quality.

mod common;

use b2_core::vault::Vault;
use b2_core::Error;
use common::{golden_vault_copy, reindexed_vault, MEMORY_ID, SRS_ID};

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
fn neighbors_of_memory_are_inbound_resolved_to_paths_and_titles() {
    let tmp = tempfile::TempDir::new().unwrap();
    let (vault, _root) = reindexed_vault(tmp.path());

    let ns = vault.neighbors(MEMORY_ID).unwrap();
    let mut labels: Vec<&str> = ns.iter().map(|n| n.label.as_str()).collect();
    labels.sort_unstable();
    assert_eq!(labels, vec!["referenced-by", "supported-by"]);

    // every neighbor is the SRS note, inbound, resolved to its path + title (the
    // filename, data-model.md §1).
    assert!(ns.iter().all(|n| n.b2id == SRS_ID));
    assert!(ns.iter().all(|n| n.direction == "inbound"));
    assert!(ns.iter().all(|n| n.path == "notes/spaced-repetition.md"));
    assert!(ns
        .iter()
        .all(|n| n.title.as_deref() == Some("spaced-repetition")));
    // the typed `supports` edge carries its explanation through.
    assert!(ns.iter().any(|n| n.relation == "supports"
        && n.explanation.as_deref() == Some("applies the forgetting curve")));
}

#[test]
fn neighbors_of_srs_are_outbound_and_ref_forms_agree() {
    let tmp = tempfile::TempDir::new().unwrap();
    let (vault, _root) = reindexed_vault(tmp.path());

    // by path, by path-without-.md, and by b2id must all resolve to the same set.
    let by_path = vault.neighbors("notes/spaced-repetition.md").unwrap();
    let by_stem = vault.neighbors("notes/spaced-repetition").unwrap();
    let by_id = vault.neighbors(SRS_ID).unwrap();

    for ns in [&by_path, &by_stem, &by_id] {
        let mut labels: Vec<&str> = ns.iter().map(|n| n.label.as_str()).collect();
        labels.sort_unstable();
        // outbound labels are the verbs themselves.
        assert_eq!(labels, vec!["references", "supports"]);
        assert!(ns.iter().all(|n| n.b2id == MEMORY_ID));
        assert!(ns.iter().all(|n| n.direction == "outbound"));
        assert!(ns.iter().all(|n| n.path == "concepts/memory.md"));
        assert!(ns.iter().all(|n| n.title.as_deref() == Some("memory")));
    }
    assert_eq!(by_path.len(), by_id.len());
    assert_eq!(by_stem.len(), by_id.len());
}

/// Every façade op that resolves a note ref rejects an unknown one the same way,
/// echoing the ref back verbatim — the single refusal the adapters map to their
/// "not found" message. Asserted once here rather than per-op across files.
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

    // 'forgetting' lives only in spaced-repetition — it must surface, resolved to
    // its note with a non-empty snippet showing the matched term.
    let srs = hits
        .iter()
        .find(|h| h.b2id == SRS_ID)
        .expect("SRS must be a hit for 'forgetting'");
    assert_eq!(srs.path, "notes/spaced-repetition.md");
    assert_eq!(srs.title.as_deref(), Some("spaced-repetition"));
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

/// Index-first honesty: before the first reindex the projection is empty, so the
/// read surfaces answer *empty*, never an error — the adapters render "nothing
/// indexed yet", not a failure.
#[test]
fn reads_before_reindex_are_empty_not_errors() {
    let tmp = tempfile::TempDir::new().unwrap();
    let root = tmp.path().join("vault");
    golden_vault_copy(&root);
    let vault = Vault::open(&root).unwrap();

    assert!(vault.search("forgetting", 10).unwrap().is_empty());
    assert!(vault.list_notes().unwrap().is_empty());
}
