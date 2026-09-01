// The launch ground (GH #225), pinned against the stylesheet it duplicates.
//
// `index.html` carries B2's two `--bg` values inline so the window paints its own ground
// in the gap between "Tauri showed the window" and "the app has a stylesheet". That is a
// second place that knows what the app looks like, which is the kind of duplication this
// repo avoids — so it is allowed here only because this file makes it *checked*. Drift
// the light or dark ground in style.css and the gate fails here, naming both values.
//
// Two more things are asserted because they are load-bearing and silent when broken:
// the cascade layer (without it the inline rule can outlive its moment and hold the wrong
// ground for a pinned theme), and the absence of any inline script (the CSP grants
// `style-src 'unsafe-inline'` and no script equivalent, so an inline script here would
// not run at all — and the ground would be back to the platform's white).
//
// Pure — reads two files, no DOM — so node runs it straight off the source like the rest.
import { readFileSync } from "node:fs";
import { join } from "node:path";
import test from "node:test";
import assert from "node:assert/strict";

const UI_DIR = join(import.meta.dirname, "..");
const INDEX = readFileSync(join(UI_DIR, "index.html"), "utf8");
const STYLE = readFileSync(join(UI_DIR, "style.css"), "utf8");

/** The `@layer launch { … }` body from index.html — matched by brace counting rather
 *  than a regex, since the block nests an `@media` inside it. */
function launchLayer(html: string): string {
  const at = html.indexOf("@layer launch");
  assert.notEqual(at, -1, "index.html no longer declares the `@layer launch` block");
  const open = html.indexOf("{", at);
  let depth = 0;
  for (let i = open; i < html.length; i++) {
    if (html[i] === "{") depth++;
    else if (html[i] === "}" && --depth === 0) return html.slice(open + 1, i);
  }
  assert.fail("the `@layer launch` block is unclosed");
}

/** Every `#rrggbb` a `background:` declaration names, in source order. */
function backgrounds(css: string): string[] {
  return [...css.matchAll(/background:\s*(#[0-9a-f]{6})/gi)].map((m) => m[1].toLowerCase());
}

/** Every `--bg:` value in source order. style.css defines it three times — the light
 *  `:root`, the pinned `[data-theme="dark"]`, and the `prefers-color-scheme: dark`
 *  block — and its own comment requires the last two to stay identical. */
function grounds(css: string): string[] {
  return [...css.matchAll(/--bg:\s*(#[0-9a-f]{6})/gi)].map((m) => m[1].toLowerCase());
}

test("style.css still defines the two grounds this file compares against", () => {
  const [light, pinnedDark, systemDark] = grounds(STYLE);
  assert.equal(grounds(STYLE).length, 3, "expected --bg in :root, [data-theme=dark] and the media block");
  assert.notEqual(light, pinnedDark, "the light and dark grounds must differ");
  assert.equal(
    pinnedDark,
    systemDark,
    "style.css's two dark bodies have diverged — a pinned Dark and a dark OS must paint the same ground",
  );
});

test("index.html's launch ground matches style.css, light and dark", () => {
  const layer = launchLayer(INDEX);
  const dark = layer.indexOf("@media");
  assert.notEqual(dark, -1, "the launch layer no longer answers prefers-color-scheme");

  const [styleLight, styleDark] = grounds(STYLE);
  assert.deepEqual(
    backgrounds(layer.slice(0, dark)),
    [styleLight],
    "the launch layer's light ground has drifted from style.css's `--bg`",
  );
  assert.deepEqual(
    backgrounds(layer.slice(dark)),
    [styleDark],
    "the launch layer's dark ground has drifted from style.css's dark `--bg`",
  );
});

test("the launch ground stays in a cascade layer, so style.css always outranks it", () => {
  // Unlayered rules beat layered ones whatever the source order, which is the whole
  // reason this is safe to ship: the moment style.css lands it wins, and a pinned theme
  // is never held back by a rule that only ever meant to cover the first few frames.
  assert.match(INDEX, /@layer launch\s*\{/);
});

test("index.html runs no inline script — the CSP would refuse it", () => {
  // `default-src 'self'` covers script-src, and loosening that in a Markdown renderer is
  // not on the table (ADR-0016). A `<script>` with a body would fail silently: no error
  // the user sees, just the thing it was meant to do quietly not happening.
  for (const [, body] of INDEX.matchAll(/<script\b[^>]*>([\s\S]*?)<\/script>/gi)) {
    assert.equal(body.trim(), "", "index.html gained an inline script; the CSP will not run it");
  }
});
