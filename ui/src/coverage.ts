// How much of the vault semantic ranking can see: the one classifier behind every surface
// that reports it (#26). The copy is each surface's own, and so is the order it asks in,
// which is why this returns the model and embedding facts separately.

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
  /** Notes embedded, and notes in all. */
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
