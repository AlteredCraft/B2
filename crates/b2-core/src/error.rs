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
}

pub type Result<T> = std::result::Result<T, Error>;
