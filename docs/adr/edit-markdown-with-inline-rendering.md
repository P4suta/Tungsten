---
status: proposed
date: 2026-11-08
---
# Edit Markdown with inline rendering

## Context

The owner now requests rendered editing, reconsidering the [separate source editor](keep-writing-and-navigation-discoverable.md).
Composition, recovery, and original Markdown must survive the change.

## Decision

Use CodeMirror [replacement decorations](https://github.com/codemirror/view/blob/main/src/editorview.ts) for inactive blocks.
Keep the active block editable as Markdown.
Reuse the server renderer and its safe links, task offsets, and media handling.
Bridge transactions to the existing draft and revision machinery.
Use contenteditable input while the pinned engine's Android EditContext path loses cursor positions.
Show independent, unsaved title and body cues in an empty document.
Bundle dependencies locally and use [Prettier standalone](https://prettier.io/docs/browser) for explicit or optional automatic formatting.
Keep a source view for exact text editing.

## Alternatives

The existing textarea is smaller but requires switching views.
A rich text document model adds a Markdown serialization boundary and can change source during ordinary edits.

## Consequences

The bundle grows and cursor, rendering, undo, and composition need integration tests.
Vendor editor styles require explicit monochrome overrides and computed-style checks.
Reconsider if rendered editing can't preserve source or keyboard operation.

## Confirmation

Check real typing, inactive rendering, source selection, composition, formatting interruption, uploads, undo, and desktop and narrow screenshots through the project gate.
