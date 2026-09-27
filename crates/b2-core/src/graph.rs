//! Graph queries over the typed `edges` table. Materialized so backlinks are one indexed
//! lookup rather than a full-vault scan (index-engine.md §3).

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

/// One neighbor of a note: the other end of an edge, plus its display label.
#[derive(Debug, Clone)]
pub struct Neighbor {
    /// The note at the other end (L1).
    pub other: String,
    pub edge_type: String,
    pub direction: Direction,
    /// The verb outbound, its inverse label inbound (data-model.md §2).
    pub label: String,
    pub explanation: Option<String>,
    /// `inline` (a body link) or `frontmatter` (a `b2_relations:` entry) (data-model.md §0).
    pub origin: String,
}

/// All neighbors of `note_path`: outbound, then inbound.
pub fn neighbors(conn: &Connection, note_path: &str) -> Result<Vec<Neighbor>> {
    let mut out = collect_neighbors(
        conn,
        "SELECT dst_path, type, explanation, origin FROM edges
         WHERE src_path = ?1 AND dst_path IS NOT NULL
         ORDER BY type, dst_path",
        note_path,
        Direction::Outbound,
    )?;
    out.extend(collect_neighbors(
        conn,
        "SELECT src_path, type, explanation, origin FROM edges
         WHERE dst_path = ?1
         ORDER BY type, src_path",
        note_path,
        Direction::Inbound,
    )?);
    Ok(out)
}

/// One direction's half of [`neighbors`], from `(other, type, explanation, origin)` rows.
fn collect_neighbors(
    conn: &Connection,
    sql: &str,
    note_path: &str,
    direction: Direction,
) -> Result<Vec<Neighbor>> {
    let mut stmt = conn.prepare(sql)?;
    let rows = stmt.query_map([note_path], |r| {
        let edge_type: String = r.get(1)?;
        let label = match direction {
            Direction::Outbound => edge_type.clone(),
            Direction::Inbound => relation::inverse_label(&edge_type).to_string(),
        };
        Ok(Neighbor {
            other: r.get(0)?,
            edge_type,
            direction,
            label,
            explanation: r.get(2)?,
            origin: r.get(3)?,
        })
    })?;
    Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
}

/// A dangling outbound link: no note or resource at its target (a typo, or a folder;
/// data-model.md §1). Surfaced, not dropped (GH #12).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Unresolved {
    /// The target as authored (`dst_path_raw`).
    pub target: String,
    pub edge_type: String,
    /// `inline` or `frontmatter`.
    pub origin: String,
    pub explanation: Option<String>,
}

/// A note's dangling outbound links, in a deterministic order: the complement of
/// [`neighbors`]'s outbound half, so a link is never silently gone (GH #12). The query
/// mirrors `edges_dangling_idx`'s predicate.
pub fn unresolved_outbound(conn: &Connection, note_path: &str) -> Result<Vec<Unresolved>> {
    let mut stmt = conn.prepare(
        "SELECT dst_path_raw, type, origin, explanation FROM edges
         WHERE src_path = ?1 AND dst_path IS NULL AND dst_resource_path IS NULL
         ORDER BY type, dst_path_raw, occurrence_index",
    )?;
    let rows = stmt.query_map([note_path], |r| {
        Ok(Unresolved {
            target: r.get(0)?,
            edge_type: r.get(1)?,
            origin: r.get(2)?,
            explanation: r.get::<_, Option<String>>(3)?,
        })
    })?;
    Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
}

/// Notes within `hops` undirected hops of `anchor`, the anchor included.
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
