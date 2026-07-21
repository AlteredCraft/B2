//! Step 2 — link parsing (planning/data-model.md §2). Pure (no DB), so these pin
//! the classification rules directly: every body link is an untyped `references`
//! edge (the body carries no B2 syntax; decision 2026-07-21), and the verb +
//! explanation are parsed only from a frontmatter `b2_relations:` entry
//! (`parse_relation`).

use b2_core::link::{parse_links, parse_relation};

#[test]
fn bare_wikilink_in_prose_is_a_references_edge() {
    let links = parse_links("See [[concepts/memory|Human memory]] for context.\n");
    assert_eq!(links.len(), 1);
    assert_eq!(links[0].edge_type, "references");
    assert_eq!(links[0].target_path, "concepts/memory");
    assert_eq!(links[0].alias.as_deref(), Some("Human memory"));
    assert_eq!(links[0].explanation, None);
    assert!(!links[0].typed);
}

#[test]
fn verb_prefixed_body_list_item_is_a_plain_reference() {
    // The old body typed-line syntax is gone: the verb and trailing text are
    // prose, and only the wikilink projects — as an untyped reference.
    let links =
        parse_links("- supports [[concepts/memory|Human memory]] — applies the forgetting curve\n");
    assert_eq!(links.len(), 1);
    assert_eq!(links[0].edge_type, "references");
    assert_eq!(links[0].target_path, "concepts/memory");
    assert_eq!(links[0].alias.as_deref(), Some("Human memory"));
    assert_eq!(links[0].explanation, None);
    assert!(!links[0].typed);
}

#[test]
fn body_links_never_gain_a_type_from_surrounding_prose() {
    // A prose link and a verb-led list item are the same thing to the parser:
    // two references edges, in document order — no shape is "special".
    let body = "Spaced repetition exploits the [[concepts/memory|Human memory]] retrieval curve.\n\n## Relations\n- supports [[concepts/memory|Human memory]] — applies the forgetting curve\n";
    let links = parse_links(body);
    assert_eq!(links.len(), 2);
    assert!(links.iter().all(|l| l.edge_type == "references"));
    assert!(links.iter().all(|l| !l.typed && l.explanation.is_none()));
}

#[test]
fn list_item_with_a_bare_link_and_no_verb_is_a_reference() {
    let links = parse_links("- [[concepts/memory|Human memory]]\n");
    assert_eq!(links.len(), 1);
    assert_eq!(links[0].edge_type, "references");
    assert!(!links[0].typed);
}

#[test]
fn lowercase_verb_lookalikes_in_prose_stay_prose() {
    // The exact hazard that killed the body syntax: `- see [[x]]` must not
    // become a typed edge of verb "see".
    let links = parse_links("- see [[concepts/memory|Human memory]] for the mechanism\n");
    assert_eq!(links.len(), 1);
    assert_eq!(links[0].edge_type, "references");
    assert!(!links[0].typed);
}

#[test]
fn a_link_without_an_alias_keeps_a_none_alias() {
    let links = parse_links("Refer to [[concepts/memory]].\n");
    assert_eq!(links[0].target_path, "concepts/memory");
    assert_eq!(links[0].alias, None);
}

#[test]
fn relation_entry_parses_verb_link_and_explanation() {
    let l =
        parse_relation("supports [[concepts/memory|Human memory]] — applies the forgetting curve")
            .unwrap();
    assert!(l.typed);
    assert_eq!(l.edge_type, "supports");
    assert_eq!(l.target_path, "concepts/memory");
    assert_eq!(l.alias.as_deref(), Some("Human memory"));
    assert_eq!(
        l.explanation.as_deref(),
        Some("applies the forgetting curve")
    );
}

#[test]
fn relation_explanation_after_a_colon_is_supported() {
    let l = parse_relation("supersedes [[notes/old-plan|Old plan]] : replaced after Q2").unwrap();
    assert_eq!(l.edge_type, "supersedes");
    assert_eq!(l.explanation.as_deref(), Some("replaced after Q2"));
}

#[test]
fn relation_tail_verb_is_kept_verbatim() {
    let l = parse_relation("inspired-by [[notes/x|X]]").unwrap();
    assert!(l.typed);
    assert_eq!(l.edge_type, "inspired-by");
    assert_eq!(l.explanation, None);
}

#[test]
fn relation_bare_link_falls_back_to_references() {
    let l = parse_relation("[[concepts/memory|Human memory]]").unwrap();
    assert!(!l.typed);
    assert_eq!(l.edge_type, "references");
    assert_eq!(l.target_path, "concepts/memory");
}
