//! The chat surfaces: `ask`, `why` and the `chat` REPL, and the streamed rendering
//! they share.

use crate::args::{Cli, LlmArgs};
use crate::cancel::{cancel_flow, ANSWERING, CANCEL};
use crate::error::{user_message, CliError};
use crate::wiring::{open_llm, open_vault, use_fake_llm};
use b2_core::llm::{ChatTurn, LlmProvider};
use b2_core::vault::{AnswerView, Vault};
use std::io::{IsTerminal, Write};
use std::ops::ControlFlow;
use std::sync::atomic::Ordering;

pub fn cmd_ask(cli: &Cli, question: &str, llm_args: &LlmArgs) -> Result<(), CliError> {
    // The provider first, deliberately: it is the cheap check, and loading the real
    // embedder below takes seconds. A stopped model server should cost a round trip,
    // not a model load followed by a failure.
    let llm = open_llm(llm_args)?;
    // Retrieval embeds the question for the vector half → the real model, like `search`.
    let vault = open_vault(cli.vault_or_cwd(), true)?;
    // Ctrl-C cancels the answer at the next token rather than killing the process, so
    // what already streamed stays on screen and is reported as partial.
    let _ = ctrlc::set_handler(|| CANCEL.store(true, Ordering::SeqCst));
    ask_streamed(&vault, llm.as_ref(), question, &[], cli.json)?;
    if !cli.json {
        note_fake_llm();
    }
    Ok(())
}

pub fn cmd_why(
    cli: &Cli,
    note: &str,
    candidate: &str,
    limit: usize,
    llm_args: &LlmArgs,
) -> Result<(), CliError> {
    let llm = open_llm(llm_args)?;
    // A pure read over stored vectors, like `similar`: nothing embeds a query, so the
    // real model is never loaded.
    let vault = open_vault(cli.vault_or_cwd(), false)?;
    let _ = ctrlc::set_handler(|| CANCEL.store(true, Ordering::SeqCst));
    let json = cli.json;
    let answer = vault.why_similar(llm.as_ref(), note, candidate, limit, &mut |token| {
        stream_token(token, json)
    })?;
    finish_answer(&answer, json);
    if !json {
        note_fake_llm();
    }
    Ok(())
}

pub fn cmd_chat(cli: &Cli, llm_args: &LlmArgs) -> Result<(), CliError> {
    let llm = open_llm(llm_args)?;
    let vault = open_vault(cli.vault_or_cwd(), true)?;
    // Ctrl-C means two different things in a REPL, and a handler that only ever meant one
    // would trap the user: mid-answer it cancels the stream (the partial text stands), but
    // at an idle prompt it must still be the way out, since swallowing it would leave
    // `/exit` and Ctrl-D as the only exits. `ctrlc` runs this on its own thread.
    let _ = ctrlc::set_handler(|| {
        if ANSWERING.load(Ordering::SeqCst) {
            CANCEL.store(true, Ordering::SeqCst);
        } else {
            // 128 + SIGINT, the shell's own convention for "interrupted".
            std::process::exit(130);
        }
    });
    // Prompts and the banner are chrome for a human at a terminal: on stderr so
    // stdout stays answers, and only when there's a terminal there to read them
    // (the `reindex` progress-line rule).
    let interactive = !cli.json && std::io::stderr().is_terminal();
    if interactive {
        eprintln!(
            "Grounded chat over your notes — answers come only from what B2 retrieves \
             from them, cited by [n]."
        );
        eprintln!(
            "Model: {}. Ctrl-C stops an answer; /exit or Ctrl-D leaves. \
             Nothing here is saved.",
            llm.model_id()
        );
    }
    if !cli.json {
        note_fake_llm();
    }
    // Session-only history (S4): the turns live in this Vec and die with the process —
    // a persisted transcript would be B2-derived state outside the Markdown.
    let mut history: Vec<ChatTurn> = Vec::new();
    let stdin = std::io::stdin();
    loop {
        if interactive {
            eprint!("\nyou> ");
            let _ = std::io::stderr().flush();
        }
        let mut line = String::new();
        if stdin.read_line(&mut line)? == 0 {
            // Ctrl-D / end of a piped script.
            break;
        }
        let question = line.trim();
        if question.is_empty() {
            continue;
        }
        if matches!(question, "/exit" | "/quit") {
            break;
        }
        // A fresh cancel budget per turn: the Ctrl-C that stopped the *last* answer
        // must not cancel this one before its first token.
        CANCEL.store(false, Ordering::SeqCst);
        ANSWERING.store(true, Ordering::SeqCst);
        let turn = ask_streamed(&vault, llm.as_ref(), question, &history, cli.json);
        ANSWERING.store(false, Ordering::SeqCst);
        match turn {
            Ok(answer) => {
                // A cancelled answer goes into the history too: the human saw that
                // text, so a follow-up referring to it ("go on") must be read
                // against what was actually said, not against a turn we pretend
                // never happened.
                history.push(ChatTurn::user(question));
                history.push(ChatTurn::assistant(&answer.answer));
            }
            // A failed turn ends the turn, not the session: a model server that
            // hiccuped is worth retyping a question at, not worth losing the
            // conversation over. The message is the same one the process would
            // have exited with.
            Err(e) => eprintln!("{}", user_message(&e)),
        }
    }
    Ok(())
}

