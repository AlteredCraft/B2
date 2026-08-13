//! CLI-level tests: run the built `b2` binary against a temp copy of the
//! golden-vault fixture and assert its output — the "run a command against a
//! fixture, assert the output" surface invariants.md names. The binary path is
//! `CARGO_BIN_EXE_b2`, which cargo provides to integration tests (so no extra test
//! harness dependency is needed). The CLI is a dumb adapter over `b2_core::Vault`;
//! these prove the wiring + output shape, not engine behavior (that's the façade
//! and engine tests).

use serde_json::Value;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

/// The golden note the graph assertions hang off, by the thing that identifies it:
/// its vault-relative path (L1).
const MEMORY_PATH: &str = "concepts/memory.md";

/// A temp copy of the golden vault, so no test can mutate the repo fixtures. The
/// `TempDir` guard is returned so it outlives the test.
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
/// proves the wiring + output shape, not model quality (CLAUDE.md).
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

/// Run `b2 <args...>` with `B2_VAULT_PATH` set (and no `-C`) — the env-var path.
fn run_with_vault_env(vault: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_b2"))
        .env("B2_EMBEDDER", "fake")
        .env("B2_VAULT_PATH", vault)
        .args(args)
        .output()
        .expect("b2 binary runs")
}

/// Run `b2 -C <vault> <args...>` with `input` piped to the child's stdin — the path
/// `b2 write` reads its new body from (an agent, `cat file |`, …).
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
    assert!(
        v.get("stamped").is_none(),
        "a reindex writes nothing to the vault, so it reports no stamps (GH #170)"
    );

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

    // --force re-chunks every note; whether it re-*embeds* is content's to decide.
    // Unchanged text hashes to vectors already stored (M4), so a forced pass over an
    // untouched vault correctly reports no embedding work rather than recomputing
    // bytes it already has.
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

    // JSON: honest `would_*` keys, never the past-tense reindex shape.
    let json = run_in(&root, &["--json", "reindex", "--dry-run"]);
    assert!(json.status.success(), "{}", stderr(&json));
    let v: Value = serde_json::from_slice(&json.stdout).unwrap();
    assert_eq!(v["would_index"], 3);
    assert_eq!(v["would_embed"], 3);
    assert!(v.get("indexed").is_none(), "not the real-reindex shape");

    // Human: says it's a preview.
    let human = run_in(&root, &["reindex", "--dry-run"]);
    assert!(human.status.success(), "{}", stderr(&human));
    assert!(stdout(&human).contains("Dry run"), "{:?}", stdout(&human));

    // Nothing was indexed, and the work is still pending — a real reindex now
    // embeds all 3. (That a *real* reindex leaves the vault byte-identical too is
    // W1's business, asserted in the engine suite; here the point is the dry run
    // did no index work.)
    assert_eq!(
        std::fs::read_to_string(root.join("fresh.md")).unwrap(),
        before
    );
    let real = run_in(&root, &["--json", "reindex"]);
    let v: Value = serde_json::from_slice(&real.stdout).unwrap();
    assert_eq!(v["indexed"], 3);
    assert_eq!(v["embedded"], 3, "the dry-run did no embedding work");
}

