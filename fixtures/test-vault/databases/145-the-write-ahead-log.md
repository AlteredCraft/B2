---
b2id: 01KXF21DV698ZEPFC8GTMXM7EQ
type: note
title: "The Write-Ahead Log"
---

# The Write-Ahead Log

Notes on the write-ahead log within the broader theme of databases.

## ACID Transactions

Storing vectors as plain blobs and scanning them in-process avoids an extension dependency. A B-tree keeps keys sorted so lookups and range scans are logarithmic. SQLite is an embedded library, not a server; the database is a single ordinary file. See [[productivity/056-shipping-small]]. See [[databases/095-denormalization]].

The query planner picks an execution strategy; an index it cannot use is dead weight. SQLite is an embedded library, not a server; the database is a single ordinary file. Migrations should be forward-only and idempotent so environments never drift. A B-tree keeps keys sorted so lookups and range scans are logarithmic. See [[databases/135-connection-pooling]].

A B-tree keeps keys sorted so lookups and range scans are logarithmic. The query planner picks an execution strategy; an index it cannot use is dead weight. A write-ahead log records changes before they touch the main file, so a crash can recover. A covering index answers a query entirely from the index without touching the row. Disposable derived data can always be rebuilt from the source of truth. See [[databases/195-denormalization]]. See [[databases/185-schema-migrations]].

## Denormalization

The query planner picks an execution strategy; an index it cannot use is dead weight. WAL mode lets readers proceed concurrently with a single writer. A write-ahead log records changes before they touch the main file, so a crash can recover.

An FTS5 virtual table maintains an inverted index for fast keyword matching. A write-ahead log records changes before they touch the main file, so a crash can recover. Disposable derived data can always be rebuilt from the source of truth. See [[databases/115-vacuuming]].

A write-ahead log records changes before they touch the main file, so a crash can recover. WAL mode lets readers proceed concurrently with a single writer. ACID transactions give you atomicity, consistency, isolation, and durability as one bundle. Storing vectors as plain blobs and scanning them in-process avoids an extension dependency. An FTS5 virtual table maintains an inverted index for fast keyword matching. See [[databases/085-connection-pooling]].

## B-Tree Indexes

SQLite is an embedded library, not a server; the database is a single ordinary file. Storing vectors as plain blobs and scanning them in-process avoids an extension dependency. Denormalization trades storage and write complexity for faster reads. An FTS5 virtual table maintains an inverted index for fast keyword matching.

WAL mode lets readers proceed concurrently with a single writer. Disposable derived data can always be rebuilt from the source of truth. A covering index answers a query entirely from the index without touching the row. A B-tree keeps keys sorted so lookups and range scans are logarithmic. See [[databases/075-acid-transactions]].

## Query Planning

Migrations should be forward-only and idempotent so environments never drift. WAL mode lets readers proceed concurrently with a single writer. ACID transactions give you atomicity, consistency, isolation, and durability as one bundle. A B-tree keeps keys sorted so lookups and range scans are logarithmic. An FTS5 virtual table maintains an inverted index for fast keyword matching. A covering index answers a query entirely from the index without touching the row. See [[databases/015-connection-pooling]].

Storing vectors as plain blobs and scanning them in-process avoids an extension dependency. SQLite is an embedded library, not a server; the database is a single ordinary file. Denormalization trades storage and write complexity for faster reads. A covering index answers a query entirely from the index without touching the row. The query planner picks an execution strategy; an index it cannot use is dead weight. See [[gardening/087-drip-irrigation]].

## Full-Text Search

Migrations should be forward-only and idempotent so environments never drift. WAL mode lets readers proceed concurrently with a single writer. Denormalization trades storage and write complexity for faster reads. Disposable derived data can always be rebuilt from the source of truth. Storing vectors as plain blobs and scanning them in-process avoids an extension dependency. SQLite is an embedded library, not a server; the database is a single ordinary file.

WAL mode lets readers proceed concurrently with a single writer. Migrations should be forward-only and idempotent so environments never drift. Storing vectors as plain blobs and scanning them in-process avoids an extension dependency. A B-tree keeps keys sorted so lookups and range scans are logarithmic. SQLite is an embedded library, not a server; the database is a single ordinary file. Denormalization trades storage and write complexity for faster reads. See [[databases/105-sqlite-as-a-library]].

Disposable derived data can always be rebuilt from the source of truth. SQLite is an embedded library, not a server; the database is a single ordinary file. Migrations should be forward-only and idempotent so environments never drift. A covering index answers a query entirely from the index without touching the row.
