//! Scoring primitives every block shares: the rank aggregate, the two-population
//! threshold window, rank notation, and label-path matching.

use crate::K;

/// Running hit@1 / hit@3 / MRR@K over 1-based ranks. With one target per query,
/// precision@k and recall@k coincide with hit@k.
#[derive(Default)]
pub struct Agg {
    pub n: usize,
    pub hit1: usize,
    pub hit3: usize,
    pub rr: f64,
}

impl Agg {
    pub fn add(&mut self, rank: Option<usize>) {
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
    pub fn hit1(&self) -> f64 {
        self.hit1 as f64 / self.n.max(1) as f64
    }
    pub fn hit3(&self) -> f64 {
        self.hit3 as f64 / self.n.max(1) as f64
    }
    pub fn mrr(&self) -> f64 {
        self.rr / self.n.max(1) as f64
    }
}

/// The admissible interval `(cut_max, keep_min]` for one threshold, read off two labelled
/// populations. When they overlap no constant works, which is a result, not a failure.
pub struct Window {
    pub cut_max: f64,
    pub keep_min: f64,
}

impl Window {
    /// `None` while either population is empty: no reading, not a wide-open window.
    pub fn read(cut: &[f64], keep: &[f64]) -> Option<Self> {
        let cut_max = cut.iter().copied().fold(f64::NEG_INFINITY, f64::max);
        let keep_min = keep.iter().copied().fold(f64::INFINITY, f64::min);
        (cut_max.is_finite() && keep_min.is_finite()).then_some(Self { cut_max, keep_min })
    }
    /// Whether any constant separates the two populations.
    pub fn open(&self) -> bool {
        self.cut_max < self.keep_min
    }
}

/// [`rank_str`] at a caller-named depth (the discovery metrics read at
/// [`SIM_K`](crate::SIM_K), not [`K`]).
pub fn rank_str_at(rank: Option<usize>, depth: usize) -> String {
    match rank {
        Some(1) => "✓1".to_string(),
        Some(r) => format!("·{r}"),
        None => format!("✗>{depth}"),
    }
}

/// Corpus notes are copied flat into the vault, so a result path equals (or ends
/// with) the labelled relevant path.
pub fn paths_match(result_path: &str, relevant: &str) -> bool {
    result_path == relevant || result_path.ends_with(&format!("/{relevant}"))
}

pub fn rank_str(rank: Option<usize>) -> String {
    match rank {
        Some(1) => "✓1".to_string(),
        Some(r) => format!("·{r}"),
        None => format!("✗>{K}"),
    }
}
