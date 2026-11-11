---
status: proposed
date: 2026-11-10
---
# Omit term positions from large substring indexes

## Context

The [evaluation](../storage-evaluation.md) found costly alphanumeric saves.
Search verifies literal substrings after retrieving candidates.
[FTS5](https://sqlite.org/fts5.html#the_detail_option) can omit positions when indexing distinct tokens.

## Decision

Keep positional trigram indexes for normalized text up to 16 KiB.
Index larger text and text containing NUL through distinct Unicode windows of one to three characters with `detail=none`.
Intersect windows for positive long terms.
Verify complete terms in the original body or normalized title.
Exclude terms through distinct tokens only when one token exactly represents the term.
Initialize indexes through the [current-schema decision](initialize-only-the-current-storage-schema.md).

## Alternatives

Keeping positions everywhere repeats costly position storage during saves.
Using distinct windows everywhere requires costly verification for common negative terms and colliding windows.
Native `detail=column` reduced writes without establishing faster saves.

## Consequences

Large-text candidates need original-text verification.
The size boundary limits positional write costs while preserving phrase rejection for ordinary notes.
Portable restoration rebuilds indexes from source records.

## Confirmation

Require reference searches, pagination, title updates, restoration, and the project gate.
Compare repeated long-note saves, populated collections, and production memory.
Reconsider after search regressions or excessive indexing costs.
