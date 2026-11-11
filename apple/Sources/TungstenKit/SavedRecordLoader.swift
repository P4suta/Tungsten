import Foundation
import Observation

enum RecordOrigin: Hashable, Sendable {
  case history
  case trash
}

enum SavedRecordLoad: Equatable {
  case loading
  case loaded(Note)
  case failed
}

struct SavedRecordRequest: Equatable {
  let record: SavedNote
  let origin: RecordOrigin
  let attempt: UUID
}

@MainActor @Observable final class SavedRecordLoader {
  private(set) var state: SavedRecordLoad = .loading
  private var generation = UUID()

  var note: Note? {
    if case .loaded(let value) = state { value } else { nil }
  }

  func load(_ record: SavedNote, origin: RecordOrigin, service: NoteService?) async throws {
    let attempt = UUID()
    generation = attempt
    state = .loading
    do {
      try Task.checkCancellation()
      guard let service else { throw ClientError.invalidServer }
      let note = try await service.saved(record, trash: origin == .trash)
      try Task.checkCancellation()
      guard generation == attempt else { throw CancellationError() }
      state = .loaded(note)
    } catch {
      if generation == attempt { state = .failed }
      throw error
    }
  }
}
