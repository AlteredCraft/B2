//! Connection-discovery candidate generation (planning/tasks.md ①, resolved
//! 2026-07-01; vision-and-scope "Connection discovery v1"). This is the first stage
//! of the three-stage pipeline — **candidate generation → [`Relator`] → review
//! loop** — and the only one that reads the graph.
//!
//! A candidate is a note **semantically near the anchor but not already connected**:
//! the *complement* of the graph, not the intersection. (The intersection —
//! semantic-nearest chunks *within* k hops — is [`crate::search::graph_filtered_search`],
//! a scoped-traversal primitive, the wrong tool here.) Generation is deliberately
//! **permissive**: it over-produces, and the [`Relator`](crate::relate::Relator) plus
//! the human are the precision gate ([`crate::suggest`]).
//!
//! Mechanics (tasks.md ①): for each of the anchor's **stored** chunk vectors, KNN the
//! vault, then score every other note by its **best** chunk-pair similarity
//! (max-sim), and finally subtract the anchor and its **direct (1-hop)** neighbors.
//! It is vector-only and **re-embeds nothing** — discovery is passage↔passage, so the
//! anchor is represented by the vectors already in `chunks_vec`, never by an
//! `embed_query` of its text (bge's asymmetric query prefix is the wrong side).
//! Graph distance beyond the 1-hop exclusion is **not** a ranking signal — graph-
//! distant "bridge" candidates ride along unboosted; weighting distance (closure vs.
//! serendipity) is a deferred, eval-gated experiment (tasks.md backlog).

use crate::db;
use crate::error::Result;
use crate::event::EventSink;
use crate::graph;
use crate::id::IdGen;
use crate::relate::{Candidate, NoteCtx, Relator};
use crate::relation;
use crate::suggest;
use rusqlite::Connection;
use std::collections::HashMap;

/// The exclusion radius: a candidate must not already be *directly* linked to the
/// anchor. Fixed at 1 by decision (tasks.md ①) so triadic-closure candidates — a note
/// two hops away, transitively related but with no direct edge — stay in the pool.
const EXCLUDE_HOPS: usize = 1;

/// One discovery candidate: a note near the anchor and not already connected, ranked
/// by `score`. Owned so the caller can build the borrowed
/// [`Candidate`](crate::relate::Candidate) view at relate-time without threading a
/// lifetime through generation.
#[derive(Debug, Clone, PartialEq)]
pub struct CandidateNote {
    /// The candidate note's `b2id`.
    pub note_b2id: String,
    /// Best chunk-pair similarity across the anchor's chunks × this note's chunks —
    /// higher is nearer (negated `sqlite-vec` distance, matching
    /// [`Hit`](crate::search::Hit)).
    pub score: f64,
    /// The candidate's chunk that achieved `score` — the evidence a
    /// [`Relator`](crate::relate::Relator) reads and the suggestion's provenance
    /// points at.
    pub evidence_chunk_id: i64,
}

/// Generate up to `limit` connection-discovery candidates for `anchor`, best score
/// first (ties broken by `note_b2id` for determinism).
///
/// Returns empty when the vault has no embedding space yet, when the anchor has no
/// chunks (unknown or empty note), or when `limit` is 0 — there is nothing to search
/// from. Excludes the anchor itself and its direct neighbors; everything else near in
/// vector space is a candidate.
pub fn candidates(conn: &Connection, anchor: &str, limit: usize) -> Result<Vec<CandidateNote>> {
    if limit == 0 || !db::embedding_space_exists(conn)? {
        return Ok(Vec::new());
    }
    let anchor_chunks = db::chunks_for_note(conn, anchor)?;
    if anchor_chunks.is_empty() {
        return Ok(Vec::new());
    }

    // The only use of the graph in generation: subtract what's already linked — the
    // anchor and everything within 1 hop (self + direct neighbors).
    let exclude = graph::reachable_within(conn, anchor, EXCLUDE_HOPS)?;

    // A full KNN pool per anchor chunk keeps max-sim exact at vault scale — the same
    // brute-force pool graph_filtered_search uses. The scale lever (a note-partition
    // column on chunks_vec for filtered KNN) is deferred (build spec §1.2 / §4).
    let pool = db::chunk_count(conn)?.max(1) as usize;

    // note_b2id → (best score so far, the chunk of that note which achieved it).
    let mut best: HashMap<String, (f64, i64)> = HashMap::new();
    for anchor_chunk in anchor_chunks {
        let Some(vector) = db::chunk_vector(conn, anchor_chunk)? else {
            continue; // a chunk with no stored vector (shouldn't occur post-embed)
        };
        for (hit_chunk, distance) in db::vector_search(conn, &vector, pool)? {
            let Some(note) = db::note_for_chunk(conn, hit_chunk)? else {
                continue;
            };
            if exclude.contains(&note) {
                continue; // the anchor or a direct neighbor — already connected
            }
            let score = -(distance as f64); // nearer = higher, matching Hit
            best.entry(note)
                .and_modify(|cur| {
                    if score > cur.0 {
                        *cur = (score, hit_chunk);
                    }
                })
                .or_insert((score, hit_chunk));
        }
    }

    let mut out: Vec<CandidateNote> = best
        .into_iter()
        .map(|(note_b2id, (score, evidence_chunk_id))| CandidateNote {
            note_b2id,
            score,
            evidence_chunk_id,
        })
        .collect();
    // Best score first; ties broken by id so the ranking (and thus `limit`'s prefix)
    // is deterministic.
    out.sort_by(|a, b| {
        b.score
            .partial_cmp(&a.score)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then(a.note_b2id.cmp(&b.note_b2id))
    });
    out.truncate(limit);
    Ok(out)
}

