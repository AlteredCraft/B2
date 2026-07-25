//! The relation vocabulary (data-model.md §2): a closed three-verb
//! stance core — `references` (neutral), `supports` (for), `contradicts`
//! (against) — with display-only inverse labels and symmetry, plus a tolerated
//! tail kept verbatim. The core encodes the one thing embedding similarity
//! cannot infer: stance. It is a relaxable policy, not a structural assumption
//! (invariants.md) — adding a verb here is the whole
//! change.

/// A core relation verb and its display metadata.
pub struct CoreVerb {
    pub verb: &'static str,
    /// The label shown for an *inbound* edge of this type (display only — the
    /// edge is stored once, directed).
    pub inverse: &'static str,
    /// Symmetric verbs are their own inverse and traverse both ways.
    pub symmetric: bool,
}

/// The closed core (data-model.md §2). Order mirrors the doc's table.
pub const CORE: &[CoreVerb] = &[
    CoreVerb {
        verb: "references",
        inverse: "referenced-by",
        symmetric: false,
    },
    CoreVerb {
        verb: "supports",
        inverse: "supported-by",
        symmetric: false,
    },
    CoreVerb {
        verb: "contradicts",
        inverse: "contradicts",
        symmetric: true,
    },
];

/// The core entry for `verb`, if it is a core verb.
pub fn core(verb: &str) -> Option<&'static CoreVerb> {
    CORE.iter().find(|c| c.verb == verb)
}

/// Whether `verb` is part of the closed core.
pub fn is_core(verb: &str) -> bool {
    core(verb).is_some()
}

/// Whether `verb` is symmetric (its own inverse). Tail verbs are treated as
/// directed.
pub fn is_symmetric(verb: &str) -> bool {
    core(verb).is_some_and(|c| c.symmetric)
}

/// The display label for an inbound edge of type `verb`. Core verbs map to their
/// inverse; a tail verb is opaque, so the verb itself is returned (data-model §2).
pub fn inverse_label(verb: &str) -> &str {
    core(verb).map_or(verb, |c| c.inverse)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The core is *closed* and its membership is what `b2 link` validates against
    /// (`Vault::link` refuses a non-core verb). A verb silently joining or leaving
    /// it would change the typing palette, so pin the set itself.
    #[test]
    fn the_core_is_exactly_the_three_stance_verbs() {
        let verbs: Vec<&str> = CORE.iter().map(|c| c.verb).collect();
        assert_eq!(verbs, vec!["references", "supports", "contradicts"]);
        assert!(verbs.iter().all(|v| is_core(v)));
        // The tolerated tail is stored verbatim but is never *core*.
        for tail in ["inspired-by", "supersedes", "Supports", ""] {
            assert!(!is_core(tail), "{tail:?} must not be a core verb");
            assert!(core(tail).is_none());
        }
    }

    /// Stance is the one thing embedding similarity cannot infer, and `contradicts`
    /// is the only verb that reads the same from both ends. Its symmetry is the
    /// single non-trivial fact in this module.
    #[test]
    fn only_contradicts_is_symmetric_and_tail_verbs_are_directed() {
        assert!(is_symmetric("contradicts"));
        assert!(!is_symmetric("references"));
        assert!(!is_symmetric("supports"));
        // An unknown verb is opaque, so it is treated as directed — never guessed
        // symmetric off its spelling.
        assert!(!is_symmetric("inspired-by"));
        assert!(!is_symmetric(""));
    }

    /// Inverse labels are display-only (the edge is stored once, directed): each
    /// core verb has its own, a symmetric verb is its own inverse, and a tail verb
    /// falls back to itself rather than gaining an invented "-by" form.
    #[test]
    fn inverse_labels_cover_core_symmetric_and_tail() {
        assert_eq!(inverse_label("references"), "referenced-by");
        assert_eq!(inverse_label("supports"), "supported-by");
        assert_eq!(inverse_label("contradicts"), "contradicts");
        assert_eq!(inverse_label("inspired-by"), "inspired-by");
        // Every core verb's inverse round-trips through the symmetry flag.
        for c in CORE {
            assert_eq!(
                c.symmetric,
                c.verb == c.inverse,
                "{}: `symmetric` must agree with the inverse label",
                c.verb
            );
        }
    }
}
