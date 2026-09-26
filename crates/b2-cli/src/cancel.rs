//! Ctrl-C as a cooperative cancel: the flags a SIGINT handler flips, read by the two long
//! loops a command can stop part-way — the reindex embed loop and the chat token stream.

use std::ops::ControlFlow;
use std::sync::atomic::{AtomicBool, Ordering};

/// Set by the SIGINT handler installed for a long-running command — by Ctrl-C in the
/// foreground, or (for a `reindex`) by another process's `b2 reindex --cancel` (GH #55),
/// which raises the *same* signal so both reach here.
///
/// Two loops read it through the same [`ControlFlow`] seam: the reindex embed loop at
/// every batch boundary, stopping *after* the current batch so a cancel leaves a
/// consistent, re-runnable index; and the chat surfaces' token callback at every token,
/// where stopping renders the partial answer honestly. `chat` clears it before each turn.
static CANCEL: AtomicBool = AtomicBool::new(false);

/// Whether `b2 chat` is streaming an answer right now — which is what decides
/// what Ctrl-C *means* in the REPL: cancel this answer, or leave. Only `chat`
/// maintains it ([`while_answering`]); every other command's SIGINT meaning is
/// unconditional.
static ANSWERING: AtomicBool = AtomicBool::new(false);

/// Route Ctrl-C to [`CANCEL`] instead of killing the process, so a long loop stops at its
/// next checkpoint and what it already did stands.
///
/// `exit_when_idle` is for a REPL, where Ctrl-C means two things and a handler that only
/// ever meant one would trap the user: mid-answer it cancels the stream, but at an idle
/// prompt it must still be the way out, since swallowing it would leave `/exit` and
/// Ctrl-D as the only exits. `ctrlc` runs the handler on its own thread.
///
/// Best-effort: if the handler can't be installed, Ctrl-C keeps its default (terminate),
/// which is still safe — a reindex writes edges and FTS before any vectors, and a chat
/// stores nothing.
pub fn install_cancel_on_sigint(exit_when_idle: bool) {
    let _ = ctrlc::set_handler(move || {
        if exit_when_idle && !ANSWERING.load(Ordering::SeqCst) {
            // 128 + SIGINT, the shell's own convention for "interrupted".
            std::process::exit(130);
        }
        CANCEL.store(true, Ordering::SeqCst);
    });
}

/// Run one REPL answer with Ctrl-C meaning "stop this answer". A fresh cancel budget per
/// turn: the Ctrl-C that stopped the *last* answer must not cancel this one before its
/// first token.
pub fn while_answering<T>(answer: impl FnOnce() -> T) -> T {
    CANCEL.store(false, Ordering::SeqCst);
    ANSWERING.store(true, Ordering::SeqCst);
    let result = answer();
    ANSWERING.store(false, Ordering::SeqCst);
    result
}

/// Map the Ctrl-C flag onto the cooperative-cancel signal the engine's loops read —
/// [`ControlFlow::Break`] once a cancel has been requested, else
/// [`ControlFlow::Continue`]. Shared by `reindex`'s two progress closures and by the
/// chat surfaces' token callback.
pub fn cancel_flow() -> ControlFlow<()> {
    if CANCEL.load(Ordering::SeqCst) {
        ControlFlow::Break(())
    } else {
        ControlFlow::Continue(())
    }
}
