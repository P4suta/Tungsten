---
status: proposed
date: 2026-11-10
---
# Maintain posting counts with durable index changes

## Context

The [comparison](../storage-evaluation.md) exceeded the search deadline after ranking capped samples as equal.
The rarer posting contained 379 notes, and the selected posting contained 100000.

## Decision

Maintain exact counts for searchable posting prefixes in the shared key-value adapters.
Apply posting differences and count changes in the same durable transaction.
Combine count changes within batches.
Start with the smallest posting and intersect selective postings by identity before reading bodies.
Verify complete terms and rebuild counts through restoration.
Refuse incompatible index formats.

## Alternatives

Larger fixed samples merely move the ambiguity to larger collections.
Progressive sampling can scan every common posting before returning a small page of common matches.
Compressed posting lists could reduce allocation but require a separate layout comparison.

## Consequences

Count records add writes and derived storage.
Batch combination limits repeated updates.
The counts describe candidate selectivity rather than guaranteeing a query deadline.

## Confirmation

Require exact counts through batch loading, edits, deletion, rollback, and restoration.
Reject malformed or missing counts for existing postings.
Repeat the failed identifier search at 100000 notes and check creation, search, and save costs at smaller scales.
Reconsider if count maintenance costs outweigh measured search gains.
