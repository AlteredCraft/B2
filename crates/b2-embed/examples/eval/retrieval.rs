//! Retrieval scoring: one pass over the labelled queries at note and chunk level, under
//! the shipped fusion or the dense-only ablation (GH #158).

use crate::labels::Labelled;
use crate::metrics::{paths_match, Agg};
use crate::K;
use b2_core::vault::Vault;

/// One query's 1-based ranks in one retrieval mode; `chunk` only for passage-labelled
/// queries.
pub struct QueryScore {
    pub note: Option<usize>,
    pub chunk: Option<usize>,
    pub top: String,
}

/// A pass over the query set in the vault's current state (keyword-only before `embed`,
/// hybrid after).
pub struct Pass {
    pub scores: Vec<QueryScore>,
    pub note: Agg,
    pub chunk: Agg,
}

/// Which retrieval a pass scores. `Fused` is the shipped `Vault::search` plus chunk-level
/// scoring; `VectorOnly` is the dense ablation (GH #158), note-level only, which shows
/// whether fusion pays its way.
#[derive(Clone, Copy, PartialEq)]
pub enum Retrieval {
    Fused,
    VectorOnly,
}

/// Score every labelled query. A chunk hit is the first top-K chunk of a relevant note that
/// contains the labelled phrase, case-insensitively.
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
