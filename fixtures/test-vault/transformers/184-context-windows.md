---
b2id: 01KXF21DZ1CWVTF6VK2RE0EFEZ
type: note
title: "Context Windows"
---

# Context Windows

Notes on context windows within the broader theme of transformer models.

## The Feed-Forward Block

Fine-tuning adapts a pretrained model to a task; prompting steers it without weight updates. Layer normalization stabilizes training by rescaling activations within each layer. Positional encodings inject order into a model that is otherwise permutation-invariant. A model truncates inputs beyond its context window, so long documents must be chunked. Self-attention lets every token weigh every other token when building its representation. See [[productivity/136-timeboxing]].

Running inference in f16 halves memory and often speeds up matmul on a capable GPU. A model truncates inputs beyond its context window, so long documents must be chunked. BERT is an encoder stack trained with masked-language modeling to produce contextual embeddings. The forward pass cost is dominated by matrix multiplies, which favor batching. Self-attention lets every token weigh every other token when building its representation. Positional encodings inject order into a model that is otherwise permutation-invariant. See [[coffee/028-the-pour-over]].

## Fine-Tuning vs Prompting

The feed-forward block applies the same two-layer network independently at each position. BERT is an encoder stack trained with masked-language modeling to produce contextual embeddings. Self-attention lets every token weigh every other token when building its representation. Running inference in f16 halves memory and often speeds up matmul on a capable GPU. Distillation trains a small student to mimic a large teacher, trading accuracy for speed. The forward pass cost is dominated by matrix multiplies, which favor batching. See [[pkm/003-local-first-vaults]]. See [[transformers/104-self-attention]].

Self-attention lets every token weigh every other token when building its representation. A subword tokenizer splits rare words into pieces so the vocabulary stays bounded. The forward pass cost is dominated by matrix multiplies, which favor batching. Layer normalization stabilizes training by rescaling activations within each layer. Distillation trains a small student to mimic a large teacher, trading accuracy for speed. Running inference in f16 halves memory and often speeds up matmul on a capable GPU.

## Self-Attention

The feed-forward block applies the same two-layer network independently at each position. A subword tokenizer splits rare words into pieces so the vocabulary stays bounded. Layer normalization stabilizes training by rescaling activations within each layer. Positional encodings inject order into a model that is otherwise permutation-invariant. Fine-tuning adapts a pretrained model to a task; prompting steers it without weight updates. See [[transformers/084-the-bert-encoder]].

Distillation trains a small student to mimic a large teacher, trading accuracy for speed. Running inference in f16 halves memory and often speeds up matmul on a capable GPU. A subword tokenizer splits rare words into pieces so the vocabulary stays bounded.

The forward pass cost is dominated by matrix multiplies, which favor batching. Distillation trains a small student to mimic a large teacher, trading accuracy for speed. Running inference in f16 halves memory and often speeds up matmul on a capable GPU. Self-attention lets every token weigh every other token when building its representation. Positional encodings inject order into a model that is otherwise permutation-invariant. A subword tokenizer splits rare words into pieces so the vocabulary stays bounded. See [[hiking/089-switchbacks]].

## CLS Pooling

Fine-tuning adapts a pretrained model to a task; prompting steers it without weight updates. Distillation trains a small student to mimic a large teacher, trading accuracy for speed. A model truncates inputs beyond its context window, so long documents must be chunked. See [[transformers/164-the-bert-encoder]].

The forward pass cost is dominated by matrix multiplies, which favor batching. Running inference in f16 halves memory and often speeds up matmul on a capable GPU. A subword tokenizer splits rare words into pieces so the vocabulary stays bounded. The feed-forward block applies the same two-layer network independently at each position.

## CLS Pooling

Fine-tuning adapts a pretrained model to a task; prompting steers it without weight updates. Layer normalization stabilizes training by rescaling activations within each layer. Distillation trains a small student to mimic a large teacher, trading accuracy for speed. See [[pkm/023-surfacing-connections]].

Running inference in f16 halves memory and often speeds up matmul on a capable GPU. CLS pooling takes the first token's final hidden state as a sentence-level embedding. Fine-tuning adapts a pretrained model to a task; prompting steers it without weight updates. BERT is an encoder stack trained with masked-language modeling to produce contextual embeddings. The feed-forward block applies the same two-layer network independently at each position.

BERT is an encoder stack trained with masked-language modeling to produce contextual embeddings. A model truncates inputs beyond its context window, so long documents must be chunked. A subword tokenizer splits rare words into pieces so the vocabulary stays bounded. The forward pass cost is dominated by matrix multiplies, which favor batching. CLS pooling takes the first token's final hidden state as a sentence-level embedding. See [[transformers/144-the-feed-forward-block]].
