---
b2id: 01KXF21DZ97YFCWM7Z76XFHWVW
type: note
title: "Chunking Strategy"
---

# Chunking Strategy

Notes on chunking strategy within the broader theme of vector search.

## The Embedding Space

Normalizing to unit length lets you rank by cosine using a plain L2 distance. Hybrid search fuses lexical BM25 scores with dense similarity, often via reciprocal rank fusion. A reranker rescouring the top candidates with a cross-encoder usually beats a bigger index. An exact brute-force scan is O(n) per query but trivially correct and cache-friendly at small scale. The centroid of a document's chunk vectors is a cheap coarse filter before the exact rescan. See [[vector-search/180-reranking-candidates]]. See [[databases/105-sqlite-as-a-library]].

A reranker rescouring the top candidates with a cross-encoder usually beats a bigger index. Normalizing to unit length lets you rank by cosine using a plain L2 distance. Recall@k measures how many of the true neighbors survive the approximate search. Cosine similarity ranks candidates by the angle between their normalized embeddings. Query and document embeddings can be asymmetric when the query carries a retrieval instruction. See [[vector-search/020-the-embedding-space]]. See [[vector-search/010-hybrid-retrieval]].

## Approximate Nearest Neighbors

Query and document embeddings can be asymmetric when the query carries a retrieval instruction. An exact brute-force scan is O(n) per query but trivially correct and cache-friendly at small scale. Product quantization compresses vectors so millions fit in memory at the cost of some precision. A reranker rescouring the top candidates with a cross-encoder usually beats a bigger index. The centroid of a document's chunk vectors is a cheap coarse filter before the exact rescan.

Dense retrieval maps text into a vector space where semantic neighbors sit close together. Recall@k measures how many of the true neighbors survive the approximate search. Cosine similarity ranks candidates by the angle between their normalized embeddings. The centroid of a document's chunk vectors is a cheap coarse filter before the exact rescan. An exact brute-force scan is O(n) per query but trivially correct and cache-friendly at small scale. Hybrid search fuses lexical BM25 scores with dense similarity, often via reciprocal rank fusion.

## Dense vs Sparse

Cosine similarity ranks candidates by the angle between their normalized embeddings. Product quantization compresses vectors so millions fit in memory at the cost of some precision. Recall@k measures how many of the true neighbors survive the approximate search. HNSW trades a little recall for a large latency win by walking a navigable small-world graph. See [[vector-search/030-approximate-nearest-neighbors]].

An exact brute-force scan is O(n) per query but trivially correct and cache-friendly at small scale. Normalizing to unit length lets you rank by cosine using a plain L2 distance. Chunk size is the quiet lever: too small and you embed noise, too large and you blur the signal. A reranker rescouring the top candidates with a cross-encoder usually beats a bigger index. See [[vector-search/140-reranking-candidates]].

Dense retrieval maps text into a vector space where semantic neighbors sit close together. The centroid of a document's chunk vectors is a cheap coarse filter before the exact rescan. Cosine similarity ranks candidates by the angle between their normalized embeddings. An exact brute-force scan is O(n) per query but trivially correct and cache-friendly at small scale. See [[vector-search/160-the-embedding-space]].
