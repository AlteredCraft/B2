//! Semantic-retrieval + discovery eval — the "separate, occasional pass" scoring
//! model quality out of CI (the eval harness, crates/b2-embed/evals/). It lives as an **example**,
//! not a test, so it never runs in the deterministic `cargo test` suite and model
//! quality can never flake CI (invariants.md). Run it on
//! demand:
//!
//! ```console
//! cargo run -p b2-embed --example eval               # score the configured model
//! cargo run -p b2-embed --example eval -- --sweep    # + chunker A/B (the #44 gate)
//! cargo run -p b2-embed --example eval -- --stemmer  # + FTS tokenizer A/B (the #157 gate)
//! ```
//!
//! One run builds a throwaway vault from the hand-labelled corpus in `evals/` and
//! scores four things through the real pipeline:
//!
//! 1. **BM25 baseline** — after `project` only (no vectors), every query is scored
//!    keyword-only. The labelled queries avoid their target's keywords, so this is
//!    the floor the model must clear.
//! 2. **Hybrid retrieval** — after `embed`, the same queries through BM25 ⊕ vector
//!    → RRF. The delta vs. the baseline is the **semantic lift** — the measured
//!    value of the one AI seam. A **vector-only ablation** rides beside it
//!    (`Vault::search_vector_only`, GH #158): the dense signal alone, so fusion
//!    has a measured single-signal baseline to answer to, and every query hybrid
//!    ranks *worse* than vector-only would is counted and named on the run.
//! 3. **Passage rank** — queries labelled with a verbatim `passage` are also
//!    scored at **chunk** level (`Vault::search_chunks`): note-rank is blind to
//!    sub-note retrieval, which is exactly what chunking levers move
//!    (index-engine.md, GH #44).
//! 4. **Discovery** — `evals/similar.json` anchors score `Vault::similar` (the
//!    centroid-shortlisted candidate generation, #38), which query-retrieval alone
//!    does not exercise. The surface **always serves the ranked list** (GH #197 —
//!    the per-anchor z existence gate is retired, so there is no floored/raw
//!    split any more; the harness keeps its two-pass structure with both passes
//!    on the one surface, as the tripwire that re-arms if a Phase-2 gate ever
//!    ships). Positive anchors score **rank** two ways: the historical
//!    per-anchor metric, which stops at the *first* mate it finds and therefore
//!    saturates — it has read 1.000 across changes it was meant to judge, so
//!    since GH #188 it is recorded in the row but no longer printed — and the
//!    per-mate companion added by GH #183, which scores every labelled mate
//!    separately so a hard one demoted behind an easy one is visible. That
//!    second one is what the printed line and the exit gate read. Beside it sits
//!    discovery's **precision** side (GH #188): the *strangers* a positive
//!    anchor serves — unlabelled notes on a list the labels do not claim —
//!    counted, named, and deliberately left ungated, since the cheapest way to
//!    shrink that count is to label the stranger. **Negative anchors** —
//!    deliberate loner notes whose labelled answer is "nothing relates" — are
//!    served their ranked nearest like every anchor under always-serve; what
//!    they measure now is *what those cards claim* (the bands, read in the
//!    calibration block — assumption A2 of GH #197) rather than suppression,
//!    and every surfaced candidate's score still lands in one of two **cosine
//!    piles** — labelled related vs. everything else surfaced — the measured
//!    distributions any future existence signal would be argued from.
//! 5. **The discovery z calibration** (GH #187) — a deep discovery pass dumps
//!    every candidate's stage-2 best-passage z (the strength band's input; it
//!    gates nothing since GH #197), split into the populations an existence
//!    bar would answer to: labelled mates, strangers on positive anchors, and
//!    negative anchors' leaders. From those it re-derives the admissible
//!    window a leader gate or member bar *would* have on the current corpus —
//!    the record of why none ships (the member window has been empty since
//!    #187 measured it; GH #196 showed the leader gate reading a dense vault
//!    dark), and the reading any Phase-2 bake-off candidate is judged
//!    against. This exists because the #150 windows were frozen into a rustdoc
//!    as if timeless and went stale the first time the corpus grew a shape
//!    they were never measured against: constants in code, measurements in the
//!    harness. The dump's z is also recomputed harness-side from the served
//!    scores, so a drift in the engine's statistic turns the block's check
//!    line into a `[FAULT]`.
//! 6. **The dense single-domain fixture** (GH #196/#197, Phase 0b) — a *second*
//!    corpus (`evals/corpus-dense/`, labels `evals/similar-dense.json`) scored in
//!    its **own throwaway vault** and recorded in its **own row** (corpus id
//!    `dense`), never averaged with the orthogonal corpus's. Fifteen notes, all
//!    genuinely inter-related, no loner: the vault-level geometry GH #196
//!    measured breaking every anchor-local existence gate (no unrelated tail ⇒
//!    every z compresses ⇒ every pane dark), which the orthogonal corpus is
//!    structurally incapable of expressing — mixing it in would perturb every
//!    existing anchor's population and dilute both instruments. Its labels are
//!    **rankings only** (expected mates per anchor, per-mate scored) and its
//!    assertions run the other way from the negatives the orthogonal corpus
//!    carries: a per-mate MRR floor, and **zero empty panes** across every note
//!    in the fixture — the mechanical form of GH #197's ruling that an empty
//!    pane may never come from an anchor-local statistic.
//! 7. **The search evidence calibration** (invariants.md D2; GH #201, Phase A
//!    of the disclosure work) — search's sibling of the z dump. Flow ② cannot
//!    currently answer *zero*: the vector half always has k nearest and RRF
//!    keeps only ranks, so a nonsense query serves `limit` confident-looking
//!    results. Before any evidence bar ships (GH #201's to earn), this block
//!    dumps the signals a query-level rule would judge, for every labelled
//!    query: the OR-sanitized BM25 match count and best BM25 score, the dense
//!    top-1 cosine, and what the shipped always-serve surface serves today —
//!    split into the positive/negative piles, with the would-be pure-cosine
//!    window re-derived each run (the GH #187 pattern, applied to search).
//!    **Negative queries** (empty `relevant` — the query-side siblings of the
//!    loner anchors) are excluded from every rank aggregate, so labelling them
//!    moved no pre-existing number.
//!
//! What this corpus **cannot** score is *candidate width*. 29 chunks is no more
//! than the candidates each signal retrieves — `chunk_candidate_pool(K)` for the
//! passage view, `note_candidate_pool(K)` for the note view, the narrower of the two
//! being what has to bind — so neither list is truncated, widening the pool cannot
//! add a candidate, and every number above is invariant under either view's headroom
//! or `search::pool_size` — a change to any of them
//! prints bit-identical scores here while reordering a real vault (GH #141). A run
//! that is blind that way says so; the property is measured by the rank-stability
//! probe (`--example stability`) on a vault big enough for the pool to bind. Note
//! the scope: `RRF_K` re-weights the same lists rather than changing them, so it
//! *does* move scores here and needs no separate instrument.
//!
//! `--sweep` re-chunks + re-embeds the same vault under variant [`ChunkConfig`]s
//! (`Vault::set_chunk_config` → `project(force)` → `embed`) and reports the same
//! scores per config — the in-process chunker A/B the #44 gate runs on.
//!
//! `--stemmer` is the #157 instrument: `Vault::rebuild_fts` swaps `chunks_fts`
//! between the shipped `porter unicode61` (schema v5 — the A/B's verdict) and the
//! unstemmed `unicode61` ablation, over the **identical** chunk rows and vectors —
//! nothing re-chunks or re-embeds, so every rank move is the tokenizer's alone.
//! BM25-only is scored under both tokenizers while the vault is still unembedded
//! (the honest lexical ablation), hybrid under both after; the dense ablation is
//! re-scored across the flip purely as an instrument check (FTS cannot reach it,
//! so any movement means the harness is broken, not the engine). Discovery is
//! not re-scored: `similar` never touches FTS.
//!
//! Every scored run appends one JSON line to `evals/results.jsonl` (gitignored),
//! so runs accumulate into a comparable dataset: "tune from numbers" needs the
//! numbers kept.

// `result_row`'s JSON literal is one `json!` expansion per key, and the row has
// grown a key per instrument (GH #158, #141, #183, #187, #188). Raising the
// limit keeps the row's shape legible in one place — the alternative is
// scattering its subtrees across helper functions to satisfy a macro, which
// costs the thing the row is for: a reader seeing every recorded field at once.
#![recursion_limit = "256"]

use b2_core::chunk::ChunkConfig;
use b2_core::db::FtsTokenizer;
use b2_core::embed::Embedder;
use b2_core::vault::{chunk_candidate_pool, note_candidate_pool, Vault};
use b2_embed::{provision, EmbedConfig, LocalEmbedder};
use serde::Deserialize;
use std::collections::HashMap;
use std::ops::ControlFlow;
use std::path::Path;
use std::time::Instant;

/// How deep we look for a relevant note/chunk when scoring.
const K: usize = 10;
/// How many `similar` candidates we look at per anchor.
const SIM_K: usize = 5;
/// How deep the z dump reads (GH #187) — every candidate note in a corpus this
/// size, since a threshold is calibrated against the whole population it has to
/// cut, not against the prefix a human-facing `limit` would show.
const Z_SCAN_LIMIT: usize = 500;
/// The reciprocity depths GH #200's candidate 1 is shown at in the headline
/// table and the per-anchor lists — [`SIM_K`] (the pane itself) with one depth
/// either side of it. They are a *display* choice only: the window below is
/// derived from the whole sweep, so no verdict turns on these three.
const FOLD_MUTUAL_K: [usize; 3] = [3, 5, 10];
/// How far the reciprocity depth is swept when re-deriving candidate 1's
/// admissible window each run (GH #187's idiom on the disclosure axis — a
/// measurement in the harness, never a constant frozen into a docstring). Half
/// the orthogonal corpus's note count: a `k` past that admits most of the vault
/// as "mutually near", which is the rule going vacuous rather than a depth
/// anyone would ship.
const FOLD_K_SWEEP: usize = 15;
/// The strength-band landmarks the desktop paints (`ui/src/strength.ts`,
/// GH #182), restated for the negatives' band readout (GH #197's A2): what a
/// loner anchor's always-served cards *claim*. Restated rather than imported —
/// the bands are UI copy, and this block is the instrument their values are
/// re-measured by.
const BAND_STRONG_Z: f64 = 2.52;
const BAND_CLEAR_Z: f64 = 1.96;
/// The soft reference floor on the default config's hybrid note hit@1.
/// Untouched by the GH #197 re-derivation: retrieval never had a gate to
/// retire.
const FLOOR_HIT1: f64 = 0.75;
/// The floor on **per-mate** discovery MRR@[`SIM_K`] (GH #188) — the
/// non-saturating rank metric GH #183 added, gated once a baseline existed to
/// price it. **Re-derived for always-serve** (GH #197): the shipped reading
/// moved from 0.633 to 0.650 when the existence gate retired — exactly the
/// returned phishing mate, served at rank 4 (+1/(4·15)) — so the floor moved
/// with it, by the same method that set it.
///
/// Placed **below** the reading, never at it: a gate pinned to today's number
/// fails on the first legitimate corpus edit, which trains the one habit this
/// harness must never train — editing a *label* to get green (process rule 2's
/// concern, stated for this metric in `docs/evals/README.md`). Per-mate is
/// label-sensitive by construction: adding a mate to an anchor changes `n` and
/// moves the aggregate whether or not the engine did anything.
///
/// Sizing, measured rather than guessed (GH #188's method, re-run for the
/// GH #197 reading): five consecutive always-serve runs on an unchanged
/// corpus/model/build reproduce every rank, z, and cosine **exactly**, so the
/// run-to-run noise floor is 0 and the headroom exists for *corpus* drift
/// instead. At n = 15 mates one mate lost from rank 1 costs 1/15 ≈ 0.067, and
/// this floor sits ~2 such losses under the shipped 0.650 — a real regression
/// trips it, a corpus edit that legitimately adds a hard mate does not.
const FLOOR_MATE_MRR: f64 = 0.52;
/// How many labelled mates the shipped surface may fail to serve at all.
///
/// **Structurally 0 under always-serve** (GH #197): both discovery passes read
/// the one ranked surface, so nothing can be reachable in one and unserved in
/// the other — the pre-#197 reading of 1 (`phishing.md`, under the retired
/// member bar) went to 0 with the gate that caused it. Kept as an assertion —
/// at zero, with no headroom, deliberately — because it is the **tripwire**
/// that re-arms the moment any Phase-2 existence signal puts a second surface
/// back in the path: a nonzero value here can only mean a gate is suppressing
/// a human-labelled relation again, which is exactly the event that must never
/// ship unmeasured twice.
const MAX_MATES_SUPPRESSED: usize = 0;
/// The floor on the **dense fixture's** per-mate MRR@[`SIM_K`] (GH #197,
/// Phase 0b) — the single-domain corpus's rank metric, gated once its baseline
/// existed to price it (the same measure-then-calibrate order as
/// [`FLOOR_MATE_MRR`]'s own history).
///
/// The reading is 0.467 (n = 14 mates) on the corpus as it stands: the model
/// recovers the within-cluster labels and ranks the three cross-cluster claims
/// lower — headroom in both directions, which is what a non-saturating
/// instrument needs. (The fixture's first reading was 0.502; a one-word
/// grammar fix in `hive-inspection.md` moved one mate a rank, the worked
/// example of why these floors carry corpus-drift margin at all.) Repeat runs
/// are bit-identical, so the margin is for corpus drift: at n = 14 one mate
/// lost from rank 1 costs 1/14 ≈ 0.071, and this floor sits ~2 such losses
/// under the reading. Process rule 2 binds hard here: in a corpus where
/// everything relates, relabelling toward the model's order would *always*
/// look plausible — a red reading argues about the notes.
const FLOOR_DENSE_MATE_MRR: f64 = 0.32;

#[derive(Deserialize)]
struct QuerySet {
    queries: Vec<Labelled>,
}

#[derive(Deserialize)]
struct Labelled {
    query: String,
    /// The vault-relative path(s) that should rank first. **Empty = a negative
    /// query** (invariants.md D2, GH #201): the labelled answer is "no
    /// matches" — the vault holds no evidence for the query, so everything
    /// served is junk by label, the query-side sibling of similar.json's
    /// negative anchors. Negative queries are excluded from every rank
    /// aggregate (adding one moves no pre-existing number) and are scored only
    /// by the search evidence calibration.
    relevant: Vec<String>,
    /// A short verbatim phrase from the target passage; when present the query is
    /// also scored at chunk level (does a top-K chunk of a relevant note contain
    /// it?). See queries.json's description for the labelling rules.
    #[serde(default)]
    passage: Option<String>,
}

#[derive(Deserialize)]
struct SimilarSet {
    anchors: Vec<SimilarLabel>,
}

#[derive(Deserialize)]
struct SimilarLabel {
    anchor: String,
    /// Corpus notes a human says belong next to `anchor`. **Empty = a negative
    /// anchor**: the labelled answer is "nothing relates", so the right result is
    /// zero candidates and everything surfaced is junk by label (similar.json).
    expected: Vec<String>,
}

/// One query's ranks in one retrieval mode: 1-based note rank, 1-based chunk rank
/// (only for passage-labelled queries), and the top note hit for display.
struct QueryScore {
    note: Option<usize>,
    chunk: Option<usize>,
    top: String,
}

/// Running hit@1 / hit@3 / MRR@K over a set of 1-based ranks. ("hit@k" — each
/// query has essentially one relevant target, so precision@k and recall@k
/// coincide with it.)
#[derive(Default)]
struct Agg {
    n: usize,
    hit1: usize,
    hit3: usize,
    rr: f64,
}

