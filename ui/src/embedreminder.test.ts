// The install-banner gating (embedreminder.ts), pinned. Pure boolean logic — no DOM —
// so node runs it straight off the source via its native type-stripping: `npm test`.
// Dependency-free like newentry.test.ts (hand-rolled assert; no @types/node).
import {
  loadReminderOptOut,
  saveReminderOptOut,
  shouldPromptEmbedInstall,
  type EmbedReminderInputs,
} from "./embedreminder.ts";

let passed = 0;

function assert(cond: boolean, msg: string): void {
  if (!cond) throw new Error(`assertion failed: ${msg}`);
}
function check(name: string, fn: () => void): void {
  fn();
  passed++;
  console.log(`  ok  ${name}`);
}

// The fresh-install case that motivates the feature: a vault with notes is open, the
// model isn't installed, nothing is downloading, and the user hasn't opted out.
const NEEDS_PROMPT: EmbedReminderInputs = {
  hasVault: true,
  semantic: false,
  notesTotal: 42,
  provisioning: false,
  dismissed: false,
};

check("prompts on a projected vault with no model installed", () => {
  assert(shouldPromptEmbedInstall(NEEDS_PROMPT), "the motivating fresh-install case");
});

check("never prompts once the model is installed (semantic live)", () => {
  assert(
    !shouldPromptEmbedInstall({ ...NEEDS_PROMPT, semantic: true }),
    "semantic on ⇒ no gap to surface",
  );
});

check("never prompts with no vault open", () => {
  assert(
    !shouldPromptEmbedInstall({ ...NEEDS_PROMPT, hasVault: false }),
    "nothing to embed without a vault",
  );
});

check("never prompts before there is anything to embed", () => {
  assert(
    !shouldPromptEmbedInstall({ ...NEEDS_PROMPT, notesTotal: 0 }),
    "empty vault or mid-projection ⇒ premature",
  );
});

check("stands down while a download is already in flight", () => {
  assert(
    !shouldPromptEmbedInstall({ ...NEEDS_PROMPT, provisioning: true }),
    "Settings owns the download; the banner's ask would be stale",
  );
});

check("respects a dismissal (session ✕ or persisted opt-out)", () => {
  assert(
    !shouldPromptEmbedInstall({ ...NEEDS_PROMPT, dismissed: true }),
    "the user asked us to stop",
  );
});

// --- the opt-out's storage ----------------------------------------------------------

check("no storage at all reads as not opted out, and saving into none is not an error", () => {
  // node has no `localStorage` — the shape of a browser refusing it in private mode.
  assert(!loadReminderOptOut(), "the reminder still shows");
  saveReminderOptOut();
});

check("an opt-out, once saved, comes back", () => {
  const store = new Map<string, string>();
  const g = globalThis as unknown as { localStorage?: unknown };
  g.localStorage = {
    getItem: (k: string) => store.get(k) ?? null,
    setItem: (k: string, v: string) => void store.set(k, v),
  };
  try {
    assert(!loadReminderOptOut(), "nothing saved yet");
    saveReminderOptOut();
    assert(loadReminderOptOut(), "saved");
  } finally {
    delete g.localStorage;
  }
});

console.log(`embedreminder: ${passed} checks passed`);
