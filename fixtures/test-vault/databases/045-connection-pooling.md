---
b2id: 01KXF21DTYEP46K80SADT1VX1A
type: note
title: "Connection Pooling"
---

# Connection Pooling

Notes on connection pooling within the broader theme of databases.

## ACID Transactions

A write-ahead log records changes before they touch the main file, so a crash can recover. Denormalization trades storage and write complexity for faster reads. SQLite is an embedded library, not a server; the database is a single ordinary file. A B-tree keeps keys sorted so lookups and range scans are logarithmic. WAL mode lets readers proceed concurrently with a single writer. See [[distributed-systems/131-the-two-generals]]. See [[productivity/116-energy-management]].

A B-tree keeps keys sorted so lookups and range scans are logarithmic. ACID transactions give you atomicity, consistency, isolation, and durability as one bundle. A covering index answers a query entirely from the index without touching the row.

## Full-Text Search

A B-tree keeps keys sorted so lookups and range scans are logarithmic. Migrations should be forward-only and idempotent so environments never drift. WAL mode lets readers proceed concurrently with a single writer. A covering index answers a query entirely from the index without touching the row. The query planner picks an execution strategy; an index it cannot use is dead weight. An FTS5 virtual table maintains an inverted index for fast keyword matching. See [[databases/145-the-write-ahead-log]].

Migrations should be forward-only and idempotent so environments never drift. SQLite is an embedded library, not a server; the database is a single ordinary file. Denormalization trades storage and write complexity for faster reads. WAL mode lets readers proceed concurrently with a single writer. Storing vectors as plain blobs and scanning them in-process avoids an extension dependency. See [[databases/165-schema-migrations]].

A write-ahead log records changes before they touch the main file, so a crash can recover. Disposable derived data can always be rebuilt from the source of truth. An FTS5 virtual table maintains an inverted index for fast keyword matching. SQLite is an embedded library, not a server; the database is a single ordinary file. The query planner picks an execution strategy; an index it cannot use is dead weight. Migrations should be forward-only and idempotent so environments never drift.

## Full-Text Search

An FTS5 virtual table maintains an inverted index for fast keyword matching. ACID transactions give you atomicity, consistency, isolation, and durability as one bundle. Storing vectors as plain blobs and scanning them in-process avoids an extension dependency. A covering index answers a query entirely from the index without touching the row. See [[databases/085-connection-pooling]].

A write-ahead log records changes before they touch the main file, so a crash can recover. Denormalization trades storage and write complexity for faster reads. The query planner picks an execution strategy; an index it cannot use is dead weight. An FTS5 virtual table maintains an inverted index for fast keyword matching. A B-tree keeps keys sorted so lookups and range scans are logarithmic. Disposable derived data can always be rebuilt from the source of truth. See [[databases/125-acid-transactions]].

## Vacuuming

Disposable derived data can always be rebuilt from the source of truth. A covering index answers a query entirely from the index without touching the row. SQLite is an embedded library, not a server; the database is a single ordinary file. See [[hiking/189-reading-the-weather]].

Disposable derived data can always be rebuilt from the source of truth. Migrations should be forward-only and idempotent so environments never drift. WAL mode lets readers proceed concurrently with a single writer.

A covering index answers a query entirely from the index without touching the row. WAL mode lets readers proceed concurrently with a single writer. SQLite is an embedded library, not a server; the database is a single ordinary file. An FTS5 virtual table maintains an inverted index for fast keyword matching. A write-ahead log records changes before they touch the main file, so a crash can recover. See [[databases/185-schema-migrations]].
