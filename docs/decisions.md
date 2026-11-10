# Decisions

The few decisions that shape B2 and would be expensive to reverse, with the reason for each.
[invariants.md](invariants.md) says what must stay true; this page says why.

- Each entry has a stable id that code comments cite (`ADR-0014`). The numbers have gaps:
  records whose content an invariant now carries in full were retired.
- Add an entry only for a decision that is expensive to reverse and whose reason can't be
  read off the code.
- When a decision changes, edit its entry. The issue that drove the change and the commit
  that shipped it are the history.

## ADR-0002. The vault is the truth; the index is a disposable projection

*Invariants S1 to S5.*

An AI layer over notes can own them (a database with an export button) or sit beside them.
Owning them is lock-in, and it turns every index bug into a data-loss risk. So
`index = projection of (the vault directory)`: delete `.b2/`, reindex, and the same index
comes back.

Ruled out: any durable state outside the Markdown (event logs, sidecar files, review
queues), and data migrations. A feature that needs durable state finds a home in the
Markdown or does without.

## ADR-0003. A note's identity is its path

*Invariants L1, L3. GH #170.*

Until 2026-08 B2 stamped a `b2id:` into every note so it could follow a file moved outside
B2. That cost an unbidden write, a machine key in every file, a collision subsystem and an
exception to S3, and no link ever named the id. Links are already written by path, so the
path is the identity.

The trade: a move made outside B2 is a delete plus a create, and inbound links dangle
visibly until repaired. Content-addressed vectors (M4) make that cheap, because nothing
re-embeds.

## ADR-0004. Markdown is the only thing B2 writes, and only on command

*Invariants W1 to W5.*

"Your files stay yours" is only true if it is mechanical. A tool that tidies frontmatter or
injects its own syntax on read has taken ownership of the vault. So reading writes nothing,
the writes are a closed list (W3), and the body is never B2's to author.

Ruled out: B2-authored body sections, typed-link syntax parsed out of prose, and
reformatting in passing. Every connection must be expressible as Markdown a person could
have typed.

## ADR-0005. AI sits behind two seams; the core is model-free

*Invariants M1, M5.*

Build for tomorrow's model. Machinery that compensates for today's model becomes the thing
that blocks a better one, and a model dependency inside the engine makes its tests slow and
nondeterministic. So `Embedder` and `LlmProvider` are traits in `b2-core`, the engine runs
on fakes, and the real models live in `b2-embed` (candle, in-process) and `b2-llm`.

What follows:

- The embedder carries the index's identity (M2). Chat carries none and stores nothing.
- The embedding model is fetched by an explicit `b2 init`, never by a surprise download.
- Per-pair adjudication, query expansion and rerankers stay out until the eval harness says
  one pays. Each would arrive as a new seam.

## ADR-0010. Typed links: two homes, a three-verb core, no suggestion queue

*Invariants G1 to G6.*

Typed relations need a home that survives B2 being deleted, without turning prose into a
markup dialect. So body links stay plain, and a verb and its explanation live only in
frontmatter `b2_relations:`.

The core is three verbs because stance is the one thing similarity can't infer. The vectors
already say two notes are related; whether one supports or contradicts the other is
something only I know.

Ruled out: an inbox of machine-proposed links with accept and reject state (it would make
the index authoritative for something), stored reciprocal links, and edge ids or provenance
written into files.

## ADR-0011. No async

B2 is a local tool whose I/O is a file walk, a SQLite handle and one HTTP stream at a time.
A runtime buys nothing and costs a large dependency tree. Everything is sync, and cancelling
a stream is returning early from a blocking read.

`make no-tokio` gates it, because one dependency added with default features can pull the
whole async HTTP stack back in while every other check stays green. That happened once
(GH #174). The only runtime in the tree is Tauri's, in the desktop host.

## ADR-0012. One typed façade, two dumb adapters

*Invariant E3.*

Two front ends over one engine grow into two subtly different products unless the boundary
is enforced. So `Vault` is the only API, each CLI or desktop command is parse, one call,
render, and the desktop reuses the CLI's `--json` view types as its IPC contract. A fix in
the core fixes both, and the app inherits the engine's tests.

## ADR-0014. Discovery always serves the ranked list

*Invariant D1. GH #196, #197, #200.*

Discovery first shipped with a z-score gate: a candidate had to stand out from the anchor's
other candidates to be shown. On a real single-subject vault, 16 of 17 panes went dark while
the ranking was correct throughout. The fault was the gate, not its tuning. A statistic over
one population can't tell "nothing is related" from "everything is related". So the list is
always served, the z only grades it, and I am the judge.

Before adding any rule that hides or folds candidates: it must win the measured bake-off on
both eval corpora and on a real vault, and "no fold" is allowed to win. The first candidate
tried (mutual nearest neighbours) lost.

## ADR-0015. A served search result is a claim of evidence

*Invariant D2. GH #201, #202, #206.*

The vector half of search always has a nearest neighbour, and rank fusion throws away the
absolute scores. Left alone, the pipeline can't answer zero: a nonsense query got ten
confident results. So search carries two absolute readings beside the fused order (lexical
coverage and best cosine) and serves rows only when one of them holds. Two signals, because
one can't tell "nothing matches" from "everything matches".

What it taught:

- The first lexical rule, a document-frequency ceiling, read clean on the labelled corpus
  and then called `drone` and `comb` stopwords in a beekeeping vault. The rule changed (IDF
  as a weight); the number was not re-tuned. A threshold read off one corpus describes that
  corpus.
- Folding the weak tail of a real query's results was measured and doesn't pay. The fused
  order isn't an evidence order, so that complaint is reranking work, not disclosure work.
