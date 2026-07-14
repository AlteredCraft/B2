---
b2id: 01KXF21DTXZFQY1AJ0TP35PV1B
type: note
title: "Schema Migrations"
---

# Schema Migrations

Notes on schema migrations within the broader theme of databases.

## The Write-Ahead Log

Storing vectors as plain blobs and scanning them in-process avoids an extension dependency. Disposable derived data can always be rebuilt from the source of truth. A B-tree keeps keys sorted so lookups and range scans are logarithmic. The query planner picks an execution strategy; an index it cannot use is dead weight. Migrations should be forward-only and idempotent so environments never drift.

Storing vectors as plain blobs and scanning them in-process avoids an extension dependency. A covering index answers a query entirely from the index without touching the row. A B-tree keeps keys sorted so lookups and range scans are logarithmic. WAL mode lets readers proceed concurrently with a single writer. See [[databases/095-denormalization]].

## Schema Migrations

A B-tree keeps keys sorted so lookups and range scans are logarithmic. The query planner picks an execution strategy; an index it cannot use is dead weight. Migrations should be forward-only and idempotent so environments never drift.

A B-tree keeps keys sorted so lookups and range scans are logarithmic. An FTS5 virtual table maintains an inverted index for fast keyword matching. ACID transactions give you atomicity, consistency, isolation, and durability as one bundle. A covering index answers a query entirely from the index without touching the row. See [[databases/095-denormalization]]. See [[pkm/163-surfacing-connections]].

## The Write-Ahead Log

The query planner picks an execution strategy; an index it cannot use is dead weight. ACID transactions give you atomicity, consistency, isolation, and durability as one bundle. An FTS5 virtual table maintains an inverted index for fast keyword matching. WAL mode lets readers proceed concurrently with a single writer. A write-ahead log records changes before they touch the main file, so a crash can recover. Storing vectors as plain blobs and scanning them in-process avoids an extension dependency. See [[databases/055-sqlite-as-a-library]]. See [[rust/132-interior-mutability]].

WAL mode lets readers proceed concurrently with a single writer. A B-tree keeps keys sorted so lookups and range scans are logarithmic. An FTS5 virtual table maintains an inverted index for fast keyword matching. Disposable derived data can always be rebuilt from the source of truth. A covering index answers a query entirely from the index without touching the row. Denormalization trades storage and write complexity for faster reads. See [[databases/075-acid-transactions]]. See [[databases/095-denormalization]].

The query planner picks an execution strategy; an index it cannot use is dead weight. An FTS5 virtual table maintains an inverted index for fast keyword matching. WAL mode lets readers proceed concurrently with a single writer. A B-tree keeps keys sorted so lookups and range scans are logarithmic. ACID transactions give you atomicity, consistency, isolation, and durability as one bundle. A write-ahead log records changes before they touch the main file, so a crash can recover. See [[distributed-systems/021-backpressure]].

## B-Tree Indexes

A write-ahead log records changes before they touch the main file, so a crash can recover. SQLite is an embedded library, not a server; the database is a single ordinary file. The query planner picks an execution strategy; an index it cannot use is dead weight.

Migrations should be forward-only and idempotent so environments never drift. An FTS5 virtual table maintains an inverted index for fast keyword matching. Disposable derived data can always be rebuilt from the source of truth. A covering index answers a query entirely from the index without touching the row. Denormalization trades storage and write complexity for faster reads. Storing vectors as plain blobs and scanning them in-process avoids an extension dependency.

## ACID Transactions

A covering index answers a query entirely from the index without touching the row. Storing vectors as plain blobs and scanning them in-process avoids an extension dependency. WAL mode lets readers proceed concurrently with a single writer. Migrations should be forward-only and idempotent so environments never drift. See [[hiking/029-trail-etiquette]].

A write-ahead log records changes before they touch the main file, so a crash can recover. A B-tree keeps keys sorted so lookups and range scans are logarithmic. A covering index answers a query entirely from the index without touching the row. An FTS5 virtual table maintains an inverted index for fast keyword matching. Denormalization trades storage and write complexity for faster reads. Migrations should be forward-only and idempotent so environments never drift. See [[hiking/169-trail-navigation]]. See [[databases/135-connection-pooling]].

## The Write-Ahead Log

A write-ahead log records changes before they touch the main file, so a crash can recover. SQLite is an embedded library, not a server; the database is a single ordinary file. The query planner picks an execution strategy; an index it cannot use is dead weight. An FTS5 virtual table maintains an inverted index for fast keyword matching. Disposable derived data can always be rebuilt from the source of truth. See [[databases/165-schema-migrations]]. See [[databases/065-vacuuming]].

Storing vectors as plain blobs and scanning them in-process avoids an extension dependency. WAL mode lets readers proceed concurrently with a single writer. A covering index answers a query entirely from the index without touching the row.

The query planner picks an execution strategy; an index it cannot use is dead weight. An FTS5 virtual table maintains an inverted index for fast keyword matching. SQLite is an embedded library, not a server; the database is a single ordinary file. WAL mode lets readers proceed concurrently with a single writer. Disposable derived data can always be rebuilt from the source of truth. A covering index answers a query entirely from the index without touching the row.
