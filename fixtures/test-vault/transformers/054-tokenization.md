---
b2id: 01KXF21DYP7NRW4SMXEH4VZZP7
type: note
title: "Tokenization"
---

# Tokenization

Notes on tokenization within the broader theme of transformer models.

## Self-Attention

Self-attention lets every token weigh every other token when building its representation. Positional encodings inject order into a model that is otherwise permutation-invariant. Fine-tuning adapts a pretrained model to a task; prompting steers it without weight updates. Running inference in f16 halves memory and often speeds up matmul on a capable GPU. A model truncates inputs beyond its context window, so long documents must be chunked. See [[hiking/199-blister-prevention]].

A model truncates inputs beyond its context window, so long documents must be chunked. Positional encodings inject order into a model that is otherwise permutation-invariant. CLS pooling takes the first token's final hidden state as a sentence-level embedding. Layer normalization stabilizes training by rescaling activations within each layer. Self-attention lets every token weigh every other token when building its representation. Distillation trains a small student to mimic a large teacher, trading accuracy for speed. See [[transformers/154-self-attention]].

## Context Windows

Running inference in f16 halves memory and often speeds up matmul on a capable GPU. The forward pass cost is dominated by matrix multiplies, which favor batching. Self-attention lets every token weigh every other token when building its representation. Fine-tuning adapts a pretrained model to a task; prompting steers it without weight updates. Positional encodings inject order into a model that is otherwise permutation-invariant.

A model truncates inputs beyond its context window, so long documents must be chunked. Distillation trains a small student to mimic a large teacher, trading accuracy for speed. Fine-tuning adapts a pretrained model to a task; prompting steers it without weight updates.

## Positional Encoding

Distillation trains a small student to mimic a large teacher, trading accuracy for speed. BERT is an encoder stack trained with masked-language modeling to produce contextual embeddings. The forward pass cost is dominated by matrix multiplies, which favor batching.

Positional encodings inject order into a model that is otherwise permutation-invariant. Layer normalization stabilizes training by rescaling activations within each layer. Distillation trains a small student to mimic a large teacher, trading accuracy for speed. Self-attention lets every token weigh every other token when building its representation.

A subword tokenizer splits rare words into pieces so the vocabulary stays bounded. BERT is an encoder stack trained with masked-language modeling to produce contextual embeddings. Running inference in f16 halves memory and often speeds up matmul on a capable GPU. Distillation trains a small student to mimic a large teacher, trading accuracy for speed. See [[transformers/084-the-bert-encoder]]. See [[distributed-systems/161-idempotent-writes]].

## CLS Pooling

A model truncates inputs beyond its context window, so long documents must be chunked. Layer normalization stabilizes training by rescaling activations within each layer. Self-attention lets every token weigh every other token when building its representation. The forward pass cost is dominated by matrix multiplies, which favor batching. Fine-tuning adapts a pretrained model to a task; prompting steers it without weight updates. Positional encodings inject order into a model that is otherwise permutation-invariant. See [[rust/032-the-newtype-pattern]]. See [[databases/065-vacuuming]].

Fine-tuning adapts a pretrained model to a task; prompting steers it without weight updates. Self-attention lets every token weigh every other token when building its representation. CLS pooling takes the first token's final hidden state as a sentence-level embedding.

Distillation trains a small student to mimic a large teacher, trading accuracy for speed. CLS pooling takes the first token's final hidden state as a sentence-level embedding. A model truncates inputs beyond its context window, so long documents must be chunked. BERT is an encoder stack trained with masked-language modeling to produce contextual embeddings. See [[transformers/184-context-windows]]. See [[transformers/014-context-windows]].

## Fine-Tuning vs Prompting

The forward pass cost is dominated by matrix multiplies, which favor batching. CLS pooling takes the first token's final hidden state as a sentence-level embedding. Distillation trains a small student to mimic a large teacher, trading accuracy for speed. Positional encodings inject order into a model that is otherwise permutation-invariant. Fine-tuning adapts a pretrained model to a task; prompting steers it without weight updates. See [[transformers/034-the-feed-forward-block]]. See [[productivity/116-energy-management]].

Distillation trains a small student to mimic a large teacher, trading accuracy for speed. A model truncates inputs beyond its context window, so long documents must be chunked. Positional encodings inject order into a model that is otherwise permutation-invariant. CLS pooling takes the first token's final hidden state as a sentence-level embedding. Running inference in f16 halves memory and often speeds up matmul on a capable GPU. See [[transformers/094-fine-tuning-vs-prompting]].

Distillation trains a small student to mimic a large teacher, trading accuracy for speed. The forward pass cost is dominated by matrix multiplies, which favor batching. CLS pooling takes the first token's final hidden state as a sentence-level embedding. A model truncates inputs beyond its context window, so long documents must be chunked. Running inference in f16 halves memory and often speeds up matmul on a capable GPU.
