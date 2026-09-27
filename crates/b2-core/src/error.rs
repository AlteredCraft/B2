//! The crate error type. Adapters match its variants to choose a user-facing message.

/// Errors surfaced by the engine. Adapters translate them into generic, actionable
/// messages.
#[derive(thiserror::Error, Debug)]
pub enum Error {
    #[error("sqlite error: {0}")]
    Sqlite(#[from] rusqlite::Error),

    #[error("io error: {0}")]
    Io(#[from] std::io::Error),

    #[error("frontmatter edit unsupported: {0}")]
    Frontmatter(String),

    /// A note reference did not resolve to an indexed note.
    #[error("note not found: {0}")]
    NoteNotFound(String),

    /// The embedder failed. A message, so `b2-core` stays free of the runtime's types.
    #[error("embedding failed: {0}")]
    Embed(String),

    /// The chat provider failed on flow ④'s answer call (a failed condense degrades
    /// instead, GH #153). A message, as for [`Error::Embed`].
    #[error("llm call failed: {0}")]
    Llm(String),

    /// One model reply asked for more tool calls than the configured cap. Separate from
    /// [`Error::Llm`] because a tool-using turn must not degrade on it: it signals a
    /// broken or hostile server.
    #[error("the model asked for more than {limit} tool calls in one reply")]
    ToolCallLimit { limit: usize },

    /// The index's recorded embedder differs from the active one, so a search would be
    /// silently wrong (index-engine.md §8).
    #[error("index built with embedding model {indexed}, but the active model is {active}; run `b2 reindex`")]
    ModelMismatch { indexed: String, active: String },

    /// `b2 mv` was given an invalid destination: empty, absolute, escaping via `..`,
    /// dot-prefixed (GH #136), or the source itself.
    #[error("invalid move destination: {0}")]
    MoveDestination(String),

    /// `b2 mv` would overwrite an existing file (data-model.md §1).
    #[error("move target already exists: {0}")]
    MoveTargetExists(String),

    /// A move failed part-way and could not be fully undone (GH #230). Carries the paths
    /// still holding the rewrite, for the user to check, and the root cause.
    #[error("move failed and could not be fully undone: {} (cause: {source})", paths.join(", "))]
    MoveIncomplete {
        paths: Vec<String>,
        source: Box<Error>,
    },

    /// A source folder doesn't exist in the vault.
    #[error("directory not found: {0}")]
    DirNotFound(String),

    /// `b2 add` was given an invalid path: empty, absolute, escaping via `..`, or
    /// dot-prefixed (GH #136).
    #[error("invalid new-note path: {0}")]
    AddDestination(String),

    /// `b2 add` would overwrite an existing file (data-model.md §1).
    #[error("note already exists: {0}")]
    AddTargetExists(String),

    /// An import was given an invalid destination: a name with separators, or a path
    /// that is empty, absolute, escaping or dot-prefixed.
    #[error("invalid import destination: {0}")]
    ImportDestination(String),

    /// An import would overwrite an existing file. Separate from
    /// [`Error::AddTargetExists`] because the file may not be a note.
    #[error("file already exists: {0}")]
    ImportTargetExists(String),

    /// `create_dir` was given an invalid folder path.
    #[error("invalid folder path: {0}")]
    DirDestination(String),

    /// `create_dir` would land on an existing file or folder.
    #[error("folder already exists: {0}")]
    DirTargetExists(String),

    /// A save's `base_revision` no longer matches the file: it changed on disk since it
    /// was read. The path is for debug detail only.
    #[error("write conflict: {0} changed on disk since it was read")]
    WriteConflict(String),

    /// `b2 link` was given a `--type` that is not a core verb (data-model.md §2), so a
    /// typo is caught rather than stored.
    #[error("not a core relation verb: {0}")]
    InvalidRelation(String),

    /// The index was written by a newer `b2` (schema above [`crate::SCHEMA_VERSION`]).
    /// Refused rather than dropped (ADR-0002); the fix is the newer `b2`.
    #[error("index schema {found} was written by a newer b2 (this build reads {supported}); run the newer b2")]
    IndexTooNew { found: i64, supported: i64 },

    /// A resource path did not resolve to an inventoried resource.
    #[error("resource not found: {0}")]
    ResourceNotFound(String),

    /// The operation works on notes but not yet on resources (e.g. `b2 similar`), so
    /// adapters say "not yet" rather than "no such file".
    #[error("not supported for resources yet: {0}")]
    ResourceUnsupported(String),
}

pub type Result<T> = std::result::Result<T, Error>;
