---
status: proposed
date: 2026-11-09
---
# Refresh open notes without replacing local drafts

## Context

Writing moves between tabs and devices.
Revision preconditions prevent overwritten saves, but inactive views remain stale and shared draft storage can import another live tab's edits.

## Decision

Check the displayed note on foreground transitions, successful writes in sibling tabs, and every fifteen seconds while visible.
Use [conditional HTTP reads](https://www.rfc-editor.org/rfc/rfc9110.html#section-13.1.2) to avoid transferring unchanged content.
Apply remote content when local writing matches its saved body.
Preserve dirty, composing, pending, and superseded editor states.
Keep revision preconditions authoritative for competing writes.
Use [Web Locks](https://www.w3.org/TR/web-locks/) to identify live draft owners within an origin.
Recover other drafts explicitly.
Automatically recover abandoned drafts when coordination establishes ownership.

## Alternatives

Manual reload leaves routine device switching stale.
Persistent collaborative streams add connection lifecycle state to personal editing.
Automatic text merging needs separate rules for overlapping changes.

## Consequences

Visible tabs make bounded requests for one note.
Simultaneous conflicting edits require recovery.
Unavailable ownership coordination leaves drafts available for explicit recovery.
Reconsider live collaboration when concurrent coauthoring becomes necessary.

## Confirmation

Test independent browser contexts, competing saves, deletion, interrupted requests, composition, reload recovery, and live draft ownership.
