---
b2id: 01KXF21DZJSY8MNXPJH50BF6Y5
type: note
title: "Reranking Candidates"
---

# Reranking Candidates

Notes on reranking candidates within the broader theme of vector search.

## Cosine Similarity

HNSW trades a little recall for a large latency win by walking a navigable small-world graph. Chunk size is the quiet lever: too small and you embed noise, too large and you blur the signal. Cosine similarity ranks candidates by the angle between their normalized embeddings. The centroid of a document's chunk vectors is a cheap coarse filter before the exact rescan. Query and document embeddings can be asymmetric when the query carries a retrieval instruction. Normalizing to unit length lets you rank by cosine using a plain L2 distance.

An exact brute-force scan is O(n) per query but trivially correct and cache-friendly at small scale. Hybrid search fuses lexical BM25 scores with dense similarity, often via reciprocal rank fusion. Query and document embeddings can be asymmetric when the query carries a retrieval instruction. HNSW trades a little recall for a large latency win by walking a navigable small-world graph. Chunk size is the quiet lever: too small and you embed noise, too large and you blur the signal. Normalizing to unit length lets you rank by cosine using a plain L2 distance. See [[vector-search/120-the-embedding-space]].

## Recall vs Latency

Product quantization compresses vectors so millions fit in memory at the cost of some precision. Chunk size is the quiet lever: too small and you embed noise, too large and you blur the signal. A reranker rescouring the top candidates with a cross-encoder usually beats a bigger index. Hybrid search fuses lexical BM25 scores with dense similarity, often via reciprocal rank fusion. HNSW trades a little recall for a large latency win by walking a navigable small-world graph. Dense retrieval maps text into a vector space where semantic neighbors sit close together. See [[vector-search/050-approximate-nearest-neighbors]]. See [[pkm/063-the-map-of-content]].

Hybrid search fuses lexical BM25 scores with dense similarity, often via reciprocal rank fusion. The centroid of a document's chunk vectors is a cheap coarse filter before the exact rescan. Chunk size is the quiet lever: too small and you embed noise, too large and you blur the signal. See [[coffee/158-freshness-and-degassing]].

Dense retrieval maps text into a vector space where semantic neighbors sit close together. Normalizing to unit length lets you rank by cosine using a plain L2 distance. The centroid of a document's chunk vectors is a cheap coarse filter before the exact rescan. Chunk size is the quiet lever: too small and you embed noise, too large and you blur the signal.

## Reranking Candidates

Dense retrieval maps text into a vector space where semantic neighbors sit close together. HNSW trades a little recall for a large latency win by walking a navigable small-world graph. An exact brute-force scan is O(n) per query but trivially correct and cache-friendly at small scale.

Query and document embeddings can be asymmetric when the query carries a retrieval instruction. The centroid of a document's chunk vectors is a cheap coarse filter before the exact rescan. Hybrid search fuses lexical BM25 scores with dense similarity, often via reciprocal rank fusion. See [[vector-search/160-the-embedding-space]]. See [[vector-search/110-recall-vs-latency]].

## Chunking Strategy

Recall@k measures how many of the true neighbors survive the approximate search. Product quantization compresses vectors so millions fit in memory at the cost of some precision. Cosine similarity ranks candidates by the angle between their normalized embeddings. Hybrid search fuses lexical BM25 scores with dense similarity, often via reciprocal rank fusion. See [[vector-search/050-approximate-nearest-neighbors]].

A reranker rescouring the top candidates with a cross-encoder usually beats a bigger index. Dense retrieval maps text into a vector space where semantic neighbors sit close together. Hybrid search fuses lexical BM25 scores with dense similarity, often via reciprocal rank fusion. Chunk size is the quiet lever: too small and you embed noise, too large and you blur the signal.

## Approximate Nearest Neighbors

An exact brute-force scan is O(n) per query but trivially correct and cache-friendly at small scale. Normalizing to unit length lets you rank by cosine using a plain L2 distance. Dense retrieval maps text into a vector space where semantic neighbors sit close together. Hybrid search fuses lexical BM25 scores with dense similarity, often via reciprocal rank fusion. Chunk size is the quiet lever: too small and you embed noise, too large and you blur the signal. See [[transformers/164-the-bert-encoder]].

Recall@k measures how many of the true neighbors survive the approximate search. Hybrid search fuses lexical BM25 scores with dense similarity, often via reciprocal rank fusion. Dense retrieval maps text into a vector space where semantic neighbors sit close together. Query and document embeddings can be asymmetric when the query carries a retrieval instruction. The centroid of a document's chunk vectors is a cheap coarse filter before the exact rescan. HNSW trades a little recall for a large latency win by walking a navigable small-world graph. See [[rust/172-zero-cost-abstractions]].

Hybrid search fuses lexical BM25 scores with dense similarity, often via reciprocal rank fusion. Normalizing to unit length lets you rank by cosine using a plain L2 distance. Cosine similarity ranks candidates by the angle between their normalized embeddings. The centroid of a document's chunk vectors is a cheap coarse filter before the exact rescan.
