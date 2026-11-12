---
status: proposed
date: 2026-11-10
---
# Compare storage with shared indexes and repeated workloads

## Context

The first comparison used sixteen notes and different search implementations.
Its long-edit results combine engine costs with avoidable index allocation and rewriting.
Storage selection requires a defensible evaluation.

## Decision

Keep the production SQLite adapter and add a SQLite key-value control using the same code as redb and Fjall.
Use packed Unicode grams, changed postings, and ordered bounded search.
Measure both preparation time and transaction time.
Rotate engine order across fresh-process repetitions with matching seeded inputs and durable acknowledgements.
Exercise increasing collections, long edits, varied text, attachments, recovery, and crashes.

## Alternatives

Repeating the initial prototype preserves an implementation confound.
Engine microbenchmarks omit search and recovery costs.
Replacing production storage before comparing behavior accepts unmeasured durability and resource costs.

## Consequences

Shared-index results isolate engines within the chosen layout.
Production-adapter results compare complete tested implementations.
Synthetic repeated runs establish bounded observations, not long-term reliability.

## Confirmation

Require equivalent returned values, complete runs, fixed limits, repeatable inputs, and visible dispersion.
Reconsider production storage only after a candidate meets functional and resource requirements at the measured scales.
