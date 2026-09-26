//! Discovery (`similar`) scoring on the orthogonal corpus: per-mate ranks, strangers and
//! the cosine piles (GH #183/#188), and the z calibration dump whose windows are re-derived
//! every run (GH #187; gating nothing since GH #197).

use crate::common::{cosine_of, passage_z, pile_stats, Band};
use crate::labels::SimilarSet;
use crate::metrics::{paths_match, Agg, Window};
use crate::{SIM_K, Z_SCAN_LIMIT};
use b2_core::vault::Vault;

/// A full pass over the discovery labels: rank aggregates over the positive
/// anchors, the suppression tally over the negative ones, and every surfaced
/// candidate's cosine sorted into the two calibration piles the quality floor
/// is read from (index-engine.md §3's ruling, PR #145).
#[derive(Default)]
pub struct SimilarPass {
    /// Rank of the first `expected` hit per positive anchor — the pre-existing
    /// hit@1 / hit@3 / MRR discovery metrics, unchanged.
    pub rank: Agg,
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
    /// **In the exit gate** since GH #188, at [`FLOOR_MATE_MRR`](crate::gate::FLOOR_MATE_MRR). It shipped reporting-only
    /// for one issue's worth of time on the measure-then-calibrate precedent, and that is
    /// where the baseline came from.
    ///
    /// (A pass-vs-pass suppression diff — `mate_raw` / `mate_suppressed`, asserted at zero —
    /// stood beside this until GH #217: once ADR-0014 retired the existence gate, both
    /// passes read the *identical* `similar` call, so the diff compared a call against
    /// itself and could never fire. An existence gate returning to the path is what the
    /// dense fixture's zero-empty-panes assertion and both per-mate MRR floors catch —
    /// against ground truth, not against a second read of the same surface.)
    pub mate: Agg,
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
    pub strangers: Vec<(String, String)>,
    /// Positive anchors serving at least one stranger — the spread behind
    /// [`Self::strangers`], since one anchor with a long tail and five anchors
    /// with one card each are different failures at the same count.
    pub stranger_anchors: usize,
    /// Negative anchors asked.
    pub neg_n: usize,
    /// Negative anchors that surfaced zero candidates. Under always-serve
    /// (GH #197) an anchor with scorable candidates always serves, so this
    /// reads 0 on this corpus — recorded for row comparability (the key's
    /// meaning, "anchors serving nothing", is unchanged), no longer asserted:
    /// the labels still say "nothing relates", and what the served cards
    /// *claim* is measured by their bands (A2's readout, calibration block).
    pub neg_clean: usize,
    /// Candidates surfaced across all negative anchors — cards whose labelled
    /// answer was "nothing".
    pub neg_cards: usize,
    /// Cosines of surfaced candidates a human labelled genuinely related.
    pub related: Vec<f64>,
    /// Cosines of everything else surfaced — non-expected candidates of positive
    /// anchors, and every candidate of a negative anchor.
    pub junk: Vec<f64>,
    /// Every anchor's surfaced list in rank order. The flat piles above judge the
    /// absolute floor; the *relative* drop-off cutoff is judged within one
    /// anchor's list (how far did #2 fall from #1?), and naming the pair behind a
    /// pile value needs the anchor too — so the order is recorded, not just the
    /// distribution.
    pub detail: Vec<AnchorDetail>,
}

/// One anchor's surfaced candidates, in rank order, for the results log.
pub struct AnchorDetail {
    pub anchor: String,
    /// True for a negative anchor (empty `expected`).
    pub negative: bool,
    /// (candidate path, cosine, human-labelled related) per surfaced candidate.
    pub candidates: Vec<(String, f64, bool)>,
}

/// One row of the z dump: a candidate in the band's unit, with the human label
/// attached.
pub struct ZCand {
    pub path: String,
    /// The stage-2 best-passage z (GH #192's unit; the strength band's input,
    /// gating nothing since GH #197) — straight from the engine's own
    /// statistics.
    pub z: f64,
    /// The same z recomputed harness-side from the served scores (z over squared
    /// best-pair distance, nearer = higher). An instrument check, not a reading:
    /// if this drifts from `z`, the engine's statistic moved and the harness's
    /// model of it is stale. `None` only when the population had zero variance
    /// and no z exists.
    pub z_recheck: Option<f64>,
    /// The stage-2 score as served (negated best chunk-pair L2) — kept so the
    /// row also records the model-comparable cosine, not only the
    /// anchor-relative z.
    pub score: f64,
    /// Labelled a mate of this anchor.
    pub mate: bool,
}

/// One anchor's **complete** reading in the band's unit — every candidate note
/// in the corpus, in z order, with the human label attached (GH #187; stage-2
/// best-passage z since GH #192; the z travels ungated on the one shipped
/// surface since GH #197, so the dump *is* the served list read deep).
///
/// The cosine piles above are the same numbers in a model-comparable unit; this
/// is the anchor-relative unit a z existence bar would judge in, which is why
/// the piles could never re-derive one's constants.
pub struct AnchorZ {
    pub anchor: String,
    /// True for a negative anchor (empty `expected`) — its whole list is
    /// strangers by label, and its leader is what a leader gate would answer to.
    pub negative: bool,
    /// Every candidate in served order, which *is* descending z.
    pub candidates: Vec<ZCand>,
}

