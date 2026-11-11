import Foundation
import OSLog

struct HTTPResult: Sendable {
  let status: Int
  let data: Data
}

typealias SendRequest = @Sendable (URLRequest, URL?) async throws -> HTTPResult

private final class RejectRedirects: NSObject, URLSessionTaskDelegate {
  func urlSession(
    _ session: URLSession, task: URLSessionTask,
    willPerformHTTPRedirection response: HTTPURLResponse, newRequest request: URLRequest
  ) async -> URLRequest? {
    nil
  }
}

final class NetworkSession: Sendable {
  private static let logger = Logger(subsystem: "com.p4suta.Tungsten", category: "network")
  let session: URLSession
  private let delegate = RejectRedirects()

  init() {
    let configuration = URLSessionConfiguration.ephemeral
    configuration.waitsForConnectivity = false
    configuration.timeoutIntervalForRequest = 20
    configuration.timeoutIntervalForResource = 120
    configuration.httpShouldSetCookies = false
    configuration.urlCache = nil
    session = URLSession(configuration: configuration)
  }

  deinit { session.invalidateAndCancel() }

  func send(_ request: URLRequest, upload: URL?) async throws -> HTTPResult {
    let operation = UUID().uuidString
    Self.logger.debug("operation=\(operation, privacy: .public) phase=request outcome=started")
    defer {
      Self.logger.debug("operation=\(operation, privacy: .public) phase=request outcome=finished")
    }
    do {
      if let upload {
        let (data, response) = try await session.upload(
          for: request, fromFile: upload, delegate: delegate)
        guard let response = response as? HTTPURLResponse, data.count <= 8 * 1024 * 1024 else {
          throw ClientError.invalidResponse
        }
        return HTTPResult(status: response.statusCode, data: data)
      }
      let (bytes, response) = try await session.bytes(for: request, delegate: delegate)
      guard let response = response as? HTTPURLResponse else { throw ClientError.invalidResponse }
      guard response.expectedContentLength <= 8 * 1024 * 1024 else { throw ClientError.tooLarge }
      var data = Data()
      for try await byte in bytes {
        guard data.count < 8 * 1024 * 1024 else { throw ClientError.tooLarge }
        data.append(byte)
      }
      return HTTPResult(status: response.statusCode, data: data)
    } catch is CancellationError {
      throw CancellationError()
    } catch let error as ClientError {
      throw error
    } catch {
      if Task.isCancelled { throw CancellationError() }
      throw ClientError.disconnected
    }
  }

  func download(_ request: URLRequest) async throws -> URL {
    do {
      let (file, response) = try await session.download(for: request, delegate: delegate)
      do {
        guard let response = response as? HTTPURLResponse else { throw ClientError.invalidResponse }
        try NoteService.check(response.statusCode)
        let size = try file.resourceValues(forKeys: [.fileSizeKey]).fileSize
        guard let size, size <= 64 * 1024 * 1024 else { throw ClientError.tooLarge }
        return file
      } catch {
        try? FileManager.default.removeItem(at: file)
        throw error
      }
    } catch let error as ClientError {
      throw error
    } catch {
      if Task.isCancelled { throw CancellationError() }
      throw ClientError.disconnected
    }
  }
}

struct NoteService: Sendable {
  let address: ServerAddress
  private let send: SendRequest
  private let network: NetworkSession?

  init(address: ServerAddress, send: SendRequest? = nil) {
    self.address = address
    if let send {
      self.send = send
      network = nil
    } else {
      let network = NetworkSession()
      self.network = network
      self.send = { request, upload in try await network.send(request, upload: upload) }
    }
  }

  private func request(
    _ parts: [String], method: String = "GET", query: [URLQueryItem] = [], body: Data? = nil,
    revision: Revision? = nil
  ) throws -> URLRequest {
    guard var url = URLComponents(url: address.url, resolvingAgainstBaseURL: false) else {
      throw ClientError.invalidServer
    }
    let allowed = CharacterSet(
      charactersIn: "abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789-._~")
    let encoded = try parts.map { part in
      guard let value = part.addingPercentEncoding(withAllowedCharacters: allowed) else {
        throw ClientError.invalidIdentity
      }
      return value
    }
    url.percentEncodedPath = "/" + encoded.joined(separator: "/")
    if !query.isEmpty { url.queryItems = query }
    guard let destination = url.url else { throw ClientError.invalidServer }
    var request = URLRequest(url: destination, cachePolicy: .reloadIgnoringLocalCacheData)
    request.httpMethod = method
    request.httpBody = body
    request.setValue("application/json", forHTTPHeaderField: "Content-Type")
    request.setValue("application/json", forHTTPHeaderField: "Accept")
    request.setValue(address.url.absoluteString, forHTTPHeaderField: "Origin")
    if let revision { request.setValue(revision.rawValue, forHTTPHeaderField: "If-Match") }
    return request
  }

