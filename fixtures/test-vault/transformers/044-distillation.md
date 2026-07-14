---
b2id: 01KXF21DYNRCA2Q45JWXBMC566
type: note
title: "Distillation"
---

# Distillation

Notes on distillation within the broader theme of transformer models.

## Positional Encoding

CLS pooling takes the first token's final hidden state as a sentence-level embedding. Distillation trains a small student to mimic a large teacher, trading accuracy for speed. The forward pass cost is dominated by matrix multiplies, which favor batching. A model truncates inputs beyond its context window, so long documents must be chunked. See [[vector-search/160-the-embedding-space]].

Self-attention lets every token weigh every other token when building its representation. The feed-forward block applies the same two-layer network independently at each position. Fine-tuning adapts a pretrained model to a task; prompting steers it without weight updates. CLS pooling takes the first token's final hidden state as a sentence-level embedding. Running inference in f16 halves memory and often speeds up matmul on a capable GPU. A model truncates inputs beyond its context window, so long documents must be chunked. See [[transformers/004-cls-pooling]].

BERT is an encoder stack trained with masked-language modeling to produce contextual embeddings. The forward pass cost is dominated by matrix multiplies, which favor batching. CLS pooling takes the first token's final hidden state as a sentence-level embedding. Running inference in f16 halves memory and often speeds up matmul on a capable GPU. Fine-tuning adapts a pretrained model to a task; prompting steers it without weight updates. Positional encodings inject order into a model that is otherwise permutation-invariant.

## Distillation

The feed-forward block applies the same two-layer network independently at each position. A model truncates inputs beyond its context window, so long documents must be chunked. Self-attention lets every token weigh every other token when building its representation. CLS pooling takes the first token's final hidden state as a sentence-level embedding. Layer normalization stabilizes training by rescaling activations within each layer. Running inference in f16 halves memory and often speeds up matmul on a capable GPU. See [[transformers/104-self-attention]].

Positional encodings inject order into a model that is otherwise permutation-invariant. A model truncates inputs beyond its context window, so long documents must be chunked. Fine-tuning adapts a pretrained model to a task; prompting steers it without weight updates. CLS pooling takes the first token's final hidden state as a sentence-level embedding. Running inference in f16 halves memory and often speeds up matmul on a capable GPU. See [[transformers/174-positional-encoding]].

The forward pass cost is dominated by matrix multiplies, which favor batching. Self-attention lets every token weigh every other token when building its representation. A model truncates inputs beyond its context window, so long documents must be chunked. Distillation trains a small student to mimic a large teacher, trading accuracy for speed. A subword tokenizer splits rare words into pieces so the vocabulary stays bounded. Running inference in f16 halves memory and often speeds up matmul on a capable GPU.

## The BERT Encoder

A model truncates inputs beyond its context window, so long documents must be chunked. Running inference in f16 halves memory and often speeds up matmul on a capable GPU. A subword tokenizer splits rare words into pieces so the vocabulary stays bounded. Positional encodings inject order into a model that is otherwise permutation-invariant.

The feed-forward block applies the same two-layer network independently at each position. Distillation trains a small student to mimic a large teacher, trading accuracy for speed. Fine-tuning adapts a pretrained model to a task; prompting steers it without weight updates.

Layer normalization stabilizes training by rescaling activations within each layer. BERT is an encoder stack trained with masked-language modeling to produce contextual embeddings. The feed-forward block applies the same two-layer network independently at each position. Self-attention lets every token weigh every other token when building its representation. A model truncates inputs beyond its context window, so long documents must be chunked. Fine-tuning adapts a pretrained model to a task; prompting steers it without weight updates. See [[transformers/194-distillation]].

## The Feed-Forward Block

The forward pass cost is dominated by matrix multiplies, which favor batching. Layer normalization stabilizes training by rescaling activations within each layer. Fine-tuning adapts a pretrained model to a task; prompting steers it without weight updates. Positional encodings inject order into a model that is otherwise permutation-invariant. Distillation trains a small student to mimic a large teacher, trading accuracy for speed.

Fine-tuning adapts a pretrained model to a task; prompting steers it without weight updates. Self-attention lets every token weigh every other token when building its representation. Running inference in f16 halves memory and often speeds up matmul on a capable GPU. The feed-forward block applies the same two-layer network independently at each position. The forward pass cost is dominated by matrix multiplies, which favor batching. See [[transformers/104-self-attention]].
