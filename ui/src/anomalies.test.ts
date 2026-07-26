// The anomaly review surface's pure half (anomalies.ts), pinned. No DOM, no IPC — node
// runs it straight off the source via its native type-stripping: `npm test`.
// Dependency-free like reconcile.test.ts (hand-rolled assert; no @types/node).
//
// What these pin is a *wording and shape* contract, and the reason it's worth pinning is
// #88: the surface these anomalies get is the only thing standing between a duplicated
// note and a human who never learns it stopped being indexed. Two properties matter most
// — the ping stays one short line no matter how many anomalies a pass found (a toast
// can't carry more), and every row says which of its paths B2 can actually open (a
// shadowed copy has no index row, so promising to reveal it would be a lie).

import {
  anomalyCount,
  anomalyKey,
  anomalyRows,
  anomalySummary,
  REVIEW_CHORD,
  type IndexAnomalies,
} from "./anomalies.ts";
import type { B2idCollision, RestampedNote } from "./types.ts";

let passed = 0;

function assert(cond: boolean, msg: string): void {
  if (!cond) throw new Error(`assertion failed: ${msg}`);
}
function check(name: string, fn: () => void): void {
  fn();
  passed++;
  console.log(`  ok  ${name}`);
}

const CLEAN: IndexAnomalies = { collisions: [], restamped: [] };

function collision(over: Partial<B2idCollision> = {}): B2idCollision {
  return {
    b2id: "01ABC",
    kept_path: "concepts/memory.md",
    precedence: "incumbent",
    shadowed_paths: ["concepts/memory copy.md"],
    ...over,
  };
}
function restamp(over: Partial<RestampedNote> = {}): RestampedNote {
  return { path: "notes/spaced-repetition.md", old_b2id: "01OLD", new_b2id: "01NEW", ...over };
}

// --- the ping ------------------------------------------------------------------------

check("a clean pass is silent — no ping, no rows, no count", () => {
  assert(anomalySummary(CLEAN) === null, "a clean pass must not toast (pulses are frequent)");
  assert(anomalyRows(CLEAN).length === 0, "a clean pass has nothing to review");
  assert(anomalyCount(CLEAN) === 0, "a clean pass shows no badge");
});

check("the ping counts each kind and points at the review chord", () => {
  const msg = anomalySummary({
    collisions: [collision(), collision({ b2id: "01DEF" })],
    restamped: [restamp()],
  });
  assert(msg !== null, "anomalies must ping");
  assert(msg!.includes("2 duplicate b2ids"), `must count collisions: ${msg}`);
  assert(msg!.includes("1 re-stamped identity"), `must count restamps: ${msg}`);
  assert(msg!.includes(REVIEW_CHORD), `must say how to get the detail back: ${msg}`);
});

check("the ping singularizes both kinds", () => {
  const one = anomalySummary({ collisions: [collision()], restamped: [] });
  assert(one!.includes("1 duplicate b2id,") === false, `no trailing comma alone: ${one}`);
  assert(one!.includes("1 duplicate b2id"), `singular collision: ${one}`);
  assert(!one!.includes("b2ids"), `must not pluralize a single collision: ${one}`);
  const many = anomalySummary({ collisions: [], restamped: [restamp(), restamp()] });
  assert(many!.includes("2 re-stamped identities"), `plural restamp: ${many}`);
});

// The regression this file exists for: three anomalies used to produce three sentences
// naming six files, joined with spaces, in a toast that cleared on a 4.5s timer.
check("the ping stays one short line no matter how many anomalies a pass found", () => {
  const msg = anomalySummary({
    collisions: [
      collision({ b2id: "01A", shadowed_paths: ["a copy.md"] }),
      collision({ b2id: "01B", shadowed_paths: ["b1.md", "b2.md"] }),
      collision({ b2id: "01C", shadowed_paths: ["c copy.md"] }),
    ],
    restamped: [restamp(), restamp({ path: "notes/other.md" })],
  })!;
  assert(!msg.includes("\n"), "one line");
  assert(msg.length < 80, `short enough for a toast — got ${msg.length} chars: ${msg}`);
  assert(!msg.includes("a copy.md"), "the ping names no paths — that's the panel's job");
});

