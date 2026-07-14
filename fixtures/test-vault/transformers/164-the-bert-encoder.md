---
b2id: 01KXF21DZ08Z73YY77DFM684DW
type: note
title: "The BERT Encoder"
---

# The BERT Encoder

Notes on the bert encoder within the broader theme of transformer models.

## Tokenization

A model truncates inputs beyond its context window, so long documents must be chunked. Positional encodings inject order into a model that is otherwise permutation-invariant. BERT is an encoder stack trained with masked-language modeling to produce contextual embeddings. The feed-forward block applies the same two-layer network independently at each position. See [[transformers/054-tokenization]].

Running inference in f16 halves memory and often speeds up matmul on a capable GPU. Layer normalization stabilizes training by rescaling activations within each layer. Self-attention lets every token weigh every other token when building its representation. BERT is an encoder stack trained with masked-language modeling to produce contextual embeddings. Fine-tuning adapts a pretrained model to a task; prompting steers it without weight updates. See [[distributed-systems/031-the-cap-tradeoff]].

Positional encodings inject order into a model that is otherwise permutation-invariant. The forward pass cost is dominated by matrix multiplies, which favor batching. Fine-tuning adapts a pretrained model to a task; prompting steers it without weight updates. A subword tokenizer splits rare words into pieces so the vocabulary stays bounded. Running inference in f16 halves memory and often speeds up matmul on a capable GPU. See [[transformers/054-tokenization]].

## The Feed-Forward Block

BERT is an encoder stack trained with masked-language modeling to produce contextual embeddings. Self-attention lets every token weigh every other token when building its representation. The feed-forward block applies the same two-layer network independently at each position. See [[transformers/094-fine-tuning-vs-prompting]]. See [[transformers/054-tokenization]].

The feed-forward block applies the same two-layer network independently at each position. CLS pooling takes the first token's final hidden state as a sentence-level embedding. Layer normalization stabilizes training by rescaling activations within each layer. Distillation trains a small student to mimic a large teacher, trading accuracy for speed. Running inference in f16 halves memory and often speeds up matmul on a capable GPU. See [[transformers/144-the-feed-forward-block]].

## Fine-Tuning vs Prompting

A model truncates inputs beyond its context window, so long documents must be chunked. Positional encodings inject order into a model that is otherwise permutation-invariant. A subword tokenizer splits rare words into pieces so the vocabulary stays bounded. The forward pass cost is dominated by matrix multiplies, which favor batching. Distillation trains a small student to mimic a large teacher, trading accuracy for speed. Self-attention lets every token weigh every other token when building its representation.

Fine-tuning adapts a pretrained model to a task; prompting steers it without weight updates. A model truncates inputs beyond its context window, so long documents must be chunked. BERT is an encoder stack trained with masked-language modeling to produce contextual embeddings. Self-attention lets every token weigh every other token when building its representation. A subword tokenizer splits rare words into pieces so the vocabulary stays bounded. Layer normalization stabilizes training by rescaling activations within each layer. See [[vector-search/140-reranking-candidates]]. See [[transformers/174-positional-encoding]].
