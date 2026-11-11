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
  @State private var previewURL: URL?
  @State private var previewDirectory: URL?
  @State private var downloading = false
  @State private var confirmingDeletion = false
  @State private var deleted = false

  var body: some View {
    Group {
      if deleted {
        Label(localized("Original File Deleted"), systemImage: "doc.badge.ellipsis")
      } else {
        Button {
          Task { await open() }
        } label: {
          Label(attachment?.name ?? localized("Loading File"), systemImage: symbol)
        }
        .disabled(downloading || attachment == nil)
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
    .quickLookPreview($previewURL)
    .task {
      do { attachment = try await model.service?.attachment(id) } catch { model.report(error) }
    }
    .onChange(of: previewURL) { if previewURL == nil { cleanup() } }
    .onDisappear { cleanup() }
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
    Button(localized("Open File"), systemImage: "doc") { Task { await open() } }
    Button(localized("Delete Original"), systemImage: "trash", role: .destructive) {
      confirmingDeletion = true
    }
  }

  private func open() async {
    guard let attachment, let service = model.service, !downloading else { return }
    downloading = true
    defer { downloading = false }
    do {
      let source = try await service.download(attachment)
      defer { try? FileManager.default.removeItem(at: source) }
      let directory = FileManager.default.temporaryDirectory.appending(path: UUID().uuidString)
      try FileManager.default.createDirectory(
        at: directory, withIntermediateDirectories: false, attributes: [.posixPermissions: 0o700])
      previewDirectory = directory
      let name = URL(fileURLWithPath: attachment.name).lastPathComponent
      let destination = directory.appending(path: name)
      try FileManager.default.moveItem(at: source, to: destination)
      previewURL = destination
    } catch {
      cleanup()
      model.report(error)
    }
  }

  private func cleanup() {
    if let previewDirectory { try? FileManager.default.removeItem(at: previewDirectory) }
    previewDirectory = nil
  }
}

enum RecordOrigin {
  case history
  case trash
}

struct SavedRecordContent: View {
  let model: AppModel
  let record: SavedNote
  let origin: RecordOrigin
  @Environment(\.dismiss) private var dismiss
  @State private var note: Note?
  @State private var restoring = false

  var body: some View {
    ScrollView {
      if let note {
        Text(note.body)
          .font(.scaledSystemBody(model.readingScale))
          .textSelection(.enabled)
          .frame(maxWidth: .infinity, alignment: .leading)
          .padding()
          .accessibilityIdentifier("saved-note-text")
      } else {
        ProgressView().accessibilityLabel(localized("Loading Version"))
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
        .disabled(note == nil || restoring)
        .accessibilityIdentifier("restore-record")
      }
    }
    .task {
      do {
        let value = try await model.service?.saved(record, trash: origin == .trash)
        guard value?.name == record.name else { throw ClientError.invalidResponse }
        note = value
      } catch { model.report(error) }
    }
    .presentationSizing(.page)
  }
}
