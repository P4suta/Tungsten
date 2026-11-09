---
status: proposed
date: 2026-11-09
---
# Update default link labels with note titles

## Context

[Internal identities](keep-note-identities-internal.md) prevent title edits from breaking destinations.
Default link labels can still show an outdated title.

## Decision

When a heading title changes, inspect other notes for inline Markdown links to the stable target.
Update a label when its escaped source exactly matches the preceding title.
Preserve destinations, explicit aliases, code, images, and unrelated links.
Validate candidate sources before writing changes.
Snapshot affected notes and give each changed source a fresh revision.
Keep the existing storage lock and atomic writes per note.
Write the changed title after its reference updates.
Don't claim a filesystem transaction across the collection.

## Alternatives

Renaming files after headings would change destinations and require more reference updates.
Updating rendered labels alone leaves exported Markdown stale.
Updating every label discards intentional aliases.

## Consequences

Explicit aliases identical to the preceding title also change.
Open drafts in affected notes keep the existing revision-conflict recovery.
Storage failure can stop a group after some independent file writes.
Reconsider a journal if collection changes must become atomic.

## Confirmation

Check relative targets, escaped Unicode titles, aliases, code, history, and stale revisions.
