//! Frontmatter `b2_relations:` — the reader (→ origin=frontmatter edges), the
//! surgical `add_relation` editor (lossless), and frontmatter-wins dedup
//! (data-model.md §0, §2, §3).

mod common;

use b2_core::embed::FakeEmbedder;
use b2_core::ingest::ingest_vault;
use b2_core::note::parse;
use b2_core::open;
use rusqlite::Connection;
use std::fs;
use std::path::Path;

const A: &str = "a.md";
const B: &str = "b.md";

fn ingest(vault: &Path, db: &Path) -> Connection {
    let conn = open(db).unwrap();
    ingest_vault(&conn, vault, &FakeEmbedder::default()).unwrap();
    conn
}

#[test]
fn add_relation_creates_a_block_when_absent() {
    let raw = "---\ntype: note\n---\nBody.\n";
    let mut n = parse(raw);
    n.add_relation("contradicts [[concepts/memory|Human memory]] — because")
        .unwrap();
    let expected = "---\ntype: note\nb2_relations:\n  - \"contradicts [[concepts/memory|Human memory]] — because\"\n---\nBody.\n";
    assert_eq!(n.as_str(), expected);
    assert_eq!(parse(n.as_str()).as_str(), expected);
    assert_eq!(
        parse(n.as_str()).fields().relations,
        vec!["contradicts [[concepts/memory|Human memory]] — because".to_string()]
    );
}

#[test]
fn add_relation_appends_to_an_existing_block() {
    let raw = "---\ntype: note\nb2_relations:\n  - \"supports [[a|A]]\"\n---\nBody.\n";
    let mut n = parse(raw);
    n.add_relation("refutes [[b|B]]").unwrap();
    let expected = "---\ntype: note\nb2_relations:\n  - \"supports [[a|A]]\"\n  - \"refutes [[b|B]]\"\n---\nBody.\n";
    assert_eq!(n.as_str(), expected);
}

#[test]
fn add_relation_preserves_other_keys_and_body() {
    let raw = "---\ntitle: Keep me\ncustom: [1, 2, 3]  # comment\ntype: note\n---\nBody stays.\nLine 2.\n";
    let mut n = parse(raw);
    n.add_relation("relates [[x|X]]").unwrap();
    let out = n.as_str();
    assert!(
        out.contains("custom: [1, 2, 3]  # comment"),
        "unknown key preserved"
    );
    assert!(out.contains("Body stays.\nLine 2.\n"), "body preserved");
    assert!(out.contains("b2_relations:\n  - \"relates [[x|X]]\"\n"));
    assert_eq!(parse(out).as_str(), out);
}

#[test]
fn add_relation_quotes_safely() {
    let raw = "---\ntype: note\n---\nBody.\n";
    let mut n = parse(raw);
    n.add_relation("relates [[x|X]] — has \"quotes\" inside")
        .unwrap();
    assert_eq!(
        parse(n.as_str()).fields().relations,
        vec!["relates [[x|X]] — has \"quotes\" inside".to_string()]
    );
}

#[test]
fn a_generic_relations_key_is_not_b2s() {
    // Another tool's `relations:` is an unknown key, never projected (data-model §1).
    let raw = "---\ntype: note\nrelations:\n  - \"supports [[a|A]]\"\n---\nBody.\n";
    let n = parse(raw);
    assert!(n.fields().relations.is_empty(), "generic key must not read");
    assert_eq!(n.as_str(), raw, "and it round-trips untouched");
}

#[test]
fn reader_projects_frontmatter_relations_as_frontmatter_edges() {
    let tmp = tempfile::TempDir::new().unwrap();
    let vault = tmp.path().join("vault");
    fs::create_dir_all(&vault).unwrap();
    fs::write(
        vault.join("a.md"),
        "---\ntype: note\ntitle: A\nb2_relations:\n  - \"supports [[b|B]]\"\n---\nBody.\n",
    )
    .unwrap();
    fs::write(
        vault.join("b.md"),
        "---\ntype: note\ntitle: B\n---\nBody.\n",
    )
    .unwrap();
    let conn = ingest(&vault, &tmp.path().join("b2.sqlite"));

    let (dst, typ, origin): (String, String, String) = conn
        .query_row(
            "SELECT dst_path, type, origin FROM edges WHERE src_path = ?1",
            [A],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )
        .unwrap();
    assert_eq!(
        (dst.as_str(), typ.as_str(), origin.as_str()),
        (B, "supports", "frontmatter")
    );
}

#[test]
fn frontmatter_wins_when_the_same_edge_is_in_both_body_and_frontmatter() {
    let tmp = tempfile::TempDir::new().unwrap();
    let vault = tmp.path().join("vault");
    fs::create_dir_all(&vault).unwrap();
    // The same (target, type) in body and frontmatter; only frontmatter carries a why.
    fs::write(
        vault.join("a.md"),
        "---\ntype: note\ntitle: A\nb2_relations:\n  - \"references [[b|B]] — the why\"\n---\nSee [[b|B]].\n",
    )
    .unwrap();
    fs::write(
        vault.join("b.md"),
        "---\ntype: note\ntitle: B\n---\nBody.\n",
    )
    .unwrap();
    let conn = ingest(&vault, &tmp.path().join("b2.sqlite"));

    // One edge, and frontmatter wins (data-model §0/§3).
    let rows: Vec<(String, Option<String>)> = {
        let mut s = conn
            .prepare("SELECT origin, explanation FROM edges WHERE src_path = ?1 AND dst_path = ?2 AND type = 'references'")
            .unwrap();
        s.query_map([A, B], |r| Ok((r.get(0)?, r.get(1)?)))
            .unwrap()
            .map(Result::unwrap)
            .collect()
    };
    assert_eq!(
        rows,
        vec![("frontmatter".to_string(), Some("the why".to_string()))]
    );
}

#[test]
fn a_typed_relation_augments_a_body_link_as_a_second_edge() {
    let tmp = tempfile::TempDir::new().unwrap();
    let vault = tmp.path().join("vault");
    fs::create_dir_all(&vault).unwrap();
    // The augment flow (data-model §2): the typed entry adds a second edge.
    fs::write(
        vault.join("a.md"),
        "---\ntype: note\ntitle: A\nb2_relations:\n  - \"supports [[b|B]] — backs it\"\n---\nSee [[b|B]].\n",
    )
    .unwrap();
    fs::write(
        vault.join("b.md"),
        "---\ntype: note\ntitle: B\n---\nBody.\n",
    )
    .unwrap();
    let conn = ingest(&vault, &tmp.path().join("b2.sqlite"));

    let mut rows: Vec<(String, String)> = {
        let mut s = conn
            .prepare("SELECT type, origin FROM edges WHERE src_path = ?1 AND dst_path = ?2")
            .unwrap();
        s.query_map([A, B], |r| Ok((r.get(0)?, r.get(1)?)))
            .unwrap()
            .map(Result::unwrap)
            .collect()
    };
    rows.sort();
    assert_eq!(
        rows,
        vec![
            ("references".to_string(), "inline".to_string()),
            ("supports".to_string(), "frontmatter".to_string()),
        ]
    );
}
