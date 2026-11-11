import { graphResponse, type Summary } from "./catalog.js";
import type { Snapshot } from "./features.js";
import { icon } from "./icons.js";
import { type Saved, savedEntries } from "./recovery.js";
import { reconcileRows } from "./rows.js";
import type { SidebarController } from "./sidebar.js";

interface ContextPort {
  state(): Snapshot | null;
  api(path: string, method?: string, input?: unknown, revision?: string | null): Promise<unknown>;
  navigate(name: string): Promise<void>;
  mutate(action: (state: Snapshot) => Promise<void>): Promise<boolean>;
  refresh(): Promise<unknown>;
  sidebar: SidebarController;
}

export interface ContextController {
  changed(state: Snapshot): void;
}

function control<T extends HTMLElement>(id: string, kind: { new (): T }): T {
  const node = document.getElementById(id);
  if (!(node instanceof kind)) throw new Error(`Missing context control: ${id}`);
  return node;
}

export function initContext(port: ContextPort): ContextController {
  const linkStatus = control("context-status", HTMLParagraphElement);
  const historyStatus = control("history-status", HTMLParagraphElement);
  const retry = control("context-retry", HTMLButtonElement);
  const history = control("history-list", HTMLUListElement);
  const savedContent = control("saved-content", HTMLTextAreaElement);
  const diff = control("version-diff", HTMLElement);
  const restore = control("restore-version", HTMLButtonElement);
  let identity = "";
  let loaded = "";
  let pending = "";
  let generation = 0;
  let versionGeneration = 0;
  let selected: Saved | null = null;
  let versions: Saved[] = [];
  let restoring = false;

  function visible(): boolean {
    return port.sidebar.visible("context") && !document.hidden;
  }
  function status(): HTMLParagraphElement {
    return port.sidebar.contextView() === "links" ? linkStatus : historyStatus;
  }
  function actions(): void {
    const state = port.state();
    restore.disabled = !selected || !state?.name || state.busy || state.composing || restoring;
  }
  function clear(): void {
    ++generation;
    ++versionGeneration;
    loaded = "";
    pending = "";
    selected = null;
    versions = [];
    savedContent.value = "";
    diff.replaceChildren();
    history.replaceChildren();
    control("outgoing-notes", HTMLUListElement).replaceChildren();
    control("incoming-notes", HTMLUListElement).replaceChildren();
    linkStatus.textContent = "";
    historyStatus.textContent = "";
    retry.hidden = true;
    actions();
  }
  function links(id: string, entries: Summary[]): void {
    const list = control(id, HTMLUListElement);
    reconcileRows(
      list,
      entries,
      (entry) => entry.name,
      (entry, existing) => {
        const row = existing ?? document.createElement("li");
        if (!existing) {
          const link = document.createElement("a");
          link.append(icon("note"), document.createElement("span"));
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
            void port.navigate(name).catch((error: unknown) => {
              linkStatus.textContent =
                error instanceof Error ? error.message : "The note is unavailable.";
            });
          });
          row.append(link);
        }
        row.dataset.name = entry.name;
        const link = row.querySelector("a");
        const text = link?.querySelector("span");
        if (link && text) {
          link.href = `/?note=${encodeURIComponent(entry.name)}`;
          text.textContent = entry.title || "Untitled note";
        }
        return row;
      },
    );
    if (!entries.length) {
      const empty = document.createElement("li");
      empty.textContent = "No links yet.";
      list.append(empty);
    }
  }
  async function viewVersion(version: Saved): Promise<void> {
    const original = identity;
    const request = ++versionGeneration;
    selected = null;
    actions();
    try {
      const value = await port.api(`/api/history/${version.id}`);
      if (original !== identity || request !== versionGeneration) return;
      if (
        typeof value !== "object" ||
        value === null ||
        !("body" in value) ||
        typeof value.body !== "string"
      )
        throw new Error("The version data is invalid.");
      const { versionDifference } = await import("./differences.js");
      if (original !== identity || request !== versionGeneration) return;
      const difference = versionDifference(port.state()?.body ?? "", value.body);
      selected = version;
      savedContent.value = value.body;
      diff.replaceChildren();
      const summary = document.createElement("p");
      summary.textContent = difference
        ? `Restoring adds ${difference.added} lines and removes ${difference.removed} lines.${difference.complete ? "" : " The comparison is abbreviated."}`
        : "The changes exceed the comparison limit. Read the saved content below.";
      diff.append(summary);
      if (difference) {
        const source = document.createElement("pre");
        source.textContent = difference.text;
        diff.append(source);
      }
      renderVersions();
    } catch (error) {
      if (original === identity && request === versionGeneration)
        historyStatus.textContent =
          error instanceof Error ? error.message : "The version is unavailable.";
    } finally {
      actions();
    }
  }
  function renderVersions(): void {
    reconcileRows(
      history,
      versions,
      (entry) => entry.id,
      (entry, existing) => {
        const row = existing ?? document.createElement("li");
        if (!existing) {
          const info = document.createElement("p");
          const button = document.createElement("button");
          button.type = "button";
          button.append(icon("history"), document.createTextNode("View version"));
          button.addEventListener("click", () => {
            const version = versions.find((version) => version.id === row.dataset.version);
            if (version) void viewVersion(version);
          });
          row.append(info, button);
        }
        row.dataset.version = entry.id;
        const info = row.querySelector("p");
        if (info)
          info.textContent = `${new Date(entry.captured).toLocaleString()} · ${entry.title}`;
        row
          .querySelector("button")
          ?.setAttribute("aria-pressed", String(selected?.id === entry.id));
        return row;
      },
    );
  }
  async function load(): Promise<void> {
    const state = port.state();
    if (!state || !visible()) return;
    if (identity !== state.identity) {
      identity = state.identity;
      clear();
    }
    const mode = port.sidebar.contextView();
    if (!state.name) {
      status().textContent =
        mode === "links"
          ? "Save the note to see its links."
          : "Save the note to start its history.";
      return;
    }
    if (state.busy || state.composing) return;
    const key = `${state.identity}\0${state.revision}\0${mode}`;
    if (loaded === key || pending === key) return;
    const request = ++generation;
    pending = key;
    status().textContent = mode === "links" ? "Loading links" : "Loading history";
    retry.hidden = true;
    try {
      const value = await port.api(
        mode === "links"
          ? `/api/graph?name=${encodeURIComponent(state.name)}&limit=120`
          : `/api/history?name=${encodeURIComponent(state.name)}`,
      );
      if (
        request !== generation ||
        port.state()?.identity !== state.identity ||
        port.sidebar.contextView() !== mode
      )
        return;
      if (mode === "links") {
        const graph = graphResponse(value);
        const outgoing = new Set(
          graph.edges.filter((edge) => edge.source === state.name).map((edge) => edge.target),
        );
        const incoming = new Set(
          graph.edges.filter((edge) => edge.target === state.name).map((edge) => edge.source),
        );
        links(
          "outgoing-notes",
          graph.notes.filter((note) => outgoing.has(note.name)),
        );
        links(
          "incoming-notes",
          graph.notes.filter((note) => incoming.has(note.name)),
        );
        linkStatus.textContent = graph.truncated
          ? "Showing a limited selection of nearby links. Explore notes to search the graph."
          : "";
      } else {
        versions = savedEntries(value);
        if (selected && !versions.some((entry) => entry.id === selected?.id)) {
          selected = null;
          savedContent.value = "";
          diff.replaceChildren();
        }
        renderVersions();
        historyStatus.textContent = versions.length
          ? `${versions.length} recovery points. Points expire after 7 days.`
          : "No recovery points from the past 7 days.";
      }
      loaded = key;
    } catch (error) {
      if (request === generation && port.state()?.identity === state.identity) {
        status().textContent =
          error instanceof Error ? error.message : "Note context is unavailable.";
        retry.hidden = false;
      }
    } finally {
      if (request === generation) pending = "";
      actions();
    }
  }
  restore.addEventListener("click", () => {
    const version = selected;
    const original = identity;
    if (!version || restoring || !confirm("Restore the selected version?")) return;
    restoring = true;
    actions();
    void (async () => {
      try {
        await port.mutate(async (state) => {
          if (state.identity !== original || !state.name) return;
          await port.api(
            `/api/history/${version.id}/restore`,
            "POST",
            { name: state.name },
            state.revision,
          );
          await port.navigate(state.name);
          await port.refresh();
        });
        loaded = "";
        await load();
      } catch (error) {
        if (identity === original)
          historyStatus.textContent =
            error instanceof Error ? error.message : "The version could not be restored.";
      } finally {
        restoring = false;
        actions();
      }
    })();
  });
  retry.addEventListener("click", () => {
    loaded = "";
    void load();
  });
  port.sidebar.subscribe(() => {
    void load();
  });
  window.addEventListener("focus", () => {
    loaded = "";
    void load();
  });
  document.addEventListener("visibilitychange", () => {
    if (!document.hidden) {
      loaded = "";
      void load();
    }
  });
  window.addEventListener("storage", (event) => {
    if (event.key === "tungsten.note-change") {
      loaded = "";
      void load();
    }
  });
  return {
    changed(state) {
      if (identity !== state.identity) {
        identity = state.identity;
        clear();
      }
      actions();
      void load();
    },
  };
}
