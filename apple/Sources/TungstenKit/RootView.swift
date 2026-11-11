import SwiftUI
import UniformTypeIdentifiers

func localized(_ key: String) -> String {
  String(localized: String.LocalizationValue(key), bundle: .module)
}

@MainActor public struct TungstenRootView: View {
  @Bindable private var model: AppModel
  @Environment(\.scenePhase) private var scenePhase
  @State private var compactColumn: NavigationSplitViewColumn = .content

  public init(model: AppModel) { self.model = model }

  public var body: some View {
    Group {
      if model.isConnected {
        NavigationSplitView(preferredCompactColumn: $compactColumn) {
          CollectionsView(model: model)
        } content: {
          NoteListView(model: model)
        } detail: {
          NoteEditorView(model: model)
        }
        .navigationSplitViewStyle(.balanced)
        .id(model.service?.address)
      } else {
        NavigationStack { ConnectionView(model: model, presentation: .initial) }
      }
    }
    .task { await model.start() }
    .task(id: model.query) {
      do { try await Task.sleep(for: .milliseconds(250)) } catch { return }
      await model.loadNotes()
    }
    .onChange(of: model.collection) {
      compactColumn = .content
      Task { await model.loadNotes() }
    }
    .onChange(of: model.activeID) { compactColumn = model.activeID == nil ? .content : .detail }
    .onChange(of: scenePhase) {
      Task {
        if scenePhase == .active {
          await model.refresh()
        } else {
          await model.saveActive()
        }
      }
    }
    .task {
      while !Task.isCancelled {
        do { try await Task.sleep(for: .seconds(15)) } catch { return }
        if scenePhase == .active { await model.refreshActive() }
      }
    }
    .sheet(item: $model.sheet) { sheet in
      Group {
        switch sheet {
        case .details:
          if let draft = model.currentDraft { NoteDetailsView(model: model, noteID: draft.id) }
        case .connection:
          NavigationStack { ConnectionView(model: model, presentation: .modal) }
        case .linkPicker: LinkPickerView(model: model)
        case .workspacePicker: WorkspacePickerView(model: model)
        case .workspaceEditor(let workspace):
          WorkspaceEditorView(model: model, workspace: workspace)
        case .trashRecord(let record):
          NavigationStack { SavedRecordContent(model: model, record: record, origin: .trash) }
            .presentationSizing(.page)
        }
      }
      .accessibilityElement(children: .contain)
      .accessibilityLabel(sheetTitle(sheet))
      .safeAreaInset(edge: .bottom) { ProblemBanner(model: model) }
    }
    .fileImporter(isPresented: $model.importingFile, allowedContentTypes: [.item]) { result in
      switch result {
      case .success(let file): Task { await model.upload(file) }
      case .failure(let error): model.report(error)
      }
    }
    .safeAreaInset(edge: .bottom) { if model.sheet == nil { ProblemBanner(model: model) } }
  }

  private func sheetTitle(_ sheet: AppSheet) -> String {
    switch sheet {
    case .details: localized("Note Details")
    case .connection: localized("Server")
    case .linkPicker: localized("Insert Note Link")
    case .workspacePicker: localized("Add to Workspace")
    case .workspaceEditor(let workspace):
      localized(workspace == nil ? "Create Workspace" : "Rename Workspace")
    case .trashRecord: localized("Trash")
    }
  }
}

private struct CollectionsView: View {
  @Bindable var model: AppModel
  @State private var deletingWorkspace: Workspace?

