---
status: proposed
date: 2026-11-08
---
# Render local files inside notes

## Context

Personal notes must display recordings and documents without leaving the editor or changing original files.
Native PDF plug-ins vary between browsers, and attachment names must remain editable without breaking note links.

## Decision

Use [HTML media controls](https://html.spec.whatwg.org/multipage/media.html#media-elements) for recordings and bundle [PDF.js](https://mozilla.github.io/pdf.js/examples/) for page rendering and extracted text.
Serve recognized content types and [byte ranges](https://www.rfc-editor.org/rfc/rfc9110.html#name-range-requests) from the existing attachment identifiers.
Keep original bytes in `.attachments` and store names, logical folders, and descriptions in compatible JSON sidecars.
Keep appearance and optional identifier-based note creation in validated browser preferences.

## Alternatives

Native PDF objects avoid a dependency but delegate page rendering to an optional browser capability.
Moving files into named directories makes disk browsing simpler but requires indirection to preserve existing links.

## Consequences

The executable grows to include the PDF parser, worker, fonts, and character maps.
Logical folder changes preserve attachment addresses and backups include their metadata.
Reconsider storage paths when direct filesystem organization becomes a required interface.

## Confirmation

Check image decoding, audio playback, PDF pixels and text, ranges, original downloads, stale edits, and archive compatibility.
Check [dialog focus](https://www.w3.org/WAI/ARIA/apg/patterns/dialog-modal/), preference recovery, narrow layouts, and connected-note navigation in the browser suite.
