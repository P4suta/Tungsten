# Windows app

Tungsten uses C# and WinUI 3 on Windows 11 for x64 and ARM64 processors.
The interface starts in English.
The [Windows client decision](adr/use-winui-and-durable-drafts-for-the-windows-client.md) records the architecture.
The Windows interface follows Microsoft's platform guidance independently of the other clients.
The project adopts applicable recommendations as policy without changing their original normative status.

## Native interface

[NavigationView](https://learn.microsoft.com/en-us/windows/apps/develop/ui/controls/navigationview) presents All Notes, Drafts, Trash, and workspaces.
The note list and editor follow the [list/details pattern](https://learn.microsoft.com/en-us/windows/apps/develop/ui/controls/list-details).
An available content width of 640 effective pixels or less switches between list and details.
The back action preserves the editor instance, selection, and scroll position.
Wide layouts display both panes.
The CommandBar remains reachable in either layout and uses standard overflow behavior.
Note creation occupies the list header, and note actions share one adaptive editor command bar.
The header insets reserve space for native back and pane buttons in minimal navigation.
The editor displays each note's heading once and gives the writing area a bounded width.
The [platform designs](interface-design.md) provide editable wide, narrow, recovery, and appearance references.

The connection form appears on first use and through the standard Settings entry.
Opening Settings checkpoints the draft.
Cancel restores the selected collection, editor, and original server address.
Connecting to the current server refreshes its contents while preserving the editor.

Markdown uses one standard [RichEditBox](https://learn.microsoft.com/en-us/windows/apps/develop/ui/controls/rich-edit-box) for Live Preview.
The [live editing decision](adr/edit-markdown-in-a-native-live-preview.md) records source and history management.
Headings, emphasis, strikethrough, code, links, lists, and quotes receive native text formatting.
The active block reveals its syntax.
Inactive blocks hide parsed delimiters without removing source characters.
Images, tables, and HTML remain readable Markdown source.
The app never evaluates HTML or loads remote images for rendering.
The native control supplies text selection and IME composition.
Clipboard commands copy Markdown source and paste Unicode text through native ranges.
Paste reads clipboard text asynchronously and verifies the editor and selection before insertion, following the [RichEditBox paste event contract](https://learn.microsoft.com/en-us/windows/windows-app-sdk/api/winrt/microsoft.ui.xaml.controls.richeditbox.paste?view=windows-app-sdk-2.0).
Bold, Italic, and Inline code insert Markdown syntax through standard CommandBar actions.
The core keeps bounded source-edit history because native RichEditBox history also records presentation formatting.
Undo and Redo restore source and selection independently of presentation changes.
The app suspends sending, remote replacement, navigation, and closing during composition.
AutoSuggestBox submits search queries.
Note details use standard lists for outgoing links, backlinks, original attachments, and saved versions.
Link and attachment rows display Markdown labels when available and keep their validated targets for navigation.
History restoration replaces text through the editor selection adapter so Undo remains available.

The operating system provides the title bar, fonts, icons, accent color, Light, Dark, and Contrast themes.
The standard title bar follows the Windows app mode through [PreferredTheme](https://learn.microsoft.com/en-us/windows/windows-app-sdk/api/winrt/microsoft.ui.windowing.appwindowtitlebar.preferredtheme?view=windows-app-sdk-2.0) instead of legacy title bar theming.
Brushes use [system color resources](https://learn.microsoft.com/en-us/windows/apps/design/signature-experiences/color).
[Mica](https://learn.microsoft.com/en-us/windows/apps/develop/ui/system-backdrops) belongs to the window backdrop, while the editor uses an ordinary system surface.
Shared monochrome styling doesn't apply to the Windows client.
The source gate rejects browser surfaces, authored colors and fonts, text scaling overrides, and forced themes.
Rejecting fixtures exercise the gate alongside native control and metadata checks.
Shell labels, dialogs, and command names use English `.resw` resources through `x:Uid` or `ResourceLoader`, following [Windows App SDK localization guidance](https://learn.microsoft.com/en-us/windows/apps/windows-app-sdk/mrtcore/localize-strings).
Code references segmented property resources with slash-separated identifiers, and the resource contract rejects dotted lookup paths.
The core remains independent of WinUI and provides English validation and recovery messages.

[InfoBar](https://learn.microsoft.com/en-us/windows/apps/develop/ui/controls/infobar) reports save failures and conflicts without dismissing input.
[ContentDialog](https://learn.microsoft.com/en-us/windows/apps/develop/ui/controls/dialogs-and-flyouts/dialogs) confirms destructive operations.
Original files use the Windows App SDK [open picker](https://learn.microsoft.com/en-us/windows/apps/develop/files/using-file-folder-pickers) and [save picker](https://learn.microsoft.com/en-us/windows/apps/develop/files/pickers-save-file), associated with the current AppWindow.
Opening an original delegates to the registered Windows app.

## Durable writing

The WinUI-independent core owns immutable save decisions, validated HTTP responses, and draft recovery.
The HTTP adapter preserves the existing API and pagination.
Release connections require an HTTPS origin without credentials, paths, queries, or fragments.
Debug builds also accept loopback HTTP for isolated server tests.
Updates send the last observed revision in `If-Match`.
Notes keep the 1 MiB server limit and original attachments keep the 64 MiB limit.
Editor input exceeding the server limit stays in the local draft.
Validation rejects the excess input before transmission.
The store accepts local text up to 8 MiB and accounts for JSON escaping in its file size limit.

Draft identities include both the canonical server origin and note name.
Files use a SHA-256-derived name under the package's LocalState directory.
Writes flush a private temporary file before atomic replacement.
Pending request data is durable before sending.
Acknowledgements advance the saved base without replacing input entered during the request.
Navigation and closing checkpoint current input independently of an outstanding network request.
The autosave timer checkpoints later input even while another action waits for the server.
A storage failure keeps the editor open and blocks navigation and closing.
The store quarantines damaged files with their original bytes, and valid drafts remain recoverable.
After damage or loss, the store reopens a valid replacement backup and blocks automatic sending.
Selecting another note checkpoints input again after storing the destination draft.
Discarding checks for input received after confirmation and preserves any later writing.

Conflicts, deletion, and uncertain delivery stop automatic sending.
Retry reads the server before deciding whether a pending write arrived.
Save as new note keeps the original draft and creates a distinct identity.
Saved notes refresh when the window activates and every fifteen seconds.
Remote updates replace clean text and preserve dirty text as a conflict.
Connection changes checkpoint the previous server's draft before switching clients.
Workspace operations preserve the active editor and keep updated workspace revisions.
The [AppInstance lifecycle API](https://learn.microsoft.com/en-us/windows/apps/windows-app-sdk/applifecycle/applifecycle-instancing) redirects duplicate activation to the existing window before another editor can write drafts.

## Keyboard and accessibility

Control+N creates a note, Control+S saves, and Control+F focuses search.
On narrow windows, Control+F reveals the note list after checkpointing the current draft.
Control+B and Control+I insert Markdown emphasis.
Control+Z undoes a source edit.
Control+Y or Control+Shift+Z redoes it.
F6 moves between collections, the note list, and the editor.
Control with the numeric plus or minus key changes editor zoom, and Control+0 restores the system default.
CommandBar actions also expose zoom from the default size through 200 percent.
Native list item names expose titles, and status messages use polite live announcements.
Keyboard and focus behavior follow [keyboard accessibility guidance](https://learn.microsoft.com/en-us/windows/apps/design/accessibility/keyboard-accessibility).

The acceptance pass follows Microsoft's [accessibility checklist](https://learn.microsoft.com/en-us/windows/apps/design/accessibility/accessibility-checklist) and [text scaling guidance](https://learn.microsoft.com/en-us/windows/apps/develop/input/text-scaling).
Verify Light, Dark, Contrast themes, 225 percent system text scaling, narrow windows, Japanese IME, Narrator, and keyboard-only operation on Windows.
Check complete labels, reachable actions, focus return, source selection, Undo and Redo, draft recovery, and absence of lost input.
Automated source and UI checks complement native assistive technology and input testing.
Hands-on checks of themes, text scaling, Japanese IME, and Narrator remain pending for the acceptance pass after implementation.

## Development and checks

The repository pins `.NET SDK` 10.0.404, Windows App SDK 2.6.5, and WinApp CLI 0.7.1.
Install project tools with `mise install` and enable Windows Developer Mode for development package deployment.
NuGet lockfiles cover portable tests and builds for each architecture.
The app declares both supported runtime identifiers so builds and package publishing restore a consistent graph.
Architecture restores use Release configuration with matching `Platform`, `RuntimeIdentifier`, and `NuGetLockFilePath` values.
The maintained build, run, and bundle commands supply matching properties and locked restore mode.
Run and bundle build with locked dependencies before invoking WinApp with `--no-restore`.
Use `mise x -- just windows source` on every host.
The pre-commit hook includes Windows source checks, and `mise x -- just check` selects the Windows gate on Windows.
Use `mise x -- just windows check` on Windows for source checks, formatting, nullable and analyzer builds, unit tests, real-server contracts, vulnerability checks, and documentation checks.
Docker Desktop must provide Linux containers for the Windows host's real-server contract command.
The contract image has explicit CPU, memory, swap, and process limits.
On macOS and Linux, `mise x -- just windows contracts` builds the existing server and runs the C# HTTP contract directly.

`mise x -- just windows check --native-only` runs native checks without Docker or documentation tooling.
Hosted CI runs documentation and real-server contracts in Linux jobs and joins both Windows jobs to the existing required summary.
`mise x -- just windows ui` exercises native controls, `accessible name`s, connection errors, writing, and local draft navigation.
The UI workflow uses the explicit `--ui-test` launch flag and a separate LocalState subdirectory.
It refuses to start while an existing Tungsten window is open, captures a screenshot, and closes its own window.
Duplicate activation must preserve one window, and recovery must reopen a native list item rather than match an editor heading.
The UI workflow checks live heading formatting, source Undo and Redo, clipboard round trips, restored selection after formatting, F6 region movement, and narrow-window navigation.
It also checks zoom shortcuts and a native workspace dialog, then redeploys the development package and compares the retained connection and complete unsent Markdown source.
Failure tests precede implementation and cover pending input, restarts, conflicts, lost responses, storage failure, corrupt files, and server changes.

## Running and distribution

Use `mise x -- just windows run` to build and launch the development package.
Use `mise x -- just windows build --arch x64` or `mise x -- just windows build --arch arm64` for a Release build.
Use `mise x -- just windows bundle --arch x64` and `mise x -- just windows bundle --arch arm64` for unsigned candidates under `dist/windows`.
Each MSIX includes the .NET and Windows App SDK runtimes.
The candidate workflow creates both architectures without publishing them.
Production signing, public distribution, and Microsoft Store submission remain separate operations.

The package identity stays `P4suta.Tungsten` with publisher `CN=P4suta` across updates.
Development redeployment preserves app data unless explicitly cleaned.
Verify a stored connection and unsent draft after redeployment before accepting an update candidate.
Don't use WinApp's `--clean` option for retention checks.
