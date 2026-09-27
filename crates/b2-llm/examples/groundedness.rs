//! Groundedness + citation-accuracy smoke for the chat seam (ADR-0013). An example, not a
//! test: it needs real models and the network, and is non-deterministic.
//!
//! ```console
//! ollama serve &                                     # or any OpenAI-compatible server
//! cargo run -p b2-llm --example groundedness         # B2_LLM_URL / B2_LLM_MODEL apply
//! ```
//!
//! Each labelled question goes through the real `Vault::ask` over the retrieval eval's
//! corpus. Scored separately, since a bad answer has more than one possible author:
//! retrieval reach (the ceiling), grounding (cited anything), citation accuracy (cited the
//! labelled note, over reached questions), refusal on negatives, and hallucinated `[n]`
//! markers.
//!
//! Appends one line to `evals/results.jsonl`. Exits 2 only when retrieval reached labelled
//! notes and no answer cited one (a broken pipeline); model quality is read off the numbers.

// The retrieval harness's shared helpers; everything they import is in our dev-deps too.
#[path = "../../b2-embed/examples/common/mod.rs"]
mod common;

use b2_core::chat::{cited_markers, ASK_PASSAGES};
use b2_core::embed::Embedder;
use b2_core::llm::{LlmProvider, NO_EVIDENCE_ANSWER};
use b2_core::vault::Vault;
use b2_embed::EmbedConfig;
use b2_llm::{LlmConfig, OpenAiCompatProvider};
use common::{append_result, git_short_sha, load_or_provision, truncate, ScratchVault};
use serde::Deserialize;
use std::error::Error;
use std::ops::ControlFlow;
use std::path::Path;
use std::time::Instant;

/// The labelled set. See `questions.json`'s own `description` for the rules.
#[derive(Debug, Deserialize)]
struct QuestionSet {
    questions: Vec<Question>,
}

#[derive(Debug, Deserialize)]
struct Question {
    question: String,
    /// The note(s) a correct answer cites. Empty means unanswerable: only a refusal is
    /// correct.
    #[serde(default)]
    expect: Vec<String>,
}

impl Question {
    fn unanswerable(&self) -> bool {
        self.expect.is_empty()
    }
}

/// What one asked question produced.
#[derive(Debug)]
struct Scored {
    question: String,
    unanswerable: bool,
    retrieval_hit: bool,
    passages: usize,
    citations: usize,
    cited_expected: bool,
    /// `[n]` markers naming no passage — invisible in the citation list.
    hallucinated_markers: usize,
    refused: bool,
    tokens: usize,
    first_token_ms: u128,
    total_ms: u128,
    answer: String,
}

fn main() {
    match run() {
        Err(e) => {
            eprintln!("groundedness eval failed: {e}");
            std::process::exit(1);
        }
        Ok(passed) => {
            if !passed {
                std::process::exit(2);
            }
        }
    }
}

fn run() -> Result<bool, Box<dyn Error>> {
    let manifest = Path::new(env!("CARGO_MANIFEST_DIR"));
    let evals_dir = manifest.join("evals");
    // The retrieval eval's corpus, whose retrieval behaviour is already characterised.
    let corpus_dir = manifest.join("../b2-embed/evals/corpus");
    let results_path = evals_dir.join("results.jsonl");

    let set: QuestionSet =
        serde_json::from_str(&std::fs::read_to_string(evals_dir.join("questions.json"))?)?;

    // Probe the chat server first, before a model download and an embed pass.
    let llm_config = LlmConfig::from_env();
    let llm = OpenAiCompatProvider::new(llm_config.clone());
    llm.probe()?;
    eprintln!(
        "[eval] chat = {} at {}",
        llm_config.model, llm_config.base_url
    );

    let embed_config = EmbedConfig::load()?;
    let embedder = load_or_provision(&embed_config)?;
    let embed_model = embedder.model_id().to_string();
    eprintln!("[eval] embedder = {embed_model}");

    let scratch = ScratchVault::copy_flat(&corpus_dir)?;
    let vault = Vault::open_with_embedder(scratch.root(), Box::new(embedder))?;
    let report = vault.reindex()?;
    eprintln!(
        "[eval] indexed {} notes; asking {} questions\n",
        report.indexed,
        set.questions.len()
    );

    let mut scored = Vec::with_capacity(set.questions.len());
    for q in &set.questions {
        scored.push(ask_one(&vault, &llm, q)?);
    }

    print_table(&scored);
    let row = summarize(&scored, &llm_config, &embed_model);
    print_summary(&row);
    append_result(&results_path, &row)?;
    eprintln!("\n[eval] appended one row to {}", results_path.display());

    // A liveness check, not a quality bar. Reaching nothing is retrieval's finding, not
    // chat's, so it doesn't fail here.
    let reached = scored
        .iter()
        .filter(|s| !s.unanswerable && s.retrieval_hit)
        .count();
    let correct = scored
        .iter()
        .filter(|s| !s.unanswerable && s.cited_expected)
        .count();
    Ok(reached == 0 || correct > 0)
}

