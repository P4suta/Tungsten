// swift-tools-version: 6.2
import PackageDescription

let package = Package(
  name: "TungstenKit",
  defaultLocalization: "en",
  platforms: [.macOS("27.0"), .iOS("27.0")],
  products: [.library(name: "TungstenKit", targets: ["TungstenKit"])],
  targets: [
    .target(name: "TungstenKit", resources: [.process("Resources")]),
    .testTarget(name: "TungstenKitTests", dependencies: ["TungstenKit"]),
  ],
  swiftLanguageModes: [.v6]
)
