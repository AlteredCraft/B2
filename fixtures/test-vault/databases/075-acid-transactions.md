---
b2id: 01KXF21DV09GB7SS6EQWJJ26T3
type: note
title: "ACID Transactions"
---

# ACID Transactions

Notes on acid transactions within the broader theme of databases.

## ACID Transactions

Storing vectors as plain blobs and scanning them in-process avoids an extension dependency. The query planner picks an execution strategy; an index it cannot use is dead weight. An FTS5 virtual table maintains an inverted index for fast keyword matching. WAL mode lets readers proceed concurrently with a single writer. See [[databases/195-denormalization]].

A B-tree keeps keys sorted so lookups and range scans are logarithmic. SQLite is an embedded library, not a server; the database is a single ordinary file. An FTS5 virtual table maintains an inverted index for fast keyword matching. Disposable derived data can always be rebuilt from the source of truth. ACID transactions give you atomicity, consistency, isolation, and durability as one bundle. See [[distributed-systems/011-leader-election]]. See [[databases/095-denormalization]].

A write-ahead log records changes before they touch the main file, so a crash can recover. An FTS5 virtual table maintains an inverted index for fast keyword matching. A B-tree keeps keys sorted so lookups and range scans are logarithmic. See [[gardening/067-overwintering]].

## Schema Migrations

A covering index answers a query entirely from the index without touching the row. WAL mode lets readers proceed concurrently with a single writer. A write-ahead log records changes before they touch the main file, so a crash can recover. Migrations should be forward-only and idempotent so environments never drift. Denormalization trades storage and write complexity for faster reads. See [[databases/165-schema-migrations]]. See [[databases/145-the-write-ahead-log]].

The query planner picks an execution strategy; an index it cannot use is dead weight. Disposable derived data can always be rebuilt from the source of truth. ACID transactions give you atomicity, consistency, isolation, and durability as one bundle. Storing vectors as plain blobs and scanning them in-process avoids an extension dependency. An FTS5 virtual table maintains an inverted index for fast keyword matching. A B-tree keeps keys sorted so lookups and range scans are logarithmic.

The query planner picks an execution strategy; an index it cannot use is dead weight. SQLite is an embedded library, not a server; the database is a single ordinary file. Storing vectors as plain blobs and scanning them in-process avoids an extension dependency. A covering index answers a query entirely from the index without touching the row. A write-ahead log records changes before they touch the main file, so a crash can recover. See [[databases/155-connection-pooling]].

## SQLite as a Library

ACID transactions give you atomicity, consistency, isolation, and durability as one bundle. SQLite is an embedded library, not a server; the database is a single ordinary file. Storing vectors as plain blobs and scanning them in-process avoids an extension dependency. See [[transformers/004-cls-pooling]]. See [[databases/095-denormalization]].

Denormalization trades storage and write complexity for faster reads. The query planner picks an execution strategy; an index it cannot use is dead weight. A B-tree keeps keys sorted so lookups and range scans are logarithmic. ACID transactions give you atomicity, consistency, isolation, and durability as one bundle. See [[databases/145-the-write-ahead-log]].

SQLite is an embedded library, not a server; the database is a single ordinary file. A covering index answers a query entirely from the index without touching the row. A B-tree keeps keys sorted so lookups and range scans are logarithmic. Disposable derived data can always be rebuilt from the source of truth. WAL mode lets readers proceed concurrently with a single writer. See [[databases/035-schema-migrations]]. See [[databases/095-denormalization]].
