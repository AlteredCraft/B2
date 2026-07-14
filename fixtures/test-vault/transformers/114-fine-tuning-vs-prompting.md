---
b2id: 01KXF21DYV9AQ9WE790HZXT4R3
type: note
title: "Fine-Tuning vs Prompting"
---

# Fine-Tuning vs Prompting

Notes on fine-tuning vs prompting within the broader theme of transformer models.

## Self-Attention

Running inference in f16 halves memory and often speeds up matmul on a capable GPU. Fine-tuning adapts a pretrained model to a task; prompting steers it without weight updates. Self-attention lets every token weigh every other token when building its representation. Distillation trains a small student to mimic a large teacher, trading accuracy for speed.

Layer normalization stabilizes training by rescaling activations within each layer. Running inference in f16 halves memory and often speeds up matmul on a capable GPU. CLS pooling takes the first token's final hidden state as a sentence-level embedding. Positional encodings inject order into a model that is otherwise permutation-invariant. Fine-tuning adapts a pretrained model to a task; prompting steers it without weight updates. See [[transformers/144-the-feed-forward-block]]. See [[databases/115-vacuuming]].

CLS pooling takes the first token's final hidden state as a sentence-level embedding. Positional encodings inject order into a model that is otherwise permutation-invariant. The feed-forward block applies the same two-layer network independently at each position. Layer normalization stabilizes training by rescaling activations within each layer. BERT is an encoder stack trained with masked-language modeling to produce contextual embeddings.

## The BERT Encoder

BERT is an encoder stack trained with masked-language modeling to produce contextual embeddings. The feed-forward block applies the same two-layer network independently at each position. CLS pooling takes the first token's final hidden state as a sentence-level embedding.

Distillation trains a small student to mimic a large teacher, trading accuracy for speed. Self-attention lets every token weigh every other token when building its representation. The feed-forward block applies the same two-layer network independently at each position. CLS pooling takes the first token's final hidden state as a sentence-level embedding. Fine-tuning adapts a pretrained model to a task; prompting steers it without weight updates.

## Fine-Tuning vs Prompting

A subword tokenizer splits rare words into pieces so the vocabulary stays bounded. The feed-forward block applies the same two-layer network independently at each position. Fine-tuning adapts a pretrained model to a task; prompting steers it without weight updates. See [[transformers/184-context-windows]]. See [[transformers/164-the-bert-encoder]].

A model truncates inputs beyond its context window, so long documents must be chunked. Fine-tuning adapts a pretrained model to a task; prompting steers it without weight updates. Layer normalization stabilizes training by rescaling activations within each layer. Positional encodings inject order into a model that is otherwise permutation-invariant. The feed-forward block applies the same two-layer network independently at each position. Self-attention lets every token weigh every other token when building its representation.

A model truncates inputs beyond its context window, so long documents must be chunked. Layer normalization stabilizes training by rescaling activations within each layer. Distillation trains a small student to mimic a large teacher, trading accuracy for speed. Self-attention lets every token weigh every other token when building its representation.
