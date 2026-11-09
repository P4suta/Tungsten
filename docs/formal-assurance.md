# Verify production decisions with bounded model checking

## Scope

The proof gate checks executable Rust with [Kani 0.68.0](https://github.com/model-checking/kani/releases/tag/kani-0.68.0).
Native proofs call the production path, identifier, revision, destination, and attribute rules in `src/rules.rs`.
Vault proofs call the dependency-free `tungsten-state` crate used by manifest persistence and the private request boundary.
Browser proofs call generated Rust functions translated from the production Boolean decisions in `ui/src/model.ts`.
The translator accepts explicit Boolean parameters, Boolean returns, identifiers, literals, negation, conjunction, and disjunction.
It rejects extra or missing functions, changed parameter meanings, side effects, implicit coercions, and unsupported syntax.

## Properties

Accepted note paths remain relative and visible, contain at most 16 components, and exclude control bytes and backslashes.
Internal identifiers and attachment extensions can't introduce directory traversal.
Revision preconditions require identical nonempty bytes, including equal lengths.
Competing writers using the same revision can't both pass after a distinct revision commits.
The competing-writer proof assumes serialized revision comparison and replacement.
The store mutex and database transaction enforce serialization.
Hash collision resistance and scheduling outside the store remain external assumptions.

Rendered destinations use permitted schemes without whitespace or control bytes.
Escaped attributes contain no raw quote or angle-bracket delimiters and expand to at most six times the input byte length.
The production byte encoder preserves input order and emits exact entities without heap allocation.
Its proof covers every byte pattern, including empty and non-ASCII inputs.
A separate proof covers every Unicode scalar and checks valid UTF-8 output and exact entity bytes.
The string adapter allocates a vector and uses Rust's checked UTF-8 conversion.
Integration tests check every Unicode scalar and long mixed strings through the adapter.

Saving requires dirty, idle, unblocked input outside composition.
The saved indicator requires an acknowledged revision, matching text, and no pending writer.
Remote reads require a visible note without an active operation.
Remote replies require the same editor, navigation generation, and revision, without loading, writing, composition, or conflict blocking.
Different local writing causes a conflict instead of replacement.
Automatic draft recovery excludes a living owner and requires coordination for owned drafts.
Legacy drafts remain recoverable.

Vault creation requires preparation before readiness.
An existing vault keeps its identity and advances from pending through optional migration to readiness.
Identical manifests are idempotent, and readiness can't regress.
Private requests require a ready manifest, a valid session, an available router, and no failed initialization.
An unlocked server doesn't grant another browser profile access without enrollment.

## Bounds and correspondence

Boolean decision proofs cover every parameter combination.
Path inputs cover lengths from zero through 240 bytes, identifiers through 45 bytes, revision comparisons through 66 bytes, and destinations through 128 bytes.
Attribute inputs cover zero through 64 bytes.
The scalar encoding proof covers the complete Rust `char` domain.
The competing-writer schedule uses distinct four-byte revision representatives and both writer orders.
Equality representatives model decision outcomes without modeling the hash algorithm.

Temporal browser and vault harnesses examine sequences of 16 operations with explicit unwinding bounds.
Browser traces model edits, save requests, acknowledgements, navigation, revision changes, composition, and refresh replies.
Text identity representatives cover repeated values and edits that return to earlier text.
Navigation counters remain within the declared sequence bound.
The traces verify compositions of production decisions under the harness's adapter model.
They don't prove the browser event loop, network transport, storage APIs, or every production event handler.
Browser tests exercise delayed replies, live sibling drafts, hidden pages, competing encrypted writes, independent profiles, and restarts at those boundaries.

Cryptographic primitives, key erasure, allocation, filesystem durability, SQLite transactions, HTTP delivery, and browser coordination keep integration checks.
The proof gate covers bounded execution and trusts the running server.

## Reproduction and refusal behavior

Run `mise x -- just proofs --dry-run` to inspect the source inventory and resource allocation without starting a verifier.
Run `mise x -- just proofs` for the pinned Linux container with two CPUs, 4 GiB of RAM, no swap, no network, and a deadline of 890 seconds.
The first image build downloads pinned tools and dependencies before the isolated verification run.
Each harness has a timeout of 300 seconds.
The [C Bounded Model Checker (CBMC) pointer model](https://diffblue.github.io/cbmc/cprover-manual/md_memory-primitives.html) uses Kani's default of 16 object bits and rejects object exhaustion.
The release gate uses the same native proof entry point before its separate fuzz and mutation checks.

The runner compares Rust syntax declarations, Kani's discovered inventory, and the executed results.
The runner hashes production inputs, adapters, harnesses, translated code, and tool configuration before building and after verification.
Changing any hashed input during a run invalidates the verdict.
Every declared harness needs an explicit unwinding bound, a reachable project assertion, and satisfied cover properties.
The gate rejects missing or duplicate harnesses, expected panics, failed assertions, failed unwinding, unreachable project assertions, unsatisfied covers, timeouts, and solver errors.
Unreachable compiler checks inside dependencies don't count as project assertions.
[Kani's unwinding assertions](https://model-checking.github.io/kani/tutorial-loop-unwinding.html) must succeed within each declared bound.
Rejection fixtures exercise incomplete results and vacuous success reports in the normal test suite.
The normal gate also type-checks the browser decisions and compiles the proof and fuzz targets.
