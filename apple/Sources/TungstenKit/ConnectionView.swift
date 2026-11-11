import SwiftUI

enum ConnectionPresentation {
  case initial
  case modal
  case settings
}

struct ConnectionView: View {
  @Bindable var model: AppModel
  let presentation: ConnectionPresentation
  @Environment(\.dismiss) private var dismiss
  @State private var address = ""

  var body: some View {
    Form {
      Section {
        serverField
          .accessibilityLabel(localized("Server Address"))
          .accessibilityIdentifier("server-address")
        Button(localized("Connect"), systemImage: "network") {
          Task {
            if await model.connect(address), presentation != .initial { dismiss() }
          }
        }
        .disabled(
          address.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty || model.connecting
        )
        .accessibilityIdentifier("connect-server")
        .keyboardShortcut(.defaultAction)
        if model.connecting { ProgressView().accessibilityLabel(localized("Connecting")) }
      } footer: {
        Text(localized("Enter the HTTPS address of the existing Tungsten server."))
      }
    }
    #if os(macOS)
      .font(.scaledSystemBody(model.readingScale))
    #endif
    .formStyle(.grouped)
    .presentationSizing(.form)
    .navigationTitle(localized(model.isConnected ? "Server" : "Connect to Server"))
    .toolbarTitleDisplayMode(.inline)
    .onAppear { address = model.service?.address.url.absoluteString ?? "" }
    .toolbar {
      if presentation == .modal {
        ToolbarItem(placement: .cancellationAction) {
          Button(role: .cancel) { dismiss() }
        }
      }
    }
  }

  @ViewBuilder private var serverField: some View {
    #if os(iOS)
      TextField(
        localized("Server Address"), text: $address,
        prompt: Text(verbatim: "https://notes.example.ts.net")
      )
      .keyboardType(.URL)
      .textInputAutocapitalization(.never)
      .autocorrectionDisabled()
    #else
      TextField(
        localized("Server Address"), text: $address,
        prompt: Text(verbatim: "https://notes.example.ts.net")
      )
      .autocorrectionDisabled()
    #endif
  }
}

public struct TungstenSettingsView: View {
  private let model: AppModel
  public init(model: AppModel) { self.model = model }

  public var body: some View {
    ConnectionView(model: model, presentation: .settings)
      .safeAreaInset(edge: .bottom) { ProblemBanner(model: model) }
  }
}

struct WorkspaceEditorView: View {
  let model: AppModel
  let workspace: Workspace?
  @Environment(\.dismiss) private var dismiss
  @State private var title: String
  @State private var saving = false
  @State private var saved = false
  @State private var confirmingDiscard = false

  private var hasUnsavedChanges: Bool { !saved && title != (workspace?.title ?? "") }

  init(model: AppModel, workspace: Workspace?) {
    self.model = model
    self.workspace = workspace
    _title = State(initialValue: workspace?.title ?? "")
  }

  var body: some View {
    NavigationStack {
      Form { workspaceName }
        .formStyle(.grouped)
        #if os(macOS)
          .font(.scaledSystemBody(model.readingScale))
        #endif
        .navigationTitle(localized(workspace == nil ? "Create Workspace" : "Rename Workspace"))
        .toolbarTitleDisplayMode(.inline)
        .toolbar {
          ToolbarItem(placement: .cancellationAction) {
            Button(role: .cancel) {
              if hasUnsavedChanges { confirmingDiscard = true } else { dismiss() }
            }
          }
          ToolbarItem(placement: .confirmationAction) {
            Button(localized("Save"), systemImage: "checkmark") {
              saving = true
              Task {
                let success: Bool
                if let workspace {
                  success = await model.renameWorkspace(workspace, title: title)
                } else {
                  success = await model.createWorkspace(title)
                }
                saving = false
                if success {
                  saved = true
                  dismiss()
                }
              }
            }
            .disabled(
              title.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty || title.count > 120
                || title.utf8.count > 480 || saving
            )
            .accessibilityIdentifier("save-workspace")
            .keyboardShortcut(.defaultAction)
          }
        }
    }
    .presentationSizing(.form)
    .confirmationDialog(
      localized("Discard Workspace Changes?"), isPresented: $confirmingDiscard,
      titleVisibility: .visible
    ) { discardActions }
    #if os(iOS)
      .background {
        SheetDismissalGuard(isModified: hasUnsavedChanges) { confirmingDiscard = true }
      }
      .presentationDetents([.medium, .large])
    #else
      .dismissalConfirmationDialog(
        localized("Discard Workspace Changes?"), shouldPresent: hasUnsavedChanges
      ) { discardActions }
    #endif
  }

