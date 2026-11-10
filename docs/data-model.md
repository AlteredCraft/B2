# Data model

What a vault holds and how B2 reads it. This is the on-disk contract: the index schema is
derived from it, never the reverse. The rules it follows are in
[invariants.md](invariants.md) (S, W, L, G).

A vault is a folder. It holds three kinds of thing:

| Thing | What it is | What B2 does with it |
|---|---|---|
| **Note** | A `.md` file: optional YAML frontmatter, then a Markdown body | Reads it. Writes to it only on command (W3). |
| **Resource** | Any other file (a PDF, an image) | Reads it. Never writes its bytes. |
| **Folder** | The directory tree | Reads it live from disk. Never indexed. |

Anything with a dot-prefixed path segment (`.git/`, `.obsidian/`, `.b2/`, `.scratch.md`) is
outside the vault: never indexed, never listed, and never a destination B2 will write to.
The file stays on disk, untouched.

## Notes

A note needs nothing: no frontmatter and no id. Its identity is its vault-relative path
(L1), and its title is its filename (L2).

```markdown
---
type: concept
tags: [learning, memory]
created: 2026-06-20
b2_relations:
  - "contradicts [[notes/cramming-works]] — short-term recall only"
---
Spaced repetition schedules reviews at expanding intervals.

It builds on [[concepts/memory|human memory]].
```

Frontmatter keys B2 knows, all optional:

| Key | Meaning |
|---|---|
| `b2_relations` | Typed links (below). The only key B2 ever writes. |
| `type` | The kind of note (`concept`, `source`, `daily`). Free-form; display only. |
| `tags` | A list of strings. |
| `created` | Set by `b2 add` when it creates a note. |
| `updated` | Read if present. B2 never stamps it. |
| `title`, `description`, `aliases`, `provenance` | Kept, not used. A link written against an alias does not resolve yet. |

Every other key is preserved byte for byte, in order (W5). B2 reformats nothing in passing:
not quoting style, not indentation, not line endings.

## Links

A link is a directed connection from a note to another note or a resource. It lives in one
of two places, and B2 never copies between them (G2):

| Home | Syntax | What it can say |
|---|---|---|
| **Body** | `[[path]]`, `[[path\|alias]]`, `[text](path)`, `![[file]]` | That this note points at that one. Always an untyped `references` edge, `origin=inline`. |
| **Frontmatter** | A `b2_relations:` entry: `"<verb> [[path]] — explanation"` | A verb and an optional why, `origin=frontmatter`. |

- You write body links. B2 reads them and never adds one.
- `b2 link` and the app's **Link** append a `b2_relations:` entry and never touch the body.
  You can write entries by hand too.
- An entry with no verb reads as `references`. The explanation follows `—` or `:`. B2
  writes entries quoted, so they are always YAML-safe, with a bare `[[path]]` target.
- A typed entry may target a note the body already links. A different verb adds a second
  edge beside the plain one. The same verb collapses to one edge, and the frontmatter row
  wins because only it carries the explanation.
- To remove a link, delete its line. It is gone on the next reindex.

### The verbs

| Verb | Stance | Direction | Seen from the other end as |
|---|---|---|---|
| `references` | neutral | directed | referenced-by |
| `supports` | for | directed | supported-by |
| `contradicts` | against | symmetric | contradicts |

This closed core is the `b2 link --type` palette and what queries can rely on (G3). Any
other verb you write by hand (`elaborates`, `part-of`) is kept verbatim and treated as
opaque: no inverse label, no special traversal. Use a stance verb when the notes take a
position on each other; `references` is the honest default.

An edge is stored once, in its source note (G4). The inverse label is computed on read, and
B2 never writes a reciprocal link into the target.

### Dangling links

A link whose target matches no note and no resource is still an edge (G5). It keeps the
text you wrote, shows as unresolved in `b2 neighbors`, `b2 explain` and the app, and heals
on the next reindex once the target exists. Renaming a note outside B2 produces exactly
this.

## The edge record

What each link becomes in the index (the `edges` table):

| Field | Value |
|---|---|
| `src_path` | The note the link is written in. Always a note (G6). |
| `dst_path`, `dst_resource_path` | The note or resource it resolves to. Both NULL when dangling. |
| `dst_path_raw` | The target exactly as written. |
| `type` | The verb. `references` for every body link. |
| `origin` | `inline` or `frontmatter`. |
| `explanation` | The why. Frontmatter entries only. |
| `caption`, `embed` | A Markdown link's text or an embed's alt text, and whether it is an embed. |

An edge's id is derived from `(src, dst, type, occurrence)`, so nothing is written into the
file to identify it.

## Resources

Every non-`.md` file is a resource: inventoried by path, classed by extension (`text`,
`html`, `pdf`, `image`, `media`, `binary`), content-hashed, shown in the file tree, and
linkable from a note. `b2 mv` moves one and repairs inbound links exactly as it does for a
note. Importing one (a drag from Finder, `Vault::import_file`) copies its bytes verbatim; a
dropped `.md` arrives as a note.

Two limits today:

- A resource's content is not searched or embedded, so it never appears in search results
  or related notes.
- A resource can't be the source of a link, because it has nowhere to write one.

## Folders

B2 proxies the filesystem: create (an occupied target is refused), move (links into the
folder are repaired), delete. Empty folders work everywhere, because the tree is read from
disk and not from the index.

## The golden vault

`fixtures/golden-vault/` is the smallest vault that exercises all of this, and what the
integration tests run against. `notes/spaced-repetition.md` links `concepts/memory.md` twice:
once in the body (`references`, inline) and once in frontmatter (`supports`, with an
explanation). So `b2 neighbors concepts/memory` returns two rows.
