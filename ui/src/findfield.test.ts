// Find-in-note's editor engine (findfield.ts), on real EditorStates — the droplink.test.ts
// rig. What's pinned is the field's contract with the bar: an effect sets the query and
// the wanted active match, a doc edit re-derives the matches and keeps the active one
// where the user was, and null closes it.

import { strict as assert } from "node:assert";
import test from "node:test";
import { EditorState } from "@codemirror/state";
import { findDecorations, findField, setFindEffect } from "./findfield.ts";

const doc = (text: string) => EditorState.create({ doc: text, extensions: [findField] });
const find = (s: EditorState) => s.field(findField);

test("closed until a query arrives", () => {
  assert.equal(find(doc("cat cat")), null);
});

test("an effect sets the query, finds every match, and clamps the active one", () => {
  const s = doc("cat dog cat").update({ effects: setFindEffect.of({ query: "cat", active: 5 }) }).state;
  const f = find(s);
  assert.equal(f?.query, "cat");
  assert.deepEqual(f?.matches.map((m) => m.from), [0, 8]);
  assert.equal(f?.active, 1, "clamped to the last match");
});

test("no match is active -1", () => {
  const s = doc("cat").update({ effects: setFindEffect.of({ query: "dog", active: 0 }) }).state;
  assert.equal(find(s)?.active, -1);
});

test("a doc edit re-derives the matches and re-anchors the active one", () => {
  let s = doc("cat dog cat").update({ effects: setFindEffect.of({ query: "cat", active: 1 }) }).state;
  // Typing at the start shifts both matches; the active one follows the one it was on.
  s = s.update({ changes: { from: 0, insert: "a " } }).state;
  const f = find(s);
  assert.deepEqual(f?.matches.map((m) => m.from), [2, 10]);
  assert.equal(f?.active, 1);
  // A new match typed in counts at once.
  s = s.update({ changes: { from: s.doc.length, insert: " cat" } }).state;
  assert.equal(find(s)?.matches.length, 3);
});

test("a transaction that neither edits nor sets leaves the value as it was", () => {
  const s = doc("cat").update({ effects: setFindEffect.of({ query: "cat", active: 0 }) }).state;
  const after = s.update({ selection: { anchor: 1 } }).state;
  assert.equal(find(after), find(s));
});

test("null closes it", () => {
  let s = doc("cat").update({ effects: setFindEffect.of({ query: "cat", active: 0 }) }).state;
  s = s.update({ effects: setFindEffect.of(null) }).state;
  assert.equal(find(s), null);
});

test("every match paints, and the active one is marked apart", () => {
  const s = doc("cat dog cat").update({ effects: setFindEffect.of({ query: "cat", active: 0 }) }).state;
  const classes: string[] = [];
  findDecorations(find(s)).between(0, s.doc.length, (_from, _to, d) => {
    classes.push(String(d.spec.class));
  });
  assert.deepEqual(classes, ["find-match is-active", "find-match"]);
  assert.equal(findDecorations(null).size, 0);
});