// ---------------------------------------------------------------------------
// Stage ② — wire candidate generation → the Relator → the suggestion queue
// (planning/tasks.md "Wire the generate pipeline"). Deterministic like the rest
// of the core: `created` and ids are passed in, notes are anchored in sorted
// b2id order, so a run is reproducible and idempotent.
// ---------------------------------------------------------------------------

/// The provenance tag stamped on every candidate this stage surfaces — it flows to
/// the suggestion's `source` (data-model.md §4). Honest about the mechanism:
/// passage↔passage max-similarity ([`candidates`]), with graph distance used only
/// as an exclusion, never as a boost (① resolved 2026-07-01).
const SIGNAL: &str = "semantic:maxsim";

/// A tally of one discovery run. Every candidate the relator saw lands in exactly
/// one bucket, so `generated + declined + non_core + existing` is the number of
/// pairs considered — the accounting the CLI reports and the tests assert.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct GenerateOutcome {
    /// New suggestions written to the queue + log.
    pub generated: usize,
    /// Relator declines (`Ok(None)`) — the prune path.
    pub declined: usize,
    /// Proposals dropped because the verb was not core (a real relator's output is
    /// validated against [`relation::is_core`], never trusted blindly — tasks.md ②).
    pub non_core: usize,
    /// Candidates whose `(src, dst, type)` already exists in any status, so
    /// re-proposal is refused — what makes a re-run idempotent.
    pub existing: usize,
}

impl GenerateOutcome {
    fn merge(&mut self, other: &GenerateOutcome) {
        self.generated += other.generated;
        self.declined += other.declined;
        self.non_core += other.non_core;
        self.existing += other.existing;
    }
}

