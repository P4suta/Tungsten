---
status: proposed
date: 2026-11-10
---
# Store notes and indexes in one local database

## Context

The requested server removes app encryption.
Million-note search must avoid startup filesystem scans and device replicas.

## Decision

Store Markdown, attachments, recovery records, and indexes in ordinary SQLite.
Commit each mutation and derived metadata together.
Use [write-ahead logging](https://www.sqlite.org/wal.html), `synchronous=FULL`, and incremental binary-object reads.
Provide bounded Markdown export, loopback listening, same-origin checks, and container isolation.
Delegate encryption and access control to the operator.

## Alternatives

Markdown files require filesystem reconciliation.
App encryption adds enrollment while trusting the running server.
Separate attachment files need another recovery protocol.
SQLite's [binary-object measurements](https://www.sqlite.org/intern-v-extern-blob.html) require hardware-specific confirmation.

## Consequences

Storage readers can read contents.
External editing requires export.
Reject unknown or incomplete databases.
Reconsider attachment storage if measured latency exceeds the capacity budget.

## Confirmation

Check rollback, revisions, restart persistence, exports, file ranges, concurrent browsers, and capacity.
Initialize storage through the [current-schema decision](initialize-only-the-current-storage-schema.md).
