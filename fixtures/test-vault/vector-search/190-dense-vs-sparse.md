---
b2id: 01KXF21DZKG54VMBMJNQJ5P6N5
type: note
title: "Dense vs Sparse"
b2_relations:
  - "supports [[vector-search/080-hnsw-graphs]] — see also"
---

# Dense vs Sparse

Notes on dense vs sparse within the broader theme of vector search.

## Approximate Nearest Neighbors

A reranker rescouring the top candidates with a cross-encoder usually beats a bigger index. Query and document embeddings can be asymmetric when the query carries a retrieval instruction. Cosine similarity ranks candidates by the angle between their normalized embeddings. Chunk size is the quiet lever: too small and you embed noise, too large and you blur the signal. Normalizing to unit length lets you rank by cosine using a plain L2 distance. See [[productivity/026-single-tasking]].

Normalizing to unit length lets you rank by cosine using a plain L2 distance. Cosine similarity ranks candidates by the angle between their normalized embeddings. Chunk size is the quiet lever: too small and you embed noise, too large and you blur the signal. Hybrid search fuses lexical BM25 scores with dense similarity, often via reciprocal rank fusion. See [[vector-search/080-hnsw-graphs]].

Normalizing to unit length lets you rank by cosine using a plain L2 distance. Recall@k measures how many of the true neighbors survive the approximate search. Product quantization compresses vectors so millions fit in memory at the cost of some precision. Query and document embeddings can be asymmetric when the query carries a retrieval instruction. A reranker rescouring the top candidates with a cross-encoder usually beats a bigger index. Cosine similarity ranks candidates by the angle between their normalized embeddings.

## Approximate Nearest Neighbors

Normalizing to unit length lets you rank by cosine using a plain L2 distance. Product quantization compresses vectors so millions fit in memory at the cost of some precision. Chunk size is the quiet lever: too small and you embed noise, too large and you blur the signal. Dense retrieval maps text into a vector space where semantic neighbors sit close together. The centroid of a document's chunk vectors is a cheap coarse filter before the exact rescan.

Cosine similarity ranks candidates by the angle between their normalized embeddings. HNSW trades a little recall for a large latency win by walking a navigable small-world graph. An exact brute-force scan is O(n) per query but trivially correct and cache-friendly at small scale.

## HNSW Graphs

Hybrid search fuses lexical BM25 scores with dense similarity, often via reciprocal rank fusion. An exact brute-force scan is O(n) per query but trivially correct and cache-friendly at small scale. Product quantization compresses vectors so millions fit in memory at the cost of some precision. Query and document embeddings can be asymmetric when the query carries a retrieval instruction. Dense retrieval maps text into a vector space where semantic neighbors sit close together. See [[distributed-systems/171-retries-and-jitter]]. See [[vector-search/120-the-embedding-space]].

An exact brute-force scan is O(n) per query but trivially correct and cache-friendly at small scale. A reranker rescouring the top candidates with a cross-encoder usually beats a bigger index. HNSW trades a little recall for a large latency win by walking a navigable small-world graph. See [[vector-search/060-cosine-similarity]]. See [[vector-search/040-dense-vs-sparse]].

Recall@k measures how many of the true neighbors survive the approximate search. An exact brute-force scan is O(n) per query but trivially correct and cache-friendly at small scale. Cosine similarity ranks candidates by the angle between their normalized embeddings. Query and document embeddings can be asymmetric when the query carries a retrieval instruction. The centroid of a document's chunk vectors is a cheap coarse filter before the exact rescan.

## Dense vs Sparse

Hybrid search fuses lexical BM25 scores with dense similarity, often via reciprocal rank fusion. Cosine similarity ranks candidates by the angle between their normalized embeddings. Chunk size is the quiet lever: too small and you embed noise, too large and you blur the signal. Query and document embeddings can be asymmetric when the query carries a retrieval instruction. An exact brute-force scan is O(n) per query but trivially correct and cache-friendly at small scale. Product quantization compresses vectors so millions fit in memory at the cost of some precision. See [[rust/122-send-and-sync]]. See [[vector-search/020-the-embedding-space]].

An exact brute-force scan is O(n) per query but trivially correct and cache-friendly at small scale. Chunk size is the quiet lever: too small and you embed noise, too large and you blur the signal. HNSW trades a little recall for a large latency win by walking a navigable small-world graph. Product quantization compresses vectors so millions fit in memory at the cost of some precision. A reranker rescouring the top candidates with a cross-encoder usually beats a bigger index.

HNSW trades a little recall for a large latency win by walking a navigable small-world graph. Cosine similarity ranks candidates by the angle between their normalized embeddings. Product quantization compresses vectors so millions fit in memory at the cost of some precision. Query and document embeddings can be asymmetric when the query carries a retrieval instruction. Normalizing to unit length lets you rank by cosine using a plain L2 distance. An exact brute-force scan is O(n) per query but trivially correct and cache-friendly at small scale. See [[vector-search/160-the-embedding-space]].
