---
status: proposed
date: 2026-11-10
---
# Store named note collections in vault metadata

## Context

The requested design uses named note collections for task switching.
Collections must survive browser replacement.
Membership describes a working set rather than a semantic Markdown link.

## Decision

Store workspaces and indexed membership in the existing SQLite database.
Initialize workspaces through the [current-schema decision](initialize-only-the-current-storage-schema.md).
Keep notes in more than one workspace without changing note bodies.
Use [foreign key actions](https://www.sqlite.org/foreignkeys.html#fk_actions) to preserve membership on storage-name changes and remove references to deleted notes.
Use per-member operations rather than replacing complete membership lists.
Require matching revisions when renaming or deleting a workspace.
Bound responses and use conditional reads.
Keep each tab's selected workspace in session storage.

## Alternatives

Browser-local collections disappear with browser storage and differ across devices.
Index notes reuse Markdown links but make working-set changes alter authored content and graph meaning.

## Consequences

Database backups preserve workspaces.
Deleting a workspace keeps its notes.
Restoring a note keeps its content but resets collection membership.
Reconsider separate metadata if requests for collections include authored index pages.

## Confirmation

Check initialization, restart persistence, rejected stale changes, independent membership edits, rename cascades, note deletion, bounded pagination, and separate browser contexts.
