---
b2id: 01KXF21DZAD26BW491EG84MKBV
type: note
title: "Dense vs Sparse"
---

# Dense vs Sparse

Notes on dense vs sparse within the broader theme of vector search.

## Quantized Vectors

A reranker rescouring the top candidates with a cross-encoder usually beats a bigger index. HNSW trades a little recall for a large latency win by walking a navigable small-world graph. Product quantization compresses vectors so millions fit in memory at the cost of some precision.

HNSW trades a little recall for a large latency win by walking a navigable small-world graph. Chunk size is the quiet lever: too small and you embed noise, too large and you blur the signal. Recall@k measures how many of the true neighbors survive the approximate search. An exact brute-force scan is O(n) per query but trivially correct and cache-friendly at small scale. Hybrid search fuses lexical BM25 scores with dense similarity, often via reciprocal rank fusion. See [[vector-search/080-hnsw-graphs]]. See [[vector-search/180-reranking-candidates]].

## Chunking Strategy

Product quantization compresses vectors so millions fit in memory at the cost of some precision. Normalizing to unit length lets you rank by cosine using a plain L2 distance. Hybrid search fuses lexical BM25 scores with dense similarity, often via reciprocal rank fusion. Chunk size is the quiet lever: too small and you embed noise, too large and you blur the signal. A reranker rescouring the top candidates with a cross-encoder usually beats a bigger index.

Chunk size is the quiet lever: too small and you embed noise, too large and you blur the signal. Query and document embeddings can be asymmetric when the query carries a retrieval instruction. A reranker rescouring the top candidates with a cross-encoder usually beats a bigger index.

Dense retrieval maps text into a vector space where semantic neighbors sit close together. A reranker rescouring the top candidates with a cross-encoder usually beats a bigger index. Product quantization compresses vectors so millions fit in memory at the cost of some precision. See [[vector-search/090-chunking-strategy]]. See [[vector-search/160-the-embedding-space]].

## Recall vs Latency

HNSW trades a little recall for a large latency win by walking a navigable small-world graph. Product quantization compresses vectors so millions fit in memory at the cost of some precision. The centroid of a document's chunk vectors is a cheap coarse filter before the exact rescan. An exact brute-force scan is O(n) per query but trivially correct and cache-friendly at small scale. See [[databases/055-sqlite-as-a-library]]. See [[vector-search/180-reranking-candidates]].

Cosine similarity ranks candidates by the angle between their normalized embeddings. Normalizing to unit length lets you rank by cosine using a plain L2 distance. An exact brute-force scan is O(n) per query but trivially correct and cache-friendly at small scale. Dense retrieval maps text into a vector space where semantic neighbors sit close together. Product quantization compresses vectors so millions fit in memory at the cost of some precision. HNSW trades a little recall for a large latency win by walking a navigable small-world graph. See [[vector-search/070-cosine-similarity]].

HNSW trades a little recall for a large latency win by walking a navigable small-world graph. Product quantization compresses vectors so millions fit in memory at the cost of some precision. A reranker rescouring the top candidates with a cross-encoder usually beats a bigger index. See [[vector-search/190-dense-vs-sparse]]. See [[vector-search/060-cosine-similarity]].

## The Embedding Space

Dense retrieval maps text into a vector space where semantic neighbors sit close together. Cosine similarity ranks candidates by the angle between their normalized embeddings. Query and document embeddings can be asymmetric when the query carries a retrieval instruction. See [[vector-search/090-chunking-strategy]]. See [[vector-search/120-the-embedding-space]].

Dense retrieval maps text into a vector space where semantic neighbors sit close together. Query and document embeddings can be asymmetric when the query carries a retrieval instruction. An exact brute-force scan is O(n) per query but trivially correct and cache-friendly at small scale. The centroid of a document's chunk vectors is a cheap coarse filter before the exact rescan. See [[vector-search/010-hybrid-retrieval]]. See [[vector-search/150-recall-vs-latency]].

## The Embedding Space

Normalizing to unit length lets you rank by cosine using a plain L2 distance. Hybrid search fuses lexical BM25 scores with dense similarity, often via reciprocal rank fusion. HNSW trades a little recall for a large latency win by walking a navigable small-world graph. See [[vector-search/200-recall-vs-latency]].

Chunk size is the quiet lever: too small and you embed noise, too large and you blur the signal. An exact brute-force scan is O(n) per query but trivially correct and cache-friendly at small scale. Cosine similarity ranks candidates by the angle between their normalized embeddings. Product quantization compresses vectors so millions fit in memory at the cost of some precision.

HNSW trades a little recall for a large latency win by walking a navigable small-world graph. Hybrid search fuses lexical BM25 scores with dense similarity, often via reciprocal rank fusion. Query and document embeddings can be asymmetric when the query carries a retrieval instruction. An exact brute-force scan is O(n) per query but trivially correct and cache-friendly at small scale. See [[productivity/056-shipping-small]]. See [[vector-search/120-the-embedding-space]].
