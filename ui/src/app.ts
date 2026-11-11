import { notePage } from "./catalog.js";
import { openDialog } from "./dialogs.js";
import { activeDraftOwners, draftOwner, uncoordinatedDraftOwner } from "./draft-ownership.js";
import { type FeatureController, initFeatures, type Snapshot, type Summary } from "./features.js";
import { icon } from "./icons.js";
import { identifier } from "./identity.js";
import {
  hasRemoteConflict,
  isSaved,
  mayApplyRemote,
  mayReadRemote,
  mayRecoverDraft,
  maySave,
} from "./model.js";
import { navigationHasFocus } from "./sidebar.js";
import { focusWriting, observeLayout, writingScrollContainer } from "./surface.js";
import { defaultTitle, noteTitle } from "./titles.js";

interface Note {
  name: string;
  body: string;
  revision: string;
  modified: number;
}
interface Draft {
  owner: string | null;
  name: string | null;
  body: string;
  revision: string | null;
  savedBody: string;
  title: string;
  candidate: string;
  attempted: boolean;
  updated: number;
}
interface StoredDraft {
  key: string;
  raw: string;
  value: Draft;
}
interface Editor extends Draft {
  key: string;
  source: StoredDraft | null;
  pending: Promise<void> | null;
  operation: boolean;
  timer: number | null;
  composing: boolean;
  blocked: boolean;
  missing: boolean;
  failure: string | null;
}

function element<T extends HTMLElement>(id: string, kind: { new (): T }): T {
  const value = document.getElementById(id);
  if (!(value instanceof kind)) throw new Error(`Missing control: ${id}`);
  return value;
}

const body = element("body", HTMLTextAreaElement);
const search = element("search", HTMLInputElement);
const status = element("save-status", HTMLParagraphElement);
const errorText = element("error", HTMLParagraphElement);
const listStatus = element("list-status", HTMLParagraphElement);
const list = element("notes", HTMLUListElement);
const draftList = element("drafts", HTMLUListElement);
const draftsPane = element("drafts-pane", HTMLElement);
const deleteButton = element("delete", HTMLButtonElement);
const retryButton = element("retry", HTMLButtonElement);
const reloadButton = element("reload", HTMLButtonElement);
const copyButton = element("copy", HTMLButtonElement);
const prefix = "tungsten.draft.";
const changeKey = "tungsten.note-change";
let current: Editor | null = null;
let listGeneration = 0;
let openGeneration = 0;
let recentlyOpened: string[] = [];
let storageWarning = "";
let features: FeatureController | null = null;
let checking: Promise<void> | null = null;
let checkTimer: number | undefined;

function record(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null;
}

function parseNote(value: unknown): Note {
  if (
    !record(value) ||
    typeof value.name !== "string" ||
    typeof value.body !== "string" ||
    typeof value.revision !== "string" ||
    typeof value.modified !== "number"
  ) {
    throw new Error("The server returned an invalid note.");
  }
  return { name: value.name, body: value.body, revision: value.revision, modified: value.modified };
}

function parseDraft(value: unknown): Draft | null {
  if (
    !record(value) ||
    !(
      value.owner === undefined ||
      value.owner === null ||
      value.owner === uncoordinatedDraftOwner ||
      (typeof value.owner === "string" && /^[a-f0-9]{32}$/.test(value.owner))
    ) ||
    !(value.name === null || typeof value.name === "string") ||
    typeof value.body !== "string" ||
    !(value.revision === null || typeof value.revision === "string") ||
    typeof value.savedBody !== "string" ||
    typeof value.title !== "string" ||
    typeof value.candidate !== "string" ||
    typeof value.attempted !== "boolean" ||
    typeof value.updated !== "number"
  )
    return null;
  return {
    owner: typeof value.owner === "string" ? value.owner : null,
    name: value.name,
    body: value.body,
    revision: value.revision,
    savedBody: value.savedBody,
    title: value.title,
    candidate: value.candidate,
    attempted: value.attempted,
    updated: value.updated,
  };
}

class ApiError extends Error {
  constructor(
    readonly status: number,
    message: string,
  ) {
    super(message);
  }
}

