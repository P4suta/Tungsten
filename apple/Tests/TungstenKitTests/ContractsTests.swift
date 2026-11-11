import Foundation
import Testing

@testable import TungstenKit

@Test func serverAddressesRequireSecureOrigins() throws {
  #expect(
    try ServerAddress("https://notes.example.ts.net/").url.absoluteString
      == "https://notes.example.ts.net")
  for value in [
    "http://notes.example", "https://person:secret@example.com", "https://example.com/prefix",
    "https://example.com?token=a", "file:///tmp/notes", "https://example.com#note",
  ] {
    #expect(throws: ClientError.self) { try ServerAddress(value) }
  }
  #expect(try ServerAddress("http://127.0.0.1:8080", permitsLoopback: true).url.scheme == "http")
  #expect(throws: ClientError.self) {
    try ServerAddress("http://100.64.0.1", permitsLoopback: true)
  }
  #expect(throws: ClientError.self) { try ServerAddress("http://127.0.0.2", permitsLoopback: true) }
}

@Test func noteIdentitiesAndRevisionsRejectUnsafeValues() throws {
  #expect(try NoteID("notes/topic.md").rawValue == "notes/topic.md")
  for value in [
    "../topic.md", "/topic.md", "topic", "notes//topic.md", "notes/../topic.md", "notes\\topic.md",
    "a.md?query",
  ] {
    #expect(throws: ClientError.self) { try NoteID(value) }
  }
  #expect(throws: ClientError.self) { try Revision("*\r\nInjected: yes") }
  #expect(try Revision("\"" + String(repeating: "a", count: 64) + "\"").rawValue.count == 66)
}

@Test func delayedSavesPreserveLaterTypingAndUseAcknowledgedRevisions() throws {
  let original = try Note.test(body: "Original", revision: "a")
  var draft = Draft(note: original)
  draft.edit("First edit")
  let write = try draft.pendingWrite()
  draft.edit("Later typing")
  let saved = try Note.test(body: "First edit", revision: "b")
  try draft.acknowledge(saved, writing: write)
  #expect(draft.body == "Later typing")
  #expect(draft.isDirty)
  #expect(try draft.pendingWrite().revision == saved.revision)
  #expect(try draft.pendingWrite().body == "Later typing")
}

@Test func staleRefreshesAndConflictsNeverReplaceDrafts() throws {
  let original = try Note.test(body: "Original", revision: "a")
  var draft = Draft(note: original)
  draft.edit("Local writing")
  let remote = try Note.test(body: "Remote writing", revision: "b")
  draft.refresh(remote)
  #expect(draft.body == "Local writing")
  #expect(draft.status == .conflict)
  #expect(throws: ClientError.self) { try draft.pendingWrite() }
  draft.retry()
  #expect(try draft.pendingWrite().revision == original.revision)
  draft.conflict()
  #expect(draft.body == "Local writing")
}

@Test func draftFilesSurviveRelaunchAndRemainIsolatedByServer() throws {
  let directory = FileManager.default.temporaryDirectory.appending(path: UUID().uuidString)
  defer { try? FileManager.default.removeItem(at: directory) }
  let first = try ServerAddress("https://first.example")
  let second = try ServerAddress("https://second.example")
  let note = try Note.test(body: "Original", revision: "a")
  let store = try DraftStore(directory: directory)
  var draft = Draft(note: note)
  draft.edit("Unsaved writing 日本語")
  try store.save(draft, server: first)
  let reopened = try DraftStore(directory: directory)
  #expect(try reopened.load(server: first, id: note.name)?.body == "Unsaved writing 日本語")
  #expect(try reopened.load(server: second, id: note.name) == nil)
  #expect(try reopened.all(server: first).drafts.count == 1)
  try reopened.remove(server: first, id: note.name)
  #expect(try reopened.load(server: first, id: note.name) == nil)
}

@Test func pendingWorkspaceMembershipSurvivesAcknowledgementAndRefresh() throws {
  let original = try Note.test(body: "Original")
  let workspace = try WorkspaceID("1")
  var draft = Draft(id: original.name, body: original.body, workspace: workspace)
  try draft.acknowledge(original, writing: draft.pendingWrite())
  #expect(draft.status == .pending)
  let remote = try Note.test(body: "Changed on another device", revision: "b")
  draft.refresh(remote)
  #expect(draft.body == remote.body)
  #expect(draft.workspace == workspace)
  #expect(draft.hasPendingWork)
  #expect(draft.status == .pending)
  draft.joinedWorkspace()
  #expect(draft.status == .saved)
  #expect(!draft.hasPendingWork)
}

@Test func malformedDraftFilesAreReportedWithoutRemovingTheFile() throws {
  let directory = FileManager.default.temporaryDirectory.appending(path: UUID().uuidString)
  defer { try? FileManager.default.removeItem(at: directory) }
  try FileManager.default.createDirectory(at: directory, withIntermediateDirectories: true)
  let damaged = directory.appending(path: "damaged.json")
  try Data("broken".utf8).write(to: damaged)
  let store = try DraftStore(directory: directory)
  let recovery = try store.all(server: ServerAddress("https://first.example"))
  #expect(recovery.drafts.isEmpty)
  #expect(
    recovery.failures.map { $0.file.resolvingSymlinksInPath() }
      == [damaged.resolvingSymlinksInPath()])
  #expect(FileManager.default.fileExists(atPath: damaged.path))
}

extension Note {
  static func test(body: String, revision: Character = "a", name: String = "topic.md") throws
    -> Note
  {
    try Note(
      name: NoteID(name), body: body, modified: 1,
      revision: Revision("\"" + String(repeating: String(revision), count: 64) + "\""))
  }
}
