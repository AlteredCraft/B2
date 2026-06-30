//! The suggestion lifecycle (planning/data-model.md §4): generate → list →
//! reject, with the durable record in the `.b2/` log and the live queue in the
//! index. Suggestions are **inert** — generating one writes the log + the index
//! queue, never a note on disk. Acceptance (the Markdown write, Flow ③) is a
//! later slice; replay's handling of accepted is in [`crate::replay`].

use crate::db;
use crate::error::Result;
use crate::event::{Event, EventSink};
use crate::id::IdGen;
use rusqlite::{Connection, OptionalExtension};

/// A pending suggestion as shown by `b2 suggest` — the typed edge plus its full
/// decision fuel (data-model.md §4).
#[derive(Debug, Clone, PartialEq)]
pub struct Suggestion {
    pub edge_id: String,
    pub src_id: String,
    pub dst_id: Option<String>,
    pub dst_path_raw: String,
    pub edge_type: String,
    pub explanation: Option<String>,
    pub by: String,
    pub source: Option<String>,
    pub confidence: Option<f64>,
    pub created: String,
}

/// Propose a typed edge `src_id --type--> dst_id`. Appends `suggestion.generated`
/// to the log (durable) then projects the queue row + provenance (disposable).
/// Returns the new edge id, or `None` if the connection already exists in any
/// status — active, pending, or rejected — so it is never re-proposed.
///
/// `created` is supplied by the caller (an ISO-8601 timestamp), keeping this a
/// deterministic, testable function rather than reaching for a wall clock.
#[allow(clippy::too_many_arguments)]
pub fn generate_suggestion(
    conn: &Connection,
    sink: &dyn EventSink,
    idgen: &dyn IdGen,
    src_id: &str,
    dst_id: &str,
    edge_type: &str,
    explanation: Option<&str>,
    by: &str,
    source: Option<&str>,
    confidence: Option<f64>,
    created: &str,
) -> Result<Option<String>> {
    if db::edge_exists(conn, src_id, dst_id, edge_type)? {
        return Ok(None);
    }

    let edge_id = idgen.new_id();
    // dst_path_raw snapshots the target's current path (without the `.md` Obsidian
    // omits) — what the inline link would use if accepted.
    let dst_path_raw = match db::resolve_b2id_to_path(conn, dst_id)? {
        Some(p) => p.strip_suffix(".md").unwrap_or(&p).to_string(),
        None => String::new(),
    };

    // Log first (durable), then project the live queue (disposable).
    sink.append(&Event::SuggestionGenerated {
        edge_id: edge_id.clone(),
        src_id: src_id.to_string(),
        dst_id: dst_id.to_string(),
        dst_path_raw: dst_path_raw.clone(),
        edge_type: edge_type.to_string(),
        explanation: explanation.map(str::to_string),
        by: by.to_string(),
        source: source.map(str::to_string),
        confidence,
        created: created.to_string(),
    })?;
    db::insert_suggested_edge(
        conn,
        &edge_id,
        src_id,
        Some(dst_id),
        &dst_path_raw,
        edge_type,
        explanation,
    )?;
    db::insert_edge_provenance(conn, &edge_id, by, source, confidence, created, None)?;

    Ok(Some(edge_id))
}

/// Reject a pending suggestion: append `suggestion.rejected` and tombstone the
/// edge so the same pair+type is never proposed again.
pub fn reject_suggestion(
    conn: &Connection,
    sink: &dyn EventSink,
    edge_id: &str,
    decided: &str,
) -> Result<()> {
    sink.append(&Event::SuggestionRejected {
        edge_id: edge_id.to_string(),
        decided: decided.to_string(),
    })?;
    db::mark_edge_rejected(conn, edge_id, decided)?;
    Ok(())
}

/// The live review queue: all `status='suggested'` edges with their provenance,
/// newest-evidence-first is left to the caller (ordered by `created`).
pub fn list_suggestions(conn: &Connection) -> Result<Vec<Suggestion>> {
    let mut stmt = conn.prepare(
        "SELECT e.id, e.src_id, e.dst_id, e.dst_path_raw, e.type, e.explanation,
                p.by, p.source, p.confidence, p.created
         FROM edges e JOIN edge_provenance p ON p.edge_id = e.id
         WHERE e.status = 'suggested'
         ORDER BY p.created, e.id",
    )?;
    let rows = stmt.query_map([], |r| {
        Ok(Suggestion {
            edge_id: r.get(0)?,
            src_id: r.get(1)?,
            dst_id: r.get(2)?,
            dst_path_raw: r.get(3)?,
            edge_type: r.get(4)?,
            explanation: r.get(5)?,
            by: r.get(6)?,
            source: r.get(7)?,
            confidence: r.get(8)?,
            created: r.get(9)?,
        })
    })?;
    Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
}

/// Look up one suggestion by edge id (None if absent / not pending).
pub fn get_suggestion(conn: &Connection, edge_id: &str) -> Result<Option<Suggestion>> {
    let mut stmt = conn.prepare(
        "SELECT e.id, e.src_id, e.dst_id, e.dst_path_raw, e.type, e.explanation,
                p.by, p.source, p.confidence, p.created
         FROM edges e JOIN edge_provenance p ON p.edge_id = e.id
         WHERE e.id = ?1 AND e.status = 'suggested'",
    )?;
    Ok(stmt
        .query_row([edge_id], |r| {
            Ok(Suggestion {
                edge_id: r.get(0)?,
                src_id: r.get(1)?,
                dst_id: r.get(2)?,
                dst_path_raw: r.get(3)?,
                edge_type: r.get(4)?,
                explanation: r.get(5)?,
                by: r.get(6)?,
                source: r.get(7)?,
                confidence: r.get(8)?,
                created: r.get(9)?,
            })
        })
        .optional()?)
}
