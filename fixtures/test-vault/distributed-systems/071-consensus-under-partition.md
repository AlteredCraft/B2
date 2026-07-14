---
b2id: 01KXF21DVJTQQJJPRRPZQPDBG5
type: note
title: "Consensus Under Partition"
---

# Consensus Under Partition

Notes on consensus under partition within the broader theme of distributed systems.

## Consensus Under Partition

Leader election lets a cluster nominate a single writer without a central coordinator. Consensus protocols like Raft keep a replicated log consistent despite crashes and partitions. Exactly-once delivery is usually a fiction; aim for at-least-once plus idempotency instead. Backpressure propagates load limits upstream so a slow consumer cannot drown a fast producer. See [[distributed-systems/111-idempotent-writes]].

Exponential backoff with jitter spreads retries out and prevents synchronized thundering herds. The two generals problem shows that no fixed number of messages guarantees agreement over a lossy link. Consensus protocols like Raft keep a replicated log consistent despite crashes and partitions. See [[distributed-systems/111-idempotent-writes]].

## The Two Generals

Exponential backoff with jitter spreads retries out and prevents synchronized thundering herds. Vector clocks capture causality so concurrent updates can be detected rather than silently lost. Exactly-once delivery is usually a fiction; aim for at-least-once plus idempotency instead. The two generals problem shows that no fixed number of messages guarantees agreement over a lossy link.

Consensus protocols like Raft keep a replicated log consistent despite crashes and partitions. The two generals problem shows that no fixed number of messages guarantees agreement over a lossy link. Backpressure propagates load limits upstream so a slow consumer cannot drown a fast producer. Exponential backoff with jitter spreads retries out and prevents synchronized thundering herds. Vector clocks capture causality so concurrent updates can be detected rather than silently lost. An idempotent write can be retried safely because applying it twice equals applying it once.

Vector clocks capture causality so concurrent updates can be detected rather than silently lost. An idempotent write can be retried safely because applying it twice equals applying it once. A monotonically increasing revision lets a writer detect that state changed under it. Clock skew makes wall-clock timestamps a poor basis for ordering events across machines. The two generals problem shows that no fixed number of messages guarantees agreement over a lossy link. Leader election lets a cluster nominate a single writer without a central coordinator. See [[databases/165-schema-migrations]].

## Eventual Consistency

Consensus protocols like Raft keep a replicated log consistent despite crashes and partitions. Leader election lets a cluster nominate a single writer without a central coordinator. Under a network partition you must choose between availability and strong consistency. Quorum reads and writes overlap by at least one node, so a read sees the latest acknowledged write. Clock skew makes wall-clock timestamps a poor basis for ordering events across machines. See [[distributed-systems/151-retries-and-jitter]].

Under a network partition you must choose between availability and strong consistency. Leader election lets a cluster nominate a single writer without a central coordinator. An idempotent write can be retried safely because applying it twice equals applying it once.

## The CAP Tradeoff

Under a network partition you must choose between availability and strong consistency. Backpressure propagates load limits upstream so a slow consumer cannot drown a fast producer. An idempotent write can be retried safely because applying it twice equals applying it once. See [[databases/065-vacuuming]]. See [[distributed-systems/191-retries-and-jitter]].

The two generals problem shows that no fixed number of messages guarantees agreement over a lossy link. Under a network partition you must choose between availability and strong consistency. Exactly-once delivery is usually a fiction; aim for at-least-once plus idempotency instead. An idempotent write can be retried safely because applying it twice equals applying it once. Exponential backoff with jitter spreads retries out and prevents synchronized thundering herds. Consensus protocols like Raft keep a replicated log consistent despite crashes and partitions. See [[distributed-systems/101-consensus-under-partition]]. See [[distributed-systems/061-quorum-reads]].

An idempotent write can be retried safely because applying it twice equals applying it once. Clock skew makes wall-clock timestamps a poor basis for ordering events across machines. Vector clocks capture causality so concurrent updates can be detected rather than silently lost. Exactly-once delivery is usually a fiction; aim for at-least-once plus idempotency instead.
