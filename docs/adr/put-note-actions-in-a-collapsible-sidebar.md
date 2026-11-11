---
status: proposed
date: 2026-11-10
---
# Put note actions in a collapsible sidebar

## Context

The owner requests a sidebar instead of scattered top-bar actions.
Writing remains the primary task.

## Decision

Group creation, exploration, Trash, and current-note actions in one labeled sidebar.
Use a dismissible drawer on every screen.
Start closed so the initial page remains a writing surface.
Follow [W3C disclosure navigation](https://www.w3.org/WAI/ARIA/apg/patterns/disclosure/examples/disclosure-navigation/) with native links and buttons, an expanded state, and Escape dismissal.
Use the [HTML popover contract](https://html.spec.whatwg.org/multipage/popover.html#the-popover-attribute) for native positioning and dismissal.
Keep icons, fixed reading width, and a visible close control.

## Alternatives

The More dialog requires another navigation layer.
A permanent sidebar reduces narrow writing space.
A modal desktop drawer blocks writing while navigation is visible.

## Consequences

Opening navigation overlays part of the paper.
Closing the drawer preserves the draft and reading position.
Navigation needs no saved preferences or responsive layout state.
Reconsider grouping if more destinations appear.

## Confirmation

Verify keyboard return, unchanged drafts and scroll, page transitions, icons, and first-use writing across browser profiles.
Check [320-pixel reflow](https://www.w3.org/WAI/WCAG22/Understanding/reflow.html), enlarged text, spacing overrides, and unobscured focus.