  static func check(_ status: Int) throws {
    switch status {
    case 200..<300: return
    case 409, 412: throw ClientError.conflict
    case 404: throw ClientError.missing
    case 413: throw ClientError.tooLarge
    default: throw ClientError.server(status)
    }
  }

  private func response<T: Decodable>(_ type: T.Type, _ request: URLRequest, upload: URL? = nil)
    async throws -> T
  {
    let result = try await send(request, upload)
    try Self.check(result.status)
    guard result.data.count <= 8 * 1024 * 1024 else { throw ClientError.tooLarge }
    do { return try JSONDecoder().decode(type, from: result.data) } catch let error as ClientError {
      throw error
    } catch { throw ClientError.invalidResponse }
  }

  private func empty(_ request: URLRequest) async throws {
    try Self.check(try await send(request, nil).status)
  }

  private func json(_ values: [String: String]) throws -> Data { try JSONEncoder().encode(values) }

  func notes(query: String, cursor: String? = nil) async throws -> NotePage {
    var items = [
      URLQueryItem(name: "paged", value: "true"), URLQueryItem(name: "limit", value: "50"),
      URLQueryItem(name: "q", value: query),
    ]
    if let cursor { items.append(URLQueryItem(name: "cursor", value: cursor)) }
    let page = try await response(NotePage.self, request(["api", "notes"], query: items))
    guard page.notes.count <= 50, page.total >= page.notes.count,
      Set(page.notes.map(\.id)).count == page.notes.count,
      page.next == nil || page.next != cursor
    else { throw ClientError.invalidResponse }
    return page
  }

  func read(_ id: NoteID) async throws -> Note {
    let note = try await response(Note.self, request(["api", "notes", id.rawValue]))
    guard note.name == id else { throw ClientError.invalidResponse }
    return note
  }

  func save(_ writing: PendingWrite) async throws -> Note {
    let outgoing =
      if let revision = writing.revision {
        try request(
          ["api", "notes", writing.id.rawValue], method: "PUT", body: json(["body": writing.body]),
          revision: revision)
      } else {
        try request(
          ["api", "notes"], method: "POST",
          body: json(["name": writing.id.rawValue, "body": writing.body]))
      }
    let note = try await response(Note.self, outgoing)
    guard note.name == writing.id else { throw ClientError.invalidResponse }
    return note
  }

  func delete(_ note: Note) async throws {
    try await empty(
      request(["api", "notes", note.name.rawValue], method: "DELETE", revision: note.revision))
  }

  func workspaces(after: String? = nil) async throws -> WorkspacePage {
    let page = try await response(
      WorkspacePage.self,
      request(
        ["api", "workspaces"], query: after.map { [URLQueryItem(name: "after", value: $0)] } ?? []))
    guard page.workspaces.count <= 50,
      Set(page.workspaces.map(\.id)).count == page.workspaces.count,
      page.next == nil || page.next != after
    else { throw ClientError.invalidResponse }
    return page
  }

  func workspaceNotes(_ id: WorkspaceID, after: String? = nil) async throws -> WorkspaceNotes {
    let page = try await response(
      WorkspaceNotes.self,
      request(
        ["api", "workspaces", id.rawValue, "notes"],
        query: after.map { [URLQueryItem(name: "after", value: $0)] } ?? []))
    guard page.workspace.id == id, page.notes.count <= 50,
      Set(page.notes.map(\.id)).count == page.notes.count,
      page.next == nil || page.next != after
    else { throw ClientError.invalidResponse }
    return page
  }

  func createWorkspace(_ title: String) async throws -> Workspace {
    try await response(
      Workspace.self, request(["api", "workspaces"], method: "POST", body: json(["title": title])))
  }

  func renameWorkspace(_ workspace: Workspace, title: String) async throws -> Workspace {
    try await response(
      Workspace.self,
      request(
        ["api", "workspaces", workspace.id.rawValue], method: "PUT", body: json(["title": title]),
        revision: workspace.revision))
  }

  func deleteWorkspace(_ workspace: Workspace) async throws {
    try await empty(
      request(
        ["api", "workspaces", workspace.id.rawValue], method: "DELETE", revision: workspace.revision
      ))
  }

  func membership(_ workspace: WorkspaceID, note: NoteID, included: Bool) async throws -> Workspace
  {
    try await response(
      Workspace.self,
      request(
        ["api", "workspaces", workspace.rawValue, "notes", note.rawValue],
        method: included ? "PUT" : "DELETE", body: included ? json([:]) : nil))
  }

