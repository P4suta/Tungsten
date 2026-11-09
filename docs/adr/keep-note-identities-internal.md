---
status: proposed
date: 2026-11-08
---
# Keep note identities internal

## Context

The owner wants a flat collection of titled notes connected by links.
Storage paths and classification controls create unwanted decisions.

## Decision

Give new notes stable random storage identifiers.
Use Markdown headings as visible titles and derive an untitled fallback without exposing identifiers.
Rename a note by editing its heading.
Keep links and stored identity stable.
Remove note filename, tag, folder, pin, and template controls and their browser handlers.
Keep existing Markdown paths, references, metadata, and archive compatibility.
Attachment folders remain a file-management concern.
Replace branding with Home navigation.
Give interactive controls icons under owner policy and preserve [visible labels and names for assistive technology](https://www.w3.org/WAI/WCAG22/Techniques/aria/ARIA14).
Redundant icons follow [decorative-image guidance](https://www.w3.org/WAI/tutorials/images/decorative/).

## Alternatives

Heading-derived filenames are readable on disk but require renaming and link maintenance.
Keeping optional organization controls preserves choices the owner has excluded.

## Consequences

Storage names become an implementation detail.
Existing paths remain valid without migration.
Reconsider if direct filesystem naming becomes a writing task.

## Confirmation

Check duplicate titles, heading changes, old paths, links, imports, downloads, and icon coverage through the project gate.
