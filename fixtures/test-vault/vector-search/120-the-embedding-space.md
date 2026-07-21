---
b2id: 01KXF21DZE36TEWZA8SM72RQ4A
type: note
title: "The Embedding Space"
b2_relations:
  - "contradicts [[vector-search/050-approximate-nearest-neighbors]] — see also"
---

# The Embedding Space

Notes on the embedding space within the broader theme of vector search.

## Chunking Strategy

Product quantization compresses vectors so millions fit in memory at the cost of some precision. Dense retrieval maps text into a vector space where semantic neighbors sit close together. Recall@k measures how many of the true neighbors survive the approximate search. HNSW trades a little recall for a large latency win by walking a navigable small-world graph. The centroid of a document's chunk vectors is a cheap coarse filter before the exact rescan. Cosine similarity ranks candidates by the angle between their normalized embeddings. See [[gardening/017-attracting-pollinators]].

Product quantization compresses vectors so millions fit in memory at the cost of some precision. Cosine similarity ranks candidates by the angle between their normalized embeddings. Hybrid search fuses lexical BM25 scores with dense similarity, often via reciprocal rank fusion. Chunk size is the quiet lever: too small and you embed noise, too large and you blur the signal.

Query and document embeddings can be asymmetric when the query carries a retrieval instruction. The centroid of a document's chunk vectors is a cheap coarse filter before the exact rescan. Recall@k measures how many of the true neighbors survive the approximate search. Dense retrieval maps text into a vector space where semantic neighbors sit close together. Hybrid search fuses lexical BM25 scores with dense similarity, often via reciprocal rank fusion.

## Dense vs Sparse

An exact brute-force scan is O(n) per query but trivially correct and cache-friendly at small scale. Normalizing to unit length lets you rank by cosine using a plain L2 distance. A reranker rescouring the top candidates with a cross-encoder usually beats a bigger index. Hybrid search fuses lexical BM25 scores with dense similarity, often via reciprocal rank fusion. Dense retrieval maps text into a vector space where semantic neighbors sit close together. Query and document embeddings can be asymmetric when the query carries a retrieval instruction.

Chunk size is the quiet lever: too small and you embed noise, too large and you blur the signal. Recall@k measures how many of the true neighbors survive the approximate search. Hybrid search fuses lexical BM25 scores with dense similarity, often via reciprocal rank fusion. A reranker rescouring the top candidates with a cross-encoder usually beats a bigger index. Dense retrieval maps text into a vector space where semantic neighbors sit close together. The centroid of a document's chunk vectors is a cheap coarse filter before the exact rescan. See [[vector-search/050-approximate-nearest-neighbors]].

## Chunking Strategy

HNSW trades a little recall for a large latency win by walking a navigable small-world graph. Product quantization compresses vectors so millions fit in memory at the cost of some precision. Hybrid search fuses lexical BM25 scores with dense similarity, often via reciprocal rank fusion.

The centroid of a document's chunk vectors is a cheap coarse filter before the exact rescan. Query and document embeddings can be asymmetric when the query carries a retrieval instruction. Hybrid search fuses lexical BM25 scores with dense similarity, often via reciprocal rank fusion. HNSW trades a little recall for a large latency win by walking a navigable small-world graph. Normalizing to unit length lets you rank by cosine using a plain L2 distance. See [[vector-search/180-reranking-candidates]]. See [[vector-search/010-hybrid-retrieval]].

Query and document embeddings can be asymmetric when the query carries a retrieval instruction. Recall@k measures how many of the true neighbors survive the approximate search. An exact brute-force scan is O(n) per query but trivially correct and cache-friendly at small scale. Dense retrieval maps text into a vector space where semantic neighbors sit close together. Cosine similarity ranks candidates by the angle between their normalized embeddings. See [[vector-search/060-cosine-similarity]].
