---
status: proposed
date: 2026-11-13
---
# Use WinUI with durable drafts for the Windows client

## Context

Windows 11 needs a native C# client preserving the server contract and unsent writing.

## Decision

Keep save transitions independent of WinUI.
Persist server-scoped drafts atomically before requests, navigation, and closing.
Send `If-Match`, keep pending writes, and stop autosave after conflicts or uncertain delivery.
Acknowledgements preserve later input.
Redirect duplicate activation before opening another editor.

Use `CommunityToolkit.Mvvm`, standard controls, system themes, and window-level Mica.
The platform design takes precedence over shared monochrome defaults.
Follow Microsoft's [list/details pattern](https://learn.microsoft.com/en-us/windows/apps/develop/ui/controls/list-details).
The [Windows guide](../windows-apps.md) records platform guidance and verification.

Isolate Unix storage tooling from portable Windows commands.
Pin dependencies and build unsigned self-contained MSIX candidates.

## Alternatives

The browser misses the requested native design.
Sharing Rust decisions adds native interop and distribution costs.

## Consequences

Windows needs native verification.
Reconsider model sharing if client behavior diverges.

## Confirmation

Require failure tests, server contracts, source fixtures, both architecture builds, accessibility, input, update retention, and affected project gates.
Remain proposed while material findings persist.
