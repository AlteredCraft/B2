//! `b2 reindex --dry-run` — a read-only preview of a reindex. Driven through the
//! [`Vault`] façade against the golden vault, deterministic under the FakeEmbedder.
//! The contract: it reports exactly what a real reindex *would* do, and writes
//! **nothing** — no `b2id` stamped to the Markdown, no note projected into the index.

mod common;

use b2_core::vault::Vault;
use b2_core::Error;
use common::{golden_vault_copy, MEMORY_ID};
use std::fs;
use std::path::Path;

/// A golden vault copy plus one note deliberately **missing** a `b2id`, so a dry-run
/// has something to report as "would stamp". Returns (vault, root, the un-stamped
/// file's path).
fn vault_with_an_unstamped_note(dir: &Path) -> (Vault, std::path::PathBuf, std::path::PathBuf) {
    let root = dir.join("vault");
    golden_vault_copy(&root);
    let fresh = root.join("fresh.md");
    fs::write(&fresh, "---\ntype: note\ntitle: Fresh\n---\nNo b2id yet.\n").unwrap();
    (Vault::open(&root).unwrap(), root, fresh)
}

#[test]
fn dry_run_previews_counts_without_writing_anything() {
    let tmp = tempfile::TempDir::new().unwrap();
    let (vault, _root, fresh) = vault_with_an_unstamped_note(tmp.path());
    let before = fs::read_to_string(&fresh).unwrap();

    // Pristine index: all three notes would be indexed + embedded; only the
    // un-stamped one would be stamped.
    let plan = vault.plan_reindex(false).unwrap();
    assert_eq!(plan.would_index, 3);
    assert_eq!(
        plan.would_embed, 3,
        "a never-embedded vault embeds every note"
    );
    assert_eq!(
        plan.would_stamp, 1,
        "only the b2id-less note would be stamped"
    );

    // The one write a reindex performs — the b2id stamp — did NOT happen: the file
    // is byte-identical.
    assert_eq!(
        fs::read_to_string(&fresh).unwrap(),
        before,
        "no stamp written"
    );
    // …and nothing was projected into the index (a golden b2id still doesn't resolve).
    assert!(matches!(
        vault.neighbors(MEMORY_ID).unwrap_err(),
        Error::NoteNotFound(_)
    ));
}

#[test]
fn dry_run_matches_what_a_real_reindex_then_does() {
    let tmp = tempfile::TempDir::new().unwrap();
    let (vault, _root, _fresh) = vault_with_an_unstamped_note(tmp.path());

    // The preview…
    let plan = vault.plan_reindex(false).unwrap();
    // …exactly equals what the real reindex reports it did.
    let report = vault.reindex().unwrap();
    assert_eq!(plan.would_index, report.indexed);
    assert_eq!(plan.would_embed, report.embedded);
    assert_eq!(plan.would_stamp, report.stamped);

    // A second preview, now against the populated index, would do nothing: every
    // note is stamped + embedded and unchanged.
    let plan2 = vault.plan_reindex(false).unwrap();
    assert_eq!(plan2.would_index, 3);
    assert_eq!(plan2.would_embed, 0, "unchanged notes would not re-embed");
    assert_eq!(
        plan2.would_stamp, 0,
        "already-stamped notes would not re-stamp"
    );
}

#[test]
fn force_dry_run_would_reembed_everything() {
    let tmp = tempfile::TempDir::new().unwrap();
    let (vault, _root, _fresh) = vault_with_an_unstamped_note(tmp.path());
    vault.reindex().unwrap();

    let plan = vault.plan_reindex(true).unwrap();
    assert_eq!(plan.would_embed, plan.would_index, "--force re-embeds all");
    assert_eq!(plan.would_stamp, 0, "everything is already stamped");
}

#[test]
fn dry_run_flags_only_a_changed_note() {
    let tmp = tempfile::TempDir::new().unwrap();
    let (vault, root, _fresh) = vault_with_an_unstamped_note(tmp.path());
    vault.reindex().unwrap();

    // Edit one note's body; a dry-run should predict exactly that one re-embed.
    let memory = root.join("concepts/memory.md");
    let mut text = fs::read_to_string(&memory).unwrap();
    text.push_str("\nAn appended paragraph changes the body hash.\n");
    fs::write(&memory, text).unwrap();

    let plan = vault.plan_reindex(false).unwrap();
    assert_eq!(plan.would_embed, 1, "only the edited note would re-embed");
    assert_eq!(plan.would_stamp, 0);
}
