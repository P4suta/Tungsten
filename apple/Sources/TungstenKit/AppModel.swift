import Foundation
import Observation

enum NoteCollection: Hashable, Sendable {
  case all
  case drafts
  case trash
  case workspace(WorkspaceID)
}

enum AppSheet: Hashable, Identifiable {
  case details
  case connection
  case linkPicker
  case workspacePicker
  case workspaceEditor(Workspace?)
  case trashRecord(SavedNote)
  var id: Self { self }
}

struct AppProblem: Identifiable {
  let id = UUID()
  let message: String
}

@MainActor @Observable public final class AppModel {
  var service: NoteService?
  var activeID: NoteID?
  var collection: NoteCollection = .all
  var query = ""
  var notes: [WorkspaceNote] = []
  var workspaces: [Workspace] = []
  var trash: [SavedNote] = []
  var recoveredDrafts: [Draft] = []
  var noteCursor: String?
  var workspaceCursor: String?
  var trashCursor: String?
  var problem: AppProblem?
  var loadingNotes = false
  var openingNote = false
  var connecting = false
  var sheet: AppSheet?
  var importingFile = false
  var uploading = false
  var readingScale = ReadingScale.standard
  var currentDraft: Draft? { activeID.flatMap { drafts[$0] } }
  var isConnected: Bool { service != nil }
  var collectionTitle: String {
    switch collection {
    case .all: localized("All Notes")
    case .drafts: localized("Drafts")
    case .trash: localized("Trash")
    case .workspace(let id):
      workspaces.first(where: { $0.id == id })?.title ?? localized("Workspace")
    }
  }

  @ObservationIgnored private var draftStore: DraftStore?
  private var drafts: [NoteID: Draft] = [:]
  @ObservationIgnored private var scheduled: [NoteID: Task<Void, Never>] = [:]
  @ObservationIgnored private var saving: [NoteID: Task<Void, Never>] = [:]
  @ObservationIgnored private var connectionGeneration = UUID()
  @ObservationIgnored private var selectionGeneration = UUID()
  @ObservationIgnored private var listingGeneration = UUID()
  @ObservationIgnored private var preferences = UserDefaults.standard

  public init() {
    do {
      let support = try FileManager.default.url(
        for: .applicationSupportDirectory, in: .userDomainMask, appropriateFor: nil, create: true)
      var directory = support.appending(path: "Tungsten/Drafts")
      #if DEBUG
        let arguments = ProcessInfo.processInfo.arguments
        if let index = arguments.firstIndex(of: "--tungsten-test-session"),
          arguments.indices.contains(index + 1),
          let session = UUID(uuidString: arguments[index + 1])
        {
          directory = support.appending(path: "Tungsten/UITests/\(session.uuidString)")
          guard let isolated = UserDefaults(suiteName: "Tungsten.UITests.\(session.uuidString)")
          else { throw ClientError.draftStorage }
          preferences = isolated
        }
      #endif
      draftStore = try DraftStore(directory: directory)
      readingScale = ReadingScale(rawValue: preferences.double(forKey: "readingScale")) ?? .standard
      if let stored = preferences.string(forKey: "serverAddress") {
        service = NoteService(address: try Self.address(stored))
      }
    } catch { report(error) }
  }

  init(service: NoteService, drafts: DraftStore, preferences: UserDefaults) {
    self.service = service
    draftStore = drafts
    self.preferences = preferences
  }

  static func address(_ text: String) throws -> ServerAddress {
    #if DEBUG
      try ServerAddress(text, permitsLoopback: true)
    #else
      try ServerAddress(text)
    #endif
  }

  func setReadingScale(_ scale: ReadingScale) {
    readingScale = scale
    preferences.set(scale.rawValue, forKey: "readingScale")
  }

  func report(_ error: any Error) {
    guard !(error is CancellationError), !Task.isCancelled else { return }
    guard (error as? CocoaError)?.code != .userCancelled else { return }
    let value = error as? ClientError ?? .operationFailed
    problem = AppProblem(message: value.errorDescription ?? "The operation failed.")
  }

  private func persist(_ draft: Draft) -> Bool {
    guard let service, let draftStore else {
      report(ClientError.draftStorage)
      return false
    }
    do {
      if draft.hasPendingWork {
        try draftStore.save(draft, server: service.address)
      } else {
        try draftStore.remove(server: service.address, id: draft.id)
      }
      return true
    } catch {
      drafts[draft.id]?.failLocally()
      report(ClientError.draftStorage)
      return false
    }
  }

