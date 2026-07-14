---
b2id: 01KXF21DZ5PX1RWYERA5KT2YT5
type: note
title: "Dense vs Sparse"
---

# Dense vs Sparse

Notes on dense vs sparse within the broader theme of vector search.

## Quantized Vectors

Hybrid search fuses lexical BM25 scores with dense similarity, often via reciprocal rank fusion. Normalizing to unit length lets you rank by cosine using a plain L2 distance. HNSW trades a little recall for a large latency win by walking a navigable small-world graph. Cosine similarity ranks candidates by the angle between their normalized embeddings. Dense retrieval maps text into a vector space where semantic neighbors sit close together. An exact brute-force scan is O(n) per query but trivially correct and cache-friendly at small scale.

Chunk size is the quiet lever: too small and you embed noise, too large and you blur the signal. Cosine similarity ranks candidates by the angle between their normalized embeddings. Normalizing to unit length lets you rank by cosine using a plain L2 distance. A reranker rescouring the top candidates with a cross-encoder usually beats a bigger index. HNSW trades a little recall for a large latency win by walking a navigable small-world graph.

## Hybrid Retrieval

Normalizing to unit length lets you rank by cosine using a plain L2 distance. Product quantization compresses vectors so millions fit in memory at the cost of some precision. The centroid of a document's chunk vectors is a cheap coarse filter before the exact rescan. Cosine similarity ranks candidates by the angle between their normalized embeddings. Recall@k measures how many of the true neighbors survive the approximate search. Chunk size is the quiet lever: too small and you embed noise, too large and you blur the signal.

Cosine similarity ranks candidates by the angle between their normalized embeddings. Hybrid search fuses lexical BM25 scores with dense similarity, often via reciprocal rank fusion. Product quantization compresses vectors so millions fit in memory at the cost of some precision. See [[vector-search/100-dense-vs-sparse]]. See [[vector-search/090-chunking-strategy]].

## The Embedding Space

Cosine similarity ranks candidates by the angle between their normalized embeddings. Query and document embeddings can be asymmetric when the query carries a retrieval instruction. Hybrid search fuses lexical BM25 scores with dense similarity, often via reciprocal rank fusion. See [[vector-search/200-recall-vs-latency]].

Product quantization compresses vectors so millions fit in memory at the cost of some precision. Cosine similarity ranks candidates by the angle between their normalized embeddings. Chunk size is the quiet lever: too small and you embed noise, too large and you blur the signal. HNSW trades a little recall for a large latency win by walking a navigable small-world graph. A reranker rescouring the top candidates with a cross-encoder usually beats a bigger index.

## Chunking Strategy

Hybrid search fuses lexical BM25 scores with dense similarity, often via reciprocal rank fusion. Normalizing to unit length lets you rank by cosine using a plain L2 distance. Dense retrieval maps text into a vector space where semantic neighbors sit close together.

Cosine similarity ranks candidates by the angle between their normalized embeddings. A reranker rescouring the top candidates with a cross-encoder usually beats a bigger index. An exact brute-force scan is O(n) per query but trivially correct and cache-friendly at small scale. Chunk size is the quiet lever: too small and you embed noise, too large and you blur the signal. Recall@k measures how many of the true neighbors survive the approximate search. See [[vector-search/120-the-embedding-space]].

## Dense vs Sparse

HNSW trades a little recall for a large latency win by walking a navigable small-world graph. Dense retrieval maps text into a vector space where semantic neighbors sit close together. Query and document embeddings can be asymmetric when the query carries a retrieval instruction. See [[vector-search/060-cosine-similarity]].

Query and document embeddings can be asymmetric when the query carries a retrieval instruction. A reranker rescouring the top candidates with a cross-encoder usually beats a bigger index. HNSW trades a little recall for a large latency win by walking a navigable small-world graph. Dense retrieval maps text into a vector space where semantic neighbors sit close together. Hybrid search fuses lexical BM25 scores with dense similarity, often via reciprocal rank fusion. Normalizing to unit length lets you rank by cosine using a plain L2 distance. See [[hiking/059-switchbacks]]. See [[transformers/124-positional-encoding]].

Chunk size is the quiet lever: too small and you embed noise, too large and you blur the signal. A reranker rescouring the top candidates with a cross-encoder usually beats a bigger index. Query and document embeddings can be asymmetric when the query carries a retrieval instruction. Dense retrieval maps text into a vector space where semantic neighbors sit close together. Hybrid search fuses lexical BM25 scores with dense similarity, often via reciprocal rank fusion.
