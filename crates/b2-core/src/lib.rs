//! B2 index engine (`b2.sqlite`) — a **disposable** projection of `Markdown`.
//!
//! Built step 0→5 per `planning/specs/completed/index-engine-build.md`; the schema is a
//! derived projection of `planning/data-model.md` and must satisfy it, never the
//! reverse. Step 0 is the substrate: open the DB with the locked pragmas over the
//! bundled, statically-linked SQLite (FTS5 compiled in; vectors are plain BLOB
//! tables scored in-process since schema v3, #38 — no extension needed).

pub mod add;
pub mod chunk;
pub mod db;
pub mod discover;
pub mod embed;
mod error;
pub mod graph;
pub mod id;
pub mod ingest;
pub mod link;
pub mod mv;
pub mod note;
mod pathspec;
pub mod relation;
pub mod search;
pub mod vault;

pub use db::{open, SCHEMA_VERSION};
pub use error::{Error, Result};
pub use vault::Vault;
