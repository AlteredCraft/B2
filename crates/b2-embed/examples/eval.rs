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
//!    does not exercise. Positive anchors score **rank** (did a labelled
//!    cluster-mate surface, and how high) two ways: the historical per-anchor
//!    metric, which stops at the *first* mate it finds and therefore saturates —
//!    it has read 1.000 across changes it was meant to judge — and the per-mate
//!    companion added by GH #183, which scores every labelled mate separately so
//!    a hard one demoted behind an easy one is visible. Read the second when
//!    judging a ranking change. **Negative anchors** — deliberate loner
//!    notes whose labelled answer is "nothing relates" — score **suppression**:
//!    does discovery return zero candidates, or serve strangers? And every
//!    surfaced candidate's score lands in one of two **cosine piles** — labelled
//!    related vs. everything else surfaced — the measured distributions the
//!    quality floor is calibrated from (index-engine.md §3's ruling, PR #145).
//!    The floor ships (GH #150, the per-anchor z rule in `discover::candidates`),
//!    so ranks + suppression score the **floored** surface while the piles come
//!    from a second, **unfloored** pass — the calibration data keeps accruing
//!    ungated. Suppression is fully gated: every negative anchor must come back
//!    clean. (The watercolor ↔ stain-removal pair that once blocked this was
//!    resolved 2026-08-11 by replacing watercolor with a cleanly orthogonal
//!    loner — an arguable negative label is corpus debt, not a target.)
//! 5. **The floor's own calibration** (GH #187) — a third discovery pass dumps
//!    every candidate's *stage-1 z*, the number `DiscoveryFloor`'s two constants
//!    are compared against, split into the three populations that actually
//!    calibrate them: labelled mates, strangers on positive anchors
//!    (`member_z`'s comparison set), and negative anchors' leaders
//!    (`leader_z`'s). From those it re-derives both admissible windows on the
//!    **current** corpus and prints the member bar's trade curve. This exists
//!    because the #150 windows were frozen into a rustdoc as if timeless and went
//!    stale the first time the corpus grew a shape they were never measured
//!    against: constants in code, measurements in the harness.
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

use b2_core::chunk::ChunkConfig;
use b2_core::db::FtsTokenizer;
use b2_core::discover::DiscoveryFloor;
use b2_core::embed::Embedder;
use b2_core::vault::{chunk_candidate_pool, note_candidate_pool, Vault};
use b2_embed::{provision, EmbedConfig, LocalEmbedder};
use serde::Deserialize;
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
/// A [`DiscoveryFloor`] that gates nothing: both bars at `-∞`, so the shipped
/// path computes every candidate's z and cuts none of them. The z dump's whole
/// trick (see [`score_floor_z`]).
const UNGATED_FLOOR: DiscoveryFloor = DiscoveryFloor {
    leader_z: f64::NEG_INFINITY,
    member_z: f64::NEG_INFINITY,
};
/// The soft reference floor on the default config's hybrid note hit@1.
const FLOOR_HIT1: f64 = 0.75;

#[derive(Deserialize)]
struct QuerySet {
    queries: Vec<Labelled>,
}

