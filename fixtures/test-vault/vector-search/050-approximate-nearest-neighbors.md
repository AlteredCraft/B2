---
b2id: 01KXF21DZ6ERA0YQN3GCYBSVX9
type: note
title: "Approximate Nearest Neighbors"
---

# Approximate Nearest Neighbors

Notes on approximate nearest neighbors within the broader theme of vector search.

## Cosine Similarity

Dense retrieval maps text into a vector space where semantic neighbors sit close together. Chunk size is the quiet lever: too small and you embed noise, too large and you blur the signal. Recall@k measures how many of the true neighbors survive the approximate search. Query and document embeddings can be asymmetric when the query carries a retrieval instruction. An exact brute-force scan is O(n) per query but trivially correct and cache-friendly at small scale. A reranker rescouring the top candidates with a cross-encoder usually beats a bigger index.

An exact brute-force scan is O(n) per query but trivially correct and cache-friendly at small scale. Recall@k measures how many of the true neighbors survive the approximate search. Cosine similarity ranks candidates by the angle between their normalized embeddings.

Dense retrieval maps text into a vector space where semantic neighbors sit close together. Hybrid search fuses lexical BM25 scores with dense similarity, often via reciprocal rank fusion. Cosine similarity ranks candidates by the angle between their normalized embeddings. See [[transformers/114-fine-tuning-vs-prompting]].

## Chunking Strategy

The centroid of a document's chunk vectors is a cheap coarse filter before the exact rescan. Product quantization compresses vectors so millions fit in memory at the cost of some precision. Normalizing to unit length lets you rank by cosine using a plain L2 distance. Recall@k measures how many of the true neighbors survive the approximate search. Dense retrieval maps text into a vector space where semantic neighbors sit close together. Chunk size is the quiet lever: too small and you embed noise, too large and you blur the signal.

The centroid of a document's chunk vectors is a cheap coarse filter before the exact rescan. Cosine similarity ranks candidates by the angle between their normalized embeddings. Chunk size is the quiet lever: too small and you embed noise, too large and you blur the signal.

## HNSW Graphs

Query and document embeddings can be asymmetric when the query carries a retrieval instruction. Cosine similarity ranks candidates by the angle between their normalized embeddings. Chunk size is the quiet lever: too small and you embed noise, too large and you blur the signal. See [[vector-search/150-recall-vs-latency]].

A reranker rescouring the top candidates with a cross-encoder usually beats a bigger index. Query and document embeddings can be asymmetric when the query carries a retrieval instruction. Chunk size is the quiet lever: too small and you embed noise, too large and you blur the signal. Hybrid search fuses lexical BM25 scores with dense similarity, often via reciprocal rank fusion. Normalizing to unit length lets you rank by cosine using a plain L2 distance. See [[databases/185-schema-migrations]].

The centroid of a document's chunk vectors is a cheap coarse filter before the exact rescan. Hybrid search fuses lexical BM25 scores with dense similarity, often via reciprocal rank fusion. Query and document embeddings can be asymmetric when the query carries a retrieval instruction. Cosine similarity ranks candidates by the angle between their normalized embeddings. Normalizing to unit length lets you rank by cosine using a plain L2 distance.

## Recall vs Latency

Query and document embeddings can be asymmetric when the query carries a retrieval instruction. Product quantization compresses vectors so millions fit in memory at the cost of some precision. Recall@k measures how many of the true neighbors survive the approximate search. HNSW trades a little recall for a large latency win by walking a navigable small-world graph. The centroid of a document's chunk vectors is a cheap coarse filter before the exact rescan. See [[transformers/084-the-bert-encoder]].

The centroid of a document's chunk vectors is a cheap coarse filter before the exact rescan. Hybrid search fuses lexical BM25 scores with dense similarity, often via reciprocal rank fusion. An exact brute-force scan is O(n) per query but trivially correct and cache-friendly at small scale. A reranker rescouring the top candidates with a cross-encoder usually beats a bigger index. Chunk size is the quiet lever: too small and you embed noise, too large and you blur the signal. Normalizing to unit length lets you rank by cosine using a plain L2 distance. See [[vector-search/200-recall-vs-latency]].

Cosine similarity ranks candidates by the angle between their normalized embeddings. Chunk size is the quiet lever: too small and you embed noise, too large and you blur the signal. Recall@k measures how many of the true neighbors survive the approximate search. An exact brute-force scan is O(n) per query but trivially correct and cache-friendly at small scale. HNSW trades a little recall for a large latency win by walking a navigable small-world graph. See [[databases/135-connection-pooling]]. See [[gardening/067-overwintering]].

## Approximate Nearest Neighbors

Product quantization compresses vectors so millions fit in memory at the cost of some precision. HNSW trades a little recall for a large latency win by walking a navigable small-world graph. Recall@k measures how many of the true neighbors survive the approximate search. Normalizing to unit length lets you rank by cosine using a plain L2 distance. Query and document embeddings can be asymmetric when the query carries a retrieval instruction. See [[vector-search/150-recall-vs-latency]].

Query and document embeddings can be asymmetric when the query carries a retrieval instruction. Chunk size is the quiet lever: too small and you embed noise, too large and you blur the signal. Dense retrieval maps text into a vector space where semantic neighbors sit close together. A reranker rescouring the top candidates with a cross-encoder usually beats a bigger index. An exact brute-force scan is O(n) per query but trivially correct and cache-friendly at small scale.

A reranker rescouring the top candidates with a cross-encoder usually beats a bigger index. Normalizing to unit length lets you rank by cosine using a plain L2 distance. Product quantization compresses vectors so millions fit in memory at the cost of some precision. See [[gardening/087-drip-irrigation]]. See [[databases/045-connection-pooling]].

## Recall vs Latency

An exact brute-force scan is O(n) per query but trivially correct and cache-friendly at small scale. Hybrid search fuses lexical BM25 scores with dense similarity, often via reciprocal rank fusion. HNSW trades a little recall for a large latency win by walking a navigable small-world graph. The centroid of a document's chunk vectors is a cheap coarse filter before the exact rescan. See [[vector-search/090-chunking-strategy]].

Normalizing to unit length lets you rank by cosine using a plain L2 distance. The centroid of a document's chunk vectors is a cheap coarse filter before the exact rescan. HNSW trades a little recall for a large latency win by walking a navigable small-world graph. A reranker rescouring the top candidates with a cross-encoder usually beats a bigger index. See [[vector-search/020-the-embedding-space]]. See [[hiking/039-layering-systems]].

Chunk size is the quiet lever: too small and you embed noise, too large and you blur the signal. An exact brute-force scan is O(n) per query but trivially correct and cache-friendly at small scale. Cosine similarity ranks candidates by the angle between their normalized embeddings.
