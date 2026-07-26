// The vault-changed reconcile sequencing (reconcile.ts), pinned. Pure async
// orchestration over injected thunks — no DOM, no IPC — so node runs it straight off
// the source via its native type-stripping: `npm test`. Dependency-free like
// embedreminder.test.ts (hand-rolled assert; no @types/node).
//
// The bug this pins against (#65 dogfood, item 4): a Finder-dropped file pulsed
// `vault-changed`, but the reconcile only re-*listed* the index — it never re-derived
// it — so the new file had no row and the tree didn't change until a manual reindex.
import { reprojectThenList } from "./reconcile.ts";

let passed = 0;

function assert(cond: boolean, msg: string): void {
  if (!cond) throw new Error(`assertion failed: ${msg}`);
}
async function check(name: string, fn: () => Promise<void>): Promise<void> {
  await fn();
  passed++;
  console.log(`  ok  ${name}`);
}

await check("projects the vault BEFORE re-listing (the Finder-drop case)", async () => {
  const calls: string[] = [];
  await reprojectThenList({
    reindexing: false,
    project: async () => {
      calls.push("project");
    },
    list: async () => {
      calls.push("list");
    },
  });
  assert(
    calls.join(",") === "project,list",
    `a dropped file must be projected into the index before the tree re-lists (got: ${calls.join(",")})`,
  );
});

await check("skips projection while a reindex is in flight, but still lists", async () => {
  const calls: string[] = [];
  await reprojectThenList({
    reindexing: true,
    project: async () => {
      calls.push("project");
    },
    list: async () => {
      calls.push("list");
    },
  });
  assert(
    calls.join(",") === "list",
    `an in-flight reindex owns the index — reconcile must only re-list (got: ${calls.join(",")})`,
  );
});

await check("a failed projection still refreshes the list (best-effort)", async () => {
  const calls: string[] = [];
  await reprojectThenList({
    reindexing: false,
    project: async () => {
      throw new Error("project refused");
    },
    list: async () => {
      calls.push("list");
    },
  });
  assert(
    calls.join(",") === "list",
    "projection is a background hum — its failure must not kill the tree refresh",
  );
});

await check("a failed list propagates (callers already own that error path)", async () => {
  let threw = false;
  try {
    await reprojectThenList({
      reindexing: false,
      project: async () => {},
      list: async () => {
        throw new Error("no vault");
      },
    });
  } catch {
    threw = true;
  }
  assert(threw, "the list thunk's error contract (loadNotes' toast-and-false) stays the caller's");
});

// --- the GH #81 anomaly surfacing: the onReport plumbing ------------------------
//
// Only the plumbing lives here. What the report is *rendered* into — the ping and the
// review panel's rows — moved to anomalies.ts when #88 grew it past one sentence, and
// anomalies.test.ts pins that wording.

await check("hands the projection's report to onReport (the pulse notice path)", async () => {
  const seen: unknown[] = [];
  const report = { collisions: [], restamped: [] };
  await reprojectThenList({
    reindexing: false,
    project: async () => report,
    list: async () => {},
    onReport: (r) => {
      seen.push(r);
    },
  });
  assert(seen.length === 1 && seen[0] === report, "onReport must receive the resolved report");
});

await check("no onReport while a reindex owns the index, or when projection fails", async () => {
  let called = 0;
  await reprojectThenList({
    reindexing: true,
    project: async () => ({}),
    list: async () => {},
    onReport: () => {
      called++;
    },
  });
  await reprojectThenList({
    reindexing: false,
    project: async () => {
      throw new Error("refused");
    },
    list: async () => {},
    onReport: () => {
      called++;
    },
  });
  assert(called === 0, "a skipped or failed projection has no report to surface");
});


console.log(`reconcile.test.ts: ${passed} checks passed`);
