//! Groundedness + citation-accuracy smoke for the chat seam — the embedder
//! eval's posture (GH #151 §"Testing & evaluation", cut as GH #154) applied to
//! the second seam.
//!
//! It lives as an **example**, not a test, for the reason `b2-embed`'s does: it
//! needs a real model (two, in fact — an embedder to retrieve with and a chat
//! model to answer with), it is non-deterministic, and it talks to the network.
//! `cargo test` must never do any of those (E2). Run it on demand:
//!
//! ```console
//! ollama serve &                                     # or any OpenAI-compatible server
//! cargo run -p b2-llm --example groundedness         # B2_LLM_URL / B2_LLM_MODEL apply
//! ```
//!
//! One run builds a throwaway vault from the retrieval eval's corpus
//! (`crates/b2-embed/evals/corpus`), embeds it with the configured model, and
//! asks each labelled question in `evals/questions.json` through the real
//! `Vault::ask` — the same flow ④ the CLI and the desktop call. Four things are
//! scored, and they are deliberately separable, because a bad answer has more
//! than one possible author:
//!
//! 1. **Retrieval reach** — did the labelled note make it into the passages at
//!    all? This is the ceiling: the model cannot cite what it was never handed,
//!    so a question that fails here is a *retrieval* result, not a chat one, and
//!    it belongs to `--example eval`.
//! 2. **Grounding** — did the answer cite anything? An uncited answer is either
//!    a refusal or general knowledge, and the grounded prompt forbids the second.
//! 3. **Citation accuracy** — did it cite the *labelled* note? The headline
//!    number, scored only over questions retrieval actually reached.
//! 4. **Refusal** — on the negative questions (empty `expect`), did it say "I
//!    don't find that in your notes" instead of confabulating? Refusing when the
//!    corpus *does* answer is scored too, as the opposite failure.
//!
//! Hallucinated `[n]` markers are counted alongside: a marker naming no passage
//! resolves to no citation (`Vault::ask` never rewrites the answer), so it is
//! invisible in the citation list and would otherwise go unmeasured.
//!
//! Every run appends one JSON line to `evals/results.jsonl` (gitignored, since
//! the numbers depend on the machine's models) — the same append-only convention
//! as the retrieval eval and `B2_LOG_FILE`. **Exit code**: 0 normally, 2 only
//! when *no* answerable question produced a correct citation, which is a broken
//! pipeline rather than a weak model. Model quality is read off the numbers, not
//! off the exit code — a gate that fails on every small local model would be a
//! gate nobody runs.

use b2_core::chat::{cited_markers, ASK_PASSAGES};
use b2_core::embed::Embedder;
use b2_core::llm::{LlmProvider, NO_EVIDENCE_ANSWER};
use b2_core::vault::Vault;
use b2_embed::{provision, EmbedConfig, LocalEmbedder};
use b2_llm::{LlmConfig, OpenAiCompatProvider};
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
    /// The note(s) a correct answer cites. **Empty means unanswerable** — the
    /// only correct answer is the refusal.
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
    /// Did retrieval hand the model a passage from a labelled note?
    retrieval_hit: bool,
    passages: usize,
    citations: usize,
    /// Did the answer cite a labelled note?
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
    // Deliberately the *retrieval* eval's corpus: chat is scored over the notes
    // whose retrieval behaviour is already characterised, so a surprise here can
    // be attributed rather than guessed at.
    let corpus_dir = manifest.join("../b2-embed/evals/corpus");
    let results_path = evals_dir.join("results.jsonl");

    let set: QuestionSet =
        serde_json::from_str(&std::fs::read_to_string(evals_dir.join("questions.json"))?)?;

    // The chat provider first: it is the one that needs a server running, and
    // finding that out after a model download and an embed pass would be rude.
    let llm_config = LlmConfig::from_env();
    let llm = OpenAiCompatProvider::new(llm_config.clone());
    llm.probe()?;
    eprintln!(
        "[eval] chat = {} at {}",
        llm_config.model, llm_config.base_url
    );

    let embed_config = EmbedConfig::load()?;
    provision(&embed_config, |line| eprintln!("[init] {line}"))?;
    let embedder = LocalEmbedder::load(&embed_config)?;
    let embed_model = embedder.model_id().to_string();
    eprintln!("[eval] embedder = {embed_model}");

    // A throwaway vault from the corpus — nothing here touches a real one.
    let tmp = tempfile::TempDir::new()?;
    let vault_root = tmp.path().join("vault");
    std::fs::create_dir_all(&vault_root)?;
    for entry in std::fs::read_dir(&corpus_dir)? {
        let entry = entry?;
        if entry.file_type()?.is_file() {
            std::fs::copy(entry.path(), vault_root.join(entry.file_name()))?;
        }
    }
    let vault = Vault::open_with_embedder(&vault_root, Box::new(embedder))?;
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

    // The floor is a liveness check, not a quality bar (see the module doc).
    let answerable_hits = scored
        .iter()
        .filter(|s| !s.unanswerable && s.retrieval_hit)
        .count();
    let correct = scored.iter().filter(|s| s.cited_expected).count();
    Ok(answerable_hits == 0 || correct > 0)
}

/// Ask one question through the real flow ④ and score what came back.
fn ask_one(vault: &Vault, llm: &dyn LlmProvider, q: &Question) -> Result<Scored, Box<dyn Error>> {
    // Retrieval, scored separately *before* the ask: `Vault::ask` retrieves the
    // same way for a single-turn question (no condensation), so this is the same
    // passage set the model is about to see — and it is the ceiling the citation
    // score is judged against.
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

    // Markers the answer *claims* minus the ones that name a real passage: what
    // the citation list can never show, because an unmatched marker resolves to
    // nothing (and the answer text is never rewritten).
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
        refused: answer.answer.contains(NO_EVIDENCE_ANSWER),
        tokens,
        first_token_ms: first_token.unwrap_or(total_ms),
        total_ms,
        answer: answer.answer,
    })
}

/// The per-question readout — the paired list is the primary evidence, exactly as
/// it is for the retrieval eval: aggregates hide which question moved.
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

/// Fold the run into the row that lands in `results.jsonl` — aggregates plus
/// every per-question detail, so a past run can be re-read without re-running it
/// (answers included: the numbers say *that* it miscited, the text says how).
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
        // The ceiling: chat can only be scored where retrieval delivered.
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

/// Append one row to the results log (creating it on first run). Append-only, so
/// runs accumulate into one dataset — the retrieval eval's convention.
fn append_result(path: &Path, row: &serde_json::Value) -> Result<(), Box<dyn Error>> {
    use std::io::Write;
    let mut f = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)?;
    writeln!(f, "{row}")?;
    Ok(())
}

/// The repo's short commit hash, best-effort (None outside a git checkout).
fn git_short_sha() -> Option<String> {
    let out = std::process::Command::new("git")
        .args(["rev-parse", "--short", "HEAD"])
        .current_dir(env!("CARGO_MANIFEST_DIR"))
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    Some(String::from_utf8_lossy(&out.stdout).trim().to_string())
}

fn truncate(s: &str, max: usize) -> String {
    match s.char_indices().nth(max) {
        None => s.to_string(),
        Some((i, _)) => format!("{}…", &s[..i.saturating_sub(1)]),
    }
}
