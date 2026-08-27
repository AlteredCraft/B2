# Search and similarity

How B2 decides what's related, in plain language, for everyone who uses B2. Read this to
understand what search and the related-notes panel are doing, what the strength dots mean,
and what the honest limits are. No math background assumed. Each idea gets its real name the
first time, and the machinery lives in [index-engine.md](index-engine.md) when you want it.

Everything here runs on your machine. Finding and relating notes uses no network, no API key,
and costs nothing per note or per search. The only heavy step is the one-time `b2 reindex`
that reads your vault. Your notes stay plain Markdown files that you own; B2 writes nothing
to them unless you ask it to.

## 1. How B2 reads a note

Before anything can be found or related, every note gets turned into something a computer can
compare. That happens in two steps.

**Step one: your note is cut into passages.** A long note isn't one idea, it's many. If B2
treated a 4,000-word article as a single lump, a paragraph deep inside it about one narrow
thing would be drowned out by everything around it. So B2 cuts each note into passages of
roughly 450 tokens' worth of text (about 340 words), preferring to cut at headings, then at
paragraph breaks, and never in the middle of a code block or a table. Consecutive passages
overlap by about 15%, so an idea that straddles a boundary isn't sliced in half.

> The term: a passage is called a **chunk**. The rules for where to cut are the **chunker**.
> Deep dive: [index-engine.md §1](index-engine.md).

**Step two: each passage becomes a list of numbers.** B2 feeds every passage to a small
language model that runs locally on your machine. The model reads the passage and outputs 768
numbers. That list of numbers is the passage's position on a map of meaning: passages about
similar things land near each other, passages about different things land far apart. The
numbers themselves are meaningless to a human; only the distances between them matter.

> The term: the list of numbers is an **embedding**, or a **vector**. The model is
> `bge-base-en-v1.5`. It runs entirely offline, downloaded once by `b2 init`. "768 numbers"
> is its **dimension**.

One pass, done once. `b2 reindex` walks your vault, cuts each note into chunks, embeds each
chunk, and files the results three ways: a keyword index (exact words), the vectors
(meaning), and your links (the graph). Everything afterwards, every search and every
related-notes panel, just reads that index back. The index is disposable: delete it,
rebuild, and you get the same thing back.

## 2. Meaning as a map

This is the one idea worth internalizing. Everything else in B2 is built on it.

Picture every passage in your vault as a pin on a map. The model places the pins: passages
about espresso end up in one neighborhood, passages about volcanoes in another. B2 never
"understands" your notes. It only measures how far apart the pins are. Close pins mean the
model thinks the passages are about similar things.

The real map has 768 directions instead of two, which nobody can picture, but nothing about
how it works depends on that. Distance is distance.

B2 measures the closeness of two pins as a number between -1 and 1, where 1 means "pointing
the same way". Two notes at 0.79 are strongly alike; two at 0.35 have little to do with each
other. That number is the raw material for everything below.

> The term: the closeness number is **cosine similarity**. B2 stores and compares it
> internally as a distance. You see it in the app only indirectly, as the strength dots on a
> card.

