---
status: proposed
date: 2026-11-08
---
# Collect unreferenced attachments after a grace period

## Context

Attachment cleanup replaces the file management page.
Uploads precede note saves, and history or trash can still require an attachment.

## Decision

Scan note sources, retained versions, and trash before deleting any attachment.
Protect references recognized by the Markdown parser.
Record the first unreferenced sweep and wait seven days before deleting bytes.
Clear the timer whenever a reference returns.
Run cleanup on startup and hourly under the existing storage lock.
Stop on unreadable sources before deleting candidates.
Remove original bytes and associated metadata without following symbolic links.
Keep existing metadata readable and include retained files in complete exports.

## Alternatives

Immediate deletion races with pending uploads and offline drafts.
Keeping a management page makes the owner identify unused files manually.
Keeping files forever fails the requested automatic cleanup.

## Consequences

Cleanup can remove unsaved attachment drafts after seven unreferenced days.
History and trash follow [bounded recovery](expire-recovery-records-before-collecting-files.md).
Reconsider the grace period if long-lived offline drafts become a supported workflow.

## Confirmation

Check recent uploads, all reference roots, expired orphans, corrupt records, symbolic links, metadata cleanup, and repeated sweeps with an explicit clock.
