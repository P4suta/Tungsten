---
status: proposed
date: 2026-11-09
---
# Copy plaintext notes into a locked encrypted vault

## Context

Existing installations contain plaintext notes and original files.
Migration must preserve identifiers, history, links, and recovery behavior without interrupting the running installation.

## Decision

Lock the source and copy it into a separate encrypted destination after browser enrollment.
Use [SQLCipher export](https://www.zetetic.net/sqlcipher/sqlcipher-api/#sqlcipher_export) for the catalog and bounded transactions for original payloads.
Keep the destination locked until the transfer copies every indexed payload and verifies its source stamp.
Resume interrupted copies using committed payload keys.
Keep the source for an explicit operator cutover.

## Alternatives

In-place deletion couples migration recovery to plaintext removal.
Exporting Markdown alone loses recovery records and file identities.

## Consequences

Migration temporarily needs both storage copies.
No app process may change the source during transfer.
Retained sources and external backups remain plaintext until the operator removes them.
Reconsider the copy protocol when external concurrent writers become supported.

## Confirmation

Check interrupted transfers, source mismatches, original file bytes, metadata, history, Trash visibility, and restart recovery.
Keep the record proposed until integration passes.
