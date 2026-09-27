//! CLI tests: run the built `b2` binary against a temp copy of the golden vault and
//! assert its output. They prove the wiring and output shape, not engine behavior.

use serde_json::Value;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

/// The golden note the graph assertions hang off (L1: a note is its path).
const MEMORY_PATH: &str = "concepts/memory.md";

/// A temp copy of the golden vault; keep the returned guard alive for the test.
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

/// Run `b2 <args...>` under the fake embedder, so CI never runs the real model.
fn run(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_b2"))
        .env("B2_EMBEDDER", "fake")
        .args(args)
        .output()
        .expect("b2 binary runs")
}

/// Run `b2 -C <vault> <args...>`.
fn run_in(vault: &Path, args: &[&str]) -> Output {
    let mut full = vec!["-C", vault.to_str().unwrap()];
    full.extend_from_slice(args);
    run(&full)
}

/// Run `b2 <args...>` with `B2_VAULT_PATH` set and no `-C`.
fn run_with_vault_env(vault: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_b2"))
        .env("B2_EMBEDDER", "fake")
        .env("B2_VAULT_PATH", vault)
        .args(args)
        .output()
        .expect("b2 binary runs")
}

/// Run `b2 -C <vault> <args...>` with `input` on stdin (how `b2 write` takes a body).
fn run_in_stdin(vault: &Path, args: &[&str], input: &str) -> Output {
    use std::io::Write as _;
    use std::process::Stdio;
    let mut full = vec!["-C", vault.to_str().unwrap()];
    full.extend_from_slice(args);
    let mut child = Command::new(env!("CARGO_BIN_EXE_b2"))
        .env("B2_EMBEDDER", "fake")
        .args(&full)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("b2 binary spawns");
    child
        .stdin
        .take()
        .expect("child stdin is piped")
        .write_all(input.as_bytes())
        .expect("write to child stdin");
    child.wait_with_output().expect("b2 binary runs")
}

fn stdout(o: &Output) -> String {
    String::from_utf8(o.stdout.clone()).unwrap()
}
fn stderr(o: &Output) -> String {
    String::from_utf8(o.stderr.clone()).unwrap()
}

/// The rows of a `search --json` payload, an object since GH #202 (it also carries D2's
/// query-level verdict).
fn results_of(v: &Value) -> &[Value] {
    v["results"]
        .as_array()
        .unwrap_or_else(|| panic!("search --json is an object with a `results` array: {v}"))
}

/// A reindexed golden vault, ready for querying.
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
    assert!(
        v.get("stamped").is_none(),
        "a reindex writes nothing to the vault, so it reports no stamps (GH #170)"
    );

    assert!(root.join(".b2/b2.sqlite").is_file());
}

#[test]
fn reindex_is_incremental_and_force_reembeds() {
    let (_g, root) = golden_vault();

    let first = run_in(&root, &["--json", "reindex"]);
    assert!(first.status.success(), "{}", stderr(&first));
    let v: Value = serde_json::from_slice(&first.stdout).unwrap();
    assert_eq!(v["indexed"], 2);
    assert_eq!(v["embedded"], 2);

    let again = run_in(&root, &["--json", "reindex"]);
    let v: Value = serde_json::from_slice(&again.stdout).unwrap();
    assert_eq!(v["embedded"], 0, "unchanged notes are not re-embedded");

    // --force re-chunks every note, but unchanged text hashes to vectors already
    // stored (M4), so nothing is re-embedded.
    let forced = run_in(&root, &["--json", "reindex", "--force"]);
    let v: Value = serde_json::from_slice(&forced.stdout).unwrap();
    assert_eq!(v["indexed"], 2, "--force re-projects everything");
    assert_eq!(
        v["embedded"], 0,
        "identical text needs no second forward pass"
    );
}

#[test]
fn reindex_dry_run_previews_and_writes_nothing() {
    let (_g, root) = golden_vault();
    std::fs::write(
        root.join("fresh.md"),
        "---\ntype: note\ntitle: Fresh\n---\nA third note.\n",
    )
    .unwrap();
    let before = std::fs::read_to_string(root.join("fresh.md")).unwrap();

    let json = run_in(&root, &["--json", "reindex", "--dry-run"]);
    assert!(json.status.success(), "{}", stderr(&json));
    let v: Value = serde_json::from_slice(&json.stdout).unwrap();
    assert_eq!(v["would_index"], 3);
    assert_eq!(v["would_embed"], 3);
    assert!(v.get("indexed").is_none(), "not the real-reindex shape");

    let human = run_in(&root, &["reindex", "--dry-run"]);
    assert!(human.status.success(), "{}", stderr(&human));
    assert!(stdout(&human).contains("Dry run"), "{:?}", stdout(&human));

    // The work is still pending: a real reindex now embeds all 3.
    assert_eq!(
        std::fs::read_to_string(root.join("fresh.md")).unwrap(),
        before
    );
    let real = run_in(&root, &["--json", "reindex"]);
    let v: Value = serde_json::from_slice(&real.stdout).unwrap();
    assert_eq!(v["indexed"], 3);
    assert_eq!(v["embedded"], 3, "the dry-run did no embedding work");
}

/// Hold a vault's reindex lock the way a running `b2 reindex` does (lock, then stamp a
/// pid). Dropping the returned `File` releases the lock, ending the "run".
fn hold_reindex_lock(vault: &Path, pid: u32) -> std::fs::File {
    use std::io::Write as _;
    let dir = vault.join(".b2");
    std::fs::create_dir_all(&dir).unwrap();
    let lock = std::fs::OpenOptions::new()
        .create(true)
        .truncate(false)
        .read(true)
        .write(true)
        .open(dir.join("reindex.lock"))
        .unwrap();
    lock.try_lock()
        .expect("nothing else holds the fixture's lock");
    lock.set_len(0).unwrap();
    writeln!(&lock, "{pid}").unwrap();
    lock
}

#[test]
fn reindex_records_its_pid_in_the_lock() {
    let (_g, root) = golden_vault();
    // Spawned rather than `run_in`, so the test knows which pid to expect (GH #55).
    let child = Command::new(env!("CARGO_BIN_EXE_b2"))
        .env("B2_EMBEDDER", "fake")
        .args(["-C", root.to_str().unwrap(), "reindex"])
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .expect("b2 binary spawns");
    let pid = child.id();
    let out = child.wait_with_output().expect("b2 binary runs");
    assert!(out.status.success(), "{}", stderr(&out));

    let recorded = std::fs::read_to_string(root.join(".b2/reindex.lock")).unwrap();
    assert_eq!(
        recorded.trim().parse::<u32>().ok(),
        Some(pid),
        "the run stamps its own pid into the lock it holds, got: {recorded:?}"
    );
}

