// The Settings rail's shape and its arrow-key walk (settingstabs.ts), pinned. Pure —
// no DOM — so node runs it straight off the source: `npm test`. Dependency-free like the
// others.
//
// What a tab rail actually gets wrong is dull: a walk that dead-ends instead of wrapping
// (so ⌃Tab stops working on the last section), Home/End that drift off the ends when a
// tab is added, a `data-settings-tab` attribute that no longer names a real section. The
// paint reads the same list the arrows do (settingstabs.ts says why), so pinning the list
// pins both — which is the point of the module existing at all (K1, GH #78).
import {
  DEFAULT_SETTINGS_TAB,
  SETTINGS_TABS,
  isSettingsTab,
  tabMove,
  tabStep,
} from "./settingstabs.ts";

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
  // The guard's real job: a `data-settings-tab` attribute read back off the DOM, and a
  // preference read back from a previous build that had different sections.
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

check("↑↓ step, Home/End land on the ends", () => {
  const first = SETTINGS_TABS[0].id;
  const last = SETTINGS_TABS[SETTINGS_TABS.length - 1].id;
  assert(tabMove(first, "ArrowDown") === SETTINGS_TABS[1].id, "↓ moves to the next tab");
  assert(tabMove(first, "ArrowUp") === last, "↑ off the first wraps to the last");
  assert(tabMove(last, "Home") === first, "Home lands on the first tab");
  assert(tabMove(first, "End") === last, "End lands on the last tab");
});

check("tabMove leaves keys it has no move for alone", () => {
  // A rail that answered every key would swallow Tab out of the dialog, ⏎ on the tab,
  // and every global chord that fires while Settings is open.
  for (const key of ["Tab", "Enter", " ", "Escape", "ArrowLeft", "ArrowRight", "a"]) {
    assert(tabMove(SETTINGS_TABS[0].id, key) === null, `${JSON.stringify(key)} is not a rail move`);
  }
});

console.log(`settingstabs: ${passed} checks passed`);