#[derive(Deserialize)]
struct Labelled {
    query: String,
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
    /// Deliberately **not** in the exit gate yet. The repo's own precedent for
    /// the discovery floor (GH #150) is measure-then-calibrate: both cheap
    /// variants failed when measured, so a floor here would be intuition
    /// wearing a number. It reports until a baseline exists to price it.
    mate: Agg,
    /// [`Self::mate`] measured on the **unfloored** surface — which is the
    /// *best-passage* ordering, making this half of a standing A/B against the
    /// shipped centroid one.
    ///
    /// Why that works: `discover::candidates` sorts by `z` then `score`, and
    /// `floor: None` leaves every `z` at `None`, so the comparator falls
    /// straight through to `score` — the exact stage-2 best-passage number.
    /// `similar` therefore orders by centroid and `similar_raw` by best
    /// passage, and the gap between these two aggregates is the cost
    /// index-engine.md §3 accepted when it made `z` the sort key, priced
    /// rather than asserted.
    ///
    /// **Read with its caveat**: the unfloored surface is also *unsuppressed*,
    /// so this is an ordering-plus-floor delta, not ordering in isolation. It
    /// is a lower bound on sensitivity — enough to prove the metric is not
    /// inert to the sort key, not a clean single-variable ablation.
    /// [`Self::mate_suppressed`] separates the two contributions.
    mate_raw: Agg,
    /// Labelled mates the unfloored surface reaches but the **shipped** one does
    /// not serve at all — the floor suppressing a human-labelled relation
    /// outright, as opposed to merely ranking it lower.
    ///
    /// This is the number that separates "demoted" from "gone", and it is the
    /// one the old per-anchor metric could never show: an anchor whose easy mate
    /// still hits scores 1.000 whether its hard mate is at rank 2 or absent
    /// entirely. Recorded because a suppressed *labelled* mate is a different
    /// (and worse) failure than a demoted one, and the two need separate
    /// evidence before anything is done about either (that is
    /// [#182](https://github.com/AlteredCraft/B2/issues/182)'s call, not this
    /// harness's — the eval measures, it does not tune).
    mate_suppressed: usize,
    /// Floored per-mate ranks in label order, so pass 2 can pair each mate's
    /// unfloored rank with its shipped one. Both passes walk `set.anchors` and
    /// each `label.expected` in the same order, so the flat index lines up.
    mate_floored: Vec<Option<usize>>,
    /// Negative anchors asked.
    neg_n: usize,
    /// Negative anchors that (correctly) surfaced zero candidates.
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

/// One anchor's **complete, ungated** stage-1 z reading — every candidate note in
/// the corpus, in z order, with the human label attached (GH #187).
///
/// The cosine piles above are the same idea one stage later and in a different
/// unit; this is the unit the [`DiscoveryFloor`] actually judges in, which is why
/// the piles could never re-derive its constants.
struct AnchorZ {
    anchor: String,
    /// True for a negative anchor (empty `expected`) — its whole list is
    /// strangers by label, and its leader is what `leader_z` answers to.
    negative: bool,
    /// (candidate path, stage-1 z, labelled a mate of this anchor) in served
    /// order, which under an ungated floor *is* descending z.
    candidates: Vec<(String, f64, bool)>,
    /// What the **shipped** floor actually serves for this anchor, read from a
    /// second pass at the same depth. Ground truth for [`FloorZ::replay_faults`]:
    /// every window and trade-curve row below is a *replay* of the floor's rule
    /// over `candidates`, and a replay that has drifted from
    /// `discover::candidates` would price a trade the engine does not make.
    served: Vec<String>,
}

impl AnchorZ {
    /// The z the leader gate reads — the top candidate's. `None` only if the
    /// anchor produced no scorable candidate at all.
    fn leader(&self) -> Option<f64> {
        self.candidates.first().map(|&(_, z, _)| z)
    }
    /// The paths a given floor would serve here, by replaying its rule over the
    /// ungated dump: the leader gate first (below it the anchor serves nothing
    /// at all), then the member bar.
    fn replay(&self, floor: &DiscoveryFloor) -> Vec<&str> {
        if !self.leader().is_some_and(|z| z >= floor.leader_z) {
            return Vec::new();
        }
        self.candidates
            .iter()
            .filter(|&&(_, z, _)| z >= floor.member_z)
            .map(|(path, _, _)| path.as_str())
            .collect()
    }
    /// This anchor's labelled mates' z's (empty for a negative anchor).
    fn mates(&self) -> impl Iterator<Item = f64> + '_ {
        self.candidates
            .iter()
            .filter(|&&(_, _, mate)| mate)
            .map(|&(_, z, _)| z)
    }
    /// Everything on this anchor's list a human did *not* label — on a positive
    /// anchor, exactly the population `member_z` has to cut.
    fn strangers(&self) -> impl Iterator<Item = f64> + '_ {
        self.candidates
            .iter()
            .filter(|&&(_, _, mate)| !mate)
            .map(|&(_, z, _)| z)
    }
}

