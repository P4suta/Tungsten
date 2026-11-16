---
status: proposed
date: 2026-11-14
---
# Use searchable notes and on-demand details on the Web

## Context

The requested redesign permits retiring secondary features.
The [reserved tracks](dock-navigation-and-note-context-beside-the-paper.md) consume writing space while closed.

## Decision

Show searchable notes beside writing on wide screens and one primary pane on narrow screens.
Reclaim closed navigation space.
Keep creation, search, save status, and return visible.
Group optional workspaces after search in a native [disclosure](https://html.spec.whatwg.org/multipage/interactive-elements.html#the-details-element).
Use a native [dialog](https://html.spec.whatwg.org/multipage/interactive-elements.html#the-dialog-element) for links, history, and deletion with standard [focus behavior](https://www.w3.org/WAI/ARIA/apg/patterns/dialog-modal/).
Retire Web graph navigation and duplicate search while preserving data, editing, recovery, and server contracts.

## Alternatives

Reserved tracks preserve stationary paper but waste space.
A permanent third pane exposes details but narrows writing.
Graphs visualize relationships but duplicate linked lists.

## Consequences

Navigation can reflow writing without changing selection.
Adopt applicable [accessibility criteria](https://www.w3.org/TR/WCAG22/) and Digital Agency [typography](https://design.digital.go.jp/dads/foundations/typography/), [layout](https://design.digital.go.jp/dads/foundations/layout/), and [button](https://design.digital.go.jp/dads/components/button/usage/) recommendations as policy.
Black and white, 18-pixel text, and 48-pixel targets are personal preferences.
Reconsider graphs for demonstrated relationship-analysis needs.

## Confirmation

Check navigation, editing, recovery, focus, 320-pixel reflow, enlargement, spacing, targets, and editable Figma compositions.
