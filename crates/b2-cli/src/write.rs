//! The write commands: `add`, `write`, `mv`, `rm` and `link`. Each needs an explicit
//! vault, so none can touch the wrong directory.

use crate::args::Cli;
use crate::emit;
use crate::error::CliError;
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
    // Embeds the new body, so the real model.
    let vault = open_vault(cli.require_vault(None)?, true)?;
    let report = vault.add_note(path, title, content)?;
    emit(cli.json, &report, |report| {
        println!("Created {}.", report.path)
    })
}

pub fn cmd_write(cli: &Cli, note: &str) -> Result<(), CliError> {
    // Model-free. Refuse a terminal stdin up front rather than hang: the body is piped.
    let stdin = std::io::stdin();
    if stdin.is_terminal() {
        return Err(CliError::StdinRequired);
    }
    let vault = open_vault(cli.require_vault(None)?, false)?;
    let mut body = String::new();
    stdin.lock().read_to_string(&mut body)?;
    // A one-shot holds no buffer, so there's no external-edit window to guard: chain on
    // the current revision.
    let current = vault.read(note)?;
    let report = vault.write(note, &body, &current.revision)?;
    emit(cli.json, &report, |report| {
        println!("Wrote {} ({} bytes).", report.path, body.len());
    })
}

pub fn cmd_mv(cli: &Cli, from: &str, to: &str) -> Result<(), CliError> {
    // Rewritten files re-embed, so the real model.
    let root = cli.require_vault(None)?;
    let vault = open_vault(root, true)?;
    // Kind dispatch (§9b #8): an existing directory is a folder, else the extension decides.
    if is_dir_arg(root, from) {
        emit(cli.json, &vault.move_dir(from, to)?, |report| {
            println!(
                "Moved {}/ → {}/ ({} note(s), {} file(s))",
                report.from, report.to, report.moved_notes, report.moved_resources
            );
            print_rewrite_tally(report.links_rewritten, report.rewrote.len());
        })
    } else if doc_kind(from) == DocKind::Resource {
        emit(cli.json, &vault.move_resource(from, to)?, |report| {
            println!("Moved {} → {}", report.from, report.to);
            print_rewrite_tally(report.links_rewritten, report.rewrote.len());
        })
    } else {
        emit(cli.json, &vault.move_note(from, to)?, |report| {
            println!("Moved {} → {}", report.from, report.to);
            print_rewrite_tally(report.links_rewritten, report.rewrote.len());
        })
    }
}

pub fn cmd_rm(cli: &Cli, target: &str, recursive: bool) -> Result<(), CliError> {
    // Never rewrites a body, so model-free.
    let root = cli.require_vault(None)?;
    let vault = open_vault(root, false)?;
    // Kind dispatch as in `mv`; a folder needs --recursive, the CLI's confirm dialog.
    if is_dir_arg(root, target) {
        if !recursive {
            return Err(CliError::RecursiveRequired(target.to_string()));
        }
        emit(cli.json, &vault.delete_dir(target)?, |report| {
            println!(
                "Deleted {}/ ({} note(s), {} file(s))",
                report.dir, report.deleted_notes, report.deleted_resources
            );
            print_dangled(&report.dangled);
        })
    } else if doc_kind(target) == DocKind::Resource {
        emit(cli.json, &vault.delete_resource(target)?, |report| {
            println!("Deleted {}", report.path);
            print_dangled(&report.dangled);
        })
    } else {
        emit(cli.json, &vault.delete_note(target)?, |report| {
            println!("Deleted {}", report.path);
            print_dangled(&report.dangled);
        })
    }
}

pub fn cmd_link(
    cli: &Cli,
    src: &str,
    dst: &str,
    edge_type: &str,
    explanation: Option<&str>,
) -> Result<(), CliError> {
    // Opens with the real model, like `add`/`mv`; a frontmatter-only edit won't re-embed.
    let vault = open_vault(cli.require_vault(None)?, true)?;
    let report = vault.link(src, dst, edge_type, explanation)?;
    emit(cli.json, &report, |report| {
        if report.created {
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
    })
}

/// Whether a `mv`/`rm` argument names an existing directory (a trailing `/` is fine).
fn is_dir_arg(root: &Path, arg: &str) -> bool {
    root.join(arg.trim_end_matches('/')).is_dir()
}

/// The `mv` tally, shared by its three arms.
fn print_rewrite_tally(links_rewritten: usize, rewrote_files: usize) {
    if links_rewritten > 0 {
        println!("Rewrote {links_rewritten} inbound link(s) across {rewrote_files} file(s).");
    } else {
        println!("No inbound links to rewrite.");
    }
}

/// The `rm` tally, shared by its three arms.
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
