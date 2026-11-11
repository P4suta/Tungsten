import Foundation
import Testing

@testable import TungstenKit

@Test(
  .enabled(if: ProcessInfo.processInfo.environment["TUNGSTEN_SERVER"] != nil),
  arguments: [false, true])
@MainActor func deletingANotePreservesWritingEnteredBeforeTheResponse(_ editing: Bool) async throws
{
  let server = try #require(ProcessInfo.processInfo.environment["TUNGSTEN_SERVER"])
  let address = try ServerAddress(server, permitsLoopback: true)
  let client = NoteService(address: address)
  let original = try await client.save(
    Draft(id: NoteID.generated(), body: "# Note before deletion").pendingWrite())
  let delayed = SuspendedRead()
  let network = NetworkSession()
  let service = NoteService(address: address) { request, upload in
    let response = try await network.send(request, upload: upload)
    if request.httpMethod == "DELETE", request.url?.path == "/api/notes/\(original.name.rawValue)" {
      _ = try await delayed.response()
    }
    return response
  }
  let directory = FileManager.default.temporaryDirectory.appending(path: UUID().uuidString)
  defer { try? FileManager.default.removeItem(at: directory) }
  let store = try DraftStore(directory: directory)
  let model = try testingModel(service: service, drafts: store)
  await model.open(original.name)
  let deleting = Task { await model.deleteCurrent() }
  while !(await delayed.hasStarted()) { await Task.yield() }
  if editing { model.edit("Writing entered while deletion was in progress") }
  await delayed.completeResponse(HTTPResult(status: 204, data: Data()))
  await deleting.value
  await #expect(throws: ClientError.missing) { try await client.read(original.name) }
  if editing {
    #expect(model.currentDraft?.body == "Writing entered while deletion was in progress")
    #expect(model.currentDraft?.status == .conflict)
    #expect(
      try store.load(server: address, id: original.name)?.body
        == "Writing entered while deletion was in progress")
    await model.saveCopy()
    let copy = try #require(model.currentDraft)
    #expect(copy.id != original.name)
    #expect(try await client.read(copy.id).body == "Writing entered while deletion was in progress")
    try await client.delete(client.read(copy.id))
  } else {
    #expect(model.activeID == nil)
    #expect(model.currentDraft == nil)
    #expect(try store.load(server: address, id: original.name) == nil)
  }
}

@Test(
  .enabled(if: ProcessInfo.processInfo.environment["TUNGSTEN_SERVER"] != nil),
  arguments: [false, true])
@MainActor func restoringHistoryPreservesWritingEnteredBeforeTheResponse(_ editing: Bool)
  async throws
{
  let server = try #require(ProcessInfo.processInfo.environment["TUNGSTEN_SERVER"])
  let address = try ServerAddress(server, permitsLoopback: true)
  let client = NoteService(address: address)
  let initial = try await client.save(
    Draft(id: NoteID.generated(), body: "# Earlier version").pendingWrite())
  var writing = Draft(note: initial)
  writing.edit("# Current version")
  let current = try await client.save(writing.pendingWrite())
  let record = try #require(try await client.history(current.name).first)
  let delayed = SuspendedRead()
  let network = NetworkSession()
  let service = NoteService(address: address) { request, upload in
    let response = try await network.send(request, upload: upload)
    if request.httpMethod == "POST", request.url?.path.hasPrefix("/api/history/") == true {
      _ = try await delayed.response()
    }
    return response
  }
  let directory = FileManager.default.temporaryDirectory.appending(path: UUID().uuidString)
  defer { try? FileManager.default.removeItem(at: directory) }
  let store = try DraftStore(directory: directory)
  let model = try testingModel(service: service, drafts: store)
  await model.open(current.name)
  let restoring = Task { await model.restore(record, fromTrash: false) }
  while !(await delayed.hasStarted()) { await Task.yield() }
  if editing { model.edit("Writing entered while restoration was in progress") }
  await delayed.completeResponse(HTTPResult(status: 200, data: Data()))
  #expect(await restoring.value)
  if editing {
    #expect(model.currentDraft?.body == "Writing entered while restoration was in progress")
    #expect(model.currentDraft?.status == .conflict)
    #expect(
      try store.load(server: address, id: current.name)?.body
        == "Writing entered while restoration was in progress")
  } else {
    #expect(model.currentDraft?.body == initial.body)
    #expect(model.currentDraft?.status == .saved)
    #expect(try store.load(server: address, id: current.name) == nil)
  }
  #expect(try await client.read(current.name).body == initial.body)
  try await client.delete(client.read(current.name))
}

