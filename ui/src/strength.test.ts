import { test } from "node:test";
import assert from "node:assert/strict";
import { strengthBand } from "./strength.ts";

// Each case names the landmark it stands on (strength.ts), so a recalibration restates
// its claim rather than nudging a number.
test("strength: the three bands sit on the calibrated landmarks", () => {
  assert.equal(strengthBand(1.6)?.label, "near match"); // under the 1.96 landmark
  assert.equal(strengthBand(1.96)?.label, "clear match"); // where the corpus's labelled leaders read
  assert.equal(strengthBand(2.529)?.label, "strong match"); // the mate population's upper quartile itself
  assert.equal(strengthBand(6.0)?.label, "strong match"); // dense-vault leaders cap out
});

// GH #182: the corpus's strongest confirmed relation (bicycle.md ↔ bike-maintenance.md).
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
  // The selected card reveals this beside the dots (K1).
  assert.equal(strengthBand(2.529)?.value, "2.5σ");
  assert.equal(strengthBand(6)?.value, "6.0σ");
});