/// One grounded ask, rendered as it arrives — the shared body of `ask` and each `chat`
/// turn (flow ④'s streaming contract: every surface streams). Under `--json` this is a
/// **JSON Lines event stream** — one `token` event per token, then one `answer` event
/// carrying the [`AnswerView`] — so an agent sees the answer forming and still gets the
/// resolved citations as data.
fn ask_streamed(
    vault: &Vault,
    llm: &dyn LlmProvider,
    question: &str,
    history: &[ChatTurn],
    json: bool,
) -> Result<AnswerView, CliError> {
    let answer = vault.ask(llm, question, history, &mut |token| {
        stream_token(token, json)
    })?;
    finish_answer(&answer, json);
    Ok(answer)
}

/// Render one streamed token — the framing `ask`, `chat` and `why` share — and report
/// whether Ctrl-C has asked the stream to stop.
fn stream_token(token: &str, json: bool) -> ControlFlow<()> {
    if json {
        print_event(&AskEvent::Token { text: token });
    } else {
        print!("{token}");
        // Streaming is the point: an unflushed line renders in one lump.
        let _ = std::io::stdout().flush();
    }
    cancel_flow()
}

/// Close a streamed answer: the final `answer` event under `--json`, else the sources.
fn finish_answer(answer: &AnswerView, json: bool) {
    if json {
        print_event(&AskEvent::Answer(answer));
    } else {
        print_answer_tail(answer);
    }
}

/// One line of the `--json` ask stream. The framing is the CLI's; the payload of
/// the final event is `b2-core`'s [`AnswerView`], which is the standing
/// convention (the view types are the adapters' shared contract — the desktop
/// carries the same two facts as Tauri events plus a command return).
#[derive(serde::Serialize)]
#[serde(tag = "event", rename_all = "snake_case")]
enum AskEvent<'a> {
    /// A token, exactly as it arrived from the model.
    Token { text: &'a str },
    /// The finished answer: text, resolved citations, and whether it was cut short.
    Answer(&'a AnswerView),
}

/// Print one event as a single line of JSON — **not** `print_json`, which is
/// pretty-printed: this is a stream, so one object per line is the contract.
fn print_event(event: &AskEvent) {
    if let Ok(line) = serde_json::to_string(event) {
        println!("{line}");
    }
}

/// The human-readable tail of an answer: end the streamed line, then the sources
/// the model cited, then — honestly — whether the answer is the whole of one.
fn print_answer_tail(answer: &AnswerView) {
    println!();
    if !answer.citations.is_empty() {
        println!("\nSources:");
        for c in &answer.citations {
            println!("  [{}] {}", c.marker, c.path);
            if !c.excerpt.is_empty() {
                println!("      {}", c.excerpt);
            }
        }
    }
    if !answer.tools.is_empty() {
        println!("\nB2 tools used:");
        for t in &answer.tools {
            // A lookup B2 made itself is marked, so the list never overstates what the
            // model chose to do.
            let by = if t.seeded { "  (made by B2)" } else { "" };
            println!("  {} {}{by}", t.name, t.arguments);
        }
    }
    if answer.cancelled {
        eprintln!("(stopped early — the answer above is partial.)");
    }
}

/// Never overstate what answered (the `search` fake-embedder caveat, applied to
/// the chat seam): under `B2_LLM=fake` the "answer" is deterministic scaffolding,
/// not a model. On stderr, so stdout stays the answer.
fn note_fake_llm() {
    if use_fake_llm() {
        eprintln!(
            "note: the fake chat provider is in use (B2_LLM=fake) — answers are \
             deterministic test scaffolding, not a model."
        );
    }
}
