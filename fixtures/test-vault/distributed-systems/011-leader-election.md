---
b2id: 01KXF21DVC8HZJYA6HAYJ9P1T8
type: note
title: "Leader Election"
---

# Leader Election

Notes on leader election within the broader theme of distributed systems.

## Backpressure

Exactly-once delivery is usually a fiction; aim for at-least-once plus idempotency instead. Leader election lets a cluster nominate a single writer without a central coordinator. An idempotent write can be retried safely because applying it twice equals applying it once. The two generals problem shows that no fixed number of messages guarantees agreement over a lossy link. Under a network partition you must choose between availability and strong consistency.

Clock skew makes wall-clock timestamps a poor basis for ordering events across machines. Quorum reads and writes overlap by at least one node, so a read sees the latest acknowledged write. An idempotent write can be retried safely because applying it twice equals applying it once. Leader election lets a cluster nominate a single writer without a central coordinator.

Under a network partition you must choose between availability and strong consistency. Clock skew makes wall-clock timestamps a poor basis for ordering events across machines. Exactly-once delivery is usually a fiction; aim for at-least-once plus idempotency instead. Leader election lets a cluster nominate a single writer without a central coordinator. Quorum reads and writes overlap by at least one node, so a read sees the latest acknowledged write.

## Idempotent Writes

Quorum reads and writes overlap by at least one node, so a read sees the latest acknowledged write. The two generals problem shows that no fixed number of messages guarantees agreement over a lossy link. Leader election lets a cluster nominate a single writer without a central coordinator. Clock skew makes wall-clock timestamps a poor basis for ordering events across machines. Under a network partition you must choose between availability and strong consistency. See [[distributed-systems/071-consensus-under-partition]]. See [[pkm/093-the-zettelkasten]].

A monotonically increasing revision lets a writer detect that state changed under it. Vector clocks capture causality so concurrent updates can be detected rather than silently lost. Consensus protocols like Raft keep a replicated log consistent despite crashes and partitions. Under a network partition you must choose between availability and strong consistency. An idempotent write can be retried safely because applying it twice equals applying it once. See [[distributed-systems/181-retries-and-jitter]].

## Eventual Consistency

Under a network partition you must choose between availability and strong consistency. Consensus protocols like Raft keep a replicated log consistent despite crashes and partitions. A monotonically increasing revision lets a writer detect that state changed under it. See [[distributed-systems/031-the-cap-tradeoff]].

The two generals problem shows that no fixed number of messages guarantees agreement over a lossy link. A monotonically increasing revision lets a writer detect that state changed under it. Consensus protocols like Raft keep a replicated log consistent despite crashes and partitions. Exactly-once delivery is usually a fiction; aim for at-least-once plus idempotency instead. See [[distributed-systems/121-consensus-under-partition]]. See [[coffee/178-the-espresso-shot]].

Leader election lets a cluster nominate a single writer without a central coordinator. Exponential backoff with jitter spreads retries out and prevents synchronized thundering herds. Exactly-once delivery is usually a fiction; aim for at-least-once plus idempotency instead. See [[coffee/118-extraction-yield]].

## The Two Generals

Under a network partition you must choose between availability and strong consistency. An idempotent write can be retried safely because applying it twice equals applying it once. Vector clocks capture causality so concurrent updates can be detected rather than silently lost. See [[distributed-systems/111-idempotent-writes]].

Consensus protocols like Raft keep a replicated log consistent despite crashes and partitions. A monotonically increasing revision lets a writer detect that state changed under it. Vector clocks capture causality so concurrent updates can be detected rather than silently lost. See [[distributed-systems/081-retries-and-jitter]].

An idempotent write can be retried safely because applying it twice equals applying it once. Under a network partition you must choose between availability and strong consistency. Exponential backoff with jitter spreads retries out and prevents synchronized thundering herds. Quorum reads and writes overlap by at least one node, so a read sees the latest acknowledged write. A monotonically increasing revision lets a writer detect that state changed under it. Leader election lets a cluster nominate a single writer without a central coordinator. See [[distributed-systems/131-the-two-generals]]. See [[distributed-systems/081-retries-and-jitter]].

## Vector Clocks

Quorum reads and writes overlap by at least one node, so a read sees the latest acknowledged write. Under a network partition you must choose between availability and strong consistency. The two generals problem shows that no fixed number of messages guarantees agreement over a lossy link. Vector clocks capture causality so concurrent updates can be detected rather than silently lost. See [[distributed-systems/191-retries-and-jitter]].

A monotonically increasing revision lets a writer detect that state changed under it. The two generals problem shows that no fixed number of messages guarantees agreement over a lossy link. Consensus protocols like Raft keep a replicated log consistent despite crashes and partitions. An idempotent write can be retried safely because applying it twice equals applying it once. See [[gardening/017-attracting-pollinators]].

## Retries and Jitter

An idempotent write can be retried safely because applying it twice equals applying it once. Exactly-once delivery is usually a fiction; aim for at-least-once plus idempotency instead. Quorum reads and writes overlap by at least one node, so a read sees the latest acknowledged write. Clock skew makes wall-clock timestamps a poor basis for ordering events across machines. Under a network partition you must choose between availability and strong consistency. The two generals problem shows that no fixed number of messages guarantees agreement over a lossy link. See [[distributed-systems/071-consensus-under-partition]].

Backpressure propagates load limits upstream so a slow consumer cannot drown a fast producer. Leader election lets a cluster nominate a single writer without a central coordinator. Exactly-once delivery is usually a fiction; aim for at-least-once plus idempotency instead. An idempotent write can be retried safely because applying it twice equals applying it once. Clock skew makes wall-clock timestamps a poor basis for ordering events across machines. The two generals problem shows that no fixed number of messages guarantees agreement over a lossy link.
