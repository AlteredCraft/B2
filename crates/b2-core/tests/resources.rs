//! Resources slice 1 — inventory & graph
//! (data-model.md §10).
//!
//! Step 0: the v4 schema — the `resources` table exists, `edges` carries the
//! resource-target columns (`dst_resource_path`, `embed`, `caption`), dangling
//! means *neither* target resolved, and the version gate drops a v3 index.

use b2_core::{open, Vault, SCHEMA_VERSION};
use rusqlite::Connection;
use std::fs;
use std::path::Path;

mod common;

/// `(path, class, size, content_hash)` rows, path-ordered — the comparable
/// projection of `resources` (mtime/indexed_at are host state, not projection).
fn resource_rows(root: &Path) -> Vec<(String, String, i64, String)> {
    let conn = open(&root.join(".b2/b2.sqlite")).unwrap();
    let mut stmt = conn
        .prepare("SELECT path, class, size, content_hash FROM resources ORDER BY path")
        .unwrap();
    let rows = stmt
        .query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)))
        .unwrap()
        .map(Result::unwrap)
        .collect::<Vec<_>>();
    rows
}

/// Column names of `table` via `pragma table_info`, for shape assertions.
fn columns(conn: &Connection, table: &str) -> Vec<String> {
    let mut stmt = conn
        .prepare(&format!("SELECT name FROM pragma_table_info('{table}')"))
        .unwrap();
    let cols = stmt
        .query_map([], |r| r.get::<_, String>(0))
        .unwrap()
        .map(Result::unwrap)
        .collect::<Vec<_>>();
    cols
}

#[test]
fn v4_schema_has_resources_table_and_widened_edges() {
    let tmp = tempfile::TempDir::new().unwrap();
    let conn = open(&tmp.path().join("b2.sqlite")).unwrap();

    let resources = columns(&conn, "resources");
    for col in [
        "path",
        "class",
        "size",
        "mtime",
        "content_hash",
        "indexed_at",
    ] {
        assert!(
            resources.iter().any(|c| c == col),
            "resources.{col} missing"
        );
    }

    let edges = columns(&conn, "edges");
    for col in ["dst_resource_path", "embed", "caption"] {
        assert!(edges.iter().any(|c| c == col), "edges.{col} missing");
    }

    // The class vocabulary is closed (research §3): a value outside it must refuse.
    let bad = conn.execute(
        "INSERT INTO resources(path, class, size, content_hash, indexed_at)
         VALUES ('x.xyz', 'mystery', 0, 'h', 'now')",
        [],
    );
    assert!(bad.is_err(), "an unknown class must violate the CHECK");
}