  private var workspaceName: some View {
    TextField(localized("Workspace Name"), text: $title)
      .accessibilityLabel(localized("Workspace Name"))
      .accessibilityIdentifier("workspace-name")
  }

  private var discardActions: some View {
    Group {
      Button(localized("Discard Changes"), role: .destructive) {
        title = workspace?.title ?? ""
        dismiss()
      }
      Button(localized("Keep Editing"), role: .cancel) {}
    }
  }
}

struct WorkspacePickerView: View {
  let model: AppModel
  @Environment(\.dismiss) private var dismiss

  var body: some View {
    NavigationStack {
      List {
        ForEach(model.workspaces) { workspace in
          Button {
            Task { if await model.addCurrent(to: workspace) { dismiss() } }
          } label: {
            Label(workspace.title, systemImage: "folder")
          }
        }
        if model.workspaceCursor != nil {
          Button(localized("Load More"), systemImage: "arrow.down") {
            Task { await model.loadWorkspaces(more: true) }
          }
        }
      }
      .font(.scaledSystemBody(model.readingScale))
      .accessibilityLabel(localized("Workspaces"))
      .overlay {
        if model.workspaces.isEmpty {
          ContentUnavailableView(
            localized("No Workspaces"), systemImage: "folder",
            description: Text(localized("Create a workspace in Collections.")))
        }
      }
      .navigationTitle(localized("Add to Workspace"))
      .toolbarTitleDisplayMode(.inline)
      .toolbar {
        ToolbarItem(placement: .cancellationAction) {
          Button(role: .cancel) { dismiss() }
        }
      }
    }
    .task { await model.loadWorkspaces() }
    .presentationSizing(.page)
  }
}

struct LinkPickerView: View {
  let model: AppModel
  @Environment(\.dismiss) private var dismiss
  @State private var query = ""
  @State private var notes: [NoteSummary] = []
  @State private var cursor: String?
  @State private var loading = false

  var body: some View {
    NavigationStack {
      List {
        ForEach(notes) { note in
          Button {
            model.insertLink(WorkspaceNote(name: note.name, title: note.title))
            dismiss()
          } label: {
            Label(note.title, systemImage: "note.text")
          }
        }
        if cursor != nil {
          Button(localized("Load More"), systemImage: "arrow.down") {
            Task { await load(more: true) }
          }
          .disabled(loading)
        }
      }
      .font(.scaledSystemBody(model.readingScale))
      .accessibilityLabel(localized("Insert Note Link"))
      .searchable(text: $query, prompt: Text(localized("Search Notes")))
      .navigationTitle(localized("Insert Note Link"))
      .toolbarTitleDisplayMode(.inline)
      .toolbar {
        ToolbarItem(placement: .cancellationAction) {
          Button(role: .cancel) { dismiss() }
        }
      }
      .overlay { if loading { ProgressView().accessibilityLabel(localized("Loading Notes")) } }
    }
    .task(id: query) {
      do { try await Task.sleep(for: .milliseconds(250)) } catch { return }
      await load(more: false)
    }
    .presentationSizing(.page)
  }

  private func load(more: Bool) async {
    guard let service = model.service else { return }
    let search = query
    loading = true
    defer { loading = false }
    do {
      let page = try await service.notes(query: search, cursor: more ? cursor : nil)
      guard !Task.isCancelled, search == query else { return }
      if more {
        let present = Set(notes.map(\.id))
        notes.append(contentsOf: page.notes.filter { !present.contains($0.id) })
      } else {
        notes = page.notes
      }
      cursor = page.next
    } catch { model.report(error) }
  }
}