@Test(.enabled(if: ProcessInfo.processInfo.environment["TUNGSTEN_SERVER"] != nil))
func sharedServerPreservesNativeWorkflows() async throws {
  let server = try #require(ProcessInfo.processInfo.environment["TUNGSTEN_SERVER"])
  let client = NoteService(address: try ServerAddress(server, permitsLoopback: true))
  let id = try NoteID.generated()
  let unique = UUID().uuidString
  let initial = Draft(id: id, body: "# Native contract \(unique)\n\nOriginal writing.")
  let created = try await client.save(initial.pendingWrite())
  let read = try await client.read(id)
  #expect(read == created)
  var draft = Draft(note: created)
  draft.edit("# Changed native contract \(unique)\n\nUpdated writing.")
  let updated = try await client.save(draft.pendingWrite())
  #expect(updated.revision != created.revision)
  await #expect(throws: ClientError.conflict) { try await client.save(draft.pendingWrite()) }
  let results = try await client.notes(query: unique)
  #expect(results.notes.contains(where: { $0.name == id }))

  let workspace = try await client.createWorkspace("Native \(unique)")
  let joined = try await client.membership(workspace.id, note: id, included: true)
  #expect(try await client.workspaceNotes(workspace.id).notes.contains(where: { $0.name == id }))
  let renamed = try await client.renameWorkspace(joined, title: "Renamed \(unique)")
  #expect(renamed.title == "Renamed \(unique)")
  await #expect(throws: ClientError.conflict) {
    try await client.renameWorkspace(workspace, title: "Stale")
  }

  let directory = FileManager.default.temporaryDirectory.appending(path: unique)
  try FileManager.default.createDirectory(at: directory, withIntermediateDirectories: false)
  defer { try? FileManager.default.removeItem(at: directory) }
  let original = Data("Original attachment 日本語\n".utf8)
  let file = directory.appending(path: "Native original.txt")
  try original.write(to: file)
  let uploaded = try await client.upload(file)
  #expect(uploaded.name == "Native original.txt")
  let metadata = try await client.attachment(uploaded.id)
  let downloaded = try await client.download(metadata)
  defer { try? FileManager.default.removeItem(at: downloaded) }
  #expect(try Data(contentsOf: downloaded) == original)

  let linkedID = try NoteID.generated()
  let linked = Draft(
    id: linkedID,
    body:
      "# Native linked contract\n\n[Reference](\(id.rawValue))\n\n[File](.attachments/\(uploaded.id.rawValue))"
  )
  let linkedNote = try await client.save(linked.pendingWrite())
  let connection = try await client.connections(linkedID)
  #expect(connection.links.contains(id))
  #expect(connection.attachments.contains(uploaded.id))
  #expect(try await client.backlinks(id).contains(where: { $0.name == linkedID }))

  let history = try await client.history(id)
  let version = try #require(history.first)
  let saved = try await client.saved(version, trash: false)
  let restored = try await client.restore(version, current: updated)
  #expect(restored.body == saved.body)
  try await client.delete(restored)
  let trash = try await client.trash()
  let deleted = try #require(trash.entries.first(where: { $0.name == id }))
  #expect(try await client.saved(deleted, trash: true).body == restored.body)
  let replacement = try await client.save(Draft(id: id, body: "# Existing identity").pendingWrite())
  let recovered = try await client.restore(deleted, current: nil)
  #expect(recovered.body == restored.body)
  #expect(recovered.name != replacement.name)
  #expect(try await client.read(id).body == replacement.body)
  try await client.delete(recovered)
  try await client.delete(replacement)
  try await client.delete(linkedNote)
  try await client.deleteAttachment(metadata)
  let currentWorkspace = try #require(
    try await client.workspaces().workspaces.first(where: { $0.id == renamed.id }))
  try await client.deleteWorkspace(currentWorkspace)
}

