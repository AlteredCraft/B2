---
b2id: 01KXF21DY3P0VH2S9CMT0SHEPZ
type: note
title: "Lifetimes Explained"
---

# Lifetimes Explained

Notes on lifetimes explained within the broader theme of Rust.

## Slices over Vecs

Prefer keys or indices over Rc<RefCell<T>> when a data structure needs to reference itself. Deriving Debug on public data types costs nothing and pays off in every log line. Accept &str and &[T] in signatures; return owned types and let the caller borrow. Lifetimes are just names for how long a borrow is valid; most can be elided.

Accept &str and &[T] in signatures; return owned types and let the caller borrow. thiserror generates Display and From impls, so a library's error enum stays declarative. Lifetimes are just names for how long a borrow is valid; most can be elided. Prefer keys or indices over Rc<RefCell<T>> when a data structure needs to reference itself. Avoid unwrap in production paths and degrade gracefully with match, if let, or the question mark. A struct that sprouts a lifetime parameter usually wants owned data or a key instead. See [[rust/192-iterators-and-laziness]]. See [[pkm/013-evergreen-notes]].

## Send and Sync

Send means a type can move across threads; Sync means it can be shared by reference. Deriving Debug on public data types costs nothing and pays off in every log line. Iterators are lazy, so chaining map and filter allocates nothing until you collect. A struct that sprouts a lifetime parameter usually wants owned data or a key instead. Prefer keys or indices over Rc<RefCell<T>> when a data structure needs to reference itself.

Send means a type can move across threads; Sync means it can be shared by reference. Iterators are lazy, so chaining map and filter allocates nothing until you collect. Avoid unwrap in production paths and degrade gracefully with match, if let, or the question mark. See [[rust/102-slices-over-vecs]].

## Slices over Vecs

Avoid unwrap in production paths and degrade gracefully with match, if let, or the question mark. Iterators are lazy, so chaining map and filter allocates nothing until you collect. A struct that sprouts a lifetime parameter usually wants owned data or a key instead. A newtype wraps a primitive to give it a distinct type and a place to hang invariants. Prefer keys or indices over Rc<RefCell<T>> when a data structure needs to reference itself. See [[rust/082-iterators-and-laziness]].

Iterators are lazy, so chaining map and filter allocates nothing until you collect. A newtype wraps a primitive to give it a distinct type and a place to hang invariants. Accept &str and &[T] in signatures; return owned types and let the caller borrow. thiserror generates Display and From impls, so a library's error enum stays declarative. Ownership gives every value a single clear owner, and the borrow checker enforces it at compile time. Send means a type can move across threads; Sync means it can be shared by reference. See [[rust/092-interior-mutability]]. See [[rust/132-interior-mutability]].

## Ownership and Borrowing

A newtype wraps a primitive to give it a distinct type and a place to hang invariants. Prefer keys or indices over Rc<RefCell<T>> when a data structure needs to reference itself. Send means a type can move across threads; Sync means it can be shared by reference. thiserror generates Display and From impls, so a library's error enum stays declarative. Ownership gives every value a single clear owner, and the borrow checker enforces it at compile time.

thiserror generates Display and From impls, so a library's error enum stays declarative. Deriving Debug on public data types costs nothing and pays off in every log line. Avoid unwrap in production paths and degrade gracefully with match, if let, or the question mark. See [[rust/112-slices-over-vecs]]. See [[rust/112-slices-over-vecs]].

## Slices over Vecs

A trait object erases the concrete type behind a vtable for runtime polymorphism. Lifetimes are just names for how long a borrow is valid; most can be elided. Accept &str and &[T] in signatures; return owned types and let the caller borrow. A newtype wraps a primitive to give it a distinct type and a place to hang invariants.

Ownership gives every value a single clear owner, and the borrow checker enforces it at compile time. Lifetimes are just names for how long a borrow is valid; most can be elided. Avoid unwrap in production paths and degrade gracefully with match, if let, or the question mark. A newtype wraps a primitive to give it a distinct type and a place to hang invariants. A struct that sprouts a lifetime parameter usually wants owned data or a key instead. Deriving Debug on public data types costs nothing and pays off in every log line. See [[rust/182-send-and-sync]]. See [[productivity/116-energy-management]].

thiserror generates Display and From impls, so a library's error enum stays declarative. Avoid unwrap in production paths and degrade gracefully with match, if let, or the question mark. Iterators are lazy, so chaining map and filter allocates nothing until you collect. Ownership gives every value a single clear owner, and the borrow checker enforces it at compile time. Lifetimes are just names for how long a borrow is valid; most can be elided.

## Iterators and Laziness

Iterators are lazy, so chaining map and filter allocates nothing until you collect. Ownership gives every value a single clear owner, and the borrow checker enforces it at compile time. A newtype wraps a primitive to give it a distinct type and a place to hang invariants. Avoid unwrap in production paths and degrade gracefully with match, if let, or the question mark. See [[rust/112-slices-over-vecs]].

A struct that sprouts a lifetime parameter usually wants owned data or a key instead. Lifetimes are just names for how long a borrow is valid; most can be elided. A trait object erases the concrete type behind a vtable for runtime polymorphism. See [[rust/112-slices-over-vecs]]. See [[rust/182-send-and-sync]].
