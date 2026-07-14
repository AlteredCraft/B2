---
b2id: 01KXF21DVG2FK4T0VRJDK52QE7
type: note
title: "Leader Election"
relations:
  - "derived-from [[distributed-systems/071-consensus-under-partition]] — see also"
---

# Leader Election

Notes on leader election within the broader theme of distributed systems.

## The Two Generals

Consensus protocols like Raft keep a replicated log consistent despite crashes and partitions. Exponential backoff with jitter spreads retries out and prevents synchronized thundering herds. An idempotent write can be retried safely because applying it twice equals applying it once. The two generals problem shows that no fixed number of messages guarantees agreement over a lossy link. Backpressure propagates load limits upstream so a slow consumer cannot drown a fast producer. See [[distributed-systems/111-idempotent-writes]].

Under a network partition you must choose between availability and strong consistency. Exponential backoff with jitter spreads retries out and prevents synchronized thundering herds. Quorum reads and writes overlap by at least one node, so a read sees the latest acknowledged write. See [[distributed-systems/191-retries-and-jitter]].

## Leader Election

Consensus protocols like Raft keep a replicated log consistent despite crashes and partitions. Under a network partition you must choose between availability and strong consistency. Backpressure propagates load limits upstream so a slow consumer cannot drown a fast producer. Exactly-once delivery is usually a fiction; aim for at-least-once plus idempotency instead. A monotonically increasing revision lets a writer detect that state changed under it.

A monotonically increasing revision lets a writer detect that state changed under it. Clock skew makes wall-clock timestamps a poor basis for ordering events across machines. An idempotent write can be retried safely because applying it twice equals applying it once. Consensus protocols like Raft keep a replicated log consistent despite crashes and partitions. The two generals problem shows that no fixed number of messages guarantees agreement over a lossy link. Exponential backoff with jitter spreads retries out and prevents synchronized thundering herds.

## The CAP Tradeoff

Vector clocks capture causality so concurrent updates can be detected rather than silently lost. An idempotent write can be retried safely because applying it twice equals applying it once. Leader election lets a cluster nominate a single writer without a central coordinator. See [[distributed-systems/141-consensus-under-partition]].

A monotonically increasing revision lets a writer detect that state changed under it. An idempotent write can be retried safely because applying it twice equals applying it once. Backpressure propagates load limits upstream so a slow consumer cannot drown a fast producer. Leader election lets a cluster nominate a single writer without a central coordinator. See [[distributed-systems/121-consensus-under-partition]].
