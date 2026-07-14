---
b2id: 01KXF21DV3AS98T3FKD0N0GHZE
type: note
title: "SQLite as a Library"
---

# SQLite as a Library

Notes on sqlite as a library within the broader theme of databases.

## SQLite as a Library

An FTS5 virtual table maintains an inverted index for fast keyword matching. WAL mode lets readers proceed concurrently with a single writer. Migrations should be forward-only and idempotent so environments never drift. See [[databases/075-acid-transactions]]. See [[databases/065-vacuuming]].

ACID transactions give you atomicity, consistency, isolation, and durability as one bundle. Disposable derived data can always be rebuilt from the source of truth. Denormalization trades storage and write complexity for faster reads. A B-tree keeps keys sorted so lookups and range scans are logarithmic. WAL mode lets readers proceed concurrently with a single writer. A covering index answers a query entirely from the index without touching the row. See [[databases/135-connection-pooling]].

## Vacuuming

Storing vectors as plain blobs and scanning them in-process avoids an extension dependency. Denormalization trades storage and write complexity for faster reads. ACID transactions give you atomicity, consistency, isolation, and durability as one bundle. An FTS5 virtual table maintains an inverted index for fast keyword matching. See [[databases/145-the-write-ahead-log]]. See [[rust/162-the-newtype-pattern]].

SQLite is an embedded library, not a server; the database is a single ordinary file. WAL mode lets readers proceed concurrently with a single writer. Denormalization trades storage and write complexity for faster reads. The query planner picks an execution strategy; an index it cannot use is dead weight. See [[databases/075-acid-transactions]].

## Query Planning

Migrations should be forward-only and idempotent so environments never drift. An FTS5 virtual table maintains an inverted index for fast keyword matching. SQLite is an embedded library, not a server; the database is a single ordinary file. A B-tree keeps keys sorted so lookups and range scans are logarithmic. See [[databases/045-connection-pooling]].

ACID transactions give you atomicity, consistency, isolation, and durability as one bundle. Denormalization trades storage and write complexity for faster reads. Migrations should be forward-only and idempotent so environments never drift. See [[databases/155-connection-pooling]].

## Schema Migrations

Migrations should be forward-only and idempotent so environments never drift. SQLite is an embedded library, not a server; the database is a single ordinary file. An FTS5 virtual table maintains an inverted index for fast keyword matching. Denormalization trades storage and write complexity for faster reads. See [[databases/185-schema-migrations]]. See [[databases/155-connection-pooling]].

A B-tree keeps keys sorted so lookups and range scans are logarithmic. Disposable derived data can always be rebuilt from the source of truth. ACID transactions give you atomicity, consistency, isolation, and durability as one bundle.
