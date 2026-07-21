// The anchored ghost graph's scene builder (GH #22): pure functions from the
// discovery state the app already holds (`explain` + `similar`) to a positioned
// scene of nodes and edges. **Deterministic, no physics** — an ego graph is a
// statement, not soup: authored edges band by category around the anchor with
// the latent (ghost) candidates on an outer orbit. That layout-follows-meaning
// stance is the whole point of drawing B2's typed graph instead of cloning an
// untyped force-directed hairball (issue #22).
//
// No DOM, no IPC, no randomness — same input, same scene — so this file is unit
// tested the way `panes.ts` is (`graph.test.ts`, plain node). `render.ts` turns
// a scene into SVG; `main.ts` wires the clicks.

import type { NeighborView, ResourceLink, SimilarView, UnresolvedLink } from "./types";

/** The stance-trio core verbs (data-model.md §2) plus the tolerated tail —
 *  color = verb is the graph's first encoding. */
export type Category = "references" | "supports" | "contradicts" | "other";

const CORE_VERBS = new Set(["references", "supports", "contradicts"]);

/** A verb's color key: core verbs are themselves; tail verbs (stored verbatim,
 *  never dropped) read as "other". */
export function categoryOf(verb: string): Category {
  return CORE_VERBS.has(verb) ? (verb as Category) : "other";
}

/** Symmetric verbs are their own inverse (relation.rs) — drawn with no arrowhead. */
const SYMMETRIC = new Set(["contradicts"]);

/** Verb display order: authored nodes sort by it so edge colors band into
 *  sectors instead of alternating around the orbit. */
const CATEGORY_ORDER: Category[] = ["references", "supports", "contradicts", "other"];

/** The logical drawing space; the SVG viewBox scales it to the pane. */
export const VIEW_W = 1000;
export const VIEW_H = 620;

/** Most ghost candidates drawn — beyond this the halo stops reading as "a few
 *  questions worth answering" and starts reading as noise. */
export const GHOST_LIMIT = 6;

/** Node radii (the square resource glyph uses `resource` as its half-side). */
export const NODE_R = { anchor: 34, note: 24, resource: 22, dangling: 22, ghost: 22 } as const;

export interface GraphNode {
  /** Stable scene identity: the note's b2id, `res:<path>`, `dangling:<n>`,
   *  `ghost:<b2id>`, or `anchor`. */
  id: string;
  kind: "anchor" | "note" | "resource" | "dangling" | "ghost";
  x: number;
  y: number;
  /** Display label (truncated); `full` keeps the whole string for the tooltip. */
  label: string;
  full: string;
  /** The quiet second line: a ghost's score, a resource class. */
  sub: string | null;
  /** Vault path a click opens (null for dangling — nothing resolved to open). */
  path: string | null;
  /** The target's title, for the link modal a ghost click opens. */
  title: string | null;
}

export interface GraphEdge {
  /** Scene-node ids, in *authored* direction (src → dst). */
  from: string;
  to: string;
  /** Endpoints, trimmed back to each node's rim so arrowheads land cleanly. */
  x1: number;
  y1: number;
  x2: number;
  y2: number;
  /** Quadratic control point separating parallel edges; null → straight line. */
  cx: number | null;
  cy: number | null;
  /** Where the verb pill (or ghost score) sits. */
  lx: number;
  ly: number;
  label: string;
  category: Category;
  /** Latent (from `similar`) — dashed teal, click-to-link. */
  ghost: boolean;
  /** Directed verbs get an arrowhead at `to`; symmetric verbs none. */
  arrow: boolean;
}

export interface GraphScene {
  nodes: GraphNode[];
  edges: GraphEdge[];
}

/** Everything the scene is a pure function of — state the app already fetched. */
export interface GraphInput {
  anchor: { path: string; title: string | null };
  connections: NeighborView[];
  resources: ResourceLink[];
  unresolved: UnresolvedLink[];
  ghosts: SimilarView[];
}

// --- internal shaping ---------------------------------------------------------------

/** One authored edge, normalized across the three target kinds. */
interface Authored {
  nodeId: string;
  kind: "note" | "resource" | "dangling";
  name: string;
  sub: string | null;
  path: string | null;
  verb: string;
  /** Drawn src → dst: outbound = anchor → node, inbound = node → anchor. */
  outbound: boolean;
}

const LABEL_MAX = 22;

function truncate(s: string): string {
  return s.length <= LABEL_MAX ? s : `${s.slice(0, LABEL_MAX - 1).trimEnd()}…`;
}

