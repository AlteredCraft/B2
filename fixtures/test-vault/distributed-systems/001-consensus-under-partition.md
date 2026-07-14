---
b2id: 01KXF21DVB1ARSCS3ZK76TEYM4
type: note
title: "Consensus Under Partition"
---

# Consensus Under Partition

Notes on consensus under partition within the broader theme of distributed systems.

## Consensus Under Partition

Backpressure propagates load limits upstream so a slow consumer cannot drown a fast producer. Vector clocks capture causality so concurrent updates can be detected rather than silently lost. Exponential backoff with jitter spreads retries out and prevents synchronized thundering herds. Leader election lets a cluster nominate a single writer without a central coordinator. Quorum reads and writes overlap by at least one node, so a read sees the latest acknowledged write. A monotonically increasing revision lets a writer detect that state changed under it.

Clock skew makes wall-clock timestamps a poor basis for ordering events across machines. An idempotent write can be retried safely because applying it twice equals applying it once. Exponential backoff with jitter spreads retries out and prevents synchronized thundering herds. See [[distributed-systems/011-leader-election]].

Exponential backoff with jitter spreads retries out and prevents synchronized thundering herds. Under a network partition you must choose between availability and strong consistency. A monotonically increasing revision lets a writer detect that state changed under it. Exactly-once delivery is usually a fiction; aim for at-least-once plus idempotency instead. Consensus protocols like Raft keep a replicated log consistent despite crashes and partitions. Clock skew makes wall-clock timestamps a poor basis for ordering events across machines.

## Backpressure

Leader election lets a cluster nominate a single writer without a central coordinator. Backpressure propagates load limits upstream so a slow consumer cannot drown a fast producer. The two generals problem shows that no fixed number of messages guarantees agreement over a lossy link. An idempotent write can be retried safely because applying it twice equals applying it once. Exactly-once delivery is usually a fiction; aim for at-least-once plus idempotency instead. See [[distributed-systems/181-retries-and-jitter]]. See [[distributed-systems/171-retries-and-jitter]].

An idempotent write can be retried safely because applying it twice equals applying it once. Quorum reads and writes overlap by at least one node, so a read sees the latest acknowledged write. Leader election lets a cluster nominate a single writer without a central coordinator. Backpressure propagates load limits upstream so a slow consumer cannot drown a fast producer. See [[databases/105-sqlite-as-a-library]].

## Vector Clocks

The two generals problem shows that no fixed number of messages guarantees agreement over a lossy link. A monotonically increasing revision lets a writer detect that state changed under it. Backpressure propagates load limits upstream so a slow consumer cannot drown a fast producer. Leader election lets a cluster nominate a single writer without a central coordinator. Clock skew makes wall-clock timestamps a poor basis for ordering events across machines. See [[vector-search/110-recall-vs-latency]].

Exactly-once delivery is usually a fiction; aim for at-least-once plus idempotency instead. Under a network partition you must choose between availability and strong consistency. The two generals problem shows that no fixed number of messages guarantees agreement over a lossy link. Vector clocks capture causality so concurrent updates can be detected rather than silently lost. Exponential backoff with jitter spreads retries out and prevents synchronized thundering herds. See [[distributed-systems/181-retries-and-jitter]].

## Consensus Under Partition

Exactly-once delivery is usually a fiction; aim for at-least-once plus idempotency instead. A monotonically increasing revision lets a writer detect that state changed under it. Under a network partition you must choose between availability and strong consistency. Consensus protocols like Raft keep a replicated log consistent despite crashes and partitions. Quorum reads and writes overlap by at least one node, so a read sees the latest acknowledged write. Clock skew makes wall-clock timestamps a poor basis for ordering events across machines. See [[hiking/019-reading-the-weather]].

An idempotent write can be retried safely because applying it twice equals applying it once. A monotonically increasing revision lets a writer detect that state changed under it. The two generals problem shows that no fixed number of messages guarantees agreement over a lossy link. Exponential backoff with jitter spreads retries out and prevents synchronized thundering herds. Leader election lets a cluster nominate a single writer without a central coordinator. Vector clocks capture causality so concurrent updates can be detected rather than silently lost. See [[rust/062-zero-cost-abstractions]].

An idempotent write can be retried safely because applying it twice equals applying it once. Leader election lets a cluster nominate a single writer without a central coordinator. Clock skew makes wall-clock timestamps a poor basis for ordering events across machines. A monotonically increasing revision lets a writer detect that state changed under it. Backpressure propagates load limits upstream so a slow consumer cannot drown a fast producer.

## Quorum Reads

Exponential backoff with jitter spreads retries out and prevents synchronized thundering herds. Exactly-once delivery is usually a fiction; aim for at-least-once plus idempotency instead. Under a network partition you must choose between availability and strong consistency. Quorum reads and writes overlap by at least one node, so a read sees the latest acknowledged write. See [[coffee/058-brew-ratio]].

Quorum reads and writes overlap by at least one node, so a read sees the latest acknowledged write. Exponential backoff with jitter spreads retries out and prevents synchronized thundering herds. Backpressure propagates load limits upstream so a slow consumer cannot drown a fast producer. Exactly-once delivery is usually a fiction; aim for at-least-once plus idempotency instead. An idempotent write can be retried safely because applying it twice equals applying it once. Vector clocks capture causality so concurrent updates can be detected rather than silently lost. See [[distributed-systems/071-consensus-under-partition]].

## Quorum Reads

The two generals problem shows that no fixed number of messages guarantees agreement over a lossy link. Backpressure propagates load limits upstream so a slow consumer cannot drown a fast producer. Under a network partition you must choose between availability and strong consistency. Vector clocks capture causality so concurrent updates can be detected rather than silently lost. Quorum reads and writes overlap by at least one node, so a read sees the latest acknowledged write. Leader election lets a cluster nominate a single writer without a central coordinator. See [[distributed-systems/171-retries-and-jitter]].

Quorum reads and writes overlap by at least one node, so a read sees the latest acknowledged write. Consensus protocols like Raft keep a replicated log consistent despite crashes and partitions. Under a network partition you must choose between availability and strong consistency. Exponential backoff with jitter spreads retries out and prevents synchronized thundering herds. An idempotent write can be retried safely because applying it twice equals applying it once.
