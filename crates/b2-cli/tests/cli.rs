//! CLI-level tests: run the built `b2` binary against a temp copy of the
//! golden-vault fixture and assert its output — the "run a command against a
//! fixture, assert the output" surface vision-and-scope names. The binary path is
//! `CARGO_BIN_EXE_b2`, which cargo provides to integration tests (so no extra test
//! harness dependency is needed). The CLI is a dumb adapter over `b2_core::Vault`;
//! these prove the wiring + output shape, not engine behavior (that's the façade
//! and engine tests).

use serde_json::Value;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

const MEMORY_ID: &str = "01JMEM0000000000000000000A";

/// A temp copy of the golden vault (so reindex, which may stamp, never touches the
/// repo fixtures). The `TempDir` guard is returned so it outlives the test.
fn golden_vault() -> (tempfile::TempDir, PathBuf) {
    let tmp = tempfile::TempDir::new().unwrap();
    let root = tmp.path().join("vault");
    let src = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/golden-vault");
    copy_dir(&src, &root);
    (tmp, root)
}

fn copy_dir(src: &Path, dst: &Path) {
    std::fs::create_dir_all(dst).unwrap();
    for entry in std::fs::read_dir(src).unwrap() {
        let entry = entry.unwrap();
        let from = entry.path();
        let to = dst.join(entry.file_name());
        if from.is_dir() {
            copy_dir(&from, &to);
        } else {
            std::fs::copy(&from, &to).unwrap();
        }
    }
}

/// Run `b2 <args...>` and capture the result. The suite runs under the fake
/// embedder (`B2_EMBEDDER=fake`) so CI never downloads or runs the real model — it
/// proves the wiring + output shape, not model quality (tasks.md testability 4–5).
fn run(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_b2"))
        .env("B2_EMBEDDER", "fake")
        .args(args)
        .output()
        .expect("b2 binary runs")
}

/// Run `b2 -C <vault> <args...>` (the common case: point at a vault).
fn run_in(vault: &Path, args: &[&str]) -> Output {
    let mut full = vec!["-C", vault.to_str().unwrap()];
    full.extend_from_slice(args);
    run(&full)
}

fn stdout(o: &Output) -> String {
    String::from_utf8(o.stdout.clone()).unwrap()
}
fn stderr(o: &Output) -> String {
    String::from_utf8(o.stderr.clone()).unwrap()
}

/// Reindex a fresh golden vault; returns the guard + root ready for querying.
fn reindexed() -> (tempfile::TempDir, PathBuf) {
    let (tmp, root) = golden_vault();
    let out = run_in(&root, &["reindex"]);
    assert!(out.status.success(), "reindex failed: {}", stderr(&out));
    (tmp, root)
}

#[test]
fn reindex_reports_counts_human_and_json() {
    let (_g, root) = golden_vault();

    let human = run_in(&root, &["reindex"]);
    assert!(human.status.success());
    assert!(
        stdout(&human).contains("Indexed 2"),
        "human output: {:?}",
        stdout(&human)
    );

    let json = run_in(&root, &["--json", "reindex"]);
    assert!(json.status.success());
    let v: Value = serde_json::from_slice(&json.stdout).unwrap();
    assert_eq!(v["indexed"], 2);
    assert_eq!(v["stamped"], 0);

    // the index + log folder is created inside the vault (one portable folder).
    assert!(root.join(".b2/b2.sqlite").is_file());
}

#[test]
fn reindex_is_incremental_and_force_reembeds() {
    let (_g, root) = golden_vault();

    // First reindex embeds both notes.
    let first = run_in(&root, &["--json", "reindex"]);
    assert!(first.status.success(), "{}", stderr(&first));
    let v: Value = serde_json::from_slice(&first.stdout).unwrap();
    assert_eq!(v["indexed"], 2);
    assert_eq!(v["embedded"], 2);

    // Nothing changed on disk → the second reindex re-embeds nothing.
    let again = run_in(&root, &["--json", "reindex"]);
    let v: Value = serde_json::from_slice(&again.stdout).unwrap();
    assert_eq!(v["embedded"], 0, "unchanged notes are not re-embedded");

    // --force re-embeds every note regardless.
    let forced = run_in(&root, &["--json", "reindex", "--force"]);
    let v: Value = serde_json::from_slice(&forced.stdout).unwrap();
    assert_eq!(v["embedded"], 2, "--force re-embeds everything");
}

