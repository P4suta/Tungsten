import Foundation
import Testing

@testable import TungstenKit

@Test func writesCarryExactRevisionAndEncodedIdentity() async throws {
  let address = try ServerAddress("https://notes.example")
  let note = try Note.test(body: "Saved", revision: "b", name: "notes/with space.md")
  let data = try JSONEncoder().encode(note)
  let client = NoteService(address: address) { request, upload in
    #expect(request.httpMethod == "PUT")
    #expect(
      request.url?.absoluteString == "https://notes.example/api/notes/notes%2Fwith%20space.md")
    #expect(
      request.value(forHTTPHeaderField: "If-Match") == "\"" + String(repeating: "a", count: 64)
        + "\"")
    #expect(request.value(forHTTPHeaderField: "Content-Type") == "application/json")
    #expect(upload == nil)
    return HTTPResult(status: 200, data: data)
  }
  let original = try Note.test(body: "Original", name: "notes/with space.md")
  var draft = Draft(note: original)
  draft.edit("Saved")
  #expect(try await client.save(draft.pendingWrite()).body == "Saved")
}

@Test func creationUsesAStableIdentityForInterruptedRequests() async throws {
  let note = try Note.test(body: "Writing")
  let client = NoteService(address: try ServerAddress("https://notes.example")) { request, _ in
    #expect(request.httpMethod == "POST")
    let body = try JSONDecoder().decode([String: String].self, from: #require(request.httpBody))
    #expect(body["name"] == note.name.rawValue)
    return HTTPResult(status: 201, data: try JSONEncoder().encode(note))
  }
  #expect(
    try await client.save(Draft(id: note.name, body: note.body).pendingWrite()).name == note.name)
}

@Test func serviceErrorsAndMismatchedIdentitiesFailExplicitly() async throws {
  let address = try ServerAddress("https://notes.example")
  for (status, expected) in [
    (412, ClientError.conflict), (409, .conflict), (404, .missing), (413, .tooLarge),
    (503, .server(503)),
  ] {
    let client = NoteService(address: address) { _, _ in HTTPResult(status: status, data: Data()) }
    await #expect(throws: expected) { try await client.read(NoteID("topic.md")) }
  }
  let foreign = try Note.test(body: "Different note", name: "other.md")
  let client = NoteService(address: address) { _, _ in
    HTTPResult(status: 200, data: try JSONEncoder().encode(foreign))
  }
  await #expect(throws: ClientError.invalidResponse) { try await client.read(NoteID("topic.md")) }
}

@Test func notePagesRejectUnboundedOrDuplicateResults() async throws {
  let row = "{\"name\":\"topic.md\",\"title\":\"Topic\",\"snippet\":\"\",\"modified\":1}"
  let payload = "{\"notes\":[" + [row, row].joined(separator: ",") + "],\"total\":2,\"next\":null}"
  let client = NoteService(address: try ServerAddress("https://notes.example")) { _, _ in
    HTTPResult(status: 200, data: Data(payload.utf8))
  }
  await #expect(throws: ClientError.invalidResponse) { try await client.notes(query: "") }
}
