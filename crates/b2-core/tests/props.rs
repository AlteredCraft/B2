//! #18 — property tests over **generated vaults**, pinning the load-bearing
//! invariants the golden-vault scenarios cover only pointwise (invariants.md):
//!
//!   1. **Round-trip is lossless** — `parse → serialize` is byte-identical for *any*
//!      text, and the surgical edits (`stamp_b2id`, `replace_body`) are exact splices
//!      that touch nothing else (data-model.md §6).
//!   2. **`full reindex ≡ incremental`** — after any sequence of *external* vault
//!      mutations (edits, adds, deletes, renames through plain `fs`, the hard path),
//!      the incrementally maintained index equals a from-scratch rebuild of the same
//!      files ("index = a pure projection of (the vault directory)").
//!   3. **Rename keeps every backlink resolving** (`b2 mv`) — and the repair lives in
//!      the *Markdown*, not just the DB: a drop-and-rebuild sees the same graph.
//!
//! **Determinism (the suite's hard rule):** the runner uses a *fixed* ChaCha seed, so
//! every run explores the identical case sequence — no flaky CI, and a failure
//! reproduces exactly. Shrinking still works. To explore new ground, change `SEED`
//! or raise the case counts locally; commit any find as a regular regression test.

use b2_core::note::parse;
use b2_core::vault::Vault;
use proptest::prelude::*;
use proptest::test_runner::{Config, RngAlgorithm, TestRng, TestRunner};
use rusqlite::Connection;
use std::fs;
use std::path::{Path, PathBuf};

/// 32 bytes for ChaCha — the one knob that picks the (fixed) case sequence.
const SEED: &[u8; 32] = b"b2-props-deterministic-seed-01!!";

/// Run `cases` deterministic cases of `strategy` through `test`, panicking with the
/// shrunken counterexample on failure (proptest's own report).
fn check<S: Strategy>(
    cases: u32,
    strategy: S,
    test: impl Fn(S::Value) -> Result<(), TestCaseError>,
) {
    let mut runner = TestRunner::new_with_rng(
        Config {
            cases,
            // Deterministic seed ⇒ nothing to persist; keeps the repo free of
            // proptest-regressions files.
            failure_persistence: None,
            ..Config::default()
        },
        TestRng::from_seed(RngAlgorithm::ChaCha, SEED),
    );
    if let Err(e) = runner.run(&strategy, test) {
        panic!("{e}");
    }
}

// --- invariant 1: lossless round-trip & surgical edits --------------------------

#[test]
fn any_text_round_trips_byte_identical() {
    check(512, any::<String>(), |s| {
        let n = parse(&s);
        prop_assert_eq!(n.as_str(), s.as_str());
        Ok(())
    });
}

/// The stamp is a *single surgical insertion* (note.rs contract): one `b2id:` line at
/// the top of the frontmatter — or a minimal block if there is none — every other
/// byte untouched, and never a re-stamp.
#[test]
fn stamping_is_a_single_surgical_insertion() {
    let id = "01JPQ000000000000000000000";
    check(512, any::<String>(), |s| {
        let before = parse(&s);
        let mut n = before.clone();
        n.stamp_b2id(id);
        if before.fields().b2id.is_some() {
            prop_assert_eq!(
                n.as_str(),
                s.as_str(),
                "an existing b2id is never re-stamped"
            );
        } else if let Some(old_fm) = before.frontmatter() {
            let want = format!("b2id: {id}\n{old_fm}");
            prop_assert_eq!(n.frontmatter(), Some(want.as_str()));
            prop_assert_eq!(n.body(), before.body(), "the body must not move");
        } else {
            let want = format!("b2id: {id}\n");
            prop_assert_eq!(n.frontmatter(), Some(want.as_str()));
            prop_assert_eq!(n.body(), s.as_str(), "the whole original text is the body");
        }
        Ok(())
    });
}