/// The ungated stage-1 z dump across every discovery anchor, plus what it says
/// about the two [`DiscoveryFloor`] constants (GH #187).
///
/// Three populations, because the two constants answer to different ones — the
/// conflation is what made "the negatives gate would catch a bad `member_z`"
/// look true when it is not: while `member_z ≤ leader_z`, a negative anchor is
/// clean **iff its leader is cut**, so no member bar can dirty (or clean) it.
/// `leader_z` is calibrated by negative-anchor leaders against positive-anchor
/// leaders; `member_z` by strangers on positive anchors against labelled mates,
/// and *nothing in the exit gate watches that second pair*.
#[derive(Default)]
struct FloorZ {
    /// Every anchor the floor could compute statistics for, in label order.
    anchors: Vec<AnchorZ>,
    /// Anchors whose candidate pool was under `FLOOR_MIN_POPULATION` or had zero
    /// variance, so no z exists to calibrate from. Named rather than silently
    /// dropped: on a corpus small enough for this to happen, every window below
    /// is measured on fewer anchors than the labels suggest.
    ungraded: Vec<String>,
}

impl FloorZ {
    /// (a) Labelled mates' z's — what `member_z` must keep.
    fn mate_z(&self) -> Vec<f64> {
        self.anchors.iter().flat_map(|a| a.mates()).collect()
    }
    /// (b) Strangers on **positive** anchors — what `member_z` must cut. The
    /// negative anchors' own candidates are deliberately excluded: they are
    /// `leader_z`'s business, and folding them in here is what would let a
    /// member window look wider than it is.
    fn stranger_z(&self) -> Vec<f64> {
        self.anchors
            .iter()
            .filter(|a| !a.negative)
            .flat_map(|a| a.strangers())
            .collect()
    }
    /// (c) Negative anchors' leaders — what `leader_z` must cut.
    fn neg_leader_z(&self) -> Vec<f64> {
        self.anchors
            .iter()
            .filter(|a| a.negative)
            .filter_map(|a| a.leader())
            .collect()
    }
    /// Positive anchors' leaders — what `leader_z` must keep, or it empties a
    /// list that has a real mate on it.
    fn pos_leader_z(&self) -> Vec<f64> {
        self.anchors
            .iter()
            .filter(|a| !a.negative)
            .filter_map(|a| a.leader())
            .collect()
    }

    /// What a candidate floor would actually serve, replayed over this dump:
    /// (labelled mates served, mates labelled, strangers served on positive
    /// anchors, negative anchors left dirty). The leader gate is applied first,
    /// exactly as `discover::candidates` does — an anchor whose leader falls
    /// below `leader_z` serves nothing at all, mates included.
    fn serve(&self, floor: &DiscoveryFloor) -> (usize, usize, usize, usize) {
        let (mut mates, mut labelled, mut strangers, mut dirty) = (0, 0, 0, 0);
        for a in &self.anchors {
            let served = a.replay(floor);
            labelled += a.mates().count();
            for &(ref path, _, mate) in &a.candidates {
                if !served.contains(&path.as_str()) {
                    continue;
                }
                match (a.negative, mate) {
                    (true, _) => dirty += 1,
                    (false, true) => mates += 1,
                    (false, false) => strangers += 1,
                }
            }
        }
        (mates, labelled, strangers, dirty)
    }

    /// Anchors where replaying the **shipped** floor over the dump disagrees
    /// with what the engine actually served — the instrument check that keeps
    /// every window and trade-curve row above honest. Empty is the only
    /// acceptable reading: the replay is a re-statement of `DiscoveryFloor`'s
    /// rule, so a disagreement means the rule moved and this block is now
    /// pricing a floor that no longer ships.
    fn replay_faults(&self) -> Vec<&str> {
        let shipped = DiscoveryFloor::default();
        self.anchors
            .iter()
            .filter(|a| {
                let mut predicted = a.replay(&shipped);
                let mut actual: Vec<&str> = a.served.iter().map(String::as_str).collect();
                predicted.sort_unstable();
                actual.sort_unstable();
                predicted != actual
            })
            .map(|a| a.anchor.as_str())
            .collect()
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

    // Load the labelled sets.
    let set: QuerySet =
        serde_json::from_str(&std::fs::read_to_string(evals_dir.join("queries.json"))?)?;
    let sim_set: SimilarSet =
        serde_json::from_str(&std::fs::read_to_string(evals_dir.join("similar.json"))?)?;

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
    let bm25 = score_pass(&vault, &set.queries, Retrieval::Fused)?;
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
        let pass = score_pass(&vault, &set.queries, Retrieval::Fused)?;
        vault.rebuild_fts(FtsTokenizer::PorterUnicode61)?;
        Some(pass)
    } else {
        None
    };

