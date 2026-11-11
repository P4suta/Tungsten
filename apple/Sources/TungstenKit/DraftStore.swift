import CryptoKit
import Foundation

struct DraftStore: Sendable {
  let directory: URL

  private struct Record: Codable {
    let format: Int
    let server: String
    let draft: Draft
  }

  init(directory: URL) throws {
    self.directory = directory
    try FileManager.default.createDirectory(
      at: directory, withIntermediateDirectories: true, attributes: [.posixPermissions: 0o700])
  }

  private func path(server: ServerAddress, id: NoteID) -> URL {
    let identity = server.url.absoluteString + "\n" + id.rawValue
    let hash = SHA256.hash(data: Data(identity.utf8)).map { String(format: "%02x", $0) }.joined()
    return directory.appending(path: hash + ".json")
  }

  func save(_ draft: Draft, server: ServerAddress) throws {
    guard draft.body.utf8.count <= 1024 * 1024 else { throw ClientError.tooLarge }
    let data = try JSONEncoder().encode(
      Record(format: 1, server: server.url.absoluteString, draft: draft))
    let destination = path(server: server, id: draft.id)
    try data.write(
      to: destination, options: [.atomic, .completeFileProtectionUntilFirstUserAuthentication])
    try FileManager.default.setAttributes(
      [.posixPermissions: 0o600], ofItemAtPath: destination.path)
  }

  func load(server: ServerAddress, id: NoteID) throws -> Draft? {
    let destination = path(server: server, id: id)
    guard FileManager.default.fileExists(atPath: destination.path) else { return nil }
    let record = try read(destination)
    guard record.server == server.url.absoluteString, record.draft.id == id else {
      throw ClientError.invalidResponse
    }
    return record.draft
  }

  func all(server: ServerAddress) throws -> [Draft] {
    try FileManager.default.contentsOfDirectory(at: directory, includingPropertiesForKeys: nil)
      .filter { $0.pathExtension == "json" }
      .map(read)
      .filter { $0.server == server.url.absoluteString && $0.draft.hasPendingWork }
      .map(\.draft)
      .sorted { $0.id.rawValue < $1.id.rawValue }
  }

  private func read(_ url: URL) throws -> Record {
    let values = try url.resourceValues(forKeys: [
      .fileSizeKey, .isRegularFileKey, .isSymbolicLinkKey,
    ])
    guard values.isRegularFile == true, values.isSymbolicLink != true,
      let size = values.fileSize, size <= 13 * 1024 * 1024
    else { throw ClientError.invalidResponse }
    let record = try JSONDecoder().decode(Record.self, from: Data(contentsOf: url))
    guard record.format == 1, record.draft.body.utf8.count <= 1024 * 1024,
      (record.draft.savedBody?.utf8.count ?? 0) <= 1024 * 1024,
      (record.draft.revision == nil) == (record.draft.savedBody == nil)
    else { throw ClientError.invalidResponse }
    return record
  }

  func remove(server: ServerAddress, id: NoteID) throws {
    let destination = path(server: server, id: id)
    if FileManager.default.fileExists(atPath: destination.path) {
      try FileManager.default.removeItem(at: destination)
    }
  }
}
