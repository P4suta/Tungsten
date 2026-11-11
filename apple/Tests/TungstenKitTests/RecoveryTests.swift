import Foundation
import Testing

@testable import TungstenKit

private let emptyNotes = Data("{\"notes\":[],\"total\":0}".utf8)

actor InterruptedUpdate {
  private var note: Note
  private var updates = 0
  init(note: Note) { self.note = note }

  func response(_ request: URLRequest) throws -> HTTPResult {
    if request.url?.path == "/api/notes" { return HTTPResult(status: 200, data: emptyNotes) }
    if request.httpMethod == "PUT" {
      guard request.value(forHTTPHeaderField: "If-Match") == note.revision.rawValue else {
        return HTTPResult(status: 409, data: Data())
      }
      let values = try JSONDecoder().decode([String: String].self, from: #require(request.httpBody))
      note = try Note.test(body: #require(values["body"]), revision: "b")
      updates += 1
      throw ClientError.disconnected
    }
    return HTTPResult(status: 200, data: try JSONEncoder().encode(note))
  }

  func updateCount() -> Int { updates }
}

actor ExistingThenMissing {
  private var reads = 0
  let note: Note
  init(note: Note) { self.note = note }

  func response() throws -> HTTPResult {
    reads += 1
    return reads == 1
      ? HTTPResult(status: 200, data: try JSONEncoder().encode(note))
      : HTTPResult(status: 404, data: Data())
  }
}

@Test @MainActor func remotelyDeletedCachedNotesLeaveIndependentFormsOpen() async throws {
  let directory = FileManager.default.temporaryDirectory.appending(path: UUID().uuidString)
  defer { try? FileManager.default.removeItem(at: directory) }
  let original = try Note.test(body: "Original")
  let remote = ExistingThenMissing(note: original)
  let service = NoteService(address: try ServerAddress("https://notes.example")) { _, _ in
    try await remote.response()
  }
  let model = try testingModel(service: service, drafts: DraftStore(directory: directory))
  await model.open(original.name)
  model.selectCollection(.all)
  model.sheet = .workspaceEditor(nil)
  await model.open(original.name)
  #expect(model.activeID == nil)
  #expect(model.currentDraft == nil)
  #expect(model.sheet == .workspaceEditor(nil))
}

@Test @MainActor func interruptedUpdatesRetryAfterRelaunchWithoutCreatingCopies() async throws {
  let directory = FileManager.default.temporaryDirectory.appending(path: UUID().uuidString)
  defer { try? FileManager.default.removeItem(at: directory) }
  let original = try Note.test(body: "Original")
  let remote = InterruptedUpdate(note: original)
  let service = NoteService(address: try ServerAddress("https://notes.example")) { request, _ in
    try await remote.response(request)
  }
  let store = try DraftStore(directory: directory)
  do {
    let model = try testingModel(service: service, drafts: store)
    await model.open(original.name)
    model.edit("Writing committed before the response was lost")
    await model.saveActive()
    #expect(model.currentDraft?.status == .failed)
  }
  let reopened = try testingModel(service: service, drafts: store)
  await reopened.open(original.name)
  #expect(reopened.currentDraft?.status == .failed)
  await reopened.retrySave()
  #expect(reopened.currentDraft?.status == .saved)
  #expect(reopened.currentDraft?.body == "Writing committed before the response was lost")
  #expect(reopened.activeID == original.name)
  #expect(await remote.updateCount() == 1)
  #expect(try store.load(server: service.address, id: original.name) == nil)
}

@Test @MainActor func interruptedCreatesRetryAfterRelaunchWithTheSameIdentity() async throws {
  let directory = FileManager.default.temporaryDirectory.appending(path: UUID().uuidString)
  defer { try? FileManager.default.removeItem(at: directory) }
  let remote = InterruptedCreate()
  let service = NoteService(address: try ServerAddress("https://notes.example")) { request, _ in
    try await remote.response(request)
  }
  let store = try DraftStore(directory: directory)
  let id: NoteID
  do {
    let model = try testingModel(service: service, drafts: store)
    model.newNote()
    id = try #require(model.activeID)
    model.edit("Writing committed before creation was acknowledged")
    await model.saveActive()
    #expect(model.currentDraft?.status == .failed)
  }
  let reopened = try testingModel(service: service, drafts: store)
  await reopened.open(id)
  #expect(reopened.currentDraft?.status == .failed)
  await reopened.retrySave()
  #expect(reopened.activeID == id)
  #expect(reopened.currentDraft?.status == .saved)
  #expect(await remote.creationCount() == 1)
}

@Test @MainActor func reopeningARemotelyDeletedNoteKeepsCopyRecoveryAvailable() async throws {
  let directory = FileManager.default.temporaryDirectory.appending(path: UUID().uuidString)
  defer { try? FileManager.default.removeItem(at: directory) }
  let original = try Note.test(body: "Original")
  var draft = Draft(note: original)
  draft.edit("Writing retained before the next request")
  let store = try DraftStore(directory: directory)
  let service = NoteService(address: try ServerAddress("https://notes.example")) { _, _ in
    HTTPResult(status: 404, data: Data())
  }
  try store.save(draft, server: service.address)
  let model = try testingModel(service: service, drafts: store)
  await model.open(original.name)
  #expect(model.currentDraft?.status == .conflict)
  #expect(model.currentDraft?.body == draft.body)
}

@Test(arguments: [true, false])
@MainActor func revertingDuringAnOutstandingWriteRemainsRecoverable(_ acknowledged: Bool)
  async throws
{
  let directory = FileManager.default.temporaryDirectory.appending(path: UUID().uuidString)
  defer { try? FileManager.default.removeItem(at: directory) }
  let delayed = SuspendedRead()
  let original = try Note.test(body: "Original")
  let written = try Note.test(body: "Sent writing", revision: "b")
  let service = NoteService(address: try ServerAddress("https://notes.example")) { request, _ in
    if request.httpMethod == "PUT" { return try await delayed.response() }
    if request.url?.path == "/api/notes" { return HTTPResult(status: 200, data: emptyNotes) }
    return HTTPResult(status: 200, data: try JSONEncoder().encode(original))
  }
  let store = try DraftStore(directory: directory)
  let model = try testingModel(service: service, drafts: store)
  await model.open(original.name)
  model.edit(written.body)
  let saving = Task { await model.saveActive() }
  while !(await delayed.hasStarted()) { await Task.yield() }
  model.edit(original.body)
  let recovered = try store.load(server: service.address, id: original.name)
  #expect(recovered?.body == original.body)
  #expect(recovered?.hasPendingWork == true)
  #expect(recovered?.status == .conflict)
  if acknowledged {
    try await delayed.complete(written)
  } else {
    await delayed.completeResponse(HTTPResult(status: 503, data: Data()))
  }
  await saving.value
  #expect(model.currentDraft?.body == original.body)
  if acknowledged {
    #expect(model.currentDraft?.isDirty == true)
  } else {
    #expect(model.currentDraft?.status == .conflict)
  }
}

@Test @MainActor func connectionProbesCannotDiscardLaterUnpersistedWriting() async throws {
  let directory = FileManager.default.temporaryDirectory.appending(path: UUID().uuidString)
  defer { try? FileManager.default.removeItem(at: directory) }
  let original = try Note.test(body: "Original")
  let service = NoteService(address: try ServerAddress("https://first.example")) { _, _ in
    HTTPResult(status: 200, data: try JSONEncoder().encode(original))
  }
  let delayed = SuspendedRead()
  let candidate = NoteService(address: try ServerAddress("https://second.example")) { request, _ in
    if request.url?.path == "/api/notes" {
      if await delayed.requestCount() == 0 { return try await delayed.response() }
      return HTTPResult(status: 200, data: emptyNotes)
    }
    return HTTPResult(status: 200, data: workspacePage)
  }
  let model = try testingModel(service: service, drafts: DraftStore(directory: directory))
  await model.open(original.name)
  let connecting = Task { await model.connect(to: candidate) }
  while !(await delayed.hasStarted()) { await Task.yield() }
  try FileManager.default.removeItem(at: directory)
  try Data("Unavailable draft directory".utf8).write(to: directory)
  model.edit("Writing entered during the connection probe")
  await delayed.completeResponse(HTTPResult(status: 200, data: emptyNotes))
  #expect(await connecting.value == false)
  #expect(model.service?.address == service.address)
  #expect(model.currentDraft?.body == "Writing entered during the connection probe")
}

private let workspacePage = Data(
  ("{\"workspaces\":[],\"revision\":\"\\\"" + String(repeating: "a", count: 64)
    + "\\\"\"}").utf8)

@Test @MainActor func changingServersClearsAnObsoleteOpeningState() async throws {
  let directory = FileManager.default.temporaryDirectory.appending(path: UUID().uuidString)
  defer { try? FileManager.default.removeItem(at: directory) }
  let delayed = SuspendedRead()
  let original = try Note.test(body: "Original")
  let service = NoteService(address: try ServerAddress("https://first.example")) { _, _ in
    try await delayed.response()
  }
  let candidate = NoteService(address: try ServerAddress("https://second.example")) { request, _ in
    HTTPResult(status: 200, data: request.url?.path == "/api/notes" ? emptyNotes : workspacePage)
  }
  let model = try testingModel(service: service, drafts: DraftStore(directory: directory))
  let opening = Task { await model.open(original.name) }
  while !(await delayed.hasStarted()) { await Task.yield() }
  #expect(await model.connect(to: candidate))
  #expect(!model.openingNote)
  try await delayed.complete(original)
  await opening.value
  #expect(!model.openingNote)
  #expect(model.activeID == nil)
}

@Test @MainActor func remotelyDeletedNotesOfferCopyRecovery() async throws {
  let directory = FileManager.default.temporaryDirectory.appending(path: UUID().uuidString)
  defer { try? FileManager.default.removeItem(at: directory) }
  let original = try Note.test(body: "Original")
  let service = NoteService(address: try ServerAddress("https://notes.example")) { request, _ in
    if request.httpMethod == "PUT" { return HTTPResult(status: 404, data: Data()) }
    if request.httpMethod == "POST" {
      let values = try JSONDecoder().decode([String: String].self, from: #require(request.httpBody))
      let copy = try Note.test(body: #require(values["body"]), name: #require(values["name"]))
      return HTTPResult(status: 201, data: try JSONEncoder().encode(copy))
    }
    if request.url?.path == "/api/notes" { return HTTPResult(status: 200, data: emptyNotes) }
    return HTTPResult(status: 200, data: try JSONEncoder().encode(original))
  }
  let model = try testingModel(service: service, drafts: DraftStore(directory: directory))
  await model.open(original.name)
  model.edit("Writing kept after remote deletion")
  await model.saveActive()
  #expect(model.currentDraft?.status == .conflict)
  await model.saveCopy()
  #expect(model.activeID != original.name)
  #expect(model.currentDraft?.body == "Writing kept after remote deletion")
  #expect(model.currentDraft?.status == .saved)
}

@Test @MainActor func damagedRecordsDoNotHideOtherRecoverableDrafts() throws {
  let directory = FileManager.default.temporaryDirectory.appending(path: UUID().uuidString)
  defer { try? FileManager.default.removeItem(at: directory) }
  let first = try ServerAddress("https://first.example")
  let second = try ServerAddress("https://second.example")
  let store = try DraftStore(directory: directory)
  let wanted = Draft(id: try NoteID("recoverable.md"), body: "Writing on this device")
  try store.save(wanted, server: first)
  try store.save(Draft(id: try NoteID("other.md"), body: "Another server"), server: second)
  let damaged = directory.appending(path: "damaged.json")
  try Data("Damaged draft".utf8).write(to: damaged)
  let model = try testingModel(service: NoteService(address: first), drafts: store)
  model.refreshDrafts()
  #expect(model.recoveredDrafts.map(\.id) == [wanted.id])
  #expect(model.problem != nil)
  #expect(try String(contentsOf: damaged, encoding: .utf8) == "Damaged draft")
}

@Test(arguments: ["topic.md", "folder/topic.md", "folder/deeper/topic.md"], ["image", "file"])
@MainActor func attachmentLinksResolveFromTheExportedRoot(_ name: String, _ kind: String)
  async throws
{
  let directory = FileManager.default.temporaryDirectory.appending(path: UUID().uuidString)
  defer { try? FileManager.default.removeItem(at: directory) }
  let store = try DraftStore(directory: directory.appending(path: "drafts"))
  let file = directory.appending(path: "sample.txt")
  try Data("x".utf8).write(to: file)
  let original = try Note.test(body: "Original", name: name)
  let id = String(repeating: "a", count: 32) + ".txt"
  let metadata: [String: Any] = [
    "id": id, "name": "Sample", "bytes": 1,
    "revision": original.revision.rawValue, "kind": kind,
  ]
  let response = try JSONSerialization.data(withJSONObject: metadata)
  let service = NoteService(address: try ServerAddress("https://notes.example")) { request, _ in
    if request.url?.path == "/api/attachments" { return HTTPResult(status: 201, data: response) }
    if request.httpMethod == "PUT" {
      let values = try JSONDecoder().decode([String: String].self, from: #require(request.httpBody))
      let saved = try Note.test(body: #require(values["body"]), revision: "b", name: name)
      return HTTPResult(status: 200, data: try JSONEncoder().encode(saved))
    }
    if request.url?.path == "/api/notes" { return HTTPResult(status: 200, data: emptyNotes) }
    return HTTPResult(status: 200, data: try JSONEncoder().encode(original))
  }
  let model = try testingModel(service: service, drafts: store)
  await model.open(original.name)
  await model.upload(file)
  let body = try #require(model.currentDraft?.body)
  let target = try #require(body.split(separator: "(").last?.dropLast(2))
  let exportRoot = URL(fileURLWithPath: "/export/")
  let document = exportRoot.appending(path: name)
  let resolved = try #require(URL(string: String(target), relativeTo: document))
  #expect(resolved.standardizedFileURL == exportRoot.appending(path: ".attachments/" + id))
  #expect(body.contains(kind == "image" ? "![Sample]" : "[Sample]"))
}