    // ---- Phase 2: embed → vector-only ablation + hybrid + discovery. ---------
    let (chunks, embed_secs) = timed_embed(&vault)?;
    let vector = score_pass(&vault, &set.queries, Retrieval::VectorOnly)?;
    let hybrid = score_pass(&vault, &set.queries, Retrieval::Fused)?;
    let similar = score_similar(&vault, &sim_set)?;
    // The floor's own calibration data, in the floor's own unit (GH #187). Runs
    // after `score_similar` and restores the shipped floor, so every metric above
    // is scored on the surface that ships.
    let floor_z = score_floor_z(&mut vault, &sim_set)?;
    eprintln!(
        "[eval] embedded {chunks} chunks in {embed_secs:.1}s ({} candidates per signal at K={K}, \
         {} for the passage view)\n",
        note_candidate_pool(K),
        chunk_candidate_pool(K)
    );
    warn_if_pool_blind(chunks);

    print_default_report(&set.queries, &bm25, &vector, &hybrid, &similar, &floor_z);

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
            &set.queries,
            Some(&bm25),
            Some(&vector),
            &hybrid,
            Some(&similar),
            Some(&floor_z),
        ),
    )?;

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
        let vec_unstemmed = score_pass(&vault, &set.queries, Retrieval::VectorOnly)?;
        let hybrid_unstemmed = score_pass(&vault, &set.queries, Retrieval::Fused)?;

        println!("\n{}", "=".repeat(78));
        println!(
            "stemmer ablation — chunks_fts rebuilt `unicode61` (unstemmed) over identical chunks + vectors (GH #157)"
        );
        let dense_moved = (0..set.queries.len())
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
        print_rank_moves(&set.queries, &bm25, bm25_unstemmed);
        println!("  hybrid, unstemmed vs shipped porter:");
        print_rank_moves(&set.queries, &hybrid, &hybrid_unstemmed);
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
                &set.queries,
                Some(bm25_unstemmed),
                Some(&vec_unstemmed),
                &hybrid_unstemmed,
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
        println!(
            "{:<24} {:>7} {:>8}   note h@1/MRR   vec h@1/MRR    chunk h@1/MRR   similar h@3   neg clean",
            "config", "chunks", "embed_s"
        );
        for (label, cfg) in variants {
            vault.set_chunk_config(cfg.clone());
            vault.project(true)?; // force: re-chunk everything, clearing vectors
            let (chunks, embed_secs) = timed_embed(&vault)?;
            let vec_pass = score_pass(&vault, &set.queries, Retrieval::VectorOnly)?;
            let pass = score_pass(&vault, &set.queries, Retrieval::Fused)?;
            let sim = score_similar(&vault, &sim_set)?;
            println!(
                "{:<24} {:>7} {:>8.1}   {:.2} / {:.3}    {:.2} / {:.3}    {:.2} / {:.3}    {:.2}          {}/{}",
                label,
                chunks,
                embed_secs,
                pass.note.hit1(),
                pass.note.mrr(),
                vec_pass.note.hit1(),
                vec_pass.note.mrr(),
                pass.chunk.hit1(),
                pass.chunk.mrr(),
                sim.rank.hit3(),
                sim.neg_clean,
                sim.neg_n,
            );
            // The readout the A/B is actually judged on: at this n every aggregate
            // delta above is 1–2 queries, so the aggregate is a smoke alarm and the
            // per-query win/loss list is the data (docs/evals/README.md, the
            // process rules).
            print_rank_moves(&set.queries, &hybrid, &pass);
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
                    &set.queries,
                    None,
                    Some(&vec_pass),
                    &pass,
                    Some(&sim),
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
    // The suppression gate (GH #150, complete): EVERY negative anchor must come
    // back clean under the shipped discovery floor — a run that serves a single
    // stranger where the label says "nothing" exits non-zero. This was a `>= 2`
    // ratchet while watercolor ↔ stain-removal — a pair the model scored the
    // corpus's strongest — sat in the negative set making its own label arguable;
    // the 2026-08-11 resolution replaced watercolor with a cleanly
    // orthogonal loner (throat-singing) rather than argue, so every negative is
    // now unambiguous and the gate is the `==` the issue specified.
    if similar.neg_clean != similar.neg_n {
        eprintln!(
            "\n[warn] {}/{} negative anchors clean under the discovery floor — the floor regressed.",
            similar.neg_clean, similar.neg_n
        );
        return Ok(false);
    }
    Ok(true)
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

/// Score the discovery labels — **two passes over two surfaces** (GH #150).
///
/// The ranks and the suppression tally are scored against the **shipped surface**
/// (the discovery floor as `Vault::open` configures it): positive anchors score
/// the 1-based rank of the first `expected` note among the top `SIM_K` floored
/// candidates, and a clean negative anchor is one the floor suppressed to zero.
/// The cosine piles and per-anchor detail come from a second, **unfloored** pass
/// (`Vault::similar_raw`): they are the floor's own calibration data, and letting
/// the floor filter them would blind every future recalibration to exactly the
/// distribution it needs (the piles' meaning is unchanged from pre-floor rows;
/// the negatives metric is the one that now measures the floor instead of raw
/// truncation).
fn score_similar(
    vault: &Vault,
    set: &SimilarSet,
) -> Result<SimilarPass, Box<dyn std::error::Error>> {
    let mut pass = SimilarPass::default();
    // Pass 1 — the shipped, floored surface: ranks + suppression.
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
        }
    }
    // Pass 2 — the raw, unfloored surface: the calibration piles + detail.
    for label in &set.anchors {
        let candidates = vault.similar_raw(&label.anchor, SIM_K)?;
        let negative = label.expected.is_empty();
        // The best-passage arm of the ordering A/B (see `SimilarPass::mate_raw`),
        // and the demoted-vs-gone split against pass 1's floored ranks.
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

/// Dump every candidate's **stage-1 z**, ungated, on every discovery anchor —
/// the third discovery pass (GH #187), and the only one that reads the floor's
/// own unit.
///
/// `similar_raw` cannot serve this: `floor: None` skips the statistics
/// altogether, so an unfloored candidate's `z` is `None`. What produces a
/// complete reading is a floor whose bars are `-∞` — every z computed, nothing
/// cut, the list still in z order — so the dump is the shipped code path with
/// its two constants moved out of the way, not a reimplementation of it that
/// could drift from what ships.
///
/// The limit is [`Z_SCAN_LIMIT`] rather than `SIM_K` for the same reason: a bar
/// is calibrated against the population it has to cut, and the strangers just
/// under a served list are precisely the ones a lower bar would admit.
///
/// The vault's floor is restored before returning, so this instrument leaves no
/// trace on the passes around it (the sweep re-scores `similar` after).
fn score_floor_z(
    vault: &mut Vault,
    set: &SimilarSet,
) -> Result<FloorZ, Box<dyn std::error::Error>> {
    vault.set_discovery_floor(Some(UNGATED_FLOOR));
    let dump = collect_floor_z(vault, set);
    vault.set_discovery_floor(Some(DiscoveryFloor::default()));
    // …and once more on the restored, shipped floor at the same depth: what the
    // engine really serves, which is what the replay behind every window and
    // trade-curve row is checked against (`FloorZ::replay_faults`).
    let mut dump = dump?;
    for anchor in &mut dump.anchors {
        anchor.served = vault
            .similar(&anchor.anchor, Z_SCAN_LIMIT)?
            .into_iter()
            .map(|c| c.path)
            .collect();
    }
    Ok(dump)
}

/// [`score_floor_z`]'s body, split out so the floor is restored even when a
/// query fails.
fn collect_floor_z(vault: &Vault, set: &SimilarSet) -> Result<FloorZ, Box<dyn std::error::Error>> {
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
        dump.anchors.push(AnchorZ {
            anchor: label.anchor.clone(),
            negative: label.expected.is_empty(),
            candidates: candidates
                .iter()
                .filter_map(|c| {
                    let mate = label.expected.iter().any(|e| paths_match(&c.path, e));
                    c.z.map(|z| (c.path.clone(), z, mate))
                })
                .collect(),
            served: Vec::new(), // filled on the shipped-floor pass, in score_floor_z
        });
    }
    Ok(dump)
}

