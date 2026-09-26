// The editor's commands (editorcmds.ts), as plans over real EditorStates parsed by the
// editor's own language config — the droplink.test.ts rig, for its reason: a mocked tree
// would let a code fence or a nested list go unrecognized here and undetected there.
// The engines underneath (format.ts, list.ts, paste.ts) have their own suites; what is
// pinned here is the adapter's half — which transaction, and when it declines.

import { strict as assert } from "node:assert";
import test from "node:test";
import { markdown, markdownLanguage } from "@codemirror/lang-markdown";
import { EditorState, type TransactionSpec } from "@codemirror/state";
import {
  formatPlan,
  inCodeContext,
  inListItem,
  insertTablePlan,
  listShiftPlan,
  pastePlan,
} from "./editorcmds.ts";
import { BOLD } from "./format.ts";
import { indentList } from "./list.ts";
import { wikilink } from "./livepreview.ts";

const LANG = markdown({ base: markdownLanguage, extensions: [wikilink] });

/** A state with the caret (or selection) where `|` marks are: one bar is a caret, two a
 *  selection between them. */
function at(marked: string): EditorState {
  const first = marked.indexOf("|");
  const second = marked.indexOf("|", first + 1);
  const doc = marked.replace(/\|/g, "");
  const selection =
    second < 0 ? { anchor: first } : { anchor: first, head: second - 1 };
  return EditorState.create({ doc, selection, extensions: [LANG] });
}

const applied = (s: EditorState, spec: TransactionSpec): string => s.update(spec).state.doc.toString();

test("⌘B wraps the selection", () => {
  const s = at("a |word| b");
  assert.equal(applied(s, formatPlan(s, BOLD)), "a **word** b");
});

test("Tab nests a list item under the one above it", () => {
  const s = at("- one\n- |two");
  const plan = listShiftPlan(s, indentList);
  assert.ok(typeof plan !== "boolean", "a transaction");
  assert.equal(applied(s, plan as TransactionSpec), "- one\n  - two");
});

test("Tab outside a list is declined, so it walks the focus ring", () => {
  assert.equal(listShiftPlan(at("just |prose"), indentList), false);
});

test("Tab in a fence is declined before the engine is asked", () => {
  const s = at("```\n- |item\n```");
  assert.equal(inCodeContext(s), true);
  assert.equal(listShiftPlan(s, indentList), false);
});

test("Tab on the first item is claimed and does nothing", () => {
  assert.equal(listShiftPlan(at("- |one\n- two"), indentList), true);
});

test("a list item behind a quote is still a list item to the tree", () => {
  assert.equal(inListItem(at("> - |a")), true);
  assert.equal(inListItem(at("plain |text")), false);
});

test("⌘T drops a table at the caret", () => {
  const s = at("|");
  const doc = applied(s, insertTablePlan(s));
  assert.ok(doc.includes("|"), doc);
  assert.ok(doc.split("\n").length >= 4, "header, rule, two rows");
});

test("rich paste converts formatted HTML, and declines what has nothing to add", () => {
  const s = at("|");
  const plan = pastePlan(s, "<p><strong>bold</strong></p>", "bold");
  assert.ok(plan !== null);
  assert.equal(applied(s, plan), "**bold**");
  assert.equal(pastePlan(s, "", "plain"), null, "no HTML flavor: CodeMirror's paste");
});

test("rich paste keeps its hands off code", () => {
  assert.equal(pastePlan(at("```\n|\n```"), "<p><strong>bold</strong></p>", "bold"), null);
});