impl Agg {
    fn add(&mut self, rank: Option<usize>) {
        self.n += 1;
        if let Some(r) = rank {
            self.rr += 1.0 / r as f64;
            if r <= 1 {
                self.hit1 += 1;
            }
            if r <= 3 {
                self.hit3 += 1;
            }
        }
    }
    fn hit1(&self) -> f64 {
        self.hit1 as f64 / self.n.max(1) as f64
    }
    fn hit3(&self) -> f64 {
        self.hit3 as f64 / self.n.max(1) as f64
    }
    fn mrr(&self) -> f64 {
        self.rr / self.n.max(1) as f64
    }
}

/// A full pass over the query set in the vault's current state (keyword-only
/// before `embed`, hybrid after): per-query scores plus note- and chunk-level
/// aggregates.
struct Pass {
    scores: Vec<QueryScore>,
    note: Agg,
    chunk: Agg,
}

/// A full pass over the discovery labels: rank aggregates over the positive
/// anchors, the suppression tally over the negative ones, and every surfaced
/// candidate's cosine sorted into the two calibration piles the quality floor
/// is read from (index-engine.md §3's ruling, PR #145).
#[derive(Default)]
struct SimilarPass {
    /// Rank of the first `expected` hit per positive anchor — the pre-existing
    /// hit@1 / hit@3 / MRR discovery metrics, unchanged.
    rank: Agg,
    /// **Per-mate** ranks: one entry per `(anchor, expected mate)` pair rather
    /// than one per anchor (GH #183). [`Self::rank`] takes the *first* labelled
    /// mate it finds and stops, so an anchor with several mates scores a hit on
    /// its easiest one and every harder mate is invisible — which is exactly
    /// how that metric sat pinned at hit@1 = hit@3 = MRR@5 = 1.000 across the
    /// centroid-vs-best-passage ordering change it was supposed to judge
    /// (index-engine.md §3). Worse, *adding* a hard mate to an existing anchor
    /// makes `rank` strictly easier, never harder.
    ///
    /// Scoring each labelled mate on its own is the "rank-sensitive readout
    /// that doesn't saturate" GH #183 asked for: a mate that drops out of the
    /// top `SIM_K`, or merely slides from rank 2 to rank 4, moves this number
    /// even while `rank` stays at ceiling. A `None` is honest here — it means
    /// that specific labelled mate did not surface at all.
    ///
    /// **In the exit gate** since GH #188, at [`FLOOR_MATE_MRR`]. It shipped
    /// reporting-only for one issue's worth of time on the discovery floor's
    /// own precedent (GH #150: measure, then calibrate — both cheap floor
    /// variants failed when measured, so a threshold picked the day a metric
    /// is born is intuition wearing a number), and that is where the baseline
    /// came from: repeated runs on an unchanged corpus/model/build are
    /// bit-identical, so the gate is sized for *corpus* drift and placed below
    /// today's reading rather than at it.
    mate: Agg,
    /// [`Self::mate`] measured on the second pass — which since GH #197 reads
    /// the **same always-serve surface** as the first (there is no unfloored
    /// sibling to diff against: `similar` *is* the ranked list).
    ///
    /// Kept, with [`Self::mate_suppressed`] beside it, as the **tripwire
    /// structure**: while nothing gates, the two aggregates agree by
    /// construction and suppression reads a structural 0. If a Phase-2
    /// bake-off ever ships an existence signal, the passes diverge again and
    /// the suppression assertion re-arms with no harness change. (Historically
    /// this pair was first the centroid-vs-passage ordering A/B, then — after
    /// GH #192 aligned the orders — the floor's isolated suppression cost.)
    mate_raw: Agg,
    /// Labelled mates the second pass reaches that the first never serves —
    /// a surface suppressing a human-labelled relation outright, as opposed to
    /// merely ranking it lower. **Structurally 0 under always-serve**
    /// (GH #197): both passes read one surface, so any nonzero value here
    /// means an existence gate is back in the path — which is exactly what the
    /// assertion at [`MAX_MATES_SUPPRESSED`] exists to catch, and why it stays
    /// asserted apart from [`Self::mate`]: a suppressed *labelled* mate is a
    /// different (and worse) failure than a demoted one, and an average over
    /// the mates cannot tell the two apart.
    mate_suppressed: usize,
    /// Floored per-mate ranks in label order, so pass 2 can pair each mate's
    /// unfloored rank with its shipped one. Both passes walk `set.anchors` and
    /// each `label.expected` in the same order, so the flat index lines up.
    mate_floored: Vec<Option<usize>>,
    /// **Strangers**: unlabelled notes the shipped surface serves on a
    /// *positive* anchor, within the same top-`SIM_K` the ranks are read at —
    /// `(anchor, path)` per card, so the smoke alarm comes with the list you
    /// argue against it with (process rule 1).
    ///
    /// This is discovery's **precision** side, and the harness had none: the
    /// gated numbers watch mate ranks (recall) and negative anchors, and the
    /// negatives gate is *structurally* blind to `member_z` — while
    /// `member_z ≤ leader_z` a negative anchor is clean iff its leader is cut,
    /// so a relaxed member bar spends its entire cost here, on positive
    /// anchors' tails, where nothing was counting (GH #187/#188).
    ///
    /// Deliberately **reported, not gated**, and the reason is the gaming
    /// direction rather than the noise: the cheapest way to shrink this count
    /// is to *label the stranger*, which silently moves the per-mate metric
    /// too. An unlabelled note served is not proof of junk — the labels are
    /// not exhaustive — so it reads as a smoke alarm with names attached, and
    /// the argument happens against the notes.
    strangers: Vec<(String, String)>,
    /// Positive anchors serving at least one stranger — the spread behind
    /// [`Self::strangers`], since one anchor with a long tail and five anchors
    /// with one card each are different failures at the same count.
    stranger_anchors: usize,
    /// Negative anchors asked.
    neg_n: usize,
    /// Negative anchors that surfaced zero candidates. Under always-serve
    /// (GH #197) an anchor with scorable candidates always serves, so this
    /// reads 0 on this corpus — recorded for row comparability (the key's
    /// meaning, "anchors serving nothing", is unchanged), no longer asserted:
    /// the labels still say "nothing relates", and what the served cards
    /// *claim* is measured by their bands (A2's readout, calibration block).
    neg_clean: usize,
    /// Candidates surfaced across all negative anchors — cards whose labelled
    /// answer was "nothing".
    neg_cards: usize,
    /// Cosines of surfaced candidates a human labelled genuinely related.
    related: Vec<f64>,
    /// Cosines of everything else surfaced — non-expected candidates of positive
    /// anchors, and every candidate of a negative anchor.
    junk: Vec<f64>,
    /// Every anchor's surfaced list in rank order. The flat piles above judge the
    /// absolute floor; the *relative* drop-off cutoff is judged within one
    /// anchor's list (how far did #2 fall from #1?), and naming the pair behind a
    /// pile value needs the anchor too — so the order is recorded, not just the
    /// distribution.
    detail: Vec<AnchorDetail>,
}

/// One anchor's surfaced candidates, in rank order, for the results log.
struct AnchorDetail {
    anchor: String,
    /// True for a negative anchor (empty `expected`).
    negative: bool,
    /// (candidate path, cosine, human-labelled related) per surfaced candidate.
    candidates: Vec<(String, f64, bool)>,
}

/// One row of the z dump: a candidate in the band's unit, with the human label
/// attached.
struct ZCand {
    path: String,
    /// The stage-2 best-passage z (GH #192's unit; the strength band's input,
    /// gating nothing since GH #197) — straight from the engine's own
    /// statistics.
    z: f64,
    /// The same z recomputed harness-side from the served scores (z over squared
    /// best-pair distance, nearer = higher). An instrument check, not a reading:
    /// if this drifts from `z`, the engine's statistic moved and the harness's
    /// model of it is stale. `None` only when the population had zero variance
    /// and no z exists.
    z_recheck: Option<f64>,
    /// The stage-2 score as served (negated best chunk-pair L2) — kept so the
    /// row also records the model-comparable cosine, not only the
    /// anchor-relative z.
    score: f64,
    /// Labelled a mate of this anchor.
    mate: bool,
}

/// One anchor's **complete** reading in the band's unit — every candidate note
/// in the corpus, in z order, with the human label attached (GH #187; stage-2
/// best-passage z since GH #192; the z travels ungated on the one shipped
/// surface since GH #197, so the dump *is* the served list read deep).
///
/// The cosine piles above are the same numbers in a model-comparable unit; this
/// is the anchor-relative unit a z existence bar would judge in, which is why
/// the piles could never re-derive one's constants.
struct AnchorZ {
    anchor: String,
    /// True for a negative anchor (empty `expected`) — its whole list is
    /// strangers by label, and its leader is what a leader gate would answer to.
    negative: bool,
    /// Every candidate in served order, which *is* descending z.
    candidates: Vec<ZCand>,
}

impl AnchorZ {
    /// The top candidate's z — what a leader gate would read. `None` only if
    /// the anchor produced no scorable candidate at all.
    fn leader(&self) -> Option<f64> {
        self.candidates.first().map(|c| c.z)
    }
    /// This anchor's labelled mates' z's (empty for a negative anchor).
    fn mates(&self) -> impl Iterator<Item = f64> + '_ {
        self.candidates.iter().filter(|c| c.mate).map(|c| c.z)
    }
    /// Everything on this anchor's list a human did *not* label — on a positive
    /// anchor, exactly the population a member bar would have to cut.
    fn strangers(&self) -> impl Iterator<Item = f64> + '_ {
        self.candidates.iter().filter(|c| !c.mate).map(|c| c.z)
    }
    /// The worst disagreement between the engine's z and the harness's own
    /// recomputation from the served scores, across this anchor's candidates —
    /// the statistic-level drift check ([`ZCand::z_recheck`]).
    fn recheck_delta(&self) -> f64 {
        self.candidates
            .iter()
            .filter_map(|c| c.z_recheck.map(|r| (c.z - r).abs()))
            .fold(0.0, f64::max)
    }
}

/// The z dump across every discovery anchor, split into the populations an
/// existence bar would answer to (GH #187; the unit is the stage-2 best-passage
/// z since GH #192, and it gates nothing since GH #197).
///
/// Three populations, because a two-bar rule's constants answer to different
/// ones — the conflation is what made "the negatives gate would catch a bad
/// `member_z`" look true when it was not: while `member_z ≤ leader_z`, a
/// negative anchor is clean **iff its leader is cut**, so no member bar can
/// dirty (or clean) it. A leader gate is calibrated by negative-anchor leaders
/// against positive-anchor leaders; a member bar by strangers on positive
/// anchors against labelled mates. Both windows are re-derived here on every
/// run — the standing record of why no such rule ships (the member window has
/// been empty since #187; GH #196 measured the leader gate darkening a dense
/// vault), and the first reading any Phase-2 bake-off candidate answers to.
#[derive(Default)]
struct FloorZ {
    /// Every anchor with computed statistics, in label order.
    anchors: Vec<AnchorZ>,
    /// Anchors whose candidate pool was under `STATS_MIN_POPULATION` or had zero
    /// variance, so no z exists to calibrate from. Named rather than silently
    /// dropped: on a corpus small enough for this to happen, every window below
    /// is measured on fewer anchors than the labels suggest.
    ungraded: Vec<String>,
}

impl FloorZ {
    /// (a) Labelled mates' z's — what a member bar would have to keep.
    fn mate_z(&self) -> Vec<f64> {
        self.anchors.iter().flat_map(|a| a.mates()).collect()
    }
    /// (b) Strangers on **positive** anchors — what a member bar would have to
    /// cut. The negative anchors' own candidates are deliberately excluded:
    /// they are the leader pair's business, and folding them in here is what
    /// would let a member window look wider than it is.
    fn stranger_z(&self) -> Vec<f64> {
        self.anchors
            .iter()
            .filter(|a| !a.negative)
            .flat_map(|a| a.strangers())
            .collect()
    }
    /// (c) Negative anchors' leaders — what a leader gate would have to cut.
    fn neg_leader_z(&self) -> Vec<f64> {
        self.anchors
            .iter()
            .filter(|a| a.negative)
            .filter_map(|a| a.leader())
            .collect()
    }
    /// Positive anchors' leaders — what a leader gate would have to keep, or it
    /// empties a list that has a real mate on it.
    fn pos_leader_z(&self) -> Vec<f64> {
        self.anchors
            .iter()
            .filter(|a| !a.negative)
            .filter_map(|a| a.leader())
            .collect()
    }

    /// The worst engine-vs-recomputed z disagreement across every anchor — the
    /// statistic-level instrument check (see [`ZCand::z_recheck`]). Small fp
    /// noise is expected (the engine z-scores f32 squared distances; the
    /// harness recomputes them from the f32-sqrt'd scores), so the printed
    /// check tolerates 1e-3 z and reports the observed maximum.
    fn recheck_delta(&self) -> f64 {
        self.anchors
            .iter()
            .map(|a| a.recheck_delta())
            .fold(0.0, f64::max)
    }
}

/// The admissible interval for one threshold, read straight off two labelled
/// populations: a bar `t` keeps everything in `keep` iff `t ≤ min(keep)`, and
/// cuts everything in `cut` iff `t > max(cut)`, so every workable constant lies
/// in `(cut_max, keep_min]` — and **no constant works at all** when the two
/// populations overlap, which is a measured result rather than a failure to
/// search harder.
struct Window {
    cut_max: f64,
    keep_min: f64,
}

impl Window {
    /// Both edges, or `None` while either population is empty (nothing to
    /// separate — an honest no-reading, not a wide-open window).
    fn read(cut: &[f64], keep: &[f64]) -> Option<Self> {
        let cut_max = cut.iter().copied().fold(f64::NEG_INFINITY, f64::max);
        let keep_min = keep.iter().copied().fold(f64::INFINITY, f64::min);
        (cut_max.is_finite() && keep_min.is_finite()).then_some(Self { cut_max, keep_min })
    }
    /// Whether any constant separates the two populations.
    fn open(&self) -> bool {
        self.cut_max < self.keep_min
    }
}

fn main() {
    match run() {
        Err(e) => {
            eprintln!("eval failed: {e}");
            std::process::exit(1);
        }
        Ok(passed) => {
            if !passed {
                std::process::exit(2);
            }
        }
    }
}