// --- the rows ------------------------------------------------------------------------

check("a collision row names the shadowed copy first, then the keeper", () => {
  const [row] = anomalyRows({ collisions: [collision()], restamped: [] });
  assert(row.kind === "collision", "kind");
  assert(row.key === "collision:01ABC", `keyed by the contested id, not list position: ${row.key}`);
  assert(row.paths.length === 2, "both halves of the collision are shown");
  assert(row.paths[0].path === "concepts/memory copy.md", "the un-indexed copy leads");
  assert(row.paths[0].role === "shadowed" && !row.paths[0].indexed, "a shadowed copy has no row");
  assert(row.paths[1].role === "kept" && row.paths[1].indexed, "the keeper is openable");
});

check("an incumbent ruling and a tie-break read differently", () => {
  const [inc] = anomalyRows({ collisions: [collision()], restamped: [] });
  assert(inc.detail.includes("already attributed"), `incumbent states the claim: ${inc.detail}`);
  const [tie] = anomalyRows({
    collisions: [collision({ precedence: "tie_break" })],
    restamped: [],
  });
  assert(tie.detail.includes("path order"), `tie-break names its rule: ${tie.detail}`);
  assert(
    tie.detail.includes("not a ruling"),
    `a tie-break must not read as a verdict about the original: ${tie.detail}`,
  );
});

check("every row carries the fix, and the fix is never automatic (W4)", () => {
  const rows = anomalyRows({ collisions: [collision()], restamped: [restamp()] });
  assert(rows.length === 2, "one row per anomaly");
  for (const row of rows) assert(row.fix.length > 0, `${row.kind} must say what to do`);
  assert(rows[0].fix.includes("b2id: line"), "the collision fix names the gesture");
  assert(rows[1].fix.includes("won't guess"), "the restamp fix says why B2 leaves it alone");
});

check("a restamp row reports old → new and the broken links it caused", () => {
  const [row] = anomalyRows({ collisions: [], restamped: [restamp()] });
  assert(row.key === "restamp:notes/spaced-repetition.md", `keyed by path: ${row.key}`);
  assert(row.detail.includes("01OLD") && row.detail.includes("01NEW"), "both identities");
  assert(row.paths[0].indexed, "the restamped note IS indexed — it opens");
  assert(row.fix.includes("broken"), "says what it cost");
});

check("row keys are unique across a multi-anomaly pass", () => {
  const rows = anomalyRows({
    collisions: [collision({ b2id: "01A" }), collision({ b2id: "01B" })],
    restamped: [restamp({ path: "x.md" }), restamp({ path: "y.md" })],
  });
  assert(new Set(rows.map((r) => r.key)).size === 4, "each row keeps its own identity");
  assert(anomalyCount({ collisions: [collision()], restamped: [restamp()] }) === 2, "badge count");
});

// --- the pulse's "is this news?" test -------------------------------------------------

check("the same outstanding anomalies fingerprint the same across passes", () => {
  const a = { collisions: [collision()], restamped: [restamp()] };
  const b = { collisions: [collision()], restamped: [restamp()] };
  assert(anomalyKey(a) === anomalyKey(b), "an unchanged set must not re-ping every pulse");
  assert(anomalyKey(null) === anomalyKey(CLEAN), "no report and a clean one are both nothing");
  assert(anomalyKey(a) !== anomalyKey(CLEAN), "a resolved set IS a change");
});

check("a new claimant on an already-reported collision is news", () => {
  const one = { collisions: [collision()], restamped: [] };
  const two = {
    collisions: [collision({ shadowed_paths: ["concepts/memory copy.md", "concepts/m2.md"] })],
    restamped: [],
  };
  assert(anomalyKey(one) !== anomalyKey(two), "a third copy of the same id must ping again");
});

console.log(`\n${passed} anomaly checks passed`);
