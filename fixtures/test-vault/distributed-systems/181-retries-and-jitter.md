---
b2id: 01KXF21DVX475CY6RAMNES955A
type: note
title: "Retries and Jitter"
---

# Retries and Jitter

Notes on retries and jitter within the broader theme of distributed systems.

## The CAP Tradeoff

Under a network partition you must choose between availability and strong consistency. The two generals problem shows that no fixed number of messages guarantees agreement over a lossy link. An idempotent write can be retried safely because applying it twice equals applying it once. Leader election lets a cluster nominate a single writer without a central coordinator. Vector clocks capture causality so concurrent updates can be detected rather than silently lost.

Backpressure propagates load limits upstream so a slow consumer cannot drown a fast producer. Exactly-once delivery is usually a fiction; aim for at-least-once plus idempotency instead. The two generals problem shows that no fixed number of messages guarantees agreement over a lossy link. Consensus protocols like Raft keep a replicated log consistent despite crashes and partitions. Quorum reads and writes overlap by at least one node, so a read sees the latest acknowledged write. Leader election lets a cluster nominate a single writer without a central coordinator. See [[hiking/139-blister-prevention]].

Clock skew makes wall-clock timestamps a poor basis for ordering events across machines. Consensus protocols like Raft keep a replicated log consistent despite crashes and partitions. A monotonically increasing revision lets a writer detect that state changed under it. The two generals problem shows that no fixed number of messages guarantees agreement over a lossy link. See [[distributed-systems/061-quorum-reads]]. See [[distributed-systems/161-idempotent-writes]].

## Leader Election

Backpressure propagates load limits upstream so a slow consumer cannot drown a fast producer. Clock skew makes wall-clock timestamps a poor basis for ordering events across machines. Quorum reads and writes overlap by at least one node, so a read sees the latest acknowledged write.

Leader election lets a cluster nominate a single writer without a central coordinator. Backpressure propagates load limits upstream so a slow consumer cannot drown a fast producer. The two generals problem shows that no fixed number of messages guarantees agreement over a lossy link. Exponential backoff with jitter spreads retries out and prevents synchronized thundering herds. See [[distributed-systems/061-quorum-reads]]. See [[distributed-systems/141-consensus-under-partition]].

Exactly-once delivery is usually a fiction; aim for at-least-once plus idempotency instead. Consensus protocols like Raft keep a replicated log consistent despite crashes and partitions. A monotonically increasing revision lets a writer detect that state changed under it. See [[distributed-systems/001-consensus-under-partition]]. See [[distributed-systems/071-consensus-under-partition]].

## Eventual Consistency

Under a network partition you must choose between availability and strong consistency. Leader election lets a cluster nominate a single writer without a central coordinator. An idempotent write can be retried safely because applying it twice equals applying it once. Consensus protocols like Raft keep a replicated log consistent despite crashes and partitions. Vector clocks capture causality so concurrent updates can be detected rather than silently lost. See [[distributed-systems/191-retries-and-jitter]]. See [[coffee/148-single-origin-beans]].

The two generals problem shows that no fixed number of messages guarantees agreement over a lossy link. Exponential backoff with jitter spreads retries out and prevents synchronized thundering herds. An idempotent write can be retried safely because applying it twice equals applying it once. Under a network partition you must choose between availability and strong consistency. Vector clocks capture causality so concurrent updates can be detected rather than silently lost. A monotonically increasing revision lets a writer detect that state changed under it.

## Quorum Reads

A monotonically increasing revision lets a writer detect that state changed under it. Vector clocks capture causality so concurrent updates can be detected rather than silently lost. Consensus protocols like Raft keep a replicated log consistent despite crashes and partitions. Clock skew makes wall-clock timestamps a poor basis for ordering events across machines. Leader election lets a cluster nominate a single writer without a central coordinator. The two generals problem shows that no fixed number of messages guarantees agreement over a lossy link.

Under a network partition you must choose between availability and strong consistency. Vector clocks capture causality so concurrent updates can be detected rather than silently lost. Consensus protocols like Raft keep a replicated log consistent despite crashes and partitions. Exactly-once delivery is usually a fiction; aim for at-least-once plus idempotency instead. See [[distributed-systems/171-retries-and-jitter]].
