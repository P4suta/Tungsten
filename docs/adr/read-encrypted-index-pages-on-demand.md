---
status: proposed
date: 2026-11-09
---
# Read encrypted index pages on demand

## Context

The owner rejects plaintext search indexes and complete index copies on devices.
The owner also rejects disclosure of search equivalence and frequency.

## Decision

Run SQLite in a browser worker over authenticated, encrypted pages.
Read immutable pages through a bounded cache and mapping tree.
Use [SQLite's virtual filesystem boundary](https://sqlite.org/vfs.html) without changing its query engine.
Publish a replacement root atomically after its objects become durable.
Keep a memory journal and withhold save acknowledgement until root publication succeeds.

## Alternatives

The current server index is efficient but exposes content.
Complete device replicas expose no server index but require corpus-sized storage.
Oblivious access conceals more traffic information at higher communication cost.

## Consequences

The server still observes object accesses, timing, and transfer sizes.
Repeated searches produce repeatable physical reads in the regression fixture.
The candidate fails to conceal search repetition.
Cold queries require network reads.
Bound query work, dirty pages, and SQLite memory.
Reconsider if latency exceeds the editing budget.

## Confirmation

Check authenticated reads, interrupted commits, competing writers, bounded caches, and search latency.
Defer adoption while search privacy remains unresolved.
Test [native encrypted storage](search-encrypted-storage-on-a-trusted-running-server.md) within an explicit running-server trust boundary.