#[test]
fn status_reports_the_running_reindex_and_its_pid() {
    let (_g, root) = reindexed();

    // The finished run left its pid in the file, but the lock, not the contents, says
    // whether a reindex is running.
    let idle = run_in(&root, &["--json", "status"]);
    assert!(idle.status.success(), "{}", stderr(&idle));
    let v: Value = serde_json::from_slice(&idle.stdout).unwrap();
    assert_eq!(v["reindex_running"], false);
    assert!(v["reindex_pid"].is_null(), "no run, no pid: {v}");

    // A synthetic pid: `status` only prints it.
    let lock = hold_reindex_lock(&root, 4242);
    let busy = run_in(&root, &["--json", "status"]);
    assert!(busy.status.success(), "{}", stderr(&busy));
    let v: Value = serde_json::from_slice(&busy.stdout).unwrap();
    assert_eq!(v["reindex_running"], true);
    assert_eq!(v["reindex_pid"], 4242);

    let human = run_in(&root, &["status"]);
    assert!(
        stdout(&human).contains("pid 4242"),
        "the pid keeps a manual `kill` available: {:?}",
        stdout(&human)
    );
    drop(lock);
}

#[test]
fn cancel_signals_the_process_the_lock_names() {
    use std::os::unix::process::ExitStatusExt as _;
    /// What Ctrl-C raises, so `--cancel` shares its path.
    const SIGINT: i32 = 2;

    let (_g, root) = reindexed();
    // A stand-in for a backgrounded run: `--cancel` owns delivering SIGINT to the pid
    // the lock names, not what the reindex does with it.
    let mut victim = Command::new("sleep")
        .arg("30")
        .spawn()
        .expect("sleep spawns");
    let lock = hold_reindex_lock(&root, victim.id());

    let out = run_in(&root, &["reindex", "--cancel"]);
    assert!(out.status.success(), "{}", stderr(&out));
    assert!(
        stdout(&out).contains(&format!("pid {}", victim.id())),
        "{:?}",
        stdout(&out)
    );

    let status = victim.wait().expect("the signalled child is reaped");
    assert_eq!(
        status.signal(),
        Some(SIGINT),
        "--cancel must deliver the same signal Ctrl-C does"
    );
    drop(lock);
}

#[test]
fn cancel_reports_json_for_agents() {
    let (_g, root) = reindexed();
    let mut victim = Command::new("sleep")
        .arg("30")
        .spawn()
        .expect("sleep spawns");
    let lock = hold_reindex_lock(&root, victim.id());

    let out = run_in(&root, &["--json", "reindex", "--cancel"]);
    assert!(out.status.success(), "{}", stderr(&out));
    let v: Value = serde_json::from_slice(&out.stdout).unwrap();
    // Honest tense: the request landed; the run stops at its next batch boundary.
    assert_eq!(v["signalled"], true);
    assert_eq!(v["pid"], victim.id());
    assert!(v.get("cancelled").is_none(), "not the reindex-report shape");

    let _ = victim.wait();
    drop(lock);
}

#[test]
fn cancel_with_no_run_in_flight_is_an_error() {
    // The lock file still holds the finished run's pid; a stale pid is not a live run.
    let (_g, root) = reindexed();
    let out = run_in(&root, &["reindex", "--cancel"]);
    assert!(
        !out.status.success(),
        "nothing to cancel is a non-zero exit"
    );
    assert!(
        stderr(&out).contains("No reindex is running"),
        "{:?}",
        stderr(&out)
    );

    // Never indexed: no lock file at all.
    let (_g2, fresh) = golden_vault();
    let out = run_in(&fresh, &["reindex", "--cancel"]);
    assert!(
        !out.status.success(),
        "nothing to cancel is a non-zero exit"
    );
    assert!(
        stderr(&out).contains("No reindex is running"),
        "{:?}",
        stderr(&out)
    );
}

#[test]
fn cancel_refuses_without_an_explicit_vault() {
    // `--cancel` keeps reindex's explicit-vault guard rather than falling back to cwd.
    let out = Command::new(env!("CARGO_BIN_EXE_b2"))
        .env("B2_EMBEDDER", "fake")
        .env_remove("B2_VAULT_PATH")
        .args(["reindex", "--cancel"])
        .output()
        .expect("b2 binary runs");
    assert!(!out.status.success(), "no vault must exit non-zero");
    assert!(
        stderr(&out).contains("No vault specified"),
        "{:?}",
        stderr(&out)
    );
}

#[test]
fn cancel_conflicts_with_the_flags_that_would_run_a_reindex() {
    let (_g, root) = golden_vault();
    for args in [
        ["reindex", "--cancel", "--force"],
        ["reindex", "--cancel", "--dry-run"],
    ] {
        let out = run_in(&root, &args);
        assert!(
            !out.status.success(),
            "`b2 {}` must be rejected",
            args.join(" ")
        );
    }
    // `--cancel` signals; it never indexes.
    assert!(!root.join(".b2/b2.sqlite").exists());
}

#[test]
fn write_commands_refuse_without_an_explicit_vault() {
    // Writing commands never fall back to the cwd; reads keep that default and are out
    // of scope. env_remove guards against a B2_VAULT_PATH leaking in from the shell.
    let write_cmds: &[&[&str]] = &[
        &["reindex"],
        &["add", "notes/new"],
        &["write", "notes/a"],
        &["mv", "notes/a", "notes/b"],
        &["rm", "notes/a"],
        &["link", "notes/a", "notes/b"],
    ];
    for args in write_cmds {
        let out = Command::new(env!("CARGO_BIN_EXE_b2"))
            .env("B2_EMBEDDER", "fake")
            .env_remove("B2_VAULT_PATH")
            .args(*args)
            .output()
            .expect("b2 binary runs");
        assert!(
            !out.status.success(),
            "`b2 {}` with no vault must exit non-zero",
            args.join(" ")
        );
        assert!(
            stderr(&out).contains("No vault specified"),
            "`b2 {}`: expected the no-vault message, got: {:?}",
            args.join(" "),
            stderr(&out)
        );
    }
}

#[test]
fn reindex_accepts_a_positional_vault() {
    let (_g, root) = golden_vault();
    let out = run(&["reindex", root.to_str().unwrap()]);
    assert!(out.status.success(), "{}", stderr(&out));
    assert!(stdout(&out).contains("Indexed 2"));
}

#[test]
fn b2_vault_path_env_var_points_at_the_vault() {
    let (_g, root) = golden_vault();
    let out = run_with_vault_env(&root, &["reindex"]);
    assert!(out.status.success(), "{}", stderr(&out));
    assert!(stdout(&out).contains("Indexed 2"), "{:?}", stdout(&out));
    assert!(root.join(".b2/b2.sqlite").is_file());
}

#[test]
fn explicit_flag_overrides_b2_vault_path_env_var() {
    let (_g_env, env_root) = golden_vault();
    let (_g_flag, flag_root) = golden_vault();

    let out = Command::new(env!("CARGO_BIN_EXE_b2"))
        .env("B2_EMBEDDER", "fake")
        .env("B2_VAULT_PATH", &env_root)
        .args(["-C", flag_root.to_str().unwrap(), "reindex"])
        .output()
        .expect("b2 binary runs");
    assert!(out.status.success(), "{}", stderr(&out));

    assert!(
        flag_root.join(".b2/b2.sqlite").is_file(),
        "the -C vault should be indexed"
    );
    assert!(
        !env_root.join(".b2/b2.sqlite").exists(),
        "an explicit -C must override $B2_VAULT_PATH"
    );
}

// --- note CRUD: add -----------------------------------------------------------

