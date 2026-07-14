---
b2id: 01KXF21DZF7E2TDM308YVYQYDX
type: note
title: "Reranking Candidates"
---

# Reranking Candidates

Notes on reranking candidates within the broader theme of vector search.

## Cosine Similarity

A reranker rescouring the top candidates with a cross-encoder usually beats a bigger index. HNSW trades a little recall for a large latency win by walking a navigable small-world graph. Dense retrieval maps text into a vector space where semantic neighbors sit close together. See [[hiking/159-blister-prevention]]. See [[productivity/036-managing-interruptions]].

Product quantization compresses vectors so millions fit in memory at the cost of some precision. HNSW trades a little recall for a large latency win by walking a navigable small-world graph. Hybrid search fuses lexical BM25 scores with dense similarity, often via reciprocal rank fusion. An exact brute-force scan is O(n) per query but trivially correct and cache-friendly at small scale.

## Chunking Strategy

An exact brute-force scan is O(n) per query but trivially correct and cache-friendly at small scale. Cosine similarity ranks candidates by the angle between their normalized embeddings. HNSW trades a little recall for a large latency win by walking a navigable small-world graph.

Cosine similarity ranks candidates by the angle between their normalized embeddings. A reranker rescouring the top candidates with a cross-encoder usually beats a bigger index. HNSW trades a little recall for a large latency win by walking a navigable small-world graph. Chunk size is the quiet lever: too small and you embed noise, too large and you blur the signal. See [[transformers/194-distillation]].

Product quantization compresses vectors so millions fit in memory at the cost of some precision. Query and document embeddings can be asymmetric when the query carries a retrieval instruction. Chunk size is the quiet lever: too small and you embed noise, too large and you blur the signal. Hybrid search fuses lexical BM25 scores with dense similarity, often via reciprocal rank fusion. An exact brute-force scan is O(n) per query but trivially correct and cache-friendly at small scale. Dense retrieval maps text into a vector space where semantic neighbors sit close together.

## HNSW Graphs

A reranker rescouring the top candidates with a cross-encoder usually beats a bigger index. HNSW trades a little recall for a large latency win by walking a navigable small-world graph. Chunk size is the quiet lever: too small and you embed noise, too large and you blur the signal. Recall@k measures how many of the true neighbors survive the approximate search. See [[vector-search/050-approximate-nearest-neighbors]]. See [[vector-search/020-the-embedding-space]].

A reranker rescouring the top candidates with a cross-encoder usually beats a bigger index. Product quantization compresses vectors so millions fit in memory at the cost of some precision. An exact brute-force scan is O(n) per query but trivially correct and cache-friendly at small scale. HNSW trades a little recall for a large latency win by walking a navigable small-world graph. Chunk size is the quiet lever: too small and you embed noise, too large and you blur the signal. See [[distributed-systems/041-leader-election]]. See [[vector-search/170-the-embedding-space]].

Hybrid search fuses lexical BM25 scores with dense similarity, often via reciprocal rank fusion. An exact brute-force scan is O(n) per query but trivially correct and cache-friendly at small scale. Cosine similarity ranks candidates by the angle between their normalized embeddings. Recall@k measures how many of the true neighbors survive the approximate search. Dense retrieval maps text into a vector space where semantic neighbors sit close together. Normalizing to unit length lets you rank by cosine using a plain L2 distance.
