---
b2id: 01KXF21DYZXM3SZ6A89KNXZG90
type: note
title: "Self-Attention"
---

# Self-Attention

Notes on self-attention within the broader theme of transformer models.

## Context Windows

Layer normalization stabilizes training by rescaling activations within each layer. Self-attention lets every token weigh every other token when building its representation. The forward pass cost is dominated by matrix multiplies, which favor batching. Fine-tuning adapts a pretrained model to a task; prompting steers it without weight updates. A model truncates inputs beyond its context window, so long documents must be chunked. BERT is an encoder stack trained with masked-language modeling to produce contextual embeddings. See [[transformers/174-positional-encoding]].

Running inference in f16 halves memory and often speeds up matmul on a capable GPU. Self-attention lets every token weigh every other token when building its representation. BERT is an encoder stack trained with masked-language modeling to produce contextual embeddings. Layer normalization stabilizes training by rescaling activations within each layer. See [[transformers/174-positional-encoding]].

The forward pass cost is dominated by matrix multiplies, which favor batching. Running inference in f16 halves memory and often speeds up matmul on a capable GPU. Layer normalization stabilizes training by rescaling activations within each layer. A subword tokenizer splits rare words into pieces so the vocabulary stays bounded. See [[transformers/164-the-bert-encoder]].

## Layer Normalization

The feed-forward block applies the same two-layer network independently at each position. Fine-tuning adapts a pretrained model to a task; prompting steers it without weight updates. Positional encodings inject order into a model that is otherwise permutation-invariant.

Running inference in f16 halves memory and often speeds up matmul on a capable GPU. Layer normalization stabilizes training by rescaling activations within each layer. The forward pass cost is dominated by matrix multiplies, which favor batching. Positional encodings inject order into a model that is otherwise permutation-invariant. See [[transformers/064-fine-tuning-vs-prompting]]. See [[transformers/024-layer-normalization]].

## CLS Pooling

Self-attention lets every token weigh every other token when building its representation. Running inference in f16 halves memory and often speeds up matmul on a capable GPU. A model truncates inputs beyond its context window, so long documents must be chunked. See [[databases/195-denormalization]].

Fine-tuning adapts a pretrained model to a task; prompting steers it without weight updates. Self-attention lets every token weigh every other token when building its representation. Distillation trains a small student to mimic a large teacher, trading accuracy for speed. The feed-forward block applies the same two-layer network independently at each position. See [[gardening/147-companion-planting]]. See [[pkm/143-evergreen-notes]].

## Layer Normalization

The feed-forward block applies the same two-layer network independently at each position. Positional encodings inject order into a model that is otherwise permutation-invariant. Distillation trains a small student to mimic a large teacher, trading accuracy for speed. Fine-tuning adapts a pretrained model to a task; prompting steers it without weight updates. A subword tokenizer splits rare words into pieces so the vocabulary stays bounded. See [[transformers/094-fine-tuning-vs-prompting]]. See [[transformers/174-positional-encoding]].

BERT is an encoder stack trained with masked-language modeling to produce contextual embeddings. CLS pooling takes the first token's final hidden state as a sentence-level embedding. Distillation trains a small student to mimic a large teacher, trading accuracy for speed. The forward pass cost is dominated by matrix multiplies, which favor batching. Layer normalization stabilizes training by rescaling activations within each layer. See [[transformers/044-distillation]]. See [[transformers/054-tokenization]].

Distillation trains a small student to mimic a large teacher, trading accuracy for speed. Self-attention lets every token weigh every other token when building its representation. Positional encodings inject order into a model that is otherwise permutation-invariant. Fine-tuning adapts a pretrained model to a task; prompting steers it without weight updates. A model truncates inputs beyond its context window, so long documents must be chunked. CLS pooling takes the first token's final hidden state as a sentence-level embedding. See [[transformers/094-fine-tuning-vs-prompting]].

## Layer Normalization

BERT is an encoder stack trained with masked-language modeling to produce contextual embeddings. Layer normalization stabilizes training by rescaling activations within each layer. Self-attention lets every token weigh every other token when building its representation. CLS pooling takes the first token's final hidden state as a sentence-level embedding. Distillation trains a small student to mimic a large teacher, trading accuracy for speed. A subword tokenizer splits rare words into pieces so the vocabulary stays bounded. See [[transformers/124-positional-encoding]]. See [[coffee/008-extraction-yield]].

Layer normalization stabilizes training by rescaling activations within each layer. The feed-forward block applies the same two-layer network independently at each position. Running inference in f16 halves memory and often speeds up matmul on a capable GPU. Distillation trains a small student to mimic a large teacher, trading accuracy for speed. Positional encodings inject order into a model that is otherwise permutation-invariant. The forward pass cost is dominated by matrix multiplies, which favor batching.

A subword tokenizer splits rare words into pieces so the vocabulary stays bounded. Self-attention lets every token weigh every other token when building its representation. A model truncates inputs beyond its context window, so long documents must be chunked. The feed-forward block applies the same two-layer network independently at each position. BERT is an encoder stack trained with masked-language modeling to produce contextual embeddings.
