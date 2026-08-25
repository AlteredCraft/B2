//! Semantic-retrieval + discovery eval — the pass that scores model quality **out of CI**
//! (ADR-0013). It lives as an *example*, not a test, so it never runs in the deterministic
//! `cargo test` suite and quality can never flake CI.
//!
//! ```console
//! cargo run -p b2-embed --example eval               # score the configured model
//! cargo run -p b2-embed --example eval -- --sweep    # + chunker A/B (the #44 gate)
//! cargo run -p b2-embed --example eval -- --stemmer  # + FTS tokenizer A/B (the #157 gate)
//! ```
//!
//! **`docs/evals/README.md` is the notebook of record** — the corpus, what the exit code
//! enforces, every verdict this harness has ruled, and the process rules. Read it before
//! touching the corpus, the labels, or a constant here. What this comment carries is only
//! what a reader of *this file* needs:
//!
//! One run builds throwaway vaults from the labelled corpora in `evals/` and scores, through
//! the real pipeline: a **BM25 baseline** (after `project` only — the floor the model must
//! clear, since the labelled queries avoid their target's keywords); **hybrid retrieval**
//! plus a **vector-only ablation** (GH #158), whose delta is the measured value of the one
//! AI seam; **passage rank** at chunk level, which is where chunking levers show; and
//! **discovery**, scored per labelled mate rather than per anchor (GH #183 — the per-anchor
//! metric saturates), with the strangers a positive anchor serves counted, named, and
//! deliberately ungated, since the cheapest way to shrink that count is to label one.
//!
//! Two calibration blocks re-derive their windows **every run** rather than quoting a
//! reading: the discovery **z dump** (GH #187) and the **search evidence bake-off**
//! (ADR-0015, GH #201/#202). That is the house rule — constants in code, measurements in the
//! harness — and it exists because the GH #150 floors were frozen into a docstring and went
//! stale the first time the corpus grew a shape they were never read against.
//!
//! The **dense single-domain fixture** (`evals/corpus-dense/`) is scored in its own vault
//! and its own row, never averaged in: fifteen genuinely inter-related notes with no loner,
//! the geometry that broke every anchor-local existence gate (ADR-0014) and killed the first
//! lexical evidence rule (ADR-0015). The orthogonal corpus is structurally incapable of
//! expressing topical concentration, so a run that judges a bar only there judges it on the
//! geometry it survives.
//!
//! What this corpus **cannot** score is *candidate width*: 29 chunks is no more than the
//! candidates each signal retrieves, so neither list is truncated and every number is
//! invariant under either view's headroom or `search::pool_size` (GH #141). A run that is
//! blind that way says so; the property is measured by `--example stability` on a vault big
//! enough for the pool to bind. `RRF_K` re-weights the *same* lists, so it does move scores
//! here and needs no separate instrument.
//!
//! `--sweep` re-chunks + re-embeds the same vault under variant [`ChunkConfig`]s. `--stemmer`
//! swaps `chunks_fts` between the shipped `porter unicode61` and the unstemmed ablation over
//! **identical** chunk rows and vectors, so every rank move is the tokenizer's alone.
//!
//! Every scored run appends one JSON line to `evals/results.jsonl` (gitignored), so runs
//! accumulate into a comparable dataset.

// `result_row`'s JSON literal is one `json!` expansion per key, and the row has
// grown a key per instrument (GH #158, #141, #183, #187, #188). Raising the
// limit keeps the row's shape legible in one place — the alternative is
// scattering its subtrees across helper functions to satisfy a macro, which
// costs the thing the row is for: a reader seeing every recorded field at once.
#![recursion_limit = "256"]

use b2_core::chunk::ChunkConfig;
use b2_core::db::FtsTokenizer;
use b2_core::embed::Embedder;
use b2_core::search::EvidenceBar;
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
/// The floor on **per-mate** discovery MRR@[`SIM_K`] (GH #188) — the non-saturating rank
/// metric, gated once a baseline existed to price it. Re-derived for always-serve
/// (ADR-0014): the reading moved 0.633 -> 0.650 when the existence gate retired.
///
/// Placed **below** the reading, never at it: a gate pinned to today's number fails on the
/// first legitimate corpus edit, which trains the one habit this harness must never train
/// (process rule 2 — editing a *label* to get green). Sizing is measured, not guessed: five
/// consecutive runs on an unchanged corpus/model/build reproduce every rank exactly, so the
/// noise floor is 0 and the headroom exists for *corpus* drift. At n = 15 mates one mate
/// lost from rank 1 costs 1/15, and this floor sits ~2 such losses under the reading.
const FLOOR_MATE_MRR: f64 = 0.52;
/// How many labelled mates the shipped surface may fail to serve at all.
///
/// **Structurally 0 under always-serve** (ADR-0014): both discovery passes read the one
/// ranked surface, so nothing can be reachable in one and unserved in the other. Kept as an
/// assertion at zero, with no headroom, because it is the **tripwire** that re-arms the
/// moment any Phase-2 existence signal puts a second surface back in the path: a nonzero
/// value can only mean a gate is suppressing a human-labelled relation again.
const MAX_MATES_SUPPRESSED: usize = 0;
/// The floor on the **dense fixture's** per-mate MRR@[`SIM_K`] (ADR-0014, Phase 0b), gated
/// once its baseline existed — the same measure-then-calibrate order as [`FLOOR_MATE_MRR`].
///
/// The reading is 0.467 (n = 14): the model recovers the within-cluster labels and ranks the
/// three cross-cluster claims lower, which is headroom in both directions. (Its first
/// reading was 0.502; a one-word grammar fix moved one mate a rank — the worked example of
/// why these floors carry corpus-drift margin.) At n = 14 one mate lost from rank 1 costs
/// 1/14, and this sits ~2 such losses under. Process rule 2 binds hard here: in a corpus
/// where everything relates, relabelling toward the model's order would always look
/// plausible — a red reading argues about the notes.
const FLOOR_DENSE_MATE_MRR: f64 = 0.32;
/// How many **labelled negative queries** the shipped evidence bar may still serve
/// (ADR-0015, GH #202). Zero: this is the defect the bar exists to fix.
///
/// A floor at its measured value rather than below it, which is the deliberate exception to
/// the house sizing method — the "headroom" a floor normally carries would be *permission to
/// serve a nonsense query*. A new negative the bar serves is either a real regression or a
/// mislabelled query; both want a red reading.
const MAX_NEGATIVES_SERVED: usize = 0;
/// How many **labelled relevant queries** the bar may cut (GH #202) — the search-side
/// tripwire ADR-0015 asserts at zero with no headroom, and the direction that costs a user
/// something real: a served nonsense row costs a little trust, a cut positive costs the
/// answer. Its precondition was met by GH #208, which labelled the date-shaped query pile.
/// The reading is 0 of 44. A nonzero value is never a calibration nudge: it means the *rule*
/// is wrong for a shape the corpus now carries.
const MAX_POSITIVES_CUT: usize = 0;
/// How many of the **dense fixture's title-as-query probes** the bar may cut (GH #202).
/// Zero, and this is the assertion that would have caught the losing rule: a note's own
/// title names a note the vault demonstrably holds, so cutting one is indefensible whatever
/// a labelled corpus says. A third row rather than headroom on [`MAX_POSITIVES_CUT`] because
/// it gates a different *geometry*: the labelled corpus minimizes shared vocabulary by
/// construction, so topical concentration is only expressible here. Titles need no labels,
/// so nothing in this reading can be relabelled to clear it.
const MAX_DENSE_TITLES_CUT: usize = 0;

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
    /// Notes beyond `relevant` that are honest evidence for the query (GH #206) —
    /// the per-hit tail depth. The judgement is **exhaustive** for every positive
    /// query: a served note in neither `relevant` nor here is irrelevant *by
    /// label*, which is the statement the tail bake-off is judged on. Never enters
    /// a rank aggregate — `relevant` alone says what should rank first.
    #[serde(default)]
    tail_relevant: Vec<String>,
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
    /// **Per-mate** ranks: one entry per `(anchor, expected mate)` pair rather than one per
    /// anchor (GH #183). [`Self::rank`] takes the *first* labelled mate it finds and stops,
    /// so an anchor scores a hit on its easiest one and every harder mate is invisible —
    /// which is how that metric sat pinned at 1.000 across the ordering change it was
    /// supposed to judge. Worse, *adding* a hard mate makes `rank` strictly easier.
    ///
    /// Scoring each mate on its own is the non-saturating readout GH #183 asked for: a mate
    /// sliding from rank 2 to rank 4 moves this number even while `rank` stays at ceiling. A
    /// `None` is honest — that mate did not surface at all.
    ///
    /// **In the exit gate** since GH #188, at [`FLOOR_MATE_MRR`]. It shipped reporting-only
    /// for one issue's worth of time on the measure-then-calibrate precedent, and that is
    /// where the baseline came from.
    mate: Agg,
    /// [`Self::mate`] measured on the second pass — which since ADR-0014 reads the **same
    /// always-serve surface** as the first, since `similar` *is* the ranked list.
    ///
    /// Kept, with [`Self::mate_suppressed`] beside it, as the **tripwire structure**: while
    /// nothing gates, the two agree by construction and suppression reads a structural 0. If
    /// a Phase-2 bake-off ever ships an existence signal the passes diverge again and the
    /// assertion re-arms with no harness change.
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
    /// **Strangers**: unlabelled notes the shipped surface serves on a *positive* anchor,
    /// within the same top-`SIM_K` the ranks are read at — `(anchor, path)` per card, so the
    /// smoke alarm comes with the list you argue against it with (process rule 1).
    ///
    /// This is discovery's **precision** side, and the harness had none: the gated numbers
    /// watch mate ranks and negative anchors, and the negatives gate is structurally blind to
    /// a member bar — while `member_z <= leader_z`, a negative anchor is clean iff its leader
    /// is cut, so a relaxed member bar spends its entire cost here (GH #187/#188).
    ///
    /// Deliberately **reported, not gated**, and the reason is the gaming direction: the
    /// cheapest way to shrink this count is to *label the stranger*, which silently moves the
    /// per-mate metric too. The labels are not exhaustive, so an unlabelled note served is
    /// not proof of junk — it reads as a smoke alarm with names attached.
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

