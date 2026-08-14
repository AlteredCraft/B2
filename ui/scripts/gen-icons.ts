// The icon vendoring step: Bootstrap Icons (twbs/icons, MIT) → `src/icons.gen.ts`.
//
// Why generate rather than depend at runtime. Three constraints meet here and only one
// shape satisfies all of them:
//
//  1. **The webview CSP is `default-src 'self'`** (crates/b2-desktop/tauri.conf.json), so
//     there is no CDN option — whatever we use has to be in the bundle.
//  2. **`npm test` runs off the source through node**, not Vite (`node
//     --experimental-strip-types --test src/*.test.ts`). So the obvious Vite-native move —
//     `import chevron from "bootstrap-icons/icons/chevron-right.svg?raw"` — would resolve
//     under `vite build` and fail under the test runner, taking every test that transitively
//     imports render.ts with it. Plain TypeScript is the only form both hosts can read.
//  3. **2078 icons ship in the package; we use ~24.** Vendoring the ones we name keeps the
//     bundle honest and the diff reviewable.
//
// So the package is a **devDependency** — the provenance and the upgrade path — and this
// script lifts the path data of the named subset into a checked-in `.ts` file.
//
//   node --experimental-strip-types scripts/gen-icons.ts            # regenerate (just icons)
//   node --experimental-strip-types scripts/gen-icons.ts --check    # fail if stale
//
// `--check` is what makes the generated file un-driftable: it runs first in `npm test`
// (so `just check` and `just ci` both carry it), and it exits non-zero — a check that
// reports a problem and exits 0 is a hole in the gate (CLAUDE.md → "Gates, CI, and the
// justfile"). Bump the dependency and the check fails until you re-run the generator,
// which is the only moment upstream path data can change under us.

