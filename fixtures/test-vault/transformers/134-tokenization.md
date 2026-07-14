---
b2id: 01KXF21DYXY79466PFTQQ489SY
type: note
title: "Tokenization"
---

# Tokenization

Notes on tokenization within the broader theme of transformer models.

## Positional Encoding

Running inference in f16 halves memory and often speeds up matmul on a capable GPU. The feed-forward block applies the same two-layer network independently at each position. Self-attention lets every token weigh every other token when building its representation. The forward pass cost is dominated by matrix multiplies, which favor batching. Distillation trains a small student to mimic a large teacher, trading accuracy for speed. See [[transformers/194-distillation]]. See [[transformers/104-self-attention]].

Fine-tuning adapts a pretrained model to a task; prompting steers it without weight updates. Self-attention lets every token weigh every other token when building its representation. Layer normalization stabilizes training by rescaling activations within each layer. A model truncates inputs beyond its context window, so long documents must be chunked. CLS pooling takes the first token's final hidden state as a sentence-level embedding.

CLS pooling takes the first token's final hidden state as a sentence-level embedding. Positional encodings inject order into a model that is otherwise permutation-invariant. Running inference in f16 halves memory and often speeds up matmul on a capable GPU.

## The Feed-Forward Block

A model truncates inputs beyond its context window, so long documents must be chunked. CLS pooling takes the first token's final hidden state as a sentence-level embedding. Positional encodings inject order into a model that is otherwise permutation-invariant. A subword tokenizer splits rare words into pieces so the vocabulary stays bounded. The feed-forward block applies the same two-layer network independently at each position. See [[databases/015-connection-pooling]]. See [[pkm/133-the-zettelkasten]].

Running inference in f16 halves memory and often speeds up matmul on a capable GPU. Self-attention lets every token weigh every other token when building its representation. The forward pass cost is dominated by matrix multiplies, which favor batching. A model truncates inputs beyond its context window, so long documents must be chunked. A subword tokenizer splits rare words into pieces so the vocabulary stays bounded. The feed-forward block applies the same two-layer network independently at each position. See [[transformers/024-layer-normalization]]. See [[hiking/079-trail-etiquette]].

## Context Windows

Positional encodings inject order into a model that is otherwise permutation-invariant. Fine-tuning adapts a pretrained model to a task; prompting steers it without weight updates. BERT is an encoder stack trained with masked-language modeling to produce contextual embeddings. The feed-forward block applies the same two-layer network independently at each position. CLS pooling takes the first token's final hidden state as a sentence-level embedding. See [[transformers/144-the-feed-forward-block]].

Running inference in f16 halves memory and often speeds up matmul on a capable GPU. BERT is an encoder stack trained with masked-language modeling to produce contextual embeddings. CLS pooling takes the first token's final hidden state as a sentence-level embedding. A model truncates inputs beyond its context window, so long documents must be chunked. A subword tokenizer splits rare words into pieces so the vocabulary stays bounded.

A subword tokenizer splits rare words into pieces so the vocabulary stays bounded. The feed-forward block applies the same two-layer network independently at each position. The forward pass cost is dominated by matrix multiplies, which favor batching. Running inference in f16 halves memory and often speeds up matmul on a capable GPU. Fine-tuning adapts a pretrained model to a task; prompting steers it without weight updates. BERT is an encoder stack trained with masked-language modeling to produce contextual embeddings. See [[transformers/104-self-attention]]. See [[transformers/154-self-attention]].
