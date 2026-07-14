---
b2id: 01KXF21DYMM1JXBTZJV8PFW3KZ
type: note
title: "Layer Normalization"
---

# Layer Normalization

Notes on layer normalization within the broader theme of transformer models.

## Self-Attention

Positional encodings inject order into a model that is otherwise permutation-invariant. CLS pooling takes the first token's final hidden state as a sentence-level embedding. A model truncates inputs beyond its context window, so long documents must be chunked. See [[vector-search/040-dense-vs-sparse]]. See [[transformers/154-self-attention]].

Self-attention lets every token weigh every other token when building its representation. A subword tokenizer splits rare words into pieces so the vocabulary stays bounded. A model truncates inputs beyond its context window, so long documents must be chunked. The feed-forward block applies the same two-layer network independently at each position. See [[transformers/194-distillation]].

Fine-tuning adapts a pretrained model to a task; prompting steers it without weight updates. The feed-forward block applies the same two-layer network independently at each position. Self-attention lets every token weigh every other token when building its representation. The forward pass cost is dominated by matrix multiplies, which favor batching. CLS pooling takes the first token's final hidden state as a sentence-level embedding. A subword tokenizer splits rare words into pieces so the vocabulary stays bounded. See [[transformers/154-self-attention]]. See [[gardening/017-attracting-pollinators]].

## The Feed-Forward Block

Layer normalization stabilizes training by rescaling activations within each layer. The feed-forward block applies the same two-layer network independently at each position. BERT is an encoder stack trained with masked-language modeling to produce contextual embeddings. Positional encodings inject order into a model that is otherwise permutation-invariant. Distillation trains a small student to mimic a large teacher, trading accuracy for speed. The forward pass cost is dominated by matrix multiplies, which favor batching. See [[transformers/194-distillation]].

A subword tokenizer splits rare words into pieces so the vocabulary stays bounded. Positional encodings inject order into a model that is otherwise permutation-invariant. CLS pooling takes the first token's final hidden state as a sentence-level embedding. A model truncates inputs beyond its context window, so long documents must be chunked. The feed-forward block applies the same two-layer network independently at each position. See [[gardening/107-overwintering]]. See [[transformers/074-self-attention]].

## Fine-Tuning vs Prompting

CLS pooling takes the first token's final hidden state as a sentence-level embedding. The forward pass cost is dominated by matrix multiplies, which favor batching. The feed-forward block applies the same two-layer network independently at each position. Self-attention lets every token weigh every other token when building its representation. A subword tokenizer splits rare words into pieces so the vocabulary stays bounded. Running inference in f16 halves memory and often speeds up matmul on a capable GPU. See [[rust/022-lifetimes-explained]]. See [[transformers/054-tokenization]].

Running inference in f16 halves memory and often speeds up matmul on a capable GPU. A model truncates inputs beyond its context window, so long documents must be chunked. CLS pooling takes the first token's final hidden state as a sentence-level embedding. See [[transformers/104-self-attention]].

BERT is an encoder stack trained with masked-language modeling to produce contextual embeddings. Self-attention lets every token weigh every other token when building its representation. Fine-tuning adapts a pretrained model to a task; prompting steers it without weight updates. The forward pass cost is dominated by matrix multiplies, which favor batching. The feed-forward block applies the same two-layer network independently at each position. See [[transformers/164-the-bert-encoder]]. See [[transformers/164-the-bert-encoder]].

## Tokenization

BERT is an encoder stack trained with masked-language modeling to produce contextual embeddings. A model truncates inputs beyond its context window, so long documents must be chunked. Layer normalization stabilizes training by rescaling activations within each layer.

Self-attention lets every token weigh every other token when building its representation. The feed-forward block applies the same two-layer network independently at each position. A model truncates inputs beyond its context window, so long documents must be chunked. A subword tokenizer splits rare words into pieces so the vocabulary stays bounded. See [[transformers/124-positional-encoding]]. See [[productivity/006-deep-work]].