/// The z dump across every discovery anchor, split into the populations an existence bar
/// would answer to (GH #187; the unit is the stage-2 best-passage z, and it gates nothing —
/// ADR-0014).
///
/// Three populations, because a two-bar rule's constants answer to different ones — the
/// conflation is what made "the negatives gate would catch a bad `member_z`" look true when
/// it was not. A leader gate is calibrated by negative-anchor leaders against positive-anchor
/// leaders; a member bar by strangers against labelled mates. Both windows are re-derived
/// every run — the standing record of why no such rule ships, and the first reading any
/// Phase-2 candidate answers to.
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
    print_search_bakeoff(&evidence, &bake_off(&evidence), &model_id);
    // The per-hit tail bake-off (GH #206) — judged from the same served lists
    // the evidence dump above recorded, against the tail_relevant keep-set.
    let tail = score_search_tail(&evidence);
    print_search_tail(&tail);

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
            Some(&tail),
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
    // The tail bake-off's cross-bench join (GH #206) — printable only here,
    // where both corpora's readings exist in one run.
    print_tail_join(&evidence, &tail, &dense.search.titles);

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
    // The negatives' suppression assertion RETIRED with the gate it watched (ADR-0014):
    // under always-serve a loner anchor serves its ranked nearest — that is the ruling, not
    // a regression. The anchors stay labelled, the strangers instrument keeps counting, and
    // what the served cards *claim* is the calibration block's band readout.
    //
    // Discovery **rank** (GH #188). The rank floor sits below its measured reading (corpus
    // drift headroom, run noise being zero); suppression, next, sits AT its structural zero
    // — a tripwire, not a budget.
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
    // The search-evidence rows (ADR-0015, GH #202), landed with the surfaces that consume
    // the verdict. Everything above is discovery's and is deliberately UNCHANGED: search's
    // bar moves no discovery rank and no reachability, so movement up there is a bug.
    //
    // All three sit at their structural zeros with no headroom — the exception to the house
    // sizing method rather than an oversight, since headroom here would read as permission
    // to serve a nonsense query or cut a real one. Skipped entirely when the model has no
    // calibrated bar (ADR-0007): asserting the absence of a verdict would fail every run on
    // a model the harness has simply not measured yet.
    match read_shipped_bar(&evidence, &model_id) {
        None => eprintln!(
            "\n[note] no calibrated evidence bar for {model_id} — D2's exit-gate rows are not \
             asserted this run (M2)."
        ),
        Some(reading) => {
            if reading.neg_served > MAX_NEGATIVES_SERVED {
                eprintln!(
                    "\n[warn] the shipped evidence bar serves {} of {} labelled NEGATIVE queries where \
                     D2 permits {MAX_NEGATIVES_SERVED} — a query the vault holds nothing for is being \
                     answered with rows (read the per-query lines above, and do NOT relabel to clear \
                     this).",
                    reading.neg_served,
                    evidence.negatives.len()
                );
                return Ok(false);
            }
            // The tripwire, and the direction that costs a user the answer rather
            // than a little trust. Its precondition is GH #208's date-shaped pile:
            // the assertion is only worth what the query shapes behind it are.
            if reading.pos_cut > MAX_POSITIVES_CUT {
                eprintln!(
                    "\n[warn] the shipped evidence bar CUTS {} of {} labelled relevant queries where D2 \
                     permits {MAX_POSITIVES_CUT} — a note the vault holds is unreachable for a query \
                     naming it. Change the RULE, not the constant (the df ceiling died exactly here).",
                    reading.pos_cut,
                    evidence.positives.len()
                );
                return Ok(false);
            }
        }
    }
    // The same two directions on the dense fixture — a different geometry, not a
    // different threshold. Topical concentration is what killed the losing rule
    // and is structurally inexpressible on the orthogonal corpus (process rule 2's
    // token audit minimizes shared vocabulary), so these are their own rows.
    // Titles carry no labels, so nothing here can be relabelled to clear it.
    let titles_cut = dense
        .search
        .titles
        .iter()
        .filter(|p| p.vouched == Some(false))
        .count();
    if titles_cut > MAX_DENSE_TITLES_CUT {
        eprintln!(
            "\n[warn] the evidence bar cuts {titles_cut} of {} dense-fixture titles where \
             {MAX_DENSE_TITLES_CUT} is permitted — a note's own title is a query naming a note the \
             vault demonstrably holds, and the lexical half has gone inert on a single-subject vault \
             (GH #201's transfer check, as an assertion).",
            dense.search.titles.len()
        );
        return Ok(false);
    }
    let nonsense_served = dense
        .search
        .nonsense
        .iter()
        .filter(|p| p.vouched == Some(true))
        .count();
    if nonsense_served > MAX_NEGATIVES_SERVED {
        eprintln!(
            "\n[warn] the evidence bar serves {nonsense_served} of {} nonsense queries on the dense \
             fixture where {MAX_NEGATIVES_SERVED} is permitted — nonsense needs no token audit in any \
             vault, which is exactly why this reading transfers.",
            dense.search.nonsense.len()
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
    let model_id = embedder.model_id().to_string();
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
        // The bar's hardest bench, for the same reason the fold's is: this is the
        // geometry that disqualified the rule that lost (GH #201).
        search: score_dense_search(&vault, &model_id)?,
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
    /// D2's shipped bar replayed on this fixture (GH #201) — see
    /// [`score_dense_search`].
    search: DenseSearch,
}

/// The shipped search evidence bar's reading **on the single-domain fixture** (ADR-0015).
///
/// This exists because the bar's first form died here and nowhere else. A hard `df <= 10%`
/// content ceiling read 0 cut / 0 served on the labelled orthogonal corpus — clean by every
/// number that bench can produce — and then classed `drone` (df 3) and `comb` (df 7) as
/// stopwords in a vault about beekeeping, cutting 3 of 15 answerable queries. The lexical
/// rule's hazard is **topical concentration**, which the orthogonal corpus cannot express, so
/// a run that judges the bar only there judges it on the geometry it survives.
///
/// The reading was first taken once, by hand. Taking it *once* is the thing GH #187 named,
/// so it is re-derived every run, here, beside the fold bench that already sweeps this
/// fixture. **In the exit gate** since GH #202, as a row of its own rather than headroom on
/// the labelled corpus's, because it watches a different *geometry*.
struct DenseSearch {
    /// Every note's own title replayed as a query — the **tripwire direction**
    /// (D2: a labelled-relevant query cut is zero with no headroom). Titles need
    /// no labels and so nothing here can be relabelled to clear a reading.
    titles: Vec<SearchProbe>,
    /// Nonsense, the defect direction. See [`DENSE_NONSENSE`].
    nonsense: Vec<SearchProbe>,
    /// `None` when the active model has no calibrated bar (M2) — the coverage
    /// readings still print, the verdicts do not exist to print.
    bar: Option<b2_core::search::EvidenceBar>,
}

/// One query's reading on the dense fixture: the two absolute signals D2 judges,
/// and the engine's own verdict rather than a restatement of it.
struct SearchProbe {
    query: String,
    /// IDF-weighted term coverage; `None` when no term carries any weight (the
    /// lexical half abstaining, not scoring zero).
    coverage: Option<f64>,
    best_cos: Option<f64>,
    /// `Vault::search_evidence`'s verdict — what would actually ship. `None`
    /// mirrors [`DenseSearch::bar`].
    vouched: Option<bool>,
    /// The served list with its per-hit provenance — the tail bake-off's dense
    /// bench (GH #206). On a title query `keep` is true for every row **by
    /// geometry**, not by label: a single-domain vault's lists are all real
    /// matches, so a tail rule that truncates one is disqualified — the same
    /// absolute GH #200 enforced for discovery, and the reason this fixture
    /// needs no `tail_relevant` labels.
    rows: Vec<ServedRow>,
}

/// The negatives replayed on the dense fixture: **nonsense only**. The labelled negatives in
/// `queries.json` are the *orthogonal* corpus's, and process rule 2's token audit is what
/// makes them negatives — an audit that says nothing about a different corpus. Against
/// `corpus-dense` the phrase-shaped ones disqualify themselves on their merits ("why parrots
/// mimic speech" shares `mimic` with `robbing-behavior.md`, which is a thing the rule
/// deliberately serves). Nonsense needs no audit in any vault, which is why it transfers.
const DENSE_NONSENSE: [&str; 2] = ["shjfasd", "vrelqip zonktar wembleforth"];

/// Replay the shipped bar over the dense fixture (see [`DenseSearch`]).
///
/// Coverage is read off [`b2_core::vault::QueryTermView::idf`] — the view's own
/// weights, not a second copy of the formula. The orthogonal corpus's bake-off
/// re-derives its arithmetic deliberately, as a drift check against the engine;
/// one such check is the check, and a second would only be two places to fix.
fn score_dense_search(
    vault: &Vault,
    model_id: &str,
) -> Result<DenseSearch, Box<dyn std::error::Error>> {
    let read = |query: &str, keep: bool| -> Result<SearchProbe, Box<dyn std::error::Error>> {
        let view = vault.search_evidence(query, K)?;
        let total: f64 = view.terms.iter().map(|t| t.idf).sum();
        Ok(SearchProbe {
            query: query.to_string(),
            coverage: (total > f64::EPSILON).then(|| {
                view.terms
                    .iter()
                    .filter(|t| t.df >= 1)
                    .map(|t| t.idf)
                    .fold(0.0, |a, b| a + b)
                    / total
            }),
            best_cos: view.best_cos,
            vouched: view.vouched,
            rows: view
                .results
                .iter()
                .map(|r| ServedRow {
                    path: r.result.path.clone(),
                    bm25_rank: r.bm25_rank,
                    cos: r.cos,
                    keep,
                })
                .collect(),
        })
    };
    let mut titles = Vec::new();
    for note in vault.list_notes()? {
        // The fixture's notes carry no frontmatter title, so the slug is the
        // query — `drone-comb` → "drone comb", which is the pair of words the
        // retired ceiling called stopwords.
        let title = note.title.clone().unwrap_or_else(|| {
            std::path::Path::new(&note.path)
                .file_stem()
                .map(|s| s.to_string_lossy().replace(['-', '_'], " "))
                .unwrap_or_default()
        });
        if !title.trim().is_empty() {
            titles.push(read(&title, true)?);
        }
    }
    Ok(DenseSearch {
        titles,
        nonsense: DENSE_NONSENSE
            .iter()
            .map(|q| read(q, false))
            .collect::<Result<_, _>>()?,
        bar: b2_core::search::EvidenceBar::for_model(model_id),
    })
}

/// Print the dense fixture's search-evidence reading (see [`DenseSearch`]).
fn print_dense_search(search: &DenseSearch) {
    println!(
        "  search bar  D2's shipped bar replayed on this geometry (GH #201; GATED since GH #202)"
    );
    // The coverage reading comes FIRST, and above the bar, because it is
    // **model-free**: a fact about this vault's vocabulary, and the lexical
    // half's whole premise. Gating it behind a calibrated bar would print
    // nothing at all on a vault the harness can still say something true about —
    // the defect PR #205's review already fixed in `calibrate.rs` (c03f8cd), met
    // again here (PR #207 review).
    let covs: Vec<f64> = search.titles.iter().filter_map(|p| p.coverage).collect();
    let cov_line = match pile_stats(&covs) {
        Some((min, med, max)) => format!("{min:.2}/{med:.2}/{max:.2}"),
        None => "— (no query carried weight)".to_string(),
    };
    println!("              title-as-query coverage min/med/max {cov_line}");

    // Only the *verdicts* below need a bar, so only they stop here.
    let Some(bar) = search.bar else {
        println!("              no calibrated bar for this model — no verdict is offered (M2)");
        return;
    };
    let cut: Vec<&SearchProbe> = search
        .titles
        .iter()
        .filter(|p| p.vouched == Some(false))
        .collect();
    let served: Vec<&SearchProbe> = search
        .nonsense
        .iter()
        .filter(|p| p.vouched == Some(true))
        .collect();
    println!(
        "              bar under test: coverage ≥ {:.2} or cos ≥ {:.3}",
        bar.min_term_coverage, bar.min_cos,
    );
    println!(
        "              cuts {}/{} title queries   ← the TRIPWIRE direction; the retired df ceiling \
         cut 3 here",
        cut.len(),
        search.titles.len()
    );
    for p in &cut {
        println!(
            "                [CUT] {:<32} cov {:>5}  cos {:>6}",
            truncate(&p.query, 32),
            p.coverage
                .map(|c| format!("{c:.2}"))
                .unwrap_or_else(|| "—".to_string()),
            p.best_cos
                .map(|c| format!("{c:.3}"))
                .unwrap_or_else(|| "—".to_string()),
        );
    }
    println!(
        "              serves {}/{} nonsense queries   ← the reported defect, on this corpus",
        served.len(),
        search.nonsense.len()
    );
}

/// The dense fixture's search reading as JSON (`search_transfer` in the dense
/// row) — every probe, so any bar is re-derivable from a row without re-running
/// the model, the `discovery_fold` convention.
fn dense_search_json(search: &DenseSearch) -> serde_json::Value {
    let probes = |pile: &[SearchProbe]| {
        pile.iter()
            .map(|p| {
                serde_json::json!({
                    "query": p.query,
                    "coverage": p.coverage.map(|c| (c * 1e4).round() / 1e4),
                    "best_cos": p.best_cos.map(|c| (c * 1e4).round() / 1e4),
                    "vouched": p.vouched,
                    // NEW subkey (absent before GH #206): the served list's
                    // per-hit provenance, so the tail constraints below are
                    // re-derivable from a row without re-running the model.
                    "rows": p.rows.iter().map(|row| serde_json::json!({
                        "path": row.path,
                        "bm25_rank": row.bm25_rank,
                        "cos": row.cos.map(|c| (c * 1e4).round() / 1e4),
                    })).collect::<Vec<_>>(),
                })
            })
            .collect::<Vec<_>>()
    };
    serde_json::json!({
        "bar": search.bar.map(|b| serde_json::json!({
            "min_term_coverage": b.min_term_coverage,
            "min_cos": b.min_cos,
        })),
        "titles_cut": search.titles.iter().filter(|p| p.vouched == Some(false)).count(),
        "nonsense_served": search.nonsense.iter().filter(|p| p.vouched == Some(true)).count(),
        "titles": probes(&search.titles),
        "nonsense": probes(&search.nonsense),
        // NEW subkey (absent before GH #206): the per-hit tail families'
        // dense-bench constraints — the absolute this fixture supplies.
        "tail": dense_tail_json(&search.titles),
    })
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
    print_dense_search(&dense.search);
    // Model-geometry reading, not a verdict, so it prints with or without a
    // calibrated bar — the same posture as the coverage line above it.
    print_dense_tail(&dense.search.titles);
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
        // NEW key (absent from rows before 2026-08-22): the shipped search bar
        // replayed on this fixture (GH #201). Deliberately NOT the orthogonal
        // row's `search_evidence`: that key holds the *labelled* bake-off, and
        // this is the label-free transfer reading — a different measurement, so
        // it takes a different name. Same convention as every key above: new,
        // never a redefinition, so no reader has to branch on `corpus` to learn
        // which shape it is holding (PR #207 review).
        "search_transfer": dense_search_json(&dense.search),
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
    /// The rule's name in every table, row and window line — one spelling, so a
    /// printed verdict and a recorded row can never name the same rule
    /// differently.
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
    /// Whether this candidate is *reciprocal* at depth `k` — i.e. the anchor
    /// sits in its own top `k`. Reciprocity is necessary but not sufficient for
    /// being above the fold: the fold is the longest reciprocal **prefix**, so a
    /// reciprocal candidate ranked after a non-reciprocal one is below it too.
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
    /// suppression does) — and GH #202 shipped no fold to charge, so it stays
    /// reporting-only; it becomes an exit-gate row with the first fold that does
    /// ship, beside the suppression tripwire that re-arms the same way.
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
    /// Whether some swept `k` satisfies both bounds at once. A bench with no
    /// labelled loner has no upper bound to satisfy and so can never report an
    /// open window on its own — it contributes the lower bound to the
    /// bake-off's joint window, which is what the printed verdict says.
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
    /// its population exists, by `make calibrate` on a real vault.
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

    // Outbound only. `neighbors` answers "what is 1 hop from here" and so returns
    // an edge from *both* of its endpoints; summing the raw counts over every
    // note would census edge **endpoints** and report candidate 2's population at
    // twice its size (PR #204 review). Counting each edge once from its source is
    // the number the rule would calibrate from.
    let mut authored_edges = 0;
    for note in vault.list_notes()? {
        authored_edges += vault
            .neighbors(&note.path)?
            .iter()
            .filter(|n| n.direction == "outbound")
            .count();
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
         and GH #202 shipped no fold either, so the exit gate has nothing here to watch until one \
         does. {} further labelled mate(s) \
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
             it is measured where one exists, by `make calibrate` on a real vault)"
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
/// One served row of a labelled query's list (GH #206): the per-hit provenance
/// RRF discards, and the row's relevance **by label** — the two things a per-hit
/// tail rule is judged between. Rank is the row's position in `rows`, never a
/// stored field: the fused order is the identity D1's prefix requirement binds.
struct ServedRow {
    path: String,
    /// 0-based rank in the BM25 list; `None` = the lexical half never ranked
    /// this chunk — the "dense-only" row.
    bm25_rank: Option<usize>,
    /// This row's own cosine to the query; `None` when the dense half never
    /// ranked it (a row is in at least one list, so `bm25_rank` and `cos` are
    /// never both absent).
    cos: Option<f64>,
    /// In the keep-set — `relevant` ∪ `tail_relevant`, exhaustive by label
    /// since GH #206: a false here is a judgement ("filler"), not an omission.
    keep: bool,
}

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
    /// The served list at [`K`], in fused order, one entry per row — path,
    /// per-hit provenance, and the row's relevance **by label** (GH #206). For a
    /// negative query every row's `keep` is false: the whole list is junk by
    /// label. `served`/`dense_only` below read this, so the counts and the rows
    /// they summarize cannot drift apart.
    rows: Vec<ServedRow>,
    /// Chunks in the index — the denominator every `df` below is judged against.
    chunk_total: usize,
    /// Every query term with its document frequency, in query order. The
    /// population the lexical-anchor rule is swept over: raw hit count and raw
    /// best-BM25 both failed to separate the piles (Phase A), so the anchor is
    /// derived from *these* rather than from either of those.
    terms: Vec<(String, usize)>,
    /// **The engine's own verdict** for this query — `Vault::search_evidence`'s `vouched`,
    /// i.e. exactly what the surfaces act on (GH #202). The exit gate counts on *this*, never
    /// on the harness's restatement below: an assertion about what ships must read what
    /// ships. `None` when the model has no calibrated bar.
    ///
    /// The restatement is kept beside it because the sweep needs it — [`bake_off`] evaluates
    /// the rule at coverages the engine cannot be asked about — and because two independent
    /// readings of one rule are a drift check when compared. So they are: [`read_shipped_bar`]
    /// prints a `[FAULT]` on any query where they disagree.
    vouched: Option<bool>,
}

impl QueryEvidence {
    /// What the shipped surface serves at [`K`] today. For a negative query
    /// this is the measured D2 defect: `limit` confident-looking results for a
    /// query the vault holds nothing for.
    fn served(&self) -> usize {
        self.rows.len()
    }

    /// Of the served rows, how many the **lexical half never ranked at all**
    /// (`bm25_rank: None`) — the per-hit shape of the same defect, and the
    /// signal the `lexical` tail rule folds on (GH #206).
    fn dense_only(&self) -> usize {
        self.rows.iter().filter(|r| r.bm25_rank.is_none()).count()
    }

    /// The share of this query's term IDF the vault carries — the harness's own
    /// restatement of [`b2_core::search::LexicalEvidence::term_coverage`], kept
    /// separate so a drift in the engine's rule shows up as the two disagreeing
    /// rather than as silence. `None` when nothing in the query carries weight.
    fn coverage(&self) -> Option<f64> {
        let idf = |df: usize| ((self.chunk_total as f64 + 1.0) / (df as f64 + 1.0)).ln();
        let total: f64 = self.terms.iter().map(|(_, df)| idf(*df)).sum();
        if total <= f64::EPSILON {
            return None;
        }
        let present: f64 = self
            .terms
            .iter()
            .filter(|(_, df)| *df >= 1)
            .map(|(_, df)| idf(*df))
            .fold(0.0, |a, b| a + b);
        Some(present / total)
    }

    /// Whether this query has a lexical anchor at `min_coverage`.
    fn anchored(&self, min_coverage: f64) -> bool {
        self.coverage().is_some_and(|c| c >= min_coverage)
    }
}

/// The `min_term_coverage` grid the bake-off sweeps: how much of a query's own
/// weight the vault must carry for the lexical half to vouch for it. Spans "a
/// twentieth" to "all of it", so the printed rows show the rule's whole
/// behaviour rather than a neighbourhood of the shipped constant.
const COVERAGES: [f64; 10] = [0.05, 0.10, 0.15, 0.20, 0.25, 0.34, 0.50, 0.67, 0.85, 1.00];

/// One coverage-bar cell of the bake-off: how the lexical half alone splits the
/// labelled piles there, and what the cosine half would then have to do for the
/// queries it leaves undecided.
struct EvidenceCell {
    coverage: f64,
    /// Positives the lexical half already vouches for.
    pos_anchored: usize,
    /// Negatives the lexical half wrongly vouches for. **Any nonzero value
    /// disqualifies the cell**: an anchored negative is served whatever the
    /// cosine bar says, so no `min_cos` can rescue it.
    neg_anchored: usize,
    /// Cosines of the positives the lexical half left undecided — the pile a
    /// `min_cos` must KEEP.
    undecided_pos: Vec<f64>,
    /// Cosines of the negatives left undecided — the pile it must CUT.
    undecided_neg: Vec<f64>,
}

impl EvidenceCell {
    /// The cosine window this cell leaves for `min_cos`, over **only** the
    /// queries the lexical half did not already decide — which is the whole
    /// point of reading it here rather than over every query: D2's rule is
    /// lexical OR semantic, so a positive with an anchor never needs its cosine
    /// kept, and a pure-cosine window (Phase A's) overstates the keep set.
    fn window(&self) -> Option<Window> {
        Window::read(&self.undecided_neg, &self.undecided_pos)
    }

    /// Whether some `min_cos` completes this cell into a rule that keeps every
    /// positive and cuts every negative.
    ///
    /// Three ways to be admissible, and the two degenerate ones are real
    /// readings rather than edge-case bookkeeping: with no undecided negatives
    /// the cosine half is inert (the lexical rule did the whole job), and with
    /// no undecided positives any bar above the negatives' best cosine works.
    fn admissible(&self) -> bool {
        if self.neg_anchored > 0 {
            return false;
        }
        match self.window() {
            Some(w) => w.open(),
            None => true,
        }
    }

    /// The lowest `min_cos` that cuts every undecided negative — the bar's
    /// measured floor. `None` when nothing is left to cut.
    fn cut_floor(&self) -> Option<f64> {
        let max = self
            .undecided_neg
            .iter()
            .copied()
            .fold(f64::NEG_INFINITY, f64::max);
        max.is_finite().then_some(max)
    }

    /// The highest `min_cos` that still keeps every undecided positive — the
    /// bar's measured ceiling. `None` when nothing is left to keep.
    fn keep_ceiling(&self) -> Option<f64> {
        let min = self
            .undecided_pos
            .iter()
            .copied()
            .fold(f64::INFINITY, f64::min);
        min.is_finite().then_some(min)
    }
}

/// Sweep the lexical rule over [`COVERAGES`], reading each cell's piles from the
/// labelled queries (GH #201, Phase C).
fn bake_off(ev: &SearchEvidence) -> Vec<EvidenceCell> {
    let mut cells = Vec::new();
    for coverage in COVERAGES {
        let split = |rows: &[QueryEvidence]| -> (usize, Vec<f64>) {
            let anchored = rows.iter().filter(|r| r.anchored(coverage)).count();
            let undecided = rows
                .iter()
                .filter(|r| !r.anchored(coverage))
                .filter_map(|r| r.best_cos)
                .collect();
            (anchored, undecided)
        };
        let (pos_anchored, undecided_pos) = split(&ev.positives);
        let (neg_anchored, undecided_neg) = split(&ev.negatives);
        cells.push(EvidenceCell {
            coverage,
            pos_anchored,
            neg_anchored,
            undecided_pos,
            undecided_neg,
        });
    }
    cells
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
                // The façade's own evidence read (GH #201) — the term dfs, the
                // per-hit provenance, and the served list in one call, so the
                // sweep judges exactly what the engine would.
                let view = vault.search_evidence(&q.query, K)?;
                Ok(QueryEvidence {
                    query: q.query.clone(),
                    bm25_hits,
                    bm25_best,
                    best_cos,
                    top,
                    rows: view
                        .results
                        .iter()
                        .map(|r| ServedRow {
                            path: r.result.path.clone(),
                            bm25_rank: r.bm25_rank,
                            cos: r.cos,
                            keep: q
                                .relevant
                                .iter()
                                .chain(q.tail_relevant.iter())
                                .any(|rel| paths_match(&r.result.path, rel)),
                        })
                        .collect(),
                    chunk_total: view.chunk_total,
                    terms: view.terms.iter().map(|t| (t.term.clone(), t.df)).collect(),
                    vouched: view.vouched,
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
             other half of any such claim (process rule 5, `make calibrate`)",
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
                r.served(),
                r.top,
            );
        }
    }
}

