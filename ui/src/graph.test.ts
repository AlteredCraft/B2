// The graph scene builder (graph.ts), pinned. Pure math + data shaping — no DOM —
// so node runs it straight off the source via its native type-stripping: `npm test`.
// Same dependency-free idiom as panes.test.ts (hand-rolled assert).
//
// What's worth pinning: the lens semantics (which edges each lens keeps and *where*
// meaning places them), the visual-language invariants (ghost cap, orbit separation,
// arrowheads only on directed verbs), and determinism — the properties the SVG
// renderer builds on, not pixel positions.

import {
  AXIS_BOW,
  buildScene,
  categoryOf,
  GHOST_LIMIT,
  lensKeeps,
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
    anchor: { path: "notes/anchor.md", title: "anchor", created: "2026-07-01" },
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

check("every core verb maps to its data-model §2 category; tail verbs are 'other'", () => {
  equal(categoryOf("references"), "referential", "references");
  equal(categoryOf("relates"), "referential", "relates");
  equal(categoryOf("elaborates"), "expository", "elaborates");
  equal(categoryOf("supports"), "evidential", "supports");
  equal(categoryOf("refutes"), "evidential", "refutes");
  equal(categoryOf("contradicts"), "evidential", "contradicts");
  equal(categoryOf("example-of"), "structural", "example-of");
  equal(categoryOf("part-of"), "structural", "part-of");
  equal(categoryOf("supersedes"), "versioning", "supersedes");
  equal(categoryOf("derived-from"), "versioning", "derived-from");
  equal(categoryOf("inspired-by"), "other", "a tail verb");
});

check("lens filters: all keeps everything, lineage keeps versioning, argument keeps evidential", () => {
  assert(lensKeeps("all", "references") && lensKeeps("all", "supersedes"), "all keeps all");
  assert(lensKeeps("lineage", "supersedes") && lensKeeps("lineage", "derived-from"), "lineage in");
  assert(!lensKeeps("lineage", "references") && !lensKeeps("lineage", "supports"), "lineage out");
  assert(
    lensKeeps("argument", "supports") && lensKeeps("argument", "refutes") && lensKeeps("argument", "contradicts"),
    "argument in",
  );
  assert(!lensKeeps("argument", "supersedes") && !lensKeeps("argument", "elaborates"), "argument out");
});

// --- the all lens (ghost graph) -----------------------------------------------------

/** A node's orbit radius around `a`, un-squashing the elliptical vertical axis. */
const orbitR = (n: GraphNode, a: GraphNode): number =>
  Math.hypot(n.x - a.x, (n.y - a.y) / ORBIT_ASPECT);

check("all lens: anchor centered; authored on the inner orbit, ghosts on the outer halo", () => {
  const s = buildScene(
    "all",
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

check("all lens: ghosts are capped, dashed-latent, and score-tagged", () => {
  const many = Array.from({ length: 10 }, (_, i) =>
    ghost({ b2id: `01G${i}`, path: `notes/g${i}.md`, title: `g${i}`, score: 0.9 - i * 0.05 }),
  );
  const s = buildScene("all", input({ ghosts: many }));
  const ghosts = s.nodes.filter((n) => n.kind === "ghost");
  equal(ghosts.length, GHOST_LIMIT, "ghost cap");
  const ghostEdges = s.edges.filter((e) => e.ghost);
  equal(ghostEdges.length, GHOST_LIMIT, "one latent edge per ghost");
  assert(ghostEdges.every((e) => !e.arrow), "latent edges carry no arrowhead");
  equal(ghosts[0].sub, "0.90", "the score is the ghost's sub-label");
});

check("all lens: a neighbor with two edges gets one node and two separated curves", () => {
  const s = buildScene(
    "all",
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

check("all lens: arrowheads follow the verb — directed yes, symmetric no; direction is authored", () => {
  const s = buildScene(
    "all",
    input({
      connections: [
        neighbor({ relation: "elaborates" }),
        neighbor({ b2id: "01Y", path: "notes/y.md", title: "y", relation: "relates" }),
        neighbor({ b2id: "01Z", path: "notes/z.md", title: "z", relation: "supports", direction: "inbound" }),
      ],
    }),
  );
  const by = (label: string) => {
    const e = s.edges.find((x) => x.label === label);
    assert(e !== undefined, `edge ${label}`);
    return e!;
  };
  assert(by("elaborates").arrow, "directed verb has an arrow");
  assert(!by("relates").arrow, "symmetric verb has none");
  equal(by("supports").from, "01Z", "an inbound edge is drawn from the neighbor…");
  equal(by("supports").to, "anchor", "…at the anchor (authored direction)");
});

check("all lens: resource and dangling targets are distinct node kinds, never hidden", () => {
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
  const s = buildScene("all", input({ resources: [resource], unresolved: [dangling] }));
  const res = s.nodes.find((n) => n.kind === "resource");
  const dang = s.nodes.find((n) => n.kind === "dangling");
  assert(res !== undefined, "resource node present");
  assert(dang !== undefined, "dangling node present");
  equal(res!.path, "resources/diagram.png", "resource opens its file");
  equal(res!.sub, "image", "resource sub is its class");
  equal(dang!.path, null, "dangling has nothing to open");
  equal(dang!.label, "[[Ebbinghaus]]", "dangling shows the authored target");
});

// --- the lineage lens ---------------------------------------------------------------

check("lineage: outbound versioning targets are the past (left), inbound the future (right)", () => {
  const s = buildScene(
    "lineage",
    input({
      connections: [
        neighbor({ b2id: "01OLD", path: "notes/old.md", title: "old", relation: "supersedes", created: "2026-05-01" }),
        neighbor({
          b2id: "01NEW",
          path: "notes/new.md",
          title: "new",
          relation: "supersedes",
          direction: "inbound",
          created: "2026-07-10",
        }),
        neighbor({ b2id: "01REF", path: "notes/ref.md", title: "ref", relation: "references" }),
      ],
    }),
  );
  const a = anchorOf(s);
  const old = s.nodes.find((n) => n.id === "01OLD");
  const nw = s.nodes.find((n) => n.id === "01NEW");
  assert(old !== undefined && nw !== undefined, "both versioning nodes placed");
  assert(old!.x < a.x, "what the anchor supersedes sits left (older)");
  assert(nw!.x > a.x, "what supersedes the anchor sits right (newer)");
  equal(old!.sub, "2026-05-01", "lineage nodes are dated (the time axis)");
  equal(a.sub, "2026-07-01", "the anchor is dated too");
  assert(!s.nodes.some((n) => n.id === "01REF"), "non-versioning edges are out of this lens");
  equal(s.edges.length, 2, "only the versioning edges are drawn");
});

// --- the argument lens --------------------------------------------------------------

check("argument: supporters left, refuters right, contradicts on the vertical fault line", () => {
  const s = buildScene(
    "argument",
    input({
      connections: [
        neighbor({ b2id: "01S", path: "notes/s.md", title: "s", relation: "supports", direction: "inbound" }),
        neighbor({ b2id: "01R", path: "notes/r.md", title: "r", relation: "refutes", direction: "inbound" }),
        neighbor({ b2id: "01C", path: "notes/c.md", title: "c", relation: "contradicts" }),
        neighbor({ b2id: "01E", path: "notes/e.md", title: "e", relation: "elaborates" }),
      ],
    }),
  );
  const a = anchorOf(s);
  const at = (id: string) => {
    const n = s.nodes.find((x) => x.id === id);
    assert(n !== undefined, `node ${id}`);
    return n!;
  };
  assert(at("01S").x < a.x, "supporter left of the claim");
  assert(at("01R").x > a.x, "refuter right of the claim");
  equal(at("01C").x, a.x, "contradicts sits on the claim's vertical axis");
  assert(at("01C").y !== a.y, "…above or below it (the fault line)");
  assert(!s.nodes.some((n) => n.id === "01E"), "non-evidential edges are out of this lens");
});

check("argument: fault-line edges bow off the axis (first right, then left) so they read as edges, not the axis", () => {
  const s = buildScene(
    "argument",
    input({
      connections: [
        neighbor({ b2id: "01C", path: "notes/c.md", title: "c", relation: "contradicts" }),
        neighbor({ b2id: "01D", path: "notes/d.md", title: "d", relation: "contradicts" }),
      ],
    }),
  );
  const a = anchorOf(s);
  const edgeTo = (id: string): GraphScene["edges"][number] => {
    const e = s.edges.find((x) => x.to === id || x.from === id);
    assert(e !== undefined, `edge for ${id}`);
    return e!;
  };
  const c = edgeTo("01C");
  const d = edgeTo("01D");
  assert(c.cx !== null && d.cx !== null, "both fault-line edges curve, not run straight along the axis");
  assert((c.cx as number) > a.x, "the first contradicts edge bows right of the axis");
  assert((d.cx as number) < a.x, "the second bows left");
  equal(Math.round((c.cx as number) - a.x), AXIS_BOW, "…by AXIS_BOW");
  assert(c.hideLabel === true && d.hideLabel === true, "the verb moves to the axis caption, not a per-edge pill");
});

check("argument: evidential edges at resources and dangling targets stay visible", () => {
  // A PDF that supports a claim, and a broken refutes link, belong on the map —
  // filtering is by verb, never by target kind (the #12/#22 repair surface).
  const s = buildScene(
    "argument",
    input({
      resources: [
        {
          path: "resources/paper.pdf",
          class: "pdf",
          relation: "supports",
          origin: "frontmatter",
          caption: null,
          embed: false,
          explanation: null,
        },
      ],
      unresolved: [{ target: "Lost note", relation: "refutes", origin: "inline", explanation: null }],
    }),
  );
  const a = anchorOf(s);
  const res = s.nodes.find((n) => n.kind === "resource");
  const dang = s.nodes.find((n) => n.kind === "dangling");
  assert(res !== undefined && res!.x < a.x, "the supporting PDF sits on the supports side");
  assert(dang !== undefined && dang!.x > a.x, "the broken refutes sits on the refutes side");
});

// --- cross-cutting ------------------------------------------------------------------

check("the scene is deterministic: same input, same scene", () => {
  const shared = input({
    connections: [neighbor({}), neighbor({ b2id: "01Y", path: "notes/y.md", title: "y", relation: "contradicts" })],
    ghosts: [ghost({}), ghost({ b2id: "01H", path: "notes/h.md", title: "h", score: 0.4 })],
  });
  equal(
    JSON.stringify(buildScene("all", shared)),
    JSON.stringify(buildScene("all", shared)),
    "identical scenes",
  );
});

check("an isolated note is just the anchor — no edges, no ghosts", () => {
  const s = buildScene("all", input({}));
  equal(s.nodes.length, 1, "anchor only");
  equal(s.edges.length, 0, "no edges");
});

check("every node lands inside the drawing space (with its own radius of margin)", () => {
  const many = Array.from({ length: 12 }, (_, i) =>
    neighbor({ b2id: `01N${i}`, path: `notes/n${i}.md`, title: `note ${i}`, relation: "references" }),
  );
  const s = buildScene("all", input({ connections: many, ghosts: [ghost({})] }));
  for (const n of s.nodes) {
    const r = NODE_R[n.kind];
    assert(n.x >= r && n.x <= VIEW_W - r, `${n.id} x in bounds (${n.x})`);
    assert(n.y >= r && n.y <= VIEW_H - r, `${n.id} y in bounds (${n.y})`);
  }
});

console.log(`graph: ${passed} checks passed`);
