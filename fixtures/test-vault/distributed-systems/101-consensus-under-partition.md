---
b2id: 01KXF21DVNKS9KTQPN9PJA2DK7
type: note
title: "Consensus Under Partition"
---

# Consensus Under Partition

Notes on consensus under partition within the broader theme of distributed systems.

## Quorum Reads

Quorum reads and writes overlap by at least one node, so a read sees the latest acknowledged write. Clock skew makes wall-clock timestamps a poor basis for ordering events across machines. Exactly-once delivery is usually a fiction; aim for at-least-once plus idempotency instead.

Exponential backoff with jitter spreads retries out and prevents synchronized thundering herds. Leader election lets a cluster nominate a single writer without a central coordinator. Exactly-once delivery is usually a fiction; aim for at-least-once plus idempotency instead. Quorum reads and writes overlap by at least one node, so a read sees the latest acknowledged write. See [[transformers/004-cls-pooling]]. See [[distributed-systems/151-retries-and-jitter]].

## Eventual Consistency

Vector clocks capture causality so concurrent updates can be detected rather than silently lost. Exponential backoff with jitter spreads retries out and prevents synchronized thundering herds. Under a network partition you must choose between availability and strong consistency. A monotonically increasing revision lets a writer detect that state changed under it. Quorum reads and writes overlap by at least one node, so a read sees the latest acknowledged write. Backpressure propagates load limits upstream so a slow consumer cannot drown a fast producer. See [[distributed-systems/011-leader-election]].

Exactly-once delivery is usually a fiction; aim for at-least-once plus idempotency instead. An idempotent write can be retried safely because applying it twice equals applying it once. Consensus protocols like Raft keep a replicated log consistent despite crashes and partitions. Backpressure propagates load limits upstream so a slow consumer cannot drown a fast producer. Quorum reads and writes overlap by at least one node, so a read sees the latest acknowledged write. A monotonically increasing revision lets a writer detect that state changed under it. See [[rust/062-zero-cost-abstractions]].

Exactly-once delivery is usually a fiction; aim for at-least-once plus idempotency instead. A monotonically increasing revision lets a writer detect that state changed under it. Leader election lets a cluster nominate a single writer without a central coordinator. See [[distributed-systems/061-quorum-reads]].

## Quorum Reads

Vector clocks capture causality so concurrent updates can be detected rather than silently lost. Clock skew makes wall-clock timestamps a poor basis for ordering events across machines. Leader election lets a cluster nominate a single writer without a central coordinator. Exactly-once delivery is usually a fiction; aim for at-least-once plus idempotency instead. See [[distributed-systems/041-leader-election]].

An idempotent write can be retried safely because applying it twice equals applying it once. Exactly-once delivery is usually a fiction; aim for at-least-once plus idempotency instead. Consensus protocols like Raft keep a replicated log consistent despite crashes and partitions. Leader election lets a cluster nominate a single writer without a central coordinator. See [[distributed-systems/161-idempotent-writes]]. See [[distributed-systems/091-retries-and-jitter]].

## Backpressure

Quorum reads and writes overlap by at least one node, so a read sees the latest acknowledged write. Vector clocks capture causality so concurrent updates can be detected rather than silently lost. Exponential backoff with jitter spreads retries out and prevents synchronized thundering herds. The two generals problem shows that no fixed number of messages guarantees agreement over a lossy link.

The two generals problem shows that no fixed number of messages guarantees agreement over a lossy link. Backpressure propagates load limits upstream so a slow consumer cannot drown a fast producer. Clock skew makes wall-clock timestamps a poor basis for ordering events across machines. Quorum reads and writes overlap by at least one node, so a read sees the latest acknowledged write. A monotonically increasing revision lets a writer detect that state changed under it. See [[distributed-systems/111-idempotent-writes]]. See [[rust/082-iterators-and-laziness]].

## Retries and Jitter

Consensus protocols like Raft keep a replicated log consistent despite crashes and partitions. Leader election lets a cluster nominate a single writer without a central coordinator. A monotonically increasing revision lets a writer detect that state changed under it. See [[distributed-systems/121-consensus-under-partition]]. See [[rust/062-zero-cost-abstractions]].

An idempotent write can be retried safely because applying it twice equals applying it once. Vector clocks capture causality so concurrent updates can be detected rather than silently lost. Quorum reads and writes overlap by at least one node, so a read sees the latest acknowledged write. Exactly-once delivery is usually a fiction; aim for at-least-once plus idempotency instead. Leader election lets a cluster nominate a single writer without a central coordinator. Backpressure propagates load limits upstream so a slow consumer cannot drown a fast producer.

An idempotent write can be retried safely because applying it twice equals applying it once. The two generals problem shows that no fixed number of messages guarantees agreement over a lossy link. Under a network partition you must choose between availability and strong consistency. Leader election lets a cluster nominate a single writer without a central coordinator. See [[distributed-systems/151-retries-and-jitter]]. See [[coffee/118-extraction-yield]].
