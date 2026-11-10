---
status: proposed
date: 2026-11-10
---
# Dock navigation and note context beside the paper

## Context

The owner requests persistent sidebars and a stationary writing page.
The drawer proposal in [the previous decision](put-note-actions-in-a-collapsible-sidebar.md) doesn't meet that interaction.

## Decision

Anchor a [CSS grid](https://www.w3.org/TR/css-grid-2/#grid-containers) to the [layout viewport](https://www.w3.org/TR/css-position-3/#fixed-pos) with separate [scroll containers](https://www.w3.org/TR/css-overflow-3/#scroll-container) for navigation, writing, and context.
Reserve equal sidebar tracks on wide screens, including collapsed states, using [explicit track sizing](https://www.w3.org/TR/css-grid-2/#track-sizing).
Keep navigation open by default and context independently collapsible.
Keep note browsing, workspace membership, and Trash inside navigation.
Keep connections and version recovery inside note context.
Keep the graph explorer separate.
Use native links and [buttons](https://www.w3.org/WAI/ARIA/apg/patterns/button/) with explicit names and states.
Narrow screens show one pane at a time to preserve [reflow](https://www.w3.org/WAI/WCAG22/Understanding/reflow.html).

## Alternatives

Visibility-dependent tracks reclaim space but move or rewrap writing.
Overlays can obscure writing when margins are insufficient.
Window-scrolling sidebars complicate independent reading positions.
Keeping three columns on narrow screens leaves insufficient writing space.

## Consequences

Typing, anchors, and restoration use the writing scroll container.
Collapsed sidebars leave whitespace.
Reconsider the pane model if simultaneous narrow-screen editing becomes necessary.

## Confirmation

Check stationary paper bounds, width, selection, scroll, note transitions, centered typing, recovery, keyboard return, icons, enlarged text, and browser profiles.
