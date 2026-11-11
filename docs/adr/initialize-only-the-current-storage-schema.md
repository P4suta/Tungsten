---
status: accepted
date: 2026-11-11
---
# Initialize only the current storage schema

## Context

Tungsten remains unreleased.
The owner explicitly excludes compatibility with previous storage formats.
Workspace and search-index migrations add conversion paths outside the supported scope.

## Decision

Create the complete current database schema in one transaction.
Accept only its exact format marker and app identity.
Remove previous-format converters and migration fixtures.
Require complete current records and search cursors without compatibility defaults.
Keep current-format archive validation, restoration, and source-data checks.
This replaces the migration requirements in the [workspace](store-named-note-collections-in-vault-metadata.md) and [search-index](omit-term-positions-from-substring-candidate-indexes.md) decisions.

## Alternatives

Retaining migrations preserves unreleased databases but adds conversion code and failure cases outside the owner's requirements.

## Consequences

Previous development databases require recreation.
Unsupported databases fail without automatic conversion.
Research results remain in the evaluation history.

## Confirmation

Check unsupported-format refusal, current-schema initialization, restart persistence, search results, archive restoration, and the project gate.
Reconsider compatibility requirements before the first public release.
