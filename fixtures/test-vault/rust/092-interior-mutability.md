---
b2id: 01KXF21DY95CZ3XDZ2CQDB56Z8
type: note
title: "Interior Mutability"
---

# Interior Mutability

Notes on interior mutability within the broader theme of Rust.

## Zero-Cost Abstractions

Accept &str and &[T] in signatures; return owned types and let the caller borrow. thiserror generates Display and From impls, so a library's error enum stays declarative. Deriving Debug on public data types costs nothing and pays off in every log line. A trait object erases the concrete type behind a vtable for runtime polymorphism. Ownership gives every value a single clear owner, and the borrow checker enforces it at compile time. Prefer keys or indices over Rc<RefCell<T>> when a data structure needs to reference itself. See [[rust/172-zero-cost-abstractions]]. See [[rust/132-interior-mutability]].

Deriving Debug on public data types costs nothing and pays off in every log line. A struct that sprouts a lifetime parameter usually wants owned data or a key instead. Avoid unwrap in production paths and degrade gracefully with match, if let, or the question mark. thiserror generates Display and From impls, so a library's error enum stays declarative. See [[rust/062-zero-cost-abstractions]].

## The Newtype Pattern

Lifetimes are just names for how long a borrow is valid; most can be elided. Accept &str and &[T] in signatures; return owned types and let the caller borrow. Avoid unwrap in production paths and degrade gracefully with match, if let, or the question mark. A trait object erases the concrete type behind a vtable for runtime polymorphism. See [[pkm/133-the-zettelkasten]]. See [[rust/102-slices-over-vecs]].

Deriving Debug on public data types costs nothing and pays off in every log line. thiserror generates Display and From impls, so a library's error enum stays declarative. Accept &str and &[T] in signatures; return owned types and let the caller borrow. Iterators are lazy, so chaining map and filter allocates nothing until you collect.

## Slices over Vecs

Iterators are lazy, so chaining map and filter allocates nothing until you collect. A struct that sprouts a lifetime parameter usually wants owned data or a key instead. A trait object erases the concrete type behind a vtable for runtime polymorphism. Avoid unwrap in production paths and degrade gracefully with match, if let, or the question mark. A newtype wraps a primitive to give it a distinct type and a place to hang invariants. Send means a type can move across threads; Sync means it can be shared by reference. See [[rust/152-zero-cost-abstractions]]. See [[vector-search/120-the-embedding-space]].

Prefer keys or indices over Rc<RefCell<T>> when a data structure needs to reference itself. Send means a type can move across threads; Sync means it can be shared by reference. Iterators are lazy, so chaining map and filter allocates nothing until you collect.

## Trait Objects

Deriving Debug on public data types costs nothing and pays off in every log line. thiserror generates Display and From impls, so a library's error enum stays declarative. Prefer keys or indices over Rc<RefCell<T>> when a data structure needs to reference itself. A newtype wraps a primitive to give it a distinct type and a place to hang invariants. See [[rust/172-zero-cost-abstractions]]. See [[rust/142-trait-objects]].

Iterators are lazy, so chaining map and filter allocates nothing until you collect. A trait object erases the concrete type behind a vtable for runtime polymorphism. Lifetimes are just names for how long a borrow is valid; most can be elided.

## Slices over Vecs

Ownership gives every value a single clear owner, and the borrow checker enforces it at compile time. Deriving Debug on public data types costs nothing and pays off in every log line. Lifetimes are just names for how long a borrow is valid; most can be elided. A newtype wraps a primitive to give it a distinct type and a place to hang invariants. thiserror generates Display and From impls, so a library's error enum stays declarative. See [[databases/085-connection-pooling]]. See [[rust/192-iterators-and-laziness]].

A trait object erases the concrete type behind a vtable for runtime polymorphism. Iterators are lazy, so chaining map and filter allocates nothing until you collect. Avoid unwrap in production paths and degrade gracefully with match, if let, or the question mark. A struct that sprouts a lifetime parameter usually wants owned data or a key instead.

Deriving Debug on public data types costs nothing and pays off in every log line. thiserror generates Display and From impls, so a library's error enum stays declarative. A struct that sprouts a lifetime parameter usually wants owned data or a key instead.

## Send and Sync

Accept &str and &[T] in signatures; return owned types and let the caller borrow. Send means a type can move across threads; Sync means it can be shared by reference. Avoid unwrap in production paths and degrade gracefully with match, if let, or the question mark. Prefer keys or indices over Rc<RefCell<T>> when a data structure needs to reference itself. Lifetimes are just names for how long a borrow is valid; most can be elided. See [[gardening/177-mulching]]. See [[rust/122-send-and-sync]].

Iterators are lazy, so chaining map and filter allocates nothing until you collect. Avoid unwrap in production paths and degrade gracefully with match, if let, or the question mark. A trait object erases the concrete type behind a vtable for runtime polymorphism. Send means a type can move across threads; Sync means it can be shared by reference. A newtype wraps a primitive to give it a distinct type and a place to hang invariants. See [[rust/072-send-and-sync]].
