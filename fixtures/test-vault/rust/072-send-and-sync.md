---
b2id: 01KXF21DY76WJ22WM5ZHEZRXG9
type: note
title: "Send and Sync"
---

# Send and Sync

Notes on send and sync within the broader theme of Rust.

## Interior Mutability

A newtype wraps a primitive to give it a distinct type and a place to hang invariants. Deriving Debug on public data types costs nothing and pays off in every log line. Lifetimes are just names for how long a borrow is valid; most can be elided. thiserror generates Display and From impls, so a library's error enum stays declarative. See [[rust/042-interior-mutability]]. See [[vector-search/170-the-embedding-space]].

Deriving Debug on public data types costs nothing and pays off in every log line. Prefer keys or indices over Rc<RefCell<T>> when a data structure needs to reference itself. Send means a type can move across threads; Sync means it can be shared by reference. Lifetimes are just names for how long a borrow is valid; most can be elided. Avoid unwrap in production paths and degrade gracefully with match, if let, or the question mark. See [[rust/132-interior-mutability]]. See [[hiking/059-switchbacks]].

## Interior Mutability

thiserror generates Display and From impls, so a library's error enum stays declarative. Avoid unwrap in production paths and degrade gracefully with match, if let, or the question mark. A struct that sprouts a lifetime parameter usually wants owned data or a key instead. Prefer keys or indices over Rc<RefCell<T>> when a data structure needs to reference itself. Iterators are lazy, so chaining map and filter allocates nothing until you collect. See [[gardening/017-attracting-pollinators]]. See [[rust/012-error-enums-with-thiserror]].

Lifetimes are just names for how long a borrow is valid; most can be elided. Send means a type can move across threads; Sync means it can be shared by reference. A newtype wraps a primitive to give it a distinct type and a place to hang invariants. Deriving Debug on public data types costs nothing and pays off in every log line. A trait object erases the concrete type behind a vtable for runtime polymorphism. See [[rust/102-slices-over-vecs]]. See [[rust/092-interior-mutability]].

## Lifetimes Explained

Iterators are lazy, so chaining map and filter allocates nothing until you collect. Ownership gives every value a single clear owner, and the borrow checker enforces it at compile time. Send means a type can move across threads; Sync means it can be shared by reference. Accept &str and &[T] in signatures; return owned types and let the caller borrow. thiserror generates Display and From impls, so a library's error enum stays declarative. See [[rust/182-send-and-sync]]. See [[coffee/048-the-espresso-shot]].

thiserror generates Display and From impls, so a library's error enum stays declarative. A newtype wraps a primitive to give it a distinct type and a place to hang invariants. Prefer keys or indices over Rc<RefCell<T>> when a data structure needs to reference itself. Avoid unwrap in production paths and degrade gracefully with match, if let, or the question mark. Deriving Debug on public data types costs nothing and pays off in every log line. Accept &str and &[T] in signatures; return owned types and let the caller borrow. See [[gardening/077-mulching]].

Deriving Debug on public data types costs nothing and pays off in every log line. Iterators are lazy, so chaining map and filter allocates nothing until you collect. Avoid unwrap in production paths and degrade gracefully with match, if let, or the question mark. A struct that sprouts a lifetime parameter usually wants owned data or a key instead. A newtype wraps a primitive to give it a distinct type and a place to hang invariants. A trait object erases the concrete type behind a vtable for runtime polymorphism. See [[rust/032-the-newtype-pattern]]. See [[hiking/129-blister-prevention]].

## Interior Mutability

Prefer keys or indices over Rc<RefCell<T>> when a data structure needs to reference itself. A trait object erases the concrete type behind a vtable for runtime polymorphism. Iterators are lazy, so chaining map and filter allocates nothing until you collect. Avoid unwrap in production paths and degrade gracefully with match, if let, or the question mark. Lifetimes are just names for how long a borrow is valid; most can be elided. See [[rust/032-the-newtype-pattern]]. See [[pkm/093-the-zettelkasten]].

Accept &str and &[T] in signatures; return owned types and let the caller borrow. A trait object erases the concrete type behind a vtable for runtime polymorphism. A struct that sprouts a lifetime parameter usually wants owned data or a key instead. Deriving Debug on public data types costs nothing and pays off in every log line. Lifetimes are just names for how long a borrow is valid; most can be elided. See [[rust/012-error-enums-with-thiserror]].

Lifetimes are just names for how long a borrow is valid; most can be elided. A newtype wraps a primitive to give it a distinct type and a place to hang invariants. Ownership gives every value a single clear owner, and the borrow checker enforces it at compile time. thiserror generates Display and From impls, so a library's error enum stays declarative. A struct that sprouts a lifetime parameter usually wants owned data or a key instead. Iterators are lazy, so chaining map and filter allocates nothing until you collect. See [[rust/182-send-and-sync]]. See [[vector-search/160-the-embedding-space]].
