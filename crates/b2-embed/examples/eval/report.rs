//! The default config's printed report (the per-query table, aggregates, discovery lines),
//! the paired per-query A/B diff, and the orthogonal corpus's JSONL row.

use crate::common::{cosine_of, pile_stats, truncate};
use crate::discovery::{print_floor_windows, FloorZ, SimilarPass};
use crate::evidence::{bake_off, best_cos_pile, QueryEvidence, SearchEvidence};
use crate::fold::{fold_json, FoldBench};
use crate::gate::FLOOR_MATE_MRR;
use crate::labels::Labelled;
use crate::metrics::{rank_str, Agg, Window};
use crate::retrieval::Pass;
use crate::tail::{tail_json, TailBench};
use crate::{K, SIM_K};
use b2_core::chunk::ChunkConfig;
use b2_core::search::EvidenceBar;
use b2_core::vault::{chunk_candidate_pool, note_candidate_pool};

pub fn print_default_report(
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

/// The paired per-query diff between two fused passes — what an A/B is actually
/// judged on. At this corpus's n, every aggregate delta is worth 1–2 queries, so
/// "hit@1 +0.05" and "these two queries flipped, this one broke" are the same
/// fact — but only the second form can be argued with, per-query, against the
/// labels (docs/evals.md, the process rules). Prints nothing but a
/// no-moves line when the variant reproduced the reference ranking exactly —
/// which, per the same rules, is itself a claim to verify against a
/// continuous quantity (the piles), never bare proof of "no effect".
pub fn print_rank_moves(queries: &[Labelled], reference: &Pass, variant: &Pass) {
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

/// Who scored a run and in which embedding space — the identity every row the run
/// appends carries (the orthogonal corpus's rows and the dense fixture's alike).
#[derive(Clone, Copy)]
pub struct RunId<'a> {
    /// The repo's short commit, `None` outside a git checkout.
    pub git: Option<&'a str>,
    pub model: &'a str,
    pub dim: usize,
}

/// One scored configuration's inputs to [`result_row`]: a short-lived, read-only view
/// over passes `run` (or an A/B) owns, assembled at the call and consumed there.
#[derive(Clone, Copy)]
pub struct RowInputs<'a> {
    /// The config's name in the row (`"default"`, `"unicode61"`, a sweep variant).
    pub label: &'a str,
    pub cfg: &'a ChunkConfig,
    /// The `chunks_fts` tokenizer the row was scored under.
    pub tokenizer: &'a str,
    pub notes: usize,
    pub chunks: usize,
    pub embed_secs: f64,
    /// The labelled positives every pass here was scored over.
    pub queries: &'a [Labelled],
    pub bm25: Option<&'a Pass>,
    pub vector: Option<&'a Pass>,
    pub hybrid: &'a Pass,
    pub similar: Option<&'a SimilarPass>,
    /// The calibration blocks, which only the default row re-derives; `None` on the
    /// ablation and sweep rows, whose keys then record `null`.
    pub calibration: Option<Calibration<'a>>,
}

/// The default row's calibration readings — present together or not at all.
#[derive(Clone, Copy)]
pub struct Calibration<'a> {
    pub floor_z: &'a FloorZ,
    pub evidence: &'a SearchEvidence,
    pub fold: &'a FoldBench,
    pub tail: &'a TailBench,
}

/// Seconds since the Unix epoch, for a row's `ts` (0 on a clock before it).
pub fn unix_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// One appendable JSONL row for a scored configuration.
pub fn result_row(run: RunId, row: RowInputs) -> serde_json::Value {
    let RunId { git, model, dim } = run;
    let RowInputs {
        label,
        cfg,
        tokenizer,
        notes,
        chunks,
        embed_secs,
        queries,
        bm25,
        vector,
        hybrid,
        similar,
        calibration,
    } = row;
    let floor_z = calibration.map(|c| c.floor_z);
    let evidence = calibration.map(|c| c.evidence);
    let fold = calibration.map(|c| c.fold);
    let tail = calibration.map(|c| c.tail);
    let ts = unix_secs();
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
        // `similar_per_mate_raw` / `similar_mates_suppressed` retired with the
        // pass-vs-pass tripwire (GH #217): under always-serve both passes read
        // the one surface through the same call, so the diff was a tautology.
        // Absent from newer rows, never redefined — the row conventions.
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
                // The ENGINE's verdict, matching the dense row's convention and
                // what the exit gate asserts — recording the harness's
                // restatement here instead would mask exactly the drift the
                // [FAULT] check exists to surface (the restatement is
                // re-derivable from `terms` + `best_cos` + `bar` regardless).
                "vouched": r.vouched,
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
