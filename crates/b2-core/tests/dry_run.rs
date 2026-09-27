//! `b2 reindex --dry-run`: forecasts exactly what a real reindex would do, and touches
//! nothing, neither the vault nor the index.

mod common;

use b2_core::vault::Vault;
use b2_core::Error;
use common::{golden_vault_copy, MEMORY_PATH};
use std::fs;
use std::path::Path;

/// The golden vault plus one note, so the counts are not all 2.
fn vault_with_an_extra_note(dir: &Path) -> (Vault, std::path::PathBuf, std::path::PathBuf) {
    let root = dir.join("vault");
    golden_vault_copy(&root);
    let fresh = root.join("fresh.md");
    fs::write(
        &fresh,
        "---\ntype: note\ntitle: Fresh\n---\nA third note.\n",
    )
    .unwrap();
    (Vault::open(&root).unwrap(), root, fresh)
}

#[test]
fn dry_run_previews_counts_without_writing_anything() {
    let tmp = tempfile::TempDir::new().unwrap();
    let (vault, root, fresh) = vault_with_an_extra_note(tmp.path());
    let paths = ["concepts/memory.md", "notes/spaced-repetition.md"];
    let before: Vec<String> = paths
        .iter()
        .map(|p| fs::read_to_string(root.join(p)).unwrap())
        .chain(std::iter::once(fs::read_to_string(&fresh).unwrap()))
        .collect();

    let plan = vault.plan_reindex(false).unwrap();
    assert_eq!(plan.would_index, 3);
    assert_eq!(
        plan.would_embed, 3,
        "a never-embedded vault embeds every note"
    );

    // The real run writes nothing either, so the dry half is the index.
    for (path, was) in paths
        .iter()
        .map(|p| root.join(p))
        .chain(std::iter::once(fresh.clone()))
        .zip(&before)
    {
        assert_eq!(&fs::read_to_string(&path).unwrap(), was);
    }
    assert!(
        matches!(
            vault.neighbors(MEMORY_PATH).unwrap_err(),
            Error::NoteNotFound(_)
        ),
        "a preview projects no rows, so nothing resolves yet"
    );
}

#[test]
fn dry_run_matches_what_a_real_reindex_then_does() {
    let tmp = tempfile::TempDir::new().unwrap();
    let (vault, _root, _fresh) = vault_with_an_extra_note(tmp.path());

    let plan = vault.plan_reindex(false).unwrap();
    let report = vault.reindex().unwrap();
    assert_eq!(plan.would_index, report.indexed);
    assert_eq!(plan.would_embed, report.embedded);

    // Unchanged chunks hash to vectors already stored.
    let plan2 = vault.plan_reindex(false).unwrap();
    assert_eq!(plan2.would_index, 3);
    assert_eq!(plan2.would_embed, 0, "unchanged notes would not re-embed");
}

#[test]
fn force_dry_run_would_reembed_everything() {
    let tmp = tempfile::TempDir::new().unwrap();
    let (vault, _root, _fresh) = vault_with_an_extra_note(tmp.path());
    vault.reindex().unwrap();

    let plan = vault.plan_reindex(true).unwrap();
    assert_eq!(plan.would_embed, plan.would_index, "--force re-embeds all");
}

#[test]
fn dry_run_flags_only_a_changed_note() {
    let tmp = tempfile::TempDir::new().unwrap();
    let (vault, root, _fresh) = vault_with_an_extra_note(tmp.path());
    vault.reindex().unwrap();

    let memory = root.join("concepts/memory.md");
    let mut text = fs::read_to_string(&memory).unwrap();
    text.push_str("\nAn appended paragraph changes the body hash.\n");
    fs::write(&memory, text).unwrap();

    let plan = vault.plan_reindex(false).unwrap();
    assert_eq!(plan.would_embed, 1, "only the edited note would re-embed");
}
