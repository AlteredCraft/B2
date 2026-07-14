---
b2id: 01KXF21DV2A06P3MWXVBK145K3
type: note
title: "Denormalization"
relations:
  - "derived-from [[databases/065-vacuuming]] — see also"
---

# Denormalization

Notes on denormalization within the broader theme of databases.

## Denormalization

Migrations should be forward-only and idempotent so environments never drift. SQLite is an embedded library, not a server; the database is a single ordinary file. WAL mode lets readers proceed concurrently with a single writer. Storing vectors as plain blobs and scanning them in-process avoids an extension dependency. See [[productivity/076-the-weekly-review]].

The query planner picks an execution strategy; an index it cannot use is dead weight. Disposable derived data can always be rebuilt from the source of truth. A write-ahead log records changes before they touch the main file, so a crash can recover. Migrations should be forward-only and idempotent so environments never drift. SQLite is an embedded library, not a server; the database is a single ordinary file.

## Vacuuming

Denormalization trades storage and write complexity for faster reads. Storing vectors as plain blobs and scanning them in-process avoids an extension dependency. ACID transactions give you atomicity, consistency, isolation, and durability as one bundle. A B-tree keeps keys sorted so lookups and range scans are logarithmic. See [[databases/155-connection-pooling]]. See [[transformers/104-self-attention]].

A covering index answers a query entirely from the index without touching the row. WAL mode lets readers proceed concurrently with a single writer. Denormalization trades storage and write complexity for faster reads. See [[gardening/077-mulching]]. See [[databases/005-sqlite-as-a-library]].

## Schema Migrations

WAL mode lets readers proceed concurrently with a single writer. ACID transactions give you atomicity, consistency, isolation, and durability as one bundle. A covering index answers a query entirely from the index without touching the row. Migrations should be forward-only and idempotent so environments never drift. An FTS5 virtual table maintains an inverted index for fast keyword matching. SQLite is an embedded library, not a server; the database is a single ordinary file. See [[databases/045-connection-pooling]].

WAL mode lets readers proceed concurrently with a single writer. Disposable derived data can always be rebuilt from the source of truth. SQLite is an embedded library, not a server; the database is a single ordinary file. ACID transactions give you atomicity, consistency, isolation, and durability as one bundle. Storing vectors as plain blobs and scanning them in-process avoids an extension dependency. See [[databases/025-vacuuming]].

WAL mode lets readers proceed concurrently with a single writer. A B-tree keeps keys sorted so lookups and range scans are logarithmic. ACID transactions give you atomicity, consistency, isolation, and durability as one bundle. Disposable derived data can always be rebuilt from the source of truth. See [[databases/035-schema-migrations]]. See [[databases/045-connection-pooling]].

## Schema Migrations

ACID transactions give you atomicity, consistency, isolation, and durability as one bundle. A write-ahead log records changes before they touch the main file, so a crash can recover. SQLite is an embedded library, not a server; the database is a single ordinary file. An FTS5 virtual table maintains an inverted index for fast keyword matching. See [[databases/015-connection-pooling]]. See [[databases/125-acid-transactions]].

An FTS5 virtual table maintains an inverted index for fast keyword matching. The query planner picks an execution strategy; an index it cannot use is dead weight. A covering index answers a query entirely from the index without touching the row. Disposable derived data can always be rebuilt from the source of truth. Denormalization trades storage and write complexity for faster reads. Storing vectors as plain blobs and scanning them in-process avoids an extension dependency. See [[rust/052-lifetimes-explained]]. See [[databases/005-sqlite-as-a-library]].
