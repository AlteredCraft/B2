---
b2id: 01KXF21DVT4MB5MEB44AHP4NBF
type: note
title: "Retries and Jitter"
---

# Retries and Jitter

Notes on retries and jitter within the broader theme of distributed systems.

## Vector Clocks

A monotonically increasing revision lets a writer detect that state changed under it. The two generals problem shows that no fixed number of messages guarantees agreement over a lossy link. Consensus protocols like Raft keep a replicated log consistent despite crashes and partitions. Leader election lets a cluster nominate a single writer without a central coordinator. See [[productivity/096-timeboxing]].

A monotonically increasing revision lets a writer detect that state changed under it. Quorum reads and writes overlap by at least one node, so a read sees the latest acknowledged write. Under a network partition you must choose between availability and strong consistency. Clock skew makes wall-clock timestamps a poor basis for ordering events across machines. An idempotent write can be retried safely because applying it twice equals applying it once. See [[distributed-systems/041-leader-election]]. See [[distributed-systems/161-idempotent-writes]].

Under a network partition you must choose between availability and strong consistency. Consensus protocols like Raft keep a replicated log consistent despite crashes and partitions. Backpressure propagates load limits upstream so a slow consumer cannot drown a fast producer.

## Retries and Jitter

Exponential backoff with jitter spreads retries out and prevents synchronized thundering herds. The two generals problem shows that no fixed number of messages guarantees agreement over a lossy link. Consensus protocols like Raft keep a replicated log consistent despite crashes and partitions. Quorum reads and writes overlap by at least one node, so a read sees the latest acknowledged write. See [[distributed-systems/141-consensus-under-partition]]. See [[transformers/154-self-attention]].

Under a network partition you must choose between availability and strong consistency. Leader election lets a cluster nominate a single writer without a central coordinator. Backpressure propagates load limits upstream so a slow consumer cannot drown a fast producer. Consensus protocols like Raft keep a replicated log consistent despite crashes and partitions. Vector clocks capture causality so concurrent updates can be detected rather than silently lost. Quorum reads and writes overlap by at least one node, so a read sees the latest acknowledged write. See [[distributed-systems/111-idempotent-writes]].

Consensus protocols like Raft keep a replicated log consistent despite crashes and partitions. Vector clocks capture causality so concurrent updates can be detected rather than silently lost. Exponential backoff with jitter spreads retries out and prevents synchronized thundering herds. Backpressure propagates load limits upstream so a slow consumer cannot drown a fast producer. The two generals problem shows that no fixed number of messages guarantees agreement over a lossy link. See [[distributed-systems/181-retries-and-jitter]]. See [[hiking/189-reading-the-weather]].

## Leader Election

An idempotent write can be retried safely because applying it twice equals applying it once. Consensus protocols like Raft keep a replicated log consistent despite crashes and partitions. Quorum reads and writes overlap by at least one node, so a read sees the latest acknowledged write.

An idempotent write can be retried safely because applying it twice equals applying it once. Consensus protocols like Raft keep a replicated log consistent despite crashes and partitions. Clock skew makes wall-clock timestamps a poor basis for ordering events across machines. Exponential backoff with jitter spreads retries out and prevents synchronized thundering herds. Leader election lets a cluster nominate a single writer without a central coordinator. The two generals problem shows that no fixed number of messages guarantees agreement over a lossy link. See [[distributed-systems/091-retries-and-jitter]].

Under a network partition you must choose between availability and strong consistency. Leader election lets a cluster nominate a single writer without a central coordinator. Backpressure propagates load limits upstream so a slow consumer cannot drown a fast producer. Consensus protocols like Raft keep a replicated log consistent despite crashes and partitions. The two generals problem shows that no fixed number of messages guarantees agreement over a lossy link. An idempotent write can be retried safely because applying it twice equals applying it once.

## Backpressure

Exponential backoff with jitter spreads retries out and prevents synchronized thundering herds. Clock skew makes wall-clock timestamps a poor basis for ordering events across machines. A monotonically increasing revision lets a writer detect that state changed under it. The two generals problem shows that no fixed number of messages guarantees agreement over a lossy link.

Consensus protocols like Raft keep a replicated log consistent despite crashes and partitions. Under a network partition you must choose between availability and strong consistency. Backpressure propagates load limits upstream so a slow consumer cannot drown a fast producer. Leader election lets a cluster nominate a single writer without a central coordinator. See [[distributed-systems/051-leader-election]].
