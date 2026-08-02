// Nested lists, the pure half — the Tab / ⇧Tab engine. main.ts wires it into the
// editor's keymap through the registry (`editor.list.indent` / `editor.list.outdent`,
// bindings.ts); this module never touches CodeMirror, so node runs its test straight off
// the source (`npm test`), like format.ts / newentry.ts / treenav.ts.
//
// Why it exists. Nesting a list was the one structural edit the editor had no gesture
// for: Enter continues a list (`markdownKeymap`, installed by `markdown()`), ⌘B wraps a
// word, but making `- b` a child of `- a` meant counting spaces by hand. Tab is the
// instinct — and Tab, unbound, is the *browser's*: it walked focus out of the buffer and
// into the next pane, which is the bug this closes.
//
// **Tab keeps its own meaning outside a list.** `indentList` returns null when the
// cursor isn't in a list item, and the binding declines, so Tab still steps the focus
// ring everywhere else — indenting a paragraph would make a code block, which nobody
// means by Tab. Inside a list it is claimed even when nothing can move (the first item of
// a list has nothing to nest under, a top-level item has nothing to lift out of): a
// gesture that sometimes ejects you from the buffer is worse than one that sometimes does
// nothing. K1's promise is unaffected either way — ⌘E leaves edit mode and ⌘1/⌘2/⌘3 move
// between panes, so the keyboard is never stuck in the editor.
//
// What it edits, and what it leaves alone. Leading whitespace, and the digits of an
// ordered marker. Nothing else: the bullet character is the author's (`-` vs `*` starts a
// *different* list in CommonMark, so rewriting one would silently restructure the note),
// and so is every byte of content. Renumbering is not a flourish — indenting `2.` out of
// `1. 2. 3.` leaves a nested list whose first item says "2.", which renders as "2.".

/** One text edit in original-document coordinates (CodeMirror's change shape). */
export interface ListChange {
  from: number;
  to: number;
  insert: string;
}

/** The edits, and the selection to land on (post-edit coordinates). Empty `changes` is a
 *  claimed-but-inert gesture — see the header. */
export interface ListEdit {
  changes: ListChange[];
  selFrom: number;
  selTo: number;
}

/** CommonMark §2.2: where tabs help define block structure they behave as if replaced by
 *  spaces to a tab stop of 4. Measuring follows the spec; what B2 *writes* is spaces. */
const TAB_STOP = 4;

/** A list item's marker, and the two columns that decide nesting. `\d{1,9}` is
 *  CommonMark's own limit on an ordered marker. */
const ITEM = /^([ \t]*)(?:([-*+])|(\d{1,9})([.)]))(?:([ \t]+)|$)/;

/** `* * *`, `---`, `___` — a thematic break, which the item pattern would otherwise read
 *  as a bullet whose content is more bullets. */
const RULE = /^[ \t]*(?:(?:\*[ \t]*){3,}|(?:-[ \t]*){3,}|(?:_[ \t]*){3,})$/;

const LEADING_WS = /^[ \t]*/;

interface Marker {
  /** Column the marker starts at — an item's own nesting level. */
  indent: number;
  /** Characters of leading whitespace: what an indent edit replaces. */
  indentLen: number;
  /** Column the item's content starts at — where a child of this item must sit. */
  content: number;
  /** The bullet character, or the ordered delimiter — the marker's *identity*. Changing
   *  either starts a new list in CommonMark (`1.` then `2)` is two lists, not one list of
   *  two), which is what bounds a run of shared numbering. */
  kind: string;
  /** The ordered number and the characters spelling it; absent on a bullet. */
  num?: number;
  numLen?: number;
}

interface Ln {
  /** Document offset of the line's first character. */
  from: number;
  text: string;
  blank: boolean;
  /** Column the first non-whitespace character sits at; 0 on a blank line. */
  indent: number;
  item?: Marker;
}

/** The column a whitespace-and-marker prefix ends at, tabs expanded per CommonMark. */
function measure(prefix: string): number {
  let col = 0;
  for (const ch of prefix) col = ch === "\t" ? col + TAB_STOP - (col % TAB_STOP) : col + 1;
  return col;
}

function leadingWs(text: string): string {
  return LEADING_WS.exec(text)?.[0] ?? "";
}

