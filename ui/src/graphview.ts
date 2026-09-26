// The connection graph's paint — the centre pane's third mode — and the two chips the
// reading bar shares with the graph bar (the graph toggle, Edit). Scene geometry is
// graph.ts's; this is only markup.

import { escapeHtml } from "./escape.ts";
import type { AppState } from "./state.ts";
import { coverage } from "./coverage.ts";
import { displayKeys } from "./bindings.ts";
import { icon, resourceIcon, sceneIcon } from "./icons.ts";
import type { NoteView } from "./types.ts";
import {
  buildScene,
  NODE_R,
  VIEW_H,
  VIEW_W,
  type Category,
  type GraphEdge,
  type GraphNode,
  type GraphScene,
} from "./graph.ts";

// --- the anchored ghost graph (GH #22) ----------------------------------------------
//
// The center pane's third mode: the open note's typed neighborhood as hand-rolled,
// deterministic SVG — scene geometry from `graph.ts` (pure, unit-tested), markup
// here, clicks delegated in main.ts. The reading key: color = edge category, solid =
// authored / dashed teal = latent (`similar`), disc = note / square = resource /
// dashed hollow = dangling. Everything renders from state the note-open already
// fetched, so entering the graph costs no IPC.

/** The graph toggle chip, shared by the reading bar (off) and the graph bar (on).
 *
 *  Its chord comes out of the live registry rather than being spelled here: ⌘G is
 *  rebindable (#121), and a tooltip naming the shipped default would be wrong for exactly
 *  the user who changed it. */
export function graphToggleHtml(active: boolean): string {
  const chord = escapeHtml(displayKeys(["graph.toggle"]));
  return `<button id="graph-toggle" class="source-toggle graph-toggle${active ? " is-active" : ""}" data-toggle-graph
      aria-pressed="${active}" aria-label="${active ? "Back to reading" : "Show the connection graph"}"
      title="${
        active
          ? `Back to reading — ${chord} or Esc`
          : `Show the connection graph — ${chord}; nodes are Tab-reachable, ⏎ opens`
      }">${icon("diagram-3")}</button>`;
}

/** The Edit chip, shared by the reading bar and the graph bar. Its chord comes out of the
 *  live registry for `graphToggleHtml`'s reason; `title` overrides the tooltip when the
 *  chip is disabled for a reason worth naming. */
export function editToggleHtml(disabled: boolean, title?: string): string {
  const hint = title ?? `Edit this note — ${displayKeys(["edit.toggle"])} (autosaves as you type)`;
  return `<button id="edit-toggle" class="edit-toggle" data-toggle-edit${
    disabled ? " disabled" : ""
  } title="${escapeHtml(hint)}">Edit</button>`;
}

/** Fixed-point SVG coordinate — keeps the markup compact and diff-stable. */
function px(v: number): string {
  return (Math.round(v * 10) / 10).toString();
}

/** One edge's path (a straight segment, or the parallel-separating quadratic). */
function edgePathD(e: GraphEdge): string {
  return e.cx === null || e.cy === null
    ? `M ${px(e.x1)} ${px(e.y1)} L ${px(e.x2)} ${px(e.y2)}`
    : `M ${px(e.x1)} ${px(e.y1)} Q ${px(e.cx)} ${px(e.cy)} ${px(e.x2)} ${px(e.y2)}`;
}

function edgeHtml(e: GraphEdge): string {
  if (e.ghost) {
    return `<path class="gedge is-ghost" d="${edgePathD(e)}"/>`;
  }
  const verb = e.label.replace(/[^a-z0-9-]/gi, "");
  const marker = e.arrow ? ` marker-end="url(#garr-${e.category})"` : "";
  const label = `<text class="gedge-label cat-${e.category}" x="${px(e.lx)}" y="${px(
    e.ly - 6,
  )}">${escapeHtml(e.label)}</text>`;
  return `<path class="gedge cat-${e.category} verb-${verb}" d="${edgePathD(e)}"${marker}/>${label}`;
}

/** A node's shape + glyph, by kind (labels are added by the group builder). */
function nodeShapeHtml(n: GraphNode): string {
  const x = px(n.x);
  const y = px(n.y);
  switch (n.kind) {
    case "anchor":
      return `<circle class="gring" cx="${x}" cy="${y}" r="${NODE_R.anchor}"/>
        <circle class="gshape" cx="${x}" cy="${y}" r="${NODE_R.anchor - 7}"/>
        <circle class="gcore" cx="${x}" cy="${y}" r="7"/>`;
    case "resource": {
      const s = NODE_R.resource - 2;
      return `<rect class="gshape" x="${px(n.x - s)}" y="${px(n.y - s)}" width="${2 * s}" height="${2 * s}" rx="9"/>
        ${sceneIcon(resourceIcon(n.sub), n.x, n.y, 16, "gglyph")}`;
    }
    case "dangling":
      return `<circle class="gshape" cx="${x}" cy="${y}" r="${NODE_R.dangling}"/>
        ${sceneIcon("exclamation-triangle", n.x, n.y, 16, "gglyph")}`;
    default:
      return `<circle class="gshape" cx="${x}" cy="${y}" r="${NODE_R[n.kind]}"/>`;
  }
}