/// `replace_body` is the byte-honest splice behind `Vault::write`: the result is
/// exactly (everything up to the old body) + the new body, for any inputs.
#[test]
fn replace_body_splices_bytes_exactly() {
    check(512, (any::<String>(), any::<String>()), |(s, new_body)| {
        let mut n = parse(&s);
        let prefix_len = s.len() - n.body().len();
        n.replace_body(&new_body);
        let want = format!("{}{}", &s[..prefix_len], new_body);
        prop_assert_eq!(n.as_str(), want.as_str());
        Ok(())
    });
}

// --- generated vaults -----------------------------------------------------------

const DIRS: &[&str] = &["", "notes", "concepts", "deep/nested"];
/// The closed stance core plus one tolerated tail verb (relation.rs).
const VERBS: &[&str] = &["references", "supports", "contradicts", "extends"];
const RES_EXTS: &[&str] = &["png", "txt", "bin"];

/// A deterministic, ULID-shaped b2id per note index, so generated vaults never
/// depend on the façade's real `UlidGen` (whose ids are random but value-irrelevant
/// to every property here).
fn prop_id(i: usize) -> String {
    format!("01JPQ{i:021}")
}

fn sans_md(path: &str) -> &str {
    path.strip_suffix(".md").unwrap_or(path)
}

#[derive(Debug, Clone)]
struct NoteSpec {
    dir_ix: usize,
    slug: String,
    /// Pre-stamped with a deterministic b2id, or left for ingest to stamp.
    stamped: bool,
    title: Option<String>,
    /// A frontmatter key B2 doesn't model — must survive everything verbatim.
    custom: Option<String>,
    paras: Vec<String>,
    /// Body wikilinks: (target note index (mod n), aliased?).
    links: Vec<(usize, bool)>,
    /// Frontmatter `b2_relations:` entries: (target note index (mod n), verb index).
    rels: Vec<(usize, usize)>,
    /// Markdown-form resource links: target resource index (mod r).
    res_links: Vec<usize>,
}

#[derive(Debug, Clone)]
struct VaultSpec {
    notes: Vec<NoteSpec>,
    /// (slug, extension index, bytes) — written under `assets/`.
    resources: Vec<(String, usize, Vec<u8>)>,
}

fn para() -> impl Strategy<Value = String> {
    prop::collection::vec("[a-z]{2,8}", 3..9).prop_map(|ws| ws.join(" "))
}

fn note_spec() -> impl Strategy<Value = NoteSpec> {
    (
        0..DIRS.len(),
        "[a-z][a-z0-9]{0,6}",
        any::<bool>(),
        prop::option::of("[A-Za-z][A-Za-z0-9 ]{0,18}"),
        prop::option::of("[a-z]{2,8}"),
        prop::collection::vec(para(), 1..4),
        prop::collection::vec((any::<usize>(), any::<bool>()), 0..4),
        prop::collection::vec((any::<usize>(), 0..VERBS.len()), 0..3),
        prop::collection::vec(any::<usize>(), 0..2),
    )
        .prop_map(
            |(dir_ix, slug, stamped, title, custom, paras, links, rels, res_links)| NoteSpec {
                dir_ix,
                slug,
                stamped,
                title,
                custom,
                paras,
                links,
                rels,
                res_links,
            },
        )
}

fn vault_spec(min_notes: usize) -> impl Strategy<Value = VaultSpec> {
    (
        prop::collection::vec(note_spec(), min_notes..6),
        prop::collection::vec(
            (
                "[a-z]{2,6}",
                0..RES_EXTS.len(),
                prop::collection::vec(any::<u8>(), 0..48),
            ),
            0..3,
        ),
    )
        .prop_map(|(notes, resources)| VaultSpec { notes, resources })
}

impl VaultSpec {
    /// Vault-relative note paths, unique by construction (the index suffix).
    fn note_paths(&self) -> Vec<String> {
        self.notes
            .iter()
            .enumerate()
            .map(|(i, n)| in_dir(n.dir_ix, &format!("{}-{}.md", n.slug, i)))
            .collect()
    }

