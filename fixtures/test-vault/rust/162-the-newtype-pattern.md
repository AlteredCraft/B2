---
b2id: 01KXF21DYGHAAF3FJFTKW6F11V
type: note
title: "The Newtype Pattern"
---

# The Newtype Pattern

Notes on the newtype pattern within the broader theme of Rust.

## Interior Mutability

Deriving Debug on public data types costs nothing and pays off in every log line. Iterators are lazy, so chaining map and filter allocates nothing until you collect. Ownership gives every value a single clear owner, and the borrow checker enforces it at compile time. A struct that sprouts a lifetime parameter usually wants owned data or a key instead. See [[rust/182-send-and-sync]].

Iterators are lazy, so chaining map and filter allocates nothing until you collect. Deriving Debug on public data types costs nothing and pays off in every log line. thiserror generates Display and From impls, so a library's error enum stays declarative. A newtype wraps a primitive to give it a distinct type and a place to hang invariants. A trait object erases the concrete type behind a vtable for runtime polymorphism. See [[rust/182-send-and-sync]]. See [[rust/142-trait-objects]].

## Lifetimes Explained

Avoid unwrap in production paths and degrade gracefully with match, if let, or the question mark. A trait object erases the concrete type behind a vtable for runtime polymorphism. Send means a type can move across threads; Sync means it can be shared by reference. A struct that sprouts a lifetime parameter usually wants owned data or a key instead. See [[rust/192-iterators-and-laziness]].

Prefer keys or indices over Rc<RefCell<T>> when a data structure needs to reference itself. Send means a type can move across threads; Sync means it can be shared by reference. A struct that sprouts a lifetime parameter usually wants owned data or a key instead. Avoid unwrap in production paths and degrade gracefully with match, if let, or the question mark.

Iterators are lazy, so chaining map and filter allocates nothing until you collect. Send means a type can move across threads; Sync means it can be shared by reference. A newtype wraps a primitive to give it a distinct type and a place to hang invariants. A trait object erases the concrete type behind a vtable for runtime polymorphism. Deriving Debug on public data types costs nothing and pays off in every log line. A struct that sprouts a lifetime parameter usually wants owned data or a key instead. See [[productivity/156-timeboxing]]. See [[rust/192-iterators-and-laziness]].

## Trait Objects

Ownership gives every value a single clear owner, and the borrow checker enforces it at compile time. Iterators are lazy, so chaining map and filter allocates nothing until you collect. A newtype wraps a primitive to give it a distinct type and a place to hang invariants. Accept &str and &[T] in signatures; return owned types and let the caller borrow.

Avoid unwrap in production paths and degrade gracefully with match, if let, or the question mark. A newtype wraps a primitive to give it a distinct type and a place to hang invariants. Deriving Debug on public data types costs nothing and pays off in every log line. A struct that sprouts a lifetime parameter usually wants owned data or a key instead. A trait object erases the concrete type behind a vtable for runtime polymorphism. Prefer keys or indices over Rc<RefCell<T>> when a data structure needs to reference itself.

## Error Enums with thiserror

Lifetimes are just names for how long a borrow is valid; most can be elided. Prefer keys or indices over Rc<RefCell<T>> when a data structure needs to reference itself. thiserror generates Display and From impls, so a library's error enum stays declarative. Accept &str and &[T] in signatures; return owned types and let the caller borrow. Send means a type can move across threads; Sync means it can be shared by reference. See [[rust/152-zero-cost-abstractions]]. See [[rust/052-lifetimes-explained]].

Iterators are lazy, so chaining map and filter allocates nothing until you collect. Deriving Debug on public data types costs nothing and pays off in every log line. Accept &str and &[T] in signatures; return owned types and let the caller borrow. Avoid unwrap in production paths and degrade gracefully with match, if let, or the question mark. See [[rust/172-zero-cost-abstractions]]. See [[coffee/158-freshness-and-degassing]].

Avoid unwrap in production paths and degrade gracefully with match, if let, or the question mark. Lifetimes are just names for how long a borrow is valid; most can be elided. A struct that sprouts a lifetime parameter usually wants owned data or a key instead. A trait object erases the concrete type behind a vtable for runtime polymorphism. Ownership gives every value a single clear owner, and the borrow checker enforces it at compile time.

## Zero-Cost Abstractions

A newtype wraps a primitive to give it a distinct type and a place to hang invariants. Avoid unwrap in production paths and degrade gracefully with match, if let, or the question mark. Iterators are lazy, so chaining map and filter allocates nothing until you collect. Send means a type can move across threads; Sync means it can be shared by reference. See [[rust/172-zero-cost-abstractions]].

Accept &str and &[T] in signatures; return owned types and let the caller borrow. Ownership gives every value a single clear owner, and the borrow checker enforces it at compile time. Iterators are lazy, so chaining map and filter allocates nothing until you collect. thiserror generates Display and From impls, so a library's error enum stays declarative. A newtype wraps a primitive to give it a distinct type and a place to hang invariants. See [[rust/012-error-enums-with-thiserror]].
