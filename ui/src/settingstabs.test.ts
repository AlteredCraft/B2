// The Settings rail's shape and its arrow-key walk (settingstabs.ts). The paint reads the
// same list the arrows do, so pinning the list pins both (K1, GH #78).
import type { KeyEventLike } from "./bindings.ts";
import {
  DEFAULT_SETTINGS_TAB,
  SETTINGS_TABS,
  isSettingsTab,
  tabDomId,
  tabMove,
  tabNavFor,
  tabStep,
} from "./settingstabs.ts";

/** A keydown, as the registry's matcher sees it. */
function press(key: string): KeyEventLike {
  return { key, metaKey: false, ctrlKey: false, shiftKey: false, altKey: false };
}

let passed = 0;

function assert(cond: boolean, msg: string): void {
  if (!cond) throw new Error(`assertion failed: ${msg}`);
}
function check(name: string, fn: () => void): void {
  fn();
  passed++;
  console.log(`  ok  ${name}`);
}

check("every tab is a full row, and the ids are unique", () => {
  assert(SETTINGS_TABS.length > 1, "a tabs interface needs more than one tab");
  const seen = new Set<string>();
  for (const t of SETTINGS_TABS) {
    assert(t.label.trim() !== "", `a tab with no label: ${t.id}`);
    assert(t.hint.trim() !== "", `a tab with no hint: ${t.id}`);
    assert(!seen.has(t.id), `duplicate tab id: ${t.id}`);
    seen.add(t.id);
  }
});

check("the default tab is one of them", () => {
  assert(isSettingsTab(DEFAULT_SETTINGS_TAB), "the default names a real section");
});

check("isSettingsTab rejects anything that isn't a section", () => {
  // Guards DOM attributes and preferences from a build with different sections.
  assert(!isSettingsTab("Keyboard"), "ids are exact, not case-folded");
  assert(!isSettingsTab(""), "the empty string is not a section");
  assert(!isSettingsTab(null), "null is not a section");
  assert(!isSettingsTab(0), "a number is not a section");
});

check("stepping forward through every tab returns to where it started", () => {
  let at = SETTINGS_TABS[0].id;
  const walked = [at];
  for (let i = 1; i < SETTINGS_TABS.length; i++) {
    at = tabStep(at, 1);
    walked.push(at);
  }
  assert(
    walked.join(",") === SETTINGS_TABS.map((t) => t.id).join(","),
    `forward walk visited ${walked.join(",")}`,
  );
  assert(tabStep(at, 1) === SETTINGS_TABS[0].id, "the last tab wraps to the first");
});

check("stepping backward wraps the other way", () => {
  assert(
    tabStep(SETTINGS_TABS[0].id, -1) === SETTINGS_TABS[SETTINGS_TABS.length - 1].id,
    "the first tab wraps to the last",
  );
});

check("the rail's moves step and wrap, and land on the ends", () => {
  const first = SETTINGS_TABS[0].id;
  const last = SETTINGS_TABS[SETTINGS_TABS.length - 1].id;
  assert(tabMove(first, "settings.tab.next") === SETTINGS_TABS[1].id, "next moves on");
  assert(tabMove(first, "settings.tab.prev") === last, "off the first wraps to the last");
  assert(tabMove(last, "settings.tab.first") === first, "first lands on the first tab");
  assert(tabMove(first, "settings.tab.last") === last, "last lands on the last tab");
});

check("the shipped keys are ↑↓ and Home/End", () => {
  assert(tabNavFor(press("ArrowDown")) === "settings.tab.next", "↓");
  assert(tabNavFor(press("ArrowUp")) === "settings.tab.prev", "↑");
  assert(tabNavFor(press("Home")) === "settings.tab.first", "Home");
  assert(tabNavFor(press("End")) === "settings.tab.last", "End");
});

check("a key the rail has no move for is left alone", () => {
  // ⌃Tab is `settings.section.next`'s, and must not be answered here too.
  for (const key of ["Tab", "Enter", " ", "Escape", "ArrowLeft", "ArrowRight", "a"]) {
    assert(tabNavFor(press(key)) === null, `${JSON.stringify(key)} is not a rail move`);
  }
  assert(
    tabNavFor({ key: "Tab", metaKey: false, ctrlKey: true, shiftKey: false, altKey: false }) === null,
    "⌃Tab belongs to settings.section.next",
  );
});

check("every tab has its own element id, the one the rail paints and focus returns to", () => {
  const ids = SETTINGS_TABS.map((t) => tabDomId(t.id));
  assert(new Set(ids).size === ids.length, "unique");
  assert(tabDomId("keyboard") === "settings-tab-keyboard", "and spelled as main.ts looks for it");
});

console.log(`settingstabs: ${passed} checks passed`);