    fn res_paths(&self) -> Vec<String> {
        self.resources
            .iter()
            .enumerate()
            .map(|(i, (slug, ext, _))| format!("assets/{}-r{}.{}", slug, i, RES_EXTS[*ext]))
            .collect()
    }

    fn write(&self, root: &Path) {
        let paths = self.note_paths();
        let res = self.res_paths();
        for (i, n) in self.notes.iter().enumerate() {
            let id = n.stamped.then(|| prop_id(i));
            write_file(
                &root.join(&paths[i]),
                render_note(id.as_deref(), n, &paths[i], &paths, &res).as_bytes(),
            );
        }
        for (i, (_, _, bytes)) in self.resources.iter().enumerate() {
            write_file(&root.join(&res[i]), bytes);
        }
    }
}

fn in_dir(dir_ix: usize, file: &str) -> String {
    let dir = DIRS[dir_ix % DIRS.len()];
    if dir.is_empty() {
        file.to_string()
    } else {
        format!("{dir}/{file}")
    }
}

fn write_file(path: &Path, bytes: &[u8]) {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).unwrap();
    }
    fs::write(path, bytes).unwrap();
}

/// Render one note's raw text. Self-links are skipped (the rename property is about
/// *inbound* repair; a self-link's semantics under `mv` is its own question).
fn render_note(
    id: Option<&str>,
    n: &NoteSpec,
    own_path: &str,
    note_targets: &[String],
    res_targets: &[String],
) -> String {
    let mut fm: Vec<String> = Vec::new();
    if let Some(id) = id {
        fm.push(format!("b2id: {id}"));
    }
    if let Some(t) = &n.title {
        fm.push(format!("title: \"{t}\""));
    }
    if let Some(c) = &n.custom {
        fm.push(format!("x_custom: {c}"));
    }
    let rel_targets: Vec<(&str, &str)> = n
        .rels
        .iter()
        .filter_map(|(t, v)| {
            if note_targets.is_empty() {
                return None;
            }
            let target = note_targets[t % note_targets.len()].as_str();
            (target != own_path).then(|| (VERBS[v % VERBS.len()], sans_md(target)))
        })
        .collect();
    if !rel_targets.is_empty() {
        fm.push("b2_relations:".to_string());
        for (verb, target) in rel_targets {
            fm.push(format!("  - \"{verb} [[{target}]] — property vault\""));
        }
    }

    let mut out = String::new();
    if !fm.is_empty() {
        out.push_str("---\n");
        for line in &fm {
            out.push_str(line);
            out.push('\n');
        }
        out.push_str("---\n");
    }
    for p in &n.paras {
        out.push_str(p);
        out.push_str("\n\n");
    }
    for (t, aliased) in &n.links {
        if note_targets.is_empty() {
            continue;
        }
        let target = note_targets[t % note_targets.len()].as_str();
        if target == own_path {
            continue;
        }
        let target = sans_md(target);
        if *aliased {
            out.push_str(&format!("See [[{target}|Alias]].\n"));
        } else {
            out.push_str(&format!("See [[{target}]].\n"));
        }
    }
    for r in &n.res_links {
        if !res_targets.is_empty() {
            out.push_str(&format!(
                "![figure]({})\n",
                res_targets[r % res_targets.len()]
            ));
        }
    }
    out
}

// --- invariant 2: full reindex ≡ incremental ------------------------------------

/// External mutations — everything through plain `fs`, never the façade's write
/// ops, because *this* is the path the incremental reindex has to reconcile (an
/// Obsidian edit, a `git pull`, a Finder rename). Indices are taken modulo the
/// live file lists at apply time; an op against an empty list is a no-op.
#[derive(Debug, Clone)]
enum Mutation {
    EditBody {
        note: usize,
        paras: Vec<String>,
        links: Vec<(usize, bool)>,
    },
    AddNote(NoteSpec),
    DeleteNote {
        note: usize,
    },
    ExternalMove {
        note: usize,
        dir_ix: usize,
        slug: String,
    },
    AddResource {
        slug: String,
        ext_ix: usize,
        bytes: Vec<u8>,
    },
    DeleteResource {
        res: usize,
    },
}

