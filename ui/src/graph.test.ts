// The graph scene builder (graph.ts), pinned. Pure math + data shaping — no DOM —
// so node runs it straight off the source via its native type-stripping: `npm test`.
// Same dependency-free idiom as panes.test.ts (hand-rolled assert).
//
// What's worth pinning: the visual-language invariants (ghost cap, orbit separation,
// arrowheads only on directed verbs) and determinism — the properties the SVG
// renderer builds on, not pixel positions.

import {
  buildScene,
  categoryOf,
  GHOST_LIMIT,
  NODE_R,
  ORBIT_ASPECT,
  VIEW_H,
  VIEW_W,
  type GraphInput,
  type GraphNode,
  type GraphScene,
} from "./graph.ts";
import type { NeighborView, ResourceLink, SimilarView, UnresolvedLink } from "./types.ts";

let passed = 0;

function assert(cond: boolean, msg: string): void {
  if (!cond) throw new Error(`assertion failed: ${msg}`);
}
function equal(actual: number | string | boolean | null, expected: number | string | boolean | null, msg: string): void {
  assert(actual === expected, `${msg} — expected ${expected}, got ${actual}`);
}
function check(name: string, fn: () => void): void {
  fn();
  passed++;
  console.log(`  ok  ${name}`);
}

// --- fixtures -----------------------------------------------------------------------

function neighbor(over: Partial<NeighborView>): NeighborView {
  return {
    b2id: "01X",
    path: "notes/x.md",
    title: "x",
    relation: "references",
    direction: "outbound",
    label: "references",
    explanation: null,
    origin: "inline",
    created: null,
    ...over,
  };
}

function ghost(over: Partial<SimilarView>): SimilarView {
  return { b2id: "01G", path: "notes/g.md", title: "g", score: 0.5, evidence: "", ...over };
}

function input(over: Partial<GraphInput>): GraphInput {
  return {
    anchor: { path: "notes/anchor.md", title: "anchor" },
    connections: [],
    resources: [],
    unresolved: [],
    ghosts: [],
    ...over,
  };
}

const anchorOf = (s: GraphScene): GraphNode => {
  const a = s.nodes.find((n) => n.kind === "anchor");
  assert(a !== undefined, "scene has an anchor");
  return a as GraphNode;
};

// --- the reading key's vocabulary ---------------------------------------------------

check("core verbs color as themselves (data-model §2); tail verbs are 'other'", () => {
  equal(categoryOf("references"), "references", "references");
  equal(categoryOf("supports"), "supports", "supports");
  equal(categoryOf("contradicts"), "contradicts", "contradicts");
  equal(categoryOf("elaborates"), "other", "a tail verb");
  equal(categoryOf("inspired-by"), "other", "another tail verb");
});

// --- the ghost graph ----------------------------------------------------------------

/** A node's orbit radius around `a`, un-squashing the elliptical vertical axis. */
const orbitR = (n: GraphNode, a: GraphNode): number =>
  Math.hypot(n.x - a.x, (n.y - a.y) / ORBIT_ASPECT);

check("anchor centered; authored on the inner orbit, ghosts on the outer halo", () => {
  const s = buildScene(
    input({
      connections: [
        neighbor({}),
        neighbor({ b2id: "01Y", path: "notes/y.md", title: "y", relation: "supports" }),
        neighbor({ b2id: "01Z", path: "notes/z.md", title: "z", relation: "part-of" }),
      ],
      ghosts: [ghost({})],
    }),
  );
  const a = anchorOf(s);
  equal(a.x, VIEW_W / 2, "anchor x");
  equal(a.y, VIEW_H / 2, "anchor y");
  const authored = s.nodes.filter((n) => n.kind === "note");
  const ghosts = s.nodes.filter((n) => n.kind === "ghost");
  equal(authored.length, 3, "three authored nodes");
  equal(ghosts.length, 1, "one ghost node");
  const r = orbitR(authored[0], a);
  assert(
    authored.every((n) => Math.abs(orbitR(n, a) - r) < 0.001),
    "authored share one orbit",
  );
  assert(orbitR(ghosts[0], a) > r + 60, "the ghost halo sits clearly outside the authored orbit");
});

