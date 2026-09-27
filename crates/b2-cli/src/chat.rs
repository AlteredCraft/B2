//! The chat surfaces: `ask`, `why` and the `chat` REPL, and the streamed rendering
//! they share.

use crate::args::{Cli, LlmArgs};
use crate::cancel::{cancel_flow, install_cancel_on_sigint, while_answering};
use crate::error::{user_message, CliError};
use crate::wiring::{open_llm, open_vault};
use b2_core::llm::{ChatTurn, LlmProvider};
use b2_core::vault::{AnswerView, Vault};
use std::io::{IsTerminal, Write};
use std::ops::ControlFlow;

pub fn cmd_ask(cli: &Cli, question: &str, llm_args: &LlmArgs) -> Result<(), CliError> {
    // The provider first: a stopped server should cost a round trip, not a model load.
    let llm = open_llm(llm_args)?;
    // Retrieval embeds the question, so the real model.
    let vault = open_vault(cli.vault_or_cwd(), true)?;
    // Ctrl-C cancels at the next token; what streamed stays, reported as partial.
    install_cancel_on_sigint(false);
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
    // Stored vectors only, like `similar`: no model load.
    let vault = open_vault(cli.vault_or_cwd(), false)?;
    install_cancel_on_sigint(false);
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
    // Mid-answer Ctrl-C cancels the stream; at an idle prompt it leaves.
    install_cancel_on_sigint(true);
    // Chrome goes to stderr, and only on a terminal.
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
    // Session-only history (S4): a persisted transcript would be state outside the Markdown.
    let mut history: Vec<ChatTurn> = Vec::new();
    let stdin = std::io::stdin();
    loop {
        if interactive {
            eprint!("\nyou> ");
            let _ = std::io::stderr().flush();
        }
        let mut line = String::new();
        if stdin.read_line(&mut line)? == 0 {
            break;
        }
        let question = line.trim();
        if question.is_empty() {
            continue;
        }
        if matches!(question, "/exit" | "/quit") {
            break;
        }
        let turn =
            while_answering(|| ask_streamed(&vault, llm.as_ref(), question, &history, cli.json));
        match turn {
            Ok(answer) => {
                // A cancelled answer is kept too: a follow-up refers to what was shown.
                history.push(ChatTurn::user(question));
                history.push(ChatTurn::assistant(&answer.answer));
            }
            // A failed turn ends the turn, not the session.
            Err(e) => eprintln!("{}", user_message(&e)),
        }
    }
    Ok(())
}

/// One grounded ask, streamed: the body of `ask` and each `chat` turn. Under `--json`, a
/// JSON Lines stream of `token` events, then one `answer` event carrying the [`AnswerView`].
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

/// Render one streamed token and report whether Ctrl-C asked the stream to stop.
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

/// One line of the `--json` ask stream. The final payload is `b2-core`'s [`AnswerView`],
/// the adapters' shared contract.
#[derive(serde::Serialize)]
#[serde(tag = "event", rename_all = "snake_case")]
enum AskEvent<'a> {
    /// A token, exactly as it arrived from the model.
    Token { text: &'a str },
    /// The finished answer: text, resolved citations, and whether it was cut short.
    Answer(&'a AnswerView),
}

/// One event per line: not `print_json`, which pretty-prints.
fn print_event(event: &AskEvent) {
    if let Ok(line) = serde_json::to_string(event) {
        println!("{line}");
    }
}

/// The human-readable tail of an answer: sources, tools, and whether it was cut short.
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
            // Marked, so the list never overstates what the model chose to do.
            let by = if t.seeded { "  (made by B2)" } else { "" };
            println!("  {} {}{by}", t.name, t.arguments);
        }
    }
    if answer.cancelled {
        eprintln!("(stopped early — the answer above is partial.)");
    }
}

/// Never overstate what answered: under `B2_LLM=fake` the answer is scaffolding. On
/// stderr, so stdout stays the answer.
fn note_fake_llm() {
    if b2_llm::fake_requested() {
        eprintln!("{}", b2_llm::FAKE_NOTICE);
    }
}
