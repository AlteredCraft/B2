//! Real-vault discovery calibration — the instrument ADR-0014's Phase 0a promoted out of
//! GH #196's hand arithmetic. It runs against **any built vault**, needs no labels, and
//! prints the numbers every discovery-surfacing ruling has turned on: per-anchor pool
//! distributions, each anchor's leader cosine and z, what a z gate would serve versus what
//! always-serve does, and the strength bands the desktop would paint. This is process rule
//! 5 made mechanical — **a constant derived from a corpus's score distribution is invalid
//! until transfer-checked on a real vault**.
//!
//! ```console
//! make calibrate VAULT=$HOME/notes                                        # per-anchor lines + the summary block
//! make calibrate VAULT=$HOME/notes ARGS=--json                            # the same reading as one JSON object
//! make calibrate VAULT=$HOME/notes ARGS="--search --json"                 # …with the search-side bench in it too
//! make calibrate VAULT=$HOME/notes ARGS="--limit 5"                       # simulate a 5-card pane
//! make calibrate VAULT=$HOME/notes ARGS="--leader-z 1.5 --member-z 1.0"   # replay a different gate
//! make calibrate VAULT=$HOME/notes ARGS="--mutual-k 5"                    # replay the fold at a different depth
//! ```
//!
//! Beside the retired z gate it replays the two **default-disclosure** candidates ADR-0014
//! admits a fold from. The **mutual-k reciprocity fold** (GH #200): B is reciprocal for A
//! iff A ranks within B's own top `mutual_k`, and the replayed default view is the ranked
//! list's longest reciprocal prefix — a cut in the served order, never a filter that skips
//! rows. That bake-off has **ruled, and no fold ships**: the window is empty on both corpora,
//! and this instrument supplied the reading that generalized it — the same `k` is a different
//! rule on every vault (10 discloses 36% of cards on the orthogonal corpus, 91% on the dense
//! fixture, 98% on `fixtures/test-vault`). The **authored-edge reference bar** is replayed
//! beside it, and this is the only instrument that *can*: the rule calibrates from the score
//! distribution of the human's own committed edges, and both eval corpora are link-free by
//! construction. Unlike reciprocity that is a distributional constant, so this instrument is
//! not an aside for it — it is the whole of its evidence.
//!
//! **It is a pure read** — stored vectors only, no model call — so it runs in seconds on any
//! personal-scale vault and never perturbs what it measures.
//!
//! The z is **recomputed harness-side from the served scores**, kept independent of the
//! engine's own statistic so the instrument can also *check* it: where the engine ships a z
//! beside a candidate, the two are diffed and a drift reports as a fault.
//!
//! The replayed gate defaults to the constants ADR-0014 retired, so the acceptance reading
//! reproduces GH #196's finding on its reporting vault: 16 of 17 anchors dark. It is a
//! **simulation** — the shipped surface gates nothing.

use b2_core::vault::{SimilarView, Vault};
use b2_embed::{EmbedConfig, LocalEmbedder};
use rusqlite::Connection;
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;

/// How deep each anchor's pool is read. A bar is judged against the population it
/// has to cut, so the read must cover the whole candidate set, not a pane's
/// prefix; anything a personal vault holds fits far under this.
const SCAN_LIMIT: usize = 100_000;
/// The replayed gate is inert under this population — `discover.rs`'s statistics
/// guard, restated here because the replay must match the rule it prices.
const MIN_POPULATION: usize = 12;
/// The strength-band landmarks the desktop paints (`ui/src/strength.ts`, GH #182):
/// `●●●` at or above the labelled-mate population's upper quartile, `●●○` at or
/// above the retired leader bar. Restated constants, not imports — the bands are
/// UI copy, and `make eval`'s calibration block is where their values are
/// re-measured.
const BAND_STRONG_Z: f64 = 2.52;
const BAND_CLEAR_Z: f64 = 1.96;

/// The z existence gate being replayed — GH #197 retired it from the engine; the
/// instrument keeps it (and any variant the flags name) priceable.
struct GateSim {
    leader_z: f64,
    member_z: f64,
}

/// One anchor's complete reading: its whole candidate pool in rank order, with
/// the harness-side z when the population carries a statistic.
struct AnchorReading {
    path: String,
    /// (candidate path, cosine, engine z if shipped) in served (nearest-first) order.
    pool: Vec<(String, f64, Option<f64>)>,
    /// Harness-recomputed z per pool entry — `None` when the population is under
    /// [`MIN_POPULATION`] or has zero variance (no statistic exists).
    z: Option<Vec<f64>>,
}

impl AnchorReading {
    fn from_candidates(path: &str, cands: &[SimilarView]) -> Self {
        let d2: Vec<f64> = cands.iter().map(|c| c.score * c.score).collect();
        let z = (d2.len() >= MIN_POPULATION)
            .then(|| {
                let n = d2.len() as f64;
                let mean = d2.iter().sum::<f64>() / n;
                let var = d2.iter().map(|d| (d - mean).powi(2)).sum::<f64>() / (n - 1.0);
                let sd = var.sqrt();
                (sd > 0.0).then(|| d2.iter().map(|d| (mean - d) / sd).collect::<Vec<f64>>())
            })
            .flatten();
        Self {
            path: path.to_string(),
            pool: cands
                .iter()
                .map(|c| (c.path.clone(), cosine_of(c.score), c.z))
                .collect(),
            z,
        }
    }

    fn cosines(&self) -> Vec<f64> {
        self.pool.iter().map(|(_, cos, _)| *cos).collect()
    }

    /// How many candidates the replayed z gate would serve, capped at `limit` —
    /// 0 with the leader gate fired ("dark"), everything (to the cap) where the
    /// pool is too small for a statistic (the rule's own inertness).
    fn gate_serves(&self, gate: &GateSim, limit: usize) -> usize {
        match &self.z {
            None => self.pool.len().min(limit),
            Some(z) => {
                if z.first().copied().unwrap_or(f64::NEG_INFINITY) < gate.leader_z {
                    0
                } else {
                    z.iter()
                        .take_while(|&&v| v >= gate.member_z)
                        .count()
                        .min(limit)
                }
            }
        }
    }

    /// Band histogram over the top-`limit` cards always-serve shows:
    /// (strong ●●●, clear ●●○, near ●○○), or `None` for an ungraded pool —
    /// the A6 readout (do dense vaults compress every card into one band?).
    fn bands(&self, limit: usize) -> Option<(usize, usize, usize)> {
        let z = self.z.as_ref()?;
        let (mut strong, mut clear, mut near) = (0, 0, 0);
        for &v in z.iter().take(limit) {
            if v >= BAND_STRONG_Z {
                strong += 1;
            } else if v >= BAND_CLEAR_Z {
                clear += 1;
            } else {
                near += 1;
            }
        }
        Some((strong, clear, near))
    }