/// Print the **search evidence bake-off** (ADR-0015, GH #201) — the query-level rule's
/// window, re-derived from the labelled piles on every run rather than quoted from the day it
/// was read. The GH #187 idiom on search's side: the constant lives in
/// [`b2_core::search::BGE_BASE_EVIDENCE_BAR`] and its *justification* is recomputed here,
/// including whether the shipped bar still sits inside the window it was read from.
fn print_search_bakeoff(ev: &SearchEvidence, cells: &[EvidenceCell], model_id: &str) {
    println!(
        "  search evidence bake-off (D2 — the query-level rule, re-derived every run; GH #201)"
    );
    for line in [
        "the rule: serve iff the query has a LEXICAL ANCHOR (the vault carries ≥ coverage of the",
        "          query's term IDF — a word in most chunks weighs ~0, a word in none weighs the",
        "          most) OR its dense top-1 clears a cosine bar. Two signals on purpose: a",
        "          one-signal test cannot tell \"nothing matches\" from \"everything matches\" (#196).",
        "`cos need` reads over ONLY the queries the lexical half left undecided — a positive with",
        "          an anchor never needs its cosine kept, so a pure-cosine window overstates.",
    ] {
        println!("    {line}");
    }
    println!(
        "    n = {} positives / {} negatives labelled",
        ev.positives.len(),
        ev.negatives.len()
    );
    println!(
        "    {:>5}  {:>9} {:>9}  {:>15}  verdict",
        "cov", "pos anch", "neg anch", "cos need"
    );
    for cell in cells {
        let need = match (cell.cut_floor(), cell.keep_ceiling()) {
            (Some(cut), Some(keep)) => format!("({cut:.3},{keep:.3}]"),
            (Some(cut), None) => format!("> {cut:.3}"),
            (None, Some(keep)) => format!("≤ {keep:.3} (inert)"),
            (None, None) => "inert".to_string(),
        };
        let verdict = if cell.neg_anchored > 0 {
            format!(
                "✗ {} negative{} anchored — no cosine bar can rescue them",
                cell.neg_anchored,
                if cell.neg_anchored == 1 { "" } else { "s" }
            )
        } else if cell.admissible() {
            "✓ admissible".to_string()
        } else {
            "✗ cosine window empty".to_string()
        };
        println!(
            "    {:>5.2}  {:>4}/{:<4} {:>4}/{:<4}  {:>15}  {}",
            cell.coverage,
            cell.pos_anchored,
            ev.positives.len(),
            cell.neg_anchored,
            ev.negatives.len(),
            need,
            verdict,
        );
    }
    let admissible: Vec<&EvidenceCell> = cells.iter().filter(|c| c.admissible()).collect();
    if admissible.is_empty() {
        for line in [
            "→ NO admissible cell on this corpus: every lexical rule either vouches for a",
            "  labelled negative or leaves an empty cosine window. D2's bar is not earned,",
            "  and no rule ships (the GH #200 outcome, on search's side).",
        ] {
            println!("    {line}");
        }
    } else {
        println!(
            "    → {} of {} cells admissible; the widest cosine window belongs to {}",
            admissible.len(),
            cells.len(),
            widest(&admissible),
        );
    }
    print_shipped_bar(ev, model_id);
    let dense_only = |rows: &[QueryEvidence]| {
        (
            rows.iter().map(|r| r.dense_only()).sum::<usize>(),
            rows.iter().map(|r| r.served()).sum::<usize>(),
        )
    };
    let (pos_only, pos_served) = dense_only(&ev.positives);
    let (neg_only, neg_served) = dense_only(&ev.negatives);
    println!("    tail reading: served rows the lexical half never ranked —");
    println!(
        "      positives {pos_only}/{pos_served}, negatives {neg_only}/{neg_served}. The per-hit \
         rules over these are"
    );
    println!("      the tail bake-off's, below (GH #206).");
}

