---
b2id: 01KXF21DVW6DS0HM74AK5XGHRN
type: note
title: "Retries and Jitter"
---

# Retries and Jitter

Notes on retries and jitter within the broader theme of distributed systems.

## Vector Clocks

Quorum reads and writes overlap by at least one node, so a read sees the latest acknowledged write. Under a network partition you must choose between availability and strong consistency. Exactly-once delivery is usually a fiction; aim for at-least-once plus idempotency instead. Leader election lets a cluster nominate a single writer without a central coordinator. Vector clocks capture causality so concurrent updates can be detected rather than silently lost. Backpressure propagates load limits upstream so a slow consumer cannot drown a fast producer.

Exactly-once delivery is usually a fiction; aim for at-least-once plus idempotency instead. Exponential backoff with jitter spreads retries out and prevents synchronized thundering herds. The two generals problem shows that no fixed number of messages guarantees agreement over a lossy link. Clock skew makes wall-clock timestamps a poor basis for ordering events across machines. Consensus protocols like Raft keep a replicated log consistent despite crashes and partitions.

## Quorum Reads

Consensus protocols like Raft keep a replicated log consistent despite crashes and partitions. Backpressure propagates load limits upstream so a slow consumer cannot drown a fast producer. Vector clocks capture causality so concurrent updates can be detected rather than silently lost. Leader election lets a cluster nominate a single writer without a central coordinator.

An idempotent write can be retried safely because applying it twice equals applying it once. Vector clocks capture causality so concurrent updates can be detected rather than silently lost. Quorum reads and writes overlap by at least one node, so a read sees the latest acknowledged write. Exactly-once delivery is usually a fiction; aim for at-least-once plus idempotency instead. Backpressure propagates load limits upstream so a slow consumer cannot drown a fast producer. See [[distributed-systems/131-the-two-generals]]. See [[coffee/098-freshness-and-degassing]].

## Leader Election

Clock skew makes wall-clock timestamps a poor basis for ordering events across machines. The two generals problem shows that no fixed number of messages guarantees agreement over a lossy link. Exponential backoff with jitter spreads retries out and prevents synchronized thundering herds. An idempotent write can be retried safely because applying it twice equals applying it once. A monotonically increasing revision lets a writer detect that state changed under it. Backpressure propagates load limits upstream so a slow consumer cannot drown a fast producer.

Leader election lets a cluster nominate a single writer without a central coordinator. A monotonically increasing revision lets a writer detect that state changed under it. Exactly-once delivery is usually a fiction; aim for at-least-once plus idempotency instead. Backpressure propagates load limits upstream so a slow consumer cannot drown a fast producer. Clock skew makes wall-clock timestamps a poor basis for ordering events across machines. Consensus protocols like Raft keep a replicated log consistent despite crashes and partitions. See [[distributed-systems/111-idempotent-writes]]. See [[vector-search/020-the-embedding-space]].

Backpressure propagates load limits upstream so a slow consumer cannot drown a fast producer. Leader election lets a cluster nominate a single writer without a central coordinator. Under a network partition you must choose between availability and strong consistency. See [[distributed-systems/051-leader-election]].

## Quorum Reads

Backpressure propagates load limits upstream so a slow consumer cannot drown a fast producer. Clock skew makes wall-clock timestamps a poor basis for ordering events across machines. Consensus protocols like Raft keep a replicated log consistent despite crashes and partitions. The two generals problem shows that no fixed number of messages guarantees agreement over a lossy link.

The two generals problem shows that no fixed number of messages guarantees agreement over a lossy link. An idempotent write can be retried safely because applying it twice equals applying it once. Consensus protocols like Raft keep a replicated log consistent despite crashes and partitions. Vector clocks capture causality so concurrent updates can be detected rather than silently lost. Exactly-once delivery is usually a fiction; aim for at-least-once plus idempotency instead. Under a network partition you must choose between availability and strong consistency.

## The Two Generals

An idempotent write can be retried safely because applying it twice equals applying it once. Exponential backoff with jitter spreads retries out and prevents synchronized thundering herds. Exactly-once delivery is usually a fiction; aim for at-least-once plus idempotency instead. Vector clocks capture causality so concurrent updates can be detected rather than silently lost. See [[distributed-systems/071-consensus-under-partition]]. See [[distributed-systems/081-retries-and-jitter]].

Clock skew makes wall-clock timestamps a poor basis for ordering events across machines. Under a network partition you must choose between availability and strong consistency. A monotonically increasing revision lets a writer detect that state changed under it. Vector clocks capture causality so concurrent updates can be detected rather than silently lost. Quorum reads and writes overlap by at least one node, so a read sees the latest acknowledged write. See [[distributed-systems/131-the-two-generals]].

Exactly-once delivery is usually a fiction; aim for at-least-once plus idempotency instead. Exponential backoff with jitter spreads retries out and prevents synchronized thundering herds. Backpressure propagates load limits upstream so a slow consumer cannot drown a fast producer. Quorum reads and writes overlap by at least one node, so a read sees the latest acknowledged write. See [[transformers/144-the-feed-forward-block]]. See [[distributed-systems/021-backpressure]].

## Backpressure

An idempotent write can be retried safely because applying it twice equals applying it once. A monotonically increasing revision lets a writer detect that state changed under it. The two generals problem shows that no fixed number of messages guarantees agreement over a lossy link. Quorum reads and writes overlap by at least one node, so a read sees the latest acknowledged write. Consensus protocols like Raft keep a replicated log consistent despite crashes and partitions. Vector clocks capture causality so concurrent updates can be detected rather than silently lost. See [[pkm/003-local-first-vaults]].

Vector clocks capture causality so concurrent updates can be detected rather than silently lost. The two generals problem shows that no fixed number of messages guarantees agreement over a lossy link. Consensus protocols like Raft keep a replicated log consistent despite crashes and partitions. An idempotent write can be retried safely because applying it twice equals applying it once. See [[gardening/067-overwintering]].
