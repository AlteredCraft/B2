//! B2 index engine (`b2.sqlite`) — a **disposable** projection of `(Markdown ∪ log)`.
//!
//! Built step 0→5 per `planning/specs/index-engine-build.md`; the schema is a
//! derived projection of `planning/data-model.md` and must satisfy it, never the
//! reverse. Step 0 is the substrate: open the DB with the locked pragmas and prove
//! FTS5 (BM25) and `sqlite-vec` (KNN) coexist in one statically-linked connection.

pub mod chunk;
pub mod db;
pub mod embed;
mod error;
pub mod event;
pub mod graph;
pub mod id;
pub mod ingest;
pub mod link;
pub mod note;
pub mod relate;
pub mod relation;
pub mod replay;
pub mod search;
pub mod suggest;
pub mod vault;

pub use db::{open, SCHEMA_VERSION};
pub use error::{Error, Result};
pub use vault::Vault;
