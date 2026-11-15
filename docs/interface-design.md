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

## App identity

The [app identity board](https://www.figma.com/design/8Zv6KUOa4OxNjdL4nKNwKB?node-id=90-341) defines a tungsten cube with a T cut into its front.
The requested monogram is essential to the identity, an exception to Microsoft's general recommendation to avoid letters in [app icons](https://learn.microsoft.com/en-us/windows/apps/design/iconography/app-icon-design).
One filled compound contour joins the cube's faces without exposed stroke caps.
Light and Dark variants share the contour, while the default variant adds opaque white facets and a white outer keyline for mixed backgrounds.

Keep the exported vector masters under `assets/app-icon`.
Run `mise x -- just app-icons generate` with ImageMagick 7 after changing them.
Run `mise x -- just app-icons check` to validate the filled paths, black and white palette, appearance geometry, derived vectors, raster dimensions, checksums, source hashes, and Windows icon frames.
Each native source gate runs the asset check without requiring a raster renderer.

Windows uses all fourteen target sizes in default, light, and dark forms, plus the five scale factors in Microsoft's [icon construction guidance](https://learn.microsoft.com/en-us/windows/apps/design/iconography/app-icon-construction).
The executable embeds the default `.ico` file, and the window selects contrasting title-bar artwork from the system appearance.
High contrast uses the default artwork with its white keyline.
The Apple vector remains unmasked in Icon Composer, letting the system render Default, Dark, Clear, and Tinted appearances under Apple's [app icon guidance](https://developer.apple.com/design/human-interface-guidelines/app-icons).

The Linux regular icon uses the desktop's [128-pixel canvas and baseline](https://developer.gnome.org/hig/guidelines/app-icons.html).
Its [16-pixel symbolic icon](https://developer.gnome.org/hig/guidelines/ui-icons.html) shows the cube's front orthogonally, with two-pixel features and system recoloring.
Web links declare actual Windows icon sizes and appearance-specific SVGs under the [HTML icon selection contract](https://html.spec.whatwg.org/multipage/links.html#rel-icon).
An opaque 180-pixel image in `PNG` format supports Safari's [Web Clip icon selection](https://developer.apple.com/library/archive/documentation/AppleApplications/Reference/SafariWebContent/ConfiguringWebApplications/ConfiguringWebApplications.html#//apple_ref/doc/uid/TP40002051-CH3-SW4).

## Verification

Compare editable text, component instances, semantic colors, control placement, and narrow states with each running client.
Native themes and accessibility settings govern actual rendering rather than fixed Figma colors.
The Figma colors are system appearance specimens.
Check long notes, composition, selection, Undo, cancellation, conflicts, interrupted saves, and return to writing through the maintained project commands.
Keep release-only checks at the candidate gate.
