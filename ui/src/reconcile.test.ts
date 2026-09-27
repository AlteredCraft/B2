// The vault-changed reconcile sequencing (reconcile.ts), over injected thunks. Pins the
// pulse re-deriving as much as it invalidates: an external add must be projected before
// the re-list (#65), and the vectors that projection clears must be re-embedded.
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
  // A read before the projection would miss the note that just changed.
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

console.log(`reconcile.test.ts: ${passed} checks passed`);