import { readFileSync, writeFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import { dirname, join } from "node:path";

const HERE = dirname(fileURLToPath(import.meta.url));
const PKG = join(HERE, "..", "node_modules", "bootstrap-icons");
const OUT = join(HERE, "..", "src", "icons.gen.ts");

/**
 * The subset, grouped by what asks for it. Adding an icon is: a name here, `just icons`,
 * then use it — `IconName` is `keyof typeof ICON_BODIES`, so a typo is a type error and a
 * name nobody imports is dead weight the reviewer can see.
 */
const MANIFEST: Record<string, readonly string[]> = {
  // Fold state + directional chrome. `chevron-right`/`-down` are the app's one fold
  // affordance (file tree, frontmatter drawer, discovery sections and cards); `-left`/`-up`
  // are history back and find-previous.
  chevrons: ["chevron-down", "chevron-left", "chevron-right", "chevron-up"],

  // The file tree's two slots: a folder's open/closed state, and one icon per thing a row
  // can be. The per-class file icons are the point of the whole change — `▣ ▶ ▤ ◇ ≡ ◆` said
  // "not a note" and nothing else.
  tree: [
    "file-earmark-binary",
    "file-earmark-code",
    "file-earmark-font",
    "file-earmark-image",
    "file-earmark-pdf",
    "file-earmark-play",
    "file-earmark-text",
    "folder",
    "folder2-open",
  ],

  // Create actions (tree head) and the top bar — `chat-dots` is the chat pane's toggle
  // (GH #155), which is a top-bar control beside Settings and the vault switcher.
  actions: ["chat-dots", "file-earmark-plus", "folder-plus", "gear", "search", "x-lg"],

  // Note-bar toggles: the connection graph and the Markdown-source view. They sit side by
  // side, so they have to come from the same family.
  toggles: ["code-slash", "diagram-3"],

  // Status: the anomaly badge, the graph's dangling node, a broken link's card marker, and
  // the "no model provisioned yet" banner.
  status: ["exclamation-triangle", "stars"],

  // A connection's direction, on the discovery card that names it.
  edges: ["arrow-left", "arrow-right"],
};

const NAMES = Object.values(MANIFEST).flat().sort();

/**
 * An icon file's *children* — everything between `<svg …>` and `</svg>`, with the
 * formatting whitespace collapsed. Children rather than the whole element because the
 * caller owns the wrapper: `icon()` sets the size, the class, and `aria-hidden`, and an
 * icon that carried its own `<svg>` would carry upstream's `width="16"` and `class="bi"`
 * into every call site.
 */
export function iconBody(svg: string): string {
  const open = svg.indexOf(">");
  const close = svg.lastIndexOf("</svg>");
  if (!svg.startsWith("<svg") || open === -1 || close === -1) {
    throw new Error("not an <svg> document");
  }
  return svg.slice(open + 1, close).replace(/\s+/g, " ").trim();
}

function read(name: string): string {
  const body = iconBody(readFileSync(join(PKG, "icons", `${name}.svg`), "utf8"));
  // Upstream ships static shapes; anything else would be arriving in our bundle unread.
  if (/<(script|style|image|foreignObject)\b/i.test(body)) {
    throw new Error(`${name}: unexpected element in upstream icon`);
  }
  return body;
}

/**
 * The upstream license, reproduced verbatim.
 *
 * Not a formality, and not satisfied by a pointer: the path data we vendor *is* a portion of
 * Bootstrap Icons, and the MIT License requires the copyright notice **and the permission
 * notice** to travel with it. An earlier version of this generator cited
 * `ui/node_modules/bootstrap-icons/LICENSE`, which is neither committed nor shipped — so
 * anyone receiving the generated file received the icons without their terms.
 *
 * It is emitted **twice**, from this one source, because source and bundle are two different
 * distributions and a comment only covers the first:
 *
 *  - as a `/*!` **legal comment**, the marker esbuild (Vite's minifier) keeps when it strips
 *    every other comment — so the notice survives into `ui/dist` and therefore into the app;
 *  - as an exported **string**, which is what `icons.test.ts` asserts against. A comment is
 *    invisible to a test that can't read the filesystem, and `--check` alone can't cover this
 *    (it compares the file to what the generator emits, so a generator edited to drop the
 *    notice would pass). The obligation deserves a check that fails.
 */
const LICENSE_TEXT = `The MIT License (MIT)

Copyright (c) 2019-2024 The Bootstrap Authors

Permission is hereby granted, free of charge, to any person obtaining a copy
of this software and associated documentation files (the "Software"), to deal
in the Software without restriction, including without limitation the rights
to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
copies of the Software, and to permit persons to whom the Software is
furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all
copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE
SOFTWARE.`;

function generate(): string {
  const version = JSON.parse(readFileSync(join(PKG, "package.json"), "utf8")).version;
  const rows = NAMES.map((n) => `  ${JSON.stringify(n)}: ${JSON.stringify(read(n))},`);
  return `// @generated by scripts/gen-icons.ts — do not edit. Run \`just icons\` to refresh.
//
// The subset is declared in scripts/gen-icons.ts; the wrapper, the sizes and the semantic
// name → meaning maps are in src/icons.ts. Nothing but the generator writes this file.

/*!
 * Bootstrap Icons v${version} — https://github.com/twbs/icons
 * @license MIT
 *
${LICENSE_TEXT.split("\n")
  .map((l) => ` * ${l}`.trimEnd())
  .join("\n")}
 */

/** Each icon's children — the markup \`icon()\` wraps in its own sized \`<svg>\`. */
export const ICON_BODIES = {
${rows.join("\n")}
} as const;

/** The upstream release this data was lifted from. \`gen-icons.ts --check\` pins it. */
export const BOOTSTRAP_ICONS_VERSION = ${JSON.stringify(version)};

/** The terms the data above is used under — see the generator for why this is a value and
 *  not only a comment. \`icons.test.ts\` asserts the notice is intact. */
export const BOOTSTRAP_ICONS_LICENSE = ${JSON.stringify(LICENSE_TEXT)};
`;
}

const next = generate();

if (process.argv.includes("--check")) {
  let current = "";
  try {
    current = readFileSync(OUT, "utf8");
  } catch {
    /* missing counts as stale */
  }
  if (current !== next) {
    console.error(
      "src/icons.gen.ts is stale — bootstrap-icons changed, or it was hand-edited.\n" +
        "Run `just icons` (or `npm --prefix ui run icons`) and commit the result.",
    );
    process.exit(1);
  }
  console.log(`  ok  src/icons.gen.ts matches bootstrap-icons (${NAMES.length} icons)`);
} else {
  writeFileSync(OUT, next);
  console.log(`wrote src/icons.gen.ts — ${NAMES.length} icons`);
}
