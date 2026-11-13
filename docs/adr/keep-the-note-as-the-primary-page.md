---
status: proposed
date: 2026-11-08
---
# Keep the note as the primary page

## Context

The requested writing surface starts blank while preserving files and recovery.

## Decision

Keep New note, Notes, and More beside a borderless Markdown editor.
Use the source heading as the readable title and keep filenames stable after creation.
Use [native dialogs](https://html.spec.whatwg.org/multipage/interactive-elements.html#the-dialog-element) and [defined focus behavior](https://www.w3.org/WAI/ARIA/apg/patterns/dialog-modal/) for navigation and files.
Put view selection and typography in saved preferences.
Keep the editor's [hidden label](https://www.w3.org/WAI/tutorials/forms/labels/#hiding-label-text) without visible placeholder text or empty metadata.
Use cancellable [input events](https://www.w3.org/TR/input-events-2/) for paired delimiters and selection edits outside composition.
Input Events remains a Working Draft, so browser tests govern supported behavior.

## Alternatives

A persistent workspace exposes every tool but displaces the writing surface.
A rich text editor adds an editing model beyond Markdown source.

## Consequences

Secondary operations need an explicit dialog or keyboard shortcut.
Empty notes fit narrow layouts and enlarged text without scrolling.
Reconsider the interaction model when inline rendered editing becomes an explicit request.

## Confirmation

Check empty geometry, heading labels, input completion, undo, composition, recovery, and attachment playback.
Check modal focus, stored preferences, and [reflow](https://www.w3.org/WAI/WCAG22/Understanding/reflow.html) in the maintained browser suite.