@Test(.enabled(if: ProcessInfo.processInfo.environment["TUNGSTEN_SERVER"] != nil))
@MainActor func restoringTrashPreservesPendingLocalWriting() async throws {
  let server = try #require(ProcessInfo.processInfo.environment["TUNGSTEN_SERVER"])
  let client = NoteService(address: try ServerAddress(server, permitsLoopback: true))
  let id = try NoteID.generated()
  let created = try await client.save(Draft(id: id, body: "# Recoverable note").pendingWrite())
  try await client.delete(created)
  let record = try #require(try await client.trash().entries.first(where: { $0.name == id }))
  let directory = FileManager.default.temporaryDirectory.appending(path: UUID().uuidString)
  defer { try? FileManager.default.removeItem(at: directory) }
  let store = try DraftStore(directory: directory)
  var pending = Draft(note: created)
  pending.edit("Writing that has not reached the server")
  try store.save(pending, server: client.address)
  let model = try testingModel(service: client, drafts: store)
  #expect(await model.restore(record, fromTrash: true))
  let restored = try #require(model.currentDraft)
  #expect(restored.id != id)
  #expect(restored.body == created.body)
  #expect(try store.load(server: client.address, id: id)?.body == pending.body)
  #expect(try await client.read(restored.id).body == created.body)
  try await client.delete(client.read(restored.id))
}

@Test(.enabled(if: ProcessInfo.processInfo.environment["TUNGSTEN_SERVER"] != nil))
@MainActor func acknowledgedWritingRefreshesTheVisibleNoteTitle() async throws {
  let server = try #require(ProcessInfo.processInfo.environment["TUNGSTEN_SERVER"])
  let client = NoteService(address: try ServerAddress(server, permitsLoopback: true))
  let directory = FileManager.default.temporaryDirectory.appending(path: UUID().uuidString)
  defer { try? FileManager.default.removeItem(at: directory) }
  let model = try testingModel(service: client, drafts: DraftStore(directory: directory))
  await model.loadNotes()
  let title = "Visible workspace \(UUID().uuidString)"
  #expect(await model.createWorkspace(title))
  let workspace = try #require(model.workspaces.first(where: { $0.title == title }))
  #expect(model.collection == .workspace(workspace.id))
  model.newNote()
  let id = try #require(model.activeID)
  model.edit("# Visible creation")
  await model.saveActive()
  #expect(model.notes.first(where: { $0.id == id })?.title == "Visible creation")
  #expect(try await client.workspaceNotes(workspace.id).notes.contains(where: { $0.id == id }))
  model.edit("# Updated visible title")
  await model.saveActive()
  #expect(model.notes.first(where: { $0.id == id })?.title == "Updated visible title")
  try await client.delete(client.read(id))
  let current = try #require(
    try await client.workspaces().workspaces.first(where: { $0.id == workspace.id }))
  try await client.deleteWorkspace(current)
}

@Test(.enabled(if: ProcessInfo.processInfo.environment["TUNGSTEN_SERVER"] != nil))
@MainActor func obsoleteWorkspaceResponsesDoNotChangeTheNewConnection() async throws {
  let server = try #require(ProcessInfo.processInfo.environment["TUNGSTEN_SERVER"])
  for status in [200, 409] {
    let directory = FileManager.default.temporaryDirectory.appending(path: UUID().uuidString)
    defer { try? FileManager.default.removeItem(at: directory) }
    let delayed = SuspendedRead()
    let service = NoteService(address: try ServerAddress("https://previous.example")) { _, _ in
      try await delayed.response()
    }
    let model = try testingModel(service: service, drafts: DraftStore(directory: directory))
    let creating = Task { await model.createWorkspace("Obsolete workspace") }
    while !(await delayed.hasStarted()) { await Task.yield() }
    let previous = try Note.test(body: "Previous server content")
    model.trash = [
      SavedNote(
        id: String(repeating: "a", count: 32), name: previous.name,
        title: previous.title, captured: 0, revision: previous.revision)
    ]
    #expect(await model.connect(server))
    #expect(model.trash.isEmpty)
    let workspace: [String: String] = [
      "id": "99999", "title": "Obsolete workspace",
      "revision": try Note.test(body: "").revision.rawValue,
    ]
    await delayed.completeResponse(
      HTTPResult(status: status, data: try JSONEncoder().encode(workspace)))
    #expect(await creating.value == false)
    #expect(model.collection == .all)
    #expect(model.problem == nil)
  }
}
