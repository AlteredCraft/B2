// How much of the vault semantic ranking can see — the one classifier behind every
// surface that has to be honest about it (#26).
//
// Five surfaces say something about embedding coverage: the search caveat, the Similar
// section's empty state, the graph's ghost hint, Settings → Index, and chat's retrieval
// note. Each used to re-derive the tiers from the three raw numbers, and they didn't all
// agree on where the lines fell. The *reading* is shared, so it lives here; the *copy* is
// each surface's own, and so is the order it asks in — chat says nothing about an empty
// vault even without a model, while search names the missing model first. That is why
// this returns both facts rather than one collapsed tier: a surface picks which one it
// asks about first, and the classifier never has to know.
//
// Pure, so node tests it straight off the source (`npm test`).

/** How much of the vault has vectors, whatever the model's state. */
export type Embedded =
  | "empty" // nothing projected yet: no notes to embed
  | "none" // notes, none embedded
  | "partial" // some embedded — the vector half is still filling, or a run stopped short
  | "all"; // every note embedded — semantic ranking sees the whole vault

export interface Coverage {
  /** Is the real embedding model installed (`VaultInfo.semantic`)? */
  readonly model: boolean;
  readonly embedded: Embedded;
  /** Notes embedded, and notes in all — for the surfaces that print the fraction. */
  readonly n: number;
  readonly m: number;
}

/** Read a vault's coverage off the three numbers `VaultInfo` carries. */
export function coverage(s: { semantic: boolean; notesEmbedded: number; notesTotal: number }): Coverage {
  const n = s.notesEmbedded;
  const m = s.notesTotal;
  const embedded: Embedded = m === 0 ? "empty" : n >= m ? "all" : n === 0 ? "none" : "partial";
  return { model: s.semantic, embedded, n, m };
}
