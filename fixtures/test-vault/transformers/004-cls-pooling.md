---
b2id: 01KXF21DYKK2Z8YXA821PFD1QC
type: note
title: "CLS Pooling"
---

# CLS Pooling

Notes on cls pooling within the broader theme of transformer models.

## The Feed-Forward Block

A subword tokenizer splits rare words into pieces so the vocabulary stays bounded. BERT is an encoder stack trained with masked-language modeling to produce contextual embeddings. Fine-tuning adapts a pretrained model to a task; prompting steers it without weight updates. A model truncates inputs beyond its context window, so long documents must be chunked. Self-attention lets every token weigh every other token when building its representation. Layer normalization stabilizes training by rescaling activations within each layer.

The feed-forward block applies the same two-layer network independently at each position. BERT is an encoder stack trained with masked-language modeling to produce contextual embeddings. Positional encodings inject order into a model that is otherwise permutation-invariant. The forward pass cost is dominated by matrix multiplies, which favor batching. Layer normalization stabilizes training by rescaling activations within each layer.

## Tokenization

Running inference in f16 halves memory and often speeds up matmul on a capable GPU. Layer normalization stabilizes training by rescaling activations within each layer. A subword tokenizer splits rare words into pieces so the vocabulary stays bounded. Fine-tuning adapts a pretrained model to a task; prompting steers it without weight updates. Distillation trains a small student to mimic a large teacher, trading accuracy for speed. BERT is an encoder stack trained with masked-language modeling to produce contextual embeddings. See [[transformers/194-distillation]]. See [[vector-search/110-recall-vs-latency]].

The feed-forward block applies the same two-layer network independently at each position. Layer normalization stabilizes training by rescaling activations within each layer. Running inference in f16 halves memory and often speeds up matmul on a capable GPU. CLS pooling takes the first token's final hidden state as a sentence-level embedding. Distillation trains a small student to mimic a large teacher, trading accuracy for speed.

Running inference in f16 halves memory and often speeds up matmul on a capable GPU. Layer normalization stabilizes training by rescaling activations within each layer. The feed-forward block applies the same two-layer network independently at each position. A subword tokenizer splits rare words into pieces so the vocabulary stays bounded. A model truncates inputs beyond its context window, so long documents must be chunked. The forward pass cost is dominated by matrix multiplies, which favor batching.

## Context Windows

Fine-tuning adapts a pretrained model to a task; prompting steers it without weight updates. A model truncates inputs beyond its context window, so long documents must be chunked. Layer normalization stabilizes training by rescaling activations within each layer. Distillation trains a small student to mimic a large teacher, trading accuracy for speed. CLS pooling takes the first token's final hidden state as a sentence-level embedding. BERT is an encoder stack trained with masked-language modeling to produce contextual embeddings. See [[distributed-systems/141-consensus-under-partition]].

A model truncates inputs beyond its context window, so long documents must be chunked. Positional encodings inject order into a model that is otherwise permutation-invariant. Layer normalization stabilizes training by rescaling activations within each layer. A subword tokenizer splits rare words into pieces so the vocabulary stays bounded. The forward pass cost is dominated by matrix multiplies, which favor batching. See [[transformers/014-context-windows]].
