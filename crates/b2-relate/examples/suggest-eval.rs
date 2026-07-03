//! Suggestion-quality eval — the relator half of the model-quality pass (tasks.md
//! step 5 / "the suggestion-quality eval"), the parallel of the semantic-retrieval
//! eval in `b2-embed` (`examples/eval.rs`). It lives as an **example, not a test**, so
//! it never runs in the deterministic, model-free `cargo test` CI and model quality can
//! never flake the suite (vision-and-scope testability point 5). Run it on demand:
//!
//! ```console
//! ANTHROPIC_API_KEY=… cargo run -p b2-relate --example suggest-eval
//! ```
//!
//! Unlike the retrieval eval it does **not** build a vault or touch the embedder: the
//! thing under test is the [`Relator`]'s *judgment*, so it feeds hand-labelled note
//! pairs straight to the real [`ClaudeRelator`] and scores the verdicts. Isolating the
//! relator this way keeps the score independent of candidate-generation / embedder
//! quality (those are separate, separately-tuned concerns). Each pair in
//! `evals/pairs.json` is either a genuine typed connection (with every defensible verb)
//! or a decline — including "same-topic but not connected" traps, because over-firing
//! is the relator's primary failure mode.
//!
//! Metrics (data-model.md §2 verbs; the standard binary-classifier trio):
//!   - **firing precision** — of the pairs it fired on, how many should connect (the
//!     over-firing gate; the relator's whole job is precision);
//!   - **firing recall** — of the pairs that should connect, how many it caught;
//!   - **verb accuracy** — of the true-positive fires, how many chose an acceptable
//!     verb (gold lists all defensible verbs because the vocabulary genuinely overlaps).
//!
//! Nondeterminism: the model is called once per pair (like the retrieval eval runs
//! each query once), so a run is a single sample, not an average. Growing the labelled
//! set — the durable audit log of real `relate()` calls (tasks.md backlog) is its
//! natural source — and a `--repeat N` agreement pass are the follow-ups.

use b2_core::relate::{Candidate, NoteCtx, Proposal, Relator};
use b2_core::relation;
use b2_relate::{ClaudeRelator, RelateConfig};
use serde::Deserialize;
use std::collections::HashMap;
use std::path::Path;

/// The provenance signal handed to the relator — stands in for candidate-gen's
/// `semantic:maxsim`. It does not reach the prompt (only the evidence chunk does), so
/// its exact value does not affect the verdict; it is here for shape-fidelity.
const SIGNAL: &str = "eval:labelled";

/// Reference floor for the over-firing gate, mirroring the retrieval eval's `p@1`
/// floor. A run below this exits non-zero so the eval can also serve as a manual
/// quality check; it is a tunable reference, not a hard contract.
const PRECISION_FLOOR: f64 = 0.75;

// --- the labelled set -------------------------------------------------------

#[derive(Deserialize)]
struct PairSet {
    #[serde(default)]
    #[allow(dead_code)]
    description: String,
    pairs: Vec<Pair>,
}

#[derive(Deserialize)]
struct Pair {
    /// Corpus filename stem (no `.md`) of the anchor note.
    anchor: String,
    /// Corpus filename stem of the candidate note.
    candidate: String,
    /// The candidate passage that "surfaced" this pair. Omitted → the candidate's
    /// first paragraph. Must be a substring of the candidate note when present.
    #[serde(default)]
    evidence: Option<String>,
    gold: Gold,
    /// Labeller comment (e.g. "hard: same-topic trap"). Ignored by scoring, printed
    /// beside misses to make tuning legible.
    #[serde(default)]
    note: Option<String>,
}

/// The gold verdict as written in JSON: either the bare string `"decline"`, or
/// `{ "connect": [verbs] }`. Untagged so both shapes parse; [`Gold::normalize`]
/// validates and lifts it into a [`GoldLabel`].
#[derive(Deserialize)]
#[serde(untagged)]
enum Gold {
    Connect { connect: Vec<String> },
    Tag(String),
}

