---
b2id: 01KXF21DYACJB5CB20SMEF7RNK
type: note
title: "Slices over Vecs"
---

# Slices over Vecs

Notes on slices over vecs within the broader theme of Rust.

## Slices over Vecs

Deriving Debug on public data types costs nothing and pays off in every log line. Iterators are lazy, so chaining map and filter allocates nothing until you collect. A struct that sprouts a lifetime parameter usually wants owned data or a key instead. A newtype wraps a primitive to give it a distinct type and a place to hang invariants. See [[rust/152-zero-cost-abstractions]].

A trait object erases the concrete type behind a vtable for runtime polymorphism. Prefer keys or indices over Rc<RefCell<T>> when a data structure needs to reference itself. Avoid unwrap in production paths and degrade gracefully with match, if let, or the question mark.

## Send and Sync

Prefer keys or indices over Rc<RefCell<T>> when a data structure needs to reference itself. Ownership gives every value a single clear owner, and the borrow checker enforces it at compile time. A trait object erases the concrete type behind a vtable for runtime polymorphism. A struct that sprouts a lifetime parameter usually wants owned data or a key instead. Lifetimes are just names for how long a borrow is valid; most can be elided. thiserror generates Display and From impls, so a library's error enum stays declarative.

Deriving Debug on public data types costs nothing and pays off in every log line. A trait object erases the concrete type behind a vtable for runtime polymorphism. Iterators are lazy, so chaining map and filter allocates nothing until you collect. Lifetimes are just names for how long a borrow is valid; most can be elided. Ownership gives every value a single clear owner, and the borrow checker enforces it at compile time.

## The Newtype Pattern

Accept &str and &[T] in signatures; return owned types and let the caller borrow. Lifetimes are just names for how long a borrow is valid; most can be elided. Iterators are lazy, so chaining map and filter allocates nothing until you collect. Deriving Debug on public data types costs nothing and pays off in every log line. A struct that sprouts a lifetime parameter usually wants owned data or a key instead. Send means a type can move across threads; Sync means it can be shared by reference. See [[rust/122-send-and-sync]]. See [[rust/002-lifetimes-explained]].

Lifetimes are just names for how long a borrow is valid; most can be elided. A struct that sprouts a lifetime parameter usually wants owned data or a key instead. Iterators are lazy, so chaining map and filter allocates nothing until you collect. See [[rust/082-iterators-and-laziness]].

Lifetimes are just names for how long a borrow is valid; most can be elided. Prefer keys or indices over Rc<RefCell<T>> when a data structure needs to reference itself. A struct that sprouts a lifetime parameter usually wants owned data or a key instead. Avoid unwrap in production paths and degrade gracefully with match, if let, or the question mark. Send means a type can move across threads; Sync means it can be shared by reference. Iterators are lazy, so chaining map and filter allocates nothing until you collect.

## Iterators and Laziness

Send means a type can move across threads; Sync means it can be shared by reference. Deriving Debug on public data types costs nothing and pays off in every log line. Accept &str and &[T] in signatures; return owned types and let the caller borrow. thiserror generates Display and From impls, so a library's error enum stays declarative. A newtype wraps a primitive to give it a distinct type and a place to hang invariants. Ownership gives every value a single clear owner, and the borrow checker enforces it at compile time. See [[distributed-systems/011-leader-election]]. See [[pkm/173-local-first-vaults]].

Accept &str and &[T] in signatures; return owned types and let the caller borrow. Send means a type can move across threads; Sync means it can be shared by reference. A struct that sprouts a lifetime parameter usually wants owned data or a key instead. Iterators are lazy, so chaining map and filter allocates nothing until you collect.
