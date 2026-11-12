# Apple applications

Use Apple's system design for the native applications under `apple`.
Build screens from standard SwiftUI, AppKit, and UIKit components, with system colors, typography, navigation, and accessibility behavior.
Keep custom materials and browser views out of the native applications.
Follow [the native client decision](docs/adr/use-native-apple-clients-with-the-shared-note-server.md) and [the Apple app guide](docs/apple-apps.md).

# Linux app

Use GTK4 and libadwaita system design under `linux`.
Follow the [Linux client decision](docs/adr/use-gtk-and-durable-drafts-for-the-linux-client.md) and [Linux app guide](docs/linux-apps.md).
Adopt applicable platform recommendations as owner policy, preserving their original normative status.
Use system appearance, fonts, accent colors, accessibility, and standard symbolic icons instead of shared monochrome styling.
Keep save decisions independent of GTK, persist drafts before sending, and preserve the existing server contract.
Run `mise x -- just linux source` on every host and `mise x -- just linux check` on Linux.
