---
b2id: 01KXF21DZ88RQE99MSTBRR3TVT
type: note
title: "HNSW Graphs"
---

# HNSW Graphs

Notes on hnsw graphs within the broader theme of vector search.

## The Embedding Space

Cosine similarity ranks candidates by the angle between their normalized embeddings. Chunk size is the quiet lever: too small and you embed noise, too large and you blur the signal. Normalizing to unit length lets you rank by cosine using a plain L2 distance. Product quantization compresses vectors so millions fit in memory at the cost of some precision. An exact brute-force scan is O(n) per query but trivially correct and cache-friendly at small scale. Dense retrieval maps text into a vector space where semantic neighbors sit close together. See [[vector-search/200-recall-vs-latency]]. See [[vector-search/050-approximate-nearest-neighbors]].

A reranker rescouring the top candidates with a cross-encoder usually beats a bigger index. Dense retrieval maps text into a vector space where semantic neighbors sit close together. Chunk size is the quiet lever: too small and you embed noise, too large and you blur the signal. HNSW trades a little recall for a large latency win by walking a navigable small-world graph.

Recall@k measures how many of the true neighbors survive the approximate search. Hybrid search fuses lexical BM25 scores with dense similarity, often via reciprocal rank fusion. HNSW trades a little recall for a large latency win by walking a navigable small-world graph. Dense retrieval maps text into a vector space where semantic neighbors sit close together. See [[vector-search/190-dense-vs-sparse]]. See [[vector-search/040-dense-vs-sparse]].

## Dense vs Sparse

Normalizing to unit length lets you rank by cosine using a plain L2 distance. Product quantization compresses vectors so millions fit in memory at the cost of some precision. Query and document embeddings can be asymmetric when the query carries a retrieval instruction. Hybrid search fuses lexical BM25 scores with dense similarity, often via reciprocal rank fusion. See [[vector-search/180-reranking-candidates]]. See [[vector-search/070-cosine-similarity]].

Query and document embeddings can be asymmetric when the query carries a retrieval instruction. An exact brute-force scan is O(n) per query but trivially correct and cache-friendly at small scale. Normalizing to unit length lets you rank by cosine using a plain L2 distance. The centroid of a document's chunk vectors is a cheap coarse filter before the exact rescan. Hybrid search fuses lexical BM25 scores with dense similarity, often via reciprocal rank fusion.

Chunk size is the quiet lever: too small and you embed noise, too large and you blur the signal. Normalizing to unit length lets you rank by cosine using a plain L2 distance. HNSW trades a little recall for a large latency win by walking a navigable small-world graph. Cosine similarity ranks candidates by the angle between their normalized embeddings. A reranker rescouring the top candidates with a cross-encoder usually beats a bigger index. See [[vector-search/110-recall-vs-latency]]. See [[vector-search/180-reranking-candidates]].

## HNSW Graphs

The centroid of a document's chunk vectors is a cheap coarse filter before the exact rescan. Query and document embeddings can be asymmetric when the query carries a retrieval instruction. Hybrid search fuses lexical BM25 scores with dense similarity, often via reciprocal rank fusion. A reranker rescouring the top candidates with a cross-encoder usually beats a bigger index. An exact brute-force scan is O(n) per query but trivially correct and cache-friendly at small scale. See [[pkm/113-spaced-repetition]].

The centroid of a document's chunk vectors is a cheap coarse filter before the exact rescan. Chunk size is the quiet lever: too small and you embed noise, too large and you blur the signal. Cosine similarity ranks candidates by the angle between their normalized embeddings. An exact brute-force scan is O(n) per query but trivially correct and cache-friendly at small scale. See [[vector-search/100-dense-vs-sparse]].

A reranker rescouring the top candidates with a cross-encoder usually beats a bigger index. Hybrid search fuses lexical BM25 scores with dense similarity, often via reciprocal rank fusion. HNSW trades a little recall for a large latency win by walking a navigable small-world graph. Dense retrieval maps text into a vector space where semantic neighbors sit close together. Product quantization compresses vectors so millions fit in memory at the cost of some precision. Cosine similarity ranks candidates by the angle between their normalized embeddings. See [[vector-search/040-dense-vs-sparse]]. See [[vector-search/160-the-embedding-space]].
