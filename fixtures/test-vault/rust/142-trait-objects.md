---
b2id: 01KXF21DYEBS4F2JTP1GM5TQKC
type: note
title: "Trait Objects"
---

# Trait Objects

Notes on trait objects within the broader theme of Rust.

## Lifetimes Explained

Ownership gives every value a single clear owner, and the borrow checker enforces it at compile time. Iterators are lazy, so chaining map and filter allocates nothing until you collect. Avoid unwrap in production paths and degrade gracefully with match, if let, or the question mark. thiserror generates Display and From impls, so a library's error enum stays declarative. A newtype wraps a primitive to give it a distinct type and a place to hang invariants. See [[rust/182-send-and-sync]].

Iterators are lazy, so chaining map and filter allocates nothing until you collect. Send means a type can move across threads; Sync means it can be shared by reference. Prefer keys or indices over Rc<RefCell<T>> when a data structure needs to reference itself. See [[databases/065-vacuuming]]. See [[rust/002-lifetimes-explained]].

## Slices over Vecs

A trait object erases the concrete type behind a vtable for runtime polymorphism. Ownership gives every value a single clear owner, and the borrow checker enforces it at compile time. A newtype wraps a primitive to give it a distinct type and a place to hang invariants. See [[rust/082-iterators-and-laziness]]. See [[rust/192-iterators-and-laziness]].

Ownership gives every value a single clear owner, and the borrow checker enforces it at compile time. A trait object erases the concrete type behind a vtable for runtime polymorphism. Deriving Debug on public data types costs nothing and pays off in every log line. Avoid unwrap in production paths and degrade gracefully with match, if let, or the question mark. Prefer keys or indices over Rc<RefCell<T>> when a data structure needs to reference itself. See [[rust/162-the-newtype-pattern]]. See [[rust/042-interior-mutability]].

Prefer keys or indices over Rc<RefCell<T>> when a data structure needs to reference itself. Ownership gives every value a single clear owner, and the borrow checker enforces it at compile time. Send means a type can move across threads; Sync means it can be shared by reference. Iterators are lazy, so chaining map and filter allocates nothing until you collect. A struct that sprouts a lifetime parameter usually wants owned data or a key instead. thiserror generates Display and From impls, so a library's error enum stays declarative. See [[rust/052-lifetimes-explained]].

## Iterators and Laziness

Lifetimes are just names for how long a borrow is valid; most can be elided. Accept &str and &[T] in signatures; return owned types and let the caller borrow. A newtype wraps a primitive to give it a distinct type and a place to hang invariants. Iterators are lazy, so chaining map and filter allocates nothing until you collect. thiserror generates Display and From impls, so a library's error enum stays declarative. See [[coffee/008-extraction-yield]]. See [[rust/182-send-and-sync]].

Ownership gives every value a single clear owner, and the borrow checker enforces it at compile time. Deriving Debug on public data types costs nothing and pays off in every log line. Iterators are lazy, so chaining map and filter allocates nothing until you collect. A struct that sprouts a lifetime parameter usually wants owned data or a key instead. Avoid unwrap in production paths and degrade gracefully with match, if let, or the question mark.

Iterators are lazy, so chaining map and filter allocates nothing until you collect. Lifetimes are just names for how long a borrow is valid; most can be elided. A struct that sprouts a lifetime parameter usually wants owned data or a key instead. A trait object erases the concrete type behind a vtable for runtime polymorphism. Avoid unwrap in production paths and degrade gracefully with match, if let, or the question mark. See [[rust/102-slices-over-vecs]]. See [[rust/032-the-newtype-pattern]].

## Slices over Vecs

Prefer keys or indices over Rc<RefCell<T>> when a data structure needs to reference itself. Lifetimes are just names for how long a borrow is valid; most can be elided. A newtype wraps a primitive to give it a distinct type and a place to hang invariants. Ownership gives every value a single clear owner, and the borrow checker enforces it at compile time. Deriving Debug on public data types costs nothing and pays off in every log line. A trait object erases the concrete type behind a vtable for runtime polymorphism. See [[rust/042-interior-mutability]].

Send means a type can move across threads; Sync means it can be shared by reference. A newtype wraps a primitive to give it a distinct type and a place to hang invariants. A struct that sprouts a lifetime parameter usually wants owned data or a key instead. Avoid unwrap in production paths and degrade gracefully with match, if let, or the question mark.

Deriving Debug on public data types costs nothing and pays off in every log line. Prefer keys or indices over Rc<RefCell<T>> when a data structure needs to reference itself. Lifetimes are just names for how long a borrow is valid; most can be elided. Accept &str and &[T] in signatures; return owned types and let the caller borrow. Iterators are lazy, so chaining map and filter allocates nothing until you collect. A trait object erases the concrete type behind a vtable for runtime polymorphism.

## Error Enums with thiserror

Avoid unwrap in production paths and degrade gracefully with match, if let, or the question mark. thiserror generates Display and From impls, so a library's error enum stays declarative. Lifetimes are just names for how long a borrow is valid; most can be elided. See [[rust/022-lifetimes-explained]]. See [[rust/092-interior-mutability]].

A trait object erases the concrete type behind a vtable for runtime polymorphism. Accept &str and &[T] in signatures; return owned types and let the caller borrow. Deriving Debug on public data types costs nothing and pays off in every log line. Avoid unwrap in production paths and degrade gracefully with match, if let, or the question mark. Lifetimes are just names for how long a borrow is valid; most can be elided. See [[hiking/099-blister-prevention]]. See [[rust/162-the-newtype-pattern]].

Ownership gives every value a single clear owner, and the borrow checker enforces it at compile time. A struct that sprouts a lifetime parameter usually wants owned data or a key instead. thiserror generates Display and From impls, so a library's error enum stays declarative. Accept &str and &[T] in signatures; return owned types and let the caller borrow. Iterators are lazy, so chaining map and filter allocates nothing until you collect. Prefer keys or indices over Rc<RefCell<T>> when a data structure needs to reference itself. See [[rust/162-the-newtype-pattern]]. See [[rust/092-interior-mutability]].