/** A note's display name: title, else the filename without `.md`. */
function noteName(title: string | null, path: string): string {
  if (title) return title;
  const base = path.split("/").pop() ?? path;
  return base.replace(/\.md$/i, "");
}

/** Flatten connections + resource links + dangling links into one authored list. */
function authoredOf(input: GraphInput): Authored[] {
  const out: Authored[] = [];
  for (const c of input.connections) {
    out.push({
      nodeId: c.b2id,
      kind: "note",
      name: noteName(c.title, c.path),
      sub: null,
      path: c.path,
      verb: c.relation,
      outbound: c.direction === "outbound",
    });
  }
  for (const r of input.resources) {
    out.push({
      nodeId: `res:${r.path}`,
      kind: "resource",
      name: r.path.split("/").pop() ?? r.path,
      sub: r.class,
      path: r.path,
      verb: r.relation,
      outbound: true,
    });
  }
  input.unresolved.forEach((u, i) => {
    out.push({
      nodeId: `dangling:${i}`,
      kind: "dangling",
      name: `[[${u.target}]]`,
      // No sub-label: the ⚠ glyph, dashed ring, and legend already say "broken".
      sub: null,
      path: null,
      verb: u.relation,
      outbound: true,
    });
  });
  return out;
}

interface Placed {
  x: number;
  y: number;
}

/** Trim a straight segment back from both node rims (+pad for the arrowhead). */
function trim(a: Placed, ra: number, b: Placed, rb: number) {
  const dx = b.x - a.x;
  const dy = b.y - a.y;
  const len = Math.hypot(dx, dy) || 1;
  const ux = dx / len;
  const uy = dy / len;
  const pad = 4;
  return {
    x1: a.x + ux * (ra + pad),
    y1: a.y + uy * (ra + pad),
    x2: b.x - ux * (rb + pad),
    y2: b.y - uy * (rb + pad),
  };
}

/** Build the edge records for `items` (all between the anchor and one node each),
 *  curving parallel edges apart and placing each label at its curve's midpoint. */
function edgesFor(
  items: Authored[],
  nodeAt: Map<string, Placed>,
  anchor: Placed,
  radiusOf: Map<string, number>,
): GraphEdge[] {
  // Parallel-edge bookkeeping: how many edges share a node, and each one's index.
  const perNode = new Map<string, number>();
  for (const it of items) perNode.set(it.nodeId, (perNode.get(it.nodeId) ?? 0) + 1);
  const seen = new Map<string, number>();

  return items.map((it) => {
    const node = nodeAt.get(it.nodeId);
    if (!node) throw new Error(`unplaced node ${it.nodeId}`);
    const r = radiusOf.get(it.nodeId) ?? NODE_R.note;
    const [a, b, ra, rb] = it.outbound
      ? [anchor, node, NODE_R.anchor, r]
      : [node, anchor, r, NODE_R.anchor];
    const seg = trim(a, ra, b, rb);

    // Parallel edges between the same pair bow apart on a perpendicular offset.
    const siblings = perNode.get(it.nodeId) ?? 1;
    const index = seen.get(it.nodeId) ?? 0;
    seen.set(it.nodeId, index + 1);
    let cx: number | null = null;
    let cy: number | null = null;
    let lx = (seg.x1 + seg.x2) / 2;
    let ly = (seg.y1 + seg.y2) / 2;
    if (siblings > 1) {
      const off = (index - (siblings - 1) / 2) * 34;
      const dx = seg.x2 - seg.x1;
      const dy = seg.y2 - seg.y1;
      const len = Math.hypot(dx, dy) || 1;
      cx = lx + (-dy / len) * off;
      cy = ly + (dx / len) * off;
      // Quadratic midpoint: B(0.5) = ¼·p0 + ½·c + ¼·p1.
      lx = 0.25 * seg.x1 + 0.5 * cx + 0.25 * seg.x2;
      ly = 0.25 * seg.y1 + 0.5 * cy + 0.25 * seg.y2;
    }

    return {
      from: it.outbound ? "anchor" : it.nodeId,
      to: it.outbound ? it.nodeId : "anchor",
      ...seg,
      cx,
      cy,
      lx,
      ly,
      label: it.verb,
      category: categoryOf(it.verb),
      ghost: false,
      arrow: !SYMMETRIC.has(it.verb),
    };
  });
}

/** One node record per distinct authored target (a pair can share several edges). */
function nodesFor(items: Authored[], nodeAt: Map<string, Placed>): GraphNode[] {
  const out: GraphNode[] = [];
  const done = new Set<string>();
  for (const it of items) {
    if (done.has(it.nodeId)) continue;
    done.add(it.nodeId);
    const at = nodeAt.get(it.nodeId);
    if (!at) continue;
    out.push({
      id: it.nodeId,
      kind: it.kind,
      x: at.x,
      y: at.y,
      label: truncate(it.name),
      full: it.name,
      sub: it.sub,
      path: it.path,
      title: null,
    });
  }
  return out;
}

