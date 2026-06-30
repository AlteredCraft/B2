//! Replay: rebuild the review state from the event log
//! (planning/data-model.md §4, build spec Flow ⑤). The pending queue and
//! rejection tombstones are the one part of the index not derivable from
//! Markdown, so replaying the log is what makes `index = projection of
//! (Markdown ∪ log)` literally true: drop `b2.sqlite`, re-scan the vault (Flow ①),
//! replay → a byte-identical index. `replay(log) ⇒ review state` is a pure
//! function — a clean deterministic seam for tests.
//!
//! Contract: replay assumes the review-state tables start empty (a fresh rebuild),
//! and that the Markdown-derived `active` edges are already projected (so an
//! accepted suggestion's active edge exists from Flow ①, and replay only removes
//! the queue row).

use crate::db;
use crate::error::Result;
use crate::event::{Event, EventSink};
use rusqlite::Connection;

/// Read the whole log and apply it to `conn`, in append order.
pub fn replay_log(conn: &Connection, sink: &dyn EventSink) -> Result<()> {
    for event in sink.read_all()? {
        apply_event(conn, &event)?;
    }
    Ok(())
}

/// Apply a single event to the index's review state. Only the `suggestion.*`
/// events touch queryable state; the rest are history only.
pub fn apply_event(conn: &Connection, event: &Event) -> Result<()> {
    match event {
        Event::SuggestionGenerated {
            edge_id,
            src_id,
            dst_id,
            dst_path_raw,
            edge_type,
            explanation,
            by,
            source,
            confidence,
            created,
        } => {
            // INSERT OR IGNORE: if this suggestion was later accepted, Flow ① has
            // already materialized its edge from frontmatter, so the generated
            // event is absorbed (no row inserted) → skip provenance to avoid a
            // dangling FK. The accepted event below is then a no-op delete.
            let inserted = db::insert_suggested_edge(
                conn,
                edge_id,
                src_id,
                Some(dst_id),
                dst_path_raw,
                edge_type,
                explanation.as_deref(),
            )?;
            if inserted {
                db::insert_edge_provenance(
                    conn,
                    edge_id,
                    by,
                    source.as_deref(),
                    *confidence,
                    created,
                    None,
                )?;
            }
        }
        Event::SuggestionRejected { edge_id, decided } => {
            db::mark_edge_rejected(conn, edge_id, decided)?;
        }
        Event::SuggestionAccepted { edge_id, .. } => {
            // The suggestion left the queue; its active edge re-derives from
            // Markdown (Flow ①), so it is never double-counted (data-model.md §4).
            db::delete_edge(conn, edge_id)?;
        }
        // History only — not replayed into queryable state.
        Event::B2idStamped { .. } => {}
    }
    Ok(())
}
