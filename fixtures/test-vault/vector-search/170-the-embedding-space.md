---
b2id: 01KXF21DZHE2DQE997JH5M4JWH
type: note
title: "The Embedding Space"
---

# The Embedding Space

Notes on the embedding space within the broader theme of vector search.

## Approximate Nearest Neighbors

Dense retrieval maps text into a vector space where semantic neighbors sit close together. The centroid of a document's chunk vectors is a cheap coarse filter before the exact rescan. Recall@k measures how many of the true neighbors survive the approximate search. HNSW trades a little recall for a large latency win by walking a navigable small-world graph. See [[vector-search/040-dense-vs-sparse]]. See [[vector-search/130-dense-vs-sparse]].

Query and document embeddings can be asymmetric when the query carries a retrieval instruction. Product quantization compresses vectors so millions fit in memory at the cost of some precision. Hybrid search fuses lexical BM25 scores with dense similarity, often via reciprocal rank fusion. An exact brute-force scan is O(n) per query but trivially correct and cache-friendly at small scale. Normalizing to unit length lets you rank by cosine using a plain L2 distance.

Recall@k measures how many of the true neighbors survive the approximate search. HNSW trades a little recall for a large latency win by walking a navigable small-world graph. An exact brute-force scan is O(n) per query but trivially correct and cache-friendly at small scale. Hybrid search fuses lexical BM25 scores with dense similarity, often via reciprocal rank fusion. See [[vector-search/160-the-embedding-space]]. See [[vector-search/030-approximate-nearest-neighbors]].

## Quantized Vectors

Cosine similarity ranks candidates by the angle between their normalized embeddings. The centroid of a document's chunk vectors is a cheap coarse filter before the exact rescan. A reranker rescouring the top candidates with a cross-encoder usually beats a bigger index. HNSW trades a little recall for a large latency win by walking a navigable small-world graph.

Chunk size is the quiet lever: too small and you embed noise, too large and you blur the signal. Dense retrieval maps text into a vector space where semantic neighbors sit close together. HNSW trades a little recall for a large latency win by walking a navigable small-world graph. See [[pkm/113-spaced-repetition]]. See [[vector-search/050-approximate-nearest-neighbors]].

Product quantization compresses vectors so millions fit in memory at the cost of some precision. Chunk size is the quiet lever: too small and you embed noise, too large and you blur the signal. Hybrid search fuses lexical BM25 scores with dense similarity, often via reciprocal rank fusion. The centroid of a document's chunk vectors is a cheap coarse filter before the exact rescan. Normalizing to unit length lets you rank by cosine using a plain L2 distance. See [[databases/105-sqlite-as-a-library]].

## Quantized Vectors

Dense retrieval maps text into a vector space where semantic neighbors sit close together. A reranker rescouring the top candidates with a cross-encoder usually beats a bigger index. Hybrid search fuses lexical BM25 scores with dense similarity, often via reciprocal rank fusion. Chunk size is the quiet lever: too small and you embed noise, too large and you blur the signal. Recall@k measures how many of the true neighbors survive the approximate search. Query and document embeddings can be asymmetric when the query carries a retrieval instruction. See [[vector-search/200-recall-vs-latency]].

Cosine similarity ranks candidates by the angle between their normalized embeddings. Product quantization compresses vectors so millions fit in memory at the cost of some precision. Recall@k measures how many of the true neighbors survive the approximate search. The centroid of a document's chunk vectors is a cheap coarse filter before the exact rescan.

## Quantized Vectors

An exact brute-force scan is O(n) per query but trivially correct and cache-friendly at small scale. HNSW trades a little recall for a large latency win by walking a navigable small-world graph. Query and document embeddings can be asymmetric when the query carries a retrieval instruction. See [[vector-search/010-hybrid-retrieval]]. See [[productivity/186-default-to-action]].

An exact brute-force scan is O(n) per query but trivially correct and cache-friendly at small scale. Normalizing to unit length lets you rank by cosine using a plain L2 distance. Hybrid search fuses lexical BM25 scores with dense similarity, often via reciprocal rank fusion. Dense retrieval maps text into a vector space where semantic neighbors sit close together. Recall@k measures how many of the true neighbors survive the approximate search. The centroid of a document's chunk vectors is a cheap coarse filter before the exact rescan.

Recall@k measures how many of the true neighbors survive the approximate search. Hybrid search fuses lexical BM25 scores with dense similarity, often via reciprocal rank fusion. An exact brute-force scan is O(n) per query but trivially correct and cache-friendly at small scale. See [[transformers/174-positional-encoding]]. See [[vector-search/020-the-embedding-space]].

## Recall vs Latency

Hybrid search fuses lexical BM25 scores with dense similarity, often via reciprocal rank fusion. Normalizing to unit length lets you rank by cosine using a plain L2 distance. Query and document embeddings can be asymmetric when the query carries a retrieval instruction. Product quantization compresses vectors so millions fit in memory at the cost of some precision. See [[hiking/099-blister-prevention]]. See [[vector-search/020-the-embedding-space]].

An exact brute-force scan is O(n) per query but trivially correct and cache-friendly at small scale. Normalizing to unit length lets you rank by cosine using a plain L2 distance. Chunk size is the quiet lever: too small and you embed noise, too large and you blur the signal. See [[vector-search/050-approximate-nearest-neighbors]]. See [[vector-search/180-reranking-candidates]].
