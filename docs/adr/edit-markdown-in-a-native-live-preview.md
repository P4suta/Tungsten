---
status: proposed
date: 2026-11-13
---
# Edit Markdown in a native live preview

## Context

The Windows client needs one live editing surface with formatted Markdown, source preservation, and native input behavior.

## Decision

Use [RichEditBox](https://learn.microsoft.com/en-us/windows/apps/develop/ui/controls/rich-edit-box) with a source-based Markdown projection.
Reveal syntax in the active block and format inactive blocks in place.
Save Markdown source and format native ranges.
Use parser spans and the documented [text object model](https://learn.microsoft.com/en-us/windows/windows-app-sdk/api/winrt/microsoft.ui.text.richedittextdocument).
Defer presentation updates during IME composition.
Keep native input and selection, with source-preserving clipboard text.
Maintain bounded source-edit history for Undo/Redo and selection restoration.
RichEditBox records formatting in its native history.
Its public projection lacks undo suspension.
Provide Markdown formatting commands as recommended for rich editing.

## Alternatives

A plain TextBox has no inline rendering.
A browser editor abandons native controls.
Separate editing and reading panes add a reading mode.
Native history mixes presentation and writing.

## Consequences

Projection must preserve source offsets and system appearance.
Unsupported Markdown remains readable source.
Reconsider history management if WinUI exposes presentation-only formatting.

## Confirmation

Require source round trips, projection failure cases, native formatting, selection, clipboard, Undo/Redo, persistence, and both architecture builds.
Hands-on IME and Narrator checks remain pending.
