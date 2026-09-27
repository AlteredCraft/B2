//! The exit gate: the assertions `make eval` exits 2 on, read off the default config's
//! passes (docs/evals.md, "The exit gate"). A manual quality gate, never a CI test.
//!
//! A rank floor sits below its reading (corpus-drift headroom; run noise is zero). The
//! search-evidence rows sit at zero, since headroom there would permit serving a nonsense
//! query or cutting a real one.

use crate::dense::DensePass;
use crate::discovery::SimilarPass;
use crate::evidence::{read_shipped_bar, SearchEvidence};
use crate::retrieval::Pass;
use crate::SIM_K;

/// The soft reference floor on the default config's hybrid note hit@1.
pub const FLOOR_HIT1: f64 = 0.75;

/// The floor on per-mate discovery MRR@[`SIM_K`] (GH #188, ADR-0014); reading 0.650.
///
/// Below the reading so a legitimate corpus edit doesn't tempt a relabel (process rule 2).
/// Runs are bit-reproducible, so the headroom is for corpus drift: ~2 mates lost from rank
/// 1 at n = 15.
pub const FLOOR_MATE_MRR: f64 = 0.52;

/// The floor on the dense fixture's per-mate MRR@[`SIM_K`] (ADR-0014); reading 0.467, sized
/// like [`FLOOR_MATE_MRR`] (~2 losses at n = 14). Where everything relates, relabelling
/// toward the model always looks plausible: a red reading argues about the notes.
pub const FLOOR_DENSE_MATE_MRR: f64 = 0.32;

/// How many labelled negative queries the shipped evidence bar may serve (ADR-0015,
/// GH #202). Zero: this is the defect the bar exists to fix, so no headroom.
pub const MAX_NEGATIVES_SERVED: usize = 0;

/// How many labelled relevant queries the bar may cut (ADR-0015, GH #202, #208). A cut
/// positive costs the user the answer. A nonzero reading means the rule is wrong, not the
/// constant.
pub const MAX_POSITIVES_CUT: usize = 0;

/// How many of the dense fixture's title-as-query probes the bar may cut (GH #202). A note's
/// own title names a note the vault holds, and topical concentration is only expressible in
/// this fixture. Titles carry no labels, so nothing here can be relabelled to clear it.
pub const MAX_DENSE_TITLES_CUT: usize = 0;

/// Whether the default config clears every exit-gate row; warns naming the first failure.
pub fn passes(
    hybrid: &Pass,
    similar: &SimilarPass,
    dense: &DensePass,
    evidence: &SearchEvidence,
    model_id: &str,
) -> bool {
    if hybrid.note.hit1() < FLOOR_HIT1 {
        eprintln!(
            "\n[warn] hybrid hit@1 {:.2} is below the {FLOOR_HIT1} reference floor — inspect the misses above.",
            hybrid.note.hit1()
        );
        return false;
    }
    // No suppression assertion on negative anchors: under always-serve (ADR-0014) a loner
    // serves its nearest by ruling. A returning existence gate is caught by the dense
    // empty-pane check and the per-mate floors (GH #217).
    if similar.mate.mrr() < FLOOR_MATE_MRR {
        eprintln!(
            "\n[warn] per-mate MRR@{SIM_K} {:.3} is below the {FLOOR_MATE_MRR:.2} floor — discovery ranking regressed \
             (read the per-mate line's mates, not the aggregate; and do NOT relabel to clear this).",
            similar.mate.mrr()
        );
        return false;
    }
    // Every note in a corpus where everything relates must serve candidates (GH #196/#197).
    if !dense.empty_panes.is_empty() {
        eprintln!(
            "\n[warn] {} of {} dense-fixture notes serve an EMPTY pane ({}) — an existence \
             gate is refusing a vault whose every note genuinely relates (GH #196).",
            dense.empty_panes.len(),
            dense.notes,
            dense.empty_panes.join(", ")
        );
        return false;
    }
    if dense.mate.mrr() < FLOOR_DENSE_MATE_MRR {
        eprintln!(
            "\n[warn] dense per-mate MRR@{SIM_K} {:.3} is below the {FLOOR_DENSE_MATE_MRR:.2} floor — \
             single-domain discovery ranking regressed (argue with the notes, not the labels).",
            dense.mate.mrr()
        );
        return false;
    }
    // The search-evidence rows (ADR-0015, GH #202). Search's bar moves no discovery rank, so
    // movement above is a bug. Skipped when the model has no calibrated bar (ADR-0007).
    match read_shipped_bar(evidence, model_id) {
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
                return false;
            }
            if reading.pos_cut > MAX_POSITIVES_CUT {
                eprintln!(
                    "\n[warn] the shipped evidence bar CUTS {} of {} labelled relevant queries where D2 \
                     permits {MAX_POSITIVES_CUT} — a note the vault holds is unreachable for a query \
                     naming it. Change the RULE, not the constant (the df ceiling died exactly here).",
                    reading.pos_cut,
                    evidence.positives.len()
                );
                return false;
            }
        }
    }
    // The same two directions on the dense fixture: a different geometry, not a different
    // threshold.
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
        return false;
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
        return false;
    }
    true
}
