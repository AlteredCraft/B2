// Nested lists, the pure half: the Tab / ⇧Tab engine (`editor.list.indent` / `outdent`,
// wired by main.ts). No CodeMirror here, so node tests it off the source.
//
// Outside a list, `indentList` returns null and Tab keeps stepping focus. Inside one
// (continuation lines and a loose list's blank interior included) Tab is claimed even
// when nothing can move: a gesture that sometimes ejects you from the buffer is worse
// than one that sometimes does nothing. A list behind a `> ` prefix isn't scanned here;
// main.ts's `inListItem` claims the key there. ⌘E and ⌘1–3 still leave the editor (K1).
//
// Edits only leading whitespace and ordered-marker digits. Bullet characters are the
// author's (`-` vs `*` starts a different list in CommonMark). Renumbering is needed:
// a nested list whose first item says "2." renders as "2.".

/** One text edit in original-document coordinates (CodeMirror's change shape). */
export interface ListChange {
  from: number;
  to: number;
  insert: string;
}

/** The edits, and the selection to land on (post-edit coordinates). Empty `changes` is a
 *  claimed-but-inert gesture. */
export interface ListEdit {
  changes: ListChange[];
  selFrom: number;
  selTo: number;
}

/** CommonMark §2.2 tab stop, for measuring; B2 writes spaces. */
const TAB_STOP = 4;

/** A list item's marker. `\d{1,9}` is CommonMark's limit on an ordered marker. */
const ITEM = /^([ \t]*)(?:([-*+])|(\d{1,9})([.)]))(?:([ \t]+)|$)/;

/** A thematic break, which `ITEM` would otherwise read as a bullet. */
const RULE = /^[ \t]*(?:(?:\*[ \t]*){3,}|(?:-[ \t]*){3,}|(?:_[ \t]*){3,})$/;

/** Block starters that interrupt a paragraph in CommonMark, so at column 0 they are never
 *  a lazy continuation of the item above. */
