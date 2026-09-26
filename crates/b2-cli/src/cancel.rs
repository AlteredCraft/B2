//! The cooperative-cancel flags a SIGINT handler flips, read by the two long loops a
//! command can stop part-way: the reindex embed loop and the chat token stream.

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
pub static CANCEL: AtomicBool = AtomicBool::new(false);

/// Whether `b2 chat` is streaming an answer right now — which is what decides
/// what Ctrl-C *means* in the REPL: cancel this answer, or leave. Only `chat`
/// maintains it (see its handler); every other command's SIGINT meaning is
/// unconditional.
pub static ANSWERING: AtomicBool = AtomicBool::new(false);

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
