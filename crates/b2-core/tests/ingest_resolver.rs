//! Step 1 — ingest into `notes`/`note_aliases` and the `b2id ⇄ path` resolver.
//!
//! Green-scenario assertions for build-plan step 1
//! (planning/specs/index-engine-build.md §4): ingest the golden vault, resolve
//! `memory ⇄ path` both ways, and prove a note missing a `b2id` is stamped on
//! disk *and* logged (via the event-sink seam; the durable JSONL log is step 4).

use b2_core::event::{Event, EventSink};
use b2_core::id::IdGen;
use b2_core::ingest::ingest_vault;
use b2_core::{db, open};
use std::fs;
use std::path::Path;
use std::sync::Mutex;

/// Test double for the step-4 event log — just collects what was appended.
struct CollectingSink(Mutex<Vec<Event>>);
impl EventSink for CollectingSink {
    fn append(&self, event: Event) {
        self.0.lock().unwrap().push(event);
    }
}
impl CollectingSink {
    fn new() -> Self {
        Self(Mutex::new(Vec::new()))
    }
    fn events(&self) -> Vec<Event> {
        self.0.lock().unwrap().clone()
    }
}

/// Deterministic id generator so stamping is assertable byte-for-byte.
struct FixedId(&'static str);
impl IdGen for FixedId {
    fn new_id(&self) -> String {
        self.0.to_string()
    }
}

fn copy_dir(src: &Path, dst: &Path) {
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

/// Copy the committed golden vault into a temp dir so ingest (which may write a
/// stamp) never mutates the repo fixtures.
fn golden_vault_copy(dst: &Path) {
    let src = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/golden-vault");
    copy_dir(&src, dst);
}

#[test]
fn ingests_golden_vault_and_resolves_b2id_path_both_ways() {
    let tmp = tempfile::TempDir::new().unwrap();
    let vault = tmp.path().join("vault");
    golden_vault_copy(&vault);

    let conn = open(&tmp.path().join("b2.sqlite")).unwrap();
    let sink = CollectingSink::new();
    let idgen = FixedId("01JSHOULDNEVERBEUSED000000");

    ingest_vault(&conn, &vault, &idgen, &sink).unwrap();

    // resolver, both directions, for concepts/memory.md
    let b2id = db::resolve_path_to_b2id(&conn, "concepts/memory.md")
        .unwrap()
        .expect("memory note should resolve");
    assert_eq!(b2id, "01JMEM0000000000000000000A");
    let path = db::resolve_b2id_to_path(&conn, &b2id)
        .unwrap()
        .expect("b2id should resolve back to a path");
    assert_eq!(path, "concepts/memory.md");

    // both golden notes landed, and neither needed stamping
    let count: i64 = conn
        .query_row("SELECT COUNT(*) FROM notes", [], |r| r.get(0))
        .unwrap();
    assert_eq!(count, 2);
    assert!(
        sink.events().is_empty(),
        "golden notes already carry a b2id — nothing to stamp"
    );
}

#[test]
fn stamps_and_logs_b2id_for_a_note_missing_one_and_persists_it_to_disk() {
    let tmp = tempfile::TempDir::new().unwrap();
    let vault = tmp.path().join("vault");
    fs::create_dir_all(&vault).unwrap();
    let note_path = vault.join("orphan.md");
    fs::write(
        &note_path,
        "---\ntype: note\ntitle: \"Orphan\"\n---\nNo id here.\n",
    )
    .unwrap();

    let conn = open(&tmp.path().join("b2.sqlite")).unwrap();
    let sink = CollectingSink::new();
    let idgen = FixedId("01JSTAMPED0000000000000000");

    ingest_vault(&conn, &vault, &idgen, &sink).unwrap();

    // the always-allowed write actually hit the file
    let on_disk = fs::read_to_string(&note_path).unwrap();
    assert_eq!(
        on_disk,
        "---\nb2id: 01JSTAMPED0000000000000000\ntype: note\ntitle: \"Orphan\"\n---\nNo id here.\n"
    );

    // and it was logged through the sink seam
    assert_eq!(
        sink.events(),
        vec![Event::B2idStamped {
            b2id: "01JSTAMPED0000000000000000".to_string(),
            path: "orphan.md".to_string(),
        }]
    );

    // the freshly stamped note resolves
    assert_eq!(
        db::resolve_path_to_b2id(&conn, "orphan.md")
            .unwrap()
            .as_deref(),
        Some("01JSTAMPED0000000000000000")
    );
}

#[test]
fn aliases_are_projected_and_searchable() {
    let tmp = tempfile::TempDir::new().unwrap();
    let vault = tmp.path().join("vault");
    fs::create_dir_all(&vault).unwrap();
    fs::write(
        vault.join("srs.md"),
        "---\nb2id: 01JALIAS00000000000000000A\ntype: concept\ntitle: \"Spaced repetition\"\naliases: [SRS, spacing-effect]\n---\nBody.\n",
    )
    .unwrap();

    let conn = open(&tmp.path().join("b2.sqlite")).unwrap();
    ingest_vault(
        &conn,
        &vault,
        &b2_core::id::UlidGen,
        &b2_core::event::NullSink,
    )
    .unwrap();

    let alias_hit: String = conn
        .query_row(
            "SELECT note_b2id FROM note_aliases WHERE alias = 'SRS'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(alias_hit, "01JALIAS00000000000000000A");
}