async function api(
  path: string,
  method = "GET",
  input?: unknown,
  revision?: string | null,
): Promise<unknown> {
  const headers: Record<string, string> = { "content-type": "application/json" };
  if (revision) headers[method === "GET" ? "if-none-match" : "if-match"] = revision;
  const options: RequestInit = {
    method,
    headers,
    signal: AbortSignal.timeout(15000),
    cache: "no-store",
  };
  if (input instanceof File) {
    headers["content-type"] = "application/octet-stream";
    headers["x-attachment-name"] = encodeURIComponent(input.name);
    options.body = input;
  } else if (input !== undefined) options.body = JSON.stringify(input);
  const response = await fetch(path, options);
  if (response.status === 204 || response.status === 304) return null;
  const text = await response.text();
  let value: unknown;
  try {
    value = JSON.parse(text);
  } catch {
    value = null;
  }
  if (!response.ok) {
    throw new ApiError(
      response.status,
      record(value) && typeof value.error === "string"
        ? value.error
        : "The server could not save the note.",
    );
  }
  return value;
}

function notePath(name: string): string {
  return `/api/notes/${encodeURIComponent(name)}`;
}
function dirty(editor: Editor): boolean {
  return editor.body !== editor.savedBody || (editor.name === null && editor.attempted);
}
function warning(): void {
  storageWarning = "Browser draft storage is unavailable. Keep this tab open until Saved appears.";
}
function remember(name: string, replace = false): void {
  recentlyOpened = [name, ...recentlyOpened.filter((entry) => entry !== name)].slice(0, 24);
  const address = new URL(location.href);
  if (address.searchParams.get("note") !== name) {
    address.searchParams.set("note", name);
    const target = address.pathname + address.search + address.hash;
    if (!replace) history.pushState(null, "", target);
    else history.replaceState(null, "", target);
  }
  try {
    localStorage.setItem("tungsten.last", name);
  } catch {
    warning();
  }
}
function message(error: unknown): string {
  return error instanceof Error
    ? error.message
    : "The operation failed. Keep your draft and retry.";
}

function drafts(): StoredDraft[] {
  const result: StoredDraft[] = [];
  try {
    for (let index = 0; index < localStorage.length; index++) {
      const key = localStorage.key(index);
      if (!key?.startsWith(prefix)) continue;
      const raw = localStorage.getItem(key);
      if (!raw) continue;
      try {
        const value = parseDraft(JSON.parse(raw));
        if (value) result.push({ key, raw, value });
        else storageWarning = "A browser draft could not be read. Its stored data has been kept.";
      } catch {
        storageWarning = "A browser draft could not be read. Its stored data has been kept.";
      }
    }
  } catch {
    warning();
  }
  return result.sort((a, b) => b.value.updated - a.value.updated);
}

function removeDraft(editor: Editor): void {
  try {
    localStorage.removeItem(editor.key);
    if (editor.source && localStorage.getItem(editor.source.key) === editor.source.raw) {
      localStorage.removeItem(editor.source.key);
    }
    editor.source = null;
  } catch {
    warning();
  }
}

function persist(editor: Editor): void {
  try {
    if (!dirty(editor) && !editor.blocked) {
      removeDraft(editor);
      return;
    }
    const draft: Draft = {
      owner: draftOwner,
      name: editor.name,
      body: editor.body,
      revision: editor.revision,
      savedBody: editor.savedBody,
      title: editor.title,
      candidate: editor.candidate,
      attempted: editor.attempted,
      updated: Date.now(),
    };
    localStorage.setItem(editor.key, JSON.stringify(draft));
  } catch {
    warning();
  }
  renderDrafts();
}

function renderDrafts(): void {
  const entries = drafts().filter(
    (draft) => draft.key !== current?.key && draft.key !== current?.source?.key,
  );
  draftList.replaceChildren();
  draftsPane.hidden = entries.length === 0;
  for (const draft of entries) {
    const button = document.createElement("button");
    button.type = "button";
    button.textContent = `Recover ${draft.value.title || "Untitled"}`;
    button.addEventListener("click", () => {
      void recover(draft);
    });
    const item = document.createElement("li");
    item.append(button);
    draftList.append(item);
  }
}

