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
    // Every query that fusion ranks worse than the dense signal alone (GH #158).
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
    // The saturated per-anchor metric is recorded in the row but not printed (GH #188).
    println!(
        "  per-mate   hit@1={:.2}  hit@3={:.2}  MRR@{SIM_K}={:.3}  (n={} mates, GATED at MRR@{SIM_K} ≥ {FLOOR_MATE_MRR:.2})",
        similar.mate.hit1(),
        similar.mate.hit3(),
        similar.mate.mrr(),
        similar.mate.n
    );
    // Never gated; see `SimilarPass::strangers`.
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
        // Under always-serve (GH #197) a negative anchor serves; its bands say what it claims.
        println!(
            "  negatives  {} loner anchors serve {} cards under always-serve (labels still say \
             \"nothing relates\"; the bands carry the honesty — leaders below)",
            similar.neg_n, similar.neg_cards
        );
    }
    // If the piles separate, the gap is the floor; if they overlap, no simple floor holds.
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
    // The same question in z, the unit a constant would be stated in.
    print_floor_windows(floor_z);
}

/// The paired per-query diff between two fused passes, which an A/B is judged on
/// (docs/evals.md). No moves is a claim to verify against the piles, not proof of no
/// effect.
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

/// Who scored a run and in which embedding space; carried by every row the run appends.
#[derive(Clone, Copy)]
pub struct RunId<'a> {
    /// The repo's short commit, `None` outside a git checkout.
    pub git: Option<&'a str>,
    pub model: &'a str,
    pub dim: usize,
}

