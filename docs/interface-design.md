# Platform interface design

The [Tungsten Figma file](https://www.figma.com/design/8Zv6KUOa4OxNjdL4nKNwKB) contains editable platform screens.
The platform pages contain editable screens across Windows, macOS, iPhone, iPad, Linux, and Web.
Reusable controls, note rows, semantic color aliases, typography, spacing, and prototype links support implementation comparisons.
The original Apple sketches remain available as earlier references.

## Windows

The [Windows writing screen](https://www.figma.com/design/8Zv6KUOa4OxNjdL4nKNwKB?node-id=15-623) places collections, searchable notes, and the editor in separate panes.
Connection belongs on first use and in Settings.
Place New note at the top of the note list, keep note actions in one adaptive command bar, and show the note heading in the writing surface.
The Windows page includes Dark appearance, stacked notes and writing, connection, an empty collection, recovery, details, history, workspace creation, and Trash.

Microsoft's [NavigationView](https://learn.microsoft.com/en-us/windows/apps/develop/ui/controls/navigationview), [list/details](https://learn.microsoft.com/en-us/windows/apps/develop/ui/controls/list-details), [command bar](https://learn.microsoft.com/en-us/windows/apps/develop/ui/controls/command-bar), and [typography](https://learn.microsoft.com/en-us/windows/apps/design/signature-experiences/typography) guidance governs the native controls.
Project policy adopts applicable recommendations.
Figma uses Noto Sans with explicit approval because its connected font service lacks Segoe UI Variable.
The app keeps the Windows system font and language fallbacks.

## Apple

The [Mac writing screen](https://www.figma.com/design/8Zv6KUOa4OxNjdL4nKNwKB?node-id=21-5240) and [iPhone editor](https://www.figma.com/design/8Zv6KUOa4OxNjdL4nKNwKB?node-id=21-5753) use SF Pro and Apple's native kit.
Use NavigationSplitView, List, TextEditor, system toolbars, forms, and sheets from the [Apple app guide](apple-apps.md).
Apple's [sidebar](https://developer.apple.com/design/human-interface-guidelines/sidebars) and [text view](https://developer.apple.com/design/human-interface-guidelines/text-views) guidance governs navigation and editing.
The system renders materials and adapts controls to Dynamic Type and appearance.

## Linux

The [Linux writing screen](https://www.figma.com/design/8Zv6KUOa4OxNjdL4nKNwKB?node-id=17-623) uses collections, notes, and native Markdown source editing.
Use the system HeaderBar, Sidebar, NavigationSplitView, TextView, dialogs, and banners from the [Linux app guide](linux-apps.md).
Follow the desktop's [sidebar](https://developer.gnome.org/hig/patterns/nav/sidebars.html) and [adaptive layout](https://developer.gnome.org/hig/guidelines/adaptive.html) guidance.
The Figma specimen uses Cantarell.
The app follows the configured desktop font, accent, and contrast settings.

## Web

The [browser writing screen](https://www.figma.com/design/8Zv6KUOa4OxNjdL4nKNwKB?node-id=21-3232) places searchable notes beside the writing surface.
Closing navigation reclaims its space.
The Web page covers modal details, history, Trash, save recovery, an unwritten note, workspace editing, and narrow writing, navigation, and details.
The [Web navigation decision](adr/use-searchable-notes-and-on-demand-details-on-the-web.md) retires graph navigation while preserving stored relationships and server contracts.
Use the existing system font stack, black and white palette, 18-pixel base text, and 48-pixel standalone targets.
Keep the note's scroll and selection when switching panes.
Preserve [reflow](https://www.w3.org/WAI/WCAG22/Understanding/reflow.html), [text enlargement](https://www.w3.org/WAI/WCAG22/Understanding/resize-text.html), visible focus, and `accessible name`s.
Follow the HTML [dialog](https://html.spec.whatwg.org/multipage/interactive-elements.html#the-dialog-element) and [disclosure](https://html.spec.whatwg.org/multipage/interactive-elements.html#the-details-element) contracts with standard [modal focus guidance](https://www.w3.org/WAI/ARIA/apg/patterns/dialog-modal/).
Apply relevant Digital Agency [typography](https://design.digital.go.jp/dads/foundations/typography/), [layout](https://design.digital.go.jp/dads/foundations/layout/), and [button](https://design.digital.go.jp/dads/components/button/usage/) recommendations as project policy.

## Verification

Compare editable text, component instances, semantic colors, control placement, and narrow states with each running client.
Native themes and accessibility settings govern actual rendering rather than fixed Figma colors.
The Figma colors are system appearance specimens.
Check long notes, composition, selection, Undo, cancellation, conflicts, interrupted saves, and return to writing through the maintained project commands.
Keep release-only checks at the candidate gate.
