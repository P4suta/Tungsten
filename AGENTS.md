# Apple applications

Use Apple's system design for the native applications under `apple`.
Build screens from standard SwiftUI, AppKit, and UIKit components, with system colors, typography, navigation, and accessibility behavior.
Keep custom materials and browser views out of the native applications.
Follow [the native client decision](docs/adr/use-native-apple-clients-with-the-shared-note-server.md) and [the Apple app guide](docs/apple-apps.md).

# Linux app

Use GTK4 and libadwaita system design under `linux`.
Follow the [Linux client decision](docs/adr/use-gtk-and-durable-drafts-for-the-linux-client.md) and [Linux app guide](docs/linux-apps.md).
Adopt applicable platform recommendations as personal policy, preserving their original normative status.
Use system appearance, fonts, accent colors, accessibility, and standard symbolic icons instead of shared monochrome styling.
Keep save decisions independent of GTK, persist drafts before sending, and preserve the existing server contract.
Run `mise x -- just linux source` on every host and `mise x -- just linux check` on Linux.

# Windows app

Use Windows platform design for the C# and WinUI 3 client under `windows`.
Follow the [Windows client decision](docs/adr/use-winui-and-durable-drafts-for-the-windows-client.md) and [Windows app guide](docs/windows-apps.md).
Use standard controls, title bars, system fonts, icons, themes, accent colors, and accessibility behavior.
Windows recommendations take precedence over shared monochrome styling and other clients' visual designs.
Keep save decisions independent of WinUI and preserve the existing server contract.
Develop behavior through failing tests before implementation.
Run `mise x -- just windows source` on every host, `mise x -- just windows check` on Windows, and `mise x -- just windows contracts` on Linux.
Run native UI checks with `mise x -- just windows ui` and verify both architecture builds before completion.
