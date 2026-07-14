---
b2id: 01KXF21DY7Z12AG76CES5JH2MY
type: note
title: "Zero-Cost Abstractions"
---

# Zero-Cost Abstractions

Notes on zero-cost abstractions within the broader theme of Rust.

## Interior Mutability

Deriving Debug on public data types costs nothing and pays off in every log line. Iterators are lazy, so chaining map and filter allocates nothing until you collect. Send means a type can move across threads; Sync means it can be shared by reference. Lifetimes are just names for how long a borrow is valid; most can be elided. Accept &str and &[T] in signatures; return owned types and let the caller borrow. Avoid unwrap in production paths and degrade gracefully with match, if let, or the question mark.

thiserror generates Display and From impls, so a library's error enum stays declarative. Ownership gives every value a single clear owner, and the borrow checker enforces it at compile time. Iterators are lazy, so chaining map and filter allocates nothing until you collect.

Lifetimes are just names for how long a borrow is valid; most can be elided. A newtype wraps a primitive to give it a distinct type and a place to hang invariants. Prefer keys or indices over Rc<RefCell<T>> when a data structure needs to reference itself. See [[vector-search/180-reranking-candidates]]. See [[rust/032-the-newtype-pattern]].

## Ownership and Borrowing

Deriving Debug on public data types costs nothing and pays off in every log line. A trait object erases the concrete type behind a vtable for runtime polymorphism. A struct that sprouts a lifetime parameter usually wants owned data or a key instead. Accept &str and &[T] in signatures; return owned types and let the caller borrow. Send means a type can move across threads; Sync means it can be shared by reference.

Send means a type can move across threads; Sync means it can be shared by reference. thiserror generates Display and From impls, so a library's error enum stays declarative. Deriving Debug on public data types costs nothing and pays off in every log line.

## The Newtype Pattern

thiserror generates Display and From impls, so a library's error enum stays declarative. Send means a type can move across threads; Sync means it can be shared by reference. Accept &str and &[T] in signatures; return owned types and let the caller borrow. See [[distributed-systems/081-retries-and-jitter]].

Send means a type can move across threads; Sync means it can be shared by reference. Ownership gives every value a single clear owner, and the borrow checker enforces it at compile time. Lifetimes are just names for how long a borrow is valid; most can be elided. Iterators are lazy, so chaining map and filter allocates nothing until you collect. Accept &str and &[T] in signatures; return owned types and let the caller borrow. See [[rust/002-lifetimes-explained]].

## The Newtype Pattern

Ownership gives every value a single clear owner, and the borrow checker enforces it at compile time. A newtype wraps a primitive to give it a distinct type and a place to hang invariants. thiserror generates Display and From impls, so a library's error enum stays declarative. Deriving Debug on public data types costs nothing and pays off in every log line. Lifetimes are just names for how long a borrow is valid; most can be elided. A struct that sprouts a lifetime parameter usually wants owned data or a key instead. See [[hiking/199-blister-prevention]].

Send means a type can move across threads; Sync means it can be shared by reference. Avoid unwrap in production paths and degrade gracefully with match, if let, or the question mark. A trait object erases the concrete type behind a vtable for runtime polymorphism. Iterators are lazy, so chaining map and filter allocates nothing until you collect. thiserror generates Display and From impls, so a library's error enum stays declarative. See [[rust/132-interior-mutability]].

## Slices over Vecs

Accept &str and &[T] in signatures; return owned types and let the caller borrow. A trait object erases the concrete type behind a vtable for runtime polymorphism. A struct that sprouts a lifetime parameter usually wants owned data or a key instead. A newtype wraps a primitive to give it a distinct type and a place to hang invariants. See [[distributed-systems/191-retries-and-jitter]].

Ownership gives every value a single clear owner, and the borrow checker enforces it at compile time. thiserror generates Display and From impls, so a library's error enum stays declarative. Avoid unwrap in production paths and degrade gracefully with match, if let, or the question mark. A struct that sprouts a lifetime parameter usually wants owned data or a key instead. A trait object erases the concrete type behind a vtable for runtime polymorphism. See [[rust/172-zero-cost-abstractions]]. See [[rust/082-iterators-and-laziness]].

A newtype wraps a primitive to give it a distinct type and a place to hang invariants. Lifetimes are just names for how long a borrow is valid; most can be elided. Send means a type can move across threads; Sync means it can be shared by reference. Ownership gives every value a single clear owner, and the borrow checker enforces it at compile time. See [[distributed-systems/181-retries-and-jitter]].