/// Name the admissible cells with the most cosine headroom, and within that band
/// the **most conservative corner** — the tightest `df` fraction and the
/// strictest coverage that still buy the widest window.
///
/// The tie matters more than the maximum does, and printing only a winner would
/// hide it: the grid's cells are not distinct rules but a plateau, and the
/// reading to carry into process rule 5's transfer check is "anywhere in this
/// band", not "at this point". A constant placed at a lone maximum would be
/// fitted to the grid's resolution.
fn widest(cells: &[&EvidenceCell]) -> String {
    let best = cells
        .iter()
        .map(|c| headroom(c))
        .fold(f64::NEG_INFINITY, f64::max);
    if !best.is_finite() {
        let inert = cells.iter().filter(|c| !headroom(c).is_finite()).count();
        return format!(
            "{inert} cell(s) where the cosine half is INERT — the lexical rule alone decides \
             every labelled query, so no `min_cos` is constrained there"
        );
    }
    let band: Vec<&&EvidenceCell> = cells
        .iter()
        .filter(|c| (headroom(c) - best).abs() < 1e-9)
        .collect();
    match band.iter().max_by(|a, b| {
        a.coverage
            .partial_cmp(&b.coverage)
            .unwrap_or(std::cmp::Ordering::Equal)
    }) {
        None => "—".to_string(),
        Some(c) => format!(
            "a plateau of {} cells at headroom {best:.3}; its strictest is coverage ≥ {:.2}",
            band.len(),
            c.coverage,
        ),
    }
}

