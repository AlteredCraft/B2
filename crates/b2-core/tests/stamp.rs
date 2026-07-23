//! Step 1 — `b2id` stamping: B2's one always-allowed write to the vault.
//!
//! Stamping inserts exactly one line (`b2id: <ulid>`) at the top of the
//! frontmatter and touches nothing else; a note that already has a `b2id` is
//! never re-stamped (data-model.md §1, §6).

use b2_core::note::parse;
use b2_core::vault::Vault;
use std::fs;

/// Frontmatter that will never YAML-parse (a tab-key line and an unclosed flow
/// sequence) — the #75 shape: extraction must still see a `b2id:` line, or the
/// stamp gate re-fires forever.
const BAD_YAML_NOTE: &str = "---\n\t: :\ntitle: [unclosed\n---\nA body line.\n";

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

/// #75 — stamping is gated on a `b2id:` line being *definitively absent*, never on
/// "the YAML wouldn't parse": unparseable frontmatter must still be stamped **at
/// most once**, and the stamped id must be readable back (else identity churns).
#[test]
fn invalid_yaml_frontmatter_is_stamped_at_most_once() {
    let mut n = parse(BAD_YAML_NOTE);
    assert!(n.fields().b2id.is_none());

    n.stamp_b2id("01JBAD000000000000000000AA");
    assert_eq!(
        n.fields().b2id.as_deref(),
        Some("01JBAD000000000000000000AA"),
        "the stamped id must be visible even though the YAML never parses"
    );

    // A fresh parse of the stamped bytes must also see it — and refuse a re-stamp.
    let stamped = n.as_str().to_string();
    let mut again = parse(&stamped);
    assert_eq!(
        again.fields().b2id.as_deref(),
        Some("01JBAD000000000000000000AA")
    );
    again.stamp_b2id("01JOTHER00000000000000000A");
    assert_eq!(again.as_str(), stamped, "never a second b2id line");
}

/// #75, end to end — reindex settles after one stamp: the second pass writes
/// nothing and the note's identity is stable in the index.
#[test]
fn reindex_settles_after_one_stamp_even_with_invalid_frontmatter() {
    let tmp = tempfile::TempDir::new().unwrap();
    let root = tmp.path().join("vault");
    fs::create_dir_all(&root).unwrap();
    let path = root.join("bad.md");
    fs::write(&path, BAD_YAML_NOTE).unwrap();

    let vault = Vault::open(&root).unwrap();
    let first = vault.reindex().unwrap();
    assert_eq!(first.stamped, 1, "the missing b2id is stamped once");
    let after_first = fs::read_to_string(&path).unwrap();
    let id = parse(&after_first)
        .fields()
        .b2id
        .clone()
        .expect("the stamp must be readable back");

    let second = vault.reindex().unwrap();
    assert_eq!(second.stamped, 0, "a second pass has nothing to stamp");
    assert_eq!(
        fs::read_to_string(&path).unwrap(),
        after_first,
        "reindex must not keep rewriting the note (#75's growth loop)"
    );
    // The projected identity is stable — the note answers by the stamped id.
    assert_eq!(vault.read(&id).unwrap().b2id, id);
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
