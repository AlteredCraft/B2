//! B2 index engine (`b2.sqlite`) — a **disposable** projection of Markdown (ADR-0002).
//! The schema is derived from `data-model.md` and must satisfy it, never the reverse.
//! SQLite is bundled and statically linked with FTS5 compiled in; vectors are plain BLOB
//! tables scored in-process, so no extension is needed (ADR-0006, ADR-0019).

pub mod add;
pub mod chat;
pub mod chunk;
pub mod db;
pub mod dirs;
pub mod discover;
pub mod embed;
mod error;
pub mod graph;
pub mod import;
pub mod ingest;
pub mod link;
pub mod llm;
pub mod mv;
pub mod note;
mod pathspec;
pub mod relation;
pub mod resource;
pub mod rm;
pub mod search;
pub mod vault;

pub use db::{open, SCHEMA_VERSION};
pub use error::{Error, Result};
pub use vault::Vault;
