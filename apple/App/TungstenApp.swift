import SwiftUI
import TungstenKit

@main
struct TungstenApp: App {
  @State private var model = AppModel()

  init() {
    #if DEBUG && os(macOS)
      let arguments = ProcessInfo.processInfo.arguments
      if let sessionIndex = arguments.firstIndex(of: "--tungsten-test-session"),
        arguments.indices.contains(sessionIndex + 1),
        UUID(uuidString: arguments[sessionIndex + 1]) != nil,
        let appearanceIndex = arguments.firstIndex(of: "-AppleInterfaceStyle"),
        arguments.indices.contains(appearanceIndex + 1)
      {
        let name: NSAppearance.Name =
          arguments[appearanceIndex + 1] == "Dark" ? .darkAqua : .aqua
        NSApplication.shared.appearance = NSAppearance(named: name)
      }
    #endif
  }

  var body: some Scene {
    #if os(macOS)
      Window("Tungsten", id: "notes") {
        TungstenRootView(model: model)
      }
      .defaultSize(width: 1100, height: 760)
      .commands { TungstenCommands(model: model) }
      Settings {
        TungstenSettingsView(model: model)
      }
      .defaultSize(width: 520, height: 320)
    #else
      WindowGroup {
        TungstenRootView(model: model)
      }
      .commands { TungstenCommands(model: model) }
    #endif
  }
}
