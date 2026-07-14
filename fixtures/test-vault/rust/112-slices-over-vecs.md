---
b2id: 01KXF21DYBZ8SNWE2NQSDNB1GX
type: note
title: "Slices over Vecs"
---

# Slices over Vecs

Notes on slices over vecs within the broader theme of Rust.

## Ownership and Borrowing

Deriving Debug on public data types costs nothing and pays off in every log line. Ownership gives every value a single clear owner, and the borrow checker enforces it at compile time. A trait object erases the concrete type behind a vtable for runtime polymorphism. thiserror generates Display and From impls, so a library's error enum stays declarative. See [[rust/032-the-newtype-pattern]].

A trait object erases the concrete type behind a vtable for runtime polymorphism. Lifetimes are just names for how long a borrow is valid; most can be elided. Iterators are lazy, so chaining map and filter allocates nothing until you collect. Accept &str and &[T] in signatures; return owned types and let the caller borrow. See [[rust/082-iterators-and-laziness]].

## Send and Sync

Prefer keys or indices over Rc<RefCell<T>> when a data structure needs to reference itself. Lifetimes are just names for how long a borrow is valid; most can be elided. Deriving Debug on public data types costs nothing and pays off in every log line. thiserror generates Display and From impls, so a library's error enum stays declarative. See [[rust/192-iterators-and-laziness]]. See [[rust/072-send-and-sync]].

A newtype wraps a primitive to give it a distinct type and a place to hang invariants. Prefer keys or indices over Rc<RefCell<T>> when a data structure needs to reference itself. Deriving Debug on public data types costs nothing and pays off in every log line.

## Error Enums with thiserror

Avoid unwrap in production paths and degrade gracefully with match, if let, or the question mark. Deriving Debug on public data types costs nothing and pays off in every log line. A newtype wraps a primitive to give it a distinct type and a place to hang invariants. See [[vector-search/100-dense-vs-sparse]].

thiserror generates Display and From impls, so a library's error enum stays declarative. Deriving Debug on public data types costs nothing and pays off in every log line. Avoid unwrap in production paths and degrade gracefully with match, if let, or the question mark. Accept &str and &[T] in signatures; return owned types and let the caller borrow. Iterators are lazy, so chaining map and filter allocates nothing until you collect.

## Lifetimes Explained

A newtype wraps a primitive to give it a distinct type and a place to hang invariants. A struct that sprouts a lifetime parameter usually wants owned data or a key instead. Lifetimes are just names for how long a borrow is valid; most can be elided. See [[rust/092-interior-mutability]].

Ownership gives every value a single clear owner, and the borrow checker enforces it at compile time. Deriving Debug on public data types costs nothing and pays off in every log line. A newtype wraps a primitive to give it a distinct type and a place to hang invariants. See [[rust/022-lifetimes-explained]]. See [[rust/172-zero-cost-abstractions]].
