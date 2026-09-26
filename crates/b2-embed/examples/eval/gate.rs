//! The exit gate: the assertions `make eval` exits 2 on, read off the **default** config's
//! passes (docs/evals.md, "The exit gate"). A manual quality gate, never a CI test.
//!
//! How each row is placed is the point, and the constants' own docs say why: a rank floor
//! sits *below* its reading (corpus-drift headroom, run noise being zero), while the
//! search-evidence rows sit *at* their structural zeros, because headroom there would read
//! as permission to serve a nonsense query or cut a real one.

use crate::dense::DensePass;
use crate::discovery::SimilarPass;
use crate::evidence::{read_shipped_bar, SearchEvidence};
use crate::retrieval::Pass;
use crate::SIM_K;

/// The soft reference floor on the default config's hybrid note hit@1.
/// Untouched by the GH #197 re-derivation: retrieval never had a gate to
/// retire.
pub const FLOOR_HIT1: f64 = 0.75;

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
pub const FLOOR_MATE_MRR: f64 = 0.52;

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
pub const FLOOR_DENSE_MATE_MRR: f64 = 0.32;

/// How many **labelled negative queries** the shipped evidence bar may still serve
/// (ADR-0015, GH #202). Zero: this is the defect the bar exists to fix.
///
/// A floor at its measured value rather than below it, which is the deliberate exception to
/// the house sizing method — the "headroom" a floor normally carries would be *permission to
/// serve a nonsense query*. A new negative the bar serves is either a real regression or a
/// mislabelled query; both want a red reading.
pub const MAX_NEGATIVES_SERVED: usize = 0;

/// How many **labelled relevant queries** the bar may cut (GH #202) — the search-side
/// tripwire ADR-0015 asserts at zero with no headroom, and the direction that costs a user
/// something real: a served nonsense row costs a little trust, a cut positive costs the
/// answer. Its precondition was met by GH #208, which labelled the date-shaped query pile.
/// The reading is 0 of 44. A nonzero value is never a calibration nudge: it means the *rule*
/// is wrong for a shape the corpus now carries.
pub const MAX_POSITIVES_CUT: usize = 0;

/// How many of the **dense fixture's title-as-query probes** the bar may cut (GH #202).
/// Zero, and this is the assertion that would have caught the losing rule: a note's own
/// title names a note the vault demonstrably holds, so cutting one is indefensible whatever
/// a labelled corpus says. A third row rather than headroom on [`MAX_POSITIVES_CUT`] because
/// it gates a different *geometry*: the labelled corpus minimizes shared vocabulary by
/// construction, so topical concentration is only expressible here. Titles need no labels,
/// so nothing in this reading can be relabelled to clear it.
pub const MAX_DENSE_TITLES_CUT: usize = 0;

/// Whether the default config clears every exit-gate row, printing a `[warn]` naming the
/// first row that fails (so a red run says which reading to argue with).
pub fn passes(
    hybrid: &Pass,
    similar: &SimilarPass,
    dense: &DensePass,
    evidence: &SearchEvidence,
    model_id: &str,
) -> bool {
    // The soft floors, on the DEFAULT config's passes — so this can double as
    // a manual quality gate. Not a CI test.
    if hybrid.note.hit1() < FLOOR_HIT1 {
        eprintln!(
            "\n[warn] hybrid hit@1 {:.2} is below the {FLOOR_HIT1} reference floor — inspect the misses above.",
            hybrid.note.hit1()
        );
        return false;
    }
    // The negatives' suppression assertion RETIRED with the gate it watched (ADR-0014):
    // under always-serve a loner anchor serves its ranked nearest — that is the ruling, not
    // a regression. The anchors stay labelled, the strangers instrument keeps counting, and
    // what the served cards *claim* is the calibration block's band readout. (The
    // pass-vs-pass suppression assertion retired later, for a different reason: it compared
    // a call against itself and could never fire — GH #217. A returning existence gate is
    // the dense zero-empty-panes assertion's and the per-mate floors' to catch.)
    //
    // Discovery **rank** (GH #188). The rank floor sits below its measured reading (corpus
    // drift headroom, run noise being zero).
    if similar.mate.mrr() < FLOOR_MATE_MRR {
        eprintln!(
            "\n[warn] per-mate MRR@{SIM_K} {:.3} is below the {FLOOR_MATE_MRR:.2} floor — discovery ranking regressed \
             (read the per-mate line's mates, not the aggregate; and do NOT relabel to clear this).",
            similar.mate.mrr()
        );
        return false;
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
        return false;
    }
    // …and its rank floor, the dense sibling of FLOOR_MATE_MRR.
    if dense.mate.mrr() < FLOOR_DENSE_MATE_MRR {
        eprintln!(
            "\n[warn] dense per-mate MRR@{SIM_K} {:.3} is below the {FLOOR_DENSE_MATE_MRR:.2} floor — \
             single-domain discovery ranking regressed (argue with the notes, not the labels).",
            dense.mate.mrr()
        );
        return false;
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
                return false;
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
