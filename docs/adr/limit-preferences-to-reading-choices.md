---
status: proposed
date: 2026-11-08
---
# Limit preferences to reading choices

## Context

Writing must work without choosing fonts, spacing, completion, or file organization.
The expanded controls in [portable workflows](extend-files-with-portable-markdown-workflows.md) expose unwanted tasks.

## Decision

Keep two saved appearance choices: reading or full width, and five named text sizes.
Use automatic formatting, completion, spellcheck, and centered typing with fixed defaults.
Keep source and reading views in More, outside appearance settings.
Remove file management, import, individual export, printing, duplication, find and replace, and note details with their handlers.
Show text counts before each nonempty note's title.
Use heading disclosures and an on-demand relation diagram instead of outline and connection tools.
Follow [disclosure semantics](https://www.w3.org/WAI/ARIA/apg/patterns/disclosure/) and [modal focus behavior](https://www.w3.org/WAI/ARIA/apg/patterns/dialog-modal/).

## Alternatives

Hiding advanced controls preserves their maintenance cost and configuration states.
Removing every view would also remove direct Markdown editing.

## Consequences

Copy and paste remain editor operations.
Complete archive export remains available.
Reconsider controls when a concrete writing task requires them.

## Confirmation

Check preset persistence, retired controls, folding, relation navigation, clipboard source, reflow, and keyboard access.
