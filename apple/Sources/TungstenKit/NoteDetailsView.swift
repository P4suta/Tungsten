import QuickLook
import SwiftUI

struct NoteDetailsView: View {
  let model: AppModel
  let noteID: NoteID
  @Environment(\.dismiss) private var dismiss
  @State private var connection: Connection?
  @State private var incoming: [NoteSummary] = []
  @State private var history: [SavedNote] = []
  @State private var historyPath: [SavedNote] = []
  @State private var loading = true
  @State private var linkLimit = 50
  @State private var fileLimit = 50

  var body: some View {
    NavigationStack(path: $historyPath) {
      List {
        Section {
          Button(localized("Insert Note Link"), systemImage: "link") {
            model.sheet = .linkPicker
          }
          .accessibilityIdentifier("insert-note-link")
          Button(localized("Add to Workspace"), systemImage: "folder.badge.plus") {
            model.sheet = .workspacePicker
          }
          .accessibilityIdentifier("add-to-workspace")
        }
        Section(localized("Linked Notes")) {
          ForEach(Array((connection?.links ?? []).prefix(linkLimit)), id: \.self) { id in
            LinkedNoteRow(model: model, id: id) { dismiss() }
          }
          if connection?.links.isEmpty == true { Text(localized("No Linked Notes")) }
          if (connection?.links.count ?? 0) > linkLimit {
            Button(localized("Load More"), systemImage: "arrow.down") { linkLimit += 50 }
          }
        }
        Section(localized("Incoming Links")) {
          ForEach(incoming) { note in
            Button {
              Task {
                await model.open(note.id)
                dismiss()
              }
            } label: {
              Label(note.title, systemImage: "link")
            }
          }
          if incoming.isEmpty && !loading { Text(localized("No Incoming Links")) }
        }
        Section(localized("Attachments")) {
          ForEach(Array((connection?.attachments ?? []).prefix(fileLimit)), id: \.self) { id in
            AttachmentRow(model: model, id: id)
          }
          if connection?.attachments.isEmpty == true { Text(localized("No Attachments")) }
          if (connection?.attachments.count ?? 0) > fileLimit {
            Button(localized("Load More"), systemImage: "arrow.down") { fileLimit += 50 }
          }
        }
        Section(localized("Version History")) {
          ForEach(history) { record in
            Group {
              #if os(macOS)
                Button {
                  historyPath.append(record)
                } label: {
                  versionLabel(record)
                }
                .buttonStyle(.borderless)
              #else
                NavigationLink(value: record) {
                  versionLabel(record)
                }
              #endif
            }
            .accessibilityIdentifier("history-\(record.id)")
          }
          if history.isEmpty && !loading { Text(localized("No Saved Versions")) }
        }
        Section {
          Button(localized("Move to Trash"), systemImage: "trash", role: .destructive) {
            model.sheet = nil
            Task { await model.deleteCurrent() }
          }
          .accessibilityIdentifier("trash-current-note")
        }
      }
      .navigationDestination(for: SavedNote.self) { record in
        SavedRecordContent(model: model, record: record, origin: .history)
      }
      .font(.scaledSystemBody(model.readingScale))
      .accessibilityLabel(localized("Note Details"))
      .navigationTitle(localized("Note Details"))
      .toolbarTitleDisplayMode(.inline)
      .toolbar {
        ToolbarItem(placement: .cancellationAction) {
          Button(role: .close) { dismiss() }
            .accessibilityIdentifier("close-note-details")
        }
      }
      .overlay {
        if loading { ProgressView().accessibilityLabel(localized("Loading Note Details")) }
      }
    }
    .task { await load() }
    .presentationSizing(.page)
  }

  private func versionLabel(_ record: SavedNote) -> some View {
    Label {
      Text(record.date, format: .dateTime)
    } icon: {
      Image(systemName: "clock.arrow.circlepath")
    }
  }

  private func load() async {
    guard let service = model.service else { return }
    loading = true
    defer { loading = false }
    do {
      await model.save(noteID)
      guard model.currentDraft?.revision != nil else { return }
      async let current = service.connections(noteID)
      async let backlinks = service.backlinks(noteID)
      async let versions = service.history(noteID)
      let result = try await (current, backlinks, versions)
      guard !Task.isCancelled else { return }
      connection = result.0
      incoming = result.1
      history = result.2
    } catch { model.report(error) }
  }
}

private struct LinkedNoteRow: View {
  let model: AppModel
  let id: NoteID
  let close: () -> Void
  @State private var title: String?