  private func preserveCurrent() -> Bool { currentDraft.map(persist) ?? true }

  func start() async {
    guard service != nil else { return }
    await loadNotes()
    await loadWorkspaces()
    refreshDrafts()
    if activeID == nil, let service,
      let value = preferences.string(forKey: "lastNote.\(service.address.url.absoluteString)"),
      let id = try? NoteID(value)
    {
      await open(id)
    }
  }

  func connect(_ text: String) async -> Bool {
    guard !connecting, preserveCurrent(), draftStore != nil else { return false }
    connecting = true
    defer { connecting = false }
    do {
      let candidate = NoteService(address: try Self.address(text))
      _ = try await candidate.notes(query: "")
      for task in scheduled.values { task.cancel() }
      scheduled.removeAll()
      saving.removeAll()
      connectionGeneration = UUID()
      selectionGeneration = UUID()
      service = candidate
      activeID = nil
      drafts = [:]
      notes = []
      workspaces = []
      trash = []
      recoveredDrafts = []
      noteCursor = nil
      workspaceCursor = nil
      trashCursor = nil
      importingFile = false
      if sheet != .connection { sheet = nil }
      collection = .all
      query = ""
      preferences.set(candidate.address.url.absoluteString, forKey: "serverAddress")
      await start()
      return true
    } catch {
      report(error)
      return false
    }
  }

  func refreshDrafts() {
    guard let service, let draftStore else { return }
    do { recoveredDrafts = try draftStore.all(server: service.address) } catch { report(error) }
  }

  func selectCollection(_ selection: NoteCollection) {
    guard preserveCurrent() else { return }
    activeID = nil
    selectionGeneration = UUID()
    openingNote = false
    collection = selection
  }

  func refresh() async {
    let connection = connectionGeneration
    await loadNotes(keepingNotes: true)
    guard connectionGeneration == connection else { return }
    await loadWorkspaces()
    guard connectionGeneration == connection else { return }
    await refreshActive()
  }

  func newNote() {
    guard service != nil, preserveCurrent() else { return }
    do {
      let id = try NoteID.generated()
      let workspace: WorkspaceID?
      if case .workspace(let value) = collection { workspace = value } else { workspace = nil }
      let draft = Draft(id: id, body: "", workspace: workspace)
      guard persist(draft) else { return }
      drafts[id] = draft
      activeID = id
      rememberSelection(id)
      selectionGeneration = UUID()
      openingNote = false
      refreshDrafts()
    } catch { report(error) }
  }

  func open(_ id: NoteID) async {
    guard let service, let draftStore, preserveCurrent() else { return }
    let generation = UUID()
    let connection = connectionGeneration
    selectionGeneration = generation
    activeID = id
    rememberSelection(id)
    openingNote = true
    defer { if selectionGeneration == generation { openingNote = false } }
    do {
      if drafts[id] == nil { drafts[id] = try draftStore.load(server: service.address, id: id) }
      if drafts[id]?.revision == nil, drafts[id] != nil { return }
      let startingRevision = drafts[id]?.revision
      let note = try await service.read(id)
      guard selectionGeneration == generation, connectionGeneration == connection, !Task.isCancelled
      else { return }
      guard drafts[id]?.revision == startingRevision else { return }
      if var draft = drafts[id] {
        draft.refresh(note)
        drafts[id] = draft
        _ = persist(draft)
      } else {
        drafts[id] = Draft(note: note)
      }
    } catch {
      guard selectionGeneration == generation, connectionGeneration == connection else { return }
      if drafts[id] == nil { activeID = nil }
      report(error)
    }
  }

  private func rememberSelection(_ id: NoteID) {
    guard let service else { return }
    preferences.set(id.rawValue, forKey: "lastNote.\(service.address.url.absoluteString)")
  }

  func edit(_ body: String) {
    guard let id = activeID, var draft = drafts[id] else { return }
    draft.edit(body)
    drafts[id] = draft
    guard persist(draft) else { return }
    schedule(id)
  }

  private func schedule(_ id: NoteID) {
    scheduled[id]?.cancel()
    scheduled[id] = Task { [weak self] in
      do { try await Task.sleep(for: .milliseconds(600)) } catch { return }
      self?.scheduled[id] = nil
      await self?.save(id)
    }
  }

