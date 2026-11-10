import { notePage } from "./catalog.js";
import type { Snapshot } from "./features.js";
import { icon } from "./icons.js";
import { noteLink } from "./links.js";
import { reconcileRows } from "./rows.js";
import type { SidebarController } from "./sidebar.js";
import { noteTitle } from "./titles.js";
import {
  type NoteEntry,
  validWorkspaceId,
  type Workspace,
  workspace,
  workspaceNotes,
  workspacePage,
} from "./workspaces.js";

interface LibraryPort {
  state(): Snapshot | null;
  api(path: string, method?: string, input?: unknown, revision?: string | null): Promise<unknown>;
  open(name: string): Promise<void>;
  create(): Promise<string | null>;
  sidebar: SidebarController;
}

export interface LibraryController {
  changed(state: Snapshot): void;
}

type Selection =
  | { kind: "all" }
  | { kind: "loading"; id: string }
  | { kind: "workspace"; value: Workspace };

function control<T extends HTMLElement>(id: string, kind: { new (): T }): T {
  const node = document.getElementById(id);
  if (!(node instanceof kind)) throw new Error(`Missing library control: ${id}`);
  return node;
}

function selectedId(): string | null {
  try {
    const value = sessionStorage.getItem("tungsten.workspace.selected");
    return validWorkspaceId(value) ? value : null;
  } catch {
    return null;
  }
}

function remember(id: string | null): void {
  try {
    if (id) sessionStorage.setItem("tungsten.workspace.selected", id);
    else sessionStorage.removeItem("tungsten.workspace.selected");
  } catch {
    return;
  }
}

function failedStatus(error: unknown, status: number): boolean {
  return (
    typeof error === "object" && error !== null && "status" in error && error.status === status
  );
}