    /// Worst |engine z − recomputed z| across the pool — the drift check, biting
    /// only where the engine shipped a z at all.
    fn recheck_delta(&self) -> f64 {
        let Some(z) = &self.z else { return 0.0 };
        self.pool
            .iter()
            .zip(z)
            .filter_map(|((_, _, engine), ours)| engine.map(|e| (e - ours).abs()))
            .fold(0.0, f64::max)
    }

    /// How many pool entries the drift check actually compared — both z's
    /// present. Zero pairs must print as "nothing to cross-check", never as a
    /// pass: a check that reports success without having run is the
    /// advisory-but-exit-0 hole this repo's gates exist to close.
    fn recheck_pairs(&self) -> usize {
        if self.z.is_none() {
            return 0;
        }
        self.pool
            .iter()
            .filter(|(_, _, engine)| engine.is_some())
            .count()
    }
}

/// `similar` scores are negated L2 over unit vectors: `cos = 1 − d²/2`.
fn cosine_of(score: f64) -> f64 {
    1.0 - (score * score) / 2.0
}

fn pile_stats(pile: &[f64]) -> Option<(f64, f64, f64)> {
    if pile.is_empty() {
        return None;
    }
    let mut sorted = pile.to_vec();
    sorted.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let mid = sorted.len() / 2;
    let median = if sorted.len().is_multiple_of(2) {
        (sorted[mid - 1] + sorted[mid]) / 2.0
    } else {
        sorted[mid]
    };
    Some((sorted[0], median, sorted[sorted.len() - 1]))
}

/// **Candidate 2** of GH #200's bake-off, replayed: an *authored-edge reference bar*. It
/// calibrates "what related looks like **in this vault**" from the one labelled population
/// every real vault carries — the score distribution of the human's committed edges — and
/// folds the default view at the longest prefix scoring at or above it.
///
/// Priceable only where that population exists, which is why it lives here rather than in
/// `make eval`: **both eval corpora are link-free by construction**. Its pair score is the
/// same statistic discovery ranks on, computed over the *linked* pairs discovery never scores
/// (the 1-hop exclusion removes exactly them). The bar is the population's **lower quartile**
/// — a candidate at least as related as the weaker quarter of what this human already linked
/// — and the whole distribution prints beside it, because a quantile is a choice and a choice
/// printed as one number is an assumption.
struct EdgeBar {
    /// Authored edges whose pair could be scored (both notes embedded).
    n: usize,
    min: f64,
    q1: f64,
    median: f64,
    max: f64,
}

impl EdgeBar {
    /// The bar itself — the lower quartile of the authored-edge cosines.
    fn bar(&self) -> f64 {
        self.q1
    }

    /// This vault's authored-edge pair cosines. Undirected and de-duplicated: an
    /// edge read from both endpoints is one relation, and counting it twice
    /// would weight the reciprocally-visible pairs double. `None` when the vault
    /// has no scorable authored edge — the rule has no population, which is a
    /// reading about the vault, not a failure of the instrument.
    fn read(vault: &Vault, conn: &Connection) -> Result<Option<Self>, Box<dyn std::error::Error>> {
        let mut seen: HashSet<(String, String)> = HashSet::new();
        let mut cos: Vec<f64> = Vec::new();
        for note in vault.list_notes()? {
            let neighbors = vault.neighbors(&note.path)?;
            if neighbors.is_empty() {
                continue;
            }
            // Loaded once per source note and dropped with it: the population is
            // read edge by edge rather than by caching the whole vault's
            // vectors, so a large vault costs time here, never memory.
            let src: Vec<Vec<f32>> = b2_core::db::note_chunk_vectors(conn, &note.path)?
                .into_iter()
                .map(|(_, v)| v)
                .collect();
            for n in neighbors {
                let pair = if note.path <= n.path {
                    (note.path.clone(), n.path.clone())
                } else {
                    (n.path.clone(), note.path.clone())
                };
                if !seen.insert(pair) {
                    continue;
                }
                let dst: Vec<Vec<f32>> = b2_core::db::note_chunk_vectors(conn, &n.path)?
                    .into_iter()
                    .map(|(_, v)| v)
                    .collect();
                // Best-passage, the same statistic `similar` ranks on: the
                // nearest chunk pair across the two notes. A pair with an
                // unembedded side (or a dangling/resource target) scores nothing
                // and drops out rather than entering the population as a zero.
                let best = src
                    .iter()
                    .flat_map(|x| dst.iter().map(move |y| b2_core::embed::l2_sq(x, y)))
                    .fold(f32::INFINITY, f32::min);
                if best.is_finite() {
                    cos.push(1.0 - (best as f64) / 2.0);
                }
            }
        }
        if cos.is_empty() {
            return Ok(None);
        }
        cos.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
        let at = |q: f64| cos[(((cos.len() - 1) as f64) * q).round() as usize];
        Ok(Some(Self {
            n: cos.len(),
            min: cos[0],
            q1: at(0.25),
            median: at(0.5),
            max: cos[cos.len() - 1],
        }))
    }

    /// How many of `pool`'s leading candidates clear the bar — candidate 2's
    /// fold, in the same prefix form candidate 1 takes.
    fn fold(&self, pool: &[(String, f64, Option<f64>)], limit: usize) -> usize {
        pool.iter()
            .take(limit)
            .take_while(|(_, cos, _)| *cos >= self.bar())
            .count()
    }
}

/// English function words, probed for **where they weigh** against a query's
/// content (GH #201). The lexical-anchor rule weighs each term by its IDF, so
/// its whole premise is that a vault's function words weigh near nothing beside
/// its subject words. That is a claim about a vault, not about English, and this
/// is where it is checked.
const FUNCTION_WORDS: [&str; 12] = [
    "the", "and", "of", "to", "a", "in", "is", "it", "that", "for", "with", "on",
];

/// Built-in nonsense queries — the **strict** half of D2's negative pile, and
/// the only half that can be built in: an off-topic *phrase* is off-topic
/// relative to a particular vault ("choreographing a ballroom waltz" is a
/// labelled negative on the eval corpus and would be a positive in a dancer's
/// vault), so the eval corpus is where those live. These are strings no vault
/// holds, which makes them vault-independent.
const NONSENSE: [&str; 4] = [
    "shjfasd",
    "vrelqip zonktar wembleforth",
    "qwolbex frunstip",
    "zzzyqx",
];

