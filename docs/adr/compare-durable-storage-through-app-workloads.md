---
status: proposed
date: 2026-11-10
---
# Compare durable storage through app workloads

## Context

The owner requests portable recovery and storage comparisons for text-heavy and attachment-heavy use.

## Decision

Keep SQLite in production and compare durable operations through development-only SQLite, redb, and Fjall adapters.
Match inputs, durability, result checks, and resource limits.
Archive Markdown, binary originals, and streamed JSON metadata.
Use stable identities for archive payload paths.
Rebuild indexes privately before atomic restoration.
Keep small regressions in source checks and heavy production checks at release.

## Alternatives

Raw key-value benchmarks omit app costs.
Database copies tie recovery to SQLite's schema.
[Turso's native full-text search](https://github.com/tursodatabase/turso#features-and-roadmap) remains experimental and stays outside the measured adapters.

## Consequences

Prototype results cover tested operations.
Operators encrypt readable archives.
Preserve identifiers, clocks, maintenance, and revision preconditions.
Reconsider production storage after both workload families meet requirements.

## Confirmation

Check round trips, corruption, interruption, conflicts, search, and references.
Reject incomplete measurements and incompatible baselines.
Compare repeatable runs before proposing adoption.
