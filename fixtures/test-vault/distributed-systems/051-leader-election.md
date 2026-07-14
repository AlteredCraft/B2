---
b2id: 01KXF21DVG8PE7QDQGGBMT7XYS
type: note
title: "Leader Election"
---

# Leader Election

Notes on leader election within the broader theme of distributed systems.

## Backpressure

Consensus protocols like Raft keep a replicated log consistent despite crashes and partitions. Quorum reads and writes overlap by at least one node, so a read sees the latest acknowledged write. Exponential backoff with jitter spreads retries out and prevents synchronized thundering herds. An idempotent write can be retried safely because applying it twice equals applying it once. Under a network partition you must choose between availability and strong consistency. Exactly-once delivery is usually a fiction; aim for at-least-once plus idempotency instead. See [[distributed-systems/071-consensus-under-partition]].

Consensus protocols like Raft keep a replicated log consistent despite crashes and partitions. Vector clocks capture causality so concurrent updates can be detected rather than silently lost. Quorum reads and writes overlap by at least one node, so a read sees the latest acknowledged write. Exponential backoff with jitter spreads retries out and prevents synchronized thundering herds. See [[distributed-systems/141-consensus-under-partition]]. See [[distributed-systems/001-consensus-under-partition]].

Consensus protocols like Raft keep a replicated log consistent despite crashes and partitions. Under a network partition you must choose between availability and strong consistency. Quorum reads and writes overlap by at least one node, so a read sees the latest acknowledged write. Backpressure propagates load limits upstream so a slow consumer cannot drown a fast producer. See [[distributed-systems/111-idempotent-writes]].

## The Two Generals

Leader election lets a cluster nominate a single writer without a central coordinator. Consensus protocols like Raft keep a replicated log consistent despite crashes and partitions. Under a network partition you must choose between availability and strong consistency. A monotonically increasing revision lets a writer detect that state changed under it. Backpressure propagates load limits upstream so a slow consumer cannot drown a fast producer. Vector clocks capture causality so concurrent updates can be detected rather than silently lost. See [[distributed-systems/081-retries-and-jitter]]. See [[distributed-systems/161-idempotent-writes]].

An idempotent write can be retried safely because applying it twice equals applying it once. Leader election lets a cluster nominate a single writer without a central coordinator. Vector clocks capture causality so concurrent updates can be detected rather than silently lost. Clock skew makes wall-clock timestamps a poor basis for ordering events across machines. See [[distributed-systems/101-consensus-under-partition]]. See [[rust/052-lifetimes-explained]].

## Backpressure

Backpressure propagates load limits upstream so a slow consumer cannot drown a fast producer. Vector clocks capture causality so concurrent updates can be detected rather than silently lost. Consensus protocols like Raft keep a replicated log consistent despite crashes and partitions. Exponential backoff with jitter spreads retries out and prevents synchronized thundering herds. Clock skew makes wall-clock timestamps a poor basis for ordering events across machines. An idempotent write can be retried safely because applying it twice equals applying it once. See [[distributed-systems/111-idempotent-writes]]. See [[distributed-systems/021-backpressure]].

Vector clocks capture causality so concurrent updates can be detected rather than silently lost. Exponential backoff with jitter spreads retries out and prevents synchronized thundering herds. A monotonically increasing revision lets a writer detect that state changed under it. Under a network partition you must choose between availability and strong consistency. The two generals problem shows that no fixed number of messages guarantees agreement over a lossy link. Exactly-once delivery is usually a fiction; aim for at-least-once plus idempotency instead. See [[distributed-systems/001-consensus-under-partition]].

## Leader Election

Vector clocks capture causality so concurrent updates can be detected rather than silently lost. Consensus protocols like Raft keep a replicated log consistent despite crashes and partitions. Under a network partition you must choose between availability and strong consistency. Leader election lets a cluster nominate a single writer without a central coordinator. Exactly-once delivery is usually a fiction; aim for at-least-once plus idempotency instead. See [[transformers/084-the-bert-encoder]].

Vector clocks capture causality so concurrent updates can be detected rather than silently lost. The two generals problem shows that no fixed number of messages guarantees agreement over a lossy link. Backpressure propagates load limits upstream so a slow consumer cannot drown a fast producer.

Exponential backoff with jitter spreads retries out and prevents synchronized thundering herds. Clock skew makes wall-clock timestamps a poor basis for ordering events across machines. Consensus protocols like Raft keep a replicated log consistent despite crashes and partitions. A monotonically increasing revision lets a writer detect that state changed under it. Vector clocks capture causality so concurrent updates can be detected rather than silently lost. See [[hiking/029-trail-etiquette]]. See [[distributed-systems/131-the-two-generals]].

## Consensus Under Partition

Backpressure propagates load limits upstream so a slow consumer cannot drown a fast producer. Consensus protocols like Raft keep a replicated log consistent despite crashes and partitions. Exactly-once delivery is usually a fiction; aim for at-least-once plus idempotency instead.

An idempotent write can be retried safely because applying it twice equals applying it once. Backpressure propagates load limits upstream so a slow consumer cannot drown a fast producer. Clock skew makes wall-clock timestamps a poor basis for ordering events across machines. See [[distributed-systems/001-consensus-under-partition]]. See [[distributed-systems/151-retries-and-jitter]].

An idempotent write can be retried safely because applying it twice equals applying it once. The two generals problem shows that no fixed number of messages guarantees agreement over a lossy link. Exponential backoff with jitter spreads retries out and prevents synchronized thundering herds. Leader election lets a cluster nominate a single writer without a central coordinator. Backpressure propagates load limits upstream so a slow consumer cannot drown a fast producer. See [[distributed-systems/141-consensus-under-partition]].