/// The validated gold label used for scoring.
#[derive(Debug, Clone, PartialEq)]
enum GoldLabel {
    /// No typed connection a careful author would record.
    Decline,
    /// A connection whose acceptable verbs are these (most-apt first, all core).
    Connect(Vec<String>),
}

impl Gold {
    /// Validate the raw label — fail fast (per the config-values policy) on an empty
    /// or non-core verb list, or an unknown tag string, so a typo in the labelled set
    /// surfaces immediately rather than skewing the score.
    fn normalize(&self) -> Result<GoldLabel, String> {
        match self {
            Gold::Connect { connect } => {
                if connect.is_empty() {
                    return Err("gold `connect` list is empty".to_string());
                }
                if let Some(bad) = connect.iter().find(|v| !relation::is_core(v)) {
                    return Err(format!("gold verb '{bad}' is not a core relation verb"));
                }
                Ok(GoldLabel::Connect(connect.clone()))
            }
            Gold::Tag(s) if s == "decline" => Ok(GoldLabel::Decline),
            Gold::Tag(s) => Err(format!(
                "unknown gold label '{s}' (expected \"decline\" or {{ \"connect\": [...] }})"
            )),
        }
    }
}

// --- the corpus -------------------------------------------------------------

/// A corpus note reduced to what the relator reads: its title and body.
struct Note {
    title: String,
    body: String,
}

impl Note {
    /// The title as an `Option<&str>` for [`NoteCtx`], `None` when untitled.
    fn title(&self) -> Option<&str> {
        (!self.title.is_empty()).then_some(self.title.as_str())
    }
}

/// Split a note file into `title` (from a leading `--- … ---` YAML block's `title:`)
/// and `body` (everything after). A deliberately minimal frontmatter reader for the
/// eval's own authored corpus — not the real parser (b2-core owns that); the eval
/// stays dependency-light and independent of the index.
fn parse_note(raw: &str) -> Note {
    let mut title = String::new();
    let mut in_frontmatter = false;
    let mut frontmatter_done = false;
    let mut body_lines: Vec<&str> = Vec::new();

    for (i, line) in raw.lines().enumerate() {
        if i == 0 && line.trim() == "---" {
            in_frontmatter = true;
            continue;
        }
        if in_frontmatter && !frontmatter_done {
            if line.trim() == "---" {
                frontmatter_done = true;
            } else if let Some(v) = line.strip_prefix("title:") {
                title = v.trim().to_string();
            }
            continue;
        }
        body_lines.push(line);
    }

    if title.is_empty() {
        // Fallback: a leading Markdown `# Heading`.
        title = body_lines
            .iter()
            .find_map(|l| l.strip_prefix("# "))
            .unwrap_or("")
            .trim()
            .to_string();
    }

    Note {
        title,
        body: body_lines.join("\n").trim().to_string(),
    }
}

/// Load every `*.md` under `dir`, keyed by filename stem (the id the labelled set
/// references).
fn load_corpus(dir: &Path) -> std::io::Result<HashMap<String, Note>> {
    let mut corpus = HashMap::new();
    for entry in std::fs::read_dir(dir)? {
        let path = entry?.path();
        if path.extension().and_then(|e| e.to_str()) != Some("md") {
            continue;
        }
        let Some(stem) = path.file_stem().and_then(|s| s.to_str()) else {
            continue;
        };
        let raw = std::fs::read_to_string(&path)?;
        corpus.insert(stem.to_string(), parse_note(&raw));
    }
    Ok(corpus)
}

/// The candidate passage for a pair: the labelled `evidence` (validated to be a real
/// substring of the candidate note), or the candidate's first paragraph as a fallback.
fn resolve_evidence(pair: &Pair, candidate: &Note) -> Result<String, String> {
    match &pair.evidence {
        Some(e) => {
            if candidate.body.contains(e.as_str()) {
                Ok(e.clone())
            } else {
                Err(format!(
                    "evidence for {}→{} is not a substring of candidate '{}' — fix the label",
                    pair.anchor, pair.candidate, pair.candidate
                ))
            }
        }
        None => Ok(candidate
            .body
            .split("\n\n")
            .next()
            .unwrap_or(&candidate.body)
            .trim()
            .to_string()),
    }
}

