// The icon registry (icons.ts), off the DOM. Hand-rolled asserts, no @types/node.
// Whether the vendored data matches upstream is `scripts/gen-icons.ts --check`'s job (it
// needs the filesystem); this pins that every meaning resolves to a real icon and that the
// markup stays the shape the panes and stylesheet assume.

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
import {
  BOOTSTRAP_ICONS_LICENSE,
  BOOTSTRAP_ICONS_VERSION,
  ICON_BODIES,
} from "./icons.gen.ts";

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
    // Children only: `icon()` owns the wrapper.
    assert(!body.includes("<svg"), `${name} carries no nested <svg>`);
    // Chrome HTML never meets the sanitizer; the generator refuses these too.
    assert(!/<script|on[a-z]+=/i.test(body), `${name} carries no script or handler`);
  }
});

check("the upstream release is recorded", () => {
  assert(/^\d+\.\d+\.\d+$/.test(BOOTSTRAP_ICONS_VERSION), "a semver string is pinned");
});

check("the icon data carries its license, not a pointer to one", () => {
  // MIT requires both notices to travel with the copy. `--check` alone would pass a
  // generator edited to drop them. Reads the exported string: comments are invisible here.
  assert(
    BOOTSTRAP_ICONS_LICENSE.includes("Copyright (c) 2019-2024 The Bootstrap Authors"),
    "the copyright notice",
  );
  assert(
    BOOTSTRAP_ICONS_LICENSE.includes(
      "The above copyright notice and this permission notice shall be included in all",
    ),
    "the permission notice",
  );
  assert(BOOTSTRAP_ICONS_LICENSE.includes("Permission is hereby granted"), "the grant itself");
  assert(
    BOOTSTRAP_ICONS_LICENSE.includes('THE SOFTWARE IS PROVIDED "AS IS"'),
    "the warranty disclaimer",
  );
});

// --- what a thing looks like ---------------------------------------------------------

check("every resource class maps to an icon the registry holds", () => {
  // The classes b2-core assigns (`ResourceSummary.class`).
  for (const cls of ["image", "media", "pdf", "html", "text", "binary"]) {
    const name = RESOURCE_ICONS[cls];
    assert(!!name, `${cls} has an icon`);
    assert(name in ICON_BODIES, `${cls} → ${name} is a real icon`);
  }
});

check("an unknown resource class falls back rather than blanking the row", () => {
  equal(resourceIcon("wingdings"), RESOURCE_ICONS.binary, "unknown → binary");
  equal(resourceIcon(""), RESOURCE_ICONS.binary, "empty → binary");
  equal(resourceIcon(null), RESOURCE_ICONS.binary, "null → binary");
  equal(resourceIcon(undefined), RESOURCE_ICONS.binary, "undefined → binary");
});

check("a note is not any resource — the tree's one first-class row reads as itself", () => {
  assert(NOTE_ICON in ICON_BODIES, "the note icon is a real icon");
  for (const [cls, name] of Object.entries(RESOURCE_ICONS)) {
    assert(name !== NOTE_ICON, `the ${cls} resource icon differs from a note's`);
  }
});

check("resource classes are mutually distinct — six icons, not one repeated", () => {
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
  for (const name of names) assert(icon(name).includes(`aria-hidden="true"`), `${name} is hidden`);
});

check("the base class is always there, and an extra one joins it", () => {
  assert(icon("search").includes(`class="icon"`), "the base class alone by default");
  equal(
    icon("search", { class: "find-glass" }).includes(`class="icon find-glass"`),
    true,
    "the caller's class joins rather than replaces",
  );
});

check("a class cannot break out of its attribute", () => {
  // Escaped at the emitter so no call site has to remember (E5).
  const hostile = `x" onload="alert(1)`;
  const html = icon("gear", { class: hostile });
  assert(!html.includes(`onload="`), "no attribute was smuggled in");
  assert(html.includes("&quot;"), "the quote is escaped in place");
  // Opening tag only (the body has its own `d="…"`): class, width, height, viewBox, fill,
  // aria-hidden.
  const openTag = html.slice(0, html.indexOf(">") + 1);
  equal(openTag.match(/ [\w-]+="/g)?.length, 6, "the six attributes icon() writes, and no seventh");

  const scene = sceneIcon("gear", 0, 0, 16, hostile);
  assert(!scene.includes(`onload="`), "sceneIcon escapes its class too");
});

check("sceneIcon centers on a point and leaves the fill to CSS", () => {
  const html = sceneIcon("exclamation-triangle", 100, 60, 16, "gglyph");
  assert(html.includes(`x="92" y="52"`), "half a box up and left of the center");
  assert(html.includes(`width="16" height="16"`), "the requested size");
  assert(!html.includes("fill="), "no fill of its own");
  assert(html.includes(`class="gglyph"`), "the scene's class");
});

console.log(`icons: ${passed} checks passed`);
