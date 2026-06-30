//! Step 1 — `b2id` stamping: B2's one always-allowed write to the vault.
//!
//! Stamping inserts exactly one line (`b2id: <ulid>`) at the top of the
//! frontmatter and touches nothing else; a note that already has a `b2id` is
//! never re-stamped (data-model.md §1, §6).

use b2_core::note::parse;

#[test]
fn stamps_missing_b2id_at_top_of_frontmatter_preserving_everything_else() {
    let raw = "---\ntype: concept\ntitle: \"No id yet\"\ncreated: 2026-06-20\n---\nBody stays exactly the same.\n";
    let mut n = parse(raw);
    assert!(n.fields().b2id.is_none());

    n.stamp_b2id("01JSTAMP00000000000000000A");

    assert_eq!(
        n.fields().b2id.as_deref(),
        Some("01JSTAMP00000000000000000A")
    );
    let expected = "---\nb2id: 01JSTAMP00000000000000000A\ntype: concept\ntitle: \"No id yet\"\ncreated: 2026-06-20\n---\nBody stays exactly the same.\n";
    assert_eq!(n.as_str(), expected);
    // The single surgical edit re-parses cleanly and is idempotent.
    assert_eq!(parse(n.as_str()).as_str(), expected);
}

#[test]
fn stamps_into_a_note_that_has_no_frontmatter_block() {
    let raw = "Just a body.\n";
    let mut n = parse(raw);
    n.stamp_b2id("01JNOFM000000000000000000A");
    let expected = "---\nb2id: 01JNOFM000000000000000000A\n---\nJust a body.\n";
    assert_eq!(n.as_str(), expected);
    assert_eq!(
        parse(n.as_str()).fields().b2id.as_deref(),
        Some("01JNOFM000000000000000000A")
    );
}

#[test]
fn never_restamps_a_note_that_already_has_a_b2id() {
    let raw = "---\nb2id: 01JEXIST00000000000000000\ntype: note\n---\nBody.\n";
    let mut n = parse(raw);
    n.stamp_b2id("01JOTHER00000000000000000A"); // must be a no-op
    assert_eq!(
        n.fields().b2id.as_deref(),
        Some("01JEXIST00000000000000000")
    );
    assert_eq!(n.as_str(), raw);
}
