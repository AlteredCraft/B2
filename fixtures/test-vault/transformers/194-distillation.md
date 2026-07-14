---
b2id: 01KXF21DZ2SVHGY3R90DW67TJ4
type: note
title: "Distillation"
---

# Distillation

Notes on distillation within the broader theme of transformer models.

## Layer Normalization

Layer normalization stabilizes training by rescaling activations within each layer. BERT is an encoder stack trained with masked-language modeling to produce contextual embeddings. A model truncates inputs beyond its context window, so long documents must be chunked. See [[transformers/034-the-feed-forward-block]].

A subword tokenizer splits rare words into pieces so the vocabulary stays bounded. A model truncates inputs beyond its context window, so long documents must be chunked. Positional encodings inject order into a model that is otherwise permutation-invariant. BERT is an encoder stack trained with masked-language modeling to produce contextual embeddings. The feed-forward block applies the same two-layer network independently at each position. Running inference in f16 halves memory and often speeds up matmul on a capable GPU. See [[transformers/124-positional-encoding]].

Positional encodings inject order into a model that is otherwise permutation-invariant. A model truncates inputs beyond its context window, so long documents must be chunked. Self-attention lets every token weigh every other token when building its representation. A subword tokenizer splits rare words into pieces so the vocabulary stays bounded. See [[transformers/164-the-bert-encoder]].

## Self-Attention

Self-attention lets every token weigh every other token when building its representation. Layer normalization stabilizes training by rescaling activations within each layer. A subword tokenizer splits rare words into pieces so the vocabulary stays bounded. Distillation trains a small student to mimic a large teacher, trading accuracy for speed. Positional encodings inject order into a model that is otherwise permutation-invariant.

The feed-forward block applies the same two-layer network independently at each position. A subword tokenizer splits rare words into pieces so the vocabulary stays bounded. Self-attention lets every token weigh every other token when building its representation. Positional encodings inject order into a model that is otherwise permutation-invariant. BERT is an encoder stack trained with masked-language modeling to produce contextual embeddings.

CLS pooling takes the first token's final hidden state as a sentence-level embedding. Running inference in f16 halves memory and often speeds up matmul on a capable GPU. A subword tokenizer splits rare words into pieces so the vocabulary stays bounded. See [[transformers/014-context-windows]].

## The BERT Encoder

The feed-forward block applies the same two-layer network independently at each position. Fine-tuning adapts a pretrained model to a task; prompting steers it without weight updates. A subword tokenizer splits rare words into pieces so the vocabulary stays bounded. BERT is an encoder stack trained with masked-language modeling to produce contextual embeddings. See [[transformers/144-the-feed-forward-block]]. See [[hiking/029-trail-etiquette]].

CLS pooling takes the first token's final hidden state as a sentence-level embedding. Distillation trains a small student to mimic a large teacher, trading accuracy for speed. Running inference in f16 halves memory and often speeds up matmul on a capable GPU. See [[transformers/064-fine-tuning-vs-prompting]]. See [[transformers/034-the-feed-forward-block]].
