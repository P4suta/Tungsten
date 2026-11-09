---
status: proposed
date: 2026-11-08
---
# Keep writing and navigation discoverable

## Context

The [primary note page](keep-the-note-as-the-primary-page.md) needs a writing cue, comfortable cursor placement, and familiar routes to secondary tasks.

## Decision

Show a Markdown example in empty notes without saving example content.
Keep the editor label and [associated instructions](https://www.w3.org/WAI/tutorials/forms/instructions/).
Center the active line while typing, with an option to turn centering off.
Offer slash commands through a keyboard-operated [suggestion list](https://www.w3.org/WAI/ARIA/apg/patterns/listbox/).
Use dedicated settings and file pages with browser history and direct return to the draft.
Use folder navigation, [breadcrumbs](https://www.w3.org/WAI/ARIA/apg/patterns/breadcrumb/), and a file table.
Place [named close icons](https://www.w3.org/WAI/WCAG22/Techniques/aria/ARIA14#example-1-a-close-button-in-a-dialog) at the upper right of dialogs.
Apply [familiar conventions and recognition](https://www.nngroup.com/articles/ten-usability-heuristics/) throughout the affected journeys.

## Alternatives

Nested dialogs need fewer routes but obscure location and return paths.
A rich text engine offers inline rendering but expands the plain Markdown editing contract.

## Consequences

Routing and cursor measurement need browser checks.
Manual scrolling remains available between edits.
Reconsider the editor when inline rendered editing becomes an owner request.

## Confirmation

Check empty guidance, wrapped-line centering, composition, slash cancellation, uploads, history navigation, file operations, and close placement.
Review desktop and narrow screenshots alongside the project gate.
