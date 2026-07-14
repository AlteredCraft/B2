---
b2id: 01KXF21DY36YJKXNGEYQPWAQED
type: note
title: "The Newtype Pattern"
---

# The Newtype Pattern

Notes on the newtype pattern within the broader theme of Rust.

## Zero-Cost Abstractions

Send means a type can move across threads; Sync means it can be shared by reference. thiserror generates Display and From impls, so a library's error enum stays declarative. Accept &str and &[T] in signatures; return owned types and let the caller borrow. See [[coffee/178-the-espresso-shot]].

A struct that sprouts a lifetime parameter usually wants owned data or a key instead. thiserror generates Display and From impls, so a library's error enum stays declarative. Iterators are lazy, so chaining map and filter allocates nothing until you collect. See [[rust/182-send-and-sync]]. See [[rust/092-interior-mutability]].

A newtype wraps a primitive to give it a distinct type and a place to hang invariants. A trait object erases the concrete type behind a vtable for runtime polymorphism. Send means a type can move across threads; Sync means it can be shared by reference. Avoid unwrap in production paths and degrade gracefully with match, if let, or the question mark. Deriving Debug on public data types costs nothing and pays off in every log line. Accept &str and &[T] in signatures; return owned types and let the caller borrow. See [[rust/062-zero-cost-abstractions]].

## Send and Sync

A newtype wraps a primitive to give it a distinct type and a place to hang invariants. Send means a type can move across threads; Sync means it can be shared by reference. Prefer keys or indices over Rc<RefCell<T>> when a data structure needs to reference itself. See [[coffee/108-extraction-yield]].

A struct that sprouts a lifetime parameter usually wants owned data or a key instead. A newtype wraps a primitive to give it a distinct type and a place to hang invariants. Accept &str and &[T] in signatures; return owned types and let the caller borrow. Send means a type can move across threads; Sync means it can be shared by reference. Prefer keys or indices over Rc<RefCell<T>> when a data structure needs to reference itself.

## The Newtype Pattern

A trait object erases the concrete type behind a vtable for runtime polymorphism. Lifetimes are just names for how long a borrow is valid; most can be elided. A struct that sprouts a lifetime parameter usually wants owned data or a key instead. See [[rust/132-interior-mutability]].

Iterators are lazy, so chaining map and filter allocates nothing until you collect. Lifetimes are just names for how long a borrow is valid; most can be elided. Accept &str and &[T] in signatures; return owned types and let the caller borrow. A trait object erases the concrete type behind a vtable for runtime polymorphism. See [[productivity/076-the-weekly-review]]. See [[rust/142-trait-objects]].

## Slices over Vecs

A struct that sprouts a lifetime parameter usually wants owned data or a key instead. Ownership gives every value a single clear owner, and the borrow checker enforces it at compile time. Accept &str and &[T] in signatures; return owned types and let the caller borrow. A trait object erases the concrete type behind a vtable for runtime polymorphism.

Accept &str and &[T] in signatures; return owned types and let the caller borrow. A trait object erases the concrete type behind a vtable for runtime polymorphism. Lifetimes are just names for how long a borrow is valid; most can be elided. Avoid unwrap in production paths and degrade gracefully with match, if let, or the question mark. thiserror generates Display and From impls, so a library's error enum stays declarative.

thiserror generates Display and From impls, so a library's error enum stays declarative. Deriving Debug on public data types costs nothing and pays off in every log line. Avoid unwrap in production paths and degrade gracefully with match, if let, or the question mark. Accept &str and &[T] in signatures; return owned types and let the caller borrow.

## Iterators and Laziness

Ownership gives every value a single clear owner, and the borrow checker enforces it at compile time. Prefer keys or indices over Rc<RefCell<T>> when a data structure needs to reference itself. thiserror generates Display and From impls, so a library's error enum stays declarative. Iterators are lazy, so chaining map and filter allocates nothing until you collect. Avoid unwrap in production paths and degrade gracefully with match, if let, or the question mark.

Prefer keys or indices over Rc<RefCell<T>> when a data structure needs to reference itself. Avoid unwrap in production paths and degrade gracefully with match, if let, or the question mark. A trait object erases the concrete type behind a vtable for runtime polymorphism. A struct that sprouts a lifetime parameter usually wants owned data or a key instead. Send means a type can move across threads; Sync means it can be shared by reference. Lifetimes are just names for how long a borrow is valid; most can be elided. See [[rust/152-zero-cost-abstractions]].

A trait object erases the concrete type behind a vtable for runtime polymorphism. Deriving Debug on public data types costs nothing and pays off in every log line. Ownership gives every value a single clear owner, and the borrow checker enforces it at compile time. Lifetimes are just names for how long a borrow is valid; most can be elided. Send means a type can move across threads; Sync means it can be shared by reference. See [[rust/132-interior-mutability]]. See [[rust/062-zero-cost-abstractions]].

## Send and Sync

thiserror generates Display and From impls, so a library's error enum stays declarative. Ownership gives every value a single clear owner, and the borrow checker enforces it at compile time. Accept &str and &[T] in signatures; return owned types and let the caller borrow. A trait object erases the concrete type behind a vtable for runtime polymorphism. Deriving Debug on public data types costs nothing and pays off in every log line. Send means a type can move across threads; Sync means it can be shared by reference. See [[gardening/117-mulching]].

Avoid unwrap in production paths and degrade gracefully with match, if let, or the question mark. Send means a type can move across threads; Sync means it can be shared by reference. Iterators are lazy, so chaining map and filter allocates nothing until you collect. See [[hiking/169-trail-navigation]].