/// Returns whether the default-config hybrid pass cleared the reference floor.
fn run() -> Result<bool, Box<dyn std::error::Error>> {
    let sweep = std::env::args().any(|a| a == "--sweep");
    let stemmer = std::env::args().any(|a| a == "--stemmer");
    let evals_dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("evals");
    let corpus_dir = evals_dir.join("corpus");
    let results_path = evals_dir.join("results.jsonl");

    // Load the labelled sets. Queries split on their labels: positives carry
    // rank labels; an empty `relevant` is a negative query (invariants.md D2,
    // GH #201), scored only by the search evidence calibration so its presence
    // moves no rank aggregate.
    let set: QuerySet =
        serde_json::from_str(&std::fs::read_to_string(evals_dir.join("queries.json"))?)?;
    let (negatives, positives): (Vec<Labelled>, Vec<Labelled>) =
        set.queries.into_iter().partition(|q| q.relevant.is_empty());
    let sim_set: SimilarSet =
        serde_json::from_str(&std::fs::read_to_string(evals_dir.join("similar.json"))?)?;
    let dense_set: SimilarSet = serde_json::from_str(&std::fs::read_to_string(
        evals_dir.join("similar-dense.json"),
    )?)?;

    // Ensure the model is available, then load it. (Provision is idempotent, so an
    // already-installed model is a no-op; a missing one is fetched here.)
    let config = EmbedConfig::load()?;
    provision(&config, |line| eprintln!("[init] {line}"))?;
    let embedder = LocalEmbedder::load(&config)?;
    let model_id = embedder.model_id().to_string();
    let dim = embedder.dim();
    eprintln!("[eval] model = {model_id} (dim {dim})\n");

    // A correctness gate, not a score: every number below is computed from batched
    // embeddings, so they only mean anything if batching is faithful.
    check_batch_matches_single(&embedder)?;

    // Build a throwaway vault from the corpus.
    let tmp = tempfile::TempDir::new()?;
    let vault_root = tmp.path().join("vault");
    std::fs::create_dir_all(&vault_root)?;
    for entry in std::fs::read_dir(&corpus_dir)? {
        let entry = entry?;
        // Regular files only: `fs::copy` errors on a directory, so a future
        // corpus/ subfolder (or any stray non-file) must not abort the run.
        if entry.file_type()?.is_file() {
            std::fs::copy(entry.path(), vault_root.join(entry.file_name()))?;
        }
    }
    let mut vault = Vault::open_with_embedder(&vault_root, Box::new(embedder))?;

    // ---- Phase 1: projection only → the BM25-only baseline. ------------------
    // The vector space does not exist yet, so `search`/`search_chunks` run
    // keyword-only (index-engine.md) — the ablation costs nothing
    // extra: it is the same vault, paused between the two passes.
    let report = vault.project(false)?;
    let bm25 = score_pass(&vault, &positives, Retrieval::Fused)?;
    eprintln!(
        "[eval] projected {} notes; BM25-only baseline scored\n",
        report.indexed
    );

    // The stemmer instrument's lexical arm is scored HERE, while the vault is
    // still projected-but-unembedded, so `search` is honestly BM25-only under
    // both tokenizers; the vault is handed back to the shipped default before
    // anything embeds.
    let bm25_unstemmed = if stemmer {
        vault.rebuild_fts(FtsTokenizer::Unicode61)?;
        let pass = score_pass(&vault, &positives, Retrieval::Fused)?;
        vault.rebuild_fts(FtsTokenizer::PorterUnicode61)?;
        Some(pass)
    } else {
        None
    };

    // ---- Phase 2: embed → vector-only ablation + hybrid + discovery. ---------
    let (chunks, embed_secs) = timed_embed(&vault)?;
    let vector = score_pass(&vault, &positives, Retrieval::VectorOnly)?;
    let hybrid = score_pass(&vault, &positives, Retrieval::Fused)?;
    let similar = score_similar(&vault, &sim_set)?;
    // The fold bake-off (GH #200, Phase B) — the candidate default-disclosure
    // rules judged on the same served lists `similar` above was scored from, so
    // no rule is compared against a surface the others did not see.
    let fold = score_fold(&vault, &sim_set, "orthogonal", false)?;
    // The z calibration dump (GH #187) — the same shipped surface read deep,
    // since the z travels ungated on it (GH #197).
    let floor_z = score_floor_z(&vault, &sim_set)?;
    // The search evidence dump (invariants.md D2, GH #201) — the query-side
    // sibling of the z calibration, read over the same built vault.
    let evidence = score_search_evidence(&vault_root, &vault, &positives, &negatives)?;
    eprintln!(
        "[eval] embedded {chunks} chunks in {embed_secs:.1}s ({} candidates per signal at K={K}, \
         {} for the passage view)\n",
        note_candidate_pool(K),
        chunk_candidate_pool(K)
    );
    warn_if_pool_blind(chunks);

    print_default_report(&positives, &bm25, &vector, &hybrid, &similar, &floor_z);
    print_fold_bench(&fold);
    print_search_evidence(&evidence);

    let git = git_short_sha();
    append_result(
        &results_path,
        result_row(
            &git,
            &model_id,
            dim,
            "default",
            &ChunkConfig::default(),
            FtsTokenizer::PorterUnicode61.sql(),
            report.indexed,
            chunks,
            embed_secs,
            &positives,
            Some(&bm25),
            Some(&vector),
            &hybrid,
            Some(&similar),
            Some(&floor_z),
            Some(&evidence),
            Some(&fold),
        ),
    )?;

    // ---- Phase 3: the dense single-domain fixture (GH #196/#197, Phase 0b). --
    // Its own throwaway vault, its own model load, its own results row (corpus
    // id `dense`) — the fixture measures a *vault-level* geometry, so nothing
    // about it may share state with the orthogonal corpus's run above.
    let dense = score_dense(&evals_dir, &dense_set)?;
    print_dense_report(&dense);
    print_fold_bench(&dense.fold);
    append_result(&results_path, dense_row(&git, &model_id, dim, &dense))?;

    // ---- Optional: the FTS tokenizer ablation (the #157 instrument). ---------
    // One lever, isolated: `rebuild_fts` swaps the tokenizer over the identical
    // chunk rows and vectors — the shipped `porter unicode61` against the
    // unstemmed `unicode61` the A/B retired, kept measurable so the verdict can
    // be re-tried as the corpus grows. Discovery is deliberately not re-scored —
    // `similar` never touches FTS (centroid shortlist + chunk vectors), so its
    // numbers cannot move and the ablation row records no `similar` keys. The
    // dense ablation IS re-scored, as an instrument check: FTS cannot reach it
    // either, so a moved dense rank means the harness is broken, not the engine.
    if stemmer {
        let bm25_unstemmed = bm25_unstemmed
            .as_ref()
            .expect("scored while unembedded above");
        vault.rebuild_fts(FtsTokenizer::Unicode61)?;
        let vec_unstemmed = score_pass(&vault, &positives, Retrieval::VectorOnly)?;
        let hybrid_unstemmed = score_pass(&vault, &positives, Retrieval::Fused)?;

        println!("\n{}", "=".repeat(78));
        println!(
            "stemmer ablation — chunks_fts rebuilt `unicode61` (unstemmed) over identical chunks + vectors (GH #157)"
        );
        let dense_moved = (0..positives.len())
            .filter(|&i| vec_unstemmed.scores[i].note != vector.scores[i].note)
            .count();
        if dense_moved == 0 {
            println!("  [check] dense ablation identical across the flip, as it must be");
        } else {
            println!(
                "  [FAULT] {dense_moved} dense rank(s) moved across an FTS-only flip — \
                 the harness is broken; distrust this run"
            );
        }
        println!("  bm25-only, unstemmed vs shipped porter:");
        print_rank_moves(&positives, &bm25, bm25_unstemmed);
        println!("  hybrid, unstemmed vs shipped porter:");
        print_rank_moves(&positives, &hybrid, &hybrid_unstemmed);
        println!(
            "  unstemmed aggregates: bm25 note {:.2} / {:.3}   hybrid note {:.2} / {:.3}   chunk {:.2} / {:.3}",
            bm25_unstemmed.note.hit1(),
            bm25_unstemmed.note.mrr(),
            hybrid_unstemmed.note.hit1(),
            hybrid_unstemmed.note.mrr(),
            hybrid_unstemmed.chunk.hit1(),
            hybrid_unstemmed.chunk.mrr(),
        );
        append_result(
            &results_path,
            result_row(
                &git,
                &model_id,
                dim,
                "unicode61",
                &ChunkConfig::default(),
                FtsTokenizer::Unicode61.sql(),
                report.indexed,
                chunks,
                embed_secs,
                &positives,
                Some(bm25_unstemmed),
                Some(&vec_unstemmed),
                &hybrid_unstemmed,
                None,
                None,
                None,
                None,
            ),
        )?;
        // Hand the vault back untainted, so a `--sweep` in the same run (and the
        // reference numbers above) stay under the shipped default tokenizer.
        vault.rebuild_fts(FtsTokenizer::PorterUnicode61)?;
    }

    // ---- Optional: the in-process chunker sweep (the #44 A/B). ---------------
    if sweep {
        // The #44 grid: both directions on each swept knob, plus the one
        // interaction worth a row. `target_tokens` brackets the 450 default
        // (250 / 350 / 600); `overlap_frac` brackets 0.15 (0.0 / 0.30);
        // `target-250+heading-path` exists because a smaller chunk carries less
        // of its own context, which is exactly when the breadcrumb prefix (D3)
        // is most plausibly worth its tokens. `chars_per_token` and
        // `backscan_tokens` stay unswept: they are calibration constants of the
        // token proxy and the boundary search, not retrieval-quality levers.
        let variants: Vec<(&str, ChunkConfig)> = vec![
            (
                "target-250",
                ChunkConfig {
                    target_tokens: 250,
                    ..ChunkConfig::default()
                },
            ),
            (
                "target-350",
                ChunkConfig {
                    target_tokens: 350,
                    ..ChunkConfig::default()
                },
            ),
            (
                "target-600",
                ChunkConfig {
                    target_tokens: 600,
                    ..ChunkConfig::default()
                },
            ),
            (
                "overlap-0",
                ChunkConfig {
                    overlap_frac: 0.0,
                    ..ChunkConfig::default()
                },
            ),
            (
                "overlap-30",
                ChunkConfig {
                    overlap_frac: 0.30,
                    ..ChunkConfig::default()
                },
            ),
            (
                "prepend-heading-path",
                ChunkConfig {
                    prepend_heading_path: true,
                    ..ChunkConfig::default()
                },
            ),
            (
                "target-250+heading-path",
                ChunkConfig {
                    target_tokens: 250,
                    prepend_heading_path: true,
                    ..ChunkConfig::default()
                },
            ),
        ];
        println!("\n{}", "=".repeat(78));
        println!("chunker sweep (same model, same corpus; default row above for reference)");
        // `mate MRR` rather than the per-anchor `similar h@3` this column used
        // to carry: that number saturates (GH #183), so as a *comparison*
        // column across variants it could only ever print 1.00 (GH #188).
        // `strangers` replaced `neg clean` when GH #197 retired the existence
        // gate: under always-serve a negative anchor always serves, so that
        // column could only ever print 0/5 — the same cannot-move failure.
        println!(
            "{:<24} {:>7} {:>8}   note h@1/MRR   vec h@1/MRR    chunk h@1/MRR   mate MRR   strangers",
            "config", "chunks", "embed_s"
        );
        for (label, cfg) in variants {
            vault.set_chunk_config(cfg.clone());
            vault.project(true)?; // force: re-chunk everything, clearing vectors
            let (chunks, embed_secs) = timed_embed(&vault)?;
            let vec_pass = score_pass(&vault, &positives, Retrieval::VectorOnly)?;
            let pass = score_pass(&vault, &positives, Retrieval::Fused)?;
            let sim = score_similar(&vault, &sim_set)?;
            println!(
                "{:<24} {:>7} {:>8.1}   {:.2} / {:.3}    {:.2} / {:.3}    {:.2} / {:.3}    {:.3}      {}",
                label,
                chunks,
                embed_secs,
                pass.note.hit1(),
                pass.note.mrr(),
                vec_pass.note.hit1(),
                vec_pass.note.mrr(),
                pass.chunk.hit1(),
                pass.chunk.mrr(),
                sim.mate.mrr(),
                sim.strangers.len(),
            );
            // The readout the A/B is actually judged on: at this n every aggregate
            // delta above is 1–2 queries, so the aggregate is a smoke alarm and the
            // per-query win/loss list is the data (docs/evals/README.md, the
            // process rules).
            print_rank_moves(&positives, &hybrid, &pass);
            append_result(
                &results_path,
                result_row(
                    &git,
                    &model_id,
                    dim,
                    label,
                    &cfg,
                    FtsTokenizer::PorterUnicode61.sql(),
                    report.indexed,
                    chunks,
                    embed_secs,
                    &positives,
                    None,
                    Some(&vec_pass),
                    &pass,
                    Some(&sim),
                    None,
                    None,
                    None,
                ),
            )?;
        }
    }

    eprintln!("\n[eval] appended run to {}", results_path.display());

    // The soft floors, on the DEFAULT config's passes — so this can double as
    // a manual quality gate. Not a CI test.
    if hybrid.note.hit1() < FLOOR_HIT1 {
        eprintln!(
            "\n[warn] hybrid hit@1 {:.2} is below the {FLOOR_HIT1} reference floor — inspect the misses above.",
            hybrid.note.hit1()
        );
        return Ok(false);
    }
    // The negatives' suppression assertion (GH #150) RETIRED with the gate it
    // watched (GH #197): under always-serve a loner anchor serves its ranked
    // nearest — that is the ruling, not a regression, so `neg_clean == neg_n`
    // would now assert the retired behavior. The anchors stay labelled, the
    // strangers instrument keeps counting, and what the served cards *claim*
    // is the calibration block's band readout (every leader paints `●○○` on
    // the corpus today).
    // Discovery **rank** (GH #188; re-derived for always-serve by GH #197).
    // The per-mate metric shipped reporting-only on purpose — the precedent
    // (GH #150) is measure-then-calibrate, and a threshold picked the day a
    // metric is born is intuition wearing a number. The rank floor sits below
    // its measured reading (corpus-drift headroom, run noise being zero);
    // suppression, next, sits AT its structural zero — a tripwire, not a
    // budget. `docs/evals/README.md` carries the failure mode neither may
    // train.
    if similar.mate.mrr() < FLOOR_MATE_MRR {
        eprintln!(
            "\n[warn] per-mate MRR@{SIM_K} {:.3} is below the {FLOOR_MATE_MRR:.2} floor — discovery ranking regressed \
             (read the per-mate line's mates, not the aggregate; and do NOT relabel to clear this).",
            similar.mate.mrr()
        );
        return Ok(false);
    }
    // Suppression is asserted separately rather than folded into the average
    // above, because a mate going from rank 5 to *absent* is a categorically
    // worse event than drifting 2 → 3, and a mean over 15 mates hides exactly
    // that (GH #188). At zero since GH #197 — see MAX_MATES_SUPPRESSED.
    if similar.mate_suppressed > MAX_MATES_SUPPRESSED {
        eprintln!(
            "\n[warn] {} of {} labelled mates suppressed where always-serve permits none — \
             an existence gate is back in the path (GH #197's tripwire).",
            similar.mate_suppressed, similar.mate.n
        );
        return Ok(false);
    }
    // The dense fixture's existence assertion (GH #196/#197): every note in a
    // corpus where everything relates must serve candidates. An empty pane here
    // can only come from an anchor-local statistic claiming "nothing relates"
    // on a vault where that is false — the exact failure GH #196 measured.
    if !dense.empty_panes.is_empty() {
        eprintln!(
            "\n[warn] {} of {} dense-fixture notes serve an EMPTY pane ({}) — an existence \
             gate is refusing a vault whose every note genuinely relates (GH #196).",
            dense.empty_panes.len(),
            dense.notes,
            dense.empty_panes.join(", ")
        );
        return Ok(false);
    }
    // …and its rank floor, the dense sibling of FLOOR_MATE_MRR.
    if dense.mate.mrr() < FLOOR_DENSE_MATE_MRR {
        eprintln!(
            "\n[warn] dense per-mate MRR@{SIM_K} {:.3} is below the {FLOOR_DENSE_MATE_MRR:.2} floor — \
             single-domain discovery ranking regressed (argue with the notes, not the labels).",
            dense.mate.mrr()
        );
        return Ok(false);
    }
    Ok(true)
}

