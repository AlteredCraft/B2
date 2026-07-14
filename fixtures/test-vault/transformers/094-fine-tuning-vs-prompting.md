---
b2id: 01KXF21DYSSD2P63AMWFN8V9RF
type: note
title: "Fine-Tuning vs Prompting"
---

# Fine-Tuning vs Prompting

Notes on fine-tuning vs prompting within the broader theme of transformer models.

## Fine-Tuning vs Prompting

The feed-forward block applies the same two-layer network independently at each position. A subword tokenizer splits rare words into pieces so the vocabulary stays bounded. A model truncates inputs beyond its context window, so long documents must be chunked. Positional encodings inject order into a model that is otherwise permutation-invariant.

The feed-forward block applies the same two-layer network independently at each position. Running inference in f16 halves memory and often speeds up matmul on a capable GPU. Distillation trains a small student to mimic a large teacher, trading accuracy for speed. See [[transformers/144-the-feed-forward-block]].

## Distillation

Distillation trains a small student to mimic a large teacher, trading accuracy for speed. Layer normalization stabilizes training by rescaling activations within each layer. Positional encodings inject order into a model that is otherwise permutation-invariant. Running inference in f16 halves memory and often speeds up matmul on a capable GPU. The forward pass cost is dominated by matrix multiplies, which favor batching. See [[distributed-systems/061-quorum-reads]].

A model truncates inputs beyond its context window, so long documents must be chunked. Fine-tuning adapts a pretrained model to a task; prompting steers it without weight updates. Positional encodings inject order into a model that is otherwise permutation-invariant. A subword tokenizer splits rare words into pieces so the vocabulary stays bounded. Layer normalization stabilizes training by rescaling activations within each layer. See [[transformers/024-layer-normalization]]. See [[transformers/164-the-bert-encoder]].

CLS pooling takes the first token's final hidden state as a sentence-level embedding. Positional encodings inject order into a model that is otherwise permutation-invariant. Fine-tuning adapts a pretrained model to a task; prompting steers it without weight updates. Distillation trains a small student to mimic a large teacher, trading accuracy for speed. A subword tokenizer splits rare words into pieces so the vocabulary stays bounded. Running inference in f16 halves memory and often speeds up matmul on a capable GPU.

## CLS Pooling

The feed-forward block applies the same two-layer network independently at each position. A model truncates inputs beyond its context window, so long documents must be chunked. Positional encodings inject order into a model that is otherwise permutation-invariant. See [[transformers/004-cls-pooling]]. See [[productivity/066-shipping-small]].

Running inference in f16 halves memory and often speeds up matmul on a capable GPU. Self-attention lets every token weigh every other token when building its representation. Positional encodings inject order into a model that is otherwise permutation-invariant. Fine-tuning adapts a pretrained model to a task; prompting steers it without weight updates. A subword tokenizer splits rare words into pieces so the vocabulary stays bounded. A model truncates inputs beyond its context window, so long documents must be chunked. See [[distributed-systems/121-consensus-under-partition]].

Layer normalization stabilizes training by rescaling activations within each layer. Positional encodings inject order into a model that is otherwise permutation-invariant. A subword tokenizer splits rare words into pieces so the vocabulary stays bounded.

## Distillation

CLS pooling takes the first token's final hidden state as a sentence-level embedding. The feed-forward block applies the same two-layer network independently at each position. A subword tokenizer splits rare words into pieces so the vocabulary stays bounded. A model truncates inputs beyond its context window, so long documents must be chunked. Fine-tuning adapts a pretrained model to a task; prompting steers it without weight updates.

The forward pass cost is dominated by matrix multiplies, which favor batching. Running inference in f16 halves memory and often speeds up matmul on a capable GPU. Distillation trains a small student to mimic a large teacher, trading accuracy for speed. Fine-tuning adapts a pretrained model to a task; prompting steers it without weight updates. See [[transformers/124-positional-encoding]]. See [[transformers/134-tokenization]].

Distillation trains a small student to mimic a large teacher, trading accuracy for speed. A model truncates inputs beyond its context window, so long documents must be chunked. The forward pass cost is dominated by matrix multiplies, which favor batching. The feed-forward block applies the same two-layer network independently at each position. Positional encodings inject order into a model that is otherwise permutation-invariant. Running inference in f16 halves memory and often speeds up matmul on a capable GPU. See [[transformers/144-the-feed-forward-block]]. See [[transformers/074-self-attention]].

## Context Windows

Self-attention lets every token weigh every other token when building its representation. Positional encodings inject order into a model that is otherwise permutation-invariant. A model truncates inputs beyond its context window, so long documents must be chunked. The feed-forward block applies the same two-layer network independently at each position. The forward pass cost is dominated by matrix multiplies, which favor batching. See [[transformers/004-cls-pooling]]. See [[transformers/154-self-attention]].

Distillation trains a small student to mimic a large teacher, trading accuracy for speed. Running inference in f16 halves memory and often speeds up matmul on a capable GPU. Self-attention lets every token weigh every other token when building its representation.

## Fine-Tuning vs Prompting

CLS pooling takes the first token's final hidden state as a sentence-level embedding. Distillation trains a small student to mimic a large teacher, trading accuracy for speed. The forward pass cost is dominated by matrix multiplies, which favor batching. BERT is an encoder stack trained with masked-language modeling to produce contextual embeddings. A model truncates inputs beyond its context window, so long documents must be chunked. See [[hiking/179-reading-the-weather]].

A model truncates inputs beyond its context window, so long documents must be chunked. The forward pass cost is dominated by matrix multiplies, which favor batching. Fine-tuning adapts a pretrained model to a task; prompting steers it without weight updates. Self-attention lets every token weigh every other token when building its representation. The feed-forward block applies the same two-layer network independently at each position.