/// Run the discovery pipeline for a single `anchor`: generate candidates
/// ([`candidates`]), judge each with `relator` (the precision gate), and persist
/// every fired-and-core proposal as a suggestion via
/// [`generate_suggestion`](crate::suggest::generate_suggestion) (Flow ③ generate).
/// This is the glue that finally turns the three built pieces — candidate
/// generation, the [`Relator`] seam, and the suggestion lifecycle — into
/// end-to-end connection discovery (tasks.md ②).
///
/// Deterministic: `created` (an ISO-8601 timestamp) and ids (`idgen`) are passed
/// in, and candidates are already ranked, so a given `idgen` mints the same ids
/// across runs. Idempotent: `generate_suggestion` refuses any pair already present
/// in any status (active, pending, or rejected), so re-running proposes nothing new
/// (counted in [`GenerateOutcome::existing`]).
pub fn generate_for_anchor(
    conn: &Connection,
    sink: &dyn EventSink,
    idgen: &dyn IdGen,
    relator: &dyn Relator,
    anchor: &str,
    top_n: usize,
    created: &str,
) -> Result<GenerateOutcome> {
    let cands = candidates(conn, anchor, top_n)?;
    let mut outcome = GenerateOutcome::default();
    if cands.is_empty() {
        return Ok(outcome);
    }

    // The anchor's context is assembled once and reused across its candidates. Its
    // `text` is the note's chunks joined (`db::note_text`) — the body as the index
    // holds it; a real relator reads it, `FakeRelator` ignores it.
    let anchor_title = db::note_title(conn, anchor)?;
    let anchor_text = db::note_text(conn, anchor)?;
    let anchor_ctx = NoteCtx {
        b2id: anchor,
        title: anchor_title.as_deref(),
        text: &anchor_text,
    };
    let by = format!("agent:{}", relator.model_id());

    for cand in cands {
        // Pre-call dedup — the cost gate. If this directed pair already has an edge (a
        // pending suggestion, or a rejection tombstone; active pairs never reach here,
        // candidate generation already excludes connected notes), it is already
        // *decided* — skip it **without** calling the relator, so a re-run doesn't
        // re-pay for settled pairs. This makes `suggest` idempotent in *cost*, not just
        // in effect ([`generate_suggestion`]'s per-type guard stays as a backstop).
        //
        // Note this is deliberately *pair-level* (any type): once a pair is pending or
        // rejected, discovery won't re-ask the model about it under a different verb —
        // a small strengthening of the per-(pair,type) tombstone (data-model.md §4),
        // required because the type isn't known until *after* the call we're avoiding.
        // *Declines* leave no edge, so they are not covered here — skipping unchanged
        // anchors wholesale (a `body_hash` gate) is the follow-up that closes that gap
        // (see tasks.md).
        if db::edge_exists_for_pair(conn, anchor, &cand.note_b2id)? {
            outcome.existing += 1;
            continue;
        }
        let cand_title = db::note_title(conn, &cand.note_b2id)?;
        let cand_text = db::note_text(conn, &cand.note_b2id)?;
        let evidence = db::chunk_text(conn, cand.evidence_chunk_id)?.unwrap_or_default();
        let candidate = Candidate {
            note: NoteCtx {
                b2id: &cand.note_b2id,
                title: cand_title.as_deref(),
                text: &cand_text,
            },
            evidence_chunk: &evidence,
            signal: SIGNAL,
            score: cand.score,
        };

        let Some(proposal) = relator.relate(&anchor_ctx, &candidate)? else {
            outcome.declined += 1; // an explicit decline — the relator pruned it
            continue;
        };
        // A real relator's verb is validated, not trusted: discovery only persists
        // a core verb (data-model.md §2). The gate deferred from the seam slice.
        if !relation::is_core(&proposal.edge_type) {
            outcome.non_core += 1;
            continue;
        }
        match suggest::generate_suggestion(
            conn,
            sink,
            idgen,
            anchor,
            &cand.note_b2id,
            &proposal.edge_type,
            Some(proposal.explanation.as_str()),
            &by,
            Some(SIGNAL),
            Some(proposal.confidence),
            created,
        )? {
            Some(_) => outcome.generated += 1,
            None => outcome.existing += 1, // already proposed/active/rejected
        }
    }
    Ok(outcome)
}

/// Live progress for a suggestion run — fired once per anchor as [`generate_all`]
/// works through the vault, so a long, network-bound run (one relator call per
/// candidate pair) shows a moving line instead of looking frozen. The discovery
/// analog of [`ingest::ReindexProgress`](crate::ingest::ReindexProgress); counts are
/// cumulative across the run.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SuggestProgress {
    /// 1-based count of anchors (notes) processed so far.
    pub anchor_index: usize,
    /// Total anchors in the run.
    pub anchors_total: usize,
    /// Cumulative **relator calls made** so far — the pairs that actually reached the
    /// model (`generated + declined + non_core`). Pairs skipped by pre-call dedup
    /// (`existing`) cost no call and are **not** counted here, so this tracks spend.
    pub calls: usize,
    /// Cumulative suggestions written so far.
    pub generated: usize,
}

/// Run [`generate_for_anchor`] over the whole vault, every note an anchor in sorted
/// `b2id` order ([`db::all_note_ids`]) so the sequence of minted ids — and thus the
/// review queue — is reproducible (tasks.md ②). Returns the summed
/// [`GenerateOutcome`].
pub fn generate_all(
    conn: &Connection,
    sink: &dyn EventSink,
    idgen: &dyn IdGen,
    relator: &dyn Relator,
    top_n: usize,
    created: &str,
) -> Result<GenerateOutcome> {
    generate_all_with_progress(conn, sink, idgen, relator, top_n, created, &mut |_| {})
}

/// [`generate_all`] with a progress callback fired after each anchor — the CLI
/// renders it as a live line on an interactive stderr. Determinism is unchanged: the
/// callback only observes cumulative counts, it never influences the run.
pub fn generate_all_with_progress(
    conn: &Connection,
    sink: &dyn EventSink,
    idgen: &dyn IdGen,
    relator: &dyn Relator,
    top_n: usize,
    created: &str,
    on_progress: &mut dyn FnMut(SuggestProgress),
) -> Result<GenerateOutcome> {
    let anchors = db::all_note_ids(conn)?;
    let anchors_total = anchors.len();
    let mut total = GenerateOutcome::default();
    for (i, anchor) in anchors.iter().enumerate() {
        let one = generate_for_anchor(conn, sink, idgen, relator, anchor, top_n, created)?;
        total.merge(&one);
        on_progress(SuggestProgress {
            anchor_index: i + 1,
            anchors_total,
            // Actual relator calls — excludes pre-call-deduped `existing` pairs.
            calls: total.generated + total.declined + total.non_core,
            generated: total.generated,
        });
    }
    Ok(total)
}