  var body: some View {
    Button {
      Task {
        await model.open(id)
        close()
      }
    } label: {
      Label(title ?? localized("Loading Note"), systemImage: "note.text")
    }
    .task {
      do { title = try await model.service?.read(id).title } catch {
        title = localized("Unavailable Note")
      }
    }
  }
}

private struct AttachmentRow: View {
  let model: AppModel
  let id: AttachmentID
  @State private var attachment: Attachment?
  @State private var preview = AttachmentPreview()
  @State private var previewRequest: UUID?
  @State private var confirmingDeletion = false
  @State private var deleted = false

  var body: some View {
    Group {
      if deleted {
        Label(localized("Original File Deleted"), systemImage: "doc.badge.ellipsis")
      } else {
        Button {
          previewRequest = UUID()
        } label: {
          Label(attachment?.name ?? localized("Loading File"), systemImage: symbol)
        }
        .disabled(preview.isLoading || attachment == nil)
        .contextMenu { fileActions }
        .swipeActions(allowsFullSwipe: false) { fileActions }
        .confirmationDialog(
          localized("Delete Original File?"), isPresented: $confirmingDeletion,
          titleVisibility: .visible
        ) {
          Button(localized("Delete Original"), role: .destructive) {
            Task {
              do {
                if let attachment {
                  try await model.service?.deleteAttachment(attachment)
                  deleted = true
                }
              } catch { model.report(error) }
            }
          }
        } message: {
          Text(localized("Other notes and saved versions will lose access to the original file."))
        }
      }
    }
    .quickLookPreview(Binding(get: { preview.file }, set: { if $0 == nil { preview.close() } }))
    .task {
      do { attachment = try await model.service?.attachment(id) } catch { model.report(error) }
    }
    .task(id: previewRequest) {
      guard previewRequest != nil else { return }
      await open()
    }
    .onDisappear {
      previewRequest = nil
      preview.close()
    }
  }

  private var symbol: String {
    switch attachment?.kind {
    case "image": "photo"
    case "audio": "waveform"
    case "video": "film"
    case "pdf": "doc.richtext"
    default: "doc"
    }
  }

  @ViewBuilder private var fileActions: some View {
    Button(localized("Open File"), systemImage: "doc") { previewRequest = UUID() }
      .disabled(preview.isLoading || attachment == nil)
    Button(localized("Delete Original"), systemImage: "trash", role: .destructive) {
      confirmingDeletion = true
    }
  }

  private func open() async {
    guard let attachment, let service = model.service else { return }
    do {
      try await preview.open(name: attachment.name) { try await service.download(attachment) }
    } catch { model.report(error) }
  }
}

struct SavedRecordContent: View {
  let model: AppModel
  let record: SavedNote
  let origin: RecordOrigin
  @Environment(\.dismiss) private var dismiss
  @State private var loader = SavedRecordLoader()
  @State private var attempt = UUID()
  @State private var restoring = false

  var body: some View {
    Group {
      switch loader.state {
      case .loaded(let note):
        ScrollView {
          Text(note.body)
            .font(.scaledSystemBody(model.readingScale))
            .textSelection(.enabled)
            .frame(maxWidth: .infinity, alignment: .leading)
            .padding()
            .accessibilityIdentifier("saved-note-text")
        }
      case .loading:
        ProgressView().accessibilityLabel(localized("Loading Version"))
      case .failed:
        ContentUnavailableView {
          Label(localized("Unable to Load Version"), systemImage: "exclamationmark.triangle")
        } actions: {
          Button(localized("Retry"), systemImage: "arrow.clockwise") { attempt = UUID() }
            .accessibilityIdentifier("retry-record")
        }
      }
    }
    .navigationTitle(record.title)
    .toolbar {
      if origin == .trash {
        ToolbarItem(placement: .cancellationAction) {
          Button(role: .cancel) { dismiss() }
        }
      }
      ToolbarItem(placement: .confirmationAction) {
        Button(
          localized(origin == .trash ? "Restore Note" : "Restore Version"),
          systemImage: "arrow.uturn.backward"
        ) {
          restoring = true
          Task {
            let success = await model.restore(record, fromTrash: origin == .trash)
            restoring = false
            if success { dismiss() }
          }
        }
        .disabled(loader.note == nil || restoring)
        .accessibilityIdentifier("restore-record")
      }
    }
    .task(id: SavedRecordRequest(record: record, origin: origin, attempt: attempt)) {
      do {
        try await loader.load(record, origin: origin, service: model.service)
      } catch { model.report(error) }
    }
    .presentationSizing(.page)
  }
}
