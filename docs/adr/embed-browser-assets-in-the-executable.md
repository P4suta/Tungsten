---
status: accepted
date: 2026-11-08
---
# Embed browser assets in the executable

## Status

Accepted in the implementation plan.

## Context

The service needs Linux executable and container distribution with few runtime requirements.

## Decision

Use Rust and axum for HTTP and embed compiled TypeScript, HTML, and CSS in the executable.
Use native HTML controls without a UI framework.
Bind to localhost by default and use Tailscale Serve for private HTTPS access.
The app has no accounts or authentication middleware.

## Consequences

The installed service needs no JavaScript runtime or separate asset directory.
Building from source needs the pinned Rust and Node.js tools.
Access depends on the existing tailnet policy.
Native textarea semantics follow the [HTML Standard](https://html.spec.whatwg.org/multipage/form-elements.html#the-textarea-element).
Labels, keyboard access, reflow, and status announcements follow the [Web Content Accessibility Guidelines](https://www.w3.org/TR/WCAG22/).
Black and white styling and 48 CSS pixel controls are owner policy.

## Alternatives

A separate frontend would require another deployed service.
A rich text editor would add behavior outside the plain text workflow.

## Confirmation

The built executable must serve its assets without a separate asset directory.
Browser tests must check desktop and mobile layouts, keyboard access, status messages, and the palette.