/// A cell's cosine headroom: how much room a `min_cos` has between the
/// negatives it must cut and the positives it must keep. An inert cosine half
/// (nothing left undecided on one side) has unbounded room and is reported as
/// such rather than scored against bounded cells.
fn headroom(cell: &EvidenceCell) -> f64 {
    match (cell.cut_floor(), cell.keep_ceiling()) {
        (Some(cut), Some(keep)) => keep - cut,
        _ => f64::INFINITY,
    }
}

/// Where the **shipped** constant stands against this run's piles: the tripwire (a labelled
/// positive the bar would cut — ADR-0015 asserts zero, no headroom) and the defect it exists
/// to fix (a labelled negative it still serves).
///
/// **In the exit gate** since GH #202, at [`MAX_POSITIVES_CUT`] and [`MAX_NEGATIVES_SERVED`].
/// Read here rather than in the printer, so the number asserted and the number explained are
/// one number. `None` when the active model has no calibrated bar.
struct ShippedBar {
    bar: EvidenceBar,
    /// Labelled positives the bar cuts — the tripwire's direction.
    pos_cut: usize,
    /// Labelled negatives it still serves — the defect's direction.
    neg_served: usize,
    /// Queries where the engine's verdict and the harness's independent
    /// restatement of the same rule **disagree** — a drift between the two, which
    /// is silence unless something looks. Expected empty; printed, not gated,
    /// because it accuses the instrument as readily as the engine and the reader
    /// has to say which.
    faults: Vec<String>,
}

fn read_shipped_bar(ev: &SearchEvidence, model_id: &str) -> Option<ShippedBar> {
    let bar = EvidenceBar::for_model(model_id)?;
    // The ENGINE's verdict, not a restatement of it: this is what the gate
    // asserts and what the surfaces act on. `None` cannot occur here — the model
    // has a bar or this function returned already — so it is counted as served,
    // the same direction the surfaces take it (M2: no verdict is not "no match").
    Some(ShippedBar {
        bar,
        pos_cut: ev
            .positives
            .iter()
            .filter(|r| r.vouched == Some(false))
            .count(),
        neg_served: ev
            .negatives
            .iter()
            .filter(|r| r.vouched != Some(false))
            .count(),
        faults: ev
            .positives
            .iter()
            .chain(ev.negatives.iter())
            .filter(|r| {
                let restated = r.anchored(bar.min_term_coverage)
                    || r.best_cos.is_some_and(|c| c >= bar.min_cos);
                r.vouched != Some(restated)
            })
            .map(|r| r.query.clone())
            .collect(),
    })
}

fn print_shipped_bar(ev: &SearchEvidence, model_id: &str) {
    let Some(ShippedBar {
        bar,
        pos_cut,
        neg_served,
        faults,
    }) = read_shipped_bar(ev, model_id)
    else {
        println!("    shipped bar: none for this model — no verdict is offered (M2)");
        return;
    };
    // Every line below reads the ENGINE's verdict, so the numbers printed are the
    // numbers gated — one reading, not two that can drift apart.
    let vouches = |r: &QueryEvidence| r.vouched != Some(false);
    println!(
        "    shipped bar: coverage ≥ {:.2}, cos ≥ {:.3}",
        bar.min_term_coverage, bar.min_cos
    );
    println!(
        "      positives it would cut  {pos_cut}/{}   ← the search-side TRIPWIRE (D2: zero, no \
         headroom; GATED at ≤ {MAX_POSITIVES_CUT})",
        ev.positives.len()
    );
    println!(
        "      negatives it still serves {neg_served}/{}   ← the defect the bar exists to fix \
         (GATED at ≤ {MAX_NEGATIVES_SERVED})",
        ev.negatives.len()
    );
    for r in ev.positives.iter().filter(|r| !vouches(r)) {
        println!(
            "      [CUT] {:<40} cov {:>5}  cos {:>6}",
            truncate(&r.query, 40),
            r.coverage()
                .map(|c| format!("{c:.2}"))
                .unwrap_or_else(|| "—".to_string()),
            r.best_cos
                .map(|c| format!("{c:.3}"))
                .unwrap_or_else(|| "—".to_string()),
        );
    }
    if !faults.is_empty() {
        println!(
            "      [FAULT] the engine's verdict and this harness's restatement of the same rule \
             disagree on {} query/queries: {}. One of the two has drifted — read the engine's \
             `LexicalEvidence`, not this line's wording.",
            faults.len(),
            faults.join(", ")
        );
    }
    for r in &ev.negatives {
        println!(
            "      {:<40} cov {:>5}  cos {:>6}  → {}",
            truncate(&r.query, 40),
            r.coverage()
                .map(|c| format!("{c:.2}"))
                .unwrap_or_else(|| "—".to_string()),
            r.best_cos
                .map(|c| format!("{c:.3}"))
                .unwrap_or_else(|| "—".to_string()),
            if vouches(r) { "SERVED" } else { "no matches" },
        );
    }
}

/// A candidate **per-hit tail rule** family (GH #206) — the fold where a real
/// query's evidence runs out, D2's per-hit half. Every family folds as a
/// **prefix cut** (invariants.md D1): the default view ends at the first served
/// row failing the family's test, and every row below folds with it — a passing
/// row under a failing one folds too, because a fold that punched holes in the
/// fused order would let row order and fold visibly disagree.
///
/// The consequence that does all the work below: a filler row served *above* a
/// keep-labelled row must pass the test too, or the keep row under it folds. So
/// each family's constraint is read over the **keep-prefix** — every row at or
/// above a list's deepest keep row — not over the keep rows alone.
#[derive(Clone, Copy, PartialEq)]
enum TailRule {
    /// Fold at the first row the lexical half never ranked (`bm25_rank: None`)
    /// — the dense-only fold, the signal GH #201 measured (0 of 410 positive
    /// rows vs 20 of 50 negative ones). Parameterless, so nothing
    /// distributional to place — but **not** scale-free: `bm25_rank` is a rank
    /// in a pool-truncated list (`search::pool_size`), so "never ranked" is
    /// partly a fact about pool depth against vault size. `make calibrate
    /// ARGS=--search` measures that rather than assuming it (process rule 5's
    /// posture, owed even without a constant).
    Lexical,
    /// Fold at the first row that is dense-only **and** under a per-hit cosine
    /// bar — the shipped query rule's shape (lexical OR semantic) read per hit.
    LexOrCos,
    /// Fold at the first row under a per-hit cosine bar, lexical rank ignored
    /// (a row the dense half never ranked fails at any bar). The single-signal
    /// baseline the two-signal family must beat — the pure-cosine window at hit
    /// granularity, included to be retired the way GH #201 retired its
    /// query-level twin.
    Cos,
    /// Fold at the first row whose cosine sits more than δ under the list's own
    /// best — the "drop-off" shape, scale-adaptive in the query and still
    /// distributional in δ.
    CosDrop,
}