  func save(_ id: NoteID) async {
    let connection = connectionGeneration
    if let pending = saving[id] {
      await pending.value
      guard connectionGeneration == connection else { return }
      if drafts[id]?.hasPendingWork == true { await save(id) }
      return
    }
    let pending = Task { [weak self] in
      guard let self, connectionGeneration == connection else { return }
      await performSave(id)
    }
    saving[id] = pending
    await pending.value
  }

  private func performSave(_ id: NoteID) async {
    let connection = connectionGeneration
    defer { if connectionGeneration == connection { saving.removeValue(forKey: id) } }
    guard let service, let draft = drafts[id], draft.hasPendingWork, persist(draft)
    else { return }
    do {
      if draft.isDirty {
        let writing = try draft.pendingWrite()
        let note: Note
        do { note = try await service.save(writing) } catch ClientError.conflict
          where writing.revision == nil
        {
          let existing = try await service.read(id)
          guard existing.body == writing.body else { throw ClientError.conflict }
          note = existing
        }
        guard connectionGeneration == connection, var current = drafts[id] else { return }
        try current.acknowledge(note, writing: writing)
        drafts[id] = current
        _ = persist(current)
      }
      if let workspace = drafts[id]?.workspace {
        let updatedWorkspace = try await service.membership(workspace, note: id, included: true)
        guard connectionGeneration == connection else { return }
        updateWorkspace(updatedWorkspace)
        drafts[id]?.joinedWorkspace()
        if let current = drafts[id] { _ = persist(current) }
      }
      refreshDrafts()
      await loadNotes(keepingNotes: true)
      guard connectionGeneration == connection else { return }
      if drafts[id]?.isDirty == true { schedule(id) }
    } catch {
      guard connectionGeneration == connection, var current = drafts[id] else { return }
      if error as? ClientError == .conflict { current.conflict() } else { current.fail() }
      drafts[id] = current
      _ = persist(current)
      report(error)
    }
  }

  func saveActive() async { if let id = activeID { await save(id) } }

  func retrySave() async {
    guard let id = activeID else { return }
    drafts[id]?.retry()
    await save(id)
  }

  func saveCopy() async {
    guard let original = currentDraft, preserveCurrent() else { return }
    do {
      let id = try NoteID.generated()
      let copy = Draft(id: id, body: original.body)
      guard persist(copy) else { return }
      drafts[id] = copy
      activeID = id
      rememberSelection(id)
      selectionGeneration = UUID()
      await save(id)
    } catch { report(error) }
  }

  func refreshActive() async {
    guard let service, let id = activeID, let draft = drafts[id], !draft.isDirty else { return }
    let generation = selectionGeneration
    let connection = connectionGeneration
    do {
      let note = try await service.read(id)
      guard generation == selectionGeneration, connection == connectionGeneration else { return }
      guard drafts[id]?.revision == draft.revision else { return }
      drafts[id]?.refresh(note)
    } catch {
      guard generation == selectionGeneration, connection == connectionGeneration,
        drafts[id]?.revision == draft.revision
      else { return }
      if error as? ClientError == .missing {
        if var local = drafts[id], local.isDirty {
          local.conflict()
          drafts[id] = local
          _ = persist(local)
        } else {
          drafts.removeValue(forKey: id)
          activeID = nil
          sheet = nil
          preferences.removeObject(forKey: "lastNote.\(service.address.url.absoluteString)")
          selectionGeneration = UUID()
        }
      } else if error as? ClientError != .disconnected {
        report(error)
      }
    }
  }

