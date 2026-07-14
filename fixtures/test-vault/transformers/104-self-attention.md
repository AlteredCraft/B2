---
b2id: 01KXF21DYTRW0BD9GJ2S6M3QZ1
type: note
title: "Self-Attention"
---

# Self-Attention

Notes on self-attention within the broader theme of transformer models.

## Context Windows

The feed-forward block applies the same two-layer network independently at each position. Positional encodings inject order into a model that is otherwise permutation-invariant. BERT is an encoder stack trained with masked-language modeling to produce contextual embeddings. A subword tokenizer splits rare words into pieces so the vocabulary stays bounded. Distillation trains a small student to mimic a large teacher, trading accuracy for speed. A model truncates inputs beyond its context window, so long documents must be chunked.

The forward pass cost is dominated by matrix multiplies, which favor batching. Running inference in f16 halves memory and often speeds up matmul on a capable GPU. A model truncates inputs beyond its context window, so long documents must be chunked. Fine-tuning adapts a pretrained model to a task; prompting steers it without weight updates. BERT is an encoder stack trained with masked-language modeling to produce contextual embeddings. Positional encodings inject order into a model that is otherwise permutation-invariant. See [[transformers/184-context-windows]]. See [[transformers/124-positional-encoding]].

Distillation trains a small student to mimic a large teacher, trading accuracy for speed. A subword tokenizer splits rare words into pieces so the vocabulary stays bounded. BERT is an encoder stack trained with masked-language modeling to produce contextual embeddings. A model truncates inputs beyond its context window, so long documents must be chunked. Fine-tuning adapts a pretrained model to a task; prompting steers it without weight updates. Running inference in f16 halves memory and often speeds up matmul on a capable GPU. See [[transformers/134-tokenization]]. See [[transformers/114-fine-tuning-vs-prompting]].

## Layer Normalization

Distillation trains a small student to mimic a large teacher, trading accuracy for speed. Layer normalization stabilizes training by rescaling activations within each layer. Self-attention lets every token weigh every other token when building its representation. The forward pass cost is dominated by matrix multiplies, which favor batching. Positional encodings inject order into a model that is otherwise permutation-invariant. A model truncates inputs beyond its context window, so long documents must be chunked.

BERT is an encoder stack trained with masked-language modeling to produce contextual embeddings. A subword tokenizer splits rare words into pieces so the vocabulary stays bounded. A model truncates inputs beyond its context window, so long documents must be chunked. Distillation trains a small student to mimic a large teacher, trading accuracy for speed. Fine-tuning adapts a pretrained model to a task; prompting steers it without weight updates. Running inference in f16 halves memory and often speeds up matmul on a capable GPU. See [[productivity/166-energy-management]].

Self-attention lets every token weigh every other token when building its representation. A subword tokenizer splits rare words into pieces so the vocabulary stays bounded. The feed-forward block applies the same two-layer network independently at each position. Distillation trains a small student to mimic a large teacher, trading accuracy for speed. Fine-tuning adapts a pretrained model to a task; prompting steers it without weight updates. See [[coffee/098-freshness-and-degassing]]. See [[transformers/054-tokenization]].

## Distillation

Distillation trains a small student to mimic a large teacher, trading accuracy for speed. Fine-tuning adapts a pretrained model to a task; prompting steers it without weight updates. A model truncates inputs beyond its context window, so long documents must be chunked. See [[pkm/083-spaced-repetition]]. See [[transformers/114-fine-tuning-vs-prompting]].

A subword tokenizer splits rare words into pieces so the vocabulary stays bounded. The forward pass cost is dominated by matrix multiplies, which favor batching. Self-attention lets every token weigh every other token when building its representation. A model truncates inputs beyond its context window, so long documents must be chunked. Positional encodings inject order into a model that is otherwise permutation-invariant. The feed-forward block applies the same two-layer network independently at each position. See [[transformers/004-cls-pooling]]. See [[gardening/177-mulching]].

## Fine-Tuning vs Prompting

Running inference in f16 halves memory and often speeds up matmul on a capable GPU. CLS pooling takes the first token's final hidden state as a sentence-level embedding. Distillation trains a small student to mimic a large teacher, trading accuracy for speed. The forward pass cost is dominated by matrix multiplies, which favor batching. Fine-tuning adapts a pretrained model to a task; prompting steers it without weight updates. See [[transformers/014-context-windows]]. See [[transformers/174-positional-encoding]].

CLS pooling takes the first token's final hidden state as a sentence-level embedding. Running inference in f16 halves memory and often speeds up matmul on a capable GPU. Distillation trains a small student to mimic a large teacher, trading accuracy for speed. BERT is an encoder stack trained with masked-language modeling to produce contextual embeddings. Positional encodings inject order into a model that is otherwise permutation-invariant. See [[transformers/144-the-feed-forward-block]]. See [[transformers/144-the-feed-forward-block]].

## Distillation

Positional encodings inject order into a model that is otherwise permutation-invariant. Self-attention lets every token weigh every other token when building its representation. Distillation trains a small student to mimic a large teacher, trading accuracy for speed. BERT is an encoder stack trained with masked-language modeling to produce contextual embeddings. Running inference in f16 halves memory and often speeds up matmul on a capable GPU. See [[pkm/013-evergreen-notes]]. See [[transformers/114-fine-tuning-vs-prompting]].

Fine-tuning adapts a pretrained model to a task; prompting steers it without weight updates. A model truncates inputs beyond its context window, so long documents must be chunked. Positional encodings inject order into a model that is otherwise permutation-invariant. Running inference in f16 halves memory and often speeds up matmul on a capable GPU. A subword tokenizer splits rare words into pieces so the vocabulary stays bounded. Self-attention lets every token weigh every other token when building its representation.

CLS pooling takes the first token's final hidden state as a sentence-level embedding. The forward pass cost is dominated by matrix multiplies, which favor batching. Running inference in f16 halves memory and often speeds up matmul on a capable GPU. Distillation trains a small student to mimic a large teacher, trading accuracy for speed. See [[transformers/174-positional-encoding]]. See [[databases/025-vacuuming]].

## Distillation

Positional encodings inject order into a model that is otherwise permutation-invariant. BERT is an encoder stack trained with masked-language modeling to produce contextual embeddings. Distillation trains a small student to mimic a large teacher, trading accuracy for speed. Running inference in f16 halves memory and often speeds up matmul on a capable GPU.

CLS pooling takes the first token's final hidden state as a sentence-level embedding. Running inference in f16 halves memory and often speeds up matmul on a capable GPU. BERT is an encoder stack trained with masked-language modeling to produce contextual embeddings. Self-attention lets every token weigh every other token when building its representation. See [[transformers/014-context-windows]].
