---
status: accepted
date: 2026-11-08
---
# Store notes as files

## Status

Accepted in the implementation plan.

## Context

A single owner writes notes through browsers on a private tailnet.
Notes must remain readable without the service.

## Decision

Store each note as a UTF-8 Markdown file directly inside one data directory.
Use the filename as its name and a content hash as its revision.
Serialize writes, check revisions, and replace files through a synchronized temporary file in the same directory.
Move deleted notes into a private trash directory.
Keep a process lock on the data directory and open note files relative to a directory descriptor without following symbolic links.

## Consequences

Backup and recovery use ordinary files.
Search reads the files without a database or a search index.
The filesystem must support Unix locking, atomic rename, and directory synchronization.

## Alternatives

A database would require an export path and add another persisted format.
Live external editing would require conflict handling between processes.

## Confirmation

File and HTTP tests must check persistence, failed writes, stale revisions, process locking, and symbolic links.
Browser tests must check draft recovery and delayed save responses.
