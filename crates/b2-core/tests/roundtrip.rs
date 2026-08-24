//! Lossless parse/serialize — `parse -> serialize -> parse` must be byte-identical,
//! preserving unknown frontmatter keys and order, comments, and whitespace. B2 achieves it
//! by keeping the raw text and only ever making the surgical edits it is asked to make.
//!
//! The *headingless* case is not pinned here: `props.rs` already proves it over 512 generated
//! strings, which are overwhelmingly frontmatter-free. What that property cannot reach — a
//! real, messy, human-authored block — is what the cases below hold.

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
fn extracts_queryable_fields_without_disturbing_raw() {
    let raw = golden("notes/spaced-repetition.md");
    let n = parse(&raw);
    let f = n.fields();
    assert_eq!(f.r#type.as_deref(), Some("concept"));
    // title is the logical value (quotes are a serialization detail kept in raw).
    assert_eq!(f.title.as_deref(), Some("Spaced repetition"));
    assert_eq!(f.created.as_deref(), Some("2026-06-20"));
}
