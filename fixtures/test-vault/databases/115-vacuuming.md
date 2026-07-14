---
b2id: 01KXF21DV4M6256ZM5P8TZAWSM
type: note
title: "Vacuuming"
---

# Vacuuming

Notes on vacuuming within the broader theme of databases.

## Connection Pooling

Storing vectors as plain blobs and scanning them in-process avoids an extension dependency. A write-ahead log records changes before they touch the main file, so a crash can recover. A covering index answers a query entirely from the index without touching the row. See [[databases/125-acid-transactions]]. See [[databases/135-connection-pooling]].

Storing vectors as plain blobs and scanning them in-process avoids an extension dependency. WAL mode lets readers proceed concurrently with a single writer. ACID transactions give you atomicity, consistency, isolation, and durability as one bundle.

ACID transactions give you atomicity, consistency, isolation, and durability as one bundle. Disposable derived data can always be rebuilt from the source of truth. WAL mode lets readers proceed concurrently with a single writer. An FTS5 virtual table maintains an inverted index for fast keyword matching. Migrations should be forward-only and idempotent so environments never drift. See [[databases/015-connection-pooling]]. See [[databases/015-connection-pooling]].

## The Write-Ahead Log

An FTS5 virtual table maintains an inverted index for fast keyword matching. Disposable derived data can always be rebuilt from the source of truth. Migrations should be forward-only and idempotent so environments never drift. ACID transactions give you atomicity, consistency, isolation, and durability as one bundle. A covering index answers a query entirely from the index without touching the row.

Storing vectors as plain blobs and scanning them in-process avoids an extension dependency. ACID transactions give you atomicity, consistency, isolation, and durability as one bundle. A write-ahead log records changes before they touch the main file, so a crash can recover. Disposable derived data can always be rebuilt from the source of truth. See [[databases/145-the-write-ahead-log]]. See [[vector-search/020-the-embedding-space]].

An FTS5 virtual table maintains an inverted index for fast keyword matching. A B-tree keeps keys sorted so lookups and range scans are logarithmic. Denormalization trades storage and write complexity for faster reads. A covering index answers a query entirely from the index without touching the row. Disposable derived data can always be rebuilt from the source of truth. The query planner picks an execution strategy; an index it cannot use is dead weight. See [[databases/145-the-write-ahead-log]].

## Vacuuming

Migrations should be forward-only and idempotent so environments never drift. An FTS5 virtual table maintains an inverted index for fast keyword matching. A covering index answers a query entirely from the index without touching the row. Denormalization trades storage and write complexity for faster reads. A write-ahead log records changes before they touch the main file, so a crash can recover. Storing vectors as plain blobs and scanning them in-process avoids an extension dependency.

SQLite is an embedded library, not a server; the database is a single ordinary file. A B-tree keeps keys sorted so lookups and range scans are logarithmic. Storing vectors as plain blobs and scanning them in-process avoids an extension dependency. Disposable derived data can always be rebuilt from the source of truth. WAL mode lets readers proceed concurrently with a single writer. An FTS5 virtual table maintains an inverted index for fast keyword matching.

## Schema Migrations

A write-ahead log records changes before they touch the main file, so a crash can recover. Storing vectors as plain blobs and scanning them in-process avoids an extension dependency. ACID transactions give you atomicity, consistency, isolation, and durability as one bundle. SQLite is an embedded library, not a server; the database is a single ordinary file. See [[databases/165-schema-migrations]].

Denormalization trades storage and write complexity for faster reads. The query planner picks an execution strategy; an index it cannot use is dead weight. A covering index answers a query entirely from the index without touching the row. SQLite is an embedded library, not a server; the database is a single ordinary file. An FTS5 virtual table maintains an inverted index for fast keyword matching. See [[databases/015-connection-pooling]]. See [[databases/175-query-planning]].

An FTS5 virtual table maintains an inverted index for fast keyword matching. Denormalization trades storage and write complexity for faster reads. ACID transactions give you atomicity, consistency, isolation, and durability as one bundle. A write-ahead log records changes before they touch the main file, so a crash can recover. A B-tree keeps keys sorted so lookups and range scans are logarithmic. See [[databases/045-connection-pooling]]. See [[databases/165-schema-migrations]].

## Query Planning

Migrations should be forward-only and idempotent so environments never drift. The query planner picks an execution strategy; an index it cannot use is dead weight. A covering index answers a query entirely from the index without touching the row. An FTS5 virtual table maintains an inverted index for fast keyword matching. SQLite is an embedded library, not a server; the database is a single ordinary file.

WAL mode lets readers proceed concurrently with a single writer. SQLite is an embedded library, not a server; the database is a single ordinary file. The query planner picks an execution strategy; an index it cannot use is dead weight. Migrations should be forward-only and idempotent so environments never drift. A write-ahead log records changes before they touch the main file, so a crash can recover. See [[databases/065-vacuuming]]. See [[databases/025-vacuuming]].