/// Score the dense single-domain fixture (GH #196/#197, Phase 0b) in a throwaway
/// vault of its own: per-mate discovery ranks against `similar-dense.json`, and
/// the empty-pane sweep across **every** note in the fixture — not only the
/// labelled anchors, because the assertion is about the surface ("no pane in a
/// dense vault is dark"), not about the labels.
fn score_dense(
    evals_dir: &Path,
    set: &SimilarSet,
) -> Result<DensePass, Box<dyn std::error::Error>> {
    let corpus_dir = evals_dir.join("corpus-dense");
    // A second model load rather than sharing the first vault's: the embedder was
    // moved into that vault, and the fixture's whole point is an isolated run.
    let config = EmbedConfig::load()?;
    let embedder = LocalEmbedder::load(&config)?;
    let tmp = tempfile::TempDir::new()?;
    let vault_root = tmp.path().join("vault");
    std::fs::create_dir_all(&vault_root)?;
    for entry in std::fs::read_dir(&corpus_dir)? {
        let entry = entry?;
        if entry.file_type()?.is_file() {
            std::fs::copy(entry.path(), vault_root.join(entry.file_name()))?;
        }
    }
    let vault = Vault::open_with_embedder(&vault_root, Box::new(embedder))?;
    vault.project(false)?;
    let (chunks, embed_secs) = timed_embed(&vault)?;

    let mut pass = DensePass {
        notes: 0,
        chunks,
        embed_secs,
        mate: Agg::default(),
        mate_ranks: Vec::new(),
        empty_panes: Vec::new(),
        detail: Vec::new(),
        // The bake-off's absolute bench (GH #200): every note is an anchor here,
        // because the ruling being tested is about the *surface* — a vault where
        // everything relates may never default to "nothing relates" — and the
        // labels cover only a few of these notes.
        fold: score_fold(&vault, set, "dense", true)?,
    };
    // The pane sweep: every note is an anchor, labelled or not.
    for note in vault.list_notes()? {
        pass.notes += 1;
        if vault.similar(&note.path, SIM_K)?.is_empty() {
            pass.empty_panes.push(note.path);
        }
    }
    // Per-mate ranks on the labelled anchors, the orthogonal corpus's metric
    // re-used verbatim (GH #183's non-saturating readout).
    for label in &set.anchors {
        let candidates = vault.similar(&label.anchor, SIM_K)?;
        for expected in &label.expected {
            let rank = candidates
                .iter()
                .position(|c| paths_match(&c.path, expected))
                .map(|p| p + 1);
            pass.mate.add(rank);
            pass.mate_ranks
                .push((label.anchor.clone(), expected.clone(), rank));
        }
        pass.detail.push(AnchorDetail {
            anchor: label.anchor.clone(),
            negative: false,
            candidates: candidates
                .iter()
                .map(|c| {
                    let related = label.expected.iter().any(|e| paths_match(&c.path, e));
                    (c.path.clone(), cosine_of(c.score), related)
                })
                .collect(),
        });
    }
    Ok(pass)
}

/// The dense fixture's reading (see [`score_dense`]).
struct DensePass {
    notes: usize,
    chunks: usize,
    embed_secs: f64,
    /// Per-mate ranks at [`SIM_K`] — the fixture's rank metric.
    mate: Agg,
    /// (anchor, mate, rank) per labelled mate, for the printed lines and the row.
    mate_ranks: Vec<(String, String, Option<usize>)>,
    /// Notes whose discovery pane served nothing — asserted empty (GH #196/#197).
    empty_panes: Vec<String>,
    detail: Vec<AnchorDetail>,
    /// The fold bake-off on this fixture (GH #200) — swept over **every** note,
    /// which is where the candidates' hardest bench is: a rule whose default
    /// view goes dark on a single-domain vault is disqualified, not re-tuned.
    fold: FoldBench,
}

fn print_dense_report(dense: &DensePass) {
    println!("\n{}", "=".repeat(78));
    println!(
        "dense fixture — corpus-dense/ ({} notes / {} chunks, single-domain, no loner; GH #196/#197)",
        dense.notes, dense.chunks
    );
    println!(
        "  per-mate   hit@1={:.2}  hit@3={:.2}  MRR@{SIM_K}={:.3}  (n={} mates, GATED at MRR@{SIM_K} ≥ {FLOOR_DENSE_MATE_MRR:.2})",
        dense.mate.hit1(),
        dense.mate.hit3(),
        dense.mate.mrr(),
        dense.mate.n
    );
    for (anchor, mate, rank) in &dense.mate_ranks {
        println!(
            "             {:>5}  {anchor} → {mate}",
            rank_str_at(*rank, SIM_K)
        );
    }
    if dense.empty_panes.is_empty() {
        println!(
            "  panes      {}/{} notes serve candidates — zero empty panes (ASSERTED: a dense vault \
             may never read as \"nothing relates\")",
            dense.notes, dense.notes
        );
    } else {
        println!(
            "  panes      {} of {} notes serve an EMPTY pane: {}",
            dense.empty_panes.len(),
            dense.notes,
            dense.empty_panes.join(", ")
        );
    }
}

/// The dense fixture's own JSONL row. Tagged `"corpus": "dense"` — the key that
/// keeps rows from ever averaging across corpora (the orthogonal rows carry
/// `"corpus": "orthogonal"`); a smaller shape than the main row on purpose,
/// since the fixture scores discovery alone.
fn dense_row(
    git: &Option<String>,
    model: &str,
    dim: usize,
    dense: &DensePass,
) -> serde_json::Value {
    let ts = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    serde_json::json!({
        "ts": ts,
        "git": git,
        "model": model,
        "dim": dim,
        "corpus": "dense",
        "notes": dense.notes,
        "chunks": dense.chunks,
        "embed_secs": dense.embed_secs,
        "similar_per_mate": { "n": dense.mate.n, "hit1": dense.mate.hit1(), "hit3": dense.mate.hit3(), "mrr": dense.mate.mrr() },
        "mates": dense.mate_ranks.iter().map(|(anchor, mate, rank)| serde_json::json!({
            "anchor": anchor, "mate": mate, "rank": rank,
        })).collect::<Vec<_>>(),
        "empty_panes": { "n": dense.notes, "empty": dense.empty_panes.len(), "detail": dense.empty_panes },
        "discovery_fold": fold_json(&dense.fold),
        "similar_detail": dense.detail.iter().map(|d| serde_json::json!({
            "anchor": d.anchor,
            "negative": d.negative,
            "candidates": d.candidates.iter().map(|(path, cos, related)| serde_json::json!({
                "path": path,
                "cos": (cos * 1e4).round() / 1e4,
                "related": related,
            })).collect::<Vec<_>>(),
        })).collect::<Vec<_>>(),
    })
}

/// [`rank_str`] at a caller-named depth (the discovery metrics read at
/// [`SIM_K`], not [`K`]).
fn rank_str_at(rank: Option<usize>, depth: usize) -> String {
    match rank {
        Some(1) => "✓1".to_string(),
        Some(r) => format!("·{r}"),
        None => format!("✗>{depth}"),
    }
}

/// Which retrieval a pass scores. `Fused` is the shipped path (`Vault::search` —
/// BM25-only before embed, hybrid after) plus chunk-level scoring for
/// passage-labelled queries; `VectorOnly` is the dense ablation
/// (`Vault::search_vector_only`, GH #158), note-level only — its chunk aggregate
/// stays empty. The ablation column is what lets a run say whether fusion paid
/// rent: the finding that RRF demotes dense rank-1 hits was established
/// by hand-decomposing fused scores once; this makes it a standing measurement.
#[derive(Clone, Copy, PartialEq)]
enum Retrieval {
    Fused,
    VectorOnly,
}

/// Score every labelled query against the vault's current state: note rank via
/// the selected retrieval, and — for passage-labelled queries on the fused path —
/// chunk rank via `search_chunks` (the first top-K chunk that belongs to a
/// relevant note AND contains the labelled phrase, case-insensitively).
fn score_pass(
    vault: &Vault,
    queries: &[Labelled],
    retrieval: Retrieval,
) -> Result<Pass, Box<dyn std::error::Error>> {
    let mut scores = Vec::with_capacity(queries.len());
    let mut note_agg = Agg::default();
    let mut chunk_agg = Agg::default();
    for q in queries {
        let results = match retrieval {
            Retrieval::Fused => vault.search(&q.query, K)?,
            Retrieval::VectorOnly => vault.search_vector_only(&q.query, K)?,
        };
        let note = results
            .iter()
            .position(|r| q.relevant.iter().any(|rel| paths_match(&r.path, rel)))
            .map(|p| p + 1);
        let top = results
            .first()
            .map(|r| r.path.clone())
            .unwrap_or_else(|| "—".to_string());
        note_agg.add(note);

        let chunk = match (&q.passage, retrieval) {
            (None, _) | (_, Retrieval::VectorOnly) => None,
            (Some(passage), Retrieval::Fused) => {
                let needle = passage.to_lowercase();
                let hits = vault.search_chunks(&q.query, K)?;
                let rank = hits
                    .iter()
                    .position(|h| {
                        q.relevant.iter().any(|rel| paths_match(&h.path, rel))
                            && h.text.to_lowercase().contains(&needle)
                    })
                    .map(|p| p + 1);
                chunk_agg.add(rank);
                rank
            }
        };
        scores.push(QueryScore { note, chunk, top });
    }
    Ok(Pass {
        scores,
        note: note_agg,
        chunk: chunk_agg,
    })
}

/// Score the discovery labels — **two passes, one surface** (GH #197).
///
/// Both passes read `Vault::similar`, the always-served ranked list. Pass 1
/// scores the ranks, the strangers, and the negatives' card tally; pass 2
/// re-reads the same surface for the cosine piles, the per-anchor detail, and
/// the pass-vs-pass suppression diff — which is structurally 0 while nothing
/// gates, and is *kept* as the tripwire that re-arms the suppression assertion
/// the moment a Phase-2 existence signal puts a second surface back
/// (see [`SimilarPass::mate_raw`]).
fn score_similar(
    vault: &Vault,
    set: &SimilarSet,
) -> Result<SimilarPass, Box<dyn std::error::Error>> {
    let mut pass = SimilarPass::default();
    // Pass 1 — the shipped surface: ranks, strangers, and the negatives' tally.
    for label in &set.anchors {
        let candidates = vault.similar(&label.anchor, SIM_K)?;
        let negative = label.expected.is_empty();
        if negative {
            pass.neg_n += 1;
            if candidates.is_empty() {
                pass.neg_clean += 1;
            }
            pass.neg_cards += candidates.len();
        } else {
            let rank = candidates
                .iter()
                .position(|c| label.expected.iter().any(|e| paths_match(&c.path, e)))
                .map(|p| p + 1);
            pass.rank.add(rank);
            // …and again per labelled mate, so a hard one can't hide behind an
            // easy one (GH #183 — see `SimilarPass::mate`).
            for expected in &label.expected {
                let mate_rank = candidates
                    .iter()
                    .position(|c| paths_match(&c.path, expected))
                    .map(|p| p + 1);
                pass.mate.add(mate_rank);
                pass.mate_floored.push(mate_rank);
            }
            // The precision side of the same list (GH #188 — see
            // `SimilarPass::strangers`): everything served here that no label
            // claims. Scored on the shipped surface at the ranks' own depth,
            // because that is the list a human is handed.
            let before = pass.strangers.len();
            for c in &candidates {
                if !label.expected.iter().any(|e| paths_match(&c.path, e)) {
                    pass.strangers.push((label.anchor.clone(), c.path.clone()));
                }
            }
            if pass.strangers.len() > before {
                pass.stranger_anchors += 1;
            }
        }
    }
    // Pass 2 — the same surface, re-read: the calibration piles + detail, and
    // the suppression tripwire against pass 1 (see `SimilarPass::mate_raw`).
    for label in &set.anchors {
        let candidates = vault.similar(&label.anchor, SIM_K)?;
        let negative = label.expected.is_empty();
        if !negative {
            for expected in &label.expected {
                let mate_rank = candidates
                    .iter()
                    .position(|c| paths_match(&c.path, expected))
                    .map(|p| p + 1);
                let floored = pass.mate_floored.get(pass.mate_raw.n).copied().flatten();
                pass.mate_raw.add(mate_rank);
                if mate_rank.is_some() && floored.is_none() {
                    pass.mate_suppressed += 1;
                }
            }
        }
        let mut ordered = Vec::with_capacity(candidates.len());
        for c in &candidates {
            let related = !negative && label.expected.iter().any(|e| paths_match(&c.path, e));
            let cos = cosine_of(c.score);
            if related {
                pass.related.push(cos);
            } else {
                pass.junk.push(cos);
            }
            ordered.push((c.path.clone(), cos, related));
        }
        pass.detail.push(AnchorDetail {
            anchor: label.anchor.clone(),
            negative,
            candidates: ordered,
        });
    }
    Ok(pass)
}

// ---------------------------------------------------------------------------
// The discovery fold bake-off (GH #200, Phase B) — the *default disclosure
// boundary*, priced on every bench this harness carries.
// ---------------------------------------------------------------------------

/// A **default disclosure rule**: a candidate answer to "how much of the ranked
/// list does the default view vouch for?" (invariants.md D1 as redrafted;
/// GH #200). Every variant is a **prefix** rule *by construction* — it returns a
/// depth, never a set — so a rule that admits rank 5 while folding rank 2 is not
/// expressible here. D1 states that admissibility requirement; this type makes
/// violating it unrepresentable, which is the difference between a check and a
/// guarantee.
#[derive(Clone, Copy, PartialEq)]
enum FoldRule {
    /// **Candidate 3, the incumbent**: no fold. The default view is the whole
    /// served prefix — GH #197's always-serve ruling, admissible per D1 and the
    /// thing the other candidates have to beat.
    NoFold,
    /// **Candidate 1**: mutual-kNN reciprocity in prefix form. Candidate B is
    /// *reciprocal* for anchor A iff A sits in B's own top `k` candidates, and
    /// the default view is the ranked list's longest reciprocal prefix. Rank-
    /// based (the hubness-correction family: mutual proximity, CSLS), so it
    /// carries no cosine or z constant — though `k` itself is measured below to
    /// be scale-dependent, which is a different objection and the decisive one.
    Mutual(usize),
}

impl FoldRule {
    fn label(self) -> String {
        match self {
            FoldRule::NoFold => "no fold".to_string(),
            FoldRule::Mutual(k) => format!("mutual-{k}"),
        }
    }

    /// How many of `rows` this rule's default view vouches for.
    fn fold(self, rows: &[FoldRow]) -> usize {
        match self {
            FoldRule::NoFold => rows.len(),
            FoldRule::Mutual(k) => rows.iter().take_while(|r| r.reciprocal_at(k)).count(),
        }
    }
}

/// One served candidate on one anchor's list, carrying everything a candidate
/// rule judges: the label, the score, and — the reciprocity signal — **where the
/// anchor sits in this candidate's own ranked list**.
struct FoldRow {
    path: String,
    cos: f64,
    /// A labelled mate of this anchor (always `false` on a negative or
    /// unlabelled anchor).
    mate: bool,
    /// The **anchor's** rank in *this candidate's* full-depth candidate list, or
    /// `None` if the anchor is not in it at all. Storing the rank rather than a
    /// per-`k` boolean is what makes the whole sweep below free: reciprocity at
    /// depth `k` is `rank ≤ k`, so every `k` is priced off one discovery pass
    /// per note instead of one per note per depth.
    recip_rank: Option<usize>,
}

impl FoldRow {
    fn reciprocal_at(&self, k: usize) -> bool {
        self.recip_rank.is_some_and(|r| r <= k)
    }
}

/// One anchor's served list, read once and judged by every candidate rule — so
/// the rules are compared on identical rows rather than on separate passes that
/// could drift.
struct FoldAnchor {
    anchor: String,
    /// A labelled **loner** (empty `expected`): its correct default view is
    /// *empty above the fold* — the assertion GH #197 retired, returning on the
    /// disclosure axis.
    negative: bool,
    /// Labelled mates in label order.
    expected: Vec<String>,
    /// The served prefix (`limit` = [`SIM_K`]) in rank order.
    rows: Vec<FoldRow>,
}

impl FoldAnchor {
    /// This anchor's labelled mates that the *served prefix* reaches, as
    /// `(mate, rank)`. A mate ranked past `limit` is not among them — it is
    /// always-serve's own miss at this depth, and counting it against a fold
    /// would charge every rule for a cost none of them caused.
    fn served_mates(&self) -> Vec<(String, usize)> {
        self.expected
            .iter()
            .filter_map(|m| {
                self.rows
                    .iter()
                    .position(|r| paths_match(&r.path, m))
                    .map(|p| (m.clone(), p + 1))
            })
            .collect()
    }
}

