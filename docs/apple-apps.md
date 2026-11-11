# Native apps for Apple platforms

Tungsten's macOS and iOS apps connect to the same note server as the browser client.
Build with Xcode 27.2.
Target macOS 27 and iOS 27.
Local interface checks use macOS 27.2 and iOS 27.2.
Apple's system design governs the native interface instead of the browser's monochrome styling.

## Interface

Use [NavigationSplitView](https://developer.apple.com/documentation/swiftui/navigationsplitview) to present collections, notes, and editing side by side on Mac and as a navigation stack on iPhone.
Use standard lists, contextual actions, search, toolbars, sheets, and confirmations.
Use [inline toolbar titles](https://developer.apple.com/documentation/swiftui/view/toolbartitledisplaymode(_:)) for connection forms and sheets to keep their titles readable at accessibility text sizes.
Let [TextField](https://developer.apple.com/documentation/swiftui/textfield) supply its own platform label instead of adding a second labeled container.
Note Details uses the system [close role](https://developer.apple.com/documentation/swiftui/buttonrole/close) because closing the sheet doesn't commit an operation.
Confirm discarded workspace edits through standard dialogs, following Apple's [sheet guidance](https://developer.apple.com/design/human-interface-guidelines/sheets).
On iOS, the native [presentation delegate](https://developer.apple.com/documentation/uikit/uiadaptivepresentationcontrollerdelegate/presentationcontrollerdidattempttodismiss(_:)) prevents dismissal and requests confirmation for unsaved workspace edits.
Mac collections and notes use standard [list selection](https://developer.apple.com/documentation/swiftui/list), while iOS uses navigation links to move between columns.
Choosing a collection clears the previous note selection and preserves pending writing before navigation.
Keep the standard Mac title bar and menu commands.
The editor uses [TextEditor](https://developer.apple.com/documentation/swiftui/texteditor) for original Markdown, native selection, composition, dictation, spelling, and Undo.
Open links, original attachments, history, and Trash through native lists and system previews.
The interface language is English.
On Mac, View > Zoom In enlarges reading text up to 200%, and View > Actual Size restores the system body size.
On iPhone and iPad, text follows Dynamic Type.

Follow Apple's [Liquid Glass adoption guide](https://developer.apple.com/documentation/technologyoverviews/adopting-liquid-glass) by leaving toolbar and sheet backgrounds to system components.
Use semantic system colors, system text styles, and the default accent.
Follow [accessibility guidance](https://developer.apple.com/design/human-interface-guidelines/accessibility) for labels, text enlargement, appearance, contrast, and keyboard access.
Use [page sizing](https://developer.apple.com/documentation/swiftui/view/presentationsizing(_:)) for list sheets and form sizing for settings and workspace forms.
Move notes to Trash without an extra confirmation, and require confirmation before emptying Trash or removing an original attachment, following Apple's [guidance for alerts](https://developer.apple.com/design/human-interface-guidelines/alerts).
Apple presents the design guidance as recommendations.
Owner policy requires applicable recommendations.

## Connection and recovery

Enter the server's HTTPS address when connecting.
Use the origin alone, such as `https://notes.example.ts.net`, without a path, query, or embedded credentials.
Change the address in Tungsten > Settings on Mac or Collections > Server on iOS.
Use View > Refresh or Command-R on Mac, or pull to refresh the note list on iOS, to load changes from another device.
The server continues to own notes, attachments, indexes, workspaces, history, and Trash.
The apps keep local drafts for interrupted writes and separate drafts by server and note.
Writes use the last acknowledged revision and preserve local text on conflict.
Writing entered while a deletion or history restoration awaits a response remains recoverable on the device.
Pending workspace assignments survive refreshed note contents.
Use Save as New Note to keep a conflicting draft under a separate identity.
Restoring a deleted note preserves an existing note or pending local draft with the same identity.
Keep transport security enabled.
Development builds support loopback connections for local checks.
The apps don't add server authentication, so the [private-access boundary](running.md#private-access) still applies.
The privacy manifest declares app-private preferences and metadata for local and user-selected files using Apple's [approved API reasons](https://developer.apple.com/documentation/bundleresources/app-privacy-configuration/nsprivacyaccessedapitypes/nsprivacyaccessedapitypereasons).

## Development

Install the tools pinned by `mise`, then build the apps with the repository commands.

```sh
mise install
mise x -- just apple build macos
mise x -- just apple build ios
open target/apple-build/Build/Products/Debug/Tungsten.app
```

The iOS command builds for the simulator.
Open `apple/Tungsten.xcodeproj` and select the TungstenIOS scheme to run on a chosen simulator.
For a physical device, select a signing team in Xcode and choose the connected device.
Git ignores the generated Xcode project.
`apple/project.yml` remains the source.
Use `mise x -- just apple build macos --release` or `mise x -- just apple build ios --release` to compile the Release configuration.
Release builds keep transport security enabled and exclude the isolated debug session controls.

Run the shared project gate and native checks before completing changes.

```sh
mise x -- just check
mise x -- just apple check
```

The native check runs Swift formatting, unit tests, API compatibility with an isolated production server, both app builds, and six system icon renditions.
Use deterministic tests for draft transitions, validated identities, revision handling, and delayed responses.
Inject transport failures to check interrupted writes without relying on UI timing.
Use the real server to check workspace, attachment, history, and recovery contracts.
Keep business-rule combinations in unit and API tests.
Choose targeted native inspection or optional UI automation for framework behavior and accessibility questions that lower-level tests leave open.
Native E2E tests are optional diagnostics.
The Apple CI and candidate gates require noninteractive checks.
The TungstenMac and TungstenIOS schemes select unit tests for the standard Xcode Test action.
Optional UI diagnostics use the separate TungstenMacInterfaceTests and TungstenIOSInterfaceTests schemes.
Project generation rejects standard schemes that include UI test bundles or omit unit tests.
The optional UI workflow checks writing, relaunch, and Trash restoration using synthetic notes.
Each iOS UI run creates and deletes its own simulator.
Native UI tests take control of the desktop and require explicit owner permission and coordinated access with other machine users.
The runner rejects UI commands before any effects unless `--allow-desktop-control` acknowledges authorized desktop access.
Its repository lock only prevents concurrent runs from the same checkout.

Use the optional appearance and accessibility tours when their coverage answers a concrete unresolved question.
Use `--only` to select `writing`, `workspace`, `empty-collections`, or `tour` diagnostics.
Use `--appearance light|dark` and `--text-size standard|enlarged` to select the relevant display conditions.
Combine `--only tour` with `--tour` to audit the selected conditions.

The Mac tour uses Light and Dark appearances at the system body size and 200% reading size.
Only isolated debug test sessions choose [the app appearance](https://developer.apple.com/documentation/appkit/nsapplication/appearance), leaving the owner's system preferences untouched.
Use [accessibility containers](https://developer.apple.com/documentation/swiftui/accessibilitychildbehavior/contain) to name content areas while preserving child controls and their labels.
The iOS tour uses Light and Dark appearances, Increase Contrast, and the largest accessibility text size.
The tours perform XCTest accessibility audits and keep app screenshots in `target/apple-ui-*/*.xcresult`.
An unnamed hosting group qualifies for the Mac description audit exception only when `isEnabled` is false.
Eligible groups must cover a window or sheet, or expose one child.
For a single child, the child must expose a readable label or value, or be a scroll or split container.
System Settings on macOS 27.2 produces the same report.
The workspace form exception accepts parentage reports that identify no element.
Finder's Go to Folder input produces the same report on macOS 27.2.
Controls and groups with content of their own remain subject to the audit.
On iOS 27.2, a minimal SwiftUI form reproduced a clipped-text prediction for a fully visible icon button and a standard single-line field.
The [clipped-text audit](https://developer.apple.com/documentation/accessibility/performing-accessibility-audits-for-your-app) predicts truncation at larger text sizes.
The tour preserves every prediction and its app screen for direct review at the largest text size.
The runner marks text clipping as requiring manual review.
The automated operation verdict doesn't establish text visibility.
Inspect complete static labels, reachable scrolling content, and editable fields before finishing that review.
Standard single-line fields scroll horizontally.
Other audit types remain failures unless a documented system-component exception applies.
Review system settings that the runner can't switch, including Reduce Motion and Reduce Transparency, on a release candidate before distribution.
An automated audit doesn't establish a complete accessibility review.

Pull requests run the Apple check when native code, the server contract, or shared tooling changes.
GitHub's [Xcode 27 image](https://github.com/actions/runner-images/blob/main/images/macos/xcode-27-arm64-Readme.md) supplies Xcode 27.2 on macOS 27.0.1.
The deployment target stays at 27.0 to support that hosted runtime.
The manual candidate workflow adds both Release builds to the noninteractive Apple check.
Distribution signing, notarization, TestFlight, and App Store publication remain separate release operations.
