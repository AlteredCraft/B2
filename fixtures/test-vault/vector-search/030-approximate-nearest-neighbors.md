---
b2id: 01KXF21DZ47PP3GVTE9V817MW0
type: note
title: "Approximate Nearest Neighbors"
relations:
  - "supports [[vector-search/010-hybrid-retrieval]] — see also"
---

# Approximate Nearest Neighbors

Notes on approximate nearest neighbors within the broader theme of vector search.

## Dense vs Sparse

Dense retrieval maps text into a vector space where semantic neighbors sit close together. Cosine similarity ranks candidates by the angle between their normalized embeddings. HNSW trades a little recall for a large latency win by walking a navigable small-world graph. Normalizing to unit length lets you rank by cosine using a plain L2 distance. Query and document embeddings can be asymmetric when the query carries a retrieval instruction. See [[vector-search/080-hnsw-graphs]]. See [[rust/082-iterators-and-laziness]].

Normalizing to unit length lets you rank by cosine using a plain L2 distance. HNSW trades a little recall for a large latency win by walking a navigable small-world graph. Recall@k measures how many of the true neighbors survive the approximate search. The centroid of a document's chunk vectors is a cheap coarse filter before the exact rescan. A reranker rescouring the top candidates with a cross-encoder usually beats a bigger index. Dense retrieval maps text into a vector space where semantic neighbors sit close together.

Cosine similarity ranks candidates by the angle between their normalized embeddings. The centroid of a document's chunk vectors is a cheap coarse filter before the exact rescan. HNSW trades a little recall for a large latency win by walking a navigable small-world graph. A reranker rescouring the top candidates with a cross-encoder usually beats a bigger index. Chunk size is the quiet lever: too small and you embed noise, too large and you blur the signal. An exact brute-force scan is O(n) per query but trivially correct and cache-friendly at small scale.

## Dense vs Sparse

Chunk size is the quiet lever: too small and you embed noise, too large and you blur the signal. Product quantization compresses vectors so millions fit in memory at the cost of some precision. Normalizing to unit length lets you rank by cosine using a plain L2 distance. See [[productivity/016-deep-work]].

Recall@k measures how many of the true neighbors survive the approximate search. A reranker rescouring the top candidates with a cross-encoder usually beats a bigger index. Dense retrieval maps text into a vector space where semantic neighbors sit close together. Query and document embeddings can be asymmetric when the query carries a retrieval instruction. See [[vector-search/070-cosine-similarity]].

The centroid of a document's chunk vectors is a cheap coarse filter before the exact rescan. Product quantization compresses vectors so millions fit in memory at the cost of some precision. Normalizing to unit length lets you rank by cosine using a plain L2 distance. Query and document embeddings can be asymmetric when the query carries a retrieval instruction. A reranker rescouring the top candidates with a cross-encoder usually beats a bigger index. Dense retrieval maps text into a vector space where semantic neighbors sit close together. See [[vector-search/130-dense-vs-sparse]]. See [[vector-search/150-recall-vs-latency]].

## Dense vs Sparse

Recall@k measures how many of the true neighbors survive the approximate search. A reranker rescouring the top candidates with a cross-encoder usually beats a bigger index. Hybrid search fuses lexical BM25 scores with dense similarity, often via reciprocal rank fusion. Product quantization compresses vectors so millions fit in memory at the cost of some precision. See [[transformers/134-tokenization]]. See [[vector-search/020-the-embedding-space]].

Dense retrieval maps text into a vector space where semantic neighbors sit close together. HNSW trades a little recall for a large latency win by walking a navigable small-world graph. The centroid of a document's chunk vectors is a cheap coarse filter before the exact rescan. Hybrid search fuses lexical BM25 scores with dense similarity, often via reciprocal rank fusion. See [[databases/105-sqlite-as-a-library]].

## Dense vs Sparse

Recall@k measures how many of the true neighbors survive the approximate search. HNSW trades a little recall for a large latency win by walking a navigable small-world graph. Cosine similarity ranks candidates by the angle between their normalized embeddings. Product quantization compresses vectors so millions fit in memory at the cost of some precision. Chunk size is the quiet lever: too small and you embed noise, too large and you blur the signal. Hybrid search fuses lexical BM25 scores with dense similarity, often via reciprocal rank fusion. See [[productivity/026-single-tasking]].

Product quantization compresses vectors so millions fit in memory at the cost of some precision. HNSW trades a little recall for a large latency win by walking a navigable small-world graph. The centroid of a document's chunk vectors is a cheap coarse filter before the exact rescan. Cosine similarity ranks candidates by the angle between their normalized embeddings. Query and document embeddings can be asymmetric when the query carries a retrieval instruction. See [[vector-search/160-the-embedding-space]].

## HNSW Graphs

Normalizing to unit length lets you rank by cosine using a plain L2 distance. Recall@k measures how many of the true neighbors survive the approximate search. Hybrid search fuses lexical BM25 scores with dense similarity, often via reciprocal rank fusion. Product quantization compresses vectors so millions fit in memory at the cost of some precision. HNSW trades a little recall for a large latency win by walking a navigable small-world graph. See [[vector-search/150-recall-vs-latency]].

Cosine similarity ranks candidates by the angle between their normalized embeddings. HNSW trades a little recall for a large latency win by walking a navigable small-world graph. The centroid of a document's chunk vectors is a cheap coarse filter before the exact rescan. Hybrid search fuses lexical BM25 scores with dense similarity, often via reciprocal rank fusion. Dense retrieval maps text into a vector space where semantic neighbors sit close together. See [[vector-search/170-the-embedding-space]].

An exact brute-force scan is O(n) per query but trivially correct and cache-friendly at small scale. Query and document embeddings can be asymmetric when the query carries a retrieval instruction. Product quantization compresses vectors so millions fit in memory at the cost of some precision.
