//! A single-in-flight slot for a long, cancellable façade op (the background embed, a
//! streaming answer): refuses a second run, releases on every exit path, and keeps a stale
//! stop request from killing the next run.

use std::ops::ControlFlow;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

/// How often [`Slot::cancel_and_wait`] re-asserts the cancel and re-checks the run.
const CANCEL_POLL: Duration = Duration::from_millis(25);

/// One op's slot: whether a run holds it, and whether that run has been asked to stop.
#[derive(Debug, Default)]
pub struct Slot {
    running: AtomicBool,
    cancel: AtomicBool,
}

/// Proof of holding a [`Slot`]; dropping it releases the slot on every exit path.
#[derive(Debug)]
pub struct SlotGuard<'a>(&'a Slot);

impl Drop for SlotGuard<'_> {
    fn drop(&mut self) {
        self.0.running.store(false, Ordering::SeqCst);
    }
}

impl Slot {
    /// Claim the slot, or `None` when a run is in flight. Clears any stale cancel, so the
    /// request that stopped the previous run can't stop this one.
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

    /// Ask the running op to stop at its next checkpoint (cooperative, so no torn writes).
    pub fn cancel(&self) {
        self.cancel.store(true, Ordering::SeqCst);
    }

    /// Whether the running op has been asked to stop.
    pub fn cancelled(&self) -> bool {
        self.cancel.load(Ordering::SeqCst)
    }

    /// The cancel flag as the façade's loops read it.
    pub fn flow(&self) -> ControlFlow<()> {
        if self.cancelled() {
            ControlFlow::Break(())
        } else {
            ControlFlow::Continue(())
        }
    }

    /// Cancel any run in flight and block until it winds down. Re-asserts the cancel on
    /// every poll, so it wins against a run that claimed the slot (clearing the flag) just
    /// after the first request.
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
        let _held = slot.try_claim();
        assert!(!slot.cancelled());
        assert!(slot.flow().is_continue());
        slot.cancel();
        assert!(slot.flow().is_break());
    }
}
