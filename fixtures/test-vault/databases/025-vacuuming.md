---
b2id: 01KXF21DTWDZMDWMT124D5V0BY
type: note
title: "Vacuuming"
---

# Vacuuming

Notes on vacuuming within the broader theme of databases.

## The Write-Ahead Log

The query planner picks an execution strategy; an index it cannot use is dead weight. Storing vectors as plain blobs and scanning them in-process avoids an extension dependency. ACID transactions give you atomicity, consistency, isolation, and durability as one bundle. See [[databases/095-denormalization]]. See [[databases/185-schema-migrations]].

Storing vectors as plain blobs and scanning them in-process avoids an extension dependency. Migrations should be forward-only and idempotent so environments never drift. A B-tree keeps keys sorted so lookups and range scans are logarithmic. SQLite is an embedded library, not a server; the database is a single ordinary file. Denormalization trades storage and write complexity for faster reads. A covering index answers a query entirely from the index without touching the row.

The query planner picks an execution strategy; an index it cannot use is dead weight. An FTS5 virtual table maintains an inverted index for fast keyword matching. Disposable derived data can always be rebuilt from the source of truth. Storing vectors as plain blobs and scanning them in-process avoids an extension dependency. Migrations should be forward-only and idempotent so environments never drift.

## Schema Migrations

A B-tree keeps keys sorted so lookups and range scans are logarithmic. Denormalization trades storage and write complexity for faster reads. WAL mode lets readers proceed concurrently with a single writer. SQLite is an embedded library, not a server; the database is a single ordinary file. See [[transformers/094-fine-tuning-vs-prompting]].

SQLite is an embedded library, not a server; the database is a single ordinary file. Migrations should be forward-only and idempotent so environments never drift. A write-ahead log records changes before they touch the main file, so a crash can recover. A covering index answers a query entirely from the index without touching the row. Storing vectors as plain blobs and scanning them in-process avoids an extension dependency. See [[databases/085-connection-pooling]].

## B-Tree Indexes

The query planner picks an execution strategy; an index it cannot use is dead weight. Disposable derived data can always be rebuilt from the source of truth. A B-tree keeps keys sorted so lookups and range scans are logarithmic. An FTS5 virtual table maintains an inverted index for fast keyword matching. Storing vectors as plain blobs and scanning them in-process avoids an extension dependency. See [[databases/045-connection-pooling]].

Storing vectors as plain blobs and scanning them in-process avoids an extension dependency. A covering index answers a query entirely from the index without touching the row. A B-tree keeps keys sorted so lookups and range scans are logarithmic. Disposable derived data can always be rebuilt from the source of truth. The query planner picks an execution strategy; an index it cannot use is dead weight. Denormalization trades storage and write complexity for faster reads. See [[productivity/056-shipping-small]]. See [[databases/135-connection-pooling]].

## The Write-Ahead Log

Storing vectors as plain blobs and scanning them in-process avoids an extension dependency. An FTS5 virtual table maintains an inverted index for fast keyword matching. A write-ahead log records changes before they touch the main file, so a crash can recover. See [[gardening/177-mulching]]. See [[databases/135-connection-pooling]].

Migrations should be forward-only and idempotent so environments never drift. Disposable derived data can always be rebuilt from the source of truth. SQLite is an embedded library, not a server; the database is a single ordinary file. The query planner picks an execution strategy; an index it cannot use is dead weight. See [[transformers/134-tokenization]]. See [[databases/135-connection-pooling]].
