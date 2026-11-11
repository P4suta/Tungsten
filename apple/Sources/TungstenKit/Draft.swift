import Foundation

enum SaveStatus: String, Codable, Sendable {
  case saved
  case pending
  case conflict
  case failed
  case unsaved
}

struct PendingWrite: Sendable {
  let id: NoteID
  let body: String
  let revision: Revision?
}

struct Draft: Codable, Identifiable, Sendable {
  let id: NoteID
  private(set) var body: String
  private(set) var savedBody: String?
  private(set) var revision: Revision?
  private(set) var status: SaveStatus
  private(set) var workspace: WorkspaceID?
  var isDirty: Bool { body != savedBody }
  var hasPendingWork: Bool { isDirty || workspace != nil }
  var title: String { NoteTitle.from(body) }

  init(note: Note) {
    id = note.name
    body = note.body
    savedBody = note.body
    revision = note.revision
    status = .saved
  }

  init(id: NoteID, body: String, workspace: WorkspaceID? = nil) {
    self.id = id
    self.body = body
    savedBody = nil
    revision = nil
    status = .pending
    self.workspace = workspace
  }

  mutating func edit(_ body: String) {
    self.body = body
    if status != .conflict { status = hasPendingWork ? .pending : .saved }
  }

  func pendingWrite() throws -> PendingWrite {
    guard status != .conflict else { throw ClientError.conflict }
    guard body.utf8.count <= 1024 * 1024 else { throw ClientError.tooLarge }
    return PendingWrite(id: id, body: body, revision: revision)
  }

  mutating func acknowledge(_ note: Note, writing: PendingWrite) throws {
    guard note.name == id, writing.id == id, writing.revision == revision else {
      throw ClientError.invalidResponse
    }
    revision = note.revision
    savedBody = note.body
    if body == writing.body { body = note.body }
    status = hasPendingWork ? .pending : .saved
  }

  mutating func refresh(_ note: Note) {
    guard note.name == id, note.revision != revision else { return }
    if isDirty && body != note.body {
      status = .conflict
      return
    }
    body = note.body
    savedBody = note.body
    revision = note.revision
    status = hasPendingWork ? .pending : .saved
  }

  mutating func conflict() { status = .conflict }
  mutating func joinedWorkspace() {
    workspace = nil
    status = hasPendingWork ? .pending : .saved
  }
  mutating func failLocally() { status = .unsaved }
  mutating func fail() { status = .failed }
  mutating func retry() { status = hasPendingWork ? .pending : .saved }
}
