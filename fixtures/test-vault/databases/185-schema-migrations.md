---
b2id: 01KXF21DVAZH051VYM4GQFA477
type: note
title: "Schema Migrations"
---

# Schema Migrations

Notes on schema migrations within the broader theme of databases.

## Vacuuming

An FTS5 virtual table maintains an inverted index for fast keyword matching. Storing vectors as plain blobs and scanning them in-process avoids an extension dependency. Denormalization trades storage and write complexity for faster reads. See [[distributed-systems/171-retries-and-jitter]]. See [[databases/175-query-planning]].

Disposable derived data can always be rebuilt from the source of truth. A write-ahead log records changes before they touch the main file, so a crash can recover. SQLite is an embedded library, not a server; the database is a single ordinary file. The query planner picks an execution strategy; an index it cannot use is dead weight. Denormalization trades storage and write complexity for faster reads. A covering index answers a query entirely from the index without touching the row. See [[databases/015-connection-pooling]]. See [[databases/135-connection-pooling]].

SQLite is an embedded library, not a server; the database is a single ordinary file. A write-ahead log records changes before they touch the main file, so a crash can recover. A covering index answers a query entirely from the index without touching the row. Denormalization trades storage and write complexity for faster reads. The query planner picks an execution strategy; an index it cannot use is dead weight. An FTS5 virtual table maintains an inverted index for fast keyword matching.

## The Write-Ahead Log

Denormalization trades storage and write complexity for faster reads. A B-tree keeps keys sorted so lookups and range scans are logarithmic. A covering index answers a query entirely from the index without touching the row. ACID transactions give you atomicity, consistency, isolation, and durability as one bundle. The query planner picks an execution strategy; an index it cannot use is dead weight. Migrations should be forward-only and idempotent so environments never drift. See [[databases/135-connection-pooling]].

A B-tree keeps keys sorted so lookups and range scans are logarithmic. SQLite is an embedded library, not a server; the database is a single ordinary file. An FTS5 virtual table maintains an inverted index for fast keyword matching. ACID transactions give you atomicity, consistency, isolation, and durability as one bundle. A write-ahead log records changes before they touch the main file, so a crash can recover. Disposable derived data can always be rebuilt from the source of truth. See [[hiking/119-switchbacks]].

A B-tree keeps keys sorted so lookups and range scans are logarithmic. SQLite is an embedded library, not a server; the database is a single ordinary file. A write-ahead log records changes before they touch the main file, so a crash can recover. Migrations should be forward-only and idempotent so environments never drift. Storing vectors as plain blobs and scanning them in-process avoids an extension dependency.

## ACID Transactions

Migrations should be forward-only and idempotent so environments never drift. Storing vectors as plain blobs and scanning them in-process avoids an extension dependency. A write-ahead log records changes before they touch the main file, so a crash can recover. The query planner picks an execution strategy; an index it cannot use is dead weight. A B-tree keeps keys sorted so lookups and range scans are logarithmic.

A covering index answers a query entirely from the index without touching the row. Disposable derived data can always be rebuilt from the source of truth. The query planner picks an execution strategy; an index it cannot use is dead weight. Migrations should be forward-only and idempotent so environments never drift. A write-ahead log records changes before they touch the main file, so a crash can recover.