// --- scoring ----------------------------------------------------------------

/// The confusion quadrant a pair lands in — "positive" = the relator fired.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Decision {
    TruePos,
    FalsePos,
    TrueNeg,
    FalseNeg,
}

/// The scored outcome of one pair.
struct PairResult {
    decision: Decision,
    /// `Some` only for a true positive: whether the fired verb was in the gold set.
    verb_ok: Option<bool>,
}

/// Score one verdict against its gold label — the pure heart of the eval, unit-tested
/// without a model. `verdict` is the relator's output (`None` = it declined).
fn score_pair(gold: &GoldLabel, verdict: Option<&Proposal>) -> PairResult {
    match (gold, verdict) {
        (GoldLabel::Connect(acceptable), Some(p)) => PairResult {
            decision: Decision::TruePos,
            verb_ok: Some(acceptable.iter().any(|v| v == &p.edge_type)),
        },
        (GoldLabel::Connect(_), None) => PairResult {
            decision: Decision::FalseNeg,
            verb_ok: None,
        },
        (GoldLabel::Decline, Some(_)) => PairResult {
            decision: Decision::FalsePos,
            verb_ok: None,
        },
        (GoldLabel::Decline, None) => PairResult {
            decision: Decision::TrueNeg,
            verb_ok: None,
        },
    }
}

// --- driver -----------------------------------------------------------------

/// A labelled pair validated against the corpus and ready to send — the anchor and
/// candidate notes resolved, the gold verdict normalized, the evidence chunk fixed.
/// Building this for the whole set **before** any API call means a bad label (unknown
/// note, non-core verb, evidence that isn't in the note) fails fast with zero spend.
struct Prepared<'a> {
    pair: &'a Pair,
    anchor: &'a Note,
    candidate: &'a Note,
    gold: GoldLabel,
    evidence: String,
}

/// Validate every pair against the corpus, up front. Any error here is a defect in the
/// labelled set, surfaced before a single (paid) relator call is made.
fn prepare<'a>(
    set: &'a PairSet,
    corpus: &'a HashMap<String, Note>,
) -> Result<Vec<Prepared<'a>>, String> {
    set.pairs
        .iter()
        .map(|pair| {
            let gold = pair.gold.normalize()?;
            let anchor = corpus
                .get(&pair.anchor)
                .ok_or_else(|| format!("unknown anchor note '{}'", pair.anchor))?;
            let candidate = corpus
                .get(&pair.candidate)
                .ok_or_else(|| format!("unknown candidate note '{}'", pair.candidate))?;
            let evidence = resolve_evidence(pair, candidate)?;
            Ok(Prepared {
                pair,
                anchor,
                candidate,
                gold,
                evidence,
            })
        })
        .collect()
}

fn main() {
    if let Err(e) = run() {
        eprintln!("suggest-eval failed: {e}");
        std::process::exit(1);
    }
}