/// Hold a vault's reindex lock exactly the way a running `b2 reindex` does — take the
/// advisory lock, then stamp a pid into it — so the cross-process readers (`b2 status`,
/// `b2 reindex --cancel`) see what a live run presents. The returned `File` must outlive
/// the assertions: dropping it releases the lock, i.e. ends the "run".
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
    // Spawned rather than `run_in`, so the test knows which pid to expect: the address
    // `--cancel` (and a manual `kill`) signals is the running process's own (GH #55).
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

    // Nothing in flight — even though the finished run above left its pid in the lock.
    // The *lock*, not the file's contents, answers "is a reindex running".
    let idle = run_in(&root, &["--json", "status"]);
    assert!(idle.status.success(), "{}", stderr(&idle));
    let v: Value = serde_json::from_slice(&idle.stdout).unwrap();
    assert_eq!(v["reindex_running"], false);
    assert!(v["reindex_pid"].is_null(), "no run, no pid: {v}");

    // A run in flight: reported, and named. (A synthetic pid — `status` only prints it.)
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
    /// SIGINT — what Ctrl-C raises, and so what `--cancel` must raise: one cancel path.
    const SIGINT: i32 = 2;

    let (_g, root) = reindexed();
    // A stand-in for a run backgrounded with `b2 reindex &`: a child that just sits
    // there until signalled. What `--cancel` owns is *delivering SIGINT to the pid the
    // lock names*; what the real reindex then does with it is the shipped Ctrl-C path.
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
    // Honest tense, like the dry-run's `would_*` keys: the request landed; the run
    // itself stops at its next batch boundary and reports the partial work.
    assert_eq!(v["signalled"], true);
    assert_eq!(v["pid"], victim.id());
    assert!(v.get("cancelled").is_none(), "not the reindex-report shape");

    let _ = victim.wait();
    drop(lock);
}

#[test]
fn cancel_with_no_run_in_flight_is_an_error() {
    // The reindex left its pid behind in the lock (nothing clears it), but the lock is
    // free — so there is nothing to cancel, and nothing to signal. A stale pid must
    // never be mistaken for a live run.
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

    // Same on a vault that has never been indexed at all (no lock file to read).
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
    // `--cancel` is a `reindex` invocation and keeps its guard: with no vault it must
    // refuse rather than fall back to the cwd and peek at some other vault's lock.
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
    // And nothing ran: `--cancel` signals, it never indexes.
    assert!(!root.join(".b2/b2.sqlite").exists());
}

#[test]
fn write_commands_refuse_without_an_explicit_vault() {
    // Every command that writes to the vault (builds the index, or creates/moves/edits
    // notes) must fail loudly when no vault is given — never silently touch the current
    // directory (the stale-binary / typo'd-env footgun that left a stray `.b2/`). Reads
    // (search/neighbors/explain/similar) keep the cwd default and are intentionally out.
    // env_remove guards against a B2_VAULT_PATH leaking in from the shell.
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
    // the spec's `b2 reindex [vault]` form, no -C.
    let out = run(&["reindex", root.to_str().unwrap()]);
    assert!(out.status.success(), "{}", stderr(&out));
    assert!(stdout(&out).contains("Indexed 2"));
}

#[test]
fn b2_vault_path_env_var_points_at_the_vault() {
    let (_g, root) = golden_vault();
    // No -C and not run from inside the vault: the vault comes from $B2_VAULT_PATH.
    let out = run_with_vault_env(&root, &["reindex"]);
    assert!(out.status.success(), "{}", stderr(&out));
    assert!(stdout(&out).contains("Indexed 2"), "{:?}", stdout(&out));
    assert!(root.join(".b2/b2.sqlite").is_file());
}

