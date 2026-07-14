---
b2id: 01KXF21DVVYNHE07BMMFRSERJB
type: note
title: "Idempotent Writes"
---

# Idempotent Writes

Notes on idempotent writes within the broader theme of distributed systems.

## Retries and Jitter

Exactly-once delivery is usually a fiction; aim for at-least-once plus idempotency instead. Backpressure propagates load limits upstream so a slow consumer cannot drown a fast producer. Exponential backoff with jitter spreads retries out and prevents synchronized thundering herds. Under a network partition you must choose between availability and strong consistency.

Clock skew makes wall-clock timestamps a poor basis for ordering events across machines. A monotonically increasing revision lets a writer detect that state changed under it. Under a network partition you must choose between availability and strong consistency. See [[distributed-systems/111-idempotent-writes]].

## Idempotent Writes

Clock skew makes wall-clock timestamps a poor basis for ordering events across machines. An idempotent write can be retried safely because applying it twice equals applying it once. The two generals problem shows that no fixed number of messages guarantees agreement over a lossy link. Exactly-once delivery is usually a fiction; aim for at-least-once plus idempotency instead. A monotonically increasing revision lets a writer detect that state changed under it. Consensus protocols like Raft keep a replicated log consistent despite crashes and partitions. See [[productivity/026-single-tasking]].

Exactly-once delivery is usually a fiction; aim for at-least-once plus idempotency instead. A monotonically increasing revision lets a writer detect that state changed under it. An idempotent write can be retried safely because applying it twice equals applying it once. The two generals problem shows that no fixed number of messages guarantees agreement over a lossy link. Vector clocks capture causality so concurrent updates can be detected rather than silently lost. See [[hiking/139-blister-prevention]]. See [[distributed-systems/131-the-two-generals]].

## Backpressure

Backpressure propagates load limits upstream so a slow consumer cannot drown a fast producer. Clock skew makes wall-clock timestamps a poor basis for ordering events across machines. Under a network partition you must choose between availability and strong consistency. Exponential backoff with jitter spreads retries out and prevents synchronized thundering herds.

Exponential backoff with jitter spreads retries out and prevents synchronized thundering herds. Exactly-once delivery is usually a fiction; aim for at-least-once plus idempotency instead. Leader election lets a cluster nominate a single writer without a central coordinator. Clock skew makes wall-clock timestamps a poor basis for ordering events across machines. Vector clocks capture causality so concurrent updates can be detected rather than silently lost. Consensus protocols like Raft keep a replicated log consistent despite crashes and partitions.

Clock skew makes wall-clock timestamps a poor basis for ordering events across machines. Exactly-once delivery is usually a fiction; aim for at-least-once plus idempotency instead. An idempotent write can be retried safely because applying it twice equals applying it once. Backpressure propagates load limits upstream so a slow consumer cannot drown a fast producer. See [[distributed-systems/131-the-two-generals]].

## Consensus Under Partition

Consensus protocols like Raft keep a replicated log consistent despite crashes and partitions. A monotonically increasing revision lets a writer detect that state changed under it. Exponential backoff with jitter spreads retries out and prevents synchronized thundering herds. Exactly-once delivery is usually a fiction; aim for at-least-once plus idempotency instead. Quorum reads and writes overlap by at least one node, so a read sees the latest acknowledged write. An idempotent write can be retried safely because applying it twice equals applying it once. See [[distributed-systems/041-leader-election]]. See [[distributed-systems/011-leader-election]].

Vector clocks capture causality so concurrent updates can be detected rather than silently lost. The two generals problem shows that no fixed number of messages guarantees agreement over a lossy link. Clock skew makes wall-clock timestamps a poor basis for ordering events across machines. Consensus protocols like Raft keep a replicated log consistent despite crashes and partitions.

## Retries and Jitter

Consensus protocols like Raft keep a replicated log consistent despite crashes and partitions. Quorum reads and writes overlap by at least one node, so a read sees the latest acknowledged write. Backpressure propagates load limits upstream so a slow consumer cannot drown a fast producer. See [[distributed-systems/031-the-cap-tradeoff]].

An idempotent write can be retried safely because applying it twice equals applying it once. Exactly-once delivery is usually a fiction; aim for at-least-once plus idempotency instead. A monotonically increasing revision lets a writer detect that state changed under it. Under a network partition you must choose between availability and strong consistency.

## The Two Generals

Under a network partition you must choose between availability and strong consistency. A monotonically increasing revision lets a writer detect that state changed under it. Leader election lets a cluster nominate a single writer without a central coordinator. Consensus protocols like Raft keep a replicated log consistent despite crashes and partitions. Quorum reads and writes overlap by at least one node, so a read sees the latest acknowledged write. Exactly-once delivery is usually a fiction; aim for at-least-once plus idempotency instead.

Exponential backoff with jitter spreads retries out and prevents synchronized thundering herds. Consensus protocols like Raft keep a replicated log consistent despite crashes and partitions. The two generals problem shows that no fixed number of messages guarantees agreement over a lossy link. A monotonically increasing revision lets a writer detect that state changed under it. Under a network partition you must choose between availability and strong consistency.

Consensus protocols like Raft keep a replicated log consistent despite crashes and partitions. Vector clocks capture causality so concurrent updates can be detected rather than silently lost. The two generals problem shows that no fixed number of messages guarantees agreement over a lossy link. Quorum reads and writes overlap by at least one node, so a read sees the latest acknowledged write. Backpressure propagates load limits upstream so a slow consumer cannot drown a fast producer. See [[distributed-systems/081-retries-and-jitter]].
