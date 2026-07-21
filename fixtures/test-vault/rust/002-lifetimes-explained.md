---
b2id: 01KXF21DY16E8W4QQNQVF6BKCJ
type: note
title: "Lifetimes Explained"
b2_relations:
  - "elaborates [[rust/082-iterators-and-laziness]] — see also"
---

# Lifetimes Explained

Notes on lifetimes explained within the broader theme of Rust.

## Zero-Cost Abstractions

Prefer keys or indices over Rc<RefCell<T>> when a data structure needs to reference itself. Deriving Debug on public data types costs nothing and pays off in every log line. Accept &str and &[T] in signatures; return owned types and let the caller borrow. Ownership gives every value a single clear owner, and the borrow checker enforces it at compile time. See [[databases/125-acid-transactions]]. See [[rust/162-the-newtype-pattern]].

Deriving Debug on public data types costs nothing and pays off in every log line. Ownership gives every value a single clear owner, and the borrow checker enforces it at compile time. A trait object erases the concrete type behind a vtable for runtime polymorphism. See [[hiking/099-blister-prevention]]. See [[rust/062-zero-cost-abstractions]].

## Iterators and Laziness

Deriving Debug on public data types costs nothing and pays off in every log line. Iterators are lazy, so chaining map and filter allocates nothing until you collect. A struct that sprouts a lifetime parameter usually wants owned data or a key instead. Prefer keys or indices over Rc<RefCell<T>> when a data structure needs to reference itself. Avoid unwrap in production paths and degrade gracefully with match, if let, or the question mark. Ownership gives every value a single clear owner, and the borrow checker enforces it at compile time. See [[rust/152-zero-cost-abstractions]]. See [[rust/142-trait-objects]].

Send means a type can move across threads; Sync means it can be shared by reference. Iterators are lazy, so chaining map and filter allocates nothing until you collect. Prefer keys or indices over Rc<RefCell<T>> when a data structure needs to reference itself. Deriving Debug on public data types costs nothing and pays off in every log line. See [[rust/042-interior-mutability]].

## Send and Sync

A newtype wraps a primitive to give it a distinct type and a place to hang invariants. Iterators are lazy, so chaining map and filter allocates nothing until you collect. Ownership gives every value a single clear owner, and the borrow checker enforces it at compile time. See [[rust/082-iterators-and-laziness]]. See [[rust/162-the-newtype-pattern]].

A newtype wraps a primitive to give it a distinct type and a place to hang invariants. Avoid unwrap in production paths and degrade gracefully with match, if let, or the question mark. Accept &str and &[T] in signatures; return owned types and let the caller borrow. Send means a type can move across threads; Sync means it can be shared by reference. Lifetimes are just names for how long a borrow is valid; most can be elided. Iterators are lazy, so chaining map and filter allocates nothing until you collect.

## Iterators and Laziness

Avoid unwrap in production paths and degrade gracefully with match, if let, or the question mark. Prefer keys or indices over Rc<RefCell<T>> when a data structure needs to reference itself. A trait object erases the concrete type behind a vtable for runtime polymorphism. See [[rust/172-zero-cost-abstractions]].

thiserror generates Display and From impls, so a library's error enum stays declarative. A trait object erases the concrete type behind a vtable for runtime polymorphism. A struct that sprouts a lifetime parameter usually wants owned data or a key instead. Iterators are lazy, so chaining map and filter allocates nothing until you collect. Send means a type can move across threads; Sync means it can be shared by reference.

Iterators are lazy, so chaining map and filter allocates nothing until you collect. Deriving Debug on public data types costs nothing and pays off in every log line. Prefer keys or indices over Rc<RefCell<T>> when a data structure needs to reference itself. See [[coffee/018-freshness-and-degassing]]. See [[rust/052-lifetimes-explained]].

## Send and Sync

Ownership gives every value a single clear owner, and the borrow checker enforces it at compile time. Prefer keys or indices over Rc<RefCell<T>> when a data structure needs to reference itself. Avoid unwrap in production paths and degrade gracefully with match, if let, or the question mark. Send means a type can move across threads; Sync means it can be shared by reference. Accept &str and &[T] in signatures; return owned types and let the caller borrow.

Lifetimes are just names for how long a borrow is valid; most can be elided. Send means a type can move across threads; Sync means it can be shared by reference. Avoid unwrap in production paths and degrade gracefully with match, if let, or the question mark. thiserror generates Display and From impls, so a library's error enum stays declarative. See [[rust/172-zero-cost-abstractions]].

Iterators are lazy, so chaining map and filter allocates nothing until you collect. Deriving Debug on public data types costs nothing and pays off in every log line. Accept &str and &[T] in signatures; return owned types and let the caller borrow. A newtype wraps a primitive to give it a distinct type and a place to hang invariants. thiserror generates Display and From impls, so a library's error enum stays declarative.
