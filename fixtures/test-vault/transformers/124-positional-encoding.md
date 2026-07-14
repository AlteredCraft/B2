---
b2id: 01KXF21DYXYY0TGRAD7BM8VAAE
type: note
title: "Positional Encoding"
---

# Positional Encoding

Notes on positional encoding within the broader theme of transformer models.

## Self-Attention

Fine-tuning adapts a pretrained model to a task; prompting steers it without weight updates. A model truncates inputs beyond its context window, so long documents must be chunked. Self-attention lets every token weigh every other token when building its representation. The forward pass cost is dominated by matrix multiplies, which favor batching. A subword tokenizer splits rare words into pieces so the vocabulary stays bounded. Layer normalization stabilizes training by rescaling activations within each layer. See [[transformers/044-distillation]].

BERT is an encoder stack trained with masked-language modeling to produce contextual embeddings. CLS pooling takes the first token's final hidden state as a sentence-level embedding. The feed-forward block applies the same two-layer network independently at each position. Distillation trains a small student to mimic a large teacher, trading accuracy for speed. A subword tokenizer splits rare words into pieces so the vocabulary stays bounded. See [[transformers/004-cls-pooling]]. See [[transformers/134-tokenization]].

Distillation trains a small student to mimic a large teacher, trading accuracy for speed. Layer normalization stabilizes training by rescaling activations within each layer. Self-attention lets every token weigh every other token when building its representation. See [[transformers/164-the-bert-encoder]]. See [[transformers/014-context-windows]].

## Self-Attention

Fine-tuning adapts a pretrained model to a task; prompting steers it without weight updates. Running inference in f16 halves memory and often speeds up matmul on a capable GPU. The forward pass cost is dominated by matrix multiplies, which favor batching. See [[transformers/164-the-bert-encoder]]. See [[pkm/003-local-first-vaults]].

The feed-forward block applies the same two-layer network independently at each position. Fine-tuning adapts a pretrained model to a task; prompting steers it without weight updates. The forward pass cost is dominated by matrix multiplies, which favor batching. A subword tokenizer splits rare words into pieces so the vocabulary stays bounded. BERT is an encoder stack trained with masked-language modeling to produce contextual embeddings. See [[transformers/014-context-windows]].

Distillation trains a small student to mimic a large teacher, trading accuracy for speed. Fine-tuning adapts a pretrained model to a task; prompting steers it without weight updates. Positional encodings inject order into a model that is otherwise permutation-invariant. BERT is an encoder stack trained with masked-language modeling to produce contextual embeddings. CLS pooling takes the first token's final hidden state as a sentence-level embedding. Layer normalization stabilizes training by rescaling activations within each layer.

## The Feed-Forward Block

Layer normalization stabilizes training by rescaling activations within each layer. Self-attention lets every token weigh every other token when building its representation. Running inference in f16 halves memory and often speeds up matmul on a capable GPU. BERT is an encoder stack trained with masked-language modeling to produce contextual embeddings. CLS pooling takes the first token's final hidden state as a sentence-level embedding. See [[transformers/014-context-windows]]. See [[hiking/089-switchbacks]].

Layer normalization stabilizes training by rescaling activations within each layer. CLS pooling takes the first token's final hidden state as a sentence-level embedding. A subword tokenizer splits rare words into pieces so the vocabulary stays bounded. The feed-forward block applies the same two-layer network independently at each position. See [[transformers/014-context-windows]].

## The BERT Encoder

A model truncates inputs beyond its context window, so long documents must be chunked. A subword tokenizer splits rare words into pieces so the vocabulary stays bounded. Fine-tuning adapts a pretrained model to a task; prompting steers it without weight updates. See [[transformers/094-fine-tuning-vs-prompting]].

Self-attention lets every token weigh every other token when building its representation. CLS pooling takes the first token's final hidden state as a sentence-level embedding. A subword tokenizer splits rare words into pieces so the vocabulary stays bounded. See [[transformers/154-self-attention]].

A model truncates inputs beyond its context window, so long documents must be chunked. The feed-forward block applies the same two-layer network independently at each position. A subword tokenizer splits rare words into pieces so the vocabulary stays bounded. Positional encodings inject order into a model that is otherwise permutation-invariant. Layer normalization stabilizes training by rescaling activations within each layer. See [[transformers/074-self-attention]]. See [[transformers/134-tokenization]].
