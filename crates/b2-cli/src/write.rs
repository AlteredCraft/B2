//! The write commands: `add`, `write`, `mv`, `rm` and `link`. Each needs an explicit
//! vault, so none can touch the wrong directory.

use crate::args::Cli;
use crate::error::CliError;
use crate::print_json;
use crate::wiring::open_vault;
use b2_core::resource::{doc_kind, DocKind};
use std::io::{IsTerminal, Read};
use std::path::Path;

pub fn cmd_add(
    cli: &Cli,
    path: &str,
    title: Option<&str>,
    content: Option<&str>,
) -> Result<(), CliError> {
    // Add writes a new note (and embeds its body) → require an explicit vault
    // (no silent cwd), and it needs the real model like `reindex`/`mv`/`link`.
    let vault = open_vault(cli.require_vault(None)?, true)?;
    let report = vault.add_note(path, title, content)?;
    if cli.json {
        print_json(&report)?;
    } else {
        println!("Created {}.", report.path);
    }
    Ok(())
}

pub fn cmd_write(cli: &Cli, note: &str) -> Result<(), CliError> {
    // A body splice + re-projection: **model-free** (like the desktop's save and
    // `rm`), still an explicit vault like every write. Refuse an interactive
    // terminal up front so the command never silently hangs waiting for
    // hand-typed input — the new body is always *piped* (an agent, `cat file |`, …).
    let stdin = std::io::stdin();
    if stdin.is_terminal() {
        return Err(CliError::StdinRequired);
    }
    let vault = open_vault(cli.require_vault(None)?, false)?;
    let mut body = String::new();
    stdin.lock().read_to_string(&mut body)?;
    // Stateless one-shot: read the current on-disk revision and chain the write on
    // it. A CLI holds no long-lived buffer, so there's no external-edit window to
    // guard here — the content-hash guard exists for the desktop's in-memory
    // buffer; for a one-shot, the contract is simply "the body becomes exactly
    // this" (`Vault::write` still keeps the frontmatter bytes untouched).
    let current = vault.read(note)?;
    let report = vault.write(note, &body, &current.revision)?;
    if cli.json {
        print_json(&report)?;
    } else {
        println!("Wrote {} ({} bytes).", report.path, body.len());
    }
    Ok(())
}

pub fn cmd_mv(cli: &Cli, from: &str, to: &str) -> Result<(), CliError> {
    // A move rewrites files (and re-embeds them on re-projection) → require an
    // explicit vault (no silent cwd), and it needs the real model the index was
    // built with, like `reindex`/`add`/`link`.
    let root = cli.require_vault(None)?;
    let vault = open_vault(root, true)?;
    // Kind dispatch (§9b #8): an existing directory moves as a folder
    // (every file under it, one rename); otherwise the two file arms
    // differ only in the report type they print.
    // The human "Moved" line differs per arm, the rewrite tally is shared.
    if is_dir_arg(root, from) {
        let report = vault.move_dir(from, to)?;
        if cli.json {
            print_json(&report)?;
        } else {
            println!(
                "Moved {}/ → {}/ ({} note(s), {} file(s))",
                report.from, report.to, report.moved_notes, report.moved_resources
            );
            print_rewrite_tally(report.links_rewritten, report.rewrote.len());
        }
    } else if doc_kind(from) == DocKind::Resource {
        let report = vault.move_resource(from, to)?;
        if cli.json {
            print_json(&report)?;
        } else {
            println!("Moved {} → {}", report.from, report.to);
            print_rewrite_tally(report.links_rewritten, report.rewrote.len());
        }
    } else {
        let report = vault.move_note(from, to)?;
        if cli.json {
            print_json(&report)?;
        } else {
            println!("Moved {} → {}", report.from, report.to);
            print_rewrite_tally(report.links_rewritten, report.rewrote.len());
        }
    }
    Ok(())
}

pub fn cmd_rm(cli: &Cli, target: &str, recursive: bool) -> Result<(), CliError> {
    // A delete removes files and index rows but never rewrites a body
    // (inbound links dangle, they aren't repaired) → **model-free**, like
    // the desktop's delete; still an explicit vault, like every write.
    let root = cli.require_vault(None)?;
    let vault = open_vault(root, false)?;
    // Kind dispatch (§9b #8), mirroring `mv`: an existing directory deletes
    // as a folder — gated on --recursive, the CLI's stand-in for the
    // desktop's confirm dialog — else the extension picks the file arm.
    if is_dir_arg(root, target) {
        if !recursive {
            return Err(CliError::RecursiveRequired(target.to_string()));
        }
        let report = vault.delete_dir(target)?;
        if cli.json {
            print_json(&report)?;
        } else {
            println!(
                "Deleted {}/ ({} note(s), {} file(s))",
                report.dir, report.deleted_notes, report.deleted_resources
            );
            print_dangled(&report.dangled);
        }
    } else if doc_kind(target) == DocKind::Resource {
        let report = vault.delete_resource(target)?;
        if cli.json {
            print_json(&report)?;
        } else {
            println!("Deleted {}", report.path);
            print_dangled(&report.dangled);
        }
    } else {
        let report = vault.delete_note(target)?;
        if cli.json {
            print_json(&report)?;
        } else {
            println!("Deleted {}", report.path);
            print_dangled(&report.dangled);
        }
    }
    Ok(())
}

pub fn cmd_link(
    cli: &Cli,
    src: &str,
    dst: &str,
    edge_type: &str,
    explanation: Option<&str>,
) -> Result<(), CliError> {
    // Link writes the source note's frontmatter and re-projects it → require an
    // explicit vault (no silent cwd), opening with the same real model the index
    // was built with (like `add`/`mv`); a frontmatter-only edit won't re-embed.
    let vault = open_vault(cli.require_vault(None)?, true)?;
    let report = vault.link(src, dst, edge_type, explanation)?;
    if cli.json {
        print_json(&report)?;
    } else if report.created {
        println!(
            "Linked {} —{}→ {}. Wrote the relation into the source note's frontmatter.",
            report.src_path, report.relation, report.dst_path
        );
    } else {
        println!(
            "Already linked {} —{}→ {}. Nothing changed.",
            report.src_path, report.relation, report.dst_path
        );
    }
    Ok(())
}

/// Whether a `mv`/`rm` argument names an existing directory under `root` — the
/// kind-dispatch test that routes to the folder arm (a trailing `/` is tolerated).
fn is_dir_arg(root: &Path, arg: &str) -> bool {
    root.join(arg.trim_end_matches('/')).is_dir()
}

/// The `mv` tally, shared by its three arms: how many inbound link targets were
/// rewritten across how many files — or that nothing linked to the moved item.
fn print_rewrite_tally(links_rewritten: usize, rewrote_files: usize) {
    if links_rewritten > 0 {
        println!("Rewrote {links_rewritten} inbound link(s) across {rewrote_files} file(s).");
    } else {
        println!("No inbound links to rewrite.");
    }
}

/// The `rm` tally, shared by its three arms: which surviving files' links now
/// dangle — or that none were affected.
fn print_dangled(dangled: &[String]) {
    if dangled.is_empty() {
        println!("No inbound links affected.");
    } else {
        println!(
            "Links in {} file(s) now unresolved: {}",
            dangled.len(),
            dangled.join(", ")
        );
    }
}
