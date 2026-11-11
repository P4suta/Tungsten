---
status: accepted
date: 2026-11-11
---
# Use native Apple clients with the shared note server

## Context

The owner requests native macOS and iOS apps with the shared server.
Preserve saved Markdown, stable note identities, conditional writes, and recovery.

## Decision

Build SwiftUI clients for both platforms with the Xcode 27.2 toolchain.
Target 27.0 to run package tests on the [hosted Xcode 27 image](https://github.com/actions/runner-images/blob/main/images/macos/xcode-27-arm64-Readme.md) with its macOS 27.0.1 runtime.
Use [NavigationSplitView](https://developer.apple.com/documentation/swiftui/navigationsplitview), standard lists, menus, sheets, and [native text editing](https://developer.apple.com/design/human-interface-guidelines/text-views).
Let system components adopt [Liquid Glass](https://developer.apple.com/documentation/technologyoverviews/adopting-liquid-glass), appearance, accent colors, and accessibility settings.
Store drafts by server and note before network writes.
Keep acknowledged revisions and stop automatic writes on conflicts.
Require HTTPS outside development loopback connections.

## Alternatives

A browser wrapper retains the interface the owner wants replaced.
Device-local databases introduce a synchronization protocol and replace the chosen shared server.
Keep the browser client available during adoption.

## Consequences

Native system editing replaces browser-specific editor gestures.
The server remains necessary for shared reads and writes.
Offline collections require a separate synchronization decision.

## Confirmation

Test API compatibility, recovery, concurrency, conflicts, and failures.
Build both apps and inspect native behavior and accessibility.
Keep native E2E optional.
Use noninteractive Apple CI and candidate checks.
Run the project gate.
