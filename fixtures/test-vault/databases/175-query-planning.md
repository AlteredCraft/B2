---
b2id: 01KXF21DV9Q30V41CJ2PA0Q3CB
type: note
title: "Query Planning"
---

# Query Planning

Notes on query planning within the broader theme of databases.

## Vacuuming

A write-ahead log records changes before they touch the main file, so a crash can recover. ACID transactions give you atomicity, consistency, isolation, and durability as one bundle. Denormalization trades storage and write complexity for faster reads. Disposable derived data can always be rebuilt from the source of truth. An FTS5 virtual table maintains an inverted index for fast keyword matching. Storing vectors as plain blobs and scanning them in-process avoids an extension dependency. See [[coffee/148-single-origin-beans]]. See [[databases/195-denormalization]].

Migrations should be forward-only and idempotent so environments never drift. A covering index answers a query entirely from the index without touching the row. The query planner picks an execution strategy; an index it cannot use is dead weight. A B-tree keeps keys sorted so lookups and range scans are logarithmic. An FTS5 virtual table maintains an inverted index for fast keyword matching. Denormalization trades storage and write complexity for faster reads. See [[coffee/058-brew-ratio]].

SQLite is an embedded library, not a server; the database is a single ordinary file. ACID transactions give you atomicity, consistency, isolation, and durability as one bundle. An FTS5 virtual table maintains an inverted index for fast keyword matching. See [[databases/135-connection-pooling]].

## ACID Transactions

ACID transactions give you atomicity, consistency, isolation, and durability as one bundle. Storing vectors as plain blobs and scanning them in-process avoids an extension dependency. Disposable derived data can always be rebuilt from the source of truth. A write-ahead log records changes before they touch the main file, so a crash can recover. Denormalization trades storage and write complexity for faster reads. A covering index answers a query entirely from the index without touching the row. See [[databases/005-sqlite-as-a-library]]. See [[databases/085-connection-pooling]].

Storing vectors as plain blobs and scanning them in-process avoids an extension dependency. Denormalization trades storage and write complexity for faster reads. Migrations should be forward-only and idempotent so environments never drift. WAL mode lets readers proceed concurrently with a single writer. An FTS5 virtual table maintains an inverted index for fast keyword matching. SQLite is an embedded library, not a server; the database is a single ordinary file. See [[hiking/149-leave-no-trace]]. See [[vector-search/060-cosine-similarity]].

Storing vectors as plain blobs and scanning them in-process avoids an extension dependency. Disposable derived data can always be rebuilt from the source of truth. Migrations should be forward-only and idempotent so environments never drift. See [[databases/115-vacuuming]].

## Denormalization

The query planner picks an execution strategy; an index it cannot use is dead weight. Storing vectors as plain blobs and scanning them in-process avoids an extension dependency. A B-tree keeps keys sorted so lookups and range scans are logarithmic. A covering index answers a query entirely from the index without touching the row. See [[databases/095-denormalization]].

A B-tree keeps keys sorted so lookups and range scans are logarithmic. A write-ahead log records changes before they touch the main file, so a crash can recover. The query planner picks an execution strategy; an index it cannot use is dead weight. Migrations should be forward-only and idempotent so environments never drift. WAL mode lets readers proceed concurrently with a single writer. See [[databases/105-sqlite-as-a-library]].

## Full-Text Search

SQLite is an embedded library, not a server; the database is a single ordinary file. A write-ahead log records changes before they touch the main file, so a crash can recover. Denormalization trades storage and write complexity for faster reads. Disposable derived data can always be rebuilt from the source of truth. The query planner picks an execution strategy; an index it cannot use is dead weight. See [[databases/025-vacuuming]]. See [[pkm/183-linking-your-thinking]].

A covering index answers a query entirely from the index without touching the row. Disposable derived data can always be rebuilt from the source of truth. A write-ahead log records changes before they touch the main file, so a crash can recover. An FTS5 virtual table maintains an inverted index for fast keyword matching. See [[databases/085-connection-pooling]].

Denormalization trades storage and write complexity for faster reads. A B-tree keeps keys sorted so lookups and range scans are logarithmic. WAL mode lets readers proceed concurrently with a single writer. See [[pkm/043-spaced-repetition]]. See [[databases/105-sqlite-as-a-library]].
