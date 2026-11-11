import Foundation
import Testing

@testable import TungstenKit

@MainActor func testingModel(service: NoteService, drafts: DraftStore) throws -> AppModel {
  let preferences = try #require(
    UserDefaults(suiteName: "Tungsten.Tests.\(UUID().uuidString)"))
  return AppModel(service: service, drafts: drafts, preferences: preferences)
}

actor SuspendedRead {
  private var resume: CheckedContinuation<HTTPResult, any Error>?
  private var requests = 0

  func response() async throws -> HTTPResult {
    requests += 1
    return try await withCheckedThrowingContinuation { resume = $0 }
  }

  func hasStarted() -> Bool { requests > 0 }
  func requestCount() -> Int { requests }

  func complete(_ note: Note) throws {
    resume?.resume(returning: HTTPResult(status: 200, data: try JSONEncoder().encode(note)))
    resume = nil
  }

  func completeResponse(_ response: HTTPResult) {
    resume?.resume(returning: response)
    resume = nil
  }
}

actor InterruptedCreate {
  private var note: Note?
  private var creations = 0

  func response(_ request: URLRequest) throws -> HTTPResult {
    if request.httpMethod == "POST" {
      if note != nil { return HTTPResult(status: 409, data: Data()) }
      let body = try #require(request.httpBody)
      let values = try JSONDecoder().decode([String: String].self, from: body)
      note = try Note.test(body: #require(values["body"]), name: #require(values["name"]))
      creations += 1
      throw ClientError.disconnected
    }
    if request.url?.path == "/api/notes", let note {
      let summary: [String: Any] = [
        "name": note.name.rawValue, "title": note.title, "snippet": note.body,
        "modified": note.modified,
      ]
      let page: [String: Any] = ["notes": [summary], "total": 1, "next": NSNull()]
      return HTTPResult(status: 200, data: try JSONSerialization.data(withJSONObject: page))
    }
    return HTTPResult(status: 200, data: try JSONEncoder().encode(#require(note)))
  }

  func creationCount() -> Int { creations }
}

@Test @MainActor func openingAnotherNoteRejectsAnObsoleteRead() async throws {
  let directory = FileManager.default.temporaryDirectory.appending(path: UUID().uuidString)
  defer { try? FileManager.default.removeItem(at: directory) }
  let slow = SuspendedRead()
  let first = try Note.test(body: "First", name: "first.md")
  let second = try Note.test(body: "Second", name: "second.md")
  let service = NoteService(address: try ServerAddress("https://notes.example")) { request, _ in
    if request.url?.path.hasSuffix("first.md") == true { return try await slow.response() }
    return HTTPResult(status: 200, data: try JSONEncoder().encode(second))
  }
  let model = try testingModel(service: service, drafts: DraftStore(directory: directory))
  let pending = Task { await model.open(first.name) }
  while !(await slow.hasStarted()) { await Task.yield() }
  await model.open(second.name)
  try await slow.complete(first)
  await pending.value
  #expect(model.activeID == second.name)
  #expect(model.currentDraft?.body == "Second")
}

@Test @MainActor func failedWritesRemainRecoverableAfterRelaunch() async throws {
  let directory = FileManager.default.temporaryDirectory.appending(path: UUID().uuidString)
  defer { try? FileManager.default.removeItem(at: directory) }
  let note = try Note.test(body: "Original")
  let service = NoteService(address: try ServerAddress("https://notes.example")) { request, _ in
    if request.httpMethod == "PUT" { throw ClientError.disconnected }
    return HTTPResult(status: 200, data: try JSONEncoder().encode(note))
  }
  let store = try DraftStore(directory: directory)
  let model = try testingModel(service: service, drafts: store)
  await model.open(note.name)
  model.edit("Writing during a network failure")
  await model.save(note.name)
  #expect(model.currentDraft?.body == "Writing during a network failure")
  #expect(model.currentDraft?.status == .failed)
  #expect(
    try store.load(server: service.address, id: note.name)?.body
      == "Writing during a network failure")
}

@Test @MainActor func localStorageFailureDoesNotClaimTheDraftIsSaved() async throws {
  let directory = FileManager.default.temporaryDirectory.appending(path: UUID().uuidString)
  defer { try? FileManager.default.removeItem(at: directory) }
  let note = try Note.test(body: "Original")
  let service = NoteService(address: try ServerAddress("https://notes.example")) { _, _ in
    HTTPResult(status: 200, data: try JSONEncoder().encode(note))
  }
  let model = try testingModel(service: service, drafts: DraftStore(directory: directory))
  await model.open(note.name)
  try FileManager.default.removeItem(at: directory)
  try Data("Unavailable draft directory".utf8).write(to: directory)
  model.edit("Writing that must remain in memory")
  #expect(model.currentDraft?.status == .unsaved)
  #expect(model.currentDraft?.body == "Writing that must remain in memory")
  #expect(model.problem != nil)
  model.selectCollection(.trash)
  #expect(model.collection == .all)
  #expect(model.activeID == note.name)
  #expect(model.currentDraft?.body == "Writing that must remain in memory")
}

@Test @MainActor func retryingAnInterruptedCreateAcknowledgesTheExistingNote() async throws {
  let directory = FileManager.default.temporaryDirectory.appending(path: UUID().uuidString)
  defer { try? FileManager.default.removeItem(at: directory) }
  let remote = InterruptedCreate()
  let service = NoteService(address: try ServerAddress("https://notes.example")) { request, _ in
    try await remote.response(request)
  }
  let store = try DraftStore(directory: directory)
  let model = try testingModel(service: service, drafts: store)
  model.newNote()
  let original = try #require(model.activeID)
  model.edit("# Writing saved before the connection broke")
  await model.saveActive()
  #expect(model.currentDraft?.status == .failed)
  await model.retrySave()
  #expect(model.activeID == original)
  #expect(model.currentDraft?.status == .saved)
  #expect(model.currentDraft?.body == "# Writing saved before the connection broke")
  #expect(await remote.creationCount() == 1)
  #expect(try store.all(server: service.address).isEmpty)
}

@Test @MainActor func refreshingWhileTypingPreservesTheLocalDraft() async throws {
  let directory = FileManager.default.temporaryDirectory.appending(path: UUID().uuidString)
  defer { try? FileManager.default.removeItem(at: directory) }
  let slow = SuspendedRead()
  let original = try Note.test(body: "Original")
  let service = NoteService(address: try ServerAddress("https://notes.example")) { _, _ in
    try await slow.response()
  }
  let model = try testingModel(service: service, drafts: DraftStore(directory: directory))
  let opening = Task { await model.open(original.name) }
  while !(await slow.hasStarted()) { await Task.yield() }
  try await slow.complete(original)
  await opening.value
  let refresh = Task { await model.refreshActive() }
  while await slow.requestCount() < 2 { await Task.yield() }
  model.edit("Typing while another device updates the note")
  try await slow.complete(Note.test(body: "Remote writing", revision: "b"))
  await refresh.value
  #expect(model.currentDraft?.body == "Typing while another device updates the note")
  #expect(model.currentDraft?.status == .conflict)
}

@Test @MainActor func joiningAnActiveSaveWaitsForItsAcknowledgement() async throws {
  let directory = FileManager.default.temporaryDirectory.appending(path: UUID().uuidString)
  defer { try? FileManager.default.removeItem(at: directory) }
  let delayed = SuspendedRead()
  let original = try Note.test(body: "Original")
  let updated = try Note.test(body: "Acknowledged writing", revision: "b")
  let service = NoteService(address: try ServerAddress("https://notes.example")) { request, _ in
    if request.httpMethod == "PUT" { return try await delayed.response() }
    if request.url?.path == "/api/notes" {
      return HTTPResult(status: 200, data: Data("{\"notes\":[],\"total\":0}".utf8))
    }
    return HTTPResult(status: 200, data: try JSONEncoder().encode(original))
  }
  let model = try testingModel(service: service, drafts: DraftStore(directory: directory))
  await model.open(original.name)
  model.edit(updated.body)
  let saving = Task { await model.saveActive() }
  while !(await delayed.hasStarted()) { await Task.yield() }
  var joiningStarted = false
  let joining = Task {
    joiningStarted = true
    await model.saveActive()
    return model.currentDraft?.revision
  }
  while !joiningStarted { await Task.yield() }
  try await delayed.complete(updated)
  await saving.value
  #expect(await joining.value == updated.revision)
}

@Test @MainActor func cancellingAFilePickerDoesNotReportAFailure() throws {
  let directory = FileManager.default.temporaryDirectory.appending(path: UUID().uuidString)
  defer { try? FileManager.default.removeItem(at: directory) }
  let service = NoteService(address: try ServerAddress("https://notes.example"))
  let model = try testingModel(service: service, drafts: DraftStore(directory: directory))
  model.report(CocoaError(.userCancelled))
  #expect(model.problem == nil)
  model.report(ClientError.disconnected)
  #expect(model.problem != nil)
}

@Test(arguments: [NoteCollection.all, .drafts, .trash])
@MainActor func selectingACollectionClearsTheNoteAndPreservesWriting(_ selection: NoteCollection)
  async throws
{
  let directory = FileManager.default.temporaryDirectory.appending(path: UUID().uuidString)
  defer { try? FileManager.default.removeItem(at: directory) }
  let note = try Note.test(body: "Original")
  let service = NoteService(address: try ServerAddress("https://notes.example")) { request, _ in
    if request.httpMethod == "PUT" { throw ClientError.disconnected }
    return HTTPResult(status: 200, data: try JSONEncoder().encode(note))
  }
  let store = try DraftStore(directory: directory)
  let model = try testingModel(service: service, drafts: store)
  await model.open(note.name)
  model.edit("Writing retained across collection navigation")
  model.selectCollection(selection)
  #expect(model.collection == selection)
  #expect(model.activeID == nil)
  #expect(
    try store.load(server: service.address, id: note.name)?.body
      == "Writing retained across collection navigation")
}
