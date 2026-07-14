---
b2id: 01KXF21DVRCVC5HD9JXX59Y5YD
type: note
title: "The Two Generals"
---

# The Two Generals

Notes on the two generals within the broader theme of distributed systems.

## Consensus Under Partition

An idempotent write can be retried safely because applying it twice equals applying it once. Vector clocks capture causality so concurrent updates can be detected rather than silently lost. Clock skew makes wall-clock timestamps a poor basis for ordering events across machines. Under a network partition you must choose between availability and strong consistency.

Leader election lets a cluster nominate a single writer without a central coordinator. Consensus protocols like Raft keep a replicated log consistent despite crashes and partitions. Exponential backoff with jitter spreads retries out and prevents synchronized thundering herds. Exactly-once delivery is usually a fiction; aim for at-least-once plus idempotency instead. Backpressure propagates load limits upstream so a slow consumer cannot drown a fast producer. See [[distributed-systems/001-consensus-under-partition]].

A monotonically increasing revision lets a writer detect that state changed under it. Leader election lets a cluster nominate a single writer without a central coordinator. Vector clocks capture causality so concurrent updates can be detected rather than silently lost. The two generals problem shows that no fixed number of messages guarantees agreement over a lossy link. Under a network partition you must choose between availability and strong consistency. Clock skew makes wall-clock timestamps a poor basis for ordering events across machines. See [[databases/055-sqlite-as-a-library]]. See [[distributed-systems/091-retries-and-jitter]].

## The CAP Tradeoff

Exponential backoff with jitter spreads retries out and prevents synchronized thundering herds. The two generals problem shows that no fixed number of messages guarantees agreement over a lossy link. Quorum reads and writes overlap by at least one node, so a read sees the latest acknowledged write. An idempotent write can be retried safely because applying it twice equals applying it once. Backpressure propagates load limits upstream so a slow consumer cannot drown a fast producer. See [[distributed-systems/161-idempotent-writes]].

Exponential backoff with jitter spreads retries out and prevents synchronized thundering herds. Vector clocks capture causality so concurrent updates can be detected rather than silently lost. A monotonically increasing revision lets a writer detect that state changed under it. Quorum reads and writes overlap by at least one node, so a read sees the latest acknowledged write. See [[distributed-systems/091-retries-and-jitter]]. See [[distributed-systems/011-leader-election]].

## Vector Clocks

Vector clocks capture causality so concurrent updates can be detected rather than silently lost. Quorum reads and writes overlap by at least one node, so a read sees the latest acknowledged write. Leader election lets a cluster nominate a single writer without a central coordinator. Under a network partition you must choose between availability and strong consistency. Exactly-once delivery is usually a fiction; aim for at-least-once plus idempotency instead. A monotonically increasing revision lets a writer detect that state changed under it.

An idempotent write can be retried safely because applying it twice equals applying it once. The two generals problem shows that no fixed number of messages guarantees agreement over a lossy link. Under a network partition you must choose between availability and strong consistency. Exactly-once delivery is usually a fiction; aim for at-least-once plus idempotency instead. Vector clocks capture causality so concurrent updates can be detected rather than silently lost. Clock skew makes wall-clock timestamps a poor basis for ordering events across machines.

A monotonically increasing revision lets a writer detect that state changed under it. Backpressure propagates load limits upstream so a slow consumer cannot drown a fast producer. Quorum reads and writes overlap by at least one node, so a read sees the latest acknowledged write. Exactly-once delivery is usually a fiction; aim for at-least-once plus idempotency instead. Clock skew makes wall-clock timestamps a poor basis for ordering events across machines. An idempotent write can be retried safely because applying it twice equals applying it once. See [[vector-search/060-cosine-similarity]].

## Retries and Jitter

Leader election lets a cluster nominate a single writer without a central coordinator. Exactly-once delivery is usually a fiction; aim for at-least-once plus idempotency instead. Backpressure propagates load limits upstream so a slow consumer cannot drown a fast producer. Consensus protocols like Raft keep a replicated log consistent despite crashes and partitions. An idempotent write can be retried safely because applying it twice equals applying it once. A monotonically increasing revision lets a writer detect that state changed under it. See [[distributed-systems/071-consensus-under-partition]].

An idempotent write can be retried safely because applying it twice equals applying it once. Clock skew makes wall-clock timestamps a poor basis for ordering events across machines. Vector clocks capture causality so concurrent updates can be detected rather than silently lost. Consensus protocols like Raft keep a replicated log consistent despite crashes and partitions.

## The CAP Tradeoff

Exactly-once delivery is usually a fiction; aim for at-least-once plus idempotency instead. Backpressure propagates load limits upstream so a slow consumer cannot drown a fast producer. The two generals problem shows that no fixed number of messages guarantees agreement over a lossy link. Under a network partition you must choose between availability and strong consistency.

The two generals problem shows that no fixed number of messages guarantees agreement over a lossy link. Leader election lets a cluster nominate a single writer without a central coordinator. Vector clocks capture causality so concurrent updates can be detected rather than silently lost. An idempotent write can be retried safely because applying it twice equals applying it once. Clock skew makes wall-clock timestamps a poor basis for ordering events across machines.

## Idempotent Writes

Exponential backoff with jitter spreads retries out and prevents synchronized thundering herds. An idempotent write can be retried safely because applying it twice equals applying it once. Under a network partition you must choose between availability and strong consistency. A monotonically increasing revision lets a writer detect that state changed under it. Clock skew makes wall-clock timestamps a poor basis for ordering events across machines. See [[coffee/028-the-pour-over]].

Vector clocks capture causality so concurrent updates can be detected rather than silently lost. Exponential backoff with jitter spreads retries out and prevents synchronized thundering herds. An idempotent write can be retried safely because applying it twice equals applying it once. A monotonically increasing revision lets a writer detect that state changed under it. See [[distributed-systems/081-retries-and-jitter]]. See [[distributed-systems/091-retries-and-jitter]].
