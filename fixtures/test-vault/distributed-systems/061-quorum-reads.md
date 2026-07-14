---
b2id: 01KXF21DVJGSA5QSHRPKC9407R
type: note
title: "Quorum Reads"
---

# Quorum Reads

Notes on quorum reads within the broader theme of distributed systems.

## The CAP Tradeoff

Consensus protocols like Raft keep a replicated log consistent despite crashes and partitions. A monotonically increasing revision lets a writer detect that state changed under it. Exactly-once delivery is usually a fiction; aim for at-least-once plus idempotency instead. Backpressure propagates load limits upstream so a slow consumer cannot drown a fast producer. See [[distributed-systems/051-leader-election]].

Exponential backoff with jitter spreads retries out and prevents synchronized thundering herds. Quorum reads and writes overlap by at least one node, so a read sees the latest acknowledged write. Leader election lets a cluster nominate a single writer without a central coordinator. Exactly-once delivery is usually a fiction; aim for at-least-once plus idempotency instead. See [[pkm/033-local-first-vaults]]. See [[coffee/148-single-origin-beans]].

Clock skew makes wall-clock timestamps a poor basis for ordering events across machines. Exactly-once delivery is usually a fiction; aim for at-least-once plus idempotency instead. Vector clocks capture causality so concurrent updates can be detected rather than silently lost. An idempotent write can be retried safely because applying it twice equals applying it once. Consensus protocols like Raft keep a replicated log consistent despite crashes and partitions. Exponential backoff with jitter spreads retries out and prevents synchronized thundering herds. See [[distributed-systems/081-retries-and-jitter]]. See [[distributed-systems/121-consensus-under-partition]].

## Eventual Consistency

An idempotent write can be retried safely because applying it twice equals applying it once. Exponential backoff with jitter spreads retries out and prevents synchronized thundering herds. Under a network partition you must choose between availability and strong consistency. Quorum reads and writes overlap by at least one node, so a read sees the latest acknowledged write.

Quorum reads and writes overlap by at least one node, so a read sees the latest acknowledged write. Consensus protocols like Raft keep a replicated log consistent despite crashes and partitions. A monotonically increasing revision lets a writer detect that state changed under it. An idempotent write can be retried safely because applying it twice equals applying it once. Under a network partition you must choose between availability and strong consistency. Clock skew makes wall-clock timestamps a poor basis for ordering events across machines.

Exponential backoff with jitter spreads retries out and prevents synchronized thundering herds. Backpressure propagates load limits upstream so a slow consumer cannot drown a fast producer. Under a network partition you must choose between availability and strong consistency. Leader election lets a cluster nominate a single writer without a central coordinator. An idempotent write can be retried safely because applying it twice equals applying it once. Consensus protocols like Raft keep a replicated log consistent despite crashes and partitions. See [[rust/042-interior-mutability]].

## Consensus Under Partition

Exactly-once delivery is usually a fiction; aim for at-least-once plus idempotency instead. Leader election lets a cluster nominate a single writer without a central coordinator. Quorum reads and writes overlap by at least one node, so a read sees the latest acknowledged write. Under a network partition you must choose between availability and strong consistency. An idempotent write can be retried safely because applying it twice equals applying it once. See [[distributed-systems/181-retries-and-jitter]].

Exponential backoff with jitter spreads retries out and prevents synchronized thundering herds. An idempotent write can be retried safely because applying it twice equals applying it once. Leader election lets a cluster nominate a single writer without a central coordinator. The two generals problem shows that no fixed number of messages guarantees agreement over a lossy link. Exactly-once delivery is usually a fiction; aim for at-least-once plus idempotency instead. Clock skew makes wall-clock timestamps a poor basis for ordering events across machines. See [[gardening/117-mulching]]. See [[distributed-systems/041-leader-election]].