#[test]
fn explicit_flag_overrides_b2_vault_path_env_var() {
    // Two distinct vaults: one named by $B2_VAULT_PATH, one by -C. The flag must win.
    let (_g_env, env_root) = golden_vault();
    let (_g_flag, flag_root) = golden_vault();

    let out = Command::new(env!("CARGO_BIN_EXE_b2"))
        .env("B2_EMBEDDER", "fake")
        .env("B2_VAULT_PATH", &env_root)
        .args(["-C", flag_root.to_str().unwrap(), "reindex"])
        .output()
        .expect("b2 binary runs");
    assert!(out.status.success(), "{}", stderr(&out));

    // Only the -C vault got an index; the env-named vault was untouched.
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

    // Human: reports the created path.
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

    // The file exists with the titled frontmatter and the body — and nothing else:
    // projecting it added no key of B2's (W1).
    let text = std::fs::read_to_string(root.join("notes/gadgets.md")).unwrap();
    assert!(!text.contains("b2id"), "nothing is stamped: {text}");
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

    // Overwrite memory's body with piped Markdown, addressed by path (L1) — in the
    // extensionless wikilink form, which the resolver's `.md` ladder accepts.
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

    // Markdown-first + byte-honest: the frontmatter is untouched, the body is the
    // piped text verbatim, and the old body is gone.
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

    // Re-projected without a reindex: the new content is searchable, the old is not.
    let hit = run_in(&root, &["--json", "search", "marmots"]);
    let v: Value = serde_json::from_slice(&hit.stdout).unwrap();
    assert!(
        v.as_array()
            .unwrap()
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
    // The new revision is the blake3 of the final bytes — nonempty, and a *later*
    // save would chain on it (the desktop's guard token, surfaced for agents too).
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
    // Title is the filename (data-model.md §1), not the frontmatter `title:`.
    assert_eq!(v["title"], "spaced-repetition");
    let conns = v["connections"].as_array().expect("connections array");
    assert_eq!(conns.len(), 2);
    assert!(conns.iter().all(|c| c["direction"] == "outbound"));
    // The two homes: the bare body link vs the typed `b2_relations:` entry.
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
fn neighbors_resolve_in_both_authored_path_forms() {
    let (_g, root) = reindexed();

    let by_path = run_in(&root, &["neighbors", "notes/spaced-repetition"]);
    assert!(by_path.status.success(), "{}", stderr(&by_path));
    let out = stdout(&by_path);
    // outbound edges: the verbs themselves, resolved to the target's title (its
    // filename, data-model.md §1).
    assert!(out.contains("supports"), "{out}");
    assert!(out.contains("references"), "{out}");
    assert!(out.contains("memory"), "{out}");

    // The other end, addressed with the `.md` a Markdown link would write.
    let by_full_path = run_in(&root, &["neighbors", MEMORY_PATH]);
    assert!(by_full_path.status.success(), "{}", stderr(&by_full_path));
    let out = stdout(&by_full_path);
    // memory sees the inbound inverse labels, from the SRS note.
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
    // actionable, on stderr, no panic / no stack trace.
    let err = stderr(&out);
    assert!(err.to_lowercase().contains("not found"), "stderr: {err}");
    assert!(!err.contains("panicked"), "stderr: {err}");
}

/// A note whose only body link is a `[[folder]]` (GH #12): a note is one `.md` file,
/// so the folder resolves to nothing. `neighbors` and `explain` must surface it as
/// unresolved rather than silently drop it.
fn guide_with_dangling_link(root: &Path) {
    std::fs::write(
        root.join("guide.md"),
        "---\nb2id: 01JGUIDE00000000000000001\ntype: note\ntitle: Guide\n---\n\
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

    // Human output flags the broken link (it would otherwise vanish entirely).
    let human = run_in(&root, &["neighbors", "guide"]);
    assert!(human.status.success(), "{}", stderr(&human));
    let text = stdout(&human).to_lowercase();
    assert!(text.contains("unresolved"), "flags the broken link: {text}");
    assert!(text.contains("hermes"), "names the target: {text}");

    // `--json` keeps its resolved-neighbors array contract — the note has no resolved
    // neighbors, so it's empty (the structured unresolved data lives in `explain`).
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

// --- connection discovery (③): similar + link ------------------------------

/// A reindexed vault of mutually **unconnected** notes, so `b2 similar` has
/// candidates to surface (the golden vault's two notes are directly linked → none).
/// Under the fake embedder the KNN pool is the whole vault, so every other note is a
/// candidate — a reliably non-empty list.
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

/// `b2 similar` as JSON (a pure read over stored vectors; no model call).
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

    // JSON: a non-empty, fully-resolved list, never including the anchor itself.
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

    // Human: prints score/path lines, not the empty-state message.
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
    // beta is a candidate of alpha before any link.
    assert!(similar(&root, "alpha.md")
        .iter()
        .any(|c| c["path"] == "beta.md"));

    // link alpha → beta; beta is now a 1-hop neighbor and drops out of the list.
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

    // Markdown-first: the typed relation lands in the source note's frontmatter,
    // under the namespaced `b2_relations:` key…
    let body = std::fs::read_to_string(&src).unwrap();
    assert!(
        body.contains("b2_relations:"),
        "link must append to frontmatter: {body}"
    );
    assert!(body.contains("supports"), "the verb is written: {body}");

    // …and the graph shows it outbound from alpha and inbound (backlink) at beta.
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
    // a second identical link writes nothing.
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

/// Run `b2 -C <vault> <args...>` with `B2_LOG` set — the structured-logging path.
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

    // stdout is still pure machine-readable data — no log line leaks into it.
    let report: Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(report["indexed"], 2);

    // stderr is JSON Lines: every line one flat JSON object (the reporting
    // contract — pipeable into jq/DuckDB/pandas as-is).
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

    // Without B2_LOG/B2_DEBUG nothing is logged — stderr is silent on success.
    let quiet = run_in(&root, &["--json", "reindex"]);
    assert!(quiet.status.success());
    assert_eq!(stderr(&quiet), "", "logging must stay opt-in");
}

#[test]
fn b2_log_file_captures_pure_jsonl_and_implies_debug() {
    let (_g, root) = golden_vault();
    let log_path = root.join("run-log.jsonl");

    // B2_LOG_FILE alone (no B2_LOG) implies `debug` and routes the log to the file.
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
    // Human mode + file sink: stderr carries no JSONL (the file is the pure capture).
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

    // Append mode: a second run accumulates rather than truncates.
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

/// `b2 explain <resource>` renders the fallback card: metadata + backlinks,
/// and `--json` emits the ResourceExplainView verbatim.
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

    // Unknown resource path → generic, actionable message; nonzero exit.
    let missing = run_in(&vault, &["explain", "resources/nope.pdf"]);
    assert!(!missing.status.success());
    assert!(
        stderr(&missing).contains("File not found in the vault"),
        "{}",
        stderr(&missing)
    );
}

/// `b2 mv <resource> <to>` moves the file and rewrites inbound links; the
/// human line matches the note-move shape.
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

/// `b2 similar <resource>` says "not yet" — honest, actionable, nonzero exit.
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

/// `b2 rm <note>` deletes the file from the vault and the disk, reporting the
/// notes whose links now dangle — human and JSON shapes.
#[test]
fn rm_deletes_a_note_and_reports_dangled() {
    let (_g, root) = reindexed();

    let rm = run_in(&root, &["rm", "concepts/memory"]);
    assert!(rm.status.success(), "{}", stderr(&rm));
    let out = stdout(&rm);
    assert!(out.contains("Deleted concepts/memory.md"), "{out}");
    assert!(out.contains("Links in 1 file(s) now unresolved"), "{out}");
    assert!(!root.join("concepts/memory.md").exists());

    // The linker's body was never rewritten — its links now surface as unresolved.
    let explain = run_in(&root, &["--json", "explain", "notes/spaced-repetition"]);
    let v: Value = serde_json::from_slice(&explain.stdout).unwrap();
    assert_eq!(v["connections"].as_array().unwrap().len(), 0);
    assert_eq!(v["unresolved"].as_array().unwrap().len(), 2);
}

/// `b2 rm --json` returns the delete report (agents read `dangled` directly).
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

/// A folder delete refuses without -r/--recursive, and removes the whole
/// subtree with it — the CLI's stand-in for the desktop's confirm dialog.
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

/// `b2 rm <file>` dispatches to the resource delete (extension-only rule).
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

/// Unknown targets fail cleanly with the generic, actionable message.
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
