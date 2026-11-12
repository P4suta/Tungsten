---
status: proposed
date: 2026-11-09
---
# Keep the editor small and load tools on demand

## Context

The requested interface provides responsive writing, readable graphs, dedicated Trash, and useful recovery.
The requirements exclude [settings](limit-preferences-to-reading-choices.md), counts, separate relations, and [exports](stream-complete-exports-with-bounded-memory.md).

## Decision

Use browser zoom and one bounded reading measure.
Keep note exploration and relations in one graph.
Use [D3 simulation](https://d3js.org/d3-force/simulation) for movement and collision.
Stop motion after settling, closing, or hiding.
Respect reduced motion and keep keyboard alternatives.
Show complete selected titles without ellipsis.

Give Trash selectable rows, restoration, deletion, and browser navigation.
Bound recovery by age and count.
Preserve the start of an editing session and checkpoint larger structural changes.
Show differences before restoration.

Split and minify browser assets with [esbuild](https://esbuild.github.io/api/#splitting).
Load secondary tools when needed.
Cache fingerprinted assets and compress text responses.
Gate initial bytes, delayed tools, interaction latency, and [Lighthouse metrics](https://developer.chrome.com/docs/lighthouse/performance/performance-scoring).

## Alternatives

Loading every tool at startup delays writing.
Unexplained snapshots obscure their restoration effect.

## Consequences

Browser zoom changes the entire interface.
Laboratory budgets cover measured fixtures.
Recovery can't replace filesystem backups.

## Confirmation

Check keyboard access, enlarged text, complete labels, icon bounds, recovery sessions, differences, and resource budgets.
