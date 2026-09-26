//! Retrieval scoring: one pass over the labelled queries at note and chunk level, under
//! the shipped fusion or the dense-only ablation (GH #158).

use crate::labels::Labelled;
use crate::metrics::{paths_match, Agg};
use crate::K;
use b2_core::vault::Vault;

/// One query's ranks in one retrieval mode: 1-based note rank, 1-based chunk rank
/// (only for passage-labelled queries), and the top note hit for display.
pub struct QueryScore {
    pub note: Option<usize>,
    pub chunk: Option<usize>,
    pub top: String,
}

/// A full pass over the query set in the vault's current state (keyword-only
/// before `embed`, hybrid after): per-query scores plus note- and chunk-level
/// aggregates.
pub struct Pass {
    pub scores: Vec<QueryScore>,
    pub note: Agg,
    pub chunk: Agg,
}

/// Which retrieval a pass scores. `Fused` is the shipped path (`Vault::search` —
/// BM25-only before embed, hybrid after) plus chunk-level scoring for
/// passage-labelled queries; `VectorOnly` is the dense ablation
/// (`Vault::search_vector_only`, GH #158), note-level only — its chunk aggregate
/// stays empty. The ablation column is what lets a run say whether fusion paid
/// rent: the finding that RRF demotes dense rank-1 hits was established
/// by hand-decomposing fused scores once; this makes it a standing measurement.
#[derive(Clone, Copy, PartialEq)]
pub enum Retrieval {
    Fused,
    VectorOnly,
}

/// Score every labelled query against the vault's current state: note rank via
/// the selected retrieval, and — for passage-labelled queries on the fused path —
/// chunk rank via `search_chunks` (the first top-K chunk that belongs to a
/// relevant note AND contains the labelled phrase, case-insensitively).
pub fn score_pass(
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
