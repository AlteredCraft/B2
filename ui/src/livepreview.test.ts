// The live-preview rules that are decidable without a running editor (livepreview.ts).
//
// Only one so far, and it's here because of a bug rather than for completeness. ⌘-click
// follows a wikilink in the editor; a plain click places the cursor, as an editor must
// (spec §3). That handler used to accept ⌃-click too — the same `metaKey || ctrlKey`
// reflex the keyboard had — and on macOS ⌃-click *is* the secondary click, so the one
// gesture navigated away and opened a context menu.
//
// The keyboard's own version of this rule is enforced twice over in bindings.test.ts and
// editorkeys.test.ts. Neither reaches a mouse handler, and "restore the Ctrl branch for
// cross-platform symmetry" is a plausible-looking edit somebody will eventually make, so
// the decision gets its own named function and this check rather than a comment.
import { isFollowClick } from "./livepreview.ts";

let passed = 0;

function assert(cond: boolean, msg: string): void {
  if (!cond) throw new Error(`assertion failed: ${msg}`);
}
function check(name: string, fn: () => void): void {
  fn();
  passed++;
  console.log(`  ok  ${name}`);
}

check("⌘-click follows a wikilink; a plain click does not", () => {
  assert(isFollowClick({ metaKey: true }), "⌘-click follows");
  assert(!isFollowClick({ metaKey: false }), "a bare click places the cursor");
});

check("⌃-click does not follow — on macOS it is the secondary click", () => {
  // The regression this file exists for. ⌃-click arrives as a primary-button mousedown
  // with `ctrlKey` set *and* generates the OS context-menu gesture, so treating it as a
  // synonym for ⌘ makes one gesture do two unrelated things.
  assert(!isFollowClick({ metaKey: false, ctrlKey: true } as MouseEvent), "⌃-click is a right-click");
  // ⌃ riding along with ⌘ is still a follow: the rule is about what ⌘ means, and the
  // predicate must not start rejecting extra modifiers it was never asked to police.
  assert(isFollowClick({ metaKey: true, ctrlKey: true } as MouseEvent), "⌃⌘-click still follows");
});

console.log(`livepreview: ${passed} checks passed`);
