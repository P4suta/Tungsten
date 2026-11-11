import Foundation
import Observation

private struct PreviewFile {
  let directory: URL
  let file: URL
}

private enum PreviewPhase {
  case idle
  case loading(UUID)
  case ready(PreviewFile)
}

@MainActor @Observable final class AttachmentPreview {
  private var phase: PreviewPhase = .idle
  private let temporaryDirectory: URL

  init(temporaryDirectory: URL = FileManager.default.temporaryDirectory) {
    self.temporaryDirectory = temporaryDirectory
  }

  var file: URL? {
    if case .ready(let value) = phase { value.file } else { nil }
  }

  var isLoading: Bool {
    if case .loading = phase { true } else { false }
  }

  func open(name: String, download: @Sendable () async throws -> URL) async throws {
    try Task.checkCancellation()
    guard !isLoading else { return }
    close()
    let attempt = UUID()
    phase = .loading(attempt)
    do {
      let source = try await download()
      defer { try? FileManager.default.removeItem(at: source) }
      try Task.checkCancellation()
      guard case .loading(let current) = phase, current == attempt else {
        throw CancellationError()
      }
      let directory = temporaryDirectory.appending(path: UUID().uuidString)
      try FileManager.default.createDirectory(
        at: directory, withIntermediateDirectories: false, attributes: [.posixPermissions: 0o700])
      do {
        let destination = directory.appending(path: URL(fileURLWithPath: name).lastPathComponent)
        try FileManager.default.moveItem(at: source, to: destination)
        phase = .ready(PreviewFile(directory: directory, file: destination))
      } catch {
        try? FileManager.default.removeItem(at: directory)
        throw error
      }
    } catch {
      if case .loading(let current) = phase, current == attempt { phase = .idle }
      throw error
    }
  }

  func close() {
    if case .ready(let value) = phase {
      try? FileManager.default.removeItem(at: value.directory)
    }
    phase = .idle
  }
}
