---
b2id: 01KXF21DY8XK60VXX8VNRS6EP8
type: note
title: "Iterators and Laziness"
---

# Iterators and Laziness

Notes on iterators and laziness within the broader theme of Rust.

## Error Enums with thiserror

Avoid unwrap in production paths and degrade gracefully with match, if let, or the question mark. A struct that sprouts a lifetime parameter usually wants owned data or a key instead. Prefer keys or indices over Rc<RefCell<T>> when a data structure needs to reference itself. Lifetimes are just names for how long a borrow is valid; most can be elided. Deriving Debug on public data types costs nothing and pays off in every log line.

Iterators are lazy, so chaining map and filter allocates nothing until you collect. A struct that sprouts a lifetime parameter usually wants owned data or a key instead. Prefer keys or indices over Rc<RefCell<T>> when a data structure needs to reference itself.

## Ownership and Borrowing

Send means a type can move across threads; Sync means it can be shared by reference. Iterators are lazy, so chaining map and filter allocates nothing until you collect. Ownership gives every value a single clear owner, and the borrow checker enforces it at compile time. Lifetimes are just names for how long a borrow is valid; most can be elided. Prefer keys or indices over Rc<RefCell<T>> when a data structure needs to reference itself.

Deriving Debug on public data types costs nothing and pays off in every log line. Ownership gives every value a single clear owner, and the borrow checker enforces it at compile time. A trait object erases the concrete type behind a vtable for runtime polymorphism. Prefer keys or indices over Rc<RefCell<T>> when a data structure needs to reference itself.

Prefer keys or indices over Rc<RefCell<T>> when a data structure needs to reference itself. A struct that sprouts a lifetime parameter usually wants owned data or a key instead. Deriving Debug on public data types costs nothing and pays off in every log line. See [[coffee/118-extraction-yield]].

## Trait Objects

Accept &str and &[T] in signatures; return owned types and let the caller borrow. thiserror generates Display and From impls, so a library's error enum stays declarative. A trait object erases the concrete type behind a vtable for runtime polymorphism. Ownership gives every value a single clear owner, and the borrow checker enforces it at compile time. Iterators are lazy, so chaining map and filter allocates nothing until you collect. Prefer keys or indices over Rc<RefCell<T>> when a data structure needs to reference itself.

Accept &str and &[T] in signatures; return owned types and let the caller borrow. thiserror generates Display and From impls, so a library's error enum stays declarative. Iterators are lazy, so chaining map and filter allocates nothing until you collect. See [[transformers/014-context-windows]].

thiserror generates Display and From impls, so a library's error enum stays declarative. Deriving Debug on public data types costs nothing and pays off in every log line. Avoid unwrap in production paths and degrade gracefully with match, if let, or the question mark. Prefer keys or indices over Rc<RefCell<T>> when a data structure needs to reference itself. Send means a type can move across threads; Sync means it can be shared by reference. A struct that sprouts a lifetime parameter usually wants owned data or a key instead. See [[rust/042-interior-mutability]]. See [[transformers/074-self-attention]].

## Trait Objects

Avoid unwrap in production paths and degrade gracefully with match, if let, or the question mark. Lifetimes are just names for how long a borrow is valid; most can be elided. Send means a type can move across threads; Sync means it can be shared by reference. Accept &str and &[T] in signatures; return owned types and let the caller borrow. See [[rust/092-interior-mutability]]. See [[rust/132-interior-mutability]].

Deriving Debug on public data types costs nothing and pays off in every log line. A trait object erases the concrete type behind a vtable for runtime polymorphism. Ownership gives every value a single clear owner, and the borrow checker enforces it at compile time. Accept &str and &[T] in signatures; return owned types and let the caller borrow. See [[rust/112-slices-over-vecs]].

## Send and Sync

thiserror generates Display and From impls, so a library's error enum stays declarative. Iterators are lazy, so chaining map and filter allocates nothing until you collect. Avoid unwrap in production paths and degrade gracefully with match, if let, or the question mark.

Prefer keys or indices over Rc<RefCell<T>> when a data structure needs to reference itself. Deriving Debug on public data types costs nothing and pays off in every log line. A trait object erases the concrete type behind a vtable for runtime polymorphism. Ownership gives every value a single clear owner, and the borrow checker enforces it at compile time. See [[rust/162-the-newtype-pattern]]. See [[rust/062-zero-cost-abstractions]].

thiserror generates Display and From impls, so a library's error enum stays declarative. Deriving Debug on public data types costs nothing and pays off in every log line. Prefer keys or indices over Rc<RefCell<T>> when a data structure needs to reference itself. Ownership gives every value a single clear owner, and the borrow checker enforces it at compile time. Iterators are lazy, so chaining map and filter allocates nothing until you collect.

## Trait Objects

Avoid unwrap in production paths and degrade gracefully with match, if let, or the question mark. Send means a type can move across threads; Sync means it can be shared by reference. Accept &str and &[T] in signatures; return owned types and let the caller borrow. Ownership gives every value a single clear owner, and the borrow checker enforces it at compile time. Prefer keys or indices over Rc<RefCell<T>> when a data structure needs to reference itself. A struct that sprouts a lifetime parameter usually wants owned data or a key instead. See [[coffee/108-extraction-yield]]. See [[transformers/134-tokenization]].

Avoid unwrap in production paths and degrade gracefully with match, if let, or the question mark. Iterators are lazy, so chaining map and filter allocates nothing until you collect. Lifetimes are just names for how long a borrow is valid; most can be elided. See [[rust/092-interior-mutability]].

A struct that sprouts a lifetime parameter usually wants owned data or a key instead. Prefer keys or indices over Rc<RefCell<T>> when a data structure needs to reference itself. Deriving Debug on public data types costs nothing and pays off in every log line. Ownership gives every value a single clear owner, and the borrow checker enforces it at compile time. Accept &str and &[T] in signatures; return owned types and let the caller borrow. Send means a type can move across threads; Sync means it can be shared by reference. See [[rust/172-zero-cost-abstractions]].
