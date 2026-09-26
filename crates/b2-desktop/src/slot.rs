//! A single-in-flight slot for a long, cancellable façade op — host **infrastructure**,
//! not engine logic. The window drives two such ops, the background embed and a streaming
//! answer, and both need the same four things: refuse a second run while one is going,
//! release on every exit path, let another thread ask the run to stop, and keep a stale
//! stop request from killing the *next* run.

use std::ops::ControlFlow;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

/// How often [`Slot::cancel_and_wait`] re-asserts the cancel flag and re-checks whether
/// the run has wound down. Short enough to feel instant on a vault switch, long enough
/// not to spin hot.
const CANCEL_POLL: Duration = Duration::from_millis(25);

/// One op's slot: whether a run holds it, and whether that run has been asked to stop.
#[derive(Debug, Default)]
pub struct Slot {
    running: AtomicBool,
    cancel: AtomicBool,
}

/// Proof of holding a [`Slot`]; dropping it releases the slot, so it is freed on
/// **every** exit path — a normal return, an early `?` (no vault, no model), a panic.
#[derive(Debug)]
pub struct SlotGuard<'a>(&'a Slot);

impl Drop for SlotGuard<'_> {
    fn drop(&mut self) {
        self.0.running.store(false, Ordering::SeqCst);
    }
}

impl Slot {
    /// Claim the slot for a fresh run, or `None` when one is already in flight. Winning
    /// also clears any stale cancel — the request that stopped the *previous* run (or a
    /// vault switch) must not stop this one before it starts.
    pub fn try_claim(&self) -> Option<SlotGuard<'_>> {
        self.running
            .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
            .ok()?;
        self.cancel.store(false, Ordering::SeqCst);
        Some(SlotGuard(self))
    }

    /// Whether a run holds the slot right now.
    pub fn in_flight(&self) -> bool {
        self.running.load(Ordering::SeqCst)
    }

    /// Ask the running op to stop at its next checkpoint. Cooperative — never a thread
    /// kill, so no torn writes. A no-op if nothing is running.
    pub fn cancel(&self) {
        self.cancel.store(true, Ordering::SeqCst);
    }

    /// Whether the running op has been asked to stop.
    pub fn cancelled(&self) -> bool {
        self.cancel.load(Ordering::SeqCst)
    }

    /// The cancel flag as the façade's loops read it: [`ControlFlow::Break`] once a stop
    /// has been asked for, else [`ControlFlow::Continue`].
    pub fn flow(&self) -> ControlFlow<()> {
        if self.cancelled() {
            ControlFlow::Break(())
        } else {
            ControlFlow::Continue(())
        }
    }

    /// Cancel any run in flight and **block until it winds down**. Re-asserts the cancel
    /// on every poll, so it wins even against a run that claimed the slot (clearing the
    /// flag) a moment after the first request; returns at once when nothing is running.
    pub fn cancel_and_wait(&self) {
        loop {
            self.cancel();
            if !self.in_flight() {
                return;
            }
            std::thread::sleep(CANCEL_POLL);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn one_claim_at_a_time_and_the_guard_releases_it() {
        let slot = Slot::default();
        let held = slot.try_claim();
        assert!(held.is_some(), "the first claim wins the slot");
        assert!(slot.in_flight());
        assert!(
            slot.try_claim().is_none(),
            "a second claim is refused while running"
        );
        drop(held);
        assert!(!slot.in_flight());
        assert!(
            slot.try_claim().is_some(),
            "the slot is reusable once released"
        );
    }

    #[test]
    fn claiming_clears_a_stale_cancel_and_a_new_request_sets_it() {
        let slot = Slot::default();
        slot.cancel();
        assert!(slot.cancelled());
        assert!(slot.flow().is_break());
        // A fresh run clears the cancel a prior run (or vault switch) left behind…
        let _held = slot.try_claim();
        assert!(!slot.cancelled());
        assert!(slot.flow().is_continue());
        // …and a new request is observable again.
        slot.cancel();
        assert!(slot.flow().is_break());
    }
}
