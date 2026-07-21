---
b2id: 01KXF21DYFB93DH9V5P68G8VHR
type: note
title: "Zero-Cost Abstractions"
b2_relations:
  - "supports [[rust/172-zero-cost-abstractions]] — see also"
---

# Zero-Cost Abstractions

Notes on zero-cost abstractions within the broader theme of Rust.

## Slices over Vecs

A newtype wraps a primitive to give it a distinct type and a place to hang invariants. A trait object erases the concrete type behind a vtable for runtime polymorphism. Accept &str and &[T] in signatures; return owned types and let the caller borrow. Avoid unwrap in production paths and degrade gracefully with match, if let, or the question mark.

Deriving Debug on public data types costs nothing and pays off in every log line. A newtype wraps a primitive to give it a distinct type and a place to hang invariants. A struct that sprouts a lifetime parameter usually wants owned data or a key instead. Iterators are lazy, so chaining map and filter allocates nothing until you collect. Lifetimes are just names for how long a borrow is valid; most can be elided. Avoid unwrap in production paths and degrade gracefully with match, if let, or the question mark. See [[rust/162-the-newtype-pattern]]. See [[coffee/008-extraction-yield]].

## Interior Mutability

A newtype wraps a primitive to give it a distinct type and a place to hang invariants. Prefer keys or indices over Rc<RefCell<T>> when a data structure needs to reference itself. Ownership gives every value a single clear owner, and the borrow checker enforces it at compile time. Deriving Debug on public data types costs nothing and pays off in every log line. A trait object erases the concrete type behind a vtable for runtime polymorphism. Send means a type can move across threads; Sync means it can be shared by reference. See [[rust/002-lifetimes-explained]].

A trait object erases the concrete type behind a vtable for runtime polymorphism. Deriving Debug on public data types costs nothing and pays off in every log line. A newtype wraps a primitive to give it a distinct type and a place to hang invariants. thiserror generates Display and From impls, so a library's error enum stays declarative. Accept &str and &[T] in signatures; return owned types and let the caller borrow. Iterators are lazy, so chaining map and filter allocates nothing until you collect. See [[rust/192-iterators-and-laziness]].

Prefer keys or indices over Rc<RefCell<T>> when a data structure needs to reference itself. Deriving Debug on public data types costs nothing and pays off in every log line. Send means a type can move across threads; Sync means it can be shared by reference. A newtype wraps a primitive to give it a distinct type and a place to hang invariants. Accept &str and &[T] in signatures; return owned types and let the caller borrow. A struct that sprouts a lifetime parameter usually wants owned data or a key instead. See [[rust/182-send-and-sync]]. See [[rust/092-interior-mutability]].

## Trait Objects

Prefer keys or indices over Rc<RefCell<T>> when a data structure needs to reference itself. Send means a type can move across threads; Sync means it can be shared by reference. Deriving Debug on public data types costs nothing and pays off in every log line. See [[rust/012-error-enums-with-thiserror]].

A newtype wraps a primitive to give it a distinct type and a place to hang invariants. A struct that sprouts a lifetime parameter usually wants owned data or a key instead. Iterators are lazy, so chaining map and filter allocates nothing until you collect. See [[rust/132-interior-mutability]].

## Ownership and Borrowing

Iterators are lazy, so chaining map and filter allocates nothing until you collect. Avoid unwrap in production paths and degrade gracefully with match, if let, or the question mark. thiserror generates Display and From impls, so a library's error enum stays declarative. Send means a type can move across threads; Sync means it can be shared by reference. Accept &str and &[T] in signatures; return owned types and let the caller borrow. See [[rust/162-the-newtype-pattern]].

Deriving Debug on public data types costs nothing and pays off in every log line. Send means a type can move across threads; Sync means it can be shared by reference. Avoid unwrap in production paths and degrade gracefully with match, if let, or the question mark. Ownership gives every value a single clear owner, and the borrow checker enforces it at compile time. Lifetimes are just names for how long a borrow is valid; most can be elided. Accept &str and &[T] in signatures; return owned types and let the caller borrow.
