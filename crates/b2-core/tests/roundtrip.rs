//! Step 1 — lossless parse/serialize (the round-trip invariant).
//!
//! `parse → serialize → parse` must be byte-identical, preserving unknown
//! frontmatter keys + order, comments, and whitespace (data-model.md §6). B2
//! achieves this by keeping the raw text and only ever making the surgical edits
//! it is asked to make — never re-dumping YAML.

use b2_core::note::parse;
use std::fs;
use std::path::Path;

fn golden(rel: &str) -> String {
    let p = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures/golden-vault")
        .join(rel);
    fs::read_to_string(p).unwrap()
}

#[test]
fn golden_notes_round_trip_byte_identical() {
    for rel in ["concepts/memory.md", "notes/spaced-repetition.md"] {
        let raw = golden(rel);
        assert_eq!(
            parse(&raw).as_str(),
            raw,
            "round-trip must be byte-identical for {rel}"
        );
    }
}

#[test]
fn round_trip_preserves_unknown_keys_comments_and_whitespace() {
    let raw = concat!(
        "---\n",
        "title:   Messy Note   \n",
        "custom_key: [a, b, c]   # inline comment\n",
        "nested:\n",
        "  k: v\n",
        "b2id: 01JTEST00000000000000000A\n",
        "tags: [x, y]\n",
        "---\n",
        "\n",
        "Body with a [[link]] and trailing spaces.   \n",
        "\n",
        "Last line, no trailing newline",
    );
    assert_eq!(parse(raw).as_str(), raw);
}

#[test]
fn round_trip_note_without_frontmatter() {
    let raw = "Just a body, no frontmatter at all.\n";
    let n = parse(raw);
    assert_eq!(n.as_str(), raw);
    assert!(n.fields().b2id.is_none());
}

#[test]
fn extracts_queryable_fields_without_disturbing_raw() {
    let raw = golden("notes/spaced-repetition.md");
    let n = parse(&raw);
    let f = n.fields();
    assert_eq!(f.b2id.as_deref(), Some("01JSRS0000000000000000000B"));
    assert_eq!(f.r#type.as_deref(), Some("concept"));
    // title is the logical value (quotes are a serialization detail kept in raw).
    assert_eq!(f.title.as_deref(), Some("Spaced repetition"));
    assert_eq!(f.created.as_deref(), Some("2026-06-20"));
}
