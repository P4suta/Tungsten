---
status: proposed
date: 2026-11-09
---
# Index Markdown and fetch bounded neighborhoods

## Context

The million-page experiment in [verification](../verification.md#scale-observations) exceeded interactive request deadlines.
Markdown must remain readable independently under [file storage](store-notes-as-markdown.md).

## Decision

Keep Markdown authoritative and derive a local SQLite catalog containing searchable text, summaries, links, and attachment references.
Use [FTS5](https://sqlite.org/fts5.html) with Unicode character grams and text validation for substring search.
Use indexed adjacency, cursor pagination, and execution limits.
Journal file changes and reconcile interrupted work and external edits on reopening.
Queue large label updates in bounded batches.
Index recovery records and attachment cleanup clocks alongside durable work queues.

## Alternatives

Scanning files retains the measured failure.
A search server adds deployment work.
Making SQLite authoritative changes portable storage.

## Consequences

Initial indexing costs time and disk space.
Rebuilding indexes must never delete original files.
Back up source files, pending work, and cleanup clocks.
External edits need a restart to enter search results.
Reconsider the index when measured latency or storage exceeds the documented limits.

## Confirmation

Check indexing recovery, substring semantics, pagination, mutations, incoming links, and reference safety.
Measure interactive workflows and bounded responses with one million real pages.