#[test]
fn add_creates_a_note_human_and_json() {
    let (_g, root) = golden_vault();

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

    // Titled frontmatter and body, and no key of B2's (W1).
    let text = std::fs::read_to_string(root.join("notes/gadgets.md")).unwrap();
    assert!(!text.contains("b2id"), "nothing is stamped: {text}");
    assert!(text.contains(r#"title: "All about gadgets""#), "{text}");
    assert!(text.contains("Gadgets are handy little devices."), "{text}");

    // Immediately searchable (the keyword half is real under the fake embedder).
    let search = run_in(&root, &["--json", "search", "gadgets"]);
    let v: Value = serde_json::from_slice(&search.stdout).unwrap();
    assert!(results_of(&v)
        .iter()
        .any(|h| h["path"] == "notes/gadgets.md"));

    let json = run_in(&root, &["--json", "add", "notes/another"]);
    assert!(json.status.success(), "{}", stderr(&json));
    let v: Value = serde_json::from_slice(&json.stdout).unwrap();
    assert_eq!(v["path"], "notes/another.md");
    assert!(v.get("b2id").is_none(), "the path is the identity (L1)");
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

// --- note CRUD: write (body edit from stdin) ----------------------------------

#[test]
fn write_replaces_body_from_stdin_and_reprojects() {
    let (_g, root) = reindexed();

    // Addressed in the extensionless wikilink form, which the resolver accepts.
    let out = run_in_stdin(
        &root,
        &["write", "concepts/memory"],
        "Memory is now all about marmots and their burrows.\n",
    );
    assert!(out.status.success(), "{}", stderr(&out));
    assert!(
        stdout(&out).contains("Wrote concepts/memory.md"),
        "{:?}",
        stdout(&out)
    );

    let text = std::fs::read_to_string(root.join("concepts/memory.md")).unwrap();
    assert!(
        text.starts_with("---\ntype: concept\ntitle: \"Human memory\"\n"),
        "frontmatter preserved verbatim: {text}"
    );
    assert!(
        text.contains("marmots and their burrows"),
        "new body written: {text}"
    );
    assert!(
        !text.contains("encodes, stores, and retrieves"),
        "old body replaced: {text}"
    );

    // Re-projected without a reindex.
    let hit = run_in(&root, &["--json", "search", "marmots"]);
    let v: Value = serde_json::from_slice(&hit.stdout).unwrap();
    assert!(
        results_of(&v)
            .iter()
            .any(|h| h["path"] == "concepts/memory.md"),
        "the new body is indexed: {v}"
    );
}

#[test]
fn write_json_returns_path_and_new_revision() {
    let (_g, root) = reindexed();

    let out = run_in_stdin(&root, &["--json", "write", MEMORY_PATH], "Fresh body.\n");
    assert!(out.status.success(), "{}", stderr(&out));
    let v: Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(v["path"], "concepts/memory.md");
    // The revision a later save chains on (the desktop's guard token).
    assert!(v["revision"].as_str().is_some_and(|s| !s.is_empty()));
}

#[test]
fn write_unknown_note_fails_cleanly() {
    let (_g, root) = reindexed();
    let out = run_in_stdin(&root, &["write", "does/not/exist"], "body\n");
    assert!(!out.status.success(), "unknown note must be a nonzero exit");
    let err = stderr(&out).to_lowercase();
    assert!(err.contains("not found"), "actionable message: {err}");
    assert!(!err.contains("panicked"), "no stack trace: {err}");
}

// --- explain: a note's connections with their why -----------------------------

#[test]
fn explain_shows_connections_human_and_json() {
    let (_g, root) = reindexed();

    let human = run_in(&root, &["explain", "notes/spaced-repetition"]);
    assert!(human.status.success(), "{}", stderr(&human));
    let out = stdout(&human);
    assert!(out.contains("spaced-repetition"), "header: {out}");
    assert!(out.contains("supports"), "{out}");
    assert!(out.contains("why:"), "the explanation is labelled: {out}");
    assert!(out.contains("forgetting curve"), "{out}");

    let json = run_in(&root, &["--json", "explain", "notes/spaced-repetition"]);
    assert!(json.status.success(), "{}", stderr(&json));
    let v: Value = serde_json::from_slice(&json.stdout).unwrap();
    assert_eq!(v["path"], "notes/spaced-repetition.md");
    // Title is the filename (data-model.md §1), not frontmatter `title:`.
    assert_eq!(v["title"], "spaced-repetition");
    let conns = v["connections"].as_array().expect("connections array");
    assert_eq!(conns.len(), 2);
    assert!(conns.iter().all(|c| c["direction"] == "outbound"));
    // The bare body link and the typed `b2_relations:` entry.
    assert!(conns
        .iter()
        .any(|c| c["label"] == "references" && c["origin"] == "inline"));
    assert!(conns
        .iter()
        .any(|c| c["label"] == "supports" && c["origin"] == "frontmatter"));
}

#[test]
fn explain_reports_an_orphan() {
    let (_g, root) = reindexed();
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
fn neighbors_resolve_in_both_authored_path_forms() {
    let (_g, root) = reindexed();

    let by_path = run_in(&root, &["neighbors", "notes/spaced-repetition"]);
    assert!(by_path.status.success(), "{}", stderr(&by_path));
    let out = stdout(&by_path);
    assert!(out.contains("supports"), "{out}");
    assert!(out.contains("references"), "{out}");
    assert!(out.contains("memory"), "{out}");

    let by_full_path = run_in(&root, &["neighbors", MEMORY_PATH]);
    assert!(by_full_path.status.success(), "{}", stderr(&by_full_path));
    let out = stdout(&by_full_path);
    // Inbound inverse labels, from the SRS note.
    assert!(out.contains("supported-by"), "{out}");
    assert!(out.contains("referenced-by"), "{out}");
    assert!(out.contains("spaced-repetition"), "{out}");
}

#[test]
fn neighbors_json_shape() {
    let (_g, root) = reindexed();
    let out = run_in(&root, &["--json", "neighbors", MEMORY_PATH]);
    assert!(out.status.success(), "{}", stderr(&out));
    let v: Value = serde_json::from_slice(&out.stdout).unwrap();
    let arr = v.as_array().expect("neighbors --json is an array");
    assert_eq!(arr.len(), 2);
    assert!(arr.iter().all(|n| n["direction"] == "inbound"));
    assert!(arr
        .iter()
        .all(|n| n["path"] == "notes/spaced-repetition.md"));
    assert!(arr.iter().any(|n| n["label"] == "supported-by"));
}

#[test]
fn neighbors_unknown_note_fails_cleanly() {
    let (_g, root) = reindexed();
    let out = run_in(&root, &["neighbors", "does/not/exist"]);
    assert!(!out.status.success(), "unknown note must be a nonzero exit");
    let err = stderr(&out);
    assert!(err.to_lowercase().contains("not found"), "stderr: {err}");
    assert!(!err.contains("panicked"), "stderr: {err}");
}

/// A note whose only body link is a `[[folder]]`, which resolves to nothing (GH #12).
fn guide_with_dangling_link(root: &Path) {
    std::fs::write(
        root.join("guide.md"),
        "---\ntype: note\ntitle: Guide\n---\n\
         - [[Hermes]] is the R&D machine\n",
    )
    .unwrap();
    let re = run_in(root, &["reindex"]);
    assert!(re.status.success(), "reindex failed: {}", stderr(&re));
}

#[test]
fn neighbors_surfaces_unresolved_links_in_human_output() {
    let (_g, root) = reindexed();
    guide_with_dangling_link(&root);

    let human = run_in(&root, &["neighbors", "guide"]);
    assert!(human.status.success(), "{}", stderr(&human));
    let text = stdout(&human).to_lowercase();
    assert!(text.contains("unresolved"), "flags the broken link: {text}");
    assert!(text.contains("hermes"), "names the target: {text}");

    // `--json` stays an array of resolved neighbors; unresolved data lives in `explain`.
    let json = run_in(&root, &["--json", "neighbors", "guide"]);
    assert!(json.status.success(), "{}", stderr(&json));
    let v: Value = serde_json::from_slice(&json.stdout).unwrap();
    assert!(
        v.as_array()
            .expect("neighbors --json is an array")
            .is_empty(),
        "no resolved neighbors: {v}"
    );
}

#[test]
fn explain_surfaces_unresolved_links_human_and_json() {
    let (_g, root) = reindexed();
    guide_with_dangling_link(&root);

    let human = run_in(&root, &["explain", "guide"]);
    assert!(human.status.success(), "{}", stderr(&human));
    let text = stdout(&human).to_lowercase();
    assert!(text.contains("unresolved"), "flags the broken link: {text}");
    assert!(text.contains("hermes"), "names the target: {text}");

    let json = run_in(&root, &["--json", "explain", "guide"]);
    assert!(json.status.success(), "{}", stderr(&json));
    let v: Value = serde_json::from_slice(&json.stdout).unwrap();
    let un = v["unresolved"].as_array().expect("unresolved array");
    assert_eq!(un.len(), 1, "one dangling link: {v}");
    assert_eq!(un[0]["target"], "Hermes");
    assert_eq!(un[0]["relation"], "references");
    assert_eq!(un[0]["origin"], "inline");
}

#[test]
fn search_finds_note_human_and_json() {
    let (_g, root) = reindexed();

    let human = run_in(&root, &["search", "forgetting"]);
    assert!(human.status.success(), "{}", stderr(&human));
    assert!(
        stdout(&human).contains("spaced-repetition"),
        "{:?}",
        stdout(&human)
    );
    assert!(
        stderr(&human).to_lowercase().contains("semantic"),
        "expected a semantic-ranking caveat on stderr: {:?}",
        stderr(&human)
    );

    let json = run_in(&root, &["--json", "search", "forgetting"]);
    assert!(json.status.success(), "{}", stderr(&json));
    let v: Value = serde_json::from_slice(&json.stdout).unwrap();
    let arr = results_of(&v);
    assert!(!arr.is_empty());
    assert!(arr
        .iter()
        .any(|h| h["path"] == "notes/spaced-repetition.md"));
    // No calibrated bar for the fake embedder, so `null`, never a guessed `false` (M2).
    assert!(v.get("vouched").is_some(), "the verdict travels: {v}");
    assert!(v["vouched"].is_null(), "fake embedder has no bar: {v}");
    assert!(
        arr[0].get("bm25_rank").is_some() && arr[0].get("cos").is_some(),
        "provenance flattened onto the row: {v}"
    );
    assert!(!stdout(&json).to_lowercase().contains("semantic"));
}

#[test]
fn search_respects_limit() {
    let (_g, root) = reindexed();
    let out = run_in(&root, &["--json", "search", "memory", "--limit", "1"]);
    assert!(out.status.success(), "{}", stderr(&out));
    let v: Value = serde_json::from_slice(&out.stdout).unwrap();
    assert!(results_of(&v).len() <= 1);
}

/// `--exclude` lets an agent re-query minus the notes it already inspected.
#[test]
fn search_exclude_drops_a_served_note_and_serves_the_rest() {
    let (_g, root) = reindexed();

    let full = run_in(&root, &["--json", "search", "memory"]);
    let v: Value = serde_json::from_slice(&full.stdout).unwrap();
    let served: Vec<String> = results_of(&v)
        .iter()
        .map(|h| h["path"].as_str().unwrap().to_string())
        .collect();
    assert!(
        served.contains(&"concepts/memory.md".to_string()),
        "{served:?}"
    );

    let out = run_in(
        &root,
        &[
            "--json",
            "search",
            "memory",
            "--exclude",
            "concepts/memory.md",
        ],
    );
    assert!(out.status.success(), "{}", stderr(&out));
    let v: Value = serde_json::from_slice(&out.stdout).unwrap();
    let remaining: Vec<String> = results_of(&v)
        .iter()
        .map(|h| h["path"].as_str().unwrap().to_string())
        .collect();
    assert!(
        !remaining.contains(&"concepts/memory.md".to_string()),
        "an excluded path is never served: {remaining:?}"
    );
    assert_eq!(
        remaining,
        served
            .into_iter()
            .filter(|p| p != "concepts/memory.md")
            .collect::<Vec<_>>(),
        "the other rows stand, in order"
    );
}

#[test]
fn search_before_reindex_is_empty_but_succeeds() {
    let (_g, root) = golden_vault();
    let out = run_in(&root, &["--json", "search", "forgetting"]);
    assert!(out.status.success(), "{}", stderr(&out));
    let v: Value = serde_json::from_slice(&out.stdout).unwrap();
    assert!(results_of(&v).is_empty());
}

// --- connection discovery (③): similar + link ------------------------------

/// A reindexed vault of unconnected notes, so `b2 similar` has candidates (the golden
/// vault's two notes are linked). Under the fake embedder every other note is one.
fn discovery_vault() -> (tempfile::TempDir, PathBuf) {
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

/// `b2 similar` as JSON.
fn similar(root: &Path, note: &str) -> Vec<Value> {
    let out = run_in(root, &["--json", "similar", note]);
    assert!(out.status.success(), "similar: {}", stderr(&out));
    serde_json::from_slice::<Value>(&out.stdout)
        .unwrap()
        .as_array()
        .unwrap()
        .clone()
}

#[test]
fn similar_lists_candidates_json_and_human() {
    let (_g, root) = discovery_vault();

    let s = similar(&root, "alpha.md");
    assert!(!s.is_empty(), "unconnected notes must surface candidates");
    for c in &s {
        assert!(c["path"].as_str().is_some_and(|v| !v.is_empty()));
        assert!(c["score"].as_f64().is_some());
        assert_ne!(
            c["path"], "alpha.md",
            "the anchor never appears in its own list"
        );
    }

    let human = run_in(&root, &["similar", "alpha.md"]);
    assert!(human.status.success(), "{}", stderr(&human));
    assert!(
        !stdout(&human).contains("No similar"),
        "expected a list: {}",
        stdout(&human)
    );
}

#[test]
fn similar_excludes_already_linked() {
    let (_g, root) = discovery_vault();
    assert!(similar(&root, "alpha.md")
        .iter()
        .any(|c| c["path"] == "beta.md"));

    let out = run_in(
        &root,
        &["link", "alpha.md", "beta.md", "--type", "supports"],
    );
    assert!(out.status.success(), "{}", stderr(&out));
    assert!(
        similar(&root, "alpha.md")
            .iter()
            .all(|c| c["path"] != "beta.md"),
        "an already-linked note must not be surfaced as similar"
    );
}

#[test]
fn similar_unknown_note_fails_cleanly() {
    let (_g, root) = discovery_vault();
    let out = run_in(&root, &["similar", "nope.md"]);
    assert!(!out.status.success(), "unknown note must be a nonzero exit");
    let err = stderr(&out).to_lowercase();
    assert!(err.contains("not found"), "actionable message: {err}");
    assert!(!err.contains("panicked"), "no stack trace: {err}");
}

/// `--limit 0` prints nothing: the empty-state copy makes a claim about the candidate
/// set that a zero ask can't check (GH #197).
#[test]
fn similar_limit_zero_prints_nothing_rather_than_a_claim() {
    let (_g, root) = discovery_vault();
    let out = run_in(&root, &["similar", "alpha.md", "--limit", "0"]);
    assert!(out.status.success(), "{}", stderr(&out));
    assert_eq!(
        stdout(&out),
        "",
        "an empty ask yields empty output, never the empty-state copy"
    );
}

/// `b2 similar NOTE --explain OTHER` (GH #236) explains a card from the same computation
/// as the list, so it stands at the rank the list showed.
#[test]
fn similar_explain_describes_the_served_row() {
    let (_g, root) = discovery_vault();
    let list = similar(&root, "alpha.md");
    let second = list[1]["path"].as_str().unwrap().to_string();

    let out = run_in(
        &root,
        &["--json", "similar", "alpha.md", "--explain", &second],
    );
    assert!(out.status.success(), "{}", stderr(&out));
    let ex: Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(ex["anchor"]["path"], "alpha.md");
    assert_eq!(ex["candidate"]["path"], second.as_str());
    assert_eq!(ex["standing"]["kind"], "ranked");
    assert_eq!(ex["standing"]["rank"], 2, "the rank the card was shown at");
    assert_eq!(ex["standing"]["served"], true);
    assert_eq!(ex["limit"], 10);
    let pairs = ex["pairs"].as_array().unwrap();
    assert!(!pairs.is_empty(), "an embedded pair has passages");
    assert!(pairs[0]["anchor"]["text"]
        .as_str()
        .unwrap()
        .contains("alpha"));
    assert!(
        (pairs[0]["score"].as_f64().unwrap() - list[1]["score"].as_f64().unwrap()).abs() < 1e-9
    );

    let human = run_in(&root, &["similar", "alpha.md", "--explain", &second]);
    assert!(human.status.success(), "{}", stderr(&human));
    let text = stdout(&human);
    assert!(text.contains("#2"), "says where it stands: {text}");
    assert!(text.contains("alpha"), "shows this note's passage: {text}");
    // Five notes under the fake embedder: nothing to grade against.
    assert!(
        text.contains("Ungraded"),
        "an ungraded explanation says so: {text}"
    );
}

#[test]
fn similar_explain_says_why_a_linked_note_is_not_a_card() {
    let (_g, root) = discovery_vault();
    let out = run_in(
        &root,
        &["link", "alpha.md", "beta.md", "--type", "supports"],
    );
    assert!(out.status.success(), "{}", stderr(&out));

    let out = run_in(
        &root,
        &["--json", "similar", "alpha.md", "--explain", "beta.md"],
    );
    assert!(out.status.success(), "{}", stderr(&out));
    let ex: Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(ex["standing"]["kind"], "linked");

    let human = run_in(&root, &["similar", "alpha.md", "--explain", "beta.md"]);
    assert!(
        stdout(&human).to_lowercase().contains("already linked"),
        "{}",
        stdout(&human)
    );
}

#[test]
fn similar_explain_unknown_note_fails_cleanly() {
    let (_g, root) = discovery_vault();
    let out = run_in(&root, &["similar", "alpha.md", "--explain", "nope.md"]);
    assert!(!out.status.success(), "unknown note must be a nonzero exit");
    let err = stderr(&out).to_lowercase();
    assert!(err.contains("not found"), "actionable message: {err}");
    assert!(!err.contains("panicked"), "no stack trace: {err}");
}

#[test]
fn link_writes_frontmatter_and_shows_in_both_directions() {
    let (_g, root) = discovery_vault();
    let src = root.join("alpha.md");
    assert!(!std::fs::read_to_string(&src)
        .unwrap()
        .contains("b2_relations:"));

    let out = run_in(
        &root,
        &["link", "alpha.md", "beta.md", "--type", "supports"],
    );
    assert!(out.status.success(), "{}", stderr(&out));
    assert!(stdout(&out).to_lowercase().contains("linked"));

    let body = std::fs::read_to_string(&src).unwrap();
    assert!(
        body.contains("b2_relations:"),
        "link must append to frontmatter: {body}"
    );
    assert!(body.contains("supports"), "the verb is written: {body}");

    let a = run_in(&root, &["--json", "neighbors", "alpha.md"]);
    let av: Value = serde_json::from_slice(&a.stdout).unwrap();
    assert!(
        av.as_array()
            .unwrap()
            .iter()
            .any(|n| n["direction"] == "outbound" && n["path"] == "beta.md"),
        "alpha → beta must be outbound: {av}"
    );
    let b = run_in(&root, &["--json", "neighbors", "beta.md"]);
    let bv: Value = serde_json::from_slice(&b.stdout).unwrap();
    assert!(
        bv.as_array()
            .unwrap()
            .iter()
            .any(|n| n["direction"] == "inbound" && n["path"] == "alpha.md"),
        "beta must show alpha as an inbound backlink: {bv}"
    );
}

#[test]
fn link_defaults_to_references_and_reports_json() {
    let (_g, root) = discovery_vault();
    let out = run_in(&root, &["--json", "link", "alpha.md", "gamma.md"]);
    assert!(out.status.success(), "{}", stderr(&out));
    let v: Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(v["relation"], "references");
    assert_eq!(v["created"], true);
    assert_eq!(v["src_path"], "alpha.md");
    assert_eq!(v["dst_path"], "gamma");
}

#[test]
fn link_is_idempotent() {
    let (_g, root) = discovery_vault();
    run_in(
        &root,
        &["link", "alpha.md", "beta.md", "--type", "supports"],
    );
    let out = run_in(
        &root,
        &[
            "--json", "link", "alpha.md", "beta.md", "--type", "supports",
        ],
    );
    assert!(out.status.success(), "{}", stderr(&out));
    let v: Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(
        v["created"], false,
        "re-linking an existing edge changes nothing"
    );
}

#[test]
fn link_invalid_type_fails_cleanly() {
    let (_g, root) = discovery_vault();
    let out = run_in(
        &root,
        &["link", "alpha.md", "beta.md", "--type", "bogus-verb"],
    );
    assert!(
        !out.status.success(),
        "a non-core verb must be a nonzero exit"
    );
    let err = stderr(&out).to_lowercase();
    assert!(
        err.contains("relation"),
        "message should name the problem: {err}"
    );
    assert!(!err.contains("panicked"), "no stack trace: {err}");
}

// ---------------------------------------------------------------------------
// structured debug logging (B2_LOG)
// ---------------------------------------------------------------------------

/// Run `b2 -C <vault> <args...>` with `B2_LOG` set.
fn run_with_log(vault: &Path, log: &str, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_b2"))
        .env("B2_EMBEDDER", "fake")
        .env("B2_LOG", log)
        .arg("-C")
        .arg(vault)
        .args(args)
        .output()
        .expect("b2 binary runs")
}

#[test]
fn b2_log_emits_jsonl_on_stderr_and_stdout_stays_pure() {
    let (_g, root) = golden_vault();

    let out = run_with_log(&root, "debug", &["--json", "reindex"]);
    assert!(out.status.success(), "{}", stderr(&out));

    // stdout stays pure data.
    let report: Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(report["indexed"], 2);

    // stderr is JSON Lines: one flat object per line.
    let err = stderr(&out);
    assert!(!err.is_empty(), "B2_LOG=debug produced no log output");
    let mut sqlite_events = 0usize;
    for line in err.lines() {
        let v: Value = serde_json::from_str(line)
            .unwrap_or_else(|e| panic!("non-JSON stderr line ({e}): {line}"));
        if v["target"] == "b2::sqlite" {
            sqlite_events += 1;
            assert!(v["duration_us"].is_u64(), "no numeric timing: {line}");
        }
    }
    assert!(
        sqlite_events > 10,
        "expected per-query timing events, got {sqlite_events}"
    );

    // Without B2_LOG/B2_DEBUG, stderr is silent on success.
    let quiet = run_in(&root, &["--json", "reindex"]);
    assert!(quiet.status.success());
    assert_eq!(stderr(&quiet), "", "logging must stay opt-in");
}

#[test]
fn b2_log_file_captures_pure_jsonl_and_implies_debug() {
    let (_g, root) = golden_vault();
    let log_path = root.join("run-log.jsonl");

    // B2_LOG_FILE alone implies `debug`.
    let run = |args: &[&str]| {
        let mut full = vec!["-C", root.to_str().unwrap()];
        full.extend_from_slice(args);
        Command::new(env!("CARGO_BIN_EXE_b2"))
            .env("B2_EMBEDDER", "fake")
            .env("B2_LOG_FILE", &log_path)
            .args(&full)
            .output()
            .expect("b2 binary runs")
    };

    let first = run(&["reindex"]);
    assert!(first.status.success(), "{}", stderr(&first));
    // With a file sink, stderr carries no JSONL.
    assert!(
        !stderr(&first).contains("\"target\""),
        "log lines leaked to stderr: {}",
        stderr(&first)
    );

    let text = std::fs::read_to_string(&log_path).unwrap();
    let sqlite_events = text
        .lines()
        .map(|l| {
            serde_json::from_str::<Value>(l)
                .unwrap_or_else(|e| panic!("non-JSON log-file line ({e}): {l}"))
        })
        .filter(|v| v["target"] == "b2::sqlite")
        .count();
    assert!(sqlite_events > 10, "got {sqlite_events} sqlite events");

    let second = run(&["search", "memory"]);
    assert!(second.status.success(), "{}", stderr(&second));
    let grown = std::fs::read_to_string(&log_path).unwrap();
    assert!(
        grown.len() > text.len() && grown.starts_with(&text),
        "second run must append to the log file"
    );
}

// ---------------------------------------------------------------------------
// Resources slice 1 — explain/mv dispatch by argument shape (spec §5)
// ---------------------------------------------------------------------------

/// `b2 explain <resource>` renders the fallback card: metadata plus backlinks.
#[test]
fn explain_dispatches_to_the_resource_card() {
    let (_tmp, vault) = golden_vault();
    std::fs::write(
        vault.join("notes/card.md"),
        "---\ntitle: Card\n---\n![a tiny diagram](../resources/diagram.png)\n",
    )
    .unwrap();
    let r = run_in(&vault, &["reindex"]);
    assert!(r.status.success(), "{}", stderr(&r));

    let human = run_in(&vault, &["explain", "resources/diagram.png"]);
    assert!(human.status.success(), "{}", stderr(&human));
    let out = stdout(&human);
    assert!(
        out.contains("resources/diagram.png (image, 67 bytes)"),
        "{out}"
    );
    assert!(out.contains("Backlinks:"), "{out}");
    assert!(
        out.contains("card (notes/card.md)  references (embed) — \"a tiny diagram\""),
        "{out}"
    );

    let json = run_in(&vault, &["--json", "explain", "resources/diagram.png"]);
    assert!(json.status.success(), "{}", stderr(&json));
    let v: Value = serde_json::from_str(&stdout(&json)).unwrap();
    assert_eq!(v["class"], "image");
    assert_eq!(v["backlinks"][0]["caption"], "a tiny diagram");
    assert_eq!(v["backlinks"][0]["embed"], true);

    let missing = run_in(&vault, &["explain", "resources/nope.pdf"]);
    assert!(!missing.status.success());
    assert!(
        stderr(&missing).contains("File not found in the vault"),
        "{}",
        stderr(&missing)
    );
}

/// `b2 mv <resource> <to>` moves the file and rewrites inbound links.
#[test]
fn mv_dispatches_to_the_resource_move() {
    let (_tmp, vault) = golden_vault();
    std::fs::write(
        vault.join("notes/uses.md"),
        "---\ntitle: Uses\n---\n![d](../resources/diagram.png)\n",
    )
    .unwrap();
    let r = run_in(&vault, &["reindex"]);
    assert!(r.status.success(), "{}", stderr(&r));

    let mv = run_in(&vault, &["mv", "resources/diagram.png", "img/diagram.png"]);
    assert!(mv.status.success(), "{}", stderr(&mv));
    let out = stdout(&mv);
    assert!(
        out.contains("Moved resources/diagram.png → img/diagram.png"),
        "{out}"
    );
    assert!(
        out.contains("Rewrote 1 inbound link(s) across 1 file(s)."),
        "{out}"
    );
    assert!(vault.join("img/diagram.png").exists());
    let body = std::fs::read_to_string(vault.join("notes/uses.md")).unwrap();
    assert!(body.contains("![d](../img/diagram.png)"), "{body}");
}

#[test]
fn similar_on_a_resource_is_honest() {
    let (_tmp, vault) = golden_vault();
    let r = run_in(&vault, &["reindex"]);
    assert!(r.status.success(), "{}", stderr(&r));

    let sim = run_in(&vault, &["similar", "resources/diagram.png"]);
    assert!(!sim.status.success());
    assert!(
        stderr(&sim).contains("isn't available yet"),
        "{}",
        stderr(&sim)
    );
}

/// `b2 rm <note>` reports the notes whose links now dangle.
#[test]
fn rm_deletes_a_note_and_reports_dangled() {
    let (_g, root) = reindexed();

    let rm = run_in(&root, &["rm", "concepts/memory"]);
    assert!(rm.status.success(), "{}", stderr(&rm));
    let out = stdout(&rm);
    assert!(out.contains("Deleted concepts/memory.md"), "{out}");
    assert!(out.contains("Links in 1 file(s) now unresolved"), "{out}");
    assert!(!root.join("concepts/memory.md").exists());

    // The linker's body is not rewritten, so its links surface as unresolved.
    let explain = run_in(&root, &["--json", "explain", "notes/spaced-repetition"]);
    let v: Value = serde_json::from_slice(&explain.stdout).unwrap();
    assert_eq!(v["connections"].as_array().unwrap().len(), 0);
    assert_eq!(v["unresolved"].as_array().unwrap().len(), 2);
}

#[test]
fn rm_json_shape() {
    let (_g, root) = reindexed();

    let rm = run_in(&root, &["--json", "rm", MEMORY_PATH]);
    assert!(rm.status.success(), "{}", stderr(&rm));
    let v: Value = serde_json::from_slice(&rm.stdout).unwrap();
    assert_eq!(v["path"], MEMORY_PATH);
    assert!(v.get("b2id").is_none(), "the path is the identity (L1)");
    assert_eq!(
        v["dangled"],
        serde_json::json!(["notes/spaced-repetition.md"])
    );
}

/// `-r` is the CLI's stand-in for the desktop's confirm dialog.
#[test]
fn rm_folder_requires_recursive() {
    let (_g, root) = reindexed();

    let refused = run_in(&root, &["rm", "resources"]);
    assert!(!refused.status.success());
    assert!(
        stderr(&refused).contains("--recursive"),
        "{}",
        stderr(&refused)
    );
    assert!(
        root.join("resources").exists(),
        "nothing deleted on refusal"
    );

    let rm = run_in(&root, &["rm", "-r", "resources/"]);
    assert!(rm.status.success(), "{}", stderr(&rm));
    let out = stdout(&rm);
    assert!(
        out.contains("Deleted resources/ (0 note(s), 4 file(s))"),
        "{out}"
    );
    assert!(!root.join("resources").exists());
}

/// Dispatch is by extension only.
#[test]
fn rm_dispatches_to_the_resource_delete() {
    let (_g, root) = reindexed();

    let rm = run_in(&root, &["--json", "rm", "resources/data.txt"]);
    assert!(rm.status.success(), "{}", stderr(&rm));
    let v: Value = serde_json::from_slice(&rm.stdout).unwrap();
    assert_eq!(v["path"], "resources/data.txt");
    assert!(
        v.get("b2id").is_none(),
        "no report carries one any more (L1)"
    );
    assert!(!root.join("resources/data.txt").exists());
}

#[test]
fn rm_unknown_target_fails_cleanly() {
    let (_g, root) = reindexed();

    let note = run_in(&root, &["rm", "no/such-note"]);
    assert!(!note.status.success());
    assert!(
        stderr(&note).contains("Note not found"),
        "{}",
        stderr(&note)
    );

    let file = run_in(&root, &["rm", "resources/nope.png"]);
    assert!(!file.status.success());
    assert!(
        stderr(&file).contains("File not found in the vault"),
        "{}",
        stderr(&file)
    );
}

// ---------------------------------------------------------------------------
// flow ④: grounded chat — `ask` and `chat` (GH #154)
// ---------------------------------------------------------------------------
//
// These run under `B2_LLM=fake`, so they prove the adapter (streaming shape, JSONL,
// session history, error phrasing), never answer quality (crates/b2-llm/evals/).

/// Run `b2 -C <vault> <args...>` with both seams faked, optionally piping `input` to
/// stdin. `B2_LLM` is set, not inherited, so the developer's shell can't leak in.
fn run_chat(vault: &Path, args: &[&str], input: Option<&str>) -> Output {
    use std::io::Write as _;
    use std::process::Stdio;
    let mut full = vec!["-C", vault.to_str().unwrap()];
    full.extend_from_slice(args);
    let mut child = Command::new(env!("CARGO_BIN_EXE_b2"))
        .env("B2_EMBEDDER", "fake")
        .env("B2_LLM", "fake")
        .args(&full)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("b2 binary runs");
    if let Some(text) = input {
        child
            .stdin
            .as_mut()
            .expect("stdin is piped")
            .write_all(text.as_bytes())
            .expect("write stdin");
    }
    drop(child.stdin.take());
    child.wait_with_output().expect("b2 binary completes")
}

/// Parse an `--json` chat stream, one object per line.
fn events(out: &Output) -> Vec<Value> {
    stdout(out)
        .lines()
        .map(|l| {
            serde_json::from_str(l).unwrap_or_else(|e| panic!("non-JSON stream line ({e}): {l}"))
        })
        .collect()
}

/// `ask --json` streams token events, then one answer event. The tokens must reassemble
/// into exactly that answer, so live and last-line readers agree.
#[test]
fn ask_json_streams_tokens_then_the_resolved_answer() {
    let (_g, root) = reindexed();

    let out = run_chat(&root, &["--json", "ask", "what is memory?"], None);
    assert!(out.status.success(), "{}", stderr(&out));
    let events = events(&out);
    let (last, tokens) = events.split_last().expect("at least the answer event");

    assert!(!tokens.is_empty(), "the answer streamed as tokens");
    assert!(
        tokens.iter().all(|e| e["event"] == "token"),
        "every line before the answer is a token event: {tokens:?}"
    );
    assert_eq!(last["event"], "answer");

    let streamed: String = tokens
        .iter()
        .map(|e| e["text"].as_str().expect("token text"))
        .collect();
    assert_eq!(
        streamed,
        last["answer"].as_str().expect("answer text"),
        "the stream and the final view must agree"
    );
    assert_eq!(last["cancelled"], false);

    // Citations resolve to real vault paths.
    let citations = last["citations"].as_array().expect("citations array");
    assert!(!citations.is_empty(), "the fake cites every passage it got");
    for c in citations {
        let path = c["path"].as_str().expect("citation path");
        assert!(
            root.join(path).exists(),
            "citation names a real note: {path}"
        );
        assert!(c["marker"].is_u64(), "markers are the [n] in the answer");
    }
}

/// `why` streams like `ask`, with citations confined to the two notes.
#[test]
fn why_json_streams_an_explanation_cited_to_the_two_notes() {
    let (_g, root) = reindexed();
    let (anchor, candidate) = ("concepts/memory.md", "notes/spaced-repetition.md");

    let out = run_chat(&root, &["--json", "why", anchor, candidate], None);
    assert!(out.status.success(), "{}", stderr(&out));
    let events = events(&out);
    let (last, tokens) = events.split_last().expect("at least the answer event");
    assert!(tokens.iter().all(|e| e["event"] == "token"), "{tokens:?}");
    assert_eq!(last["event"], "answer");
    let streamed: String = tokens
        .iter()
        .map(|e| e["text"].as_str().expect("token text"))
        .collect();
    assert_eq!(streamed, last["answer"].as_str().expect("answer text"));

    let citations = last["citations"].as_array().expect("citations array");
    assert!(!citations.is_empty(), "the fake cites every passage it got");
    for c in citations {
        let path = c["path"].as_str().expect("citation path");
        assert!(path == anchor || path == candidate, "{path}");
    }

    // The answer says which B2 tools ran, the pair lookup among them.
    let tools = last["tools"].as_array().expect("tools array");
    assert!(
        tools.iter().any(|t| t["name"] == "b2_passage_pairs"),
        "{tools:?}"
    );
    assert!(tools.iter().all(|t| t["seeded"].is_boolean()));

    let out = run_chat(&root, &["why", anchor, candidate], None);
    assert!(stdout(&out).contains("B2 tools used:"), "{}", stdout(&out));

    let out = run_chat(&root, &["why", anchor, "nope.md"], None);
    assert!(!out.status.success());
    assert!(stderr(&out).contains("nope.md"), "{}", stderr(&out));
}

/// The fake-provider caveat goes to stderr, so `b2 ask … > answer.txt` captures only
/// the answer and its sources.
#[test]
fn ask_prints_the_answer_then_its_sources_and_keeps_stdout_clean() {
    let (_g, root) = reindexed();

    let out = run_chat(&root, &["ask", "what is memory?"], None);
    assert!(out.status.success(), "{}", stderr(&out));
    let text = stdout(&out);
    assert!(text.contains("[1]"), "the answer cites its passage: {text}");
    assert!(text.contains("Sources:"), "{text}");
    assert!(
        text.contains("concepts/memory.md") || text.contains("notes/spaced-repetition.md"),
        "a source names the note it came from: {text}"
    );
    assert!(
        !text.contains("note:"),
        "the fake-provider caveat belongs on stderr: {text}"
    );
    assert!(
        stderr(&out).contains("B2_LLM=fake"),
        "never overstate what answered: {}",
        stderr(&out)
    );
}

/// `chat` answers each piped turn and leaves on `/exit`. Later turns carry history,
/// which is what distinguishes a chat turn from a bare `ask`.
#[test]
fn chat_answers_every_turn_and_carries_the_conversation_forward() {
    let (_g, root) = reindexed();

    let out = Command::new(env!("CARGO_BIN_EXE_b2"))
        .env("B2_EMBEDDER", "fake")
        .env("B2_LLM", "fake")
        // The façade's span reports whether a turn had history.
        .env("B2_LOG", "b2::vault=debug")
        .args(["-C", root.to_str().unwrap(), "--json", "chat"])
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .and_then(|mut child| {
            use std::io::Write as _;
            child
                .stdin
                .as_mut()
                .expect("stdin is piped")
                .write_all(b"what is memory?\ntell me more\n/exit\n")?;
            drop(child.stdin.take());
            child.wait_with_output()
        })
        .expect("b2 binary runs");
    assert!(out.status.success(), "{}", stderr(&out));

    let answers: Vec<Value> = events(&out)
        .into_iter()
        .filter(|e| e["event"] == "answer")
        .collect();
    assert_eq!(answers.len(), 2, "one answer per piped turn");

    // Read from the `ask` span: the same fact the orchestration branched on.
    let multi_turn: Vec<bool> = stderr(&out)
        .lines()
        .filter_map(|l| serde_json::from_str::<Value>(l).ok())
        .filter(|v| v["span"]["name"] == "ask")
        .filter_map(|v| v["span"]["multi_turn"].as_bool())
        .collect();
    assert_eq!(
        multi_turn,
        [false, true],
        "history is session state the second turn retrieves against"
    );
}

/// End-of-input (Ctrl-D, or a finished script) ends the session like `/exit`.
#[test]
fn chat_ends_on_end_of_input() {
    let (_g, root) = reindexed();

    let out = run_chat(&root, &["--json", "chat"], Some("what is memory?\n"));
    assert!(out.status.success(), "{}", stderr(&out));
    assert_eq!(
        events(&out)
            .iter()
            .filter(|e| e["event"] == "answer")
            .count(),
        1
    );
}

/// Ask against an endpoint nothing is serving, from the `--llm-url` flag.
fn ask_dead_endpoint(root: &Path, url: &str) -> Output {
    Command::new(env!("CARGO_BIN_EXE_b2"))
        .env("B2_EMBEDDER", "fake")
        .env_remove("B2_LLM")
        .args([
            "-C",
            root.to_str().unwrap(),
            "ask",
            "what is memory?",
            "--llm-url",
            url,
        ])
        .output()
        .expect("b2 binary runs")
}

/// No model server is one actionable sentence naming the endpoint tried (E4); detail
/// stays behind `B2_DEBUG`.
#[test]
fn ask_without_a_model_server_says_so_and_names_the_endpoint() {
    let (_g, root) = reindexed();

    // Port 9 (discard) is never served: an immediate refusal.
    let out = ask_dead_endpoint(&root, "http://127.0.0.1:9/v1");
    assert!(!out.status.success(), "a stopped server is an error");
    let err = stderr(&out);
    assert!(
        err.contains("Can't reach the model server at http://127.0.0.1:9/v1"),
        "{err}"
    );
    assert!(
        err.contains("--llm-url"),
        "the fix names the knob that sets the endpoint: {err}"
    );
    // A non-Ollama endpoint gets no `ollama serve` advice.
    assert!(
        !err.to_lowercase().contains("ollama"),
        "advice about the wrong program: {err}"
    );
    assert!(
        !err.contains("os error"),
        "no transport internals without B2_DEBUG: {err}"
    );
    assert!(stdout(&out).is_empty(), "nothing was answered");
}

/// On Ollama's port, the fix is to start the daemon (E4, GH #154).
#[test]
fn an_unreachable_ollama_endpoint_names_ollamas_own_fix() {
    let (_g, root) = reindexed();

    // `.invalid` never resolves (RFC 2606), so a local Ollama can't answer instead.
    let out = ask_dead_endpoint(&root, "http://nothing.invalid:11434/v1");
    assert!(!out.status.success());
    let err = stderr(&out);
    assert!(err.contains("is Ollama running?"), "{err}");
    assert!(
        err.contains("ollama serve"),
        "the fix is in the message: {err}"
    );
}

/// An explicit flag beats the environment, as with `B2_VAULT_PATH`. Observable because
/// the failure names the endpoint tried.
#[test]
fn an_explicit_llm_url_beats_the_environment() {
    let (_g, root) = reindexed();

    let out = Command::new(env!("CARGO_BIN_EXE_b2"))
        .env("B2_EMBEDDER", "fake")
        .env_remove("B2_LLM")
        .env("B2_LLM_URL", "http://127.0.0.1:9/v1")
        .args([
            "-C",
            root.to_str().unwrap(),
            "ask",
            "what is memory?",
            "--llm-url",
            "http://127.0.0.1:10/v1",
        ])
        .output()
        .expect("b2 binary runs");
    assert!(!out.status.success());
    let err = stderr(&out);
    assert!(
        err.contains("http://127.0.0.1:10/v1"),
        "the flag's endpoint is the one tried: {err}"
    );
    assert!(
        !err.contains("http://127.0.0.1:9/v1"),
        "the environment's was overridden: {err}"
    );
}

#[test]
fn the_llm_url_environment_variable_is_used_when_no_flag_is_given() {
    let (_g, root) = reindexed();

    let out = Command::new(env!("CARGO_BIN_EXE_b2"))
        .env("B2_EMBEDDER", "fake")
        .env_remove("B2_LLM")
        .env("B2_LLM_URL", "http://127.0.0.1:9/v1")
        .args(["-C", root.to_str().unwrap(), "ask", "what is memory?"])
        .output()
        .expect("b2 binary runs");
    assert!(!out.status.success());
    assert!(
        stderr(&out).contains("http://127.0.0.1:9/v1"),
        "{}",
        stderr(&out)
    );
}
