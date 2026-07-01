//! The relator seam (planning/vision-and-scope.md "Connection discovery v1"; the
//! "build for tomorrow's model" tenet). Connection discovery is a three-stage
//! pipeline — **candidate generation → typed, explained suggestion → review loop**
//! — and only the middle stage needs a model. Candidate generation is deterministic
//! ([`crate::search::graph_filtered_search`] and kin); the review loop is
//! deterministic ([`crate::suggest`]). Classifying a candidate pair into a *typed,
//! explained* connection — or declining it — is the one judgment that needs an LLM,
//! so it sits behind this trait exactly as embeddings sit behind [`crate::embed::Embedder`].
//!
//! The engine is built and tested against a deterministic [`FakeRelator`]; a real
//! LLM-backed relator drops in through this same seam later, in its own crate, so
//! `b2-core` stays model-free (the `LocalEmbedder` / `b2-embed` precedent). What the
//! relator produces maps straight onto [`crate::suggest::generate_suggestion`]: it
//! owns `edge_type`, `explanation`, and `confidence`, and names itself via
//! [`model_id`](Relator::model_id) (recorded in provenance `by` as `agent:<id>`).
//! Candidate generation owns `src_id`/`dst_id`/`source`; the caller owns `created`.

use crate::error::Result;
use crate::relation;

/// The read-only view of a note the relator judges against: enough to classify and
/// explain a connection, no more. Borrowed so the caller can hand rows straight
/// from the index without cloning.
#[derive(Debug, Clone, Copy)]
pub struct NoteCtx<'a> {
    pub b2id: &'a str,
    pub title: Option<&'a str>,
    /// The note body, or a representative slice of it.
    pub text: &'a str,
}

/// One candidate connection surfaced by candidate generation, with the evidence
/// that surfaced it — the relator's input alongside the anchor.
#[derive(Debug, Clone, Copy)]
pub struct Candidate<'a> {
    pub note: NoteCtx<'a>,
    /// The specific chunk of `note` that matched (the vector⨝graph hit).
    pub evidence_chunk: &'a str,
    /// How this candidate surfaced (e.g. `"semantic+graph:2h"`). Free-form; flows
    /// through to the suggestion's provenance `source` (data-model.md §4).
    pub signal: &'a str,
    /// The candidate-generation score (RRF / similarity); a prior for the relator.
    pub score: f64,
}

/// A relator's verdict when it fires: a typed, explained connection to propose.
/// Absence (`Ok(None)` from [`relate`](Relator::relate)) is the deliberate decline.
#[derive(Debug, Clone, PartialEq)]
pub struct Proposal {
    /// The relation verb. **Must** be a core verb (data-model.md §2 closed set) —
    /// discovery never emits tail verbs. [`FakeRelator`] upholds this by drawing
    /// only from [`relation::CORE`]; the pipeline validates a real relator's output
    /// against [`relation::is_core`] before persisting.
    pub edge_type: String,
    /// The "why" — lands verbatim in the suggestion and, on accept, in frontmatter.
    pub explanation: String,
    /// `0.0–1.0`, for triaging the review queue. **Not** the fire/decline gate: the
    /// relator itself decides that by returning `Some`/`None`.
    pub confidence: f64,
}

/// Classifies a candidate connection into a typed, explained [`Proposal`] — or
/// declines it. The precision gate of connection discovery: candidate generation
/// over-produces (every near neighbor is a candidate), and the relator is what
/// prunes non-connections down to the ones worth a human's review.
pub trait Relator {
    /// Stable identifier; recorded in provenance `by` as `agent:<model_id>`
    /// (data-model.md §4). A swap is observable there, like the embedder's.
    fn model_id(&self) -> &str;

    /// Judge one candidate against `anchor`. `Ok(None)` is a **decline** — no real
    /// typed connection here. `Ok(Some(p))` proposes `anchor --p.edge_type--> candidate`.
    ///
    /// Fallible for the same reason [`crate::embed::Embedder::embed`] is: the fake
    /// never fails, but a real model runs inference that can, and the discovery path
    /// must surface that rather than panic. Batching a whole anchor's candidates into
    /// one call is a later optimization that can live behind this same seam (a
    /// batched method with a pairwise default) — deferred per the Bitter-Lesson tenet.
    fn relate(&self, anchor: &NoteCtx, candidate: &Candidate) -> Result<Option<Proposal>>;
}

/// Deterministic, content-addressed relator for tests/dev — the [`FakeEmbedder`]
/// analog (testability stack, point 4). The verdict for a pair is a pure function
/// of the two `b2id`s, so a discovery run is reproducible and drop-&-rebuild yields
/// the same suggestions. It is **not** a real judgment: it hashes the pair to pick a
/// core verb, a confidence, and a decline bucket — it never reads the note text — so
/// its explanation says so and nothing here should be mistaken for semantics.
///
/// [`FakeEmbedder`]: crate::embed::FakeEmbedder
#[derive(Debug, Clone, Copy, Default)]
pub struct FakeRelator;

impl FakeRelator {
    pub fn new() -> Self {
        Self
    }
}

impl Relator for FakeRelator {
    fn model_id(&self) -> &str {
        "fake-relator-v1"
    }

    fn relate(&self, anchor: &NoteCtx, candidate: &Candidate) -> Result<Option<Proposal>> {
        // Content-address on the ordered pair of b2ids: same pair → same verdict,
        // independent of text edits or candidate-gen scoring, so pipeline tests are
        // reproducible. blake3 (already a dep, via FakeEmbedder) as a stable PRF.
        let mut hasher = blake3::Hasher::new();
        hasher.update(anchor.b2id.as_bytes());
        hasher.update(b"\0");
        hasher.update(candidate.note.b2id.as_bytes());
        let digest = hasher.finalize();
        let bytes = digest.as_bytes();

        // Decline one bucket in four — exercises the prune path deterministically so
        // tests can assert both "fires" and "declines" without a real model.
        if bytes[0].is_multiple_of(4) {
            return Ok(None);
        }

        let verb = relation::CORE[bytes[1] as usize % relation::CORE.len()].verb;
        // Confidence in [0.5, 1.0]: a fired stub reads as plausibly-confident without
        // ever claiming certainty.
        let confidence = 0.5 + (bytes[2] as f64 / 255.0) * 0.5;

        Ok(Some(Proposal {
            edge_type: verb.to_string(),
            explanation: format!(
                "deterministic stub: `{verb}` chosen by hashing the note pair — not a real semantic judgment"
            ),
            confidence,
        }))
    }
}