/// What one rule reads on one bench — GH #200's judged quantities, each carrying
/// the per-anchor list it is argued from (process rule 1: the aggregate is a
/// smoke alarm, the named list is the data).
struct FoldReading {
    rule: FoldRule,
    /// Labelled mates the served prefix reaches but the default view does
    /// **not** vouch for: `(anchor, mate, rank)`. **The fold's own cost**, and
    /// the quantity GH #200 judges a candidate at zero on — the suppression
    /// tripwire's disclosure-axis form. Reported rather than gated while
    /// nothing folds (it reads a structural 0 under always-serve, exactly as
    /// suppression does); GH #202 takes it into the exit gate with the surfaces
    /// if a fold ever ships.
    mates_folded: Vec<(String, String, usize)>,
    /// Labelled mates above the fold — the other half of the same count.
    mates_above: usize,
    /// Unlabelled notes the default view vouches for on a *positive* anchor —
    /// the strangers count, read above the fold instead of over the whole served
    /// prefix. Expected to shrink; deliberately ungated for the standing reason
    /// (the cheapest way to shrink it is to label the stranger).
    strangers_above: Vec<(String, String)>,
    /// Negative (loner) anchors whose default view is empty — the honest answer
    /// their label always claimed, and what always-serve cannot assert.
    neg_empty: usize,
    /// Non-negative anchors whose default view is empty. On the dense fixture
    /// **any** entry here is disqualifying (a vault where everything relates may
    /// never *default* to "nothing relates"); on the orthogonal corpus it is the
    /// same event read through the labels.
    dark_panes: Vec<String>,
    /// Cards above the fold, summed — the default view's size against
    /// always-serve's.
    cards_above: usize,
}

/// The **admissible-`k` window** for candidate 1, re-derived from this run
/// (GH #187's idiom, moved onto the disclosure axis: print the window a rule
/// *would* have rather than freezing a constant into a docstring). Two bounds
/// that must overlap for any `k` to be shippable on this bench:
///
/// - `keep_min` — the smallest `k` that folds **no** labelled mate and darkens
///   **no** pane. Below it the rule hides relations a human labelled.
/// - `loner_max` — the largest `k` at which **every** labelled loner's default
///   view is empty. Above it the fold stops making the claim it exists to make,
///   and a loner's pane fills up again.
struct FoldWindow {
    keep_min: Option<usize>,
    loner_max: Option<usize>,
    /// The smallest swept `k` whose fold equals always-serve on every anchor —
    /// where the rule becomes vacuous (it vouches for everything).
    vacuous_at: Option<usize>,
    /// The best the loner claim ever gets on this bench, as `(empty, largest k
    /// still achieving it)` — printed because "never all of them" and "four of
    /// five up to k = 11" are different findings, and only the second says how
    /// close the rule came.
    loner_best: Option<(usize, usize)>,
    /// The largest swept `k` that still darkens a pane. Every `k` at or below it
    /// is disqualified outright on a corpus where everything relates (D1's
    /// absolute), so this is the hard floor under any window.
    dark_below: Option<usize>,
}

impl FoldWindow {
    fn open(&self) -> bool {
        matches!((self.keep_min, self.loner_max), (Some(a), Some(b)) if a <= b)
    }
}

/// One bench's complete bake-off reading.
struct FoldBench {
    corpus: &'static str,
    anchors: Vec<FoldAnchor>,
    /// The headline rules: always-serve plus [`FOLD_MUTUAL_K`]'s detail depths.
    readings: Vec<FoldReading>,
    /// `(k, reading)` across the whole swept range — what the window is derived
    /// from, and printed as a compact table so the shape of the trade is visible
    /// rather than asserted.
    sweep: Vec<(usize, FoldReading)>,
    window: FoldWindow,
    /// Negative anchors on this bench (0 on the dense fixture, which has no
    /// loner by construction).
    neg_n: usize,
    /// Labelled mates the served prefix never reaches at `limit` — always-serve's
    /// own miss, identical under every rule. Printed beside the folded count so
    /// no rule is charged for it.
    mates_unserved: usize,
    /// The median *full-depth* candidate pool an anchor here has — what a
    /// reciprocity depth is a fraction OF. A `k` is only meaningful against it:
    /// "top 7" of a 14-candidate pool and "top 7" of a 30-candidate one are
    /// different claims, and the two corpora's windows below are only
    /// comparable in this unit.
    pool_median: usize,
    /// Authored edges in the built vault — **candidate 2's** entire calibration
    /// population ("what related looks like in this vault", read off the human's
    /// own committed links). Both eval corpora are link-free by construction, so
    /// this reads 0 and candidate 2 is *unpriceable here*: it is measured where
    /// its population exists, by `just calibrate` on a real vault.
    authored_edges: usize,
}

/// Every note's own ranked candidate list as `path → rank`, read at full depth —
/// the reciprocity lookup candidate 1 is judged on. Read over **every note in
/// the vault**, not only the labelled anchors: reciprocity is decided by the
/// *candidate's* list, and most candidates are not anchors.
fn reciprocity_ranks(
    vault: &Vault,
) -> Result<HashMap<String, HashMap<String, usize>>, Box<dyn std::error::Error>> {
    let mut out = HashMap::new();
    for note in vault.list_notes()? {
        let ranks = vault
            .similar(&note.path, Z_SCAN_LIMIT)?
            .into_iter()
            .enumerate()
            .map(|(i, c)| (c.path, i + 1))
            .collect::<HashMap<String, usize>>();
        out.insert(note.path, ranks);
    }
    Ok(out)
}

/// Score the fold bake-off on one built vault. `sweep_all` reads **every note**
/// as an anchor (the dense fixture's pane sweep, where the assertion is about
/// the surface rather than the labels); otherwise only the labelled anchors are
/// read, matching [`score_similar`]'s depth and population exactly.
fn score_fold(
    vault: &Vault,
    set: &SimilarSet,
    corpus: &'static str,
    sweep_all: bool,
) -> Result<FoldBench, Box<dyn std::error::Error>> {
    let recip = reciprocity_ranks(vault)?;

    let mut anchors: Vec<(String, Option<&SimilarLabel>)> = Vec::new();
    if sweep_all {
        for note in vault.list_notes()? {
            let label = set
                .anchors
                .iter()
                .find(|l| paths_match(&note.path, &l.anchor));
            anchors.push((note.path, label));
        }
    } else {
        for label in &set.anchors {
            anchors.push((label.anchor.clone(), Some(label)));
        }
    }

    let mut authored_edges = 0;
    for note in vault.list_notes()? {
        authored_edges += vault.neighbors(&note.path)?.len();
    }

    let mut rows_by_anchor: Vec<FoldAnchor> = Vec::new();
    let mut neg_n = 0;
    for (anchor, label) in anchors {
        let expected: Vec<String> = label.map(|l| l.expected.clone()).unwrap_or_default();
        // A *labelled* loner. An unlabelled anchor on the dense sweep is not a
        // negative — that fixture carries no loner at all, and reading "no
        // label" as "nothing relates" would invent the claim the bench exists
        // to test.
        let negative = label.is_some() && expected.is_empty();
        if negative {
            neg_n += 1;
        }
        let rows = vault
            .similar(&anchor, SIM_K)?
            .into_iter()
            .map(|c| FoldRow {
                mate: expected.iter().any(|e| paths_match(&c.path, e)),
                recip_rank: recip.get(&c.path).and_then(|ranks| {
                    ranks
                        .iter()
                        .find(|(p, _)| paths_match(p, &anchor))
                        .map(|(_, r)| *r)
                }),
                cos: cosine_of(c.score),
                path: c.path,
            })
            .collect();
        rows_by_anchor.push(FoldAnchor {
            anchor,
            negative,
            expected,
            rows,
        });
    }

    let mates_unserved = rows_by_anchor
        .iter()
        .map(|a| a.expected.len() - a.served_mates().len())
        .sum();
    let readings = std::iter::once(FoldRule::NoFold)
        .chain(FOLD_MUTUAL_K.iter().map(|&k| FoldRule::Mutual(k)))
        .map(|rule| read_fold(&rows_by_anchor, rule))
        .collect();
    let sweep: Vec<(usize, FoldReading)> = (1..=FOLD_K_SWEEP)
        .map(|k| (k, read_fold(&rows_by_anchor, FoldRule::Mutual(k))))
        .collect();
    let window = FoldWindow {
        keep_min: sweep
            .iter()
            .find(|(_, r)| r.mates_folded.is_empty() && r.dark_panes.is_empty())
            .map(|(k, _)| *k),
        loner_max: (neg_n > 0)
            .then(|| {
                sweep
                    .iter()
                    .filter(|(_, r)| r.neg_empty == neg_n)
                    .map(|(k, _)| *k)
                    .next_back()
            })
            .flatten(),
        vacuous_at: sweep
            .iter()
            .find(|(_, r)| {
                rows_by_anchor
                    .iter()
                    .all(|a| r.rule.fold(&a.rows) == a.rows.len())
            })
            .map(|(k, _)| *k),
        loner_best: (neg_n > 0)
            .then(|| {
                let best = sweep.iter().map(|(_, r)| r.neg_empty).max()?;
                let last = sweep
                    .iter()
                    .filter(|(_, r)| r.neg_empty == best)
                    .map(|(k, _)| *k)
                    .next_back()?;
                Some((best, last))
            })
            .flatten(),
        dark_below: sweep
            .iter()
            .filter(|(_, r)| !r.dark_panes.is_empty())
            .map(|(k, _)| *k)
            .next_back(),
    };

    let pool_median = {
        let mut sizes: Vec<usize> = recip.values().map(|r| r.len()).collect();
        sizes.sort_unstable();
        sizes.get(sizes.len() / 2).copied().unwrap_or(0)
    };

    Ok(FoldBench {
        corpus,
        pool_median,
        anchors: rows_by_anchor,
        readings,
        sweep,
        window,
        neg_n,
        mates_unserved,
        authored_edges,
    })
}

/// Judge one rule against one bench's rows.
fn read_fold(anchors: &[FoldAnchor], rule: FoldRule) -> FoldReading {
    let mut reading = FoldReading {
        rule,
        mates_folded: Vec::new(),
        mates_above: 0,
        strangers_above: Vec::new(),
        neg_empty: 0,
        dark_panes: Vec::new(),
        cards_above: 0,
    };
    for a in anchors {
        let fold = rule.fold(&a.rows);
        reading.cards_above += fold;
        if fold == 0 {
            if a.negative {
                reading.neg_empty += 1;
            } else if !a.rows.is_empty() {
                reading.dark_panes.push(a.anchor.clone());
            }
        }
        for (mate, rank) in a.served_mates() {
            if rank <= fold {
                reading.mates_above += 1;
            } else {
                reading.mates_folded.push((a.anchor.clone(), mate, rank));
            }
        }
        // The precision side, read above the fold: unlabelled cards the default
        // view vouches for on a positive anchor.
        if !a.negative && !a.expected.is_empty() {
            for row in a.rows.iter().take(fold) {
                if !row.mate {
                    reading
                        .strangers_above
                        .push((a.anchor.clone(), row.path.clone()));
                }
            }
        }
    }
    reading
}

/// The bake-off's printed readout: the headline rules side by side, the swept
/// `k` the window is derived from, and the per-anchor folds — which is the list
/// the verdict is argued from (process rule 1).
fn print_fold_bench(bench: &FoldBench) {
    println!("\n{}", "=".repeat(78));
    println!(
        "discovery fold bake-off — the default disclosure boundary on the {} corpus \
         (GH #200, Phase B; invariants.md D1)",
        bench.corpus
    );
    println!(
        "  {} anchors read at limit={SIM_K} over a median {}-candidate pool; a fold is a PREFIX of \
         the served order and everything below it stays served (D1)",
        bench.anchors.len(),
        bench.pool_median
    );
    println!(
        "\n  {:<10} {:>11}  {:>12}  {:>15}  {:>12}  {:>10}",
        "rule", "cards above", "mates folded", "strangers above", "loners empty", "dark panes"
    );
    for r in &bench.readings {
        println!(
            "  {:<10} {:>11}  {:>12}  {:>15}  {:>12}  {:>10}",
            r.rule.label(),
            r.cards_above,
            r.mates_folded.len(),
            r.strangers_above.len(),
            format!("{}/{}", r.neg_empty, bench.neg_n),
            r.dark_panes.len(),
        );
    }
    println!(
        "  (mates folded: served within limit={SIM_K} but below the fold — the fold's OWN cost, and \
         the quantity GH #200 judges a candidate at 0 on. Reported, not gated: nothing folds today, \
         so the exit gate has nothing to watch until one ships (#202). {} further labelled mate(s) \
         rank past limit={SIM_K} under every rule, always-serve included, so no fold is charged \
         for them.)",
        bench.mates_unserved
    );

    // The named cost of every rule that has one — the list, not the count.
    for r in &bench.readings {
        if r.mates_folded.is_empty() {
            continue;
        }
        println!("  {} folds labelled mates:", r.rule.label());
        for (anchor, mate, rank) in &r.mates_folded {
            println!("             rank {rank}   {anchor} → {mate}");
        }
    }

    // The swept window — the #187 idiom on the disclosure axis: derive the
    // rule's admissible range from THIS run rather than quote a constant.
    println!(
        "\n  mutual-k sweep (k = 1..{FOLD_K_SWEEP}; the window a shippable k would have to sit in)"
    );
    println!(
        "  {:>3}  {:>11}  {:>12}  {:>15}  {:>12}  {:>10}",
        "k", "cards above", "mates folded", "strangers above", "loners empty", "dark panes"
    );
    for (k, r) in &bench.sweep {
        println!(
            "  {k:>3}  {:>11}  {:>12}  {:>15}  {:>12}  {:>10}",
            r.cards_above,
            r.mates_folded.len(),
            r.strangers_above.len(),
            format!("{}/{}", r.neg_empty, bench.neg_n),
            r.dark_panes.len(),
        );
    }
    let frac = |k: usize| {
        if bench.pool_median == 0 {
            String::new()
        } else {
            format!(
                " (= {:.2} of the {}-candidate pool)",
                k as f64 / bench.pool_median as f64,
                bench.pool_median
            )
        }
    };
    match bench.window.keep_min {
        Some(k) => println!(
            "    k ≥ {k}{}   folds no labelled mate and darkens no pane on this corpus",
            frac(k)
        ),
        None => println!("    (no swept k folds zero mates with zero dark panes on this corpus)"),
    }
    match bench.window.loner_max {
        Some(k) => println!(
            "    k ≤ {k}{}   folds every labelled loner's default view to empty",
            frac(k)
        ),
        None if bench.neg_n == 0 => {
            println!(
                "    (no labelled loner on this corpus — the upper bound is the other bench's)"
            )
        }
        None => println!("    (no swept k empties every loner's default view on this corpus)"),
    }
    if let Some(k) = bench.window.vacuous_at {
        println!(
            "    k ≥ {k}   the fold equals always-serve on every anchor here — the rule stops \
             claiming anything"
        );
    }
    if let Some(k) = bench.window.dark_below {
        println!(
            "    k ≤ {k}   darkens at least one pane here — disqualified outright where the labels \
             say everything relates (D1's absolute)"
        );
    }
    if let Some((best, last)) = bench.window.loner_best {
        println!(
            "    best loner claim: {best}/{} empty, holding to k = {last}",
            bench.neg_n
        );
    }
    // The verdict names *which* bound failed. "No k works" and "this corpus
    // cannot decide alone" are different sentences, and a bench with no loner
    // can only ever supply the lower bound.
    println!(
        "    → window {}",
        match (bench.neg_n, bench.window.keep_min, bench.window.loner_max) {
            (0, Some(k), _) => format!(
                "UNDECIDABLE on this corpus alone — no loner here, so it supplies only the lower \
                 bound (k ≥ {k}) and the absolute above; the upper bound is the other bench's"
            ),
            (0, None, _) => "UNDECIDABLE on this corpus alone — no loner here, and no swept k is \
                 even clean on the labels"
                .to_string(),
            (_, _, None) => format!(
                "EMPTY — no swept k empties every loner's default view, so the fold never fully \
                 makes the claim it exists to make ({})",
                bench
                    .window
                    .keep_min
                    .map(|k| format!("its lower bound here is k ≥ {k}"))
                    .unwrap_or_else(|| "and no swept k is clean on the labels either".into())
            ),
            (_, Some(a), Some(b)) if a <= b => format!(
                "OPEN on this corpus at k ∈ [{a}, {b}] — the other benches are the rest \
                     of the claim"
            ),
            (_, Some(a), Some(b)) => format!(
                "EMPTY — the k that stops folding labelled mates (≥ {a}) is past the k that still \
                 empties every loner's view (≤ {b}), so no constant separates them"
            ),
            (_, None, Some(b)) => format!(
                "EMPTY — no swept k is clean on the labels at all, while the loner claim holds \
                 only to k ≤ {b}"
            ),
        }
    );

    // The per-anchor lists at the headline depths.
    println!("\n  per anchor — served / above the fold, by rule:");
    print!("  {:<40} {:>7}", "anchor", "served");
    for r in &bench.readings {
        if r.rule != FoldRule::NoFold {
            print!(" {:>9}", r.rule.label());
        }
    }
    println!();
    for a in &bench.anchors {
        print!(
            "  {:<40} {:>7}",
            format!(
                "{}{}",
                truncate(&a.anchor, 36),
                if a.negative { " [loner]" } else { "" }
            ),
            a.rows.len()
        );
        for r in &bench.readings {
            if r.rule != FoldRule::NoFold {
                let fold = r.rule.fold(&a.rows);
                let lost = a
                    .served_mates()
                    .iter()
                    .filter(|(_, rank)| *rank > fold)
                    .count();
                print!(
                    " {:>9}",
                    if lost > 0 {
                        format!("{fold}(-{lost})")
                    } else {
                        fold.to_string()
                    }
                );
            }
        }
        println!();
    }

    // Candidate 2's census — printed on every bench, because "we did not measure
    // it" and "there was nothing to measure it on" are different sentences and
    // only the second is an argument.
    println!(
        "\n  candidate 2 (authored-edge reference bar): {} authored edges in this corpus — {}",
        bench.authored_edges,
        if bench.authored_edges == 0 {
            "UNPRICEABLE here (a link-free corpus offers the rule no population to calibrate from; \
             it is measured where one exists, by `just calibrate` on a real vault)"
        } else {
            "priceable — see the calibrate replay"
        }
    );
}

