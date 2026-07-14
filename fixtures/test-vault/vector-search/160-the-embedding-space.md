---
b2id: 01KXF21DZH1T0GARCYYX9YXNVX
type: note
title: "The Embedding Space"
---

# The Embedding Space

Notes on the embedding space within the broader theme of vector search.

## Hybrid Retrieval

A reranker rescouring the top candidates with a cross-encoder usually beats a bigger index. Cosine similarity ranks candidates by the angle between their normalized embeddings. Normalizing to unit length lets you rank by cosine using a plain L2 distance. Recall@k measures how many of the true neighbors survive the approximate search. Hybrid search fuses lexical BM25 scores with dense similarity, often via reciprocal rank fusion. See [[vector-search/050-approximate-nearest-neighbors]]. See [[vector-search/140-reranking-candidates]].

Normalizing to unit length lets you rank by cosine using a plain L2 distance. Recall@k measures how many of the true neighbors survive the approximate search. A reranker rescouring the top candidates with a cross-encoder usually beats a bigger index. An exact brute-force scan is O(n) per query but trivially correct and cache-friendly at small scale. Hybrid search fuses lexical BM25 scores with dense similarity, often via reciprocal rank fusion. See [[vector-search/180-reranking-candidates]].

## Approximate Nearest Neighbors

Cosine similarity ranks candidates by the angle between their normalized embeddings. Chunk size is the quiet lever: too small and you embed noise, too large and you blur the signal. Query and document embeddings can be asymmetric when the query carries a retrieval instruction. See [[vector-search/170-the-embedding-space]].

Recall@k measures how many of the true neighbors survive the approximate search. Normalizing to unit length lets you rank by cosine using a plain L2 distance. A reranker rescouring the top candidates with a cross-encoder usually beats a bigger index. Query and document embeddings can be asymmetric when the query carries a retrieval instruction. See [[vector-search/060-cosine-similarity]].

Recall@k measures how many of the true neighbors survive the approximate search. Dense retrieval maps text into a vector space where semantic neighbors sit close together. Chunk size is the quiet lever: too small and you embed noise, too large and you blur the signal. Normalizing to unit length lets you rank by cosine using a plain L2 distance. A reranker rescouring the top candidates with a cross-encoder usually beats a bigger index. See [[vector-search/180-reranking-candidates]].

## HNSW Graphs

Dense retrieval maps text into a vector space where semantic neighbors sit close together. Query and document embeddings can be asymmetric when the query carries a retrieval instruction. Hybrid search fuses lexical BM25 scores with dense similarity, often via reciprocal rank fusion. Normalizing to unit length lets you rank by cosine using a plain L2 distance. HNSW trades a little recall for a large latency win by walking a navigable small-world graph. Cosine similarity ranks candidates by the angle between their normalized embeddings. See [[vector-search/100-dense-vs-sparse]]. See [[vector-search/040-dense-vs-sparse]].

Dense retrieval maps text into a vector space where semantic neighbors sit close together. Hybrid search fuses lexical BM25 scores with dense similarity, often via reciprocal rank fusion. HNSW trades a little recall for a large latency win by walking a navigable small-world graph. See [[transformers/184-context-windows]]. See [[gardening/147-companion-planting]].

Query and document embeddings can be asymmetric when the query carries a retrieval instruction. Cosine similarity ranks candidates by the angle between their normalized embeddings. Normalizing to unit length lets you rank by cosine using a plain L2 distance. A reranker rescouring the top candidates with a cross-encoder usually beats a bigger index. Recall@k measures how many of the true neighbors survive the approximate search.

## Hybrid Retrieval

Cosine similarity ranks candidates by the angle between their normalized embeddings. Product quantization compresses vectors so millions fit in memory at the cost of some precision. Normalizing to unit length lets you rank by cosine using a plain L2 distance. See [[distributed-systems/011-leader-election]]. See [[vector-search/140-reranking-candidates]].

Product quantization compresses vectors so millions fit in memory at the cost of some precision. An exact brute-force scan is O(n) per query but trivially correct and cache-friendly at small scale. Dense retrieval maps text into a vector space where semantic neighbors sit close together. Chunk size is the quiet lever: too small and you embed noise, too large and you blur the signal. Normalizing to unit length lets you rank by cosine using a plain L2 distance. The centroid of a document's chunk vectors is a cheap coarse filter before the exact rescan. See [[coffee/178-the-espresso-shot]].
