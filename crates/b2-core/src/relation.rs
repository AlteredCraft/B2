//! The relation vocabulary (planning/data-model.md §2): a closed three-verb
//! stance core — `references` (neutral), `supports` (for), `contradicts`
//! (against) — with display-only inverse labels and symmetry, plus a tolerated
//! tail kept verbatim. The core encodes the one thing embedding similarity
//! cannot infer: stance. It is a relaxable policy, not a structural assumption
//! (vision-and-scope, design philosophy) — adding a verb here is the whole
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