fn run() -> Result<(), Box<dyn std::error::Error>> {
    let evals_dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("evals");
    let corpus = load_corpus(&evals_dir.join("corpus"))?;
    let set: PairSet =
        serde_json::from_str(&std::fs::read_to_string(evals_dir.join("pairs.json"))?)?;

    // Validate all labels against the corpus first — a data typo must not cost a call.
    let prepared = prepare(&set, &corpus)?;
    eprintln!("[eval] {} pairs validated against corpus\n", prepared.len());

    // Only now construct the real relator. Fails fast here (missing key / bad config)
    // rather than mid-run, mirroring the CLI.
    let config = RelateConfig::load()?;
    let relator = ClaudeRelator::from_config(&config)?;
    eprintln!("[eval] relator model = {}\n", config.model);

    let mut tp = 0usize;
    let mut fp = 0usize;
    let mut tn = 0usize;
    let mut fn_ = 0usize;
    let mut verb_correct = 0usize;
    let mut misses: Vec<String> = Vec::new();

    println!("{:<3} {:<34} {:<12} model", "", "pair", "gold");
    println!("{}", "-".repeat(74));

    for item in &prepared {
        let pair = item.pair;
        let gold = &item.gold;

        let anchor_ctx = NoteCtx {
            b2id: &pair.anchor,
            title: item.anchor.title(),
            text: &item.anchor.body,
        };
        let candidate_view = Candidate {
            note: NoteCtx {
                b2id: &pair.candidate,
                title: item.candidate.title(),
                text: &item.candidate.body,
            },
            evidence_chunk: &item.evidence,
            signal: SIGNAL,
            score: 1.0,
        };

        let verdict = relator.relate(&anchor_ctx, &candidate_view)?;
        let result = score_pair(gold, verdict.as_ref());

        match result.decision {
            Decision::TruePos => {
                tp += 1;
                if result.verb_ok == Some(true) {
                    verb_correct += 1;
                }
            }
            Decision::FalsePos => fp += 1,
            Decision::TrueNeg => tn += 1,
            Decision::FalseNeg => fn_ += 1,
        }

        let mark = match result.decision {
            Decision::TrueNeg => "✓",
            Decision::TruePos if result.verb_ok == Some(true) => "✓",
            Decision::TruePos => "~", // right call, verb outside the gold set
            Decision::FalsePos | Decision::FalseNeg => "✗",
        };
        let gold_str = match gold {
            GoldLabel::Decline => "decline".to_string(),
            GoldLabel::Connect(vs) => vs.first().cloned().unwrap_or_default(),
        };
        let model_str = match verdict.as_ref() {
            Some(p) => format!("{} ({:.2})", p.edge_type, p.confidence),
            None => "decline".to_string(),
        };
        let label = format!("{} → {}", pair.anchor, pair.candidate);
        println!(
            "{mark:<3} {:<34} {gold_str:<12} {model_str}",
            truncate(&label, 34)
        );

        if mark != "✓" {
            let why = pair.note.as_deref().unwrap_or("");
            misses.push(format!(
                "  {mark} {label}: gold={gold_str}, model={model_str}{}",
                if why.is_empty() {
                    String::new()
                } else {
                    format!("  — {why}")
                }
            ));
        }
    }

    let connect_gold = tp + fn_;
    let decline_gold = tn + fp;
    let fired = tp + fp;
    let precision = if fired > 0 {
        tp as f64 / fired as f64
    } else {
        1.0
    };
    let recall = if connect_gold > 0 {
        tp as f64 / connect_gold as f64
    } else {
        1.0
    };
    let f1 = if precision + recall > 0.0 {
        2.0 * precision * recall / (precision + recall)
    } else {
        0.0
    };
    let verb_acc = if tp > 0 {
        verb_correct as f64 / tp as f64
    } else {
        1.0
    };

    if !misses.is_empty() {
        println!("\nmisses ({}):", misses.len());
        for m in &misses {
            println!("{m}");
        }
    }

    println!("\n{}", "=".repeat(74));
    println!(
        "pairs={}   connect-gold={connect_gold}   decline-gold={decline_gold}",
        set.pairs.len()
    );
    println!(
        "firing:  precision={:.2} ({tp}/{fired} fired)   recall={:.2} ({tp}/{connect_gold})   F1={:.2}",
        precision, recall, f1
    );
    println!(
        "verb:    accuracy={:.2} ({verb_correct}/{tp} true positives took an acceptable verb)",
        verb_acc
    );

    let usage = relator.usage();
    println!(
        "tokens:  ~ {} input + {} output over {} call(s)",
        usage.input_tokens, usage.output_tokens, usage.calls
    );

    if precision < PRECISION_FLOOR {
        eprintln!(
            "\n[warn] firing precision {:.2} is below the {:.2} reference floor — the relator is over-firing; inspect the misses above.",
            precision, PRECISION_FLOOR
        );
        std::process::exit(2);
    }
    Ok(())
}