  func loadNotes(more: Bool = false, keepingNotes: Bool = false) async {
    guard let service else { return }
    let generation = UUID()
    listingGeneration = generation
    let connection = connectionGeneration
    let selection = collection
    let search = query
    let cursor = more ? noteCursor : nil
    if !more {
      if !keepingNotes { notes = [] }
      noteCursor = nil
    }
    loadingNotes = true
    defer { if listingGeneration == generation { loadingNotes = false } }
    do {
      let values: [WorkspaceNote]
      let following: String?
      switch selection {
      case .all:
        let page = try await service.notes(query: search, cursor: cursor)
        values = page.notes.map { WorkspaceNote(name: $0.name, title: $0.title) }
        following = page.next
      case .workspace(let id):
        let page = try await service.workspaceNotes(id, after: cursor)
        guard listingGeneration == generation, connectionGeneration == connection else { return }
        updateWorkspace(page.workspace)
        values = page.notes
        following = page.next
      case .drafts:
        refreshDrafts()
        values = recoveredDrafts.filter {
          search.isEmpty || $0.body.localizedStandardContains(search)
        }.map { WorkspaceNote(name: $0.id, title: $0.title) }
        following = nil
      case .trash:
        await loadTrash()
        return
      }
      guard listingGeneration == generation, connectionGeneration == connection, !Task.isCancelled
      else { return }
      if more {
        let present = Set(notes.map(\.id))
        notes.append(contentsOf: values.filter { !present.contains($0.id) })
      } else {
        notes = values
      }
      noteCursor = following
    } catch { if listingGeneration == generation { report(error) } }
  }

  func loadWorkspaces(more: Bool = false) async {
    guard let service else { return }
    let connection = connectionGeneration
    do {
      let page = try await service.workspaces(after: more ? workspaceCursor : nil)
      guard connectionGeneration == connection, !Task.isCancelled else { return }
      if more {
        let present = Set(workspaces.map(\.id))
        workspaces.append(contentsOf: page.workspaces.filter { !present.contains($0.id) })
      } else {
        workspaces = page.workspaces
      }
      workspaceCursor = page.next
    } catch { if connectionGeneration == connection { report(error) } }
  }

  func loadTrash(more: Bool = false) async {
    guard let service else { return }
    let connection = connectionGeneration
    do {
      let page = try await service.trash(cursor: more ? trashCursor : nil)
      guard connectionGeneration == connection, !Task.isCancelled else { return }
      if more {
        let present = Set(trash.map(\.id))
        trash.append(contentsOf: page.entries.filter { !present.contains($0.id) })
      } else {
        trash = page.entries
      }
      trashCursor = page.next
    } catch { if connectionGeneration == connection { report(error) } }
  }

  func createWorkspace(_ title: String) async -> Bool {
    guard let service else { return false }
    let connection = connectionGeneration
    do {
      let workspace = try await service.createWorkspace(title)
      guard connectionGeneration == connection else { return false }
      await loadWorkspaces()
      guard connectionGeneration == connection else { return false }
      collection = .workspace(workspace.id)
      return true
    } catch {
      if connectionGeneration == connection { report(error) }
      return false
    }
  }

  func renameWorkspace(_ workspace: Workspace, title: String) async -> Bool {
    guard let service else { return false }
    let connection = connectionGeneration
    do {
      _ = try await service.renameWorkspace(workspace, title: title)
      guard connectionGeneration == connection else { return false }
      await loadWorkspaces()
      return connectionGeneration == connection
    } catch {
      if connectionGeneration == connection { report(error) }
      return false
    }
  }

  func deleteWorkspace(_ workspace: Workspace) async {
    guard let service else { return }
    let connection = connectionGeneration
    do {
      try await service.deleteWorkspace(workspace)
      guard connectionGeneration == connection else { return }
      if collection == .workspace(workspace.id) { collection = .all }
      await loadWorkspaces()
    } catch { if connectionGeneration == connection { report(error) } }
  }

  func addCurrent(to workspace: Workspace) async -> Bool {
    guard let id = activeID, let service else { return false }
    let connection = connectionGeneration
    await save(id)
    guard connectionGeneration == connection, drafts[id]?.isDirty == false else { return false }
    do {
      let updated = try await service.membership(workspace.id, note: id, included: true)
      guard connectionGeneration == connection else { return false }
      updateWorkspace(updated)
      await loadNotes()
      return true
    } catch {
      if connectionGeneration == connection { report(error) }
      return false
    }
  }

  func remove(_ id: NoteID, from workspace: WorkspaceID) async {
    guard let service else { return }
    let connection = connectionGeneration
    do {
      let updated = try await service.membership(workspace, note: id, included: false)
      guard connectionGeneration == connection else { return }
      updateWorkspace(updated)
      await loadNotes()
    } catch { if connectionGeneration == connection { report(error) } }
  }

  private func updateWorkspace(_ workspace: Workspace) {
    if let index = workspaces.firstIndex(where: { $0.id == workspace.id }) {
      workspaces[index] = workspace
    } else {
      workspaces.append(workspace)
    }
  }

