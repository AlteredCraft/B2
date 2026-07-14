---
b2id: 01KXF21DYQ4DXYB6K7Q1B9CX8J
type: note
title: "Fine-Tuning vs Prompting"
---

# Fine-Tuning vs Prompting

Notes on fine-tuning vs prompting within the broader theme of transformer models.

## The BERT Encoder

A model truncates inputs beyond its context window, so long documents must be chunked. CLS pooling takes the first token's final hidden state as a sentence-level embedding. Fine-tuning adapts a pretrained model to a task; prompting steers it without weight updates.

A model truncates inputs beyond its context window, so long documents must be chunked. The forward pass cost is dominated by matrix multiplies, which favor batching. CLS pooling takes the first token's final hidden state as a sentence-level embedding. Positional encodings inject order into a model that is otherwise permutation-invariant. BERT is an encoder stack trained with masked-language modeling to produce contextual embeddings. See [[productivity/106-managing-interruptions]]. See [[transformers/184-context-windows]].

## The Feed-Forward Block

CLS pooling takes the first token's final hidden state as a sentence-level embedding. Positional encodings inject order into a model that is otherwise permutation-invariant. Self-attention lets every token weigh every other token when building its representation. Distillation trains a small student to mimic a large teacher, trading accuracy for speed. The feed-forward block applies the same two-layer network independently at each position.

Self-attention lets every token weigh every other token when building its representation. BERT is an encoder stack trained with masked-language modeling to produce contextual embeddings. The forward pass cost is dominated by matrix multiplies, which favor batching. See [[transformers/074-self-attention]]. See [[transformers/084-the-bert-encoder]].

Fine-tuning adapts a pretrained model to a task; prompting steers it without weight updates. A model truncates inputs beyond its context window, so long documents must be chunked. Positional encodings inject order into a model that is otherwise permutation-invariant. Layer normalization stabilizes training by rescaling activations within each layer. The feed-forward block applies the same two-layer network independently at each position. Distillation trains a small student to mimic a large teacher, trading accuracy for speed.

## Tokenization

Fine-tuning adapts a pretrained model to a task; prompting steers it without weight updates. Distillation trains a small student to mimic a large teacher, trading accuracy for speed. Self-attention lets every token weigh every other token when building its representation. Positional encodings inject order into a model that is otherwise permutation-invariant. The forward pass cost is dominated by matrix multiplies, which favor batching. Layer normalization stabilizes training by rescaling activations within each layer. See [[distributed-systems/171-retries-and-jitter]].

A subword tokenizer splits rare words into pieces so the vocabulary stays bounded. The forward pass cost is dominated by matrix multiplies, which favor batching. Fine-tuning adapts a pretrained model to a task; prompting steers it without weight updates. BERT is an encoder stack trained with masked-language modeling to produce contextual embeddings. Layer normalization stabilizes training by rescaling activations within each layer.