check("ghosts are capped, dashed-latent, and score-tagged", () => {
  const many = Array.from({ length: 10 }, (_, i) =>
    ghost({ b2id: `01G${i}`, path: `notes/g${i}.md`, title: `g${i}`, score: 0.9 - i * 0.05 }),
  );
  const s = buildScene(input({ ghosts: many }));
  const ghosts = s.nodes.filter((n) => n.kind === "ghost");
  equal(ghosts.length, GHOST_LIMIT, "ghost cap");
  const ghostEdges = s.edges.filter((e) => e.ghost);
  equal(ghostEdges.length, GHOST_LIMIT, "one latent edge per ghost");
  assert(ghostEdges.every((e) => !e.arrow), "latent edges carry no arrowhead");
  equal(ghosts[0].sub, "0.90", "the score is the ghost's sub-label");
});

check("a neighbor with two edges gets one node and two separated curves", () => {
  const s = buildScene(
    input({
      connections: [
        neighbor({ relation: "references" }),
        neighbor({ relation: "elaborates", label: "elaborates" }),
      ],
    }),
  );
  equal(s.nodes.filter((n) => n.kind === "note").length, 1, "deduped node");
  const edges = s.edges.filter((e) => !e.ghost);
  equal(edges.length, 2, "both edges drawn");
  assert(edges.every((e) => e.cx !== null), "parallel edges bow apart (curved)");
  assert(edges[0].cx !== edges[1].cx || edges[0].cy !== edges[1].cy, "distinct control points");
});

check("arrowheads follow the verb — directed yes, symmetric no; direction is authored", () => {
  const s = buildScene(
    input({
      connections: [
        neighbor({ relation: "elaborates" }),
        neighbor({ b2id: "01Y", path: "notes/y.md", title: "y", relation: "contradicts" }),
        neighbor({ b2id: "01Z", path: "notes/z.md", title: "z", relation: "supports", direction: "inbound" }),
      ],
    }),
  );
  const by = (label: string) => {
    const e = s.edges.find((x) => x.label === label);
    assert(e !== undefined, `edge ${label}`);
    return e!;
  };
  assert(by("elaborates").arrow, "a directed (tail) verb has an arrow");
  assert(!by("contradicts").arrow, "the symmetric verb has none");
  equal(by("supports").from, "01Z", "an inbound edge is drawn from the neighbor…");
  equal(by("supports").to, "anchor", "…at the anchor (authored direction)");
});

check("resource and dangling targets are distinct node kinds, never hidden", () => {
  const resource: ResourceLink = {
    path: "resources/diagram.png",
    class: "image",
    relation: "references",
    origin: "inline",
    caption: "a diagram",
    embed: true,
    explanation: null,
  };
  const dangling: UnresolvedLink = {
    target: "Ebbinghaus",
    relation: "references",
    origin: "inline",
    explanation: null,
  };
  const s = buildScene(input({ resources: [resource], unresolved: [dangling] }));
  const res = s.nodes.find((n) => n.kind === "resource");
  const dang = s.nodes.find((n) => n.kind === "dangling");
  assert(res !== undefined, "resource node present");
  assert(dang !== undefined, "dangling node present");
  equal(res!.path, "resources/diagram.png", "resource opens its file");
  equal(res!.sub, "image", "resource sub is its class");
  equal(dang!.path, null, "dangling has nothing to open");
  equal(dang!.label, "[[Ebbinghaus]]", "dangling shows the authored target");
});

// --- cross-cutting ------------------------------------------------------------------

check("the scene is deterministic: same input, same scene", () => {
  const shared = input({
    connections: [neighbor({}), neighbor({ b2id: "01Y", path: "notes/y.md", title: "y", relation: "contradicts" })],
    ghosts: [ghost({}), ghost({ b2id: "01H", path: "notes/h.md", title: "h", score: 0.4 })],
  });
  equal(
    JSON.stringify(buildScene(shared)),
    JSON.stringify(buildScene(shared)),
    "identical scenes",
  );
});

check("an isolated note is just the anchor — no edges, no ghosts", () => {
  const s = buildScene(input({}));
  equal(s.nodes.length, 1, "anchor only");
  equal(s.edges.length, 0, "no edges");
});

check("every node lands inside the drawing space (with its own radius of margin)", () => {
  const many = Array.from({ length: 12 }, (_, i) =>
    neighbor({ b2id: `01N${i}`, path: `notes/n${i}.md`, title: `note ${i}`, relation: "references" }),
  );
  const s = buildScene(input({ connections: many, ghosts: [ghost({})] }));
  for (const n of s.nodes) {
    const r = NODE_R[n.kind];
    assert(n.x >= r && n.x <= VIEW_W - r, `${n.id} x in bounds (${n.x})`);
    assert(n.y >= r && n.y <= VIEW_H - r, `${n.id} y in bounds (${n.y})`);
  }
});

console.log(`graph: ${passed} checks passed`);
