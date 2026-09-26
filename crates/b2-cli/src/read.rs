//! The read commands: the graph (`neighbors`, `explain`), `search`, and discovery
//! (`similar`, `similar --explain`), with the presentation helpers they share.

use crate::args::Cli;
use crate::error::CliError;
use crate::wiring::open_vault;
use crate::{emit, print_json};
use b2_core::resource::{doc_kind, DocKind};
use b2_core::vault::{
    ExplainView, ResourceExplainView, SearchEvidenceView, SimilarExplainView, SimilarStanding,
};

pub fn cmd_neighbors(cli: &Cli, note: &str) -> Result<(), CliError> {
    // Neighbors is a pure graph query — it never embeds, so don't require
    // the model (no needless `b2 init` just to explore the graph).
    let vault = open_vault(cli.vault_or_cwd(), false)?;
    let neighbors = vault.neighbors(note)?;
    // Dangling outbound links (a `[[folder]]` or a typo) that resolve to no
    // note or resource — surfaced, not dropped (GH #12). `--json` keeps its
    // resolved-neighbors array contract; the full structured picture,
    // including these, is `b2 explain --json`.
    let unresolved = vault.unresolved_links(note)?;
    emit(cli.json, &neighbors, |neighbors| {
        if neighbors.is_empty() && unresolved.is_empty() {
            println!("No neighbors.");
            return;
        }
        for n in neighbors {
            let arrow = arrow(&n.direction);
            let name = display_name(n.title.as_deref(), &n.path);
            let explanation = n
                .explanation
                .as_deref()
                .map(|e| format!(" — {e}"))
                .unwrap_or_default();
            println!("{arrow} {}  {name} ({}){explanation}", n.label, n.path);
        }
        for u in &unresolved {
            println!(
                "⚠ {}  [[{}]] — unresolved (no matching note or file)",
                u.relation, u.target
            );
        }
    })
}

pub fn cmd_explain(cli: &Cli, note: &str) -> Result<(), CliError> {
    // Explain is a pure graph read (edges + their explanations), no embed —
    // like `neighbors`, it opens with the fake and needs no `b2 init`.
    let vault = open_vault(cli.vault_or_cwd(), false)?;
    // Kind dispatch by the argument's own shape (core's one rule, §9b #8):
    // a resource arg gets the fallback card's view — metadata + backlinks.
    if doc_kind(note) == DocKind::Resource {
        return emit(
            cli.json,
            &vault.explain_resource(note)?,
            print_resource_card,
        );
    }
    emit(cli.json, &vault.explain(note)?, print_explanation)
}

