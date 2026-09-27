//! Ctrl-C as a cooperative cancel: the flags a SIGINT handler flips, read by the two long
//! loops a command can stop part-way — the reindex embed loop and the chat token stream.

use std::ops::ControlFlow;
use std::sync::atomic::{AtomicBool, Ordering};

/// Set on SIGINT, from Ctrl-C or `b2 reindex --cancel` (GH #55). The reindex loop stops
/// after the current batch, leaving a consistent index; the chat stream stops at the next
/// token. `chat` clears it before each turn.
static CANCEL: AtomicBool = AtomicBool::new(false);

/// Whether `b2 chat` is mid-answer, which decides whether Ctrl-C cancels or leaves.
static ANSWERING: AtomicBool = AtomicBool::new(false);

/// Route Ctrl-C to [`CANCEL`], so a long loop stops at its next checkpoint. With
/// `exit_when_idle` (a REPL), Ctrl-C at an idle prompt still exits. Best-effort: the
/// default terminate is still safe.
pub fn install_cancel_on_sigint(exit_when_idle: bool) {
    let _ = ctrlc::set_handler(move || {
        if exit_when_idle && !ANSWERING.load(Ordering::SeqCst) {
            // 128 + SIGINT, the shell's own convention for "interrupted".
            std::process::exit(130);
        }
        CANCEL.store(true, Ordering::SeqCst);
    });
}

/// Run one REPL answer with Ctrl-C meaning "stop this answer", clearing the last turn's
/// cancel first.
pub fn while_answering<T>(answer: impl FnOnce() -> T) -> T {
    CANCEL.store(false, Ordering::SeqCst);
    ANSWERING.store(true, Ordering::SeqCst);
    let result = answer();
    ANSWERING.store(false, Ordering::SeqCst);
    result
}

/// The Ctrl-C flag as the [`ControlFlow`] the engine's loops read.
pub fn cancel_flow() -> ControlFlow<()> {
    if CANCEL.load(Ordering::SeqCst) {
        ControlFlow::Break(())
    } else {
        ControlFlow::Continue(())
    }
}