/// A `similar` score is negated L2 distance between L2-normalized vectors
/// (the real embedder normalizes every row), so it converts exactly:
/// `cos = 1 − d²/2`. Cosine is the unit the floor ruling is stated in and the
/// unit that survives a model swap comparison, so the piles are recorded in it.
fn cosine_of(score: f64) -> f64 {
    1.0 - (score * score) / 2.0
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
    println!(
        "  discovery  hit@1={:.2}  hit@3={:.2}  MRR@{SIM_K}={:.3}  (any mate — saturates)",
        similar.rank.hit1(),
        similar.rank.hit3(),
        similar.rank.mrr()
    );
    // The non-saturating companion (GH #183): every labelled mate scored on its
    // own, so a hard mate demoted behind an easy one is visible. This is the row
    // to read when judging a *ranking* change; the one above can only go down.
    println!(
        "  per-mate   hit@1={:.2}  hit@3={:.2}  MRR@{SIM_K}={:.3}  (n={} mates, ungated)",
        similar.mate.hit1(),
        similar.mate.hit3(),
        similar.mate.mrr(),
        similar.mate.n
    );
    // The ordering A/B: shipped centroid-z order vs. the unfloored best-passage
    // order. A non-zero ΔMRR is the proof the per-mate metric is not inert to the
    // sort key — the property GH #183 existed to obtain.
    println!(
        "   └ best-passage order  hit@1={:.2}  hit@3={:.2}  MRR@{SIM_K}={:.3}   ΔMRR(centroid−passage)={:+.3}",
        similar.mate_raw.hit1(),
        similar.mate_raw.hit3(),
        similar.mate_raw.mrr(),
        similar.mate.mrr() - similar.mate_raw.mrr()
    );
    if similar.mate_suppressed > 0 {
        println!(
            "   └ {} of {} labelled mates are SUPPRESSED on the shipped surface (reachable unfloored,\n\
             \x20    served never) — the floor dropping a human-labelled relation, not merely demoting it",
            similar.mate_suppressed, similar.mate.n
        );
    }
    if similar.neg_n > 0 {
        // Suppression: a clean negative anchor surfaced zero candidates. Red by
        // design until the quality floor lands (index-engine.md §3, PR #145).
        println!(
            "  negatives  {}/{} anchors clean under the floor — {} cards surfaced where the label says \"nothing\"",
            similar.neg_clean, similar.neg_n, similar.neg_cards
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
    // The same question one stage earlier and in the floor's own unit, where an
    // answer is actionable: the piles are stage-2 cosines, and no constant in
    // the code is ever compared against one.
    print_floor_windows(floor_z);
}

/// Re-derive both [`DiscoveryFloor`] windows on the corpus as it stands today,
/// and price what the member bar costs where no window exists (GH #187).
///
/// Printed every run, and recorded in the row, precisely so no number here ever
/// gets copied into a doc comment again: the #150 windows were measured once,
/// frozen into `DiscoveryFloor`'s rustdoc, and silently falsified the first time
/// the corpus grew a note shape they were never derived from. The instrument is
/// the citable thing; the constants stay in the code.
fn print_floor_windows(z: &FloorZ) {
    let shipped = DiscoveryFloor::default();
    let (mates, strangers) = (z.mate_z(), z.stranger_z());
    let (neg_leaders, pos_leaders) = (z.neg_leader_z(), z.pos_leader_z());
    println!(
        "  floor calibration (stage-1 z, ungated — the unit DiscoveryFloor judges in, GH #187)"
    );
    // Instrument check before the readings, the same posture as the dense
    // ablation's: every line below replays the floor's rule over the dump, so
    // if the replay and the engine disagree the readings are fiction.
    match z.replay_faults().as_slice() {
        [] => println!("    [check] replayed floor matches the engine on every anchor"),
        faults => println!(
            "    [FAULT] replayed floor disagrees with the engine on {} anchor(s) ({}) — \
             DiscoveryFloor's rule moved; distrust every window and trade row below",
            faults.len(),
            faults.join(", ")
        ),
    }
    for (label, pile, role) in [
        ("mates", &mates, "member_z must KEEP"),
        (
            "strangers",
            &strangers,
            "member_z must CUT (positive anchors)",
        ),
        ("neg leaders", &neg_leaders, "leader_z must CUT"),
        ("pos leaders", &pos_leaders, "leader_z must KEEP"),
    ] {
        match pile_stats(pile) {
            Some((min, med, max)) => println!(
                "    {label:<12} n={:<4} min/med/max {min:+.3}/{med:+.3}/{max:+.3}   ← {role}",
                pile.len()
            ),
            None => println!("    {label:<12} n=0    (nothing labelled — no reading)"),
        }
    }
    for (name, shipped_at, win) in [
        (
            "leader",
            shipped.leader_z,
            Window::read(&neg_leaders, &pos_leaders),
        ),
        ("member", shipped.member_z, Window::read(&strangers, &mates)),
    ] {
        match win {
            None => println!("    {name} window  no reading (a population was empty)"),
            Some(w) if w.open() => println!(
                "    {name} window  ({:+.3}, {:+.3}]  — shipped {name}_z={shipped_at:.2} {}",
                w.cut_max,
                w.keep_min,
                if shipped_at > w.cut_max && shipped_at <= w.keep_min {
                    "sits inside"
                } else {
                    "sits OUTSIDE its own measured window"
                }
            ),
            Some(w) => println!(
                "    {name} window  EMPTY — the population it must cut reaches {:+.3} while the one \n\
                 \x20                 it must keep starts at {:+.3}; the two INVERT, and no constant \n\
                 \x20                 separates an inversion (shipped {name}_z={shipped_at:.2})",
                w.cut_max, w.keep_min
            ),
        }
    }
    // What the shipped bar actually serves, and what each rescue would cost —
    // the trade the issue refused to guess at, replayed over the dump rather
    // than argued from the aggregates.
    let (served, labelled, admitted, dirty) = z.serve(&shipped);
    println!(
        "    shipped floor serves {served}/{labelled} labelled mates, {admitted} strangers on \
         positive anchors, {dirty} dirty negatives"
    );
    let mut rescues: Vec<f64> = z
        .anchors
        .iter()
        .filter(|a| !a.negative)
        .flat_map(|a| a.mates())
        .filter(|&m| m < shipped.member_z)
        .collect();
    rescues.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    rescues.dedup();
    if !rescues.is_empty() {
        println!(
            "    member_z trade curve (leader_z held at {:.2}) — every row a suppressed mate's own z:",
            shipped.leader_z
        );
        for bar in rescues.iter().rev() {
            let floor = DiscoveryFloor {
                member_z: *bar,
                ..shipped.clone()
            };
            let (m, n, s, d) = z.serve(&floor);
            println!("      member_z={bar:+.3}  mates {m}/{n}  strangers {s}  dirty negatives {d}");
        }
    }
    if !z.ungraded.is_empty() {
        println!(
            "    [warn] no z statistics for {} anchor(s) ({}) — pool under FLOOR_MIN_POPULATION or \
             zero variance; the windows above are measured without them",
            z.ungraded.len(),
            z.ungraded.join(", ")
        );
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
        // stage-1 z, the three populations the two constants answer to, and both
        // re-derived windows (GH #187). Same convention as every key above: new,
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
            let shipped = DiscoveryFloor::default();
            let (served, labelled, admitted, dirty) = z.serve(&shipped);
            serde_json::json!({
                "piles": {
                    "mates": round(&mates),
                    "strangers_on_positives": round(&strangers),
                    "negative_leaders": round(&neg_leaders),
                    "positive_leaders": round(&pos_leaders),
                },
                "window_leader": win(&neg_leaders, &pos_leaders),
                "window_member": win(&strangers, &mates),
                "shipped": {
                    "leader_z": shipped.leader_z,
                    "member_z": shipped.member_z,
                    "mates_served": served,
                    "mates_labelled": labelled,
                    "strangers_served": admitted,
                    "negatives_dirty": dirty,
                },
                "ungraded_anchors": z.ungraded,
                // Empty is the only trustworthy reading — see FloorZ::replay_faults.
                "replay_faults": z.replay_faults(),
                // Per-anchor and per-candidate, because a window edge is only
                // arguable once you can name the pair that set it.
                "detail": z.anchors.iter().map(|a| serde_json::json!({
                    "anchor": a.anchor,
                    "negative": a.negative,
                    "candidates": a.candidates.iter().map(|(path, z, mate)| serde_json::json!({
                        "path": path,
                        "z": (z * 1e4).round() / 1e4,
                        "mate": mate,
                    })).collect::<Vec<_>>(),
                })).collect::<Vec<_>>(),
            })
        }),
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
