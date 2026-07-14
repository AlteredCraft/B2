---
b2id: 01KXF21DYCPR0AYV7VQEVZBN7J
type: note
title: "Interior Mutability"
---

# Interior Mutability

Notes on interior mutability within the broader theme of Rust.

## Error Enums with thiserror

thiserror generates Display and From impls, so a library's error enum stays declarative. A newtype wraps a primitive to give it a distinct type and a place to hang invariants. Avoid unwrap in production paths and degrade gracefully with match, if let, or the question mark. See [[transformers/124-positional-encoding]]. See [[rust/092-interior-mutability]].

Prefer keys or indices over Rc<RefCell<T>> when a data structure needs to reference itself. Deriving Debug on public data types costs nothing and pays off in every log line. Lifetimes are just names for how long a borrow is valid; most can be elided. See [[rust/112-slices-over-vecs]].

Lifetimes are just names for how long a borrow is valid; most can be elided. Avoid unwrap in production paths and degrade gracefully with match, if let, or the question mark. Accept &str and &[T] in signatures; return owned types and let the caller borrow. See [[rust/022-lifetimes-explained]]. See [[rust/152-zero-cost-abstractions]].

## Trait Objects

Prefer keys or indices over Rc<RefCell<T>> when a data structure needs to reference itself. A struct that sprouts a lifetime parameter usually wants owned data or a key instead. Accept &str and &[T] in signatures; return owned types and let the caller borrow. A newtype wraps a primitive to give it a distinct type and a place to hang invariants. Ownership gives every value a single clear owner, and the borrow checker enforces it at compile time. See [[rust/002-lifetimes-explained]].

Prefer keys or indices over Rc<RefCell<T>> when a data structure needs to reference itself. Iterators are lazy, so chaining map and filter allocates nothing until you collect. Avoid unwrap in production paths and degrade gracefully with match, if let, or the question mark. Lifetimes are just names for how long a borrow is valid; most can be elided. A newtype wraps a primitive to give it a distinct type and a place to hang invariants.

## Interior Mutability

thiserror generates Display and From impls, so a library's error enum stays declarative. Iterators are lazy, so chaining map and filter allocates nothing until you collect. A trait object erases the concrete type behind a vtable for runtime polymorphism. Avoid unwrap in production paths and degrade gracefully with match, if let, or the question mark.

A trait object erases the concrete type behind a vtable for runtime polymorphism. Iterators are lazy, so chaining map and filter allocates nothing until you collect. Accept &str and &[T] in signatures; return owned types and let the caller borrow. Lifetimes are just names for how long a borrow is valid; most can be elided. Prefer keys or indices over Rc<RefCell<T>> when a data structure needs to reference itself. See [[rust/112-slices-over-vecs]]. See [[rust/012-error-enums-with-thiserror]].

A newtype wraps a primitive to give it a distinct type and a place to hang invariants. thiserror generates Display and From impls, so a library's error enum stays declarative. A struct that sprouts a lifetime parameter usually wants owned data or a key instead. A trait object erases the concrete type behind a vtable for runtime polymorphism. Send means a type can move across threads; Sync means it can be shared by reference. Ownership gives every value a single clear owner, and the borrow checker enforces it at compile time. See [[rust/082-iterators-and-laziness]].

## Send and Sync

Avoid unwrap in production paths and degrade gracefully with match, if let, or the question mark. Accept &str and &[T] in signatures; return owned types and let the caller borrow. Lifetimes are just names for how long a borrow is valid; most can be elided. A newtype wraps a primitive to give it a distinct type and a place to hang invariants. Ownership gives every value a single clear owner, and the borrow checker enforces it at compile time. See [[rust/092-interior-mutability]]. See [[rust/152-zero-cost-abstractions]].

Accept &str and &[T] in signatures; return owned types and let the caller borrow. thiserror generates Display and From impls, so a library's error enum stays declarative. A trait object erases the concrete type behind a vtable for runtime polymorphism. Send means a type can move across threads; Sync means it can be shared by reference. Ownership gives every value a single clear owner, and the borrow checker enforces it at compile time. A struct that sprouts a lifetime parameter usually wants owned data or a key instead. See [[distributed-systems/071-consensus-under-partition]].

thiserror generates Display and From impls, so a library's error enum stays declarative. Prefer keys or indices over Rc<RefCell<T>> when a data structure needs to reference itself. A struct that sprouts a lifetime parameter usually wants owned data or a key instead. Accept &str and &[T] in signatures; return owned types and let the caller borrow. See [[rust/102-slices-over-vecs]].