/// How many notes contribute a title-as-query positive. A cap rather than a
/// sample: the walk takes them in `list_notes` order, which is deterministic, so
/// a re-run on an unchanged vault reads the same queries.
const MAX_TITLE_QUERIES: usize = 400;

/// One query's transfer reading — the same two absolute signals `make eval`'s
/// bake-off judges, read on a real vault instead of a labelled corpus.
struct SearchProbe {
    query: String,
    /// Share of the query's term IDF this vault carries. `None` when nothing in
    /// the query carries weight here.
    coverage: Option<f64>,
    /// Dense top-1 cosine; `None` on a vault with no embedding space.
    best_cos: Option<f64>,
    /// What the shipped bar would say — `true` = the default view vouches.
    vouched: bool,
    /// For a title-as-query probe, the note the title came from — the one row
    /// this bench can certify as relevant with no label (GH #206). `None` for
    /// nonsense.
    own_path: Option<String>,
    /// The served list's per-hit provenance, in fused order — what the tail
    /// families' constraints are read from.
    rows: Vec<TailRow>,
}

impl SearchProbe {
    /// Where this probe's own note landed in the served list, 0-based — `None`
    /// when retrieval never served it within the limit (or the probe has no own
    /// note at all, which is every nonsense negative).
    fn own_rank(&self) -> Option<usize> {
        let own = self.own_path.as_ref()?;
        self.rows.iter().position(|r| &r.path == own)
    }
}

/// One served row's per-hit provenance (GH #206) — `EvidencedResult`, shorn of
/// the display fields this instrument never prints.
struct TailRow {
    path: String,
    bm25_rank: Option<usize>,
    cos: Option<f64>,
}

/// One tail family's edge: the tightest constant that still hides no own note,
/// and the served row that pins it there.
///
/// Owned rather than borrowed out of the probe pile: the edge outlives the fold
/// that finds it — it is carried to both renderings — and a lifetime here would
/// buy nothing but the borrow.
struct TailEdge {
    value: f64,
    query: String,
    path: String,
}

/// A row served above its own note carrying **no finite cosine** — the reading
/// that kills a cosine family outright rather than merely constraining it.
struct DeadRow {
    query: String,
    path: String,
    /// Dense-only besides, which kills the two-signal family too: no lexical
    /// rank and no finite cosine passes at any bar.
    dense_only: bool,
}

/// An own note the dense-only fold would hide: a row the lexical half never
/// ranked, served *above* the one row this bench certifies.
struct LexHidden {
    query: String,
    /// 0-based, like every other rank this instrument reports (the text block
    /// prints them +1).
    own_rank: usize,
    dense_only_rank: usize,
}

/// The per-hit tail families (GH #206) priced on this vault: how tight each
/// family's constant could be drawn before it hides a row the bench certifies.
struct TailReading {
    own_served: usize,
    own_missed: usize,
    dense_only_rows: usize,
    total_rows: usize,
    lex_hidden: Vec<LexHidden>,
    cos_edge: Option<TailEdge>,
    lexcos_edge: Option<TailEdge>,
    drop_edge: Option<TailEdge>,
    cos_dead: Option<DeadRow>,
}

impl TailReading {
    /// `make eval`'s tail bake-off derives each family's admissible window from
    /// the labelled corpora; this is the reading that says whether such a window
    /// survives a real vault (process rule 5 — owed even by the parameterless
    /// family, whose "lexical half never ranked it" signal is partly a fact
    /// about pool depth against vault size). No labels here: the one served row
    /// a title query certifies is its **own note**, so each family is priced on
    /// what its constant would have to be to hide none of them — the tripwire
    /// direction. The fold is a prefix cut (D1), so every row served above an
    /// own note must pass the family's test too.
    fn read(positives: &[SearchProbe]) -> Self {
        let mut r = Self {
            own_served: 0,
            own_missed: 0,
            dense_only_rows: 0,
            total_rows: 0,
            lex_hidden: Vec::new(),
            cos_edge: None,
            lexcos_edge: None,
            drop_edge: None,
            cos_dead: None,
        };
        for p in positives {
            r.total_rows += p.rows.len();
            r.dense_only_rows += p.rows.iter().filter(|row| row.bm25_rank.is_none()).count();
            let Some(own) = p.own_rank() else {
                r.own_missed += 1;
                continue;
            };
            r.own_served += 1;
            // The drop family's reference is the list's own best served cosine —
            // the whole list's, not the prefix's, matching how the fold would read.
            let best = p
                .rows
                .iter()
                .filter_map(|row| row.cos)
                .fold(f64::NEG_INFINITY, f64::max);
            let prefix = &p.rows[..=own];
            if let Some(first) = prefix.iter().position(|row| row.bm25_rank.is_none()) {
                r.lex_hidden.push(LexHidden {
                    query: p.query.clone(),
                    own_rank: own,
                    dense_only_rank: first,
                });
            }
            for row in prefix {
                // Finiteness-filtered (PR #212 review): a NaN cosine is *no
                // reading*, and `is_none_or` would otherwise let a NaN first row
                // seed an edge. It lands in the dead arm, named, never skipped.
                match row.cos.filter(|c| c.is_finite()) {
                    None => {
                        if r.cos_dead.is_none() {
                            r.cos_dead = Some(DeadRow {
                                query: p.query.clone(),
                                path: row.path.clone(),
                                dense_only: row.bm25_rank.is_none(),
                            });
                        }
                    }
                    Some(c) => {
                        let edge = |value: f64| TailEdge {
                            value,
                            query: p.query.clone(),
                            path: row.path.clone(),
                        };
                        if r.cos_edge.as_ref().is_none_or(|e| c < e.value) {
                            r.cos_edge = Some(edge(c));
                        }
                        let drop = best - c;
                        if r.drop_edge.as_ref().is_none_or(|e| drop > e.value) {
                            r.drop_edge = Some(edge(drop));
                        }
                        if row.bm25_rank.is_none()
                            && r.lexcos_edge.as_ref().is_none_or(|e| c < e.value)
                        {
                            r.lexcos_edge = Some(edge(c));
                        }
                    }
                }
            }
        }
        r
    }
}

/// The half of the search bench that needs a calibrated bar: everything below
/// the model-free function-word reading, which is judged against one.
struct JudgedSearch {
    bar: b2_core::search::EvidenceBar,
    positives: Vec<SearchProbe>,
    negatives: Vec<SearchProbe>,
    tail: TailReading,
}

