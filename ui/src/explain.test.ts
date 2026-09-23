import { test } from "node:test";
import assert from "node:assert/strict";
import {
  explainSummary,
  fieldCaption,
  fieldHelp,
  fieldStrip,
  standingText,
  wholeNoteText,
} from "./explain.ts";
import type { PassagePairView, SimilarExplainView } from "./types.ts";

function pair(z: number | null, identical = false): PassagePairView {
  return {
    anchor: { heading_path: null, text: "a" },
    candidate: { heading_path: null, text: identical ? "a" : "b" },
    score: -0.5,
    z,
    identical,
  };
}

function view(over: Partial<SimilarExplainView> = {}): SimilarExplainView {
  return {
    anchor: { path: "a.md", title: "A" },
    candidate: { path: "b.md", title: "B" },
    limit: 10,
    standing: { kind: "ranked", rank: 2, of: 200, served: true },
    z: 2.1,
    centroid_rank: 2,
    population: [3, 2.1, 1, 0, -1],
    pairs: [pair(2.1)],
    shared_neighbors: [],
    ...over,
  };
}

test("explain: identical text wins over every other reading", () => {
  const s = explainSummary(view({ pairs: [pair(3, true), pair(2.9)] }));
  assert.equal(s?.label, "Identical text");
});

test("explain: an ungraded field gets no label, only facts", () => {
  assert.equal(explainSummary(view({ pairs: [pair(null), pair(null)] })), null);
  assert.equal(explainSummary(view({ pairs: [] })), null);
});

test("explain: the label counts passages at the ●●○ landmark", () => {
  assert.equal(explainSummary(view({ pairs: [pair(1.9), pair(1)] }))?.label, "No passage stands out");
  assert.equal(explainSummary(view({ pairs: [pair(2.0)] }))?.label, "A clear match");
  assert.equal(
    explainSummary(view({ pairs: [pair(3.2), pair(1.1), pair(0.2)] }))?.label,
    "One section matches",
  );
  assert.equal(
    explainSummary(view({ pairs: [pair(3.2), pair(2.0), pair(0.2), pair(0)] }))?.label,
    "Broadly similar",
  );
  assert.equal(
    explainSummary(view({ pairs: [pair(3.2), pair(2.0), pair(0.2), pair(0), pair(-1)] }))?.label,
    "A few sections match",
  );
  // The landmark is inclusive, as the band's is.
  assert.equal(explainSummary(view({ pairs: [pair(1.96), pair(0)] }))?.label, "One section matches");
});

test("explain: the standing says why a note is, or isn't, a card", () => {
  assert.equal(standingText(view()), "Card #2 of the 10 shown · 200 notes compared");
  // A small vault shows fewer cards than the limit: the count is what the list shows.
  assert.equal(
    standingText(view({ standing: { kind: "ranked", rank: 2, of: 5, served: true } })),
    "Card #2 of the 5 shown · 5 notes compared",
  );
  assert.equal(
    standingText(view({ standing: { kind: "ranked", rank: 34, of: 200, served: false } })),
    "Ranked #34 of 200 compared, past the 10 shown",
  );
  assert.match(standingText(view({ standing: { kind: "linked" } })), /Already linked/);
  assert.match(standingText(view({ standing: { kind: "unembedded" } })), /Not embedded/);
  assert.match(
    standingText(view({ standing: { kind: "not_shortlisted", shortlist: 200 }, centroid_rank: 412 })),
    /#412, past the first 200/,
  );
});

test("explain: whole-note rank is pointed out only when it differs", () => {
  assert.equal(wholeNoteText(view()), null);
  assert.equal(
    wholeNoteText(view({ centroid_rank: 23, standing: { kind: "ranked", rank: 1, of: 200, served: true } })),
    "Judged as a whole note it ranks #23. Its best passage ranks it #1.",
  );
  assert.equal(wholeNoteText(view({ standing: { kind: "linked" }, centroid_rank: null })), null);
});

test("explain: the strip always spans both landmarks, so a compressed field reads as one", () => {
  const s = fieldStrip([1.2, 1.1, 1.0, 0.9], 1.2);
  assert.ok(s);
  assert.ok(s.strong < 1 && s.clear < s.strong, "both marks are on the strip");
  assert.ok(s.dots.every((d) => d < s.clear), "a field with nothing clear sits left of ●●○");
  assert.equal(s.marker, s.dots[0]);
  assert.ok([...s.dots, s.marker ?? 0, s.clear, s.strong].every((x) => x >= 0 && x <= 1));
});

test("explain: a wide field stretches the strip instead of spilling past it", () => {
  const s = fieldStrip([6.5, 1, -2.5], 6.5);
  assert.ok(s);
  assert.equal(s.marker, 1);
  assert.equal(Math.min(...s.dots), 0);
});

test("explain: no population, no strip", () => {
  assert.equal(fieldStrip([], null), null);
});

test("explain: the caption names whose strip it is and counts each band once", () => {
  assert.equal(
    fieldCaption([3, 2.6, 2.0, 1.0, 0], "12 Factor App"),
    "The 5 notes closest to 12 Factor App · 2 at ●●● · 1 at ●●○ · 2 below",
  );
});

test("explain: the ? names both notes and says what the dashed lines are", () => {
  const help = fieldHelp(
    view({
      anchor: { path: "12.md", title: "12 Factor App" },
      candidate: { path: "ab.md", title: "Architecture bible" },
    }),
  ).join(" ");
  assert.match(help, /closest to 12 Factor App/);
  assert.match(help, /blue line is Architecture bible/);
  assert.match(help, /fixed cut-offs, the same on every note/);
  assert.match(help, /1\.96σ/);
  assert.match(help, /2\.52σ/);
  assert.match(help, /don’t decide what is listed/);
  assert.match(help, /from Architecture bible’s side/);
});

test("explain: two notes with one title are told apart by path", () => {
  const help = fieldHelp(
    view({
      anchor: { path: "a/polish.md", title: "polish" },
      candidate: { path: "b/polish.md", title: "polish" },
    }),
  ).join(" ");
  assert.match(help, /closest to a\/polish\.md/);
  assert.match(help, /blue line is b\/polish\.md/);
});

test("explain: an untitled note is named by its path", () => {
  const help = fieldHelp(view({ anchor: { path: "a.md", title: null } })).join(" ");
  assert.match(help, /closest to a\.md/);
});

test("explain: the strip labels whole σ steps, with 0 (the average) always on it", () => {
  const s = fieldStrip([4.0, 3.1, 1.2, 0.3, -0.4, -1.7], 4.0);
  assert.ok(s);
  assert.deepEqual(
    s.ticks.map((t) => t.label),
    ["−1σ", "0σ", "1σ", "2σ", "3σ", "4σ"],
  );
  const zero = s.ticks.find((t) => t.label === "0σ");
  assert.ok(zero && zero.at > 0 && zero.at < 1);
  assert.ok(s.ticks.every((t, i, all) => i === 0 || t.at > all[i - 1].at), "left to right");
  assert.ok(s.ticks.every((t) => t.at >= 0 && t.at <= 1), "on the strip");
});

test("explain: a very wide strip labels every other σ so the labels don't collide", () => {
  const s = fieldStrip([9.5, 0, -2.2], 9.5);
  assert.ok(s);
  assert.deepEqual(
    s.ticks.map((t) => t.label),
    ["−2σ", "0σ", "2σ", "4σ", "6σ", "8σ"],
  );
});