/// The bake-off as one JSON subtree (`discovery_fold`): every rule's reading,
/// the swept window it was judged against, and the per-anchor rows both were
/// computed from — so a verdict cited from a row can be re-derived without
/// re-running the model, which is what `results.jsonl` is for.
fn fold_json(bench: &FoldBench) -> serde_json::Value {
    let reading = |r: &FoldReading| {
        serde_json::json!({
            "rule": r.rule.label(),
            "cards_above": r.cards_above,
            "mates_above": r.mates_above,
            "mates_folded": r.mates_folded.iter().map(|(a, m, rank)| serde_json::json!({
                "anchor": a, "mate": m, "rank": rank,
            })).collect::<Vec<_>>(),
            "strangers_above": r.strangers_above.iter().map(|(a, p)| serde_json::json!({
                "anchor": a, "path": p,
            })).collect::<Vec<_>>(),
            "neg_empty": r.neg_empty,
            "dark_panes": r.dark_panes,
        })
    };
    serde_json::json!({
        "corpus": bench.corpus,
        "limit": SIM_K,
        "pool_median": bench.pool_median,
        "neg_n": bench.neg_n,
        // Always-serve's own miss at this limit, recorded apart from every
        // rule's cost so the two can never be added together by a later reader.
        "mates_unserved": bench.mates_unserved,
        // Candidate 2's population, recorded even at zero: "unpriceable here"
        // is a measurement, and a row that omitted it would read as untried.
        "authored_edges": bench.authored_edges,
        "rules": bench.readings.iter().map(reading).collect::<Vec<_>>(),
        "sweep": bench.sweep.iter().map(|(k, r)| serde_json::json!({
            "k": k,
            "reading": reading(r),
        })).collect::<Vec<_>>(),
        "window": {
            "swept_to": FOLD_K_SWEEP,
            "keep_min": bench.window.keep_min,
            "loner_max": bench.window.loner_max,
            "vacuous_at": bench.window.vacuous_at,
            "dark_below": bench.window.dark_below,
            "loner_best": bench.window.loner_best.map(|(empty, k)| serde_json::json!({
                "empty": empty, "of": bench.neg_n, "holds_to_k": k,
            })),
            "open": bench.window.open(),
        },
        "anchors": bench.anchors.iter().map(|a| serde_json::json!({
            "anchor": a.anchor,
            "negative": a.negative,
            "served": a.rows.len(),
            "folds": bench.readings.iter().map(|r| serde_json::json!({
                "rule": r.rule.label(),
                "fold": r.rule.fold(&a.rows),
            })).collect::<Vec<_>>(),
            "rows": a.rows.iter().map(|row| serde_json::json!({
                "path": row.path,
                "cos": (row.cos * 1e4).round() / 1e4,
                "mate": row.mate,
                // The anchor's rank in THIS candidate's own list — the whole
                // reciprocity signal, from which any k is re-derivable.
                "recip_rank": row.recip_rank,
            })).collect::<Vec<_>>(),
        })).collect::<Vec<_>>(),
    })
}

/// Dump every candidate's **band-unit z** (stage-2 best-passage z, GH #192) on
/// every discovery anchor — the deep discovery pass (GH #187).
///
/// Since GH #197 the z travels ungated on the one shipped surface, so the dump
/// is simply `similar` read at [`Z_SCAN_LIMIT`] rather than `SIM_K`: a bar is
/// calibrated against the population it has to cut, and the strangers just
/// under a served prefix are precisely the ones a lower bar would admit. (This
/// used to need the engine's floor moved out of the way with `-∞` bars; the
/// machinery went with the gate.)
fn score_floor_z(vault: &Vault, set: &SimilarSet) -> Result<FloorZ, Box<dyn std::error::Error>> {
    let mut dump = FloorZ::default();
    for label in &set.anchors {
        let candidates = vault.similar(&label.anchor, Z_SCAN_LIMIT)?;
        // z is uniform within one query by construction (`discover::candidates`
        // gives every surviving candidate a z or none of them), so the leader's
        // presence decides the whole list — an anchor with no statistics is
        // named, never half-counted.
        let Some(true) = candidates.first().map(|c| c.z.is_some()) else {
            dump.ungraded.push(label.anchor.clone());
            continue;
        };
        // The band z, recomputed harness-side over the same population the
        // engine just served: z over squared best-pair distance (score is
        // negated L2, so d² = score²), oriented nearer = higher. Should equal
        // the engine's own z to fp noise — the statistic-level drift check.
        let d2: Vec<f64> = candidates.iter().map(|c| c.score * c.score).collect();
        let recheck = passage_z(&d2);
        dump.anchors.push(AnchorZ {
            anchor: label.anchor.clone(),
            negative: label.expected.is_empty(),
            candidates: candidates
                .iter()
                .enumerate()
                .filter_map(|(i, c)| {
                    let mate = label.expected.iter().any(|e| paths_match(&c.path, e));
                    c.z.map(|z| ZCand {
                        path: c.path.clone(),
                        z,
                        z_recheck: recheck.as_ref().map(|r| r[i]),
                        score: c.score,
                        mate,
                    })
                })
                .collect(),
        });
    }
    Ok(dump)
}

/// One labelled query's evidence reading (invariants.md D2; GH #201, Phase A of
/// the disclosure work): the signals a query-level evidence rule would judge,
/// dumped before any rule exists so GH #201 derives its rule from measurement
/// rather than assumption. Nothing here gates anything.
struct QueryEvidence {
    query: String,
    /// Chunks the OR-sanitized FTS5 expression matches at all. **Not** a
    /// lexical-anchor test on phrase queries — `fts5_query` ORs every
    /// alphanumeric term, so stopwords saturate this count; recorded precisely
    /// so that saturation stays measured instead of assumed away.
    bm25_hits: usize,
    /// Best BM25 score over those matches, sign-flipped so higher = better
    /// (FTS5's `rank` is more-negative-is-better). `None` when nothing matches
    /// — the honest zero the fused surface currently cannot say.
    bm25_best: Option<f64>,
    /// Best cosine between the embedded query and any stored chunk vector (the
    /// dense top-1) — the strongest semantic evidence the vault holds for this
    /// query, which RRF discards before the surface sees it.
    best_cos: Option<f64>,
    /// The note that dense top-1 belongs to, naming what the number points at.
    top: String,
    /// What the shipped surface serves at [`K`] today. For a negative query
    /// this is the measured D2 defect: `limit` confident-looking results for a
    /// query the vault holds nothing for.
    served: usize,
}

/// The search evidence dump (GH #201): every labelled query's reading, split
/// into the piles a query-level evidence bar answers to — positives it must
/// keep reachable, negatives it must answer "no matches".
struct SearchEvidence {
    positives: Vec<QueryEvidence>,
    negatives: Vec<QueryEvidence>,
}

/// One side's best-cos pile — the population the query-window derivation and
/// the printed pile lines both read (queries with no dense reading drop out).
fn best_cos_pile(rows: &[QueryEvidence]) -> Vec<f64> {
    rows.iter().filter_map(|r| r.best_cos).collect()
}

/// Score the search evidence calibration (invariants.md D2; GH #201) — a pure
/// read over the already-built vault. Per labelled query: the lexical half's
/// reading via a direct `chunks_fts` probe (the engine's own sanitized
/// expression — the absolute signals RRF discards), the dense top-1 via the
/// vector-only ablation path, and the shipped surface's served count.
fn score_search_evidence(
    vault_root: &Path,
    vault: &Vault,
    positives: &[Labelled],
    negatives: &[Labelled],
) -> Result<SearchEvidence, Box<dyn std::error::Error>> {
    // A second read connection beside the Vault's own — C1: readers are
    // unrestricted, and the probe wants FTS5's `rank`, which no façade read
    // exposes (deliberately: bm25 units are engine internals everywhere but
    // this instrument).
    let conn = b2_core::open(&vault_root.join(".b2").join("b2.sqlite"))?;
    let rows = |queries: &[Labelled]| -> Result<Vec<QueryEvidence>, Box<dyn std::error::Error>> {
        queries
            .iter()
            .map(|q| {
                let (bm25_hits, bm25_best) = bm25_probe(&conn, &q.query)?;
                let dense = vault.search_vector_only(&q.query, 1)?;
                let best_cos = dense.first().map(|h| cosine_of(h.score));
                let top = dense
                    .first()
                    .map(|h| h.path.clone())
                    .unwrap_or_else(|| "—".to_string());
                let served = vault.search(&q.query, K)?.len();
                Ok(QueryEvidence {
                    query: q.query.clone(),
                    bm25_hits,
                    bm25_best,
                    best_cos,
                    top,
                    served,
                })
            })
            .collect()
    };
    Ok(SearchEvidence {
        positives: rows(positives)?,
        negatives: rows(negatives)?,
    })
}

/// The lexical half's reading for one raw query, probed directly over
/// `chunks_fts` with the engine's own sanitized expression
/// ([`b2_core::search::fts5_query`]): how many chunks match at all, and the
/// best BM25 score among them (FTS5 `rank`, sign-flipped so higher = better).
/// These are exactly the absolute signals the fused path computes and then
/// discards at RRF — which is why the instrument reads them raw.
fn bm25_probe(
    conn: &rusqlite::Connection,
    query: &str,
) -> Result<(usize, Option<f64>), Box<dyn std::error::Error>> {
    let expr = b2_core::search::fts5_query(query);
    if expr.is_empty() {
        return Ok((0, None));
    }
    let hits: i64 = conn.query_row(
        "SELECT count(*) FROM chunks_fts WHERE chunks_fts MATCH ?1",
        [&expr],
        |r| r.get(0),
    )?;
    let hits = usize::try_from(hits).unwrap_or(0);
    if hits == 0 {
        return Ok((0, None));
    }
    let best: f64 = conn.query_row(
        "SELECT rank FROM chunks_fts WHERE chunks_fts MATCH ?1 ORDER BY rank LIMIT 1",
        [&expr],
        |r| r.get(0),
    )?;
    Ok((hits, Some(-best)))
}

/// Print the search evidence calibration (invariants.md D2; GH #201) — the
/// query-side sibling of [`print_floor_windows`]. Reported, nothing gates: the
/// piles are what GH #201's query-level bar is argued from, and any constant
/// read off them owes process rule 5's real-vault transfer check before it
/// ships. The window's caveat is structural: D2's rule is lexical OR semantic
/// evidence, so a *pure-cosine* window overstates what a real bar must keep —
/// a positive query with a lexical anchor never needs its cosine kept.
fn print_search_evidence(ev: &SearchEvidence) {
    println!(
        "  search evidence calibration (D2 — reported, nothing gates; the query bar is GH #201's \
         to earn)"
    );
    let pos_cos = best_cos_pile(&ev.positives);
    let neg_cos = best_cos_pile(&ev.negatives);
    for (label, pile, role) in [
        (
            "pos best-cos",
            &pos_cos,
            "a pure-cosine bar would have to KEEP",
        ),
        ("neg best-cos", &neg_cos, "any query bar would have to CUT"),
    ] {
        match pile_stats(pile) {
            Some((min, med, max)) => println!(
                "    {label:<12} n={:<4} min/med/max {min:.3}/{med:.3}/{max:.3}   ← {role}",
                pile.len()
            ),
            None => println!("    {label:<12} n=0    (nothing labelled — no reading)"),
        }
    }
    let hits: Vec<f64> = ev.positives.iter().map(|r| r.bm25_hits as f64).collect();
    if let Some((min, med, max)) = pile_stats(&hits) {
        println!(
            "    pos bm25     hits min/med/max {min:.0}/{med:.0}/{max:.0}   (OR-saturated: stopwords \
             match too, so a raw count is not a lexical anchor — GH #201 derives one from this dump)"
        );
    }
    match Window::read(&neg_cos, &pos_cos) {
        None => println!(
            "    cos window   no reading (a pile is empty — label negative queries to give GH #201 \
             its CUT side)"
        ),
        Some(w) if w.open() => println!(
            "    cos window   ({:.3}, {:.3}]  — open on THIS corpus for a pure-cosine query bar; \
             the real keep-set is smaller (lexical evidence keeps its own), and a real vault is the \
             other half of any such claim (process rule 5, `just calibrate`)",
            w.cut_max, w.keep_min
        ),
        Some(w) => println!(
            "    cos window   EMPTY for a pure-cosine bar — negatives reach {:.3} while positives \
             start at {:.3}; D2's two-signal rule (lexical OR semantic) is argued from the per-query \
             lines, not this window",
            w.cut_max, w.keep_min
        ),
    }
    if ev.negatives.is_empty() {
        println!(
            "    negatives    n=0   (none labelled — queries.json takes empty `relevant` as \
             \"no matches\")"
        );
    } else {
        println!(
            "    negatives (labelled answer: NO MATCHES — what the shipped surface serves instead):"
        );
        for r in &ev.negatives {
            println!(
                "      {:<40} bm25 {:>3} hit{}  best {:>6}  cos {:>6}  served {:>2}/{K}  top {}",
                truncate(&r.query, 40),
                r.bm25_hits,
                if r.bm25_hits == 1 { " " } else { "s" },
                r.bm25_best
                    .map(|b| format!("{b:.2}"))
                    .unwrap_or_else(|| "—".to_string()),
                r.best_cos
                    .map(|c| format!("{c:.3}"))
                    .unwrap_or_else(|| "—".to_string()),
                r.served,
                r.top,
            );
        }
    }
}

/// A `similar` score is negated L2 distance between L2-normalized vectors
/// (the real embedder normalizes every row), so it converts exactly:
/// `cos = 1 − d²/2`. Cosine is the unit the floor ruling is stated in and the
/// unit that survives a model swap comparison, so the piles are recorded in it.
fn cosine_of(score: f64) -> f64 {
    1.0 - (score * score) / 2.0
}

