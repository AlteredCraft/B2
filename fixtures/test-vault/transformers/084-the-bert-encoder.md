---
b2id: 01KXF21DYRVCYH31Z80N51JE2N
type: note
title: "The BERT Encoder"
---

# The BERT Encoder

Notes on the bert encoder within the broader theme of transformer models.

## Distillation

CLS pooling takes the first token's final hidden state as a sentence-level embedding. The forward pass cost is dominated by matrix multiplies, which favor batching. Fine-tuning adapts a pretrained model to a task; prompting steers it without weight updates. See [[transformers/064-fine-tuning-vs-prompting]]. See [[transformers/064-fine-tuning-vs-prompting]].

Running inference in f16 halves memory and often speeds up matmul on a capable GPU. Distillation trains a small student to mimic a large teacher, trading accuracy for speed. Positional encodings inject order into a model that is otherwise permutation-invariant. The forward pass cost is dominated by matrix multiplies, which favor batching. BERT is an encoder stack trained with masked-language modeling to produce contextual embeddings. See [[rust/032-the-newtype-pattern]]. See [[transformers/174-positional-encoding]].

The forward pass cost is dominated by matrix multiplies, which favor batching. Layer normalization stabilizes training by rescaling activations within each layer. Distillation trains a small student to mimic a large teacher, trading accuracy for speed. A subword tokenizer splits rare words into pieces so the vocabulary stays bounded. CLS pooling takes the first token's final hidden state as a sentence-level embedding. A model truncates inputs beyond its context window, so long documents must be chunked.

## The Feed-Forward Block

Positional encodings inject order into a model that is otherwise permutation-invariant. Running inference in f16 halves memory and often speeds up matmul on a capable GPU. A model truncates inputs beyond its context window, so long documents must be chunked. Layer normalization stabilizes training by rescaling activations within each layer. The forward pass cost is dominated by matrix multiplies, which favor batching. Fine-tuning adapts a pretrained model to a task; prompting steers it without weight updates.

Distillation trains a small student to mimic a large teacher, trading accuracy for speed. The feed-forward block applies the same two-layer network independently at each position. BERT is an encoder stack trained with masked-language modeling to produce contextual embeddings. See [[transformers/074-self-attention]].

Distillation trains a small student to mimic a large teacher, trading accuracy for speed. Self-attention lets every token weigh every other token when building its representation. Fine-tuning adapts a pretrained model to a task; prompting steers it without weight updates. CLS pooling takes the first token's final hidden state as a sentence-level embedding. See [[transformers/184-context-windows]]. See [[transformers/044-distillation]].

## Layer Normalization

Self-attention lets every token weigh every other token when building its representation. A model truncates inputs beyond its context window, so long documents must be chunked. The feed-forward block applies the same two-layer network independently at each position. CLS pooling takes the first token's final hidden state as a sentence-level embedding. See [[transformers/164-the-bert-encoder]].

A model truncates inputs beyond its context window, so long documents must be chunked. The forward pass cost is dominated by matrix multiplies, which favor batching. The feed-forward block applies the same two-layer network independently at each position. Layer normalization stabilizes training by rescaling activations within each layer. Positional encodings inject order into a model that is otherwise permutation-invariant.

Running inference in f16 halves memory and often speeds up matmul on a capable GPU. A subword tokenizer splits rare words into pieces so the vocabulary stays bounded. Fine-tuning adapts a pretrained model to a task; prompting steers it without weight updates. CLS pooling takes the first token's final hidden state as a sentence-level embedding. See [[gardening/057-drip-irrigation]].

## Layer Normalization

Running inference in f16 halves memory and often speeds up matmul on a capable GPU. A model truncates inputs beyond its context window, so long documents must be chunked. Layer normalization stabilizes training by rescaling activations within each layer. The feed-forward block applies the same two-layer network independently at each position. See [[transformers/164-the-bert-encoder]]. See [[transformers/184-context-windows]].

Distillation trains a small student to mimic a large teacher, trading accuracy for speed. Layer normalization stabilizes training by rescaling activations within each layer. Positional encodings inject order into a model that is otherwise permutation-invariant. BERT is an encoder stack trained with masked-language modeling to produce contextual embeddings. Self-attention lets every token weigh every other token when building its representation. See [[transformers/094-fine-tuning-vs-prompting]].