/** Read the document as lines, each classified as a list item or not. */
function scan(doc: string): Ln[] {
  const out: Ln[] = [];
  let from = 0;
  for (const text of doc.split("\n")) {
    const ws = leadingWs(text);
    const blank = ws.length === text.length;
    const indent = measure(ws);
    const m = blank || RULE.test(text) ? null : ITEM.exec(text);
    if (!m) {
      out.push({ from, text, blank, indent });
    } else {
      // An unmatched group is undefined at runtime, which `RegExpExecArray`'s `string[]`
      // index signature doesn't say — hence the explicit types rather than a destructure
      // that would read as total.
      const lead: string = m[1];
      const bullet: string | undefined = m[2];
      const digits: string | undefined = m[3];
      const delim: string | undefined = m[4];
      const gap: string | undefined = m[5];
      const markerText = bullet ?? `${digits}${delim}`;
      const afterMarker = measure(lead + markerText);
      // CommonMark: content sits one column past the marker plus the gap — except that a
      // gap of five or more spaces is code indentation, and an item with no content at
      // all has no gap to measure. Both cases put the content column one past the marker.
      const gapped = gap === undefined ? afterMarker + 1 : measure(lead + markerText + gap);
      const item: Marker = {
        indent,
        indentLen: lead.length,
        content: gapped - afterMarker > 4 ? afterMarker + 1 : gapped,
        // The alternation matched one branch or the other, so one of these is a string.
        kind: bullet ?? delim ?? "",
      };
      if (digits !== undefined) {
        item.num = Number(digits);
        item.numLen = digits.length;
      }
      out.push({ from, text, blank, indent, item });
    }
    from += text.length + 1;
  }
  return out;
}

function lineIndexAt(lines: readonly Ln[], pos: number): number {
  let lo = 0;
  let hi = lines.length - 1;
  while (lo < hi) {
    const mid = (lo + hi + 1) >> 1;
    if (lines[mid].from <= pos) lo = mid;
    else hi = mid - 1;
  }
  return lo;
}

/** The virtual nesting level of a line — the document's own, or the one the pending edit
 *  gives it. Every structural walk reads the list through one of these, which is what
 *  lets the same code answer "who is my sibling?" before and after the move. */
type Level = (i: number) => number;

/** The run of lines the list around `i` occupies: items, their indented continuations,
 *  and the single blank lines between them. Two blank lines, or an unindented paragraph,
 *  end it.
 *
 *  Bounding the walks below to a block is what keeps them honest across the whole note: a
 *  sibling search that ran to the top of the document would happily adopt an unrelated
 *  list three paragraphs up. */
function blockOf(lines: readonly Ln[], i: number): [number, number] {
  const listish = (j: number): boolean =>
    lines[j].item !== undefined || (!lines[j].blank && lines[j].indent > 0);
  let start = i;
  for (let j = i - 1; j >= 0; ) {
    if (listish(j)) {
      start = j;
      j--;
    } else if (lines[j].blank && j > 0 && listish(j - 1)) {
      start = j - 1;
      j -= 2;
    } else break;
  }
  let end = i;
  for (let j = i + 1; j < lines.length; ) {
    if (listish(j)) {
      end = j;
      j++;
    } else if (lines[j].blank && j + 1 < lines.length && listish(j + 1)) {
      end = j + 1;
      j += 2;
    } else break;
  }
  return [start, end];
}

/** The item directly above `i` at the same level, or -1 when `i` is the first of its run.
 *
 *  A shallower item is the parent (so there is no previous sibling), a deeper one belongs
 *  to an earlier sibling, and a paragraph at or above our level ends the run. */
function prevSibling(
  lines: readonly Ln[],
  level: Level,
  block: [number, number],
  i: number,
): number {
  const mine = level(i);
  for (let j = i - 1; j >= block[0]; j--) {
    const l = lines[j];
    if (l.blank) continue;
    if (l.item) {
      const at = level(j);
      if (at === mine) return j;
      if (at < mine) return -1;
      continue;
    }
    if (level(j) <= mine) return -1;
  }
  return -1;
}

/** The mirror of `prevSibling`, downwards — only the renumbering needs it. */
function nextSibling(
  lines: readonly Ln[],
  level: Level,
  block: [number, number],
  i: number,
): number {
  const mine = level(i);
  for (let j = i + 1; j <= block[1]; j++) {
    const l = lines[j];
    if (l.blank) continue;
    if (l.item) {
      const at = level(j);
      if (at === mine) return j;
      if (at < mine) return -1;
      continue;
    }
    if (level(j) <= mine) return -1;
  }
  return -1;
}

/** The nearest enclosing item — what ⇧Tab lifts out to. Unlike `prevSibling` this reads
 *  past a paragraph: an item's own continuation lines sit between it and its children. */
function parentOf(
  lines: readonly Ln[],
  level: Level,
  block: [number, number],
  i: number,
): number {
  const mine = level(i);
  for (let j = i - 1; j >= block[0]; j--) {
    if (lines[j].item && level(j) < mine) return j;
  }
  return -1;
}

