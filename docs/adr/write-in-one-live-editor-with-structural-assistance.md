---
status: proposed
date: 2026-11-09
---
# Write in one live editor with structural editing

## Context

The owner requests one writing surface without folding controls, an editor icon, view selection, or complete archive downloads.
The owner also requests strict heading ranks, code highlighting, and paired delimiter deletion.

## Decision

Keep one Live preview editor and original Markdown storage.
Remove folding, alternate views, and archive routes.
Normalize headings during automatic formatting: the first heading has rank one, later headings start at rank two, and descending ranks never skip a level.
The stricter single-title rule is owner policy guided by [W3C heading structure](https://www.w3.org/WAI/tutorials/page-structure/headings/).
Use bundled [Highlight.js grammars](https://highlightjs.readthedocs.io/en/latest/api.html) with monochrome token styles.
Parse matched Markdown delimiters before mirroring deletions.
Preserve content and unrelated symbols.

## Alternatives

Keeping view and folding preferences preserves controls the owner rejects.
Rejecting invalid headings interrupts writing.
Automatic formatting repairs their structure with Undo available.

## Consequences

Heading ranks may change after editing.
Unknown code languages stay literal.
Original attachments keep their existing links.
Reconsider editing rules when reproduction shows lost content or unstable selection.

## Confirmation

Check structure, literal code, pair deletion, Undo, composition, alignment, and attachment access across browser engines.