/** The tooltip line(s) for a node — also the activation affordance's explanation.
 *  Phrased for both hands: an activatable node says "⏎", because the graph is
 *  reachable by Tab and arrow keys too (K1, GH #78), not by mouse alone. */
function nodeTitle(n: GraphNode): string {
  switch (n.kind) {
    case "anchor":
      return `${n.full} — the open note. Click or ⏎ to return to reading.`;
    case "ghost":
      // The strength figure when there is one; a bare "similar but not linked"
      // otherwise. "similarity ?" claimed a measurement existed and was merely
      // unavailable — an ungraded candidate was never measured at all.
      return `${n.full} — similar but not linked${
        n.sub ? ` (${n.sub} above this note's other candidates)` : ""
      }. Click or ⏎ to link it; right-click (or ${displayKeys(["menu.open"])}) for more.`;
    case "dangling":
      return `${n.full} resolves to no note or file — fix the link in the note.`;
    case "resource":
      return `${n.full} (${n.sub ?? "file"}) — click or ⏎ to open.`;
    default:
      return `${n.full} — click or ⏎ to open.`;
  }
}

/** The accessible name for a focusable node — what a screen reader announces, and
 *  what the node's own `<title>` can't be (that one is the mouse tooltip prose). */
function nodeAriaLabel(n: GraphNode): string {
  switch (n.kind) {
    case "anchor":
      return `${n.full} — the open note; back to reading`;
    case "ghost":
      return `${n.full} — similar but unlinked; link it`;
    case "resource":
      return `${n.full} — open this file`;
    default:
      return `${n.full} — open this note`;
  }
}

/**
 * One scene node as an interactive `<g>`, its incident edges inside it so a pure-CSS
 * hover lights the node *and* its edges while the rest of the scene dims. The click
 * affordance rides existing delegation: notes reuse `data-open`, resources
 * `data-open-resource`; ghosts get `data-ghost-link` (→ the link palette) plus the
 * `data-card-*` pair the right-click menu reads; the anchor toggles back to reading.
 *
 * An activatable node is also a **keyboard** control (K1, GH #78): `tabindex="0"` puts
 * it in the Tab order and `role="button"` says what it is, so a graph is walkable and
 * openable with no mouse. main.ts turns ⏎/Space on a focused node into the same click
 * these attributes already answer — one activation path, not two. A `dangling` node
 * stays inert: it opens nothing (there's nothing there), so it isn't a tab stop.
 *
 * It also carries `data-gnode` — the scene id (graph.ts), which is what the note pane's
 * focus restoration re-finds it *by* after an `innerHTML` swap destroys the element
 * itself (GH #91). The discovery row's `data-side-row` for the graph: an identity that
 * outlives the repaint, not a pointer into it.
 */
function nodeGroupHtml(n: GraphNode, edges: GraphEdge[], order: number): string {
  const attrs: string[] = [`class="gnode is-${n.kind}"`, `style="--i:${order}"`];
  if (n.kind === "note" && n.path) attrs.push(`data-open="${escapeHtml(n.path)}"`);
  if (n.kind === "anchor") attrs.push(`data-toggle-graph="1"`);
  if (n.kind === "resource" && n.path) attrs.push(`data-open-resource="${escapeHtml(n.path)}"`);
  if (n.kind === "ghost" && n.path) {
    attrs.push(
      `data-ghost-link="${escapeHtml(n.path)}"`,
      `data-card-path="${escapeHtml(n.path)}"`,
      `data-card-title="${escapeHtml(n.title ?? "")}"`,
    );
  }
  if (n.kind !== "dangling") {
    attrs.push(
      `tabindex="0"`,
      `role="button"`,
      `aria-label="${escapeHtml(nodeAriaLabel(n))}"`,
      `data-gnode="${escapeHtml(n.id)}"`,
    );
  }
  const r = NODE_R[n.kind];
  // Text goes on the side of the node facing *away* from the anchor (above for the
  // upper half of the scene), so a label never sits in its own edge's path.
  const above = n.kind !== "anchor" && n.y < VIEW_H / 2 - 20;
  const label = `<text class="gnode-label" x="${px(n.x)}" y="${px(
    above ? n.y - r - 14 : n.y + r + 18,
  )}">${escapeHtml(n.label)}</text>`;
  const sub = n.sub
    ? `<text class="gnode-sub" x="${px(n.x)}" y="${px(
        above ? n.y - r - 29 : n.y + r + 33,
      )}">${escapeHtml(n.sub)}</text>`
    : "";
  return `<g ${attrs.join(" ")}>
      <title>${escapeHtml(nodeTitle(n))}</title>
      ${edges.map(edgeHtml).join("")}
      ${nodeShapeHtml(n)}
      ${label}${sub}
    </g>`;
}

/** The honest ghost-halo caveat (coverage.ts's tiers, like `searchCaveat`, #26): why there are
 *  no ghosts right now, or null when there are (or when silence is the honest state). */
