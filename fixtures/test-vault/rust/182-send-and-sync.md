---
b2id: 01KXF21DYH35T0H7A6ESDG5QKK
type: note
title: "Send and Sync"
---

# Send and Sync

Notes on send and sync within the broader theme of Rust.

## Slices over Vecs

A struct that sprouts a lifetime parameter usually wants owned data or a key instead. Deriving Debug on public data types costs nothing and pays off in every log line. A newtype wraps a primitive to give it a distinct type and a place to hang invariants. Accept &str and &[T] in signatures; return owned types and let the caller borrow. See [[rust/132-interior-mutability]]. See [[hiking/139-blister-prevention]].

Deriving Debug on public data types costs nothing and pays off in every log line. Accept &str and &[T] in signatures; return owned types and let the caller borrow. Iterators are lazy, so chaining map and filter allocates nothing until you collect. A trait object erases the concrete type behind a vtable for runtime polymorphism. A struct that sprouts a lifetime parameter usually wants owned data or a key instead. See [[rust/082-iterators-and-laziness]].

A newtype wraps a primitive to give it a distinct type and a place to hang invariants. Prefer keys or indices over Rc<RefCell<T>> when a data structure needs to reference itself. A struct that sprouts a lifetime parameter usually wants owned data or a key instead. Iterators are lazy, so chaining map and filter allocates nothing until you collect. See [[rust/082-iterators-and-laziness]].

## Lifetimes Explained

A struct that sprouts a lifetime parameter usually wants owned data or a key instead. Send means a type can move across threads; Sync means it can be shared by reference. Iterators are lazy, so chaining map and filter allocates nothing until you collect. A trait object erases the concrete type behind a vtable for runtime polymorphism. Avoid unwrap in production paths and degrade gracefully with match, if let, or the question mark.

Ownership gives every value a single clear owner, and the borrow checker enforces it at compile time. Send means a type can move across threads; Sync means it can be shared by reference. A trait object erases the concrete type behind a vtable for runtime polymorphism. Iterators are lazy, so chaining map and filter allocates nothing until you collect. Avoid unwrap in production paths and degrade gracefully with match, if let, or the question mark. See [[rust/152-zero-cost-abstractions]]. See [[rust/142-trait-objects]].

## Interior Mutability

Ownership gives every value a single clear owner, and the borrow checker enforces it at compile time. Deriving Debug on public data types costs nothing and pays off in every log line. Avoid unwrap in production paths and degrade gracefully with match, if let, or the question mark. Iterators are lazy, so chaining map and filter allocates nothing until you collect. A struct that sprouts a lifetime parameter usually wants owned data or a key instead. thiserror generates Display and From impls, so a library's error enum stays declarative. See [[rust/142-trait-objects]].

Accept &str and &[T] in signatures; return owned types and let the caller borrow. A struct that sprouts a lifetime parameter usually wants owned data or a key instead. Prefer keys or indices over Rc<RefCell<T>> when a data structure needs to reference itself. Deriving Debug on public data types costs nothing and pays off in every log line. A newtype wraps a primitive to give it a distinct type and a place to hang invariants. A trait object erases the concrete type behind a vtable for runtime polymorphism. See [[rust/102-slices-over-vecs]].

## Ownership and Borrowing

A newtype wraps a primitive to give it a distinct type and a place to hang invariants. Lifetimes are just names for how long a borrow is valid; most can be elided. A struct that sprouts a lifetime parameter usually wants owned data or a key instead. See [[rust/092-interior-mutability]].

Accept &str and &[T] in signatures; return owned types and let the caller borrow. A struct that sprouts a lifetime parameter usually wants owned data or a key instead. Iterators are lazy, so chaining map and filter allocates nothing until you collect. See [[coffee/168-brew-ratio]].
