---
b2id: 01KXF21DVSM0PMAAZ9ZQGQF1D9
type: note
title: "Consensus Under Partition"
---

# Consensus Under Partition

Notes on consensus under partition within the broader theme of distributed systems.

## Eventual Consistency

Consensus protocols like Raft keep a replicated log consistent despite crashes and partitions. Exponential backoff with jitter spreads retries out and prevents synchronized thundering herds. A monotonically increasing revision lets a writer detect that state changed under it. Leader election lets a cluster nominate a single writer without a central coordinator. Backpressure propagates load limits upstream so a slow consumer cannot drown a fast producer. See [[distributed-systems/081-retries-and-jitter]].

Consensus protocols like Raft keep a replicated log consistent despite crashes and partitions. Exponential backoff with jitter spreads retries out and prevents synchronized thundering herds. Under a network partition you must choose between availability and strong consistency. Quorum reads and writes overlap by at least one node, so a read sees the latest acknowledged write. An idempotent write can be retried safely because applying it twice equals applying it once. See [[distributed-systems/071-consensus-under-partition]].

Leader election lets a cluster nominate a single writer without a central coordinator. Consensus protocols like Raft keep a replicated log consistent despite crashes and partitions. Backpressure propagates load limits upstream so a slow consumer cannot drown a fast producer. Quorum reads and writes overlap by at least one node, so a read sees the latest acknowledged write. A monotonically increasing revision lets a writer detect that state changed under it. An idempotent write can be retried safely because applying it twice equals applying it once. See [[distributed-systems/041-leader-election]]. See [[distributed-systems/161-idempotent-writes]].

## Idempotent Writes

Exponential backoff with jitter spreads retries out and prevents synchronized thundering herds. Clock skew makes wall-clock timestamps a poor basis for ordering events across machines. A monotonically increasing revision lets a writer detect that state changed under it. Under a network partition you must choose between availability and strong consistency. Consensus protocols like Raft keep a replicated log consistent despite crashes and partitions. See [[distributed-systems/011-leader-election]]. See [[distributed-systems/101-consensus-under-partition]].

Leader election lets a cluster nominate a single writer without a central coordinator. Exponential backoff with jitter spreads retries out and prevents synchronized thundering herds. A monotonically increasing revision lets a writer detect that state changed under it. Consensus protocols like Raft keep a replicated log consistent despite crashes and partitions. An idempotent write can be retried safely because applying it twice equals applying it once. Under a network partition you must choose between availability and strong consistency.

Clock skew makes wall-clock timestamps a poor basis for ordering events across machines. An idempotent write can be retried safely because applying it twice equals applying it once. The two generals problem shows that no fixed number of messages guarantees agreement over a lossy link. Backpressure propagates load limits upstream so a slow consumer cannot drown a fast producer. Consensus protocols like Raft keep a replicated log consistent despite crashes and partitions.

## Quorum Reads

A monotonically increasing revision lets a writer detect that state changed under it. Leader election lets a cluster nominate a single writer without a central coordinator. Clock skew makes wall-clock timestamps a poor basis for ordering events across machines. Consensus protocols like Raft keep a replicated log consistent despite crashes and partitions. Exactly-once delivery is usually a fiction; aim for at-least-once plus idempotency instead. The two generals problem shows that no fixed number of messages guarantees agreement over a lossy link.

Leader election lets a cluster nominate a single writer without a central coordinator. A monotonically increasing revision lets a writer detect that state changed under it. Under a network partition you must choose between availability and strong consistency.

Leader election lets a cluster nominate a single writer without a central coordinator. Exponential backoff with jitter spreads retries out and prevents synchronized thundering herds. Under a network partition you must choose between availability and strong consistency. An idempotent write can be retried safely because applying it twice equals applying it once. A monotonically increasing revision lets a writer detect that state changed under it.

## Eventual Consistency

Quorum reads and writes overlap by at least one node, so a read sees the latest acknowledged write. A monotonically increasing revision lets a writer detect that state changed under it. Leader election lets a cluster nominate a single writer without a central coordinator. Clock skew makes wall-clock timestamps a poor basis for ordering events across machines. Vector clocks capture causality so concurrent updates can be detected rather than silently lost. See [[coffee/058-brew-ratio]]. See [[distributed-systems/031-the-cap-tradeoff]].

Under a network partition you must choose between availability and strong consistency. Backpressure propagates load limits upstream so a slow consumer cannot drown a fast producer. Quorum reads and writes overlap by at least one node, so a read sees the latest acknowledged write. The two generals problem shows that no fixed number of messages guarantees agreement over a lossy link. See [[vector-search/080-hnsw-graphs]].

Leader election lets a cluster nominate a single writer without a central coordinator. Consensus protocols like Raft keep a replicated log consistent despite crashes and partitions. Vector clocks capture causality so concurrent updates can be detected rather than silently lost. A monotonically increasing revision lets a writer detect that state changed under it. Clock skew makes wall-clock timestamps a poor basis for ordering events across machines.