fn mutation() -> impl Strategy<Value = Mutation> {
    prop_oneof![
        (
            any::<usize>(),
            prop::collection::vec(para(), 1..3),
            prop::collection::vec((any::<usize>(), any::<bool>()), 0..3),
        )
            .prop_map(|(note, paras, links)| Mutation::EditBody { note, paras, links }),
        note_spec().prop_map(Mutation::AddNote),
        any::<usize>().prop_map(|note| Mutation::DeleteNote { note }),
        (any::<usize>(), 0..DIRS.len(), "[a-z]{2,6}")
            .prop_map(|(note, dir_ix, slug)| Mutation::ExternalMove { note, dir_ix, slug }),
        (
            "[a-z]{2,6}",
            0..RES_EXTS.len(),
            prop::collection::vec(any::<u8>(), 0..48),
        )
            .prop_map(|(slug, ext_ix, bytes)| Mutation::AddResource {
                slug,
                ext_ix,
                bytes
            }),
        any::<usize>().prop_map(|res| Mutation::DeleteResource { res }),
    ]
}

/// The live vault-file bookkeeping mutations run against (paths only — content
/// lives on disk, where the invariant says it must).
struct LiveVault {
    root: PathBuf,
    notes: Vec<String>,
    resources: Vec<String>,
    /// Monotonic counter salting minted filenames/ids so they never collide.
    minted: usize,
}

fn apply(m: &Mutation, lv: &mut LiveVault) {
    match m {
        Mutation::EditBody { note, paras, links } if !lv.notes.is_empty() => {
            let path = lv.notes[note % lv.notes.len()].clone();
            let raw = fs::read_to_string(lv.root.join(&path)).unwrap();
            let mut n = parse(&raw);
            let mut body = String::new();
            for p in paras {
                body.push_str(p);
                body.push_str("\n\n");
            }
            for (t, aliased) in links {
                let target = lv.notes[t % lv.notes.len()].as_str();
                if target == path {
                    continue;
                }
                let target = sans_md(target);
                if *aliased {
                    body.push_str(&format!("See [[{target}|Alias]].\n"));
                } else {
                    body.push_str(&format!("See [[{target}]].\n"));
                }
            }
            n.replace_body(&body);
            fs::write(lv.root.join(&path), n.as_str()).unwrap();
        }
        Mutation::AddNote(spec) => {
            lv.minted += 1;
            let path = in_dir(spec.dir_ix, &format!("{}-x{}.md", spec.slug, lv.minted));
            let id = spec.stamped.then(|| prop_id(1000 + lv.minted));
            let contents = render_note(id.as_deref(), spec, &path, &lv.notes, &lv.resources);
            write_file(&lv.root.join(&path), contents.as_bytes());
            lv.notes.push(path);
        }
        Mutation::DeleteNote { note } if !lv.notes.is_empty() => {
            let ix = note % lv.notes.len();
            fs::remove_file(lv.root.join(&lv.notes[ix])).unwrap();
            lv.notes.remove(ix);
        }
        Mutation::ExternalMove { note, dir_ix, slug } if !lv.notes.is_empty() => {
            lv.minted += 1;
            let ix = note % lv.notes.len();
            let to = in_dir(*dir_ix, &format!("{}-m{}.md", slug, lv.minted));
            let to_abs = lv.root.join(&to);
            if let Some(parent) = to_abs.parent() {
                fs::create_dir_all(parent).unwrap();
            }
            fs::rename(lv.root.join(&lv.notes[ix]), &to_abs).unwrap();
            lv.notes[ix] = to;
        }
        Mutation::AddResource {
            slug,
            ext_ix,
            bytes,
        } => {
            lv.minted += 1;
            let path = format!(
                "assets/{}-y{}.{}",
                slug,
                lv.minted,
                RES_EXTS[ext_ix % RES_EXTS.len()]
            );
            write_file(&lv.root.join(&path), bytes);
            lv.resources.push(path);
        }
        Mutation::DeleteResource { res } if !lv.resources.is_empty() => {
            let ix = res % lv.resources.len();
            fs::remove_file(lv.root.join(&lv.resources[ix])).unwrap();
            lv.resources.remove(ix);
        }
        _ => {} // an op against an emptied list — no-op
    }
}