function renderStatus(editor: Editor): void {
  if (editor !== current) return;
  document.title = noteTitle(editor.body);
  const busy = editor.pending !== null || editor.operation;
  body.readOnly = editor.operation;
  status.textContent = editor.blocked
    ? "Conflict"
    : busy
      ? "Saving"
      : editor.failure
        ? "Save failed"
        : isSaved(editor.body === editor.savedBody, editor.revision !== null, busy)
          ? "Saved"
          : dirty(editor)
            ? "Unsaved"
            : "Ready";
  const error = editor.failure || storageWarning;
  errorText.textContent = error;
  errorText.hidden = error === "";
  element("sidebar-status", HTMLParagraphElement).textContent = error;
  element("recovery", HTMLDivElement).hidden = error === "" && !editor.blocked;
  retryButton.hidden = editor.failure === null || editor.blocked;
  reloadButton.hidden = !editor.blocked || editor.name === null || editor.missing;
  copyButton.hidden = editor.failure === null && !editor.blocked;
  deleteButton.disabled = busy || editor.name === null || editor.blocked;
  features?.changed();
  resizePaper();
}

function resizePaper(): void {
  const paper = writingScrollContainer();
  if (paper.hidden) return;
  document.documentElement.style.setProperty("--writing-height", `${paper.clientHeight}px`);
}
window.addEventListener("resize", resizePaper);
observeLayout(
  [writingScrollContainer(), document.querySelector("header") ?? document.body],
  resizePaper,
);

function blank(): Editor {
  const id = identifier();
  const text = `# ${defaultTitle(new Date())}\n\n`;
  return {
    owner: draftOwner,
    name: null,
    body: text,
    revision: null,
    savedBody: text,
    title: noteTitle(text),
    candidate: `${id}.md`,
    attempted: false,
    updated: Date.now(),
    key: prefix + id,
    source: null,
    pending: null,
    operation: false,
    timer: null,
    composing: false,
    blocked: false,
    missing: false,
    failure: null,
  };
}

function show(editor: Editor, focus = false): void {
  const changed = current?.key !== editor.key;
  current = editor;
  body.value = editor.body;
  body.disabled = false;
  document.body.classList.remove("show-notes");
  renderStatus(editor);
  if (changed) document.dispatchEvent(new Event("tungsten:note"));
  renderDrafts();
  for (const button of list.querySelectorAll("button")) {
    if (editor.name !== null && button.dataset.name === editor.name)
      button.setAttribute("aria-current", "true");
    else button.removeAttribute("aria-current");
  }
  if (focus) {
    features?.write();
    focusWriting(body);
  }
}

function showNote(note: Note, replace = false, focus = false): void {
  const editor = Object.assign(blank(), {
    name: note.name,
    body: note.body,
    savedBody: note.body,
    revision: note.revision,
    title: noteTitle(note.body),
  });
  remember(note.name, replace);
  show(editor, focus);
}

function forgetDeleted(editor: Editor): void {
  editor.blocked = true;
  editor.missing = true;
  if (editor.timer !== null) window.clearTimeout(editor.timer);
  editor.timer = null;
  recentlyOpened = recentlyOpened.filter((name) => name !== editor.name);
  try {
    if (localStorage.getItem("tungsten.last") === editor.name)
      localStorage.removeItem("tungsten.last");
  } catch {
    warning();
  }
  removeDraft(editor);
  const address = new URL(location.href);
  if (address.searchParams.get("note") === editor.name) {
    address.searchParams.delete("note");
    address.hash = "";
    history.replaceState(null, "", address.pathname + address.search);
  }
}

async function openAfterDeletion(editor: Editor): Promise<void> {
  const generation = ++openGeneration;
  body.disabled = true;
  features?.changed();
  status.textContent = "Loading";
  try {
    const page = notePage(await api("/api/notes?paged=true&limit=1"));
    if (generation !== openGeneration || current !== editor) return;
    const candidates = new Set([...recentlyOpened, ...page.notes.map((note) => note.name)]);
    for (const name of candidates) {
      try {
        const note = parseNote(await api(notePath(name)));
        if (generation !== openGeneration || current !== editor) return;
        showNote(note, true, mayResumeWriting());
        return;
      } catch (error) {
        if (!(error instanceof ApiError) || error.status !== 404) throw error;
        recentlyOpened = recentlyOpened.filter((entry) => entry !== name);
      }
    }
    if (generation === openGeneration && current === editor) show(editor, mayResumeWriting());
  } catch (error) {
    if (generation !== openGeneration || current !== editor) return;
    editor.failure = message(error);
  } finally {
    if (generation === openGeneration && current === editor) {
      body.disabled = false;
      renderStatus(editor);
    }
  }
}

