//! The labelled sets (`queries.json`, `similar.json`, `similar-dense.json`) and the lint
//! that refuses to score against labels the corpus cannot honour.

use serde::Deserialize;
use std::collections::HashMap;
use std::path::Path;

#[derive(Deserialize)]
pub struct QuerySet {
    pub queries: Vec<Labelled>,
}

#[derive(Deserialize)]
pub struct Labelled {
    pub query: String,
    /// The vault-relative path(s) that should rank first. **Empty = a negative
    /// query** (invariants.md D2, GH #201): the labelled answer is "no
    /// matches" — the vault holds no evidence for the query, so everything
    /// served is junk by label, the query-side sibling of similar.json's
    /// negative anchors. Negative queries are excluded from every rank
    /// aggregate (adding one moves no pre-existing number) and are scored only
    /// by the search evidence calibration.
    pub relevant: Vec<String>,
    /// A short verbatim phrase from the target passage; when present the query is
    /// also scored at chunk level (does a top-K chunk of a relevant note contain
    /// it?). See queries.json's description for the labelling rules.
    #[serde(default)]
    pub passage: Option<String>,
    /// Notes beyond `relevant` that are honest evidence for the query (GH #206) —
    /// the per-hit tail depth. The judgement is **exhaustive** for every positive
    /// query: a served note in neither `relevant` nor here is irrelevant *by
    /// label*, which is the statement the tail bake-off is judged on. Never enters
    /// a rank aggregate — `relevant` alone says what should rank first.
    #[serde(default)]
    pub tail_relevant: Vec<String>,
}

#[derive(Deserialize)]
pub struct SimilarSet {
    pub anchors: Vec<SimilarLabel>,
}

#[derive(Deserialize)]
pub struct SimilarLabel {
    pub anchor: String,
    /// Corpus notes a human says belong next to `anchor`. **Empty = a negative
    /// anchor**: the labelled answer is "nothing relates", so the right result is
    /// zero candidates and everything surfaced is junk by label (similar.json).
    pub expected: Vec<String>,
}

/// Every note in one corpus dir, as `file name → lowercased content` — the
/// ground the label lint checks against. Lowercased once, so the passage check
/// matches the way [`score_pass`](crate::retrieval::score_pass) will (case-insensitive containment).
pub fn corpus_texts(dir: &Path) -> Result<HashMap<String, String>, Box<dyn std::error::Error>> {
    let mut out = HashMap::new();
    for entry in std::fs::read_dir(dir)? {
        let entry = entry?;
        if entry.file_type()?.is_file() {
            out.insert(
                entry.file_name().to_string_lossy().into_owned(),
                std::fs::read_to_string(entry.path())?.to_lowercase(),
            );
        }
    }
    Ok(out)
}

/// Refuse to score against labels the corpus cannot honour (see the call site).
///
/// Checks: every labelled path (`relevant`, `tail_relevant`, `anchor`,
/// `expected`) names a note in its corpus; every `passage` occurs verbatim
/// (case-insensitively) in a note the query labels relevant — the containment
/// chunk scoring will test; and a negative query carries neither a `passage`
/// nor a `tail_relevant` (its whole list is junk by label already, per the
/// queries.json rules). Faults are all printed before the run refuses, so one
/// run names every problem rather than the first.
pub fn lint_labels(
    corpus_dir: &Path,
    dense_dir: &Path,
    positives: &[Labelled],
    negatives: &[Labelled],
    sim_set: &SimilarSet,
    dense_set: &SimilarSet,
) -> Result<(), Box<dyn std::error::Error>> {
    let corpus = corpus_texts(corpus_dir)?;
    let dense = corpus_texts(dense_dir)?;
    let mut faults: Vec<String> = Vec::new();

    for q in positives {
        for (key, paths) in [
            ("relevant", &q.relevant),
            ("tail_relevant", &q.tail_relevant),
        ] {
            for p in paths {
                if !corpus.contains_key(p) {
                    faults.push(format!(
                        "queries.json: `{key}` names no corpus note: {p:?} (query {:?})",
                        q.query
                    ));
                }
            }
        }
        if let Some(passage) = &q.passage {
            // A blank passage is the opposite defect from a typo'd one: every
            // string contains "", so it would lint clean here and then "match"
            // every top-K chunk of a relevant note in the chunk scoring —
            // silently inflating chunk rank instead of reading a miss
            // (PR #221 review).
            if passage.trim().is_empty() {
                faults.push(format!(
                    "queries.json: `passage` is blank (query {:?}) — it would match every chunk \
                     and inflate chunk rank",
                    q.query
                ));
            } else {
                let needle = passage.to_lowercase();
                let found = q
                    .relevant
                    .iter()
                    .filter_map(|p| corpus.get(p))
                    .any(|text| text.contains(&needle));
                if !found {
                    faults.push(format!(
                        "queries.json: `passage` {passage:?} is not verbatim in any relevant note \
                         (query {:?}) — chunk rank would read a permanent miss",
                        q.query
                    ));
                }
            }
        }
    }
    for q in negatives {
        if q.passage.is_some() {
            faults.push(format!(
                "queries.json: negative query {:?} carries a `passage` — negatives score no rank",
                q.query
            ));
        }
        if !q.tail_relevant.is_empty() {
            faults.push(format!(
                "queries.json: negative query {:?} carries `tail_relevant` — its whole served \
                 list is junk by label",
                q.query
            ));
        }
    }
    for (file, set, texts) in [
        ("similar.json", sim_set, &corpus),
        ("similar-dense.json", dense_set, &dense),
    ] {
        for label in &set.anchors {
            if !texts.contains_key(&label.anchor) {
                faults.push(format!(
                    "{file}: `anchor` names no corpus note: {:?}",
                    label.anchor
                ));
            }
            for e in &label.expected {
                if !texts.contains_key(e) {
                    faults.push(format!(
                        "{file}: `expected` names no corpus note: {e:?} (anchor {:?})",
                        label.anchor
                    ));
                }
            }
        }
    }
    if faults.is_empty() {
        return Ok(());
    }
    for f in &faults {
        eprintln!("[lint] {f}");
    }
    Err(format!(
        "{} label lint fault(s) — a label the corpus cannot honour scores as a permanent miss \
         and reads as an engine regression; fix the label file (or the note) before scoring",
        faults.len()
    )
    .into())
}
