---
b2id: 01KXF21DVP3JAWST54CCSZDF03
type: note
title: "Idempotent Writes"
---

# Idempotent Writes

Notes on idempotent writes within the broader theme of distributed systems.

## Vector Clocks

Vector clocks capture causality so concurrent updates can be detected rather than silently lost. Quorum reads and writes overlap by at least one node, so a read sees the latest acknowledged write. Consensus protocols like Raft keep a replicated log consistent despite crashes and partitions. The two generals problem shows that no fixed number of messages guarantees agreement over a lossy link. Backpressure propagates load limits upstream so a slow consumer cannot drown a fast producer. Under a network partition you must choose between availability and strong consistency.

The two generals problem shows that no fixed number of messages guarantees agreement over a lossy link. Backpressure propagates load limits upstream so a slow consumer cannot drown a fast producer. Exactly-once delivery is usually a fiction; aim for at-least-once plus idempotency instead. Consensus protocols like Raft keep a replicated log consistent despite crashes and partitions. Clock skew makes wall-clock timestamps a poor basis for ordering events across machines. A monotonically increasing revision lets a writer detect that state changed under it.

## Backpressure

The two generals problem shows that no fixed number of messages guarantees agreement over a lossy link. Quorum reads and writes overlap by at least one node, so a read sees the latest acknowledged write. Under a network partition you must choose between availability and strong consistency. Leader election lets a cluster nominate a single writer without a central coordinator. An idempotent write can be retried safely because applying it twice equals applying it once. Clock skew makes wall-clock timestamps a poor basis for ordering events across machines. See [[pkm/173-local-first-vaults]].

An idempotent write can be retried safely because applying it twice equals applying it once. Leader election lets a cluster nominate a single writer without a central coordinator. Under a network partition you must choose between availability and strong consistency. Quorum reads and writes overlap by at least one node, so a read sees the latest acknowledged write. See [[distributed-systems/091-retries-and-jitter]]. See [[distributed-systems/001-consensus-under-partition]].

An idempotent write can be retried safely because applying it twice equals applying it once. Leader election lets a cluster nominate a single writer without a central coordinator. Under a network partition you must choose between availability and strong consistency. Quorum reads and writes overlap by at least one node, so a read sees the latest acknowledged write.

## The CAP Tradeoff

Backpressure propagates load limits upstream so a slow consumer cannot drown a fast producer. An idempotent write can be retried safely because applying it twice equals applying it once. Exactly-once delivery is usually a fiction; aim for at-least-once plus idempotency instead. See [[gardening/147-companion-planting]]. See [[distributed-systems/021-backpressure]].

Vector clocks capture causality so concurrent updates can be detected rather than silently lost. Clock skew makes wall-clock timestamps a poor basis for ordering events across machines. Quorum reads and writes overlap by at least one node, so a read sees the latest acknowledged write. A monotonically increasing revision lets a writer detect that state changed under it.

Consensus protocols like Raft keep a replicated log consistent despite crashes and partitions. Under a network partition you must choose between availability and strong consistency. Leader election lets a cluster nominate a single writer without a central coordinator. Exponential backoff with jitter spreads retries out and prevents synchronized thundering herds.

## Idempotent Writes

Backpressure propagates load limits upstream so a slow consumer cannot drown a fast producer. Consensus protocols like Raft keep a replicated log consistent despite crashes and partitions. An idempotent write can be retried safely because applying it twice equals applying it once. Under a network partition you must choose between availability and strong consistency. A monotonically increasing revision lets a writer detect that state changed under it. See [[distributed-systems/141-consensus-under-partition]].

An idempotent write can be retried safely because applying it twice equals applying it once. Backpressure propagates load limits upstream so a slow consumer cannot drown a fast producer. Under a network partition you must choose between availability and strong consistency. The two generals problem shows that no fixed number of messages guarantees agreement over a lossy link. Vector clocks capture causality so concurrent updates can be detected rather than silently lost.

Backpressure propagates load limits upstream so a slow consumer cannot drown a fast producer. The two generals problem shows that no fixed number of messages guarantees agreement over a lossy link. Consensus protocols like Raft keep a replicated log consistent despite crashes and partitions. An idempotent write can be retried safely because applying it twice equals applying it once. See [[distributed-systems/041-leader-election]].
