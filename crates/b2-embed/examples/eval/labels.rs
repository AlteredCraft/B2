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
    /// The vault-relative path(s) that should rank first. Empty means a negative query
    /// (D2, GH #201): everything served is junk by label. Negatives enter no rank
    /// aggregate; only the search evidence calibration scores them.
    pub relevant: Vec<String>,
    /// A verbatim phrase from the target passage; when present the query is also scored at
    /// chunk level. Labelling rules are in queries.json.
    #[serde(default)]
    pub passage: Option<String>,
    /// Other notes that are honest evidence for the query (GH #206). Exhaustive: a served
    /// note in neither list is irrelevant by label. Never enters a rank aggregate.
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
    /// Corpus notes a human says belong next to `anchor`. Empty means a negative anchor:
    /// everything surfaced is junk by label.
    pub expected: Vec<String>,
}

/// Every note in one corpus dir as `file name → lowercased content`, lowercased to match
/// [`score_pass`](crate::retrieval::score_pass)'s case-insensitive containment.
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

/// Refuse to score against labels the corpus cannot honour: a missing path, a `passage` not
/// in a relevant note, or a negative query with a `passage` or `tail_relevant`. Prints every
/// fault before refusing.
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
            // Every string contains "", so a blank passage would match every chunk and
            // inflate chunk rank (PR #221).
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
