//! Step 2 — link parsing: the two body constructs that become edges
//! (planning/data-model.md §2). Pure (no DB), so these pin the classification
//! rules directly.

use b2_core::link::parse_links;

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
fn typed_relation_line_is_a_typed_edge_with_explanation() {
    let links =
        parse_links("- supports [[concepts/memory|Human memory]] — applies the forgetting curve\n");
    assert_eq!(links.len(), 1);
    assert_eq!(links[0].edge_type, "supports");
    assert_eq!(links[0].target_path, "concepts/memory");
    assert_eq!(links[0].alias.as_deref(), Some("Human memory"));
    assert_eq!(
        links[0].explanation.as_deref(),
        Some("applies the forgetting curve")
    );
    assert!(links[0].typed);
}

#[test]
fn golden_body_yields_exactly_references_plus_supports_not_a_double_count() {
    // The typed line also *contains* a wikilink; it must not also count as a
    // bare reference (data-model §2). The golden body has one prose link + one
    // typed line → exactly two edges.
    let body = "Spaced repetition exploits the [[concepts/memory|Human memory]] retrieval curve.\n\n## Relations\n- supports [[concepts/memory|Human memory]] — applies the forgetting curve\n";
    let links = parse_links(body);
    assert_eq!(
        links.len(),
        2,
        "one references (prose) + one supports (typed)"
    );
    let mut types: Vec<&str> = links.iter().map(|l| l.edge_type.as_str()).collect();
    types.sort_unstable();
    assert_eq!(types, vec!["references", "supports"]);
}

#[test]
fn list_item_with_a_bare_link_and_no_verb_is_a_reference() {
    let links = parse_links("- [[concepts/memory|Human memory]]\n");
    assert_eq!(links.len(), 1);
    assert_eq!(links[0].edge_type, "references");
    assert!(!links[0].typed);
}

#[test]
fn prose_list_item_with_a_capitalized_word_is_not_a_typed_edge() {
    // "See" is not a lowercase-kebab verb, so the line is prose → the link is a
    // plain reference, not a typed edge of type "See".
    let links = parse_links("- See [[concepts/memory|Human memory]] for the mechanism\n");
    assert_eq!(links.len(), 1);
    assert_eq!(links[0].edge_type, "references");
    assert!(!links[0].typed);
}

#[test]
fn explanation_after_a_colon_is_supported() {
    let links = parse_links("- supersedes [[notes/old-plan|Old plan]] : replaced after Q2\n");
    assert_eq!(links[0].edge_type, "supersedes");
    assert_eq!(links[0].explanation.as_deref(), Some("replaced after Q2"));
}

#[test]
fn a_tail_verb_is_kept_verbatim() {
    let links = parse_links("- inspired-by [[notes/x|X]]\n");
    assert_eq!(links.len(), 1);
    assert_eq!(links[0].edge_type, "inspired-by");
    assert!(links[0].typed);
    assert_eq!(links[0].explanation, None);
}

#[test]
fn a_link_without_an_alias_keeps_a_none_alias() {
    let links = parse_links("Refer to [[concepts/memory]].\n");
    assert_eq!(links[0].target_path, "concepts/memory");
    assert_eq!(links[0].alias, None);
}