function mayResumeWriting(): boolean {
  const compactContext =
    document.body.dataset.layout === "compact" && document.body.dataset.context === "open";
  return !document.querySelector("dialog[open]") && (compactContext || !navigationHasFocus());
}

async function refresh(
  cursor: string | null = null,
  trail: (string | null)[] = [],
): Promise<Summary[]> {
  const generation = ++listGeneration;
  const query = search.value;
  try {
    const value = await api(
      `/api/notes?paged=true&limit=24&q=${encodeURIComponent(query)}${cursor ? `&cursor=${encodeURIComponent(cursor)}` : ""}`,
    );
    const page = notePage(value);
    const notes = page.notes;
    if (generation !== listGeneration || query !== search.value) return notes;
    list.replaceChildren();
    const visible = features?.filter(notes) ?? notes;
    list.hidden = !search.value.trim();
    for (const note of search.value.trim() ? visible.slice(0, 24) : []) {
      const button = document.createElement("button");
      button.type = "button";
      button.textContent = note.title || noteTitle(note.snippet);
      button.dataset.name = note.name;
      if (current?.name === note.name) button.setAttribute("aria-current", "true");
      button.addEventListener("click", () => {
        void features?.open(note.name).catch(() => {});
      });
      const item = document.createElement("li");
      item.append(button);
      if (search.value && note.snippet) {
        const snippet = document.createElement("p");
        snippet.className = "snippet";
        snippet.textContent = note.snippet;
        item.append(snippet);
      }
      list.append(item);
    }
    if (search.value.trim() && (trail.length || page.next)) {
      const item = document.createElement("li");
      for (const [label, destination, history] of [
        ...(trail.length
          ? [["Previous results", trail.at(-1) ?? null, trail.slice(0, -1)] as const]
          : []),
        ...(page.next ? [["More results", page.next, [...trail, cursor]] as const] : []),
      ]) {
        const button = document.createElement("button");
        button.type = "button";
        button.append(
          icon(label === "More results" ? "expand" : "undo"),
          document.createTextNode(label),
        );
        button.addEventListener("click", () => {
          void refresh(destination, [...history])
            .then(() => list.querySelector("button")?.focus())
            .catch(() => {});
        });
        item.append(button);
      }
      list.append(item);
    }
    listStatus.textContent = !search.value.trim()
      ? ""
      : visible.length === 0
        ? "No notes found."
        : visible.length + (visible.length === 1 ? " note" : " notes");
    renderDrafts();
    features?.searchResults(visible);
    return notes;
  } catch (error) {
    if (generation === listGeneration && query === search.value)
      listStatus.textContent =
        error instanceof ApiError ? error.message : "Could not load notes. Try searching again.";
    throw error;
  }
}

async function openNote(name: string, propagate = false): Promise<void> {
  const generation = ++openGeneration;
  body.disabled = true;
  features?.changed();
  deleteButton.disabled = true;
  status.textContent = "Loading";
  try {
    if (current?.name === name && !current.operation && !(await flush(current))) {
      body.disabled = false;
      show(current);
      return;
    }
    if (generation !== openGeneration) return;
    const note = parseNote(await api(notePath(name)));
    if (generation !== openGeneration) return;
    body.disabled = false;
    showNote(note);
  } catch (error) {
    if (generation !== openGeneration) return;
    body.disabled = false;
    if (current) {
      current.failure = message(error);
      renderStatus(current);
    } else {
      show(blank());
      errorText.textContent = message(error);
      errorText.hidden = false;
      element("recovery", HTMLDivElement).hidden = false;
    }
    if (propagate) throw error;
  }
}

async function recover(draft: StoredDraft): Promise<void> {
  const generation = ++openGeneration;
  const editor = Object.assign(blank(), draft.value);
  editor.key = prefix + identifier();
  editor.owner = draftOwner;
  editor.source = draft;
  if (editor.name !== null) {
    try {
      const note = parseNote(await api(notePath(editor.name)));
      editor.blocked = note.revision !== editor.revision && note.body !== editor.body;
      editor.revision = note.revision;
      editor.savedBody = note.body;
      if (editor.blocked)
        editor.failure = "The saved note changed. Reload it or save your draft as a copy.";
    } catch (error) {
      if (
        error instanceof ApiError &&
        error.status === 404 &&
        editor.revision !== null &&
        !dirty(editor)
      ) {
        if (generation !== openGeneration) return;
        forgetDeleted(editor);
        const replacement = blank();
        element("notes-dialog", HTMLDialogElement).close();
        show(replacement, true);
        await openAfterDeletion(replacement);
        return;
      }
      editor.failure = message(error);
      editor.blocked = error instanceof ApiError && error.status === 404;
      editor.missing = editor.blocked;
    }
  }
  if (generation !== openGeneration) return;
  if (editor.name) remember(editor.name);
  element("notes-dialog", HTMLDialogElement).close();
  show(editor, true);
  persist(editor);
  if (dirty(editor) && !editor.blocked) {
    editor.failure ??= "Recovered an unsaved draft. Select Retry to save it.";
    renderStatus(editor);
  }
}

