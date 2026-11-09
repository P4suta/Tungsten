import { openDialog } from "./dialogs.js";
import { icon } from "./icons.js";
import { identifier } from "./identity.js";
import { currentPage } from "./pages.js";
import { type Saved, savedEntries } from "./recovery.js";

interface TrashPort {
  api(path: string, method?: string, input?: unknown, revision?: string): Promise<unknown>;
  navigate(name: string): Promise<void>;
  refresh(): Promise<unknown>;
}

function control<T extends HTMLElement>(id: string, kind: { new (): T }): T {
  const element = document.getElementById(id);
  if (!(element instanceof kind)) throw new Error(`Missing trash control: ${id}`);
  return element;
}

export function initTrash(port: TrashPort): void {
  const list = control("trash-list", HTMLDivElement);
  const status = control("trash-status", HTMLParagraphElement);
  const restore = control("restore-trashed", HTMLButtonElement);
  const empty = control("empty-trash", HTMLButtonElement);
  const previous = control("trash-previous", HTMLButtonElement);
  const next = control("trash-next", HTMLButtonElement);
  const preview = control("deleted-note", HTMLDialogElement);
  let entries: Saved[] = [];
  let selected: Saved | null = null;
  let busy = false;
  let reloadPending = false;
  let generation = 0;
  const cursors: string[] = [""];
  let index = 0;
  let nextCursor: string | null = null;
  let typed = "";
  let typedAt = 0;

  function actions(): void {
    restore.disabled = busy || !selected;
    empty.disabled = busy || !entries.length;
    previous.disabled = busy || index === 0;
    next.disabled = busy || nextCursor === null;
    list.inert = busy;
    list.setAttribute("aria-busy", String(busy));
  }
  function choose(saved: Saved, focus = false): void {
    selected = saved;
    for (const row of list.querySelectorAll<HTMLElement>("[role=option]")) {
      const active = row.dataset.id === saved.id;
      row.setAttribute("aria-selected", String(active));
      row.tabIndex = active ? 0 : -1;
      if (active && focus) row.focus();
    }
    actions();
  }
  async function run(action: () => Promise<void>): Promise<void> {
    if (busy) return;
    busy = true;
    actions();
    try {
      await action();
    } catch (error) {
      status.textContent = error instanceof Error ? error.message : "Trash is unavailable.";
    } finally {
      busy = false;
      actions();
      if (reloadPending && currentPage() === "trash") {
        reloadPending = false;
        void run(load);
      }
    }
  }
  async function load(): Promise<void> {
    const current = ++generation;
    status.textContent = "Loading deleted notes";
    const value = await port.api(
      `/api/trash?page=true&cursor=${encodeURIComponent(cursors[index] ?? "")}`,
    );
    if (current !== generation || currentPage() !== "trash") return;
    if (
      typeof value !== "object" ||
      value === null ||
      !("entries" in value) ||
      !("next" in value) ||
      (value.next !== null && typeof value.next !== "string")
    )
      throw new Error("The trash page is invalid.");
    entries = savedEntries(value.entries);
    nextCursor = value.next;
    selected = null;
    list.replaceChildren();
    for (const [offset, saved] of entries.entries()) {
      const row = document.createElement("div");
      row.setAttribute("role", "option");
      row.setAttribute("aria-selected", "false");
      row.setAttribute("aria-label", saved.title);
      row.setAttribute("aria-setsize", "-1");
      row.setAttribute("aria-posinset", String(index * 50 + offset + 1));
      row.tabIndex = offset === 0 ? 0 : -1;
      row.dataset.id = saved.id;
      const name = document.createElement("span");
      name.textContent = saved.title;
      name.className = "file-row-name";
      const date = document.createElement("time");
      date.dateTime = new Date(saved.captured).toISOString();
      date.textContent = new Date(saved.captured).toLocaleString();
      row.append(icon("note"), name, date);
      row.addEventListener("click", () => choose(saved, true));
      row.addEventListener("focus", () => choose(saved));
      row.addEventListener("dblclick", () => {
        void run(openSelected);
      });
      list.append(row);
    }
    list.hidden = !entries.length;
    status.textContent = entries.length
      ? `Page ${index + 1} · ${entries.length} deleted notes`
      : "Trash is empty.";
    actions();
  }
  async function openSelected(): Promise<void> {
    const saved = selected;
    if (!saved) return;
    const value = await port.api(`/api/trash/${saved.id}`);
    if (
      typeof value !== "object" ||
      value === null ||
      !("body" in value) ||
      typeof value.body !== "string"
    )
      throw new Error("The deleted note is invalid.");
    control("deleted-title", HTMLHeadingElement).textContent = saved.title;
    control("deleted-content", HTMLTextAreaElement).value = value.body;
    openDialog(preview, list.querySelector<HTMLElement>("[aria-selected=true]") ?? list);
  }
  list.addEventListener("keydown", (event) => {
    if (event.isComposing || event.ctrlKey || event.metaKey || event.altKey || busy) return;
    const position = entries.findIndex((entry) => entry.id === selected?.id);
    let target = position;
    if (event.key === "ArrowDown") target = Math.min(entries.length - 1, position + 1);
    else if (event.key === "ArrowUp") target = Math.max(0, position - 1);
    else if (event.key === "Home") target = 0;
    else if (event.key === "End") target = entries.length - 1;
    else if (event.key === "Enter") {
      event.preventDefault();
      void run(openSelected);
      return;
    } else if (event.key === " ") {
      event.preventDefault();
      return;
    } else if (event.key.length === 1) {
      const now = Date.now();
      typed = now - typedAt < 700 ? typed + event.key : event.key;
      typedAt = now;
      const order = [...entries.slice(position + 1), ...entries.slice(0, position + 1)];
      const match = order.find((entry) =>
        entry.title.toLocaleLowerCase().startsWith(typed.toLocaleLowerCase()),
      );
      target = match ? entries.indexOf(match) : position;
    } else return;
    event.preventDefault();
    const saved = entries[target];
    if (saved) choose(saved, true);
  });
  restore.addEventListener("click", () => {
    const saved = selected;
    if (!saved) return;
    void run(async () => {
      let value: unknown;
      try {
        value = await port.api(`/api/trash/${saved.id}/restore`, "POST", {});
      } catch (error) {
        if (
          typeof error !== "object" ||
          error === null ||
          !("status" in error) ||
          error.status !== 409
        )
          throw error;
        value = await port.api(`/api/trash/${saved.id}/restore`, "POST", {
          name: `${identifier()}.md`,
        });
      }
      if (
        typeof value !== "object" ||
        value === null ||
        !("name" in value) ||
        typeof value.name !== "string"
      )
        throw new Error("The restored note is invalid.");
      await port.navigate(value.name);
      await port.refresh();
    });
  });
  empty.addEventListener("click", () => {
    if (!confirm("Permanently delete every note in trash?")) return;
    void run(async () => {
      await port.api("/api/trash", "DELETE");
      index = 0;
      cursors.splice(1);
      await load();
    });
  });
  previous.addEventListener("click", () => {
    if (index > 0) {
      index--;
      void run(load);
    }
  });
  next.addEventListener("click", () => {
    if (nextCursor) {
      cursors[++index] = nextCursor;
      void run(load);
    }
  });
  document.addEventListener("tungsten:page", () => {
    if (currentPage() === "trash") {
      if (busy) reloadPending = true;
      else void run(load);
    } else {
      ++generation;
      reloadPending = false;
    }
  });
  document.addEventListener("tungsten:trash", () => {
    index = 0;
    cursors.splice(1);
    ++generation;
    if (currentPage() === "trash") {
      if (busy) reloadPending = true;
      else void run(load);
    }
  });
  if (currentPage() === "trash") void run(load);
  actions();
}
