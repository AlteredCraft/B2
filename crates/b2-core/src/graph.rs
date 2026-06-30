//! Graph queries over the typed `edges` table. Inversion (backlinks) is the
//! reason the graph is materialized rather than parsed at read time
//! (planning/index-engine.md §3): a note's inbound edges live in every *other*
//! note, so `neighbors` is one indexed lookup, not a full-vault scan.

use crate::error::Result;
use crate::relation;
use rusqlite::Connection;
use std::collections::HashSet;

/// Which way an edge points relative to the note being asked about.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Direction {
    Outbound,
    Inbound,
}

/// One neighbor of a note: the note at the other end of an active edge, plus the
/// display label (the verb for outbound, the inverse for inbound).
#[derive(Debug, Clone)]
pub struct Neighbor {
    /// The `b2id` at the other end of the edge.
    pub other: String,
    /// The stored relation verb.
    pub edge_type: String,
    pub direction: Direction,
    /// Display label: the verb itself outbound, the inverse label inbound
    /// (data-model.md §2). Symmetric verbs read the same both ways.
    pub label: String,
    pub explanation: Option<String>,
}

/// All active neighbors of `b2id` — outbound edges (this note → others) then
/// inbound edges (others → this note), each labeled for display. Suggested and
/// rejected edges are excluded (`status='active'` only).
pub fn neighbors(conn: &Connection, b2id: &str) -> Result<Vec<Neighbor>> {
    let mut out = Vec::new();

    let mut stmt = conn.prepare(
        "SELECT dst_id, type, explanation FROM edges
         WHERE src_id = ?1 AND status = 'active' AND dst_id IS NOT NULL
         ORDER BY type, dst_id",
    )?;
    let rows = stmt.query_map([b2id], |r| {
        Ok((
            r.get::<_, String>(0)?,
            r.get::<_, String>(1)?,
            r.get::<_, Option<String>>(2)?,
        ))
    })?;
    for row in rows {
        let (other, edge_type, explanation) = row?;
        let label = edge_type.clone();
        out.push(Neighbor {
            other,
            edge_type,
            direction: Direction::Outbound,
            label,
            explanation,
        });
    }

    let mut stmt = conn.prepare(
        "SELECT src_id, type, explanation FROM edges
         WHERE dst_id = ?1 AND status = 'active'
         ORDER BY type, src_id",
    )?;
    let rows = stmt.query_map([b2id], |r| {
        Ok((
            r.get::<_, String>(0)?,
            r.get::<_, String>(1)?,
            r.get::<_, Option<String>>(2)?,
        ))
    })?;
    for row in rows {
        let (other, edge_type, explanation) = row?;
        let label = relation::inverse_label(&edge_type).to_string();
        out.push(Neighbor {
            other,
            edge_type,
            direction: Direction::Inbound,
            label,
            explanation,
        });
    }

    Ok(out)
}

/// The set of notes within `hops` typed hops of `anchor` (inclusive of `anchor`),
/// traversing `active` edges **undirected** — a note related to the anchor either
/// way is reachable. This is the hop set the graph-filtered discovery join uses
/// (index-engine.md §3). `hops = 0` is just the anchor.
pub fn reachable_within(conn: &Connection, anchor: &str, hops: usize) -> Result<HashSet<String>> {
    let mut seen = HashSet::from([anchor.to_string()]);
    let mut frontier = vec![anchor.to_string()];
    for _ in 0..hops {
        let mut next = Vec::new();
        for node in &frontier {
            for nb in neighbors(conn, node)? {
                if seen.insert(nb.other.clone()) {
                    next.push(nb.other);
                }
            }
        }
        if next.is_empty() {
            break;
        }
        frontier = next;
    }
    Ok(seen)
}