impl AnchorZ {
    /// The top candidate's z — what a leader gate would read. `None` only if
    /// the anchor produced no scorable candidate at all.
    pub fn leader(&self) -> Option<f64> {
        self.candidates.first().map(|c| c.z)
    }
    /// This anchor's labelled mates' z's (empty for a negative anchor).
    pub fn mates(&self) -> impl Iterator<Item = f64> + '_ {
        self.candidates.iter().filter(|c| c.mate).map(|c| c.z)
    }
    /// Everything on this anchor's list a human did *not* label — on a positive
    /// anchor, exactly the population a member bar would have to cut.
    pub fn strangers(&self) -> impl Iterator<Item = f64> + '_ {
        self.candidates.iter().filter(|c| !c.mate).map(|c| c.z)
    }
    /// The worst disagreement between the engine's z and the harness's own
    /// recomputation from the served scores, across this anchor's candidates —
    /// the statistic-level drift check ([`ZCand::z_recheck`]).
    pub fn recheck_delta(&self) -> f64 {
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
pub struct FloorZ {
    /// Every anchor with computed statistics, in label order.
    pub anchors: Vec<AnchorZ>,
    /// Anchors whose candidate pool was under `STATS_MIN_POPULATION` or had zero
    /// variance, so no z exists to calibrate from. Named rather than silently
    /// dropped: on a corpus small enough for this to happen, every window below
    /// is measured on fewer anchors than the labels suggest.
    pub ungraded: Vec<String>,
}

impl FloorZ {
    /// (a) Labelled mates' z's — what a member bar would have to keep.
    pub fn mate_z(&self) -> Vec<f64> {
        self.anchors.iter().flat_map(|a| a.mates()).collect()
    }
    /// (b) Strangers on **positive** anchors — what a member bar would have to
    /// cut. The negative anchors' own candidates are deliberately excluded:
    /// they are the leader pair's business, and folding them in here is what
    /// would let a member window look wider than it is.
    pub fn stranger_z(&self) -> Vec<f64> {
        self.anchors
            .iter()
            .filter(|a| !a.negative)
            .flat_map(|a| a.strangers())
            .collect()
    }
    /// (c) Negative anchors' leaders — what a leader gate would have to cut.
    pub fn neg_leader_z(&self) -> Vec<f64> {
        self.anchors
            .iter()
            .filter(|a| a.negative)
            .filter_map(|a| a.leader())
            .collect()
    }
    /// Positive anchors' leaders — what a leader gate would have to keep, or it
    /// empties a list that has a real mate on it.
    pub fn pos_leader_z(&self) -> Vec<f64> {
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
    pub fn recheck_delta(&self) -> f64 {
        self.anchors
            .iter()
            .map(|a| a.recheck_delta())
            .fold(0.0, f64::max)
    }
}

/// Score the discovery labels — **one pass, one surface** (GH #197).
///
/// Everything reads `Vault::similar`, the always-served ranked list: the ranks,
/// the strangers, the negatives' card tally, and the cosine piles + per-anchor
/// detail the calibration blocks consume. (A second pass with a pass-vs-pass
/// suppression diff existed while the GH #150 existence gate shipped; under
/// always-serve it re-read the identical call and could never fire, so it
/// retired — GH #217, and see [`SimilarPass::mate`] for what catches a
/// returning gate instead.)
pub fn score_similar(
    vault: &Vault,
    set: &SimilarSet,
) -> Result<SimilarPass, Box<dyn std::error::Error>> {
    let mut pass = SimilarPass::default();
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
        // The calibration piles + per-anchor detail, off the same served list.
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

/// Dump every candidate's **band-unit z** (stage-2 best-passage z, GH #192) on
/// every discovery anchor — the deep discovery pass (GH #187).
///
/// Since GH #197 the z travels ungated on the one shipped surface, so the dump
/// is simply `similar` read at [`Z_SCAN_LIMIT`] rather than `SIM_K`: a bar is
/// calibrated against the population it has to cut, and the strangers just
/// under a served prefix are precisely the ones a lower bar would admit. (This
/// used to need the engine's floor moved out of the way with `-∞` bars; the
/// machinery went with the gate.)
pub fn score_floor_z(
    vault: &Vault,
    set: &SimilarSet,
) -> Result<FloorZ, Box<dyn std::error::Error>> {
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

/// Re-derive the admissible windows a z existence rule *would* have on the
/// corpus as it stands today (GH #187) — the standing record of why none ships
/// (GH #197), and the first reading any Phase-2 bake-off candidate answers to.
///
/// Printed every run, and recorded in the row, precisely so no number here ever
/// gets copied into a doc comment again: the #150 windows were measured once,
/// frozen into a rustdoc, and silently falsified the first time the corpus grew
/// a note shape they were never derived from. The instrument is the citable
/// thing; any constant stays in the code it governs.
pub fn print_floor_windows(z: &FloorZ) {
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
                Band::of(c.z).glyph()
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
