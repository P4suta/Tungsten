---
status: proposed
date: 2026-11-12
---
# Use native Linux editing with durable drafts

## Context

The owner requests GTK4, libadwaita, shared-server access, and x86_64 and aarch64 Flatpak bundles.

## Decision

Keep save decisions in a dependency-free `no_std` model with HTTP, storage, and GTK adapters.
Reuse `NoteName`, `Revision`, and the server API.
Persist server-scoped drafts before transmission, reject stale responses, and stop autosave after uncertain delivery or conflicts.

Adopt applicable [Human Interface Guidelines](https://developer.gnome.org/hig/) recommendations as owner policy.
Use native navigation and editing with system appearance, fonts, accessibility, and icons, overriding monochrome defaults.
The [Linux app guide](../linux-apps.md) records applicable sources and verification.

Use runtime `org.gnome.Platform//51`, pinned Rust, and vendored offline builds.
Follow [Flatpak conventions](https://docs.flatpak.org/en/latest/conventions.html) and [sandbox permissions](https://docs.flatpak.org/en/latest/sandbox-permissions.html).

## Alternatives

The browser client avoids another UI implementation but doesn't provide the requested GTK interface.
Complete offline caching adds synchronization and migration work.

## Consequences

The `desktop` feature isolates GTK from macOS checks.
GTK requires separate Linux verification.
Reconsider synchronization for complete offline collections.

## Confirmation

Require model, server, native, Flatpak, and project checks.
Extend release proofs and mutation tests.
Keep proposed until native and distribution checks pass.