/// The whole search-side reading, **computed once**. The text block and the
/// `--json` object are two renderings of this one value rather than two
/// computations of it, so a sweep that scripts the JSON and a human reading the
/// table cannot be looking at different numbers.
struct SearchReading {
    /// The embedding space the reading was taken in — carried so the renderer
    /// names the same model the bar was looked up for, never one passed beside it.
    model_id: String,
    /// Chunks in the index — the scale every weight below is read against.
    chunk_total: usize,
    /// The weight a word the vault has never seen would carry: the scale the
    /// function words are read against, since that is what an absent *content*
    /// word contributes to the same sum.
    absent_idf: f64,
    /// Each [`FUNCTION_WORDS`] entry with the weight it carries here.
    function_words: Vec<(String, f64)>,
    /// `None` when the active model has no calibrated bar — the piles need one
    /// to be judged, so none of them was read.
    judged: Option<JudgedSearch>,
}

impl SearchReading {
    /// The heaviest function word's weight — the anchor test's premise, which
    /// holds only where it is small beside [`Self::absent_idf`].
    fn heaviest(&self) -> f64 {
        self.function_words
            .iter()
            .map(|(_, idf)| *idf)
            .fold(0.0_f64, f64::max)
    }

    /// That weight as a share of an absent word's, in percent — the printed form.
    fn heaviest_share(&self) -> f64 {
        100.0 * self.heaviest() / self.absent_idf.max(f64::EPSILON)
    }
}

/// The **search evidence transfer check** (ADR-0015) — process rule 5's bench for the
/// query-level bar, which is a distributional constant and therefore invalid until a real
/// vault has answered for it.
///
/// It needs no labels, and that is the point: the positives are each note's own **title**, a
/// query the vault demonstrably holds material for by construction, and the negatives are
/// [`NONSENSE`]. Neither is a hand-label, so running this on someone's notes costs them
/// nothing and the reading cannot be tuned by relabelling.
///
/// It **can** see the tripwire direction — a bar that cuts queries a real vault holds
/// material for is the failure ADR-0014 punished. It **cannot** see the paraphrase case,
/// which needs judgement and is what the labelled corpus is for. Read the two together.
fn read_search_transfer(
    vault: &Vault,
    conn: &Connection,
    model_id: &str,
    limit: usize,
) -> Result<SearchReading, Box<dyn std::error::Error>> {
    // The function-word reading is **model-free**: it is a fact about this
    // vault's vocabulary, and it is the lexical anchor's whole premise. It is
    // read before the bar lookup, and printed above it, because gating it behind
    // a calibrated bar (as this did until the PR #205 review) left a
    // fake-embedded vault printing nothing at all, which the recipe's own help
    // text promised it would not.
    let probe = b2_core::search::lexical_evidence(conn, &FUNCTION_WORDS.join(" "))?;
    let reading = SearchReading {
        model_id: model_id.to_string(),
        chunk_total: probe.chunk_total,
        absent_idf: probe.idf(0),
        function_words: probe
            .terms
            .iter()
            .map(|t| (t.term.clone(), probe.idf(t.df)))
            .collect(),
        judged: None,
    };

    // Only the *verdicts* need a bar, so only they stop here.
    let Some(bar) = b2_core::search::EvidenceBar::for_model(model_id) else {
        return Ok(reading);
    };

    let read = |query: &str,
                own_path: Option<String>|
     -> Result<SearchProbe, Box<dyn std::error::Error>> {
        let view = vault.search_evidence(query, limit)?;
        let idf = |df: usize| ((view.chunk_total as f64 + 1.0) / (df as f64 + 1.0)).ln();
        let total: f64 = view.terms.iter().map(|t| idf(t.df)).sum();
        let coverage = (total > f64::EPSILON).then(|| {
            view.terms
                .iter()
                .filter(|t| t.df >= 1)
                .map(|t| idf(t.df))
                .fold(0.0, |a, b| a + b)
                / total
        });
        Ok(SearchProbe {
            query: query.to_string(),
            coverage,
            best_cos: view.best_cos,
            // The engine's own verdict, not a restatement of it: this bench
            // prices what would actually ship.
            vouched: view.vouched.unwrap_or(true),
            own_path,
            rows: view
                .results
                .iter()
                .map(|r| TailRow {
                    path: r.result.path.clone(),
                    bm25_rank: r.bm25_rank,
                    cos: r.cos,
                })
                .collect(),
        })
    };

    let mut titles: Vec<(String, String)> = Vec::new();
    for note in vault.list_notes()? {
        if titles.len() == MAX_TITLE_QUERIES {
            break;
        }
        let title = note.title.clone().unwrap_or_else(|| {
            std::path::Path::new(&note.path)
                .file_stem()
                .map(|s| s.to_string_lossy().replace(['-', '_'], " "))
                .unwrap_or_default()
        });
        if !title.trim().is_empty() {
            titles.push((title, note.path));
        }
    }
    let positives: Vec<SearchProbe> = titles
        .iter()
        .map(|(t, path)| read(t, Some(path.clone())))
        .collect::<Result<_, _>>()?;
    let negatives: Vec<SearchProbe> = NONSENSE
        .iter()
        .map(|q| read(q, None))
        .collect::<Result<_, _>>()?;
    let tail = TailReading::read(&positives);
    Ok(SearchReading {
        judged: Some(JudgedSearch {
            bar,
            positives,
            negatives,
            tail,
        }),
        ..reading
    })
}