export function initLibrary(port: LibraryPort): LibraryController {
  const groups = control("workspace-list", HTMLUListElement);
  const notes = control("sidebar-notes", HTMLUListElement);
  const status = control("library-status", HTMLParagraphElement);
  const retry = control("library-retry", HTMLButtonElement);
  const search = control("sidebar-search", HTMLInputElement);
  const heading = control("library-title", HTMLHeadingElement);
  const form = control("workspace-form", HTMLFormElement);
  const title = control("workspace-title", HTMLInputElement);
  const formStatus = control("workspace-form-status", HTMLParagraphElement);
  const formHeading = control("workspace-form-heading", HTMLHeadingElement);
  const save = control("save-workspace", HTMLButtonElement);
  const create = control("new-note", HTMLButtonElement);
  const addCurrent = control("add-current-note", HTMLButtonElement);
  const previous = control("sidebar-previous", HTMLButtonElement);
  const next = control("sidebar-next", HTMLButtonElement);
  const groupPrevious = control("workspace-previous", HTMLButtonElement);
  const groupNext = control("workspace-next", HTMLButtonElement);
  const restoredId = selectedId();
  let selection: Selection = restoredId ? { kind: "loading", id: restoredId } : { kind: "all" };
  let entries: Workspace[] = [];
  let noteEntries: NoteEntry[] = [];
  let metadataRevision: string | null = null;
  let metadataPath = "";
  let groupIndex = 0;
  const groupCursors = [""];
  let groupNextCursor: string | null = null;
  let index = 0;
  const cursors = [""];
  let nextCursor: string | null = null;
  let epoch = 0;
  let selectionEpoch = 0;
  let needMetadata = true;
  let needNotes = true;
  let task: Promise<void> | null = null;
  let editing: Workspace | null = null;
  let mutation: (() => Promise<void>) | null = null;
  let busy = false;
  let opening: string | null = null;
  let debounce: number | null = null;
  const added = new Set<string>();

  function active(): Workspace | null {
    return selection.kind === "workspace" ? selection.value : null;
  }
  function identity(): string | null {
    return selection.kind === "all"
      ? null
      : selection.kind === "loading"
        ? selection.id
        : selection.value.id;
  }
  function visible(): boolean {
    return (
      port.sidebar.visible("navigation") &&
      port.sidebar.libraryView() === "notes" &&
      !document.hidden
    );
  }
  function announce(): void {
    try {
      localStorage.setItem("tungsten.workspaces.changed", `${Date.now()}:${Math.random()}`);
    } catch {
      return;
    }
  }
  function actions(): void {
    const selected = active();
    addCurrent.hidden = !selected;
    addCurrent.disabled = busy || !port.state()?.name;
    control("rename-workspace", HTMLButtonElement).hidden = !selected;
    control("remove-workspace", HTMLButtonElement).hidden = !selected;
    previous.disabled = index === 0;
    next.disabled = nextCursor === null;
    groupPrevious.disabled = groupIndex === 0;
    groupNext.disabled = groupNextCursor === null;
    control("workspace-pages", HTMLElement).hidden = groupIndex === 0 && groupNextCursor === null;
    control("sidebar-pages", HTMLElement).hidden = index === 0 && nextCursor === null;
    control("all-notes", HTMLButtonElement).setAttribute(
      "aria-pressed",
      String(selection.kind === "all"),
    );
    heading.textContent = search.value.trim()
      ? "Search results"
      : (selected?.title ?? (selection.kind === "loading" ? "Workspace" : "All notes"));
  }
  function choose(value: Workspace | null): void {
    selection = value ? { kind: "workspace", value } : { kind: "all" };
    remember(value?.id ?? null);
    ++selectionEpoch;
    ++epoch;
    index = 0;
    cursors.splice(0, cursors.length, "");
    search.value = "";
    added.clear();
    renderGroups();
    actions();
    requestRefresh(false);
  }
  async function run(action: () => Promise<void>): Promise<void> {
    if (busy) return;
    busy = true;
    mutation = null;
    retry.hidden = true;
    actions();
    try {
      await action();
      announce();
      requestRefresh(true);
    } catch (error) {
      status.textContent = error instanceof Error ? error.message : "The workspace change failed.";
      mutation = action;
      retry.hidden = false;
      if (failedStatus(error, 412) || failedStatus(error, 404)) requestRefresh(true);
    } finally {
      busy = false;
      actions();
    }
  }
  async function include(id: string, name: string, included: boolean): Promise<void> {
    const value = workspace(
      await port.api(
        `/api/workspaces/${id}/notes/${encodeURIComponent(name)}`,
        included ? "PUT" : "DELETE",
        included ? {} : undefined,
      ),
    );
    if (identity() === value.id) {
      selection = { kind: "workspace", value };
      if (included) added.add(name);
      else added.delete(name);
    }
  }
  function renderGroups(): void {
    const selected = active();
    const values =
      selected && !entries.some((entry) => entry.id === selected.id)
        ? [...entries, selected]
        : entries;
    reconcileRows(
      groups,
      values,
      (entry) => entry.id,
      (entry, existing) => {
        const row = existing ?? document.createElement("li");
        if (!existing) {
          const button = document.createElement("button");
          button.type = "button";
          button.append(icon("folder"), document.createElement("span"));
          button.addEventListener("click", () => {
            const value = entries.find((entry) => entry.id === row.dataset.workspace) ?? active();
            if (value) choose(value);
          });
          button.addEventListener("dragover", (event) => {
            if (event.dataTransfer?.types.includes("application/x-tungsten-note")) {
              event.preventDefault();
              event.dataTransfer.dropEffect = "link";
            }
          });
          button.addEventListener("drop", (event) => {
            const name = event.dataTransfer?.getData("application/x-tungsten-note");
            const id = row.dataset.workspace;
            if (!name || !id || !validWorkspaceId(id)) return;
            event.preventDefault();
            void run(() => include(id, name, true));
          });
          row.append(button);
        }
        row.dataset.workspace = entry.id;
        const button = row.querySelector("button");
        const label = button?.querySelector("span");
        if (button && label) {
          label.textContent = entry.title;
          button.setAttribute("aria-pressed", String(entry.id === identity()));
        }
        return row;
      },
    );
  }
  async function open(name: string): Promise<void> {
    if (opening === name) return;
    opening = name;
    try {
      await port.open(name);
    } catch (error) {
      status.textContent = error instanceof Error ? error.message : "The note could not be opened.";
    } finally {
      if (opening === name) opening = null;
    }
  }
  function renderNotes(): void {
    const selected = active();
    const current = port.state();
    reconcileRows(
      notes,
      noteEntries,
      (entry) => entry.name,
      (entry, existing) => {
        const row = existing ?? document.createElement("li");
        if (!existing) {
          const link = document.createElement("a");
          const label = document.createElement("span");
          link.append(icon("note"), label);
          link.draggable = true;
          link.addEventListener("click", (event) => {
            if (
              event.button !== 0 ||
              event.ctrlKey ||
              event.metaKey ||
              event.shiftKey ||
              event.altKey
            )
              return;
            const name = row.dataset.name;
            if (!name) return;
            event.preventDefault();
            void open(name);
          });
          link.addEventListener("dragstart", (event) => {
            const name = row.dataset.name;
            if (!event.dataTransfer || !name) return;
            event.dataTransfer.effectAllowed = "copyLink";
            event.dataTransfer.setData("application/x-tungsten-note", name);
            event.dataTransfer.setData("text/uri-list", link.href);
            event.dataTransfer.setData(
              "text/plain",
              noteLink(
                port.state()?.name ?? "Untitled.md",
                name,
                label.textContent ?? "Untitled note",
              ),
            );
          });
          const action = document.createElement("button");
          action.type = "button";
          action.className = "icon-button membership-action";
          action.addEventListener("click", () => {
            const group = active();
            const name = row.dataset.name;
            if (group && name)
              void run(() => include(group.id, name, Boolean(search.value.trim())));
          });
          row.append(link, action);
        }
        row.dataset.name = entry.name;
        const link = row.querySelector("a");
        const label = link?.querySelector("span");
        const action = row.querySelector("button");
        const text =
          current?.name === entry.name ? noteTitle(current.body) : entry.title || "Untitled note";
        if (link && label) {
          link.href = `/?note=${encodeURIComponent(entry.name)}`;
          label.textContent = text;
          if (current?.name === entry.name) link.setAttribute("aria-current", "page");
          else link.removeAttribute("aria-current");
        }
        if (action) {
          const adding = Boolean(search.value.trim());
          action.hidden = !selected;
          action.disabled = busy || (adding && added.has(entry.name));
          const operation = adding ? "Add" : "Remove";
          action.setAttribute(
            "aria-label",
            `${operation} ${text} ${adding ? "to" : "from"} ${selected?.title ?? "workspace"}`,
          );
          action.title = adding ? "Add to workspace" : "Remove from workspace";
          const symbol = adding && added.has(entry.name) ? "check" : adding ? "add" : "close";
          if (action.querySelector("svg")?.getAttribute("data-icon") !== symbol)
            action.replaceChildren(icon(symbol));
        }
        return row;
      },
    );
    actions();
  }
  async function loadMetadata(): Promise<void> {
    const after = groupCursors[groupIndex] ?? "";
    const path = `/api/workspaces?limit=50${after ? `&after=${after}` : ""}`;
    const value = await port.api(
      path,
      "GET",
      undefined,
      metadataPath === path ? metadataRevision : null,
    );
    if (after !== (groupCursors[groupIndex] ?? "")) return;
    if (value !== null) {
      const page = workspacePage(value);
      entries = page.workspaces;
      groupNextCursor = page.next;
      metadataRevision = page.revision;
      metadataPath = path;
      const id = identity();
      if (id) {
        let selected = entries.find((entry) => entry.id === id);
        if (!selected) {
          try {
            selected = workspace(await port.api(`/api/workspaces/${id}`));
          } catch (error) {
            if (!failedStatus(error, 404)) throw error;
            if (identity() === id) choose(null);
          }
        }
        if (selected && identity() === id) selection = { kind: "workspace", value: selected };
      }
      renderGroups();
      actions();
    }
  }
  async function loadNotes(): Promise<void> {
    if (selection.kind === "loading") return;
    const generation = epoch;
    const query = search.value.trim();
    const group = active();
    const after = cursors[index] ?? "";
    if (!notes.children.length) status.textContent = "Loading notes";
    let entries: NoteEntry[];
    let following: string | null;
    if (group && !query) {
      const page = workspaceNotes(
        await port.api(
          `/api/workspaces/${group.id}/notes?limit=50${after ? `&after=${after}` : ""}`,
        ),
      );
      if (generation !== epoch || identity() !== group.id) return;
      selection = { kind: "workspace", value: page.workspace };
      entries = page.notes;
      following = page.next;
    } else {
      const page = notePage(
        await port.api(
          `/api/notes?paged=true&limit=50&q=${encodeURIComponent(query)}${after ? `&cursor=${encodeURIComponent(after)}` : ""}`,
        ),
      );
      if (generation !== epoch) return;
      if (page.notes.length > 50) throw new Error("The note list exceeds its page limit.");
      entries = page.notes;
      following = page.next;
    }
    noteEntries = entries;
    nextCursor = following;
    if (!mutation)
      status.textContent = entries.length
        ? ""
        : query
          ? "No matching notes."
          : group
            ? "Add a note or create one in this workspace."
            : "Create a note to start writing.";
    renderNotes();
  }
  function requestRefresh(metadata: boolean): void {
    needMetadata ||= metadata;
    needNotes = true;
    if (task || !visible()) return;
    task = (async () => {
      while (visible() && (needMetadata || needNotes)) {
        if (needMetadata) {
          needMetadata = false;
          await loadMetadata();
        }
        if (needNotes) {
          needNotes = false;
          await loadNotes();
        }
      }
    })()
      .catch((error: unknown) => {
        status.textContent = error instanceof Error ? error.message : "Notes are unavailable.";
        needMetadata = true;
        needNotes = true;
        retry.hidden = false;
      })
      .finally(() => {
        task = null;
      });
  }
  function beginForm(value: Workspace | null): void {
    editing = value;
    title.value = value?.title ?? "";
    formHeading.textContent = value ? `Rename ${value.title}` : "New workspace";
    formStatus.textContent = "";
    form.hidden = false;
    title.focus({ preventScroll: true });
    title.select();
  }
  control("create-workspace", HTMLButtonElement).addEventListener("click", () => beginForm(null));
  control("rename-workspace", HTMLButtonElement).addEventListener("click", () => {
    const value = active();
    if (value) beginForm(value);
  });
  control("cancel-workspace", HTMLButtonElement).addEventListener("click", () => {
    form.hidden = true;
    control(editing ? "rename-workspace" : "create-workspace", HTMLButtonElement).focus({
      preventScroll: true,
    });
  });
  form.addEventListener("submit", (event) => {
    event.preventDefault();
    if (save.disabled) return;
    const target = editing;
    const selectedAtStart = selectionEpoch;
    save.disabled = true;
    formStatus.textContent = "Saving workspace";
    void (async () => {
      try {
        const value = workspace(
          await port.api(
            target ? `/api/workspaces/${target.id}` : "/api/workspaces",
            target ? "PUT" : "POST",
            { title: title.value.trim() },
            target?.revision,
          ),
        );
        const focused = form.contains(document.activeElement);
        form.hidden = true;
        if (selectionEpoch === selectedAtStart) choose(value);
        announce();
        requestRefresh(true);
        if (focused) control("create-workspace", HTMLButtonElement).focus({ preventScroll: true });
      } catch (error) {
        formStatus.textContent =
          error instanceof Error ? error.message : "The workspace could not be saved.";
        if (target && failedStatus(error, 412)) {
          try {
            editing = workspace(await port.api(`/api/workspaces/${target.id}`));
            formHeading.textContent = `Rename ${editing.title}`;
          } catch {
            editing = target;
          }
        }
      } finally {
        save.disabled = false;
      }
    })();
  });
  control("all-notes", HTMLButtonElement).addEventListener("click", () => choose(null));
  control("remove-workspace", HTMLButtonElement).addEventListener("click", () => {
    const group = active();
    if (!group) return;
    void run(async () => {
      const current = workspace(await port.api(`/api/workspaces/${group.id}`));
      if (!confirm(`Remove ${current.title}? Its notes will be kept.`)) return;
      await port.api(`/api/workspaces/${group.id}`, "DELETE", undefined, current.revision);
      if (identity() === group.id) choose(null);
    });
  });
  create.addEventListener("click", () => {
    const target = identity();
    create.disabled = true;
    void (async () => {
      try {
        const name = await port.create();
        if (!name) {
          status.textContent = "The note is unsaved. Retry saving before adding it to a workspace.";
          return;
        }
        if (target) await run(() => include(target, name, true));
        else requestRefresh(false);
      } catch (error) {
        status.textContent =
          error instanceof Error ? error.message : "The note could not be created.";
      } finally {
        create.disabled = false;
      }
    })();
  });
  addCurrent.addEventListener("click", () => {
    const group = active();
    const name = port.state()?.name;
    if (group && name) void run(() => include(group.id, name, true));
  });
  retry.addEventListener("click", () => {
    if (mutation) void run(mutation);
    else {
      retry.hidden = true;
      requestRefresh(true);
    }
  });
  search.addEventListener("input", () => {
    ++epoch;
    index = 0;
    cursors.splice(0, cursors.length, "");
    added.clear();
    if (debounce !== null) clearTimeout(debounce);
    debounce = window.setTimeout(() => requestRefresh(false), 150);
  });
  previous.addEventListener("click", () => {
    if (index > 0) {
      --index;
      ++epoch;
      requestRefresh(false);
    }
  });
  next.addEventListener("click", () => {
    if (nextCursor) {
      cursors[++index] = nextCursor;
      ++epoch;
      requestRefresh(false);
    }
  });
  groupPrevious.addEventListener("click", () => {
    if (groupIndex > 0) {
      --groupIndex;
      requestRefresh(true);
    }
  });
  groupNext.addEventListener("click", () => {
    if (groupNextCursor) {
      groupCursors[++groupIndex] = groupNextCursor;
      requestRefresh(true);
    }
  });
  port.sidebar.subscribe(() => {
    if (visible() && (needMetadata || needNotes)) requestRefresh(needMetadata);
  });
  window.addEventListener("storage", (event) => {
    if (event.key === "tungsten.workspaces.changed" || event.key === "tungsten.note-change")
      requestRefresh(true);
  });
  window.addEventListener("focus", () => requestRefresh(true));
  document.addEventListener("visibilitychange", () => {
    if (!document.hidden) requestRefresh(true);
  });
  document.addEventListener("tungsten:trash", () => requestRefresh(true));
  window.setInterval(() => {
    if (visible() && !form.contains(document.activeElement) && document.activeElement !== search)
      requestRefresh(true);
  }, 30000);
  actions();
  requestRefresh(true);
  return {
    changed(state) {
      if (state.name)
        noteEntries = noteEntries.map((entry) =>
          entry.name === state.name ? { name: entry.name, title: noteTitle(state.body) } : entry,
        );
      renderNotes();
    },
  };
}
