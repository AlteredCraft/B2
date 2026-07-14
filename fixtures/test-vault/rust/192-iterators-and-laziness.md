---
b2id: 01KXF21DYJT4F5CVDEQ4ZK54CF
type: note
title: "Iterators and Laziness"
---

# Iterators and Laziness

Notes on iterators and laziness within the broader theme of Rust.

## Slices over Vecs

Accept &str and &[T] in signatures; return owned types and let the caller borrow. Lifetimes are just names for how long a borrow is valid; most can be elided. Ownership gives every value a single clear owner, and the borrow checker enforces it at compile time. Deriving Debug on public data types costs nothing and pays off in every log line. See [[rust/062-zero-cost-abstractions]].

Ownership gives every value a single clear owner, and the borrow checker enforces it at compile time. Deriving Debug on public data types costs nothing and pays off in every log line. Iterators are lazy, so chaining map and filter allocates nothing until you collect. Avoid unwrap in production paths and degrade gracefully with match, if let, or the question mark. A newtype wraps a primitive to give it a distinct type and a place to hang invariants. See [[databases/095-denormalization]].

## Interior Mutability

Prefer keys or indices over Rc<RefCell<T>> when a data structure needs to reference itself. Iterators are lazy, so chaining map and filter allocates nothing until you collect. Accept &str and &[T] in signatures; return owned types and let the caller borrow. Deriving Debug on public data types costs nothing and pays off in every log line. A struct that sprouts a lifetime parameter usually wants owned data or a key instead. Avoid unwrap in production paths and degrade gracefully with match, if let, or the question mark. See [[rust/112-slices-over-vecs]].

thiserror generates Display and From impls, so a library's error enum stays declarative. Deriving Debug on public data types costs nothing and pays off in every log line. Avoid unwrap in production paths and degrade gracefully with match, if let, or the question mark. Prefer keys or indices over Rc<RefCell<T>> when a data structure needs to reference itself. Ownership gives every value a single clear owner, and the borrow checker enforces it at compile time. See [[coffee/178-the-espresso-shot]].

## Trait Objects

thiserror generates Display and From impls, so a library's error enum stays declarative. A trait object erases the concrete type behind a vtable for runtime polymorphism. Prefer keys or indices over Rc<RefCell<T>> when a data structure needs to reference itself.

A trait object erases the concrete type behind a vtable for runtime polymorphism. Iterators are lazy, so chaining map and filter allocates nothing until you collect. Accept &str and &[T] in signatures; return owned types and let the caller borrow. Send means a type can move across threads; Sync means it can be shared by reference. Lifetimes are just names for how long a borrow is valid; most can be elided. See [[rust/042-interior-mutability]].

## Interior Mutability

A trait object erases the concrete type behind a vtable for runtime polymorphism. Iterators are lazy, so chaining map and filter allocates nothing until you collect. thiserror generates Display and From impls, so a library's error enum stays declarative. Deriving Debug on public data types costs nothing and pays off in every log line. See [[hiking/079-trail-etiquette]]. See [[vector-search/120-the-embedding-space]].

Iterators are lazy, so chaining map and filter allocates nothing until you collect. A newtype wraps a primitive to give it a distinct type and a place to hang invariants. Accept &str and &[T] in signatures; return owned types and let the caller borrow. Avoid unwrap in production paths and degrade gracefully with match, if let, or the question mark. See [[vector-search/170-the-embedding-space]].
