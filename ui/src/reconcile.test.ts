// The vault-changed reconcile sequencing (reconcile.ts), pinned. Pure async
// orchestration over injected thunks — no DOM, no IPC — so node runs it straight off
// the source via its native type-stripping: `npm test`. Dependency-free like
// embedreminder.test.ts (hand-rolled assert; no @types/node).
//
// The two bugs this pins against, both "the pulse re-derived less than it invalidated":
//   • #65 dogfood, item 4 — a Finder-dropped file pulsed `vault-changed`, but the
//     reconcile only re-*listed* the index, never re-derived it, so the new file had no
//     row and the tree didn't change until a manual reindex.
//   • the discovery half — the projection that fixed that *clears* a re-chunked note's
//     vectors and centroid (`db::replace_chunks`) and, being model-free, never refills
//     them. An externally edited note came back with an empty Similar pane, and stayed
//     that way until the human reindexed by hand.
import { reconcileIndex } from "./reconcile.ts";

let passed = 0;

function assert(cond: boolean, msg: string): void {
  if (!cond) throw new Error(`assertion failed: ${msg}`);
}
async function check(name: string, fn: () => Promise<void>): Promise<void> {
  await fn();
  passed++;
  console.log(`  ok  ${name}`);
}

/** The heal deps, defaulted to "nothing owed" so a test opts into the vector half. */
function deps(over: Partial<Parameters<typeof reconcileIndex>[0]> = {}) {
  return {
    reindexing: false,
    project: async () => ({}),
    list: async () => {},
    vectorsPending: async () => false,
    healVectors: () => {},
    ...over,
  };
}

await check("projects the vault BEFORE re-listing (the Finder-drop case)", async () => {
  const calls: string[] = [];
  await reconcileIndex(
    deps({
      project: async () => {
        calls.push("project");
      },
      list: async () => {
        calls.push("list");
      },
    }),
  );
  assert(
    calls.join(",") === "project,list",
    `a dropped file must be projected into the index before the tree re-lists (got: ${calls.join(",")})`,
  );
});

await check("skips projection while a reindex is in flight, but still lists", async () => {
  const calls: string[] = [];
  await reconcileIndex(
    deps({
      reindexing: true,
      project: async () => {
        calls.push("project");
      },
      list: async () => {
        calls.push("list");
      },
    }),
  );
  assert(
    calls.join(",") === "list",
    `an in-flight reindex owns the index — reconcile must only re-list (got: ${calls.join(",")})`,
  );
});

await check("a failed projection still refreshes the list (best-effort)", async () => {
  const calls: string[] = [];
  await reconcileIndex(
    deps({
      project: async () => {
        throw new Error("project refused");
      },
      list: async () => {
        calls.push("list");
      },
    }),
  );
  assert(
    calls.join(",") === "list",
    "projection is a background hum — its failure must not kill the tree refresh",
  );
});

await check("a failed list propagates (callers already own that error path)", async () => {
  let threw = false;
  try {
    await reconcileIndex(
      deps({
        list: async () => {
          throw new Error("no vault");
        },
      }),
    );
  } catch {
    threw = true;
  }
  assert(threw, "the list thunk's error contract (loadNotes' toast-and-false) stays the caller's");
});

// --- the vector heal: the projection clears what only an embed can put back ---------

await check("schedules the trailing embed when the projection left vectors owed", async () => {
  const calls: string[] = [];
  await reconcileIndex(
    deps({
      project: async () => {
        calls.push("project");
      },
      list: async () => {
        calls.push("list");
      },
      vectorsPending: async () => {
        calls.push("pending?");
        return true;
      },
      healVectors: () => {
        calls.push("heal");
      },
    }),
  );
  assert(
    calls.join(",") === "project,list,pending?,heal",
    `an externally edited note's vectors die in the projection and only the embed puts them back (got: ${calls.join(",")})`,
  );
});

await check("asks about coverage AFTER projecting, never before", async () => {
  // The projection is what clears the vectors, so a coverage read taken before it
  // would answer about the *old* index and miss exactly the note that just changed.
  let projected = false;
  let askedBeforeProject = false;
  await reconcileIndex(
    deps({
      project: async () => {
        projected = true;
      },
      vectorsPending: async () => {
        if (!projected) askedBeforeProject = true;
        return false;
      },
    }),
  );
  assert(!askedBeforeProject, "the pending set is only honest once the projection has run");
});

await check("schedules no embed when nothing is missing a vector", async () => {
  let healed = 0;
  await reconcileIndex(
    deps({
      vectorsPending: async () => false,
      healVectors: () => {
        healed++;
      },
    }),
  );
  assert(healed === 0, "a quiescent pulse must not load the model to embed nothing");
});

await check("heals after a FAILED projection too (the pending set is DB-derived)", async () => {
  let healed = 0;
  await reconcileIndex(
    deps({
      project: async () => {
        throw new Error("half-projected");
      },
      vectorsPending: async () => true,
      healVectors: () => {
        healed++;
      },
    }),
  );
  assert(healed === 1, "vectors owed are owed whether or not this pass got that far");
});

await check("never heals under an in-flight reindex (that run owns the embed)", async () => {
  const calls: string[] = [];
  await reconcileIndex(
    deps({
      reindexing: true,
      vectorsPending: async () => {
        calls.push("pending?");
        return true;
      },
      healVectors: () => {
        calls.push("heal");
      },
    }),
  );
  assert(calls.length === 0, `a reindex embeds its own pending set (got: ${calls.join(",")})`);
});

await check("a failed coverage read is a hum, not a reconcile failure", async () => {
  let threw = false;
  try {
    await reconcileIndex(
      deps({
        vectorsPending: async () => {
          throw new Error("vault_info failed");
        },
      }),
    );
  } catch {
    threw = true;
  }
  assert(!threw, "coverage is a hint — the tree refresh already landed and must stand");
});

// --- the GH #81 anomaly surfacing: the onReport plumbing ------------------------
//
// Only the plumbing lives here. What the report is *rendered* into — the ping and the
// review panel's rows — moved to anomalies.ts when #88 grew it past one sentence, and
// anomalies.test.ts pins that wording.

await check("hands the projection's report to onReport (the pulse notice path)", async () => {
  const seen: unknown[] = [];
  const report = { collisions: [], restamped: [] };
  await reconcileIndex(
    deps({
      project: async () => report,
      onReport: (r) => {
        seen.push(r);
      },
    }),
  );
  assert(seen.length === 1 && seen[0] === report, "onReport must receive the resolved report");
});

await check("no onReport while a reindex owns the index, or when projection fails", async () => {
  let called = 0;
  await reconcileIndex(
    deps({
      reindexing: true,
      onReport: () => {
        called++;
      },
    }),
  );
  await reconcileIndex(
    deps({
      project: async () => {
        throw new Error("refused");
      },
      onReport: () => {
        called++;
      },
    }),
  );
  assert(called === 0, "a skipped or failed projection has no report to surface");
});

console.log(`reconcile.test.ts: ${passed} checks passed`);
