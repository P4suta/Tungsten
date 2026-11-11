import SwiftUI

struct ProblemBanner: View {
  let model: AppModel

  var body: some View {
    if let problem = model.problem {
      HStack(alignment: .top) {
        Label(problem.message, systemImage: "exclamationmark.triangle")
          .font(.scaledSystemBody(model.readingScale))
          .textSelection(.enabled)
          .accessibilityIdentifier("operation-problem")
        Spacer()
        Button(localized("Dismiss"), systemImage: "xmark") { model.problem = nil }
          .labelStyle(.iconOnly)
      }
      .padding()
    }
  }
}
