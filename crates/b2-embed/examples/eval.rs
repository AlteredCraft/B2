//! Semantic-retrieval eval — the "separate, occasional pass" scoring model quality
//! (tasks.md step 5). It lives as an **example**, not a test, so it never runs in
//! the deterministic `cargo test` CI and model quality can never flake the suite
//! (vision-and-scope testability point 5). Run it on demand:
//!
//! ```console
//! cargo run -p b2-embed --example eval           # uses the configured model
//! ```
//!
//! It builds a throwaway vault from the hand-labelled corpus in `evals/`, reindexes
//! it through the **real** embedder + the full hybrid pipeline (BM25 ⊕ vector → RRF),
//! then scores each labelled query by the rank of its relevant note. The queries are
//! written to avoid the target's keywords, so a passing score is genuinely semantic
//! lift, not lexical overlap.
//!
//! Scope: this covers **semantic search** quality. **Suggestion** quality (the other
//! half of step 5) is scaffolded here but lands with the connection-discovery
//! pipeline — nothing generates suggestions yet (tasks.md "After that").

use b2_core::embed::Embedder;
use b2_core::vault::Vault;
use b2_embed::{provision, EmbedConfig, LocalEmbedder};
use serde::Deserialize;
use std::path::Path;

#[derive(Deserialize)]
struct QuerySet {
    queries: Vec<Labelled>,
}

#[derive(Deserialize)]
struct Labelled {
    query: String,
    relevant: Vec<String>,
}

/// How deep we look for a relevant note when scoring.
const K: usize = 10;

fn main() {
    if let Err(e) = run() {
        eprintln!("eval failed: {e}");
        std::process::exit(1);
    }
}

fn run() -> Result<(), Box<dyn std::error::Error>> {
    let evals_dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("evals");
    let corpus_dir = evals_dir.join("corpus");

    // Load the labelled query set.
    let set: QuerySet =
        serde_json::from_str(&std::fs::read_to_string(evals_dir.join("queries.json"))?)?;

    // Ensure the model is available, then load it. (Provision is idempotent, so an
    // already-installed model is a no-op; a missing one is fetched here.)
    let config = EmbedConfig::load()?;
    provision(&config, |line| eprintln!("[init] {line}"))?;
    let embedder = LocalEmbedder::load(&config)?;
    eprintln!("[eval] model = {} (dim {})\n", config.model, embedder.dim());

    // Build a throwaway vault from the corpus and index it with the real model.
    let tmp = tempfile::TempDir::new()?;
    let vault_root = tmp.path().join("vault");
    std::fs::create_dir_all(&vault_root)?;
    for entry in std::fs::read_dir(&corpus_dir)? {
        let entry = entry?;
        std::fs::copy(entry.path(), vault_root.join(entry.file_name()))?;
    }
    let vault = Vault::open_with_embedder(&vault_root, Box::new(embedder))?;
    let report = vault.reindex()?;
    eprintln!("[eval] indexed {} notes\n", report.indexed);

    // Score each query by the rank (1-based) of its first relevant note.
    let mut hits_at_1 = 0usize;
    let mut hits_at_3 = 0usize;
    let mut reciprocal_sum = 0.0f64;
    let n = set.queries.len();

    println!("{:<6}  {:<44}  top hit", "rank", "query");
    println!("{}", "-".repeat(78));
    for q in &set.queries {
        let results = vault.search(&q.query, K)?;
        let rank = results
            .iter()
            .position(|r| q.relevant.iter().any(|rel| paths_match(&r.path, rel)))
            .map(|p| p + 1);

        let top = results.first().map(|r| r.path.as_str()).unwrap_or("—");
        let rank_str = rank
            .map(|r| r.to_string())
            .unwrap_or_else(|| format!(">{K}"));
        let mark = match rank {
            Some(1) => "✓",
            Some(_) => "·",
            None => "✗",
        };
        println!(
            "{mark} {:<4}  {:<44}  {}",
            rank_str,
            truncate(&q.query, 44),
            top
        );

        if let Some(r) = rank {
            reciprocal_sum += 1.0 / r as f64;
            if r <= 1 {
                hits_at_1 += 1;
            }
            if r <= 3 {
                hits_at_3 += 1;
            }
        }
    }

    let p_at_1 = hits_at_1 as f64 / n as f64;
    let p_at_3 = hits_at_3 as f64 / n as f64;
    let mrr = reciprocal_sum / n as f64;
    println!("\n{}", "=".repeat(78));
    println!(
        "queries={n}   precision@1={:.2}   precision@3={:.2}   MRR@{K}={:.3}",
        p_at_1, p_at_3, mrr
    );

    // A soft floor so this can also gate a manual quality check. Not a CI test.
    if p_at_1 < 0.75 {
        eprintln!(
            "\n[warn] precision@1 {:.2} is below the 0.75 reference floor — inspect the misses above.",
            p_at_1
        );
        std::process::exit(2);
    }
    Ok(())
}

/// Corpus notes are copied flat into the vault, so a result path equals (or ends
/// with) the labelled relevant path.
fn paths_match(result_path: &str, relevant: &str) -> bool {
    result_path == relevant || result_path.ends_with(&format!("/{relevant}"))
}

fn truncate(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        s.to_string()
    } else {
        let cut: String = s.chars().take(max - 1).collect();
        format!("{cut}…")
    }
}