  private func savedCurrent() throws -> Note {
    guard let draft = currentDraft, !draft.isDirty, let revision = draft.revision else {
      throw ClientError.conflict
    }
    return try Note(name: draft.id, body: draft.body, modified: 0, revision: revision)
  }

  func deleteCurrent() async {
    guard let service, let id = activeID else { return }
    let connection = connectionGeneration
    await save(id)
    guard connectionGeneration == connection else { return }
    do {
      let note = try savedCurrent()
      guard note.name == id else { return }
      try await service.delete(note)
      guard connectionGeneration == connection else { return }
      scheduled.removeValue(forKey: id)?.cancel()
      if var current = drafts[id], current.body != note.body {
        current.conflict()
        drafts[id] = current
        _ = persist(current)
      } else {
        drafts.removeValue(forKey: id)
        try draftStore?.remove(server: service.address, id: id)
        if activeID == id {
          activeID = nil
          selectionGeneration = UUID()
          preferences.removeObject(forKey: "lastNote.\(service.address.url.absoluteString)")
        }
      }
      await loadNotes()
      refreshDrafts()
      await loadWorkspaces()
    } catch { if connectionGeneration == connection { report(error) } }
  }

  func restore(_ record: SavedNote, fromTrash: Bool) async -> Bool {
    guard let service, preserveCurrent() else { return false }
    let connection = connectionGeneration
    let selection = selectionGeneration
    do {
      if !fromTrash { await saveActive() }
      guard connectionGeneration == connection else { return false }
      let current = try fromTrash ? nil : savedCurrent()
      guard fromTrash || current?.name == record.name else { throw ClientError.conflict }
      let local =
        try drafts[record.name] ?? draftStore?.load(server: service.address, id: record.name)
      let destination = try fromTrash && local?.hasPendingWork == true ? NoteID.generated() : nil
      let note = try await service.restore(record, current: current, name: destination)
      guard connectionGeneration == connection else { return false }
      var restored = drafts[note.name] ?? Draft(note: note)
      restored.refresh(note)
      drafts[note.name] = restored
      if selectionGeneration == selection {
        activeID = note.name
        rememberSelection(note.name)
        selectionGeneration = UUID()
      }
      _ = persist(restored)
      await loadNotes()
      if fromTrash { await loadTrash() }
      await loadWorkspaces()
      return true
    } catch {
      if connectionGeneration == connection { report(error) }
      return false
    }
  }

  func emptyTrash() async {
    guard let service else { return }
    let connection = connectionGeneration
    do {
      try await service.emptyTrash()
      guard connectionGeneration == connection else { return }
      await loadTrash()
    } catch { if connectionGeneration == connection { report(error) } }
  }

  func insertLink(_ note: WorkspaceNote) {
    guard let draft = currentDraft else { return }
    let label = note.title.replacingOccurrences(of: "\\", with: "\\\\").replacingOccurrences(
      of: "[", with: "\\["
    ).replacingOccurrences(of: "]", with: "\\]")
    guard
      let target = note.name.rawValue.addingPercentEncoding(
        withAllowedCharacters: .urlPathAllowed.subtracting(CharacterSet(charactersIn: "?#()")))
    else { return }
    let prefix = String(repeating: "../", count: draft.id.rawValue.split(separator: "/").count - 1)
    edit(draft.body + "\n[\(label)](\(prefix)\(target))\n")
  }

  func upload(_ file: URL) async {
    guard let service, let id = activeID, !uploading else { return }
    let connection = connectionGeneration
    uploading = true
    let access = file.startAccessingSecurityScopedResource()
    defer {
      if access { file.stopAccessingSecurityScopedResource() }
      uploading = false
    }
    do {
      let attachment = try await service.upload(file)
      guard connectionGeneration == connection, var draft = drafts[id] else { return }
      let name = attachment.name.replacingOccurrences(of: "\\", with: "\\\\").replacingOccurrences(
        of: "[", with: "\\["
      ).replacingOccurrences(of: "]", with: "\\]")
      let prefix = attachment.kind == "image" ? "!" : ""
      draft.edit(draft.body + "\n\(prefix)[\(name)](.attachments/\(attachment.id.rawValue))\n")
      drafts[id] = draft
      if persist(draft) { await save(id) }
    } catch { if connectionGeneration == connection { report(error) } }
  }
}
