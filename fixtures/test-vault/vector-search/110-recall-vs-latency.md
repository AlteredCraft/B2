---
b2id: 01KXF21DZBH1AW1SC5JKFNQAGT
type: note
title: "Recall vs Latency"
---

# Recall vs Latency

Notes on recall vs latency within the broader theme of vector search.

## Approximate Nearest Neighbors

Cosine similarity ranks candidates by the angle between their normalized embeddings. Recall@k measures how many of the true neighbors survive the approximate search. The centroid of a document's chunk vectors is a cheap coarse filter before the exact rescan. An exact brute-force scan is O(n) per query but trivially correct and cache-friendly at small scale.

Dense retrieval maps text into a vector space where semantic neighbors sit close together. Hybrid search fuses lexical BM25 scores with dense similarity, often via reciprocal rank fusion. Cosine similarity ranks candidates by the angle between their normalized embeddings. Product quantization compresses vectors so millions fit in memory at the cost of some precision. A reranker rescouring the top candidates with a cross-encoder usually beats a bigger index. See [[vector-search/180-reranking-candidates]]. See [[vector-search/020-the-embedding-space]].

## Cosine Similarity

Chunk size is the quiet lever: too small and you embed noise, too large and you blur the signal. Hybrid search fuses lexical BM25 scores with dense similarity, often via reciprocal rank fusion. HNSW trades a little recall for a large latency win by walking a navigable small-world graph. The centroid of a document's chunk vectors is a cheap coarse filter before the exact rescan. Cosine similarity ranks candidates by the angle between their normalized embeddings. A reranker rescouring the top candidates with a cross-encoder usually beats a bigger index. See [[vector-search/050-approximate-nearest-neighbors]].

Cosine similarity ranks candidates by the angle between their normalized embeddings. The centroid of a document's chunk vectors is a cheap coarse filter before the exact rescan. Chunk size is the quiet lever: too small and you embed noise, too large and you blur the signal. Dense retrieval maps text into a vector space where semantic neighbors sit close together. Product quantization compresses vectors so millions fit in memory at the cost of some precision. Hybrid search fuses lexical BM25 scores with dense similarity, often via reciprocal rank fusion. See [[vector-search/080-hnsw-graphs]]. See [[vector-search/140-reranking-candidates]].

## Cosine Similarity

HNSW trades a little recall for a large latency win by walking a navigable small-world graph. Query and document embeddings can be asymmetric when the query carries a retrieval instruction. The centroid of a document's chunk vectors is a cheap coarse filter before the exact rescan. Chunk size is the quiet lever: too small and you embed noise, too large and you blur the signal. Normalizing to unit length lets you rank by cosine using a plain L2 distance. Hybrid search fuses lexical BM25 scores with dense similarity, often via reciprocal rank fusion. See [[vector-search/130-dense-vs-sparse]]. See [[vector-search/140-reranking-candidates]].

Dense retrieval maps text into a vector space where semantic neighbors sit close together. The centroid of a document's chunk vectors is a cheap coarse filter before the exact rescan. Chunk size is the quiet lever: too small and you embed noise, too large and you blur the signal. Query and document embeddings can be asymmetric when the query carries a retrieval instruction. Normalizing to unit length lets you rank by cosine using a plain L2 distance. A reranker rescouring the top candidates with a cross-encoder usually beats a bigger index. See [[vector-search/010-hybrid-retrieval]].

Query and document embeddings can be asymmetric when the query carries a retrieval instruction. Recall@k measures how many of the true neighbors survive the approximate search. Chunk size is the quiet lever: too small and you embed noise, too large and you blur the signal. See [[vector-search/050-approximate-nearest-neighbors]].

## Dense vs Sparse

Normalizing to unit length lets you rank by cosine using a plain L2 distance. Cosine similarity ranks candidates by the angle between their normalized embeddings. Hybrid search fuses lexical BM25 scores with dense similarity, often via reciprocal rank fusion. Query and document embeddings can be asymmetric when the query carries a retrieval instruction. Recall@k measures how many of the true neighbors survive the approximate search. Dense retrieval maps text into a vector space where semantic neighbors sit close together. See [[vector-search/200-recall-vs-latency]].

Chunk size is the quiet lever: too small and you embed noise, too large and you blur the signal. Cosine similarity ranks candidates by the angle between their normalized embeddings. Query and document embeddings can be asymmetric when the query carries a retrieval instruction. Product quantization compresses vectors so millions fit in memory at the cost of some precision.

Query and document embeddings can be asymmetric when the query carries a retrieval instruction. The centroid of a document's chunk vectors is a cheap coarse filter before the exact rescan. HNSW trades a little recall for a large latency win by walking a navigable small-world graph. Product quantization compresses vectors so millions fit in memory at the cost of some precision. Cosine similarity ranks candidates by the angle between their normalized embeddings. See [[vector-search/120-the-embedding-space]].

## Recall vs Latency

Recall@k measures how many of the true neighbors survive the approximate search. Chunk size is the quiet lever: too small and you embed noise, too large and you blur the signal. Cosine similarity ranks candidates by the angle between their normalized embeddings. An exact brute-force scan is O(n) per query but trivially correct and cache-friendly at small scale.

Cosine similarity ranks candidates by the angle between their normalized embeddings. The centroid of a document's chunk vectors is a cheap coarse filter before the exact rescan. HNSW trades a little recall for a large latency win by walking a navigable small-world graph.

## The Embedding Space

An exact brute-force scan is O(n) per query but trivially correct and cache-friendly at small scale. Product quantization compresses vectors so millions fit in memory at the cost of some precision. Normalizing to unit length lets you rank by cosine using a plain L2 distance. Chunk size is the quiet lever: too small and you embed noise, too large and you blur the signal. HNSW trades a little recall for a large latency win by walking a navigable small-world graph. Query and document embeddings can be asymmetric when the query carries a retrieval instruction. See [[productivity/056-shipping-small]].

Product quantization compresses vectors so millions fit in memory at the cost of some precision. The centroid of a document's chunk vectors is a cheap coarse filter before the exact rescan. Normalizing to unit length lets you rank by cosine using a plain L2 distance. Hybrid search fuses lexical BM25 scores with dense similarity, often via reciprocal rank fusion. An exact brute-force scan is O(n) per query but trivially correct and cache-friendly at small scale. Chunk size is the quiet lever: too small and you embed noise, too large and you blur the signal. See [[transformers/144-the-feed-forward-block]].

Recall@k measures how many of the true neighbors survive the approximate search. Product quantization compresses vectors so millions fit in memory at the cost of some precision. A reranker rescouring the top candidates with a cross-encoder usually beats a bigger index. The centroid of a document's chunk vectors is a cheap coarse filter before the exact rescan. Normalizing to unit length lets you rank by cosine using a plain L2 distance. An exact brute-force scan is O(n) per query but trivially correct and cache-friendly at small scale. See [[vector-search/030-approximate-nearest-neighbors]].
