// Exercise the actual main.ts index wiring with controlled IPC and timers. Extracting
// declarations through TypeScript's parser avoids booting the DOM/Tauri shell, without
// copying the scheduling logic into a test double.
import { strict as assert } from "node:assert";
import { readFileSync } from "node:fs";
import test from "node:test";
import { setImmediate } from "node:timers/promises";
import { createContext, runInContext } from "node:vm";
import ts from "typescript";
import { reconcileIndex } from "./reconcile.ts";

const source = ts.createSourceFile(
  "main.ts",
  readFileSync(new URL("./main.ts", import.meta.url), "utf8"),
  ts.ScriptTarget.Latest,
  true,
);
const functions = new Set([
  "trackIndexing", "beginIndexRun", "endIndexRun", "autoIndexOnOpen",
  "scheduleTrailingEmbed", "runTrailingEmbed", "onVaultChanged",
  "reconcileExternalChange", "scheduleAutosave", "saveNow", "runSaveChain",
  "cancelReindex",
  "canAutoEmbed", "drainVaultChanges",
]);
const variables = new Set([
  "indexingRun", "embedTimer", "inFlight", "reconcileInFlight", "reconcilePending",
  "TRAILING_EMBED_MS",
  "AUTOSAVE_MS", "autosaveTimer", "autosavePaused", "trailingDirty", "editorView",
  "reconcileEmbedPending",
]);
const declarations = source.statements.filter((s) =>
  ts.isFunctionDeclaration(s) ? functions.has(s.name?.text ?? "") :
  ts.isVariableStatement(s) && s.declarationList.declarations.some((d) =>
    ts.isIdentifier(d.name) && variables.has(d.name.text)),
);
const code = ts.transpileModule(declarations.map((s) => s.getText(source)).join("\n"), {
  compilerOptions: { target: ts.ScriptTarget.ES2021, module: ts.ModuleKind.None },
}).outputText;

function deferred() {
  let resolve = () => {};
  const promise = new Promise<void>((done) => { resolve = done; });
  return { promise, resolve };
}

function harness() {
  const calls: string[] = [];
  const state = {
    vaultRoot: "/vault" as string | null, semantic: true,
    notesTotal: 2, notesEmbedded: 2, reindexing: false,
    reindexCancelling: false, editing: false, fmEditing: false,
    current: null as { path: string; body: string; revision: string } | null,
    currentResource: null,
  };
  // Disk and indexed contents deliberately start out different despite 2/2 coverage.
  const disk = new Map([["a.md", "edited"], ["new.md", "added"]]);
  let index = new Map([["a.md", "old"], ["deleted.md", "removed"]]);
  let now = 0;
  let nextTimer = 0;
  const timers = new Map<number, { at: number; fn: () => void }>();
  const noOp = () => {};
  const env = {
    state, render: noOp, paintReindex: noOp,
    flash: (message: string) => calls.push(message), errText: String,
    reconcileIndex,
    refreshConnections: async () => {},
    isWriteConflict: () => false,
    api: {
      cancelReindex: async () => {},
      project: async () => {
        calls.push("project");
        if (JSON.stringify([...disk]) !== JSON.stringify([...index])) state.notesEmbedded = 0;
        index = new Map(disk);
        state.notesTotal = index.size;
      },
      writeNote: async (path: string, body: string) => {
        calls.push("save");
        disk.set(path, body);
        index.set(path, body);
        state.notesEmbedded = 0;
        return { revision: "saved" };
      },
    },
    loadNotes: async () => { calls.push("list"); },
    refreshEmbedStatus: async () => { calls.push("coverage"); },
    embedWithProgress: async () => {
      calls.push("embed");
      state.notesEmbedded = state.notesTotal;
      return { cancelled: false };
    },
    refreshOpenNoteAfterIndex: async () => { calls.push("refresh"); },
    refreshDiscovery: async () => { calls.push("discovery"); },
    window: {
      setTimeout: (fn: () => void, delay: number) => {
        const id = ++nextTimer;
        timers.set(id, { at: now + delay, fn });
        return id;
      },
    },
    clearTimeout: (id: number) => { timers.delete(id); },
  };
  const context = createContext(env);
  runInContext(code, context);
  function run(expression: string) { return runInContext(expression, context); }
  async function settle() {
    // Drain promise continuations without advancing the simulated quiet period.
    await setImmediate();
  }
  async function advance(ms: number) {
    now += ms;
    for (const [id, timer] of [...timers]) {
      if (timer.at <= now) { timers.delete(id); timer.fn(); }
    }
    await settle();
  }
  return { state, calls, disk, env, run, advance, settle, indexed: () => [...index] };
}

test("opening a previously complete index reconciles offline edits, adds and deletes", async () => {
  const h = harness();
  await h.run("autoIndexOnOpen(state.vaultRoot)");
  assert.deepEqual(h.indexed(), [...h.disk]);
  assert.equal(h.state.notesEmbedded, h.disk.size);
  assert.ok(h.calls.indexOf("project") < h.calls.indexOf("embed"));
});

test("opening a keyword-only vault also reconciles disk changes", async () => {
  const h = harness();
  h.state.semantic = false;
  h.state.notesEmbedded = 0;
  await h.run("autoIndexOnOpen(state.vaultRoot)");
  assert.deepEqual(h.indexed(), [...h.disk]);
  assert.ok(!h.calls.includes("embed"));
});

test("an unchanged vault is scanned but does not load the embedding model", async () => {
  const h = harness();
  h.disk.clear();
  for (const [path, body] of h.indexed()) h.disk.set(path, body);
  await h.run("autoIndexOnOpen(state.vaultRoot)");
  assert.ok(h.calls.includes("project"));
  assert.ok(!h.calls.includes("embed"));
});

