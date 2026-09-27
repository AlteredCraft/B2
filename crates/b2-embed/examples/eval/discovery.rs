//! Discovery (`similar`) scoring on the orthogonal corpus: per-mate ranks, strangers and
//! the cosine piles (GH #183/#188), and the z calibration dump whose windows are re-derived
//! every run (GH #187; gating nothing since GH #197).

use crate::common::{cosine_of, passage_z, pile_stats, Band};
use crate::labels::SimilarSet;
use crate::metrics::{paths_match, Agg, Window};
use crate::{SIM_K, Z_SCAN_LIMIT};
use b2_core::vault::Vault;

/// A full pass over the discovery labels: rank aggregates, the negative-anchor tally, and
/// the cosine calibration piles (index-engine.md §3, PR #145).
#[derive(Default)]
pub struct SimilarPass {
    /// Rank of the first `expected` hit per positive anchor.
    pub rank: Agg,
    /// Per-mate ranks, one per `(anchor, expected mate)` pair (GH #183). [`Self::rank`]
    /// stops at the easiest mate and saturates; this moves when any mate slides. Gated at
    /// [`FLOOR_MATE_MRR`](crate::gate::FLOOR_MATE_MRR) (GH #188).
    pub mate: Agg,
    /// Strangers: `(anchor, path)` for unlabelled notes served on a positive anchor within
    /// `SIM_K`, discovery's precision side (GH #187/#188). Reported, not gated: the cheapest
    /// way to shrink it is to label the stranger, and labels aren't exhaustive.
    pub strangers: Vec<(String, String)>,
    /// Positive anchors serving at least one stranger: the spread behind the count.
    pub stranger_anchors: usize,
    pub neg_n: usize,
    /// Negative anchors that surfaced zero candidates. Reads 0 under always-serve (GH #197);
    /// kept for row comparability, not asserted.
    pub neg_clean: usize,
    /// Candidates surfaced across all negative anchors.
    pub neg_cards: usize,
    /// Cosines of surfaced candidates a human labelled genuinely related.
    pub related: Vec<f64>,
    /// Cosines of everything else surfaced.
    pub junk: Vec<f64>,
    /// Every anchor's surfaced list in rank order, for relative drop-off readings and for
    /// naming the pair behind a pile value.
    pub detail: Vec<AnchorDetail>,
}

/// One anchor's surfaced candidates, in rank order, for the results log.
pub struct AnchorDetail {
    pub anchor: String,
    pub negative: bool,
    /// (candidate path, cosine, human-labelled related) per surfaced candidate.
    pub candidates: Vec<(String, f64, bool)>,
}

/// One row of the z dump: a candidate's z with its human label.
pub struct ZCand {
    pub path: String,
    /// The engine's stage-2 best-passage z (GH #192): the strength band's input.
    pub z: f64,
    /// The same z recomputed from the served scores; a drift from `z` means the engine's
    /// statistic moved. `None` at zero variance.
    pub z_recheck: Option<f64>,
    /// The stage-2 score as served (negated best chunk-pair L2), for the cosine.
    pub score: f64,
    pub mate: bool,
}

/// One anchor's complete reading in z order: every candidate note, labelled (GH #187).
/// The anchor-relative unit a z existence bar would judge in, unlike the cosine piles.
pub struct AnchorZ {
    pub anchor: String,
    pub negative: bool,
    /// Every candidate in served order, which is descending z.
    pub candidates: Vec<ZCand>,
}

impl AnchorZ {
    /// The top candidate's z, what a leader gate would read.
    pub fn leader(&self) -> Option<f64> {
        self.candidates.first().map(|c| c.z)
    }
    pub fn mates(&self) -> impl Iterator<Item = f64> + '_ {
        self.candidates.iter().filter(|c| c.mate).map(|c| c.z)
    }
    /// Unlabelled candidates: on a positive anchor, what a member bar would have to cut.
    pub fn strangers(&self) -> impl Iterator<Item = f64> + '_ {
        self.candidates.iter().filter(|c| !c.mate).map(|c| c.z)
    }
    /// The worst [`ZCand::z_recheck`] disagreement across this anchor's candidates.
    pub fn recheck_delta(&self) -> f64 {
        self.candidates
            .iter()
            .filter_map(|c| c.z_recheck.map(|r| (c.z - r).abs()))
            .fold(0.0, f64::max)
    }
}

