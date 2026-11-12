# Linux app

The native client uses Rust, GTK4, and libadwaita with the existing shared server.
The default language is English.
Run `mise x -- just linux check` on Linux for model, adapter, source, desktop, and metadata checks.
Run `mise x -- just linux build` and `mise x -- just linux run` to build and start the desktop app.
The `desktop` feature keeps GTK dependencies out of the default workspace checks on macOS.

## Writing and recovery

Connect using an HTTPS origin without a path, credentials, query, or fragment.
Debug builds also accept loopback HTTP addresses.
The client refuses redirects and bounds response sizes.
Notes use the server's validated `NoteName` and `Revision` types.
The editor displays Markdown source through [GtkTextView](https://docs.gtk.org/gtk4/class.TextView.html) and [GtkTextBuffer](https://docs.gtk.org/gtk4/class.TextBuffer.html), preserving system input methods, selection, clipboard, and Undo and Redo.

The app stores drafts under `XDG_DATA_HOME` and separates records by canonical server origin and note identity.
The storage adapter synchronizes private temporary files and atomically renames them before transmission.
The outgoing body and acknowledged revision remain recoverable until the adapter records the response.
Writes send `If-Match` for existing notes.
An acknowledgement advances the revision without replacing text typed during the request.
Conflicts, remote deletion, and uncertain delivery stop autosave.
Retry compares the saved remote text with the pending write before resuming.
Reload keeps local writing recoverable, and Save as New Note creates a separate identity.
Damaged draft files remain on disk while other valid drafts recover.
Local persistence failure prevents leaving the editor or closing the window.

All Notes and Drafts support search.
Workspaces and their notes use bounded server pages.
Note Details exposes outgoing links, backlinks, original attachments, and saved versions.
History restoration requires the same editor text and generation observed before loading the version.
Trash previews deleted text and restores a separate note without replacing a colliding name.
File selection and opening originals use GTK's [FileDialog](https://docs.gtk.org/gtk4/class.FileDialog.html) and [FileLauncher](https://docs.gtk.org/gtk4/class.FileLauncher.html), which use desktop portals inside Flatpak.

## System design policy

Personal policy requires applicable [Human Interface Guidelines](https://developer.gnome.org/hig/) recommendations.
The source documents remain recommendations, rather than specifications imposing universal requirements.
Native system design takes precedence over shared monochrome styling and icon preferences in the Linux UI.
The [UI styling guidance](https://developer.gnome.org/hig/guidelines/ui-styling.html) governs system Light and Dark appearance.
The app leaves accent colors and High Contrast to libadwaita.
The [typography guidance](https://developer.gnome.org/hig/guidelines/typography.html) requires system fonts and relative system styles.
The source gate rejects app style sheets, fixed font sizes, custom materials, and browser wrappers.

[NavigationSplitView](https://gnome.pages.gitlab.gnome.org/libadwaita/doc/main/class.NavigationSplitView.html) and [Sidebar](https://gnome.pages.gitlab.gnome.org/libadwaita/doc/main/class.Sidebar.html) organize Collections, notes, and writing.
The [adaptive guidance](https://developer.gnome.org/hig/guidelines/adaptive.html) applies to narrow windows, large text, and changing display scale.
Split views collapse into pages with visible back controls.
[HeaderBar](https://gnome.pages.gitlab.gnome.org/libadwaita/doc/main/class.HeaderBar.html) supplies back controls for the enclosing navigation views.
Each narrow breakpoint sets both split views because libadwaita applies only the last matching [Breakpoint](https://gnome.pages.gitlab.gnome.org/libadwaita/doc/main/class.Breakpoint.html).
[UI icon guidance](https://developer.gnome.org/hig/guidelines/ui-icons.html) governs standard symbolic icons and the choice between a label and an icon.
Collection rows combine icons and labels.
Header controls use recognizable icons and a descriptive `accessible name`.
[Dialog guidance](https://developer.gnome.org/hig/patterns/feedback/dialogs.html) governs explicit cancellation, safe defaults, and destructive confirmations.

The [accessibility guidance](https://developer.gnome.org/hig/guidelines/accessibility.html) applies to names, keyboard access, visible focus, High Contrast, and enlarged text.
Every interactive element uses standard GTK semantics.
Use keyboard navigation, a screen reader, the screen keyboard, and Japanese input in isolated `GNOME` and `KDE Plasma` sessions.
Inspect Light, Dark, High Contrast, enlarged text, and narrow windows for clipping and unreachable actions.
Automated checks establish their individual assertions and don't establish screen reader or input-method behavior without exercising the actual software.

## Flatpak

The app ID is `io.github.p4suta.tungsten`.
The desktop entry, AppStream metadata, and app icon share the ID under [Flatpak conventions](https://docs.flatpak.org/en/latest/conventions.html).
The runtime and SDK use branch `51` of the `GNOME` platform.
The build uses the Rust version from `mise.toml` and Cargo dependencies from the committed lockfile.
`mise x -- just linux bundle --arch aarch64` and the equivalent `--arch x86_64` prefetch sources, vendor dependencies, and build offline in the SDK.
The command writes bundles under `dist/linux` as local installation candidates.
Install the matching bundle with `flatpak --user install dist/linux/io.github.p4suta.tungsten-aarch64.flatpak`, substituting `x86_64` when needed.
Launch it with `flatpak run io.github.p4suta.tungsten`.
Build prerequisites include Flatpak, flatpak-builder, OSTree, Xvfb, D-Bus, and the selected architecture's `org.gnome.Platform//51` and `org.gnome.Sdk//51`.
The bundle contains project licenses and vendored dependency notices.
The generated Rust toolchain module disables debug extraction to preserve the prebuilt compiler's Executable and Linkable Format (ELF) segments.
An interrupted build can resume with `--resume` and its generated directory under `target/linux`.
Resumption refreshes project sources and verifies cached Rust archive checksums.
Flathub publication requires a separate operation.

The [sandbox permission guidance](https://docs.flatpak.org/en/latest/sandbox-permissions.html) governs the manifest.
The manifest grants network, rendering, Wayland, and fallback X11 with shared interprocess communication for X11 rendering.
Don't grant host filesystem access, session bus access, or privileged devices.
Portals grant access to selected files and their default applications.
The candidate workflow builds, validates, installs, and launches both architectures.

## Verification

Deterministic model tests cover durable writes, concurrent edits, blocked autosave, stale responses, and navigation refusal.
Real-server contract tests cover conditional writes, interrupted acknowledgements, recovery, Workspaces, history, Trash, Unicode paths, and original attachment bytes.
Source checks reject prohibited styling, malformed metadata, excessive sandbox permissions, and invalid app identities with rejecting fixtures.
Native interaction checks use an isolated session.
Run one desktop session per test user to isolate portal, input method, and accessibility services.
Enable X11 compositing when inspecting transparent window shadows.
The native gate uses [libadwaita's High Contrast debug override](https://gitlab.gnome.org/GNOME/libadwaita/-/blob/1.9.4/src/adw-settings.c) in a separate process and checks the resulting system style.
History replacement uses a single reversible [TextBuffer user action](https://docs.gtk.org/gtk4/method.TextBuffer.set_text.html), while opening another note clears the prior Undo history.
Production save decisions join the existing bounded Kani inventory and release mutation gate.
Run `mise x -- just check`, the Linux checks, both Flatpak architecture checks, and affected Apple checks before implementation completion.
Full proofs and mutation testing keep the existing release timing.
