---
b2id: 01KXF21DYQNXKH4Z9AF5B1WFPA
type: note
title: "Self-Attention"
---

# Self-Attention

Notes on self-attention within the broader theme of transformer models.

## CLS Pooling

A model truncates inputs beyond its context window, so long documents must be chunked. Running inference in f16 halves memory and often speeds up matmul on a capable GPU. Distillation trains a small student to mimic a large teacher, trading accuracy for speed. Positional encodings inject order into a model that is otherwise permutation-invariant. Self-attention lets every token weigh every other token when building its representation. BERT is an encoder stack trained with masked-language modeling to produce contextual embeddings. See [[transformers/034-the-feed-forward-block]].

Positional encodings inject order into a model that is otherwise permutation-invariant. A subword tokenizer splits rare words into pieces so the vocabulary stays bounded. A model truncates inputs beyond its context window, so long documents must be chunked. The feed-forward block applies the same two-layer network independently at each position. See [[transformers/004-cls-pooling]]. See [[transformers/054-tokenization]].

A model truncates inputs beyond its context window, so long documents must be chunked. Positional encodings inject order into a model that is otherwise permutation-invariant. Running inference in f16 halves memory and often speeds up matmul on a capable GPU. Distillation trains a small student to mimic a large teacher, trading accuracy for speed. The forward pass cost is dominated by matrix multiplies, which favor batching. The feed-forward block applies the same two-layer network independently at each position. See [[distributed-systems/021-backpressure]]. See [[transformers/124-positional-encoding]].

## The Feed-Forward Block

Distillation trains a small student to mimic a large teacher, trading accuracy for speed. A model truncates inputs beyond its context window, so long documents must be chunked. The feed-forward block applies the same two-layer network independently at each position. CLS pooling takes the first token's final hidden state as a sentence-level embedding. BERT is an encoder stack trained with masked-language modeling to produce contextual embeddings. See [[transformers/054-tokenization]].

Positional encodings inject order into a model that is otherwise permutation-invariant. A subword tokenizer splits rare words into pieces so the vocabulary stays bounded. Running inference in f16 halves memory and often speeds up matmul on a capable GPU.

The feed-forward block applies the same two-layer network independently at each position. Distillation trains a small student to mimic a large teacher, trading accuracy for speed. Layer normalization stabilizes training by rescaling activations within each layer. The forward pass cost is dominated by matrix multiplies, which favor batching. Fine-tuning adapts a pretrained model to a task; prompting steers it without weight updates. See [[transformers/024-layer-normalization]].

## Tokenization

The forward pass cost is dominated by matrix multiplies, which favor batching. Running inference in f16 halves memory and often speeds up matmul on a capable GPU. Distillation trains a small student to mimic a large teacher, trading accuracy for speed. See [[transformers/034-the-feed-forward-block]]. See [[transformers/094-fine-tuning-vs-prompting]].

Running inference in f16 halves memory and often speeds up matmul on a capable GPU. A subword tokenizer splits rare words into pieces so the vocabulary stays bounded. A model truncates inputs beyond its context window, so long documents must be chunked. Positional encodings inject order into a model that is otherwise permutation-invariant. Layer normalization stabilizes training by rescaling activations within each layer.

## The BERT Encoder

Running inference in f16 halves memory and often speeds up matmul on a capable GPU. Fine-tuning adapts a pretrained model to a task; prompting steers it without weight updates. Layer normalization stabilizes training by rescaling activations within each layer. CLS pooling takes the first token's final hidden state as a sentence-level embedding.

CLS pooling takes the first token's final hidden state as a sentence-level embedding. Self-attention lets every token weigh every other token when building its representation. Fine-tuning adapts a pretrained model to a task; prompting steers it without weight updates. A model truncates inputs beyond its context window, so long documents must be chunked. Layer normalization stabilizes training by rescaling activations within each layer. See [[coffee/128-extraction-yield]]. See [[transformers/184-context-windows]].

CLS pooling takes the first token's final hidden state as a sentence-level embedding. BERT is an encoder stack trained with masked-language modeling to produce contextual embeddings. Fine-tuning adapts a pretrained model to a task; prompting steers it without weight updates. Positional encodings inject order into a model that is otherwise permutation-invariant. See [[transformers/014-context-windows]].

## The Feed-Forward Block

Running inference in f16 halves memory and often speeds up matmul on a capable GPU. Positional encodings inject order into a model that is otherwise permutation-invariant. Layer normalization stabilizes training by rescaling activations within each layer. Distillation trains a small student to mimic a large teacher, trading accuracy for speed. The forward pass cost is dominated by matrix multiplies, which favor batching. BERT is an encoder stack trained with masked-language modeling to produce contextual embeddings.

The feed-forward block applies the same two-layer network independently at each position. Running inference in f16 halves memory and often speeds up matmul on a capable GPU. A model truncates inputs beyond its context window, so long documents must be chunked. A subword tokenizer splits rare words into pieces so the vocabulary stays bounded. The forward pass cost is dominated by matrix multiplies, which favor batching. Self-attention lets every token weigh every other token when building its representation. See [[databases/155-connection-pooling]].
