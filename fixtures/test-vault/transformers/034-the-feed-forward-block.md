---
b2id: 01KXF21DYNNGVND3VXPZF16C9E
type: note
title: "The Feed-Forward Block"
---

# The Feed-Forward Block

Notes on the feed-forward block within the broader theme of transformer models.

## Layer Normalization

Positional encodings inject order into a model that is otherwise permutation-invariant. The feed-forward block applies the same two-layer network independently at each position. Layer normalization stabilizes training by rescaling activations within each layer. Running inference in f16 halves memory and often speeds up matmul on a capable GPU. The forward pass cost is dominated by matrix multiplies, which favor batching. See [[transformers/144-the-feed-forward-block]].

The forward pass cost is dominated by matrix multiplies, which favor batching. The feed-forward block applies the same two-layer network independently at each position. CLS pooling takes the first token's final hidden state as a sentence-level embedding. A model truncates inputs beyond its context window, so long documents must be chunked. See [[transformers/104-self-attention]].

## The BERT Encoder

Distillation trains a small student to mimic a large teacher, trading accuracy for speed. Self-attention lets every token weigh every other token when building its representation. Fine-tuning adapts a pretrained model to a task; prompting steers it without weight updates. BERT is an encoder stack trained with masked-language modeling to produce contextual embeddings. See [[transformers/184-context-windows]].

Distillation trains a small student to mimic a large teacher, trading accuracy for speed. The forward pass cost is dominated by matrix multiplies, which favor batching. A subword tokenizer splits rare words into pieces so the vocabulary stays bounded. Positional encodings inject order into a model that is otherwise permutation-invariant. Self-attention lets every token weigh every other token when building its representation. Fine-tuning adapts a pretrained model to a task; prompting steers it without weight updates. See [[transformers/164-the-bert-encoder]]. See [[transformers/164-the-bert-encoder]].

## Self-Attention

Layer normalization stabilizes training by rescaling activations within each layer. Distillation trains a small student to mimic a large teacher, trading accuracy for speed. The forward pass cost is dominated by matrix multiplies, which favor batching.

BERT is an encoder stack trained with masked-language modeling to produce contextual embeddings. Positional encodings inject order into a model that is otherwise permutation-invariant. Fine-tuning adapts a pretrained model to a task; prompting steers it without weight updates. The feed-forward block applies the same two-layer network independently at each position. A subword tokenizer splits rare words into pieces so the vocabulary stays bounded. Running inference in f16 halves memory and often speeds up matmul on a capable GPU. See [[pkm/183-linking-your-thinking]]. See [[transformers/184-context-windows]].
