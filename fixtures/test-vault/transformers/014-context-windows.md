---
b2id: 01KXF21DYK4GF2M6N1BEZPPZGK
type: note
title: "Context Windows"
---

# Context Windows

Notes on context windows within the broader theme of transformer models.

## Positional Encoding

Positional encodings inject order into a model that is otherwise permutation-invariant. CLS pooling takes the first token's final hidden state as a sentence-level embedding. The feed-forward block applies the same two-layer network independently at each position. Self-attention lets every token weigh every other token when building its representation. Distillation trains a small student to mimic a large teacher, trading accuracy for speed. Running inference in f16 halves memory and often speeds up matmul on a capable GPU. See [[transformers/064-fine-tuning-vs-prompting]].

The feed-forward block applies the same two-layer network independently at each position. Self-attention lets every token weigh every other token when building its representation. Layer normalization stabilizes training by rescaling activations within each layer. BERT is an encoder stack trained with masked-language modeling to produce contextual embeddings. A subword tokenizer splits rare words into pieces so the vocabulary stays bounded. A model truncates inputs beyond its context window, so long documents must be chunked. See [[transformers/114-fine-tuning-vs-prompting]].

## CLS Pooling

BERT is an encoder stack trained with masked-language modeling to produce contextual embeddings. Fine-tuning adapts a pretrained model to a task; prompting steers it without weight updates. Self-attention lets every token weigh every other token when building its representation. See [[transformers/174-positional-encoding]]. See [[transformers/164-the-bert-encoder]].

BERT is an encoder stack trained with masked-language modeling to produce contextual embeddings. Self-attention lets every token weigh every other token when building its representation. A model truncates inputs beyond its context window, so long documents must be chunked. A subword tokenizer splits rare words into pieces so the vocabulary stays bounded. Fine-tuning adapts a pretrained model to a task; prompting steers it without weight updates. The feed-forward block applies the same two-layer network independently at each position.

## Context Windows

A model truncates inputs beyond its context window, so long documents must be chunked. Distillation trains a small student to mimic a large teacher, trading accuracy for speed. The forward pass cost is dominated by matrix multiplies, which favor batching. Positional encodings inject order into a model that is otherwise permutation-invariant. Fine-tuning adapts a pretrained model to a task; prompting steers it without weight updates. Running inference in f16 halves memory and often speeds up matmul on a capable GPU. See [[transformers/074-self-attention]].

Layer normalization stabilizes training by rescaling activations within each layer. A model truncates inputs beyond its context window, so long documents must be chunked. Self-attention lets every token weigh every other token when building its representation. The forward pass cost is dominated by matrix multiplies, which favor batching. BERT is an encoder stack trained with masked-language modeling to produce contextual embeddings.

Layer normalization stabilizes training by rescaling activations within each layer. A subword tokenizer splits rare words into pieces so the vocabulary stays bounded. The feed-forward block applies the same two-layer network independently at each position. Self-attention lets every token weigh every other token when building its representation. Running inference in f16 halves memory and often speeds up matmul on a capable GPU. See [[transformers/124-positional-encoding]].

## The Feed-Forward Block

BERT is an encoder stack trained with masked-language modeling to produce contextual embeddings. Self-attention lets every token weigh every other token when building its representation. The feed-forward block applies the same two-layer network independently at each position. Distillation trains a small student to mimic a large teacher, trading accuracy for speed. Layer normalization stabilizes training by rescaling activations within each layer.

Distillation trains a small student to mimic a large teacher, trading accuracy for speed. BERT is an encoder stack trained with masked-language modeling to produce contextual embeddings. A model truncates inputs beyond its context window, so long documents must be chunked. Positional encodings inject order into a model that is otherwise permutation-invariant. Self-attention lets every token weigh every other token when building its representation. See [[transformers/044-distillation]]. See [[transformers/134-tokenization]].