Why raw closeness numbers are never shown to you: "0.79" only means something relative to the
rest of your vault. In a vault of tightly related research notes, 0.79 might be unremarkable;
in a scrapbook of unrelated topics it would be a standout. So B2 never grades a relationship
by its raw number. See [how the list is graded](#6-reading-the-strength-dots).

## 3. Search: two opinions, fused

When you type a query, B2 asks two completely different systems the same question, then
merges their answers.

- **Opinion 1: the words.** A classic keyword search: which passages actually contain your
  search words? It rewards rare words heavily. If you search "moleskin", the one note
  containing it wins outright. It understands nothing; it counts.
  > The term: **BM25**, the standard keyword-ranking formula, running on SQLite's full-text
  > index.
- **Opinion 2: the meaning.** Your query is turned into a pin on the same map as your
  passages, and B2 finds the nearest ones. It never needs to share a single word with the
  note it finds; it matches ideas.
  > The term: **vector search**, also called semantic search or **KNN** ("k nearest
  > neighbors").

**Why you need both.** Each is blind where the other sees. Search "how do leaves turn light
into food" and keyword search returns nothing useful: your note says "photosynthesis" and
shares almost no words with the question. Meaning search nails it. Now search for an exact
error code or a person's surname: meaning search returns a vague neighborhood, while keyword
search lands on the one note that literally contains it.

**The merge uses positions, not scores.** The two systems produce numbers on incomparable
scales, so B2 ignores the scores entirely and merges by rank: each list awards a note points
based on where it placed, and the points are added. A note that both systems like beats a
note only one of them loves.

> The term: the merge is **Reciprocal Rank Fusion (RRF)**. A note at position r in a list
> scores `1 / (60 + r)`. The 60 is a deliberate damper; without it, a first place would
> dominate everything else. Searching both ways and merging is called **hybrid search**.

### When nothing matches: search answers zero

That takes deliberate machinery, because the pipeline above can't do it by itself. The
meaning half always has a nearest neighbor ("nearest" is a fact about your vault, not about
your query), and the merge throws away both systems' actual scores in favor of positions. So
by the time anything could ask "is any of this actually relevant?", the two numbers that
could answer are gone. Left alone, typing `Fasdfadsf` would get ten confident-looking
results.

So B2 reads two absolute signals beside the merged order, and serves results only if either
one says the vault holds something:

1. **Do your words appear at all, weighted by how rare they are?** Not "does anything match":
   almost any query shares some word with almost any vault, and a query that matches only
   through *a*, *to*, and *my* has told you nothing. Each word is weighted by how many
   passages contain it, so a word in most of them counts for nearly nothing and a word in
   none counts for the most there is. B2 asks what share of your query's own weight the vault
   actually carries. Stopwords are therefore measured, never a shipped list of words to
   ignore, which matters in a single-subject vault: no fixed list would know that *comb* is a
   content word in a beekeeping vault.
2. **Or is the nearest passage genuinely near?** The plain distance, before ranks replaced
   it: a backstop for the real questions your notes answer in words you didn't use.

Two independent signals, deliberately, because one couldn't do it: a single test can't tell
"nothing here is related" from "everything here is related". That is the same lesson the
related-notes panel learned the hard way (section 5). A query that clears neither gets a
plain "no matches", and B2 shows you none of its nearest guesses. Not folded away behind a
"show anyway", not counted, just not offered. The nearest list is genuinely out of reach for
that one query. That is the accepted price of the answer being trustworthy: what B2 shows
you, it vouches for.

Worth knowing: the bar is measured per embedding model, so a model B2 has no measurement for
yet gets no verdict at all rather than a guessed one. Search behaves exactly as it always did
there, including in offline/dev mode. And `b2 search --json` still hands an agent every row
plus the verdict, even when the answer is "no matches": a program that is told the vault
vouches for nothing can be honest about the rows anyway, where a person handed ten results
cannot.

### Three smaller heuristics you may notice

- **Word endings are trimmed.** The keyword index reduces "running", "runs", and "ran" to a
  common root so they match each other. The term is **stemming** (B2 uses the Porter
  stemmer). The cost is occasional over-merging: *universe* and *university* collapse to the
  same root, which is why B2's test suite deliberately includes that trap.
- **Punctuation in your query is neutralized.** Search-syntax characters are stripped before
  your words reach the keyword index, so typing a question with apostrophes and quotes can't
  break it.
- **Only a working set is ranked.** Each system returns a bounded pool of candidate passages
  (for a normal ten-result search, 150 per system; the passage-level view keeps 60) rather
  than the entire vault. On a large vault this is what keeps search instant. It also means a
  note far outside both pools can't be rescued by fusion.

## 4. Similarity: the two-stage engine

The related-notes panel answers a different question from search. Search asks "what matches
this query?" Similarity asks "what in my vault belongs next to this note, that I haven't
already connected?"

Running it on a note (the **anchor**) happens in two passes, for speed:

1. **Pass one: a fast shortlist.** Comparing every passage against every other passage would
   be slow on a big vault. So B2 first gives each note a single average position (the middle
   of all its passages) and uses that to shortlist a few hundred plausible notes. This pass
   is about recall: it is deliberately generous, and exists only to avoid scoring the whole
   vault.
2. **Pass two: the real comparison.** For each shortlisted note, B2 compares every passage of
   the anchor against every passage of the candidate and keeps the single best matching
   pair. That best pair is the note's score, and the passage that achieved it is shown to you
   on the card as the evidence for why this note appeared.

> The terms: the note's average position is its **centroid**. The best-pair comparison is
> **max-sim**, or best-passage scoring. Deep dive: [index-engine.md §4](index-engine.md).

**Why the second pass matters: the buried gem.** An average is a lie about any note that
covers more than one subject. Consider a weekly journal with seven unrelated sections, one of
them a genuinely excellent account of a lava field. Its average position sits in the middle
of nowhere, near nothing. But one passage of it is an outstanding match for your volcano
note. Judge the average and you lose the gem. B2 ranks and grades this note on its best
passage, not its average, so a strong section inside a messy note can still surface, with
that section quoted on the card. This is also why the same journal doesn't get wrongly
suggested for your unrelated notes: its average may drift near them, but no single passage of
it actually matches, so it's cut.

**Notes you've already linked are removed.** Discovery shows you what you *haven't*
connected. Anything one link away from the anchor is excluded, because you already know about
it. Notes two links away stay in, since a related-but-not-directly-linked note is exactly the
connection worth finding.

**B2 suggests; you decide.** Nothing in this panel changes your notes. A connection exists
only when you author it, with `b2 link` or by dragging a card into your note in the app.
There is no suggestion queue and no pending state: you are the precision gate.

## 5. The list is always served, and graded, not gated

"What in my vault belongs next to this note?" is a relative question, and the ranked list
answers it. B2 always shows you the nearest unlinked notes. The strength dots, not an empty
panel, carry the quality signal.

Why no statistical rule for staying quiet? Because any such rule has to read the anchor's own
candidates, and that reading is ambiguous in exactly the case that matters. "Stands out from
the background" quietly assumes most of your vault is unrelated to any given note. A vault
where everything shares one subject (a research vault, a project vault, a single-subject
zettelkasten) breaks that assumption by construction: the background is itself related,
nothing can stand out from it, and a gate reads the vault with the *most* to connect as
having nothing. An anchor-local statistic cannot tell "nothing is related" from "everything
is related", so B2 lets none claim to. (That is invariant D1; the measurements that settled
it are recorded in
[ADR-0014](../ADRs/0014-discovery-always-serves-the-ranked-prefix.md).)

So the rule is simple: the ranked list is served, and you are the judge. The panel is empty
only when there is genuinely nothing to compare (no unlinked note with stored vectors yet),
and it says exactly that, never "nothing relates". The per-candidate statistic survives as
the input to the strength dots: a within-list grading, so a research vault reads "here are
your nearest, all middling" instead of a dark panel.

Could a smarter "show nothing" rule ever ship? Only by earning it. Any candidate has to win a
measured bake-off: on the standard test corpus, on a deliberately single-domain one built for
exactly this failure, and on real vaults via a calibration command. "No rule at all" is
allowed to win it, and has, so far. What ships today is also what every comparable tool
ships: a ranked list, with the score as a signal rather than a verdict.

Search is the other side of that same coin, and it goes the other way. Nothing here
contradicts section 3's "no matches": *related to this note* is a comparison with no zero
point (everything in your vault is somewhat related to everything), while *relevant to this
query* has an honest zero, and B2 can measure it in two independent ways. Where a claim can
be checked, B2 makes it; where it can't, B2 hands you the ranking and gets out of the way.

## 6. Reading the strength dots

Each card in the app carries three dots. They are that candidate's z-score, banded. Not a raw
similarity, and not a percentage.

- **●●● strong match.** 2.52σ and above: in the top quartile of relationships a human has
  confirmed as real on the test corpus.
- **●●○ clear match.** 1.96σ and above: where the test corpus's confirmed relationships
  typically lead their lists.
- **●○○ near match.** Below 1.96σ: near in this list's own terms; worth a skeptical look.

Because the dots and the ordering of the list are read from the same number, they can never
disagree with each other. A card higher in the list never has fewer dots than one below it.
The dots grade; they never decide what appears. The list itself is always the ranked nearest.

The dots are relative to the note you're on. "●●●" means strong compared with *this note's
other candidates*, not "95% similar". The same pair of notes can legitimately show different
dots from each side, because each side has different competition. A known consequence: in a
tightly single-subject vault, where every candidate is close, the dots can read uniformly
modest. See the open questions below.

> The term: a **z-score** ("σ", sigma) says how far above the typical value something sits,
> measured in units of the normal spread. In B2 it grades the dots, within one note's list,
> and decides nothing about what appears.

## 7. Known limits and honest caveats

- **Open question: in a tightly single-subject vault, the strength dots can read uniformly
  modest.** The panel always serves the ranked list (section 5); what remains open is the
  grading. The dots compare each candidate with the note's other candidates, and in a vault
  where every candidate is close, that comparison compresses: genuinely strong relationships
  can all paint one or two dots. The list order is unaffected and correct; only the dots lose
  resolution. Measuring that compression on real vaults (`make calibrate` exists for exactly
  this), and deciding what the dots should reference instead, is the named next step.
- **A relation the model simply gets wrong can't be rescued by ranking.** If the model scores
  one unrelated pair unusually highly, that pair sits high in the list by definition. One
  such case is a known, tracked residue in B2's own test corpus: now a mis-ordering (the real
  relation appears, a few places lower than it should) rather than an absence.
