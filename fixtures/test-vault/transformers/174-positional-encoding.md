---
b2id: 01KXF21DZ0YFR0FAC2V2RBMH93
type: note
title: "Positional Encoding"
---

# Positional Encoding

Notes on positional encoding within the broader theme of transformer models.

## Layer Normalization

Positional encodings inject order into a model that is otherwise permutation-invariant. Running inference in f16 halves memory and often speeds up matmul on a capable GPU. BERT is an encoder stack trained with masked-language modeling to produce contextual embeddings. See [[pkm/013-evergreen-notes]]. See [[pkm/083-spaced-repetition]].

The feed-forward block applies the same two-layer network independently at each position. A subword tokenizer splits rare words into pieces so the vocabulary stays bounded. Fine-tuning adapts a pretrained model to a task; prompting steers it without weight updates. See [[hiking/149-leave-no-trace]].

## CLS Pooling

Distillation trains a small student to mimic a large teacher, trading accuracy for speed. A model truncates inputs beyond its context window, so long documents must be chunked. The forward pass cost is dominated by matrix multiplies, which favor batching. Running inference in f16 halves memory and often speeds up matmul on a capable GPU. See [[transformers/144-the-feed-forward-block]].

Positional encodings inject order into a model that is otherwise permutation-invariant. Self-attention lets every token weigh every other token when building its representation. A model truncates inputs beyond its context window, so long documents must be chunked. See [[transformers/194-distillation]].

## The Feed-Forward Block

A subword tokenizer splits rare words into pieces so the vocabulary stays bounded. Self-attention lets every token weigh every other token when building its representation. Fine-tuning adapts a pretrained model to a task; prompting steers it without weight updates. A model truncates inputs beyond its context window, so long documents must be chunked. The feed-forward block applies the same two-layer network independently at each position.

A subword tokenizer splits rare words into pieces so the vocabulary stays bounded. BERT is an encoder stack trained with masked-language modeling to produce contextual embeddings. Layer normalization stabilizes training by rescaling activations within each layer.

## Positional Encoding

CLS pooling takes the first token's final hidden state as a sentence-level embedding. Layer normalization stabilizes training by rescaling activations within each layer. The feed-forward block applies the same two-layer network independently at each position. The forward pass cost is dominated by matrix multiplies, which favor batching. Self-attention lets every token weigh every other token when building its representation. A subword tokenizer splits rare words into pieces so the vocabulary stays bounded.

The feed-forward block applies the same two-layer network independently at each position. The forward pass cost is dominated by matrix multiplies, which favor batching. Running inference in f16 halves memory and often speeds up matmul on a capable GPU. Fine-tuning adapts a pretrained model to a task; prompting steers it without weight updates.

## Context Windows

Self-attention lets every token weigh every other token when building its representation. CLS pooling takes the first token's final hidden state as a sentence-level embedding. BERT is an encoder stack trained with masked-language modeling to produce contextual embeddings. Layer normalization stabilizes training by rescaling activations within each layer.

Layer normalization stabilizes training by rescaling activations within each layer. Self-attention lets every token weigh every other token when building its representation. CLS pooling takes the first token's final hidden state as a sentence-level embedding. See [[transformers/084-the-bert-encoder]].

## Fine-Tuning vs Prompting

A subword tokenizer splits rare words into pieces so the vocabulary stays bounded. A model truncates inputs beyond its context window, so long documents must be chunked. Self-attention lets every token weigh every other token when building its representation. Running inference in f16 halves memory and often speeds up matmul on a capable GPU.

Fine-tuning adapts a pretrained model to a task; prompting steers it without weight updates. A model truncates inputs beyond its context window, so long documents must be chunked. Self-attention lets every token weigh every other token when building its representation. Distillation trains a small student to mimic a large teacher, trading accuracy for speed. The forward pass cost is dominated by matrix multiplies, which favor batching. A subword tokenizer splits rare words into pieces so the vocabulary stays bounded.
