//! Frontmatter `relations:` — the reader (→ origin=frontmatter edges), the
//! surgical `add_relation` editor (lossless), and inline-wins dedup
//! (planning/data-model.md §0, §2, §3).

mod common;

use b2_core::embed::FakeEmbedder;
use b2_core::event::NullSink;
use b2_core::id::UlidGen;
use b2_core::ingest::ingest_vault;
use b2_core::note::parse;
use b2_core::open;
use rusqlite::Connection;
use std::fs;
use std::path::Path;

const A: &str = "01JA0000000000000000000001";
const B: &str = "01JB0000000000000000000002";

fn ingest(vault: &Path, db: &Path) -> Connection {
    let conn = open(db).unwrap();
    ingest_vault(&conn, vault, &UlidGen, &NullSink, &FakeEmbedder::default()).unwrap();
    conn
}

#[test]
fn add_relation_creates_a_block_when_absent() {
    let raw = "---\nb2id: 01JX\ntype: note\n---\nBody.\n";
    let mut n = parse(raw);
    n.add_relation("contradicts [[concepts/memory|Human memory]] — because")
        .unwrap();
    let expected = "---\nb2id: 01JX\ntype: note\nrelations:\n  - \"contradicts [[concepts/memory|Human memory]] — because\"\n---\nBody.\n";
    assert_eq!(n.as_str(), expected);
    // re-parse is stable + the entry reads back
    assert_eq!(parse(n.as_str()).as_str(), expected);
    assert_eq!(
        parse(n.as_str()).fields().relations,
        vec!["contradicts [[concepts/memory|Human memory]] — because".to_string()]
    );
}

#[test]
fn add_relation_appends_to_an_existing_block() {
    let raw = "---\nb2id: 01JX\ntype: note\nrelations:\n  - \"supports [[a|A]]\"\n---\nBody.\n";
    let mut n = parse(raw);
    n.add_relation("refutes [[b|B]]").unwrap();
    let expected = "---\nb2id: 01JX\ntype: note\nrelations:\n  - \"supports [[a|A]]\"\n  - \"refutes [[b|B]]\"\n---\nBody.\n";
    assert_eq!(n.as_str(), expected);
}

#[test]
fn add_relation_preserves_other_keys_and_body() {
    let raw = "---\ntitle: Keep me\nb2id: 01JX\ncustom: [1, 2, 3]  # comment\ntype: note\n---\nBody stays.\nLine 2.\n";
    let mut n = parse(raw);
    n.add_relation("relates [[x|X]]").unwrap();
    let out = n.as_str();
    assert!(
        out.contains("custom: [1, 2, 3]  # comment"),
        "unknown key preserved"
    );
    assert!(out.contains("Body stays.\nLine 2.\n"), "body preserved");
    assert!(out.contains("relations:\n  - \"relates [[x|X]]\"\n"));
    assert_eq!(parse(out).as_str(), out); // round-trip
}

#[test]
fn add_relation_quotes_safely() {
    let raw = "---\nb2id: 01JX\ntype: note\n---\nBody.\n";
    let mut n = parse(raw);
    n.add_relation("relates [[x|X]] — has \"quotes\" inside")
        .unwrap();
    // the embedded quotes are escaped, and it re-parses to the original spec
    assert_eq!(
        parse(n.as_str()).fields().relations,
        vec!["relates [[x|X]] — has \"quotes\" inside".to_string()]
    );
}

#[test]
fn reader_projects_frontmatter_relations_as_frontmatter_edges() {
    let tmp = tempfile::TempDir::new().unwrap();
    let vault = tmp.path().join("vault");
    fs::create_dir_all(&vault).unwrap();
    fs::write(
        vault.join("a.md"),
        format!("---\nb2id: {A}\ntype: note\ntitle: A\nrelations:\n  - \"supports [[b|B]]\"\n---\nBody.\n"),
    )
    .unwrap();
    fs::write(
        vault.join("b.md"),
        format!("---\nb2id: {B}\ntype: note\ntitle: B\n---\nBody.\n"),
    )
    .unwrap();
    let conn = ingest(&vault, &tmp.path().join("b2.sqlite"));

    let (dst, typ, origin): (String, String, String) = conn
        .query_row(
            "SELECT dst_id, type, origin FROM edges WHERE src_id = ?1",
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
fn inline_wins_when_the_same_edge_is_in_both_body_and_frontmatter() {
    let tmp = tempfile::TempDir::new().unwrap();
    let vault = tmp.path().join("vault");
    fs::create_dir_all(&vault).unwrap();
    // a references b in the BODY *and* declares the same edge in frontmatter
    fs::write(
        vault.join("a.md"),
        format!("---\nb2id: {A}\ntype: note\ntitle: A\nrelations:\n  - \"references [[b|B]]\"\n---\nSee [[b|B]].\n"),
    )
    .unwrap();
    fs::write(
        vault.join("b.md"),
        format!("---\nb2id: {B}\ntype: note\ntitle: B\n---\nBody.\n"),
    )
    .unwrap();
    let conn = ingest(&vault, &tmp.path().join("b2.sqlite"));

    // exactly one references edge a→b, and it is origin=inline (body wins)
    let origins: Vec<String> = {
        let mut s = conn
            .prepare("SELECT origin FROM edges WHERE src_id = ?1 AND dst_id = ?2 AND type = 'references'")
            .unwrap();
        s.query_map([A, B], |r| r.get(0))
            .unwrap()
            .map(Result::unwrap)
            .collect()
    };
    assert_eq!(origins, vec!["inline".to_string()]);
}
