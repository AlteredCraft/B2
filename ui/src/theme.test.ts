// The appearance preference (theme.ts): what it may be, how <html> carries it, and its
// storage — including storage that isn't there (node has none, which is the shape of a
// browser refusing it in private mode) and a stored value that was hand-edited.

import { strict as assert } from "node:assert";
import test from "node:test";
import { isThemePref, loadThemePref, saveThemePref, themeAttr } from "./theme.ts";

/** Run `fn` with a working in-memory localStorage installed, then take it away again. */
function withStorage(seed: Record<string, string>, fn: (store: Map<string, string>) => void): void {
  const store = new Map(Object.entries(seed));
  const g = globalThis as unknown as { localStorage?: unknown };
  g.localStorage = {
    getItem: (k: string) => store.get(k) ?? null,
    setItem: (k: string, v: string) => void store.set(k, v),
  };
  try {
    fn(store);
  } finally {
    delete g.localStorage;
  }
}

test("only the three preferences are preferences", () => {
  for (const t of ["system", "light", "dark"]) assert.ok(isThemePref(t), t);
  for (const t of [null, "", "Dark", "sepia"]) assert.ok(!isThemePref(t), String(t));
});

test("system follows the OS: no attribute; the others pin one", () => {
  assert.equal(themeAttr("system"), null);
  assert.equal(themeAttr("light"), "light");
  assert.equal(themeAttr("dark"), "dark");
});

test("no storage at all is System, and saving into none is not an error", () => {
  assert.equal(loadThemePref(), "system");
  saveThemePref("dark");
});

test("a saved choice comes back; a hand-edited one is System", () => {
  withStorage({}, (store) => {
    saveThemePref("dark");
    assert.equal(store.get("b2:theme"), "dark");
    assert.equal(loadThemePref(), "dark");
  });
  withStorage({ "b2:theme": "sepia" }, () => assert.equal(loadThemePref(), "system"));
});