- **Whether any "show nothing" rule ever ships in the related-notes panel is an open,
  evidence-gated question.** Every candidate must win the measured bake-off section 5
  describes, "no rule at all" is an admissible winner, and no candidate has yet beaten it.
  This is the related-notes panel only: search *does* answer "no matches", because a query
  has an honest zero where a note-to-note comparison does not.
- **Quality depends on your reindex being current.** Discovery reads stored vectors and never
  re-embeds, so notes edited since the last index are compared on their old text. The desktop
  app re-indexes automatically on changes; the CLI does it when you run `b2 reindex`.
- **An index built with the fake embedder is meaningless.** If `B2_EMBEDDER=fake` was set
  (offline/dev mode), vectors are content hashes, not semantics. B2 detects this and serves
  the list ungraded rather than pretending to measure strength over noise.
- **Only Markdown is read for meaning.** Other files in your vault are tracked as resources
  but have no passages or vectors yet, so they never appear as similar notes.

## 8. Every component and knob, in one table

What is actually in the pipeline, what each part decides, and the value it ships with.

| Component | What it decides | Ships as | Deeper dive |
|---|---|---|---|
| Chunker | Where a note is cut into passages; prefers headings, then paragraph breaks; never splits code or tables | ~450 tokens, 15% overlap | [index-engine.md §1](index-engine.md) |
| Embedding model | Turns a passage into its position on the meaning map | `bge-base-en-v1.5`, 768 dimensions, local | [architecture.md](architecture.md) |
| Keyword index | Which passages contain your words; rare words count for more | SQLite FTS5, BM25, Porter stemming | [index-engine.md §4](index-engine.md) |
| Vector scan | Which passages are nearest in meaning | Exact comparison, in-process, no extension | [index-engine.md §4](index-engine.md) |
| Fusion | How the two rankings become one | RRF with a damping constant of 60; ties break toward the meaning signal | [index-engine.md §4](index-engine.md) |
| Candidate pools | How deep each signal looks before fusing | Per signal, at a 10-result search: 150 passages (note view), 60 (passage view) | [index-engine.md §4](index-engine.md) |
| Shortlist (pass 1) | Which notes are worth comparing properly | 20× the requested count, at least 200 notes | [index-engine.md §4](index-engine.md) |
| Best-passage score (pass 2) | How related two notes actually are, and which passage proves it | Best pair across all passages of both | [index-engine.md §4](index-engine.md) |
| Link exclusion | Which candidates are hidden as already known | Anything 1 link away from the anchor | [index-engine.md §4](index-engine.md) |
| Surfacing rule | What the related-notes panel shows | The ranked list, always; no statistical gate (D1, ADR-0014) | Section 5 |
| Strength bands | How many dots a card gets (grading only, never what appears) | ●●● ≥ 2.52σ, ●●○ ≥ 1.96σ, ●○○ below; ungraded under 12 candidates | Section 6 |
| Grounded chat | Which passages an answer may cite | Top 10 passages from the same hybrid search | [index-engine.md §6](index-engine.md) |

