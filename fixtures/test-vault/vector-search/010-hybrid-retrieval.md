---
b2id: 01KXF21DZ3P1XQH98REF8GY2HQ
type: note
title: "Hybrid Retrieval"
relations:
  - "part-of [[vector-search/070-cosine-similarity]] — see also"
---

# Hybrid Retrieval

Notes on hybrid retrieval within the broader theme of vector search.

## Reranking Candidates

Query and document embeddings can be asymmetric when the query carries a retrieval instruction. A reranker rescouring the top candidates with a cross-encoder usually beats a bigger index. Dense retrieval maps text into a vector space where semantic neighbors sit close together. The centroid of a document's chunk vectors is a cheap coarse filter before the exact rescan. Recall@k measures how many of the true neighbors survive the approximate search. Normalizing to unit length lets you rank by cosine using a plain L2 distance.

Hybrid search fuses lexical BM25 scores with dense similarity, often via reciprocal rank fusion. Chunk size is the quiet lever: too small and you embed noise, too large and you blur the signal. The centroid of a document's chunk vectors is a cheap coarse filter before the exact rescan. An exact brute-force scan is O(n) per query but trivially correct and cache-friendly at small scale. Product quantization compresses vectors so millions fit in memory at the cost of some precision. Query and document embeddings can be asymmetric when the query carries a retrieval instruction. See [[vector-search/110-recall-vs-latency]].

## The Embedding Space

Query and document embeddings can be asymmetric when the query carries a retrieval instruction. A reranker rescouring the top candidates with a cross-encoder usually beats a bigger index. The centroid of a document's chunk vectors is a cheap coarse filter before the exact rescan. Product quantization compresses vectors so millions fit in memory at the cost of some precision. An exact brute-force scan is O(n) per query but trivially correct and cache-friendly at small scale. Chunk size is the quiet lever: too small and you embed noise, too large and you blur the signal. See [[productivity/146-timeboxing]].

HNSW trades a little recall for a large latency win by walking a navigable small-world graph. A reranker rescouring the top candidates with a cross-encoder usually beats a bigger index. Hybrid search fuses lexical BM25 scores with dense similarity, often via reciprocal rank fusion. See [[hiking/139-blister-prevention]].

## Chunking Strategy

An exact brute-force scan is O(n) per query but trivially correct and cache-friendly at small scale. Recall@k measures how many of the true neighbors survive the approximate search. A reranker rescouring the top candidates with a cross-encoder usually beats a bigger index. See [[pkm/053-the-zettelkasten]].

HNSW trades a little recall for a large latency win by walking a navigable small-world graph. Normalizing to unit length lets you rank by cosine using a plain L2 distance. The centroid of a document's chunk vectors is a cheap coarse filter before the exact rescan. See [[vector-search/120-the-embedding-space]]. See [[vector-search/050-approximate-nearest-neighbors]].

An exact brute-force scan is O(n) per query but trivially correct and cache-friendly at small scale. Chunk size is the quiet lever: too small and you embed noise, too large and you blur the signal. Hybrid search fuses lexical BM25 scores with dense similarity, often via reciprocal rank fusion. See [[vector-search/200-recall-vs-latency]]. See [[vector-search/080-hnsw-graphs]].

## Hybrid Retrieval

The centroid of a document's chunk vectors is a cheap coarse filter before the exact rescan. HNSW trades a little recall for a large latency win by walking a navigable small-world graph. An exact brute-force scan is O(n) per query but trivially correct and cache-friendly at small scale. Normalizing to unit length lets you rank by cosine using a plain L2 distance. See [[vector-search/100-dense-vs-sparse]].

Cosine similarity ranks candidates by the angle between their normalized embeddings. Hybrid search fuses lexical BM25 scores with dense similarity, often via reciprocal rank fusion. HNSW trades a little recall for a large latency win by walking a navigable small-world graph. Chunk size is the quiet lever: too small and you embed noise, too large and you blur the signal. Normalizing to unit length lets you rank by cosine using a plain L2 distance. Recall@k measures how many of the true neighbors survive the approximate search.
