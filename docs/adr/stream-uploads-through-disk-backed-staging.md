---
status: proposed
date: 2026-11-10
---
# Stream uploads through disk-backed staging

## Context

The requirements favor a small self-hosted memory footprint.
Buffering a 64 MiB upload and binding its SQLite value creates file-sized allocations.

## Decision

Stage binary uploads in an [unnamed file](https://docs.rs/tempfile/3.27.0/tempfile/fn.tempfile_in.html) beside the database.
Copy bounded chunks through [SQLite incremental I/O](https://www.sqlite.org/c3ref/blob_open.html) and commit bytes, hashes, and metadata together.
Keep one upload slot through ingestion and commit.
Cap compatibility JSON uploads at 1 MiB.
Use a 2 MiB [page cache](https://www.sqlite.org/pragma.html#pragma_cache_size), file-backed temporary storage, and no memory mapping.

## Alternatives

Direct database streaming holds the store lock while a slow client sends bytes.
Whole-file buffering avoids staging I/O but scales memory with attachment size.
Separate permanent files require another durability protocol.

## Consequences

Staging adds disk I/O and up to 64 MiB of temporary storage.
Closing the final staging handle removes partial uploads, including process termination.
Large collections rely on indexed reads and reclaimable filesystem cache.
Reconsider cache size if measured search latency exceeds the existing budget.

## Confirmation

Check original bytes, hashes, cancellation, short streams, size limits, rollback, concurrent writes, and restart.
Measure resident high-water marks in a 64 MiB container without swap.
