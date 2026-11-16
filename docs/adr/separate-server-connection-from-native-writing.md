---
status: proposed
date: 2026-11-14
---
# Separate server connection from native writing

## Context

The Windows window keeps connection and management controls beside every note.
The controls reduce the writing area even after connecting.

## Decision

Show the connection form on first use and through the standard [NavigationView Settings entry](https://learn.microsoft.com/en-us/windows/apps/develop/ui/controls/navigationview#pane).
Checkpoint writing before opening connection settings and preserve the current note when canceling.
Keep note creation in the list header and note actions in one [adaptive command bar](https://learn.microsoft.com/en-us/windows/apps/develop/ui/controls/command-bar#commands).
Use the [list/details pattern](https://learn.microsoft.com/en-us/windows/apps/develop/ui/controls/list-details) with a back action when space requires stacked panes.
Keep system typography, appearance, selection, focus, and window controls.

The [editable platform designs](../interface-design.md) describe each client's native presentation and the browser's monochrome surface.
Keep save decisions and the server contract unchanged.

## Alternatives

The persistent server field makes server switching immediate but consumes space during writing.
A custom navigation shell permits finer placement but replaces standard adaptive and accessibility behavior.

## Consequences

Changing servers requires opening Settings.
Reconsider the connection entry point if frequent server switching becomes necessary.

## Confirmation

Check initial connection, canceling Settings, note creation, selection, editing, recovery, narrow navigation, and retained drafts.
Run source contracts, native UI checks, both architecture builds, and the project gate.
