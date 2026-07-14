---
b2id: 01KXF21DVM7D0X06QXY2KXF3XC
type: note
title: "Retries and Jitter"
---

# Retries and Jitter

Notes on retries and jitter within the broader theme of distributed systems.

## Consensus Under Partition

Exactly-once delivery is usually a fiction; aim for at-least-once plus idempotency instead. Under a network partition you must choose between availability and strong consistency. Clock skew makes wall-clock timestamps a poor basis for ordering events across machines. A monotonically increasing revision lets a writer detect that state changed under it. Vector clocks capture causality so concurrent updates can be detected rather than silently lost. The two generals problem shows that no fixed number of messages guarantees agreement over a lossy link. See [[distributed-systems/161-idempotent-writes]].

The two generals problem shows that no fixed number of messages guarantees agreement over a lossy link. Consensus protocols like Raft keep a replicated log consistent despite crashes and partitions. Vector clocks capture causality so concurrent updates can be detected rather than silently lost. Clock skew makes wall-clock timestamps a poor basis for ordering events across machines. Backpressure propagates load limits upstream so a slow consumer cannot drown a fast producer. Exponential backoff with jitter spreads retries out and prevents synchronized thundering herds. See [[distributed-systems/021-backpressure]].

Quorum reads and writes overlap by at least one node, so a read sees the latest acknowledged write. Exactly-once delivery is usually a fiction; aim for at-least-once plus idempotency instead. The two generals problem shows that no fixed number of messages guarantees agreement over a lossy link. A monotonically increasing revision lets a writer detect that state changed under it. Consensus protocols like Raft keep a replicated log consistent despite crashes and partitions. See [[distributed-systems/111-idempotent-writes]].

## Vector Clocks

Quorum reads and writes overlap by at least one node, so a read sees the latest acknowledged write. Consensus protocols like Raft keep a replicated log consistent despite crashes and partitions. Backpressure propagates load limits upstream so a slow consumer cannot drown a fast producer. Exponential backoff with jitter spreads retries out and prevents synchronized thundering herds. Leader election lets a cluster nominate a single writer without a central coordinator. Clock skew makes wall-clock timestamps a poor basis for ordering events across machines. See [[productivity/076-the-weekly-review]].

Exponential backoff with jitter spreads retries out and prevents synchronized thundering herds. Leader election lets a cluster nominate a single writer without a central coordinator. Backpressure propagates load limits upstream so a slow consumer cannot drown a fast producer. An idempotent write can be retried safely because applying it twice equals applying it once. The two generals problem shows that no fixed number of messages guarantees agreement over a lossy link. See [[distributed-systems/101-consensus-under-partition]].

Vector clocks capture causality so concurrent updates can be detected rather than silently lost. Under a network partition you must choose between availability and strong consistency. A monotonically increasing revision lets a writer detect that state changed under it. Exactly-once delivery is usually a fiction; aim for at-least-once plus idempotency instead. The two generals problem shows that no fixed number of messages guarantees agreement over a lossy link. Clock skew makes wall-clock timestamps a poor basis for ordering events across machines.

## Retries and Jitter

Quorum reads and writes overlap by at least one node, so a read sees the latest acknowledged write. Vector clocks capture causality so concurrent updates can be detected rather than silently lost. Under a network partition you must choose between availability and strong consistency. Consensus protocols like Raft keep a replicated log consistent despite crashes and partitions.

Exponential backoff with jitter spreads retries out and prevents synchronized thundering herds. Clock skew makes wall-clock timestamps a poor basis for ordering events across machines. Quorum reads and writes overlap by at least one node, so a read sees the latest acknowledged write. Leader election lets a cluster nominate a single writer without a central coordinator. See [[distributed-systems/141-consensus-under-partition]].

A monotonically increasing revision lets a writer detect that state changed under it. Backpressure propagates load limits upstream so a slow consumer cannot drown a fast producer. Vector clocks capture causality so concurrent updates can be detected rather than silently lost. Exponential backoff with jitter spreads retries out and prevents synchronized thundering herds. Exactly-once delivery is usually a fiction; aim for at-least-once plus idempotency instead. See [[vector-search/160-the-embedding-space]]. See [[vector-search/100-dense-vs-sparse]].

## Idempotent Writes

Backpressure propagates load limits upstream so a slow consumer cannot drown a fast producer. Clock skew makes wall-clock timestamps a poor basis for ordering events across machines. A monotonically increasing revision lets a writer detect that state changed under it. Leader election lets a cluster nominate a single writer without a central coordinator. An idempotent write can be retried safely because applying it twice equals applying it once. Exponential backoff with jitter spreads retries out and prevents synchronized thundering herds.

Backpressure propagates load limits upstream so a slow consumer cannot drown a fast producer. Under a network partition you must choose between availability and strong consistency. Exactly-once delivery is usually a fiction; aim for at-least-once plus idempotency instead. Exponential backoff with jitter spreads retries out and prevents synchronized thundering herds. See [[transformers/064-fine-tuning-vs-prompting]].

## Retries and Jitter

Vector clocks capture causality so concurrent updates can be detected rather than silently lost. Quorum reads and writes overlap by at least one node, so a read sees the latest acknowledged write. A monotonically increasing revision lets a writer detect that state changed under it. Consensus protocols like Raft keep a replicated log consistent despite crashes and partitions. See [[coffee/128-extraction-yield]].

A monotonically increasing revision lets a writer detect that state changed under it. Vector clocks capture causality so concurrent updates can be detected rather than silently lost. An idempotent write can be retried safely because applying it twice equals applying it once. Exponential backoff with jitter spreads retries out and prevents synchronized thundering herds. Exactly-once delivery is usually a fiction; aim for at-least-once plus idempotency instead. See [[distributed-systems/101-consensus-under-partition]]. See [[transformers/064-fine-tuning-vs-prompting]].

## Eventual Consistency

Quorum reads and writes overlap by at least one node, so a read sees the latest acknowledged write. An idempotent write can be retried safely because applying it twice equals applying it once. Backpressure propagates load limits upstream so a slow consumer cannot drown a fast producer. Exponential backoff with jitter spreads retries out and prevents synchronized thundering herds. Exactly-once delivery is usually a fiction; aim for at-least-once plus idempotency instead. See [[hiking/029-trail-etiquette]]. See [[vector-search/020-the-embedding-space]].

Consensus protocols like Raft keep a replicated log consistent despite crashes and partitions. The two generals problem shows that no fixed number of messages guarantees agreement over a lossy link. Under a network partition you must choose between availability and strong consistency. Exactly-once delivery is usually a fiction; aim for at-least-once plus idempotency instead. Backpressure propagates load limits upstream so a slow consumer cannot drown a fast producer.
