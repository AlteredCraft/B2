---
b2id: 01KXF21DTYZ41HN2C4SJQXN2TZ
type: note
title: "SQLite as a Library"
---

# SQLite as a Library

Notes on sqlite as a library within the broader theme of databases.

## Query Planning

ACID transactions give you atomicity, consistency, isolation, and durability as one bundle. An FTS5 virtual table maintains an inverted index for fast keyword matching. The query planner picks an execution strategy; an index it cannot use is dead weight. Disposable derived data can always be rebuilt from the source of truth. See [[databases/135-connection-pooling]]. See [[databases/195-denormalization]].

Denormalization trades storage and write complexity for faster reads. SQLite is an embedded library, not a server; the database is a single ordinary file. A covering index answers a query entirely from the index without touching the row. Storing vectors as plain blobs and scanning them in-process avoids an extension dependency.

## Full-Text Search

WAL mode lets readers proceed concurrently with a single writer. Denormalization trades storage and write complexity for faster reads. An FTS5 virtual table maintains an inverted index for fast keyword matching. The query planner picks an execution strategy; an index it cannot use is dead weight. Disposable derived data can always be rebuilt from the source of truth.

A write-ahead log records changes before they touch the main file, so a crash can recover. A covering index answers a query entirely from the index without touching the row. Disposable derived data can always be rebuilt from the source of truth. SQLite is an embedded library, not a server; the database is a single ordinary file. The query planner picks an execution strategy; an index it cannot use is dead weight.

The query planner picks an execution strategy; an index it cannot use is dead weight. Disposable derived data can always be rebuilt from the source of truth. A covering index answers a query entirely from the index without touching the row. A write-ahead log records changes before they touch the main file, so a crash can recover. See [[databases/135-connection-pooling]].

## Denormalization

Denormalization trades storage and write complexity for faster reads. SQLite is an embedded library, not a server; the database is a single ordinary file. Disposable derived data can always be rebuilt from the source of truth. A B-tree keeps keys sorted so lookups and range scans are logarithmic. WAL mode lets readers proceed concurrently with a single writer. See [[databases/105-sqlite-as-a-library]]. See [[databases/175-query-planning]].

Migrations should be forward-only and idempotent so environments never drift. Disposable derived data can always be rebuilt from the source of truth. Denormalization trades storage and write complexity for faster reads. A covering index answers a query entirely from the index without touching the row. A write-ahead log records changes before they touch the main file, so a crash can recover. An FTS5 virtual table maintains an inverted index for fast keyword matching. See [[databases/155-connection-pooling]].

A B-tree keeps keys sorted so lookups and range scans are logarithmic. Denormalization trades storage and write complexity for faster reads. Migrations should be forward-only and idempotent so environments never drift. The query planner picks an execution strategy; an index it cannot use is dead weight.
