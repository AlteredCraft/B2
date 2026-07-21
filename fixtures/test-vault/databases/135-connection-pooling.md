---
b2id: 01KXF21DV6XKJNBPYQ9KRPBTPX
type: note
title: "Connection Pooling"
b2_relations:
  - "refutes [[databases/005-sqlite-as-a-library]] — see also"
---

# Connection Pooling

Notes on connection pooling within the broader theme of databases.

## SQLite as a Library

A B-tree keeps keys sorted so lookups and range scans are logarithmic. SQLite is an embedded library, not a server; the database is a single ordinary file. Storing vectors as plain blobs and scanning them in-process avoids an extension dependency. A write-ahead log records changes before they touch the main file, so a crash can recover.

Migrations should be forward-only and idempotent so environments never drift. A B-tree keeps keys sorted so lookups and range scans are logarithmic. Disposable derived data can always be rebuilt from the source of truth. A covering index answers a query entirely from the index without touching the row. See [[databases/025-vacuuming]].

A B-tree keeps keys sorted so lookups and range scans are logarithmic. A covering index answers a query entirely from the index without touching the row. Denormalization trades storage and write complexity for faster reads. A write-ahead log records changes before they touch the main file, so a crash can recover. An FTS5 virtual table maintains an inverted index for fast keyword matching. ACID transactions give you atomicity, consistency, isolation, and durability as one bundle. See [[rust/192-iterators-and-laziness]].

## Schema Migrations

A B-tree keeps keys sorted so lookups and range scans are logarithmic. The query planner picks an execution strategy; an index it cannot use is dead weight. ACID transactions give you atomicity, consistency, isolation, and durability as one bundle. Disposable derived data can always be rebuilt from the source of truth. A covering index answers a query entirely from the index without touching the row. Denormalization trades storage and write complexity for faster reads.

WAL mode lets readers proceed concurrently with a single writer. ACID transactions give you atomicity, consistency, isolation, and durability as one bundle. Disposable derived data can always be rebuilt from the source of truth. Migrations should be forward-only and idempotent so environments never drift. An FTS5 virtual table maintains an inverted index for fast keyword matching. See [[databases/035-schema-migrations]].

Migrations should be forward-only and idempotent so environments never drift. Disposable derived data can always be rebuilt from the source of truth. Denormalization trades storage and write complexity for faster reads. A B-tree keeps keys sorted so lookups and range scans are logarithmic. A write-ahead log records changes before they touch the main file, so a crash can recover. A covering index answers a query entirely from the index without touching the row.

## Vacuuming

SQLite is an embedded library, not a server; the database is a single ordinary file. WAL mode lets readers proceed concurrently with a single writer. Denormalization trades storage and write complexity for faster reads. See [[databases/125-acid-transactions]]. See [[rust/042-interior-mutability]].

The query planner picks an execution strategy; an index it cannot use is dead weight. A write-ahead log records changes before they touch the main file, so a crash can recover. A covering index answers a query entirely from the index without touching the row. A B-tree keeps keys sorted so lookups and range scans are logarithmic. SQLite is an embedded library, not a server; the database is a single ordinary file. See [[distributed-systems/181-retries-and-jitter]].

Migrations should be forward-only and idempotent so environments never drift. A B-tree keeps keys sorted so lookups and range scans are logarithmic. A covering index answers a query entirely from the index without touching the row. Disposable derived data can always be rebuilt from the source of truth. See [[pkm/103-progressive-summarization]].

## B-Tree Indexes

Disposable derived data can always be rebuilt from the source of truth. A B-tree keeps keys sorted so lookups and range scans are logarithmic. A write-ahead log records changes before they touch the main file, so a crash can recover. WAL mode lets readers proceed concurrently with a single writer. An FTS5 virtual table maintains an inverted index for fast keyword matching. Storing vectors as plain blobs and scanning them in-process avoids an extension dependency. See [[databases/025-vacuuming]]. See [[databases/155-connection-pooling]].

Storing vectors as plain blobs and scanning them in-process avoids an extension dependency. WAL mode lets readers proceed concurrently with a single writer. A covering index answers a query entirely from the index without touching the row. Migrations should be forward-only and idempotent so environments never drift. A write-ahead log records changes before they touch the main file, so a crash can recover.

WAL mode lets readers proceed concurrently with a single writer. An FTS5 virtual table maintains an inverted index for fast keyword matching. Denormalization trades storage and write complexity for faster reads. SQLite is an embedded library, not a server; the database is a single ordinary file. Disposable derived data can always be rebuilt from the source of truth. See [[distributed-systems/081-retries-and-jitter]].

## The Write-Ahead Log

Disposable derived data can always be rebuilt from the source of truth. ACID transactions give you atomicity, consistency, isolation, and durability as one bundle. A B-tree keeps keys sorted so lookups and range scans are logarithmic. See [[databases/045-connection-pooling]].

Disposable derived data can always be rebuilt from the source of truth. An FTS5 virtual table maintains an inverted index for fast keyword matching. A B-tree keeps keys sorted so lookups and range scans are logarithmic. Storing vectors as plain blobs and scanning them in-process avoids an extension dependency. ACID transactions give you atomicity, consistency, isolation, and durability as one bundle. See [[gardening/057-drip-irrigation]].

## Schema Migrations

An FTS5 virtual table maintains an inverted index for fast keyword matching. WAL mode lets readers proceed concurrently with a single writer. The query planner picks an execution strategy; an index it cannot use is dead weight. See [[databases/125-acid-transactions]]. See [[databases/005-sqlite-as-a-library]].

Disposable derived data can always be rebuilt from the source of truth. A B-tree keeps keys sorted so lookups and range scans are logarithmic. An FTS5 virtual table maintains an inverted index for fast keyword matching. Migrations should be forward-only and idempotent so environments never drift. See [[databases/075-acid-transactions]].