/** The two sibling walks, narrowed to the same *list*.
 *
 *  A neighbour at my level is not necessarily in my list: CommonMark starts a new one at
 *  every change of marker — `1.` then `2)`, or `-` then `*` — so numbering must stop at
 *  that boundary even though the indentation doesn't.
 *
 *  Deliberately **not** what the nesting target uses. "Which item am I nested under?" is
 *  a question about columns, and `* b` landing under `- a` is the shape the author asked
 *  for by pressing Tab; "which items share my numbering?" is a question about the list,
 *  and the two answers part company exactly here. */
function prevInRun(lines: readonly Ln[], level: Level, block: [number, number], i: number): number {
  const j = prevSibling(lines, level, block, i);
  return j >= 0 && lines[j].item?.kind === lines[i].item?.kind ? j : -1;
}

function nextInRun(lines: readonly Ln[], level: Level, block: [number, number], i: number): number {
  const j = nextSibling(lines, level, block, i);
  return j >= 0 && lines[j].item?.kind === lines[i].item?.kind ? j : -1;
}

/** The consecutive items of `i`'s own list at `i`'s level, `i` included, in document
 *  order — the run a numbering sequence runs over. */
function groupOf(
  lines: readonly Ln[],
  level: Level,
  block: [number, number],
  i: number,
): number[] {
  const out = [i];
  for (let j = prevInRun(lines, level, block, i); j >= 0; ) {
    out.unshift(j);
    j = prevInRun(lines, level, block, j);
  }
  for (let j = nextInRun(lines, level, block, i); j >= 0; ) {
    out.push(j);
    j = nextInRun(lines, level, block, j);
  }
  return out;
}

/**
 * Indent (`dir` 1) or outdent (-1) the list item(s) the selection covers.
 *
 * Returns null when the selection touches no list item at all — the caller's cue to
 * decline the keystroke and leave Tab to the platform.
 */
function listEdit(doc: string, from: number, to: number, dir: 1 | -1): ListEdit | null {
  const lines = scan(doc);
  const first = lineIndexAt(lines, from);
  let last = lineIndexAt(lines, to);
  // A selection ending at column 0 stops short of that line, the way every editor's
  // line-wise command reads it.
  if (last > first && to === lines[last].from) last--;

  let head = -1;
  for (let i = first; i <= last && head < 0; i++) if (lines[i].item) head = i;
  if (head < 0) return null;

  const inert: ListEdit = { changes: [], selFrom: from, selTo: to };
  const item = lines[head].item;
  if (!item) return null;

  const block = blockOf(lines, head);
  const before: Level = (i) => lines[i].indent;

  // Where the head item is going: under its previous sibling (to that item's content
  // column, which is where CommonMark puts a child), or out to its parent's own column.
  let target: number;
  if (dir === 1) {
    const sib = prevSibling(lines, before, block, head);
    const sibItem = sib >= 0 ? lines[sib].item : undefined;
    if (!sibItem) return inert; // the first item of a list has nothing to nest under
    target = sibItem.content;
  } else {
    const par = parentOf(lines, before, block, head);
    const parItem = par >= 0 ? lines[par].item : undefined;
    if (!parItem) return inert; // already at the top level
    target = parItem.indent;
  }
  const delta = target - item.indent;
  if (delta === 0) return inert;

  // The move carries the item's own subtree with it: the lines below the selection that
  // are indented past its last item are its children and continuations, and leaving them
  // behind would re-parent them onto whatever the head landed beside.
  let tail = head;
  for (let i = head; i <= last; i++) if (lines[i].item) tail = i;
  let end = last;
  for (let j = last + 1; j <= block[1]; j++) {
    if (lines[j].blank) continue;
    if (lines[j].indent <= lines[tail].indent) break;
    end = j;
  }

  // Planned per line, emitted below in one pass — a line can be both re-indented and
  // re-numbered, and the two rewrites *touch*: they would go to CodeMirror as an
  // insertion at the line start and a replacement starting at the same offset, whose
  // relative order a ChangeSet does not promise. One edit over the whole prefix has no
  // order to get wrong.
  const wsShift: number[] = Array.from(lines, () => 0);
  const newWs: (string | undefined)[] = Array.from(lines, () => undefined);
  for (let i = head; i <= end; i++) {
    const l = lines[i];
    if (l.blank) continue;
    const oldWs = leadingWs(l.text);
    const next = " ".repeat(Math.max(0, l.indent + delta));
    if (next !== oldWs) newWs[i] = next;
    wsShift[i] = next.length - oldWs.length;
  }

  // The list as it will be, so the renumbering below reasons about the structure the
  // author is about to see rather than the one they had.
  const after: Level = (i) =>
    i >= head && i <= end && !lines[i].blank
      ? Math.max(0, lines[i].indent + delta)
      : lines[i].indent;
  const moved = (i: number): boolean => i >= head && i <= end && lines[i].item !== undefined;

  // Two runs can come out mis-numbered: the one the head joined, and the one it left.
  // `groupOf` reads them off the post-move structure; the anchors are just a line known
  // to be in each. The head's old neighbours stay where they were, so they still name the
  // run it left — unless the selection took them along, in which case there is no run
  // left behind to fix.
  const anchors = [head];
  const oldPrev = prevSibling(lines, before, block, head);
  const oldNext = nextSibling(lines, before, block, head);
  if (oldPrev >= 0 && !moved(oldPrev)) anchors.push(oldPrev);
  if (oldNext >= 0 && !moved(oldNext)) anchors.push(oldNext);

  const wanted = new Map<number, number>();
  const done = new Set<number>();
  for (const anchor of anchors) {
    const group = groupOf(lines, after, block, anchor);
    if (done.has(group[0])) continue;
    done.add(group[0]);
    renumber(lines, group, before, block, moved, wanted);
  }

  const numShift: number[] = Array.from(lines, () => 0);
  const changes: ListChange[] = [];
  for (let i = 0; i < lines.length; i++) {
    const l = lines[i];
    const ws = newWs[i];
    const want = wanted.get(i);
    const it = l.item;
    if (want !== undefined && it?.numLen !== undefined) {
      const num = String(want);
      numShift[i] = num.length - it.numLen;
      const past = l.from + it.indentLen + it.numLen;
      changes.push(
        ws === undefined
          ? { from: l.from + it.indentLen, to: past, insert: num }
          : { from: l.from, to: past, insert: ws + num },
      );
    } else if (ws !== undefined) {
      changes.push({ from: l.from, to: l.from + leadingWs(l.text).length, insert: ws });
    }
  }

  const shift = (i: number): number => wsShift[i] + numShift[i];
  const mapPos = (pos: number): number => {
    const i = lineIndexAt(lines, pos);
    let acc = 0;
    for (let j = 0; j < i; j++) acc += shift(j);
    const wsLen = leadingWs(lines[i].text).length;
    const off = pos - lines[i].from;
    const wsNow = wsLen + wsShift[i];
    // A caret inside the indentation has no column of its own to keep — it rides to the
    // front of the text. Past it, the caret keeps its distance from the content.
    const inLine = off <= wsLen ? wsNow : Math.max(wsNow, off + shift(i));
    return lines[i].from + acc + inLine;
  };

  return { changes, selFrom: mapPos(from), selTo: mapPos(to) };
}