  var body: some View {
    List(
      selection: Binding<NoteCollection?>(
        get: { model.collection }, set: { if let value = $0 { model.selectCollection(value) } })
    ) {
      CollectionRow(collection: .all, title: localized("All Notes"), symbol: "note.text")
        .accessibilityIdentifier("collection-all")
      CollectionRow(collection: .drafts, title: localized("Drafts"), symbol: "square.and.pencil")
        .accessibilityIdentifier("collection-drafts")
      CollectionRow(collection: .trash, title: localized("Trash"), symbol: "trash")
        .accessibilityIdentifier("collection-trash")
      Section(localized("Workspaces")) {
        ForEach(model.workspaces) { workspace in
          CollectionRow(
            collection: .workspace(workspace.id), title: workspace.title, symbol: "folder"
          )
          .accessibilityIdentifier("workspace-\(workspace.id.rawValue)")
          .contextMenu { workspaceActions(workspace) }
          .swipeActions(allowsFullSwipe: false) { workspaceActions(workspace) }
        }
        if model.workspaceCursor != nil {
          Button(localized("Load More"), systemImage: "arrow.down") {
            Task { await model.loadWorkspaces(more: true) }
          }
        }
        Button(localized("New Workspace"), systemImage: "folder.badge.plus") {
          model.sheet = .workspaceEditor(nil)
        }
        .accessibilityIdentifier("create-workspace")
        .buttonStyle(.borderless)
        .fixedSize(horizontal: false, vertical: true)
      }
    }
    .font(.scaledSystemBody(model.readingScale))
    .accessibilityLabel(localized("Collections"))
    .navigationTitle(localized("Collections"))
    .toolbar {
      #if os(iOS)
        ToolbarItem(placement: .primaryAction) {
          Button(localized("Server"), systemImage: "server.rack") { model.sheet = .connection }
            .accessibilityIdentifier("server-settings")
        }
      #endif
    }
    .confirmationDialog(
      localized("Delete Workspace?"),
      isPresented: Binding(
        get: { deletingWorkspace != nil }, set: { if !$0 { deletingWorkspace = nil } }),
      titleVisibility: .visible
    ) {
      Button(localized("Delete Workspace"), role: .destructive) {
        if let workspace = deletingWorkspace { Task { await model.deleteWorkspace(workspace) } }
      }
    } message: {
      Text(localized("The notes remain in All Notes."))
    }
  }

  @ViewBuilder private func workspaceActions(_ workspace: Workspace) -> some View {
    Button(localized("Rename Workspace"), systemImage: "pencil") {
      model.sheet = .workspaceEditor(workspace)
    }
    Button(localized("Delete Workspace"), systemImage: "trash", role: .destructive) {
      deletingWorkspace = workspace
    }
  }
}

private struct NoteListView: View {
  @Bindable var model: AppModel
  @State private var emptyingTrash = false

  var body: some View {
    Group {
      if model.collection == .trash {
        List {
          ForEach(model.trash) { record in
            Button {
              model.sheet = .trashRecord(record)
            } label: {
              Label {
                VStack(alignment: .leading) {
                  Text(record.title).foregroundStyle(Color.primary)
                  Text(record.date, style: .date).font(.caption).foregroundStyle(Color.secondary)
                }
              } icon: {
                Image(systemName: "note.text")
              }
            }
            .contextMenu {
              Button(localized("Restore Note"), systemImage: "arrow.uturn.backward") {
                model.sheet = .trashRecord(record)
              }
            }
            .swipeActions {
              Button(localized("Restore Note"), systemImage: "arrow.uturn.backward") {
                model.sheet = .trashRecord(record)
              }
            }
          }
          if model.trashCursor != nil {
            Button(localized("Load More"), systemImage: "arrow.down") {
              Task { await model.loadTrash(more: true) }
            }
          }
        }
        .accessibilityLabel(localized("Trash"))
        .overlay {
          if model.trash.isEmpty && !model.loadingNotes {
            emptyState("Trash Is Empty", symbol: "trash")
          }
        }
        .toolbar {
          ToolbarItem(placement: .primaryAction) {
            Button(localized("Empty Trash"), systemImage: "trash.slash", role: .destructive) {
              emptyingTrash = true
            }
            .disabled(model.trash.isEmpty)
          }
        }
      } else {
        List(
          selection: Binding<NoteID?>(
            get: { model.activeID }, set: { if let id = $0 { Task { await model.open(id) } } })
        ) {
          ForEach(model.notes) { entry in
            NoteRow(entry: entry)
              .accessibilityIdentifier("note-\(entry.id.rawValue)")
              .contextMenu { noteActions(entry) }
              .swipeActions(allowsFullSwipe: false) { noteActions(entry) }
          }
          if model.noteCursor != nil {
            Button(localized("Load More"), systemImage: "arrow.down") {
              Task { await model.loadNotes(more: true) }
            }
          }
        }
        .accessibilityLabel(model.collectionTitle)
        .overlay {
          if model.notes.isEmpty && !model.loadingNotes {
            ContentUnavailableView {
              Label(
                localized(model.query.isEmpty ? "No Notes" : "No Results"), systemImage: "note.text"
              )
              .accessibilityIdentifier("empty-notes")
            } description: {
              Text(
                localized(
                  model.query.isEmpty
                    ? "Create a note to start writing." : "Try a different search."))
            } actions: {
              Button(localized("New Note"), systemImage: "square.and.pencil") { model.newNote() }
                .accessibilityIdentifier("empty-create-note")
            }
          }
        }
        .searchable(text: $model.query, prompt: Text(localized("Search Notes")))
        .toolbar {
          ToolbarItem(placement: .primaryAction) {
            Button(localized("New Note"), systemImage: "square.and.pencil") { model.newNote() }
              .accessibilityIdentifier("create-note")
          }
        }
      }
    }
    .font(.scaledSystemBody(model.readingScale))
    .accessibilityElement(children: .contain)
    .accessibilityLabel(model.collectionTitle)
    .navigationTitle(model.collectionTitle)
    .overlay {
      if model.loadingNotes { ProgressView().accessibilityLabel(localized("Loading Notes")) }
    }
    .refreshable { await model.loadNotes() }
    .confirmationDialog(
      localized("Empty Trash?"), isPresented: $emptyingTrash, titleVisibility: .visible
    ) {
      Button(localized("Empty Trash"), role: .destructive) { Task { await model.emptyTrash() } }
    } message: {
      Text(localized("Deleted notes will be permanently removed."))
    }
  }

