import { test } from "node:test";
import assert from "node:assert/strict";
import { strengthBand } from "./strength.ts";

test("strength: the three bands sit on the calibrated landmarks", () => {
  assert.equal(strengthBand(1.9)?.label, "near match"); // just cleared the floor
  assert.equal(strengthBand(2.3)?.label, "clear match");
  assert.equal(strengthBand(3.0)?.label, "strong match");
  assert.equal(strengthBand(6.0)?.label, "strong match"); // dense-vault leaders cap out
});

test("strength: no z, no band — a statistic that wasn't computed isn't claimed", () => {
  assert.equal(strengthBand(undefined), null);
  assert.equal(strengthBand(null), null);
  assert.equal(strengthBand(Number.NaN), null);
});

test("strength: the tooltip spells the z for whoever wants the number", () => {
  const band = strengthBand(2.53);
  assert.ok(band && band.title.includes("2.5σ"));
  assert.ok(band && band.glyph === "●●○");
});

test("strength: the value is the bare figure, for the card that is selected", () => {
  // The selected card reveals this beside the dots — the same number the tooltip
  // spells out in prose, with none of the prose (hover is not the only way in).
  assert.equal(strengthBand(2.53)?.value, "2.5σ");
  assert.equal(strengthBand(6)?.value, "6.0σ");
});
