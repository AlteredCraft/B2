//! The crate error type. Step 1 introduces it because ingest mixes SQLite and
//! filesystem failures; earlier steps were SQLite-only.

/// Errors surfaced by the index engine. Kept internal/structured — user-facing
/// surfaces (CLI, future GUI) translate these into generic, actionable messages.
#[derive(thiserror::Error, Debug)]
pub enum Error {
    #[error("sqlite error: {0}")]
    Sqlite(#[from] rusqlite::Error),

    #[error("io error: {0}")]
    Io(#[from] std::io::Error),

    #[error("serialization error: {0}")]
    Serde(#[from] serde_json::Error),

    #[error("frontmatter edit unsupported: {0}")]
    Frontmatter(String),

    /// A note reference (path or `b2id`) did not resolve to any indexed note —
    /// the one domain error the façade distinguishes from "found, no results".
    #[error("note not found: {0}")]
    NoteNotFound(String),

    /// The embedder failed to produce a vector (real-model tensor/runtime error).
    /// Kept as a message so `b2-core` stays free of the embedding runtime's types.
    #[error("embedding failed: {0}")]
    Embed(String),

    /// The index's recorded embedding model/dim differs from the active embedder,
    /// so its vectors are incomparable with new query vectors. A read (search)
    /// fails fast with this rather than returning silently wrong results; the fix
    /// is a `reindex` (which re-embeds). See index-engine.md §8 and tasks.md.
    #[error("index built with embedding model {indexed}, but the active model is {active}; run `b2 reindex`")]
    ModelMismatch { indexed: String, active: String },

    /// `b2 mv` was given a destination that isn't a valid vault-relative Markdown
    /// path — empty, absolute, escaping the vault via `..`, or the source itself.
    #[error("invalid move destination: {0}")]
    MoveDestination(String),

    /// `b2 mv` would overwrite an existing file — refused (the vault never clobbers,
    /// data-model.md §1). The path is echoed for the user-facing message.
    #[error("move target already exists: {0}")]
    MoveTargetExists(String),
}

pub type Result<T> = std::result::Result<T, Error>;