  func connections(_ id: NoteID) async throws -> Connection {
    let result = try await response(
      [Connection].self,
      request(
        ["api", "connections"],
        query: [
          URLQueryItem(name: "name", value: id.rawValue), URLQueryItem(name: "limit", value: "120"),
        ]))
    guard result.count <= 120, let value = result.first(where: { $0.name == id }) else {
      throw ClientError.invalidResponse
    }
    return value
  }

  func backlinks(_ id: NoteID) async throws -> [NoteSummary] {
    let result = try await response(
      [NoteSummary].self,
      request(["api", "backlinks"], query: [URLQueryItem(name: "name", value: id.rawValue)]))
    guard result.count <= 120 else { throw ClientError.invalidResponse }
    return result
  }

  func history(_ id: NoteID) async throws -> [SavedNote] {
    let result = try await response(
      [SavedNote].self,
      request(["api", "history"], query: [URLQueryItem(name: "name", value: id.rawValue)]))
    guard result.count <= 50 else { throw ClientError.invalidResponse }
    return result
  }

  private func recordID(_ value: String) throws -> String {
    guard value.count == 32,
      value.utf8.allSatisfy({ (48...57).contains($0) || (97...102).contains($0) })
    else { throw ClientError.invalidIdentity }
    return value
  }

  func saved(_ record: SavedNote, trash: Bool) async throws -> Note {
    let note = try await response(
      Note.self, request(["api", trash ? "trash" : "history", recordID(record.id)]))
    guard note.name == record.name else { throw ClientError.invalidResponse }
    return note
  }

  func restore(_ record: SavedNote, current: Note?, name: NoteID? = nil) async throws -> Note {
    guard current == nil || (current?.name == record.name && name == nil) else {
      throw ClientError.invalidIdentity
    }
    let target = current?.name ?? name ?? record.name
    let body = try json(["name": target.rawValue])
    let path = try ["api", current == nil ? "trash" : "history", recordID(record.id), "restore"]
    do {
      let note = try await response(
        Note.self, request(path, method: "POST", body: body, revision: current?.revision))
      guard note.name == target else { throw ClientError.invalidResponse }
      return note
    } catch ClientError.conflict where current == nil {
      let name = try NoteID.generated()
      let note = try await response(
        Note.self, request(path, method: "POST", body: json(["name": name.rawValue])))
      guard note.name == name else { throw ClientError.invalidResponse }
      return note
    }
  }

  func trash(cursor: String? = nil) async throws -> TrashPage {
    var query = [URLQueryItem(name: "page", value: "true")]
    if let cursor { query.append(URLQueryItem(name: "cursor", value: cursor)) }
    let page = try await response(TrashPage.self, request(["api", "trash"], query: query))
    guard page.entries.count <= 50, page.next == nil || page.next != cursor else {
      throw ClientError.invalidResponse
    }
    return page
  }

  func emptyTrash() async throws { try await empty(request(["api", "trash"], method: "DELETE")) }

  func attachment(_ id: AttachmentID) async throws -> Attachment {
    let value = try await response(Attachment.self, request(["api", "attachments", id.rawValue]))
    guard value.id == id, (0...64 * 1024 * 1024).contains(value.bytes) else {
      throw ClientError.invalidResponse
    }
    return value
  }

  func upload(_ file: URL) async throws -> Attachment {
    let size = try file.resourceValues(forKeys: [.fileSizeKey]).fileSize
    guard let size, (0...64 * 1024 * 1024).contains(size) else { throw ClientError.tooLarge }
    guard
      let name = file.lastPathComponent.addingPercentEncoding(
        withAllowedCharacters: .alphanumerics), name.utf8.count <= 720
    else { throw ClientError.invalidIdentity }
    var upload = try request(["api", "attachments"], method: "POST")
    upload.setValue("application/octet-stream", forHTTPHeaderField: "Content-Type")
    upload.setValue(name, forHTTPHeaderField: "X-Attachment-Name")
    upload.setValue(String(size), forHTTPHeaderField: "Content-Length")
    return try await response(Attachment.self, upload, upload: file)
  }

  func download(_ attachment: Attachment) async throws -> URL {
    guard let network else { throw ClientError.invalidResponse }
    return try await network.download(request([".attachments", attachment.id.rawValue]))
  }

  func deleteAttachment(_ attachment: Attachment) async throws {
    try await empty(
      request(
        ["api", "attachments", attachment.id.rawValue], method: "DELETE",
        revision: attachment.revision))
  }
}
