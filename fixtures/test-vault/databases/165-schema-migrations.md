---
b2id: 01KXF21DV8WPKNPMDZ569FGRVS
type: note
title: "Schema Migrations"
---

# Schema Migrations

Notes on schema migrations within the broader theme of databases.

## SQLite as a Library

A write-ahead log records changes before they touch the main file, so a crash can recover. WAL mode lets readers proceed concurrently with a single writer. An FTS5 virtual table maintains an inverted index for fast keyword matching. Migrations should be forward-only and idempotent so environments never drift. A B-tree keeps keys sorted so lookups and range scans are logarithmic. See [[databases/145-the-write-ahead-log]].

Disposable derived data can always be rebuilt from the source of truth. WAL mode lets readers proceed concurrently with a single writer. A write-ahead log records changes before they touch the main file, so a crash can recover.

ACID transactions give you atomicity, consistency, isolation, and durability as one bundle. Migrations should be forward-only and idempotent so environments never drift. A covering index answers a query entirely from the index without touching the row. WAL mode lets readers proceed concurrently with a single writer. See [[databases/155-connection-pooling]].

## ACID Transactions

A covering index answers a query entirely from the index without touching the row. Storing vectors as plain blobs and scanning them in-process avoids an extension dependency. Migrations should be forward-only and idempotent so environments never drift. The query planner picks an execution strategy; an index it cannot use is dead weight. See [[distributed-systems/131-the-two-generals]].

A write-ahead log records changes before they touch the main file, so a crash can recover. Migrations should be forward-only and idempotent so environments never drift. Storing vectors as plain blobs and scanning them in-process avoids an extension dependency. SQLite is an embedded library, not a server; the database is a single ordinary file. The query planner picks an execution strategy; an index it cannot use is dead weight. Denormalization trades storage and write complexity for faster reads. See [[pkm/003-local-first-vaults]]. See [[databases/025-vacuuming]].

## The Write-Ahead Log

Disposable derived data can always be rebuilt from the source of truth. ACID transactions give you atomicity, consistency, isolation, and durability as one bundle. Storing vectors as plain blobs and scanning them in-process avoids an extension dependency. SQLite is an embedded library, not a server; the database is a single ordinary file. A B-tree keeps keys sorted so lookups and range scans are logarithmic. See [[databases/075-acid-transactions]].

ACID transactions give you atomicity, consistency, isolation, and durability as one bundle. A write-ahead log records changes before they touch the main file, so a crash can recover. An FTS5 virtual table maintains an inverted index for fast keyword matching. WAL mode lets readers proceed concurrently with a single writer. Storing vectors as plain blobs and scanning them in-process avoids an extension dependency.

## Schema Migrations

Migrations should be forward-only and idempotent so environments never drift. The query planner picks an execution strategy; an index it cannot use is dead weight. SQLite is an embedded library, not a server; the database is a single ordinary file. See [[databases/105-sqlite-as-a-library]]. See [[databases/065-vacuuming]].

SQLite is an embedded library, not a server; the database is a single ordinary file. An FTS5 virtual table maintains an inverted index for fast keyword matching. WAL mode lets readers proceed concurrently with a single writer. A B-tree keeps keys sorted so lookups and range scans are logarithmic. A covering index answers a query entirely from the index without touching the row. Denormalization trades storage and write complexity for faster reads. See [[distributed-systems/181-retries-and-jitter]]. See [[databases/095-denormalization]].
