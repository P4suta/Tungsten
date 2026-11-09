---
status: proposed
date: 2026-11-08
---
# Extend files with portable Markdown workflows

## Status

Proposed for the expanded personal Markdown app.
The decision extends the flat layout in [file storage](store-notes-as-markdown.md).

## Context

Reading, links, attachments, and recovery must share the existing file-based save protocol.
The app still serves one owner without accounts.

## Decision

Accept visible relative note paths and open every directory component without following symbolic links.
Keep Markdown sources unchanged and put attachments, snapshots, trash, and pins in hidden sibling directories.
Use one CommonMark parser for preview, tags, links, and outline data.
Escape raw HTML and use safe links and local attachment images.
Export ordinary files in ZIP archives and import notes without replacing existing content.

## Consequences

Existing root notes remain valid.
Folder moves use the same revision checks as renames.
Snapshots preserve replaced text and trash supports recovery through the browser.
Preview needs a server connection.

## Alternatives

A database would add a second source of truth.
A rich text editor would change the source editing workflow.

## Confirmation

Test nested paths, symbolic links, hostile Markdown, stale restores, archive traversal, and attachment round trips.
Keep parsing and filesystem boundaries covered by fixtures and integration tests.
