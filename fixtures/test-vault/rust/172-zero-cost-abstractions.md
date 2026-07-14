---
b2id: 01KXF21DYGDBK2Q1R30SJDQZ5Q
type: note
title: "Zero-Cost Abstractions"
---

# Zero-Cost Abstractions

Notes on zero-cost abstractions within the broader theme of Rust.

## Zero-Cost Abstractions

Lifetimes are just names for how long a borrow is valid; most can be elided. Send means a type can move across threads; Sync means it can be shared by reference. Ownership gives every value a single clear owner, and the borrow checker enforces it at compile time. Deriving Debug on public data types costs nothing and pays off in every log line. A newtype wraps a primitive to give it a distinct type and a place to hang invariants.

A trait object erases the concrete type behind a vtable for runtime polymorphism. A newtype wraps a primitive to give it a distinct type and a place to hang invariants. Prefer keys or indices over Rc<RefCell<T>> when a data structure needs to reference itself. Send means a type can move across threads; Sync means it can be shared by reference. thiserror generates Display and From impls, so a library's error enum stays declarative. Deriving Debug on public data types costs nothing and pays off in every log line. See [[databases/045-connection-pooling]].

Prefer keys or indices over Rc<RefCell<T>> when a data structure needs to reference itself. Accept &str and &[T] in signatures; return owned types and let the caller borrow. A trait object erases the concrete type behind a vtable for runtime polymorphism. thiserror generates Display and From impls, so a library's error enum stays declarative.

## Interior Mutability

Prefer keys or indices over Rc<RefCell<T>> when a data structure needs to reference itself. Avoid unwrap in production paths and degrade gracefully with match, if let, or the question mark. Deriving Debug on public data types costs nothing and pays off in every log line. A struct that sprouts a lifetime parameter usually wants owned data or a key instead.

Prefer keys or indices over Rc<RefCell<T>> when a data structure needs to reference itself. A struct that sprouts a lifetime parameter usually wants owned data or a key instead. Deriving Debug on public data types costs nothing and pays off in every log line. Accept &str and &[T] in signatures; return owned types and let the caller borrow. See [[rust/062-zero-cost-abstractions]].

## The Newtype Pattern

A struct that sprouts a lifetime parameter usually wants owned data or a key instead. Lifetimes are just names for how long a borrow is valid; most can be elided. thiserror generates Display and From impls, so a library's error enum stays declarative. A trait object erases the concrete type behind a vtable for runtime polymorphism.

Lifetimes are just names for how long a borrow is valid; most can be elided. A trait object erases the concrete type behind a vtable for runtime polymorphism. Iterators are lazy, so chaining map and filter allocates nothing until you collect.

## Trait Objects

Iterators are lazy, so chaining map and filter allocates nothing until you collect. Avoid unwrap in production paths and degrade gracefully with match, if let, or the question mark. Send means a type can move across threads; Sync means it can be shared by reference. Prefer keys or indices over Rc<RefCell<T>> when a data structure needs to reference itself. A trait object erases the concrete type behind a vtable for runtime polymorphism. See [[rust/182-send-and-sync]].

Prefer keys or indices over Rc<RefCell<T>> when a data structure needs to reference itself. Iterators are lazy, so chaining map and filter allocates nothing until you collect. A trait object erases the concrete type behind a vtable for runtime polymorphism. Accept &str and &[T] in signatures; return owned types and let the caller borrow. Lifetimes are just names for how long a borrow is valid; most can be elided. See [[productivity/026-single-tasking]]. See [[productivity/176-energy-management]].

A struct that sprouts a lifetime parameter usually wants owned data or a key instead. Lifetimes are just names for how long a borrow is valid; most can be elided. thiserror generates Display and From impls, so a library's error enum stays declarative. Prefer keys or indices over Rc<RefCell<T>> when a data structure needs to reference itself. See [[productivity/196-deep-work]]. See [[rust/042-interior-mutability]].

## The Newtype Pattern

Deriving Debug on public data types costs nothing and pays off in every log line. Lifetimes are just names for how long a borrow is valid; most can be elided. A newtype wraps a primitive to give it a distinct type and a place to hang invariants. A trait object erases the concrete type behind a vtable for runtime polymorphism. Avoid unwrap in production paths and degrade gracefully with match, if let, or the question mark. Accept &str and &[T] in signatures; return owned types and let the caller borrow. See [[databases/095-denormalization]]. See [[rust/142-trait-objects]].

Lifetimes are just names for how long a borrow is valid; most can be elided. Send means a type can move across threads; Sync means it can be shared by reference. Accept &str and &[T] in signatures; return owned types and let the caller borrow. Deriving Debug on public data types costs nothing and pays off in every log line. Ownership gives every value a single clear owner, and the borrow checker enforces it at compile time.

Prefer keys or indices over Rc<RefCell<T>> when a data structure needs to reference itself. Iterators are lazy, so chaining map and filter allocates nothing until you collect. A trait object erases the concrete type behind a vtable for runtime polymorphism. A struct that sprouts a lifetime parameter usually wants owned data or a key instead. Accept &str and &[T] in signatures; return owned types and let the caller borrow. See [[rust/192-iterators-and-laziness]].
