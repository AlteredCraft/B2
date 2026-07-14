---
b2id: 01KXF21DZ8V3G7G7JHMYVFHR7W
type: note
title: "Cosine Similarity"
---

# Cosine Similarity

Notes on cosine similarity within the broader theme of vector search.

## Cosine Similarity

Query and document embeddings can be asymmetric when the query carries a retrieval instruction. The centroid of a document's chunk vectors is a cheap coarse filter before the exact rescan. Recall@k measures how many of the true neighbors survive the approximate search. See [[distributed-systems/041-leader-election]]. See [[coffee/048-the-espresso-shot]].

Hybrid search fuses lexical BM25 scores with dense similarity, often via reciprocal rank fusion. Query and document embeddings can be asymmetric when the query carries a retrieval instruction. An exact brute-force scan is O(n) per query but trivially correct and cache-friendly at small scale. HNSW trades a little recall for a large latency win by walking a navigable small-world graph. Cosine similarity ranks candidates by the angle between their normalized embeddings. The centroid of a document's chunk vectors is a cheap coarse filter before the exact rescan. See [[vector-search/120-the-embedding-space]].

## Hybrid Retrieval

Query and document embeddings can be asymmetric when the query carries a retrieval instruction. The centroid of a document's chunk vectors is a cheap coarse filter before the exact rescan. HNSW trades a little recall for a large latency win by walking a navigable small-world graph. See [[vector-search/080-hnsw-graphs]].

Product quantization compresses vectors so millions fit in memory at the cost of some precision. Normalizing to unit length lets you rank by cosine using a plain L2 distance. The centroid of a document's chunk vectors is a cheap coarse filter before the exact rescan. See [[productivity/096-timeboxing]].

## HNSW Graphs

An exact brute-force scan is O(n) per query but trivially correct and cache-friendly at small scale. Normalizing to unit length lets you rank by cosine using a plain L2 distance. A reranker rescouring the top candidates with a cross-encoder usually beats a bigger index. Dense retrieval maps text into a vector space where semantic neighbors sit close together. HNSW trades a little recall for a large latency win by walking a navigable small-world graph.

An exact brute-force scan is O(n) per query but trivially correct and cache-friendly at small scale. Normalizing to unit length lets you rank by cosine using a plain L2 distance. Cosine similarity ranks candidates by the angle between their normalized embeddings. Query and document embeddings can be asymmetric when the query carries a retrieval instruction. The centroid of a document's chunk vectors is a cheap coarse filter before the exact rescan.

## Cosine Similarity

An exact brute-force scan is O(n) per query but trivially correct and cache-friendly at small scale. Dense retrieval maps text into a vector space where semantic neighbors sit close together. Normalizing to unit length lets you rank by cosine using a plain L2 distance. Query and document embeddings can be asymmetric when the query carries a retrieval instruction.

Chunk size is the quiet lever: too small and you embed noise, too large and you blur the signal. Dense retrieval maps text into a vector space where semantic neighbors sit close together. A reranker rescouring the top candidates with a cross-encoder usually beats a bigger index. See [[gardening/157-composting-basics]].

## HNSW Graphs

A reranker rescouring the top candidates with a cross-encoder usually beats a bigger index. Dense retrieval maps text into a vector space where semantic neighbors sit close together. An exact brute-force scan is O(n) per query but trivially correct and cache-friendly at small scale.

Normalizing to unit length lets you rank by cosine using a plain L2 distance. Chunk size is the quiet lever: too small and you embed noise, too large and you blur the signal. Query and document embeddings can be asymmetric when the query carries a retrieval instruction. A reranker rescouring the top candidates with a cross-encoder usually beats a bigger index. Cosine similarity ranks candidates by the angle between their normalized embeddings. See [[vector-search/020-the-embedding-space]].