/// The z dump across every discovery anchor (GH #187; gates nothing, ADR-0014).
///
/// A leader gate is calibrated by negative- against positive-anchor leaders; a member bar by
/// strangers against mates. Keep the populations separate: conflating them made a bad
/// member bar look caught when it was not.
#[derive(Default)]
pub struct FloorZ {
    /// Every anchor with computed statistics, in label order.
    pub anchors: Vec<AnchorZ>,
    /// Anchors with no z (pool under `STATS_MIN_POPULATION`, or zero variance), named so the
    /// windows aren't read as covering them.
    pub ungraded: Vec<String>,
}

impl FloorZ {
    /// Labelled mates' z's: what a member bar would have to keep.
    pub fn mate_z(&self) -> Vec<f64> {
        self.anchors.iter().flat_map(|a| a.mates()).collect()
    }
    /// Strangers on positive anchors: what a member bar would have to cut. Negative anchors
    /// are excluded; they would make the member window look wider than it is.
    pub fn stranger_z(&self) -> Vec<f64> {
        self.anchors
            .iter()
            .filter(|a| !a.negative)
            .flat_map(|a| a.strangers())
            .collect()
    }
    /// Negative anchors' leaders: what a leader gate would have to cut.
    pub fn neg_leader_z(&self) -> Vec<f64> {
        self.anchors
            .iter()
            .filter(|a| a.negative)
            .filter_map(|a| a.leader())
            .collect()
    }
    /// Positive anchors' leaders: what a leader gate would have to keep.
    pub fn pos_leader_z(&self) -> Vec<f64> {
        self.anchors
            .iter()
            .filter(|a| !a.negative)
            .filter_map(|a| a.leader())
            .collect()
    }

    /// The worst [`ZCand::z_recheck`] disagreement across every anchor. f32 round-trips add
    /// noise, so the printed check tolerates 1e-3.
    pub fn recheck_delta(&self) -> f64 {
        self.anchors
            .iter()
            .map(|a| a.recheck_delta())
            .fold(0.0, f64::max)
    }
}

/// Score the discovery labels in one pass over `Vault::similar`, the always-served list
/// (GH #197).
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
            // Per labelled mate, so a hard one can't hide behind an easy one (GH #183).
            for expected in &label.expected {
                let mate_rank = candidates
                    .iter()
                    .position(|c| paths_match(&c.path, expected))
                    .map(|p| p + 1);
                pass.mate.add(mate_rank);
            }
            // Strangers (GH #188), at the ranks' own depth.
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

/// Dump every candidate's z on every discovery anchor (GH #187, #192): `similar` read at
/// [`Z_SCAN_LIMIT`], since the strangers just past a served prefix are what a lower bar
/// would admit.
pub fn score_floor_z(
    vault: &Vault,
    set: &SimilarSet,
) -> Result<FloorZ, Box<dyn std::error::Error>> {
    let mut dump = FloorZ::default();
    for label in &set.anchors {
        let candidates = vault.similar(&label.anchor, Z_SCAN_LIMIT)?;
        // `discover::candidates` gives every candidate a z or none, so the leader decides.
        let Some(true) = candidates.first().map(|c| c.z.is_some()) else {
            dump.ungraded.push(label.anchor.clone());
            continue;
        };
        // Score is negated L2, so d² = score². Should match the engine's z to fp noise.
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

/// Re-derive the windows a z existence rule would have on today's corpus (GH #187), the
/// record of why none ships (GH #197). Cite this output; don't copy its numbers into docs.
pub fn print_floor_windows(z: &FloorZ) {
    let (mates, strangers) = (z.mate_z(), z.stranger_z());
    let (neg_leaders, pos_leaders) = (z.neg_leader_z(), z.pos_leader_z());
    println!(
        "  discovery z calibration (stage-2 best-passage z — the band's input; gates NOTHING \
         since GH #197)"
    );
    // Tolerance covers f32 sqrt/square round-trip noise.
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
    // Negative anchors' leaders are served under always-serve; the band is what they claim.
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