/// `explain` on a resource, for a human: the fallback card's metadata and backlinks.
fn print_resource_card(view: &ResourceExplainView) {
    println!("{} ({}, {} bytes)", view.path, view.class, view.size);
    if view.backlinks.is_empty() {
        println!("No backlinks yet.");
    } else {
        println!("Backlinks:");
        for b in &view.backlinks {
            let name = display_name(b.title.as_deref(), &b.path);
            let mut line = format!("  ← {name} ({})  {}", b.path, b.r#type);
            decorate(&mut line, b.embed, b.caption.as_deref());
            println!("{line}");
        }
    }
}

/// `explain` on a note, for a human: every connection with its "why", then the links
/// at resources and the ones that resolve to nothing.
fn print_explanation(view: &ExplainView) {
    let name = display_name(view.title.as_deref(), &view.path);
    println!("{name} ({})", view.path);
    if view.connections.is_empty() && view.resources.is_empty() && view.unresolved.is_empty() {
        // Zero connections at all — nothing links to it and it links to
        // nothing (an orphan; the kernel only surfaces, never archives).
        println!("No connections yet.");
    } else if !view.connections.is_empty() {
        println!("Connections:");
        for c in &view.connections {
            let arrow = arrow(&c.direction);
            let target = display_name(c.title.as_deref(), &c.path);
            println!(
                "  {arrow} {}  {target} ({})  [{}]",
                c.label, c.path, c.origin
            );
            if let Some(why) = &c.explanation {
                println!("      why: {why}");
            }
        }
        // If nothing points *at* the note, it's an orphan — surfaced, not
        // acted on (invariants.md; files are only touched when asked).
        if !view.connections.iter().any(|c| c.direction == "inbound") {
            println!("No inbound links — this note is an orphan.");
        }
    }
    // Outbound links at resources (images, PDFs, …) — the third target
    // kind an edge can have, shown from the note's side (GH #22).
    if !view.resources.is_empty() {
        println!("Resource links:");
        for r in &view.resources {
            let mut line = format!(
                "  → {}  {} ({})  [{}]",
                r.relation, r.path, r.class, r.origin
            );
            decorate(&mut line, r.embed, r.caption.as_deref());
            println!("{line}");
            if let Some(why) = &r.explanation {
                println!("      why: {why}");
            }
        }
    }
    // Dangling outbound links (a `[[folder]]` or a typo): a note is one
    // `.md` file, so these resolve to nothing — shown as broken rather
    // than silently dropped (GH #12).
    if !view.unresolved.is_empty() {
        println!("Unresolved links:");
        for u in &view.unresolved {
            println!(
                "  ⚠ {}  [[{}]]  (no matching note or file)  [{}]",
                u.relation, u.target, u.origin
            );
            if let Some(why) = &u.explanation {
                println!("      why: {why}");
            }
        }
    }
}

pub fn cmd_search(
    cli: &Cli,
    query: &str,
    limit: usize,
    exclude: &[String],
) -> Result<(), CliError> {
    // Search embeds the query for the vector half → it needs the real model.
    let vault = open_vault(cli.vault_or_cwd(), true)?;
    // The evidence read, not the bare list (invariants.md D2, GH #201/#202): the
    // rows are identical and in the same order, and the verdict beside them is
    // what lets this command say **"no matches"** honestly instead of serving
    // `limit` confident-looking results for a query the vault holds nothing for.
    // `--exclude` paths are the caller's subtraction, never the verdict's.
    let view = vault.search_evidence_excluding(query, limit, exclude)?;
    // `--json` is the whole view, verdict included. This is an OBJECT where `--json` used
    // to be an array — a deliberate break (GH #202), because a query-level verdict has
    // nowhere to live in a list of rows; the rows themselves stay additive.
    //
    // The JSON serves the rows even at `vouched: false`, where the human surface shows
    // none: an agent handed the rows *plus* an explicit verdict can be honest about them,
    // where a human handed rows alone cannot.
    emit(cli.json, &view, |view| {
        println!("{}", search_report(view, query));
        // Honesty (never overstate): with the fake embedder the vector half
        // isn't semantic. Under the real model it is, so no caveat. Kept on
        // stderr so stdout stays pure results.
        if b2_embed::fake_requested() {
            eprintln!(
                "note: keyword (BM25) ranking is live; semantic ranking is off (fake embedder)."
            );
        }
    })
}

/// The human-mode rendering of a search — ADR-0015's three verdict states as one pure
/// function (GH #202).
///
/// Pure, and separate from [`cmd_search`], because the state that matters most is the one
/// the integration suite structurally cannot reach: `Some(false)` needs a *calibrated*
/// bar, and the fake embedder the suite runs under has none by design. A branch reachable
/// only under the real model is still the fast suite's to own once the rendering is
/// separated from the retrieval.
fn search_report(view: &SearchEvidenceView, query: &str) -> String {
    match view.vouched {
        // D2's "no matches", strict: the vault holds neither a lexical anchor
        // nor semantic proximity clearing the model's bar, so the
        // nearest-by-meaning rows are not shown at all — no reveal, no `--all`.
        // Offering them behind a flag would still be this command putting them
        // forward as candidates (GH #202, decision 1).
        Some(false) => format!("No matches. Nothing in the vault matches “{query}”."),
        // `Some(true)` — the vault vouches for these — and `None`, which is *no
        // verdict at all*: the active embedder has no calibrated bar (the fake
        // one, or any model until the harness measures it — M2), and reading
        // that as "no matches" would blank a dev vault. Three states, three
        // behaviors; `None` is never folded into `false`.
        _ if view.results.is_empty() => "No results.".to_string(),
        _ => view
            .results
            .iter()
            .map(|r| {
                let name = display_name(r.result.title.as_deref(), &r.result.path);
                let head = format!("{:.4}  {name} ({})", r.result.score, r.result.path);
                if r.result.snippet.is_empty() {
                    head
                } else {
                    format!("{head}\n    {}", r.result.snippet)
                }
            })
            .collect::<Vec<_>>()
            .join("\n"),
    }
}

pub fn cmd_similar(cli: &Cli, note: &str, limit: usize) -> Result<(), CliError> {
    // Candidate generation reads the *stored* vectors (no query embedding), so
    // like `neighbors` it needs no live model — a prior `reindex` supplies them.
    // Open with the fake; it's a pure, instant local read. (The z grading keys
    // on the vault's RECORDED model id, so opening with the fake for reading
    // never turns it off.)
    let vault = open_vault(cli.vault_or_cwd(), false)?;
    let results = vault.similar(note, limit)?;
    if cli.json {
        print_json(&results)?;
    } else if limit == 0 {
        // An ask for nothing yields nothing, silently: the empty-state copy
        // below reads the candidate set, and a zero limit proves nothing about
        // it — printing "nothing to compare" here would be the one way this
        // command could still make a claim it can't check (GH #197).
    } else if results.is_empty() {
        // Two honest empty states, and neither claims "nothing relates"
        // (GH #197): at a nonzero ask, an empty list means the candidate set is
        // genuinely empty — nothing unlinked with stored vectors to compare —
        // or the vault's similarity isn't semantic yet.
        let status = vault.embed_status()?;
        if status.embedded > 0 {
            println!("Nothing unlinked has stored vectors to compare.");
        } else {
            println!(
                "No similar notes. (If you haven't yet, run `b2 init` then `b2 reindex` so similarity is semantic.)"
            );
        }
    } else {
        for r in &results {
            let name = display_name(r.title.as_deref(), &r.path);
            println!("{:.4}  {name} ({})", r.score, r.path);
            if !r.evidence.is_empty() {
                println!("    {}", r.evidence);
            }
        }
        // Nudge toward the commit step, on stderr so stdout stays pure results.
        eprintln!("Commit one with:  b2 link {note} <note> --type <verb>");
    }
    Ok(())
}

/// Longest passage excerpt `similar --explain` prints per side, so a pair stays readable
/// in a terminal. `--json` carries the full text.
const EXPLAIN_EXCERPT_CHARS: usize = 200;

pub fn cmd_explain_similar(
    cli: &Cli,
    note: &str,
    other: &str,
    limit: usize,
) -> Result<(), CliError> {
    // A pure read over stored vectors, like `similar`: the fake is enough to open with.
    let vault = open_vault(cli.vault_or_cwd(), false)?;
    let ex = vault.explain_similar(note, other, limit)?;
    emit(cli.json, &ex, |ex| print_similar_explanation(ex, limit))
}

/// `similar --explain`, for a human: where the candidate stands against the list shown
/// at `limit` (and why, if it is not a card), then the passage pairs behind it.
fn print_similar_explanation(ex: &SimilarExplainView, limit: usize) {
    let anchor = display_name(ex.anchor.title.as_deref(), &ex.anchor.path);
    let candidate = display_name(ex.candidate.title.as_deref(), &ex.candidate.path);
    println!(
        "{candidate} ({}) against {anchor} ({})",
        ex.candidate.path, ex.anchor.path
    );
    let z = ex.z.map(|z| format!(", z {z:.2}")).unwrap_or_default();
    let standing = match ex.standing {
        SimilarStanding::SameNote => "That is the same note.".to_string(),
        SimilarStanding::AnchorUnembedded => format!(
            "{anchor} has no stored vectors yet, so there is nothing to compare from. Run `b2 reindex`."
        ),
        SimilarStanding::Linked => {
            "Already linked, so Similar leaves it out: it shows what you haven't connected."
                .to_string()
        }
        SimilarStanding::Unembedded => {
            "Not embedded yet, so it can't be compared. Run `b2 reindex`.".to_string()
        }
        SimilarStanding::NotShortlisted { shortlist } => format!(
            "Never scored: judged as a whole note it ranks #{}, past the first-pass shortlist of {shortlist}.",
            ex.centroid_rank.unwrap_or(0)
        ),
        SimilarStanding::Ranked {
            rank,
            of,
            served: true,
        } => format!("Shown at #{rank} of {limit} ({of} notes scored{z})."),
        SimilarStanding::Ranked {
            rank,
            of,
            served: false,
        } => format!("Ranked #{rank} of {of} scored{z}: past the {limit} shown."),
    };
    println!("{standing}");
    if let (Some(c), SimilarStanding::Ranked { rank, .. }) = (ex.centroid_rank, ex.standing) {
        println!("Judged as a whole note it ranks #{c}; by its best passage, #{rank}.");
    }
    if !ex.shared_neighbors.is_empty() {
        let names: Vec<String> = ex
            .shared_neighbors
            .iter()
            .map(|n| format!("{} ({})", display_name(n.title.as_deref(), &n.path), n.path))
            .collect();
        println!("Both link with: {}", names.join(", "));
    }
    if !ex.pairs.is_empty() {
        println!("\nPassage pairs, nearest first:");
        // Pairs carry a grade only when the list does; say why they don't, once, rather
        // than print bare pairs that read as unmeasured by accident.
        if ex.pairs.iter().all(|p| p.z.is_none()) {
            println!(
                "     Ungraded: too few notes to compare against, or a vault indexed without the real model."
            );
        }
    }
    for (i, p) in ex.pairs.iter().enumerate() {
        let grade = p.z.map(|z| format!("  z {z:.2}")).unwrap_or_default();
        let same = if p.identical { "  identical text" } else { "" };
        println!("{:>3}.{grade}{same}", i + 1);
        for (side, passage) in [("this", &p.anchor), ("that", &p.candidate)] {
            let heading = passage
                .heading_path
                .as_deref()
                .map(|h| format!("[{h}] "))
                .unwrap_or_default();
            println!(
                "     {side}: {heading}{}",
                excerpt(&passage.text, EXPLAIN_EXCERPT_CHARS)
            );
        }
    }
}

/// A passage flattened to one line and cut at `max` characters.
fn excerpt(text: &str, max: usize) -> String {
    let flat = text.split_whitespace().collect::<Vec<_>>().join(" ");
    if flat.chars().count() <= max {
        flat
    } else {
        format!("{}…", flat.chars().take(max).collect::<String>().trim_end())
    }
}

/// The presentation rule for naming a note: its title when it has one, else its
/// vault-relative path.
fn display_name<'a>(title: Option<&'a str>, path: &'a str) -> &'a str {
    title.unwrap_or(path)
}

/// The direction glyph: `→` for an outbound edge (this note → other), `←` inbound.
fn arrow(direction: &str) -> &'static str {
    if direction == "outbound" {
        "→"
    } else {
        "←"
    }
}

