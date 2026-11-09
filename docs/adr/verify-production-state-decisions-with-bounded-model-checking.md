---
status: proposed
date: 2026-11-10
---
# Verify production state decisions with bounded model checking

## Context

The owner requires broader formal assurance for concurrent editing and encrypted storage.
Example tests can't check every decision input or operation ordering.
Proofs must execute production decisions and preserve their language semantics.

## Decision

Put vault state decisions in a dependency-free Rust crate without the standard library.
Translate the restricted, pure TypeScript decision functions into Rust through the maintained syntax parser.
Reject unsupported syntax and extra or missing functions.
Use [Kani](https://model-checking.github.io/kani/usage.html) for exhaustive finite inputs and explicitly bounded transition sequences.
Require reachable success and refusal cases, successful unwinding assertions, and complete harness execution.
Run verification in the bounded Linux container.

## Alternatives

Independent handwritten browser models can drift from production.
Browser integration tests cover external behavior without proving all schedules.
Unbounded mathematical proofs need implementation refinement.

## Consequences

Cryptography, filesystem durability, browser APIs, and network delivery keep integration checks.
The proof boundary includes the verified adapter inputs and declared sequence bounds.
Timeouts and unreachable assertions block a successful verdict.
Reconsider the verifier when production semantics exceed the supported translation.

## Confirmation

Reject altered contracts and incomplete harness results.
Verify production call sites, positive and refusal cases, and bounded competing operations.