test("a stale vault-open request cannot project the newly selected vault", async () => {
  const h = harness();
  h.state.notesTotal = 0;
  await h.run('autoIndexOnOpen("/departed")');
  assert.deepEqual(h.calls, []);
});

test("filesystem pulses during indexing coalesce and reconcile after it finishes", async () => {
  const h = harness();
  h.run("beginIndexRun()");
  await h.run("onVaultChanged()");
  await h.run("onVaultChanged()");
  assert.ok(!h.calls.includes("project"));
  h.run("endIndexRun()");
  await h.settle();
  assert.deepEqual(h.indexed(), [...h.disk]);
  assert.equal(h.calls.filter((c) => c === "project").length, 1);
  await h.advance(2000);
  assert.equal(h.state.notesEmbedded, h.disk.size);
});

test("repeated save requests embed only after two seconds of quiet", async () => {
  const h = harness();
  h.state.notesEmbedded = 0;
  h.run("scheduleTrailingEmbed()");
  await h.advance(1500);
  h.run("scheduleTrailingEmbed()");
  await h.advance(1500);
  assert.ok(!h.calls.includes("embed"));
  await h.advance(500);
  assert.equal(h.calls.filter((c) => c === "embed").length, 1);
});

test("a delayed save embed survives an active run without replacing its tracked promise", async () => {
  const h = harness();
  const active = deferred();
  Object.assign(h.env, { active: active.promise });
  h.run("beginIndexRun(); trackIndexing(active); globalThis.originalRun = indexingRun");
  h.state.notesEmbedded = 0;
  h.run("scheduleTrailingEmbed()");
  await h.advance(2000);
  assert.equal(h.run("indexingRun === originalRun"), true);
  assert.ok(!h.calls.includes("embed"));
  h.run("endIndexRun()");
  active.resolve();
  await h.advance(2000);
  assert.ok(h.calls.includes("embed"));
});

test("embedding waits for a filesystem projection that is still running", async () => {
  const h = harness();
  const projection = deferred();
  h.env.api.project = async () => { h.calls.push("project"); await projection.promise; };
  h.state.notesEmbedded = 0;
  const reconcile = h.run("onVaultChanged()");
  h.run("scheduleTrailingEmbed()");
  await h.advance(2000);
  assert.ok(!h.calls.includes("embed"));
  projection.resolve();
  await reconcile;
  await h.advance(2000);
  assert.ok(h.calls.includes("embed"));
});

test("a trailing embed refreshes coverage and skips an already complete index", async () => {
  const h = harness();
  h.run("scheduleTrailingEmbed()");
  await h.advance(2000);
  assert.ok(h.calls.includes("coverage"));
  assert.ok(!h.calls.includes("embed"));
});

test("in-app edits save after one idle second and embed two seconds after saving", async () => {
  const h = harness();
  h.state.editing = true;
  h.state.current = { path: "a.md", body: "old", revision: "original" };
  h.run('editorView = { state: { doc: { toString: () => "typed content" } } }');
  h.run("scheduleAutosave()");
  await h.advance(750);
  h.run("scheduleAutosave()");
  await h.advance(750);
  assert.ok(!h.calls.includes("save"));
  await h.advance(250);
  assert.equal(h.disk.get("a.md"), "typed content");
  assert.equal(h.state.current.revision, "saved");
  assert.ok(!h.calls.includes("embed"));
  await h.advance(1999);
  assert.ok(!h.calls.includes("embed"));
  await h.advance(1);
  assert.equal(h.calls.filter((c) => c === "embed").length, 1);
});

test("startup cancellation during projection keeps keyword indexing but skips embedding", async () => {
  const h = harness();
  const project = h.env.api.project;
  h.env.api.project = async () => {
    await project();
    h.state.reindexCancelling = true;
  };
  await h.run("autoIndexOnOpen(state.vaultRoot)");
  assert.deepEqual(h.indexed(), [...h.disk]);
  assert.ok(!h.calls.includes("embed"));
  assert.equal(h.state.reindexing, false);
});

test("a keyword-only external edit projects without attempting to load a model", async () => {
  const h = harness();
  h.state.semantic = false;
  await h.run("onVaultChanged()");
  await h.advance(2000);
  assert.deepEqual(h.indexed(), [...h.disk]);
  assert.ok(!h.calls.includes("embed"));
});

test("Cancel drains queued disk changes but does not restart embedding until another edit", async () => {
  const h = harness();
  h.run("beginIndexRun()");
  await h.run("onVaultChanged()");
  h.run("scheduleTrailingEmbed()");
  await h.run("cancelReindex()");
  h.run("endIndexRun()");
  await h.settle();
  assert.deepEqual(h.indexed(), [...h.disk]);
  await h.advance(2000);
  assert.ok(!h.calls.includes("embed"), "the user's Cancel must stick");
  h.disk.set("later.md", "another edit after cancellation");
  await h.run("onVaultChanged()");
  await h.advance(2000);
  assert.ok(h.calls.includes("embed"));
});

test("Cancel also suppresses embedding from a reconciliation already in flight", async () => {
  const h = harness();
  const projection = deferred();
  const project = h.env.api.project;
  h.env.api.project = async () => {
    await projection.promise;
    await project();
  };
  const reconcile = h.run("onVaultChanged()");
  h.run("beginIndexRun()");
  await h.run("cancelReindex()");
  h.run("endIndexRun()");
  projection.resolve();
  await reconcile;
  assert.deepEqual(h.indexed(), [...h.disk]);
  await h.advance(2000);
  assert.ok(!h.calls.includes("embed"), "an earlier pulse must not undo Cancel");
});
