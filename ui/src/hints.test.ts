// Chord hints follow the live keyboard (hints.ts, and render.ts's own hints).
//
// Two halves. The first rebinds and reads the hints back: a tooltip that still names the
// shipped chord after a rebind is the bug this module exists to prevent. The second is
// the guard that keeps a new one from being written by hand: it walks every string
// literal in the UI's sources — through TypeScript's own parser, so comments are never
// mistaken for strings — and fails on anything spelled like a chord. The fix for a
// failure is `displayKeys([...])`, never an entry in the exemption list; that list is for
// text that names a key the user *can't* rebind, and each entry says why.

import { strict as assert } from "node:assert";
import { readFileSync, readdirSync } from "node:fs";
import test from "node:test";
import ts from "typescript";
import { DEFAULT_BINDINGS, setActiveBindings } from "./bindings.ts";
import { editDoneTitle, shellHints } from "./hints.ts";
import { applyOverrides } from "./keymap.ts";
import { contextMenuHtml, treePaneHtml } from "./render.ts";
import { shortcuts } from "./shortcuts.ts";
import { state, type AppState } from "./state.ts";

/** Run `fn` with a rebound keyboard installed, and always put the defaults back. */
function rebound(overrides: Record<string, string[]>, fn: () => void): void {
  setActiveBindings(applyOverrides(DEFAULT_BINDINGS, overrides));
  try {
    fn();
  } finally {
    setActiveBindings(DEFAULT_BINDINGS);
  }
}

test("the shell's hints name the shipped chords by default", () => {
  const h = shellHints();
  assert.equal(h["nav-back"].title, "Back (⌘[)");
  assert.equal(h["nav-forward"].title, "Forward (⌘])");
  assert.equal(h["search-input"].placeholder, "Search the vault…  ⇧⌘F");
  assert.equal(h["search-input"].title, "Search the vault — ⇧⌘F (⌘F finds inside the open note)");
  assert.equal(h["open-chat"].title, "Ask your notes (⌘J)");
  assert.equal(h["open-settings"].title, "Settings (⌘,)");
  assert.equal(h["find-prev"].title, "Previous match (⇧⌘G / ⇧⏎)");
  assert.equal(h["find-next"].title, "Next match (⌘G / ⏎)");
  assert.equal(h["find-close"].title, "Close (Esc)");
  assert.equal(editDoneTitle(), "Save and return to reading — ⌘E (⌘S flushes anytime)");
});

test("a rebind moves the shell's hints with it", () => {
  rebound(
    {
      "nav.back": ["Mod-Alt-["],
      "settings.toggle": ["Mod-Alt-,"],
      "search.focus": ["Mod-Alt-f"],
      "edit.toggle": ["Mod-Alt-e"],
    },
    () => {
      const h = shellHints();
      assert.equal(h["nav-back"].title, "Back (⌥⌘[)");
      assert.equal(h["open-settings"].title, "Settings (⌥⌘,)");
      assert.equal(h["search-input"].placeholder, "Search the vault…  ⌥⌘F");
      assert.ok(editDoneTitle().includes("⌥⌘E"), editDoneTitle());
    },
  );
});

test("a rebind moves the tree's and the menu's chords", () => {
  const s: AppState = {
    ...state,
    vaultRoot: "/vault",
    notes: [{ path: "a.md", title: "a" }] as AppState["notes"],
    contextMenu: { kind: "tree", x: 0, y: 0, dir: "", node: { path: "a.md", nodeKind: "note" } },
  } as AppState;
  rebound({ "tree.rename": ["F3"], "tree.new-note": ["Mod-Alt-n"] }, () => {
    const menu = contextMenuHtml(s);
    assert.ok(menu.includes(">F3<"), "Rename names the rebound chord");
    assert.ok(!menu.includes(">F2<"), "and not the shipped one");
    assert.ok(menu.includes(">⌥⌘N<"), "New note names the rebound chord");
    const tree = treePaneHtml(s);
    assert.ok(tree.includes("F3 rename"), "the tree's crib follows the rebind");
    assert.ok(tree.includes("(⌥⌘N)"), "and so does the New note icon");
  });
});

test("the sheet's Settings heading follows the rebind", () => {
  const title = () => shortcuts().find((g) => g.title.startsWith("Settings"))?.title;
  assert.equal(title(), "Settings (⌘,)");
  rebound({ "settings.toggle": ["Mod-Alt-,"] }, () => assert.equal(title(), "Settings (⌥⌘,)"));
});

// --- the guard ------------------------------------------------------------------------

/** A chord as a person would type it into a string: modifier glyphs then a key, or a bare
 *  function key. A lone glyph ("hold ⌘", "⇧ for a bigger step") is prose about a
 *  modifier, not a chord, and doesn't match. */
const CHORD =
  /[⌃⌥⇧⌘]+(?:F\d{1,2}|Enter|Tab|Esc|[A-Za-z0-9,.[\]/;'`=\-⏎⌫⌦←→↑↓])|(?<![A-Za-z0-9])F\d{1,2}(?![A-Za-z0-9])/u;

/** Files that are *allowed* to spell chords: the registry itself, where every chord is
 *  declared. */
const OWNERS = new Set(["bindings.ts"]);

/** Text that names a key no rebind can move. Each entry is matched exactly. */
const EXEMPT = [
  // The sheet's Back/Forward row: the ⌘←/⌘→ aliases, which a rebind leaves alone
  // (`Binding.aliases`).
  "⌘← / ⌘→ too",
  // The chat composer's newline: the textarea's own behaviour, not a binding.
  "⇧⏎ for a new line",
];

function literalTexts(file: string, src: string): { line: number; text: string }[] {
  const sf = ts.createSourceFile(file, src, ts.ScriptTarget.Latest, true);
  const out: { line: number; text: string }[] = [];
  const visit = (n: ts.Node): void => {
    if (
      ts.isStringLiteral(n) ||
      ts.isNoSubstitutionTemplateLiteral(n) ||
      ts.isTemplateHead(n) ||
      ts.isTemplateMiddle(n) ||
      ts.isTemplateTail(n)
    ) {
      out.push({ line: sf.getLineAndCharacterOfPosition(n.getStart()).line + 1, text: n.text });
    }
    ts.forEachChild(n, visit);
  };
  visit(sf);
  return out;
}

test("no UI string spells a chord by hand", () => {
  const dir = new URL(".", import.meta.url);
  const found: string[] = [];
  for (const file of readdirSync(dir)) {
    if (!file.endsWith(".ts") || file.endsWith(".test.ts") || OWNERS.has(file)) continue;
    const src = readFileSync(new URL(file, dir), "utf8");
    for (const { line, text } of literalTexts(file, src)) {
      // An HTML comment inside a template is markup nobody sees.
      let t = text.replace(/<!--[\s\S]*?-->/g, "");
      for (const e of EXEMPT) t = t.split(e).join("");
      const m = t.match(CHORD);
      if (m) found.push(`${file}:${line} spells "${m[0]}" — derive it with displayKeys([...])`);
    }
  }
  assert.deepEqual(found, []);
});

test("the guard's pattern catches what it is for, and not prose about a modifier", () => {
  for (const s of ["(⌘N)", "⇧⌘N", "⌘⏎ saves", "F2 rename", "⇧F10", "⌃Tab cycles", "⌘,", "⌘["])
    assert.ok(CHORD.test(s), s);
  for (const s of ["Hold ⌘ on its own", "⇧ for a bigger step", "<kbd>⌘</kbd>", "F2F", "⏎ open"])
    assert.ok(!CHORD.test(s), s);
});
