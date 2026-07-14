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
    /// Edge provenance — `inline` (human body link), `frontmatter` (a relation B2
    /// accepted, or a human/importer authored) (data-model.md §0). Only `active`
    /// edges are returned, so `suggested` never appears here. `b2 explain` surfaces
    /// this so a human body link reads distinctly from a B2-committed one.
    pub origin: String,
}

/// All neighbors of `b2id` — outbound edges (this note → others) then inbound edges
/// (others → this note), each labeled for display. Every edge is authored and active
/// (there is no suggestion lifecycle), so this is the note's full typed graph.
pub fn neighbors(conn: &Connection, b2id: &str) -> Result<Vec<Neighbor>> {
    let mut out = Vec::new();

    let mut stmt = conn.prepare(
        "SELECT dst_id, type, explanation, origin FROM edges
         WHERE src_id = ?1 AND dst_id IS NOT NULL
         ORDER BY type, dst_id",
    )?;
    let rows = stmt.query_map([b2id], |r| {
        Ok((
            r.get::<_, String>(0)?,
            r.get::<_, String>(1)?,
            r.get::<_, Option<String>>(2)?,
            r.get::<_, String>(3)?,
        ))
    })?;
    for row in rows {
        let (other, edge_type, explanation, origin) = row?;
        let label = edge_type.clone();
        out.push(Neighbor {
            other,
            edge_type,
            direction: Direction::Outbound,
            label,
            explanation,
            origin,
        });
    }

    let mut stmt = conn.prepare(
        "SELECT src_id, type, explanation, origin FROM edges
         WHERE dst_id = ?1
         ORDER BY type, src_id",
    )?;
    let rows = stmt.query_map([b2id], |r| {
        Ok((
            r.get::<_, String>(0)?,
            r.get::<_, String>(1)?,
            r.get::<_, Option<String>>(2)?,
            r.get::<_, String>(3)?,
        ))
    })?;
    for row in rows {
        let (other, edge_type, explanation, origin) = row?;
        let label = relation::inverse_label(&edge_type).to_string();
        out.push(Neighbor {
            other,
            edge_type,
            direction: Direction::Inbound,
            label,
            explanation,
            origin,
        });
    }

    Ok(out)
}

/// One outbound link that resolved to **nothing** — neither a note nor a resource
/// exists at its target, so it is a *dangling* edge (`dst_id IS NULL AND
/// dst_resource_path IS NULL`). A note is one `.md` file (data-model.md §1), so a
/// `[[Hermes]]` that names a *folder* — or a plain typo — never resolves. These are
/// surfaced rather than silently dropped so a broken link is visible (GH #12).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Unresolved {
    /// The target exactly as authored (`dst_path_raw`) — e.g. `Hermes`.
    pub target: String,
    /// The relation verb (`references` for a bare link).
    pub edge_type: String,
    /// Edge origin — `inline` (a body link) or `frontmatter` (a `relations:` entry).
    pub origin: String,
    pub explanation: Option<String>,
}

/// A note's **dangling** outbound links: the edges it authored whose target resolves
/// to no note and no resource, in a deterministic order. The complement of
/// [`neighbors`]'s outbound half (which keeps only `dst_id IS NOT NULL`) — together
/// they cover every outbound edge, so a link is either a resolved neighbor or a
/// surfaced unresolved link, never silently gone (GH #12). Backed by the
/// `edges_dangling_idx` partial index, whose predicate this query mirrors.
pub fn unresolved_outbound(conn: &Connection, b2id: &str) -> Result<Vec<Unresolved>> {
    let mut stmt = conn.prepare(
        "SELECT dst_path_raw, type, origin, explanation FROM edges
         WHERE src_id = ?1 AND dst_id IS NULL AND dst_resource_path IS NULL
         ORDER BY type, dst_path_raw, occurrence_index",
    )?;
    let rows = stmt.query_map([b2id], |r| {
        Ok(Unresolved {
            target: r.get(0)?,
            edge_type: r.get(1)?,
            origin: r.get(2)?,
            explanation: r.get::<_, Option<String>>(3)?,
        })
    })?;
    Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
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