const INTERRUPTER = /^(?:#{1,6}(?:[ \t]|$)|```|~~~|>)/;

const LEADING_WS = /^[ \t]*/;

interface Marker {
  /** Column the marker starts at — an item's own nesting level. */
  indent: number;
  /** Characters of leading whitespace: what an indent edit replaces. */
  indentLen: number;
  /** Column the item's content starts at — where a child of this item must sit. */
  content: number;
  /** The bullet character or ordered delimiter. A change starts a new list in CommonMark,
   *  which bounds a run of shared numbering. */
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
      // Unmatched groups are undefined at runtime, which the index signature doesn't say.
      const lead: string = m[1];
      const bullet: string | undefined = m[2];
      const digits: string | undefined = m[3];
      const delim: string | undefined = m[4];
      const gap: string | undefined = m[5];
      const markerText = bullet ?? `${digits}${delim}`;
      const afterMarker = measure(lead + markerText);
      // CommonMark: content starts after the gap, unless the gap is 5+ (code) or absent;
      // then it is one past the marker.
      const gapped = gap === undefined ? afterMarker + 1 : measure(lead + markerText + gap);
      const item: Marker = {
        indent,
        indentLen: lead.length,
        content: gapped - afterMarker > 4 ? afterMarker + 1 : gapped,
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

/** A line's nesting level, before or after the pending edit, so the same walks answer
 *  "who is my sibling?" on either side of the move. */
type Level = (i: number) => number;

/** Is this line list material: an item, or an indented continuation? */
function listish(lines: readonly Ln[], j: number): boolean {
  return lines[j].item !== undefined || (!lines[j].blank && lines[j].indent > 0);
}

/** The run of lines the list around `i` occupies: items, continuations and single blank
 *  lines. Bounds the walks below so they never adopt an unrelated list further up. */
function blockOf(lines: readonly Ln[], i: number): [number, number] {
  let start = i;
  for (let j = i - 1; j >= 0; ) {
    if (listish(lines, j)) {
      start = j;
      j--;
    } else if (lines[j].blank && j > 0 && listish(lines, j - 1)) {
      start = j - 1;
      j -= 2;
    } else break;
  }
  let end = i;
  for (let j = i + 1; j < lines.length; ) {
    if (listish(lines, j)) {
      end = j;
      j++;
    } else if (lines[j].blank && j + 1 < lines.length && listish(lines, j + 1)) {
      end = j + 1;
      j += 2;
    } else break;
  }
  return [start, end];
}

/** The item a non-item, non-blank line belongs to: the nearest item above in the block.
 *  A column-0 line past a blank is a new paragraph (lazy continuation needs adjacency),
 *  and a column-0 block starter is never a continuation. */
function owningItem(lines: readonly Ln[], i: number): number {
  const line = lines[i];
  if (line.indent === 0 && (RULE.test(line.text) || INTERRUPTER.test(line.text))) return -1;
  const [start] = blockOf(lines, i);
  for (let j = i - 1; j >= start; j--) {
    if (lines[j].item) return j;
    if (lines[j].blank && line.indent === 0) return -1;
  }
  return -1;
}

/** The item directly above `i` at the same level, or -1 when `i` is the first of its run. */
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

/** The mirror of `prevSibling`, downwards. */
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

/** The nearest enclosing item, what ⇧Tab lifts out to. Reads past continuation lines. */
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

/** The sibling walks narrowed to the same list (a marker change starts a new one), for
 *  numbering. Nesting uses plain `prevSibling`: that is a question about columns. */
function prevInRun(lines: readonly Ln[], level: Level, block: [number, number], i: number): number {
  const j = prevSibling(lines, level, block, i);
  return j >= 0 && lines[j].item?.kind === lines[i].item?.kind ? j : -1;
}

function nextInRun(lines: readonly Ln[], level: Level, block: [number, number], i: number): number {
  const j = nextSibling(lines, level, block, i);
  return j >= 0 && lines[j].item?.kind === lines[i].item?.kind ? j : -1;
}

/** The consecutive items of `i`'s list at `i`'s level, in document order. */
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
 * Indent (`dir` 1) or outdent (-1) the list item(s) the selection covers. Null when it
 * touches no list, so the caller leaves Tab to the platform.
 */
function listEdit(doc: string, from: number, to: number, dir: 1 | -1): ListEdit | null {
  const lines = scan(doc);
  const first = lineIndexAt(lines, from);
  let last = lineIndexAt(lines, to);
  // A selection ending at column 0 stops short of that line.
  if (last > first && to === lines[last].from) last--;

  let head = -1;
  for (let i = first; i <= last && head < 0; i++) if (lines[i].item) head = i;

  const inert: ListEdit = { changes: [], selFrom: from, selTo: to };
  if (head < 0) {
    // No marker line, but the caret may still be in the list: a continuation acts on its
    // item; a blank interior to a loose list is claimed and swallowed.
    if (lines[first].blank) {
      // Interior: list material on both sides, with a real item above in the block
      // (indented code has continuations but no items).
      if (first === 0 || !listish(lines, first - 1)) return null;
      if (first + 1 >= lines.length || !listish(lines, first + 1)) return null;
      const [start] = blockOf(lines, first);
      for (let j = first - 1; j >= start; j--) if (lines[j].item) return inert;
      return null;
    }
    head = owningItem(lines, first);
    if (head < 0) return null;
  }
  const item = lines[head].item;
  if (!item) return null;

  const block = blockOf(lines, head);
  const before: Level = (i) => lines[i].indent;

  // Target column: the previous sibling's content column, or the parent's own column.
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

  // Carry the subtree: lines below indented past the last selected item move too.
  let tail = head;
  for (let i = head; i <= last; i++) if (lines[i].item) tail = i;
  let end = last;
  for (let j = last + 1; j <= block[1]; j++) {
    if (lines[j].blank) continue;
    if (lines[j].indent <= lines[tail].indent) break;
    end = j;
  }

  // Planned per line and emitted as one edit per prefix: a re-indent and a renumber would
  // touch at one offset, and a ChangeSet doesn't promise their order.
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

  // The list as it will be, for renumbering.
  const after: Level = (i) =>
    i >= head && i <= end && !lines[i].blank
      ? Math.max(0, lines[i].indent + delta)
      : lines[i].indent;
  const moved = (i: number): boolean => i >= head && i <= end && lines[i].item !== undefined;

  // Renumber the run the head joined and the one it left (named by its old neighbours,
  // unless they moved with it).
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
    // A caret in the indentation rides to the text; past it, it keeps its offset.
    const inLine = off <= wsLen ? wsNow : Math.max(wsNow, off + shift(i));
    return lines[i].from + acc + inLine;
  };

  return { changes, selFrom: mapPos(from), selTo: mapPos(to) };
}

/**
 * Give one run of sibling items the numbers it should carry. It keeps the author's start
 * number, or starts at 1 when the run is newly headed. An unmoved lazy `1. 1. 1.` run is
 * left alone.
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

  // `prevInRun`, not `prevSibling`: a `5)` below a `1.` heads its own list and keeps 5.
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

/** Apply changes to a document, as CodeMirror would. For the suite. */
export function applyChanges(doc: string, changes: readonly ListChange[]): string {
  let out = "";
  let at = 0;
  for (const c of changes) {
    out += doc.slice(at, c.from) + c.insert;
    at = c.to;
  }
  return out + doc.slice(at);
}
