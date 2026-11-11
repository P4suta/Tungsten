import Foundation

enum ClientError: Error, Equatable, LocalizedError {
  case invalidServer
  case invalidResponse
  case invalidIdentity
  case conflict
  case missing
  case tooLarge
  case disconnected
  case draftStorage
  case draftRecovery
  case server(Int)
  case operationFailed

  var errorDescription: String? {
    switch self {
    case .invalidServer: "Enter the server's HTTPS address without a path, password, or query."
    case .invalidResponse: "The server returned an invalid response."
    case .invalidIdentity: "The saved identifier is invalid."
    case .conflict: "The item changed on another device. Reopen the item and retry."
    case .missing: "The item is no longer available."
    case .tooLarge: "Notes must be at most 1 MiB and attachments at most 64 MiB."
    case .disconnected: "Unable to reach the server. Check the address and network connection."
    case .draftStorage: "Unable to save the draft on this device. Keep the editor open and retry."
    case .draftRecovery:
      "Some local drafts could not be read, and their original files have been kept."
    case .server: "The server could not complete the operation. Keep the draft and retry."
    case .operationFailed: "Unable to complete the operation. Keep the draft and retry."
    }
  }
}

struct ServerAddress: Hashable, Sendable {
  let url: URL

  init(_ value: String, permitsLoopback: Bool = false) throws {
    let value = value.trimmingCharacters(in: .whitespacesAndNewlines)
    guard var parts = URLComponents(string: value),
      let host = parts.host?.lowercased(), !host.isEmpty,
      parts.user == nil, parts.password == nil, parts.query == nil, parts.fragment == nil,
      parts.path.isEmpty || parts.path == "/",
      parts.port.map({ (1...65535).contains($0) }) ?? true
    else { throw ClientError.invalidServer }
    let loopback = ["localhost", "127.0.0.1", "[::1]", "::1"].contains(host)
    guard parts.scheme == "https" || (parts.scheme == "http" && permitsLoopback && loopback)
    else { throw ClientError.invalidServer }
    parts.host = host
    parts.path = ""
    if parts.port == (parts.scheme == "https" ? 443 : 80) { parts.port = nil }
    guard let url = parts.url else { throw ClientError.invalidServer }
    self.url = url
  }
}

protocol StringValue: Codable, Hashable, Sendable {
  var rawValue: String { get }
  init(_ value: String) throws
}

extension StringValue {
  init(from decoder: any Decoder) throws {
    try self.init(decoder.singleValueContainer().decode(String.self))
  }

  func encode(to encoder: any Encoder) throws {
    var container = encoder.singleValueContainer()
    try container.encode(rawValue)
  }
}

struct NoteID: StringValue {
  let rawValue: String

  init(_ value: String) throws {
    let parts = value.split(separator: "/", omittingEmptySubsequences: false)
    guard (4...240).contains(value.utf8.count), value.hasSuffix(".md"),
      !value.dropLast(3).hasSuffix(" "), parts.count <= 16,
      !value.utf8.contains(where: { $0 < 32 || $0 == 127 || $0 == 92 }),
      parts.allSatisfy({
        !$0.isEmpty && !$0.hasPrefix(".") && !$0.hasPrefix(" ") && !$0.hasSuffix(" ")
      })
    else { throw ClientError.invalidIdentity }
    rawValue = value
  }

  static func generated() throws -> NoteID {
    try NoteID(UUID().uuidString.replacingOccurrences(of: "-", with: "").lowercased() + ".md")
  }
}

struct Revision: StringValue {
  let rawValue: String

  init(_ value: String) throws {
    guard value.utf8.count == 66, value.first == "\"", value.last == "\"",
      value.dropFirst().dropLast().utf8.allSatisfy({
        (48...57).contains($0) || (65...70).contains($0) || (97...102).contains($0)
      })
    else { throw ClientError.invalidResponse }
    rawValue = value
  }
}

struct AttachmentID: StringValue {
  let rawValue: String

  init(_ value: String) throws {
    let parts = value.split(separator: ".", omittingEmptySubsequences: false)
    guard parts.count == 2, parts[0].count == 32, (1...12).contains(parts[1].count),
      parts[0].utf8.allSatisfy({ (48...57).contains($0) || (97...102).contains($0) }),
      parts[1].utf8.allSatisfy({
        (48...57).contains($0) || (65...90).contains($0) || (97...122).contains($0)
      })
    else { throw ClientError.invalidIdentity }
    rawValue = value
  }
}

struct WorkspaceID: StringValue {
  let rawValue: String

  init(_ value: String) throws {
    guard let number = Int64(value), number > 0, String(number) == value else {
      throw ClientError.invalidIdentity
    }
    rawValue = value
  }
}

struct Note: Codable, Equatable, Sendable, Identifiable {
  let name: NoteID
  let body: String
  let modified: Double
  let revision: Revision
  var id: NoteID { name }
  var title: String { NoteTitle.from(body) }

  init(name: NoteID, body: String, modified: Double, revision: Revision) throws {
    guard body.utf8.count <= 1024 * 1024 else { throw ClientError.tooLarge }
    guard modified.isFinite, modified >= 0 else { throw ClientError.invalidResponse }
    self.name = name
    self.body = body
    self.modified = modified
    self.revision = revision
  }

  init(from decoder: any Decoder) throws {
    let fields = try decoder.container(keyedBy: CodingKeys.self)
    try self.init(
      name: fields.decode(NoteID.self, forKey: .name),
      body: fields.decode(String.self, forKey: .body),
      modified: fields.decode(Double.self, forKey: .modified),
      revision: fields.decode(Revision.self, forKey: .revision))
  }
}

enum NoteTitle {
  static func from(_ body: String) -> String {
    let lines = body.split(whereSeparator: \.isNewline)
    let heading = lines.first(where: { $0.hasPrefix("# ") })?.dropFirst(2)
    let title = heading.map(String.init) ?? lines.first.map(String.init) ?? "Untitled Note"
    return title.trimmingCharacters(in: .whitespaces)
  }
}

struct NoteSummary: Decodable, Identifiable, Sendable {
  let name: NoteID
  let title: String
  let snippet: String
  let modified: Double
  var id: NoteID { name }
}

struct NotePage: Decodable, Sendable {
  let notes: [NoteSummary]
  let next: String?
  let total: Int
}

struct Workspace: Decodable, Hashable, Identifiable, Sendable {
  let id: WorkspaceID
  let title: String
  let revision: Revision
}

struct WorkspacePage: Decodable, Sendable {
  let workspaces: [Workspace]
  let next: String?
  let revision: Revision
}

struct WorkspaceNote: Decodable, Identifiable, Sendable {
  let name: NoteID
  let title: String
  var id: NoteID { name }
}

struct WorkspaceNotes: Decodable, Sendable {
  let workspace: Workspace
  let notes: [WorkspaceNote]
  let next: String?
}

struct SavedNote: Decodable, Hashable, Identifiable, Sendable {
  let id: String
  let name: NoteID
  let title: String
  let captured: Double
  let revision: Revision
  var date: Date { Date(timeIntervalSince1970: captured / 1000) }
}

struct TrashPage: Decodable, Sendable {
  let entries: [SavedNote]
  let next: String?
}

struct Connection: Decodable, Sendable {
  let name: NoteID
  let links: [NoteID]
  let attachments: [AttachmentID]
}

struct Attachment: Decodable, Identifiable, Sendable {
  let id: AttachmentID
  let name: String
  let bytes: Int
  let revision: Revision
  let kind: String
}