/// The schema-version gate: a v3 index is dropped wholesale and rebuilt at v4 —
/// no migration code, ever (the disposable-index tenet).
#[test]
fn v3_index_is_dropped_and_rebuilt_at_v4() {
    let tmp = tempfile::TempDir::new().unwrap();
    let db_path = tmp.path().join("b2.sqlite");

    {
        let conn = open(&db_path).unwrap();
        conn.execute_batch(
            "INSERT INTO notes(b2id, path, type, body_hash, indexed_at)
               VALUES ('01JX000000000000000000000A', 'a.md', 'note', 'h', 'now');
             INSERT INTO resources(path, class, size, content_hash, indexed_at)
               VALUES ('img.png', 'image', 3, 'h', 'now');",
        )
        .unwrap();
        // Simulate an index built by the previous schema.
        conn.execute(
            "UPDATE meta SET value = '3' WHERE key = 'schema_version'",
            [],
        )
        .unwrap();
    }

    let conn = open(&db_path).unwrap();
    let version: String = conn
        .query_row(
            "SELECT value FROM meta WHERE key = 'schema_version'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(version, SCHEMA_VERSION.to_string());
    let notes: i64 = conn
        .query_row("SELECT count(*) FROM notes", [], |r| r.get(0))
        .unwrap();
    let resources: i64 = conn
        .query_row("SELECT count(*) FROM resources", [], |r| r.get(0))
        .unwrap();
    assert_eq!((notes, resources), (0, 0), "the gate must drop v3 rows");
}

/// A resource-targeted edge is FK-checked, deduped by the partial unique index,
/// and **re-dangles** (dst_resource_path → NULL, dst_path_raw retained) when its
/// target row is pruned — the ON DELETE SET NULL that keeps pruning one statement.
#[test]
fn resource_edges_are_fk_checked_and_redangle_on_prune() {
    let tmp = tempfile::TempDir::new().unwrap();
    let conn = open(&tmp.path().join("b2.sqlite")).unwrap();

    conn.execute_batch(
        "INSERT INTO notes(b2id, path, type, body_hash, indexed_at)
           VALUES ('01JX000000000000000000000A', 'a.md', 'note', 'h', 'now');",
    )
    .unwrap();

    // FK: the target row must exist.
    let orphan = conn.execute(
        "INSERT INTO edges(id, src_id, dst_resource_path, dst_path_raw, type, origin)
         VALUES ('e0', '01JX000000000000000000000A', 'missing.png', 'missing.png',
                 'references', 'inline')",
        [],
    );
    assert!(orphan.is_err(), "dst_resource_path must be FK-enforced");

    conn.execute_batch(
        "INSERT INTO resources(path, class, size, content_hash, indexed_at)
           VALUES ('img.png', 'image', 3, 'h', 'now');
         INSERT INTO edges(id, src_id, dst_resource_path, dst_path_raw, type, origin, embed, caption)
           VALUES ('e1', '01JX000000000000000000000A', 'img.png', 'img.png',
                   'references', 'inline', 1, 'a sailboat');",
    )
    .unwrap();

    // Dedup: same (src, resource, type, occurrence) must refuse — NULL dst_id makes
    // the note-edge UNIQUE constraint inert here, hence the partial index.
    let dup = conn.execute(
        "INSERT INTO edges(id, src_id, dst_resource_path, dst_path_raw, type, origin)
         VALUES ('e2', '01JX000000000000000000000A', 'img.png', 'img.png',
                 'references', 'inline')",
        [],
    );
    assert!(
        dup.is_err(),
        "duplicate resource edge must violate the partial unique index"
    );

    // Prune the resource: the edge survives as dangling, raw text retained.
    conn.execute("DELETE FROM resources WHERE path = 'img.png'", [])
        .unwrap();
    let (dst_resource, raw): (Option<String>, String) = conn
        .query_row(
            "SELECT dst_resource_path, dst_path_raw FROM edges WHERE id = 'e1'",
            [],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .unwrap();
    assert_eq!(dst_resource, None, "prune must re-dangle the edge");
    assert_eq!(
        raw, "img.png",
        "the authored raw target must survive the prune"
    );
}

// ---------------------------------------------------------------------------
// Step 2 — the generalized walk: inventory, hashing, pruning (spec §2)
// ---------------------------------------------------------------------------

/// The walk inventories every non-`.md` file, classified by extension, and skips
/// dot-prefixed files and folders (`.DS_Store` is not vault material).
#[test]
fn walk_inventories_and_classifies_resources() {
    let tmp = tempfile::TempDir::new().unwrap();
    common::golden_vault_copy(tmp.path());
    fs::write(tmp.path().join("resources/.hidden.txt"), "nope").unwrap();

    let vault = Vault::open(tmp.path()).unwrap();
    let report = vault.project(false).unwrap();

    let rows = resource_rows(tmp.path());
    let classes: Vec<(&str, &str)> = rows
        .iter()
        .map(|(p, c, _, _)| (p.as_str(), c.as_str()))
        .collect();
    assert_eq!(
        classes,
        vec![
            ("resources/blob.bin", "binary"),
            ("resources/clipping.html", "html"),
            ("resources/data.txt", "text"),
            ("resources/diagram.png", "image"),
        ],
        "inventory must cover exactly the non-dot resources, classified"
    );
    assert_eq!(report.resources_indexed, 4);
    assert_eq!(report.resources_pruned, 0);
    assert!(report.skipped.is_empty(), "a clean vault skips nothing");
}

/// An unchanged `(size, mtime)` short-circuits the byte read: the stored hash is
/// only recomputed when the stat changes (hashing is the pass's one byte-read).
#[test]
fn unchanged_stat_short_circuits_the_rehash() {
    let tmp = tempfile::TempDir::new().unwrap();
    common::golden_vault_copy(tmp.path());
    let vault = Vault::open(tmp.path()).unwrap();
    vault.project(false).unwrap();

    let txt = tmp.path().join("resources/data.txt");
    let before = resource_rows(tmp.path());
    let original_mtime = fs::metadata(&txt).unwrap().modified().unwrap();

    // Same-length different bytes, mtime restored: the stat is identical, so the
    // pass must not re-read — the stored hash stays (observably) stale.
    let stale_bytes = "PLAIN text resource for the inventory tests\n";
    fs::write(&txt, stale_bytes).unwrap();
    fs::File::options()
        .write(true)
        .open(&txt)
        .unwrap()
        .set_modified(original_mtime)
        .unwrap();
    vault.project(false).unwrap();
    assert_eq!(
        resource_rows(tmp.path()),
        before,
        "matching (size, mtime) must not re-hash"
    );

    // A touched mtime re-reads and refreshes the hash. Bump it a clear +2s past the
    // original: stored mtime is whole-second granularity (ingest.rs `as_secs`), so
    // `now()` can land in the *same second* as `original_mtime` and (correctly) not
    // re-hash — a flake. A fixed offset makes the stat change deterministically.
    fs::File::options()
        .write(true)
        .open(&txt)
        .unwrap()
        .set_modified(original_mtime + std::time::Duration::from_secs(2))
        .unwrap();
    vault.project(false).unwrap();
    let after = resource_rows(tmp.path());
    let hash_of = |rows: &[(String, String, i64, String)]| {
        rows.iter()
            .find(|(p, _, _, _)| p == "resources/data.txt")
            .map(|(_, _, _, h)| h.clone())
            .unwrap()
    };
    assert_ne!(
        hash_of(&after),
        hash_of(&before),
        "a changed stat must re-hash the bytes"
    );
    assert_eq!(
        hash_of(&after),
        blake3::hash(stale_bytes.as_bytes()).to_hex().to_string()
    );
}

/// A deleted file's inventory row is pruned on the next projection pass.
#[test]
fn pruning_deletes_rows_the_walk_no_longer_sees() {
    let tmp = tempfile::TempDir::new().unwrap();
    common::golden_vault_copy(tmp.path());
    let vault = Vault::open(tmp.path()).unwrap();
    vault.project(false).unwrap();

    fs::remove_file(tmp.path().join("resources/blob.bin")).unwrap();
    let report = vault.project(false).unwrap();

    assert_eq!(report.resources_indexed, 3);
    assert_eq!(report.resources_pruned, 1);
    assert!(
        !resource_rows(tmp.path())
            .iter()
            .any(|(p, _, _, _)| p == "resources/blob.bin"),
        "the deleted file's row must be pruned"
    );
}

/// `full-reindex ≡ incremental-update`, extended over resource add/change/delete
/// (spec §7): a vault mutated then incrementally re-projected matches a fresh
/// build of the same tree.
#[test]
fn incremental_resource_update_equals_full_rebuild() {
    let mutate = |root: &Path| {
        fs::write(root.join("resources/new-note-data.csv"), "a,b\n1,2\n").unwrap();
        fs::write(
            root.join("resources/data.txt"),
            "changed content, new length\n",
        )
        .unwrap();
        fs::remove_file(root.join("resources/blob.bin")).unwrap();
    };

    // Incremental: project, mutate, project again.
    let a = tempfile::TempDir::new().unwrap();
    common::golden_vault_copy(a.path());
    let vault_a = Vault::open(a.path()).unwrap();
    vault_a.project(false).unwrap();
    mutate(a.path());
    vault_a.project(false).unwrap();

    // Fresh: the same final tree, projected once from scratch.
    let b = tempfile::TempDir::new().unwrap();
    common::golden_vault_copy(b.path());
    mutate(b.path());
    let vault_b = Vault::open(b.path()).unwrap();
    vault_b.project(false).unwrap();

    assert_eq!(
        resource_rows(a.path()),
        resource_rows(b.path()),
        "incremental resource update must equal a full rebuild"
    );
}

// ---------------------------------------------------------------------------
// Step 4 — resolution: kind dispatch, dst_resource_path, dangling (spec §3)
// ---------------------------------------------------------------------------

/// All edges out of one source path: `(dst_id, dst_resource_path, dst_path_raw,
/// type, embed, caption)`, in raw-target order.
type EdgeTuple = (
    Option<String>,
    Option<String>,
    String,
    String,
    i64,
    Option<String>,
);
fn edges_from(root: &Path, src_path: &str) -> Vec<EdgeTuple> {
    let conn = open(&root.join(".b2/b2.sqlite")).unwrap();
    let mut stmt = conn
        .prepare(
            "SELECT e.dst_id, e.dst_resource_path, e.dst_path_raw, e.type, e.embed, e.caption
             FROM edges e JOIN notes n ON n.b2id = e.src_id
             WHERE n.path = ?1 ORDER BY e.dst_path_raw, e.occurrence_index",
        )
        .unwrap();
    let rows = stmt
        .query_map([src_path], |r| {
            Ok((
                r.get(0)?,
                r.get(1)?,
                r.get(2)?,
                r.get(3)?,
                r.get(4)?,
                r.get(5)?,
            ))
        })
        .unwrap()
        .map(Result::unwrap)
        .collect::<Vec<_>>();
    rows
}

/// Resource links resolve to `dst_resource_path` (never `dst_id`), capture the
/// authored caption + embed marker, and a missing target dangles with both
/// resolution columns NULL and the raw text retained.
#[test]
fn resource_links_resolve_capture_and_dangle() {
    let tmp = tempfile::TempDir::new().unwrap();
    common::golden_vault_copy(tmp.path());
    fs::write(
        tmp.path().join("notes/with-resources.md"),
        "---\ntitle: With resources\n---\n\
         ![a tiny diagram](../resources/diagram.png)\n\
         [the data](../resources/data.txt)\n\
         ![[resources/blob.bin]]\n\
         [gone](../resources/nope.png)\n\
         [ext](https://example.com/x.png)\n",
    )
    .unwrap();

    let vault = Vault::open(tmp.path()).unwrap();
    vault.project(false).unwrap();

    let edges = edges_from(tmp.path(), "notes/with-resources.md");
    assert_eq!(
        edges,
        vec![
            // note-relative Markdown targets resolve against resources/
            (
                None,
                Some("resources/data.txt".into()),
                "../resources/data.txt".into(),
                "references".into(),
                0,
                Some("the data".into()),
            ),
            (
                None,
                Some("resources/diagram.png".into()),
                "../resources/diagram.png".into(),
                "references".into(),
                1,
                Some("a tiny diagram".into()),
            ),
            // a missing target dangles: both columns NULL, raw retained
            (
                None,
                None,
                "../resources/nope.png".into(),
                "references".into(),
                0,
                Some("gone".into()),
            ),
            // wikilink embeds resolve vault-root
            (
                None,
                Some("resources/blob.bin".into()),
                "resources/blob.bin".into(),
                "references".into(),
                1,
                None,
            ),
        ],
        "external URL yields nothing; the rest resolve or dangle as authored"
    );
}

/// Markdown-form links to notes are edges too (extension dispatch), and a
/// `#fragment` is stripped for the lookup while `dst_path_raw` keeps it.
#[test]
fn markdown_note_links_resolve_with_fragment_stripped() {
    let tmp = tempfile::TempDir::new().unwrap();
    common::golden_vault_copy(tmp.path());
    fs::write(
        tmp.path().join("notes/md-note-links.md"),
        "---\ntitle: md links\n---\n\
         [background](../concepts/memory.md)\n\
         [section](../concepts/memory.md#retrieval)\n",
    )
    .unwrap();

    let vault = Vault::open(tmp.path()).unwrap();
    vault.project(false).unwrap();

    let edges = edges_from(tmp.path(), "notes/md-note-links.md");
    assert_eq!(edges.len(), 2);
    for (dst_id, dst_resource, raw, r#type, _, _) in &edges {
        assert_eq!(dst_id.as_deref(), Some(common::MEMORY_ID), "raw: {raw}");
        assert_eq!(*dst_resource, None);
        assert_eq!(r#type, "references");
    }
    // occurrence disambiguates the two edges to the same (target, type)
    assert!(edges
        .iter()
        .any(|e| e.2 == "../concepts/memory.md#retrieval"));
}

// ---------------------------------------------------------------------------
// Step 5 — the façade: list_resources / explain_resource / move_resource (§4)
// ---------------------------------------------------------------------------

/// `list_resources` mirrors the inventory, path-ordered, with class + stat.
#[test]
fn list_resources_returns_the_inventory() {
    let tmp = tempfile::TempDir::new().unwrap();
    common::golden_vault_copy(tmp.path());
    let vault = Vault::open(tmp.path()).unwrap();
    vault.project(false).unwrap();

    let listed = vault.list_resources().unwrap();
    let brief: Vec<(&str, &str)> = listed
        .iter()
        .map(|r| (r.path.as_str(), r.class.as_str()))
        .collect();
    assert_eq!(
        brief,
        vec![
            ("resources/blob.bin", "binary"),
            ("resources/clipping.html", "html"),
            ("resources/data.txt", "text"),
            ("resources/diagram.png", "image"),
        ]
    );
    assert!(listed.iter().all(|r| r.size > 0));
}

/// The fallback card: metadata + backlinks with authored context; an unknown
/// path errors ResourceNotFound.
#[test]
fn explain_resource_carries_metadata_and_backlinks() {
    let tmp = tempfile::TempDir::new().unwrap();
    common::golden_vault_copy(tmp.path());
    fs::write(
        tmp.path().join("notes/card.md"),
        "---\ntitle: Card\n---\n![a tiny diagram](../resources/diagram.png)\n",
    )
    .unwrap();
    let vault = Vault::open(tmp.path()).unwrap();
    vault.project(false).unwrap();

    let view = vault.explain_resource("resources/diagram.png").unwrap();
    assert_eq!(view.class, "image");
    assert_eq!(view.size, 67);
    assert_eq!(view.content_hash.len(), 64, "blake3 hex");
    assert_eq!(view.backlinks.len(), 1);
    let b = &view.backlinks[0];
    assert_eq!(b.path, "notes/card.md");
    // The backlink shows the linking note's title — its filename (data-model.md §1).
    assert_eq!(b.title.as_deref(), Some("card"));
    assert_eq!(b.r#type, "references");
    assert_eq!(b.caption.as_deref(), Some("a tiny diagram"));
    assert!(b.embed);

    let missing = vault.explain_resource("resources/nope.pdf");
    assert!(matches!(missing, Err(b2_core::Error::ResourceNotFound(_))));
}

/// A resource move rewrites inbound links in BOTH syntaxes, each keeping its own
/// convention (note-relative stays relative, vault-root stays root), moves the
/// file, and leaves the index equal to a fresh rebuild.
#[test]
fn move_resource_rewrites_inbound_links_and_reprojects() {
    let tmp = tempfile::TempDir::new().unwrap();
    common::golden_vault_copy(tmp.path());
    fs::write(
        tmp.path().join("notes/uses-diagram.md"),
        "---\ntitle: Uses\n---\n\
         ![d](../resources/diagram.png)\n\
         see ![[resources/diagram.png|hero]] too\n",
    )
    .unwrap();
    let vault = Vault::open(tmp.path()).unwrap();
    vault.project(false).unwrap();

    let report = vault
        .move_resource("resources/diagram.png", "img/diagram.png")
        .unwrap();
    assert_eq!(report.from, "resources/diagram.png");
    assert_eq!(report.to, "img/diagram.png");
    assert_eq!(report.rewrote, vec!["notes/uses-diagram.md".to_string()]);
    assert_eq!(report.links_rewritten, 2);

    // Each authored form kept its convention.
    let body = fs::read_to_string(tmp.path().join("notes/uses-diagram.md")).unwrap();
    assert!(
        body.contains("![d](../img/diagram.png)"),
        "md form re-relativized: {body}"
    );
    assert!(
        body.contains("![[img/diagram.png|hero]]"),
        "wikilink stays vault-root: {body}"
    );

    // File moved; inventory + edges follow.
    assert!(tmp.path().join("img/diagram.png").exists());
    assert!(!tmp.path().join("resources/diagram.png").exists());
    let edges = edges_from(tmp.path(), "notes/uses-diagram.md");
    assert!(
        edges
            .iter()
            .all(|(_, dst, _, _, _, _)| dst.as_deref() == Some("img/diagram.png")),
        "all edges resolve at the new path: {edges:?}"
    );

    // Refuses to clobber, refuses unknown source.
    fs::write(tmp.path().join("img/other.png"), "x").unwrap();
    vault.project(false).unwrap();
    assert!(matches!(
        vault.move_resource("img/diagram.png", "img/other.png"),
        Err(b2_core::Error::MoveTargetExists(_))
    ));
    assert!(matches!(
        vault.move_resource("resources/diagram.png", "elsewhere.png"),
        Err(b2_core::Error::ResourceNotFound(_))
    ));
}

/// `similar` on a resource anchor is honest: "not yet" for an inventoried
/// resource (never a silent empty), the usual not-found for an unknown path.
#[test]
fn similar_on_a_resource_errs_not_yet() {
    let tmp = tempfile::TempDir::new().unwrap();
    common::golden_vault_copy(tmp.path());
    let vault = Vault::open(tmp.path()).unwrap();
    vault.project(false).unwrap();

    assert!(matches!(
        vault.similar("resources/diagram.png", 5),
        Err(b2_core::Error::ResourceUnsupported(_))
    ));
    assert!(matches!(
        vault.similar("resources/nope.png", 5),
        Err(b2_core::Error::NoteNotFound(_))
    ));
}
