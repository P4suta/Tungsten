---
status: proposed
date: 2026-11-08
---
# Render Markdown with text decorations

## Context

Replacing entire paragraphs in [inline rendering](edit-markdown-with-inline-rendering.md) changes editing geometry.
Inline emphasis alone leaves lists, quotes, and fenced code looking like source.

## Decision

Keep exact source, including empty input, and synchronize selection through adapters.
Render ordinary text with CodeMirror [mark and replacement decorations](https://codemirror.net/docs/ref/#view.Decoration), using the [Lezer Markdown parser](https://code.haverbeke.berlin/lezer/markdown) for syntax boundaries.
Reveal delimiters around the selected inline expression while keeping neighboring expressions styled.
Keep heading and list geometry independent of selection and asynchronous server previews.
Render [Markdown containers](https://spec.commonmark.org/0.31.2/#container-blocks) with line decorations and fixed-width marker widgets.
Reveal a container marker when the selection touches it, and keep code literal.
Use sanitized server widgets for attachments and complex interactive content.
Keep [input instructions](https://www.w3.org/WAI/tutorials/forms/instructions/) outside saved text.

## Alternatives

Whole-block HTML changes editing geometry.
A rich text model requires serialization and changes the existing source and recovery contract.

## Consequences

Source ranges need tests for nesting, escapes, Unicode, and incomplete input.
The server remains authoritative for safe links and attachments.
Reconsider if text decorations can't preserve input, media behavior, or layout.

## Confirmation

Check container rendering, stable geometry, selection, composition, keyboard input, and uploads in browsers.
Check input and decoration assertions with generated cases, coverage, and mutations.