None of these are guesses. Every value above was chosen by measuring it against a
hand-labelled corpus of notes and expected answers, and several were chosen by rejecting a
change that looked better on paper. That test harness, its rules, and the record of every
verdict live in [evals.md](evals.md).

## 9. Glossary

- **Anchor.** The note you're currently looking at: the one whose related notes are being
  found.
- **BM25.** The standard formula for keyword ranking. Scores a passage on how many of your
  search words it contains, weighting rare words far more heavily than common ones. Knows
  nothing about meaning.
- **Centroid** ("the note's average position"). One position representing a whole note, made
  by averaging all its passages. Fast to compare, but misleading for a note covering several
  subjects, which is why B2 uses it only to build a shortlist, never to grade a result.
- **Chunk** ("passage"). A slice of a note, roughly 450 tokens, cut at a heading or paragraph
  break. Everything B2 compares is a chunk, not a whole note.
- **Cosine similarity.** The closeness of two positions on the meaning map, from -1 to 1. 1
  means "identical direction". B2 computes it but never shows it to you raw, because its
  meaning depends entirely on the vault around it.
- **Embedding** ("vector"). The list of 768 numbers a model produces for a passage: its
  position on the map of meaning. Passages about similar things get nearby positions.
- **Embedding model.** The local AI model that produces embeddings. B2 ships with
  `bge-base-en-v1.5`, downloaded once by `b2 init` and run on your own machine.
