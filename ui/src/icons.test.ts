// The icon registry (icons.ts), pinned. Pure string logic — no DOM — so node runs it
// straight off the source via its native type-stripping: `npm test`. Dependency-free like
// newentry.test.ts (hand-rolled assert; no @types/node).
//
// The half this file does **not** cover is deliberate: whether the checked-in path data
// still matches upstream Bootstrap Icons is `scripts/gen-icons.ts --check`'s job, and it
// runs first in the same `npm test` command. That check needs the filesystem and this file
// doesn't, which is the whole reason the split exists — a test that reads node_modules
// would drag @types/node into a suite whose other 15 files are dependency-free.
//
// What's left is the part a generator can't get wrong for you: that every *meaning* the app
// asks about resolves to a real icon, and that the markup around it stays the shape the
// panes and the stylesheet assume.

import {
  directionIcon,
  foldChevron,
  folderIcon,
  icon,
  NOTE_ICON,
  RESOURCE_ICONS,
  resourceIcon,
  sceneIcon,
  type IconName,
} from "./icons.ts";
import { BOOTSTRAP_ICONS_VERSION, ICON_BODIES } from "./icons.gen.ts";

let passed = 0;

function assert(cond: boolean, msg: string): void {
  if (!cond) throw new Error(`assertion failed: ${msg}`);
}
function equal(actual: unknown, expected: unknown, msg: string): void {
  assert(
    actual === expected,
    `${msg} — expected ${JSON.stringify(expected)}, got ${JSON.stringify(actual)}`,
  );
}
function check(name: string, fn: () => void): void {
  fn();
  passed++;
  console.log(`  ok  ${name}`);
}

const names = Object.keys(ICON_BODIES) as IconName[];

// --- the vendored data ---------------------------------------------------------------

check("every icon carries drawable markup and nothing else", () => {
  assert(names.length > 0, "the registry is not empty");
  for (const name of names) {
    const body = ICON_BODIES[name];
    assert(body.length > 0, `${name} has a body`);
    // Children only — `icon()` owns the wrapper, so an icon that brought its own <svg>
    // would smuggle upstream's width="16" and class="bi" into every call site.
    assert(!body.includes("<svg"), `${name} carries no nested <svg>`);
    // The bundle is B2's own markup, but it arrives from a package that upgrades: a
    // sanitizer never sees it (chrome HTML doesn't go through renderMarkdown), so the
    // generator refuses these at vendoring time and this is the second reading of it.
    assert(!/<script|on[a-z]+=/i.test(body), `${name} carries no script or handler`);
  }
});

check("the upstream release is recorded", () => {
  assert(/^\d+\.\d+\.\d+$/.test(BOOTSTRAP_ICONS_VERSION), "a semver string is pinned");
});

// --- what a thing looks like ---------------------------------------------------------
//
// The maps below are the reason this module exists rather than call sites naming icons:
// "what does a PDF look like in B2" has one answer, and these pin that every answer is a
// name the registry can actually draw.

check("every resource class maps to an icon the registry holds", () => {
  // The classes b2-core assigns (`ResourceSummary.class`) — the set the tree, the resource
  // card, and the graph's square nodes all render from.
  for (const cls of ["image", "media", "pdf", "html", "text", "binary"]) {
    const name = RESOURCE_ICONS[cls];
    assert(!!name, `${cls} has an icon`);
    assert(name in ICON_BODIES, `${cls} → ${name} is a real icon`);
  }
});

check("an unknown resource class falls back rather than blanking the row", () => {
  // The class comes from the host, so an unfamiliar one is a vault B2 hasn't learned about
  // yet — a row with no icon at all would read as a rendering bug rather than an unknown
  // file type.
  equal(resourceIcon("wingdings"), RESOURCE_ICONS.binary, "unknown → binary");
  equal(resourceIcon(""), RESOURCE_ICONS.binary, "empty → binary");
  equal(resourceIcon(null), RESOURCE_ICONS.binary, "null → binary");
  equal(resourceIcon(undefined), RESOURCE_ICONS.binary, "undefined → binary");
});

