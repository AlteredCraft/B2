---
b2id: 01KXF21DY4JRTR2HEM3Q7WRMSR
type: note
title: "Interior Mutability"
---

# Interior Mutability

Notes on interior mutability within the broader theme of Rust.

## Slices over Vecs

A trait object erases the concrete type behind a vtable for runtime polymorphism. Accept &str and &[T] in signatures; return owned types and let the caller borrow. Deriving Debug on public data types costs nothing and pays off in every log line. Send means a type can move across threads; Sync means it can be shared by reference. A struct that sprouts a lifetime parameter usually wants owned data or a key instead. See [[coffee/078-water-temperature]].

A newtype wraps a primitive to give it a distinct type and a place to hang invariants. Prefer keys or indices over Rc<RefCell<T>> when a data structure needs to reference itself. Send means a type can move across threads; Sync means it can be shared by reference. thiserror generates Display and From impls, so a library's error enum stays declarative. Iterators are lazy, so chaining map and filter allocates nothing until you collect. A struct that sprouts a lifetime parameter usually wants owned data or a key instead. See [[gardening/187-mulching]]. See [[productivity/096-timeboxing]].

## The Newtype Pattern

A trait object erases the concrete type behind a vtable for runtime polymorphism. Prefer keys or indices over Rc<RefCell<T>> when a data structure needs to reference itself. A newtype wraps a primitive to give it a distinct type and a place to hang invariants. Lifetimes are just names for how long a borrow is valid; most can be elided. Deriving Debug on public data types costs nothing and pays off in every log line. See [[rust/002-lifetimes-explained]].

Avoid unwrap in production paths and degrade gracefully with match, if let, or the question mark. A struct that sprouts a lifetime parameter usually wants owned data or a key instead. Prefer keys or indices over Rc<RefCell<T>> when a data structure needs to reference itself.

## Send and Sync

Deriving Debug on public data types costs nothing and pays off in every log line. Accept &str and &[T] in signatures; return owned types and let the caller borrow. Prefer keys or indices over Rc<RefCell<T>> when a data structure needs to reference itself. A trait object erases the concrete type behind a vtable for runtime polymorphism. See [[pkm/003-local-first-vaults]].

Avoid unwrap in production paths and degrade gracefully with match, if let, or the question mark. A newtype wraps a primitive to give it a distinct type and a place to hang invariants. Accept &str and &[T] in signatures; return owned types and let the caller borrow. Send means a type can move across threads; Sync means it can be shared by reference. Deriving Debug on public data types costs nothing and pays off in every log line. See [[rust/112-slices-over-vecs]]. See [[transformers/154-self-attention]].

Lifetimes are just names for how long a borrow is valid; most can be elided. thiserror generates Display and From impls, so a library's error enum stays declarative. Deriving Debug on public data types costs nothing and pays off in every log line. Send means a type can move across threads; Sync means it can be shared by reference. See [[gardening/127-soil-ph]]. See [[rust/192-iterators-and-laziness]].

## Error Enums with thiserror

Avoid unwrap in production paths and degrade gracefully with match, if let, or the question mark. Iterators are lazy, so chaining map and filter allocates nothing until you collect. thiserror generates Display and From impls, so a library's error enum stays declarative. Send means a type can move across threads; Sync means it can be shared by reference. Ownership gives every value a single clear owner, and the borrow checker enforces it at compile time. See [[rust/152-zero-cost-abstractions]]. See [[rust/182-send-and-sync]].

A struct that sprouts a lifetime parameter usually wants owned data or a key instead. Ownership gives every value a single clear owner, and the borrow checker enforces it at compile time. Accept &str and &[T] in signatures; return owned types and let the caller borrow. Avoid unwrap in production paths and degrade gracefully with match, if let, or the question mark. See [[rust/102-slices-over-vecs]]. See [[productivity/006-deep-work]].

Send means a type can move across threads; Sync means it can be shared by reference. Ownership gives every value a single clear owner, and the borrow checker enforces it at compile time. A newtype wraps a primitive to give it a distinct type and a place to hang invariants. Lifetimes are just names for how long a borrow is valid; most can be elided. See [[gardening/127-soil-ph]].

## Error Enums with thiserror

A newtype wraps a primitive to give it a distinct type and a place to hang invariants. thiserror generates Display and From impls, so a library's error enum stays declarative. Ownership gives every value a single clear owner, and the borrow checker enforces it at compile time. Iterators are lazy, so chaining map and filter allocates nothing until you collect. Lifetimes are just names for how long a borrow is valid; most can be elided. See [[databases/165-schema-migrations]].

Lifetimes are just names for how long a borrow is valid; most can be elided. Send means a type can move across threads; Sync means it can be shared by reference. Deriving Debug on public data types costs nothing and pays off in every log line. A trait object erases the concrete type behind a vtable for runtime polymorphism.