/// The index's *logical* content, one sorted line per row — everything the
/// projection derives, minus what may legitimately differ between two builds of
/// the same files: `indexed_at` (wall-clock, set DB-side), `mtime` (fs metadata,
/// not projection output), and `chunks.id` (an internal rowid; chunks are keyed
/// here by their identity `(note, seq)`). Vectors and centroids are included —
/// the FakeEmbedder is content-addressed, so they too must be pure projections.
fn dump(root: &Path) -> Vec<String> {
    let conn = b2_core::open(&root.join(".b2").join("b2.sqlite")).unwrap();
    let sections: &[(&str, &str, usize)] = &[
        (
            "note",
            "SELECT b2id, path, type, ifnull(title,''), ifnull(description,''),
                    ifnull(created,''), ifnull(updated,''), body_hash
             FROM notes ORDER BY path",
            8,
        ),
        (
            "alias",
            "SELECT note_b2id, alias FROM note_aliases ORDER BY note_b2id, alias",
            2,
        ),
        (
            "chunk",
            "SELECT note_b2id, CAST(seq AS TEXT), CAST(char_start AS TEXT),
                    CAST(char_end AS TEXT), CAST(token_count AS TEXT),
                    ifnull(heading_path,''), text
             FROM chunks ORDER BY note_b2id, seq",
            7,
        ),
        (
            "edge",
            "SELECT id, src_id, ifnull(dst_id,''), ifnull(dst_resource_path,''),
                    dst_path_raw, type, origin, ifnull(explanation,''),
                    CAST(embed AS TEXT), ifnull(caption,''), CAST(occurrence_index AS TEXT)
             FROM edges ORDER BY id",
            11,
        ),
        (
            "res",
            "SELECT path, class, CAST(size AS TEXT), content_hash
             FROM resources ORDER BY path",
            4,
        ),
        (
            "vec",
            "SELECT c.note_b2id, CAST(c.seq AS TEXT), hex(e.vector)
             FROM embeddings e JOIN chunks c ON c.id = e.chunk_id
             ORDER BY c.note_b2id, c.seq",
            3,
        ),
        (
            "centroid",
            "SELECT note_b2id, hex(centroid) FROM note_centroids ORDER BY note_b2id",
            2,
        ),
    ];
    let mut out = Vec::new();
    for (section, sql, cols) in sections {
        // The vector tables exist only once an embed pass has run (their existence
        // is the "this vault has an embedding space" signal) — absent table,
        // empty section.
        let guard_table = match *section {
            "vec" => Some("embeddings"),
            "centroid" => Some("note_centroids"),
            _ => None,
        };
        if let Some(t) = guard_table {
            if !table_exists(&conn, t) {
                continue;
            }
        }
        let mut stmt = conn.prepare(sql).unwrap();
        let mut rows = stmt.query([]).unwrap();
        while let Some(row) = rows.next().unwrap() {
            let mut parts = vec![section.to_string()];
            for i in 0..*cols {
                parts.push(row.get::<_, String>(i).unwrap());
            }
            out.push(parts.join("\t"));
        }
    }
    out
}

fn table_exists(conn: &Connection, name: &str) -> bool {
    conn.query_row(
        "SELECT count(*) FROM sqlite_master WHERE type = 'table' AND name = ?1",
        [name],
        |r| r.get::<_, i64>(0),
    )
    .unwrap()
        > 0
}