/// Ask one question through the real flow ④ and score what came back.
fn ask_one(vault: &Vault, llm: &dyn LlmProvider, q: &Question) -> Result<Scored, Box<dyn Error>> {
    // A single-turn `Vault::ask` retrieves exactly this passage set (no condensation).
    let passages = vault.search_chunks(&q.question, ASK_PASSAGES)?;
    let retrieval_hit = passages.iter().any(|p| q.expect.contains(&p.path));

    let started = Instant::now();
    let mut tokens = 0usize;
    let mut first_token: Option<u128> = None;
    let answer = vault.ask(llm, &q.question, &[], &mut |_| {
        tokens += 1;
        first_token.get_or_insert_with(|| started.elapsed().as_millis());
        ControlFlow::Continue(())
    })?;
    let total_ms = started.elapsed().as_millis();

    // An unmatched marker resolves to no citation, so count it here.
    let claimed = cited_markers(&answer.answer, usize::MAX).len();
    let hallucinated_markers = claimed.saturating_sub(answer.citations.len());

    Ok(Scored {
        question: q.question.clone(),
        unanswerable: q.unanswerable(),
        retrieval_hit,
        passages: passages.len(),
        citations: answer.citations.len(),
        cited_expected: answer.citations.iter().any(|c| q.expect.contains(&c.path)),
        hallucinated_markers,
        // The sentence plus no citations: a cited "refusal" is a confabulation, and real
        // models pad the sentence, so exact match would measure verbosity.
        refused: answer.answer.contains(NO_EVIDENCE_ANSWER) && answer.citations.is_empty(),
        tokens,
        first_token_ms: first_token.unwrap_or(total_ms),
        total_ms,
        answer: answer.answer,
    })
}

/// The per-question readout, the primary evidence: aggregates hide which question moved.
fn print_table(scored: &[Scored]) {
    println!("per-question");
    println!(
        "{:<58} {:>5} {:>5} {:>6} {:>7} {:>8}  verdict",
        "question", "psg", "cite", "hallu", "ttft_ms", "total_ms"
    );
    for s in scored {
        let verdict = if s.unanswerable {
            if s.refused {
                "refused (correct)"
            } else {
                "CONFABULATED"
            }
        } else if s.cited_expected {
            "cited the note"
        } else if !s.retrieval_hit {
            "retrieval missed (not a chat result)"
        } else if s.refused {
            "REFUSED WITH EVIDENCE PRESENT"
        } else {
            "MISCITED"
        };
        println!(
            "{:<58} {:>5} {:>5} {:>6} {:>7} {:>8}  {}",
            truncate(&s.question, 58),
            s.passages,
            s.citations,
            s.hallucinated_markers,
            s.first_token_ms,
            s.total_ms,
            verdict
        );
    }
}

/// The `results.jsonl` row: aggregates plus every per-question detail and answer, so a past
/// run can be re-read without re-running it.
fn summarize(scored: &[Scored], llm: &LlmConfig, embed_model: &str) -> serde_json::Value {
    let answerable: Vec<&Scored> = scored.iter().filter(|s| !s.unanswerable).collect();
    let negatives: Vec<&Scored> = scored.iter().filter(|s| s.unanswerable).collect();
    let reached: Vec<&&Scored> = answerable.iter().filter(|s| s.retrieval_hit).collect();

    let share = |n: usize, d: usize| if d == 0 { 0.0 } else { n as f64 / d as f64 };
    let mean = |v: Vec<u128>| -> f64 {
        if v.is_empty() {
            0.0
        } else {
            v.iter().sum::<u128>() as f64 / v.len() as f64
        }
    };

    serde_json::json!({
        "run": "groundedness",
        "commit": git_short_sha(),
        "chat_model": llm.model,
        "chat_endpoint": llm.base_url,
        "embed_model": embed_model,
        "questions": scored.len(),
        "retrieval_reach": share(reached.len(), answerable.len()),
        "citation_accuracy": share(
            reached.iter().filter(|s| s.cited_expected).count(),
            reached.len()
        ),
        "grounding_rate": share(
            answerable.iter().filter(|s| s.citations > 0).count(),
            answerable.len()
        ),
        "refusal_accuracy": share(negatives.iter().filter(|s| s.refused).count(), negatives.len()),
        "false_refusals": reached.iter().filter(|s| s.refused).count(),
        "hallucinated_markers": scored.iter().map(|s| s.hallucinated_markers).sum::<usize>(),
        "mean_first_token_ms": mean(scored.iter().map(|s| s.first_token_ms).collect()),
        "mean_total_ms": mean(scored.iter().map(|s| s.total_ms).collect()),
        "per_question": scored.iter().map(|s| serde_json::json!({
            "question": s.question,
            "unanswerable": s.unanswerable,
            "retrieval_hit": s.retrieval_hit,
            "passages": s.passages,
            "citations": s.citations,
            "cited_expected": s.cited_expected,
            "hallucinated_markers": s.hallucinated_markers,
            "refused": s.refused,
            "tokens": s.tokens,
            "first_token_ms": s.first_token_ms,
            "total_ms": s.total_ms,
            "answer": s.answer,
        })).collect::<Vec<_>>(),
    })
}

fn print_summary(row: &serde_json::Value) {
    let pct = |k: &str| row[k].as_f64().unwrap_or_default() * 100.0;
    println!("\nsummary");
    println!(
        "  retrieval reach      {:>5.1}%   (the ceiling — a miss here is `--example eval`'s)",
        pct("retrieval_reach")
    );
    println!(
        "  citation accuracy    {:>5.1}%   (of the questions retrieval reached)",
        pct("citation_accuracy")
    );
    println!(
        "  grounding rate       {:>5.1}%   (answers that cited anything)",
        pct("grounding_rate")
    );
    println!(
        "  refusal accuracy     {:>5.1}%   (negatives correctly declined)",
        pct("refusal_accuracy")
    );
    println!(
        "  false refusals       {:>5}     hallucinated markers {}",
        row["false_refusals"], row["hallucinated_markers"]
    );
    println!(
        "  first token          {:>5.0}ms   total {:.0}ms (mean)",
        row["mean_first_token_ms"].as_f64().unwrap_or_default(),
        row["mean_total_ms"].as_f64().unwrap_or_default()
    );
}