#[test]
fn reindex_dry_run_previews_and_writes_nothing() {
    let (_g, root) = golden_vault();
    // A note with no b2id, so the preview has something to "would stamp".
    std::fs::write(
        root.join("fresh.md"),
        "---\ntype: note\ntitle: Fresh\n---\nNo b2id yet.\n",
    )
    .unwrap();
    let before = std::fs::read_to_string(root.join("fresh.md")).unwrap();

    // JSON: honest `would_*` keys, never the past-tense reindex shape.
    let json = run_in(&root, &["--json", "reindex", "--dry-run"]);
    assert!(json.status.success(), "{}", stderr(&json));
    let v: Value = serde_json::from_slice(&json.stdout).unwrap();
    assert_eq!(v["would_index"], 3);
    assert_eq!(v["would_embed"], 3);
    assert_eq!(v["would_stamp"], 1);
    assert!(v.get("indexed").is_none(), "not the real-reindex shape");

    // Human: says it's a preview and made no changes.
    let human = run_in(&root, &["reindex", "--dry-run"]);
    assert!(human.status.success(), "{}", stderr(&human));
    assert!(stdout(&human).contains("Dry run"), "{:?}", stdout(&human));
    assert!(
        stdout(&human).contains("No changes made"),
        "{:?}",
        stdout(&human)
    );

    // Nothing was written: the b2id-less note is byte-identical (never stamped),
    // and the work is still pending — a real reindex now embeds all 3.
    assert_eq!(
        std::fs::read_to_string(root.join("fresh.md")).unwrap(),
        before
    );
    let real = run_in(&root, &["--json", "reindex"]);
    let v: Value = serde_json::from_slice(&real.stdout).unwrap();
    assert_eq!(v["indexed"], 3);
    assert_eq!(v["embedded"], 3, "the dry-run did no embedding work");
    assert_eq!(v["stamped"], 1);
}

#[test]
fn reindex_accepts_a_positional_vault() {
    let (_g, root) = golden_vault();
    // the spec's `b2 reindex [vault]` form, no -C.
    let out = run(&["reindex", root.to_str().unwrap()]);
    assert!(out.status.success(), "{}", stderr(&out));
    assert!(stdout(&out).contains("Indexed 2"));
}

// --- note CRUD: add -----------------------------------------------------------

