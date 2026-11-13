---
status: proposed
date: 2026-11-09
---
# Navigate notes through their links

## Context

The requested navigation replaces the collection navigation in [writing and navigation](keep-writing-and-navigation-discoverable.md) with topic exploration.
An index note linking everything would add connections without subject meaning.

## Decision

Home opens an interactive graph of stored Markdown links.
Select a note to explore neighbors or open it.
Search still covers the full collection, including disconnected notes.
Use bounded deterministic [D3 force](https://d3js.org/d3-force/simulation) layout, draggable nodes, pan, zoom, and keyboard controls.
Keep note controls at their normal text and target sizes while zoom changes positions.
Bound each graph view to 120 notes and disclose the count.
New notes start with an editable timestamp heading and a separate stable storage identity.
Creating a note from the graph links it to the selected topic.
Don't create index notes or synthetic edges.

## Alternatives

A complete list requires recognizing a title among unrelated notes.
An automatic root note creates a large hub without explaining relationships.

## Consequences

Existing links determine the useful structure.
Search reaches notes outside the visible graph.
Reconsider the bound for larger collections.

## Confirmation

Check topic exploration, search, disconnected notes, drag, zoom, keyboard access, creation, and narrow layouts.
