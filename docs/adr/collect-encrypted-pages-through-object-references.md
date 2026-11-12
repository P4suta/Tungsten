---
status: proposed
date: 2026-11-09
---
# Collect encrypted pages through object references

## Context

Immutable encrypted pages leave obsolete objects after edits and failed uploads.
The prototype remains unadopted.
[Native encrypted storage](search-encrypted-storage-on-a-trusted-running-server.md) replaces the candidate.
The requirements exclude permanent accumulation and corpus-sized maintenance on devices.

## Decision

Include sorted child object hashes in each stored envelope.
Hash the complete envelope to bind ciphertext and references to its identity.
The server keeps object bytes and reference counts in transactional SQLite tables.
Keep reference counts separate from object payloads.
Accept references to existing objects to prevent cycles and dangling edges.
Index incoming references for [foreign key checks](https://sqlite.org/foreignkeys.html#required_and_suggested_database_indexes).
Protect replaced roots and unreferenced uploads for five minutes.
Collect eligible objects in bounded batches and decrement their children's counts.

## Alternatives

Device-side marking requires reading the entire mapping tree.
Server-side marking scans the complete object graph.
Retaining every object makes ordinary editing accumulate storage indefinitely.

## Consequences

The server retains physical page sharing metadata, object sizes, and object references.
Search terms, titles, and posting lists remain encrypted.
Reference counts make collection work proportional to discarded objects.
Reconsider retention if a bounded request can outlive the protected interval.

## Confirmation

Check interrupted uploads, shared children, competing roots, retention, and bounded reclamation.