function schedule(editor: Editor): void {
  if (editor.timer !== null) window.clearTimeout(editor.timer);
  editor.timer = window.setTimeout(() => {
    editor.timer = null;
    void save(editor);
  }, 500);
}

async function save(editor: Editor): Promise<void> {
  if (editor.pending) return editor.pending;
  if (!maySave(dirty(editor), editor.operation, editor.composing, editor.blocked)) return;
  if (!editor.attempted && editor.name === null) {
    editor.attempted = true;
  }
  const target = editor.name ?? editor.candidate;
  const created = editor.name === null;
  const sent = editor.body;
  editor.failure = null;
  persist(editor);
  const work = async (): Promise<void> => {
    try {
      await features?.beforeSave(editor.key, sent);
      let note: Note;
      try {
        note = parseNote(
          await api(
            editor.name === null ? "/api/notes" : notePath(target),
            editor.name === null ? "POST" : "PUT",
            editor.name === null ? { name: target, body: sent } : { body: sent },
            editor.revision,
          ),
        );
      } catch (error) {
        let saved: Note | null = null;
        try {
          saved = parseNote(await api(notePath(target)));
        } catch {
          saved = null;
        }
        if (saved?.body !== sent) throw error;
        note = saved;
      }
      editor.name = note.name;
      editor.revision = note.revision;
      editor.savedBody = sent;
      announceChange(note.name);
      if (current === editor) {
        remember(note.name, created);
      }
      persist(editor);
      void refresh().catch(() => {});
    } catch (error) {
      editor.failure = message(error);
      editor.blocked = error instanceof ApiError && [404, 409, 412].includes(error.status);
      editor.missing = error instanceof ApiError && error.status === 404;
      persist(editor);
    }
  };
  editor.pending = work().finally(() => {
    editor.pending = null;
    renderStatus(editor);
    if (dirty(editor) && !editor.failure && !editor.blocked && !editor.composing) void save(editor);
  });
  renderStatus(editor);
  return editor.pending;
}

async function flush(editor: Editor): Promise<boolean> {
  while (dirty(editor) || editor.pending) {
    if (editor.composing || editor.blocked || editor.failure) return false;
    await save(editor);
  }
  return true;
}

body.addEventListener("input", () => {
  if (!current) return;
  current.body = body.value;
  current.title = noteTitle(body.value);
  persist(current);
  renderStatus(current);
  schedule(current);
});
document.addEventListener("compositionstart", () => {
  if (current) current.composing = true;
});
document.addEventListener("compositionend", () => {
  if (!current) return;
  current.composing = false;
  current.body = body.value;
  persist(current);
  renderStatus(current);
  schedule(current);
});
function createDraft(title: string, text: string, focus = true, save = true): Editor {
  element("notes-dialog", HTMLDialogElement).close();
  ++openGeneration;
  const editor = blank();
  editor.body = text || (title ? `# ${title}\n\n` : editor.body);
  editor.savedBody = save ? "" : editor.body;
  editor.title = noteTitle(editor.body);
  if (location.search || location.hash) history.pushState(null, "", "/");
  show(editor, focus);
  if (!text) body.setSelectionRange(body.value.length, body.value.length);
  persist(editor);
  if (save && editor.body) schedule(editor);
  return editor;
}