impl TailRule {
    const ALL: [TailRule; 4] = [
        TailRule::Lexical,
        TailRule::LexOrCos,
        TailRule::Cos,
        TailRule::CosDrop,
    ];

    fn label(self) -> &'static str {
        match self {
            TailRule::Lexical => "lexical (dense-only fold)",
            TailRule::LexOrCos => "lex-or-cos ≥ c",
            TailRule::Cos => "cos ≥ c",
            TailRule::CosDrop => "cos ≥ best − δ",
        }
    }

    /// Whether `row` passes this family's per-hit test at constant `p`
    /// (ignored by `Lexical`; `best` is the row's list's best served cosine,
    /// read only by `CosDrop`).
    fn passes(self, row: &ServedRow, p: f64, best: f64) -> bool {
        match self {
            TailRule::Lexical => row.bm25_rank.is_some(),
            TailRule::LexOrCos => row.bm25_rank.is_some() || row.cos.is_some_and(|c| c >= p),
            TailRule::Cos => row.cos.is_some_and(|c| c >= p),
            TailRule::CosDrop => row.cos.is_some_and(|c| c >= best - p),
        }
    }

    /// Where this family folds `rows` at constant `p`: the index of the first
    /// failing row, `rows.len()` when nothing folds. The prefix-cut definition
    /// lives here and nowhere else.
    fn fold(self, rows: &[ServedRow], p: f64) -> usize {
        let best = best_served_cos(rows);
        rows.iter()
            .position(|r| !self.passes(r, p, best))
            .unwrap_or(rows.len())
    }
}

/// The best served cosine of one list — [`TailRule::CosDrop`]'s reference
/// point. Read off the *served* rows, not the vault-wide dense top-1: the drop
/// rule is a claim about a list's own shape.
fn best_served_cos(rows: &[ServedRow]) -> f64 {
    rows.iter()
        .filter_map(|r| r.cos)
        .fold(f64::NEG_INFINITY, f64::max)
}

/// The keep-prefix of one served list: every row at or above its deepest
/// keep-labelled row. Empty when nothing served is keep-labelled — such a list
/// constrains no rule (there is nothing a fold could wrongly hide).
fn keep_prefix(rows: &[ServedRow]) -> &[ServedRow] {
    match rows.iter().rposition(|r| r.keep) {
        Some(last) => &rows[..=last],
        None => &[],
    }
}

/// One family's constraint, re-derived from a set of served lists (GH #187's
/// idiom on the per-hit axis): the range of its constant, if any, at which no
/// keep-prefix row anywhere fails. Both edges carry the row that set them,
/// because a window edge is only arguable once the pair is named.
struct TailConstraint {
    /// A keep-prefix row that cannot pass at **any** constant — the family is
    /// dead on this bench (e.g. a row with no cosine under a cosine family).
    dead: Option<(String, String)>,
    /// `Cos`/`LexOrCos`: the highest admissible bar (the binding keep-prefix
    /// row's own cosine). `CosDrop`: the lowest admissible δ (the binding
    /// row's drop from its list's best). `None` = unconstrained — no
    /// keep-prefix row engages the family's test at all.
    edge: Option<f64>,
    /// The (query-or-title, path) that set `edge`.
    edge_row: Option<(String, String)>,
    /// `Lexical` only: keep-prefix rows failing its fixed test. Nonzero =
    /// inadmissible, and each failure is a keep row the fold would hide.
    lexical_violations: usize,
}

/// Read one family's [`TailConstraint`] over `(list name, full rows, keep-prefix
/// length)` triples, where every keep-prefix row must pass. The dense fixture
/// reuses this with whole lists as keep-prefixes: on a single-domain vault every
/// served row is a real match, so the absolute "truncate nothing" is the same
/// constraint with the keep-set saturated. The full list rides along because
/// [`TailRule::CosDrop`]'s reference is the **list's** best served cosine —
/// reading it off the prefix alone could understate a required δ when the best
/// row sits below the prefix, and the constraint must read exactly what the
/// fold would.
fn tail_constraint<'a>(
    rule: TailRule,
    lists: impl Iterator<Item = (&'a str, &'a [ServedRow], usize)>,
) -> TailConstraint {
    let mut out = TailConstraint {
        dead: None,
        edge: None,
        edge_row: None,
        lexical_violations: 0,
    };
    for (name, rows, prefix_len) in lists {
        let best = best_served_cos(rows);
        for row in &rows[..prefix_len] {
            match rule {
                TailRule::Lexical => {
                    if row.bm25_rank.is_none() {
                        out.lexical_violations += 1;
                        if out.dead.is_none() {
                            out.dead = Some((name.to_string(), row.path.clone()));
                        }
                    }
                }
                // The cosine arms read the row's cosine through a finiteness
                // filter (PR #212 review): a NaN would sail through `c < e`
                // comparisons as silently-true-nowhere — worse, `is_none_or`
                // admits the FIRST row unconditionally, so a NaN first row
                // would seed the edge. A non-finite reading is *no reading*,
                // and the honest arm for that is `dead`, with the row named —
                // never a silent skip that reports the family unconstrained.
                TailRule::LexOrCos => {
                    if row.bm25_rank.is_none() {
                        match row.cos.filter(|c| c.is_finite()) {
                            // A served row is in at least one list, so a
                            // dense-only row carries a cosine unless it is
                            // non-finite — either way, no finite reading means
                            // this row can pass at no bar.
                            None => {
                                if out.dead.is_none() {
                                    out.dead = Some((name.to_string(), row.path.clone()));
                                }
                            }
                            Some(c) => {
                                if out.edge.is_none_or(|e| c < e) {
                                    out.edge = Some(c);
                                    out.edge_row = Some((name.to_string(), row.path.clone()));
                                }
                            }
                        }
                    }
                }
                TailRule::Cos => match row.cos.filter(|c| c.is_finite()) {
                    None => {
                        if out.dead.is_none() {
                            out.dead = Some((name.to_string(), row.path.clone()));
                        }
                    }
                    Some(c) => {
                        if out.edge.is_none_or(|e| c < e) {
                            out.edge = Some(c);
                            out.edge_row = Some((name.to_string(), row.path.clone()));
                        }
                    }
                },
                TailRule::CosDrop => match row.cos.filter(|c| c.is_finite()) {
                    None => {
                        if out.dead.is_none() {
                            out.dead = Some((name.to_string(), row.path.clone()));
                        }
                    }
                    Some(c) => {
                        let drop = best - c;
                        if out.edge.is_none_or(|e| drop > e) {
                            out.edge = Some(drop);
                            out.edge_row = Some((name.to_string(), row.path.clone()));
                        }
                    }
                },
            }
        }
    }
    out
}

/// What a family buys at constant `p` on the labelled lists: rows cut, split by
/// label. At an admissible constant `kept_cut` is zero **by construction** —
/// the constraint above is exactly "every keep-prefix row passes" — so a
/// nonzero here is an arithmetic fault, printed as such rather than assumed
/// away.
struct TailPayoff {
    filler_cut: usize,
    kept_cut: usize,
}

fn tail_payoff(rule: TailRule, lists: &[&QueryEvidence], p: f64) -> TailPayoff {
    let mut out = TailPayoff {
        filler_cut: 0,
        kept_cut: 0,
    };
    for q in lists {
        let fold = rule.fold(&q.rows, p);
        for row in &q.rows[fold..] {
            if row.keep {
                out.kept_cut += 1;
            } else {
                out.filler_cut += 1;
            }
        }
    }
    out
}

/// The per-hit tail bake-off's labelled-corpus reading (GH #206): every
/// family's re-derived constraint plus its payoff at the constraint's own edge
/// — the most aggressive admissible point, since payoff is monotone in the
/// constant. Judged over the **positives**: the query-level bar already
/// answers the negatives whole (GH #201), so a tail rule ships riding on a
/// vouched verdict; the negatives' would-be reading is kept as context for
/// what the rule would buy where a query bar had missed.
struct TailBench {
    keep_rows: usize,
    filler_rows: usize,
    /// Positives whose keep-prefix is the whole served list — a fold has
    /// nothing to cut there under any admissible rule.
    saturated: usize,
    /// The **oracle** ceiling: rows below each list's last keep row — what a
    /// perfect prefix fold (one placed by the labels themselves) would cut.
    /// Every family's payoff is read against this, because "cuts N rows" means
    /// nothing until the reachable maximum is beside it.
    oracle: usize,
    families: Vec<TailFamilyReading>,
    /// Junk rows the parameterless lexical fold would cut on the negatives'
    /// served lists, of their total — the GH #201 `dense_only` reading, priced
    /// as a fold.
    neg_lexical_cut: usize,
    neg_rows: usize,
}

struct TailFamilyReading {
    rule: TailRule,
    constraint: TailConstraint,
    /// Payoff at the constraint's edge; `None` when the family is dead here or
    /// (for the constant families) unconstrained-and-therefore-identical to a
    /// simpler family.
    payoff: Option<TailPayoff>,
}