- **Hybrid search.** Running keyword and meaning search together and merging the results:
  B2's default, because each covers the other's blind spot.
- **KNN** ("k nearest neighbors"). Finding the k closest positions to a given one. What
  "search by meaning" and "related notes" both do underneath.
- **Max-sim** ("best-passage scoring"). Scoring two notes by their single best-matching pair
  of passages, rather than by their averages. Lets one strong section inside a messy note
  count for something.
- **Reciprocal Rank Fusion (RRF).** The method for merging two ranked lists using each item's
  position rather than its score. Necessary because keyword scores and distance scores aren't
  on comparable scales.
- **Stemming.** Trimming words to a common root in the keyword index so "running" matches
  "run". Improves recall; occasionally over-merges (*universe* / *university*).
- **z-score** ("σ", "sigma"). How far above the typical value something sits, measured in
  units of the normal spread. "2.5σ" means "well above this note's ordinary candidates". In
  B2 it grades the strength dots, within one note's list, and decides nothing about what
  appears.

## 10. Deeper dives

- [index-engine.md](index-engine.md): the whole engine spec. Chunking and indexing (§1, §3),
  both retrieval flows stage by stage (§4), the seams (§6).
- [architecture.md](architecture.md): the whole system, the crates, and where each flow
  lives.
- [evals.md](evals.md): how every number on this page was measured, and the verdicts.
- [invariants.md](invariants.md), [data-model.md](data-model.md): the spec the code is a
  projection of.
- [quickstart.md](quickstart.md): install, index a vault, and run your first search.