element("show-notes", HTMLButtonElement).addEventListener("click", (event) => {
  event.preventDefault();
  openDialog(element("notes-dialog", HTMLDialogElement), element("show-notes", HTMLButtonElement));
  search.focus();
  search.value = "";
  void features?.home();
  void refresh().catch(() => {});
});
search.addEventListener("input", () => {
  void refresh().catch(() => {});
});
retryButton.addEventListener("click", () => {
  if (!current) return;
  current.failure = null;
  void save(current);
});
reloadButton.addEventListener("click", () => {
  const editor = current;
  if (!editor?.name || !confirm("Discard this draft and reload the saved note?")) return;
  void api(notePath(editor.name))
    .then((value) => {
      const note = parseNote(value);
      removeDraft(editor);
      const loaded = Object.assign(blank(), {
        name: note.name,
        body: note.body,
        savedBody: note.body,
        revision: note.revision,
        title: noteTitle(note.body),
      });
      show(loaded);
    })
    .catch((error: unknown) => {
      editor.failure = message(error);
      renderStatus(editor);
    });
});
copyButton.addEventListener("click", () => {
  if (!current) return;
  const previous = current;
  const copy = blank();
  copy.body = previous.body;
  copy.savedBody = "";
  copy.title = `${previous.title || "Note"} copy-${identifier().slice(0, 8)}`;
  copy.attempted = true;
  try {
    const raw = localStorage.getItem(previous.key);
    if (raw) copy.source = { key: previous.key, raw, value: previous };
  } catch {
    warning();
  }
  show(copy);
  persist(copy);
  void save(copy);
});
deleteButton.addEventListener("click", () => {
  const editor = current;
  if (!editor?.name || !confirm("Move this note to trash?")) return;
  element("close-context", HTMLButtonElement).focus({ preventScroll: true });
  const generation = openGeneration;
  void (async () => {
    if ((dirty(editor) || editor.pending) && !(await flush(editor))) return;
    if (editor.name === null) return;
    editor.operation = true;
    renderStatus(editor);
    try {
      try {
        await api(notePath(editor.name), "DELETE", undefined, editor.revision);
      } catch (error) {
        if (!(error instanceof ApiError) || error.status !== 404) throw error;
      }
      forgetDeleted(editor);
      announceChange(editor.name);
      document.dispatchEvent(new Event("tungsten:trash"));
      if (current === editor) {
        const replacement = blank();
        show(replacement);
        if (generation === openGeneration) await openAfterDeletion(replacement);
        else {
          body.disabled = true;
          features?.changed();
          status.textContent = "Loading";
        }
      }
      void refresh().catch(() => {});
    } catch (error) {
      editor.failure = message(error);
      editor.blocked = error instanceof ApiError && [404, 412].includes(error.status);
      persist(editor);
      renderStatus(editor);
    } finally {
      editor.operation = false;
      renderStatus(editor);
    }
  })();
});
document.addEventListener("keydown", (event) => {
  if ((event.ctrlKey || event.metaKey) && event.key.toLowerCase() === "s") {
    event.preventDefault();
    if (current) void save(current);
  }
});
function announceChange(name: string): void {
  try {
    localStorage.setItem(changeKey, JSON.stringify({ name, event: identifier() }));
  } catch {
    warning();
  }
}

async function synchronize(): Promise<void> {
  if (checking) return checking;
  const editor = current;
  if (!editor?.name) return;
  if (
    !mayReadRemote(
      !document.hidden,
      !writingScrollContainer().hidden,
      body.disabled,
      editor.pending !== null,
      editor.operation,
      editor.composing,
      editor.blocked,
    )
  )
    return;
  const name = editor.name;
  const revision = editor.revision;
  const generation = openGeneration;
  checking = (async () => {
    let note: Note | null = null;
    let missing = false;
    try {
      const value = await api(notePath(name), "GET", undefined, revision);
      if (value !== null) note = parseNote(value);
    } catch (error) {
      if (!(error instanceof ApiError) || error.status !== 404) return;
      missing = true;
    }
    if (
      !mayApplyRemote(
        current === editor,
        generation === openGeneration,
        editor.revision === revision,
        body.disabled,
        editor.pending !== null,
        editor.operation,
        editor.composing,
        editor.blocked,
      )
    )
      return;
    if (missing) {
      if (dirty(editor)) {
        editor.blocked = true;
        editor.missing = true;
        editor.failure =
          "The note was deleted in another tab or device. Save a copy to keep your draft.";
        persist(editor);
        renderStatus(editor);
      } else {
        forgetDeleted(editor);
        const replacement = blank();
        show(replacement);
        await openAfterDeletion(replacement);
      }
      return;
    }
    if (!note || note.revision === revision) return;
    if (hasRemoteConflict(dirty(editor), editor.body === note.body)) {
      editor.blocked = true;
      editor.failure =
        "The note changed in another tab or device. Reload it or save your draft as a copy.";
      persist(editor);
      renderStatus(editor);
      return;
    }
    const start = body.selectionStart;
    const end = body.selectionEnd;
    editor.body = note.body;
    editor.savedBody = note.body;
    editor.revision = note.revision;
    editor.title = noteTitle(note.body);
    editor.failure = null;
    body.value = note.body;
    body.setSelectionRange(Math.min(start, note.body.length), Math.min(end, note.body.length));
    persist(editor);
    renderStatus(editor);
  })().finally(() => {
    checking = null;
  });
  return checking;
}

