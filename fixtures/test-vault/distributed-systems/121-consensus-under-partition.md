---
b2id: 01KXF21DVQZV69JDZ003H055D7
type: note
title: "Consensus Under Partition"
b2_relations:
  - "elaborates [[distributed-systems/081-retries-and-jitter]] — see also"
---

# Consensus Under Partition

Notes on consensus under partition within the broader theme of distributed systems.

## Quorum Reads

Consensus protocols like Raft keep a replicated log consistent despite crashes and partitions. Exactly-once delivery is usually a fiction; aim for at-least-once plus idempotency instead. An idempotent write can be retried safely because applying it twice equals applying it once. Backpressure propagates load limits upstream so a slow consumer cannot drown a fast producer. A monotonically increasing revision lets a writer detect that state changed under it.

The two generals problem shows that no fixed number of messages guarantees agreement over a lossy link. A monotonically increasing revision lets a writer detect that state changed under it. Clock skew makes wall-clock timestamps a poor basis for ordering events across machines. Vector clocks capture causality so concurrent updates can be detected rather than silently lost. See [[gardening/147-companion-planting]]. See [[distributed-systems/041-leader-election]].

## Eventual Consistency

The two generals problem shows that no fixed number of messages guarantees agreement over a lossy link. Clock skew makes wall-clock timestamps a poor basis for ordering events across machines. Leader election lets a cluster nominate a single writer without a central coordinator. Exponential backoff with jitter spreads retries out and prevents synchronized thundering herds. See [[rust/182-send-and-sync]].

An idempotent write can be retried safely because applying it twice equals applying it once. Consensus protocols like Raft keep a replicated log consistent despite crashes and partitions. Exponential backoff with jitter spreads retries out and prevents synchronized thundering herds. Vector clocks capture causality so concurrent updates can be detected rather than silently lost. See [[distributed-systems/041-leader-election]]. See [[distributed-systems/161-idempotent-writes]].

## Consensus Under Partition

Under a network partition you must choose between availability and strong consistency. Clock skew makes wall-clock timestamps a poor basis for ordering events across machines. Consensus protocols like Raft keep a replicated log consistent despite crashes and partitions. Quorum reads and writes overlap by at least one node, so a read sees the latest acknowledged write. An idempotent write can be retried safely because applying it twice equals applying it once. See [[distributed-systems/011-leader-election]]. See [[distributed-systems/021-backpressure]].

Vector clocks capture causality so concurrent updates can be detected rather than silently lost. Exponential backoff with jitter spreads retries out and prevents synchronized thundering herds. An idempotent write can be retried safely because applying it twice equals applying it once. See [[distributed-systems/191-retries-and-jitter]].

An idempotent write can be retried safely because applying it twice equals applying it once. Exactly-once delivery is usually a fiction; aim for at-least-once plus idempotency instead. Leader election lets a cluster nominate a single writer without a central coordinator. Exponential backoff with jitter spreads retries out and prevents synchronized thundering herds. See [[distributed-systems/171-retries-and-jitter]]. See [[distributed-systems/191-retries-and-jitter]].

## The CAP Tradeoff

Consensus protocols like Raft keep a replicated log consistent despite crashes and partitions. Quorum reads and writes overlap by at least one node, so a read sees the latest acknowledged write. Vector clocks capture causality so concurrent updates can be detected rather than silently lost. Exponential backoff with jitter spreads retries out and prevents synchronized thundering herds. The two generals problem shows that no fixed number of messages guarantees agreement over a lossy link. See [[distributed-systems/151-retries-and-jitter]].

Quorum reads and writes overlap by at least one node, so a read sees the latest acknowledged write. Under a network partition you must choose between availability and strong consistency. An idempotent write can be retried safely because applying it twice equals applying it once. A monotonically increasing revision lets a writer detect that state changed under it. Consensus protocols like Raft keep a replicated log consistent despite crashes and partitions.

## Retries and Jitter

Exactly-once delivery is usually a fiction; aim for at-least-once plus idempotency instead. Backpressure propagates load limits upstream so a slow consumer cannot drown a fast producer. Under a network partition you must choose between availability and strong consistency. See [[distributed-systems/101-consensus-under-partition]]. See [[distributed-systems/151-retries-and-jitter]].

Backpressure propagates load limits upstream so a slow consumer cannot drown a fast producer. Exactly-once delivery is usually a fiction; aim for at-least-once plus idempotency instead. An idempotent write can be retried safely because applying it twice equals applying it once. A monotonically increasing revision lets a writer detect that state changed under it. Clock skew makes wall-clock timestamps a poor basis for ordering events across machines. See [[distributed-systems/141-consensus-under-partition]]. See [[hiking/119-switchbacks]].

Under a network partition you must choose between availability and strong consistency. Exponential backoff with jitter spreads retries out and prevents synchronized thundering herds. Quorum reads and writes overlap by at least one node, so a read sees the latest acknowledged write. Clock skew makes wall-clock timestamps a poor basis for ordering events across machines. Consensus protocols like Raft keep a replicated log consistent despite crashes and partitions. The two generals problem shows that no fixed number of messages guarantees agreement over a lossy link. See [[distributed-systems/171-retries-and-jitter]]. See [[vector-search/050-approximate-nearest-neighbors]].

## Leader Election

Leader election lets a cluster nominate a single writer without a central coordinator. Vector clocks capture causality so concurrent updates can be detected rather than silently lost. Consensus protocols like Raft keep a replicated log consistent despite crashes and partitions. An idempotent write can be retried safely because applying it twice equals applying it once. Exactly-once delivery is usually a fiction; aim for at-least-once plus idempotency instead. See [[distributed-systems/171-retries-and-jitter]]. See [[distributed-systems/131-the-two-generals]].

Consensus protocols like Raft keep a replicated log consistent despite crashes and partitions. Clock skew makes wall-clock timestamps a poor basis for ordering events across machines. Quorum reads and writes overlap by at least one node, so a read sees the latest acknowledged write. Leader election lets a cluster nominate a single writer without a central coordinator. Under a network partition you must choose between availability and strong consistency.

Exactly-once delivery is usually a fiction; aim for at-least-once plus idempotency instead. An idempotent write can be retried safely because applying it twice equals applying it once. The two generals problem shows that no fixed number of messages guarantees agreement over a lossy link. Quorum reads and writes overlap by at least one node, so a read sees the latest acknowledged write. Clock skew makes wall-clock timestamps a poor basis for ordering events across machines. Vector clocks capture causality so concurrent updates can be detected rather than silently lost. See [[distributed-systems/101-consensus-under-partition]].