  private func emptyState(_ title: String, symbol: String) -> some View {
    ContentUnavailableView(localized(title), systemImage: symbol)
  }

  @ViewBuilder private func noteActions(_ entry: WorkspaceNote) -> some View {
    Button(localized("Open Note"), systemImage: "note.text") { Task { await model.open(entry.id) } }
    if case .workspace(let id) = model.collection {
      Button(localized("Remove from Workspace"), systemImage: "folder.badge.minus") {
        Task { await model.remove(entry.id, from: id) }
      }
    }
  }
}

private struct CollectionRow: View {
  let collection: NoteCollection
  let title: String
  let symbol: String

  var body: some View {
    #if os(macOS)
      Label(title, systemImage: symbol)
        .fixedSize(horizontal: false, vertical: true)
        .tag(collection)
    #else
      NavigationLink(value: collection) {
        Label(title, systemImage: symbol).fixedSize(horizontal: false, vertical: true)
      }
    #endif
  }
}

private struct NoteRow: View {
  let entry: WorkspaceNote

  var body: some View {
    #if os(macOS)
      Label(entry.title, systemImage: "note.text")
        .fixedSize(horizontal: false, vertical: true)
        .tag(entry.id)
    #else
      NavigationLink(value: entry.id) { Label(entry.title, systemImage: "note.text") }
    #endif
  }
}

private struct NoteEditorView: View {
  @Bindable var model: AppModel

  var body: some View {
    Group {
      if let draft = model.currentDraft {
        TextEditor(
          text: Binding(
            get: {
              model.currentDraft?.id == draft.id
                ? model.currentDraft?.body ?? draft.body : draft.body
            }, set: { if model.activeID == draft.id { model.edit($0) } })
        )
        .font(.scaledSystemBody(model.readingScale))
        .padding()
        .accessibilityLabel(localized("Note Text"))
        .accessibilityIdentifier("note-editor")
        .id(draft.id)
        .navigationTitle(draft.title)
        .safeAreaInset(edge: .bottom) { saveStatus(draft) }
        .toolbar {
          ToolbarItem(placement: .primaryAction) {
            Button(localized("Attach File"), systemImage: "paperclip") {
              model.importingFile = true
            }
            .disabled(model.uploading)
            .accessibilityIdentifier("attach-file")
          }
          ToolbarItem(placement: .primaryAction) {
            Button(localized("Note Details"), systemImage: "info.circle") {
              model.sheet = .details
            }
            .accessibilityIdentifier("note-details")
          }
        }
      } else if model.openingNote {
        ProgressView().accessibilityLabel(localized("Opening Note"))
      } else {
        ContentUnavailableView {
          Label(localized("Select a Note"), systemImage: "note.text")
        } description: {
          Text(localized("Open a note or create a note to start writing."))
        } actions: {
          Button(localized("New Note"), systemImage: "square.and.pencil") { model.newNote() }
            .accessibilityIdentifier("detail-create-note")
        }
      }
    }
    .accessibilityElement(children: .contain)
    .accessibilityLabel(localized("Note Text"))
  }