#[test]
fn add_creates_a_note_human_and_json() {
    let (_g, root) = golden_vault();

    // Human: reports the created path + b2id.
    let human = run_in(
        &root,
        &[
            "add",
            "notes/gadgets",
            "--title",
            "All about gadgets",
            "--content",
            "Gadgets are handy little devices.",
        ],
    );
    assert!(human.status.success(), "{}", stderr(&human));
    assert!(stdout(&human).contains("Created"), "{:?}", stdout(&human));
    assert!(
        stdout(&human).contains("notes/gadgets.md"),
        "{:?}",
        stdout(&human)
    );

    // The file exists with a stamped, titled frontmatter and the body.
    let text = std::fs::read_to_string(root.join("notes/gadgets.md")).unwrap();
    assert!(text.contains("b2id:"), "{text}");
    assert!(text.contains(r#"title: "All about gadgets""#), "{text}");
    assert!(text.contains("Gadgets are handy little devices."), "{text}");

    // Immediately searchable (keyword half is real even under the fake embedder).
    let search = run_in(&root, &["--json", "search", "gadgets"]);
    let v: Value = serde_json::from_slice(&search.stdout).unwrap();
    assert!(v
        .as_array()
        .unwrap()
        .iter()
        .any(|h| h["path"] == "notes/gadgets.md"));

    // JSON: a new note at a different path returns the report shape.
    let json = run_in(&root, &["--json", "add", "notes/another"]);
    assert!(json.status.success(), "{}", stderr(&json));
    let v: Value = serde_json::from_slice(&json.stdout).unwrap();
    assert_eq!(v["path"], "notes/another.md");
    assert!(v["b2id"].as_str().is_some_and(|s| !s.is_empty()));
}

#[test]
fn add_refuses_to_clobber_and_reports_it() {
    let (_g, root) = golden_vault();
    let out = run_in(&root, &["add", "concepts/memory.md"]);
    assert!(!out.status.success(), "clobber must be a nonzero exit");
    let err = stderr(&out).to_lowercase();
    assert!(err.contains("already exists"), "actionable message: {err}");
    assert!(!err.contains("panicked"), "no stack trace: {err}");
}

#[test]
fn add_invalid_path_fails_cleanly() {
    let (_g, root) = golden_vault();
    let out = run_in(&root, &["add", "../escape.md"]);
    assert!(!out.status.success(), "invalid path must be a nonzero exit");
    let err = stderr(&out).to_lowercase();
    assert!(err.contains("path"), "actionable message: {err}");
    assert!(!err.contains("panicked"), "no stack trace: {err}");
}

// --- explain: a note's connections with their why -----------------------------

#[test]
fn explain_shows_connections_human_and_json() {
    let (_g, root) = reindexed();

    let human = run_in(&root, &["explain", "notes/spaced-repetition"]);
    assert!(human.status.success(), "{}", stderr(&human));
    let out = stdout(&human);
    assert!(out.contains("Spaced repetition"), "header: {out}");
    assert!(out.contains("elaborates"), "{out}");
    assert!(out.contains("why:"), "the explanation is labelled: {out}");
    assert!(out.contains("forgetting curve"), "{out}");

    let json = run_in(&root, &["--json", "explain", "notes/spaced-repetition"]);
    assert!(json.status.success(), "{}", stderr(&json));
    let v: Value = serde_json::from_slice(&json.stdout).unwrap();
    assert_eq!(v["path"], "notes/spaced-repetition.md");
    assert_eq!(v["title"], "Spaced repetition");
    let conns = v["connections"].as_array().expect("connections array");
    assert_eq!(conns.len(), 2);
    assert!(conns.iter().all(|c| c["direction"] == "outbound"));
    assert!(conns.iter().all(|c| c["origin"] == "inline"));
    assert!(conns.iter().any(|c| c["label"] == "elaborates"));
}

#[test]
fn explain_reports_an_orphan() {
    let (_g, root) = reindexed();
    // A note nothing links to and that links to nothing.
    let out = run_in(&root, &["add", "islands/lonely", "--content", "By itself."]);
    assert!(out.status.success(), "{}", stderr(&out));

    let explain = run_in(&root, &["explain", "islands/lonely"]);
    assert!(explain.status.success(), "{}", stderr(&explain));
    let text = stdout(&explain).to_lowercase();
    assert!(
        text.contains("no connections"),
        "an isolated note reports no connections: {text}"
    );
}

#[test]
fn explain_unknown_note_fails_cleanly() {
    let (_g, root) = reindexed();
    let out = run_in(&root, &["explain", "does/not/exist"]);
    assert!(!out.status.success(), "unknown note must be a nonzero exit");
    let err = stderr(&out).to_lowercase();
    assert!(err.contains("not found"), "stderr: {err}");
    assert!(!err.contains("panicked"), "stderr: {err}");
}

#[test]
fn neighbors_by_path_and_by_b2id() {
    let (_g, root) = reindexed();

    let by_path = run_in(&root, &["neighbors", "notes/spaced-repetition"]);
    assert!(by_path.status.success(), "{}", stderr(&by_path));
    let out = stdout(&by_path);
    // outbound edges: the verbs themselves, resolved to the target's title.
    assert!(out.contains("elaborates"), "{out}");
    assert!(out.contains("references"), "{out}");
    assert!(out.contains("Human memory"), "{out}");

    let by_id = run_in(&root, &["neighbors", MEMORY_ID]);
    assert!(by_id.status.success(), "{}", stderr(&by_id));
    let out = stdout(&by_id);
    // memory sees the inbound inverse labels, from the SRS note.
    assert!(out.contains("elaborated-by"), "{out}");
    assert!(out.contains("referenced-by"), "{out}");
    assert!(out.contains("Spaced repetition"), "{out}");
}

#[test]
fn neighbors_json_shape() {
    let (_g, root) = reindexed();
    let out = run_in(&root, &["--json", "neighbors", MEMORY_ID]);
    assert!(out.status.success(), "{}", stderr(&out));
    let v: Value = serde_json::from_slice(&out.stdout).unwrap();
    let arr = v.as_array().expect("neighbors --json is an array");
    assert_eq!(arr.len(), 2);
    assert!(arr.iter().all(|n| n["direction"] == "inbound"));
    assert!(arr
        .iter()
        .all(|n| n["path"] == "notes/spaced-repetition.md"));
    assert!(arr.iter().any(|n| n["label"] == "elaborated-by"));
}

#[test]
fn neighbors_unknown_note_fails_cleanly() {
    let (_g, root) = reindexed();
    let out = run_in(&root, &["neighbors", "does/not/exist"]);
    assert!(!out.status.success(), "unknown note must be a nonzero exit");
    // actionable, on stderr, no panic / no stack trace.
    let err = stderr(&out);
    assert!(err.to_lowercase().contains("not found"), "stderr: {err}");
    assert!(!err.contains("panicked"), "stderr: {err}");
}

#[test]
fn search_finds_note_human_and_json() {
    let (_g, root) = reindexed();

    let human = run_in(&root, &["search", "forgetting"]);
    assert!(human.status.success(), "{}", stderr(&human));
    assert!(
        stdout(&human).contains("Spaced repetition"),
        "{:?}",
        stdout(&human)
    );
    // honesty caveat lives on stderr (human mode only).
    assert!(
        stderr(&human).to_lowercase().contains("semantic"),
        "expected a semantic-ranking caveat on stderr: {:?}",
        stderr(&human)
    );

    let json = run_in(&root, &["--json", "search", "forgetting"]);
    assert!(json.status.success(), "{}", stderr(&json));
    let v: Value = serde_json::from_slice(&json.stdout).unwrap();
    let arr = v.as_array().expect("search --json is an array");
    assert!(!arr.is_empty());
    assert!(arr
        .iter()
        .any(|h| h["path"] == "notes/spaced-repetition.md"));
    // --json stdout is pure data: no caveat leaks into it.
    assert!(!stdout(&json).to_lowercase().contains("semantic"));
}

#[test]
fn search_respects_limit() {
    let (_g, root) = reindexed();
    let out = run_in(&root, &["--json", "search", "memory", "--limit", "1"]);
    assert!(out.status.success(), "{}", stderr(&out));
    let v: Value = serde_json::from_slice(&out.stdout).unwrap();
    assert!(v.as_array().unwrap().len() <= 1);
}

#[test]
fn search_before_reindex_is_empty_but_succeeds() {
    let (_g, root) = golden_vault();
    // never reindexed → no hits, but a clean exit (not an error).
    let out = run_in(&root, &["--json", "search", "forgetting"]);
    assert!(out.status.success(), "{}", stderr(&out));
    let v: Value = serde_json::from_slice(&out.stdout).unwrap();
    assert!(v.as_array().unwrap().is_empty());
}

// --- suggestion lifecycle (③): suggest / accept / reject ---------------------

/// A reindexed vault of mutually **unconnected** notes, so connection discovery has
/// candidates to surface (the golden vault's two notes are directly linked → none).
/// With the fake embedder the KNN pool is the whole vault, so every other note is a
/// candidate and the `FakeRelator` fires on most — a reliably non-empty queue.
fn suggestable_vault() -> (tempfile::TempDir, PathBuf) {
    let tmp = tempfile::TempDir::new().unwrap();
    let root = tmp.path().join("vault");
    std::fs::create_dir_all(&root).unwrap();
    for name in ["alpha", "beta", "gamma", "delta", "epsilon"] {
        std::fs::write(
            root.join(format!("{name}.md")),
            format!("---\ntype: note\ntitle: {name}\n---\nA short note about {name}.\n"),
        )
        .unwrap();
    }
    let out = run_in(&root, &["reindex"]);
    assert!(out.status.success(), "reindex: {}", stderr(&out));
    (tmp, root)
}

/// The pending queue as JSON (re-runs generation, which is idempotent).
fn queue(root: &Path) -> Vec<Value> {
    let out = run_in(root, &["--json", "suggest"]);
    assert!(out.status.success(), "suggest: {}", stderr(&out));
    serde_json::from_slice::<Value>(&out.stdout)
        .unwrap()
        .as_array()
        .unwrap()
        .clone()
}

#[test]
fn suggest_generates_and_lists_json_and_human() {
    let (_g, root) = suggestable_vault();

    // JSON: a non-empty array of fully-resolved suggestions with a usable handle.
    let q = queue(&root);
    assert!(!q.is_empty(), "unconnected notes must yield candidates");
    for s in &q {
        assert!(s["edge_id"].as_str().is_some_and(|v| !v.is_empty()));
        assert!(s["src_path"].as_str().is_some());
        assert!(s["dst_path"].as_str().is_some());
        assert!(s["relation"].as_str().is_some());
        assert_eq!(s["by"], "agent:fake-relator-v1");
    }

    // Human: each suggestion prints its id handle; the stub-relator caveat and the
    // generation summary live on stderr (stdout stays pure results).
    let human = run_in(&root, &["suggest"]);
    assert!(human.status.success(), "{}", stderr(&human));
    let first_id = q[0]["edge_id"].as_str().unwrap();
    assert!(
        stdout(&human).contains(first_id),
        "human list should show the id handle: {:?}",
        stdout(&human)
    );
    let err = stderr(&human).to_lowercase();
    assert!(
        err.contains("stub relator"),
        "expected honesty caveat: {err}"
    );
    assert!(
        err.contains("generated"),
        "expected a generation summary: {err}"
    );
}

#[test]
fn suggest_before_reindex_is_empty_but_succeeds() {
    let (_g, root) = golden_vault();
    // never reindexed → no vector space → no candidates, but a clean exit.
    let out = run_in(&root, &["--json", "suggest"]);
    assert!(out.status.success(), "{}", stderr(&out));
    let v: Value = serde_json::from_slice(&out.stdout).unwrap();
    assert!(v.as_array().unwrap().is_empty());
}

#[test]
fn accept_writes_the_link_and_removes_it_from_the_queue() {
    let (_g, root) = suggestable_vault();
    let q = queue(&root);
    let s = &q[0];
    let id = s["edge_id"].as_str().unwrap();
    let src_path = s["src_path"].as_str().unwrap();

    // The source note carries no authored relations yet.
    let src_file = root.join(src_path);
    assert!(!std::fs::read_to_string(&src_file)
        .unwrap()
        .contains("relations:"));

    let out = run_in(&root, &["accept", id]);
    assert!(out.status.success(), "{}", stderr(&out));
    assert!(stdout(&out).to_lowercase().contains("accepted"));

    // Markdown-first: the typed link now lives in the note's frontmatter…
    assert!(
        std::fs::read_to_string(&src_file)
            .unwrap()
            .contains("relations:"),
        "accept must append the link to the source note's frontmatter"
    );
    // …and the accepted suggestion has left the pending queue (it is now active).
    assert!(
        queue(&root).iter().all(|s| s["edge_id"] != id),
        "an accepted suggestion must not remain pending"
    );
}

#[test]
fn reject_tombstones_and_removes_it_from_the_queue() {
    let (_g, root) = suggestable_vault();
    let id = {
        let q = queue(&root);
        q[0]["edge_id"].as_str().unwrap().to_string()
    };

    let out = run_in(&root, &["reject", &id]);
    assert!(out.status.success(), "{}", stderr(&out));
    assert!(stdout(&out).to_lowercase().contains("rejected"));

    // Tombstoned → gone from the queue and never re-proposed on a later `suggest`.
    assert!(
        queue(&root).iter().all(|s| s["edge_id"] != id.as_str()),
        "a rejected pair must not be re-proposed"
    );
}

#[test]
fn accept_reject_json_shape() {
    let (_g, root) = suggestable_vault();
    let q = queue(&root);
    // reject the first, accept the second, both in --json mode.
    let reject_id = q[0]["edge_id"].as_str().unwrap();
    let accept_id = q[1]["edge_id"].as_str().unwrap();

    let r = run_in(&root, &["--json", "reject", reject_id]);
    assert!(r.status.success(), "{}", stderr(&r));
    let rv: Value = serde_json::from_slice(&r.stdout).unwrap();
    assert_eq!(rv["rejected"], true);
    assert_eq!(rv["edge_id"], reject_id);

    let a = run_in(&root, &["--json", "accept", accept_id]);
    assert!(a.status.success(), "{}", stderr(&a));
    let av: Value = serde_json::from_slice(&a.stdout).unwrap();
    assert_eq!(av["accepted"], true);
    assert_eq!(av["edge_id"], accept_id);
}

#[test]
fn accept_unknown_id_fails_cleanly() {
    let (_g, root) = suggestable_vault();
    let out = run_in(&root, &["accept", "01JNOTASUGGESTION00000000000"]);
    assert!(!out.status.success(), "unknown id must be a nonzero exit");
    let err = stderr(&out).to_lowercase();
    assert!(err.contains("suggestion"), "actionable message: {err}");
    assert!(!err.contains("panicked"), "no stack trace: {err}");
}