fn score_search_tail(ev: &SearchEvidence) -> TailBench {
    let positives: Vec<&QueryEvidence> = ev.positives.iter().collect();
    let keep_rows = positives
        .iter()
        .flat_map(|q| q.rows.iter())
        .filter(|r| r.keep)
        .count();
    let filler_rows = positives
        .iter()
        .flat_map(|q| q.rows.iter())
        .filter(|r| !r.keep)
        .count();
    let saturated = positives
        .iter()
        .filter(|q| !q.rows.is_empty() && keep_prefix(&q.rows).len() == q.rows.len())
        .count();
    let oracle = positives
        .iter()
        .map(|q| q.rows.len() - keep_prefix(&q.rows).len())
        .sum();
    let families = TailRule::ALL
        .iter()
        .map(|&rule| {
            let constraint = tail_constraint(
                rule,
                positives.iter().map(|q| {
                    (
                        q.query.as_str(),
                        q.rows.as_slice(),
                        keep_prefix(&q.rows).len(),
                    )
                }),
            );
            let payoff = match rule {
                TailRule::Lexical => (constraint.lexical_violations == 0)
                    .then(|| tail_payoff(rule, &positives, f64::NAN)),
                _ => match (&constraint.dead, constraint.edge) {
                    (None, Some(edge)) => Some(tail_payoff(rule, &positives, edge)),
                    _ => None,
                },
            };
            TailFamilyReading {
                rule,
                constraint,
                payoff,
            }
        })
        .collect();
    let neg_lexical_cut = ev
        .negatives
        .iter()
        .map(|q| q.rows.len() - TailRule::Lexical.fold(&q.rows, f64::NAN))
        .sum();
    TailBench {
        keep_rows,
        filler_rows,
        saturated,
        oracle,
        families,
        neg_lexical_cut,
        neg_rows: ev.negatives.iter().map(|q| q.rows.len()).sum(),
    }
}

fn print_search_tail(bench: &TailBench) {
    println!(
        "  search tail bake-off (D2 per-hit — GH #206; the labels are GH #206's tail_relevant)"
    );
    for line in [
        "the rule under audition: end the DEFAULT VIEW at the first served row failing a",
        "          per-hit evidence test — a PREFIX CUT (D1), so a filler row above a keep row",
        "          must pass too or the keep row folds with it. Constraint re-derived per run",
        "          over the keep-prefixes; payoff read at the constraint's own edge. \"No tail",
        "          fold\" is an admissible winner (the GH #200 outcome, on search's side).",
    ] {
        println!("    {line}");
    }
    println!(
        "    served rows over the positives: {} keep / {} filler by label; {} list(s) saturated \
         (keep to the last served row)",
        bench.keep_rows, bench.filler_rows, bench.saturated
    );
    println!(
        "    oracle ceiling: a fold placed by the labels themselves (each list's last keep row) \
         would cut {} of {} — every payoff below is read against this",
        bench.oracle, bench.filler_rows
    );
    for f in &bench.families {
        let constraint = match f.rule {
            TailRule::Lexical => {
                if f.constraint.lexical_violations == 0 {
                    "admissible (no dense-only keep-prefix row)".to_string()
                } else {
                    let (q, p) = f.constraint.dead.as_ref().expect("violation names a row");
                    format!(
                        "✗ {} keep-prefix row(s) are dense-only — first: {} → {}",
                        f.constraint.lexical_violations,
                        truncate(q, 28),
                        p
                    )
                }
            }
            _ => match (&f.constraint.dead, f.constraint.edge) {
                (Some((q, p)), _) => {
                    format!(
                        "DEAD — {} → {} has no cosine to clear any bar",
                        truncate(q, 28),
                        p
                    )
                }
                (None, None) => "unconstrained (no keep-prefix row engages the test)".to_string(),
                (None, Some(edge)) => {
                    let (q, p) = f
                        .constraint
                        .edge_row
                        .as_ref()
                        .map(|(q, p)| (truncate(q, 28), p.clone()))
                        .unwrap_or_default();
                    match f.rule {
                        TailRule::CosDrop => format!("δ ≥ {edge:.3}  (set by {q} → {p})"),
                        _ => format!("c ≤ {edge:.3}  (set by {q} → {p})"),
                    }
                }
            },
        };
        let payoff = match &f.payoff {
            None => "—".to_string(),
            Some(p) if p.kept_cut > 0 => format!(
                "[FAULT] cuts {} keep row(s) at its own edge — the constraint arithmetic is wrong",
                p.kept_cut
            ),
            Some(p) => format!("cuts {} of {} filler rows", p.filler_cut, bench.filler_rows),
        };
        println!("    {:<24} {:<58} {}", f.rule.label(), constraint, payoff);
    }
    println!(
        "    negatives context: the lexical fold alone would cut {}/{} of their junk rows — the \
         query-level bar already cuts all of them (GH #201), so a tail rule is judged on what it \
         buys ABOVE that bar",
        bench.neg_lexical_cut, bench.neg_rows
    );
}

/// The tail bake-off's JSON (`search_tail` in the orthogonal row): each
/// family's re-derived constraint and edge payoff. The served rows it was read
/// from are already in `search_evidence` per query, so any other constant is
/// re-derivable from the row — the `discovery_fold` convention.
fn tail_json(bench: &TailBench) -> serde_json::Value {
    let round = |v: f64| (v * 1e4).round() / 1e4;
    serde_json::json!({
        "keep_rows": bench.keep_rows,
        "filler_rows": bench.filler_rows,
        "saturated_lists": bench.saturated,
        "oracle": bench.oracle,
        "neg_lexical_cut": bench.neg_lexical_cut,
        "neg_rows": bench.neg_rows,
        "families": bench.families.iter().map(|f| serde_json::json!({
            "rule": f.rule.label(),
            "dead": f.constraint.dead.as_ref().map(|(q, p)| serde_json::json!([q, p])),
            "edge": f.constraint.edge.map(round),
            "edge_row": f.constraint.edge_row.as_ref().map(|(q, p)| serde_json::json!([q, p])),
            "lexical_violations": f.constraint.lexical_violations,
            "filler_cut_at_edge": f.payoff.as_ref().map(|p| p.filler_cut),
            "kept_cut_at_edge": f.payoff.as_ref().map(|p| p.kept_cut),
        })).collect::<Vec<_>>(),
    })
}

/// One family's dense-fixture reading (GH #206): the same [`TailConstraint`]
/// read over **whole** title lists — every served row is a real match by
/// geometry, so the keep-prefix is the entire list and the constraint *is* the
/// absolute ("truncate nothing"). `lexical_cut` is the parameterless family's
/// visible cost here: rows its fold would remove from title queries' views.
struct TailFamilyTransfer {
    rule: TailRule,
    constraint: TailConstraint,
    lexical_cut: usize,
}

fn dense_tail_families(titles: &[SearchProbe]) -> Vec<TailFamilyTransfer> {
    TailRule::ALL
        .iter()
        .map(|&rule| TailFamilyTransfer {
            rule,
            constraint: tail_constraint(
                rule,
                titles
                    .iter()
                    .map(|t| (t.query.as_str(), t.rows.as_slice(), t.rows.len())),
            ),
            lexical_cut: match rule {
                TailRule::Lexical => titles
                    .iter()
                    .map(|t| t.rows.len() - rule.fold(&t.rows, f64::NAN))
                    .sum(),
                _ => 0,
            },
        })
        .collect()
}

/// Print the dense fixture's tail-transfer reading (GH #206) — the per-hit
/// sibling of the query bar's `search_transfer` block, and the bench that
/// carries D1's absolute for this bake-off.
fn print_dense_tail(titles: &[SearchProbe]) {
    println!(
        "  tail        the per-hit tail bench on this geometry (GH #206): every served row of a"
    );
    println!(
        "              title query is a real match, so a rule that folds one is disqualified —"
    );
    println!("              the GH #200 absolute, on search's side");
    for f in dense_tail_families(titles) {
        let reading = match f.rule {
            TailRule::Lexical => {
                if f.lexical_cut == 0 {
                    "cuts 0 title rows".to_string()
                } else {
                    let (q, p) = f.constraint.dead.as_ref().expect("cut names a row");
                    format!(
                        "✗ cuts {} title row(s) — first: {} → {}",
                        f.lexical_cut,
                        truncate(q, 24),
                        p
                    )
                }
            }
            _ => match (&f.constraint.dead, f.constraint.edge) {
                (Some((q, p)), _) => {
                    format!("DEAD — {} → {} has no cosine", truncate(q, 24), p)
                }
                (None, None) => "unconstrained (no row engages the test)".to_string(),
                (None, Some(edge)) => {
                    let (q, p) = f
                        .constraint
                        .edge_row
                        .as_ref()
                        .map(|(q, p)| (truncate(q, 24), p.clone()))
                        .unwrap_or_default();
                    match f.rule {
                        TailRule::CosDrop => {
                            format!("needs δ ≥ {edge:.3} to fold nothing (set by {q} → {p})")
                        }
                        _ => format!("needs c ≤ {edge:.3} to fold nothing (set by {q} → {p})"),
                    }
                }
            },
        };
        println!("              {:<24} {}", f.rule.label(), reading);
    }
}

/// The dense tail-transfer reading as JSON, nested under the dense row's
/// `search_transfer` key (additive subkey, per the row conventions).
fn dense_tail_json(titles: &[SearchProbe]) -> serde_json::Value {
    let round = |v: f64| (v * 1e4).round() / 1e4;
    serde_json::json!(dense_tail_families(titles)
        .iter()
        .map(|f| serde_json::json!({
            "rule": f.rule.label(),
            "dead": f.constraint.dead.as_ref().map(|(q, p)| serde_json::json!([q, p])),
            "edge": f.constraint.edge.map(round),
            "edge_row": f.constraint.edge_row.as_ref().map(|(q, p)| serde_json::json!([q, p])),
            "lexical_cut": f.lexical_cut,
        }))
        .collect::<Vec<_>>())
}

