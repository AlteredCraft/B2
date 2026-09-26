//! The relation vocabulary (ADR-0010): a closed three-verb stance core — `references`
//! (neutral), `supports` (for), `contradicts` (against) — with display-only inverse
//! labels, plus a tolerated tail kept verbatim. The core encodes the one thing embedding
//! similarity cannot infer: stance. Adding a verb here is the whole change.

/// A core relation verb and its display metadata.
pub struct CoreVerb {
    pub verb: &'static str,
    /// The label shown for an *inbound* edge of this type (display only — the
    /// edge is stored once, directed). The symmetric verb, `contradicts`, is its own
    /// inverse (data-model.md §2).
    pub inverse: &'static str,
}

/// The closed core (data-model.md §2). Order mirrors the doc's table.
pub const CORE: &[CoreVerb] = &[
    CoreVerb {
        verb: "references",
        inverse: "referenced-by",
    },
    CoreVerb {
        verb: "supports",
        inverse: "supported-by",
    },
    CoreVerb {
        verb: "contradicts",
        inverse: "contradicts",
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

    /// Inverse labels are display-only (the edge is stored once, directed): each
    /// directed core verb has its own, and a tail verb falls back to itself rather
    /// than gaining an invented "-by" form.
    #[test]
    fn inverse_labels_cover_the_directed_core_and_the_tail() {
        assert_eq!(inverse_label("references"), "referenced-by");
        assert_eq!(inverse_label("supports"), "supported-by");
        assert_eq!(inverse_label("inspired-by"), "inspired-by");
    }

    /// Stance is the one thing embedding similarity cannot infer, and `contradicts`
    /// is the only core verb that reads the same from both ends: its inverse is
    /// itself, and no other core verb's is.
    #[test]
    fn only_contradicts_is_its_own_inverse() {
        assert_eq!(inverse_label("contradicts"), "contradicts");
        let own_inverse: Vec<&str> = CORE
            .iter()
            .filter(|c| c.verb == c.inverse)
            .map(|c| c.verb)
            .collect();
        assert_eq!(own_inverse, vec!["contradicts"]);
    }
}
