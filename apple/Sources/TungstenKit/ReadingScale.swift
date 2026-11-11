import SwiftUI

enum ReadingScale: Double, CaseIterable, Sendable {
  case standard = 1
  case enlarged = 1.25
  case large = 1.5
  case extraLarge = 1.75
  case twice = 2

  var larger: ReadingScale {
    switch self {
    case .standard: .enlarged
    case .enlarged: .large
    case .large: .extraLarge
    case .extraLarge, .twice: .twice
    }
  }

  var smaller: ReadingScale {
    switch self {
    case .standard, .enlarged: .standard
    case .large: .enlarged
    case .extraLarge: .large
    case .twice: .extraLarge
    }
  }
}

extension Font {
  @MainActor static func scaledSystemBody(_ scale: ReadingScale) -> Font {
    #if os(macOS)
      Font(scaledNativeBody(scale))
    #else
      .body
    #endif
  }
}

#if os(macOS)
  @MainActor func scaledNativeBody(_ scale: ReadingScale) -> NSFont {
    let base = NSFont.preferredFont(forTextStyle: .body)
    return NSFontManager.shared.convert(base, toSize: base.pointSize * scale.rawValue)
  }
#endif