/// Append a resource line's decorations: the `(embed)` marker and the quoted caption.
fn decorate(line: &mut String, embed: bool, caption: Option<&str>) {
    use std::fmt::Write as _;
    if embed {
        line.push_str(" (embed)");
    }
    if let Some(c) = caption {
        let _ = write!(line, " — \"{c}\"");
    }
}

/// ADR-0015's three verdict states as rendered by [`search_report`] (GH #202).
///
/// The integration suite spawns the binary under `B2_EMBEDDER=fake`, whose embedder has no
/// calibrated bar — so `vouched` there is always `None` and the two states that matter are
/// unreachable from it. That is a fact about the model seam, not a hole to leave: the
/// rendering is pure, so it is tested here directly rather than hidden behind an
/// `#[ignore]` the suite would keep reporting as present.
#[cfg(test)]
mod tests {
    use super::*;
    use b2_core::vault::{EvidencedResult, SearchResult};

    fn view(vouched: Option<bool>, n: usize) -> SearchEvidenceView {
        SearchEvidenceView {
            results: (0..n)
                .map(|i| EvidencedResult {
                    result: SearchResult {
                        path: format!("notes/n{i}.md"),
                        title: Some(format!("Note {i}")),
                        score: 0.5,
                        snippet: "a matched line".to_string(),
                    },
                    bm25_rank: Some(i),
                    vector_rank: Some(i),
                    cos: Some(0.7),
                })
                .collect(),
            vouched,
            chunk_total: 100,
            terms: Vec::new(),
            best_cos: Some(0.7),
        }
    }

