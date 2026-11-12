---
status: proposed
date: 2026-11-10
---
# Verify production state decisions with bounded model checking

## Context

Concurrent editing and input validation require broader formal assurance.
Example tests can't check every decision input or operation ordering.
Proofs must execute production decisions and preserve their language semantics.

## Decision

Keep native byte-validation decisions in the production rules module.
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

Filesystem durability, browser APIs, and network delivery keep integration checks.
The proof boundary includes the verified adapter inputs and declared sequence bounds.
Timeouts and unreachable assertions block a successful verdict.
Reconsider the verifier when production semantics exceed the supported translation.

## Confirmation

Reject altered contracts and incomplete harness results.
Verify production call sites, positive and refusal cases, and bounded competing operations.
