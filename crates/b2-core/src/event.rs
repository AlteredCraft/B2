//! The event-log sink seam (data-model.md §4). The durable append-only JSONL log
//! and `replay()` arrive in step 4; step 1 needs only the `append(event)` seam so
//! a `b2id` stamp can be recorded. Production defaults to [`NullSink`]; tests use
//! a collecting double.

/// A consequential operation B2 performed, worth remembering as history. The
/// variant set grows step by step; step 1 emits only `B2idStamped`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Event {
    /// B2 stamped a missing `b2id` into a note (its one always-allowed write).
    B2idStamped { b2id: String, path: String },
}

/// Where consequential events go. Behind a trait so the durable JSONL sink can
/// drop in later without touching producers (data-model.md §4).
pub trait EventSink {
    fn append(&self, event: Event);
}

/// Default sink: drops every event. Used until the durable log lands in step 4.
#[derive(Debug, Default, Clone, Copy)]
pub struct NullSink;

impl EventSink for NullSink {
    fn append(&self, _event: Event) {}
}