check("a note is not any resource — the tree's one first-class row reads as itself", () => {
  // Markdown is the vault's sole *authored* subset (data-model.md), and the tree says so:
  // the written-on page is the note's, so a .txt peer can't wear it.
  assert(NOTE_ICON in ICON_BODIES, "the note icon is a real icon");
  for (const [cls, name] of Object.entries(RESOURCE_ICONS)) {
    assert(name !== NOTE_ICON, `the ${cls} resource icon differs from a note's`);
  }
});

check("resource classes are mutually distinct — six icons, not one repeated", () => {
  // The complaint that started this: `▣ ▶ ▤ ◇ ≡ ◆` were distinct but meaningless. Distinct
  // is still the floor — an icon shared by two classes says less than the shapes did.
  const used = Object.values(RESOURCE_ICONS);
  equal(new Set(used).size, used.length, "no icon serves two classes");
});

check("fold state and folder state each have two faces", () => {
  assert(foldChevron(true) !== foldChevron(false), "open and closed chevrons differ");
  assert(folderIcon(true) !== folderIcon(false), "open and closed folders differ");
  for (const name of [foldChevron(true), foldChevron(false), folderIcon(true), folderIcon(false)])
    assert(name in ICON_BODIES, `${name} is a real icon`);
});

check("a connection's direction reads out or in, and nothing else", () => {
  assert(directionIcon("outbound") !== directionIcon("inbound"), "the two directions differ");
  // Anything that isn't outbound is inbound: the card's arrow points *at* the open note,
  // which is the honest default for a direction string the host may extend.
  equal(directionIcon("inbound"), directionIcon("whatever"), "unknown reads as inbound");
});

// --- the markup ----------------------------------------------------------------------

check("icon() wraps the body in a sized, current-colored, hidden svg", () => {
  const html = icon("gear", { size: 20 });
  assert(html.startsWith("<svg "), "an svg element");
  assert(html.includes(`width="20" height="20"`), "the requested size");
  assert(html.includes(`viewBox="0 0 16 16"`), "the art's own 16×16 grid, whatever the size");
  assert(html.includes(`fill="currentColor"`), "no color of its own");
  assert(html.includes(ICON_BODIES.gear), "the vendored body, verbatim");
  assert(html.endsWith("</svg>"), "closed");
});

check("every icon is hidden from the accessibility tree, with no opt-out", () => {
  // An icon in this UI is always *beside* its accessible name, never instead of it: a
  // control names itself with aria-label or its visible text (K1's "reachable"). An icon
  // that also announced would double every button up.
  for (const name of names) assert(icon(name).includes(`aria-hidden="true"`), `${name} is hidden`);
});

check("the base class is always there, and an extra one joins it", () => {
  // `.icon` is what the stylesheet's one icon rule hangs on; a caller's class is additive.
  assert(icon("search").includes(`class="icon"`), "the base class alone by default");
  equal(
    icon("search", { class: "find-glass" }).includes(`class="icon find-glass"`),
    true,
    "the caller's class joins rather than replaces",
  );
});

check("sceneIcon centers on a point and leaves the fill to CSS", () => {
  // The graph draws in scene coordinates, so a node's icon is placed, not laid out. And its
  // color is the stylesheet's (.gglyph, plus the dangling override) — a fill attribute here
  // would be one more thing that rule has to out-specify.
  const html = sceneIcon("exclamation-triangle", 100, 60, 16, "gglyph");
  assert(html.includes(`x="92" y="52"`), "half a box up and left of the center");
  assert(html.includes(`width="16" height="16"`), "the requested size");
  assert(!html.includes("fill="), "no fill of its own");
  assert(html.includes(`class="gglyph"`), "the scene's class");
});

console.log(`icons: ${passed} checks passed`);