/// The tail bake-off's **cross-bench join** (GH #206), printed once both
/// corpora have been read in the same run: a family ships only if some
/// constant is admissible on the labelled corpus *and* folds nothing on the
/// dense fixture *and* still cuts labelled filler there — read at the joint
/// edge, the most aggressive point both benches allow.
fn print_tail_join(ev: &SearchEvidence, orth: &TailBench, titles: &[SearchProbe]) {
    println!("\n{}", "=".repeat(78));
    println!("search tail — the cross-bench join (GH #206; both corpora, one run)");
    let positives: Vec<&QueryEvidence> = ev.positives.iter().collect();
    let dense = dense_tail_families(titles);
    let mut winner = false;
    for (orth_f, dense_f) in orth.families.iter().zip(&dense) {
        let rule = orth_f.rule;
        let verdict =
            match rule {
                TailRule::Lexical => {
                    if orth_f.constraint.lexical_violations > 0 {
                        format!(
                        "✗ inadmissible on the labelled corpus ({} keep-prefix row(s) dense-only)",
                        orth_f.constraint.lexical_violations
                    )
                    } else if dense_f.lexical_cut > 0 {
                        format!(
                            "✗ disqualified on the dense fixture (cuts {} title rows)",
                            dense_f.lexical_cut
                        )
                    } else {
                        let cut = orth_f.payoff.as_ref().map(|p| p.filler_cut).unwrap_or(0);
                        if cut == 0 {
                            "✓ admissible and VACUOUS — cuts 0 of the labelled filler, so it buys \
                         nothing the query bar has not already bought"
                                .to_string()
                        } else {
                            winner = true;
                            format!(
                                "✓ admissible on both benches — cuts {cut} of the {} an oracle \
                                 fold reaches",
                                orth.oracle
                            )
                        }
                    }
                }
                _ => {
                    let orth_dead = orth_f.constraint.dead.is_some();
                    let dense_dead = dense_f.constraint.dead.is_some();
                    if orth_dead || dense_dead {
                        format!(
                            "✗ DEAD on the {} bench (a required row has no cosine)",
                            if orth_dead { "labelled" } else { "dense" }
                        )
                    } else {
                        // Joint edge: the tighter bench binds. `None` = that bench
                        // leaves the constant free.
                        let joint = match rule {
                            TailRule::CosDrop => {
                                match (orth_f.constraint.edge, dense_f.constraint.edge) {
                                    (Some(a), Some(b)) => Some(a.max(b)),
                                    (a, b) => a.or(b),
                                }
                            }
                            _ => match (orth_f.constraint.edge, dense_f.constraint.edge) {
                                (Some(a), Some(b)) => Some(a.min(b)),
                                (a, b) => a.or(b),
                            },
                        };
                        match joint {
                            None => {
                                "degenerates to the lexical fold (no row on either bench engages \
                                 the test) — see that family's verdict"
                                    .to_string()
                            }
                            Some(edge) => {
                                let payoff = tail_payoff(rule, &positives, edge);
                                if payoff.kept_cut > 0 {
                                    format!(
                                    "[FAULT] cuts {} keep row(s) at the joint edge {edge:.3} — \
                                     the join arithmetic is wrong",
                                    payoff.kept_cut
                                )
                                } else if payoff.filler_cut == 0 {
                                    format!(
                                    "✓ admissible to {} {edge:.3} and VACUOUS — cuts 0 labelled \
                                     filler there",
                                    if rule == TailRule::CosDrop { "δ ≥" } else { "c ≤" }
                                )
                                } else {
                                    winner = true;
                                    format!(
                                    "✓ admissible at {} {edge:.3} on both benches — cuts {} of \
                                     the {} an oracle fold reaches",
                                    if rule == TailRule::CosDrop { "δ ≥" } else { "c ≤" },
                                    payoff.filler_cut,
                                    orth.oracle
                                )
                                }
                            }
                        }
                    }
                }
            };
        println!("  {:<24} {}", rule.label(), verdict);
    }
    if winner {
        println!(
            "  → the ✓ family/families above survive both corpora at their joint edges. That is \
             ADMISSIBILITY, not a shipping order: a joint edge sits AT a bench's own binding row \
             (zero headroom — the constant placement the house sizing method forbids), each payoff \
             reads against the oracle ceiling above, and a shipped constant owes process rule 5's \
             real-vault reading besides (`make calibrate ARGS=--search`, the tail block). The ruling of \
             record lives in docs/evals/README.md."
        );
    } else {
        println!(
            "  → NO family survives both benches with a nonzero payoff: the incumbent — no \
             per-hit tail fold — stands, the GH #200 outcome on search's side. The filler the \
             complaint names is above every admissible fold, so the honesty still rides on the \
             query-level bar and the copy."
        );
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

/// `LocalEmbedder::embed_batch` must be a faithful map of `embed`: right-padding short rows
/// to the batch's longest and masking them out has to leave each row's CLS vector unchanged.
/// The reindex path batches freely, so a regression here would silently corrupt every stored
/// vector — and every score this eval prints.
///
/// It lives in the eval rather than `cargo test` because it needs the provisioned model,
/// which the fast suite deliberately never touches (ADR-0013). Running it here means it
/// actually runs, instead of sitting behind an `#[ignore]` nobody passes `--ignored` to.
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
/// A corpus with no more chunks than a signal's candidate pool truncates *neither* list, so
/// both are already complete, widening cannot add a candidate, and every score above is
/// invariant under **candidate width**: a change to either view's headroom or to
/// `search::pool_size` prints bit-identical numbers here while genuinely reordering a real
/// vault (GH #141). Judged on the **narrower** of the two pools, since blindness is a claim
/// about every number the run prints.
///
/// Scoped deliberately to width — `RRF_K` re-weights the *same* two lists, so it reorders
/// results on any corpus, and this eval sees that. A warning, not a gate: the point is that a
/// reader must not take an unmoved number as evidence of no change. The property itself is
/// measured by `--example stability`, on a vault big enough for the pool to bind.
fn warn_if_pool_blind(chunks: usize) {
    let pool = chunk_candidate_pool(K).min(note_candidate_pool(K));
    if chunks <= pool {
        eprintln!(
            "[warn] {chunks} chunks ≤ {pool}-candidate pool — neither signal is truncated here, so a\n\
             \x20      candidate-width change (either hit pool, pool_size) cannot move any number in this run\n\
             \x20      (GH #141). `make stability` measures that property on a large vault. (RRF_K is\n\
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
                 other half of any such claim (process rule 5, `make calibrate`)",
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
    tail: Option<&TailBench>,
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
            let bar = EvidenceBar::for_model(model);
            let row = |r: &QueryEvidence| serde_json::json!({
                "q": r.query,
                "bm25_hits": r.bm25_hits,
                "bm25_best": r.bm25_best.map(|b| (b * 1e4).round() / 1e4),
                "best_cos": r.best_cos.map(|c| (c * 1e4).round() / 1e4),
                "top": r.top,
                "served": r.served(),
                // NEW keys (absent from rows before 2026-08-22, GH #201 Phase C):
                // the per-query evidence the bake-off is swept over, recorded raw
                // so any (fraction, coverage, cos) cell is re-derivable from a row
                // without re-running the model — the `discovery_fold` convention.
                "dense_only": r.dense_only(),
                // NEW key (absent from rows before GH #206): the served list
                // itself, per row — path, per-hit provenance, and relevance by
                // label — so any per-hit tail rule is re-derivable from a row
                // without re-running the model, the `discovery_fold` convention
                // at hit granularity. `keep` reads `relevant` ∪ `tail_relevant`.
                "rows": r.rows.iter().map(|row| serde_json::json!({
                    "path": row.path,
                    "bm25_rank": row.bm25_rank,
                    "cos": row.cos.map(|c| (c * 1e4).round() / 1e4),
                    "keep": row.keep,
                })).collect::<Vec<_>>(),
                "chunk_total": r.chunk_total,
                "terms": r.terms.iter().map(|(t, df)| serde_json::json!([t, df]))
                    .collect::<Vec<_>>(),
                "vouched": bar.map(|b| r.anchored(b.min_term_coverage)
                    || r.best_cos.is_some_and(|c| c >= b.min_cos)),
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
                // NEW key (GH #201, Phase C): the whole bake-off grid, so the
                // admissible window is re-derivable from the row — including the
                // shipped bar's own three constants, which is what makes a later
                // reader able to see that a bar has drifted out of the window it
                // was read from.
                "bar": bar.map(|b| serde_json::json!({
                    "min_term_coverage": b.min_term_coverage,
                    "min_cos": b.min_cos,
                })),
                "bakeoff": bake_off(e).iter().map(|c| serde_json::json!({
                    "min_term_coverage": c.coverage,
                    "pos_anchored": c.pos_anchored,
                    "neg_anchored": c.neg_anchored,
                    "cos_cut_floor": c.cut_floor().map(|v| (v * 1e4).round() / 1e4),
                    "cos_keep_ceiling": c.keep_ceiling().map(|v| (v * 1e4).round() / 1e4),
                    "admissible": c.admissible(),
                })).collect::<Vec<_>>(),
            })
        }),
        // The fold bake-off (GH #200, Phase B) — every candidate rule's reading
        // on this run, with the per-anchor folds it was read from. Recorded on
        // the default row only: the sweep's variants re-chunk the corpus, and a
        // disclosure rule judged on a non-shipped chunker is a number about the
        // chunker.
        "discovery_fold": fold.map(fold_json),
        // NEW key (absent from rows before GH #206): the per-hit tail bake-off
        // — each family's re-derived constraint and edge payoff. Default row
        // only, for the same reason as `discovery_fold`; the served rows it was
        // read from are under `search_evidence`, so any other constant is
        // re-derivable from the row.
        "search_tail": tail.map(tail_json),
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