function visibilityChanged(): void {
  if (document.hidden) {
    window.clearInterval(checkTimer);
    checkTimer = undefined;
    if (current && !current.failure && !current.blocked) void save(current);
  } else {
    checkTimer ??= window.setInterval(() => {
      void synchronize();
    }, 15000);
    void synchronize();
  }
}

document.addEventListener("visibilitychange", visibilityChanged);
window.addEventListener("focus", () => {
  void synchronize();
});
document.addEventListener("tungsten:page", () => {
  void synchronize();
});
window.addEventListener("online", () => {
  if (current && !current.blocked) {
    current.failure = null;
    void save(current);
  }
});
window.addEventListener("storage", (event) => {
  renderDrafts();
  if (event.key !== changeKey || !event.newValue) return;
  try {
    const value: unknown = JSON.parse(event.newValue);
    if (record(value) && value.name === current?.name) void synchronize();
  } catch {
    return;
  }
});
window.addEventListener("beforeunload", (event) => {
  if (current && dirty(current)) {
    persist(current);
    event.preventDefault();
    event.returnValue = "";
  }
});

async function boot(): Promise<void> {
  const linked = new URL(location.href).searchParams.get("note");
  const owners = await activeDraftOwners();
  const recovered = drafts().find((draft) =>
    mayRecoverDraft(
      !linked || draft.value.name === linked,
      draft.value.owner === null,
      owners !== null,
      draft.value.owner !== null && Boolean(owners?.has(draft.value.owner)),
    ),
  );
  if (recovered) {
    await recover(recovered);
    void refresh().catch(() => {});
    return;
  }
  try {
    const notes = await refresh();
    if (current) return;
    if (linked) {
      await openNote(linked);
      if (location.hash) await features?.anchor(location.hash);
      return;
    }
    let last: string | null = null;
    try {
      last = localStorage.getItem("tungsten.last");
    } catch {
      warning();
    }
    const selected = notes.find((note) => note.name === last) ?? notes[0];
    if (selected) await openNote(selected.name);
    else show(blank());
  } catch (error) {
    if (!current) show(blank());
    if (current) {
      current.failure = message(error);
      renderStatus(current);
    }
  }
}

function snapshot(): Snapshot | null {
  if (!current) return null;
  return {
    identity: current.key,
    name: current.name,
    title: current.title,
    body: current.body,
    savedBody: current.savedBody,
    revision: current.revision,
    busy: body.disabled || current.pending !== null || current.operation,
    composing: current.composing,
  };
}

features = initFeatures({
  state: snapshot,
  api,
  open: (name) => {
    element("notes-dialog", HTMLDialogElement).close();
    return openNote(name, true);
  },
  blank: createDraft,
  create: async () => {
    const created = createDraft("", "");
    return (await flush(created)) ? created.name : null;
  },
  mutate: async (action) => {
    const editor = current;
    if (!editor || editor.operation || editor.composing) return false;
    if (editor.name === null) editor.attempted = true;
    if (!(await flush(editor)))
      throw new Error("Save or recover the draft before changing the saved note.");
    if (current !== editor) return false;
    editor.operation = true;
    renderStatus(editor);
    try {
      const state = snapshot();
      if (state) await action(state);
      if (editor.name) announceChange(editor.name);
    } finally {
      editor.operation = false;
      renderStatus(editor);
    }
    return true;
  },
  refresh,
});

window.addEventListener("popstate", () => {
  const name = new URL(location.href).searchParams.get("note");
  if (current && name === current.name) return;
  if (name)
    void openNote(name).then(async () => {
      if (current?.name === name && location.hash) await features?.anchor(location.hash);
    });
  else {
    ++openGeneration;
    show(blank());
  }
});

void boot();
visibilityChanged();
