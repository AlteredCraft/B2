---
b2id: 01KXF21DYYTJYKQVC681F5JCE5
type: note
title: "The Feed-Forward Block"
relations:
  - "example-of [[transformers/124-positional-encoding]] — see also"
---

# The Feed-Forward Block

Notes on the feed-forward block within the broader theme of transformer models.

## Distillation

Fine-tuning adapts a pretrained model to a task; prompting steers it without weight updates. Running inference in f16 halves memory and often speeds up matmul on a capable GPU. Distillation trains a small student to mimic a large teacher, trading accuracy for speed.

Distillation trains a small student to mimic a large teacher, trading accuracy for speed. BERT is an encoder stack trained with masked-language modeling to produce contextual embeddings. Layer normalization stabilizes training by rescaling activations within each layer. A subword tokenizer splits rare words into pieces so the vocabulary stays bounded. The feed-forward block applies the same two-layer network independently at each position. Running inference in f16 halves memory and often speeds up matmul on a capable GPU. See [[transformers/124-positional-encoding]]. See [[transformers/154-self-attention]].

## Self-Attention

BERT is an encoder stack trained with masked-language modeling to produce contextual embeddings. Distillation trains a small student to mimic a large teacher, trading accuracy for speed. Self-attention lets every token weigh every other token when building its representation. The feed-forward block applies the same two-layer network independently at each position.

CLS pooling takes the first token's final hidden state as a sentence-level embedding. Distillation trains a small student to mimic a large teacher, trading accuracy for speed. Fine-tuning adapts a pretrained model to a task; prompting steers it without weight updates. A subword tokenizer splits rare words into pieces so the vocabulary stays bounded. Layer normalization stabilizes training by rescaling activations within each layer. See [[pkm/193-the-map-of-content]]. See [[transformers/074-self-attention]].

## Fine-Tuning vs Prompting

Distillation trains a small student to mimic a large teacher, trading accuracy for speed. A subword tokenizer splits rare words into pieces so the vocabulary stays bounded. CLS pooling takes the first token's final hidden state as a sentence-level embedding. A model truncates inputs beyond its context window, so long documents must be chunked. Positional encodings inject order into a model that is otherwise permutation-invariant. See [[transformers/154-self-attention]].

Positional encodings inject order into a model that is otherwise permutation-invariant. Self-attention lets every token weigh every other token when building its representation. BERT is an encoder stack trained with masked-language modeling to produce contextual embeddings. A subword tokenizer splits rare words into pieces so the vocabulary stays bounded. A model truncates inputs beyond its context window, so long documents must be chunked. See [[transformers/124-positional-encoding]].

Layer normalization stabilizes training by rescaling activations within each layer. BERT is an encoder stack trained with masked-language modeling to produce contextual embeddings. Self-attention lets every token weigh every other token when building its representation. Distillation trains a small student to mimic a large teacher, trading accuracy for speed. See [[productivity/106-managing-interruptions]].

## Fine-Tuning vs Prompting

A subword tokenizer splits rare words into pieces so the vocabulary stays bounded. Positional encodings inject order into a model that is otherwise permutation-invariant. Running inference in f16 halves memory and often speeds up matmul on a capable GPU. CLS pooling takes the first token's final hidden state as a sentence-level embedding. Layer normalization stabilizes training by rescaling activations within each layer.

A subword tokenizer splits rare words into pieces so the vocabulary stays bounded. Positional encodings inject order into a model that is otherwise permutation-invariant. BERT is an encoder stack trained with masked-language modeling to produce contextual embeddings. The forward pass cost is dominated by matrix multiplies, which favor batching. The feed-forward block applies the same two-layer network independently at each position.