function ghostHintHtml(state: AppState): string {
  if (state.similar.length > 0) return "";
  if (state.discoveringSimilar)
    return `<div class="graph-hint is-scanning"><span class="spinner"></span>scanning for latent connections…</div>`;
  const c = coverage(state);
  if (!c.model)
    return `<div class="graph-hint">ghost connections need the semantic model — run <code>b2 init</code>, then Reindex</div>`;
  if (c.embedded === "none" || c.embedded === "partial")
    return `<div class="graph-hint">ghosts appear once the vault is embedded — Reindex</div>`;
  return "";
}

/** The centered guidance when there's nothing to draw (the anchor always shows). */
function graphEmptyHtml(state: AppState, scene: GraphScene): string {
  if (scene.edges.length > 0) return "";
  if (state.discoveringSimilar) return "";
  return `<div class="graph-empty"><p>No connections yet.</p>
    <p class="muted">B2 floats similar-but-unlinked notes here as ghosts — click one to make the connection real.</p></div>`;
}

/** The reading key, one quiet strip: verb colors, edge states, node shapes. */
function graphLegendHtml(): string {
  const cats: [Category, string][] = [
    ["references", "references"],
    ["supports", "supports"],
    ["contradicts", "contradicts"],
  ];
  const dots = cats
    .map(([c, label]) => `<span class="leg"><span class="leg-dot cat-${c}"></span>${label}</span>`)
    .join("");
  return `<div class="graph-legend" aria-hidden="true">${dots}
      <span class="leg"><span class="leg-dash"></span>ghost (unlinked)</span>
      <span class="leg"><span class="leg-square"></span>file</span>
      <span class="leg"><span class="leg-broken">${icon("exclamation-triangle", {
        size: 11,
      })}</span>broken</span>
    </div>`;
}

/** Arrowhead markers, one per category (an SVG marker can't inherit its edge's
 *  stroke everywhere yet). */
function graphDefsHtml(): string {
  const cats: Category[] = ["references", "supports", "contradicts", "other"];
  const arrow = (id: string, cls: string) =>
    `<marker id="${id}" viewBox="0 0 10 10" refX="8.5" refY="5" markerWidth="7.5" markerHeight="7.5" orient="auto-start-reverse">
       <path d="M0 0.8 L9.5 5 L0 9.2 z" class="${cls}"/>
     </marker>`;
  return `<defs>${cats.map((c) => arrow(`garr-${c}`, `garr cat-${c}`)).join("")}</defs>`;
}

/**
 * The graph pane — the note pane's third mode (Reading / Editing / Graph). Bar:
 * the same action chips as reading; stage: the SVG scene (fills the pane,
 * `viewBox`-scaled) with overlay hints; footer: the reading key.
 */
export function graphPaneHtml(state: AppState, n: NoteView): string {
  const scene = buildScene({
    anchor: { path: n.path, title: n.title },
    connections: state.connections,
    resources: state.resourceLinks,
    unresolved: state.unresolved,
    ghosts: state.similar,
  });

  // Edges live inside their node's group (hover affordance); the anchor renders
  // last so it always paints on top of edge crossings.
  const byNode = new Map<string, GraphEdge[]>();
  for (const e of scene.edges) {
    const owner = e.from === "anchor" ? e.to : e.from;
    const list = byNode.get(owner) ?? [];
    list.push(e);
    byNode.set(owner, list);
  }
  // Paint order: ghosts lowest (their long dashed spokes must pass *under* the
  // authored orbit), authored above them, the anchor on top of everything. The
  // stagger index is narrative, not paint, order: authored pops first, ghosts after.
  const authoredNodes = scene.nodes.filter((node) => node.kind !== "anchor" && node.kind !== "ghost");
  const ghostNodes = scene.nodes.filter((node) => node.kind === "ghost");
  const anchor = scene.nodes.find((node) => node.kind === "anchor");
  const groups = [
    ...ghostNodes.map((node, i) =>
      nodeGroupHtml(node, byNode.get(node.id) ?? [], authoredNodes.length + 1 + i),
    ),
    ...authoredNodes.map((node, i) => nodeGroupHtml(node, byNode.get(node.id) ?? [], i + 1)),
    ...(anchor ? [nodeGroupHtml(anchor, [], 0)] : []),
  ].join("");

  return `<div class="graph-view">
      <div class="graph-bar">
        <div class="note-bar-actions">
          ${graphToggleHtml(true)}
          ${editToggleHtml(state.loading)}
        </div>
      </div>
      <div class="graph-stage">
        <svg class="graph-svg" viewBox="0 0 ${VIEW_W} ${VIEW_H}" preserveAspectRatio="xMidYMid meet"
             role="img" aria-label="Connection graph for ${escapeHtml(n.title ?? n.path)}">
          ${graphDefsHtml()}
          ${groups}
        </svg>
        ${graphEmptyHtml(state, scene)}
        ${ghostHintHtml(state)}
      </div>
      ${graphLegendHtml()}
    </div>`;
}
