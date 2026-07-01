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
fn reindex_accepts_a_positional_vault() {
    let (_g, root) = golden_vault();
    // the spec's `b2 reindex [vault]` form, no -C.
    let out = run(&["reindex", root.to_str().unwrap()]);
    assert!(out.status.success(), "{}", stderr(&out));
    assert!(stdout(&out).contains("Indexed 2"));
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