    #[test]
    fn an_unvouched_query_shows_the_empty_state_and_none_of_its_rows() {
        // Strict (GH #202, decision 1): the engine served rows, and the surface
        // shows none of them — no reveal, no count of what is being withheld,
        // since either would put them forward as candidates after all.
        let out = search_report(&view(Some(false), 10), "Fasdfadsf");
        assert!(out.starts_with("No matches."), "{out}");
        assert!(!out.contains("notes/n0.md"), "no row leaks: {out}");
        assert!(out.contains("Fasdfadsf"), "names the query back: {out}");
    }

    #[test]
    fn a_vouched_query_serves_its_rows() {
        let out = search_report(&view(Some(true), 2), "memory");
        assert!(
            out.contains("notes/n0.md") && out.contains("notes/n1.md"),
            "{out}"
        );
        assert!(out.contains("a matched line"), "snippets ride along: {out}");
    }

    #[test]
    fn no_calibrated_bar_serves_its_rows_exactly_as_before() {
        // `None` is *no verdict*, never "no matches" (M2): the fake embedder and
        // every uncalibrated model land here, and folding it into `false` would
        // blank the app on a dev vault.
        let out = search_report(&view(None, 2), "memory");
        assert!(out.contains("notes/n0.md"), "{out}");
        assert!(!out.contains("No matches"), "{out}");
    }

    #[test]
    fn an_empty_list_reads_as_no_results_whatever_the_verdict() {
        // The genuinely-empty list keeps its own older copy, which claims
        // nothing about evidence — an unbuilt index is not a judgment.
        for vouched in [Some(true), None] {
            let out = search_report(&view(vouched, 0), "memory");
            assert_eq!(out, "No results.", "{vouched:?}");
        }
    }
}
