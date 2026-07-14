---
b2id: 01KXF21DVFZ49PDRXTA4RM88DW
type: note
title: "The CAP Tradeoff"
---

# The CAP Tradeoff

Notes on the cap tradeoff within the broader theme of distributed systems.

## Quorum Reads

Clock skew makes wall-clock timestamps a poor basis for ordering events across machines. The two generals problem shows that no fixed number of messages guarantees agreement over a lossy link. Vector clocks capture causality so concurrent updates can be detected rather than silently lost. Under a network partition you must choose between availability and strong consistency. Quorum reads and writes overlap by at least one node, so a read sees the latest acknowledged write.

Exponential backoff with jitter spreads retries out and prevents synchronized thundering herds. The two generals problem shows that no fixed number of messages guarantees agreement over a lossy link. Consensus protocols like Raft keep a replicated log consistent despite crashes and partitions. A monotonically increasing revision lets a writer detect that state changed under it. Quorum reads and writes overlap by at least one node, so a read sees the latest acknowledged write. Exactly-once delivery is usually a fiction; aim for at-least-once plus idempotency instead.

Exactly-once delivery is usually a fiction; aim for at-least-once plus idempotency instead. Backpressure propagates load limits upstream so a slow consumer cannot drown a fast producer. The two generals problem shows that no fixed number of messages guarantees agreement over a lossy link. Vector clocks capture causality so concurrent updates can be detected rather than silently lost. See [[distributed-systems/131-the-two-generals]].

## The Two Generals

Exactly-once delivery is usually a fiction; aim for at-least-once plus idempotency instead. Consensus protocols like Raft keep a replicated log consistent despite crashes and partitions. The two generals problem shows that no fixed number of messages guarantees agreement over a lossy link. An idempotent write can be retried safely because applying it twice equals applying it once. Clock skew makes wall-clock timestamps a poor basis for ordering events across machines. Leader election lets a cluster nominate a single writer without a central coordinator. See [[distributed-systems/081-retries-and-jitter]].

Exactly-once delivery is usually a fiction; aim for at-least-once plus idempotency instead. The two generals problem shows that no fixed number of messages guarantees agreement over a lossy link. Exponential backoff with jitter spreads retries out and prevents synchronized thundering herds. Quorum reads and writes overlap by at least one node, so a read sees the latest acknowledged write. A monotonically increasing revision lets a writer detect that state changed under it. Under a network partition you must choose between availability and strong consistency. See [[distributed-systems/121-consensus-under-partition]]. See [[transformers/014-context-windows]].

## The CAP Tradeoff

Backpressure propagates load limits upstream so a slow consumer cannot drown a fast producer. Exponential backoff with jitter spreads retries out and prevents synchronized thundering herds. Clock skew makes wall-clock timestamps a poor basis for ordering events across machines. Under a network partition you must choose between availability and strong consistency. See [[pkm/123-linking-your-thinking]]. See [[distributed-systems/181-retries-and-jitter]].

Clock skew makes wall-clock timestamps a poor basis for ordering events across machines. Consensus protocols like Raft keep a replicated log consistent despite crashes and partitions. Leader election lets a cluster nominate a single writer without a central coordinator. Vector clocks capture causality so concurrent updates can be detected rather than silently lost. The two generals problem shows that no fixed number of messages guarantees agreement over a lossy link. Under a network partition you must choose between availability and strong consistency.

## Retries and Jitter

An idempotent write can be retried safely because applying it twice equals applying it once. Vector clocks capture causality so concurrent updates can be detected rather than silently lost. Exponential backoff with jitter spreads retries out and prevents synchronized thundering herds. Quorum reads and writes overlap by at least one node, so a read sees the latest acknowledged write. Clock skew makes wall-clock timestamps a poor basis for ordering events across machines. Exactly-once delivery is usually a fiction; aim for at-least-once plus idempotency instead. See [[distributed-systems/061-quorum-reads]].

Exponential backoff with jitter spreads retries out and prevents synchronized thundering herds. A monotonically increasing revision lets a writer detect that state changed under it. Vector clocks capture causality so concurrent updates can be detected rather than silently lost. See [[vector-search/110-recall-vs-latency]].