/// One scored configuration's inputs to [`result_row`]: a short-lived, read-only view.
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
    pub queries: &'a [Labelled],
    pub bm25: Option<&'a Pass>,
    pub vector: Option<&'a Pass>,
    pub hybrid: &'a Pass,
    pub similar: Option<&'a SimilarPass>,
    /// Only the default row re-derives these; other rows record `null`.
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
///
/// Row convention: a key is added, never redefined, so a mixed file shows a missing field
/// rather than a number whose meaning changed. Dates note when a key first appeared.
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
        // Since 2026-08-18; absent means orthogonal. Never average across corpora.
        "corpus": "orthogonal",
        "config": {
            "label": label,
            "target_tokens": cfg.target_tokens,
            "overlap_frac": cfg.overlap_frac,
            "chars_per_token": cfg.chars_per_token,
            "backscan_tokens": cfg.backscan_tokens,
            "prepend_heading_path": cfg.prepend_heading_path,
        },
        // Since 2026-08-11; earlier rows are `unicode61` (GH #157).
        "tokenizer": tokenizer,
        "notes": notes,
        "chunks": chunks,
        // A `pool_blind` row can't show a candidate-width change (GH #141). Earlier rows'
        // flat `pool` key is not reused.
        "pool_note": note_candidate_pool(K),
        "pool_chunk": chunk_candidate_pool(K),
        "pool_blind": chunks <= chunk_candidate_pool(K).min(note_candidate_pool(K)),
        "embed_secs": embed_secs,
        "note": {
            "bm25": bm25.map(|p| agg(&p.note)),
            // Since 2026-08-10 (GH #158).
            "vector": vector.map(|p| agg(&p.note)),
            "hybrid": agg(&hybrid.note),
        },
        "chunk": {
            "bm25": bm25.map(|p| agg(&p.chunk)),
            "hybrid": agg(&hybrid.chunk),
        },
        "similar": similar.map(|s| agg(&s.rank)),
        // Since 2026-08-17 (GH #183).
        "similar_per_mate": similar.map(|s| agg(&s.mate)),
        // `similar_per_mate_raw` / `similar_mates_suppressed` retired (GH #217).
        // Since 2026-08-17 (GH #188), with the pairs so a reader can check them.
        "similar_strangers": similar.map(|s| serde_json::json!({
            "cards": s.strangers.len(),
            "anchors": s.stranger_anchors,
            "positives": s.rank.n,
            // So a SIM_K change shows in the row rather than redefining the count.
            "depth": SIM_K,
            "detail": s.strangers.iter().map(|(anchor, path)| serde_json::json!({
                "anchor": anchor,
                "path": path,
            })).collect::<Vec<_>>(),
        })),
        "similar_negatives": similar.map(|s| serde_json::json!({
            "n": s.neg_n, "clean": s.neg_clean, "cards": s.neg_cards,
        })),
        // Cosine, 4 decimals.
        "similar_piles": similar.map(|s| serde_json::json!({
            "related": s.related.iter().map(|c| (c * 1e4).round() / 1e4).collect::<Vec<_>>(),
            "junk": s.junk.iter().map(|c| (c * 1e4).round() / 1e4).collect::<Vec<_>>(),
        })),
        // The piles above with per-anchor rank order kept.
        "similar_detail": similar.map(|s| s.detail.iter().map(|d| serde_json::json!({
            "anchor": d.anchor,
            "negative": d.negative,
            "candidates": d.candidates.iter().map(|(path, cos, related)| serde_json::json!({
                "path": path,
                "cos": (cos * 1e4).round() / 1e4,
                "related": related,
            })).collect::<Vec<_>>(),
        })).collect::<Vec<_>>()),
        // Since 2026-08-17 (GH #187): the z populations and both re-derived windows.
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
                // Rows before GH #192 lack this and are stage-1 centroid z: never compare
                // across the flip.
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
                // ~0 is the only trustworthy reading (FloorZ::recheck_delta).
                "z_recheck_max_delta": z.recheck_delta(),
                // Per candidate, so the pair behind a window edge can be named.
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
        // Since 2026-08-22 (D2, GH #201).
        "search_evidence": evidence.map(|e| {
            let bar = EvidenceBar::for_model(model);
            let row = |r: &QueryEvidence| serde_json::json!({
                "q": r.query,
                "bm25_hits": r.bm25_hits,
                "bm25_best": r.bm25_best.map(|b| (b * 1e4).round() / 1e4),
                "best_cos": r.best_cos.map(|c| (c * 1e4).round() / 1e4),
                "top": r.top,
                "served": r.served(),
                // Raw, so any bake-off cell is re-derivable from the row.
                "dense_only": r.dense_only(),
                // Since GH #206. `keep` reads `relevant` ∪ `tail_relevant`.
                "rows": r.rows.iter().map(|row| serde_json::json!({
                    "path": row.path,
                    "bm25_rank": row.bm25_rank,
                    "cos": row.cos.map(|c| (c * 1e4).round() / 1e4),
                    "keep": row.keep,
                })).collect::<Vec<_>>(),
                "chunk_total": r.chunk_total,
                "terms": r.terms.iter().map(|(t, df)| serde_json::json!([t, df]))
                    .collect::<Vec<_>>(),
                // The engine's verdict, not the restatement, which would mask drift.
                "vouched": r.vouched,
            });
            let (pos_cos, neg_cos) = (best_cos_pile(&e.positives), best_cos_pile(&e.negatives));
            serde_json::json!({
                // So a K change shows in the row rather than redefining `served`.
                "k": K,
                "positives": e.positives.iter().map(row).collect::<Vec<_>>(),
                "negatives": e.negatives.iter().map(row).collect::<Vec<_>>(),
                "window_best_cos": Window::read(&neg_cos, &pos_cos).map(|w| serde_json::json!({
                    "cut_max": (w.cut_max * 1e4).round() / 1e4,
                    "keep_min": (w.keep_min * 1e4).round() / 1e4,
                    "open": w.open(),
                })),
                // The whole grid and the shipped bar, so drift out of the window is visible.
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
        // Default row only (GH #200): on a sweep variant it would measure the chunker.
        "discovery_fold": fold.map(fold_json),
        // Since GH #206. Default row only, like `discovery_fold`.
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