#[test]
fn incremental_reindex_equals_full_rebuild() {
    check(
        32,
        (vault_spec(1), prop::collection::vec(mutation(), 1..6)),
        |(spec, muts)| {
            let tmp = tempfile::TempDir::new().unwrap();
            let root = tmp.path().join("vault");
            spec.write(&root);

            let vault = Vault::open(&root).unwrap();
            vault.reindex().unwrap();

            let mut lv = LiveVault {
                root: root.clone(),
                notes: spec.note_paths(),
                resources: spec.res_paths(),
                minted: 0,
            };
            // Reindex after EVERY mutation, so the incremental path is a *chain* of
            // reconciles — the shape a live vault actually produces — not one lump.
            for m in &muts {
                apply(m, &mut lv);
                vault.reindex().unwrap();
            }
            let incremental = dump(&root);
            drop(vault);

            // The index is disposable: drop it and rebuild from the same files.
            fs::remove_dir_all(root.join(".b2")).unwrap();
            let fresh = Vault::open(&root).unwrap();
            fresh.reindex().unwrap();
            drop(fresh);
            let full = dump(&root);

            prop_assert_eq!(incremental, full);
            Ok(())
        },
    );
}

// --- invariant 3: rename keeps every backlink resolving --------------------------

/// A note's inbound set as sortable `(label, src_b2id)` pairs — the thing a move
/// must leave unchanged (same helper shape as tests/mv.rs).
fn inbound(vault: &Vault, note_ref: &str) -> Vec<(String, String)> {
    let mut ns: Vec<(String, String)> = vault
        .neighbors(note_ref)
        .unwrap()
        .into_iter()
        .filter(|n| n.direction == "inbound")
        .map(|n| (n.label, n.b2id))
        .collect();
    ns.sort();
    ns
}

#[test]
fn rename_keeps_every_backlink_resolving() {
    check(
        32,
        (vault_spec(2), any::<usize>(), 0..DIRS.len(), "[a-z]{2,6}"),
        |(mut spec, mover, dest_dir, dest_slug)| {
            // Known ids for every note so the graph is addressable by construction.
            for n in &mut spec.notes {
                n.stamped = true;
            }
            let tmp = tempfile::TempDir::new().unwrap();
            let root = tmp.path().join("vault");
            spec.write(&root);

            let paths = spec.note_paths();
            let mover_ix = mover % paths.len();
            let mover_id = prop_id(mover_ix);
            // Guarantee at least one inbound link, whatever the generator rolled:
            // a neighboring note gains a body wikilink to the mover.
            let other = &paths[(mover_ix + 1) % paths.len()];
            let other_abs = root.join(other);
            let mut other_raw = fs::read_to_string(&other_abs).unwrap();
            other_raw.push_str(&format!("\nAlso see [[{}]].\n", sans_md(&paths[mover_ix])));
            fs::write(&other_abs, other_raw).unwrap();

            let vault = Vault::open(&root).unwrap();
            vault.reindex().unwrap();

            let before = inbound(&vault, &mover_id);
            prop_assert!(
                !before.is_empty(),
                "the appended link guarantees an inbound edge"
            );

            // `moved-` + an alpha-only slug can never collide with the generated
            // files (whose names always end in a `-<counter>` suffix).
            let dest = in_dir(dest_dir, &format!("moved-{dest_slug}.md"));
            vault.move_note(&paths[mover_ix], &dest).unwrap();

            prop_assert_eq!(&inbound(&vault, &mover_id), &before, "backlink set by b2id");
            prop_assert_eq!(
                &inbound(&vault, &dest),
                &before,
                "resolvable by the new path"
            );
            prop_assert!(
                vault.read(&paths[mover_ix]).is_err(),
                "the old path must no longer resolve"
            );
            drop(vault);

            // The repair must live in the Markdown, not just the DB: a fresh
            // rebuild from the files alone sees the identical backlink set.
            fs::remove_dir_all(root.join(".b2")).unwrap();
            let fresh = Vault::open(&root).unwrap();
            fresh.reindex().unwrap();
            prop_assert_eq!(
                &inbound(&fresh, &mover_id),
                &before,
                "the graph survives a rebuild"
            );
            prop_assert_eq!(
                fresh.read(&dest).unwrap().b2id,
                mover_id,
                "the moved file answers at its new path"
            );
            Ok(())
        },
    );
}
