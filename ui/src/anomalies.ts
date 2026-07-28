// The GH #81 index anomalies, shaped for *review* (GH #88) — pure data in, rows and
// strings out. No DOM, no IPC, so node runs its test straight off the source
// (`npm test`), like reconcile.ts / shortcuts.ts / treenav.ts.
//
// Why this is its own module rather than the one-line formatter it replaces: the first
// cut flattened every collision and restamp in a pass into a single space-joined
// paragraph and handed it to a 4.5s toast. On a real vault (1122 notes, three unrelated
// duplicate `b2id`s) that reads as one run-on wall naming six files, with no way to
// reach any of them, no way to act on one anomaly without the others, and no way to get
// it back once the timer fires (#88). So the two jobs that formatter conflated are split
// here: a one-line **ping** (`anomalySummary`) short enough for a toast, and the
// **structured rows** (`anomalyRows`) the review panel paints — one anomaly per row,
// each path a control rather than inert text.
//
// Nothing here is stored. Both functions are pure functions of the `ProjectReport` the
// latest pass returned (index-engine.md §8, S2): a notice exists because the vault + the
// index still say so, and it stops existing the moment a pass comes back clean. A panel
// reopened after a restart shows nothing until the next pass re-derives the same anomaly.

import { displayKeys } from "./bindings.ts";
import type { B2idCollision, RestampedNote } from "./types.ts";

/** The slice of a `ProjectReport` the anomaly surfaces read (GH #81). */
export interface IndexAnomalies {
  collisions: B2idCollision[];
  restamped: RestampedNote[];
}

/** The chord that opens the review panel — quoted in the ping, so the toast tells you
 *  how to get the detail back after it clears. Projected from the keyboard registry
 *  (bindings.ts), which is what keeps it equal to the wiring and to the `?` sheet
 *  rather than being a third place the same chord is spelled by hand. */
export const REVIEW_CHORD = displayKeys(["anomalies.toggle"]);

/**
 * One path an anomaly row names, and what B2 can do with it.
 *
 * `indexed` is the whole difference between the two halves of a collision, and the
 * reason the panel can't just wire every path to reveal-in-tree: the file that **kept**
 * the identity has a `notes` row, so it opens and the tree reveals it; a **shadowed**
 * copy has no row — and the tree lists notes from the index, so the copy isn't in the
 * tree either. The honest affordance there is its path, ready to paste into Finder or
 * another editor, which is where the human resolves it anyway (B2 never edits either
 * file of its own accord — W4).
 */
export interface AnomalyPath {
  path: string;
  role: "kept" | "shadowed" | "restamped";
  /** Whether the index holds a row for this path (⇒ it can be opened and revealed). */
  indexed: boolean;
}

/** One reviewable anomaly: what happened, to which files, and what the human can do. */
export interface AnomalyRow {
  kind: "collision" | "restamp";
  /**
   * Identity for the paint — a DOM key that survives a repaint, derived from the
   * anomaly itself (the contested id, or the restamped path) rather than from list
   * position, so a row keeps its identity when an *earlier* anomaly is resolved.
   */
  key: string;
  /** The headline — what kind of anomaly this is. */
  title: string;
  /** What the pass found and what it did about it, in one sentence. */
  detail: string;
  /** What the human can do — B2 surfaces, never fixes (W4). */
  fix: string;
  /** Shadowed/affected paths first: they are what's missing from the index. */
  paths: AnomalyPath[];
}

/** How many anomalies the latest pass surfaced — the badge's count. */
export function anomalyCount(report: IndexAnomalies): number {
  return report.collisions.length + report.restamped.length;
}

/**
 * The review panel's rows — one per anomaly, in report order (the core emits
 * collisions sorted by contested id, so the order is already reproducible).
 */
export function anomalyRows(report: IndexAnomalies): AnomalyRow[] {
  const rows: AnomalyRow[] = [];
  for (const c of report.collisions) {
    const claimants = c.shadowed_paths.length + 1;
    const others = c.shadowed_paths.length === 1 ? "the other stays" : "the others stay";
    // The precedence distinction is the point of surfacing rather than fixing: an
    // incumbent is a confident ruling, a tie-break is only a reproducible one, and the
    // human must be told which they're looking at before they delete anything.
    const detail =
      c.precedence === "incumbent"
        ? `The b2id ${c.b2id} is claimed by ${claimants} files. The index already attributed it to ${c.kept_path}, so that file keeps the identity and ${others} out of the index.`
        : `The b2id ${c.b2id} is claimed by ${claimants} files and the index holds no earlier claim (a fresh or rebuilt index), so path order kept ${c.kept_path}. That is a tie-break for reproducibility, not a ruling about which file is the original.`;
    rows.push({
      kind: "collision",
      key: `collision:${c.b2id}`,
      title: "Duplicate b2id",
      detail,
      fix: "Delete the copy, or remove its b2id: line to give it a fresh identity on the next pass. (Deleting the original instead hands the identity to the copy.) B2 edits neither file on its own.",
      paths: [
        ...c.shadowed_paths.map(
          (path): AnomalyPath => ({ path, role: "shadowed", indexed: false }),
        ),
        { path: c.kept_path, role: "kept", indexed: true },
      ],
    });
  }
  for (const r of report.restamped) {
    rows.push({
      kind: "restamp",
      key: `restamp:${r.path}`,
      title: "New identity stamped",
      detail: `${r.path} carried no b2id: line when this pass read it — removed or blanked outside B2 — so a fresh one was stamped: ${r.old_b2id} → ${r.new_b2id}.`,
      fix: "Links that pointed at the old identity now read as broken. If that wasn't intended, restore the old b2id: line by hand — B2 won't guess, because removing that line is also how you ask for a fresh identity.",
      paths: [{ path: r.path, role: "restamped", indexed: true }],
    });
  }
  return rows;
}

/**
 * A fingerprint of an anomaly set — equal exactly when the same anomalies, over the
 * same files, are still outstanding.
 *
 * The fs-watch pulse re-projects on **every** external change, so an unresolved
 * collision reports again on each unrelated save in the vault. Toasting all of those
 * is noise the badge is already carrying quietly, so the pulse pings only when this
 * changes. Built from the rows (rather than a raw `JSON.stringify`) so it tracks
 * exactly what a human would call "the same anomaly": the contested id or restamped
 * path, plus the files it names — a *third* copy of an already-reported duplicate is
 * news, a second save of an unrelated note is not.
 */
export function anomalyKey(report: IndexAnomalies | null): string {
  if (!report) return "";
  return anomalyRows(report)
    .map((r) => `${r.key}>${r.paths.map((p) => p.path).join(",")}`)
    .join("|");
}

/**
 * The ping: one short line for the reindex flash and the fs-watch pulse, naming *what*
 * needs attention and *how to get to it* — never the detail itself, which is the panel's
 * job and outlives the toast's timer there. `null` on a clean pass (the common case:
 * pulses are frequent and must stay silent).
 */
export function anomalySummary(report: IndexAnomalies): string | null {
  const parts: string[] = [];
  const dup = report.collisions.length;
  const re = report.restamped.length;
  if (dup) parts.push(`${dup} duplicate b2id${dup === 1 ? "" : "s"}`);
  if (re) parts.push(`${re} re-stamped identit${re === 1 ? "y" : "ies"}`);
  if (!parts.length) return null;
  return `⚠ ${parts.join(", ")} — ${REVIEW_CHORD} to review`;
}
