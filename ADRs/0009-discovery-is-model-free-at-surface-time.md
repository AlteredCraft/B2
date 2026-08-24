# ADR-0009 — Discovery surfaces candidates; the human commits the link

- **Status:** Accepted · 2026-07-12
- **Refs:** invariants G1, D1 · `index-engine.md` §3, §5 · GH #38

## Context

The obvious design is an inbox of machine-proposed links with accept/reject state. That makes the
index authoritative for something (ADR-0002), adds a lifecycle to edges, and asks the user to
maintain a queue.

## Decision

- **`b2 similar`** ranks the semantically nearest *unlinked* notes in **two stages**: a coarse
  O(notes) scan over `note_centroids` shortlists candidates, then exact max-sim over only that
  shortlist's chunk vectors, minus the anchor's 1-hop graph neighbours. **No model call at surface
  time.**
- **`b2 link`** (or, in the GUI, dragging a card onto a line to type a `[[wikilink]]`) is the human
  committing. A connection exists only once authored.
- **There is no suggestion queue**, no pending state, and nothing inert in the graph.

## Consequences

- The human is the precision gate, which is what lets discovery favour recall.
- Ranking quality is a surfacing question, not a storage one — see ADR-0014.