  @ViewBuilder private func saveStatus(_ draft: Draft) -> some View {
    VStack(alignment: .leading) {
      Label(localized(statusText(draft.status)), systemImage: statusSymbol(draft.status))
        .font(.scaledSystemBody(model.readingScale))
        .foregroundStyle(Color.secondary)
        .accessibilityIdentifier("save-status")
      if draft.status == .conflict {
        Text(localized("The saved note changed. Your writing is saved on this device."))
        Button(localized("Save as New Note"), systemImage: "doc.badge.plus") {
          Task { await model.saveCopy() }
        }
        .accessibilityIdentifier("save-copy")
      } else if draft.status == .failed || draft.status == .unsaved {
        Button(localized("Retry Save"), systemImage: "arrow.clockwise") {
          Task { await model.retrySave() }
        }
        .accessibilityIdentifier("retry-save")
      }
    }
    .font(.scaledSystemBody(model.readingScale))
    .frame(maxWidth: .infinity, alignment: .leading)
    .padding()
  }

  private func statusText(_ status: SaveStatus) -> String {
    switch status {
    case .saved: "Saved"
    case .pending: "Draft Saved on This Device"
    case .failed: "Unable to Save to Server"
    case .unsaved: "Unable to Save on This Device"
    case .conflict: "Conflicting Changes"
    }
  }

  private func statusSymbol(_ status: SaveStatus) -> String {
    switch status {
    case .saved: "checkmark.circle"
    case .pending: "square.and.pencil"
    case .failed: "exclamationmark.arrow.circlepath"
    case .unsaved: "exclamationmark.triangle"
    case .conflict: "exclamationmark.triangle"
    }
  }
}

public struct TungstenCommands: Commands {
  private let model: AppModel
  public init(model: AppModel) { self.model = model }

  public var body: some Commands {
    #if os(macOS)
      CommandGroup(after: .toolbar) {
        Button(localized("Refresh"), systemImage: "arrow.clockwise") {
          Task { await model.refresh() }
        }
        .keyboardShortcut("r")
        .disabled(!model.isConnected)
        Divider()
        Button(localized("Zoom In"), systemImage: "plus.magnifyingglass") {
          model.setReadingScale(model.readingScale.larger)
        }
        .keyboardShortcut("+", modifiers: .command)
        .disabled(model.readingScale == .twice)
        Button(localized("Zoom Out"), systemImage: "minus.magnifyingglass") {
          model.setReadingScale(model.readingScale.smaller)
        }
        .keyboardShortcut("-", modifiers: .command)
        .disabled(model.readingScale == .standard)
        Button(localized("Actual Size"), systemImage: "textformat.size") {
          model.setReadingScale(.standard)
        }
        .keyboardShortcut("0", modifiers: .command)
      }
    #endif
    CommandGroup(replacing: .newItem) {
      Button(localized("New Note"), systemImage: "square.and.pencil") { model.newNote() }
        .keyboardShortcut("n")
        .disabled(!model.isConnected)
    }
    CommandGroup(replacing: .saveItem) {
      Button(localized("Save"), systemImage: "square.and.arrow.down") {
        Task { await model.saveActive() }
      }
      .keyboardShortcut("s")
      .disabled(model.currentDraft == nil)
    }
    CommandMenu(localized("Note")) {
      Button(localized("Note Details"), systemImage: "info.circle") { model.sheet = .details }
        .keyboardShortcut("i", modifiers: [.command, .option])
        .disabled(model.currentDraft == nil)
      Button(localized("Insert Note Link"), systemImage: "link") { model.sheet = .linkPicker }
        .keyboardShortcut("k")
        .disabled(model.currentDraft == nil)
      Button(localized("Attach File"), systemImage: "paperclip") { model.importingFile = true }
        .disabled(model.currentDraft == nil)
      Button(localized("Move to Trash"), systemImage: "trash", role: .destructive) {
        Task { await model.deleteCurrent() }
      }
      .disabled(model.currentDraft == nil)
    }
  }
}
