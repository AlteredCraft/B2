---
b2id: 01KXF21DYBAPBAKENYN8GH318H
type: note
title: "Send and Sync"
---

# Send and Sync

Notes on send and sync within the broader theme of Rust.

## Zero-Cost Abstractions

Lifetimes are just names for how long a borrow is valid; most can be elided. A newtype wraps a primitive to give it a distinct type and a place to hang invariants. Ownership gives every value a single clear owner, and the borrow checker enforces it at compile time. Accept &str and &[T] in signatures; return owned types and let the caller borrow. Prefer keys or indices over Rc<RefCell<T>> when a data structure needs to reference itself.

Prefer keys or indices over Rc<RefCell<T>> when a data structure needs to reference itself. Send means a type can move across threads; Sync means it can be shared by reference. A newtype wraps a primitive to give it a distinct type and a place to hang invariants. Iterators are lazy, so chaining map and filter allocates nothing until you collect. See [[transformers/044-distillation]].

## Slices over Vecs

Send means a type can move across threads; Sync means it can be shared by reference. Deriving Debug on public data types costs nothing and pays off in every log line. Lifetimes are just names for how long a borrow is valid; most can be elided. Ownership gives every value a single clear owner, and the borrow checker enforces it at compile time. Iterators are lazy, so chaining map and filter allocates nothing until you collect. Prefer keys or indices over Rc<RefCell<T>> when a data structure needs to reference itself.

Lifetimes are just names for how long a borrow is valid; most can be elided. Accept &str and &[T] in signatures; return owned types and let the caller borrow. Deriving Debug on public data types costs nothing and pays off in every log line. Iterators are lazy, so chaining map and filter allocates nothing until you collect. See [[rust/052-lifetimes-explained]]. See [[rust/052-lifetimes-explained]].

## Slices over Vecs

A newtype wraps a primitive to give it a distinct type and a place to hang invariants. Accept &str and &[T] in signatures; return owned types and let the caller borrow. Ownership gives every value a single clear owner, and the borrow checker enforces it at compile time. Avoid unwrap in production paths and degrade gracefully with match, if let, or the question mark.

Prefer keys or indices over Rc<RefCell<T>> when a data structure needs to reference itself. Iterators are lazy, so chaining map and filter allocates nothing until you collect. Lifetimes are just names for how long a borrow is valid; most can be elided. A newtype wraps a primitive to give it a distinct type and a place to hang invariants. A struct that sprouts a lifetime parameter usually wants owned data or a key instead. See [[rust/192-iterators-and-laziness]].

## Interior Mutability

A newtype wraps a primitive to give it a distinct type and a place to hang invariants. A struct that sprouts a lifetime parameter usually wants owned data or a key instead. Iterators are lazy, so chaining map and filter allocates nothing until you collect. See [[rust/032-the-newtype-pattern]]. See [[rust/092-interior-mutability]].

A newtype wraps a primitive to give it a distinct type and a place to hang invariants. thiserror generates Display and From impls, so a library's error enum stays declarative. A trait object erases the concrete type behind a vtable for runtime polymorphism. Lifetimes are just names for how long a borrow is valid; most can be elided. Deriving Debug on public data types costs nothing and pays off in every log line. A struct that sprouts a lifetime parameter usually wants owned data or a key instead.

Avoid unwrap in production paths and degrade gracefully with match, if let, or the question mark. Iterators are lazy, so chaining map and filter allocates nothing until you collect. Lifetimes are just names for how long a borrow is valid; most can be elided. Ownership gives every value a single clear owner, and the borrow checker enforces it at compile time. thiserror generates Display and From impls, so a library's error enum stays declarative. See [[rust/092-interior-mutability]].

## Iterators and Laziness

Send means a type can move across threads; Sync means it can be shared by reference. Iterators are lazy, so chaining map and filter allocates nothing until you collect. Lifetimes are just names for how long a borrow is valid; most can be elided. See [[rust/132-interior-mutability]]. See [[rust/132-interior-mutability]].

Send means a type can move across threads; Sync means it can be shared by reference. Iterators are lazy, so chaining map and filter allocates nothing until you collect. Deriving Debug on public data types costs nothing and pays off in every log line. See [[productivity/056-shipping-small]].

Iterators are lazy, so chaining map and filter allocates nothing until you collect. Send means a type can move across threads; Sync means it can be shared by reference. A trait object erases the concrete type behind a vtable for runtime polymorphism. See [[rust/082-iterators-and-laziness]].

## Send and Sync

Ownership gives every value a single clear owner, and the borrow checker enforces it at compile time. Deriving Debug on public data types costs nothing and pays off in every log line. thiserror generates Display and From impls, so a library's error enum stays declarative. Lifetimes are just names for how long a borrow is valid; most can be elided. A trait object erases the concrete type behind a vtable for runtime polymorphism. A newtype wraps a primitive to give it a distinct type and a place to hang invariants.

A struct that sprouts a lifetime parameter usually wants owned data or a key instead. Avoid unwrap in production paths and degrade gracefully with match, if let, or the question mark. Accept &str and &[T] in signatures; return owned types and let the caller borrow. Ownership gives every value a single clear owner, and the borrow checker enforces it at compile time. Send means a type can move across threads; Sync means it can be shared by reference. See [[productivity/156-timeboxing]].
