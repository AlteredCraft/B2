---
b2id: 01KXF21DY2XK1M0W3HH87VN3P2
type: note
title: "Error Enums with thiserror"
---

# Error Enums with thiserror

Notes on error enums with thiserror within the broader theme of Rust.

## Zero-Cost Abstractions

Iterators are lazy, so chaining map and filter allocates nothing until you collect. Accept &str and &[T] in signatures; return owned types and let the caller borrow. A newtype wraps a primitive to give it a distinct type and a place to hang invariants. Prefer keys or indices over Rc<RefCell<T>> when a data structure needs to reference itself. Lifetimes are just names for how long a borrow is valid; most can be elided. See [[rust/002-lifetimes-explained]]. See [[rust/022-lifetimes-explained]].

Ownership gives every value a single clear owner, and the borrow checker enforces it at compile time. thiserror generates Display and From impls, so a library's error enum stays declarative. Send means a type can move across threads; Sync means it can be shared by reference. Iterators are lazy, so chaining map and filter allocates nothing until you collect. Avoid unwrap in production paths and degrade gracefully with match, if let, or the question mark.

Iterators are lazy, so chaining map and filter allocates nothing until you collect. Deriving Debug on public data types costs nothing and pays off in every log line. Prefer keys or indices over Rc<RefCell<T>> when a data structure needs to reference itself. A struct that sprouts a lifetime parameter usually wants owned data or a key instead. Accept &str and &[T] in signatures; return owned types and let the caller borrow.

## The Newtype Pattern

A trait object erases the concrete type behind a vtable for runtime polymorphism. Ownership gives every value a single clear owner, and the borrow checker enforces it at compile time. Prefer keys or indices over Rc<RefCell<T>> when a data structure needs to reference itself. See [[rust/052-lifetimes-explained]].

Send means a type can move across threads; Sync means it can be shared by reference. Accept &str and &[T] in signatures; return owned types and let the caller borrow. Lifetimes are just names for how long a borrow is valid; most can be elided. A trait object erases the concrete type behind a vtable for runtime polymorphism. See [[databases/105-sqlite-as-a-library]].

## The Newtype Pattern

Ownership gives every value a single clear owner, and the borrow checker enforces it at compile time. Deriving Debug on public data types costs nothing and pays off in every log line. thiserror generates Display and From impls, so a library's error enum stays declarative. A struct that sprouts a lifetime parameter usually wants owned data or a key instead.

Send means a type can move across threads; Sync means it can be shared by reference. Ownership gives every value a single clear owner, and the borrow checker enforces it at compile time. Prefer keys or indices over Rc<RefCell<T>> when a data structure needs to reference itself. thiserror generates Display and From impls, so a library's error enum stays declarative. A trait object erases the concrete type behind a vtable for runtime polymorphism. Deriving Debug on public data types costs nothing and pays off in every log line. See [[databases/075-acid-transactions]].
