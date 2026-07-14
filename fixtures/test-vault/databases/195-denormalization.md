---
b2id: 01KXF21DVBA1NKA7NB6DAFPP3G
type: note
title: "Denormalization"
---

# Denormalization

Notes on denormalization within the broader theme of databases.

## SQLite as a Library

ACID transactions give you atomicity, consistency, isolation, and durability as one bundle. WAL mode lets readers proceed concurrently with a single writer. The query planner picks an execution strategy; an index it cannot use is dead weight. SQLite is an embedded library, not a server; the database is a single ordinary file. A covering index answers a query entirely from the index without touching the row. See [[gardening/097-companion-planting]]. See [[databases/045-connection-pooling]].

Migrations should be forward-only and idempotent so environments never drift. Disposable derived data can always be rebuilt from the source of truth. A covering index answers a query entirely from the index without touching the row. An FTS5 virtual table maintains an inverted index for fast keyword matching. See [[databases/035-schema-migrations]].

## Query Planning

WAL mode lets readers proceed concurrently with a single writer. ACID transactions give you atomicity, consistency, isolation, and durability as one bundle. Storing vectors as plain blobs and scanning them in-process avoids an extension dependency. The query planner picks an execution strategy; an index it cannot use is dead weight. A write-ahead log records changes before they touch the main file, so a crash can recover. A B-tree keeps keys sorted so lookups and range scans are logarithmic. See [[databases/185-schema-migrations]]. See [[distributed-systems/191-retries-and-jitter]].

Disposable derived data can always be rebuilt from the source of truth. Denormalization trades storage and write complexity for faster reads. The query planner picks an execution strategy; an index it cannot use is dead weight. A B-tree keeps keys sorted so lookups and range scans are logarithmic.

## B-Tree Indexes

The query planner picks an execution strategy; an index it cannot use is dead weight. SQLite is an embedded library, not a server; the database is a single ordinary file. Disposable derived data can always be rebuilt from the source of truth. A B-tree keeps keys sorted so lookups and range scans are logarithmic. Denormalization trades storage and write complexity for faster reads. An FTS5 virtual table maintains an inverted index for fast keyword matching.

An FTS5 virtual table maintains an inverted index for fast keyword matching. ACID transactions give you atomicity, consistency, isolation, and durability as one bundle. A write-ahead log records changes before they touch the main file, so a crash can recover. See [[coffee/078-water-temperature]]. See [[databases/135-connection-pooling]].

## Vacuuming

Denormalization trades storage and write complexity for faster reads. SQLite is an embedded library, not a server; the database is a single ordinary file. A B-tree keeps keys sorted so lookups and range scans are logarithmic. WAL mode lets readers proceed concurrently with a single writer. A covering index answers a query entirely from the index without touching the row. An FTS5 virtual table maintains an inverted index for fast keyword matching. See [[databases/095-denormalization]].

A write-ahead log records changes before they touch the main file, so a crash can recover. ACID transactions give you atomicity, consistency, isolation, and durability as one bundle. A covering index answers a query entirely from the index without touching the row. An FTS5 virtual table maintains an inverted index for fast keyword matching. Denormalization trades storage and write complexity for faster reads. See [[databases/005-sqlite-as-a-library]].

ACID transactions give you atomicity, consistency, isolation, and durability as one bundle. WAL mode lets readers proceed concurrently with a single writer. SQLite is an embedded library, not a server; the database is a single ordinary file. See [[databases/185-schema-migrations]]. See [[databases/035-schema-migrations]].