/// Truncate `s` to `max` chars with an ellipsis, so the table stays aligned.
fn truncate(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        s.to_string()
    } else {
        let cut: String = s.chars().take(max - 1).collect();
        format!("{cut}…")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn proposal(verb: &str) -> Proposal {
        Proposal {
            edge_type: verb.to_string(),
            explanation: "because".to_string(),
            confidence: 0.8,
        }
    }

    #[test]
    fn gold_connect_parses_and_validates() {
        let g = Gold::Connect {
            connect: vec!["elaborates".into(), "references".into()],
        };
        assert_eq!(
            g.normalize().unwrap(),
            GoldLabel::Connect(vec!["elaborates".into(), "references".into()])
        );
    }

    #[test]
    fn gold_decline_tag_parses() {
        assert_eq!(
            Gold::Tag("decline".into()).normalize().unwrap(),
            GoldLabel::Decline
        );
    }

    #[test]
    fn gold_rejects_empty_non_core_and_bad_tag() {
        assert!(Gold::Connect { connect: vec![] }.normalize().is_err());
        assert!(Gold::Connect {
            connect: vec!["is-friends-with".into()]
        }
        .normalize()
        .is_err());
        assert!(Gold::Tag("connect".into()).normalize().is_err());
    }

    #[test]
    fn gold_json_shapes_deserialize() {
        // Object → Connect; bare string → Tag. Both must parse under the untagged enum.
        let connect: Gold = serde_json::from_str(r#"{ "connect": ["relates"] }"#).unwrap();
        assert!(matches!(connect, Gold::Connect { .. }));
        let decline: Gold = serde_json::from_str(r#""decline""#).unwrap();
        assert!(matches!(decline, Gold::Tag(_)));
    }

    #[test]
    fn score_pair_covers_the_four_quadrants() {
        let connect = GoldLabel::Connect(vec!["supports".into(), "references".into()]);
        // True positive, acceptable verb.
        let r = score_pair(&connect, Some(&proposal("references")));
        assert_eq!(r.decision, Decision::TruePos);
        assert_eq!(r.verb_ok, Some(true));
        // True positive, verb outside the gold set.
        let r = score_pair(&connect, Some(&proposal("part-of")));
        assert_eq!(r.decision, Decision::TruePos);
        assert_eq!(r.verb_ok, Some(false));
        // False negative — should have connected, declined.
        assert_eq!(score_pair(&connect, None).decision, Decision::FalseNeg);
        // False positive — should have declined, fired.
        assert_eq!(
            score_pair(&GoldLabel::Decline, Some(&proposal("relates"))).decision,
            Decision::FalsePos
        );
        // True negative.
        assert_eq!(
            score_pair(&GoldLabel::Decline, None).decision,
            Decision::TrueNeg
        );
    }

    #[test]
    fn parse_note_reads_frontmatter_title_and_body() {
        let raw = "---\ntype: note\ntitle: Grind Size\n---\nBody line one.\nBody line two.\n";
        let note = parse_note(raw);
        assert_eq!(note.title, "Grind Size");
        assert_eq!(note.body, "Body line one.\nBody line two.");
    }

    #[test]
    fn parse_note_without_frontmatter_falls_back_to_heading() {
        let note = parse_note("# Just A Heading\n\nsome body");
        assert_eq!(note.title, "Just A Heading");
        assert_eq!(note.body, "# Just A Heading\n\nsome body");
    }

    fn decline_pair(evidence: Option<&str>) -> Pair {
        Pair {
            anchor: "a".into(),
            candidate: "c".into(),
            evidence: evidence.map(str::to_string),
            gold: Gold::Tag("decline".into()),
            note: None,
        }
    }

    #[test]
    fn resolve_evidence_defaults_and_validates() {
        let candidate = Note {
            title: "T".into(),
            body: "first para sentence.\n\nsecond para.".into(),
        };
        // Omitted → the candidate's first paragraph.
        assert_eq!(
            resolve_evidence(&decline_pair(None), &candidate).unwrap(),
            "first para sentence."
        );
        // Present and a real substring → returned verbatim.
        assert_eq!(
            resolve_evidence(&decline_pair(Some("second para.")), &candidate).unwrap(),
            "second para."
        );
        // Present but not a substring → error (catches label drift).
        assert!(resolve_evidence(&decline_pair(Some("not in the note")), &candidate).is_err());
    }
}
