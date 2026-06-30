//! The `b2id` generator seam. Identity is ULID-style (data-model.md §1); the
//! generator is a trait so tests can stamp deterministic ids while production
//! mints real ULIDs — the same swappable-seam discipline the AI parts use.

/// Mints a new `b2id`. The value is a bare ULID; the namespacing is in the
/// frontmatter *key* (`b2id`, not `id`), not a value prefix (data-model.md §1).
pub trait IdGen {
    fn new_id(&self) -> String;
}

/// Production generator: a fresh ULID per call.
#[derive(Debug, Default, Clone, Copy)]
pub struct UlidGen;

impl IdGen for UlidGen {
    fn new_id(&self) -> String {
        ulid::Ulid::new().to_string()
    }
}