function anchorNode(input: GraphInput, at: Placed): GraphNode {
  const name = noteName(input.anchor.title, input.anchor.path);
  return {
    id: "anchor",
    kind: "anchor",
    x: at.x,
    y: at.y,
    label: truncate(name),
    full: name,
    sub: null,
    path: input.anchor.path,
    title: input.anchor.title,
  };
}

/** Radii per node id, for rim-trimming edges. */
function radii(items: Authored[]): Map<string, number> {
  const m = new Map<string, number>();
  for (const it of items) m.set(it.nodeId, NODE_R[it.kind]);
  return m;
}

/** Stable authored order: category bands first (so orbit colors cluster), then
 *  name, then node id — fully deterministic. */
function sortAuthored(items: Authored[]): Authored[] {
  return [...items].sort((p, q) => {
    const c =
      CATEGORY_ORDER.indexOf(categoryOf(p.verb)) - CATEGORY_ORDER.indexOf(categoryOf(q.verb));
    if (c !== 0) return c;
    const n = p.name.localeCompare(q.name);
    if (n !== 0) return n;
    return p.nodeId.localeCompare(q.nodeId);
  });
}

/** Orbits are ellipses, not circles: the pane is wide (1000×620), so the vertical
 *  radius is this fraction of the horizontal one — the scene fills the width
 *  without the outer halo clipping the top or bottom. */
export const ORBIT_ASPECT = 0.6;

/** Evenly spaced orbit positions starting at the top (−90°). `rx` is the
 *  horizontal radius; the vertical one follows [`ORBIT_ASPECT`]. */
function ring(center: Placed, rx: number, n: number, phase = -Math.PI / 2): Placed[] {
  return Array.from({ length: n }, (_, i) => {
    const a = phase + (i * 2 * Math.PI) / n;
    return {
      x: center.x + rx * Math.cos(a),
      y: center.y + rx * ORBIT_ASPECT * Math.sin(a),
    };
  });
}

// --- the scene ----------------------------------------------------------------------

const CENTER: Placed = { x: VIEW_W / 2, y: VIEW_H / 2 };

/** The ghost graph — the module's one entry point: authored edges on an inner orbit
 *  (category-banded), the top `similar` candidates as a dashed outer halo of
 *  not-yet-links. */
export function buildScene(input: GraphInput): GraphScene {
  const authored = sortAuthored(authoredOf(input));
  const ghosts = input.ghosts.slice(0, GHOST_LIMIT);

  // Distinct authored targets, in band order (several edges can share a node).
  const ids: string[] = [];
  for (const it of authored) if (!ids.includes(it.nodeId)) ids.push(it.nodeId);

  const r1 = ids.length <= 6 ? 250 : 300;
  const inner = ring(CENTER, r1, Math.max(ids.length, 1));
  const nodeAt = new Map<string, Placed>();
  ids.forEach((id, i) => nodeAt.set(id, inner[i]));

  // Ghost halo: outside the authored orbit, phase-shifted half a step so ghosts
  // sit between authored spokes instead of stacking on them.
  const r2 = 400;
  const phase = -Math.PI / 2 + (ghosts.length ? Math.PI / ghosts.length : 0) + 0.35;
  const halo = ring(CENTER, r2, Math.max(ghosts.length, 1), phase);

  const nodes: GraphNode[] = [anchorNode(input, CENTER), ...nodesFor(authored, nodeAt)];
  const edges = edgesFor(authored, nodeAt, CENTER, radii(authored));

  ghosts.forEach((g, i) => {
    const at = halo[i];
    const name = noteName(g.title, g.path);
    nodes.push({
      id: `ghost:${g.b2id}`,
      kind: "ghost",
      x: at.x,
      y: at.y,
      label: truncate(name),
      full: name,
      sub: g.score.toFixed(2),
      path: g.path,
      title: g.title,
    });
    const seg = trim(CENTER, NODE_R.anchor, at, NODE_R.ghost);
    edges.push({
      from: "anchor",
      to: `ghost:${g.b2id}`,
      ...seg,
      cx: null,
      cy: null,
      lx: (seg.x1 + seg.x2) / 2,
      ly: (seg.y1 + seg.y2) / 2,
      label: g.score.toFixed(2),
      category: "other",
      ghost: true,
      arrow: false,
    });
  });

  return { nodes, edges };
}