/**
 * Give one run of sibling items the numbers it should carry, appending the edits.
 *
 * The start number is the author's where they chose it (a list opening at `5.` keeps
 * opening at `5.`) and 1 where the run is newly headed — an item that had a sibling above
 * it before the move and doesn't now is the first item of a list that didn't exist a
 * keystroke ago, and a list that starts at "2." renders as "2.".
 *
 * The one run left untouched is the lazy `1. 1. 1.` style, which renders identically and
 * is a deliberate way to write Markdown; nothing moved in or out of it, so nothing about
 * it is wrong.
 */
function renumber(
  lines: readonly Ln[],
  group: readonly number[],
  before: Level,
  block: [number, number],
  moved: (i: number) => boolean,
  wanted: Map<number, number>,
): void {
  const nums = group.map((i) => lines[i].item?.num);
  if (nums.some((n) => n === undefined)) return; // a bullet run numbers nothing
  const settled = !group.some(moved);
  if (settled && group.length > 1 && nums.every((n) => n === nums[0])) return;

  // `prevInRun`, not `prevSibling`: "was this the head of its list?" has to ask about the
  // list. A `5)` item under a `1.` one *is* a head — its own — and reading the neighbour
  // above as a sibling would restart it at 1 and lose the number the author chose.
  const lead = group[0];
  const wasFirst = prevInRun(lines, before, block, lead) < 0;
  const start = wasFirst ? (nums[0] ?? 1) : 1;
  group.forEach((i, k) => {
    const want = start + k;
    if (lines[i].item?.num !== want) wanted.set(i, want);
  });
}

/** Tab — nest the list item(s) the selection covers one level deeper. */
export function indentList(doc: string, from: number, to: number): ListEdit | null {
  return listEdit(doc, from, to, 1);
}

/** ⇧Tab — lift the list item(s) the selection covers out one level. */
export function outdentList(doc: string, from: number, to: number): ListEdit | null {
  return listEdit(doc, from, to, -1);
}

/** Apply an edit to a document — the suite's mirror of what CodeMirror does with the
 *  changes, and the only honest way to assert on the Markdown that comes out. Exported
 *  for the test; the app dispatches the changes instead. */
export function applyChanges(doc: string, changes: readonly ListChange[]): string {
  let out = "";
  let at = 0;
  for (const c of changes) {
    out += doc.slice(at, c.from) + c.insert;
    at = c.to;
  }
  return out + doc.slice(at);
}
