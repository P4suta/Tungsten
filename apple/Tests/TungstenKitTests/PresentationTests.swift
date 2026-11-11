import Foundation
import Testing

@testable import TungstenKit

actor SuspendedDownload {
  private var continuation: CheckedContinuation<URL, any Error>?

  func response() async throws -> URL {
    try await withCheckedThrowingContinuation { continuation = $0 }
  }

  func hasStarted() -> Bool { continuation != nil }
  func complete(_ file: URL) {
    continuation?.resume(returning: file)
    continuation = nil
  }
}

@Test(arguments: [true, false])
@MainActor func obsoletePreviewDownloadsDoNotCreateFiles(_ cancelled: Bool) async throws {
  let directory = FileManager.default.temporaryDirectory.appending(path: UUID().uuidString)
  defer { try? FileManager.default.removeItem(at: directory) }
  let previews = directory.appending(path: "previews")
  try FileManager.default.createDirectory(at: previews, withIntermediateDirectories: true)
  let source = directory.appending(path: "download")
  try Data("Original file".utf8).write(to: source)
  let delayed = SuspendedDownload()
  let preview = AttachmentPreview(temporaryDirectory: previews)
  let opening = Task { try await preview.open(name: "sample.txt") { try await delayed.response() } }
  while !(await delayed.hasStarted()) { await Task.yield() }
  if cancelled { opening.cancel() }
  preview.close()
  await delayed.complete(source)
  _ = await opening.result
  #expect(preview.file == nil)
  #expect(!preview.isLoading)
  #expect(try FileManager.default.contentsOfDirectory(atPath: previews.path).isEmpty)
  #expect(!FileManager.default.fileExists(atPath: source.path))
}

@Test @MainActor func previewFilesExistOnlyWhileOwnedByThePresentation() async throws {
  let directory = FileManager.default.temporaryDirectory.appending(path: UUID().uuidString)
  defer { try? FileManager.default.removeItem(at: directory) }
  let previews = directory.appending(path: "previews")
  try FileManager.default.createDirectory(at: previews, withIntermediateDirectories: true)
  let source = directory.appending(path: "download")
  let data = Data("Original file".utf8)
  try data.write(to: source)
  let preview = AttachmentPreview(temporaryDirectory: previews)
  try await preview.open(name: "sample.txt") { source }
  let file = try #require(preview.file)
  #expect(try Data(contentsOf: file) == data)
  preview.close()
  #expect(preview.file == nil)
  #expect(!FileManager.default.fileExists(atPath: file.path))
  #expect(try FileManager.default.contentsOfDirectory(atPath: previews.path).isEmpty)
}

@Test @MainActor func anOlderDownloadCannotReplaceTheCurrentPreview() async throws {
  let directory = FileManager.default.temporaryDirectory.appending(path: UUID().uuidString)
  defer { try? FileManager.default.removeItem(at: directory) }
  let previews = directory.appending(path: "previews")
  try FileManager.default.createDirectory(at: previews, withIntermediateDirectories: true)
  let first = directory.appending(path: "first")
  let second = directory.appending(path: "second")
  try Data("Earlier file".utf8).write(to: first)
  try Data("Current file".utf8).write(to: second)
  let delayed = SuspendedDownload()
  let preview = AttachmentPreview(temporaryDirectory: previews)
  let earlier = Task { try await preview.open(name: "first.txt") { try await delayed.response() } }
  while !(await delayed.hasStarted()) { await Task.yield() }
  preview.close()
  try await preview.open(name: "second.txt") { second }
  let current = try #require(preview.file)
  await delayed.complete(first)
  _ = await earlier.result
  #expect(preview.file == current)
  #expect(try String(contentsOf: current, encoding: .utf8) == "Current file")
  #expect(try FileManager.default.contentsOfDirectory(atPath: previews.path).count == 1)
  #expect(!FileManager.default.fileExists(atPath: first.path))
  preview.close()
  #expect(try FileManager.default.contentsOfDirectory(atPath: previews.path).isEmpty)
}

actor SavedRecordResponses {
  private var count = 0
  let note: Note
  init(note: Note) { self.note = note }

  func response() throws -> HTTPResult {
    count += 1
    if count == 1 { throw ClientError.disconnected }
    return HTTPResult(status: 200, data: try JSONEncoder().encode(note))
  }
}

@Test @MainActor func failedSavedRecordsLeaveLoadingAndCanRetry() async throws {
  let note = try Note.test(body: "Saved writing")
  let record = SavedNote(
    id: String(repeating: "a", count: 32), name: note.name, title: note.title, captured: 1,
    revision: note.revision)
  let responses = SavedRecordResponses(note: note)
  let service = NoteService(address: try ServerAddress("https://notes.example")) { _, _ in
    try await responses.response()
  }
  let loader = SavedRecordLoader()
  await #expect(throws: ClientError.disconnected) {
    try await loader.load(record, origin: .history, service: service)
  }
  #expect(loader.state == .failed)
  #expect(loader.note == nil)
  try await loader.load(record, origin: .history, service: service)
  #expect(loader.state == .loaded(note))
}

@Test @MainActor func cancelledSavedRecordsRejectLateResponses() async throws {
  let note = try Note.test(body: "Saved writing")
  let record = SavedNote(
    id: String(repeating: "a", count: 32), name: note.name, title: note.title, captured: 1,
    revision: note.revision)
  let delayed = SuspendedRead()
  let service = NoteService(address: try ServerAddress("https://notes.example")) { _, _ in
    try await delayed.response()
  }
  let loader = SavedRecordLoader()
  let loading = Task { try await loader.load(record, origin: .trash, service: service) }
  while !(await delayed.hasStarted()) { await Task.yield() }
  loading.cancel()
  try await delayed.complete(note)
  await #expect(throws: CancellationError.self) { try await loading.value }
  #expect(loader.note == nil)
}

@Test(arguments: [true, false])
@MainActor func olderSavedRecordsCannotReplaceTheCurrentRecord(_ succeeds: Bool) async throws {
  let first = try Note.test(body: "Earlier record", name: "first.md")
  let second = try Note.test(body: "Current record", name: "second.md")
  let earlier = SavedNote(
    id: String(repeating: "a", count: 32), name: first.name, title: first.title, captured: 1,
    revision: first.revision)
  let current = SavedNote(
    id: String(repeating: "b", count: 32), name: second.name, title: second.title, captured: 2,
    revision: second.revision)
  let delayed = SuspendedRead()
  let service = NoteService(address: try ServerAddress("https://notes.example")) { request, _ in
    if request.url?.lastPathComponent == earlier.id { return try await delayed.response() }
    return HTTPResult(status: 200, data: try JSONEncoder().encode(second))
  }
  let loader = SavedRecordLoader()
  let loading = Task { try await loader.load(earlier, origin: .history, service: service) }
  while !(await delayed.hasStarted()) { await Task.yield() }
  try await loader.load(current, origin: .history, service: service)
  if succeeds {
    try await delayed.complete(first)
  } else {
    await delayed.completeResponse(HTTPResult(status: 503, data: Data()))
  }
  _ = await loading.result
  #expect(loader.note == second)
}
