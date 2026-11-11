import SwiftUI
import Testing

@testable import TungstenKit

@Test func readingScaleHasBoundedReversibleSteps() {
  #expect(ReadingScale.standard.smaller == .standard)
  #expect(ReadingScale.twice.larger == .twice)
  for scale in ReadingScale.allCases where scale != .twice {
    #expect(scale.larger.smaller == scale)
  }
}

#if os(macOS)
  @Test @MainActor func nativeBodyFontReachesTwoHundredPercent() {
    let standard = NSFont.preferredFont(forTextStyle: .body)
    #expect(scaledNativeBody(.twice).pointSize == standard.pointSize * 2)
    #expect(scaledNativeBody(.standard).familyName == standard.familyName)
  }
#endif
