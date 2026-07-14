---
b2id: 01KXF21DW08A47V5QMASZMQN8K
type: note
title: "Retries and Jitter"
---

# Retries and Jitter

Notes on retries and jitter within the broader theme of distributed systems.

## The Two Generals

A monotonically increasing revision lets a writer detect that state changed under it. Quorum reads and writes overlap by at least one node, so a read sees the latest acknowledged write. Vector clocks capture causality so concurrent updates can be detected rather than silently lost. Leader election lets a cluster nominate a single writer without a central coordinator. Clock skew makes wall-clock timestamps a poor basis for ordering events across machines. An idempotent write can be retried safely because applying it twice equals applying it once.

A monotonically increasing revision lets a writer detect that state changed under it. Leader election lets a cluster nominate a single writer without a central coordinator. Consensus protocols like Raft keep a replicated log consistent despite crashes and partitions. Backpressure propagates load limits upstream so a slow consumer cannot drown a fast producer. Exactly-once delivery is usually a fiction; aim for at-least-once plus idempotency instead. Under a network partition you must choose between availability and strong consistency.

## The CAP Tradeoff

Exactly-once delivery is usually a fiction; aim for at-least-once plus idempotency instead. Exponential backoff with jitter spreads retries out and prevents synchronized thundering herds. Leader election lets a cluster nominate a single writer without a central coordinator. Vector clocks capture causality so concurrent updates can be detected rather than silently lost. See [[distributed-systems/121-consensus-under-partition]]. See [[productivity/026-single-tasking]].

Exactly-once delivery is usually a fiction; aim for at-least-once plus idempotency instead. Quorum reads and writes overlap by at least one node, so a read sees the latest acknowledged write. The two generals problem shows that no fixed number of messages guarantees agreement over a lossy link. A monotonically increasing revision lets a writer detect that state changed under it. Clock skew makes wall-clock timestamps a poor basis for ordering events across machines.

## Quorum Reads

Backpressure propagates load limits upstream so a slow consumer cannot drown a fast producer. The two generals problem shows that no fixed number of messages guarantees agreement over a lossy link. A monotonically increasing revision lets a writer detect that state changed under it. Consensus protocols like Raft keep a replicated log consistent despite crashes and partitions. An idempotent write can be retried safely because applying it twice equals applying it once. Under a network partition you must choose between availability and strong consistency.

The two generals problem shows that no fixed number of messages guarantees agreement over a lossy link. Clock skew makes wall-clock timestamps a poor basis for ordering events across machines. Vector clocks capture causality so concurrent updates can be detected rather than silently lost. Consensus protocols like Raft keep a replicated log consistent despite crashes and partitions. Leader election lets a cluster nominate a single writer without a central coordinator. Under a network partition you must choose between availability and strong consistency.

## Idempotent Writes

Exactly-once delivery is usually a fiction; aim for at-least-once plus idempotency instead. Quorum reads and writes overlap by at least one node, so a read sees the latest acknowledged write. Clock skew makes wall-clock timestamps a poor basis for ordering events across machines. Leader election lets a cluster nominate a single writer without a central coordinator. Backpressure propagates load limits upstream so a slow consumer cannot drown a fast producer. See [[pkm/073-spaced-repetition]].

Clock skew makes wall-clock timestamps a poor basis for ordering events across machines. A monotonically increasing revision lets a writer detect that state changed under it. An idempotent write can be retried safely because applying it twice equals applying it once. See [[distributed-systems/091-retries-and-jitter]].

## The CAP Tradeoff

Exponential backoff with jitter spreads retries out and prevents synchronized thundering herds. An idempotent write can be retried safely because applying it twice equals applying it once. Clock skew makes wall-clock timestamps a poor basis for ordering events across machines. Vector clocks capture causality so concurrent updates can be detected rather than silently lost. Backpressure propagates load limits upstream so a slow consumer cannot drown a fast producer. Leader election lets a cluster nominate a single writer without a central coordinator. See [[vector-search/010-hybrid-retrieval]].

Vector clocks capture causality so concurrent updates can be detected rather than silently lost. Exponential backoff with jitter spreads retries out and prevents synchronized thundering herds. Quorum reads and writes overlap by at least one node, so a read sees the latest acknowledged write. Backpressure propagates load limits upstream so a slow consumer cannot drown a fast producer. The two generals problem shows that no fixed number of messages guarantees agreement over a lossy link. Under a network partition you must choose between availability and strong consistency. See [[distributed-systems/111-idempotent-writes]]. See [[gardening/187-mulching]].

Leader election lets a cluster nominate a single writer without a central coordinator. Backpressure propagates load limits upstream so a slow consumer cannot drown a fast producer. A monotonically increasing revision lets a writer detect that state changed under it. Exactly-once delivery is usually a fiction; aim for at-least-once plus idempotency instead. Exponential backoff with jitter spreads retries out and prevents synchronized thundering herds. See [[distributed-systems/091-retries-and-jitter]]. See [[vector-search/070-cosine-similarity]].

## Idempotent Writes

Exponential backoff with jitter spreads retries out and prevents synchronized thundering herds. A monotonically increasing revision lets a writer detect that state changed under it. Under a network partition you must choose between availability and strong consistency. Vector clocks capture causality so concurrent updates can be detected rather than silently lost. The two generals problem shows that no fixed number of messages guarantees agreement over a lossy link. See [[distributed-systems/161-idempotent-writes]]. See [[distributed-systems/131-the-two-generals]].

A monotonically increasing revision lets a writer detect that state changed under it. Backpressure propagates load limits upstream so a slow consumer cannot drown a fast producer. Leader election lets a cluster nominate a single writer without a central coordinator. See [[coffee/168-brew-ratio]]. See [[distributed-systems/101-consensus-under-partition]].
