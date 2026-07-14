---
b2id: 01KXF21DZ7MRQPKPN87GDAK4T7
type: note
title: "Cosine Similarity"
---

# Cosine Similarity

Notes on cosine similarity within the broader theme of vector search.

## Hybrid Retrieval

The centroid of a document's chunk vectors is a cheap coarse filter before the exact rescan. Normalizing to unit length lets you rank by cosine using a plain L2 distance. Chunk size is the quiet lever: too small and you embed noise, too large and you blur the signal. See [[distributed-systems/091-retries-and-jitter]].

HNSW trades a little recall for a large latency win by walking a navigable small-world graph. Normalizing to unit length lets you rank by cosine using a plain L2 distance. Hybrid search fuses lexical BM25 scores with dense similarity, often via reciprocal rank fusion. Recall@k measures how many of the true neighbors survive the approximate search. See [[coffee/198-the-pour-over]].

The centroid of a document's chunk vectors is a cheap coarse filter before the exact rescan. Dense retrieval maps text into a vector space where semantic neighbors sit close together. An exact brute-force scan is O(n) per query but trivially correct and cache-friendly at small scale. Normalizing to unit length lets you rank by cosine using a plain L2 distance. A reranker rescouring the top candidates with a cross-encoder usually beats a bigger index. Product quantization compresses vectors so millions fit in memory at the cost of some precision.

## HNSW Graphs

Recall@k measures how many of the true neighbors survive the approximate search. Product quantization compresses vectors so millions fit in memory at the cost of some precision. Normalizing to unit length lets you rank by cosine using a plain L2 distance. See [[vector-search/120-the-embedding-space]]. See [[vector-search/200-recall-vs-latency]].

An exact brute-force scan is O(n) per query but trivially correct and cache-friendly at small scale. Chunk size is the quiet lever: too small and you embed noise, too large and you blur the signal. Product quantization compresses vectors so millions fit in memory at the cost of some precision. Dense retrieval maps text into a vector space where semantic neighbors sit close together. A reranker rescouring the top candidates with a cross-encoder usually beats a bigger index.

## Dense vs Sparse

An exact brute-force scan is O(n) per query but trivially correct and cache-friendly at small scale. Query and document embeddings can be asymmetric when the query carries a retrieval instruction. Dense retrieval maps text into a vector space where semantic neighbors sit close together. Recall@k measures how many of the true neighbors survive the approximate search. The centroid of a document's chunk vectors is a cheap coarse filter before the exact rescan. See [[vector-search/120-the-embedding-space]]. See [[vector-search/160-the-embedding-space]].

An exact brute-force scan is O(n) per query but trivially correct and cache-friendly at small scale. Query and document embeddings can be asymmetric when the query carries a retrieval instruction. Product quantization compresses vectors so millions fit in memory at the cost of some precision. A reranker rescouring the top candidates with a cross-encoder usually beats a bigger index. The centroid of a document's chunk vectors is a cheap coarse filter before the exact rescan. Hybrid search fuses lexical BM25 scores with dense similarity, often via reciprocal rank fusion.

## Chunking Strategy

The centroid of a document's chunk vectors is a cheap coarse filter before the exact rescan. Hybrid search fuses lexical BM25 scores with dense similarity, often via reciprocal rank fusion. Product quantization compresses vectors so millions fit in memory at the cost of some precision. An exact brute-force scan is O(n) per query but trivially correct and cache-friendly at small scale. Cosine similarity ranks candidates by the angle between their normalized embeddings. See [[distributed-systems/041-leader-election]].

Hybrid search fuses lexical BM25 scores with dense similarity, often via reciprocal rank fusion. Cosine similarity ranks candidates by the angle between their normalized embeddings. Normalizing to unit length lets you rank by cosine using a plain L2 distance. Dense retrieval maps text into a vector space where semantic neighbors sit close together. Product quantization compresses vectors so millions fit in memory at the cost of some precision. See [[gardening/127-soil-ph]].

Query and document embeddings can be asymmetric when the query carries a retrieval instruction. Product quantization compresses vectors so millions fit in memory at the cost of some precision. Recall@k measures how many of the true neighbors survive the approximate search.
