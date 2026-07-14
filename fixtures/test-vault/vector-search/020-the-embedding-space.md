---
b2id: 01KXF21DZ3AYZTG1Y440BCC0Z5
type: note
title: "The Embedding Space"
---

# The Embedding Space

Notes on the embedding space within the broader theme of vector search.

## Recall vs Latency

Query and document embeddings can be asymmetric when the query carries a retrieval instruction. Cosine similarity ranks candidates by the angle between their normalized embeddings. Chunk size is the quiet lever: too small and you embed noise, too large and you blur the signal. Hybrid search fuses lexical BM25 scores with dense similarity, often via reciprocal rank fusion. Dense retrieval maps text into a vector space where semantic neighbors sit close together. The centroid of a document's chunk vectors is a cheap coarse filter before the exact rescan. See [[vector-search/150-recall-vs-latency]].

Query and document embeddings can be asymmetric when the query carries a retrieval instruction. Dense retrieval maps text into a vector space where semantic neighbors sit close together. Product quantization compresses vectors so millions fit in memory at the cost of some precision. See [[vector-search/140-reranking-candidates]]. See [[vector-search/140-reranking-candidates]].

Hybrid search fuses lexical BM25 scores with dense similarity, often via reciprocal rank fusion. Normalizing to unit length lets you rank by cosine using a plain L2 distance. Recall@k measures how many of the true neighbors survive the approximate search. A reranker rescouring the top candidates with a cross-encoder usually beats a bigger index. Dense retrieval maps text into a vector space where semantic neighbors sit close together. Query and document embeddings can be asymmetric when the query carries a retrieval instruction. See [[vector-search/120-the-embedding-space]].

## Dense vs Sparse

Hybrid search fuses lexical BM25 scores with dense similarity, often via reciprocal rank fusion. Query and document embeddings can be asymmetric when the query carries a retrieval instruction. An exact brute-force scan is O(n) per query but trivially correct and cache-friendly at small scale. Chunk size is the quiet lever: too small and you embed noise, too large and you blur the signal. Dense retrieval maps text into a vector space where semantic neighbors sit close together.

A reranker rescouring the top candidates with a cross-encoder usually beats a bigger index. An exact brute-force scan is O(n) per query but trivially correct and cache-friendly at small scale. Product quantization compresses vectors so millions fit in memory at the cost of some precision. Dense retrieval maps text into a vector space where semantic neighbors sit close together. Normalizing to unit length lets you rank by cosine using a plain L2 distance. See [[vector-search/110-recall-vs-latency]].

## The Embedding Space

Dense retrieval maps text into a vector space where semantic neighbors sit close together. HNSW trades a little recall for a large latency win by walking a navigable small-world graph. Normalizing to unit length lets you rank by cosine using a plain L2 distance.

Chunk size is the quiet lever: too small and you embed noise, too large and you blur the signal. Recall@k measures how many of the true neighbors survive the approximate search. Hybrid search fuses lexical BM25 scores with dense similarity, often via reciprocal rank fusion. The centroid of a document's chunk vectors is a cheap coarse filter before the exact rescan. Normalizing to unit length lets you rank by cosine using a plain L2 distance. A reranker rescouring the top candidates with a cross-encoder usually beats a bigger index. See [[vector-search/080-hnsw-graphs]].

HNSW trades a little recall for a large latency win by walking a navigable small-world graph. An exact brute-force scan is O(n) per query but trivially correct and cache-friendly at small scale. Normalizing to unit length lets you rank by cosine using a plain L2 distance. Query and document embeddings can be asymmetric when the query carries a retrieval instruction. See [[vector-search/060-cosine-similarity]].

## Quantized Vectors

A reranker rescouring the top candidates with a cross-encoder usually beats a bigger index. The centroid of a document's chunk vectors is a cheap coarse filter before the exact rescan. An exact brute-force scan is O(n) per query but trivially correct and cache-friendly at small scale. Product quantization compresses vectors so millions fit in memory at the cost of some precision. See [[vector-search/150-recall-vs-latency]].

Dense retrieval maps text into a vector space where semantic neighbors sit close together. A reranker rescouring the top candidates with a cross-encoder usually beats a bigger index. Query and document embeddings can be asymmetric when the query carries a retrieval instruction. Product quantization compresses vectors so millions fit in memory at the cost of some precision.

Chunk size is the quiet lever: too small and you embed noise, too large and you blur the signal. An exact brute-force scan is O(n) per query but trivially correct and cache-friendly at small scale. A reranker rescouring the top candidates with a cross-encoder usually beats a bigger index.

## Recall vs Latency

An exact brute-force scan is O(n) per query but trivially correct and cache-friendly at small scale. Dense retrieval maps text into a vector space where semantic neighbors sit close together. Chunk size is the quiet lever: too small and you embed noise, too large and you blur the signal. A reranker rescouring the top candidates with a cross-encoder usually beats a bigger index. See [[vector-search/120-the-embedding-space]]. See [[databases/195-denormalization]].

Recall@k measures how many of the true neighbors survive the approximate search. Dense retrieval maps text into a vector space where semantic neighbors sit close together. Product quantization compresses vectors so millions fit in memory at the cost of some precision. A reranker rescouring the top candidates with a cross-encoder usually beats a bigger index. HNSW trades a little recall for a large latency win by walking a navigable small-world graph. See [[gardening/137-crop-rotation]].

HNSW trades a little recall for a large latency win by walking a navigable small-world graph. Recall@k measures how many of the true neighbors survive the approximate search. Normalizing to unit length lets you rank by cosine using a plain L2 distance. Cosine similarity ranks candidates by the angle between their normalized embeddings. See [[transformers/044-distillation]]. See [[productivity/156-timeboxing]].
