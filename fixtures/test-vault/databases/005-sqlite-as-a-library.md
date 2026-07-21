---
b2id: 01KXF21DTQNNG8Y9P12FHDJFJ5
type: note
title: "SQLite as a Library"
b2_relations:
  - "relates [[databases/185-schema-migrations]] — see also"
---

# SQLite as a Library

Notes on sqlite as a library within the broader theme of databases.

## The Write-Ahead Log

ACID transactions give you atomicity, consistency, isolation, and durability as one bundle. The query planner picks an execution strategy; an index it cannot use is dead weight. Denormalization trades storage and write complexity for faster reads. A covering index answers a query entirely from the index without touching the row. See [[databases/125-acid-transactions]].

ACID transactions give you atomicity, consistency, isolation, and durability as one bundle. Storing vectors as plain blobs and scanning them in-process avoids an extension dependency. SQLite is an embedded library, not a server; the database is a single ordinary file. A write-ahead log records changes before they touch the main file, so a crash can recover. A B-tree keeps keys sorted so lookups and range scans are logarithmic. See [[databases/055-sqlite-as-a-library]]. See [[databases/115-vacuuming]].

## Schema Migrations

Disposable derived data can always be rebuilt from the source of truth. SQLite is an embedded library, not a server; the database is a single ordinary file. ACID transactions give you atomicity, consistency, isolation, and durability as one bundle. Migrations should be forward-only and idempotent so environments never drift. A B-tree keeps keys sorted so lookups and range scans are logarithmic. See [[databases/115-vacuuming]].

Disposable derived data can always be rebuilt from the source of truth. An FTS5 virtual table maintains an inverted index for fast keyword matching. Storing vectors as plain blobs and scanning them in-process avoids an extension dependency.

Denormalization trades storage and write complexity for faster reads. SQLite is an embedded library, not a server; the database is a single ordinary file. Migrations should be forward-only and idempotent so environments never drift. Storing vectors as plain blobs and scanning them in-process avoids an extension dependency. A write-ahead log records changes before they touch the main file, so a crash can recover. Disposable derived data can always be rebuilt from the source of truth. See [[databases/165-schema-migrations]].

## B-Tree Indexes

A covering index answers a query entirely from the index without touching the row. Storing vectors as plain blobs and scanning them in-process avoids an extension dependency. ACID transactions give you atomicity, consistency, isolation, and durability as one bundle. Denormalization trades storage and write complexity for faster reads. See [[databases/015-connection-pooling]].

A B-tree keeps keys sorted so lookups and range scans are logarithmic. Denormalization trades storage and write complexity for faster reads. Disposable derived data can always be rebuilt from the source of truth. Migrations should be forward-only and idempotent so environments never drift. A covering index answers a query entirely from the index without touching the row. See [[databases/115-vacuuming]].

ACID transactions give you atomicity, consistency, isolation, and durability as one bundle. Disposable derived data can always be rebuilt from the source of truth. SQLite is an embedded library, not a server; the database is a single ordinary file. Storing vectors as plain blobs and scanning them in-process avoids an extension dependency. WAL mode lets readers proceed concurrently with a single writer. A write-ahead log records changes before they touch the main file, so a crash can recover. See [[databases/065-vacuuming]].

## B-Tree Indexes

The query planner picks an execution strategy; an index it cannot use is dead weight. Storing vectors as plain blobs and scanning them in-process avoids an extension dependency. A covering index answers a query entirely from the index without touching the row. A B-tree keeps keys sorted so lookups and range scans are logarithmic.

Denormalization trades storage and write complexity for faster reads. Storing vectors as plain blobs and scanning them in-process avoids an extension dependency. WAL mode lets readers proceed concurrently with a single writer. See [[databases/105-sqlite-as-a-library]]. See [[databases/145-the-write-ahead-log]].

The query planner picks an execution strategy; an index it cannot use is dead weight. A write-ahead log records changes before they touch the main file, so a crash can recover. A covering index answers a query entirely from the index without touching the row. WAL mode lets readers proceed concurrently with a single writer. See [[transformers/094-fine-tuning-vs-prompting]]. See [[databases/105-sqlite-as-a-library]].

## Denormalization

Denormalization trades storage and write complexity for faster reads. The query planner picks an execution strategy; an index it cannot use is dead weight. ACID transactions give you atomicity, consistency, isolation, and durability as one bundle. Disposable derived data can always be rebuilt from the source of truth. See [[databases/085-connection-pooling]].

SQLite is an embedded library, not a server; the database is a single ordinary file. A B-tree keeps keys sorted so lookups and range scans are logarithmic. Denormalization trades storage and write complexity for faster reads.

SQLite is an embedded library, not a server; the database is a single ordinary file. Migrations should be forward-only and idempotent so environments never drift. An FTS5 virtual table maintains an inverted index for fast keyword matching. WAL mode lets readers proceed concurrently with a single writer. Disposable derived data can always be rebuilt from the source of truth. Storing vectors as plain blobs and scanning them in-process avoids an extension dependency. See [[databases/045-connection-pooling]].

## Vacuuming

An FTS5 virtual table maintains an inverted index for fast keyword matching. ACID transactions give you atomicity, consistency, isolation, and durability as one bundle. The query planner picks an execution strategy; an index it cannot use is dead weight. A covering index answers a query entirely from the index without touching the row. See [[databases/105-sqlite-as-a-library]].

A covering index answers a query entirely from the index without touching the row. An FTS5 virtual table maintains an inverted index for fast keyword matching. SQLite is an embedded library, not a server; the database is a single ordinary file. A write-ahead log records changes before they touch the main file, so a crash can recover. See [[databases/125-acid-transactions]]. See [[databases/025-vacuuming]].

SQLite is an embedded library, not a server; the database is a single ordinary file. Migrations should be forward-only and idempotent so environments never drift. WAL mode lets readers proceed concurrently with a single writer. Denormalization trades storage and write complexity for faster reads. The query planner picks an execution strategy; an index it cannot use is dead weight. ACID transactions give you atomicity, consistency, isolation, and durability as one bundle. See [[databases/195-denormalization]]. See [[rust/162-the-newtype-pattern]].
