---
b2id: 01KXF21DZMASWBNVBG309YST9Y
type: note
title: "Recall vs Latency"
---

# Recall vs Latency

Notes on recall vs latency within the broader theme of vector search.

## Hybrid Retrieval

Chunk size is the quiet lever: too small and you embed noise, too large and you blur the signal. HNSW trades a little recall for a large latency win by walking a navigable small-world graph. Hybrid search fuses lexical BM25 scores with dense similarity, often via reciprocal rank fusion. See [[hiking/019-reading-the-weather]].

Dense retrieval maps text into a vector space where semantic neighbors sit close together. Hybrid search fuses lexical BM25 scores with dense similarity, often via reciprocal rank fusion. Cosine similarity ranks candidates by the angle between their normalized embeddings. The centroid of a document's chunk vectors is a cheap coarse filter before the exact rescan.

The centroid of a document's chunk vectors is a cheap coarse filter before the exact rescan. Recall@k measures how many of the true neighbors survive the approximate search. Query and document embeddings can be asymmetric when the query carries a retrieval instruction. Hybrid search fuses lexical BM25 scores with dense similarity, often via reciprocal rank fusion. Chunk size is the quiet lever: too small and you embed noise, too large and you blur the signal. See [[transformers/144-the-feed-forward-block]]. See [[vector-search/080-hnsw-graphs]].

## The Embedding Space

A reranker rescouring the top candidates with a cross-encoder usually beats a bigger index. The centroid of a document's chunk vectors is a cheap coarse filter before the exact rescan. Hybrid search fuses lexical BM25 scores with dense similarity, often via reciprocal rank fusion. Product quantization compresses vectors so millions fit in memory at the cost of some precision.

Chunk size is the quiet lever: too small and you embed noise, too large and you blur the signal. Recall@k measures how many of the true neighbors survive the approximate search. Dense retrieval maps text into a vector space where semantic neighbors sit close together. HNSW trades a little recall for a large latency win by walking a navigable small-world graph. Hybrid search fuses lexical BM25 scores with dense similarity, often via reciprocal rank fusion.

## Dense vs Sparse

The centroid of a document's chunk vectors is a cheap coarse filter before the exact rescan. Normalizing to unit length lets you rank by cosine using a plain L2 distance. Recall@k measures how many of the true neighbors survive the approximate search. Query and document embeddings can be asymmetric when the query carries a retrieval instruction. See [[vector-search/180-reranking-candidates]].

Query and document embeddings can be asymmetric when the query carries a retrieval instruction. Dense retrieval maps text into a vector space where semantic neighbors sit close together. HNSW trades a little recall for a large latency win by walking a navigable small-world graph. The centroid of a document's chunk vectors is a cheap coarse filter before the exact rescan. Chunk size is the quiet lever: too small and you embed noise, too large and you blur the signal. Product quantization compresses vectors so millions fit in memory at the cost of some precision. See [[vector-search/120-the-embedding-space]]. See [[vector-search/170-the-embedding-space]].

Hybrid search fuses lexical BM25 scores with dense similarity, often via reciprocal rank fusion. Query and document embeddings can be asymmetric when the query carries a retrieval instruction. A reranker rescouring the top candidates with a cross-encoder usually beats a bigger index. See [[vector-search/130-dense-vs-sparse]].

## Chunking Strategy

HNSW trades a little recall for a large latency win by walking a navigable small-world graph. The centroid of a document's chunk vectors is a cheap coarse filter before the exact rescan. A reranker rescouring the top candidates with a cross-encoder usually beats a bigger index. Recall@k measures how many of the true neighbors survive the approximate search. Hybrid search fuses lexical BM25 scores with dense similarity, often via reciprocal rank fusion. Query and document embeddings can be asymmetric when the query carries a retrieval instruction.

Product quantization compresses vectors so millions fit in memory at the cost of some precision. Dense retrieval maps text into a vector space where semantic neighbors sit close together. Query and document embeddings can be asymmetric when the query carries a retrieval instruction. Hybrid search fuses lexical BM25 scores with dense similarity, often via reciprocal rank fusion. Cosine similarity ranks candidates by the angle between their normalized embeddings. HNSW trades a little recall for a large latency win by walking a navigable small-world graph. See [[coffee/028-the-pour-over]]. See [[pkm/133-the-zettelkasten]].

## Cosine Similarity

Cosine similarity ranks candidates by the angle between their normalized embeddings. Dense retrieval maps text into a vector space where semantic neighbors sit close together. Recall@k measures how many of the true neighbors survive the approximate search. See [[vector-search/030-approximate-nearest-neighbors]]. See [[vector-search/120-the-embedding-space]].

Chunk size is the quiet lever: too small and you embed noise, too large and you blur the signal. HNSW trades a little recall for a large latency win by walking a navigable small-world graph. Normalizing to unit length lets you rank by cosine using a plain L2 distance. Product quantization compresses vectors so millions fit in memory at the cost of some precision. Hybrid search fuses lexical BM25 scores with dense similarity, often via reciprocal rank fusion. See [[vector-search/190-dense-vs-sparse]]. See [[vector-search/140-reranking-candidates]].

## Cosine Similarity

Cosine similarity ranks candidates by the angle between their normalized embeddings. Query and document embeddings can be asymmetric when the query carries a retrieval instruction. An exact brute-force scan is O(n) per query but trivially correct and cache-friendly at small scale. A reranker rescouring the top candidates with a cross-encoder usually beats a bigger index. Hybrid search fuses lexical BM25 scores with dense similarity, often via reciprocal rank fusion. Dense retrieval maps text into a vector space where semantic neighbors sit close together.

Normalizing to unit length lets you rank by cosine using a plain L2 distance. Dense retrieval maps text into a vector space where semantic neighbors sit close together. An exact brute-force scan is O(n) per query but trivially correct and cache-friendly at small scale. See [[distributed-systems/051-leader-election]]. See [[vector-search/090-chunking-strategy]].

Normalizing to unit length lets you rank by cosine using a plain L2 distance. Chunk size is the quiet lever: too small and you embed noise, too large and you blur the signal. HNSW trades a little recall for a large latency win by walking a navigable small-world graph. Cosine similarity ranks candidates by the angle between their normalized embeddings. A reranker rescouring the top candidates with a cross-encoder usually beats a bigger index. Query and document embeddings can be asymmetric when the query carries a retrieval instruction. See [[vector-search/050-approximate-nearest-neighbors]].