/// The search reading as the text block — the human half of what
/// [`read_search_transfer`] measured.
fn print_search_transfer(reading: &SearchReading) {
    println!();
    println!("search evidence transfer check (D2 — process rule 5's bench for GH #201's bar)");

    println!("  chunks {}", reading.chunk_total);
    let absent = reading.absent_idf;
    print!("  function-word weight (vs {absent:.2} for a word the vault lacks) ");
    for (term, idf) in &reading.function_words {
        print!("{term} {idf:.2}  ");
    }
    println!();
    let share = reading.heaviest_share();
    if absent > 0.0 && reading.heaviest() / absent <= 0.25 {
        println!("    → heaviest function word carries {share:.0}% of an absent word's weight —");
        println!("      the anchor test is reading subject words here, which is what it is for");
    } else {
        println!("    → [warn] heaviest function word carries {share:.0}% of an absent word's");
        println!("      weight — on this vault function words are not cheap, so every coverage");
        println!("      reading below is diluted by them");
    }

    let Some(judged) = &reading.judged else {
        let model_id = &reading.model_id;
        println!("  no calibrated bar for '{model_id}' — the piles below need one to be judged");
        println!("  (M2: a bar read off one model's distances says nothing about another's)");
        return;
    };
    println!(
        "  bar under test: coverage ≥ {:.2}, cos ≥ {:.3}",
        judged.bar.min_term_coverage, judged.bar.min_cos
    );

    let positives = &judged.positives;
    let cut: Vec<&SearchProbe> = positives.iter().filter(|p| !p.vouched).collect();
    let covs: Vec<f64> = positives.iter().filter_map(|p| p.coverage).collect();
    let coss: Vec<f64> = positives.iter().filter_map(|p| p.best_cos).collect();
    println!(
        "  title-as-query positives (n={}, each note's own title)",
        positives.len()
    );
    match pile_stats(&covs) {
        Some((min, med, max)) => {
            println!("    coverage  min/med/max {min:.2}/{med:.2}/{max:.2}")
        }
        None => println!("    coverage  no reading (no note contributed a query)"),
    }
    match pile_stats(&coss) {
        Some((min, med, max)) => {
            println!("    best-cos  min/med/max {min:.3}/{med:.3}/{max:.3}")
        }
        None => println!(
            "    best-cos  no reading — no embedding space here, so only the bar's \
             model-free lexical half is being checked"
        ),
    }
    println!(
        "    the bar would cut {}/{}   ← the search-side TRIPWIRE direction (D2: zero, no headroom)",
        cut.len(),
        positives.len()
    );
    for p in cut.iter().take(10) {
        println!(
            "      cut: {:<44} cov {:>5}  cos {:>6}",
            truncate(&p.query, 44),
            fmt_opt(p.coverage, 2),
            fmt_opt(p.best_cos, 3),
        );
    }
    if cut.len() > 10 {
        println!("      … and {} more", cut.len() - 10);
    }
    // The closest calls, not a sample: a bar is placed against the queries that
    // nearly miss, and on a vault of any size those are the ten weakest — the
    // same reason the eval prints piles rather than means.
    let mut weakest: Vec<&SearchProbe> = positives.iter().collect();
    weakest.sort_by(|a, b| {
        a.best_cos
            .unwrap_or(f64::INFINITY)
            .partial_cmp(&b.best_cos.unwrap_or(f64::INFINITY))
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    println!("    closest calls (lowest-cosine positives — where a bar placement bites first):");
    for p in weakest.iter().take(10) {
        println!(
            "      {:<44} cov {:>5}  cos {:>6}  → {}",
            truncate(&p.query, 44),
            fmt_opt(p.coverage, 2),
            fmt_opt(p.best_cos, 3),
            if p.vouched { "served" } else { "CUT" },
        );
    }

    let negatives = &judged.negatives;
    let served = negatives.iter().filter(|p| p.vouched).count();
    println!(
        "  nonsense negatives (n={}, built-in — the vault-independent half of the pile)",
        negatives.len()
    );
    for p in negatives {
        println!(
            "      {:<44} cov {:>5}  cos {:>6}  → {}",
            truncate(&p.query, 44),
            fmt_opt(p.coverage, 2),
            fmt_opt(p.best_cos, 3),
            if p.vouched { "SERVED" } else { "no matches" },
        );
    }
    println!("    the bar serves {served}/{}", negatives.len());

    // ---- The per-hit tail families (GH #206), priced on this vault. ----------
    let tail = &judged.tail;
    let own_served = tail.own_served;
    println!(
        "  tail transfer (GH #206 — the per-hit tail families priced on this vault; judged on \
         the one"
    );
    println!("    row a title query certifies with no label: its own note)");
    println!(
        "    own note served within limit for {own_served}/{} titles ({} missed — \
         retrieval's, not a fold's)",
        positives.len(),
        tail.own_missed
    );
    println!(
        "    dense-only rows among all served rows: {}/{} — how often \
         the lexical signal fires at this scale",
        tail.dense_only_rows, tail.total_rows
    );
    match tail.lex_hidden.first() {
        None => println!(
            "    lexical (dense-only fold)   hides 0/{own_served} own notes — no dense-only row \
             above any of them"
        ),
        Some(h) => println!(
            "    lexical (dense-only fold)   ✗ hides {}/{own_served} own notes — first: {} (own \
             note rank {}, dense-only row at rank {})",
            tail.lex_hidden.len(),
            truncate(&h.query, 32),
            h.own_rank + 1,
            h.dense_only_rank + 1
        ),
    }
    let bar_line = |label: &str, edge: &Option<TailEdge>, floor: bool| match edge {
        None => {
            println!("    {label:<27} unconstrained (no row above an own note engages the test)")
        }
        Some(e) => println!(
            "    {label:<27} needs {} {:.3} to hide none (set by {} → {})",
            if floor { "δ ≥" } else { "c ≤" },
            e.value,
            truncate(&e.query, 32),
            e.path
        ),
    };
    match &tail.cos_dead {
        Some(d) => println!(
            "    cos ≥ c                     DEAD — {} → {} is served above its own note with no \
             cosine",
            truncate(&d.query, 32),
            d.path
        ),
        None => bar_line("cos ≥ c", &tail.cos_edge, false),
    }
    match &tail.cos_dead {
        // A dead row that is also dense-only kills the two-signal family too:
        // no lexical rank and no finite cosine passes at no bar.
        Some(d) if d.dense_only => println!(
            "    lex-or-cos ≥ c              DEAD — {} → {} is dense-only with no finite cosine",
            truncate(&d.query, 32),
            d.path
        ),
        _ => bar_line("lex-or-cos ≥ c", &tail.lexcos_edge, false),
    }
    match &tail.cos_dead {
        Some(_) => println!("    cos ≥ best − δ              DEAD — same row as the cos family"),
        None => bar_line("cos ≥ best − δ", &tail.drop_edge, true),
    }
    println!(
        "    → read these against the corpus windows in `make eval`'s tail bake-off: a family \
         ships only"
    );
    println!("      where the joint corpus edge also hides nothing here (process rule 5)");
}

/// A fixed-precision optional reading, or an em dash where there is none — the
/// one spelling of "no reading" the table uses.
fn fmt_opt(v: Option<f64>, places: usize) -> String {
    v.map(|c| format!("{c:.places$}"))
        .unwrap_or_else(|| "—".to_string())
}

fn main() {
    if let Err(e) = run() {
        eprintln!("calibrate failed: {e}");
        std::process::exit(1);
    }
}

fn run() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let mut vault_path: Option<PathBuf> = None;
    let mut limit = 10usize;
    let mut gate = GateSim {
        leader_z: 1.96,
        member_z: 1.49,
    };
    let mut mutual_k_flag: Option<usize> = None;
    let mut json = false;
    let mut search = false;
    let mut it = args.iter();
    while let Some(a) = it.next() {
        match a.as_str() {
            "--limit" => limit = it.next().ok_or("--limit needs a value")?.parse()?,
            "--leader-z" => gate.leader_z = it.next().ok_or("--leader-z needs a value")?.parse()?,
            "--member-z" => gate.member_z = it.next().ok_or("--member-z needs a value")?.parse()?,
            "--mutual-k" => {
                mutual_k_flag = Some(it.next().ok_or("--mutual-k needs a value")?.parse()?)
            }
            "--json" => json = true,
            "--search" => search = true,
            other if vault_path.is_none() && !other.starts_with('-') => {
                vault_path = Some(PathBuf::from(other));
            }
            other => return Err(format!("unrecognized argument: {other}").into()),
        }
    }
    let vault_root = vault_path.ok_or(
        "usage: calibrate <vault> [--limit N] [--leader-z Z] [--member-z Z] [--mutual-k N] \
         [--search] [--json]",
    )?;
    // The reciprocity depth defaults to the simulated pane size, resolved after
    // parsing so flag order can't matter.
    let mutual_k = mutual_k_flag.unwrap_or(limit);

    // A pure read over stored vectors, so the fake-embedder open is correct — the
    // same posture as `b2 similar`. The recorded identity is read straight from
    // the index, because it is what the vectors actually are, not what an
    // injected embedder would be.
    let vault = Vault::open(&vault_root)?;
    let conn = b2_core::open(&vault_root.join(".b2").join("b2.sqlite"))?;
    let Some((model, dim)) = b2_core::db::recorded_embedder(&conn)? else {
        return Err(
            "this vault has no embedding space — run `b2 init` then `b2 reindex` first".into(),
        );
    };
    if model == b2_core::embed::FAKE_MODEL_ID {
        eprintln!(
            "[warn] this vault is fake-embedded — hash vectors have no semantic geometry, so every \
             distribution below is noise. Reindex with the real model before reading anything off this."
        );
    }

    let notes = vault.list_notes()?;
    let mut readings: Vec<AnchorReading> = Vec::new();
    let mut poolless: Vec<String> = Vec::new();
    for note in &notes {
        let cands = vault.similar(&note.path, SCAN_LIMIT)?;
        if cands.is_empty() {
            // No stored vectors on the anchor, or nothing unlinked with vectors
            // to compare against — a genuinely empty candidate set, named rather
            // than averaged in as a zero.
            poolless.push(note.path.clone());
        } else {
            readings.push(AnchorReading::from_candidates(&note.path, &cands));
        }
    }

    // The mutual-k reciprocity fold, replayed (GH #200). Every note's own top `mutual_k`
    // candidate paths come from the same full-depth pools just read, so reciprocity costs no
    // extra discovery pass. A candidate with no pool of its own cannot reciprocate. The fold
    // is the ranked list's longest reciprocal prefix, capped at `limit` — prefix form is
    // ADR-0014's admissibility requirement, since a fold that skipped rank 2 to admit rank 5
    // would visibly disagree with the row order.
    let top_of: HashMap<&str, HashSet<&str>> = readings
        .iter()
        .map(|r| {
            (
                r.path.as_str(),
                r.pool
                    .iter()
                    .take(mutual_k)
                    .map(|(p, _, _)| p.as_str())
                    .collect::<HashSet<&str>>(),
            )
        })
        .collect();
    let folds: Vec<(usize, Vec<bool>)> = readings
        .iter()
        .map(|r| {
            let recip: Vec<bool> = r
                .pool
                .iter()
                .map(|(p, _, _)| {
                    top_of
                        .get(p.as_str())
                        .is_some_and(|s| s.contains(r.path.as_str()))
                })
                .collect();
            let fold = recip.iter().take(limit).take_while(|&&b| b).count();
            (fold, recip)
        })
        .collect();

    // Candidate 2 of the same bake-off (GH #200): the authored-edge reference
    // bar, priceable only where the human has committed edges — which is why it
    // is measured here and not in `make eval` (both eval corpora are link-free
    // by construction, so the rule has no population there).
    let edge_bar = EdgeBar::read(&vault, &conn)?;

    // The search-side bench (GH #201), opt-in because it is the one part of this
    // instrument that is **not** a pure read: judging the cosine half means
    // embedding a query, which means loading the real model. The lexical half
    // needs no model at all, so a fake-embedded vault still gets that much.
    let read_search = || -> Result<SearchReading, Box<dyn std::error::Error>> {
        if model == b2_core::embed::FAKE_MODEL_ID {
            read_search_transfer(&vault, &conn, &model, limit)
        } else {
            let config = EmbedConfig::load()?;
            let embedder = LocalEmbedder::load(&config)?;
            let real = Vault::open_with_embedder(&vault_root, Box::new(embedder))?;
            read_search_transfer(&real, &conn, &model, limit)
        }
    };

    if json {
        // Read before anything is printed, so a failure to load the model is an
        // error rather than a half-written object on stdout.
        let search = search.then(read_search).transpose()?;
        print_json(
            &vault_root,
            &model,
            dim,
            limit,
            &gate,
            mutual_k,
            &readings,
            &folds,
            &edge_bar,
            &poolless,
            &search,
        );
        return Ok(());
    }

    println!(
        "[calibrate] {}  model {model} (dim {dim})  {} notes, {} anchors with candidates, {} without",
        vault_root.display(),
        notes.len(),
        readings.len(),
        poolless.len()
    );
    println!(
        "[calibrate] replayed gate: leader_z {:.2} / member_z {:.2}, inert under {MIN_POPULATION} \
         candidates; replayed fold: mutual-k {mutual_k}, longest reciprocal prefix (simulations — \
         the shipped surface serves the ranked list, GH #197/#200)\n",
        gate.leader_z, gate.member_z
    );

    println!(
        "{:<44} {:>4}  {:^23}  {:^17}  {:>11}  {:>5}  {:>5}  {:>9}",
        "anchor", "n", "cos min/med/max", "leader cos / z", "gate serves", "fold", "e-bar", "bands"
    );
    println!("{}", "-".repeat(120));
    let mut dark = 0usize;
    let mut gate_served_total = 0usize;
    let mut fold_served_total = 0usize;
    let mut fold_empty = 0usize;
    let mut always_served_total = 0usize;
    let (mut strong, mut clear, mut near, mut ungraded) = (0usize, 0usize, 0usize, 0usize);
    let mut all_cos: Vec<f64> = Vec::new();
    let mut leader_zs: Vec<f64> = Vec::new();
    let mut worst_drift = 0.0f64;
    for (r, (fold, _)) in readings.iter().zip(&folds) {
        let cos = r.cosines();
        let (min, med, max) = pile_stats(&cos).unwrap_or((f64::NAN, f64::NAN, f64::NAN));
        all_cos.extend_from_slice(&cos);
        let leader_cos = cos.first().copied().unwrap_or(f64::NAN);
        let leader_z = r.z.as_ref().and_then(|z| z.first().copied());
        if let Some(z) = leader_z {
            leader_zs.push(z);
        }
        let served = r.gate_serves(&gate, limit);
        let always = r.pool.len().min(limit);
        gate_served_total += served;
        fold_served_total += fold;
        if *fold == 0 {
            fold_empty += 1;
        }
        always_served_total += always;
        if served == 0 {
            dark += 1;
        }
        let bands = r.bands(limit);
        match bands {
            Some((s, c, n)) => {
                strong += s;
                clear += c;
                near += n;
            }
            None => ungraded += 1,
        }
        worst_drift = worst_drift.max(r.recheck_delta());
        println!(
            "{:<44} {:>4}  {:>6.3}/{:>6.3}/{:>6.3}   {:>6.3} / {:>6}  {:>7}/{:<3}  {:>4}  {:>5}  {}",
            truncate(&r.path, 44),
            r.pool.len(),
            min,
            med,
            max,
            leader_cos,
            leader_z
                .map(|z| format!("{z:+.3}"))
                .unwrap_or_else(|| "—".into()),
            if served == 0 {
                "DARK".to_string()
            } else {
                served.to_string()
            },
            always,
            fold,
            edge_bar
                .as_ref()
                .map(|b| b.fold(&r.pool, limit).to_string())
                .unwrap_or_else(|| "—".into()),
            match bands {
                Some((s, c, n)) => format!("{s}●●● {c}●●○ {n}●○○"),
                None => "ungraded".to_string(),
            }
        );
    }
    for path in &poolless {
        println!(
            "{:<44}    0  (no candidate pool — no stored vectors to compare)",
            truncate(path, 44)
        );
    }

    println!("\n{}", "=".repeat(78));
    if let Some((min, med, max)) = pile_stats(&all_cos) {
        println!(
            "  cosine, all pools     min/med/max {min:.3}/{med:.3}/{max:.3}  (n={})",
            all_cos.len()
        );
    }
    match pile_stats(&leader_zs) {
        Some((min, med, max)) => println!(
            "  leader z              min/med/max {min:+.3}/{med:+.3}/{max:+.3}  (n={} graded anchors)",
            leader_zs.len()
        ),
        None => println!("  leader z              no graded anchor (every pool under {MIN_POPULATION} or zero variance)"),
    }
    println!(
        "  replayed z gate       {dark}/{} anchors dark, {gate_served_total} candidates served",
        readings.len()
    );
    println!(
        "  mutual-{mutual_k} fold         {fold_empty}/{} anchors fold to an empty default view, \
         {fold_served_total} candidates above the fold ({} below, still served — a fold, not a gate)",
        readings.len(),
        always_served_total.saturating_sub(fold_served_total)
    );
    println!(
        "  always-serve (top-{limit})  0/{} anchors dark, {always_served_total} candidates served",
        readings.len()
    );
    match &edge_bar {
        Some(bar) => {
            let (mut above, mut empty) = (0usize, 0usize);
            for r in &readings {
                let fold = bar.fold(&r.pool, limit);
                above += fold;
                if fold == 0 {
                    empty += 1;
                }
            }
            println!(
                "  authored-edge bar     {empty}/{} anchors fold to an empty default view, \
                 {above} candidates above the bar {:.3} (n={} edges, cos min/q1/med/max \
                 {:.3}/{:.3}/{:.3}/{:.3})",
                readings.len(),
                bar.bar(),
                bar.n,
                bar.min,
                bar.q1,
                bar.median,
                bar.max
            );
        }
        None => println!(
            "  authored-edge bar     UNPRICEABLE on this vault — no scorable authored edge, so \
             candidate 2 has no population to calibrate from"
        ),
    }
    println!(
        "  bands at top-{limit}       {strong} ●●● / {clear} ●●○ / {near} ●○○ across graded anchors ({ungraded} ungraded)"
    );
    let recheck_pairs: usize = readings.iter().map(|r| r.recheck_pairs()).sum();
    if worst_drift > 1e-3 {
        println!(
            "  [FAULT] engine z disagrees with the recomputation by up to {worst_drift:.3} — \
             the judged statistic moved; distrust the replay above"
        );
    } else if recheck_pairs == 0 {
        println!(
            "  [note]  the engine shipped no z on this vault (ungraded space or tiny pools) — \
             nothing to cross-check"
        );
    } else {
        println!(
            "  [check] engine z matches the recomputation over {recheck_pairs} candidates \
             (max Δ {worst_drift:.1e})"
        );
    }

    // The table comes first and the model loads after it, so the discovery
    // reading is on screen while the embedder is read off disk.
    if search {
        print_search_transfer(&read_search()?);
    }
    Ok(())
}

/// The same reading as one JSON object on stdout — for scripting a sweep (the A7
/// population-size sweep, a Phase-2 bake-off harness) without scraping the table.
#[allow(clippy::too_many_arguments)]
fn print_json(
    vault_root: &std::path::Path,
    model: &str,
    dim: usize,
    limit: usize,
    gate: &GateSim,
    mutual_k: usize,
    readings: &[AnchorReading],
    folds: &[(usize, Vec<bool>)],
    edge_bar: &Option<EdgeBar>,
    poolless: &[String],
    search: &Option<SearchReading>,
) {
    let row = serde_json::json!({
        "vault": vault_root.display().to_string(),
        "model": model,
        "dim": dim,
        "limit": limit,
        "gate": { "leader_z": gate.leader_z, "member_z": gate.member_z, "min_population": MIN_POPULATION },
        // The replayed fold's depth (GH #200): candidate B is reciprocal iff
        // the anchor sits in B's own top `mutual_k`; `fold_serves` below is the
        // ranked list's longest reciprocal prefix, capped at `limit`.
        "mutual_k": mutual_k,
        // Candidate 2's population and the bar read off it (GH #200) — `null`
        // on a link-free vault, which is the rule's own reading there.
        "edge_bar": edge_bar.as_ref().map(|b| serde_json::json!({
            "n": b.n,
            "bar": r4(b.bar()),
            "min": r4(b.min),
            "q1": r4(b.q1),
            "median": r4(b.median),
            "max": r4(b.max),
        })),
        "anchors": readings.iter().zip(folds).map(|(r, (fold, recip))| serde_json::json!({
            "anchor": r.path,
            "n": r.pool.len(),
            "gate_serves": r.gate_serves(gate, limit),
            "fold_serves": fold,
            "edge_bar_serves": edge_bar.as_ref().map(|b| b.fold(&r.pool, limit)),
            "always_serves": r.pool.len().min(limit),
            "bands": r.bands(limit).map(|(s, c, n)| serde_json::json!({ "strong": s, "clear": c, "near": n })),
            "pool": r.pool.iter().enumerate().map(|(i, (path, cos, engine_z))| serde_json::json!({
                "path": path,
                "cos": r4(*cos),
                "z": r.z.as_ref().map(|z| r4(z[i])),
                "engine_z": engine_z.map(r4),
                "reciprocal": recip[i],
            })).collect::<Vec<_>>(),
        })).collect::<Vec<_>>(),
        "no_pool": poolless,
        // Additive beside the discovery object, never redefining a key of it
        // (GH #219): `null` means `--search` was not asked for, which is a fact
        // about the invocation, not about the vault.
        "search": search.as_ref().map(search_json),
    });
    println!("{row}");
}

/// The search-side bench as JSON (GH #219) — the same reading
/// [`print_search_transfer`] renders as text.
///
/// It follows the discovery object's **re-derivability** convention: every
/// summary the text block prints is left to the consumer, and what is emitted is
/// what the summary was computed *from* — each probe's own query, coverage,
/// cosine and the engine's own `vouched`, plus every served row's provenance. A
/// sweep that wants "how many titles did the bar cut" counts them; a sweep that
/// wants something the table never printed can have it too.
///
/// The one exception is the tail families, which are emitted **derived**: a
/// family's edge is an extremum over the whole positive pile, and a consumer
/// re-deriving it would be re-implementing the fold's admissibility rule rather
/// than reading a number off it. They are re-derivable from `positives` all the
/// same.
fn search_json(reading: &SearchReading) -> serde_json::Value {
    let probe = |p: &SearchProbe| {
        serde_json::json!({
            "query": p.query,
            "coverage": p.coverage.map(r4),
            "best_cos": p.best_cos.map(r4),
            // The engine's verdict, not a restatement of it.
            "vouched": p.vouched,
            "own_path": p.own_path,
            // 0-based, like `bm25_rank`; `null` = never served within `limit`.
            "own_rank": p.own_rank(),
            "rows": p.rows.iter().map(|r| serde_json::json!({
                "path": r.path,
                "bm25_rank": r.bm25_rank,
                "cos": r.cos.map(r4),
            })).collect::<Vec<_>>(),
        })
    };
    // A family reads one of three ways, exactly as the text block prints it:
    // `needs` (this row pins the constant), `unconstrained` (no row engages the
    // test), `dead` (a row no constant can admit).
    let edge = |e: &Option<TailEdge>| match e {
        None => serde_json::json!({ "status": "unconstrained" }),
        Some(e) => serde_json::json!({
            "status": "needs",
            "needs": r4(e.value),
            "query": e.query,
            "path": e.path,
        }),
    };
    let dead = |d: &DeadRow| {
        serde_json::json!({
            "status": "dead",
            "query": d.query,
            "path": d.path,
        })
    };
    serde_json::json!({
        "chunk_total": reading.chunk_total,
        // The lexical anchor's premise, model-free and read per vault: what a
        // word this vault has never seen weighs, and what each function word
        // weighs beside it.
        "function_words": {
            "absent_idf": r4(reading.absent_idf),
            "heaviest_share_pct": r4(reading.heaviest_share()),
            "terms": reading.function_words.iter().map(|(term, idf)| serde_json::json!({
                "term": term,
                "idf": r4(*idf),
            })).collect::<Vec<_>>(),
        },
        // One key, not four: the bar and the three piles read against it exist
        // together or not at all, so they nest under the `Option` that decides
        // it rather than each carrying its own `null` for a consumer to test.
        // `null` = no calibrated bar for this model, so nothing under it was
        // read: the piles need one to be judged (M2). The function-word reading
        // above is model-free and is there either way.
        "judged": reading.judged.as_ref().map(|j| serde_json::json!({
            "bar": {
                "min_term_coverage": j.bar.min_term_coverage,
                "min_cos": j.bar.min_cos,
            },
            "positives": j.positives.iter().map(probe).collect::<Vec<_>>(),
            "negatives": j.negatives.iter().map(probe).collect::<Vec<_>>(),
            "tail": {
                "own_served": j.tail.own_served,
                "own_missed": j.tail.own_missed,
                "dense_only_rows": j.tail.dense_only_rows,
                "total_rows": j.tail.total_rows,
                "families": {
                    "lexical": {
                        "hides": j.tail.lex_hidden.len(),
                        "of": j.tail.own_served,
                        "hidden": j.tail.lex_hidden.iter().map(|h| serde_json::json!({
                            "query": h.query,
                            "own_rank": h.own_rank,
                            "dense_only_rank": h.dense_only_rank,
                        })).collect::<Vec<_>>(),
                    },
                    "cos": match &j.tail.cos_dead {
                        Some(d) => dead(d),
                        None => edge(&j.tail.cos_edge),
                    },
                    // A dead row that is also dense-only kills the two-signal
                    // family too; one that is not leaves it constrained.
                    "lex_or_cos": match &j.tail.cos_dead {
                        Some(d) if d.dense_only => dead(d),
                        _ => edge(&j.tail.lexcos_edge),
                    },
                    "cos_drop": match &j.tail.cos_dead {
                        Some(d) => dead(d),
                        None => edge(&j.tail.drop_edge),
                    },
                },
            },
        })),
    })
}

/// Four decimals, the one rounding this object applies — enough to reproduce
/// every printed reading, short of dumping a float's full noise into a dataset
/// meant to be diffed.
fn r4(x: f64) -> f64 {
    (x * 1e4).round() / 1e4
}

fn truncate(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        s.to_string()
    } else {
        let cut: String = s.chars().take(max - 1).collect();
        format!("{cut}…")
    }
}