/// Z-score a population of squared distances, oriented nearer = higher — the
/// harness's own restatement of the arithmetic `discover::candidates` applies
/// to the stage-2 best-pair distances (GH #192), kept so the engine's z can be
/// cross-checked rather than merely trusted. `None` when no meaningful
/// statistic exists (under two values, or zero variance), mirroring the
/// engine's own inertness guard.
fn passage_z(d2: &[f64]) -> Option<Vec<f64>> {
    if d2.len() < 2 {
        return None;
    }
    let n = d2.len() as f64;
    let mean = d2.iter().sum::<f64>() / n;
    let var = d2.iter().map(|d| (d - mean).powi(2)).sum::<f64>() / (n - 1.0);
    let sd = var.sqrt();
    (sd > 0.0).then(|| d2.iter().map(|d| (mean - d) / sd).collect())
}

/// (min, median, max) of a pile, or None while it's empty.
fn pile_stats(pile: &[f64]) -> Option<(f64, f64, f64)> {
    if pile.is_empty() {
        return None;
    }
    let mut sorted = pile.to_vec();
    sorted.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let mid = sorted.len() / 2;
    let median = if sorted.len().is_multiple_of(2) {
        (sorted[mid - 1] + sorted[mid]) / 2.0
    } else {
        sorted[mid]
    };
    Some((sorted[0], median, sorted[sorted.len() - 1]))
}

/// `LocalEmbedder::embed_batch` must be a faithful map of `embed`: right-padding
/// short rows to the batch's longest and masking them out has to leave each row's
/// CLS vector unchanged. The reindex path batches freely, so a regression here
/// would silently corrupt every stored vector — and every score this eval prints.
///
/// This lives in the eval rather than in `cargo test` because it needs the
/// provisioned model, which the fast suite deliberately never touches (root
/// `CLAUDE.md`, "Keep `cargo test` fast, deterministic, and model-free"). Running
/// it here means it actually runs, on every `just eval`, instead of sitting behind
/// an `#[ignore]` nobody passes `--ignored` to.
fn check_batch_matches_single(model: &LocalEmbedder) -> Result<(), Box<dyn std::error::Error>> {
    // Deliberately varied lengths, so batching pads the short rows to the longest.
    let texts = [
        "Spaced repetition schedules reviews at increasing intervals.",
        "Sleep consolidates memory.",
        "Short.",
        "Focus and sustained attention shape what is later recalled from long-term memory across days.",
    ];
    let refs: Vec<&str> = texts.to_vec();
    let batched = model.embed_batch(&refs)?;
    if batched.len() != texts.len() {
        return Err(format!(
            "embed_batch returned {} rows for {} texts",
            batched.len(),
            texts.len()
        )
        .into());
    }
    let mut worst = f32::INFINITY;
    for (text, batched_row) in texts.iter().zip(&batched) {
        let single = model.embed(text)?;
        if batched_row.len() != single.len() {
            return Err(format!("batched/single dim mismatch for {text:?}").into());
        }
        // Both rows are L2-normalized, so the dot product is cosine similarity;
        // padding must not move it off ~1.0. Non-finite is checked first and
        // explicitly: every comparison against a NaN is false, so `cos <= 0.9999`
        // alone would wave a NaN row *through* the gate — the one failure mode a
        // correctness check must not have.
        let cos: f32 = batched_row.iter().zip(&single).map(|(a, b)| a * b).sum();
        if !cos.is_finite() {
            return Err(format!("batched embedding is non-finite for {text:?}: {cos}").into());
        }
        worst = worst.min(cos);
        if cos <= 0.9999 {
            return Err(format!(
                "batched embedding differs from single for {text:?}: cosine {cos}"
            )
            .into());
        }
    }
    eprintln!("[eval] batch ≡ single: worst-row cosine {worst:.6}\n");
    Ok(())
}

/// Run the embed pass, timing it and counting the chunks it filled.
fn timed_embed(vault: &Vault) -> Result<(usize, f64), Box<dyn std::error::Error>> {
    let mut chunks = 0usize;
    let t0 = Instant::now();
    vault.embed(&mut |p| {
        chunks = p.chunks_done;
        ControlFlow::Continue(())
    })?;
    Ok((chunks, t0.elapsed().as_secs_f64()))
}

/// State the one thing this corpus **cannot** measure, on every run that can't.
///
/// Retrieval pulls [`note_candidate_pool`] candidates from each signal before
/// fusing, or [`chunk_candidate_pool`] for the passage view. A corpus with no more
/// chunks than that truncates *neither* list — BM25 has fewer matches than its
/// `LIMIT`, the vector scan tops out at the stored vectors — so both lists are
/// already complete, widening the pool cannot add a candidate, and every score above
/// is invariant under **candidate width**. A change to either view's headroom or to
/// `search::pool_size` then prints bit-identical numbers here while genuinely
/// reordering a real vault (GH #141; the worked example is GH #140/#142).
///
/// Judged on the **narrower** of the two pools, which since #142 is the passage
/// view's: blindness is a claim about every number the run prints, and a corpus that
/// fits inside the narrower pool fits inside the wider one too.
///
/// Scoped deliberately to width. `RRF_K` re-weights the *same* two lists
/// (`Σ 1/(k+rank+1)`), so it reorders results on any corpus — measured on this one:
/// k = 60 → 10 moves note ranks across the query set. This eval sees that; it is
/// only blind to candidates it was never going to be handed.
///
/// A warning, not a gate: this eval is out-of-CI and human-read, and the point is
/// that a reader must not take an unmoved number as evidence of no change. The
/// property itself is measured by the rank-stability probe (`--example stability`),
/// which runs on a vault big enough for the pool to bind.
fn warn_if_pool_blind(chunks: usize) {
    let pool = chunk_candidate_pool(K).min(note_candidate_pool(K));
    if chunks <= pool {
        eprintln!(
            "[warn] {chunks} chunks ≤ {pool}-candidate pool — neither signal is truncated here, so a\n\
             \x20      candidate-width change (either hit pool, pool_size) cannot move any number in this run\n\
             \x20      (GH #141). `just stability` measures that property on a large vault. (RRF_K is\n\
             \x20      not in that set — it re-weights the same lists, and this corpus does see it.)\n"
        );
    }
}

fn print_default_report(
    queries: &[Labelled],
    bm25: &Pass,
    vector: &Pass,
    hybrid: &Pass,
    similar: &SimilarPass,
    floor_z: &FloorZ,
) {
    println!(
        "{:>5} {:>6} {:>6} {:>6}  {:<40}  top hybrid hit",
        "bm25", "vec", "hybrid", "chunk", "query"
    );
    println!("{}", "-".repeat(102));
    for (i, q) in queries.iter().enumerate() {
        println!(
            "{:>5} {:>6} {:>6} {:>6}  {:<40}  {}",
            rank_str(bm25.scores[i].note),
            rank_str(vector.scores[i].note),
            rank_str(hybrid.scores[i].note),
            match q.passage {
                Some(_) => rank_str(hybrid.scores[i].chunk),
                None => "".to_string(),
            },
            truncate(&q.query, 40),
            hybrid.scores[i].top,
        );
    }

    println!("\n{}", "=".repeat(78));
    println!("note rank (n={}, K={K}):", queries.len());
    println!(
        "  bm25-only  hit@1={:.2}  hit@3={:.2}  MRR@{K}={:.3}",
        bm25.note.hit1(),
        bm25.note.hit3(),
        bm25.note.mrr()
    );
    println!(
        "  vec-only   hit@1={:.2}  hit@3={:.2}  MRR@{K}={:.3}",
        vector.note.hit1(),
        vector.note.hit3(),
        vector.note.mrr()
    );
    println!(
        "  hybrid     hit@1={:.2}  hit@3={:.2}  MRR@{K}={:.3}   semantic lift: {:+.2} hit@1",
        hybrid.note.hit1(),
        hybrid.note.hit3(),
        hybrid.note.mrr(),
        hybrid.note.hit1() - bm25.note.hit1(),
    );
    // The standing form of the fusion finding (GH #158): every query
    // where fusing the two signals ranked the labelled answer WORSE than the
    // dense signal alone would have. RRF's consensus bias makes some of this
    // inevitable; the point is that it is counted and named on every run instead
    // of rediscovered by hand-decomposing scores.
    let demoted: Vec<usize> = (0..queries.len())
        .filter(|&i| {
            let Some(v) = vector.scores[i].note else {
                return false;
            };
            match hybrid.scores[i].note {
                Some(h) => h > v,
                None => true,
            }
        })
        .collect();
    if demoted.is_empty() {
        println!("  fusion     no query ranks worse under hybrid than under vector alone");
    } else {
        println!(
            "  fusion     {} quer{} rank worse under hybrid than under vector alone:",
            demoted.len(),
            if demoted.len() == 1 { "y" } else { "ies" }
        );
        for &i in &demoted {
            println!(
                "             vec {} → hybrid {}   {}",
                rank_str(vector.scores[i].note),
                rank_str(hybrid.scores[i].note),
                truncate(&queries[i].query, 48)
            );
        }
    }
    if hybrid.chunk.n > 0 {
        println!("chunk rank (passage-labelled, n={}):", hybrid.chunk.n);
        println!(
            "  bm25-only  hit@1={:.2}  hit@3={:.2}  MRR@{K}={:.3}",
            bm25.chunk.hit1(),
            bm25.chunk.hit3(),
            bm25.chunk.mrr()
        );
        println!(
            "  hybrid     hit@1={:.2}  hit@3={:.2}  MRR@{K}={:.3}",
            hybrid.chunk.hit1(),
            hybrid.chunk.hit3(),
            hybrid.chunk.mrr()
        );
    }
    println!(
        "similar (n={} positive + {} negative, K={SIM_K}):",
        similar.rank.n, similar.neg_n
    );
    // The per-anchor metric (first mate found, then stop) is **no longer
    // printed** (GH #188): it read 1.000 across every change it was meant to
    // judge, and a line that cannot move is a line that trains skimming. It is
    // still recorded — `results.jsonl`'s `"similar"` key is unchanged, so rows
    // stay comparable back to the first run — so retiring the line costs the
    // dataset nothing.
    println!(
        "  per-mate   hit@1={:.2}  hit@3={:.2}  MRR@{SIM_K}={:.3}  (n={} mates, GATED at MRR@{SIM_K} ≥ {FLOOR_MATE_MRR:.2})",
        similar.mate.hit1(),
        similar.mate.hit3(),
        similar.mate.mrr(),
        similar.mate.n
    );
    // The pass-vs-pass suppression tripwire (see `SimilarPass::mate_raw`):
    // silent at its structural 0, loud the day an existence gate is back in
    // the path. No "unfloored" comparison line any more — both passes read the
    // one always-serve surface, and a line that cannot move trains skimming.
    if similar.mate_suppressed > 0 {
        println!(
            "   └ {} of {} labelled mates are SUPPRESSED — pass 2 reaches a mate pass 1 never\n\
             \x20    serves, which under always-serve (GH #197) means an existence gate is back in\n\
             \x20    the path (GATED at ≤ {MAX_MATES_SUPPRESSED})",
            similar.mate_suppressed, similar.mate.n
        );
    }
    // Discovery's precision side (GH #188) — reported with names, never gated;
    // see `SimilarPass::strangers` for why gating it would reward labelling.
    println!(
        "  strangers  {} card{} on {}/{} positive anchors (unlabelled notes served in the top-{SIM_K} — \
         a smoke alarm, not a gate: labels aren't exhaustive)",
        similar.strangers.len(),
        if similar.strangers.len() == 1 { "" } else { "s" },
        similar.stranger_anchors,
        similar.rank.n
    );
    for (anchor, path) in &similar.strangers {
        println!("             {anchor} → {path}");
    }
    if similar.neg_n > 0 {
        // Under always-serve (GH #197) a negative anchor serves its ranked
        // nearest like any other — that is the ruling, not a regression. What
        // these anchors measure now is what the served cards *claim*: their
        // bands (A2's readout, printed per leader in the calibration block).
        println!(
            "  negatives  {} loner anchors serve {} cards under always-serve (labels still say \
             \"nothing relates\"; the bands carry the honesty — leaders below)",
            similar.neg_n, similar.neg_cards
        );
    }
    // The two calibration piles. If they separate, the gap IS the floor, read
    // off measured data; if they overlap, no simple floor can hold and the
    // escalation path (a discovery-side pair-scorer) is justified by data.
    if let (Some((r_min, r_med, r_max)), Some((j_min, j_med, j_max))) =
        (pile_stats(&similar.related), pile_stats(&similar.junk))
    {
        println!(
            "  piles(cos) related n={:<3} min/med/max {:.3}/{:.3}/{:.3}",
            similar.related.len(),
            r_min,
            r_med,
            r_max
        );
        println!(
            "             junk    n={:<3} min/med/max {:.3}/{:.3}/{:.3}",
            similar.junk.len(),
            j_min,
            j_med,
            j_max
        );
        let gap = r_min - j_max;
        println!(
            "             related-min − junk-max = {:+.3} ({})",
            gap,
            if gap > 0.0 {
                "piles separate — the gap is the floor"
            } else {
                "piles overlap — a simple floor cuts both"
            }
        );
    }
    // The same question in the floor's own anchor-relative unit, where an
    // answer is actionable: the piles are absolute cosines, and no constant in
    // the code is ever compared against one.
    print_floor_windows(floor_z);
}

/// Re-derive the admissible windows a z existence rule *would* have on the
/// corpus as it stands today (GH #187) — the standing record of why none ships
/// (GH #197), and the first reading any Phase-2 bake-off candidate answers to.
///
/// Printed every run, and recorded in the row, precisely so no number here ever
/// gets copied into a doc comment again: the #150 windows were measured once,
/// frozen into a rustdoc, and silently falsified the first time the corpus grew
/// a note shape they were never derived from. The instrument is the citable
/// thing; any constant stays in the code it governs.
fn print_floor_windows(z: &FloorZ) {
    let (mates, strangers) = (z.mate_z(), z.stranger_z());
    let (neg_leaders, pos_leaders) = (z.neg_leader_z(), z.pos_leader_z());
    println!(
        "  discovery z calibration (stage-2 best-passage z — the band's input; gates NOTHING \
         since GH #197)"
    );
    // The statistic-level instrument check: the dump's z is recomputed from the
    // served scores, so a drift in the engine's statistic can't pass silently
    // (tolerance covers f32 sqrt/square round-trip noise).
    let recheck = z.recheck_delta();
    if recheck <= 1e-3 {
        println!("    [check] harness recomputation matches the engine z (max Δ {recheck:.1e})");
    } else {
        println!(
            "    [FAULT] harness recomputation disagrees with the engine z by up to {recheck:.3} — \
             the statistic moved; distrust every reading below"
        );
    }
    for (label, pile, role) in [
        ("mates", &mates, "a member bar would have to KEEP"),
        (
            "strangers",
            &strangers,
            "a member bar would have to CUT (positive anchors)",
        ),
        (
            "neg leaders",
            &neg_leaders,
            "a leader gate would have to CUT",
        ),
        (
            "pos leaders",
            &pos_leaders,
            "a leader gate would have to KEEP",
        ),
    ] {
        match pile_stats(pile) {
            Some((min, med, max)) => println!(
                "    {label:<12} n={:<4} min/med/max {min:+.3}/{med:+.3}/{max:+.3}   ← {role}",
                pile.len()
            ),
            None => println!("    {label:<12} n=0    (nothing labelled — no reading)"),
        }
    }
    for (name, win) in [
        ("leader", Window::read(&neg_leaders, &pos_leaders)),
        ("member", Window::read(&strangers, &mates)),
    ] {
        match win {
            None => println!("    {name} window  no reading (a population was empty)"),
            Some(w) if w.open() => println!(
                "    {name} window  ({:+.3}, {:+.3}]  — open on THIS corpus; a real vault is the \
                 other half of any such claim (process rule 5, `just calibrate`)",
                w.cut_max, w.keep_min
            ),
            Some(w) => println!(
                "    {name} window  EMPTY — the population it must cut reaches {:+.3} while the one \n\
                 \x20                 it must keep starts at {:+.3}; the two INVERT, and no constant \n\
                 \x20                 separates an inversion",
                w.cut_max, w.keep_min
            ),
        }
    }
    // Every negative anchor's leader with the band it paints — A2's readout:
    // under always-serve these cards ARE served, and the band is what they
    // claim to the human whose labels say "nothing relates".
    println!(
        "    negative anchors' leaders (served under always-serve; the band carries the honesty):"
    );
    for a in z.anchors.iter().filter(|a| a.negative) {
        match a.candidates.first() {
            Some(c) => println!(
                "      {} → {}  {:+.3}  {}",
                a.anchor,
                c.path,
                c.z,
                band_glyph(c.z)
            ),
            None => println!("      {}  (no candidates)", a.anchor),
        }
    }
    if !z.ungraded.is_empty() {
        println!(
            "    [warn] no z statistics for {} anchor(s) ({}) — pool under STATS_MIN_POPULATION or \
             zero variance; the windows above are measured without them",
            z.ungraded.len(),
            z.ungraded.join(", ")
        );
    }
}

