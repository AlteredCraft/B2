---
b2id: 01KXF21DV5BWYCG51HWSM3STTZ
type: note
title: "ACID Transactions"
---

# ACID Transactions

Notes on acid transactions within the broader theme of databases.

## SQLite as a Library

A write-ahead log records changes before they touch the main file, so a crash can recover. An FTS5 virtual table maintains an inverted index for fast keyword matching. WAL mode lets readers proceed concurrently with a single writer. ACID transactions give you atomicity, consistency, isolation, and durability as one bundle.

WAL mode lets readers proceed concurrently with a single writer. Disposable derived data can always be rebuilt from the source of truth. Storing vectors as plain blobs and scanning them in-process avoids an extension dependency. ACID transactions give you atomicity, consistency, isolation, and durability as one bundle. The query planner picks an execution strategy; an index it cannot use is dead weight.

Denormalization trades storage and write complexity for faster reads. Migrations should be forward-only and idempotent so environments never drift. WAL mode lets readers proceed concurrently with a single writer. A B-tree keeps keys sorted so lookups and range scans are logarithmic. See [[databases/075-acid-transactions]].

## B-Tree Indexes

A write-ahead log records changes before they touch the main file, so a crash can recover. Migrations should be forward-only and idempotent so environments never drift. SQLite is an embedded library, not a server; the database is a single ordinary file. Denormalization trades storage and write complexity for faster reads. See [[vector-search/060-cosine-similarity]].

An FTS5 virtual table maintains an inverted index for fast keyword matching. The query planner picks an execution strategy; an index it cannot use is dead weight. Denormalization trades storage and write complexity for faster reads. A write-ahead log records changes before they touch the main file, so a crash can recover. Migrations should be forward-only and idempotent so environments never drift. A B-tree keeps keys sorted so lookups and range scans are logarithmic.

## B-Tree Indexes

ACID transactions give you atomicity, consistency, isolation, and durability as one bundle. A covering index answers a query entirely from the index without touching the row. An FTS5 virtual table maintains an inverted index for fast keyword matching.

The query planner picks an execution strategy; an index it cannot use is dead weight. SQLite is an embedded library, not a server; the database is a single ordinary file. A covering index answers a query entirely from the index without touching the row.

## Query Planning

SQLite is an embedded library, not a server; the database is a single ordinary file. A B-tree keeps keys sorted so lookups and range scans are logarithmic. Storing vectors as plain blobs and scanning them in-process avoids an extension dependency. See [[databases/095-denormalization]].

SQLite is an embedded library, not a server; the database is a single ordinary file. Storing vectors as plain blobs and scanning them in-process avoids an extension dependency. ACID transactions give you atomicity, consistency, isolation, and durability as one bundle.

An FTS5 virtual table maintains an inverted index for fast keyword matching. The query planner picks an execution strategy; an index it cannot use is dead weight. Disposable derived data can always be rebuilt from the source of truth. A write-ahead log records changes before they touch the main file, so a crash can recover. A covering index answers a query entirely from the index without touching the row. SQLite is an embedded library, not a server; the database is a single ordinary file.
