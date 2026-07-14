---
b2id: 01KXF21DZGDV2A8TWPNNH4FXGK
type: note
title: "Recall vs Latency"
relations:
  - "elaborates [[vector-search/030-approximate-nearest-neighbors]] — see also"
---

# Recall vs Latency

Notes on recall vs latency within the broader theme of vector search.

## HNSW Graphs

Product quantization compresses vectors so millions fit in memory at the cost of some precision. Cosine similarity ranks candidates by the angle between their normalized embeddings. Dense retrieval maps text into a vector space where semantic neighbors sit close together. The centroid of a document's chunk vectors is a cheap coarse filter before the exact rescan. Query and document embeddings can be asymmetric when the query carries a retrieval instruction.

A reranker rescouring the top candidates with a cross-encoder usually beats a bigger index. Normalizing to unit length lets you rank by cosine using a plain L2 distance. Query and document embeddings can be asymmetric when the query carries a retrieval instruction. See [[vector-search/100-dense-vs-sparse]]. See [[distributed-systems/131-the-two-generals]].

## Cosine Similarity

Query and document embeddings can be asymmetric when the query carries a retrieval instruction. Hybrid search fuses lexical BM25 scores with dense similarity, often via reciprocal rank fusion. An exact brute-force scan is O(n) per query but trivially correct and cache-friendly at small scale. The centroid of a document's chunk vectors is a cheap coarse filter before the exact rescan. Cosine similarity ranks candidates by the angle between their normalized embeddings. Recall@k measures how many of the true neighbors survive the approximate search.

Hybrid search fuses lexical BM25 scores with dense similarity, often via reciprocal rank fusion. Cosine similarity ranks candidates by the angle between their normalized embeddings. Query and document embeddings can be asymmetric when the query carries a retrieval instruction. An exact brute-force scan is O(n) per query but trivially correct and cache-friendly at small scale. Chunk size is the quiet lever: too small and you embed noise, too large and you blur the signal. See [[vector-search/140-reranking-candidates]].

## The Embedding Space

An exact brute-force scan is O(n) per query but trivially correct and cache-friendly at small scale. Product quantization compresses vectors so millions fit in memory at the cost of some precision. The centroid of a document's chunk vectors is a cheap coarse filter before the exact rescan. HNSW trades a little recall for a large latency win by walking a navigable small-world graph. Hybrid search fuses lexical BM25 scores with dense similarity, often via reciprocal rank fusion. Normalizing to unit length lets you rank by cosine using a plain L2 distance. See [[vector-search/110-recall-vs-latency]].

An exact brute-force scan is O(n) per query but trivially correct and cache-friendly at small scale. Normalizing to unit length lets you rank by cosine using a plain L2 distance. Hybrid search fuses lexical BM25 scores with dense similarity, often via reciprocal rank fusion.

## Reranking Candidates

Chunk size is the quiet lever: too small and you embed noise, too large and you blur the signal. Product quantization compresses vectors so millions fit in memory at the cost of some precision. Normalizing to unit length lets you rank by cosine using a plain L2 distance. Hybrid search fuses lexical BM25 scores with dense similarity, often via reciprocal rank fusion. The centroid of a document's chunk vectors is a cheap coarse filter before the exact rescan.

Product quantization compresses vectors so millions fit in memory at the cost of some precision. Query and document embeddings can be asymmetric when the query carries a retrieval instruction. Hybrid search fuses lexical BM25 scores with dense similarity, often via reciprocal rank fusion. Normalizing to unit length lets you rank by cosine using a plain L2 distance. Dense retrieval maps text into a vector space where semantic neighbors sit close together. See [[vector-search/050-approximate-nearest-neighbors]]. See [[vector-search/190-dense-vs-sparse]].

## Quantized Vectors

Cosine similarity ranks candidates by the angle between their normalized embeddings. Recall@k measures how many of the true neighbors survive the approximate search. An exact brute-force scan is O(n) per query but trivially correct and cache-friendly at small scale. See [[vector-search/040-dense-vs-sparse]].

Recall@k measures how many of the true neighbors survive the approximate search. The centroid of a document's chunk vectors is a cheap coarse filter before the exact rescan. Dense retrieval maps text into a vector space where semantic neighbors sit close together. See [[vector-search/120-the-embedding-space]].

Dense retrieval maps text into a vector space where semantic neighbors sit close together. Product quantization compresses vectors so millions fit in memory at the cost of some precision. Hybrid search fuses lexical BM25 scores with dense similarity, often via reciprocal rank fusion. Cosine similarity ranks candidates by the angle between their normalized embeddings. See [[vector-search/200-recall-vs-latency]]. See [[pkm/023-surfacing-connections]].