/// The strength band a z paints (`ui/src/strength.ts`'s landmarks, restated —
/// see [`BAND_STRONG_Z`]).
fn band_glyph(z: f64) -> &'static str {
    if z >= BAND_STRONG_Z {
        "●●●"
    } else if z >= BAND_CLEAR_Z {
        "●●○"
    } else {
        "●○○"
    }
}

/// The paired per-query diff between two fused passes — what an A/B is actually
/// judged on. At this corpus's n, every aggregate delta is worth 1–2 queries, so
/// "hit@1 +0.05" and "these two queries flipped, this one broke" are the same
/// fact — but only the second form can be argued with, per-query, against the
/// labels (docs/evals/README.md, the process rules). Prints nothing but a
/// no-moves line when the variant reproduced the reference ranking exactly —
/// which, per the same rules, is itself a claim to verify against a
/// continuous quantity (the piles), never bare proof of "no effect".
fn print_rank_moves(queries: &[Labelled], reference: &Pass, variant: &Pass) {
    let improved = |a: Option<usize>, b: Option<usize>| match (a, b) {
        (None, Some(_)) => true,
        (Some(x), Some(y)) => y < x,
        _ => false,
    };
    let mut note_up = 0usize;
    let mut note_down = 0usize;
    let mut lines = Vec::new();
    for (i, q) in queries.iter().enumerate() {
        let (a, b) = (reference.scores[i].note, variant.scores[i].note);
        if a != b {
            if improved(a, b) {
                note_up += 1;
            } else {
                note_down += 1;
            }
            lines.push(format!(
                "    Δ note   {:>5} → {:<5}  {}",
                rank_str(a),
                rank_str(b),
                truncate(&q.query, 48)
            ));
        }
        if q.passage.is_some() {
            let (a, b) = (reference.scores[i].chunk, variant.scores[i].chunk);
            if a != b {
                lines.push(format!(
                    "    Δ chunk  {:>5} → {:<5}  {}",
                    rank_str(a),
                    rank_str(b),
                    truncate(&q.query, 48)
                ));
            }
        }
    }
    if lines.is_empty() {
        println!("    Δ vs default: no per-query rank moved (verify against the piles before reading this as \"no effect\")");
        return;
    }
    println!("    Δ vs default — note ranks: {note_up} improved, {note_down} worsened",);
    for line in lines {
        println!("{line}");
    }
}

/// One appendable JSONL row for a scored configuration.
#[allow(clippy::too_many_arguments)]
fn result_row(
    git: &Option<String>,
    model: &str,
    dim: usize,
    label: &str,
    cfg: &ChunkConfig,
    tokenizer: &str,
    notes: usize,
    chunks: usize,
    embed_secs: f64,
    queries: &[Labelled],
    bm25: Option<&Pass>,
    vector: Option<&Pass>,
    hybrid: &Pass,
    similar: Option<&SimilarPass>,
    floor_z: Option<&FloorZ>,
    evidence: Option<&SearchEvidence>,
    fold: Option<&FoldBench>,
) -> serde_json::Value {
    let ts = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let agg = |a: &Agg| serde_json::json!({ "n": a.n, "hit1": a.hit1(), "hit3": a.hit3(), "mrr": a.mrr() });
    serde_json::json!({
        "ts": ts,
        "git": git,
        "model": model,
        "dim": dim,
        // NEW key (absent from rows before 2026-08-18): which corpus this row
        // scored — the dense single-domain fixture appends its own `"dense"`
        // rows (GH #196/#197), and rows must never average across corpora.
        // Absent = this corpus, so every older row stays comparable unchanged.
        "corpus": "orthogonal",
        "config": {
            "label": label,
            "target_tokens": cfg.target_tokens,
            "overlap_frac": cfg.overlap_frac,
            "chars_per_token": cfg.chars_per_token,
            "backscan_tokens": cfg.backscan_tokens,
            "prepend_heading_path": cfg.prepend_heading_path,
        },
        // NEW key (absent from rows before 2026-08-11, which were all scored
        // under the then-default `unicode61`): the chunks_fts tokenizer this row
        // was scored under (GH #157). Top-level rather than inside `config`,
        // which stays the ChunkConfig alone.
        "tokenizer": tokenizer,
        "notes": notes,
        "chunks": chunks,
        // The caveat travels with the numbers: a row whose corpus fit inside the
        // retrieval pool had both candidate lists complete, so no candidate-width
        // change could have affected it and comparing it across one proves nothing
        // (GH #141). Equality is blind too — a pool exactly the size of the corpus
        // truncates nothing either. Both depths are recorded because since #142 the
        // two views differ; the flat `pool` key of earlier rows is deliberately
        // *not* reused, so a reader of a mixed file sees a missing field rather than
        // one number silently meaning something narrower than it used to.
        "pool_note": note_candidate_pool(K),
        "pool_chunk": chunk_candidate_pool(K),
        "pool_blind": chunks <= chunk_candidate_pool(K).min(note_candidate_pool(K)),
        "embed_secs": embed_secs,
        "note": {
            "bm25": bm25.map(|p| agg(&p.note)),
            // NEW key (absent from rows before 2026-08-10): the dense ablation —
            // `Vault::search_vector_only`, the single-signal baseline fusion is
            // judged against (GH #158). Same convention as pool_note/pool_chunk:
            // a new key, never a redefined one.
            "vector": vector.map(|p| agg(&p.note)),
            "hybrid": agg(&hybrid.note),
        },
        "chunk": {
            "bm25": bm25.map(|p| agg(&p.chunk)),
            "hybrid": agg(&hybrid.chunk),
        },
        // "similar" keeps its pre-negative shape (the positive anchors' rank agg)
        // so rows stay comparable across the change; the negative-anchor tally and
        // the calibration piles are NEW keys, absent from older rows rather than
        // redefining an existing one — the same convention as pool_note/pool_chunk.
        "similar": similar.map(|s| agg(&s.rank)),
        // NEW key (absent from rows before 2026-08-17): the per-mate,
        // non-saturating companion to "similar" (GH #183). Same convention
        // again — a new key, never a redefined one, so every older row stays
        // comparable on "similar" itself.
        "similar_per_mate": similar.map(|s| agg(&s.mate)),
        "similar_per_mate_raw": similar.map(|s| agg(&s.mate_raw)),
        "similar_mates_suppressed": similar.map(|s| s.mate_suppressed),
        // NEW key (absent from rows before 2026-08-17): discovery's precision
        // side — unlabelled notes served on positive anchors at the ranks' own
        // depth (GH #188). Same convention as every key above: new, never a
        // redefinition. Recorded with the pairs, because the count alone is
        // unarguable and the pairs are what a reader checks against the notes.
        "similar_strangers": similar.map(|s| serde_json::json!({
            "cards": s.strangers.len(),
            "anchors": s.stranger_anchors,
            "positives": s.rank.n,
            // The depth the count is read at — the ranks' own top-K, so a
            // future SIM_K change shows up in the row instead of silently
            // redefining the number.
            "depth": SIM_K,
            // The pairs, because a bare count is unarguable: this metric is a
            // smoke alarm whose correct answer is sometimes "that label is
            // missing", and that argument needs the notes named.
            "detail": s.strangers.iter().map(|(anchor, path)| serde_json::json!({
                "anchor": anchor,
                "path": path,
            })).collect::<Vec<_>>(),
        })),
        "similar_negatives": similar.map(|s| serde_json::json!({
            "n": s.neg_n, "clean": s.neg_clean, "cards": s.neg_cards,
        })),
        // Cosine, 4 decimals: enough to place a floor, short enough to keep rows
        // readable. Related = human-labelled matches; junk = everything else
        // surfaced (see score_similar).
        "similar_piles": similar.map(|s| serde_json::json!({
            "related": s.related.iter().map(|c| (c * 1e4).round() / 1e4).collect::<Vec<_>>(),
            "junk": s.junk.iter().map(|c| (c * 1e4).round() / 1e4).collect::<Vec<_>>(),
        })),
        // The same scores with their per-anchor rank order kept: the relative
        // drop-off cutoff is judged within one anchor's list, and tracing a pile
        // value back to its pair needs the anchor. The piles above are this,
        // flattened — kept anyway, because the flat distributions are what a
        // quick jq/pandas histogram wants.
        "similar_detail": similar.map(|s| s.detail.iter().map(|d| serde_json::json!({
            "anchor": d.anchor,
            "negative": d.negative,
            "candidates": d.candidates.iter().map(|(path, cos, related)| serde_json::json!({
                "path": path,
                "cos": (cos * 1e4).round() / 1e4,
                "related": related,
            })).collect::<Vec<_>>(),
        })).collect::<Vec<_>>()),
        // NEW key (absent from rows before 2026-08-17): the floor's own
        // calibration data in the floor's own unit — every candidate's ungated
        // judge z, the populations the two constants answer to, and both
        // re-derived windows (GH #187; the unit changed from stage-1 centroid z
        // to stage-2 best-passage z with GH #192 — the row's "unit" field is
        // what tells the two apart). Same convention as every key above: new,
        // never a redefinition. This is the row the next recalibration reads, and
        // the reason no window belongs in a doc comment — `null` on the ablation
        // rows, which do not re-derive it.
        "discovery_z": floor_z.map(|z| {
            let (mates, strangers) = (z.mate_z(), z.stranger_z());
            let (neg_leaders, pos_leaders) = (z.neg_leader_z(), z.pos_leader_z());
            let win = |cut: &[f64], keep: &[f64]| Window::read(cut, keep).map(|w| serde_json::json!({
                "cut_max": (w.cut_max * 1e4).round() / 1e4,
                "keep_min": (w.keep_min * 1e4).round() / 1e4,
                "open": w.open(),
            }));
            let round = |v: &[f64]| v.iter().map(|z| (z * 1e4).round() / 1e4).collect::<Vec<_>>();
            serde_json::json!({
                // The unit these piles/windows are measured in. Rows before
                // GH #192 carried no key here and are stage-1 centroid z — a
                // different unit; never compare across the flip. Since GH #197
                // the z gates nothing (the "shipped" and "replay_faults" keys
                // of earlier rows retired with the gate — absent, per the
                // convention, rather than redefined).
                "unit": "stage2-best-passage",
                "piles": {
                    "mates": round(&mates),
                    "strangers_on_positives": round(&strangers),
                    "negative_leaders": round(&neg_leaders),
                    "positive_leaders": round(&pos_leaders),
                },
                "window_leader": win(&neg_leaders, &pos_leaders),
                "window_member": win(&strangers, &mates),
                "ungraded_anchors": z.ungraded,
                // ~0 is the only trustworthy reading — see FloorZ::recheck_delta.
                "z_recheck_max_delta": z.recheck_delta(),
                // Per-anchor and per-candidate, because a window edge is only
                // arguable once you can name the pair that set it. `z` is the
                // stage-2 best-passage z, `cos` the same pair's cosine (the
                // model-comparable unit).
                "detail": z.anchors.iter().map(|a| serde_json::json!({
                    "anchor": a.anchor,
                    "negative": a.negative,
                    "candidates": a.candidates.iter().map(|c| serde_json::json!({
                        "path": c.path,
                        "z": (c.z * 1e4).round() / 1e4,
                        "cos": (cosine_of(c.score) * 1e4).round() / 1e4,
                        "mate": c.mate,
                    })).collect::<Vec<_>>(),
                })).collect::<Vec<_>>(),
            })
        }),
        // NEW key (absent from rows before 2026-08-22): the search evidence
        // dump (invariants.md D2, GH #201) — per labelled query, the absolute
        // signals RRF discards (BM25 hit count + best score, dense top-1
        // cosine) and the shipped surface's served count, positives and
        // negatives apart, with the would-be pure-cosine query window. Same
        // convention as every key above: new, never a redefinition. `null` on
        // ablation/sweep rows, which do not re-derive it.
        "search_evidence": evidence.map(|e| {
            let row = |r: &QueryEvidence| serde_json::json!({
                "q": r.query,
                "bm25_hits": r.bm25_hits,
                "bm25_best": r.bm25_best.map(|b| (b * 1e4).round() / 1e4),
                "best_cos": r.best_cos.map(|c| (c * 1e4).round() / 1e4),
                "top": r.top,
                "served": r.served,
            });
            let (pos_cos, neg_cos) = (best_cos_pile(&e.positives), best_cos_pile(&e.negatives));
            serde_json::json!({
                // The depth `served` is read at, so a future K change shows up
                // in the row instead of silently redefining the number.
                "k": K,
                "positives": e.positives.iter().map(row).collect::<Vec<_>>(),
                "negatives": e.negatives.iter().map(row).collect::<Vec<_>>(),
                "window_best_cos": Window::read(&neg_cos, &pos_cos).map(|w| serde_json::json!({
                    "cut_max": (w.cut_max * 1e4).round() / 1e4,
                    "keep_min": (w.keep_min * 1e4).round() / 1e4,
                    "open": w.open(),
                })),
            })
        }),
        // The fold bake-off (GH #200, Phase B) — every candidate rule's reading
        // on this run, with the per-anchor folds it was read from. Recorded on
        // the default row only: the sweep's variants re-chunk the corpus, and a
        // disclosure rule judged on a non-shipped chunker is a number about the
        // chunker.
        "discovery_fold": fold.map(fold_json),
        "queries": queries.iter().enumerate().map(|(i, q)| serde_json::json!({
            "q": q.query,
            "bm25": bm25.map(|p| p.scores[i].note),
            "vector": vector.map(|p| p.scores[i].note),
            "hybrid": hybrid.scores[i].note,
            "chunk": hybrid.scores[i].chunk,
        })).collect::<Vec<_>>(),
    })
}

/// Append one row to the results log (creating it on first run). Append-only, so
/// runs accumulate into one dataset — the same convention as `B2_LOG_FILE`.
fn append_result(path: &Path, row: serde_json::Value) -> Result<(), Box<dyn std::error::Error>> {
    use std::io::Write;
    let mut f = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)?;
    writeln!(f, "{row}")?;
    Ok(())
}

/// The repo's short commit hash, best-effort (None outside a git checkout).
fn git_short_sha() -> Option<String> {
    let out = std::process::Command::new("git")
        .args(["rev-parse", "--short", "HEAD"])
        .current_dir(env!("CARGO_MANIFEST_DIR"))
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    Some(String::from_utf8_lossy(&out.stdout).trim().to_string())
}

/// Corpus notes are copied flat into the vault, so a result path equals (or ends
/// with) the labelled relevant path.
fn paths_match(result_path: &str, relevant: &str) -> bool {
    result_path == relevant || result_path.ends_with(&format!("/{relevant}"))
}

fn rank_str(rank: Option<usize>) -> String {
    match rank {
        Some(1) => "✓1".to_string(),
        Some(r) => format!("·{r}"),
        None => format!("✗>{K}"),
    }
}

fn truncate(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        s.to_string()
    } else {
        let cut: String = s.chars().take(max - 1).collect();
        format!("{cut}…")
    }
}
