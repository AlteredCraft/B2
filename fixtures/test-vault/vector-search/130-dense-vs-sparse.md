---
b2id: 01KXF21DZEMC1Q7GWN2TBXXHZX
type: note
title: "Dense vs Sparse"
---

# Dense vs Sparse

Notes on dense vs sparse within the broader theme of vector search.

## The Embedding Space

Dense retrieval maps text into a vector space where semantic neighbors sit close together. HNSW trades a little recall for a large latency win by walking a navigable small-world graph. An exact brute-force scan is O(n) per query but trivially correct and cache-friendly at small scale. Query and document embeddings can be asymmetric when the query carries a retrieval instruction. See [[vector-search/010-hybrid-retrieval]]. See [[gardening/177-mulching]].

The centroid of a document's chunk vectors is a cheap coarse filter before the exact rescan. Hybrid search fuses lexical BM25 scores with dense similarity, often via reciprocal rank fusion. HNSW trades a little recall for a large latency win by walking a navigable small-world graph. See [[rust/132-interior-mutability]].

Chunk size is the quiet lever: too small and you embed noise, too large and you blur the signal. Hybrid search fuses lexical BM25 scores with dense similarity, often via reciprocal rank fusion. Product quantization compresses vectors so millions fit in memory at the cost of some precision. A reranker rescouring the top candidates with a cross-encoder usually beats a bigger index. Recall@k measures how many of the true neighbors survive the approximate search. An exact brute-force scan is O(n) per query but trivially correct and cache-friendly at small scale. See [[vector-search/060-cosine-similarity]]. See [[databases/135-connection-pooling]].

## Hybrid Retrieval

A reranker rescouring the top candidates with a cross-encoder usually beats a bigger index. Cosine similarity ranks candidates by the angle between their normalized embeddings. An exact brute-force scan is O(n) per query but trivially correct and cache-friendly at small scale. Product quantization compresses vectors so millions fit in memory at the cost of some precision. Recall@k measures how many of the true neighbors survive the approximate search. Hybrid search fuses lexical BM25 scores with dense similarity, often via reciprocal rank fusion. See [[vector-search/090-chunking-strategy]].

HNSW trades a little recall for a large latency win by walking a navigable small-world graph. Cosine similarity ranks candidates by the angle between their normalized embeddings. Query and document embeddings can be asymmetric when the query carries a retrieval instruction. Chunk size is the quiet lever: too small and you embed noise, too large and you blur the signal. An exact brute-force scan is O(n) per query but trivially correct and cache-friendly at small scale. Product quantization compresses vectors so millions fit in memory at the cost of some precision. See [[vector-search/030-approximate-nearest-neighbors]].

## Dense vs Sparse

Cosine similarity ranks candidates by the angle between their normalized embeddings. An exact brute-force scan is O(n) per query but trivially correct and cache-friendly at small scale. Normalizing to unit length lets you rank by cosine using a plain L2 distance. See [[vector-search/150-recall-vs-latency]].

Query and document embeddings can be asymmetric when the query carries a retrieval instruction. HNSW trades a little recall for a large latency win by walking a navigable small-world graph. Chunk size is the quiet lever: too small and you embed noise, too large and you blur the signal. See [[productivity/026-single-tasking]]. See [[vector-search/150-recall-vs-latency]].

## Approximate Nearest Neighbors

Product quantization compresses vectors so millions fit in memory at the cost of some precision. The centroid of a document's chunk vectors is a cheap coarse filter before the exact rescan. Recall@k measures how many of the true neighbors survive the approximate search. Hybrid search fuses lexical BM25 scores with dense similarity, often via reciprocal rank fusion.

HNSW trades a little recall for a large latency win by walking a navigable small-world graph. Normalizing to unit length lets you rank by cosine using a plain L2 distance. Recall@k measures how many of the true neighbors survive the approximate search. See [[vector-search/170-the-embedding-space]].

Hybrid search fuses lexical BM25 scores with dense similarity, often via reciprocal rank fusion. Dense retrieval maps text into a vector space where semantic neighbors sit close together. A reranker rescouring the top candidates with a cross-encoder usually beats a bigger index. See [[vector-search/070-cosine-similarity]].

## Cosine Similarity

An exact brute-force scan is O(n) per query but trivially correct and cache-friendly at small scale. HNSW trades a little recall for a large latency win by walking a navigable small-world graph. Normalizing to unit length lets you rank by cosine using a plain L2 distance. Chunk size is the quiet lever: too small and you embed noise, too large and you blur the signal. See [[vector-search/080-hnsw-graphs]].

Hybrid search fuses lexical BM25 scores with dense similarity, often via reciprocal rank fusion. Dense retrieval maps text into a vector space where semantic neighbors sit close together. An exact brute-force scan is O(n) per query but trivially correct and cache-friendly at small scale. Query and document embeddings can be asymmetric when the query carries a retrieval instruction. Chunk size is the quiet lever: too small and you embed noise, too large and you blur the signal. A reranker rescouring the top candidates with a cross-encoder usually beats a bigger index. See [[rust/162-the-newtype-pattern]]. See [[coffee/028-the-pour-over]].

## Approximate Nearest Neighbors

Cosine similarity ranks candidates by the angle between their normalized embeddings. Query and document embeddings can be asymmetric when the query carries a retrieval instruction. Dense retrieval maps text into a vector space where semantic neighbors sit close together. HNSW trades a little recall for a large latency win by walking a navigable small-world graph.

Cosine similarity ranks candidates by the angle between their normalized embeddings. Recall@k measures how many of the true neighbors survive the approximate search. An exact brute-force scan is O(n) per query but trivially correct and cache-friendly at small scale. See [[coffee/158-freshness-and-degassing]].

Cosine similarity ranks candidates by the angle between their normalized embeddings. An exact brute-force scan is O(n) per query but trivially correct and cache-friendly at small scale. Query and document embeddings can be asymmetric when the query carries a retrieval instruction. See [[vector-search/180-reranking-candidates]].
