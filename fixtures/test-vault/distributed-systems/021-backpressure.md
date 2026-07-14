---
b2id: 01KXF21DVECTXJ1GCDQYXAGYM9
type: note
title: "Backpressure"
---

# Backpressure

Notes on backpressure within the broader theme of distributed systems.

## Backpressure

An idempotent write can be retried safely because applying it twice equals applying it once. Consensus protocols like Raft keep a replicated log consistent despite crashes and partitions. Exponential backoff with jitter spreads retries out and prevents synchronized thundering herds. A monotonically increasing revision lets a writer detect that state changed under it. Leader election lets a cluster nominate a single writer without a central coordinator.

Quorum reads and writes overlap by at least one node, so a read sees the latest acknowledged write. Clock skew makes wall-clock timestamps a poor basis for ordering events across machines. Under a network partition you must choose between availability and strong consistency. Exactly-once delivery is usually a fiction; aim for at-least-once plus idempotency instead.

Leader election lets a cluster nominate a single writer without a central coordinator. A monotonically increasing revision lets a writer detect that state changed under it. The two generals problem shows that no fixed number of messages guarantees agreement over a lossy link. Backpressure propagates load limits upstream so a slow consumer cannot drown a fast producer. Consensus protocols like Raft keep a replicated log consistent despite crashes and partitions. See [[pkm/043-spaced-repetition]]. See [[distributed-systems/141-consensus-under-partition]].

## Leader Election

Exponential backoff with jitter spreads retries out and prevents synchronized thundering herds. Backpressure propagates load limits upstream so a slow consumer cannot drown a fast producer. Quorum reads and writes overlap by at least one node, so a read sees the latest acknowledged write. Clock skew makes wall-clock timestamps a poor basis for ordering events across machines. Consensus protocols like Raft keep a replicated log consistent despite crashes and partitions. Vector clocks capture causality so concurrent updates can be detected rather than silently lost. See [[vector-search/090-chunking-strategy]]. See [[distributed-systems/081-retries-and-jitter]].

A monotonically increasing revision lets a writer detect that state changed under it. The two generals problem shows that no fixed number of messages guarantees agreement over a lossy link. An idempotent write can be retried safely because applying it twice equals applying it once. See [[distributed-systems/071-consensus-under-partition]].

Exactly-once delivery is usually a fiction; aim for at-least-once plus idempotency instead. Clock skew makes wall-clock timestamps a poor basis for ordering events across machines. Leader election lets a cluster nominate a single writer without a central coordinator. See [[distributed-systems/141-consensus-under-partition]]. See [[productivity/176-energy-management]].

## Vector Clocks

Leader election lets a cluster nominate a single writer without a central coordinator. The two generals problem shows that no fixed number of messages guarantees agreement over a lossy link. Under a network partition you must choose between availability and strong consistency. Quorum reads and writes overlap by at least one node, so a read sees the latest acknowledged write. A monotonically increasing revision lets a writer detect that state changed under it.

Quorum reads and writes overlap by at least one node, so a read sees the latest acknowledged write. Backpressure propagates load limits upstream so a slow consumer cannot drown a fast producer. Clock skew makes wall-clock timestamps a poor basis for ordering events across machines. See [[distributed-systems/031-the-cap-tradeoff]]. See [[distributed-systems/191-retries-and-jitter]].

Consensus protocols like Raft keep a replicated log consistent despite crashes and partitions. Exactly-once delivery is usually a fiction; aim for at-least-once plus idempotency instead. Clock skew makes wall-clock timestamps a poor basis for ordering events across machines. Under a network partition you must choose between availability and strong consistency. See [[productivity/156-timeboxing]]. See [[distributed-systems/031-the-cap-tradeoff]].

## Backpressure

Backpressure propagates load limits upstream so a slow consumer cannot drown a fast producer. The two generals problem shows that no fixed number of messages guarantees agreement over a lossy link. Vector clocks capture causality so concurrent updates can be detected rather than silently lost. A monotonically increasing revision lets a writer detect that state changed under it.

An idempotent write can be retried safely because applying it twice equals applying it once. Leader election lets a cluster nominate a single writer without a central coordinator. A monotonically increasing revision lets a writer detect that state changed under it. Consensus protocols like Raft keep a replicated log consistent despite crashes and partitions. See [[databases/035-schema-migrations]]. See [[distributed-systems/111-idempotent-writes]].

The two generals problem shows that no fixed number of messages guarantees agreement over a lossy link. A monotonically increasing revision lets a writer detect that state changed under it. Clock skew makes wall-clock timestamps a poor basis for ordering events across machines.
