import { test } from "node:test";
import assert from "node:assert/strict";
import { strengthBand } from "./strength.ts";

// The landmarks are measured on the eval corpus's labelled populations in the
// stage-2 best-passage unit (GH #182/#192; they began as the retired existence
// gate's bars, kept as landmarks after GH #197 retired the gate) — see
// strength.ts. Each case names the landmark it stands on, so a future
// recalibration has to restate what it is claiming rather than nudge a number.
test("strength: the three bands sit on the calibrated landmarks", () => {
  assert.equal(strengthBand(1.6)?.label, "near match"); // under the 1.96 landmark
  assert.equal(strengthBand(1.96)?.label, "clear match"); // where the corpus's labelled leaders read
  assert.equal(strengthBand(2.529)?.label, "strong match"); // the mate population's upper quartile itself
  assert.equal(strengthBand(6.0)?.label, "strong match"); // dense-vault leaders cap out
});

// The regression GH #182 fixed: the bands were left on the retired centroid z's
// landmarks (3.0 / 2.3) after GH #192 changed the unit under them, which graded
// the corpus's strongest confirmed relation — bicycle.md ↔ bike-maintenance.md,
// the top of the whole labelled-mate population — as a middling two-dot card.
test("strength: the strongest labelled relation reads as strong, not middling", () => {
  assert.equal(strengthBand(2.87)?.glyph, "●●●");
});

test("strength: no z, no band — a statistic that wasn't computed isn't claimed", () => {
  assert.equal(strengthBand(undefined), null);
  assert.equal(strengthBand(null), null);
  assert.equal(strengthBand(Number.NaN), null);
});

test("strength: the tooltip spells the z for whoever wants the number", () => {
  const band = strengthBand(2.13);
  assert.ok(band && band.title.includes("2.1σ"));
  assert.ok(band && band.glyph === "●●○");
});

test("strength: the value is the bare figure, for the card that is selected", () => {
  // The selected card reveals this beside the dots — the same number the tooltip
  // spells out in prose, with none of the prose (hover is not the only way in).
  assert.equal(strengthBand(2.529)?.value, "2.5σ");
  assert.equal(strengthBand(6)?.value, "6.0σ");
});
