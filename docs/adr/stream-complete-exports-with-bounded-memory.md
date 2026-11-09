---
status: proposed
date: 2026-11-09
---
# Stream complete exports with bounded memory

## Context

The owner requires complete exports.
The earlier ZIP implementation buffered every file and rejected collections exceeding 10000 files or 256 MiB.

## Decision

Stream a tape archive containing Markdown, original attachments, pins, and recovery records.
Use the [archive writer](https://docs.rs/tar/0.4.46/tar/struct.Builder.html) through bounded buffers and a channel with backpressure.
Permit one export at a time and release the note lock before transferring files.
Stop writing when the download disconnects.
Open files relative to directory handles without following symbolic links.

## Alternatives

A buffered ZIP preserves the previous format but exceeds memory at the required scale.
A temporary archive needs extra disk space before downloading.

## Consequences

Tape archives remain readable with ordinary archive tools and need no central directory in memory.
Exports read files during transfer and aren't transactional snapshots.
Stop the service before making a consistent filesystem backup.

## Confirmation

Check original bytes, recovery records, symbolic link exclusion, disconnection, and exports exceeding the former file limit.
