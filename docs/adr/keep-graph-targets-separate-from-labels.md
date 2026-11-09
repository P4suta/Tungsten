---
status: proposed
date: 2026-11-09
---
# Keep graph targets separate from labels

## Context

The generated ten thousand page notebook exposes overlapping controls in the [linked graph](navigate-notes-through-their-links.md).
Fitting long button labels into the viewport prevents ordinary pointer selection.

## Decision

Keep each graph node's icon button at the owner's 48 CSS pixel target size.
Render its title as a separate visual label that never intercepts pointer events.
Show labels when space permits, with the full `accessible name`, native title, and selected-note heading available.
Bound the visible neighborhood by viewport dimensions, up to 120 nodes.
Show direct links before distant high-degree nodes.
Preserve search, pan, zoom, keyboard operation, and draggable nodes.
Use native [button semantics](https://www.w3.org/WAI/ARIA/apg/patterns/button/) and [target spacing](https://www.w3.org/WAI/WCAG22/Understanding/target-size-minimum.html).

## Alternatives

Shrinking text and controls would sacrifice readable names and target size.
Keeping every label visible preserves clutter and blocked pointer selection.

## Consequences

Some titles require selection or zooming.
Search reaches notes beyond the visible neighborhood.
Reconsider the layout when dense neighborhoods remain difficult to explore.

## Confirmation

Exercise ordinary pointer selection, search, zoom, keyboard navigation, and narrow screens with the actual generated collection.
