---
status: proposed
date: 2026-11-09
---
# Expire recovery records before collecting files

## Context

Count-limited history without expiry can keep attachments indefinitely in quiet notes.
The owner wants recovery from mistakes without permanent file storage.

## Decision

Expire history and trash after seven days during storage cleanup.
Keep at most 50 checkpoints per note and combine changes within five-minute windows.
Replace checkpoint identities when combining changes so a viewed identity never restores different bytes.
Preserve capture timestamps through metadata changes.
Use directory timestamps for existing records without capture metadata.
Follow expiry with the attachment grace period in [automatic collection](collect-unreferenced-attachments-after-a-grace-period.md).
Offer permanent deletion from an embedded attachment when the owner wants to retract an upload immediately.
Explain that retained note versions also lose access to the original file.

## Alternatives

Permanent history prevents automatic reclamation.
No recovery leaves accidental edits dependent on external backups.
External Git management remains available for owners who want long-term history.

## Consequences

Short-lived edits can share a checkpoint.
[Obsidian](https://obsidian.md/help/plugins/file-recovery) also defaults to five-minute snapshots retained for seven days.
Reconsider the limits if recovery requests routinely exceed them.

## Confirmation

Check explicit capture times, coalescing, stable restoration identities, expiry, legacy metadata, and attachment references.
