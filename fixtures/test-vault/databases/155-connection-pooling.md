---
b2id: 01KXF21DV7HSCKGQ3GA3DK4YK2
type: note
title: "Connection Pooling"
---

# Connection Pooling

Notes on connection pooling within the broader theme of databases.

## Connection Pooling

WAL mode lets readers proceed concurrently with a single writer. ACID transactions give you atomicity, consistency, isolation, and durability as one bundle. The query planner picks an execution strategy; an index it cannot use is dead weight. A write-ahead log records changes before they touch the main file, so a crash can recover.

SQLite is an embedded library, not a server; the database is a single ordinary file. The query planner picks an execution strategy; an index it cannot use is dead weight. A covering index answers a query entirely from the index without touching the row. Disposable derived data can always be rebuilt from the source of truth. Denormalization trades storage and write complexity for faster reads.

ACID transactions give you atomicity, consistency, isolation, and durability as one bundle. A covering index answers a query entirely from the index without touching the row. Denormalization trades storage and write complexity for faster reads. Disposable derived data can always be rebuilt from the source of truth. WAL mode lets readers proceed concurrently with a single writer. The query planner picks an execution strategy; an index it cannot use is dead weight. See [[rust/052-lifetimes-explained]].

## Schema Migrations

A write-ahead log records changes before they touch the main file, so a crash can recover. SQLite is an embedded library, not a server; the database is a single ordinary file. An FTS5 virtual table maintains an inverted index for fast keyword matching. A covering index answers a query entirely from the index without touching the row. Denormalization trades storage and write complexity for faster reads. See [[databases/075-acid-transactions]]. See [[databases/035-schema-migrations]].

Denormalization trades storage and write complexity for faster reads. ACID transactions give you atomicity, consistency, isolation, and durability as one bundle. The query planner picks an execution strategy; an index it cannot use is dead weight. Storing vectors as plain blobs and scanning them in-process avoids an extension dependency.

WAL mode lets readers proceed concurrently with a single writer. The query planner picks an execution strategy; an index it cannot use is dead weight. Denormalization trades storage and write complexity for faster reads. Migrations should be forward-only and idempotent so environments never drift.

## Query Planning

A write-ahead log records changes before they touch the main file, so a crash can recover. A B-tree keeps keys sorted so lookups and range scans are logarithmic. WAL mode lets readers proceed concurrently with a single writer. ACID transactions give you atomicity, consistency, isolation, and durability as one bundle. Storing vectors as plain blobs and scanning them in-process avoids an extension dependency. A covering index answers a query entirely from the index without touching the row. See [[databases/035-schema-migrations]]. See [[vector-search/090-chunking-strategy]].

SQLite is an embedded library, not a server; the database is a single ordinary file. ACID transactions give you atomicity, consistency, isolation, and durability as one bundle. The query planner picks an execution strategy; an index it cannot use is dead weight. Disposable derived data can always be rebuilt from the source of truth. Denormalization trades storage and write complexity for faster reads. Migrations should be forward-only and idempotent so environments never drift. See [[databases/125-acid-transactions]].

## SQLite as a Library

Disposable derived data can always be rebuilt from the source of truth. A covering index answers a query entirely from the index without touching the row. A write-ahead log records changes before they touch the main file, so a crash can recover. ACID transactions give you atomicity, consistency, isolation, and durability as one bundle. Migrations should be forward-only and idempotent so environments never drift. See [[databases/075-acid-transactions]].

ACID transactions give you atomicity, consistency, isolation, and durability as one bundle. The query planner picks an execution strategy; an index it cannot use is dead weight. A B-tree keeps keys sorted so lookups and range scans are logarithmic. Denormalization trades storage and write complexity for faster reads. WAL mode lets readers proceed concurrently with a single writer. Disposable derived data can always be rebuilt from the source of truth.

Migrations should be forward-only and idempotent so environments never drift. An FTS5 virtual table maintains an inverted index for fast keyword matching. A B-tree keeps keys sorted so lookups and range scans are logarithmic. See [[databases/115-vacuuming]].

## Connection Pooling

Storing vectors as plain blobs and scanning them in-process avoids an extension dependency. An FTS5 virtual table maintains an inverted index for fast keyword matching. A B-tree keeps keys sorted so lookups and range scans are logarithmic. WAL mode lets readers proceed concurrently with a single writer. A write-ahead log records changes before they touch the main file, so a crash can recover. Denormalization trades storage and write complexity for faster reads. See [[coffee/138-the-espresso-shot]]. See [[coffee/108-extraction-yield]].

A write-ahead log records changes before they touch the main file, so a crash can recover. WAL mode lets readers proceed concurrently with a single writer. SQLite is an embedded library, not a server; the database is a single ordinary file. The query planner picks an execution strategy; an index it cannot use is dead weight. A B-tree keeps keys sorted so lookups and range scans are logarithmic.
